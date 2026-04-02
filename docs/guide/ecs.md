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
for (entity, pos) in world.query::<&Position>() {
    println!("Entity at ({}, {})", pos.x, pos.y);
}
```

Modify components:

```rust
for (entity, mut pos) in world.query_mut::<&mut Position>() {
    pos.x += 1.0;
}
```

Mutable queries yield `Mut<T>` smart pointers. Reading via `Deref` does not
flag the component as changed; only writing via `DerefMut` does. This means
`query_changed` only reports entities that were actually mutated.

Query two components at once:

```rust
for (entity, (pos, vel)) in world.query::<(&Position, &Velocity)>() {
    println!("Moving entity at ({}, {}) with velocity ({}, {})", pos.x, pos.y, vel.x, vel.y);
}
```

If you prefer the old fixed-arity helpers, `query2`, `query2_mut`, `query3`, and
`query3_mut` are available as thin wrappers over the typed query-spec API.

> Queries return lazy iterators — call `.collect::<Vec<_>>()` if you need `len()` or indexing.

Query with filters:

```rust
use galeon_engine::{With, Without};

for (entity, pos) in world.query_filtered::<&Position, (With<Velocity>, Without<Health>)>() {
    println!("Only units that can move and are not health-tracked");
}
```

Query three components (immutable):

```rust
for (entity, (pos, vel, hp)) in world.query::<(&Position, &Velocity, &Health)>() {
    // Process entities with all three components
}
```

Query three components (mutable):

```rust
for (entity, (mut pos, mut vel, mut hp)) in world.query_mut::<(&mut Position, &mut Velocity, &mut Health)>() {
    pos.x += vel.x;
    hp.current -= 1;
}
```

Direct typed access by entity:

```rust
if let Some((health, vel)) = world.one::<(&Health, &Velocity)>(unit) {
    println!("HP: {}/{}", health.current, health.max);
}

