# Data Loading

Game data is defined in RON files — Rust Object Notation. RON maps 1:1 to
Rust types, so a data file is essentially a serialized struct.

## Unit Templates

A unit template defines what a unit type IS — its name, stats, and properties.

```ron
// data/units/sentinel.ron
UnitTemplate(
    name: "Sentinel",
    stats: UnitStats(
        hp: 120,
        speed: 45.0,
        combat_rating: 18,
        build_time: 15.0,
    ),
)
```

## Loading

Load all RON files from a directory:

```rust
use galeon_engine::DataRegistry;
use std::path::Path;

let registry = DataRegistry::load_units_from_dir(Path::new("data/units/"))?;
let sentinel = registry.unit("sentinel").unwrap();
println!("{}: {} HP", sentinel.name, sentinel.stats.hp);
```

Or load from a string (useful for tests):

```rust
let ron = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#;
let registry = DataRegistry::load_unit_from_str("scout", ron)?;
```

## Spawning from Templates

Templates stamp into ECS components:

```rust
let template = registry.unit("sentinel").unwrap();
world.spawn((
    Name(template.name.clone()),
    template.stats.clone(),
    Position { x: 10.0, y: 20.0 },
));
```

## Why RON?

- Maps directly to Rust structs — no manual parsing
- Supports comments, trailing commas, named fields
- Validation = deserialization — if it parses, it's valid
- Diffable in git — human-readable text files
