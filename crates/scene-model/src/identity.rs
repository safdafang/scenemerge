//! Identity matching between two versions of a file, and normalisation of
//! values so that they can be compared across versions.
//!
//! Rules (see the repository's `DECISIONS.md`):
//!
//! * **ext_resource**: same `uid`; otherwise same `path`. The local `id` is
//!   only a label inside one file, so renumbering is invisible.
//! * **sub_resource**: same `id`; otherwise same `type` and equal normalised
//!   content.
//! * **node**: same `unique_id` (when present on both sides and unique);
//!   otherwise same name under the matched parent (i.e. same path, but robust to
//!   an ancestor being renamed); otherwise the rename/move heuristic: same
//!   `type` and `instance` and a similarity score at or above
//!   [`MatchOptions::rename_threshold`].
//!
//! Values are normalised by replacing every `ExtResource("id")` /
//! `SubResource("id")` with a token naming the *matched entity*, so that
//! `ExtResource("1_abc")` in one version equals `ExtResource("3_xyz")` in the
//! other when both refer to the same file.

use std::collections::HashMap;

use godot_text::{RefKind, Value};

use crate::model::{Node, ParentRef, Prop, SceneFile};

/// Options that influence matching and comparison.
#[derive(Debug, Clone)]
pub struct MatchOptions {
    /// Minimum similarity (0..=1) for an unmatched removed node and an
    /// unmatched added node to be reported as a rename/move. Default `0.65`.
    pub rename_threshold: f64,
    /// Absolute tolerance for float comparison. Default `0.0` (exact).
    pub epsilon: f64,
}

impl Default for MatchOptions {
    fn default() -> Self {
        MatchOptions {
            rename_threshold: 0.65,
            epsilon: 0.0,
        }
    }
}

/// How a node pair was matched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchHow {
    /// Same `unique_id`.
    UniqueId,
    /// Same name under the matched parent.
    Path,
    /// Rename/move heuristic, with the similarity score.
    Heuristic(f64),
}

/// The correspondence between entities of file A and file B.
#[derive(Debug, Clone, Default)]
pub struct Matching {
    /// For each ext-resource of A, its counterpart in B.
    pub ext_a_to_b: Vec<Option<usize>>,
    /// For each ext-resource of B, its counterpart in A.
    pub ext_b_to_a: Vec<Option<usize>>,
    /// For each sub-resource of A, its counterpart in B.
    pub sub_a_to_b: Vec<Option<usize>>,
    /// For each sub-resource of B, its counterpart in A.
    pub sub_b_to_a: Vec<Option<usize>>,
    /// For each node of A, its counterpart in B.
    pub node_a_to_b: Vec<Option<usize>>,
    /// For each node of B, its counterpart in A.
    pub node_b_to_a: Vec<Option<usize>>,
    /// For each node of A that is matched, how.
    pub node_how: Vec<Option<MatchHow>>,
    ext_ids_a: HashMap<String, usize>,
    ext_ids_b: HashMap<String, usize>,
    sub_ids_a: HashMap<String, usize>,
    sub_ids_b: HashMap<String, usize>,
}

/// Which side of a [`Matching`] a value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The first file.
    A,
    /// The second file.
    B,
}

fn id_index<'a>(ids: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    // Later definitions win, as in Godot's loader.
    ids.enumerate().map(|(i, id)| (id.to_string(), i)).collect()
}

impl Matching {
    /// Computes the correspondence between `a` and `b`.
    pub fn compute(a: &SceneFile, b: &SceneFile, opts: &MatchOptions) -> Matching {
        let mut m = Matching {
            ext_a_to_b: vec![None; a.ext_resources.len()],
            ext_b_to_a: vec![None; b.ext_resources.len()],
            sub_a_to_b: vec![None; a.sub_resources.len()],
            sub_b_to_a: vec![None; b.sub_resources.len()],
            node_a_to_b: vec![None; a.nodes.len()],
            node_b_to_a: vec![None; b.nodes.len()],
            node_how: vec![None; a.nodes.len()],
            ext_ids_a: id_index(a.ext_resources.iter().map(|e| e.id.as_str())),
            ext_ids_b: id_index(b.ext_resources.iter().map(|e| e.id.as_str())),
            sub_ids_a: id_index(a.sub_resources.iter().map(|s| s.id.as_str())),
            sub_ids_b: id_index(b.sub_resources.iter().map(|s| s.id.as_str())),
        };
        m.match_ext(a, b);
        m.match_sub(a, b, opts);
        m.match_nodes(a, b, opts);
        m
    }

