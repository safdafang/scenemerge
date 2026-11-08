//! Snapshot tests of the human and JSON diff output.
//!
//! Each directory in `tests/data/` holds `old.*` and `new.*`. Review changed
//! snapshots with `cargo insta review` (or `INSTA_UPDATE=always cargo test`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::{Path, PathBuf};

use scene_diff::{canonical_text, diff, render_human, to_json, DiffOptions, HumanOptions};
use scene_model::SceneFile;

fn case_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn load(dir: &Path, stem: &str) -> SceneFile {
    for ext in ["tscn", "tres"] {
        let p = dir.join(format!("{stem}.{ext}"));
        if p.exists() {
            return SceneFile::parse_bytes(&fs::read(&p).unwrap())
                .unwrap_or_else(|e| panic!("{}", e.with_path(&p)));
        }
    }
    panic!("missing {stem} in {}", dir.display());
}

fn root(f: &SceneFile) -> String {
    f.nodes.first().map_or(".".into(), |n| n.name.clone())
}

fn run(name: &str) -> (String, String) {
    let dir = case_dir(name);
    let (a, b) = (load(&dir, "old"), load(&dir, "new"));
    let d = diff(&a, &b, &DiffOptions::default());
    let human = render_human(
        name,
        &d,
        &HumanOptions {
            old_root: root(&a),
            new_root: root(&b),
            ..HumanOptions::default()
        },
    );
    let json = to_json("old", "new", &d);
    (human, json)
}

macro_rules! snapshot_case {
    ($fn_name:ident, $case:literal) => {
        #[test]
        fn $fn_name() {
            let (human, json) = run($case);
            insta::assert_snapshot!(concat!($case, "-human"), human);
            insta::assert_snapshot!(concat!($case, "-json"), json);
        }
    };
}

snapshot_case!(showcase, "showcase");
snapshot_case!(signals_groups_order, "signals-groups-order");
snapshot_case!(sub_resources, "sub-resources");
snapshot_case!(resource_file, "resource-file");
snapshot_case!(inherited_editable, "inherited-editable");

#[test]
fn editor_noise_produces_no_changes() {
    let dir = case_dir("editor-noise");
    let (a, b) = (load(&dir, "old"), load(&dir, "new"));
    let d = diff(&a, &b, &DiffOptions::default());
    assert!(d.is_empty(), "{:#?}", d.changes);
}

#[test]
fn canonical_text_hides_editor_noise() {
    // Same scene: ext ids renumbered and reordered, load_steps dropped,
    // unique ids added, CRLF, odd spacing, `0` spelled `0.0`.
    let a = concat!(
        "[gd_scene load_steps=3 format=3 uid=\"uid://x\"]\n\n",
        "[ext_resource type=\"Script\" path=\"res://a.gd\" id=\"1_aaaaa\"]\n",
        "[ext_resource type=\"Texture2D\" path=\"res://b.png\" id=\"2_bbbbb\"]\n\n",
        "[node name=\"Root\" type=\"Sprite2D\"]\n",
        "script = ExtResource(\"1_aaaaa\")\n",
        "texture = ExtResource(\"2_bbbbb\")\n",
        "offset = Vector2(0, 1.5)\n",
        "speed = 2.0\n\n",
        "[node name=\"Child\" type=\"Node\" parent=\".\"]\n",
    );
    let b = concat!(
        "[gd_scene format=3 uid=\"uid://x\"]\n\n",
        "[ext_resource type=\"Texture2D\" path=\"res://b.png\" id=\"1_zzzzz\"]\n",
        "[ext_resource type=\"Script\" path=\"res://a.gd\" id=\"2_yyyyy\"]\n\n",
        "[node name=\"Root\" type=\"Sprite2D\" unique_id=5]\n",
        "script = ExtResource( \"2_yyyyy\" )\n",
        "texture = ExtResource(\"1_zzzzz\")\n",
        "offset = Vector2( 0.0, 1.50 )\n",
        "speed = 2.00\n\n",
        "[node name=\"Child\" type=\"Node\" parent=\".\" unique_id=6]\n",
    )
    .replace('\n', "\r\n");
    let a = SceneFile::parse(a).unwrap();
    let b = SceneFile::parse(&b).unwrap();
    assert_eq!(canonical_text(&a), canonical_text(&b));
}
#[test]
fn canonical_text_snapshot() {
    let dir = case_dir("showcase");
    insta::assert_snapshot!("showcase-canonical-new", canonical_text(&load(&dir, "new")));
}

#[test]
fn every_fixture_has_no_diff_with_itself() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/demo");
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "tscn" || x == "tres") {
            continue;
        }
        let f = SceneFile::parse_bytes(&fs::read(&p).unwrap()).unwrap();
        let d = diff(&f, &f, &DiffOptions::default());
        assert!(d.is_empty(), "{}: {:?}", p.display(), d.changes);
    }
}

#[test]
fn ignore_editor_metadata_and_epsilon() {
    let a = SceneFile::parse(
        "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nposition = Vector2(1, 2)\nmetadata/_edit_lock_ = true\n",
    )
    .unwrap();
    let b = SceneFile::parse(
        "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node2D\"]\nposition = Vector2(1.0001, 2)\n",
    )
    .unwrap();
    assert_eq!(diff(&a, &b, &DiffOptions::default()).changes.len(), 2);
    let mut opts = DiffOptions {
        ignore_editor_metadata: true,
        ..DiffOptions::default()
    };
    opts.matching.epsilon = 0.001;
    assert!(diff(&a, &b, &opts).is_empty());
}
