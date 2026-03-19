// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

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
}

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
    ) {
        self.entity_ids.push(entity_id);
        self.entity_generations.push(entity_generation);
        self.transforms.extend_from_slice(position);
        self.transforms.extend_from_slice(rotation);
        self.transforms.extend_from_slice(scale);
        self.visibility.push(visible as u8);
        self.mesh_handles.push(mesh_id);
        self.material_handles.push(material_id);
    }

    /// Number of entities in this packet.
    pub fn entity_count(&self) -> usize {
        self.entity_ids.len()
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
        );
        assert_eq!(p.entity_count(), 1);
        assert_eq!(p.entity_ids[0], 42);
        assert_eq!(p.entity_generations[0], 0);
        assert_eq!(p.transforms.len(), TRANSFORM_STRIDE);
        assert_eq!(p.transforms[0], 1.0); // pos.x
        assert_eq!(p.transforms[6], 1.0); // rot.w
        assert_eq!(p.visibility[0], 1);
        assert_eq!(p.mesh_handles[0], 10);
        assert_eq!(p.material_handles[0], 20);
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
        );
        assert_eq!(p.entity_count(), 2);
        assert_eq!(p.transforms.len(), TRANSFORM_STRIDE * 2);
        assert_eq!(p.visibility[1], 0); // false
    }
}
