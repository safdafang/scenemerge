//! End-to-end: `scenemerge install` + `git merge` / `git rebase` in a real repository.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{exe, Repo};

const BASE: &str = r#"[gd_scene load_steps=3 format=3 uid="uid://blevel00000"]

[ext_resource type="Script" uid="uid://bscript0000" path="res://level.gd" id="1_aaaaa"]
[ext_resource type="PackedScene" uid="uid://benemy00000" path="res://enemy.tscn" id="2_bbbbb"]

[node name="Level" type="Node2D"]
script = ExtResource("1_aaaaa")

[node name="Camera2D" type="Camera2D" parent="."]
zoom = Vector2(1, 1)

[node name="Enemies" type="Node2D" parent="."]

[node name="Enemy" parent="Enemies" instance=ExtResource("2_bbbbb")]
position = Vector2(100, 0)
"#;

/// Branch "feature": the editor renumbered the ext resources, the camera zoom
/// changed and a second enemy was added.
const FEATURE: &str = r#"[gd_scene format=3 uid="uid://blevel00000"]

[ext_resource type="PackedScene" uid="uid://benemy00000" path="res://enemy.tscn" id="1_xxxxx"]
[ext_resource type="Script" uid="uid://bscript0000" path="res://level.gd" id="2_yyyyy"]

[node name="Level" type="Node2D"]
script = ExtResource("2_yyyyy")

[node name="Camera2D" type="Camera2D" parent="."]
zoom = Vector2(2, 2)

[node name="Enemies" type="Node2D" parent="."]

[node name="Enemy" parent="Enemies" instance=ExtResource("1_xxxxx")]
position = Vector2(100, 0)

[node name="Enemy2" parent="Enemies" instance=ExtResource("1_xxxxx")]
position = Vector2(200, 0)
"#;

/// Branch "main": a HUD texture was added (a new ext resource) and the first
/// enemy moved.
const MAIN: &str = r#"[gd_scene load_steps=4 format=3 uid="uid://blevel00000"]

[ext_resource type="Script" uid="uid://bscript0000" path="res://level.gd" id="1_aaaaa"]
[ext_resource type="PackedScene" uid="uid://benemy00000" path="res://enemy.tscn" id="2_bbbbb"]
[ext_resource type="Texture2D" uid="uid://bhud0000000" path="res://hud.png" id="3_ccccc"]

[node name="Level" type="Node2D"]
script = ExtResource("1_aaaaa")

[node name="Camera2D" type="Camera2D" parent="."]
zoom = Vector2(1, 1)

[node name="Enemies" type="Node2D" parent="."]

[node name="Enemy" parent="Enemies" instance=ExtResource("2_bbbbb")]
position = Vector2(150, 0)

[node name="Hud" type="Sprite2D" parent="."]
texture = ExtResource("3_ccccc")
"#;

