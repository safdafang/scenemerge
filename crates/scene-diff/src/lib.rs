//! Semantic diff of Godot 4 scenes and resources.
//!
//! [`diff`] compares two [`scene_model::SceneFile`]s entity by entity (see
//! [`scene_model::Matching`] for how entities are paired) and returns a list of
//! [`Change`]s. The result can be rendered for humans ([`render_human`]),
//! serialised to JSON ([`to_json`], schema in `docs/json-schema.md`), and a file
//! can be written in a canonical text form for Git `textconv`
//! ([`canonical_text`]).
//!
//! ```
//! use scene_diff::{diff, Change, DiffOptions};
//! use scene_model::SceneFile;
//!
//! let a = SceneFile::parse("[gd_scene format=3]\n\n[node name=\"Cam\" type=\"Camera3D\"]\nfov = 75.0\n")?;
//! let b = SceneFile::parse("[gd_scene format=3]\n\n[node name=\"Cam\" type=\"Camera3D\"]\nfov = 90.0\n")?;
//! let d = diff(&a, &b, &DiffOptions::default());
//! assert!(matches!(&d.changes[..], [Change::PropertyChanged { property, old, new, .. }]
//!     if property == "fov" && old == "75.0" && new == "90.0"));
//! # Ok::<(), scene_model::ModelError>(())
//! ```

mod canonical;
mod change;
mod components;
mod diff;
mod render;

pub use canonical::canonical_text;
pub use change::{Change, ComponentChange, ConnectionKey, FieldChange, Target};
pub use diff::{diff, display_value, DiffOptions, FileDiff};
pub use render::{render_human, HumanOptions};

/// Version of the JSON output schema (`docs/json-schema.md`).
pub const SCHEMA_VERSION: u32 = 1;

/// JSON document for one compared file pair.
#[derive(Debug, serde::Serialize)]
pub struct JsonFileDiff<'a> {
    /// Path of the old version, as given on the command line or by Git.
    pub old_path: &'a str,
    /// Path of the new version.
    pub new_path: &'a str,
    /// Changes.
    pub changes: &'a [Change],
}

/// The top-level JSON document.
#[derive(Debug, serde::Serialize)]
pub struct JsonReport<'a> {
    /// Always [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// One entry per compared file pair.
    pub files: Vec<JsonFileDiff<'a>>,
}

/// Serialises a single file diff as a pretty-printed JSON report.
pub fn to_json(old_path: &str, new_path: &str, diff: &FileDiff) -> String {
    let report = JsonReport {
        schema_version: SCHEMA_VERSION,
        files: vec![JsonFileDiff {
            old_path,
            new_path,
            changes: &diff.changes,
        }],
    };
    serde_json::to_string_pretty(&report).unwrap_or_default()
}
