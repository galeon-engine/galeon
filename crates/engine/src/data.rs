// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use crate::component::Component;
use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

/// Unit stats — the numeric properties of a unit type.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct UnitStats {
    pub hp: i32,
    pub speed: f32,
    #[serde(default)]
    pub combat_rating: i32,
    #[serde(default)]
    pub build_time: f32,
}

impl Component for UnitStats {}

/// A unit template loaded from a RON file.
///
/// Templates are blueprints — they define what a unit IS. At spawn time,
/// the template stamps its data into ECS components.
#[derive(Debug, Clone, Deserialize)]
pub struct UnitTemplate {
    pub name: String,
    pub stats: UnitStats,
}

/// Registry of game data loaded from RON files.
///
/// Currently supports unit templates. Will expand to buildings, tech trees,
/// weapons, etc.
#[derive(Debug)]
pub struct DataRegistry {
    units: HashMap<String, UnitTemplate>,
}

impl DataRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            units: HashMap::new(),
        }
    }

    /// Load all unit templates from RON files in the given directory.
    ///
    /// Each `.ron` file in the directory is parsed as a `UnitTemplate`.
    /// The filename (without extension) becomes the lookup key.
    pub fn load_units_from_dir(dir: &Path) -> Result<Self, Box<DataLoadError>> {
        let mut units = HashMap::new();

        let entries = std::fs::read_dir(dir).map_err(|e| {
            Box::new(DataLoadError::Io {
                path: dir.to_path_buf(),
                source: e,
            })
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                Box::new(DataLoadError::Io {
                    path: dir.to_path_buf(),
                    source: e,
                })
            })?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "ron") {
                let key = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let contents = std::fs::read_to_string(&path).map_err(|e| {
                    Box::new(DataLoadError::Io {
                        path: path.clone(),
                        source: e,
                    })
                })?;

                let template: UnitTemplate = ron::from_str(&contents).map_err(|e| {
                    Box::new(DataLoadError::Ron {
                        path: path.clone(),
                        source: e,
                    })
                })?;

                units.insert(key, template);
            }
        }

        Ok(Self { units })
    }

    /// Load a single unit template from a RON string.
    pub fn load_unit_from_str(key: &str, ron_str: &str) -> Result<Self, Box<DataLoadError>> {
        let template: UnitTemplate = ron::from_str(ron_str).map_err(|e| {
            Box::new(DataLoadError::RonParse {
                key: key.to_string(),
                source: e,
            })
        })?;
        let mut units = HashMap::new();
        units.insert(key.to_string(), template);
        Ok(Self { units })
    }

    /// Get a unit template by name.
    pub fn unit(&self, name: &str) -> Option<&UnitTemplate> {
        self.units.get(name)
    }

    /// Returns the number of loaded unit templates.
    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    /// Reload a single unit template from a RON string, replacing the existing entry.
    ///
    /// On success, replaces the template at `key` in the registry.
    /// On error, returns the parse error WITHOUT modifying the registry.
    pub fn reload_unit(&mut self, key: &str, ron_str: &str) -> Result<(), Box<DataLoadError>> {
        let template: UnitTemplate = ron::from_str(ron_str).map_err(|e| {
            Box::new(DataLoadError::RonParse {
                key: key.to_string(),
                source: e,
            })
        })?;
        self.units.insert(key.to_string(), template);
        Ok(())
    }

    /// Merge another registry into this one.
    pub fn merge(&mut self, other: DataRegistry) {
        self.units.extend(other.units);
    }
}

impl Default for DataRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during data loading.
#[derive(Debug)]
pub enum DataLoadError {
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Ron {
        path: std::path::PathBuf,
        source: ron::error::SpannedError,
    },
    RonParse {
        key: String,
        source: ron::error::SpannedError,
    },
}

impl std::fmt::Display for DataLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataLoadError::Io { path, source } => {
                write!(f, "IO error reading {}: {}", path.display(), source)
            }
            DataLoadError::Ron { path, source } => {
                write!(f, "RON parse error in {}: {}", path.display(), source)
            }
            DataLoadError::RonParse { key, source } => {
                write!(f, "RON parse error for '{}': {}", key, source)
            }
        }
    }
}