fn setup(repo: &Repo) {
    let out = repo.scenemerge(&["install", "--command", &exe()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    repo.write("level.tscn", BASE);
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("level.tscn", FEATURE);
    repo.commit_all("feature");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("level.tscn", MAIN);
    repo.commit_all("main");
}

fn assert_merged(text: &str) {
    for needle in [
        "zoom = Vector2(2, 2)",       // from feature
        "position = Vector2(150, 0)", // from main
        "[node name=\"Enemy2\" parent=\"Enemies\"",
        "[node name=\"Hud\" type=\"Sprite2D\" parent=\".\"]",
        "path=\"res://hud.png\"",
    ] {
        assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
    }
    assert!(!text.contains("<<<<<<<"), "{text}");
    let f = scene_model::SceneFile::parse(text).unwrap();
    assert!(f.validate().is_empty(), "{:?}", f.validate());
    assert_eq!(f.ext_resources.len(), 3, "{text}");
}

#[test]
fn plain_git_would_conflict_but_scenemerge_merges() {
    // Without the driver, Git's line merge conflicts on this history.
    let plain = Repo::new();
    plain.write("level.tscn", BASE);
    plain.commit_all("base");
    plain.git(&["checkout", "-q", "-b", "feature"]);
    plain.write("level.tscn", FEATURE);
    plain.commit_all("feature");
    plain.git(&["checkout", "-q", "main"]);
    plain.write("level.tscn", MAIN);
    plain.commit_all("main");
    let out = plain.run("git", &["merge", "--no-edit", "feature"]);
    assert!(
        !out.status.success(),
        "expected a textual conflict without scenemerge"
    );

    // With the driver the same merge is clean.
    let repo = Repo::new();
    setup(&repo);
    let out = repo.run("git", &["merge", "--no-edit", "feature"]);
    assert!(
        out.status.success(),
        "git merge failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let merged = repo.read("level.tscn");
    assert_merged(&merged);
    // The result keeps "ours" (main) ids, so the diff against main is minimal.
    assert!(merged.contains("id=\"3_ccccc\""), "{merged}");
    // 3 resources, as in main already: load_steps stays 4.
    assert!(merged.contains("load_steps=4"), "{merged}");
}

#[test]
fn git_rebase_uses_the_driver_too() {
    let repo = Repo::new();
    setup(&repo);
    repo.git(&["checkout", "-q", "feature"]);
    let out = repo.run("git", &["rebase", "main"]);
    assert!(
        out.status.success(),
        "git rebase failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_merged(&repo.read("level.tscn"));
}

#[test]
fn real_conflict_leaves_property_level_markers() {
    let repo = Repo::new();
    setup(&repo);
    // Make "main" also change the zoom, differently.
    repo.write(
        "level.tscn",
        &MAIN.replace("zoom = Vector2(1, 1)", "zoom = Vector2(3, 3)"),
    );
    repo.commit_all("main zoom");
    let out = repo.run("git", &["merge", "--no-edit", "feature"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CONFLICT (property): level.tscn: node:Camera2D zoom"),
        "{stderr}"
    );
    let text = repo.read("level.tscn");
    // Only the zoom line is in conflict; everything else merged.
    let markers: Vec<&str> = text.lines().filter(|l| l.starts_with("<<<<<<<")).collect();
    assert_eq!(markers.len(), 1, "{text}");
    assert!(
        // Git >= 2.44 passes its labels (%X/%Y); older versions get the defaults.
        text.contains(
            "<<<<<<< HEAD\nzoom = Vector2(3, 3)\n=======\nzoom = Vector2(2, 2)\n>>>>>>> feature\n"
        ) || text.contains(
            "<<<<<<< ours\nzoom = Vector2(3, 3)\n=======\nzoom = Vector2(2, 2)\n>>>>>>> theirs\n"
        ),
        "{text}"
    );
    assert!(text.contains("[node name=\"Enemy2\""), "{text}");
    // Git knows the file is unmerged.
    let status = repo.git(&["status", "--porcelain"]);
    assert!(status.contains("UU level.tscn"), "{status}");
}

#[test]
fn unparseable_file_falls_back_to_a_line_merge() {
    let repo = Repo::new();
    let out = repo.scenemerge(&["install", "--command", &exe()]);
    assert!(out.status.success());
    let godot3 = "[gd_scene load_steps=2 format=2]\n\n[node name=\"A\" type=\"Node2D\"]\nposition = Vector2( 0, 0 )\n\n[node name=\"B\" type=\"Node2D\" parent=\".\"]\n";
    repo.write("old.tscn", godot3);
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("old.tscn", &godot3.replace("( 0, 0 )", "( 1, 1 )"));
    repo.commit_all("feature");
    repo.git(&["checkout", "-q", "main"]);
    repo.write(
        "old.tscn",
        &format!("{godot3}\n[node name=\"C\" type=\"Node2D\" parent=\".\"]\n"),
    );
    repo.commit_all("main");
    let out = repo.run("git", &["merge", "--no-edit", "feature"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = repo.read("old.tscn");
    assert!(
        text.contains("Vector2( 1, 1 )") && text.contains("name=\"C\""),
        "{text}"
    );
}
