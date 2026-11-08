#![no_main]

use libfuzzer_sys::fuzz_target;
use scene_merge::{merge, ConflictStyle, MergeOptions};
use scene_model::SceneFile;

fuzz_target!(|data: &[u8]| {
    let mut parts = data.splitn(3, |&c| c == 0);
    let a = parts.next().unwrap_or(&[]);
    let b = parts.next().unwrap_or(a);
    let c = parts.next().unwrap_or(b);
    let (Ok(fa), Ok(fb), Ok(fc)) = (
        SceneFile::parse_bytes(a),
        SceneFile::parse_bytes(b),
        SceneFile::parse_bytes(c),
    ) else {
        return;
    };
    let _ = fa.validate();
    let d = scene_diff::diff(&fa, &fb, &scene_diff::DiffOptions::default());
    let _ = scene_diff::to_json("a", "b", &d);
    let _ = scene_diff::render_human("x", &d, &scene_diff::HumanOptions::default());
    let _ = scene_diff::canonical_text(&fa);
    assert!(scene_diff::diff(&fa, &fa, &scene_diff::DiffOptions::default()).is_empty());
    let valid = fa.validate().is_empty() && fb.validate().is_empty() && fc.validate().is_empty();
    for style in [ConflictStyle::Markers, ConflictStyle::Ours, ConflictStyle::Theirs] {
        let opts = MergeOptions {
            conflict: style,
            ..MergeOptions::default()
        };
        let r = merge(&fa, &fb, &fc, &opts);
        if valid && fa.kind == fb.kind && fb.kind == fc.kind {
            assert!(r.is_ok(), "{:?}", r.err());
        }
    }
});