impl std::error::Error for DataLoadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn deserialize_unit_template_from_ron() {
        let ron_str = r#"
            UnitTemplate(
                name: "Sentinel",
                stats: UnitStats(
                    hp: 120,
                    speed: 45.0,
                    combat_rating: 18,
                    build_time: 15.0,
                ),
            )
        "#;

        let template: UnitTemplate = ron::from_str(ron_str).unwrap();
        assert_eq!(template.name, "Sentinel");
        assert_eq!(template.stats.hp, 120);
        assert!((template.stats.speed - 45.0).abs() < f32::EPSILON);
        assert_eq!(template.stats.combat_rating, 18);
    }

    #[test]
    fn load_unit_from_str() {
        let ron_str = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#;
        let registry = DataRegistry::load_unit_from_str("scout", ron_str).unwrap();
        let unit = registry.unit("scout").unwrap();
        assert_eq!(unit.name, "Scout");
        assert_eq!(unit.stats.hp, 50);
    }

    #[test]
    fn load_units_from_directory() {
        let dir = tempfile::tempdir().unwrap();

        let sentinel_path = dir.path().join("sentinel.ron");
        let mut f = std::fs::File::create(&sentinel_path).unwrap();
        writeln!(
            f,
            r#"UnitTemplate(name: "Sentinel", stats: UnitStats(hp: 120, speed: 45.0))"#
        )
        .unwrap();

        let scout_path = dir.path().join("scout.ron");
        let mut f = std::fs::File::create(&scout_path).unwrap();
        writeln!(
            f,
            r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#
        )
        .unwrap();

        let txt_path = dir.path().join("notes.txt");
        std::fs::write(&txt_path, "not a ron file").unwrap();

        let registry = DataRegistry::load_units_from_dir(dir.path()).unwrap();
        assert_eq!(registry.unit_count(), 2);
        assert_eq!(registry.unit("sentinel").unwrap().stats.hp, 120);
        assert_eq!(registry.unit("scout").unwrap().stats.hp, 50);
    }

    #[test]
    fn missing_optional_fields_default() {
        let ron_str = r#"UnitTemplate(name: "Minimal", stats: UnitStats(hp: 10, speed: 1.0))"#;
        let template: UnitTemplate = ron::from_str(ron_str).unwrap();
        assert_eq!(template.stats.combat_rating, 0);
        assert!((template.stats.build_time - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reload_unit_updates_existing() {
        let ron_v1 = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#;
        let mut registry = DataRegistry::load_unit_from_str("scout", ron_v1).unwrap();
        assert_eq!(registry.unit("scout").unwrap().stats.hp, 50);

        let ron_v2 = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 75, speed: 90.0))"#;
        registry.reload_unit("scout", ron_v2).unwrap();
        assert_eq!(registry.unit("scout").unwrap().stats.hp, 75);
        assert!((registry.unit("scout").unwrap().stats.speed - 90.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reload_unit_preserves_on_error() {
        let ron_v1 = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: 50, speed: 80.0))"#;
        let mut registry = DataRegistry::load_unit_from_str("scout", ron_v1).unwrap();

        let bad_ron = r#"UnitTemplate(name: "Scout", stats: UnitStats(hp: "not a number"))"#;
        let result = registry.reload_unit("scout", bad_ron);
        assert!(result.is_err());
        // Old data preserved
        assert_eq!(registry.unit("scout").unwrap().stats.hp, 50);
    }

    #[test]
    fn reload_unit_adds_new_entry() {
        let mut registry = DataRegistry::new();
        assert_eq!(registry.unit_count(), 0);

        let ron_str = r#"UnitTemplate(name: "Tank", stats: UnitStats(hp: 200, speed: 20.0))"#;
        registry.reload_unit("tank", ron_str).unwrap();
        assert_eq!(registry.unit_count(), 1);
        assert_eq!(registry.unit("tank").unwrap().stats.hp, 200);
    }

    #[test]
    fn bad_ron_gives_descriptive_error() {
        let bad_ron = r#"UnitTemplate(name: "Bad", stats: UnitStats(hp: "not a number"))"#;
        let result = DataRegistry::load_unit_from_str("bad", bad_ron);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("bad"));
    }
}
