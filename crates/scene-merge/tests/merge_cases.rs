//! Merge scenarios from `tests/merge-cases/<name>/`:
//!
//! * `base.*`, `ours.*`, `theirs.*` — the inputs (`.tscn` or `.tres`);
//! * `expected.*` — the exact merged output;
//! * `expected-report.json` — the conflict report;
//! * `options.txt` (optional) — `conflict=ours|theirs|markers`.
//!
//! Set `SCENEMERGE_BLESS=1` to (re)write the expected files from the actual
//! output — then review the diff before committing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::path::{Path, PathBuf};

use scene_merge::{merge, ConflictStyle, MergeOptions, Report, REPORT_SCHEMA_VERSION};
use scene_model::SceneFile;

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/merge-cases")
}

fn find(dir: &Path, stem: &str) -> Option<PathBuf> {
    ["tscn", "tres"]
        .iter()
        .map(|e| dir.join(format!("{stem}.{e}")))
        .find(|p| p.exists())
}

fn load(p: &Path) -> SceneFile {
    SceneFile::parse_bytes(&fs::read(p).unwrap()).unwrap_or_else(|e| panic!("{}", e.with_path(p)))
}

/// Runs one case; returns a description of the mismatch, if any.
fn run_case(dir: &Path) -> Option<String> {
    let name = dir.file_name().unwrap().to_string_lossy().to_string();
    let base = find(dir, "base").unwrap_or_else(|| panic!("{name}: no base"));
    let ext = base.extension().unwrap().to_string_lossy().to_string();
    let ours = dir.join(format!("ours.{ext}"));
    let theirs = dir.join(format!("theirs.{ext}"));
    let mut opts = MergeOptions::default();
    if let Ok(o) = fs::read_to_string(dir.join("options.txt")) {
        for line in o.lines() {
            match line.trim() {
                "conflict=ours" => opts.conflict = ConflictStyle::Ours,
                "conflict=theirs" => opts.conflict = ConflictStyle::Theirs,
                "conflict=markers" | "" => {}
                other => panic!("{name}: unknown option {other}"),
            }
        }
    }
    let out = merge(&load(&base), &load(&ours), &load(&theirs), &opts)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    let report = serde_json::to_string_pretty(&Report {
        schema_version: REPORT_SCHEMA_VERSION,
        path: None,
        clean: out.is_clean(),
        conflicts: &out.conflicts,
        resolutions: &out.resolutions,
    })
    .unwrap()
        + "\n";
    let expected_path = dir.join(format!("expected.{ext}"));
    let report_path = dir.join("expected-report.json");
    if std::env::var_os("SCENEMERGE_BLESS").is_some() {
        fs::write(&expected_path, &out.text).unwrap();
        fs::write(&report_path, &report).unwrap();
        return None;
    }
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("{name}: missing expected.{ext} (run with SCENEMERGE_BLESS=1)"));
    let expected_report = fs::read_to_string(&report_path).unwrap_or_default();
    let mut problems = Vec::new();
    if out.text != expected {
        problems.push(format!(
            "output differs:\n--- expected\n{expected}\n--- actual\n{}",
            out.text
        ));
    }
    if report != expected_report {
        problems.push(format!(
            "report differs:\n--- expected\n{expected_report}\n--- actual\n{report}"
        ));
    }
    // A clean merge must be a valid file that Godot can load.
    if out.is_clean() {
        let f = SceneFile::parse(&out.text)
            .unwrap_or_else(|e| panic!("{name}: clean output does not parse: {e}"));
        let issues = f.validate();
        assert!(issues.is_empty(), "{name}: {issues:?}");
    } else if opts.conflict != ConflictStyle::Markers {
        SceneFile::parse(&out.text)
            .unwrap_or_else(|e| panic!("{name}: resolved output does not parse: {e}"));
    }
    (!problems.is_empty()).then(|| format!("== {name}\n{}", problems.join("\n")))
}

#[test]
fn merge_cases() {
    let mut dirs: Vec<PathBuf> = fs::read_dir(cases_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(
        dirs.len() >= 25,
        "expected at least 25 merge cases, found {}",
        dirs.len()
    );
    let failures: Vec<String> = dirs.iter().filter_map(|d| run_case(d)).collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
