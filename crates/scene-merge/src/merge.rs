use std::collections::{HashMap, HashSet};

use godot_text::{rewrite_refs, BodyItem, Header, RefKind, Section, Value};
use scene_model::{MatchOptions, Matching, ParentRef, SceneFile};

use crate::conflict::{Conflict, ConflictKind, ConflictStyle, Resolution};
use crate::edit::{
    add_property, put_block, remove_property, set_header_field, set_property, split_trailing,
    with_eol, MarkerStyle,
};
use crate::ids::{fnv, IdGen};
use crate::MergeError;

/// Options for [`crate::merge`].
#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    /// How conflicts are written.
    pub conflict: ConflictStyle,
    /// Conflict marker size and labels.
    pub markers: MarkerStyle,
    /// Matching options (rename threshold, float epsilon).
    pub matching: MatchOptions,
}

/// The result of a merge.
#[derive(Debug, Clone)]
pub struct MergeOutput {
    /// The merged file.
    pub text: String,
    /// Conflicts (empty for a clean merge).
    pub conflicts: Vec<Conflict>,
    /// Automatic adjustments (renumbered ids, …).
    pub resolutions: Vec<Resolution>,
}

impl MergeOutput {
    /// Whether the merge had no conflicts.
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

const TAG_OPEN: char = '\u{E000}';
const TAG_CLOSE: char = '\u{E001}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// Index of the section in OURS' document.
    Ours(usize),
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrapSide {
    /// Shown on our side of the markers only (their side is empty).
    OursOnly,
    /// Shown on their side only.
    TheirsOnly,
}

#[derive(Debug, Clone)]
struct OutSec {
    sec: Section,
    origin: Origin,
    removed: bool,
    /// Whole-section conflict wrap: (conflict index, side).
    wrap: Option<(usize, WrapSide)>,
    /// Header conflict: (conflict index, their variant of the header line).
    header_conflict: Option<(usize, String)>,
    /// Ext/sub: may be removed if unreferenced (their side deleted it).
    removable: bool,
}

impl OutSec {
    fn new(sec: Section, origin: Origin) -> Self {
        OutSec {
            sec,
            origin,
            removed: false,
            wrap: None,
            header_conflict: None,
            removable: false,
        }
    }

    fn id(&self) -> String {
        match self.sec.header.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.raw.clone(),
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum OutParent {
    Root,
    Node(usize),
    External(String),
}

#[derive(Debug, Clone)]
struct OutNode {
    s: OutSec,
    ours: Option<usize>,
    theirs: Option<usize>,
    name: String,
    parent: OutParent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    KeepOurs,
    TakeTheirs,
    Conflict,
}

fn decide(eq_bo: bool, eq_bt: bool, eq_ot: bool) -> Decision {
    if eq_bt || eq_ot {
        Decision::KeepOurs
    } else if eq_bo {
        Decision::TakeTheirs
    } else {
        Decision::Conflict
    }
}

fn opt_eq(m: &Matching, a: Option<&Value>, b: Option<&Value>, eps: f64) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => m.values_equal(x, y, eps),
        _ => false,
    }
}

fn field_value<'a>(h: Option<&'a Header>, key: &str) -> Option<&'a Value> {
    h.and_then(|h| h.get(key))
}

fn prop_value<'a>(s: Option<&'a Section>, key: &str) -> Option<&'a godot_text::Property> {
    s.and_then(|s| s.property(key))
}

const SOFT_FIELDS: &[&str] = &[
    "unique_id",
    "parent_id_path",
    "owner_uid_path",
    "from_uid_path",
    "to_uid_path",
];

enum Edit {
    SetField(String, Option<String>),
    SetProp(String, String),
    AddProp(String, String, Option<String>),
    RemoveProp(String),
    PropConflict {
        ci: usize,
        key: String,
        ours_line: String,
        theirs_line: String,
        after: Option<String>,
    },
}

struct Merger<'a> {
    base: &'a SceneFile,
    ours: &'a SceneFile,
    theirs: &'a SceneFile,
    m_bo: Matching,
    m_bt: Matching,
    m_ot: Matching,
    opts: &'a MergeOptions,
    eol: &'static str,
    eps: f64,
    ids: IdGen,

    header: OutSec,
    exts: Vec<OutSec>,
    subs: Vec<OutSec>,
    resource: Option<OutSec>,
    nodes: Vec<OutNode>,
    order: Vec<usize>,
    conns: Vec<OutSec>,
    editables: Vec<OutSec>,

    /// ours node index → out node index
    o2out: Vec<Option<usize>>,
    /// theirs node index → out node index
    t2out: Vec<Option<usize>>,
    /// ours ext/sub index → out index
    o_ext: Vec<Option<usize>>,
    o_sub: Vec<Option<usize>>,
    /// theirs local id → out local id (import cache)
    t_ext_map: HashMap<String, String>,
    t_sub_map: HashMap<String, String>,
    importing_subs: HashSet<String>,
    /// THEIRS' children per parent, in file order.
    theirs_children: HashMap<ParentRef, Vec<usize>>,
    /// Whether an out node is in `order` (placed).
    in_order: Vec<bool>,
    /// unique_ids used by nodes in the output.
    used_uids: HashSet<i64>,
    /// Out ids referenced from conflict blocks (kept alive by GC).
    extra_refs: Vec<(RefKind, String)>,

    conflicts: Vec<Conflict>,
    resolutions: Vec<Resolution>,
}

fn sec(f: &SceneFile, section: usize) -> Option<&Section> {
    f.doc.sections.get(section)
}

impl<'a> Merger<'a> {
    fn new(
        base: &'a SceneFile,
        ours: &'a SceneFile,
        theirs: &'a SceneFile,
        opts: &'a MergeOptions,
    ) -> Result<Self, MergeError> {
        let m_bo = Matching::compute(base, ours, &opts.matching);
        let m_bt = Matching::compute(base, theirs, &opts.matching);
        let m_ot = Matching::compute(ours, theirs, &opts.matching);
        let eol = ours.doc.line_ending().as_str();
        let seed = fnv(&format!(
            "{}\u{0}{}",
            ours.descriptor.uid.as_deref().unwrap_or(""),
            theirs.doc.to_text().len()
        ));
        let header_sec = ours
            .doc
            .sections
            .first()
            .cloned()
            .ok_or_else(|| MergeError::Internal("OURS has no descriptor".into()))?;
        let mut m = Merger {
            base,
            ours,
            theirs,
            m_bo,
            m_bt,
            m_ot,
            opts,
            eol,
            eps: opts.matching.epsilon,
            ids: IdGen::new(seed),
            header: OutSec::new(header_sec, Origin::Ours(0)),
            exts: Vec::new(),
            subs: Vec::new(),
            resource: None,
            nodes: Vec::new(),
            order: Vec::new(),
            conns: Vec::new(),
            editables: Vec::new(),
            o2out: vec![None; ours.nodes.len()],
            t2out: vec![None; theirs.nodes.len()],
            o_ext: vec![None; ours.ext_resources.len()],
            o_sub: vec![None; ours.sub_resources.len()],
            t_ext_map: HashMap::new(),
            t_sub_map: HashMap::new(),
            importing_subs: HashSet::new(),
            theirs_children: {
                let mut map: HashMap<ParentRef, Vec<usize>> = HashMap::new();
                for (k, n) in theirs.nodes.iter().enumerate() {
                    map.entry(n.parent.clone()).or_default().push(k);
                }
                map
            },
            in_order: Vec::new(),
            used_uids: ours.nodes.iter().filter_map(|n| n.unique_id).collect(),
            extra_refs: Vec::new(),
            conflicts: Vec::new(),
            resolutions: Vec::new(),
        };
        m.init_from_ours();
        Ok(m)
    }

    fn init_from_ours(&mut self) {
        let o = self.ours;
        for (i, e) in o.ext_resources.iter().enumerate() {
            if let Some(s) = sec(o, e.section) {
                if let Some(slot) = self.o_ext.get_mut(i) {
                    *slot = Some(self.exts.len());
                }
                self.exts
                    .push(OutSec::new(s.clone(), Origin::Ours(e.section)));
            }
        }
        for (i, sr) in o.sub_resources.iter().enumerate() {
            if let Some(s) = sec(o, sr.section) {
                if let Some(slot) = self.o_sub.get_mut(i) {
                    *slot = Some(self.subs.len());
                }
                self.subs
                    .push(OutSec::new(s.clone(), Origin::Ours(sr.section)));
            }
        }
        if let Some(rs) = o.resource_section {
            if let Some(s) = sec(o, rs) {
                self.resource = Some(OutSec::new(s.clone(), Origin::Ours(rs)));
            }
        }
        for (i, n) in o.nodes.iter().enumerate() {
            let Some(s) = sec(o, n.section) else { continue };
            let parent = match &n.parent {
                ParentRef::None => OutParent::Root,
                ParentRef::Node(p) => match self.o2out.get(*p).copied().flatten() {
                    Some(x) => OutParent::Node(x),
                    None => OutParent::External(n.parent_path.clone().unwrap_or_default()),
                },
                ParentRef::External(p) => OutParent::External(p.clone()),
            };
            let idx = self.nodes.len();
            if let Some(slot) = self.o2out.get_mut(i) {
                *slot = Some(idx);
            }
            self.order.push(idx);
            self.in_order.push(true);
            self.nodes.push(OutNode {
                s: OutSec::new(s.clone(), Origin::Ours(n.section)),
                ours: Some(i),
                theirs: None,
                name: n.name.clone(),
                parent,
            });
        }
        for c in &o.connections {
            if let Some(s) = sec(o, c.section) {
                self.conns
                    .push(OutSec::new(s.clone(), Origin::Ours(c.section)));
            }
        }
        for e in &o.editables {
            if let Some(s) = sec(o, e.section) {
                self.editables
                    .push(OutSec::new(s.clone(), Origin::Ours(e.section)));
            }
        }
    }

