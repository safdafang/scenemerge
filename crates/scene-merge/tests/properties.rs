//! Property-based tests (spec §8.4), with real fixtures as the base:
//!
//! * a random single mutation is reported by `diff` as exactly that change;
//! * `merge(base, x, base) == x` byte for byte;
//! * `merge(base, x, x) == x` byte for byte;
//! * `merge(base, base, x)` is semantically equal to `x`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use godot_text::{BodyItem, Document, Header, Property, Section};
use proptest::prelude::*;
use scene_diff::{diff, Change, DiffOptions, Target};
use scene_merge::{merge, MergeOptions};
use scene_model::SceneFile;

/// Scene fixtures small enough for many iterations.
fn corpus() -> &'static Vec<String> {
    static CORPUS: OnceLock<Vec<String>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/demo");
        let mut v: Vec<(String, String)> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tscn"))
            .filter(|p| fs::metadata(p).map(|m| m.len() < 24_000).unwrap_or(false))
            .map(|p| (p.display().to_string(), fs::read_to_string(&p).unwrap()))
            .collect();
        v.sort();
        v.into_iter().map(|(_, t)| t).collect()
    })
}

#[derive(Debug, Clone)]
enum Mutation {
    ChangeProperty,
    AddProperty,
    RemoveProperty,
    RenameNode,
    DeleteLeaf,
    AddNode,
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        Just(Mutation::ChangeProperty),
        Just(Mutation::AddProperty),
        Just(Mutation::RemoveProperty),
        Just(Mutation::RenameNode),
        Just(Mutation::DeleteLeaf),
        Just(Mutation::AddNode),
    ]
}

/// What the mutation should look like in the diff.
#[derive(Debug)]
enum Expect {
    PropChanged(String, String),
    PropAdded(String, String),
    PropRemoved(String, String),
    Renamed(String, String),
    Removed(String),
    Added(String),
}

fn node_sections(doc: &Document) -> Vec<usize> {
    doc.sections
        .iter()
        .enumerate()
        .filter(|(_, s)| s.header.name == "node")
        .map(|(i, _)| i)
        .collect()
}

fn pick<T: Clone>(v: &[T], r: u64) -> Option<T> {
    if v.is_empty() {
        None
    } else {
        Some(v[(r % v.len() as u64) as usize].clone())
    }
}

