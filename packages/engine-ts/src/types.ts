// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/**
 * Shape of the WASM-exported frame packet.
 *
 * wasm-bindgen returns Vec<f32> as Float32Array, Vec<u32> as Uint32Array, etc.
 * This interface matches the Rust `WasmFramePacket` getter API.
 */
export interface FramePacketView {
  readonly entity_count: number;
  readonly entity_ids: Uint32Array;
  readonly transforms: Float32Array;
  readonly visibility: Uint8Array;
  readonly mesh_handles: Uint32Array;
  readonly material_handles: Uint32Array;
}

/** Number of f32 values per entity in the transforms array. */
export const TRANSFORM_STRIDE = 10;