    fn add_conflict(&mut self, c: Conflict) -> usize {
        self.conflicts.push(c);
        self.conflicts.len() - 1
    }

    fn style(&self) -> ConflictStyle {
        self.opts.conflict
    }

    fn block(&self, idx: usize, ours: &str, theirs: &str) -> String {
        let mut style = self.opts.markers.clone();
        style.ours = format!("{}{TAG_OPEN}{idx}{TAG_CLOSE}", style.ours);
        style.block(ours, theirs, self.eol)
    }

    fn import_raw(&mut self, raw: &str) -> String {
        let mut wanted: Vec<(RefKind, String)> = Vec::new();
        let _ = rewrite_refs(raw, |k, id| {
            wanted.push((k, id.to_string()));
            None
        });
        let mut map: HashMap<(RefKind, String), String> = HashMap::new();
        for (k, id) in wanted {
            let new_id = match k {
                RefKind::Ext => self.import_ext(&id),
                RefKind::Sub => self.import_sub(&id),
            };
            map.insert((k, id), new_id);
        }
        let out = rewrite_refs(raw, |k, id| {
            map.get(&(k, id.to_string()))
                .filter(|n| n.as_str() != id)
                .cloned()
        })
        .unwrap_or_else(|_| raw.to_string());
        with_eol(&out, self.eol)
    }

    fn keep_refs_of(&mut self, raw: &str) {
        let mut found = Vec::new();
        let _ = rewrite_refs(raw, |k, id| {
            found.push((k, id.to_string()));
            None
        });
        self.extra_refs.extend(found);
    }

    fn used_ext_ids(&self) -> HashSet<String> {
        self.exts
            .iter()
            .filter(|e| !e.removed)
            .map(OutSec::id)
            .collect()
    }

    fn used_sub_ids(&self) -> HashSet<String> {
        self.subs
            .iter()
            .filter(|e| !e.removed)
            .map(OutSec::id)
            .collect()
    }

    fn import_ext(&mut self, id: &str) -> String {
        if let Some(v) = self.t_ext_map.get(id) {
            return v.clone();
        }
        let Some(j) = self.theirs.ext_by_id(id) else {
            return id.to_string();
        };
        if let Some(i) = self.m_ot.ext_b_to_a.get(j).copied().flatten() {
            if let Some(x) = self.o_ext.get(i).copied().flatten() {
                if let Some(e) = self.exts.get_mut(x) {
                    e.removed = false;
                    e.removable = false;
                    let out_id = e.id();
                    self.t_ext_map.insert(id.to_string(), out_id.clone());
                    return out_id;
                }
            }
        }
        let Some(te) = self.theirs.ext_resources.get(j) else {
            return id.to_string();
        };
        let Some(s) = sec(self.theirs, te.section).cloned() else {
            return id.to_string();
        };
        let used = self.used_ext_ids();
        let mut out = OutSec::new(s, Origin::New);
        let out_id = if used.contains(id) {
            let n = self.exts.iter().filter(|e| !e.removed).count() + 1;
            let new_id = self.ids.unique(&n.to_string(), &used);
            let _ = out.sec.header.set_field("id", format!("\"{new_id}\""));
            self.resolutions.push(Resolution {
                kind: "ext_resource_id".into(),
                target: format!("ext_resource:{}", te.path),
                from: id.to_string(),
                to: new_id.clone(),
            });
            new_id
        } else {
            id.to_string()
        };
        out.sec.header.raw = with_eol(&out.sec.header.raw, self.eol);
        self.exts.push(out);
        self.t_ext_map.insert(id.to_string(), out_id.clone());
        out_id
    }

    fn import_sub(&mut self, id: &str) -> String {
        if let Some(v) = self.t_sub_map.get(id) {
            return v.clone();
        }
        let Some(j) = self.theirs.sub_by_id(id) else {
            return id.to_string();
        };
        let via_base = self
            .m_bt
            .sub_b_to_a
            .get(j)
            .copied()
            .flatten()
            .and_then(|b| self.m_bo.sub_a_to_b.get(b).copied().flatten());
        let both_added = self.m_ot.sub_b_to_a.get(j).copied().flatten().filter(|&i| {
            self.m_bo.sub_b_to_a.get(i).copied().flatten().is_none()
                && self.m_bt.sub_b_to_a.get(j).copied().flatten().is_none()
                && match (
                    self.ours.sub_resources.get(i),
                    self.theirs.sub_resources.get(j),
                ) {
                    (Some(a), Some(b)) => {
                        a.type_name == b.type_name
                            && self.m_ot.props_equal(&a.props, &b.props, self.eps)
                    }
                    _ => false,
                }
        });
        if let Some(i) = via_base.or(both_added) {
            if let Some(x) = self.o_sub.get(i).copied().flatten() {
                if let Some(s) = self.subs.get_mut(x) {
                    s.removed = false;
                    s.removable = false;
                    let out_id = s.id();
                    self.t_sub_map.insert(id.to_string(), out_id.clone());
                    return out_id;
                }
            }
        }
        if !self.importing_subs.insert(id.to_string()) {
            return id.to_string();
        }
        let Some(ts) = self.theirs.sub_resources.get(j) else {
            return id.to_string();
        };
        let Some(s) = sec(self.theirs, ts.section).cloned() else {
            return id.to_string();
        };
        let type_name = ts.type_name.clone();
        let mut out = OutSec::new(s, Origin::New);
        self.import_section_refs(&mut out.sec);
        let used = self.used_sub_ids();
        let out_id = if used.contains(id) {
            let new_id = self.ids.unique(&type_name, &used);
            let _ = out.sec.header.set_field("id", format!("\"{new_id}\""));
            self.resolutions.push(Resolution {
                kind: "sub_resource_id".into(),
                target: format!("sub_resource:{type_name}"),
                from: id.to_string(),
                to: new_id.clone(),
            });
            new_id
        } else {
            id.to_string()
        };
        self.subs.push(out);
        self.t_sub_map.insert(id.to_string(), out_id.clone());
        self.importing_subs.remove(id);
        out_id
    }

    fn import_section_refs(&mut self, s: &mut Section) {
        let fields: Vec<(String, String)> = s
            .header
            .fields
            .iter()
            .map(|f| (f.key.clone(), f.raw_value.clone()))
            .collect();
        for (k, raw) in fields {
            let new = self.import_raw(&raw);
            if new != raw {
                let _ = s.header.set_field(&k, new);
            }
        }
        s.header.raw = with_eol(&s.header.raw, self.eol);
        let mut items = std::mem::take(&mut s.body);
        for item in &mut items {
            match item {
                BodyItem::Property(p) => {
                    let new = self.import_raw(&p.raw_value);
                    if new != p.raw_value {
                        let _ = p.set_raw_value(new);
                    }
                    p.lead = with_eol(&p.lead, self.eol);
                }
                BodyItem::Trivia(t) => *t = with_eol(t, self.eol),
            }
        }
        s.body = items;
    }

    fn merge_fields(
        &mut self,
        b: Option<&Header>,
        o: &Header,
        t: Option<&Header>,
        skip: &[&str],
    ) -> (Vec<Edit>, Vec<String>) {
        let mut keys: Vec<String> = Vec::new();
        for h in [b, Some(o), t].into_iter().flatten() {
            for f in &h.fields {
                if !skip.contains(&f.key.as_str()) && !keys.contains(&f.key) {
                    keys.push(f.key.clone());
                }
            }
        }
        let mut edits = Vec::new();
        let mut conflicts = Vec::new();
        for key in keys {
            let (vb, vo, vt) = (field_value(b, &key), o.get(&key), field_value(t, &key));
            let d = decide(
                opt_eq(&self.m_bo, vb, vo, self.eps),
                opt_eq(&self.m_bt, vb, vt, self.eps),
                opt_eq(&self.m_ot, vo, vt, self.eps),
            );
            match d {
                Decision::KeepOurs => {}
                Decision::TakeTheirs => {
                    let raw = t.and_then(|h| h.field(&key)).map(|f| f.raw_value.clone());
                    let raw = raw.map(|r| self.import_raw(&r));
                    edits.push(Edit::SetField(key, raw));
                }
                Decision::Conflict if SOFT_FIELDS.contains(&key.as_str()) => {}
                Decision::Conflict => conflicts.push(key),
            }
        }
        (edits, conflicts)
    }

