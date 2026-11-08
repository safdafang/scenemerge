//! End-to-end: `scenemerge install` + `git diff` in a real repository.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{exe, Repo};

const BEFORE: &str = r#"[gd_scene load_steps=3 format=3 uid="uid://bplayer0000"]

[ext_resource type="Script" uid="uid://bscript0000" path="res://player.gd" id="1_aaaaa"]
[ext_resource type="Texture2D" uid="uid://bicon000000" path="res://icon.svg" id="2_bbbbb"]

[node name="Player" type="CharacterBody3D"]
script = ExtResource("1_aaaaa")

[node name="Camera3D" type="Camera3D" parent="."]
fov = 75.0

[node name="Sprite" type="Sprite3D" parent="."]
texture = ExtResource("2_bbbbb")
"#;

// The editor renumbered and reordered the ext resources and dropped
// load_steps; the only real change is the camera's fov.
const AFTER: &str = r#"[gd_scene format=3 uid="uid://bplayer0000"]

[ext_resource type="Texture2D" uid="uid://bicon000000" path="res://icon.svg" id="1_qqqqq"]
[ext_resource type="Script" uid="uid://bscript0000" path="res://player.gd" id="2_wwwww"]

[node name="Player" type="CharacterBody3D"]
script = ExtResource("2_wwwww")

[node name="Camera3D" type="Camera3D" parent="."]
fov = 90.0

[node name="Sprite" type="Sprite3D" parent="."]
texture = ExtResource("1_qqqqq")
"#;

