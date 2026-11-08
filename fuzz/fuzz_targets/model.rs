//! Model building and diffing must never panic.
//!
//! The input is split at the first NUL byte into two files.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (a, b) = match data.iter().position(|&c| c == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, data),
    };
    let (Ok(fa), Ok(fb)) = (
        scene_model::SceneFile::parse_bytes(a),
        scene_model::SceneFile::parse_bytes(b),
    ) else {
        return;
    };
    let _ = fa.validate();
    let d = scene_diff::diff(&fa, &fb, &scene_diff::DiffOptions::default());
    let _ = scene_diff::to_json("a", "b", &d);
    let _ = scene_diff::render_human("x", &d, &scene_diff::HumanOptions::default());
    let _ = scene_diff::canonical_text(&fa);
    // A file never differs from itself.
    assert!(scene_diff::diff(&fa, &fa, &scene_diff::DiffOptions::default()).is_empty());
});
