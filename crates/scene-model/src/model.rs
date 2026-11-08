use std::collections::HashMap;

use godot_text::{Document, Header, ParseError, Value};

use crate::error::ModelError;

/// Whether the file is a scene or a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// `[gd_scene ...]` (`.tscn`)
    Scene,
    /// `[gd_resource ...]` (`.tres`)
    Resource,
}

/// The `[gd_scene]` / `[gd_resource]` descriptor.
#[derive(Debug, Clone)]
pub struct Descriptor {
    /// Section index in the document.
    pub section: usize,
    /// `format=` (3 or 4 for Godot 4).
    pub format: i64,
    /// `uid="uid://..."`.
    pub uid: Option<String>,
    /// `type=` of a resource file.
    pub resource_type: Option<String>,
    /// `script_class=` of a resource file.
    pub script_class: Option<String>,
    /// Deprecated `load_steps=` (written before Godot 4.6).
    pub load_steps: Option<i64>,
}

/// A property of a node, sub-resource or `[resource]` section.
#[derive(Debug, Clone)]
pub struct Prop {
    /// Property name.
    pub key: String,
    /// Parsed value.
    pub value: Value,
    /// Value text exactly as written.
    pub raw: String,
}

/// `[ext_resource ...]`
#[derive(Debug, Clone)]
pub struct ExtResource {
    /// Section index in the document.
    pub section: usize,
    /// Local id (`id="1_abcde"`).
    pub id: String,
    /// `type=`.
    pub type_name: String,
    /// `uid=`.
    pub uid: Option<String>,
    /// `path=`.
    pub path: String,
}

impl ExtResource {
    /// Human-readable key: the path, which is what users recognise.
    pub fn display_key(&self) -> &str {
        &self.path
    }
}

/// `[sub_resource ...]`
#[derive(Debug, Clone)]
pub struct SubResource {
    /// Section index in the document.
    pub section: usize,
    /// Local id (`id="BoxMesh_abcde"`).
    pub id: String,
    /// `type=`.
    pub type_name: String,
    /// Properties (duplicates resolved, last wins).
    pub props: Vec<Prop>,
}

/// Where a node's parent lives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParentRef {
    /// The scene root (no `parent=`).
    None,
    /// A node defined in this file (index into [`SceneFile::nodes`]).
    Node(usize),
    /// A node that is not defined in this file; it comes from an instanced or
    /// inherited scene. Holds the `parent=` path text.
    External(String),
}

/// `[node ...]`
#[derive(Debug, Clone)]
pub struct Node {
    /// Section index in the document.
    pub section: usize,
    /// `name=`.
    pub name: String,
    /// `type=` (absent for instances and for overrides of inherited nodes).
    pub type_name: Option<String>,
    /// `parent=` exactly as written (`None` for the root).
    pub parent_path: Option<String>,
    /// Resolved parent.
    pub parent: ParentRef,
    /// Path relative to the root as used in `parent=` attributes: `.` for the
    /// root, `Child`, `Child/Grandchild`.
    pub path: String,
    /// `unique_id=` (Godot 4.6+).
    pub unique_id: Option<i64>,
    /// `instance=` value (an `ExtResource(...)`).
    pub instance: Option<Value>,
    /// `instance_placeholder=` path.
    pub instance_placeholder: Option<String>,
    /// `owner=`.
    pub owner: Option<String>,
    /// `index=` (sibling position for nodes added to inherited children).
    pub index: Option<i64>,
    /// `groups=[...]`.
    pub groups: Vec<String>,
    /// Header fields not listed above (e.g. `parent_id_path`, `node_paths`).
    pub other_fields: Vec<(String, Value)>,
    /// Properties (duplicates resolved, last wins).
    pub props: Vec<Prop>,
}

/// `[connection ...]`
#[derive(Debug, Clone)]
pub struct Connection {
    /// Section index in the document.
    pub section: usize,
    /// `signal=`.
    pub signal: String,
    /// `from=` node path.
    pub from: String,
    /// `to=` node path.
    pub to: String,
    /// `method=`.
    pub method: String,
    /// `flags=` (defaults to `CONNECT_PERSIST` = 2).
    pub flags: i64,
    /// `unbinds=` (defaults to 0).
    pub unbinds: i64,
    /// `binds=` value, if any.
    pub binds: Option<Value>,
    /// Other header fields (e.g. `from_uid_path`, `to_uid_path`).
    pub other_fields: Vec<(String, Value)>,
}

