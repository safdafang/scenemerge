use std::collections::{HashMap, HashSet};

use godot_text::{RefKind, Value};
use scene_model::{Connection, MatchOptions, Matching, Node, ParentRef, Prop, SceneFile};

use crate::change::{Change, ComponentChange, ConnectionKey, FieldChange, Target};
use crate::components::components;

/// Options for [`diff`].
#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    /// Matching options (rename threshold, float epsilon).
    pub matching: MatchOptions,
    /// Ignore `metadata/_edit_*` properties (editor-only state such as locks
    /// and groups in the 2D editor).
    pub ignore_editor_metadata: bool,
}

/// The semantic diff of two versions of a file.
#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    /// Changes in a stable order: descriptor, ext resources, sub-resources,
    /// nodes (tree order), `[resource]`, connections, editables.
    pub changes: Vec<Change>,
}

impl FileDiff {
    /// Whether the two versions are semantically identical.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

const IDENTITY_FIELDS: &[&str] = &["parent_id_path", "owner_uid_path"];
const CONNECTION_IDENTITY_FIELDS: &[&str] = &["from_uid_path", "to_uid_path"];

/// Renders a value for humans: `ExtResource` ids are replaced by the path of
/// the referenced file.
pub fn display_value(file: &SceneFile, v: &Value) -> String {
    if !has_ext_ref(v) {
        return v.to_string();
    }
    v.map_refs(&mut |kind, id| match kind {
        RefKind::Ext => match file.ext_by_id(id) {
            Some(i) => Value::ExtResource(
                file.ext_resources
                    .get(i)
                    .map(|e| e.path.clone())
                    .unwrap_or_default(),
            ),
            None => Value::ExtResource(id.to_string()),
        },
        RefKind::Sub => Value::SubResource(id.to_string()),
    })
    .to_string()
}

fn has_ext_ref(v: &Value) -> bool {
    let mut found = false;
    v.for_each_ref(&mut |k, _| found |= k == RefKind::Ext);
    found
}

struct Ctx<'a> {
    a: &'a SceneFile,
    b: &'a SceneFile,
    m: Matching,
    opts: &'a DiffOptions,
    paths_a: HashMap<&'a str, usize>,
}

impl Ctx<'_> {
    fn eq(&self, va: &Value, vb: &Value) -> bool {
        self.m.values_equal(va, vb, self.opts.matching.epsilon)
    }

    fn show_a(&self, v: &Value) -> String {
        display_value(self.a, v)
    }

    fn show_b(&self, v: &Value) -> String {
        display_value(self.b, v)
    }

    fn ignored_key(&self, key: &str) -> bool {
        self.opts.ignore_editor_metadata && key.starts_with("metadata/_edit_")
    }

    fn props(&self, pa: &[Prop], pb: &[Prop], target: &Target, out: &mut Vec<Change>) {
        let index_a: HashMap<&str, &Prop> = pa.iter().map(|p| (p.key.as_str(), p)).collect();
        let keys_b: HashSet<&str> = pb.iter().map(|p| p.key.as_str()).collect();
        for p in pb {
            if self.ignored_key(&p.key) {
                continue;
            }
            match index_a.get(p.key.as_str()) {
                None => out.push(Change::PropertyAdded {
                    target: target.clone(),
                    property: p.key.clone(),
                    new: self.show_b(&p.value),
                }),
                Some(old) if !self.eq(&old.value, &p.value) => {
                    let comps: Vec<ComponentChange> = components(
                        &old.value,
                        &p.value,
                        &|v| self.show_a(v),
                        &|v| self.show_b(v),
                        &|x, y| self.eq(x, y),
                    );
                    out.push(Change::PropertyChanged {
                        target: target.clone(),
                        property: p.key.clone(),
                        old: self.show_a(&old.value),
                        new: self.show_b(&p.value),
                        components: comps,
                    });
                }
                Some(_) => {}
            }
        }
        for p in pa {
            if !keys_b.contains(p.key.as_str()) && !self.ignored_key(&p.key) {
                out.push(Change::PropertyRemoved {
                    target: target.clone(),
                    property: p.key.clone(),
                    old: self.show_a(&p.value),
                });
            }
        }
    }

    fn translate_path(&self, path: &str) -> String {
        let mut end = path.len();
        loop {
            let prefix = path.get(..end).unwrap_or("");
            if let Some(&i) = self.paths_a.get(prefix) {
                return match self.m.node_a_to_b.get(i).copied().flatten() {
                    Some(j) => {
                        let new_prefix = self.b.nodes.get(j).map_or(prefix, |n| n.path.as_str());
                        format!("{new_prefix}{}", path.get(end..).unwrap_or(""))
                    }
                    None => path.to_string(),
                };
            }
            match prefix.rfind('/') {
                Some(k) => end = k,
                None => return path.to_string(),
            }
        }
    }
}

