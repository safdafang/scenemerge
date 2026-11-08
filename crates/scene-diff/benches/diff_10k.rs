#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::fmt::Write as _;

use criterion::{criterion_group, criterion_main, Criterion};
use scene_diff::{diff, DiffOptions};
use scene_model::SceneFile;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// A scene with `groups × per_group` nodes plus the root and group nodes.
/// `variant` 1 changes ~1% of the properties, renames and removes a few
/// nodes and adds some, the kind of change a real commit makes.
pub fn generate(groups: usize, per_group: usize, variant: u32, unique_ids: bool) -> String {
    let mut s = String::new();
    s.push_str("[gd_scene format=3 uid=\"uid://bbench00001\"]\n\n");
    s.push_str("[ext_resource type=\"Texture2D\" uid=\"uid://bbenchtex01\" path=\"res://icon.svg\" id=\"1_icon1\"]\n");
    s.push_str("[ext_resource type=\"Script\" uid=\"uid://bbenchscr01\" path=\"res://enemy.gd\" id=\"2_scr01\"]\n\n");
    for i in 0..20 {
        let _ = write!(
            s,
            "[sub_resource type=\"RectangleShape2D\" id=\"RectangleShape2D_{i:05}\"]\nsize = Vector2({}, 16)\n\n",
            8 + i
        );
    }
    let uid = |n: usize| {
        if unique_ids {
            format!(" unique_id={}", 1000 + n)
        } else {
            String::new()
        }
    };
    let _ = writeln!(s, "[node name=\"Level\" type=\"Node2D\"{}]\n", uid(0));
    let mut n = 1;
    for g in 0..groups {
        let _ = writeln!(
            s,
            "[node name=\"Group{g}\" type=\"Node2D\" parent=\".\"{}]\nposition = Vector2({}, 0)\n",
            uid(n),
            g * 100
        );
        n += 1;
        for k in 0..per_group {
            let id = g * per_group + k;
            if variant == 1 && id % 997 == 3 {
                n += 1;
                continue;
            }
            let name = if variant == 1 && id % 991 == 5 {
                format!("Renamed{k}")
            } else {
                format!("Enemy{k}")
            };
            let x = if variant == 1 && id % 101 == 7 {
                k * 16 + 1
            } else {
                k * 16
            };
            let _ = writeln!(
                s,
                "[node name=\"{name}\" type=\"Sprite2D\" parent=\"Group{g}\"{} groups=[\"enemies\"]]\n\
                 script = ExtResource(\"2_scr01\")\n\
                 texture = ExtResource(\"1_icon1\")\n\
                 position = Vector2({x}, {})\n\
                 modulate = Color(1, 1, 1, 1)\n\
                 metadata/shape = SubResource(\"RectangleShape2D_{:05}\")\n",
                uid(n),
                g * 8,
                id % 20
            );
            n += 1;
        }
        if variant == 1 && g % 10 == 0 {
            let _ = writeln!(
                s,
                "[node name=\"Added{g}\" type=\"Node2D\" parent=\"Group{g}\"]\n"
            );
        }
    }
    for g in 0..groups {
        let _ = writeln!(
            s,
            "[connection signal=\"visibility_changed\" from=\"Group{g}\" to=\".\" method=\"_on_visibility_changed\"]"
        );
    }
    s
}

fn bench(c: &mut Criterion) {
    for unique_ids in [true, false] {
        let a = generate(100, 100, 0, unique_ids);
        let b = generate(100, 100, 1, unique_ids);
        let label = if unique_ids {
            "with unique_id"
        } else {
            "without unique_id"
        };
        c.bench_function(&format!("parse+diff 10k nodes ({label})"), |bench| {
            bench.iter(|| {
                let fa = SceneFile::parse(&a).unwrap();
                let fb = SceneFile::parse(&b).unwrap();
                let d = diff(&fa, &fb, &DiffOptions::default());
                assert!(!d.is_empty());
                d
            });
        });
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench
}
criterion_main!(benches);