    fn merge_props(
        &mut self,
        b: Option<&Section>,
        o: &Section,
        t: Option<&Section>,
        target: &str,
    ) -> Vec<Edit> {
        let mut keys: Vec<String> = Vec::new();
        for s in [Some(o), t, b].into_iter().flatten() {
            for p in s.properties() {
                if !keys.contains(&p.key) {
                    keys.push(p.key.clone());
                }
            }
        }
        let two_way = b.is_none();
        let mut edits = Vec::new();
        for key in keys {
            let (pb, po, pt) = (prop_value(b, &key), o.property(&key), prop_value(t, &key));
            let (vb, vo, vt) = (
                pb.map(|p| &p.value),
                po.map(|p| &p.value),
                pt.map(|p| &p.value),
            );
            let d = if two_way {
                match (vo, vt) {
                    (Some(x), Some(y)) if !self.m_ot.values_equal(x, y, self.eps) => {
                        Decision::Conflict
                    }
                    (None, Some(_)) => Decision::TakeTheirs,
                    _ => Decision::KeepOurs,
                }
            } else {
                decide(
                    opt_eq(&self.m_bo, vb, vo, self.eps),
                    opt_eq(&self.m_bt, vb, vt, self.eps),
                    opt_eq(&self.m_ot, vo, vt, self.eps),
                )
            };
            let after = t.and_then(|ts| prev_existing_key(ts, &key, o));
            match d {
                Decision::KeepOurs => {}
                Decision::TakeTheirs => match pt {
                    Some(p) => {
                        let raw = self.import_raw(&p.raw_value);
                        if po.is_some() {
                            edits.push(Edit::SetProp(key, raw));
                        } else {
                            edits.push(Edit::AddProp(key, raw, after));
                        }
                    }
                    None => edits.push(Edit::RemoveProp(key)),
                },
                Decision::Conflict => {
                    let show_o = po.map(|p| p.raw_value.clone());
                    let show_t = pt.map(|p| p.raw_value.clone());
                    let resolution = self.style();
                    let ci = self.add_conflict(Conflict {
                        kind: ConflictKind::Property,
                        target: target.to_string(),
                        property: Some(key.clone()),
                        base: pb.map(|p| p.raw_value.clone()),
                        ours: show_o,
                        theirs: show_t,
                        resolution,
                        line: None,
                    });
                    match resolution {
                        ConflictStyle::Ours => {}
                        ConflictStyle::Theirs => match pt {
                            Some(p) => {
                                let raw = self.import_raw(&p.raw_value);
                                if po.is_some() {
                                    edits.push(Edit::SetProp(key, raw));
                                } else {
                                    edits.push(Edit::AddProp(key, raw, after));
                                }
                            }
                            None => edits.push(Edit::RemoveProp(key)),
                        },
                        ConflictStyle::Markers => {
                            let ours_line = po.map(|p| p.raw()).unwrap_or_default();
                            let theirs_line = match pt {
                                Some(p) => {
                                    let raw = self.import_raw(&p.raw_value);
                                    format!("{}{raw}", with_eol(&p.lead, self.eol))
                                }
                                None => String::new(),
                            };
                            self.keep_refs_of(&ours_line);
                            self.keep_refs_of(&theirs_line);
                            edits.push(Edit::PropConflict {
                                ci,
                                key,
                                ours_line,
                                theirs_line,
                                after,
                            });
                        }
                    }
                }
            }
        }
        edits
    }

    fn apply(&mut self, s: &mut Section, edits: Vec<Edit>) {
        let eol = self.eol;
        for e in edits {
            match e {
                Edit::SetField(k, Some(raw)) => {
                    let _ = set_header_field(&mut s.header, &k, raw);
                }
                Edit::SetField(k, None) => {
                    s.header.remove_field(&k);
                }
                Edit::SetProp(k, raw) => {
                    let _ = set_property(s, &k, raw, eol);
                }
                Edit::AddProp(k, raw, after) => {
                    let _ = add_property(s, &k, raw, after.as_deref(), eol);
                }
                Edit::RemoveProp(k) => {
                    remove_property(s, &k);
                }
                Edit::PropConflict {
                    ci,
                    key,
                    ours_line,
                    theirs_line,
                    after,
                } => {
                    let block = self.block(ci, &ours_line, &theirs_line);
                    put_block(s, &key, block, after.as_deref(), eol);
                }
            }
        }
    }

