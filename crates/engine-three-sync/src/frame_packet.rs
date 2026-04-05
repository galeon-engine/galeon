// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::collections::HashMap;

use galeon_engine::FrameEvent;

/// Per-channel float data attached to a [`FramePacket`].
///
/// The `data` array is flat: for `n` entities and a stride of `s`, the values
/// for entity `i` live at `data[i * stride .. i * stride + stride]`.
#[derive(Debug, Clone)]
pub struct ChannelData {
    /// Number of f32 values per entity in this channel.
    pub stride: usize,
    /// Flat buffer of all per-entity values for this channel.
    pub data: Vec<f32>,
}

/// Packed per-frame render data extracted from the ECS.
///
/// Uses a struct-of-arrays layout for efficient WASM transport.
/// Each array is parallel — index `i` in every array refers to the same entity.
///
/// Transform data is packed as 10 contiguous floats per entity:
/// `[pos.x, pos.y, pos.z, rot.x, rot.y, rot.z, rot.w, scale.x, scale.y, scale.z]`
pub struct FramePacket {
    pub entity_ids: Vec<u32>,
    pub entity_generations: Vec<u32>,
    pub transforms: Vec<f32>,
    pub visibility: Vec<u8>,
    pub mesh_handles: Vec<u32>,
    pub material_handles: Vec<u32>,
    /// Parent entity index for scene-graph hierarchy (one u32 per entity).
    /// [`SCENE_ROOT`] means the entity is a child of the scene root.
    pub parent_ids: Vec<u32>,
    /// Three.js object type discriminant (one u8 per entity).
    pub object_types: Vec<u8>,
    /// Named per-entity float channels for game-specific render data.
    pub custom_channels: HashMap<String, ChannelData>,
    /// One-shot events for audio/VFX triggers (fire-and-forget, not per-entity).
    pub events: Vec<FrameEvent>,
    /// Per-entity change flags for incremental extraction.
    /// Bit 0: transform, Bit 1: visibility, Bit 2: mesh, Bit 3: material,
    /// Bit 4: object type, Bit 5: parent.
    /// Empty for full extractions.
    pub change_flags: Vec<u8>,
    /// Monotonic version counter — incremented when extractable state changes.
    /// Consumers can skip `applyFrame()` when the version hasn't changed.
    pub frame_version: u64,
}

/// Change-flag bit positions.
pub const CHANGED_TRANSFORM: u8 = 1 << 0;
pub const CHANGED_VISIBILITY: u8 = 1 << 1;
pub const CHANGED_MESH: u8 = 1 << 2;
pub const CHANGED_MATERIAL: u8 = 1 << 3;
pub const CHANGED_OBJECT_TYPE: u8 = 1 << 4;
pub const CHANGED_PARENT: u8 = 1 << 5;

/// Sentinel value in `parent_ids` meaning "child of scene root" (no parent entity).
pub const SCENE_ROOT: u32 = u32::MAX;

/// Number of f32 values per entity in the transforms array.
pub const TRANSFORM_STRIDE: usize = 10;

impl FramePacket {
    /// Create an empty frame packet.
    pub fn new() -> Self {
        Self {
            entity_ids: Vec::new(),
            entity_generations: Vec::new(),
            transforms: Vec::new(),
            visibility: Vec::new(),
            mesh_handles: Vec::new(),
            material_handles: Vec::new(),
            parent_ids: Vec::new(),
            object_types: Vec::new(),
            custom_channels: HashMap::new(),
            events: Vec::new(),
            change_flags: Vec::new(),
            frame_version: 0,
        }
    }

    /// Create a frame packet with pre-allocated capacity.
    pub fn with_capacity(entity_count: usize) -> Self {
        Self {
            entity_ids: Vec::with_capacity(entity_count),
            entity_generations: Vec::with_capacity(entity_count),
            transforms: Vec::with_capacity(entity_count * TRANSFORM_STRIDE),
            visibility: Vec::with_capacity(entity_count),
            mesh_handles: Vec::with_capacity(entity_count),
            material_handles: Vec::with_capacity(entity_count),
            parent_ids: Vec::with_capacity(entity_count),
            object_types: Vec::with_capacity(entity_count),
            custom_channels: HashMap::new(),
            events: Vec::new(),
            change_flags: Vec::with_capacity(entity_count),
            frame_version: 0,
        }
    }

