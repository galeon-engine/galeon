// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/** Render snapshot contract version shared across Rust/WASM and TypeScript consumers. */
export const RENDER_CONTRACT_VERSION = 1;

/** Bitmasks for incremental frame rows; values match Rust `galeon_engine_three_sync::frame_packet`. */
export const CHANGED_TRANSFORM = 1 << 0;
/** Visibility changed — matches Rust `CHANGED_VISIBILITY`. */
export const CHANGED_VISIBILITY = 1 << 1;
/** Mesh handle changed — matches Rust `CHANGED_MESH`. */
export const CHANGED_MESH = 1 << 2;
/** Material handle changed — matches Rust `CHANGED_MATERIAL`. */
export const CHANGED_MATERIAL = 1 << 3;
/** Object type changed — matches Rust `CHANGED_OBJECT_TYPE`. */
export const CHANGED_OBJECT_TYPE = 1 << 4;
/** Parent entity changed — matches Rust `CHANGED_PARENT`. */
export const CHANGED_PARENT = 1 << 5;

/** Sentinel value meaning "child of scene root" (no parent entity). Matches Rust `SCENE_ROOT`. */
export const SCENE_ROOT = 0xffff_ffff;

/**
 * Shape of the WASM-exported frame packet.
 *
 * wasm-bindgen returns Vec<f32> as Float32Array, Vec<u32> as Uint32Array, etc.
 * This interface matches the Rust `WasmFramePacket` getter API.
 */
export interface FramePacketView {
  /**
   * Render contract version emitted by Rust.
   *
   * Omitted only for legacy packets produced before contract versioning.
   */
  readonly contract_version?: number;
  readonly entity_count: number;
  readonly entity_ids: Uint32Array;
  readonly entity_generations: Uint32Array;
  readonly transforms: Float32Array;
  readonly visibility: Uint8Array;
  readonly mesh_handles: Uint32Array;
  readonly material_handles: Uint32Array;
  /** Parent entity indices. `SCENE_ROOT` (0xFFFFFFFF) = child of scene root. */
  readonly parent_ids: Uint32Array;
  /** Set for incremental extraction; omit or empty for full frames (all fields apply). */
  readonly change_flags?: Uint8Array;
  /** Object type per entity (0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group). */
  readonly object_types?: Uint8Array;
  /** Monotonic frame version — skip applyFrame() when unchanged. Omit for always-apply backward compat. */
  readonly frame_version?: bigint;
  readonly custom_channel_count: number;
  custom_channel_name_at(index: number): string;
  custom_channel_stride(name: string): number;
  custom_channel_data(name: string): Float32Array;

  // -- One-shot events (audio/VFX triggers) --
  /** Number of one-shot events in this frame. */
  readonly event_count?: number;
  /** Event type IDs (one u32 per event, parallel to other event arrays). */
  readonly event_kinds?: Uint32Array;
  /** Source entity indices (one u32 per event). */
  readonly event_entities?: Uint32Array;
  /** Event positions (3 floats per event: x, y, z). */
  readonly event_positions?: Float32Array;
  /** Event intensities (one f32 per event). */
  readonly event_intensities?: Float32Array;
  /** Extra event payload (4 floats per event: color, direction, variant, etc.). */
  readonly event_data?: Float32Array;
}

/** Number of f32 values per entity in the transforms array. */
export const TRANSFORM_STRIDE = 10;

/** Object type discriminant — values match Rust `ObjectType` repr(u8). */
export const enum ObjectType {
  Mesh = 0,
  PointLight = 1,
  DirectionalLight = 2,
  LineSegments = 3,
  Group = 4,
}

export interface FramePacketContractOptions {
  /**
   * Keep one-minor backward compatibility with pre-versioned packets.
   * Set to `false` to require `contract_version`.
   */
  allowMissingContractVersion?: boolean;
  /**
   * Validate custom channel payload lengths by calling `custom_channel_*`.
   * Disabled by default to avoid repeated callback work in hot paths.
   */
  validateCustomChannels?: boolean;
}

/** Contract validation failure for malformed or incompatible render packets. */
export class FramePacketContractError extends Error {
  constructor(message: string) {
    super(`[FramePacket] ${message}`);
    this.name = "FramePacketContractError";
  }
}

