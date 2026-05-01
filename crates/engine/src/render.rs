// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine_macros::Component;

use crate::entity::Entity;

/// 3D transform: position, rotation (quaternion), scale.
///
/// Flat array layout for efficient extraction into typed buffers.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl Transform {
    /// Identity transform: origin, no rotation, unit scale.
    pub fn identity() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        }
    }

    /// Create a transform with only position set.
    pub fn from_position(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
            ..Self::identity()
        }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

/// Whether an entity is visible to the renderer.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    pub visible: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Handle to a mesh asset. The renderer maps this ID to a Three.js geometry.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshHandle {
    pub id: u32,
}

/// Handle to a material asset. The renderer maps this ID to a Three.js material.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialHandle {
    pub id: u32,
}

/// Marks an entity as a member of a GPU-instanced mesh batch.
///
/// When present, the renderer routes the entity's transform into a shared
/// `THREE.InstancedMesh` keyed by the wrapped [`MeshHandle`], instead of
/// creating a standalone `Object3D` per entity. Used for crowd-scale
/// rendering (1000+ entities sharing one geometry).
///
/// The wrapped `MeshHandle` is the instance-group identifier — entities that
/// share the same `InstanceOf(handle)` share the same `InstancedMesh`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstanceOf(pub MeshHandle);

/// Parent entity for scene-graph hierarchy.
///
/// Attaching this component to an entity makes it a child of the referenced
/// entity in the render scene graph. The renderer uses this to build
/// Three.js parent-child relationships so transforms inherit correctly.
///
/// Entities without this component are children of the scene root.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParentEntity(pub Entity);

/// What kind of Three.js object to create for this entity.
///
/// Extracted as a `u8` in the FramePacket. The TS renderer uses this
/// to pick the correct constructor (Mesh, PointLight, etc.).
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ObjectType {
    /// `THREE.Mesh` — the default for renderable entities.
    #[default]
    Mesh = 0,
    /// `THREE.PointLight` — omni-directional light source.
    PointLight = 1,
    /// `THREE.DirectionalLight` — sun-like parallel light.
    DirectionalLight = 2,
    /// `THREE.LineSegments` — debug line rendering.
    LineSegments = 3,
    /// `THREE.Group` — container for hierarchy.
    Group = 4,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_identity() {
        let t = Transform::identity();
        assert_eq!(t.position, [0.0, 0.0, 0.0]);
        assert_eq!(t.rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(t.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn transform_from_position() {
        let t = Transform::from_position(1.0, 2.0, 3.0);
        assert_eq!(t.position, [1.0, 2.0, 3.0]);
        assert_eq!(t.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn visibility_default_is_visible() {
        assert!(Visibility::default().visible);
    }

    #[test]
    fn parent_entity_stores_entity() {
        let entity = crate::entity::Entity::from_raw(42, 0);
        let parent = ParentEntity(entity);
        assert_eq!(parent.0, entity);
    }

    #[test]
    fn object_type_default_is_mesh() {
        assert_eq!(ObjectType::default(), ObjectType::Mesh);
    }

    #[test]
    fn instance_of_wraps_mesh_handle() {
        let handle = MeshHandle { id: 42 };
        let tag = InstanceOf(handle);
        assert_eq!(tag.0, handle);
        assert_eq!(tag.0.id, 42);
    }

    #[test]
    fn instance_of_equality_is_by_mesh_handle() {
        assert_eq!(
            InstanceOf(MeshHandle { id: 7 }),
            InstanceOf(MeshHandle { id: 7 })
        );
        assert_ne!(
            InstanceOf(MeshHandle { id: 7 }),
            InstanceOf(MeshHandle { id: 8 })
        );
    }

    #[test]
    fn object_type_as_u8() {
        assert_eq!(ObjectType::Mesh as u8, 0);
        assert_eq!(ObjectType::PointLight as u8, 1);
        assert_eq!(ObjectType::DirectionalLight as u8, 2);
        assert_eq!(ObjectType::LineSegments as u8, 3);
        assert_eq!(ObjectType::Group as u8, 4);
    }
}