/// Applies a mutation to `text`; returns the new text and the expected change,
/// or `None` when the mutation does not apply to this file.
fn apply(text: &str, m: &Mutation, r1: u64, r2: u64) -> Option<(String, Expect)> {
    let model = SceneFile::parse(text).ok()?;
    let mut doc = model.doc.clone();
    let nodes = node_sections(&doc);
    let node_index = |section: usize| model.nodes.iter().position(|n| n.section == section);
    // Nodes that are referenced by path from somewhere else are not renamed/deleted.
    let referenced = |path: &str| {
        model.connections.iter().any(|c| {
            c.from == path
                || c.to == path
                || c.from.starts_with(&format!("{path}/"))
                || c.to.starts_with(&format!("{path}/"))
        }) || model
            .editables
            .iter()
            .any(|e| e.path == path || e.path.starts_with(&format!("{path}/")))
            || model.nodes.iter().any(|n| n.owner.as_deref() == Some(path))
    };
    match m {
        Mutation::ChangeProperty | Mutation::RemoveProperty => {
            let candidates: Vec<(usize, String)> = nodes
                .iter()
                .flat_map(|&s| {
                    doc.sections[s]
                        .properties()
                        .map(move |p| (s, p.key.clone()))
                        .collect::<Vec<_>>()
                })
                .collect();
            let (s, key) = pick(&candidates, r1)?;
            let path = model.nodes[node_index(s)?].path.clone();
            // Duplicate keys make "the" property ambiguous.
            if doc.sections[s]
                .properties()
                .filter(|p| p.key == key)
                .count()
                != 1
            {
                return None;
            }
            let sec = &mut doc.sections[s];
            if let Mutation::ChangeProperty = m {
                let new = format!("{}", 1_000_000 + (r2 % 1_000_000));
                for p in sec.properties_mut() {
                    if p.key == key {
                        if p.raw_value == new {
                            return None;
                        }
                        p.set_raw_value(new.clone()).unwrap();
                    }
                }
                Some((doc.to_text(), Expect::PropChanged(path, key)))
            } else {
                let i = sec
                    .body
                    .iter()
                    .position(|b| matches!(b, BodyItem::Property(p) if p.key == key))?;
                sec.body.remove(i);
                if matches!(sec.body.get(i), Some(BodyItem::Trivia(_))) && i > 0 {
                    // keep the line structure valid: drop the line break
                    sec.body.remove(i - 1);
                }
                Some((doc.to_text(), Expect::PropRemoved(path, key)))
            }
        }
        Mutation::AddProperty => {
            let s = pick(&nodes, r1)?;
            let path = model.nodes[node_index(s)?].path.clone();
            let key = format!("metadata/prop_test_{}", r2 % 1000);
            let sec = &mut doc.sections[s];
            if sec.property(&key).is_some() {
                return None;
            }
            sec.body.insert(
                0,
                BodyItem::Property(Property::new(&key, "42".into()).unwrap()),
            );
            sec.body.insert(0, BodyItem::Trivia("\n".into()));
            Some((doc.to_text(), Expect::PropAdded(path, key)))
        }
        Mutation::RenameNode => {
            let leaves: Vec<usize> = nodes
                .iter()
                .copied()
                .filter(|&s| {
                    let Some(i) = node_index(s) else { return false };
                    i > 0 && model.children_of(i).is_empty() && !referenced(&model.nodes[i].path)
                })
                .collect();
            let s = pick(&leaves, r1)?;
            let i = node_index(s)?;
            let old = model.nodes[i].path.clone();
            let new_name = format!("Renamed{}", r2 % 10_000);
            let new_path = scene_model::join_path(
                model.nodes[i].parent_path.as_deref().unwrap_or("."),
                &new_name,
            );
            if model.node_by_path(&new_path).is_some() {
                return None;
            }
            doc.sections[s]
                .header
                .set_field("name", format!("\"{new_name}\""))
                .unwrap();
            Some((doc.to_text(), Expect::Renamed(old, new_name)))
        }
        Mutation::DeleteLeaf => {
            let leaves: Vec<usize> = nodes
                .iter()
                .copied()
                .filter(|&s| {
                    let Some(i) = node_index(s) else { return false };
                    i > 0 && model.children_of(i).is_empty() && !referenced(&model.nodes[i].path)
                })
                .collect();
            let s = pick(&leaves, r1)?;
            let path = model.nodes[node_index(s)?].path.clone();
            // The previous section keeps its own separator; make sure the file
            // still ends with a line break.
            doc.sections.remove(s);
            let mut t = doc.to_text();
            if !t.ends_with('\n') {
                t.push('\n');
            }
            Some((t, Expect::Removed(path)))
        }
        Mutation::AddNode => {
            let s = pick(&nodes, r1)?;
            let i = node_index(s)?;
            let parent = model.nodes[i].path.clone();
            let name = format!("Added{}", r2 % 10_000);
            let path = scene_model::join_path(&parent, &name);
            if model.node_by_path(&path).is_some() {
                return None;
            }
            let header = Header::from_raw_fields(
                "node",
                [
                    ("name", format!("\"{name}\"")),
                    ("type", "\"Node\"".to_string()),
                    ("parent", format!("\"{parent}\"")),
                ],
            )
            .unwrap();
            let mut sec = Section::new(header);
            sec.body.push(BodyItem::Trivia("\n\n".into()));
            // Insert after the last node section.
            let last_node = *nodes.last()?;
            let (before, after) = doc.sections.split_at(last_node + 1);
            let mut sections = before.to_vec();
            // Ensure a blank line before the new section.
            let mut prev_text = sections.last()?.to_text();
            if !prev_text.ends_with("\n\n") {
                if let Some(last) = sections.last_mut() {
                    last.body.push(BodyItem::Trivia(
                        if prev_text.ends_with('\n') {
                            "\n"
                        } else {
                            "\n\n"
                        }
                        .into(),
                    ));
                    prev_text.push('\n');
                }
            }
            sections.push(sec);
            sections.extend_from_slice(after);
            doc.sections = sections;
            Some((doc.to_text(), Expect::Added(path)))
        }
    }
}

