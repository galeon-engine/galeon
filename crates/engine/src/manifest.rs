// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Protocol manifest — a machine-readable schema describing all protocol items.
//!
//! The manifest is the intermediate representation between Rust protocol
//! definitions and generated artifacts (TypeScript types, HTTP schemas, etc.).
//! It decouples code generation from Rust compilation: generators read the
//! manifest, not the Rust AST.
//!
//! # Usage
//!
//! Protocol attribute macros (`#[galeon_engine::command]`, etc.) automatically
//! register items via [`inventory`]. Call [`ProtocolManifest::collect`] to
//! gather all registered items into a manifest.
//!
//! ```ignore
//! let manifest = ProtocolManifest::collect("my-protocol@0.1");
//! let json = manifest.to_json_pretty().unwrap();
//! println!("{json}");
//! ```

use crate::protocol::ProtocolKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A field descriptor used in compile-time registration.
///
/// Uses `&'static str` because values are baked into the binary by macros.
/// Converted to [`ManifestField`] for serialization.
pub struct FieldEntry {
    /// Field name.
    pub name: &'static str,
    /// Type path as a string (e.g., `"u64"`, `"Vec<UnitView>"`).
    pub ty: &'static str,
}

/// A registered protocol item with its metadata and field information.
///
/// Instances are created by attribute macros and collected via [`inventory`].
pub struct ProtocolRegistration {
    /// Stable protocol name (from `ProtocolMeta::name()`).
    pub name: &'static str,
    /// Protocol kind discriminant.
    pub kind: ProtocolKind,
    /// Fields of the struct (empty for unit structs).
    pub fields: &'static [FieldEntry],
    /// Doc comment (first line), if any.
    pub doc: &'static str,
    /// Explicit surface memberships. Empty means "default surface only".
    pub surfaces: &'static [&'static str],
}

inventory::collect!(ProtocolRegistration);

/// A field within a manifest entry (serializable).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestField {
    /// Field name.
    pub name: String,
    /// Type path as a string.
    pub ty: String,
}

/// A single entry in the manifest for a protocol item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Stable protocol name.
    pub name: String,
    /// Protocol kind.
    pub kind: ProtocolKind,
    /// Struct fields (name + type string).
    pub fields: Vec<ManifestField>,
    /// Doc comment (first line).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub doc: String,
    /// Explicit surface memberships. Empty means the manifest's default surface.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
}

/// The complete protocol manifest for a project.
///
/// Human-readable, git-diffable JSON output. Carries versioning metadata
/// from day one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolManifest {
    /// Schema version of the manifest format itself.
    pub manifest_version: String,
    /// The project's protocol version (e.g., `"my-game@0.1"`).
    pub protocol_version: String,
    /// The implicit surface for unannotated protocol items.
    #[serde(
        default = "ProtocolManifest::default_surface_name_owned",
        skip_serializing_if = "ProtocolManifest::is_default_surface_name"
    )]
    pub default_surface: String,
    /// All resolved surface names referenced by the manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
    /// Command entries.
    pub commands: Vec<ManifestEntry>,
    /// Query entries.
    pub queries: Vec<ManifestEntry>,
    /// Event entries.
    pub events: Vec<ManifestEntry>,
    /// DTO entries.
    pub dtos: Vec<ManifestEntry>,
}

impl ProtocolManifest {
    /// Current manifest schema version.
    pub const MANIFEST_VERSION: &'static str = "2";
    /// The default surface name for single-surface projects.
    pub const DEFAULT_SURFACE: &'static str = "default";

    fn default_surface_name_owned() -> String {
        Self::DEFAULT_SURFACE.to_string()
    }

    fn is_default_surface_name(name: &String) -> bool {
        name == Self::DEFAULT_SURFACE
    }

    /// Collect all protocol items registered via attribute macros.
    ///
    /// `protocol_version` is the project's protocol version string
    /// (e.g., `"my-game@0.1"`). If empty, defaults to the
    /// calling crate's version.
    pub fn collect(protocol_version: &str) -> Self {
        Self::collect_with_default_surface(protocol_version, Self::DEFAULT_SURFACE)
    }

    /// Collect all protocol items, resolving unannotated items into `default_surface`.
    pub fn collect_with_default_surface(protocol_version: &str, default_surface: &str) -> Self {
        let mut commands = Vec::new();
        let mut queries = Vec::new();
        let mut events = Vec::new();
        let mut dtos = Vec::new();
        let mut surface_names = BTreeSet::new();

        for reg in inventory::iter::<ProtocolRegistration> {
            let mut surfaces: Vec<String> = reg
                .surfaces
                .iter()
                .map(|surface| surface.to_string())
                .collect();
            surfaces.sort();
            surfaces.dedup();
            let resolved_surfaces = if surfaces.is_empty() {
                vec![default_surface.to_string()]
            } else {
                surfaces.clone()
            };
            surface_names.extend(resolved_surfaces);
            let entry = ManifestEntry {
                name: reg.name.to_string(),
                kind: reg.kind,
                fields: reg
                    .fields
                    .iter()
                    .map(|f| ManifestField {
                        name: f.name.to_string(),
                        ty: f.ty.to_string(),
                    })
                    .collect(),
                doc: reg.doc.to_string(),
                surfaces,
            };
            match reg.kind {
                ProtocolKind::Command => commands.push(entry),
                ProtocolKind::Query => queries.push(entry),
                ProtocolKind::Event => events.push(entry),
                ProtocolKind::Dto => dtos.push(entry),
            }
        }

        // Sort for deterministic, diffable output.
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        queries.sort_by(|a, b| a.name.cmp(&b.name));
        events.sort_by(|a, b| a.name.cmp(&b.name));
        dtos.sort_by(|a, b| a.name.cmp(&b.name));

        ProtocolManifest {
            manifest_version: Self::MANIFEST_VERSION.to_string(),
            protocol_version: protocol_version.to_string(),
            default_surface: default_surface.to_string(),
            surfaces: surface_names.into_iter().collect(),
            commands,
            queries,
            events,
            dtos,
        }
    }

    /// Check whether an entry belongs to a surface, resolving the default surface when needed.
    pub fn entry_belongs_to_surface(
        entry: &ManifestEntry,
        surface: &str,
        default_surface: &str,
    ) -> bool {
        if entry.surfaces.is_empty() {
            surface == default_surface
        } else {
            entry.surfaces.iter().any(|candidate| candidate == surface)
        }
    }

    /// Return the manifest's effective surface names.
    ///
    /// Schema v2 manifests persist `surfaces`, but older manifests may be
    /// deserialized without that field. In that case, derive the surface set
    /// from entry memberships so descriptor/codegen consumers degrade safely.
    pub fn resolved_surface_names(&self) -> Vec<String> {
        if !self.surfaces.is_empty() {
            return self.surfaces.clone();
        }

        let mut surface_names = BTreeSet::new();
        for entry in self
            .commands
            .iter()
            .chain(self.queries.iter())
            .chain(self.events.iter())
            .chain(self.dtos.iter())
        {
            if entry.surfaces.is_empty() {
                surface_names.insert(self.default_surface.clone());
            } else {
                surface_names.extend(entry.surfaces.iter().cloned());
            }
        }

        // If no surfaces were found, ensure the default surface is included
        if surface_names.is_empty() {
            surface_names.insert(self.default_surface.clone());
        }

        surface_names.into_iter().collect()
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize to RON format.
    pub fn to_ron_pretty(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }
}
