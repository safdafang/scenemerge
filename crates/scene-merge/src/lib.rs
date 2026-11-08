//! Three-way semantic merge of Godot 4 scenes and resources.
//!
//! [`merge`] combines BASE, OURS and THEIRS on the level of nodes, properties,
//! resources and connections (see `DECISIONS.md` for the exact rules) and
//! writes the result starting from OURS, so that untouched parts are copied
//! byte for byte.
//!
//! ```
//! use scene_merge::{merge, MergeOptions};
//! use scene_model::SceneFile;
//!
//! let base = SceneFile::parse("[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nx = 1\ny = 1\n")?;
//! let ours = SceneFile::parse("[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nx = 2\ny = 1\n")?;
//! let theirs = SceneFile::parse("[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nx = 1\ny = 3\n")?;
//! let out = merge(&base, &ours, &theirs, &MergeOptions::default()).unwrap();
//! assert!(out.is_clean());
//! assert_eq!(out.text, "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nx = 2\ny = 3\n");
//! # Ok::<(), scene_model::ModelError>(())
//! ```

mod conflict;
mod edit;
mod ids;
mod merge;

pub use conflict::{Conflict, ConflictKind, ConflictStyle, Report, Resolution};
pub use edit::MarkerStyle;
pub use merge::{MergeOptions, MergeOutput};

use scene_model::SceneFile;

/// Version of the JSON conflict report schema (`docs/json-schema.md`).
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Errors that prevent a merge.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MergeError {
    /// The three files are not all scenes or all resources.
    #[error("cannot merge a scene with a resource file")]
    KindMismatch,
    /// The merge produced an invalid file (a bug; please report it).
    #[error("internal error: {0}")]
    Internal(String),
}

/// Merges THEIRS' changes (relative to BASE) into OURS.
///
/// A result without conflict markers is re-parsed and validated before it is
/// returned; if that fails, [`MergeError::Internal`] is returned instead of a
/// broken file.
pub fn merge(
    base: &SceneFile,
    ours: &SceneFile,
    theirs: &SceneFile,
    opts: &MergeOptions,
) -> Result<MergeOutput, MergeError> {
    let out = merge::run(base, ours, theirs, opts)?;
    let has_markers = opts.conflict == ConflictStyle::Markers && !out.conflicts.is_empty();
    if !has_markers {
        let parsed = SceneFile::parse(&out.text)
            .map_err(|e| MergeError::Internal(format!("merged output does not parse: {e}")))?;
        let issues = parsed.validate();
        // Problems that were already present in OURS are not the merge's fault.
        let ours_issues = ours.validate();
        let new: Vec<_> = issues
            .iter()
            .filter(|i| !ours_issues.iter().any(|o| o.message == i.message))
            .collect();
        if let Some(first) = new.first() {
            return Err(MergeError::Internal(format!(
                "merged output is invalid at line {}: {}",
                first.line, first.message
            )));
        }
    }
    Ok(out)
}