/// Computes the semantic diff between `a` (old) and `b` (new).
pub fn diff(a: &SceneFile, b: &SceneFile, opts: &DiffOptions) -> FileDiff {
    let m = Matching::compute(a, b, &opts.matching);
    let paths_a = a
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.path.as_str(), i))
        .collect();
    let ctx = Ctx {
        a,
        b,
        m,
        opts,
        paths_a,
    };
    let mut out = Vec::new();
    header(&ctx, &mut out);
    ext_resources(&ctx, &mut out);
    sub_resources(&ctx, &mut out);
    nodes(&ctx, &mut out);
    if a.resource_section.is_some() || b.resource_section.is_some() {
        ctx.props(
            &a.resource_props,
            &b.resource_props,
            &Target::Resource,
            &mut out,
        );
    }
    connections(&ctx, &mut out);
    editables(&ctx, &mut out);
    FileDiff { changes: out }
}

fn header(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    let (Some(ha), Some(hb)) = (ctx.a.doc.sections.first(), ctx.b.doc.sections.first()) else {
        return;
    };
    let (ha, hb) = (&ha.header, &hb.header);
    let mut keys: Vec<&str> = ha.fields.iter().map(|f| f.key.as_str()).collect();
    for f in &hb.fields {
        if !keys.contains(&f.key.as_str()) {
            keys.push(f.key.as_str());
        }
    }
    for key in keys {
        if key == "load_steps" {
            continue;
        }
        let va = ha.get(key);
        let vb = hb.get(key);
        let same = match (va, vb) {
            (Some(x), Some(y)) => x.semantic_eq(y, 0.0),
            (None, None) => true,
            _ => false,
        };
        if !same {
            out.push(Change::HeaderChanged(FieldChange {
                field: key.to_string(),
                old: va.map(ToString::to_string),
                new: vb.map(ToString::to_string),
            }));
        }
    }
    if ha.name != hb.name {
        out.push(Change::HeaderChanged(FieldChange {
            field: "kind".into(),
            old: Some(ha.name.clone()),
            new: Some(hb.name.clone()),
        }));
    }
}

fn opt_change(field: &str, old: Option<&str>, new: Option<&str>) -> Option<FieldChange> {
    (old != new).then(|| FieldChange {
        field: field.into(),
        old: old.map(str::to_string),
        new: new.map(str::to_string),
    })
}

fn ext_resources(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    for (j, eb) in ctx.b.ext_resources.iter().enumerate() {
        match ctx.m.ext_b_to_a.get(j).copied().flatten() {
            None => out.push(Change::ExtResourceAdded {
                path: eb.path.clone(),
                type_name: eb.type_name.clone(),
                uid: eb.uid.clone(),
            }),
            Some(i) => {
                let Some(ea) = ctx.a.ext_resources.get(i) else {
                    continue;
                };
                let changes: Vec<FieldChange> = [
                    opt_change("path", Some(&ea.path), Some(&eb.path)),
                    opt_change("type", Some(&ea.type_name), Some(&eb.type_name)),
                    opt_change("uid", ea.uid.as_deref(), eb.uid.as_deref()),
                ]
                .into_iter()
                .flatten()
                .collect();
                if !changes.is_empty() {
                    out.push(Change::ExtResourceChanged {
                        path: eb.path.clone(),
                        changes,
                    });
                }
            }
        }
    }
    for (i, ea) in ctx.a.ext_resources.iter().enumerate() {
        if ctx.m.ext_a_to_b.get(i).copied().flatten().is_none() {
            out.push(Change::ExtResourceRemoved {
                path: ea.path.clone(),
                type_name: ea.type_name.clone(),
                uid: ea.uid.clone(),
            });
        }
    }
}