    /// Push one entity's render data into the packet.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        entity_id: u32,
        entity_generation: u32,
        position: &[f32; 3],
        rotation: &[f32; 4],
        scale: &[f32; 3],
        visible: bool,
        mesh_id: u32,
        material_id: u32,
        parent_id: u32,
        object_type: u8,
    ) {
        self.entity_ids.push(entity_id);
        self.entity_generations.push(entity_generation);
        self.transforms.extend_from_slice(position);
        self.transforms.extend_from_slice(rotation);
        self.transforms.extend_from_slice(scale);
        self.visibility.push(visible as u8);
        self.mesh_handles.push(mesh_id);
        self.material_handles.push(material_id);
        self.parent_ids.push(parent_id);
        self.object_types.push(object_type);
    }

    /// Push render data with change flags (incremental extraction).
    #[allow(clippy::too_many_arguments)]
    pub fn push_incremental(
        &mut self,
        entity_id: u32,
        generation: u32,
        position: &[f32; 3],
        rotation: &[f32; 4],
        scale: &[f32; 3],
        visible: bool,
        mesh_id: u32,
        material_id: u32,
        parent_id: u32,
        object_type: u8,
        flags: u8,
    ) {
        self.push(
            entity_id,
            generation,
            position,
            rotation,
            scale,
            visible,
            mesh_id,
            material_id,
            parent_id,
            object_type,
        );
        self.change_flags.push(flags);
    }

    /// Number of entities in this packet.
    pub fn entity_count(&self) -> usize {
        self.entity_ids.len()
    }

    /// Number of custom channels attached to this packet.
    pub fn channel_count(&self) -> usize {
        self.custom_channels.len()
    }

    /// Sorted list of custom channel names.
    ///
    /// Sorted alphabetically so callers get a deterministic order regardless
    /// of insertion order.
    pub fn channel_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.custom_channels.keys().map(|s| s.as_str()).collect();
        names.sort_unstable();
        names
    }

    /// Look up a custom channel by name. Returns `None` if the channel does
    /// not exist.
    pub fn channel(&self, name: &str) -> Option<&ChannelData> {
        self.custom_channels.get(name)
    }

    /// Number of one-shot events in this frame.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl Default for FramePacket {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_packet() {
        let p = FramePacket::new();
        assert_eq!(p.entity_count(), 0);
        assert!(p.entity_ids.is_empty());
        assert_eq!(p.channel_count(), 0);
    }

    #[test]
    fn custom_channel_insertion() {
        let mut p = FramePacket::new();
        p.custom_channels.insert(
            "health".to_string(),
            ChannelData {
                stride: 1,
                data: vec![1.0, 0.5],
            },
        );
        assert_eq!(p.channel_count(), 1);
        assert_eq!(p.channel_names(), vec!["health"]);
        let ch = p.channel("health").unwrap();
        assert_eq!(ch.stride, 1);
        assert_eq!(ch.data, vec![1.0, 0.5]);
    }

    #[test]
    fn multiple_custom_channels() {
        let mut p = FramePacket::new();
        p.custom_channels.insert(
            "wear".to_string(),
            ChannelData {
                stride: 1,
                data: vec![0.0, 0.9],
            },
        );
        p.custom_channels.insert(
            "anim".to_string(),
            ChannelData {
                stride: 2,
                data: vec![0.0, 1.0, 0.5, 0.0],
            },
        );
        assert_eq!(p.channel_count(), 2);
        assert_eq!(p.channel_names(), vec!["anim", "wear"]);
    }

    #[test]
    fn channel_returns_none_for_missing() {
        let p = FramePacket::new();
        assert!(p.channel("nonexistent").is_none());
    }

    #[test]
    fn push_one_entity() {
        let mut p = FramePacket::new();
        p.push(
            42,
            0,
            &[1.0, 2.0, 3.0],
            &[0.0, 0.0, 0.0, 1.0],
            &[1.0, 1.0, 1.0],
            true,
            10,
            20,
            SCENE_ROOT,
            0,
        );
        assert_eq!(p.entity_count(), 1);
        assert_eq!(p.entity_ids[0], 42);
        assert_eq!(p.entity_generations[0], 0);
        assert_eq!(p.transforms.len(), TRANSFORM_STRIDE);
        assert_eq!(p.transforms[0], 1.0);
        assert_eq!(p.transforms[6], 1.0);
        assert_eq!(p.visibility[0], 1);
        assert_eq!(p.mesh_handles[0], 10);
        assert_eq!(p.material_handles[0], 20);
        assert_eq!(p.parent_ids[0], SCENE_ROOT);
        assert_eq!(p.object_types[0], 0);
    }

    #[test]
    fn push_multiple_entities() {
        let mut p = FramePacket::with_capacity(2);
        p.push(
            0,
            0,
            &[0.0; 3],
            &[0.0, 0.0, 0.0, 1.0],
            &[1.0; 3],
            true,
            1,
            1,
            SCENE_ROOT,
            0,
        );
        p.push(
            1,
            0,
            &[5.0; 3],
            &[0.0, 0.0, 0.0, 1.0],
            &[2.0; 3],
            false,
            2,
            3,
            0,
            0,
        );
        assert_eq!(p.entity_count(), 2);
        assert_eq!(p.transforms.len(), TRANSFORM_STRIDE * 2);
        assert_eq!(p.visibility[1], 0);
        assert_eq!(p.parent_ids[0], SCENE_ROOT);
        assert_eq!(p.parent_ids[1], 0);
        assert_eq!(p.object_types[0], 0);
        assert_eq!(p.object_types[1], 0);
    }

    #[test]
    fn new_packet_has_frame_version_zero() {
        let p = FramePacket::new();
        assert_eq!(p.frame_version, 0);
        let p2 = FramePacket::with_capacity(10);
        assert_eq!(p2.frame_version, 0);
    }

    #[test]
    fn push_stores_object_type() {
        let mut p = FramePacket::new();
        p.push(
            1,
            0,
            &[0.0; 3],
            &[0.0, 0.0, 0.0, 1.0],
            &[1.0; 3],
            true,
            10,
            20,
            SCENE_ROOT,
            2,
        );
        assert_eq!(p.entity_count(), 1);
        assert_eq!(p.object_types[0], 2);
    }
}
