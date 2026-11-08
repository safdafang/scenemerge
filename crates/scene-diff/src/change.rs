//! Change types. The JSON form of these types is a stable, documented
//! interface (`docs/json-schema.md`); add fields only in a backward-compatible
//! way and bump [`crate::SCHEMA_VERSION`] for breaking changes.

use serde::Serialize;

/// The entity a property change applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// A node, by path (new path for nodes that exist in the new version).
    Node {
        /// Node path relative to the root (`.` is the root).
        path: String,
    },
    /// A `[sub_resource]`, by id in the new version.
    SubResource {
        /// Local id.
        id: String,
        /// Resource type.
        #[serde(rename = "type")]
        type_name: String,
    },
    /// The `[resource]` section of a `.tres` file.
    Resource,
}

/// One changed component of a compound value, e.g. `origin.y` of a `Transform3D`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentChange {
    /// Component name: `x`, `origin.y`, `basis.x.z`, `[3]`, `["key"]`, …
    pub name: String,
    /// Old value text (absent if the component was added).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// New value text (absent if the component was removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

/// A changed header field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldChange {
    /// Field name.
    pub field: String,
    /// Old value text (absent if added).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// New value text (absent if removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
}

/// A connection, identified by `(signal, from, to, method)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionKey {
    /// Signal name.
    pub signal: String,
    /// Source node path.
    pub from: String,
    /// Target node path.
    pub to: String,
    /// Method name.
    pub method: String,
}

/// A semantic change between two versions of a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum Change {
    /// A field of the `[gd_scene]`/`[gd_resource]` descriptor changed
    /// (`load_steps` is ignored).
    HeaderChanged(FieldChange),

    /// An external resource was added.
    ExtResourceAdded {
        /// `path=`.
        path: String,
        /// `type=`.
        #[serde(rename = "type")]
        type_name: String,
        /// `uid=`.
        #[serde(skip_serializing_if = "Option::is_none")]
        uid: Option<String>,
    },
    /// An external resource was removed.
    ExtResourceRemoved {
        /// `path=`.
        path: String,
        /// `type=`.
        #[serde(rename = "type")]
        type_name: String,
        /// `uid=`.
        #[serde(skip_serializing_if = "Option::is_none")]
        uid: Option<String>,
    },
    /// An external resource's `path`, `type` or `uid` changed.
    ExtResourceChanged {
        /// New `path=`.
        path: String,
        /// Changed fields.
        changes: Vec<FieldChange>,
    },

    /// A sub-resource was added.
    SubResourceAdded {
        /// Local id.
        id: String,
        /// Type.
        #[serde(rename = "type")]
        type_name: String,
    },
    /// A sub-resource was removed.
    SubResourceRemoved {
        /// Local id.
        id: String,
        /// Type.
        #[serde(rename = "type")]
        type_name: String,
    },
    /// A sub-resource's type changed (property changes are reported as
    /// `Property*` changes with a `sub_resource` target).
    SubResourceChanged {
        /// Local id (new version).
        id: String,
        /// Old type.
        old_type: String,
        /// New type.
        new_type: String,
    },

    /// A node was added.
    NodeAdded {
        /// Path of the new node.
        path: String,
        /// `type=`.
        #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
        type_name: Option<String>,
        /// Path of the instanced scene, if the node is an instance.
        #[serde(skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    /// A node was removed.
    NodeRemoved {
        /// Path of the removed node (old version).
        path: String,
        /// `type=`.
        #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
        type_name: Option<String>,
    },
    /// A node was renamed (same parent).
    NodeRenamed {
        /// Old path.
        path: String,
        /// New name.
        new_name: String,
        /// New path.
        new_path: String,
    },
    /// A node got a different parent (possibly also a new name).
    NodeMoved {
        /// Old path.
        path: String,
        /// New path.
        new_path: String,
    },
    /// A node changed position among its siblings.
    NodeReordered {
        /// Node path (new version).
        path: String,
        /// Old position among siblings (0-based).
        old_position: usize,
        /// New position among siblings (0-based).
        new_position: usize,
    },
    /// A node's `type=` changed.
    NodeTypeChanged {
        /// Node path (new version).
        path: String,
        /// Old type.
        #[serde(skip_serializing_if = "Option::is_none")]
        old: Option<String>,
        /// New type.
        #[serde(skip_serializing_if = "Option::is_none")]
        new: Option<String>,
    },
    /// A node's `instance=` changed.
    InstanceChanged {
        /// Node path (new version).
        path: String,
        /// Old instanced scene.
        #[serde(skip_serializing_if = "Option::is_none")]
        old: Option<String>,
        /// New instanced scene.
        #[serde(skip_serializing_if = "Option::is_none")]
        new: Option<String>,
    },
    /// A node's groups changed.
    GroupsChanged {
        /// Node path (new version).
        path: String,
        /// Groups added.
        added: Vec<String>,
        /// Groups removed.
        removed: Vec<String>,
    },
    /// Another node header attribute changed (`owner`, `index`,
    /// `instance_placeholder`, `node_paths`, …).
    NodeAttributeChanged {
        /// Node path (new version).
        path: String,
        /// The attribute.
        #[serde(flatten)]
        change: FieldChange,
    },

    /// A property was added.
    PropertyAdded {
        /// Where.
        target: Target,
        /// Property name.
        property: String,
        /// New value.
        new: String,
    },
    /// A property was removed (it is back to its default value).
    PropertyRemoved {
        /// Where.
        target: Target,
        /// Property name.
        property: String,
        /// Old value.
        old: String,
    },
    /// A property value changed.
    PropertyChanged {
        /// Where.
        target: Target,
        /// Property name.
        property: String,
        /// Old value.
        old: String,
        /// New value.
        new: String,
        /// Changed components of compound values, when they can be identified.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        components: Vec<ComponentChange>,
    },

    /// A signal connection was added.
    ConnectionAdded(ConnectionKey),
    /// A signal connection was removed.
    ConnectionRemoved(ConnectionKey),
    /// A connection's `flags`, `binds` or `unbinds` changed.
    ConnectionChanged {
        /// The connection (new version).
        #[serde(flatten)]
        connection: ConnectionKey,
        /// Changed attributes.
        changes: Vec<FieldChange>,
    },

    /// An `[editable path=...]` marker was added.
    EditableAdded {
        /// Instance path.
        path: String,
    },
    /// An `[editable path=...]` marker was removed.
    EditableRemoved {
        /// Instance path.
        path: String,
    },
}

