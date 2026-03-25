// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use std::any::TypeId;

use crate::component::Component;
use crate::deadline::{DeadlineId, Timestamp};
use crate::entity::Entity;
use crate::system_param::Access;
use crate::world::{Bundle, UnsafeWorldCell, World};

/// A boxed, type-erased command closure.
type BoxedCommand = Box<dyn FnOnce(&mut World)>;

// =============================================================================
// CommandBuffer — internal queue of deferred world mutations
// =============================================================================

/// A buffer of deferred structural mutations applied between schedule stages.
///
/// Commands are not the hot iteration path, so boxing each command is
/// acceptable. The buffer is drained by [`World::apply_commands`].
pub struct CommandBuffer {
    queue: Vec<BoxedCommand>,
}

impl CommandBuffer {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    /// Push a type-erased command onto the buffer.
    fn push(&mut self, cmd: impl FnOnce(&mut World) + 'static) {
        self.queue.push(Box::new(cmd));
    }

    /// Returns the number of queued commands.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if no commands are queued.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Take all queued commands out, leaving the buffer empty.
    ///
    /// Used by [`World::apply_commands`] to drain the queue without
    /// holding a borrow on the buffer while executing commands.
    pub(crate) fn take(&mut self) -> Vec<BoxedCommand> {
        std::mem::take(&mut self.queue)
    }
}

impl Default for CommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Commands — typed system parameter for deferred mutations
// =============================================================================

/// System parameter that queues structural mutations for deferred application.
///
/// `Commands` buffers spawn, despawn, insert, and remove operations. These
/// are applied between schedule stages via [`World::apply_commands`], avoiding
/// mid-iteration archetype changes.
///
/// ```rust,ignore
/// fn spawn_units(mut cmds: Commands<'_>) {
///     cmds.spawn((Position { x: 0.0, y: 0.0 },));
///     cmds.despawn(old_entity);
///     cmds.insert(entity, Health(100));
///     cmds.remove::<Velocity>(entity);
/// }
/// ```
pub struct Commands<'w> {
    buffer: &'w mut CommandBuffer,
}

impl<'w> Commands<'w> {
    /// Spawn an entity with the given component bundle (deferred).
    pub fn spawn<B: Bundle + Send + 'static>(&mut self, bundle: B) {
        self.buffer.push(move |world: &mut World| {
            world.spawn(bundle);
        });
    }

    /// Despawn an entity (deferred).
    pub fn despawn(&mut self, entity: Entity) {
        self.buffer.push(move |world: &mut World| {
            world.despawn(entity);
        });
    }

    /// Insert a component into an entity (deferred).
    ///
    /// If the entity already has this component type, the value is overwritten.
    pub fn insert<C: Component>(&mut self, entity: Entity, component: C) {
        self.buffer.push(move |world: &mut World| {
            world.insert(entity, component);
        });
    }

    /// Remove a component from an entity (deferred).
    pub fn remove<C: Component>(&mut self, entity: Entity) {
        self.buffer.push(move |world: &mut World| {
            world.remove::<C>(entity);
        });
    }

    /// Schedule a deadline event (deferred).
    ///
    /// The event type must have been registered with
    /// [`World::add_deadline_type::<T>()`].
    pub fn schedule_deadline<T: Send + 'static>(&mut self, deadline: Timestamp, event: T) {
        self.buffer.push(move |world: &mut World| {
            world.schedule_deadline(deadline, event);
        });
    }

    /// Cancel a previously scheduled deadline (deferred).
    pub fn cancel_deadline<T: Send + 'static>(&mut self, id: DeadlineId) {
        self.buffer.push(move |world: &mut World| {
            world.cancel_deadline::<T>(id);
        });
    }

    /// Returns the number of queued commands.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if no commands are queued.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

// =============================================================================
// SystemParam implementation
// =============================================================================