fn sub_resources(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    for (j, sb) in ctx.b.sub_resources.iter().enumerate() {
        match ctx.m.sub_b_to_a.get(j).copied().flatten() {
            None => out.push(Change::SubResourceAdded {
                id: sb.id.clone(),
                type_name: sb.type_name.clone(),
            }),
            Some(i) => {
                let Some(sa) = ctx.a.sub_resources.get(i) else {
                    continue;
                };
                if sa.type_name != sb.type_name {
                    out.push(Change::SubResourceChanged {
                        id: sb.id.clone(),
                        old_type: sa.type_name.clone(),
                        new_type: sb.type_name.clone(),
                    });
                }
                let target = Target::SubResource {
                    id: sb.id.clone(),
                    type_name: sb.type_name.clone(),
                };
                ctx.props(&sa.props, &sb.props, &target, out);
            }
        }
    }
    for (i, sa) in ctx.a.sub_resources.iter().enumerate() {
        if ctx.m.sub_a_to_b.get(i).copied().flatten().is_none() {
            out.push(Change::SubResourceRemoved {
                id: sa.id.clone(),
                type_name: sa.type_name.clone(),
            });
        }
    }
}

fn instance_display(file: &SceneFile, n: &Node) -> Option<String> {
    n.instance.as_ref().map(|v| match v {
        Value::ExtResource(id) => file
            .ext_by_id(id)
            .and_then(|i| file.ext_resources.get(i))
            .map_or_else(|| id.clone(), |e| e.path.clone()),
        other => display_value(file, other),
    })
}

fn nodes(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    let (a, b) = (ctx.a, ctx.b);
    let moved: HashSet<usize> = (0..b.nodes.len())
        .filter(|&j| {
            let Some(i) = ctx.m.node_b_to_a.get(j).copied().flatten() else {
                return false;
            };
            match (a.nodes.get(i), b.nodes.get(j)) {
                (Some(na), Some(nb)) => !same_parent(ctx, na, nb),
                _ => false,
            }
        })
        .collect();
    let reordered = reorders(ctx, &moved);
    for (j, nb) in b.nodes.iter().enumerate() {
        let Some(i) = ctx.m.node_b_to_a.get(j).copied().flatten() else {
            out.push(Change::NodeAdded {
                path: nb.path.clone(),
                type_name: nb.type_name.clone(),
                instance: instance_display(b, nb),
            });
            continue;
        };
        let Some(na) = a.nodes.get(i) else { continue };
        let path = nb.path.clone();
        if moved.contains(&j) {
            out.push(Change::NodeMoved {
                path: na.path.clone(),
                new_path: nb.path.clone(),
            });
        } else if na.name != nb.name {
            out.push(Change::NodeRenamed {
                path: na.path.clone(),
                new_name: nb.name.clone(),
                new_path: nb.path.clone(),
            });
        }
        if let Some((old_position, new_position)) = reordered.get(&j) {
            out.push(Change::NodeReordered {
                path: path.clone(),
                old_position: *old_position,
                new_position: *new_position,
            });
        }
        if na.type_name != nb.type_name {
            out.push(Change::NodeTypeChanged {
                path: path.clone(),
                old: na.type_name.clone(),
                new: nb.type_name.clone(),
            });
        }
        let inst_same = match (&na.instance, &nb.instance) {
            (Some(x), Some(y)) => ctx.eq(x, y),
            (None, None) => true,
            _ => false,
        };
        if !inst_same {
            out.push(Change::InstanceChanged {
                path: path.clone(),
                old: instance_display(a, na),
                new: instance_display(b, nb),
            });
        }
        let added: Vec<String> = nb
            .groups
            .iter()
            .filter(|g| !na.groups.contains(g))
            .cloned()
            .collect();
        let removed: Vec<String> = na
            .groups
            .iter()
            .filter(|g| !nb.groups.contains(g))
            .cloned()
            .collect();
        if !added.is_empty() || !removed.is_empty() {
            out.push(Change::GroupsChanged {
                path: path.clone(),
                added,
                removed,
            });
        }
        for change in [
            opt_change("owner", na.owner.as_deref(), nb.owner.as_deref()),
            opt_change(
                "index",
                na.index.map(|x| x.to_string()).as_deref(),
                nb.index.map(|x| x.to_string()).as_deref(),
            ),
            opt_change(
                "instance_placeholder",
                na.instance_placeholder.as_deref(),
                nb.instance_placeholder.as_deref(),
            ),
        ]
        .into_iter()
        .flatten()
        {
            out.push(Change::NodeAttributeChanged {
                path: path.clone(),
                change,
            });
        }
        other_fields(&na.other_fields, &nb.other_fields, IDENTITY_FIELDS)
            .into_iter()
            .for_each(|change| {
                out.push(Change::NodeAttributeChanged {
                    path: path.clone(),
                    change,
                })
            });
        ctx.props(&na.props, &nb.props, &Target::Node { path }, out);
    }
    for (i, na) in a.nodes.iter().enumerate() {
        if ctx.m.node_a_to_b.get(i).copied().flatten().is_none() {
            out.push(Change::NodeRemoved {
                path: na.path.clone(),
                type_name: na.type_name.clone(),
            });
        }
    }
}

