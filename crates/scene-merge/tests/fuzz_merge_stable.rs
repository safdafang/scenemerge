#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use scene_diff::{diff, DiffOptions};
use scene_merge::{merge, ConflictStyle, MergeOptions};
use scene_model::SceneFile;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

const VALUES: &[&str] = &[
    "1",
    "2.5",
    "-3",
    "true",
    "false",
    "null",
    "\"text\"",
    "&\"name\"",
    "Vector2(1, 2)",
    "Vector3(0, 1, 0)",
    "Color(1, 0, 0, 1)",
    "[1, 2]",
    "{\n\"k\": 1\n}",
    "NodePath(\"..\")",
    "PackedStringArray(\"a\")",
];

fn mutate(rng: &mut Rng, text: &str, others: &[String]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for _ in 0..=rng.below(3) {
        if lines.is_empty() {
            break;
        }
        let i = rng.below(lines.len());
        match rng.below(7) {
            0 | 1 => {
                if let Some(eq) = lines[i].find(" = ") {
                    if !lines[i].starts_with('[') {
                        let v = VALUES[rng.below(VALUES.len())];
                        lines[i] = format!("{} = {v}", &lines[i][..eq]);
                    }
                }
            }
            2 => {
                let key = format!("fuzz_{}", rng.below(5));
                let v = VALUES[rng.below(VALUES.len())];
                lines.insert(i + 1, format!("{key} = {v}"));
            }
            3 => {
                lines.remove(i);
            }
            4 => {
                let l = lines[i].clone();
                lines.insert(i, l);
            }
            5 => {
                if lines[i].starts_with("[node name=\"") {
                    lines[i] = lines[i].replacen("[node name=\"", "[node name=\"Z", 1);
                }
            }
            _ => {
                if let Some(o) = others.get(rng.below(others.len())) {
                    let hs: Vec<&str> = o.lines().filter(|l| l.starts_with("[node")).collect();
                    if let Some(h) = hs.get(rng.below(hs.len().max(1))) {
                        lines.push(String::new());
                        lines.push((*h).to_string());
                    }
                }
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn check(base: &str, ours: &str, theirs: &str) {
    let (Ok(b), Ok(o), Ok(t)) = (
        SceneFile::parse(base),
        SceneFile::parse(ours),
        SceneFile::parse(theirs),
    ) else {
        return;
    };
    assert!(diff(&o, &o, &DiffOptions::default()).is_empty());
    let _ = diff(&b, &o, &DiffOptions::default());
    let all_valid = b.validate().is_empty() && o.validate().is_empty() && t.validate().is_empty();
    for style in [
        ConflictStyle::Markers,
        ConflictStyle::Ours,
        ConflictStyle::Theirs,
    ] {
        let opts = MergeOptions {
            conflict: style,
            ..MergeOptions::default()
        };
        match merge(&b, &o, &t, &opts) {
            Ok(out) => {
                if style != ConflictStyle::Markers || out.is_clean() {
                    let m = SceneFile::parse(&out.text).unwrap_or_else(|e| {
                        panic!("merged output does not parse: {e}\n{}", out.text)
                    });
                    if all_valid {
                        assert!(m.validate().is_empty(), "{:?}\n{}", m.validate(), out.text);
                    }
                }
            }
            Err(e) => {
                assert!(
                    !all_valid || b.kind != o.kind || o.kind != t.kind,
                    "merge failed on valid input: {e}"
                );
            }
        }
    }
}

#[test]
fn merge_pipeline_fuzz() {
    let seconds: u64 = std::env::var("FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/demo");
    let mut corpus: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "tscn" || x == "tres"))
        .filter(|p| fs::metadata(p).map(|m| m.len() < 12_000).unwrap_or(false))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    corpus.sort();
    let seed = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0xfeed_beef_u64);
    let mut rng = Rng(seed);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut n = 0u64;
    while Instant::now() < deadline {
        let base = corpus[rng.below(corpus.len())].clone();
        let ours = mutate(&mut rng, &base, &corpus);
        let theirs = mutate(&mut rng, &base, &corpus);
        let r = catch_unwind(AssertUnwindSafe(|| check(&base, &ours, &theirs)));
        if r.is_err() {
            let out = std::env::temp_dir().join("scenemerge-merge-fuzz");
            let _ = fs::create_dir_all(&out);
            let _ = fs::write(out.join("base.tscn"), &base);
            let _ = fs::write(out.join("ours.tscn"), &ours);
            let _ = fs::write(out.join("theirs.tscn"), &theirs);
            panic!("failure; inputs saved to {}", out.display());
        }
        n += 1;
    }
    eprintln!("merge fuzz: {n} triples in {seconds}s, seed {seed}");
}