    fn header_conflict(
        &mut self,
        out: &mut OutSec,
        t: Option<&Header>,
        fields: &[String],
        kind: ConflictKind,
        target: String,
        b: Option<&Header>,
    ) {
        if fields.is_empty() {
            return;
        }
        let show = |h: Option<&Header>| -> Option<String> {
            h.map(|h| {
                fields
                    .iter()
                    .map(|k| {
                        format!(
                            "{k}={}",
                            h.field(k).map_or("(none)", |f| f.raw_value.as_str())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        };
        let resolution = self.style();
        let ci = self.add_conflict(Conflict {
            kind,
            target,
            property: Some(fields.join(",")),
            base: show(b),
            ours: show(Some(&out.sec.header)),
            theirs: show(t),
            resolution,
            line: None,
        });
        let mut variant = out.sec.header.clone();
        for k in fields {
            match t.and_then(|h| h.field(k)).map(|f| f.raw_value.clone()) {
                Some(raw) => {
                    let raw = self.import_raw(&raw);
                    let _ = set_header_field(&mut variant, k, raw);
                }
                None => {
                    variant.remove_field(k);
                }
            }
        }
        match resolution {
            ConflictStyle::Ours => {}
            ConflictStyle::Theirs => out.sec.header = variant,
            ConflictStyle::Markers => {
                self.keep_refs_of(&variant.raw);
                out.header_conflict = Some((ci, variant.raw));
            }
        }
    }

    fn merge_header(&mut self) {
        let b = sec(self.base, 0).map(|s| &s.header);
        let t = sec(self.theirs, 0).map(|s| &s.header);
        let o = self.header.sec.header.clone();
        let (edits, conflicts) = self.merge_fields(b, &o, t, &["load_steps", "format"]);
        let mut s = self.header.sec.clone();
        self.apply(&mut s, edits);
        let mut out = OutSec::new(s, Origin::Ours(0));
        self.header_conflict(
            &mut out,
            t,
            &conflicts,
            ConflictKind::Header,
            "header".into(),
            b,
        );
        let fo = self.ours.descriptor.format;
        let ft = self.theirs.descriptor.format;
        if ft > fo {
            let _ = out.sec.header.set_field("format", ft.to_string());
        }
        self.header = out;
    }

    fn merge_ext_resources(&mut self) {
        for (bi, _) in self.base.ext_resources.iter().enumerate() {
            let oi = self.m_bo.ext_a_to_b.get(bi).copied().flatten();
            let ti = self.m_bt.ext_a_to_b.get(bi).copied().flatten();
            let Some(oi) = oi else { continue };
            let Some(x) = self.o_ext.get(oi).copied().flatten() else {
                continue;
            };
            match ti {
                None => {
                    if let Some(e) = self.exts.get_mut(x) {
                        e.removable = true;
                    }
                }
                Some(ti) => {
                    let bh = self
                        .base
                        .ext_resources
                        .get(bi)
                        .and_then(|e| sec(self.base, e.section))
                        .map(|s| s.header.clone());
                    let th = self
                        .theirs
                        .ext_resources
                        .get(ti)
                        .and_then(|e| sec(self.theirs, e.section))
                        .map(|s| s.header.clone());
                    let Some(mut out) = self.exts.get(x).cloned() else {
                        continue;
                    };
                    let oh = out.sec.header.clone();
                    let (edits, conflicts) =
                        self.merge_fields(bh.as_ref(), &oh, th.as_ref(), &["id"]);
                    self.apply(&mut out.sec, edits);
                    let path = self
                        .ours
                        .ext_resources
                        .get(oi)
                        .map(|e| e.path.clone())
                        .unwrap_or_default();
                    self.header_conflict(
                        &mut out,
                        th.as_ref(),
                        &conflicts,
                        ConflictKind::ExtResource,
                        format!("ext_resource:{path}"),
                        bh.as_ref(),
                    );
                    if let Some(slot) = self.exts.get_mut(x) {
                        *slot = out;
                    }
                }
            }
        }
    }

    fn merge_sub_resources(&mut self) {
        for (bi, _) in self.base.sub_resources.iter().enumerate() {
            let Some(oi) = self.m_bo.sub_a_to_b.get(bi).copied().flatten() else {
                continue;
            };
            let Some(x) = self.o_sub.get(oi).copied().flatten() else {
                continue;
            };
            let Some(ti) = self.m_bt.sub_a_to_b.get(bi).copied().flatten() else {
                if let Some(s) = self.subs.get_mut(x) {
                    s.removable = true;
                }
                continue;
            };
            let bs = self
                .base
                .sub_resources
                .get(bi)
                .and_then(|s| sec(self.base, s.section))
                .cloned();
            let ts = self
                .theirs
                .sub_resources
                .get(ti)
                .and_then(|s| sec(self.theirs, s.section))
                .cloned();
            let Some(mut out) = self.subs.get(x).cloned() else {
                continue;
            };
            let target = format!("sub_resource:{}", out.id());
            let osec = out.sec.clone();
            let (fedits, fconf) = self.merge_fields(
                bs.as_ref().map(|s| &s.header),
                &osec.header,
                ts.as_ref().map(|s| &s.header),
                &["id"],
            );
            let pedits = self.merge_props(bs.as_ref(), &osec, ts.as_ref(), &target);
            self.apply(&mut out.sec, fedits);
            self.apply(&mut out.sec, pedits);
            self.header_conflict(
                &mut out,
                ts.as_ref().map(|s| &s.header),
                &fconf,
                ConflictKind::NodeAttribute,
                target,
                bs.as_ref().map(|s| &s.header),
            );
            if let Some(slot) = self.subs.get_mut(x) {
                *slot = out;
            }
        }
    }

    fn merge_resource_section(&mut self) {
        let (Some(o), Some(tr)) = (self.resource.clone(), self.theirs.resource_section) else {
            return;
        };
        let b = self
            .base
            .resource_section
            .and_then(|i| sec(self.base, i))
            .cloned();
        let t = sec(self.theirs, tr).cloned();
        let edits = self.merge_props(b.as_ref(), &o.sec, t.as_ref(), "resource");
        let mut out = o;
        self.apply(&mut out.sec, edits);
        self.resource = Some(out);
    }

    fn node_sec<'f>(&self, f: &'f SceneFile, i: usize) -> Option<&'f Section> {
        f.nodes.get(i).and_then(|n| sec(f, n.section))
    }

    fn node_changed(&self, m: &Matching, f: &SceneFile, i: usize, b: usize) -> bool {
        let (Some(nb), Some(nf)) = (self.base.nodes.get(b), f.nodes.get(i)) else {
            return true;
        };
        if nb.name != nf.name {
            return true;
        }
        let same_parent = match (&nb.parent, &nf.parent) {
            (ParentRef::None, ParentRef::None) => true,
            (ParentRef::Node(pb), ParentRef::Node(pf)) => {
                m.node_a_to_b.get(*pb).copied().flatten() == Some(*pf)
            }
            (ParentRef::External(x), ParentRef::External(y)) => x == y,
            _ => false,
        };
        if !same_parent {
            return true;
        }
        let (Some(sb), Some(sf)) = (self.node_sec(self.base, b), self.node_sec(f, i)) else {
            return true;
        };
        let fields_differ = {
            let mut keys: Vec<&str> = sb.header.fields.iter().map(|f| f.key.as_str()).collect();
            keys.extend(sf.header.fields.iter().map(|f| f.key.as_str()));
            keys.iter().any(|k| {
                !matches!(*k, "name" | "parent" | "parent_id_path" | "unique_id")
                    && !opt_eq(m, sb.header.get(k), sf.header.get(k), self.eps)
            })
        };
        fields_differ || !m.props_equal(&nb.props, &nf.props, self.eps)
    }

    fn theirs_prev_sibling_out(&self, j: usize) -> Option<usize> {
        let tn = self.theirs.nodes.get(j)?;
        let siblings = self.theirs_children.get(&tn.parent)?;
        let pos = siblings.iter().position(|&k| k == j)?;
        siblings
            .get(..pos)?
            .iter()
            .rev()
            .filter_map(|&k| self.t2out.get(k).copied().flatten())
            .find(|&x| {
                self.nodes.get(x).is_some_and(|n| !n.s.removed)
                    && self.in_order.get(x).copied().unwrap_or(false)
            })
    }

    fn out_parent_of_theirs(&self, parent: &ParentRef) -> Option<OutParent> {
        match parent {
            ParentRef::None => Some(OutParent::Root),
            ParentRef::Node(tp) => self
                .t2out
                .get(*tp)
                .copied()
                .flatten()
                .filter(|&x| self.nodes.get(x).is_some_and(|n| !n.s.removed))
                .map(OutParent::Node),
            ParentRef::External(p) => Some(OutParent::External(p.clone())),
        }
    }

    fn is_descendant(&self, x: usize, ancestor: usize) -> bool {
        let mut cur = x;
        for _ in 0..self.nodes.len() + 1 {
            match self.nodes.get(cur).map(|n| &n.parent) {
                Some(OutParent::Node(p)) => {
                    if *p == ancestor {
                        return true;
                    }
                    cur = *p;
                }
                _ => return false,
            }
        }
        false
    }

    fn block_end(&self, a: usize) -> usize {
        let Some(start) = self.order.iter().position(|&n| n == a) else {
            return self.order.len();
        };
        let mut end = start + 1;
        while let Some(&n) = self.order.get(end) {
            if !self.is_descendant(n, a) {
                break;
            }
            end += 1;
        }
        end
    }

    fn block_positions(&self, x: usize) -> Vec<usize> {
        self.order
            .iter()
            .enumerate()
            .filter(|(_, &n)| n == x || self.is_descendant(n, x))
            .map(|(p, _)| p)
            .collect()
    }

    fn place(&mut self, x: usize, anchor: Option<usize>) {
        let block: Vec<usize> = if self.in_order.get(x).copied().unwrap_or(false) {
            let block: Vec<usize> = self
                .block_positions(x)
                .iter()
                .filter_map(|&p| self.order.get(p).copied())
                .collect();
            let set: HashSet<usize> = block.iter().copied().collect();
            self.order.retain(|n| !set.contains(n));
            block
        } else {
            vec![x]
        };
        let insert_at = match anchor {
            Some(a) => self.block_end(a),
            None => match self.nodes.get(x).map(|n| n.parent.clone()) {
                Some(OutParent::Node(p)) => self
                    .order
                    .iter()
                    .position(|&n| n == p)
                    .map_or(self.order.len(), |i| i + 1),
                _ => self.order.len(),
            },
        };
        let mut insert_at = insert_at.min(self.order.len());
        let parent = self.nodes.get(x).map(|n| n.parent.clone());
        while let Some(&next) = self.order.get(insert_at) {
            let ours_only_sibling = self.nodes.get(next).is_some_and(|n| {
                Some(&n.parent) == parent.as_ref()
                    && n.theirs.is_none()
                    && n.ours.is_some_and(|oi| {
                        self.m_bo.node_b_to_a.get(oi).copied().flatten().is_none()
                    })
            });
            if !ours_only_sibling {
                break;
            }
            insert_at = self.block_end(next).max(insert_at + 1);
        }
        for &n in &block {
            if let Some(f) = self.in_order.get_mut(n) {
                *f = true;
            }
        }
        let tail = self.order.split_off(insert_at);
        self.order.extend(block);
        self.order.extend(tail);
    }

    fn import_node(&mut self, j: usize, parent: OutParent) -> Option<usize> {
        let tn = self.theirs.nodes.get(j)?;
        let mut s = self.node_sec(self.theirs, j)?.clone();
        self.import_section_refs(&mut s);
        if let Some(u) = tn.unique_id {
            if self.used_uids.contains(&u) {
                let new = self.ids.unique_node_id(&self.used_uids);
                let _ = s.header.set_field("unique_id", new.to_string());
                self.used_uids.insert(new);
                self.resolutions.push(Resolution {
                    kind: "unique_id".into(),
                    target: format!("node:{}", tn.path),
                    from: u.to_string(),
                    to: new.to_string(),
                });
            } else {
                self.used_uids.insert(u);
            }
        }
        let idx = self.nodes.len();
        self.in_order.push(false);
        self.nodes.push(OutNode {
            s: OutSec::new(s, Origin::New),
            ours: None,
            theirs: Some(j),
            name: tn.name.clone(),
            parent,
        });
        if let Some(slot) = self.t2out.get_mut(j) {
            *slot = Some(idx);
        }
        Some(idx)
    }

    fn merge_nodes(&mut self) {
        let (base, ours, theirs) = (self.base, self.ours, self.theirs);
        for (bi, _) in base.nodes.iter().enumerate() {
            let oi = self.m_bo.node_a_to_b.get(bi).copied().flatten();
            let ti = self.m_bt.node_a_to_b.get(bi).copied().flatten();
            if let (Some(oi), Some(ti)) = (oi, ti) {
                if let Some(x) = self.o2out.get(oi).copied().flatten() {
                    if let Some(n) = self.nodes.get_mut(x) {
                        n.theirs = Some(ti);
                    }
                    if let Some(slot) = self.t2out.get_mut(ti) {
                        *slot = Some(x);
                    }
                }
            }
        }
        for (j, _) in theirs.nodes.iter().enumerate() {
            if self.m_bt.node_b_to_a.get(j).copied().flatten().is_some() {
                continue;
            }
            let Some(oi) = self.m_ot.node_b_to_a.get(j).copied().flatten() else {
                continue;
            };
            let same_place = match (ours.nodes.get(oi), theirs.nodes.get(j)) {
                (Some(on), Some(tn)) => {
                    on.name == tn.name
                        && match (&on.parent, &tn.parent) {
                            (ParentRef::Node(po), ParentRef::Node(pt)) => {
                                let a = self.o2out.get(*po).copied().flatten();
                                a.is_some() && a == self.t2out.get(*pt).copied().flatten()
                            }
                            (a, b) => a == b,
                        }
                }
                _ => false,
            };
            let exact = match self.m_ot.node_how.get(oi).copied().flatten() {
                Some(scene_model::MatchHow::Path) => true,
                Some(scene_model::MatchHow::UniqueId) => same_place,
                _ => false,
            };
            if !exact || self.m_bo.node_b_to_a.get(oi).copied().flatten().is_some() {
                continue;
            }
            if let Some(x) = self.o2out.get(oi).copied().flatten() {
                if let Some(n) = self.nodes.get_mut(x) {
                    n.theirs = Some(j);
                }
                if let Some(slot) = self.t2out.get_mut(j) {
                    *slot = Some(x);
                }
            }
        }

        let mut pending_moves: Vec<(usize, ParentRef)> = Vec::new();
        for x in 0..self.nodes.len() {
            let (Some(oi), Some(ti)) = self
                .nodes
                .get(x)
                .map_or((None, None), |n| (n.ours, n.theirs))
            else {
                continue;
            };
            let bi = self.m_bo.node_b_to_a.get(oi).copied().flatten();
            if let Some(b) = bi {
                let same_text = match (self.node_sec(base, b), self.node_sec(theirs, ti)) {
                    (Some(sb), Some(st)) => sections_identical(sb, st),
                    _ => false,
                };
                let same_parent = match (
                    base.nodes.get(b).map(|n| &n.parent),
                    theirs.nodes.get(ti).map(|n| &n.parent),
                ) {
                    (Some(ParentRef::Node(pb)), Some(ParentRef::Node(pt))) => {
                        self.m_bt.node_a_to_b.get(*pb).copied().flatten() == Some(*pt)
                    }
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
                if same_text && same_parent {
                    continue;
                }
            }
            let Some(n) = self.nodes.get(x).cloned() else {
                continue;
            };
            let bs = bi.and_then(|b| self.node_sec(base, b)).cloned();
            let ts = self.node_sec(theirs, ti).cloned();
            let target = format!(
                "node:{}",
                ours.nodes.get(oi).map_or("", |n| n.path.as_str())
            );
            let mut out = n.s.clone();
            let osec = out.sec.clone();

            let (nb, no, nt) = (
                bi.and_then(|b| base.nodes.get(b)).map(|n| n.name.clone()),
                ours.nodes.get(oi).map(|n| n.name.clone()),
                theirs.nodes.get(ti).map(|n| n.name.clone()),
            );
            let mut name = n.name.clone();
            let mut header_conflicts: Vec<String> = Vec::new();
            match bi.map(|_| decide(nb == no, nb == nt, no == nt)) {
                Some(Decision::TakeTheirs) => {
                    if let Some(t) = nt.clone() {
                        name = t;
                    }
                }
                Some(Decision::Conflict) => header_conflicts.push("name".into()),
                None if no != nt => header_conflicts.push("name".into()),
                _ => {}
            }

            let parent_b = bi.and_then(|b| base.nodes.get(b)).map(|n| n.parent.clone());
            let parent_o = ours.nodes.get(oi).map(|n| n.parent.clone());
            let parent_t = theirs.nodes.get(ti).map(|n| n.parent.clone());
            let same_bo = match (&parent_b, &parent_o) {
                (Some(ParentRef::Node(pb)), Some(ParentRef::Node(po))) => {
                    self.m_bo.node_a_to_b.get(*pb).copied().flatten() == Some(*po)
                }
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            let same_bt = match (&parent_b, &parent_t) {
                (Some(ParentRef::Node(pb)), Some(ParentRef::Node(pt))) => {
                    self.m_bt.node_a_to_b.get(*pb).copied().flatten() == Some(*pt)
                }
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            let new_parent = n.parent.clone();
            if !same_bt {
                let target_parent = parent_t.as_ref().and_then(|p| self.out_parent_of_theirs(p));
                let same_ot = target_parent.as_ref() == Some(&n.parent);
                if same_ot {
                } else if same_bo {
                    if let Some(p) = parent_t.clone() {
                        pending_moves.push((x, p));
                    }
                } else {
                    header_conflicts.push("parent".into());
                }
            }

            let (fedits, fconf) = self.merge_fields(
                bs.as_ref().map(|s| &s.header),
                &osec.header,
                ts.as_ref().map(|s| &s.header),
                &["name", "parent"],
            );
            header_conflicts.extend(fconf);
            let pedits = self.merge_props(bs.as_ref(), &osec, ts.as_ref(), &target);
            self.apply(&mut out.sec, fedits);
            self.apply(&mut out.sec, pedits);
            if name != n.name {
                let _ = out
                    .sec
                    .header
                    .set_field("name", format!("\"{}\"", godot_text::escape_c(&name)));
            }
            let kind = if header_conflicts.iter().any(|k| k == "name") {
                ConflictKind::Rename
            } else if header_conflicts.iter().any(|k| k == "parent") {
                ConflictKind::Move
            } else if bi.is_none() {
                ConflictKind::AddAdd
            } else {
                ConflictKind::NodeAttribute
            };
            let th = ts.as_ref().map(|s| s.header.clone());
            let shown_conflicts: Vec<String> = header_conflicts.clone();
            let style = self.style();
            self.header_conflict(
                &mut out,
                th.as_ref(),
                &shown_conflicts,
                kind,
                target,
                bs.as_ref().map(|s| &s.header),
            );
            if style == ConflictStyle::Theirs {
                if header_conflicts.iter().any(|k| k == "name") {
                    if let Some(t) = nt {
                        name = t;
                    }
                }
                if header_conflicts.iter().any(|k| k == "parent") {
                    if let Some(p) = parent_t.clone() {
                        pending_moves.push((x, p));
                    }
                }
            }
            if let Some(slot) = self.nodes.get_mut(x) {
                slot.s = out;
                slot.name = name;
                slot.parent = new_parent;
            }
        }

        self.theirs_additions();

        for (x, pt) in pending_moves {
            let target = self.out_parent_of_theirs(&pt).filter(|p| match p {
                OutParent::Node(q) => *q != x && !self.is_descendant(*q, x),
                _ => true,
            });
            match target {
                Some(p) => {
                    if let Some(n) = self.nodes.get_mut(x) {
                        n.parent = p;
                    }
                    if let Some(j) = self.nodes.get(x).and_then(|n| n.theirs) {
                        let anchor = self.theirs_prev_sibling_out(j);
                        self.place(x, anchor);
                    }
                }
                None => {
                    let Some(mut out) = self.nodes.get(x).map(|n| n.s.clone()) else {
                        continue;
                    };
                    let th = self
                        .nodes
                        .get(x)
                        .and_then(|n| n.theirs)
                        .and_then(|j| self.node_sec(theirs, j))
                        .map(|s| s.header.clone());
                    let target = format!("node:{}", self.final_path(x));
                    self.header_conflict(
                        &mut out,
                        th.as_ref(),
                        &["parent".to_string()],
                        ConflictKind::Move,
                        target,
                        None,
                    );
                    if let Some(n) = self.nodes.get_mut(x) {
                        n.s = out;
                    }
                }
            }
        }

        self.apply_theirs_deletions();

        self.apply_theirs_reorders();
        self.check_name_collisions();
    }

    fn apply_theirs_deletions(&mut self) {
        let (base, ours) = (self.base, self.ours);
        let deleted: Vec<usize> = (0..self.nodes.len())
            .filter(|&x| {
                let Some(n) = self.nodes.get(x) else {
                    return false;
                };
                let Some(oi) = n.ours else { return false };
                n.theirs.is_none()
                    && self
                        .m_bo
                        .node_b_to_a
                        .get(oi)
                        .copied()
                        .flatten()
                        .is_some_and(|b| self.m_bt.node_a_to_b.get(b).copied().flatten().is_none())
            })
            .collect();
        let deleted_set: HashSet<usize> = deleted.iter().copied().collect();
        let roots: Vec<usize> = deleted
            .iter()
            .copied()
            .filter(|&x| match self.nodes.get(x).map(|n| &n.parent) {
                Some(OutParent::Node(p)) => !deleted_set.contains(p),
                _ => true,
            })
            .collect();
        for r in roots {
            let subtree: Vec<usize> = std::iter::once(r)
                .chain((0..self.nodes.len()).filter(|&n| self.is_descendant(n, r)))
                .collect();
            let dirty = subtree.iter().any(|&n| {
                if !deleted_set.contains(&n) {
                    return true;
                }
                let Some(oi) = self.nodes.get(n).and_then(|n| n.ours) else {
                    return true;
                };
                let Some(b) = self.m_bo.node_b_to_a.get(oi).copied().flatten() else {
                    return true;
                };
                self.node_changed(&self.m_bo, ours, oi, b)
            });
            if !dirty {
                for n in subtree {
                    if let Some(node) = self.nodes.get_mut(n) {
                        node.s.removed = true;
                    }
                }
                continue;
            }
            let path = self
                .nodes
                .get(r)
                .and_then(|n| n.ours)
                .and_then(|oi| ours.nodes.get(oi))
                .map(|n| n.path.clone())
                .unwrap_or_default();
            let resolution = self.style();
            let ci = self.add_conflict(Conflict {
                kind: ConflictKind::DeleteModify,
                target: format!("node:{path}"),
                property: None,
                base: Some("(present)".into()),
                ours: Some("(modified)".into()),
                theirs: None,
                resolution,
                line: None,
            });
            let _ = base;
            for n in subtree {
                if let Some(node) = self.nodes.get_mut(n) {
                    match resolution {
                        ConflictStyle::Ours => {}
                        ConflictStyle::Theirs => node.s.removed = true,
                        ConflictStyle::Markers => node.s.wrap = Some((ci, WrapSide::OursOnly)),
                    }
                }
            }
        }
    }

    fn theirs_additions(&mut self) {
        let (base, theirs) = (self.base, self.theirs);
        let mut handled: HashSet<usize> = HashSet::new();
        for (bi, _) in base.nodes.iter().enumerate() {
            let oi = self.m_bo.node_a_to_b.get(bi).copied().flatten();
            let ti = self.m_bt.node_a_to_b.get(bi).copied().flatten();
            let (None, Some(ti)) = (oi, ti) else { continue };
            let parent_deleted_by_ours = match base.nodes.get(bi).map(|n| &n.parent) {
                Some(ParentRef::Node(pb)) => {
                    self.m_bo.node_a_to_b.get(*pb).copied().flatten().is_none()
                }
                _ => false,
            };
            if parent_deleted_by_ours {
                continue;
            }
            let subtree: Vec<usize> = std::iter::once(ti)
                .chain((0..theirs.nodes.len()).filter(|&k| theirs_is_descendant(theirs, k, ti)))
                .collect();
            for &k in &subtree {
                handled.insert(k);
            }
            let dirty =
                subtree
                    .iter()
                    .any(|&k| match self.m_bt.node_b_to_a.get(k).copied().flatten() {
                        None => true,
                        Some(b) => {
                            self.m_bo.node_a_to_b.get(b).copied().flatten().is_none()
                                && self.node_changed(&self.m_bt, theirs, k, b)
                        }
                    });
            if !dirty {
                continue;
            }
            let path = theirs
                .nodes
                .get(ti)
                .map(|n| n.path.clone())
                .unwrap_or_default();
            let resolution = self.style();
            let ci = self.add_conflict(Conflict {
                kind: ConflictKind::DeleteModify,
                target: format!("node:{path}"),
                property: None,
                base: Some("(present)".into()),
                ours: None,
                theirs: Some("(modified)".into()),
                resolution,
                line: None,
            });
            if resolution == ConflictStyle::Ours {
                continue;
            }
            for &k in &subtree {
                if self.t2out.get(k).copied().flatten().is_some() {
                    continue;
                }
                let Some(parent) = theirs
                    .nodes
                    .get(k)
                    .and_then(|n| self.out_parent_of_theirs(&n.parent))
                else {
                    continue;
                };
                if let Some(x) = self.import_node(k, parent) {
                    let anchor = self.theirs_prev_sibling_out(k);
                    self.place(x, anchor);
                    if resolution == ConflictStyle::Markers {
                        if let Some(n) = self.nodes.get_mut(x) {
                            n.s.wrap = Some((ci, WrapSide::TheirsOnly));
                        }
                    }
                }
            }
        }
        for (j, tn) in theirs.nodes.iter().enumerate() {
            if handled.contains(&j)
                || self.t2out.get(j).copied().flatten().is_some()
                || self.m_bt.node_b_to_a.get(j).copied().flatten().is_some()
            {
                continue;
            }
            let Some(parent) = self.out_parent_of_theirs(&tn.parent) else {
                continue;
            };
            if let Some(x) = self.import_node(j, parent) {
                let anchor = self.theirs_prev_sibling_out(j);
                self.place(x, anchor);
            }
        }
    }

    fn apply_theirs_reorders(&mut self) {
        let (base, ours, theirs) = (self.base, self.ours, self.theirs);
        let in_all = |b: usize| {
            self.m_bo.node_a_to_b.get(b).copied().flatten().is_some()
                && self.m_bt.node_a_to_b.get(b).copied().flatten().is_some()
        };
        let prev_siblings = |f: &SceneFile, to_base: &dyn Fn(usize) -> Option<usize>| {
            let mut last: HashMap<&ParentRef, Option<usize>> = HashMap::new();
            let mut out = Vec::with_capacity(f.nodes.len());
            for (k, n) in f.nodes.iter().enumerate() {
                let slot = last.entry(&n.parent).or_insert(None);
                out.push(*slot);
                if let Some(b) = to_base(k).filter(|&b| in_all(b)) {
                    *slot = Some(b);
                }
            }
            out
        };
        let pb = prev_siblings(base, &|k| Some(k));
        let po = prev_siblings(ours, &|k| self.m_bo.node_b_to_a.get(k).copied().flatten());
        let pt = prev_siblings(theirs, &|k| self.m_bt.node_b_to_a.get(k).copied().flatten());
        let mut to_place = Vec::new();
        for (j, _) in theirs.nodes.iter().enumerate() {
            let Some(bi) = self.m_bt.node_b_to_a.get(j).copied().flatten() else {
                continue;
            };
            let Some(oi) = self.m_bo.node_a_to_b.get(bi).copied().flatten() else {
                continue;
            };
            let Some(x) = self.t2out.get(j).copied().flatten() else {
                continue;
            };
            let (b, o, t) = (pb.get(bi), po.get(oi), pt.get(j));
            if t != b && o == b {
                to_place.push((j, x));
            }
        }
        for (j, x) in to_place {
            let anchor = self.theirs_prev_sibling_out(j);
            self.place(x, anchor);
        }
    }

    fn check_name_collisions(&mut self) {
        let mut seen: HashMap<(OutParent, String), usize> = HashMap::new();
        let live: Vec<usize> = self
            .order
            .iter()
            .copied()
            .filter(|&x| self.nodes.get(x).is_some_and(|n| !n.s.removed))
            .collect();
        for x in live {
            let Some(n) = self.nodes.get(x).cloned() else {
                continue;
            };
            let key = (n.parent.clone(), n.name.clone());
            if let Some(&other) = seen.get(&key) {
                let ours_name = n
                    .ours
                    .and_then(|oi| self.ours.nodes.get(oi))
                    .map(|o| o.name.clone());
                let resolution = self.style();
                self.add_conflict(Conflict {
                    kind: ConflictKind::Rename,
                    target: format!("node:{}", n.name),
                    property: Some("name".into()),
                    base: None,
                    ours: ours_name.clone(),
                    theirs: Some(n.name.clone()),
                    resolution,
                    line: None,
                });
                let _ = other;
                let fallback = ours_name
                    .filter(|o| *o != n.name)
                    .unwrap_or_else(|| format!("{}2", n.name));
                if let Some(node) = self.nodes.get_mut(x) {
                    node.name = fallback.clone();
                    let _ = node
                        .s
                        .sec
                        .header
                        .set_field("name", format!("\"{}\"", godot_text::escape_c(&fallback)));
                }
                seen.insert((n.parent.clone(), fallback), x);
            } else {
                seen.insert(key, x);
            }
        }
    }

    fn final_path(&self, x: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut cur = x;
        for _ in 0..=self.nodes.len() {
            let Some(n) = self.nodes.get(cur) else { break };
            match &n.parent {
                OutParent::Root => {
                    parts.reverse();
                    return if parts.is_empty() {
                        ".".into()
                    } else {
                        parts.join("/")
                    };
                }
                OutParent::Node(p) => {
                    parts.push(n.name.clone());
                    cur = *p;
                }
                OutParent::External(path) => {
                    parts.push(n.name.clone());
                    parts.reverse();
                    return scene_model::join_path(path, &parts.join("/"));
                }
            }
        }
        parts.reverse();
        parts.join("/")
    }

    fn translate(&self, f: Which, path: &str) -> String {
        let file = match f {
            Which::Base => self.base,
            Which::Ours => self.ours,
            Which::Theirs => self.theirs,
        };
        let mut end = path.len();
        loop {
            let prefix = path.get(..end).unwrap_or("");
            if let Some(i) = file.node_by_path(prefix) {
                let out = match f {
                    Which::Ours => self.o2out.get(i).copied().flatten(),
                    Which::Theirs => self.t2out.get(i).copied().flatten(),
                    Which::Base => self
                        .m_bo
                        .node_a_to_b
                        .get(i)
                        .copied()
                        .flatten()
                        .and_then(|o| self.o2out.get(o).copied().flatten())
                        .or_else(|| {
                            self.m_bt
                                .node_a_to_b
                                .get(i)
                                .copied()
                                .flatten()
                                .and_then(|t| self.t2out.get(t).copied().flatten())
                        }),
                };
                return match out {
                    Some(x) => format!("{}{}", self.final_path(x), path.get(end..).unwrap_or("")),
                    None => path.to_string(),
                };
            }
            match prefix.rfind('/') {
                Some(k) => end = k,
                None => return path.to_string(),
            }
        }
    }

    fn conn_key(&self, f: Which, c: &scene_model::Connection) -> (String, String, String, String) {
        (
            c.signal.clone(),
            self.translate(f, &c.from),
            self.translate(f, &c.to),
            c.method.clone(),
        )
    }

    fn merge_connections(&mut self) {
        let (base, ours, theirs) = (self.base, self.ours, self.theirs);
        let keys_b: Vec<_> = base
            .connections
            .iter()
            .map(|c| self.conn_key(Which::Base, c))
            .collect();
        let keys_o: Vec<_> = ours
            .connections
            .iter()
            .map(|c| self.conn_key(Which::Ours, c))
            .collect();
        let keys_t: Vec<_> = theirs
            .connections
            .iter()
            .map(|c| self.conn_key(Which::Theirs, c))
            .collect();
        let find = |keys: &[(String, String, String, String)],
                    k: &(String, String, String, String)| {
            keys.iter().position(|x| x == k)
        };
        for (i, ko) in keys_o.iter().enumerate() {
            let Some(mut out) = self.conns.get(i).cloned() else {
                continue;
            };
            let bi = find(&keys_b, ko);
            let ti = find(&keys_t, ko);
            let bs = bi
                .and_then(|b| base.connections.get(b))
                .and_then(|c| sec(base, c.section));
            let ts = ti
                .and_then(|t| theirs.connections.get(t))
                .and_then(|c| sec(theirs, c.section));
            let target = format!("connection:{}:{}->{}::{}", ko.0, ko.1, ko.2, ko.3);
            match (bs, ts) {
                (Some(_), None) => {
                    let oh = &out.sec.header;
                    let unchanged = bs.is_some_and(|b| {
                        b.header.fields.iter().chain(oh.fields.iter()).all(|f| {
                            matches!(f.key.as_str(), "signal" | "from" | "to" | "method")
                                || opt_eq(
                                    &self.m_bo,
                                    b.header.get(&f.key),
                                    oh.get(&f.key),
                                    self.eps,
                                )
                        })
                    });
                    if unchanged {
                        out.removed = true;
                    } else {
                        let resolution = self.style();
                        let ci = self.add_conflict(Conflict {
                            kind: ConflictKind::Connection,
                            target,
                            property: None,
                            base: bs.map(|s| s.header.raw.clone()),
                            ours: Some(out.sec.header.raw.clone()),
                            theirs: None,
                            resolution,
                            line: None,
                        });
                        match resolution {
                            ConflictStyle::Ours => {}
                            ConflictStyle::Theirs => out.removed = true,
                            ConflictStyle::Markers => out.wrap = Some((ci, WrapSide::OursOnly)),
                        }
                    }
                }
                (b, Some(t)) => {
                    let bh = b.map(|s| s.header.clone());
                    let th = t.header.clone();
                    let oh = out.sec.header.clone();
                    let (edits, conflicts) = if bh.is_some() {
                        self.merge_fields(
                            bh.as_ref(),
                            &oh,
                            Some(&th),
                            &["signal", "from", "to", "method"],
                        )
                    } else {
                        let mut c = Vec::new();
                        for f in th.fields.iter().chain(oh.fields.iter()) {
                            let k = f.key.as_str();
                            if matches!(k, "signal" | "from" | "to" | "method")
                                || c.contains(&f.key)
                            {
                                continue;
                            }
                            if !opt_eq(&self.m_ot, oh.get(k), th.get(k), self.eps) {
                                c.push(f.key.clone());
                            }
                        }
                        (Vec::new(), c)
                    };
                    self.apply(&mut out.sec, edits);
                    self.header_conflict(
                        &mut out,
                        Some(&th),
                        &conflicts,
                        ConflictKind::Connection,
                        target,
                        bh.as_ref(),
                    );
                }
                (None, None) => {}
            }
            set_endpoints(&mut out.sec, &ko.1, &ko.2);
            if let Some(slot) = self.conns.get_mut(i) {
                *slot = out;
            }
        }
        for (j, kt) in keys_t.iter().enumerate() {
            if find(&keys_o, kt).is_some() {
                continue;
            }
            let Some(tc) = theirs.connections.get(j) else {
                continue;
            };
            let Some(ts) = sec(theirs, tc.section) else {
                continue;
            };
            match find(&keys_b, kt) {
                None => {
                    let mut s = ts.clone();
                    self.import_section_refs(&mut s);
                    set_endpoints(&mut s, &kt.1, &kt.2);
                    self.conns.push(OutSec::new(s, Origin::New));
                }
                Some(bi) => {
                    let Some(bs) = base.connections.get(bi).and_then(|c| sec(base, c.section))
                    else {
                        continue;
                    };
                    let changed = bs
                        .header
                        .fields
                        .iter()
                        .chain(ts.header.fields.iter())
                        .any(|f| {
                            !matches!(f.key.as_str(), "signal" | "from" | "to" | "method")
                                && !opt_eq(
                                    &self.m_bt,
                                    bs.header.get(&f.key),
                                    ts.header.get(&f.key),
                                    self.eps,
                                )
                        });
                    if !changed {
                        continue;
                    }
                    let resolution = self.style();
                    let ci = self.add_conflict(Conflict {
                        kind: ConflictKind::Connection,
                        target: format!("connection:{}:{}->{}::{}", kt.0, kt.1, kt.2, kt.3),
                        property: None,
                        base: Some(bs.header.raw.clone()),
                        ours: None,
                        theirs: Some(ts.header.raw.clone()),
                        resolution,
                        line: None,
                    });
                    if resolution == ConflictStyle::Ours {
                        continue;
                    }
                    let mut s = ts.clone();
                    self.import_section_refs(&mut s);
                    set_endpoints(&mut s, &kt.1, &kt.2);
                    let mut o = OutSec::new(s, Origin::New);
                    if resolution == ConflictStyle::Markers {
                        o.wrap = Some((ci, WrapSide::TheirsOnly));
                    }
                    self.conns.push(o);
                }
            }
        }
    }

    fn merge_editables(&mut self) {
        let (base, ours, theirs) = (self.base, self.ours, self.theirs);
        let pb: Vec<String> = base
            .editables
            .iter()
            .map(|e| self.translate(Which::Base, &e.path))
            .collect();
        let po: Vec<String> = ours
            .editables
            .iter()
            .map(|e| self.translate(Which::Ours, &e.path))
            .collect();
        let pt: Vec<String> = theirs
            .editables
            .iter()
            .map(|e| self.translate(Which::Theirs, &e.path))
            .collect();
        for (i, p) in po.iter().enumerate() {
            if let Some(e) = self.editables.get_mut(i) {
                if pb.contains(p) && !pt.contains(p) {
                    e.removed = true;
                } else if e.sec.header.get_str("path") != Some(p.as_str()) {
                    let _ = e
                        .sec
                        .header
                        .set_field("path", format!("\"{}\"", godot_text::escape_c(p)));
                }
            }
        }
        for (j, p) in pt.iter().enumerate() {
            if po.contains(p) || pb.contains(p) {
                continue;
            }
            if let Some(s) = theirs.editables.get(j).and_then(|e| sec(theirs, e.section)) {
                let mut s = s.clone();
                let _ = s
                    .header
                    .set_field("path", format!("\"{}\"", godot_text::escape_c(p)));
                s.header.raw = with_eol(&s.header.raw, self.eol);
                self.editables.push(OutSec::new(s, Origin::New));
            }
        }
    }

    fn collect_garbage(&mut self) {
        let mut ext_refs: HashSet<String> = HashSet::new();
        let mut sub_refs: HashSet<String> = HashSet::new();
        let note =
            |k: RefKind, id: &str, ext: &mut HashSet<String>, sub: &mut HashSet<String>| match k {
                RefKind::Ext => {
                    ext.insert(id.to_string());
                }
                RefKind::Sub => {
                    sub.insert(id.to_string());
                }
            };
        let scan = |s: &Section, ext: &mut HashSet<String>, sub: &mut HashSet<String>| {
            for f in &s.header.fields {
                f.value.for_each_ref(&mut |k, id| note(k, id, ext, sub));
            }
            for p in s.properties() {
                p.value.for_each_ref(&mut |k, id| note(k, id, ext, sub));
            }
        };
        let live_nodes = self.nodes.iter().filter(|n| !n.s.removed).map(|n| &n.s);
        for s in live_nodes
            .chain(self.conns.iter().filter(|c| !c.removed))
            .chain(self.resource.iter())
        {
            scan(&s.sec, &mut ext_refs, &mut sub_refs);
            if let Some((_, v)) = &s.header_conflict {
                let _ = rewrite_refs(v, |k, id| {
                    match k {
                        RefKind::Ext => ext_refs.insert(id.to_string()),
                        RefKind::Sub => sub_refs.insert(id.to_string()),
                    };
                    None
                });
            }
        }
        for (k, id) in &self.extra_refs {
            match k {
                RefKind::Ext => ext_refs.insert(id.clone()),
                RefKind::Sub => sub_refs.insert(id.clone()),
            };
        }
        let mut changed = true;
        while changed {
            changed = false;
            for s in &self.subs {
                if s.removed {
                    continue;
                }
                let id = s.id();
                if s.removable && !sub_refs.contains(&id) {
                    continue;
                }
                let before = (ext_refs.len(), sub_refs.len());
                scan(&s.sec, &mut ext_refs, &mut sub_refs);
                if !s.removable {
                    sub_refs.insert(id);
                }
                if (ext_refs.len(), sub_refs.len()) != before {
                    changed = true;
                }
            }
        }
        for s in &mut self.subs {
            if s.removable && !sub_refs.contains(&s.id()) {
                s.removed = true;
            }
        }
        for e in &mut self.exts {
            if e.removable && !ext_refs.contains(&e.id()) {
                e.removed = true;
            }
        }
    }

    fn sort_subs(&mut self) {
        let live: Vec<usize> = (0..self.subs.len())
            .filter(|&i| self.subs.get(i).is_some_and(|s| !s.removed))
            .collect();
        let index: HashMap<String, usize> = live
            .iter()
            .filter_map(|&i| self.subs.get(i).map(|s| (s.id(), i)))
            .collect();
        let deps = |i: usize| -> Vec<usize> {
            let mut out = Vec::new();
            if let Some(s) = self.subs.get(i) {
                for p in s.sec.properties() {
                    p.value.for_each_ref(&mut |k, id| {
                        if k == RefKind::Sub {
                            if let Some(&d) = index.get(id) {
                                out.push(d);
                            }
                        }
                    });
                }
            }
            out
        };
        let mut state = vec![0u8; self.subs.len()];
        let mut sorted = Vec::new();
        fn visit(
            i: usize,
            deps: &dyn Fn(usize) -> Vec<usize>,
            state: &mut [u8],
            sorted: &mut Vec<usize>,
            depth: usize,
        ) {
            if state.get(i).copied() != Some(0) || depth > 10_000 {
                return;
            }
            if let Some(s) = state.get_mut(i) {
                *s = 1;
            }
            for d in deps(i) {
                visit(d, deps, state, sorted, depth + 1);
            }
            if let Some(s) = state.get_mut(i) {
                *s = 2;
            }
            sorted.push(i);
        }
        for &i in &live {
            visit(i, &deps, &mut state, &mut sorted, 0);
        }
        let mut new_subs = Vec::with_capacity(sorted.len());
        for i in sorted {
            if let Some(s) = self.subs.get(i) {
                new_subs.push(s.clone());
            }
        }
        self.subs = new_subs;
    }

    fn finish_nodes(&mut self) {
        let order = self.order.clone();
        for x in order {
            let Some(n) = self.nodes.get(x).cloned() else {
                continue;
            };
            if n.s.removed {
                continue;
            }
            let want = match &n.parent {
                OutParent::Root => None,
                OutParent::Node(p) => Some(self.final_path(*p)),
                OutParent::External(p) => Some(p.clone()),
            };
            let have = n.s.sec.header.get_str("parent").map(str::to_string);
            let have_norm = have.as_deref().map(|h| {
                let mut s = h;
                while let Some(r) = s.strip_prefix("./") {
                    s = r;
                }
                if s.is_empty() {
                    ".".to_string()
                } else {
                    s.to_string()
                }
            });
            if want != have_norm {
                if let Some(node) = self.nodes.get_mut(x) {
                    match want {
                        Some(p) => {
                            let _ = set_header_field(
                                &mut node.s.sec.header,
                                "parent",
                                format!("\"{}\"", godot_text::escape_c(&p)),
                            );
                        }
                        None => {
                            node.s.sec.header.remove_field("parent");
                        }
                    }
                }
            }
        }
    }

    fn finish_header(&mut self) {
        let n_res = self.exts.iter().filter(|e| !e.removed).count() + self.subs.len();
        let n_ours = self.ours.ext_resources.len() + self.ours.sub_resources.len();
        if self.header.sec.header.field("load_steps").is_some() && n_res != n_ours {
            let _ = self
                .header
                .sec
                .header
                .set_field("load_steps", (n_res + 1).to_string());
        }
        let out_needs4 = needs_format4(
            self.subs
                .iter()
                .chain(self.nodes.iter().filter(|n| !n.s.removed).map(|n| &n.s))
                .chain(self.resource.iter())
                .flat_map(|s| s.sec.properties()),
        );
        let ours_needs4 = needs_format4(self.ours.doc.sections.iter().flat_map(|s| s.properties()));
        let format = match self.header.sec.header.get("format") {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(3),
            _ => 3,
        };
        if out_needs4 && !ours_needs4 && format < 4 {
            let _ = self.header.sec.header.set_field("format", "4".into());
        }
    }

    fn write(&self) -> (String, Vec<(usize, usize)>) {
        let mut seq: Vec<(&OutSec, &'static str)> = vec![(&self.header, "gd")];
        seq.extend(
            self.exts
                .iter()
                .filter(|s| !s.removed)
                .map(|s| (s, "ext_resource")),
        );
        seq.extend(
            self.subs
                .iter()
                .filter(|s| !s.removed)
                .map(|s| (s, "sub_resource")),
        );
        if let Some(r) = &self.resource {
            seq.push((r, "resource"));
        }
        for &x in &self.order {
            if let Some(n) = self.nodes.get(x) {
                if !n.s.removed {
                    seq.push((&n.s, "node"));
                }
            }
        }
        seq.extend(
            self.conns
                .iter()
                .filter(|s| !s.removed)
                .map(|s| (s, "connection")),
        );
        seq.extend(
            self.editables
                .iter()
                .filter(|s| !s.removed)
                .map(|s| (s, "editable")),
        );

        let ours_secs = &self.ours.doc.sections;
        let ours_last = ours_secs.len().saturating_sub(1);
        let final_sep = ours_secs
            .last()
            .map(|s| split_trailing(s).1)
            .unwrap_or_default();

        let successors = live_ours_successors(self);
        let mut text = String::new();
        if self.ours.doc.bom {
            text.push('\u{feff}');
        }
        text.push_str(&self.ours.doc.leading);
        let mut i = 0;
        while i < seq.len() {
            let Some(&(s, kind)) = seq.get(i) else { break };
            if let Some((ci, side)) = s.wrap {
                let mut j = i;
                let mut inner = String::new();
                while let Some(&(sj, kj)) = seq.get(j) {
                    if sj.wrap != Some((ci, side)) {
                        break;
                    }
                    let (content, _) = split_trailing(&sj.sec);
                    if !inner.is_empty() {
                        let next_sep = default_sep(kind, kj, self.eol);
                        inner.push_str(next_sep);
                    }
                    inner.push_str(content.trim_end_matches(['\r', '\n']));
                    inner.push_str(self.eol);
                    j += 1;
                }
                let inner = inner.trim_end_matches(['\r', '\n']).to_string();
                let block = match side {
                    WrapSide::OursOnly => self.block(ci, &inner, ""),
                    WrapSide::TheirsOnly => self.block(ci, "", &inner),
                };
                text.push_str(&block);
                text.push_str(self.eol);
                let next_kind = seq.get(j).map(|n| n.1);
                match next_kind {
                    Some(nk) => text.push_str(default_sep(kind, nk, self.eol)),
                    None => text.push_str(&final_sep),
                }
                i = j;
                continue;
            }
            let (mut content, own_sep) = split_trailing(&s.sec);
            if let Some((ci, variant)) = &s.header_conflict {
                let rest = content
                    .get(s.sec.header.raw.len()..)
                    .unwrap_or("")
                    .to_string();
                content = format!("{}{rest}", self.block(*ci, &s.sec.header.raw, variant));
            }
            let next = seq.get(i + 1);
            if next.is_some() && !content.ends_with('\n') {
                content.push_str(self.eol);
            }
            text.push_str(&content);
            match next {
                None => {
                    if let Origin::Ours(k) = s.origin {
                        if k == ours_last {
                            text.push_str(&own_sep);
                        } else {
                            text.push_str(&final_sep);
                        }
                    } else {
                        text.push_str(&final_sep);
                    }
                }
                Some(&(ns, nk)) => {
                    let adjacent = match (s.origin, ns.origin) {
                        (Origin::Ours(a), Origin::Ours(b)) => successors.get(&a) == Some(&b),
                        _ => false,
                    };
                    if adjacent && ns.wrap.is_none() {
                        text.push_str(&own_sep);
                    } else {
                        text.push_str(default_sep(kind, nk, self.eol));
                    }
                }
            }
            i += 1;
        }
        extract_tags(&text, self.opts.markers.size)
    }
}

fn live_ours_successors(m: &Merger<'_>) -> HashMap<usize, usize> {
    let mut live: Vec<usize> = std::iter::once(&m.header)
        .chain(m.exts.iter())
        .chain(m.subs.iter())
        .chain(m.resource.iter())
        .chain(m.nodes.iter().map(|n| &n.s))
        .chain(m.conns.iter())
        .chain(m.editables.iter())
        .filter(|s| !s.removed)
        .filter_map(|s| match s.origin {
            Origin::Ours(k) => Some(k),
            Origin::New => None,
        })
        .collect();
    live.sort_unstable();
    live.windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect()
}

fn default_sep(kind: &str, next: &str, eol: &'static str) -> &'static str {
    if kind == next && matches!(kind, "ext_resource" | "connection" | "editable") {
        ""
    } else {
        eol
    }
}

fn extract_tags(text: &str, marker_size: usize) -> (String, Vec<(usize, usize)>) {
    if !text.contains(TAG_OPEN) {
        return (text.to_string(), Vec::new());
    }
    let open = "<".repeat(marker_size);
    let mut out = String::with_capacity(text.len());
    let mut lines = Vec::new();
    for (n, line) in text.split_inclusive('\n').enumerate() {
        if line.starts_with(&open) {
            if let (Some(a), Some(b)) = (line.find(TAG_OPEN), line.find(TAG_CLOSE)) {
                if let Some(idx) = line
                    .get(a + TAG_OPEN.len_utf8()..b)
                    .and_then(|s| s.parse().ok())
                {
                    lines.push((idx, n + 1));
                }
                out.push_str(line.get(..a).unwrap_or(""));
                out.push_str(line.get(b + TAG_CLOSE.len_utf8()..).unwrap_or(""));
                continue;
            }
        }
        out.push_str(line);
    }
    (out, lines)
}

fn sections_identical(a: &Section, b: &Section) -> bool {
    a.header.raw == b.header.raw
        && a.body.len() == b.body.len()
        && a.body.iter().zip(&b.body).all(|(x, y)| match (x, y) {
            (BodyItem::Trivia(p), BodyItem::Trivia(q)) => p == q,
            (BodyItem::Property(p), BodyItem::Property(q)) => {
                p.lead == q.lead && p.raw_value == q.raw_value
            }
            _ => false,
        })
}

fn needs_format4<'p>(mut props: impl Iterator<Item = &'p godot_text::Property>) -> bool {
    props.any(|p| {
        p.raw_value.contains("PackedVector4Array") || p.raw_value.contains("PackedByteArray(\"")
    })
}

fn set_endpoints(s: &mut Section, from: &str, to: &str) {
    for (field, value) in [("from", from), ("to", to)] {
        if s.header.get_str(field) != Some(value) {
            let _ = s
                .header
                .set_field(field, format!("\"{}\"", godot_text::escape_c(value)));
        }
    }
}

#[derive(Clone, Copy)]
enum Which {
    Base,
    Ours,
    Theirs,
}

fn theirs_is_descendant(f: &SceneFile, x: usize, ancestor: usize) -> bool {
    let mut cur = x;
    for _ in 0..=f.nodes.len() {
        match f.nodes.get(cur).map(|n| &n.parent) {
            Some(ParentRef::Node(p)) => {
                if *p == ancestor {
                    return true;
                }
                cur = *p;
            }
            _ => return false,
        }
    }
    false
}

fn prev_existing_key(theirs: &Section, key: &str, ours: &Section) -> Option<String> {
    let mut prev = None;
    for p in theirs.properties() {
        if p.key == key {
            return prev;
        }
        if ours.property(&p.key).is_some() {
            prev = Some(p.key.clone());
        }
    }
    prev
}

/// Runs the merge.
pub(crate) fn run(
    base: &SceneFile,
    ours: &SceneFile,
    theirs: &SceneFile,
    opts: &MergeOptions,
) -> Result<MergeOutput, MergeError> {
    if base.kind != ours.kind || ours.kind != theirs.kind {
        return Err(MergeError::KindMismatch);
    }
    let mut m = Merger::new(base, ours, theirs, opts)?;
    m.merge_header();
    m.merge_ext_resources();
    m.merge_sub_resources();
    m.merge_resource_section();
    m.merge_nodes();
    m.merge_connections();
    m.merge_editables();
    m.finish_nodes();
    m.collect_garbage();
    m.sort_subs();
    m.finish_header();
    let (text, lines) = m.write();
    let mut conflicts = m.conflicts;
    for (idx, line) in lines {
        if let Some(c) = conflicts.get_mut(idx) {
            c.line = Some(line);
        }
    }
    Ok(MergeOutput {
        text,
        conflicts,
        resolutions: m.resolutions,
    })
}