if let Some(mut health) = world.one_mut::<&mut Health>(unit) {
    health.current -= 10;
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

### Parameterized Systems (recommended)

Systems declare their data access in their function signature using parameter
types: `Res<T>`, `ResMut<T>`, `Query<T>`, `QueryMut<T>`. The engine
extracts each parameter automatically at runtime.

```rust
use galeon_engine::{Res, ResMut, Query, QueryMut};

fn movement_system(dt: Res<'_, DeltaTime>, mut positions: QueryMut<'_, Position>) {
    for (_, pos) in positions.iter_mut() {
        pos.x += dt.0 as f32;
    }
}

fn apply_gravity(gravity: Res<'_, Gravity>, mut velocities: QueryMut<'_, Velocity>) {
    for (_, vel) in velocities.iter_mut() {
        vel.y -= gravity.0;
    }
}
```

Conflict detection: if two parameters in the same system would alias (e.g.,
`Res<T>` + `ResMut<T>`), the engine panics at registration time.

### Deferred Mutations with Commands

Ordinary systems should prefer deferred structural changes (spawn, despawn,
insert, remove) via the `Commands` system parameter. These are buffered and
applied between schedule stages, avoiding mid-iteration archetype changes.

```rust
use galeon_engine::Commands;

fn spawn_reinforcements(mut cmds: Commands<'_>) {
    cmds.spawn((Position { x: 0.0, y: 0.0 }, Health { current: 100, max: 100 },));
}

fn cleanup_dead(
    units: Query<'_, Health>,
    mut cmds: Commands<'_>,
) {
    for (entity, hp) in units.iter() {
        if hp.current <= 0 {
            cmds.despawn(entity);
        }
    }
}
```

Available operations:

| Method | Effect |
|--------|--------|
| `cmds.spawn(bundle)` | Deferred entity spawn |
| `cmds.despawn(entity)` | Deferred entity despawn |
| `cmds.insert(entity, component)` | Deferred component insert (archetype migration) |
| `cmds.remove::<C>(entity)` | Deferred component removal (archetype migration) |

Commands are applied automatically between schedule stages. You can also call
`world.apply_commands()` manually in setup code.

## Schedule

Systems are grouped into stages. Stages run in the order they're registered.
Within a stage, systems run in registration order.

```rust
let mut schedule = Schedule::new();

// Turbofish required for type inference:
schedule.add_system::<(QueryMut<'_, Position>,)>("simulate", "movement", movement_fn);
schedule.add_system::<(Res<'_, DeltaTime>,)>("sync", "three_sync", sync_system);

// Run one tick
schedule.run(&mut world);
```

The three-stage model (`input` → `simulate` → `sync`) ensures input is
processed before simulation, and simulation completes before rendering sync.

Queued commands are applied automatically between stages, so deferred
structural mutations from one stage are visible to the next.

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

## Change Detection

Every component column stores a per-row `changed_tick`. When you write through
a `Mut<T>` (via `DerefMut`), the tick is stamped at the current world tick.
Read-only access via `Deref` does not stamp.

```rust
// Only entities whose Position changed after `since_tick`:
for (entity, pos) in world.query_changed::<Position>(since_tick) {
    // pos was actually mutated
}
```

### Interior mutability

Components that use interior mutability (`AtomicUsize`, `Mutex<T>`) can be
mutated through `Deref` without triggering `DerefMut`. Call `set_changed()`
explicitly to ensure change detection sees the modification:

```rust
for (_, mut counter) in world.query_mut::<&mut AtomicCounter>() {
    counter.0.fetch_add(1, Ordering::Relaxed); // interior mutation
    counter.set_changed(); // manual stamp
}
```

## Modifying Component Sets

Beyond spawning and despawning, you can add or remove individual components
from an existing entity. These operations migrate the entity to a new archetype.

```rust
// Add a component to an existing entity
world.insert(entity, Velocity { x: 1.0, y: 0.0 });

// Remove a component from an entity (returns the value)
let vel = world.remove::<Velocity>(entity);
```

Both operations are O(1) after the first transition thanks to the archetype
edge cache (see Storage Internals below).

## Storage Internals

### Archetype Storage

`World` stores all entity state in **archetype tables**. An archetype is a
group of entities that share exactly the same set of component types. Within an
archetype, each component type occupies a contiguous `Column<T>` (a typed
`Vec<T>`), so iterating any component type is a linear scan with no pointer
chasing.

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

- **`EntityMetaStore`** — tracks every live entity with an `EntityLocation`
  (archetype ID + row index), enabling O(1) lookup from entity to component
  data.
- **`ArchetypeLayout`** — a sorted set of `TypeId`s that uniquely identifies
  which component types an archetype holds. Two layouts with the same types
  (in any order) are equal and hash the same.
- **`Column<T>`** — a typed `Vec<T>` storing one component type for all
  entities in an archetype. Columns are independently borrowable, eliminating
  any need for unsafe double-borrows when querying multiple components.
- **`Archetype`** — owns the entity list and all columns. Upholds the
  invariant that `entities.len() == column.len()` for every column.
- **`ArchetypeStore`** — the registry of all archetypes, keyed by layout.
  `get_or_create(layout)` returns the existing archetype or allocates a new
  one. Provides `get_two_mut` for safe simultaneous mutable access to two
  archetypes via `split_at_mut`.
- **Edge cache** — each archetype caches the target archetype for adding or
  removing a specific component type, making `insert<C>` and `remove<C>`
  migrations O(1) after the first transition.

How the public API maps to internals:

| Operation | Mechanism |
|-----------|-----------|
| `spawn(bundle)` | Computes layout from bundle `type_ids()`, calls `get_or_create`, appends row |
| `despawn(entity)` | Looks up `EntityLocation`, swap-removes the row — O(1) |
| `get` / `get_mut` | `EntityLocation` → archetype → column → row index — O(1) |
| `insert<C>(entity, val)` | Migrates entity to `current_layout + C` archetype |
| `remove<C>(entity)` | Migrates entity to `current_layout − C` archetype |
| `query` / `query_mut` | Iterates only archetypes whose layout matches the typed query spec |
| `query_filtered` | Adds `With<T>` / `Without<T>` archetype filtering with no per-entity checks |
| `one` / `one_mut` | Uses `EntityLocation` for typed single-entity fetch |

Benefits of this design:

- **No unsafe double-borrow** — columns are separate `Vec<T>`s, not entries in
  a shared `HashMap`
- **O(1) entity location** — `EntityMetaStore` tracks exactly where each entity lives
- **Cache-friendly iteration** — entities with the same components are
  co-located in contiguous memory
- **Structural grouping** — queries only visit archetypes that match, skipping
  irrelevant entities entirely
- **O(1) despawn** — swap-remove leaves no gaps and requires no compaction

### Bundle Trait

The `Bundle` trait drives archetype-aware spawning. It requires three methods:

- `type_ids()` — returns the sorted `TypeId` slice used to compute the archetype layout
- `register_columns(archetype)` — ensures the archetype has a column for each type
- `push_into_columns(archetype)` — moves component values into the matching columns

Tuple bundles up to size 8 are provided by the engine. Custom bundle types can
implement the trait directly.

### Hot vs Cold Storage

| Storage Class | Use For | Why |
|---------------|---------|-----|
| **Hot** (archetype columns, default) | Transforms, movement, health, combat state, AI state | Iterated every tick, must be cache-friendly |
| **Cold** (future: separate store) | Names, debug tags, editor metadata | Rarely iterated, should not pollute hot storage |

Currently all components use archetype column storage. Cold/sparse-side storage
for editor metadata is a planned future addition.
