// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import {
  CHANGED_TRANSFORM,
  TRANSFORM_STRIDE,
  assertFramePacketContract,
  hasIncrementalChangeFlags,
  type FramePacketView,
} from "./index.js";

export type StableRenderId = string | number;

export interface TimedState<T> {
  readonly timeMs: number;
  readonly value: T;
}

export interface StateInterpolator<T> {
  interpolate(from: T, to: T, alpha: number): T;
}

export interface StateInterpolationBufferOptions {
  readonly maxHistoryMs?: number;
}

/**
 * Generic render-time state buffer keyed by stable render ids.
 *
 * Authoritative updates can arrive at lower frequency than rendering; this
 * buffer gives adapters a deterministic place to sample state between updates.
 */
export class StateInterpolationBuffer<K extends StableRenderId, T> {
  private readonly states = new Map<K, TimedState<T>[]>();
  private readonly maxHistoryMs: number;

  constructor(
    private readonly interpolator: StateInterpolator<T>,
    options: StateInterpolationBufferOptions = {},
  ) {
    this.maxHistoryMs = options.maxHistoryMs ?? 1_000;
  }

  push(key: K, timeMs: number, value: T): void {
    const history = this.states.get(key) ?? [];
    history.push({ timeMs, value });
    history.sort((a, b) => a.timeMs - b.timeMs);
    this.pruneHistory(history, timeMs - this.maxHistoryMs);
    this.states.set(key, history);
  }

  sample(key: K, timeMs: number): T | undefined {
    const history = this.states.get(key);
    if (history === undefined || history.length === 0) {
      return undefined;
    }

    const first = history[0]!;
    if (timeMs <= first.timeMs) {
      return first.value;
    }

    const last = history[history.length - 1]!;
    if (timeMs >= last.timeMs) {
      return last.value;
    }

    for (let i = 1; i < history.length; i++) {
      const next = history[i]!;
      if (timeMs > next.timeMs) {
        continue;
      }
      const prev = history[i - 1]!;
      const span = next.timeMs - prev.timeMs;
      const alpha = span <= 0 ? 1 : (timeMs - prev.timeMs) / span;
      return this.interpolator.interpolate(prev.value, next.value, alpha);
    }

    return last.value;
  }

  snapshot(timeMs: number): Map<K, T> {
    const result = new Map<K, T>();
    for (const key of this.states.keys()) {
      const value = this.sample(key, timeMs);
      if (value !== undefined) {
        result.set(key, value);
      }
    }
    return result;
  }

  delete(key: K): void {
    this.states.delete(key);
  }

  clear(): void {
    this.states.clear();
  }

  keys(): IterableIterator<K> {
    return this.states.keys();
  }

  private pruneHistory(history: TimedState<T>[], minTimeMs: number): void {
    while (history.length > 2 && history[1]!.timeMs < minTimeMs) {
      history.shift();
    }
  }
}

export interface TransformState {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  readonly qx: number;
  readonly qy: number;
  readonly qz: number;
  readonly qw: number;
  readonly sx: number;
  readonly sy: number;
  readonly sz: number;
}

export interface TransformFrameSample {
  readonly key: string;
  readonly entityId: number;
  readonly generation: number;
  readonly visible: boolean;
  readonly transform: TransformState;
}

export interface TransformFrameIngestionOptions {
  readonly now?: () => number;
  readonly interpolationDelayMs?: number;
  readonly maxHistoryMs?: number;
}

interface TrackedEntity {
  readonly entityId: number;
  readonly generation: number;
  visible: boolean;
}

export function frameEntityKey(entityId: number, generation: number): string {
  return `${entityId}:${generation}`;
}

export const transformInterpolator: StateInterpolator<TransformState> = {
  interpolate(from, to, alpha) {
    const t = clamp01(alpha);
    const q = normalizeQuaternion(lerpQuaternion(from, to, t));
    return {
      x: lerp(from.x, to.x, t),
      y: lerp(from.y, to.y, t),
      z: lerp(from.z, to.z, t),
      qx: q.qx,
      qy: q.qy,
      qz: q.qz,
      qw: q.qw,
      sx: lerp(from.sx, to.sx, t),
      sy: lerp(from.sy, to.sy, t),
      sz: lerp(from.sz, to.sz, t),
    };
  },
};

/**
 * Ingests authoritative FramePacket transforms and exposes render-time samples.
 */
export class TransformFrameIngestion {
  private readonly now: () => number;
  private readonly interpolationDelayMs: number;
  private readonly entities = new Map<string, TrackedEntity>();
  private readonly transforms: StateInterpolationBuffer<string, TransformState>;