    fn match_ext(&mut self, a: &SceneFile, b: &SceneFile) {
        let mut by_uid: HashMap<&str, usize> = HashMap::new();
        for (j, e) in b.ext_resources.iter().enumerate() {
            if let Some(u) = e.uid.as_deref() {
                by_uid.entry(u).or_insert(j);
            }
        }
        for (i, e) in a.ext_resources.iter().enumerate() {
            if let Some(&j) = e.uid.as_deref().and_then(|u| by_uid.get(u)) {
                if self.ext_b_to_a.get(j).copied().flatten().is_none() {
                    self.link_ext(i, j);
                }
            }
        }
        let mut by_path: HashMap<&str, Vec<usize>> = HashMap::new();
        for (j, e) in b.ext_resources.iter().enumerate() {
            if self.ext_b_to_a.get(j).copied().flatten().is_none() {
                by_path.entry(e.path.as_str()).or_default().push(j);
            }
        }
        for (i, e) in a.ext_resources.iter().enumerate() {
            if self.ext_a_to_b.get(i).copied().flatten().is_some() {
                continue;
            }
            if let Some(list) = by_path.get_mut(e.path.as_str()) {
                if !list.is_empty() {
                    let j = list.remove(0);
                    self.link_ext(i, j);
                }
            }
        }
    }

    fn link_ext(&mut self, i: usize, j: usize) {
        if let Some(s) = self.ext_a_to_b.get_mut(i) {
            *s = Some(j);
        }
        if let Some(s) = self.ext_b_to_a.get_mut(j) {
            *s = Some(i);
        }
    }

    fn link_sub(&mut self, i: usize, j: usize) {
        if let Some(s) = self.sub_a_to_b.get_mut(i) {
            *s = Some(j);
        }
        if let Some(s) = self.sub_b_to_a.get_mut(j) {
            *s = Some(i);
        }
    }

    fn link_node(&mut self, i: usize, j: usize, how: MatchHow) {
        if let Some(s) = self.node_a_to_b.get_mut(i) {
            *s = Some(j);
        }
        if let Some(s) = self.node_b_to_a.get_mut(j) {
            *s = Some(i);
        }
        if let Some(s) = self.node_how.get_mut(i) {
            *s = Some(how);
        }
    }

    fn match_sub(&mut self, a: &SceneFile, b: &SceneFile, opts: &MatchOptions) {
        // 1. Same id.
        for (j, s) in b.sub_resources.iter().enumerate() {
            if let Some(&i) = self.sub_ids_a.get(&s.id) {
                if self.sub_a_to_b.get(i).copied().flatten().is_none() {
                    self.link_sub(i, j);
                }
            }
        }
        // 2. Same type and equal normalised content (in file order, so that
        //    references to earlier sub-resources are already resolved).
        for (j, s) in b.sub_resources.iter().enumerate() {
            if self.sub_b_to_a.get(j).copied().flatten().is_some() {
                continue;
            }
            let found = a.sub_resources.iter().enumerate().find(|(i, sa)| {
                self.sub_a_to_b.get(*i).copied().flatten().is_none()
                    && sa.type_name == s.type_name
                    && self.props_equal(&sa.props, &s.props, opts.epsilon)
            });
            if let Some((i, _)) = found {
                self.link_sub(i, j);
            }
        }
    }

    /// Whether two property lists (A side, B side) are equal after normalisation.
    pub fn props_equal(&self, pa: &[Prop], pb: &[Prop], epsilon: f64) -> bool {
        pa.len() == pb.len()
            && pa.iter().all(|x| {
                pb.iter().find(|y| y.key == x.key).is_some_and(|y| {
                    self.normalize(Side::A, &x.value)
                        .semantic_eq(&self.normalize(Side::B, &y.value), epsilon)
                })
            })
    }

    /// Replaces local resource ids by tokens of the matched entities, so that
    /// values from A and B can be compared with [`Value::semantic_eq`].
    pub fn normalize(&self, side: Side, v: &Value) -> Value {
        v.map_refs(&mut |kind, id| {
            let token = match (side, kind) {
                (Side::A, RefKind::Ext) => match self.ext_ids_a.get(id) {
                    Some(i) => format!("#{i}"),
                    None => format!("?{id}"),
                },
                (Side::B, RefKind::Ext) => match self.ext_ids_b.get(id) {
                    Some(&j) => match self.ext_b_to_a.get(j).copied().flatten() {
                        Some(i) => format!("#{i}"),
                        None => format!("#b{j}"),
                    },
                    None => format!("?{id}"),
                },
                (Side::A, RefKind::Sub) => match self.sub_ids_a.get(id) {
                    Some(i) => format!("#{i}"),
                    None => format!("?{id}"),
                },
                (Side::B, RefKind::Sub) => match self.sub_ids_b.get(id) {
                    Some(&j) => match self.sub_b_to_a.get(j).copied().flatten() {
                        Some(i) => format!("#{i}"),
                        None => format!("#b{j}"),
                    },
                    None => format!("?{id}"),
                },
            };
            match kind {
                RefKind::Ext => Value::ExtResource(token),
                RefKind::Sub => Value::SubResource(token),
            }
        })
    }