// SAFETY: access() reports a unique marker type. fetch() only touches the
// command buffer field, which no other SystemParam accesses. The buffer is
// a separate field from resources and archetypes.
unsafe impl crate::system_param::SystemParam for Commands<'_> {
    type Item<'w> = Commands<'w>;

    fn access() -> Vec<Access> {
        // Use the CommandBuffer TypeId as a marker. This prevents two Commands
        // params in the same system (which would alias) while not conflicting
        // with any Res/ResMut/Query/QueryMut.
        vec![Access::ResWrite(TypeId::of::<CommandBuffer>())]
    }

    unsafe fn fetch<'w>(world: UnsafeWorldCell) -> Commands<'w> {
        Commands {
            buffer: unsafe { world.commands_mut() },
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::system_param::SystemParam;

    #[derive(Debug, Clone, PartialEq)]
    struct Pos {
        x: f32,
        y: f32,
    }
    impl Component for Pos {}

    #[derive(Debug, Clone, PartialEq)]
    struct Vel {
        x: f32,
        y: f32,
    }
    impl Component for Vel {}

    #[allow(dead_code)]
    #[derive(Debug, Clone, PartialEq)]
    struct Health(i32);
    impl Component for Health {}

    // -- CommandBuffer --

    #[test]
    fn command_buffer_starts_empty() {
        let buf = CommandBuffer::new();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn command_buffer_tracks_length() {
        let mut buf = CommandBuffer::new();
        buf.push(|_| {});
        buf.push(|_| {});
        assert_eq!(buf.len(), 2);
        assert!(!buf.is_empty());
    }

    #[test]
    fn command_buffer_take_drains_all() {
        let mut buf = CommandBuffer::new();
        buf.push(|world: &mut World| {
            world.spawn((Pos { x: 1.0, y: 2.0 },));
        });
        buf.push(|world: &mut World| {
            world.spawn((Pos { x: 3.0, y: 4.0 },));
        });

        let mut world = World::new();
        let commands = buf.take();
        assert!(buf.is_empty());
        for cmd in commands {
            cmd(&mut world);
        }
        assert_eq!(world.entity_count(), 2);
    }

    // -- Commands typed API --

    #[test]
    fn commands_spawn_deferred() {
        let mut world = World::new();
        assert_eq!(world.entity_count(), 0);

        // Queue a spawn via Commands.
        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.spawn((Pos { x: 1.0, y: 2.0 },));
        }

        // Not spawned yet.
        assert_eq!(world.entity_count(), 0);

        // Apply commands.
        world.apply_commands();
        assert_eq!(world.entity_count(), 1);

        let xs: Vec<f32> = world.query::<&Pos>().map(|(_, p)| p.x).collect();
        assert_eq!(xs, vec![1.0]);
    }

    #[test]
    fn commands_despawn_deferred() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.despawn(e);
        }

        // Still alive until apply.
        assert!(world.is_alive(e));

        world.apply_commands();
        assert!(!world.is_alive(e));
    }

    #[test]
    fn commands_insert_deferred() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.insert(e, Vel { x: 3.0, y: 4.0 });
        }

        // Vel not yet present.
        assert!(world.get::<Vel>(e).is_none());

        world.apply_commands();
        assert_eq!(world.get::<Vel>(e).unwrap().x, 3.0);
        // Pos preserved.
        assert_eq!(world.get::<Pos>(e).unwrap().x, 1.0);
    }

    #[test]
    fn commands_remove_deferred() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 }, Vel { x: 3.0, y: 4.0 }));

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.remove::<Vel>(e);
        }

        // Vel still present until apply.
        assert!(world.get::<Vel>(e).is_some());

        world.apply_commands();
        assert!(world.get::<Vel>(e).is_none());
        assert_eq!(world.get::<Pos>(e).unwrap().x, 1.0);
    }

    #[test]
    fn commands_multiple_ops_applied_in_order() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            // Insert then remove — net effect is no Vel.
            cmds.insert(e, Vel { x: 10.0, y: 20.0 });
            cmds.remove::<Vel>(e);
        }

        world.apply_commands();
        assert!(world.get::<Vel>(e).is_none());
    }

    #[test]
    fn commands_on_dead_entity_is_safe() {
        let mut world = World::new();
        let e = world.spawn((Pos { x: 1.0, y: 2.0 },));
        world.despawn(e);

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.insert(e, Vel { x: 1.0, y: 1.0 });
            cmds.despawn(e);
            cmds.remove::<Pos>(e);
        }

        // Should not panic.
        world.apply_commands();
    }

    #[test]
    fn commands_spawn_multi_component() {
        let mut world = World::new();

        {
            let buf = world.command_buffer_mut();
            let mut cmds = Commands { buffer: buf };
            cmds.spawn((Pos { x: 1.0, y: 2.0 }, Vel { x: 3.0, y: 4.0 }));
        }

        world.apply_commands();
        assert_eq!(world.entity_count(), 1);

        let results: Vec<_> = world.query::<(&Pos, &Vel)>().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.0.x, 1.0);
        assert_eq!(results[0].1.1.x, 3.0);
    }

    // -- SystemParam integration --

    #[test]
    fn commands_access_uses_command_buffer_marker() {
        let access = <Commands<'_> as SystemParam>::access();
        assert_eq!(access.len(), 1);
        assert_eq!(access[0], Access::ResWrite(TypeId::of::<CommandBuffer>()));
    }

    #[test]
    fn commands_does_not_conflict_with_res() {
        use crate::system_param::{Res, has_conflicts};
        let a = <Commands<'_> as SystemParam>::access();
        let b = <Res<'_, i32> as SystemParam>::access();
        assert!(!has_conflicts(&a, &b));
    }

    #[test]
    fn commands_does_not_conflict_with_query() {
        use crate::system_param::{Query, has_conflicts};
        let a = <Commands<'_> as SystemParam>::access();
        let b = <Query<'_, Pos> as SystemParam>::access();
        assert!(!has_conflicts(&a, &b));
    }

    #[test]
    fn commands_fetch_via_unsafe_world_cell() {
        let mut world = World::new();
        let cell = unsafe { UnsafeWorldCell::new(&mut world as *mut World) };
        unsafe {
            let mut cmds: Commands<'_> = <Commands<'_> as SystemParam>::fetch(cell);
            cmds.spawn((Pos { x: 42.0, y: 0.0 },));
        }
        world.apply_commands();
        assert_eq!(world.entity_count(), 1);
    }
}
