// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Handler registration seam — the boundary between generated adapters and
//! game-owned domain logic.
//!
//! Galeon generates adapter glue. The game project implements handlers.
//! Both local and remote adapters target the same [`HandlerRegistry`].
//!
//! # Architecture
//!
//! ```text
//! Protocol definitions (game crate)
//!         │
//!         ▼
//! ┌─────────────────┐
//! │ HandlerRegistry │ ← game registers handlers here
//! └────────┬────────┘
//!          │
//!     ┌────┴────┐
//!     ▼         ▼
//!   Local    Remote
//!  Adapter   Adapter
//! ```
//!
//! # Design Rules
//!
//! - One command type → one handler entry
//! - One query type → one handler entry
//! - Game project owns all handlers; Galeon does not generate domain logic
//! - Local and remote adapters target the same boundary

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::protocol::{Command, ProtocolQuery};

// =============================================================================
// Handler traits — game implements these
// =============================================================================

/// Handler for a command type. Game project implements this.
///
/// `C` is the command type (e.g., `DispatchShip`).
/// `R` is the response type (e.g., `()` or a result DTO).
pub trait CommandHandler<C: Command, R: Serialize>: Send + Sync {
    /// Execute the command and return a response.
    fn handle(&self, cmd: C) -> Result<R, String>;
}

/// Handler for a query type. Game project implements this.
///
/// `Q` is the query type (e.g., `GetFleetSnapshot`).
/// `R` is the response type (e.g., `FleetSnapshot` DTO).
pub trait QueryHandler<Q: ProtocolQuery, R: Serialize>: Send + Sync {
    /// Execute the query and return a response.
    fn handle(&self, query: Q) -> Result<R, String>;
}

// =============================================================================
// Type-erased handler wrappers (internal)
// =============================================================================

/// A type-erased command handler that works with JSON strings.
///
/// This is the boundary between typed game handlers and transport adapters.
trait ErasedCommandHandler: Send + Sync {
    /// Deserialize request JSON, call the typed handler, serialize response.
    fn handle_json(&self, request: &str) -> Result<String, String>;

    /// Call the typed handler with a boxed Any (for local adapter).
    fn handle_any(&self, cmd: Box<dyn Any>) -> Result<Box<dyn Any>, String>;
}

/// A type-erased query handler.
trait ErasedQueryHandler: Send + Sync {
    fn handle_json(&self, request: &str) -> Result<String, String>;
    fn handle_any(&self, query: Box<dyn Any>) -> Result<Box<dyn Any>, String>;
}

/// Wraps a typed CommandHandler into an erased one.
struct CommandHandlerWrapper<C, R, H> {
    handler: H,
    _phantom: std::marker::PhantomData<(C, R)>,
}