function assertLength(
  field: string,
  actual: number,
  expected: number,
): void {
  if (actual !== expected) {
    throw new FramePacketContractError(
      `${field} length mismatch: expected ${expected}, got ${actual}`,
    );
  }
}

const EMPTY_U32 = new Uint32Array(0);
const EMPTY_F32 = new Float32Array(0);

/** True when this packet is incremental and carries per-row change flags. */
export function hasIncrementalChangeFlags(packet: FramePacketView): boolean {
  return packet.change_flags !== undefined && packet.change_flags.length > 0;
}

/**
 * Validate render packet structural invariants and contract compatibility.
 *
 * Keep this check at adapter boundaries to fail fast on hot-update shape drift.
 */
export function assertFramePacketContract(
  packet: FramePacketView,
  options: FramePacketContractOptions = {},
): void {
  const {
    allowMissingContractVersion = true,
    validateCustomChannels = false,
  } = options;
  const version = packet.contract_version;
  if (version == null) {
    if (!allowMissingContractVersion) {
      throw new FramePacketContractError(
        "missing contract_version on frame packet",
      );
    }
  } else if (version !== RENDER_CONTRACT_VERSION) {
    throw new FramePacketContractError(
      `unsupported contract_version ${version}; expected ${RENDER_CONTRACT_VERSION}`,
    );
  }

  const entityCount = packet.entity_count;
  if (!Number.isInteger(entityCount) || entityCount < 0) {
    throw new FramePacketContractError(
      `entity_count must be a non-negative integer, got ${entityCount}`,
    );
  }

  assertLength("entity_ids", packet.entity_ids.length, entityCount);
  assertLength(
    "entity_generations",
    packet.entity_generations.length,
    entityCount,
  );
  assertLength(
    "transforms",
    packet.transforms.length,
    entityCount * TRANSFORM_STRIDE,
  );
  assertLength("visibility", packet.visibility.length, entityCount);
  assertLength("mesh_handles", packet.mesh_handles.length, entityCount);
  assertLength("material_handles", packet.material_handles.length, entityCount);
  assertLength("parent_ids", packet.parent_ids.length, entityCount);

  if (packet.object_types !== undefined) {
    assertLength("object_types", packet.object_types.length, entityCount);
  }

  if (packet.change_flags !== undefined && packet.change_flags.length > 0) {
    assertLength("change_flags", packet.change_flags.length, entityCount);
  }

  const eventCount = packet.event_count ?? 0;
  if (!Number.isInteger(eventCount) || eventCount < 0) {
    throw new FramePacketContractError(
      `event_count must be a non-negative integer, got ${eventCount}`,
    );
  }
  const eventKinds = packet.event_kinds ?? EMPTY_U32;
  const eventEntities = packet.event_entities ?? EMPTY_U32;
  const eventPositions = packet.event_positions ?? EMPTY_F32;
  const eventIntensities = packet.event_intensities ?? EMPTY_F32;
  const eventData = packet.event_data ?? EMPTY_F32;
  assertLength("event_kinds", eventKinds.length, eventCount);
  assertLength("event_entities", eventEntities.length, eventCount);
  assertLength(
    "event_positions",
    eventPositions.length,
    eventCount * 3,
  );
  assertLength(
    "event_intensities",
    eventIntensities.length,
    eventCount,
  );
  assertLength("event_data", eventData.length, eventCount * 4);

  const channelCount = packet.custom_channel_count;
  if (!Number.isInteger(channelCount) || channelCount < 0) {
    throw new FramePacketContractError(
      `custom_channel_count must be a non-negative integer, got ${channelCount}`,
    );
  }

  if (validateCustomChannels) {
    for (let i = 0; i < channelCount; i++) {
      const name = packet.custom_channel_name_at(i);
      const stride = packet.custom_channel_stride(name);
      if (!Number.isInteger(stride) || stride <= 0) {
        throw new FramePacketContractError(
          `custom channel "${name}" has invalid stride ${stride}`,
        );
      }
      const data = packet.custom_channel_data(name);
      assertLength(
        `custom_channel_data(${name})`,
        data.length,
        stride * entityCount,
      );
    }
  }
}
