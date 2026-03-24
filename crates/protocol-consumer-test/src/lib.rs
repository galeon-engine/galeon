// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Downstream consumer test: this crate depends ONLY on galeon-engine,
//! not on serde directly. The macros must work without a direct serde dep.

#[galeon_engine::command]
pub struct MoveUnit {
    pub unit_id: u64,
    pub target_x: f32,
    pub target_y: f32,
}

#[galeon_engine::query]
pub struct GetUnitPosition;

#[galeon_engine::event]
pub struct UnitMoved {
    pub unit_id: u64,
    pub x: f32,
    pub y: f32,
}

#[galeon_engine::dto]
pub struct UnitSnapshot {
    pub unit_id: u64,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use galeon_engine::protocol::{ProtocolKind, ProtocolMeta};

    #[test]
    fn command_compiles_and_serializes() {
        let cmd = MoveUnit {
            unit_id: 1,
            target_x: 10.0,
            target_y: 20.0,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let back: MoveUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(back.unit_id, 1);
        assert_eq!(MoveUnit::kind(), ProtocolKind::Command);
    }

    #[test]
    fn query_unit_struct_works() {
        let q = GetUnitPosition;
        let json = serde_json::to_string(&q).unwrap();
        let _back: GetUnitPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(GetUnitPosition::kind(), ProtocolKind::Query);
        let _ = q;
    }

    #[test]
    fn event_compiles_and_serializes() {
        let evt = UnitMoved {
            unit_id: 1,
            x: 5.0,
            y: 10.0,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: UnitMoved = serde_json::from_str(&json).unwrap();
        assert_eq!(back.unit_id, 1);
        assert_eq!(UnitMoved::kind(), ProtocolKind::Event);
    }

    #[test]
    fn dto_compiles_clones_and_serializes() {
        let dto = UnitSnapshot {
            unit_id: 1,
            name: "Scout".to_string(),
        };
        let cloned = dto.clone();
        assert_eq!(cloned.name, "Scout");
        let json = serde_json::to_string(&dto).unwrap();
        let back: UnitSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Scout");
        assert_eq!(UnitSnapshot::kind(), ProtocolKind::Dto);
    }
}
