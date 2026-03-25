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

/// Command: dispatch a ship on a contract route.
#[derive(Debug, Serialize, Deserialize)]
struct DispatchShip {
    ship_id: u64,
    contract_id: u64,
}

/// Query: request the current fleet snapshot (unit struct — no params).
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct GetFleetSnapshot;

/// Event: a ship arrived at its destination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ShipArrived {
    ship_id: u64,
    destination: String,
}

/// DTO: fleet status view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FleetSnapshot {
    ships_in_transit: u32,
    ships_docked: u32,
}

// =============================================================================
// 2. Handler seam (game project owns these — Galeon never generates them)
// =============================================================================

/// Minimal game state for the proof.
struct GameState {
    ships_in_transit: u32,
    ships_docked: u32,
    events: Vec<ShipArrived>,
}

impl GameState {
    fn new() -> Self {
        Self {
            ships_in_transit: 0,
            ships_docked: 3,
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
    fn handle_dispatch_ship(&mut self, cmd: DispatchShip) -> CmdResult;
    fn handle_get_fleet_snapshot(&self) -> QueryResult<FleetSnapshot>;
    fn drain_events(&mut self) -> Vec<ShipArrived>;
}

impl GameHandlers for GameState {
    fn handle_dispatch_ship(&mut self, cmd: DispatchShip) -> CmdResult {
        if self.ships_docked == 0 {
            return Err("no ships available".into());
        }
        self.ships_docked -= 1;
        self.ships_in_transit += 1;
        self.events.push(ShipArrived {
            ship_id: cmd.ship_id,
            destination: format!("contract-{}", cmd.contract_id),
        });
        Ok(())
    }

    fn handle_get_fleet_snapshot(&self) -> QueryResult<FleetSnapshot> {
        Ok(FleetSnapshot {
            ships_in_transit: self.ships_in_transit,
            ships_docked: self.ships_docked,
        })
    }

    fn drain_events(&mut self) -> Vec<ShipArrived> {
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
    fn dispatch_ship(&mut self, cmd: DispatchShip) -> CmdResult {
        self.state.handle_dispatch_ship(cmd)
    }

    fn get_fleet_snapshot(&self) -> QueryResult<FleetSnapshot> {
        self.state.handle_get_fleet_snapshot()
    }

    fn drain_events(&mut self) -> Vec<ShipArrived> {
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
    /// Simulate POST /commands/dispatch-ship
    fn dispatch_ship_json(&mut self, request_json: &str) -> String {
        let cmd: DispatchShip = serde_json::from_str(request_json).unwrap();
        let result = self.state.handle_dispatch_ship(cmd);
        match result {
            Ok(()) => r#"{"ok":true}"#.to_string(),
            Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
        }
    }

    /// Simulate GET /queries/fleet-snapshot
    fn get_fleet_snapshot_json(&self) -> String {
        let result = self.state.handle_get_fleet_snapshot().unwrap();
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

        let snapshot = local.get_fleet_snapshot().unwrap();
        println!("Before: {:?}", snapshot);
        assert_eq!(snapshot.ships_docked, 3);
        assert_eq!(snapshot.ships_in_transit, 0);

        local
            .dispatch_ship(DispatchShip {
                ship_id: 1,
                contract_id: 42,
            })
            .unwrap();

        let snapshot = local.get_fleet_snapshot().unwrap();
        println!("After dispatch: {:?}", snapshot);
        assert_eq!(snapshot.ships_docked, 2);
        assert_eq!(snapshot.ships_in_transit, 1);

        let events = local.drain_events();
        println!("Events: {:?}", events);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            ShipArrived {
                ship_id: 1,
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

        let snapshot_json = remote.get_fleet_snapshot_json();
        println!("GET /queries/fleet-snapshot -> {}", snapshot_json);
        let snapshot: FleetSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        assert_eq!(snapshot.ships_docked, 3);

        let request = r#"{"ship_id":2,"contract_id":99}"#;
        let response = remote.dispatch_ship_json(request);
        println!("POST /commands/dispatch-ship {} -> {}", request, response);
        assert!(response.contains("true"));

        let snapshot_json = remote.get_fleet_snapshot_json();
        let snapshot: FleetSnapshot = serde_json::from_str(&snapshot_json).unwrap();
        println!("GET /queries/fleet-snapshot -> {:?}", snapshot);
        assert_eq!(snapshot.ships_docked, 2);
        assert_eq!(snapshot.ships_in_transit, 1);

        let event_jsons = remote.drain_events_json();
        println!("WS events: {:?}", event_jsons);
        assert_eq!(event_jsons.len(), 1);
        let event: ShipArrived = serde_json::from_str(&event_jsons[0]).unwrap();
        assert_eq!(event.ship_id, 2);
        assert_eq!(event.destination, "contract-99");
    }
    println!("Remote adapter: PASS\n");

    println!("=== Proof Complete ===");
    println!("Same GameHandlers trait, same protocol types,");
    println!("two execution modes (local + remote) — architecture validated.");
}