fn stdout(o: &std::process::Output) -> String {
    assert!(
        o.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn install_is_idempotent_and_uninstall_cleans_up() {
    let repo = Repo::new();
    repo.write(".gitattributes", "*.png binary\n");
    let first = stdout(&repo.scenemerge(&["install", "--command", &exe()]));
    assert!(first.contains("Installed"), "{first}");
    let attrs = repo.read(".gitattributes");
    assert!(attrs.starts_with("*.png binary\n"), "{attrs}");
    assert!(
        attrs.contains("*.tscn diff=godot-scene merge=godot-scene\n"),
        "{attrs}"
    );
    assert!(
        attrs.contains("*.tres diff=godot-scene merge=godot-scene\n"),
        "{attrs}"
    );
    let config = repo.git(&["config", "--local", "--get-regexp", "godot-scene"]);

    // Second run: nothing changes, nothing is duplicated.
    let second = stdout(&repo.scenemerge(&["install", "--command", &exe()]));
    assert!(second.contains("already installed"), "{second}");
    assert_eq!(repo.read(".gitattributes"), attrs);
    assert_eq!(
        repo.git(&["config", "--local", "--get-regexp", "godot-scene"]),
        config
    );
    assert_eq!(attrs.matches("*.tscn diff=godot-scene").count(), 1);
    assert!(config.contains("merge.godot-scene.driver"), "{config}");
    assert!(
        config.contains("merge %O %A %B -o %A --path %P"),
        "{config}"
    );
    assert!(config.contains("diff.godot-scene.textconv"), "{config}");

    let out = stdout(&repo.scenemerge(&["uninstall"]));
    assert!(out.contains("Uninstalled"), "{out}");
    assert_eq!(repo.read(".gitattributes"), "*.png binary\n");
    let left = repo.run("git", &["config", "--local", "--get-regexp", "godot-scene"]);
    assert!(!left.status.success(), "config left behind");
    let again = stdout(&repo.scenemerge(&["uninstall"]));
    assert!(again.contains("nothing to do"), "{again}");
}

#[test]
fn global_install_writes_global_attributes() {
    let repo = Repo::new();
    stdout(&repo.scenemerge(&["install", "--global", "--command", &exe()]));
    let attrs = repo
        .home
        .path()
        .join(".config")
        .join("git")
        .join("attributes");
    let text = common::read(&attrs);
    assert!(
        text.contains("*.tscn diff=godot-scene merge=godot-scene"),
        "{text}"
    );
    let cfg = repo.git(&["config", "--global", "--get", "merge.godot-scene.name"]);
    assert_eq!(cfg.trim(), "Godot semantic scene merge");
    assert!(!repo.path(".gitattributes").exists());
    stdout(&repo.scenemerge(&["uninstall", "--global"]));
    assert!(!common::read(&attrs).contains("godot-scene"));
}

#[test]
fn git_diff_with_textconv_hides_id_renumbering() {
    let repo = Repo::new();
    stdout(&repo.scenemerge(&["install", "--command", &exe()]));
    repo.write("player.tscn", BEFORE);
    repo.commit_all("add player");
    repo.write("player.tscn", AFTER);
    let d = repo.git(&["diff", "--no-color", "player.tscn"]);
    let changed: Vec<&str> = d
        .lines()
        .filter(|l| {
            (l.starts_with('+') || l.starts_with('-'))
                && !l.starts_with("+++")
                && !l.starts_with("---")
        })
        .collect();
    assert_eq!(changed, ["-fov = 75.0", "+fov = 90.0"], "{d}");
}

#[test]
fn git_diff_with_semantic_external_driver() {
    let repo = Repo::new();
    stdout(&repo.scenemerge(&["install", "--diff", "semantic", "--command", &exe()]));
    let cfg = repo.git(&["config", "--local", "--get-regexp", "diff.godot-scene"]);
    assert!(
        cfg.contains("git-diff") && !cfg.contains("textconv"),
        "{cfg}"
    );
    repo.write("player.tscn", BEFORE);
    repo.commit_all("add player");
    repo.write("player.tscn", AFTER);
    let d = repo.git(&["diff", "player.tscn"]);
    assert_eq!(d, "player.tscn\n~ Camera3D\n    fov: 75.0 → 90.0\n", "{d}");

    // New and deleted files are shown too (Git passes /dev/null).
    repo.write("enemy.tscn", BEFORE);
    repo.git(&["add", "-N", "enemy.tscn"]);
    let d = repo.git(&["diff", "enemy.tscn"]);
    assert!(d.starts_with("enemy.tscn (new file)\n"), "{d}");
    assert!(d.contains("+ Camera3D (Camera3D)"), "{d}");
}

#[test]
fn git_external_diff_environment_variable() {
    // Without any installation, GIT_EXTERNAL_DIFF alone also works.
    let repo = Repo::new();
    repo.write("player.tscn", BEFORE);
    repo.commit_all("add player");
    repo.write("player.tscn", AFTER);
    let out = repo.run_env(
        "git",
        &["diff", "--ext-diff", "player.tscn"],
        &[("GIT_EXTERNAL_DIFF", &format!("{} git-diff", exe()))],
    );
    let text = stdout(&out);
    assert_eq!(
        text,
        "player.tscn
~ Camera3D
    fov: 75.0 → 90.0
",
        "{text}"
    );
}

#[test]
fn git_external_diff_protocol_arguments() {
    let repo = Repo::new();
    repo.write("a.tscn", BEFORE);
    repo.write("b.tscn", AFTER);
    let hex = "0".repeat(40);
    let a = repo.path("a.tscn").display().to_string();
    let b = repo.path("b.tscn").display().to_string();
    let out = repo.scenemerge(&[
        "git-diff",
        "scenes/player.tscn",
        &a,
        &hex,
        "100644",
        &b,
        &hex,
        "100644",
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).contains("fov: 75.0 → 90.0"));
    // Rename form: 9 arguments, the 8th is the new path.
    let out = repo.scenemerge(&[
        "git-diff",
        "old.tscn",
        &a,
        &hex,
        "100644",
        &b,
        &hex,
        "100644",
        "new.tscn",
        "similarity index 90%\n",
    ]);
    assert!(stdout(&out).starts_with("old.tscn → new.tscn\n"));
    // Unmerged path: a single argument.
    let out = repo.scenemerge(&["git-diff", "conflicted.tscn"]);
    assert_eq!(out.status.code(), Some(0));
}
