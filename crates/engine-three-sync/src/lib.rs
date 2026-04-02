// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

mod extract;
mod frame_packet;
mod snapshot;

pub use extract::extract_frame;
pub use frame_packet::{ChannelData, FramePacket, TRANSFORM_STRIDE};
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
#[wasm_bindgen]
pub struct WasmEngine {
    engine: Engine,
}

#[allow(clippy::new_without_default)]
#[wasm_bindgen]
impl WasmEngine {
    /// Create a new engine instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            engine: Engine::new(),
        }
    }

    /// Advance the simulation by `elapsed` seconds (fixed-timestep).
    ///
    /// Returns the number of simulation ticks that executed.
    pub fn tick(&mut self, elapsed: f64) -> u32 {
        self.engine.tick(elapsed)
    }

    /// Extract the current frame's render data as a packed packet.
    pub fn extract_frame(&self) -> WasmFramePacket {
        let packet = extract_frame(self.engine.world());
        WasmFramePacket { inner: packet }
    }

    /// Extract a debug snapshot as a JSON string for tooling.
    ///
    /// This is the tooling path — human-readable, NOT used for rendering.
    pub fn debug_snapshot(&self) -> String {
        let snap = extract_debug_snapshot(self.engine.world());
        snapshot_to_json(&snap)
    }

    /// Pause the simulation.
    pub fn pause(&mut self) {
        self.engine.pause();
    }

    /// Resume the simulation.
    pub fn resume(&mut self) {
        self.engine.resume();
    }

    /// Set simulation speed (1.0 = normal, 2.0 = fast, 0.5 = slow).
    pub fn set_speed(&mut self, scale: f64) {
        self.engine.set_speed(scale);
    }

    /// Returns true if paused.
    pub fn is_paused(&self) -> bool {
        self.engine.is_paused()
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

    /// Number of custom data channels in this frame.
    #[wasm_bindgen(getter)]
    pub fn custom_channel_count(&self) -> u32 {
        self.inner.channel_count() as u32
    }

    /// Get the name of a custom channel by index (sorted alphabetically).
    ///
    /// Returns empty string if index is out of bounds.
    pub fn custom_channel_name_at(&self, index: u32) -> String {
        let names = self.inner.channel_names();
        names
            .get(index as usize)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Get the stride (floats per entity) for a named custom channel.
    ///
    /// Returns 0 if the channel does not exist.
    pub fn custom_channel_stride(&self, name: &str) -> u32 {
        self.inner
            .channel(name)
            .map(|ch| ch.stride as u32)
            .unwrap_or(0)
    }

    /// Get the flat float data for a named custom channel.
    ///
    /// Returns an empty array if the channel does not exist.
    pub fn custom_channel_data(&self, name: &str) -> Vec<f32> {
        self.inner
            .channel(name)
            .map(|ch| ch.data.clone())
            .unwrap_or_default()
    }
}
