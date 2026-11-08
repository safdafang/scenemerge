//! Prints the time of each phase of a 10k-node diff (development aid;
//! uses the benchmark's generator and allocator).
#![allow(missing_docs, clippy::unwrap_used)]
#[path = "../benches/diff_10k.rs"]
#[allow(dead_code)]
mod bench;
use std::time::Instant;
fn main() {
    let a = bench::generate(100, 100, 0, true);
    let b = bench::generate(100, 100, 1, true);
    for _ in 0..3 {
        let t = Instant::now();
        let da = godot_text::Document::parse(&a).unwrap();
        let db = godot_text::Document::parse(&b).unwrap();
        let t_parse = t.elapsed();
        let t = Instant::now();
        let fa = scene_model::SceneFile::from_document(da).unwrap();
        let fb = scene_model::SceneFile::from_document(db).unwrap();
        let t_model = t.elapsed();
        let t = Instant::now();
        let m = scene_model::Matching::compute(&fa, &fb, &Default::default());
        let t_match = t.elapsed();
        let t = Instant::now();
        let d = scene_diff::diff(&fa, &fb, &Default::default());
        let t_diff = t.elapsed();
        println!("parse {t_parse:?}  model {t_model:?}  matching {t_match:?}  diff(incl. matching) {t_diff:?}  changes {}  matched {}", d.changes.len(), m.node_a_to_b.iter().flatten().count());
    }
}