  constructor(options: TransformFrameIngestionOptions = {}) {
    this.now = options.now ?? (() => performance.now());
    this.interpolationDelayMs = options.interpolationDelayMs ?? 100;
    this.transforms = new StateInterpolationBuffer(transformInterpolator, {
      maxHistoryMs: options.maxHistoryMs,
    });
  }

  ingestFrame(packet: FramePacketView, receivedAtMs = this.now()): void {
    assertFramePacketContract(packet);

    const isIncremental = hasIncrementalChangeFlags(packet);
    const activeKeys = new Set<string>();
    for (let i = 0; i < packet.entity_count; i++) {
      const entityId = packet.entity_ids[i]!;
      const generation = packet.entity_generations[i]!;
      const key = frameEntityKey(entityId, generation);
      activeKeys.add(key);

      const tracked = this.entities.get(key) ?? {
        entityId,
        generation,
        visible: true,
      };
      tracked.visible = packet.visibility[i]! === 1;
      this.entities.set(key, tracked);

      const flags = packet.change_flags;
      const changedTransform =
        !isIncremental ||
        flags === undefined ||
        (flags[i]! & CHANGED_TRANSFORM) !== 0 ||
        this.transforms.sample(key, receivedAtMs) === undefined;
      if (changedTransform) {
        this.transforms.push(
          key,
          receivedAtMs,
          transformStateFromPacket(packet, i),
        );
      }
    }

    if (!isIncremental) {
      for (const key of Array.from(this.entities.keys())) {
        if (activeKeys.has(key)) {
          continue;
        }
        this.entities.delete(key);
        this.transforms.delete(key);
      }
    }
  }

  sampleFrame(renderTimeMs = this.now() - this.interpolationDelayMs): TransformFrameSample[] {
    const samples: TransformFrameSample[] = [];
    for (const [key, entity] of this.entities) {
      const transform = this.transforms.sample(key, renderTimeMs);
      if (transform === undefined) {
        continue;
      }
      samples.push({
        key,
        entityId: entity.entityId,
        generation: entity.generation,
        visible: entity.visible,
        transform,
      });
    }
    return samples;
  }

  sampleEntity(
    entityId: number,
    generation: number,
    renderTimeMs = this.now() - this.interpolationDelayMs,
  ): TransformState | undefined {
    return this.transforms.sample(
      frameEntityKey(entityId, generation),
      renderTimeMs,
    );
  }

  clear(): void {
    this.entities.clear();
    this.transforms.clear();
  }
}

export function transformStateFromPacket(
  packet: FramePacketView,
  index: number,
): TransformState {
  const offset = index * TRANSFORM_STRIDE;
  return {
    x: packet.transforms[offset]!,
    y: packet.transforms[offset + 1]!,
    z: packet.transforms[offset + 2]!,
    qx: packet.transforms[offset + 3]!,
    qy: packet.transforms[offset + 4]!,
    qz: packet.transforms[offset + 5]!,
    qw: packet.transforms[offset + 6]!,
    sx: packet.transforms[offset + 7]!,
    sy: packet.transforms[offset + 8]!,
    sz: packet.transforms[offset + 9]!,
  };
}

function lerp(from: number, to: number, alpha: number): number {
  return from + (to - from) * alpha;
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function lerpQuaternion(
  from: TransformState,
  to: TransformState,
  alpha: number,
): Pick<TransformState, "qx" | "qy" | "qz" | "qw"> {
  const dot =
    from.qx * to.qx +
    from.qy * to.qy +
    from.qz * to.qz +
    from.qw * to.qw;
  const sign = dot < 0 ? -1 : 1;
  return {
    qx: lerp(from.qx, to.qx * sign, alpha),
    qy: lerp(from.qy, to.qy * sign, alpha),
    qz: lerp(from.qz, to.qz * sign, alpha),
    qw: lerp(from.qw, to.qw * sign, alpha),
  };
}

function normalizeQuaternion(
  q: Pick<TransformState, "qx" | "qy" | "qz" | "qw">,
): Pick<TransformState, "qx" | "qy" | "qz" | "qw"> {
  const length = Math.hypot(q.qx, q.qy, q.qz, q.qw);
  if (length === 0) {
    return { qx: 0, qy: 0, qz: 0, qw: 1 };
  }
  return {
    qx: q.qx / length,
    qy: q.qy / length,
    qz: q.qz / length,
    qw: q.qw / length,
  };
}