impl Connection {
    /// The identity of a connection: `(signal, from, to, method)`.
    pub fn key(&self) -> (&str, &str, &str, &str) {
        (&self.signal, &self.from, &self.to, &self.method)
    }
}

/// Highest supported `format=` value. Godot 4 writes `format=3`, or `format=4`
/// when the file needs the newer syntax (`PackedVector4Array`, base64
/// `PackedByteArray`), see `ResourceLoaderText::FORMAT_VERSION`.
pub const MAX_FORMAT: i64 = 4;

/// Default value of `flags` in `[connection]` (`Object::CONNECT_PERSIST`).
pub const DEFAULT_CONNECTION_FLAGS: i64 = 2;

/// `[editable path="..."]`
#[derive(Debug, Clone)]
pub struct Editable {
    /// Section index in the document.
    pub section: usize,
    /// `path=`.
    pub path: String,
}

/// A structural problem found by [`SceneFile::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// 1-based line of the offending section.
    pub line: usize,
    /// Description.
    pub message: String,
}

/// A parsed `.tscn`/`.tres` file with its semantic structure.
#[derive(Debug, Clone)]
pub struct SceneFile {
    /// The lossless document.
    pub doc: Document,
    /// Scene or resource.
    pub kind: FileKind,
    /// File descriptor.
    pub descriptor: Descriptor,
    /// `[ext_resource]` sections in file order.
    pub ext_resources: Vec<ExtResource>,
    /// `[sub_resource]` sections in file order.
    pub sub_resources: Vec<SubResource>,
    /// `[node]` sections in file order.
    pub nodes: Vec<Node>,
    /// `[connection]` sections in file order.
    pub connections: Vec<Connection>,
    /// `[editable]` sections in file order.
    pub editables: Vec<Editable>,
    /// Section index of `[resource]` (resource files).
    pub resource_section: Option<usize>,
    /// Properties of `[resource]`.
    pub resource_props: Vec<Prop>,
    /// Node path → index into `nodes`.
    path_index: HashMap<String, usize>,
}

fn str_field(h: &Header, key: &str) -> Option<String> {
    h.get_str(key).map(str::to_string)
}

