// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

mod extract;
mod frame_packet;
mod snapshot;

pub use extract::{extract_frame, extract_frame_incremental};
pub use frame_packet::{
    CHANGED_MATERIAL, CHANGED_MESH, CHANGED_OBJECT_TYPE, CHANGED_PARENT, CHANGED_TRANSFORM,
    CHANGED_VISIBILITY, ChannelData, FramePacket, SCENE_ROOT, TRANSFORM_STRIDE,
};
pub use snapshot::{
    DebugSnapshot, EntitySnapshot, TransformSnapshot, extract_debug_snapshot, snapshot_to_json,
};

use galeon_engine::{
    Component, Engine, Entity, MaterialHandle, MeshHandle, ObjectType, Transform, Visibility,
};
use wasm_bindgen::prelude::*;

/// Marker component for entities spawned from JavaScript via [`WasmEngine::spawn_entity`].
///
/// Prevents JS from despawning plugin-spawned entities and enables bulk cleanup
/// via [`WasmEngine::despawn_all_js_entities`].
#[derive(Component, Debug, Clone, Copy)]
pub struct JsSpawned;

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

impl WasmEngine {
    /// Wrap an app-configured Rust engine in the generic WASM bridge.
    pub fn from_engine(engine: Engine) -> Self {
        Self { engine }
    }

    /// Borrow the underlying Rust engine for app-owned configuration.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Mutably borrow the underlying Rust engine for app-owned bootstrap.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

#[wasm_bindgen]
impl WasmEngine {
    /// Create a generic bridge engine with an empty ECS world.
    ///
    /// App-owned wrapper crates should configure their own plugins,
    /// resources, and entities in Rust via [`WasmEngine::engine_mut`] or
    /// [`WasmEngine::from_engine`] before exposing the handle to JavaScript.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
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

    // -------------------------------------------------------------------------
    // Dynamic entity spawn / despawn
    // -------------------------------------------------------------------------

    /// Spawn a new renderable entity from JavaScript.
    ///
    /// `transform` is a 10-element `Float32Array`:
    /// `[pos.x, pos.y, pos.z, rot.x, rot.y, rot.z, rot.w, scale.x, scale.y, scale.z]`
    /// — the same packing used by [`WasmFramePacket::transforms`].
    ///
    /// `object_type` selects the Three.js object class:
    /// 0=Mesh (default), 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group.
    ///
    /// Returns `[entity_index, entity_generation]` as a 2-element array.
    /// The entity will appear in the next `extract_frame()` call.
    ///
    /// Returns an empty array if `transform` has fewer than 10 elements.
    pub fn spawn_entity(
        &mut self,
        mesh_id: u32,
        material_id: u32,
        transform: &[f32],
        object_type: u8,
    ) -> Vec<u32> {
        if transform.len() < TRANSFORM_STRIDE {
            return Vec::new();
        }
        let obj_type = match object_type {
            1 => ObjectType::PointLight,
            2 => ObjectType::DirectionalLight,
            3 => ObjectType::LineSegments,
            4 => ObjectType::Group,
            _ => ObjectType::Mesh,
        };
        let entity = self.engine.world_mut().spawn((
            Transform {
                position: [transform[0], transform[1], transform[2]],
                rotation: [transform[3], transform[4], transform[5], transform[6]],
                scale: [transform[7], transform[8], transform[9]],
            },
            Visibility { visible: true },
            MeshHandle { id: mesh_id },
            MaterialHandle { id: material_id },
            obj_type,
            JsSpawned,
        ));
        vec![entity.index(), entity.generation()]
    }

    /// Despawn a JS-spawned entity by its index and generation.
    ///
    /// Returns `true` if the entity was alive, JS-spawned, and has been removed.
    /// Returns `false` if the entity was already dead, the generation doesn't
    /// match (stale handle), or the entity was not spawned from JS.
    pub fn despawn_entity(&mut self, index: u32, generation: u32) -> bool {
        let entity = Entity::from_raw(index, generation);
        let world = self.engine.world_mut();
        if world.get::<JsSpawned>(entity).is_none() {
            return false;
        }
        world.despawn(entity)
    }

    /// Despawn all entities that were spawned from JavaScript.
    ///
    /// Returns the number of entities removed. Plugin-spawned entities
    /// are never touched.
    pub fn despawn_all_js_entities(&mut self) -> u32 {
        let world = self.engine.world_mut();
        let js_entities: Vec<Entity> = world
            .query::<&JsSpawned>()
            .map(|(entity, _)| entity)
            .collect();
        let count = js_entities.len() as u32;
        for entity in js_entities {
            world.despawn(entity);
        }
        count
    }

    /// Returns the number of entities currently spawned from JavaScript.
    pub fn js_entity_count(&self) -> u32 {
        self.engine.world().query::<&JsSpawned>().count() as u32
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::from_engine(Engine::new())
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

    /// Parent entity indices (one u32 per entity).
    /// `u32::MAX` ([`SCENE_ROOT`]) means the entity is a child of the scene root.
    #[wasm_bindgen(getter)]
    pub fn parent_ids(&self) -> Vec<u32> {
        self.inner.parent_ids.clone()
    }

    /// Per-entity change bitmasks for incremental extraction (parallel to other
    /// entity arrays). Empty for full `extract_frame` packets; consumers should
    /// treat that as "update all fields" (e.g. flag `0xFF` per entity).
    #[wasm_bindgen(getter)]
    pub fn change_flags(&self) -> Vec<u8> {
        self.inner.change_flags.clone()
    }

    /// Object type discriminants (one u8 per entity:
    /// 0=Mesh, 1=PointLight, 2=DirectionalLight, 3=LineSegments, 4=Group).
    #[wasm_bindgen(getter)]
    pub fn object_types(&self) -> Vec<u8> {
        self.inner.object_types.clone()
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
