// SPDX-License-Identifier: AGPL-3.0-only OR Commercial
//
//! Proof-of-concept: one command, one query, one event, one DTO,
//! one local adapter, one remote adapter — same handler seam.
//!
//! This validates the architecture from #67 Decision 6 before codegen exists.
//! All adapters and the handler seam are hand-written here to prove the pattern.
//! Galeon will later generate the adapter glue; the handler seam stays manual.

use serde::{Deserialize, Serialize};

// =============================================================================
// 1. Protocol definitions (would live in the game protocol crate)
// =============================================================================

/// Command: spawn a unit at a location.
#[derive(Debug, Serialize, Deserialize)]
struct SpawnUnit {
    unit_id: u64,
    location_id: u64,
}

/// Query: request the current world snapshot (unit struct — no params).
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct GetWorldSnapshot;

/// Event: a unit has been destroyed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct UnitDestroyed {
    unit_id: u64,
    destination: String,
}

/// DTO: world status view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct WorldSnapshot {
    units_active: u32,
    units_idle: u32,
}

// =============================================================================
// 2. Handler seam (game project owns these — Galeon never generates them)
// =============================================================================

/// Minimal game state for the proof.
struct GameState {
    units_active: u32,
    units_idle: u32,
    events: Vec<UnitDestroyed>,
}

impl GameState {
    fn new() -> Self {
        Self {
            units_active: 0,
            units_idle: 3,
            events: Vec::new(),
        }
    }
}

/// Handler result for commands.
type CmdResult = Result<(), String>;

/// Handler result for queries.
type QueryResult<T> = Result<T, String>;

/// The handler seam — one entry per command/query. Game project implements this.
/// Local and remote adapters both target this same trait.
trait GameHandlers {
    fn handle_spawn_unit(&mut self, cmd: SpawnUnit) -> CmdResult;
    fn handle_get_world_snapshot(&self) -> QueryResult<WorldSnapshot>;
    fn drain_events(&mut self) -> Vec<UnitDestroyed>;
}

impl GameHandlers for GameState {
    fn handle_spawn_unit(&mut self, cmd: SpawnUnit) -> CmdResult {
        if self.units_idle == 0 {
            return Err("no units available".into());
        }
        self.units_idle -= 1;
        self.units_active += 1;
        self.events.push(UnitDestroyed {
            unit_id: cmd.unit_id,
            destination: format!("contract-{}", cmd.location_id),
        });
        Ok(())
    }

    fn handle_get_world_snapshot(&self) -> QueryResult<WorldSnapshot> {
        Ok(WorldSnapshot {
            units_active: self.units_active,
            units_idle: self.units_idle,
        })
    }

    fn drain_events(&mut self) -> Vec<UnitDestroyed> {
        std::mem::take(&mut self.events)
    }
}

// =============================================================================
// 3. Local adapter (in-process — direct dispatch, no serialization)
// =============================================================================

/// Local adapter: calls handlers directly without serialization.
/// This is the shape Galeon would generate for in-process mode.
struct LocalAdapter<'a> {
    state: &'a mut GameState,
}

impl<'a> LocalAdapter<'a> {
    fn spawn_unit(&mut self, cmd: SpawnUnit) -> CmdResult {
        self.state.handle_spawn_unit(cmd)
    }

    fn get_world_snapshot(&self) -> QueryResult<WorldSnapshot> {
        self.state.handle_get_world_snapshot()
    }

    fn drain_events(&mut self) -> Vec<UnitDestroyed> {
        self.state.drain_events()
    }
}

// =============================================================================
// 4. Remote adapter (simulated HTTP — serializes through JSON boundary)
// =============================================================================

/// Simulated remote adapter: serializes request/response through JSON.
/// In production, this would be HTTP POST/GET; here we simulate the boundary.
struct RemoteAdapter<'a> {
    state: &'a mut GameState,
}

impl<'a> RemoteAdapter<'a> {
    /// Simulate POST /commands/dispatch-unit
    fn spawn_unit_json(&mut self, request_json: &str) -> String {
        let cmd: SpawnUnit = serde_json::from_str(request_json).unwrap();
        let result = self.state.handle_spawn_unit(cmd);
        match result {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
        }
    }

    /// Simulate GET /queries/world-snapshot
    fn get_world_snapshot_json(&self) -> String {
        let result = self.state.handle_get_world_snapshot().unwrap();
        serde_json::to_string(&result).unwrap()
    }

    /// Simulate WS event stream
    fn drain_events_json(&mut self) -> Vec<String> {
        self.state
            .drain_events()
            .into_iter()
            .map(|e| serde_json::to_string(&e).unwrap())
            .collect()
    }
}

// =============================================================================
// 5. Proof: same protocol, same handlers, two execution modes
// =============================================================================

fn main() {
    println!("=== Protocol Proof of Concept ===\n");

    // --- Local adapter (in-process) ---
    println!("--- Local Adapter (in-process) ---");
    let mut state = GameState::new();
    {
        let mut local = LocalAdapter { state: &mut state };

        let snapshot = local.get_world_snapshot().unwrap();
        println!("Before: {:?}", snapshot);
        assert_eq!(snapshot.units_idle, 3);
        assert_eq!(snapshot.units_active, 0);

        local
            .spawn_unit(SpawnUnit {
                unit_id: 1,
                location_id: 42,
            })
            .unwrap();

        let snapshot = local.get_world_snapshot().unwrap();
        println!("After dispatch: {:?}", snapshot);
        assert_eq!(snapshot.units_idle, 2);
        assert_eq!(snapshot.units_active, 1);

        let events = local.drain_events();
        println!("Events: {:?}", events);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            UnitDestroyed {
                unit_id: 1,
                destination: "contract-42".into(),
            }
        );
    }
    println!("Local adapter: PASS\n");

    // --- Remote adapter (simulated HTTP/JSON boundary) ---
    println!("--- Remote Adapter (simulated HTTP) ---");
    let mut state = GameState::new();
    {
        let mut remote = RemoteAdapter { state: &mut state };

        let snapshot_json = remote.get_world_snapshot_json();
        println!("GET /queries/world-snapshot -> {}", snapshot_json);
        let snapshot: WorldSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        assert_eq!(snapshot.units_idle, 3);

        let request = r#"{"unit_id":2,"location_id":99}"#;
        let response = remote.spawn_unit_json(request);
        println!("POST /commands/dispatch-unit {} -> {}", request, response);
        assert!(response.contains("true"));

        let snapshot_json = remote.get_world_snapshot_json();
        let snapshot: WorldSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        println!("GET /queries/world-snapshot -> {:?}", snapshot);
        assert_eq!(snapshot.units_idle, 2);
        assert_eq!(snapshot.units_active, 1);

        let event_jsons = remote.drain_events_json();
        println!("WS events: {:?}", event_jsons);
        assert_eq!(event_jsons.len(), 1);
        let event: UnitDestroyed = serde_json::from_str(&event_jsons[0]).unwrap();
        assert_eq!(event.unit_id, 2);
        assert_eq!(event.destination, "contract-99");
    }
    println!("Remote adapter: PASS\n");

    println!("=== Proof Complete ===");
    println!("Same GameHandlers trait, same protocol types,");
    println!("two execution modes (local + remote) — architecture validated.");
}
