// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/**
 * Shape of the WASM-exported frame packet.
 *
 * wasm-bindgen returns Vec<f32> as Float32Array, Vec<u32> as Uint32Array, etc.
 * This interface matches the Rust `WasmFramePacket` getter API.
 */
/** Transform (position / rotation / scale) changed — matches Rust `CHANGED_TRANSFORM`. */
export const CHANGED_TRANSFORM = 1 << 0;
/** Visibility changed — matches Rust `CHANGED_VISIBILITY`. */
export const CHANGED_VISIBILITY = 1 << 1;
/** Mesh handle changed — matches Rust `CHANGED_MESH`. */
export const CHANGED_MESH = 1 << 2;
/** Material handle changed — matches Rust `CHANGED_MATERIAL`. */
export const CHANGED_MATERIAL = 1 << 3;

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
  readonly custom_channel_count: number;
  custom_channel_name_at(index: number): string;
  custom_channel_stride(name: string): number;
  custom_channel_data(name: string): Float32Array;
}

/** Number of f32 values per entity in the transforms array. */
export const TRANSFORM_STRIDE = 10;
