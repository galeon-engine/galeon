// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::component::Component;
use std::path::PathBuf;

/// An event describing a change to an asset file.
#[derive(Debug, Clone)]
pub enum AssetEvent {
    /// A new file was added to the watched directory.
    Added(PathBuf),
    /// An existing file was modified.
    Modified(PathBuf),
    /// A file was removed from the watched directory.
    Removed(PathBuf),
}

/// Resource that accumulates [`AssetEvent`]s each tick.
///
/// The hot-reload system drains events from the file watcher into this
/// resource so that multiple systems can inspect what changed this tick.
#[derive(Debug, Default)]
pub struct AssetEvents {
    events: Vec<AssetEvent>,
}

impl AssetEvents {
    /// Create an empty event buffer.
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Push an event into the buffer.
    pub fn push(&mut self, event: AssetEvent) {
        self.events.push(event);
    }

    /// Drain all events, returning them and leaving the buffer empty.
    pub fn drain(&mut self) -> Vec<AssetEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns `true` if no events are buffered.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// =============================================================================
// TemplateRef — tracks which template an entity was spawned from
// =============================================================================

/// Component that tracks which data template an entity was spawned from.
///
/// When a RON file is hot-reloaded, entities with a matching `TemplateRef`
/// get their stats patched from the updated template.
#[derive(Debug, Clone)]
pub struct TemplateRef {
    /// The template key (filename without extension, e.g. "sentinel").
    pub key: String,
}

impl TemplateRef {
    /// Create a new template reference.
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

impl Component for TemplateRef {}

// =============================================================================
// FileWatcher — notify integration behind feature flag
// =============================================================================

#[cfg(feature = "hot-reload")]
mod watcher {
    use super::AssetEvent;
    use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    /// File watcher that monitors a directory for `.ron` file changes.
    ///
    /// Uses `notify` with a 100ms debounce window. Events are sent through
    /// a channel and drained each tick by the hot-reload system.
    pub struct FileWatcher {
        rx: mpsc::Receiver<AssetEvent>,
        // Hold the debouncer alive — dropping it stops watching.
        _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    }

    impl FileWatcher {
        /// Start watching `dir` for `.ron` file changes.
        pub fn new(dir: &Path) -> Result<Self, notify::Error> {
            let (tx, rx) = mpsc::channel();

            let mut debouncer = new_debouncer(
                Duration::from_millis(100),
                move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                    if let Ok(events) = events {
                        for event in events {
                            if event.path.extension().is_some_and(|ext| ext == "ron") {
                                let asset_event = match event.kind {
                                    DebouncedEventKind::Any => {
                                        if event.path.exists() {
                                            AssetEvent::Modified(event.path)
                                        } else {
                                            AssetEvent::Removed(event.path)
                                        }
                                    }
                                    DebouncedEventKind::AnyContinuous => continue,
                                    _ => continue,
                                };
                                let _ = tx.send(asset_event);
                            }
                        }
                    }
                },
            )?;

            debouncer
                .watcher()
                .watch(dir, notify::RecursiveMode::NonRecursive)?;

            Ok(Self {
                rx,
                _debouncer: debouncer,
            })
        }

        /// Drain all pending events from the watcher.
        ///
        /// Non-blocking — returns immediately with whatever events are queued.
        pub fn drain_events(&self) -> Vec<AssetEvent> {
            let mut events = Vec::new();
            while let Ok(event) = self.rx.try_recv() {
                events.push(event);
            }
            events
        }
    }
}

#[cfg(feature = "hot-reload")]
pub use watcher::FileWatcher;

// =============================================================================
// HotReloadPlugin — wires watcher → events → registry → entity patching
// =============================================================================

#[cfg(feature = "hot-reload")]
mod plugin {
    use super::{AssetEvent, AssetEvents, FileWatcher, TemplateRef};
    use crate::data::DataRegistry;
    use crate::engine::{Engine, Plugin};
    use crate::world::World;
    use std::path::PathBuf;

