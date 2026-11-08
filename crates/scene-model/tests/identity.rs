//! One test per identity rule of the specification (section 4).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use scene_model::{FileKind, MatchHow, MatchOptions, Matching, ModelError, ParentRef, SceneFile};

fn scene(body: &str) -> SceneFile {
    let text = format!("[gd_scene format=3 uid=\"uid://test\"]\n\n{body}");
    SceneFile::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn matching(a: &SceneFile, b: &SceneFile) -> Matching {
    Matching::compute(a, b, &MatchOptions::default())
}

fn node(f: &SceneFile, path: &str) -> usize {
    f.node_by_path(path)
        .unwrap_or_else(|| panic!("no node {path}"))
}

fn prop<'a>(f: &'a SceneFile, path: &str, key: &str) -> &'a godot_text::Value {
    &f.nodes[node(f, path)]
        .props
        .iter()
        .find(|p| p.key == key)
        .unwrap()
        .value
}

// ---------------------------------------------------------------- ext_resource

#[test]
fn ext_resource_renumbering_is_invisible() {
    let a = scene(
        r#"[ext_resource type="Texture2D" uid="uid://tex" path="res://a.png" id="1_aaaaa"]
[ext_resource type="Script" uid="uid://scr" path="res://p.gd" id="2_bbbbb"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("1_aaaaa")
script = ExtResource("2_bbbbb")
"#,
    );
    let b = scene(
        r#"[ext_resource type="Script" uid="uid://scr" path="res://p.gd" id="1_zzzzz"]
[ext_resource type="Texture2D" uid="uid://tex" path="res://a.png" id="2_yyyyy"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("2_yyyyy")
script = ExtResource("1_zzzzz")
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.ext_a_to_b, vec![Some(1), Some(0)]);
    for key in ["texture", "script"] {
        assert!(
            m.values_equal(prop(&a, ".", key), prop(&b, ".", key), 0.0),
            "{key}"
        );
    }
}

#[test]
fn ext_resource_matched_by_path_without_uid() {
    let a = scene(
        r#"[ext_resource type="Script" path="res://p.gd" id="1"]

[node name="Root" type="Node"]
script = ExtResource("1")
"#,
    );
    let b = scene(
        r#"[ext_resource type="Script" path="res://p.gd" id="7_abcde"]

[node name="Root" type="Node"]
script = ExtResource("7_abcde")
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.ext_a_to_b, vec![Some(0)]);
    assert!(m.values_equal(prop(&a, ".", "script"), prop(&b, ".", "script"), 0.0));
}

#[test]
fn ext_resource_uid_wins_over_path() {
    // The file was moved: same uid, new path. It is the same resource.
    let a = scene(
        r#"[ext_resource type="Texture2D" uid="uid://tex" path="res://old/a.png" id="1_a"]
[ext_resource type="Texture2D" uid="uid://other" path="res://new/a.png" id="2_b"]

[node name="Root" type="Node"]
"#,
    );
    let b = scene(
        r#"[ext_resource type="Texture2D" uid="uid://tex" path="res://new/a.png" id="1_a"]

[node name="Root" type="Node"]
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.ext_a_to_b, vec![Some(0), None]);
}

#[test]
fn different_ext_resources_are_not_equal() {
    let a = scene(
        r#"[ext_resource type="Texture2D" path="res://a.png" id="1"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("1")
"#,
    );
    let b = scene(
        r#"[ext_resource type="Texture2D" path="res://b.png" id="1"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("1")
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.ext_a_to_b, vec![None]);
    assert!(!m.values_equal(prop(&a, ".", "texture"), prop(&b, ".", "texture"), 0.0));
}

// ---------------------------------------------------------------- sub_resource

#[test]
fn sub_resource_matched_by_id() {
    let a = scene(
        r#"[sub_resource type="BoxShape3D" id="BoxShape3D_a"]
size = Vector3(1, 1, 1)

[node name="Root" type="CollisionShape3D"]
shape = SubResource("BoxShape3D_a")
"#,
    );
    let b = scene(
        r#"[sub_resource type="BoxShape3D" id="BoxShape3D_a"]
size = Vector3(2, 2, 2)

[node name="Root" type="CollisionShape3D"]
shape = SubResource("BoxShape3D_a")
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.sub_a_to_b, vec![Some(0)]);
    // The reference itself is unchanged even though the content changed.
    assert!(m.values_equal(prop(&a, ".", "shape"), prop(&b, ".", "shape"), 0.0));
}

#[test]
fn sub_resource_matched_by_type_and_content_when_id_changes() {
    let a = scene(
        r#"[sub_resource type="Gradient" id="Gradient_old"]
offsets = PackedFloat32Array(0, 1)

[sub_resource type="GradientTexture1D" id="GradientTexture1D_old"]
gradient = SubResource("Gradient_old")

[node name="Root" type="Sprite2D"]
texture = SubResource("GradientTexture1D_old")
"#,
    );
    let b = scene(
        r#"[sub_resource type="Gradient" id="Gradient_new"]
offsets = PackedFloat32Array(0, 1)

[sub_resource type="GradientTexture1D" id="GradientTexture1D_new"]
gradient = SubResource("Gradient_new")

[node name="Root" type="Sprite2D"]
texture = SubResource("GradientTexture1D_new")
"#,
    );
    let m = matching(&a, &b);
    // Nested: the texture only matches because its gradient matched first.
    assert_eq!(m.sub_a_to_b, vec![Some(0), Some(1)]);
    assert!(m.values_equal(prop(&a, ".", "texture"), prop(&b, ".", "texture"), 0.0));
}

#[test]
fn sub_resource_with_new_id_and_new_content_is_new() {
    let a = scene(
        r#"[sub_resource type="BoxShape3D" id="BoxShape3D_a"]
size = Vector3(1, 1, 1)

[node name="Root" type="Node"]
"#,
    );
    let b = scene(
        r#"[sub_resource type="BoxShape3D" id="BoxShape3D_b"]
size = Vector3(5, 1, 1)

[node name="Root" type="Node"]
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(m.sub_a_to_b, vec![None]);
}

// ----------------------------------------------------------------------- nodes

#[test]
fn nodes_matched_by_unique_id_across_rename_and_move() {
    let a = scene(
        r#"[node name="Level" type="Node3D" unique_id=1]

[node name="Enemies" type="Node3D" parent="." unique_id=2]

[node name="Enemy" type="CharacterBody3D" parent="Enemies" unique_id=3]

[node name="Mesh" type="MeshInstance3D" parent="Enemies/Enemy" unique_id=4]
"#,
    );
    let b = scene(
        r#"[node name="Level" type="Node3D" unique_id=1]

[node name="Enemies" type="Node3D" parent="." unique_id=2]

[node name="Bosses" type="Node3D" parent="." unique_id=5]

[node name="Grunt" type="CharacterBody3D" parent="Bosses" unique_id=3]

[node name="Mesh" type="MeshInstance3D" parent="Bosses/Grunt" unique_id=4]
"#,
    );
    let m = matching(&a, &b);
    let enemy = node(&a, "Enemies/Enemy");
    assert_eq!(m.node_a_to_b[enemy], Some(node(&b, "Bosses/Grunt")));
    assert_eq!(m.node_how[enemy], Some(MatchHow::UniqueId));
    assert_eq!(
        m.node_a_to_b[node(&a, "Enemies/Enemy/Mesh")],
        Some(node(&b, "Bosses/Grunt/Mesh"))
    );
    assert_eq!(m.node_b_to_a[node(&b, "Bosses")], None);
}

#[test]
fn nodes_matched_by_path_without_unique_id() {
    let a = scene(
        r#"[node name="Root" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Sprite2D" parent="A"]
"#,
    );
    let b = scene(
        r#"[node name="Root" type="Node2D"]

[node name="B" type="Sprite2D" parent="A"]

[node name="A" type="Node2D" parent="."]
"#
        .replace(
            "[node name=\"B\" type=\"Sprite2D\" parent=\"A\"]\n\n[node name=\"A\" type=\"Node2D\" parent=\".\"]",
            "[node name=\"A\" type=\"Node2D\" parent=\".\"]\n\n[node name=\"B\" type=\"Sprite2D\" parent=\"A\"]",
        )
        .as_str(),
    );
    let m = matching(&a, &b);
    assert_eq!(m.node_a_to_b, vec![Some(0), Some(1), Some(2)]);
    assert!(m.node_how.iter().all(|h| *h == Some(MatchHow::Path)));
}

#[test]
fn root_rename_keeps_identity_and_children() {
    let a = scene("[node name=\"Player\" type=\"Node2D\"]\n\n[node name=\"Sprite\" type=\"Sprite2D\" parent=\".\"]\n");
    let b = scene("[node name=\"Hero\" type=\"Node2D\"]\n\n[node name=\"Sprite\" type=\"Sprite2D\" parent=\".\"]\n");
    let m = matching(&a, &b);
    assert_eq!(m.node_a_to_b, vec![Some(0), Some(1)]);
}

#[test]
fn rename_heuristic_without_unique_id() {
    let a = scene(
        r#"[node name="Root" type="Node3D"]

[node name="Enemy" type="CharacterBody3D" parent="."]
speed = 5.0
health = 100
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Mesh" type="MeshInstance3D" parent="Enemy"]
cast_shadow = 0
"#,
    );
    let b = scene(
        r#"[node name="Root" type="Node3D"]

[node name="Grunt" type="CharacterBody3D" parent="."]
speed = 5.0
health = 120
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 0)

[node name="Mesh" type="MeshInstance3D" parent="Grunt"]
cast_shadow = 0
"#,
    );
    let m = matching(&a, &b);
    let enemy = node(&a, "Enemy");
    assert_eq!(m.node_a_to_b[enemy], Some(node(&b, "Grunt")));
    assert!(matches!(m.node_how[enemy], Some(MatchHow::Heuristic(s)) if s >= 0.65));
    // Children follow via the matched parent.
    assert_eq!(
        m.node_a_to_b[node(&a, "Enemy/Mesh")],
        Some(node(&b, "Grunt/Mesh"))
    );
    assert_eq!(m.node_how[node(&a, "Enemy/Mesh")], Some(MatchHow::Path));
}

#[test]
fn move_heuristic_into_new_parent() {
    let a = scene(
        r#"[node name="Level" type="Node2D"]

[node name="Pickups" type="Node2D" parent="."]

[node name="Coin" type="Area2D" parent="Pickups"]
value = 10
position = Vector2(5, 5)
"#,
    );
    let b = scene(
        r#"[node name="Level" type="Node2D"]

[node name="Pickups" type="Node2D" parent="."]

[node name="Coins" type="Node2D" parent="."]

[node name="Coin" type="Area2D" parent="Coins"]
value = 10
position = Vector2(5, 5)
"#,
    );
    let m = matching(&a, &b);
    assert_eq!(
        m.node_a_to_b[node(&a, "Pickups/Coin")],
        Some(node(&b, "Coins/Coin"))
    );
}

#[test]
fn heuristic_requires_same_type() {
    let a = scene("[node name=\"Root\" type=\"Node\"]\n\n[node name=\"A\" type=\"Sprite2D\" parent=\".\"]\nx = 1\n");
    let b = scene("[node name=\"Root\" type=\"Node\"]\n\n[node name=\"B\" type=\"Label\" parent=\".\"]\nx = 1\n");
    let m = matching(&a, &b);
    assert_eq!(m.node_a_to_b, vec![Some(0), None]);
}

#[test]
fn heuristic_threshold_is_configurable() {
    let a = scene("[node name=\"Root\" type=\"Node\"]\n\n[node name=\"A\" type=\"Sprite2D\" parent=\".\"]\nx = 1\n");
    let b = scene("[node name=\"Root\" type=\"Node\"]\n\n[node name=\"B\" type=\"Sprite2D\" parent=\".\"]\nx = 1\n");
    let m = Matching::compute(&a, &b, &MatchOptions::default());
    assert_eq!(m.node_a_to_b[1], Some(1));
    let strict = MatchOptions {
        rename_threshold: 0.99,
        ..MatchOptions::default()
    };
    let m = Matching::compute(&a, &b, &strict);
    assert_eq!(m.node_a_to_b[1], None);
}

#[test]
fn duplicate_unique_ids_fall_back_to_paths() {
    let a = scene("[node name=\"Root\" type=\"Node\" unique_id=1]\n\n[node name=\"A\" type=\"Node\" parent=\".\" unique_id=7]\n\n[node name=\"B\" type=\"Node\" parent=\".\" unique_id=7]\n");
    let b = scene("[node name=\"Root\" type=\"Node\" unique_id=1]\n\n[node name=\"B\" type=\"Node\" parent=\".\" unique_id=7]\n\n[node name=\"A\" type=\"Node\" parent=\".\" unique_id=7]\n");
    let m = matching(&a, &b);
    assert_eq!(m.node_a_to_b, vec![Some(0), Some(2), Some(1)]);
}

#[test]
fn inherited_scene_nodes_with_external_parents() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/synthetic/inherited-scene.tscn"),
    )
    .unwrap();
    let a = SceneFile::parse(&text).unwrap();
    let gun = node(&a, "Arm/Hand/Gun");
    assert_eq!(a.nodes[gun].parent, ParentRef::External("Arm/Hand".into()));
    assert!(a.nodes[0].instance.is_some());
    // Without unique ids the override nodes still match by external path.
    let stripped = text
        .lines()
        .map(|l| {
            l.split(" unique_id=")
                .next()
                .map(|head| {
                    if l.contains(" unique_id=") {
                        let rest = l.split(" unique_id=").nth(1).unwrap();
                        let after = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                        format!("{head}{after}")
                    } else {
                        l.to_string()
                    }
                })
                .unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let b = SceneFile::parse(&stripped).unwrap();
    let m = matching(&a, &b);
    assert!(
        m.node_a_to_b.iter().all(Option::is_some),
        "{:?}",
        m.node_a_to_b
    );
}

// ------------------------------------------------------------- model & header

#[test]
fn descriptor_and_connections() {
    let f = SceneFile::parse(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/synthetic/no-unique-id.tscn"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(f.kind, FileKind::Scene);
    assert_eq!(f.descriptor.load_steps, Some(3));
    assert_eq!(f.descriptor.uid.as_deref(), Some("uid://bnounique0001"));
    let c = &f.connections[0];
    assert_eq!(
        c.key(),
        ("body_entered", "Area2D", ".", "_on_area_2d_body_entered")
    );
    assert_eq!((c.flags, c.unbinds), (3, 1));
    assert_eq!(f.nodes[node(&f, "Area2D")].groups, ["hitbox", "player"]);
    assert!(f.validate().is_empty(), "{:?}", f.validate());
}

#[test]
fn godot3_files_are_rejected_clearly() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/invalid/godot3-format2.tscn"),
    )
    .unwrap();
    let err = SceneFile::parse(&text).unwrap_err().with_path("old.tscn");
    assert!(matches!(
        err,
        ModelError::UnsupportedFormat { format: 2, .. }
    ));
    let msg = err.to_string();
    assert!(msg.contains("old.tscn") && msg.contains("Godot 3"), "{msg}");
}

#[test]
fn structural_errors() {
    for (body, needle) in [
        ("[node name=\"A\" type=\"Node\"]\n\n[node name=\"B\" type=\"Node\"]\n", "root"),
        ("[node name=\"A\" type=\"Node\" parent=\".\"]\n", "must be the root"),
        ("[node name=\"A\" type=\"Node\"]\n\n[node name=\"B\" parent=\".\"]\n\n[node name=\"B\" parent=\".\"]\n", "Duplicate"),
        ("[frobnicate]\n", "Unknown section"),
        ("[node type=\"Node\"]\n", "'name'"),
    ] {
        let text = format!("[gd_scene format=3]\n\n{body}");
        let err = SceneFile::parse(&text).unwrap_err().to_string();
        assert!(err.contains(needle), "{body}: {err}");
    }
}

#[test]
fn validate_reports_dangling_and_out_of_order_references() {
    let f = scene(
        r#"[sub_resource type="GradientTexture1D" id="T"]
gradient = SubResource("G")

[sub_resource type="Gradient" id="G"]

[node name="Root" type="Sprite2D"]
texture = ExtResource("missing")
"#,
    );
    let issues: Vec<_> = f.validate().into_iter().map(|i| i.message).collect();
    assert!(
        issues
            .iter()
            .any(|m| m.contains("used before it is defined")),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|m| m.contains("ExtResource(\"missing\")")),
        "{issues:?}"
    );
}

#[test]
fn all_fixtures_build_a_valid_model() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut n = 0;
    for sub in ["demo", "synthetic"] {
        for e in std::fs::read_dir(dir.join(sub)).unwrap().flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x == "tscn" || x == "tres") {
                continue;
            }
            let f = SceneFile::parse_bytes(&std::fs::read(&p).unwrap())
                .unwrap_or_else(|e| panic!("{}", e.with_path(&p)));
            let issues = f.validate();
            assert!(issues.is_empty(), "{}: {issues:?}", p.display());
            // A file matched against itself is matched completely, by identity.
            let m = Matching::compute(&f, &f, &MatchOptions::default());
            assert!(
                m.node_a_to_b.iter().enumerate().all(|(i, j)| *j == Some(i)),
                "{}",
                p.display()
            );
            assert!(
                m.ext_a_to_b.iter().enumerate().all(|(i, j)| *j == Some(i)),
                "{}",
                p.display()
            );
            assert!(
                m.sub_a_to_b.iter().enumerate().all(|(i, j)| *j == Some(i)),
                "{}",
                p.display()
            );
            n += 1;
        }
    }
    assert!(n > 150);
}
