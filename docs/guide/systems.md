# Systems - Parameter Extraction

Systems in Galeon are plain Rust functions. Instead of manually extracting
data from the world, systems declare what they need in their signature:

## Before (manual extraction)

```rust
fn movement_system(world: &mut World) {
    let dt = world.resource::<DeltaTime>().0;
    for (_, pos, vel) in world.query2_mut::<Position, Velocity>() {
        pos.x += vel.x * dt;
    }
}
```

## After (parameter extraction)

```rust
fn movement_system(
    mut positions: QueryMut<'_, Position>,
    velocities: Query<'_, Velocity>,
    dt: Res<'_, DeltaTime>,
) {
    for (_, pos) in positions.iter_mut() {
        pos.x += dt.0 as f32;
    }
}
```

## Available Parameters

| Type | Access | Panics If |
|------|--------|-----------|
| `Res<T>` | Immutable resource `T` | Resource not inserted |
| `ResMut<T>` | Mutable resource `T` | Resource not inserted |
| `Query<T>` | Immutable query over component `T` | Never (returns empty) |
| `QueryMut<T>` | Mutable query over component `T` | Never (returns empty) |

Systems support up to 8 parameters.

## Registration

Parameterized systems currently require an explicit parameter tuple at registration:

```rust
engine.add_system::<(QueryMut<'_, Position>, Query<'_, Velocity>, Res<'_, DeltaTime>)>(
    "simulate",
    "movement",
    movement_system,
);
```

Legacy `fn(&mut World)` systems still work, but registration must also be explicit:

```rust
engine.add_system::<()>("update", "legacy", legacy_system as fn(&mut World));
```

## Conflict Detection

The engine checks at registration time that no system has conflicting
parameters. For example, `Res<T>` and `ResMut<T>` for the same `T` in the
same system will panic:

```rust
fn bad_system(a: Res<'_, Time>, b: ResMut<'_, Time>) { todo!() }
engine.add_system::<(Res<'_, Time>, ResMut<'_, Time>)>("update", "bad", bad_system); // PANIC
```

Different types never conflict: `Res<Time>` + `ResMut<Config>` is fine.
Resource access and component access are separate namespaces, so
`Res<Time>` + `QueryMut<Time>` is fine (if `Time` is both a resource and
a component, which is unusual but valid).

## Limitations

- Queries are single-component. For multi-component queries (like `query2_mut`), use `fn(&mut World)` style or combine two separate `Query`/`QueryMut` params.
- No `Changed<T>` or `Added<T>` filter parameters yet - use `world.query_changed::<T>(since)` directly.