impl Change {
    /// The JSON `kind` tag of this change.
    pub fn kind(&self) -> &'static str {
        match self {
            Change::HeaderChanged(_) => "HeaderChanged",
            Change::ExtResourceAdded { .. } => "ExtResourceAdded",
            Change::ExtResourceRemoved { .. } => "ExtResourceRemoved",
            Change::ExtResourceChanged { .. } => "ExtResourceChanged",
            Change::SubResourceAdded { .. } => "SubResourceAdded",
            Change::SubResourceRemoved { .. } => "SubResourceRemoved",
            Change::SubResourceChanged { .. } => "SubResourceChanged",
            Change::NodeAdded { .. } => "NodeAdded",
            Change::NodeRemoved { .. } => "NodeRemoved",
            Change::NodeRenamed { .. } => "NodeRenamed",
            Change::NodeMoved { .. } => "NodeMoved",
            Change::NodeReordered { .. } => "NodeReordered",
            Change::NodeTypeChanged { .. } => "NodeTypeChanged",
            Change::InstanceChanged { .. } => "InstanceChanged",
            Change::GroupsChanged { .. } => "GroupsChanged",
            Change::NodeAttributeChanged { .. } => "NodeAttributeChanged",
            Change::PropertyAdded { .. } => "PropertyAdded",
            Change::PropertyRemoved { .. } => "PropertyRemoved",
            Change::PropertyChanged { .. } => "PropertyChanged",
            Change::ConnectionAdded(_) => "ConnectionAdded",
            Change::ConnectionRemoved(_) => "ConnectionRemoved",
            Change::ConnectionChanged { .. } => "ConnectionChanged",
            Change::EditableAdded { .. } => "EditableAdded",
            Change::EditableRemoved { .. } => "EditableRemoved",
        }
    }
}