fn int_field(h: &Header, key: &str) -> Option<i64> {
    match h.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| Some(n.as_f64() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn props_of(section: &godot_text::Section) -> Vec<Prop> {
    let all: Vec<&godot_text::Property> = section.properties().collect();
    let has_duplicates = if all.len() <= 32 {
        all.iter()
            .enumerate()
            .any(|(i, p)| all.iter().skip(i + 1).any(|q| q.key == p.key))
    } else {
        let mut seen = std::collections::HashSet::with_capacity(all.len());
        !all.iter().all(|p| seen.insert(p.key.as_str()))
    };
    let mut out: Vec<Prop> = Vec::with_capacity(all.len());
    for (i, p) in all.iter().enumerate() {
        if has_duplicates && all.iter().skip(i + 1).any(|q| q.key == p.key) {
            continue;
        }
        out.push(Prop {
            key: p.key.clone(),
            value: p.value.clone(),
            raw: p.raw_value.clone(),
        });
    }
    out
}

/// Joins a `parent=` path and a node name into the node's path.
pub fn join_path(parent: &str, name: &str) -> String {
    if parent == "." || parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

const NODE_FIELDS: &[&str] = &[
    "name",
    "type",
    "parent",
    "unique_id",
    "instance",
    "instance_placeholder",
    "owner",
    "index",
    "groups",
];

const CONNECTION_FIELDS: &[&str] = &[
    "signal", "from", "to", "method", "flags", "unbinds", "binds",
];

impl SceneFile {
    /// Parses and models a file from bytes.
    pub fn parse_bytes(bytes: &[u8]) -> Result<SceneFile, ModelError> {
        let doc = Document::parse_bytes(bytes)?;
        SceneFile::from_document(doc)
    }

    /// Parses and models a file from text.
    pub fn parse(text: &str) -> Result<SceneFile, ModelError> {
        let doc = Document::parse(text)?;
        SceneFile::from_document(doc)
    }

    /// Builds the semantic model of a parsed document.
    pub fn from_document(doc: Document) -> Result<SceneFile, ModelError> {
        let first = doc.sections.first().ok_or_else(|| {
            ModelError::structure(
                1,
                "Empty file: missing [gd_scene] or [gd_resource] descriptor",
            )
        })?;
        let kind = match first.header.name.as_str() {
            "gd_scene" => FileKind::Scene,
            "gd_resource" => FileKind::Resource,
            other => {
                return Err(ModelError::structure(
                    first.header.line,
                    format!(
                    "Expected [gd_scene] or [gd_resource] as the first section, found [{other}]"
                ),
                ))
            }
        };
        let h = &first.header;
        let format = int_field(h, "format").unwrap_or(3);
        if format <= 2 {
            return Err(ModelError::UnsupportedFormat {
                format,
                message: "Godot 3 (format=2) files are not supported; open the project in Godot 4 to convert it".into(),
                path: None,
            });
        }
        if format > MAX_FORMAT {
            return Err(ModelError::UnsupportedFormat {
                format,
                message: "this file was saved by a newer Godot version than scenemerge understands"
                    .into(),
                path: None,
            });
        }
        let descriptor = Descriptor {
            section: 0,
            format,
            uid: str_field(h, "uid"),
            resource_type: str_field(h, "type"),
            script_class: str_field(h, "script_class"),
            load_steps: int_field(h, "load_steps"),
        };
        if kind == FileKind::Resource && descriptor.resource_type.is_none() {
            return Err(ModelError::structure(
                h.line,
                "[gd_resource] requires a 'type' field",
            ));
        }

        let mut file = SceneFile {
            kind,
            descriptor,
            ext_resources: Vec::new(),
            sub_resources: Vec::new(),
            nodes: Vec::new(),
            connections: Vec::new(),
            editables: Vec::new(),
            resource_section: None,
            resource_props: Vec::new(),
            path_index: HashMap::new(),
            doc: Document::default(),
        };
        let mut by_path: HashMap<String, usize> = HashMap::new();

        for (si, s) in doc.sections.iter().enumerate().skip(1) {
            let h = &s.header;
            let req = |key: &str| {
                str_field(h, key).ok_or_else(|| {
                    ModelError::structure(
                        h.line,
                        format!("[{}] is missing the '{key}' field", h.name),
                    )
                })
            };
            match h.name.as_str() {
                "ext_resource" => {
                    let id = match h.get("id") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Number(n)) => n.raw.clone(),
                        _ => {
                            return Err(ModelError::structure(
                                h.line,
                                "[ext_resource] is missing the 'id' field",
                            ))
                        }
                    };
                    file.ext_resources.push(ExtResource {
                        section: si,
                        id,
                        type_name: str_field(h, "type").unwrap_or_default(),
                        uid: str_field(h, "uid"),
                        path: req("path")?,
                    });
                }
                "sub_resource" => {
                    let id = match h.get("id") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Number(n)) => n.raw.clone(),
                        _ => {
                            return Err(ModelError::structure(
                                h.line,
                                "[sub_resource] is missing the 'id' field",
                            ))
                        }
                    };
                    file.sub_resources.push(SubResource {
                        section: si,
                        id,
                        type_name: req("type")?,
                        props: props_of(s),
                    });
                }
                "resource" => {
                    if file.resource_section.is_some() {
                        return Err(ModelError::structure(
                            h.line,
                            "More than one [resource] section",
                        ));
                    }
                    file.resource_section = Some(si);
                    file.resource_props = props_of(s);
                }
                "node" => {
                    let name = req("name")?;
                    let parent_path = str_field(h, "parent");
                    let (parent, path) = match &parent_path {
                        None => {
                            if !file.nodes.is_empty() {
                                return Err(ModelError::structure(
                                    h.line,
                                    format!("Node '{name}' has no 'parent' but the scene already has a root node"),
                                ));
                            }
                            (ParentRef::None, ".".to_string())
                        }
                        Some(p) => {
                            if file.nodes.is_empty() {
                                return Err(ModelError::structure(
                                    h.line,
                                    format!("The first node ('{name}') must be the root and have no 'parent'"),
                                ));
                            }
                            let p_norm = normalize_parent(p);
                            let parent = match by_path.get(&p_norm) {
                                Some(&i) => ParentRef::Node(i),
                                None => ParentRef::External(p_norm.clone()),
                            };
                            (parent, join_path(&p_norm, &name))
                        }
                    };
                    if by_path.contains_key(&path) {
                        return Err(ModelError::structure(
                            h.line,
                            format!("Duplicate node path '{path}'"),
                        ));
                    }
                    by_path.insert(path.clone(), file.nodes.len());
                    let groups = match h.get("groups") {
                        Some(Value::Array(items)) => items
                            .iter()
                            .filter_map(|v| match v {
                                Value::String(s) | Value::StringName(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    file.nodes.push(Node {
                        section: si,
                        name,
                        type_name: str_field(h, "type"),
                        parent_path,
                        parent,
                        path,
                        unique_id: int_field(h, "unique_id"),
                        instance: h.get("instance").cloned(),
                        instance_placeholder: str_field(h, "instance_placeholder"),
                        owner: str_field(h, "owner"),
                        index: int_field(h, "index"),
                        groups,
                        other_fields: h
                            .fields
                            .iter()
                            .filter(|f| !NODE_FIELDS.contains(&f.key.as_str()))
                            .map(|f| (f.key.clone(), f.value.clone()))
                            .collect(),
                        props: props_of(s),
                    });
                }
                "connection" => {
                    file.connections.push(Connection {
                        section: si,
                        signal: req("signal")?,
                        from: req("from")?,
                        to: req("to")?,
                        method: req("method")?,
                        flags: int_field(h, "flags").unwrap_or(DEFAULT_CONNECTION_FLAGS),
                        unbinds: int_field(h, "unbinds").unwrap_or(0),
                        binds: h.get("binds").cloned(),
                        other_fields: h
                            .fields
                            .iter()
                            .filter(|f| !CONNECTION_FIELDS.contains(&f.key.as_str()))
                            .map(|f| (f.key.clone(), f.value.clone()))
                            .collect(),
                    });
                }
                "editable" => {
                    file.editables.push(Editable {
                        section: si,
                        path: req("path")?,
                    });
                }
                "gd_scene" | "gd_resource" => {
                    return Err(ModelError::structure(
                        h.line,
                        format!("Unexpected second [{}] descriptor", h.name),
                    ));
                }
                other => {
                    return Err(ModelError::structure(
                        h.line,
                        format!("Unknown section [{other}]"),
                    ));
                }
            }
        }
        if kind == FileKind::Resource && !file.nodes.is_empty() {
            return Err(ModelError::structure(
                file.nodes.first().map_or(1, |n| {
                    doc.sections.get(n.section).map_or(1, |s| s.header.line)
                }),
                "[node] sections are only allowed in scenes ([gd_scene])",
            ));
        }
        file.doc = doc;
        file.path_index = by_path;
        Ok(file)
    }

    /// Index of the ext-resource with local id `id`.
    pub fn ext_by_id(&self, id: &str) -> Option<usize> {
        self.ext_resources.iter().position(|e| e.id == id)
    }

    /// Index of the sub-resource with local id `id`.
    pub fn sub_by_id(&self, id: &str) -> Option<usize> {
        self.sub_resources.iter().position(|s| s.id == id)
    }

    /// Index of the node with path `path` (`.` is the root).
    pub fn node_by_path(&self, path: &str) -> Option<usize> {
        self.path_index.get(path).copied()
    }

    /// The line where section `section` starts.
    pub fn line_of(&self, section: usize) -> usize {
        self.doc.sections.get(section).map_or(0, |s| s.header.line)
    }

    /// Path for display: the root's name for the root, otherwise the path
    /// relative to the root (as in `parent=`).
    pub fn display_path(&self, node: usize) -> String {
        match self.nodes.get(node) {
            Some(n) if n.path == "." => n.name.clone(),
            Some(n) => n.path.clone(),
            None => String::new(),
        }
    }

    /// Indices of the nodes whose parent is `node` (in file order).
    pub fn children_of(&self, node: usize) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent == ParentRef::Node(node))
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether a node path (relative to the root) can exist at runtime: it
    /// names a node of this file, or lies inside an instanced scene (the
    /// scene is inherited, or a prefix of the path is an instance).
    pub fn path_exists(&self, path: &str) -> bool {
        if self.node_by_path(path).is_some()
            || self.nodes.first().is_some_and(|n| n.instance.is_some())
        {
            return true;
        }
        let mut prefix = path;
        loop {
            if self
                .node_by_path(prefix)
                .and_then(|i| self.nodes.get(i))
                .is_some_and(|m| m.instance.is_some())
            {
                return true;
            }
            match prefix.rfind('/') {
                Some(k) => prefix = prefix.get(..k).unwrap_or(""),
                None => return false,
            }
        }
    }

    /// Checks references and ordering rules that Godot enforces when loading.
    pub fn validate(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let line = |s: usize| self.line_of(s);
        let mut phase = 0;
        for (si, s) in self.doc.sections.iter().enumerate().skip(1) {
            let p = match s.header.name.as_str() {
                "ext_resource" => 1,
                "sub_resource" => 2,
                _ => 3,
            };
            if p < phase {
                issues.push(Issue {
                    line: line(si),
                    message: format!(
                        "[{}] appears after a later kind of section; Godot expects ext_resource → sub_resource → nodes/resource",
                        s.header.name
                    ),
                });
            }
            phase = phase.max(p);
        }
        let mut seen = HashMap::new();
        for e in &self.ext_resources {
            if seen.insert(e.id.as_str(), ()).is_some() {
                issues.push(Issue {
                    line: line(e.section),
                    message: format!("Duplicate ext_resource id \"{}\"", e.id),
                });
            }
        }
        let mut seen = HashMap::new();
        for s in &self.sub_resources {
            if seen.insert(s.id.as_str(), ()).is_some() {
                issues.push(Issue {
                    line: line(s.section),
                    message: format!("Duplicate sub_resource id \"{}\"", s.id),
                });
            }
        }
        let sub_pos: HashMap<&str, usize> = self
            .sub_resources
            .iter()
            .map(|s| (s.id.as_str(), s.section))
            .collect();
        let check_value = |v: &Value, section: usize, issues: &mut Vec<Issue>| {
            v.for_each_ref(&mut |kind, id| match kind {
                godot_text::RefKind::Ext => {
                    if self.ext_by_id(id).is_none() {
                        issues.push(Issue {
                            line: line(section),
                            message: format!("ExtResource(\"{id}\") is not defined"),
                        });
                    }
                }
                godot_text::RefKind::Sub => match sub_pos.get(id) {
                    None => issues.push(Issue {
                        line: line(section),
                        message: format!("SubResource(\"{id}\") is not defined"),
                    }),
                    Some(&def) if def >= section => issues.push(Issue {
                        line: line(section),
                        message: format!("SubResource(\"{id}\") is used before it is defined"),
                    }),
                    Some(_) => {}
                },
            });
        };
        for s in &self.sub_resources {
            for p in &s.props {
                check_value(&p.value, s.section, &mut issues);
            }
        }
        for n in &self.nodes {
            for p in &n.props {
                check_value(&p.value, n.section, &mut issues);
            }
            if let Some(i) = &n.instance {
                check_value(i, n.section, &mut issues);
            }
        }
        if let Some(rs) = self.resource_section {
            for p in &self.resource_props {
                check_value(&p.value, rs, &mut issues);
            }
        }
        if self.kind == FileKind::Scene && self.nodes.is_empty() {
            issues.push(Issue {
                line: line(0),
                message: "The scene has no root node".into(),
            });
        }
        for n in &self.nodes {
            if let ParentRef::External(p) = &n.parent {
                if !self.path_exists(p) {
                    issues.push(Issue {
                        line: line(n.section),
                        message: format!("Parent node '{p}' of '{}' does not exist", n.name),
                    });
                }
            }
        }
        for c in &self.connections {
            for (end, path) in [("from", &c.from), ("to", &c.to)] {
                if !self.path_exists(path) {
                    issues.push(Issue {
                        line: line(c.section),
                        message: format!(
                            "Connection '{}' {end}=\"{path}\" does not name a node in this scene",
                            c.signal
                        ),
                    });
                }
            }
        }
        issues
    }
}

fn normalize_parent(p: &str) -> String {
    let mut s = p;
    while let Some(rest) = s.strip_prefix("./") {
        s = rest;
    }
    if s.is_empty() {
        ".".to_string()
    } else {
        s.to_string()
    }
}

impl From<ParseError> for ModelError {
    fn from(e: ParseError) -> Self {
        ModelError::Parse(e)
    }
}
