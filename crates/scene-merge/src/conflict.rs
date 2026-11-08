//! Conflicts and the JSON conflict report.

use serde::Serialize;

/// How conflicts are written to the merged file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStyle {
    /// Git conflict markers around the smallest conflicting unit (one
    /// property line, one header, one connection, or one node block).
    #[default]
    Markers,
    /// Take our side for every conflict.
    Ours,
    /// Take their side for every conflict.
    Theirs,
}

/// What a conflict is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The same property was changed differently on both sides.
    Property,
    /// A node header attribute (`type`, `instance`, `groups`, …) was changed
    /// differently on both sides.
    NodeAttribute,
    /// The node was renamed differently on both sides, or renamed to a name
    /// that is already taken.
    Rename,
    /// The node was moved to different parents.
    Move,
    /// One side deleted a node, the other changed it (or added children to it).
    DeleteModify,
    /// Both sides added a node at the same path with different types or instances.
    AddAdd,
    /// The same connection was changed differently, or deleted on one side and
    /// changed on the other.
    Connection,
    /// An external resource's path/type/uid was changed differently.
    ExtResource,
    /// A file descriptor field was changed differently.
    Header,
}

/// One conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    /// Kind of conflict.
    pub kind: ConflictKind,
    /// What is in conflict: `node:Player/Camera3D`, `sub_resource:Box_1`,
    /// `resource`, `connection:pressed:Button->.::_on_pressed`, `ext_resource:res://a.png`,
    /// `header`.
    pub target: String,
    /// Property or attribute name, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub property: Option<String>,
    /// Base value (absent = did not exist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Our value (absent = deleted / not present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ours: Option<String>,
    /// Their value (absent = deleted / not present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theirs: Option<String>,
    /// How it was resolved in the output: `markers`, `ours` or `theirs`.
    pub resolution: ConflictStyle,
    /// 1-based line in the merged output where the conflict (or its markers) starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

/// Something the merge changed on its own to keep the result valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolution {
    /// `ext_resource_id`, `sub_resource_id` or `unique_id`.
    pub kind: String,
    /// The entity concerned.
    pub target: String,
    /// The value taken from their side.
    pub from: String,
    /// The value written instead.
    pub to: String,
}

/// The JSON conflict report (`--report`), see `docs/json-schema.md`.
#[derive(Debug, Serialize)]
pub struct Report<'a> {
    /// [`crate::REPORT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The file being merged (`--path`), if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<&'a str>,
    /// Whether the merge was clean.
    pub clean: bool,
    /// Conflicts.
    pub conflicts: &'a [Conflict],
    /// Automatic adjustments (renumbered ids, …).
    pub resolutions: &'a [Resolution],
}