fn other_fields(
    fa: &[(String, Value)],
    fb: &[(String, Value)],
    ignore: &[&str],
) -> Vec<FieldChange> {
    let mut out = Vec::new();
    for (k, vb) in fb {
        if ignore.contains(&k.as_str()) {
            continue;
        }
        match fa.iter().find(|(ka, _)| ka == k) {
            Some((_, va)) if va.semantic_eq(vb, 0.0) => {}
            Some((_, va)) => out.push(FieldChange {
                field: k.clone(),
                old: Some(va.to_string()),
                new: Some(vb.to_string()),
            }),
            None => out.push(FieldChange {
                field: k.clone(),
                old: None,
                new: Some(vb.to_string()),
            }),
        }
    }
    for (k, va) in fa {
        if !ignore.contains(&k.as_str()) && !fb.iter().any(|(kb, _)| kb == k) {
            out.push(FieldChange {
                field: k.clone(),
                old: Some(va.to_string()),
                new: None,
            });
        }
    }
    out
}

fn same_parent(ctx: &Ctx<'_>, na: &Node, nb: &Node) -> bool {
    match (&na.parent, &nb.parent) {
        (ParentRef::None, ParentRef::None) => true,
        (ParentRef::Node(pa), ParentRef::Node(pb)) => {
            ctx.m.node_a_to_b.get(*pa).copied().flatten() == Some(*pb)
        }
        (ParentRef::External(x), ParentRef::External(y)) => x == y,
        _ => false,
    }
}

fn reorders(ctx: &Ctx<'_>, moved: &HashSet<usize>) -> HashMap<usize, (usize, usize)> {
    let mut by_parent_b: HashMap<ParentRef, Vec<usize>> = HashMap::new();
    for (j, nb) in ctx.b.nodes.iter().enumerate() {
        by_parent_b.entry(nb.parent.clone()).or_default().push(j);
    }
    let mut by_parent_a: HashMap<ParentRef, Vec<usize>> = HashMap::new();
    for (i, na) in ctx.a.nodes.iter().enumerate() {
        by_parent_a.entry(na.parent.clone()).or_default().push(i);
    }
    let mut result = HashMap::new();
    for (parent_b, kids_b) in &by_parent_b {
        let parent_a = match parent_b {
            ParentRef::Node(pb) => match ctx.m.node_b_to_a.get(*pb).copied().flatten() {
                Some(pa) => ParentRef::Node(pa),
                None => continue,
            },
            other => other.clone(),
        };
        let Some(kids_a) = by_parent_a.get(&parent_a) else {
            continue;
        };
        let common_b: Vec<usize> = kids_b
            .iter()
            .copied()
            .filter(|j| {
                !moved.contains(j) && ctx.m.node_b_to_a.get(*j).copied().flatten().is_some()
            })
            .collect();
        if common_b.len() < 2 {
            continue;
        }
        let pos_b: HashMap<usize, usize> =
            common_b.iter().enumerate().map(|(p, j)| (*j, p)).collect();
        let seq: Vec<usize> = kids_a
            .iter()
            .filter_map(|i| ctx.m.node_a_to_b.get(*i).copied().flatten())
            .filter(|j| pos_b.contains_key(j))
            .collect();
        let seq_pos: Vec<usize> = seq.iter().filter_map(|j| pos_b.get(j).copied()).collect();
        let keep = lis(&seq_pos);
        for (a_pos, j) in seq.iter().enumerate() {
            let Some(&bp) = pos_b.get(j) else { continue };
            if !keep.contains(&bp) {
                let old_position = ctx
                    .m
                    .node_b_to_a
                    .get(*j)
                    .copied()
                    .flatten()
                    .and_then(|i| kids_a.iter().position(|k| *k == i))
                    .unwrap_or(a_pos);
                let new_position = kids_b.iter().position(|k| k == j).unwrap_or(bp);
                result.insert(*j, (old_position, new_position));
            }
        }
    }
    result
}