impl<C, R, H> ErasedCommandHandler for CommandHandlerWrapper<C, R, H>
where
    C: Command,
    R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    H: CommandHandler<C, R> + Send + Sync,
{
    fn handle_json(&self, request: &str) -> Result<String, String> {
        let cmd: C = serde_json::from_str(request).map_err(|e| e.to_string())?;
        let response = self.handler.handle(cmd)?;
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn handle_any(&self, cmd: Box<dyn Any>) -> Result<Box<dyn Any>, String> {
        let cmd = *cmd.downcast::<C>().map_err(|_| "type mismatch")?;
        let response = self.handler.handle(cmd)?;
        Ok(Box::new(response))
    }
}

/// Wraps a typed QueryHandler into an erased one.
struct QueryHandlerWrapper<Q, R, H> {
    handler: H,
    _phantom: std::marker::PhantomData<(Q, R)>,
}

impl<Q, R, H> ErasedQueryHandler for QueryHandlerWrapper<Q, R, H>
where
    Q: ProtocolQuery,
    R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
    H: QueryHandler<Q, R> + Send + Sync,
{
    fn handle_json(&self, request: &str) -> Result<String, String> {
        let query: Q = serde_json::from_str(request).map_err(|e| e.to_string())?;
        let response = self.handler.handle(query)?;
        serde_json::to_string(&response).map_err(|e| e.to_string())
    }

    fn handle_any(&self, query: Box<dyn Any>) -> Result<Box<dyn Any>, String> {
        let query = *query.downcast::<Q>().map_err(|_| "type mismatch")?;
        let response = self.handler.handle(query)?;
        Ok(Box::new(response))
    }
}

// =============================================================================
// HandlerRegistry — the registration seam
// =============================================================================

/// Registry of command and query handlers.
///
/// Game projects register handlers here. Both local and remote adapters
/// dispatch through this registry.
pub struct HandlerRegistry {
    commands: HashMap<TypeId, (String, Box<dyn ErasedCommandHandler>)>,
    queries: HashMap<TypeId, (String, Box<dyn ErasedQueryHandler>)>,
}

impl HandlerRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            queries: HashMap::new(),
        }
    }

    /// Register a command handler.
    ///
    /// Panics if a handler for this command type is already registered.
    pub fn register_command<C, R, H>(&mut self, handler: H)
    where
        C: Command,
        R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
        H: CommandHandler<C, R> + 'static,
    {
        let type_id = TypeId::of::<C>();
        let name = type_name::<C>().to_string();
        assert!(
            !self.commands.contains_key(&type_id),
            "duplicate command handler for {}",
            name
        );
        self.commands.insert(
            type_id,
            (
                name,
                Box::new(CommandHandlerWrapper {
                    handler,
                    _phantom: std::marker::PhantomData::<(C, R)>,
                }),
            ),
        );
    }

    /// Register a query handler.
    ///
    /// Panics if a handler for this query type is already registered.
    pub fn register_query<Q, R, H>(&mut self, handler: H)
    where
        Q: ProtocolQuery,
        R: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static,
        H: QueryHandler<Q, R> + 'static,
    {
        let type_id = TypeId::of::<Q>();
        let name = type_name::<Q>().to_string();
        assert!(
            !self.queries.contains_key(&type_id),
            "duplicate query handler for {}",
            name
        );
        self.queries.insert(
            type_id,
            (
                name,
                Box::new(QueryHandlerWrapper {
                    handler,
                    _phantom: std::marker::PhantomData::<(Q, R)>,
                }),
            ),
        );
    }

    // -------------------------------------------------------------------------
    // Local adapter interface (in-process, typed dispatch)
    // -------------------------------------------------------------------------

    /// Dispatch a command in-process (local adapter path).
    pub fn dispatch_command<C: Command + 'static, R: 'static>(&self, cmd: C) -> Result<R, String> {
        let entry = self
            .commands
            .get(&TypeId::of::<C>())
            .ok_or_else(|| format!("no handler for command {}", type_name::<C>()))?;

        let result = entry.1.handle_any(Box::new(cmd))?;
        let boxed = result
            .downcast::<R>()
            .map_err(|_| "response type mismatch".to_string())?;
        Ok(*boxed)
    }

    /// Dispatch a query in-process (local adapter path).
    pub fn dispatch_query<Q: ProtocolQuery + 'static, R: 'static>(
        &self,
        query: Q,
    ) -> Result<R, String> {
        let entry = self
            .queries
            .get(&TypeId::of::<Q>())
            .ok_or_else(|| format!("no handler for query {}", type_name::<Q>()))?;

        let result = entry.1.handle_any(Box::new(query))?;
        let boxed = result
            .downcast::<R>()
            .map_err(|_| "response type mismatch".to_string())?;
        Ok(*boxed)
    }

    // -------------------------------------------------------------------------
    // Remote adapter interface (JSON boundary)
    // -------------------------------------------------------------------------

    /// Dispatch a command via JSON (remote adapter path).
    ///
    /// `command_name` is matched against registered handler type names.
    pub fn dispatch_command_json(
        &self,
        command_type_id: TypeId,
        request_json: &str,
    ) -> Result<String, String> {
        let entry = self
            .commands
            .get(&command_type_id)
            .ok_or("unknown command")?;
        entry.1.handle_json(request_json)
    }

    /// Dispatch a query via JSON (remote adapter path).
    pub fn dispatch_query_json(
        &self,
        query_type_id: TypeId,
        request_json: &str,
    ) -> Result<String, String> {
        let entry = self.queries.get(&query_type_id).ok_or("unknown query")?;
        entry.1.handle_json(request_json)
    }

    /// Returns the number of registered command handlers.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Returns the number of registered query handlers.
    pub fn query_count(&self) -> usize {
        self.queries.len()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Sample protocol items --

    #[derive(Debug, Serialize, Deserialize)]
    struct DispatchShip {
        ship_id: u64,
        contract_id: u64,
    }
    impl Command for DispatchShip {}

    #[derive(Debug, Serialize, Deserialize)]
    struct GetFleetSnapshot;
    impl ProtocolQuery for GetFleetSnapshot {}

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct FleetSnapshot {
        ships_in_transit: u32,
        ships_docked: u32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct DispatchResult {
        ok: bool,
    }

    // -- Sample handlers --

    struct ShipDispatcher;

    impl CommandHandler<DispatchShip, DispatchResult> for ShipDispatcher {
        fn handle(&self, _cmd: DispatchShip) -> Result<DispatchResult, String> {
            Ok(DispatchResult { ok: true })
        }
    }

    struct FleetQuerier;

    impl QueryHandler<GetFleetSnapshot, FleetSnapshot> for FleetQuerier {
        fn handle(&self, _query: GetFleetSnapshot) -> Result<FleetSnapshot, String> {
            Ok(FleetSnapshot {
                ships_in_transit: 2,
                ships_docked: 5,
            })
        }
    }

    // -- Registry tests --

    #[test]
    fn register_and_dispatch_command_local() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);

        let result: DispatchResult = registry
            .dispatch_command(DispatchShip {
                ship_id: 1,
                contract_id: 42,
            })
            .unwrap();

        assert!(result.ok);
    }

    #[test]
    fn register_and_dispatch_query_local() {
        let mut registry = HandlerRegistry::new();
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        let snapshot: FleetSnapshot = registry.dispatch_query(GetFleetSnapshot).unwrap();

        assert_eq!(snapshot.ships_in_transit, 2);
        assert_eq!(snapshot.ships_docked, 5);
    }

    #[test]
    fn dispatch_command_json() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);

        let response = registry
            .dispatch_command_json(
                TypeId::of::<DispatchShip>(),
                r#"{"ship_id":1,"contract_id":42}"#,
            )
            .unwrap();

        assert!(response.contains("true"));
    }

    #[test]
    fn dispatch_query_json() {
        let mut registry = HandlerRegistry::new();
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        let response = registry
            .dispatch_query_json(TypeId::of::<GetFleetSnapshot>(), "null")
            .unwrap();

        let snapshot: FleetSnapshot = serde_json::from_str(&response).unwrap();
        assert_eq!(snapshot.ships_docked, 5);
    }

    #[test]
    fn same_registry_serves_both_adapters() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        // Local adapter
        let local_result: DispatchResult = registry
            .dispatch_command(DispatchShip {
                ship_id: 1,
                contract_id: 42,
            })
            .unwrap();
        assert!(local_result.ok);

        let local_snapshot: FleetSnapshot = registry.dispatch_query(GetFleetSnapshot).unwrap();
        assert_eq!(local_snapshot.ships_docked, 5);

        // Remote adapter (JSON)
        let json_result = registry
            .dispatch_command_json(
                TypeId::of::<DispatchShip>(),
                r#"{"ship_id":2,"contract_id":99}"#,
            )
            .unwrap();
        assert!(json_result.contains("true"));

        let json_snapshot = registry
            .dispatch_query_json(TypeId::of::<GetFleetSnapshot>(), "null")
            .unwrap();
        let remote_snapshot: FleetSnapshot = serde_json::from_str(&json_snapshot).unwrap();
        assert_eq!(remote_snapshot.ships_docked, 5);
    }

    #[test]
    fn missing_handler_returns_error() {
        let registry = HandlerRegistry::new();
        let result = registry.dispatch_command::<DispatchShip, DispatchResult>(DispatchShip {
            ship_id: 1,
            contract_id: 1,
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no handler"));
    }

    #[test]
    #[should_panic(expected = "duplicate command handler")]
    fn duplicate_handler_panics() {
        let mut registry = HandlerRegistry::new();
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
    }

    #[test]
    fn registry_counts() {
        let mut registry = HandlerRegistry::new();
        assert_eq!(registry.command_count(), 0);
        assert_eq!(registry.query_count(), 0);

        registry.register_command::<DispatchShip, DispatchResult, _>(ShipDispatcher);
        registry.register_query::<GetFleetSnapshot, FleetSnapshot, _>(FleetQuerier);

        assert_eq!(registry.command_count(), 1);
        assert_eq!(registry.query_count(), 1);
    }
}
