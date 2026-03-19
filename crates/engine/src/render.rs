// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine_macros::Component;

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
}
