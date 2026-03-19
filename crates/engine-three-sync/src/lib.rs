// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

mod extract;
mod frame_packet;
mod snapshot;

pub use extract::{extract_frame, extract_frame_incremental};
pub use frame_packet::{FramePacket, TRANSFORM_STRIDE};
pub use snapshot::{
    DebugSnapshot, EntitySnapshot, TransformSnapshot, extract_debug_snapshot, snapshot_to_json,
};

use galeon_engine::Engine;
use wasm_bindgen::prelude::*;

/// Returns the engine version string to the JS runtime.
#[wasm_bindgen]
pub fn version() -> String {
    galeon_engine::engine_version().to_string()
}

// =============================================================================
// WasmEngine — JS-facing engine handle
// =============================================================================

/// JS-facing handle to the Galeon engine.
///
/// Wraps the Rust `Engine` and exposes tick + frame extraction to JavaScript.
///
/// `last_extracted_cursor` tracks the monotonic change cursor observed at the
/// previous extraction. Each call stores `World::current_change_cursor()` so
/// the next extraction can detect same-tick mutations as well as later ticks.
/// Initialized to `0` so that the first extraction flags everything as changed.
#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine,
    last_extracted_cursor: u64,
}

#[allow(clippy::new_without_default)]
#[wasm_bindgen]
impl WasmEngine {
    /// Create a new engine instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            engine: Engine::new(),
            last_extracted_cursor: 0,
        }
    }

    /// Advance the simulation by `elapsed` seconds (fixed-timestep).
    ///
    /// Returns the number of simulation ticks that executed.
    pub fn tick(&mut self, elapsed: f64) -> u32 {
        self.engine.tick(elapsed)
    }

    /// Extract the current frame's render data as a packed packet.
    ///
    /// Uses incremental extraction: `change_flags` on the returned packet
    /// indicate which entities changed since the previous call to this method.
    /// The very first call flags all entities as changed (since tick 0).
    pub fn extract_frame(&mut self) -> WasmFramePacket {
        let since = self.last_extracted_cursor;
        self.last_extracted_cursor = self.engine.world().current_change_cursor();
        let packet = extract_frame_incremental(self.engine.world(), since);
        WasmFramePacket { inner: packet }
    }

    /// Extract a debug snapshot as a JSON string for tooling.
    ///
    /// This is the tooling path — human-readable, NOT used for rendering.
    pub fn debug_snapshot(&self) -> String {
        let snap = extract_debug_snapshot(self.engine.world());
        snapshot_to_json(&snap)
    }
}

// =============================================================================
// WasmFramePacket — JS-facing packed render data
// =============================================================================

/// JS-facing packed frame data.
///
/// Each getter returns a flat typed array. All arrays are parallel:
/// index `i` in every array refers to the same entity.
#[wasm_bindgen]
pub struct WasmFramePacket {
    inner: FramePacket,
}

#[wasm_bindgen]
impl WasmFramePacket {
    /// Number of renderable entities in this frame.
    #[wasm_bindgen(getter)]
    pub fn entity_count(&self) -> u32 {
        self.inner.entity_count() as u32
    }

    /// Entity IDs (one u32 per entity).
    #[wasm_bindgen(getter)]
    pub fn entity_ids(&self) -> Vec<u32> {
        self.inner.entity_ids.clone()
    }

    /// Entity generations (one u32 per entity, parallel to entity_ids).
    ///
    /// A generation mismatch for the same index means the slot was reused
    /// after despawn — the renderer must treat it as a new entity.
    #[wasm_bindgen(getter)]
    pub fn entity_generations(&self) -> Vec<u32> {
        self.inner.entity_generations.clone()
    }

    /// Packed transform data (10 f32 per entity: pos3 + rot4 + scale3).
    #[wasm_bindgen(getter)]
    pub fn transforms(&self) -> Vec<f32> {
        self.inner.transforms.clone()
    }

    /// Visibility flags (1 u8 per entity: 1 = visible, 0 = hidden).
    #[wasm_bindgen(getter)]
    pub fn visibility(&self) -> Vec<u8> {
        self.inner.visibility.clone()
    }

    /// Mesh handle IDs (one u32 per entity).
    #[wasm_bindgen(getter)]
    pub fn mesh_handles(&self) -> Vec<u32> {
        self.inner.mesh_handles.clone()
    }

    /// Material handle IDs (one u32 per entity).
    #[wasm_bindgen(getter)]
    pub fn material_handles(&self) -> Vec<u32> {
        self.inner.material_handles.clone()
    }

    /// Change flags (one u8 per entity, parallel to all other arrays).
    ///
    /// A value of `1` means the entity had at least one render-facing component
    /// mutated since the last call to `WasmEngine::extract_frame()`.
    /// A value of `0` means the entity is unchanged and the renderer may skip
    /// updating its scene-graph node.
    #[wasm_bindgen(getter)]
    pub fn change_flags(&self) -> Vec<u8> {
        self.inner.change_flags.clone()
    }
}