fn lis(seq: &[usize]) -> HashSet<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; seq.len()];
    for (i, &x) in seq.iter().enumerate() {
        let pos = tails.partition_point(|&t| seq.get(t).is_some_and(|&v| v < x));
        if pos > 0 {
            if let Some(p) = prev.get_mut(i) {
                *p = tails.get(pos - 1).copied();
            }
        }
        if pos == tails.len() {
            tails.push(i);
        } else if let Some(t) = tails.get_mut(pos) {
            *t = i;
        }
    }
    let mut out = HashSet::new();
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        if let Some(&v) = seq.get(i) {
            out.insert(v);
        }
        cur = prev.get(i).copied().flatten();
    }
    out
}

fn connection_key(c: &Connection, from: String, to: String) -> ConnectionKey {
    ConnectionKey {
        signal: c.signal.clone(),
        from,
        to,
        method: c.method.clone(),
    }
}

fn connections(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    let a_keys: Vec<ConnectionKey> = ctx
        .a
        .connections
        .iter()
        .map(|c| connection_key(c, ctx.translate_path(&c.from), ctx.translate_path(&c.to)))
        .collect();
    let mut used = vec![false; a_keys.len()];
    for cb in &ctx.b.connections {
        let kb = connection_key(cb, cb.from.clone(), cb.to.clone());
        let found = a_keys
            .iter()
            .enumerate()
            .find(|(i, k)| !used.get(*i).copied().unwrap_or(true) && **k == kb)
            .map(|(i, _)| i);
        match found {
            None => out.push(Change::ConnectionAdded(kb)),
            Some(i) => {
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
                let Some(ca) = ctx.a.connections.get(i) else {
                    continue;
                };
                let mut changes = Vec::new();
                if ca.flags != cb.flags {
                    changes.push(FieldChange {
                        field: "flags".into(),
                        old: Some(ca.flags.to_string()),
                        new: Some(cb.flags.to_string()),
                    });
                }
                if ca.unbinds != cb.unbinds {
                    changes.push(FieldChange {
                        field: "unbinds".into(),
                        old: Some(ca.unbinds.to_string()),
                        new: Some(cb.unbinds.to_string()),
                    });
                }
                let binds_same = match (&ca.binds, &cb.binds) {
                    (Some(x), Some(y)) => ctx.eq(x, y),
                    (None, None) => true,
                    (Some(Value::Array(v)), None) | (None, Some(Value::Array(v))) => v.is_empty(),
                    _ => false,
                };
                if !binds_same {
                    changes.push(FieldChange {
                        field: "binds".into(),
                        old: ca.binds.as_ref().map(|v| ctx.show_a(v)),
                        new: cb.binds.as_ref().map(|v| ctx.show_b(v)),
                    });
                }
                changes.extend(other_fields(
                    &ca.other_fields,
                    &cb.other_fields,
                    CONNECTION_IDENTITY_FIELDS,
                ));
                if !changes.is_empty() {
                    out.push(Change::ConnectionChanged {
                        connection: kb,
                        changes,
                    });
                }
            }
        }
    }
    for (i, ca) in ctx.a.connections.iter().enumerate() {
        if !used.get(i).copied().unwrap_or(true) {
            out.push(Change::ConnectionRemoved(connection_key(
                ca,
                ca.from.clone(),
                ca.to.clone(),
            )));
        }
    }
}

fn editables(ctx: &Ctx<'_>, out: &mut Vec<Change>) {
    let a_paths: Vec<String> = ctx
        .a
        .editables
        .iter()
        .map(|e| ctx.translate_path(&e.path))
        .collect();
    for e in &ctx.b.editables {
        if !a_paths.contains(&e.path) {
            out.push(Change::EditableAdded {
                path: e.path.clone(),
            });
        }
    }
    let b_paths: Vec<&str> = ctx.b.editables.iter().map(|e| e.path.as_str()).collect();
    for (e, translated) in ctx.a.editables.iter().zip(&a_paths) {
        if !b_paths.contains(&translated.as_str()) {
            out.push(Change::EditableRemoved {
                path: e.path.clone(),
            });
        }
    }
}
