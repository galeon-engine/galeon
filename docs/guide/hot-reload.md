# Hot-Reload for RON Data Files

Hot-reload lets you edit `.ron` data files while the engine is running and see
changes reflected immediately — no restart needed.

## Feature Flag

Hot-reload depends on OS-native file watching (`notify` crate) and is gated
behind the `hot-reload` feature flag. It is excluded from WASM builds.

```bash
cargo run --features hot-reload
```

## Setup

Add the `HotReloadPlugin` to your engine and ensure a `DataRegistry` resource
is present:

```rust
use galeon_engine::{Engine, DataRegistry, HotReloadPlugin};
use std::path::Path;

let data_dir = Path::new("data/units");
let mut engine = Engine::new();

// Load initial data.
let registry = DataRegistry::load_units_from_dir(data_dir).unwrap();
engine.insert_resource(registry);

// Enable hot-reload — watches data_dir for .ron changes.
engine.add_plugin(HotReloadPlugin::new(data_dir));
```

## How It Works

```
notify::Watcher (OS thread, 100ms debounce)
    │ std::sync::mpsc channel
    ▼
AssetEvents resource (drained each tick in pre_update)
    │
    ▼
Reload system
    ├── DataRegistry::reload_unit(key, new_ron)
    └── Patch entities with matching TemplateRef
```

1. **File watcher** monitors the data directory for `.ron` file changes using
   OS-native events (inotify on Linux, kqueue on macOS, ReadDirectoryChanges
   on Windows). A 100ms debounce window collapses rapid writes.

2. **Drain system** (`pre_update` stage) moves pending events from the watcher
   channel into the `AssetEvents` resource.

3. **Reload system** (`pre_update` stage) re-deserializes changed files into
   `DataRegistry`. On parse error, the old data is preserved and the error is
   logged to stderr.

4. **Entity patching** queries all entities with a `TemplateRef` matching a
   reloaded key and replaces their `UnitStats` component with the updated
   template stats.

## TemplateRef Component

Tag entities with `TemplateRef` to opt into automatic stat patching:

```rust
use galeon_engine::{TemplateRef, UnitStats};

// When spawning from a template:
let stats = registry.unit("sentinel").unwrap().stats.clone();
let entity = world.spawn((
    TemplateRef::new("sentinel"),
    stats,
));
```

When `sentinel.ron` is edited, the entity's `UnitStats` are automatically
replaced with the new values.

## Error Handling

- **Bad RON edits** never crash the engine — the parse error is logged and the
  previous data is preserved.
- **File read errors** (permission denied, file locked) are logged and skipped.
- **Removed files** are ignored — the last known data stays in the registry.

## Limitations

- **Desktop only.** WASM builds cannot watch the filesystem. The editor can
  send explicit reload commands as a complement.
- **UnitStats only.** Currently patches `UnitStats` components. Other component
  types would need additional patching logic.
- **Full replacement.** Entity-local stat overrides are not preserved — the
  entire `UnitStats` is replaced from the template.
- **Single directory.** The watcher monitors one directory (the one passed to
  `HotReloadPlugin::new`).
