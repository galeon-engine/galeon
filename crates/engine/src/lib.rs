// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

pub use galeon_engine_macros::Component;

/// Returns the engine version string.
pub fn engine_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_not_empty() {
        assert!(!engine_version().is_empty());
    }

    #[test]
    fn derive_component_compiles() {
        #[derive(Component)]
        struct Position {
            _x: f32,
            _y: f32,
        }

        let _pos = Position { _x: 0.0, _y: 0.0 };
    }
}