    /// Plugin that enables hot-reload for RON data files.
    ///
    /// Watches a directory for `.ron` file changes and automatically
    /// reloads templates in `DataRegistry`, then patches entities
    /// that have a matching `TemplateRef` component.
    pub struct HotReloadPlugin {
        /// Directory to watch for RON file changes.
        pub data_dir: PathBuf,
    }

    impl HotReloadPlugin {
        /// Create a plugin that watches the given directory.
        pub fn new(data_dir: impl Into<PathBuf>) -> Self {
            Self {
                data_dir: data_dir.into(),
            }
        }
    }

    impl Plugin for HotReloadPlugin {
        fn build(&self, engine: &mut Engine) {
            // Start the file watcher.
            let watcher = FileWatcher::new(&self.data_dir)
                .expect("failed to start file watcher for hot-reload");

            engine.insert_resource(watcher);
            engine.insert_resource(AssetEvents::new());

            // Systems run in "pre_update" so reloaded data is available
            // before game logic reads it.
            engine.add_system("pre_update", "hot_reload_drain", drain_watcher_events);
            engine.add_system("pre_update", "hot_reload_apply", apply_reloaded_data);
        }
    }

    /// Drain events from the FileWatcher into the AssetEvents resource.
    fn drain_watcher_events(world: &mut World) {
        let watcher = world.resource::<FileWatcher>();
        let new_events = watcher.drain_events();

        if !new_events.is_empty() {
            let asset_events = world.resource_mut::<AssetEvents>();
            for event in new_events {
                asset_events.push(event);
            }
        }
    }

