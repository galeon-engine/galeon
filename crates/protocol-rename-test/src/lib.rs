// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Renamed-dep consumer test: this crate depends on galeon-engine as `engine`.
//! The macros must still resolve `galeon_engine::` paths via extern crate injection.

// Note: the dependency is named `engine` in Cargo.toml, not `galeon_engine`.
// The `extern crate galeon_engine` emitted by the macros resolves this.

#[engine::command]
pub struct Attack {
    pub target_id: u64,
}

#[engine::dto]
pub struct AttackResult {
    pub damage: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::protocol::{ProtocolKind, ProtocolMeta};

    #[test]
    fn renamed_dep_command_works() {
        let cmd = Attack { target_id: 42 };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: Attack = serde_json::from_str(&json).unwrap();
        assert_eq!(back.target_id, 42);
        assert_eq!(Attack::kind(), ProtocolKind::Command);
    }

    #[test]
    fn renamed_dep_dto_works() {
        let dto = AttackResult { damage: 100 };
        let cloned = dto.clone();
        assert_eq!(cloned.damage, 100);
        assert_eq!(AttackResult::kind(), ProtocolKind::Dto);
    }
}
