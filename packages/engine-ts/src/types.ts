// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

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

/**
 * Shape of the WASM-exported frame packet.
 *
 * wasm-bindgen returns Vec<f32> as Float32Array, Vec<u32> as Uint32Array, etc.
 * This interface matches the Rust `WasmFramePacket` getter API.
 */
export interface FramePacketView {
  readonly entity_count: number;
  readonly entity_ids: Uint32Array;
  readonly entity_generations: Uint32Array;
  readonly transforms: Float32Array;
  readonly visibility: Uint8Array;
  readonly mesh_handles: Uint32Array;
  readonly material_handles: Uint32Array;
  /** Set for incremental extraction; omit or empty for full frames (all fields apply). */
  readonly change_flags?: Uint8Array;
  /** Object type per entity (0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group). Omit for all-Mesh frames. */
  readonly object_types?: Uint8Array;
  readonly custom_channel_count: number;
  custom_channel_name_at(index: number): string;
  custom_channel_stride(name: string): number;
  custom_channel_data(name: string): Float32Array;
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