    /// Whether value `va` (from A) equals `vb` (from B) after normalisation.
    pub fn values_equal(&self, va: &Value, vb: &Value, epsilon: f64) -> bool {
        self.normalize(Side::A, va)
            .semantic_eq(&self.normalize(Side::B, vb), epsilon)
    }

    fn match_nodes(&mut self, a: &SceneFile, b: &SceneFile, opts: &MatchOptions) {
        // 1. unique_id, when unique on both sides.
        let uid_index = |f: &SceneFile| {
            let mut map: HashMap<i64, Option<usize>> = HashMap::new();
            for (i, n) in f.nodes.iter().enumerate() {
                if let Some(u) = n.unique_id {
                    map.entry(u)
                        .and_modify(|e| *e = None) // duplicate → unusable
                        .or_insert(Some(i));
                }
            }
            map
        };
        let ua = uid_index(a);
        let ub = uid_index(b);
        for (u, ia) in &ua {
            if let (Some(i), Some(Some(j))) = (ia, ub.get(u)) {
                self.link_node(*i, *j, MatchHow::UniqueId);
            }
        }
        // 2. The roots always correspond.
        if let (Some(_), Some(_)) = (a.nodes.first(), b.nodes.first()) {
            if self.node_a_to_b.first().copied().flatten().is_none()
                && self.node_b_to_a.first().copied().flatten().is_none()
            {
                self.link_node(0, 0, MatchHow::Path);
            }
        }
        // 3./4. Alternate the path pass and the heuristic until stable.
        for _round in 0..8 {
            self.path_pass(a, b);
            if self.heuristic_pass(a, b, opts, true) > 0 {
                continue;
            }
            if self.heuristic_pass(a, b, opts, false) == 0 {
                break;
            }
        }
    }

    /// Key of a node's parent in B's coordinates, for path matching.
    fn parent_key_in_b(&self, parent: &ParentRef) -> Option<ParentKey> {
        match parent {
            ParentRef::None => None,
            ParentRef::Node(pa) => self
                .node_a_to_b
                .get(*pa)
                .copied()
                .flatten()
                .map(ParentKey::Node),
            ParentRef::External(p) => Some(ParentKey::External(p.clone())),
        }
    }

    fn path_pass(&mut self, a: &SceneFile, b: &SceneFile) {
        let mut index: HashMap<(ParentKey, &str), usize> = HashMap::new();
        for (j, n) in b.nodes.iter().enumerate() {
            if self.node_b_to_a.get(j).copied().flatten().is_some() {
                continue;
            }
            let key = match &n.parent {
                ParentRef::None => continue,
                ParentRef::Node(p) => ParentKey::Node(*p),
                ParentRef::External(p) => ParentKey::External(p.clone()),
            };
            index.insert((key, n.name.as_str()), j);
        }
        // A is in file order, so parents are matched before their children.
        for (i, n) in a.nodes.iter().enumerate() {
            if self.node_a_to_b.get(i).copied().flatten().is_some() {
                continue;
            }
            let Some(pk) = self.parent_key_in_b(&n.parent) else {
                continue;
            };
            if let Some(j) = index.remove(&(pk, n.name.as_str())) {
                self.link_node(i, j, MatchHow::Path);
            }
        }
    }

