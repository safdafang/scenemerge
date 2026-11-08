#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = godot_text::Document::parse_bytes(data) {
        assert_eq!(doc.to_text().as_bytes(), data);
    }
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = godot_text::parse_value_str(s);
        let _ = godot_text::rewrite_refs(s, |_, id| Some(format!("{id}_x")));
    }
});