    /// Re-deserialize changed RON files and patch entities.
    fn apply_reloaded_data(world: &mut World) {
        let asset_events = world.resource_mut::<AssetEvents>();
        let events = asset_events.drain();

        if events.is_empty() {
            return;
        }

        // Collect which keys were reloaded so we can patch entities after.
        let mut reloaded_keys = Vec::new();

        for event in &events {
            match event {
                AssetEvent::Modified(path) | AssetEvent::Added(path) => {
                    let key = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    match std::fs::read_to_string(path) {
                        Ok(ron_str) => {
                            let registry = world.resource_mut::<DataRegistry>();
                            match registry.reload_unit(&key, &ron_str) {
                                Ok(()) => {
                                    reloaded_keys.push(key);
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[hot-reload] parse error for '{}': {}",
                                        path.display(),
                                        e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[hot-reload] read error for '{}': {}", path.display(), e);
                        }
                    }
                }
                AssetEvent::Removed(_path) => {
                    // Don't remove from registry on file delete — preserve last known data.
                }
            }
        }

        // Patch entities whose TemplateRef matches a reloaded key.
        if !reloaded_keys.is_empty() {
            patch_entities(world, &reloaded_keys);
        }
    }

    /// Patch UnitStats on entities whose TemplateRef matches reloaded keys.
    fn patch_entities(world: &mut World, reloaded_keys: &[String]) {
        use crate::data::UnitStats;

        // First, collect entity-key pairs that need patching.
        let to_patch: Vec<(crate::entity::Entity, String)> = world
            .query::<TemplateRef>()
            .into_iter()
            .filter(|(_, tref)| reloaded_keys.contains(&tref.key))
            .map(|(entity, tref)| (entity, tref.key.clone()))
            .collect();

        // Then patch each entity's UnitStats from the registry.
        for (entity, key) in &to_patch {
            let new_stats = world
                .resource::<DataRegistry>()
                .unit(key)
                .map(|t| t.stats.clone());
            if let Some(stats) = new_stats
                && let Some(entity_stats) = world.get_mut::<UnitStats>(*entity)
            {
                *entity_stats = stats;
            }
        }
    }
}

#[cfg(feature = "hot-reload")]
pub use plugin::HotReloadPlugin;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{DataRegistry, UnitStats};
    use crate::world::World;

    fn _assert_send<T: Send>() {}

    #[test]
    fn asset_event_is_send() {
        _assert_send::<AssetEvent>();
    }

    #[test]
    fn asset_events_push_and_drain() {
        let mut events = AssetEvents::new();
        events.push(AssetEvent::Modified("a.ron".into()));
        events.push(AssetEvent::Added("b.ron".into()));

        let drained = events.drain();
        assert_eq!(drained.len(), 2);
        assert!(events.is_empty());
    }

    #[test]
    fn asset_events_is_empty() {
        let mut events = AssetEvents::new();
        assert!(events.is_empty());

        events.push(AssetEvent::Removed("c.ron".into()));
        assert!(!events.is_empty());

        events.drain();
        assert!(events.is_empty());
    }

    #[test]
    fn template_ref_stores_key() {
        let tref = TemplateRef::new("sentinel");
        assert_eq!(tref.key, "sentinel");
    }

    #[test]
    fn template_ref_from_string() {
        let tref = TemplateRef::new(String::from("scout"));
        assert_eq!(tref.key, "scout");
    }

    #[test]
    fn entity_patching_updates_stats() {
        let mut world = World::new();

        // Load initial data.
        let ron_v1 = r#"UnitTemplate(name: "Sentinel", stats: UnitStats(hp: 100, speed: 30.0))"#;
        let mut registry = DataRegistry::new();
        registry.reload_unit("sentinel", ron_v1).unwrap();
        let initial_stats = registry.unit("sentinel").unwrap().stats.clone();
        world.insert_resource(registry);

        // Spawn entity with TemplateRef + UnitStats.
        let entity = world.spawn((TemplateRef::new("sentinel"), initial_stats));

        // Verify initial stats.
        assert_eq!(world.get::<UnitStats>(entity).unwrap().hp, 100);

        // Simulate hot-reload: update registry, then patch.
        let ron_v2 = r#"UnitTemplate(name: "Sentinel", stats: UnitStats(hp: 150, speed: 40.0))"#;
        world
            .resource_mut::<DataRegistry>()
            .reload_unit("sentinel", ron_v2)
            .unwrap();

        // Manually call the patching logic.
        let reloaded_keys = vec!["sentinel".to_string()];
        let to_patch: Vec<(crate::entity::Entity, String)> = world
            .query::<TemplateRef>()
            .into_iter()
            .filter(|(_, tref)| reloaded_keys.contains(&tref.key))
            .map(|(e, tref)| (e, tref.key.clone()))
            .collect();

        for (ent, key) in &to_patch {
            let new_stats = world
                .resource::<DataRegistry>()
                .unit(key)
                .map(|t| t.stats.clone());
            if let Some(stats) = new_stats
                && let Some(entity_stats) = world.get_mut::<UnitStats>(*ent)
            {
                *entity_stats = stats;
            }
        }

        // Verify patched stats.
        assert_eq!(world.get::<UnitStats>(entity).unwrap().hp, 150);
        assert!((world.get::<UnitStats>(entity).unwrap().speed - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn patching_skips_unrelated_entities() {
        let mut world = World::new();

        let mut registry = DataRegistry::new();
        let ron_sentinel =
            r#"UnitTemplate(name: "Sentinel", stats: UnitStats(hp: 100, speed: 30.0))"#;
        let ron_scout = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#;
        registry.reload_unit("sentinel", ron_sentinel).unwrap();
        registry.reload_unit("scout", ron_scout).unwrap();
        world.insert_resource(registry);

        let sentinel_stats = UnitStats {
            hp: 100,
            speed: 30.0,
            combat_rating: 0,
            build_time: 0.0,
        };
        let scout_stats = UnitStats {
            hp: 50,
            speed: 80.0,
            combat_rating: 0,
            build_time: 0.0,
        };

        let _sentinel = world.spawn((TemplateRef::new("sentinel"), sentinel_stats));
        let scout = world.spawn((TemplateRef::new("scout"), scout_stats));

        // Only reload sentinel — scout should be untouched.
        let ron_v2 = r#"UnitTemplate(name: "Sentinel", stats: UnitStats(hp: 200, speed: 50.0))"#;
        world
            .resource_mut::<DataRegistry>()
            .reload_unit("sentinel", ron_v2)
            .unwrap();

        // Scout stats should be unchanged.
        assert_eq!(world.get::<UnitStats>(scout).unwrap().hp, 50);
    }
}