fn check_diff(base: &SceneFile, x: &SceneFile, expect: &Expect) -> Result<(), TestCaseError> {
    let d = diff(base, x, &DiffOptions::default());
    let ok = match (expect, d.changes.as_slice()) {
        (
            Expect::PropChanged(p, k),
            [Change::PropertyChanged {
                target: Target::Node { path },
                property,
                ..
            }],
        )
        | (
            Expect::PropAdded(p, k),
            [Change::PropertyAdded {
                target: Target::Node { path },
                property,
                ..
            }],
        )
        | (
            Expect::PropRemoved(p, k),
            [Change::PropertyRemoved {
                target: Target::Node { path },
                property,
                ..
            }],
        ) => path == p && property == k,
        (Expect::Renamed(p, n), [Change::NodeRenamed { path, new_name, .. }]) => {
            path == p && new_name == n
        }
        (Expect::Removed(p), [Change::NodeRemoved { path, .. }]) => path == p,
        (Expect::Added(p), [Change::NodeAdded { path, .. }]) => path == p,
        _ => false,
    };
    prop_assert!(ok, "expected {expect:?}, got {:#?}", d.changes);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

    #[test]
    fn single_mutation_is_reported_exactly(file in 0usize..1000, m in mutation(), r1 in any::<u64>(), r2 in any::<u64>()) {
        let texts = corpus();
        let text = &texts[file % texts.len()];
        let Some((mutated, expect)) = apply(text, &m, r1, r2) else { return Ok(()) };
        let base = SceneFile::parse(text).unwrap();
        let x = SceneFile::parse(&mutated).unwrap_or_else(|e| panic!("{e}\n{mutated}"));
        check_diff(&base, &x, &expect)?;
    }

    #[test]
    fn merge_identities(file in 0usize..1000, m in mutation(), r1 in any::<u64>(), r2 in any::<u64>()) {
        let texts = corpus();
        let text = &texts[file % texts.len()];
        let Some((mutated, _)) = apply(text, &m, r1, r2) else { return Ok(()) };
        let base = SceneFile::parse(text).unwrap();
        let x = SceneFile::parse(&mutated).unwrap();
        let opts = MergeOptions::default();

        let a = merge(&base, &x, &base, &opts).unwrap();
        prop_assert!(a.is_clean());
        prop_assert_eq!(&a.text, &mutated, "merge(base, x, base) != x");

        let b = merge(&base, &x, &x, &opts).unwrap();
        prop_assert!(b.is_clean());
        prop_assert_eq!(&b.text, &mutated, "merge(base, x, x) != x");

        let c = merge(&base, &base, &x, &opts).unwrap();
        prop_assert!(c.is_clean());
        let merged = SceneFile::parse(&c.text).unwrap();
        let d = diff(&x, &merged, &DiffOptions::default());
        prop_assert!(d.is_empty(), "merge(base, base, x) differs from x: {:#?}\n{}", d.changes, c.text);
    }

    #[test]
    fn independent_mutations_merge_cleanly(
        file in 0usize..1000, m1 in mutation(), m2 in mutation(),
        r1 in any::<u64>(), r2 in any::<u64>(), r3 in any::<u64>(), r4 in any::<u64>(),
    ) {
        // Two mutations of the same base on two branches merge into a file
        // that contains both (when they touch different things).
        let texts = corpus();
        let text = &texts[file % texts.len()];
        let Some((ours_text, e1)) = apply(text, &m1, r1, r2) else { return Ok(()) };
        let Some((theirs_text, e2)) = apply(text, &m2, r3, r4) else { return Ok(()) };
        let target = |e: &Expect| match e {
            Expect::PropChanged(p, _) | Expect::PropAdded(p, _) | Expect::PropRemoved(p, _)
            | Expect::Renamed(p, _) | Expect::Removed(p) | Expect::Added(p) => p.clone(),
        };
        let (t1, t2) = (target(&e1), target(&e2));
        // Skip overlapping edits (same node, or one inside the other).
        if t1 == t2 || t1.starts_with(&format!("{t2}/")) || t2.starts_with(&format!("{t1}/"))
            || t1 == "." || t2 == "." {
            return Ok(());
        }
        let base = SceneFile::parse(text).unwrap();
        let ours = SceneFile::parse(&ours_text).unwrap();
        let theirs = SceneFile::parse(&theirs_text).unwrap();
        let out = merge(&base, &ours, &theirs, &MergeOptions::default()).unwrap();
        prop_assert!(out.is_clean(), "conflicts: {:?}", out.conflicts);
        let merged = SceneFile::parse(&out.text).unwrap();
        // Everything theirs changed is in the result relative to ours...
        let d_theirs = diff(&base, &theirs, &DiffOptions::default());
        let d_result = diff(&ours, &merged, &DiffOptions::default());
        prop_assert_eq!(d_theirs.changes.len(), d_result.changes.len(),
            "theirs: {:#?}\nresult vs ours: {:#?}\n{}", d_theirs.changes, d_result.changes, out.text);
        // ...and everything ours changed is in the result relative to theirs.
        let d_ours = diff(&base, &ours, &DiffOptions::default());
        let d_result2 = diff(&theirs, &merged, &DiffOptions::default());
        prop_assert_eq!(d_ours.changes.len(), d_result2.changes.len(),
            "ours: {:#?}\nresult vs theirs: {:#?}", d_ours.changes, d_result2.changes);
    }
}

#[test]
fn mutations_apply_to_most_fixtures() {
    // Guard against the property tests silently skipping most inputs.
    let texts = corpus();
    assert!(texts.len() >= 50, "{}", texts.len());
    for m in [
        Mutation::ChangeProperty,
        Mutation::AddProperty,
        Mutation::RemoveProperty,
        Mutation::RenameNode,
        Mutation::DeleteLeaf,
        Mutation::AddNode,
    ] {
        let applied = texts
            .iter()
            .filter(|t| (0..4u64).any(|seed| apply(t, &m, seed * 7919, seed * 104_729).is_some()))
            .count();
        assert!(
            applied * 10 >= texts.len() * 7,
            "{m:?} applies to only {applied} of {} fixtures",
            texts.len()
        );
    }
}
