#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::{Path, PathBuf};

use godot_text::{BodyItem, Document};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == ".git" || n == ".godot") {
                continue;
            }
            collect(&p, out);
        } else if p
            .extension()
            .is_some_and(|e| e == "tscn" || e == "tres" || e == "escn")
        {
            out.push(p);
        }
    }
}

fn check_round_trip(files: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for path in files {
        let bytes = fs::read(path).unwrap();
        match Document::parse_bytes(&bytes) {
            Ok(doc) => {
                let out = doc.to_text();
                if out.as_bytes() != bytes.as_slice() {
                    let at = out
                        .bytes()
                        .zip(bytes.iter())
                        .position(|(a, b)| a != *b)
                        .unwrap_or(out.len().min(bytes.len()));
                    failures.push(format!("{}: output differs at byte {at}", path.display()));
                }
                for s in &doc.sections {
                    for item in &s.body {
                        if let BodyItem::Trivia(t) = item {
                            let stray = t
                                .lines()
                                .map(str::trim)
                                .any(|l| !l.is_empty() && !l.starts_with(';'));
                            assert!(!stray, "{}: unexpected trivia {t:?}", path.display());
                        }
                    }
                }
            }
            Err(e) => failures.push(e.with_path(path).to_string()),
        }
    }
    failures
}

#[test]
fn fixtures_round_trip_byte_for_byte() {
    let mut files = Vec::new();
    collect(&fixtures_dir().join("demo"), &mut files);
    collect(&fixtures_dir().join("synthetic"), &mut files);
    assert!(
        files.len() >= 50,
        "expected at least 50 fixtures, found {}",
        files.len()
    );
    let failures = check_round_trip(&files);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn external_corpus_round_trips() {
    let Some(dir) = std::env::var_os("SCENEMERGE_CORPUS") else {
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    eprintln!(
        "checking {} files from {}",
        files.len(),
        dir.to_string_lossy()
    );
    let failures = check_round_trip(&files);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn invalid_files_report_location() {
    let dir = fixtures_dir().join("invalid");
    let err = |name: &str| {
        let bytes = fs::read(dir.join(name)).unwrap();
        Document::parse_bytes(&bytes).unwrap_err()
    };
    let e = err("syntax-error.tscn");
    assert_eq!((e.line, e.column), (4, 12), "{e}");
    assert!(e.message.contains("Expected 2 arguments"), "{e}");
    let e = err("unterminated-string.tres");
    assert_eq!(e.line, 4, "{e}");
    assert!(e.message.contains("Unterminated string"), "{e}");
    let shown = e.with_path("x.tres").to_string();
    assert!(
        shown.starts_with("x.tres:4:8: Unterminated string"),
        "{shown}"
    );
    assert!(shown.contains("text = \"never closed"), "{shown}");
}