    /// Matches remaining nodes by similarity. With `frontier_only`, only pairs
    /// whose parents are both already matched (renames, moves between existing
    /// parents) are considered. Returns the number of new matches.
    fn heuristic_pass(
        &mut self,
        a: &SceneFile,
        b: &SceneFile,
        opts: &MatchOptions,
        frontier_only: bool,
    ) -> usize {
        let parent_matched_a = |m: &Matching, n: &Node| match &n.parent {
            ParentRef::Node(p) => m.node_a_to_b.get(*p).copied().flatten().is_some(),
            _ => true,
        };
        let parent_matched_b = |m: &Matching, n: &Node| match &n.parent {
            ParentRef::Node(p) => m.node_b_to_a.get(*p).copied().flatten().is_some(),
            _ => true,
        };
        let removed: Vec<usize> = (0..a.nodes.len())
            .filter(|&i| self.node_a_to_b.get(i).copied().flatten().is_none())
            .filter(|&i| {
                !frontier_only || a.nodes.get(i).is_some_and(|n| parent_matched_a(self, n))
            })
            .collect();
        let added: Vec<usize> = (0..b.nodes.len())
            .filter(|&j| self.node_b_to_a.get(j).copied().flatten().is_none())
            .filter(|&j| {
                !frontier_only || b.nodes.get(j).is_some_and(|n| parent_matched_b(self, n))
            })
            .collect();
        if removed.is_empty() || added.is_empty() {
            return 0;
        }
        // Bucket by (type, instance) to keep the pair count low.
        let bucket_key = |m: &Matching, side: Side, n: &Node| {
            let inst = n
                .instance
                .as_ref()
                .map(|v| m.normalize(side, v).to_string())
                .unwrap_or_default();
            (n.type_name.clone().unwrap_or_default(), inst)
        };
        let mut buckets: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for &j in &added {
            if let Some(n) = b.nodes.get(j) {
                buckets
                    .entry(bucket_key(self, Side::B, n))
                    .or_default()
                    .push(j);
            }
        }
        let children_a = child_names(a);
        let children_b = child_names(b);
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        const MAX_PAIRS: usize = 250_000;
        for &i in &removed {
            let Some(na) = a.nodes.get(i) else { continue };
            let Some(cands) = buckets.get(&bucket_key(self, Side::A, na)) else {
                continue;
            };
            for &j in cands {
                let Some(nb) = b.nodes.get(j) else { continue };
                let score = self.similarity(
                    na,
                    nb,
                    children_a.get(&i).map_or(&[][..], Vec::as_slice),
                    children_b.get(&j).map_or(&[][..], Vec::as_slice),
                    opts.epsilon,
                );
                if score >= opts.rename_threshold {
                    pairs.push((score, i, j));
                }
                if pairs.len() > MAX_PAIRS {
                    break;
                }
            }
        }
        // Greedy, best score first; ties broken by file order for determinism.
        pairs.sort_by(|x, y| {
            y.0.partial_cmp(&x.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(x.1.cmp(&y.1))
                .then(x.2.cmp(&y.2))
        });
        let mut made = 0;
        for (score, i, j) in pairs {
            if self.node_a_to_b.get(i).copied().flatten().is_none()
                && self.node_b_to_a.get(j).copied().flatten().is_none()
            {
                self.link_node(i, j, MatchHow::Heuristic(score));
                made += 1;
            }
        }
        made
    }

    /// Similarity of node `na` (A) and `nb` (B) in `0..=1`:
    /// `0.6 * content + 0.25 * same parent + 0.15 * same name`, where content
    /// combines property similarity (same value 1, same key 0.5) and the
    /// overlap of child names.
    fn similarity(&self, na: &Node, nb: &Node, ca: &[&str], cb: &[&str], epsilon: f64) -> f64 {
        let mut keys: Vec<&str> = na.props.iter().map(|p| p.key.as_str()).collect();
        for p in &nb.props {
            if !keys.contains(&p.key.as_str()) {
                keys.push(p.key.as_str());
            }
        }
        let mut prop_score = 0.0;
        for k in &keys {
            let pa = na.props.iter().find(|p| p.key == *k);
            let pb = nb.props.iter().find(|p| p.key == *k);
            if let (Some(pa), Some(pb)) = (pa, pb) {
                prop_score += if self.values_equal(&pa.value, &pb.value, epsilon) {
                    1.0
                } else {
                    0.5
                };
            }
        }
        let mut child_union: Vec<&str> = ca.to_vec();
        for c in cb {
            if !child_union.contains(c) {
                child_union.push(c);
            }
        }
        let child_common = ca.iter().filter(|c| cb.contains(c)).count() as f64;
        let total = keys.len() + child_union.len();
        let content = if total == 0 {
            1.0
        } else {
            (prop_score + child_common) / total as f64
        };
        let same_parent = match (&na.parent, &nb.parent) {
            (ParentRef::Node(pa), ParentRef::Node(pb)) => {
                self.node_a_to_b.get(*pa).copied().flatten() == Some(*pb)
            }
            (ParentRef::External(x), ParentRef::External(y)) => x == y,
            _ => false,
        };
        0.6 * content
            + if same_parent { 0.25 } else { 0.0 }
            + if na.name == nb.name { 0.15 } else { 0.0 }
    }
}

fn child_names(f: &SceneFile) -> HashMap<usize, Vec<&str>> {
    let mut map: HashMap<usize, Vec<&str>> = HashMap::new();
    for n in &f.nodes {
        if let ParentRef::Node(p) = n.parent {
            map.entry(p).or_default().push(n.name.as_str());
        }
    }
    map
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ParentKey {
    Node(usize),
    External(String),
}
