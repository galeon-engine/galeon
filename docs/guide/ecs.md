# ECS — Entity Component System

Galeon uses an ECS at its core. All game state lives here — entities are
lightweight IDs, components are plain data structs, and systems are functions
that operate on components.

## Entities

An entity is just an ID — a `u32` index paired with a `u32` generation. The
generation prevents bugs where you hold a stale reference to a despawned entity.

```rust
let entity = world.spawn((Position { x: 0.0, y: 0.0 },));
assert!(world.is_alive(entity));

world.despawn(entity);
assert!(!world.is_alive(entity));
```

## Components

A component is any Rust struct that derives `Component`. Components are pure
data — no behavior.

```rust
use galeon_engine::Component;

#[derive(Component)]
struct Position { x: f32, y: f32 }

#[derive(Component)]
struct Health { current: i32, max: i32 }

#[derive(Component)]
struct Velocity { x: f32, y: f32 }
```

## Spawning

Spawn entities with a tuple of components:

```rust
// Single component
let e = world.spawn((Position { x: 0.0, y: 0.0 },));

// Multiple components (up to 8)
let unit = world.spawn((
    Position { x: 10.0, y: 20.0 },
    Health { current: 100, max: 100 },
    Velocity { x: 0.0, y: 0.0 },
));
```

## Querying

Read components:

```rust
for (entity, pos) in world.query::<Position>() {
    println!("Entity at ({}, {})", pos.x, pos.y);
}
```

Modify components:

```rust
for (entity, pos) in world.query_mut::<Position>() {
    pos.x += 1.0;
}
```

Query two components at once:

```rust
for (entity, pos, vel) in world.query2::<Position, Velocity>() {
    println!("Moving entity at ({}, {}) with velocity ({}, {})", pos.x, pos.y, vel.x, vel.y);
}
```

> Queries return lazy iterators — call `.collect::<Vec<_>>()` if you need `len()` or indexing.

Query three components (immutable):

```rust
for (entity, pos, vel, hp) in world.query3::<Position, Velocity, Health>() {
    // Process entities with all three components
}
```

Query three components (mutable):

```rust
for (entity, pos, vel, hp) in world.query3_mut::<Position, Velocity, Health>() {
    pos.x += vel.x;
    hp.current -= 1;
}
```

Direct access by entity:

```rust
if let Some(health) = world.get::<Health>(unit) {
    println!("HP: {}/{}", health.current, health.max);
}
```

## Resources

Resources are world-global singletons — data that isn't tied to a specific
entity. Delta time, tick count, configuration.

```rust
struct DeltaTime(f64);

world.insert_resource(DeltaTime(0.016));

let dt = world.resource::<DeltaTime>().0;
```

## Systems

A system is a plain function that takes `&mut World`:

```rust
fn movement_system(world: &mut World) {
    let dt = world.resource::<DeltaTime>().0;
    for (_, pos, vel) in world.query2_mut::<Position, Velocity>() {
        pos.x += vel.x * dt;
        pos.y += vel.y * dt;
    }
}
```

## Schedule

Systems are grouped into stages. Stages run in the order they're registered.
Within a stage, systems run in registration order.

```rust
let mut schedule = Schedule::new();
schedule.add_system("input",    "read_input",  input_system);
schedule.add_system("simulate", "movement",    movement_system);
schedule.add_system("simulate", "combat",      combat_system);
schedule.add_system("sync",     "three_sync",  sync_system);

// Run one tick
schedule.run(&mut world);
```

The three-stage model (`input` → `simulate` → `sync`) ensures input is
processed before simulation, and simulation completes before rendering sync.

## Component Trait

All component types must implement the `Component` marker trait. The trait
requires `Send + Sync + 'static`, ensuring components are safe to share across
threads.

```rust
use galeon_engine::Component;

#[derive(Component, Clone, Debug)]
struct Position { x: f32, y: f32 }
```

The `#[derive(Component)]` macro generates the trait impl automatically.

## Storage Internals

### Archetype Storage (new)

Entities are grouped into **archetypes** — tables where each row is an entity
and each column is a component type. All entities in an archetype share the
same set of component types.

```
Archetype [Position, Velocity]     Archetype [Position, Health]
┌────────┬──────────┬──────────┐   ┌────────┬──────────┬────────┐
│ Entity │ Position │ Velocity │   │ Entity │ Position │ Health │
├────────┼──────────┼──────────┤   ├────────┼──────────┼────────┤
│  e0    │ (1, 2)   │ (3, 4)   │   │  e2    │ (5, 6)   │  100   │
│  e1    │ (7, 8)   │ (9, 0)   │   │  e3    │ (0, 0)   │   80   │
└────────┴──────────┴──────────┘   └────────┴──────────┴────────┘
```

Key data structures:

- **`ArchetypeLayout`** — sorted set of `TypeId`s identifying which components
  an archetype holds. Two layouts with the same types (regardless of input
  order) are equal and hash the same.
- **`Column<T>`** — a typed `Vec<T>` storing one component type within one
  archetype. Columns are independently borrowable (no double-borrow needed for
  multi-component queries).
- **`Archetype`** — owns the entity list and columns. Maintains the invariant
  that `entities.len() == column.len()` for all columns at all times.
- **`ArchetypeStore`** — registry of all archetypes, indexed by layout.
  `get_or_create(layout)` returns the existing archetype or creates a new one.
- **`EntityMetaStore`** — extends entity metadata with `EntityLocation`
  (archetype ID + row), enabling O(1) entity-to-archetype lookup.
- **Edge cache** — each archetype caches the target archetype for adding or
  removing a specific component type, making archetype migrations O(1) after
  the first transition.

Benefits over the previous sparse set design:

- **No unsafe double-borrow** — columns are separate `Vec<T>`s, not entries in
  a shared `HashMap`
- **O(1) entity location** — `EntityMeta` tracks exactly where each entity lives
- **Cache-friendly iteration** — entities with the same components are
  co-located in contiguous memory
- **Structural grouping** — queries only visit archetypes that match, skipping
  irrelevant entities entirely

### Sparse Sets (legacy)

The previous storage model used per-type sparse sets. These remain in the
codebase during the transition and will be removed when World migrates to
archetype storage.

### Hot vs Cold Storage

| Storage Class | Use For | Why |
|---------------|---------|-----|
| **Hot** (archetype columns, default) | Transforms, movement, health, combat state, AI state | Iterated every tick, must be cache-friendly |
| **Cold** (future: separate store) | Names, debug tags, editor metadata | Rarely iterated, should not pollute hot storage |

Currently all components use archetype column storage. Cold/sparse-side storage
for editor metadata is a planned future addition.
