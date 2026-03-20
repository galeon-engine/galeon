// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

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
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
