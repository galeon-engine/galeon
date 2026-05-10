// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

import {
  CHANGED_TRANSFORM,
  FramePacketContractError,
  TRANSFORM_STRIDE,
  assertFramePacketContract,
  type FramePacketView,
} from "./index.js";

/** String key used to correlate render samples over time. */
export type StableRenderId = string;

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
    const sample = { timeMs, value };
    const last = history[history.length - 1];
    if (last === undefined || timeMs >= last.timeMs) {
      history.push(sample);
    } else {
      history.splice(this.findInsertionIndex(history, timeMs), 0, sample);
    }
    const latestTimeMs = history[history.length - 1]!.timeMs;
    this.pruneHistory(history, latestTimeMs - this.maxHistoryMs);
    this.states.set(key, history);
  }

  has(key: K): boolean {
    return this.states.has(key);
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

  private findInsertionIndex(history: TimedState<T>[], timeMs: number): number {
    let low = 0;
    let high = history.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (history[middle]!.timeMs <= timeMs) {
        low = middle + 1;
      } else {
        high = middle;
      }
    }
    return low;
  }

  /**
   * Keep at least two samples so interpolation remains stable while trimming
   * entries older than the retention window.
   */
  private pruneHistory(history: TimedState<T>[], minTimeMs: number): void {
    let pruneCount = 0;
    while (
      history.length - pruneCount > 2 &&
      history[pruneCount + 1]!.timeMs < minTimeMs
    ) {
      pruneCount++;
    }
    if (pruneCount > 0) {
      history.splice(0, pruneCount);
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
  /** Canonical stable id, derived from `entityId:generation`. */
  readonly key: string;
  /** Source entity id from the authoritative frame packet. */
  readonly entityId: number;
  /** Generation paired with `entityId`; together they derive `key`. */
  readonly generation: number;
  readonly visible: boolean;
  readonly transform: TransformState;
}

export interface TransformFrameIngestionOptions {
  readonly now?: () => number;
  readonly interpolationDelayMs?: number;
  readonly maxHistoryMs?: number;
}

export type TransformFrameIngestionMode = "full" | "incremental";

export interface TransformFrameIngestOptions {
  readonly mode?: TransformFrameIngestionMode;
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

  /**
   * Ingest an authoritative frame snapshot. Full mode is the default.
   * Use `mode: "incremental"` (or `ingestIncrementalFrame`) for delta packets.
   */
  ingestFrame(
    packet: FramePacketView,
    receivedAtMs = this.now(),
    options: TransformFrameIngestOptions = {},
  ): void {
    const mode = options.mode ?? "full";
    const isIncremental = mode === "incremental";
    if (isIncremental && packet.entity_count > 0) {
      const flagCount = packet.change_flags?.length ?? 0;
      if (flagCount !== packet.entity_count) {
        throw new FramePacketContractError(
          `incremental ingestion requires change_flags length ${packet.entity_count}, got ${flagCount}`,
        );
      }
    }

    assertFramePacketContract(packet);

    const flags = packet.change_flags;
    const activeKeys = new Set<string>();
    const nextEntities = new Map<string, TrackedEntity>();
    for (const [key, entity] of this.entities) {
      nextEntities.set(key, { ...entity });
    }

    const stagedTransforms: Array<{
      readonly key: string;
      readonly transform: TransformState;
    }> = [];
    const stagedTransformKeys = new Set<string>();

    for (let i = 0; i < packet.entity_count; i++) {
      const entityId = packet.entity_ids[i]!;
      const generation = packet.entity_generations[i]!;
      const key = frameEntityKey(entityId, generation);
      activeKeys.add(key);

      const tracked = nextEntities.get(key) ?? {
        entityId,
        generation,
        visible: true,
      };
      tracked.visible = packet.visibility[i]! === 1;
      nextEntities.set(key, tracked);

      const rowFlags = isIncremental ? flags![i]! : 0;
      const changedTransform =
        !isIncremental ||
        (rowFlags & CHANGED_TRANSFORM) !== 0 ||
        stagedTransformKeys.has(key) ||
        !this.transforms.has(key);
      if (changedTransform) {
        stagedTransforms.push({
          key,
          transform: transformStateFromPacket(packet, i),
        });
        stagedTransformKeys.add(key);
      }
    }

    const removedKeys: string[] = [];
    if (!isIncremental) {
      for (const key of nextEntities.keys()) {
        if (activeKeys.has(key)) {
          continue;
        }
        removedKeys.push(key);
        nextEntities.delete(key);
      }
    }

    this.entities.clear();
    for (const [key, entity] of nextEntities) {
      this.entities.set(key, entity);
    }
    for (const key of removedKeys) {
      this.transforms.delete(key);
    }
    for (const update of stagedTransforms) {
      this.transforms.push(update.key, receivedAtMs, update.transform);
    }
  }

  /** Convenience wrapper for ingesting incremental delta packets. */
  ingestIncrementalFrame(
    packet: FramePacketView,
    receivedAtMs = this.now(),
  ): void {
    this.ingestFrame(packet, receivedAtMs, { mode: "incremental" });
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
  const transform: TransformState = {
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
  assertFiniteTransform(transform, index);
  return transform;
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
  // Quaternion q and -q encode the same rotation. Flip sign on negative dot
  // so interpolation follows the shortest arc and avoids visible inversion.
  const sign = dot < 0 ? -1 : 1;
  return {
    qx: lerp(from.qx, to.qx * sign, alpha),
    qy: lerp(from.qy, to.qy * sign, alpha),
    qz: lerp(from.qz, to.qz * sign, alpha),
    qw: lerp(from.qw, to.qw * sign, alpha),
  };
}

function assertFiniteTransform(transform: TransformState, index: number): void {
  for (const [field, value] of Object.entries(transform)) {
    if (!Number.isFinite(value)) {
      throw new FramePacketContractError(
        `transforms[${index}] has non-finite ${field}: ${value}`,
      );
    }
  }
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
