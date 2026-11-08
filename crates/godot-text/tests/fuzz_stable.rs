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

use godot_text::{parse_value_str, rewrite_refs, Document};

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

const TOKENS: &[&[u8]] = &[
    b"[",
    b"]",
    b"(",
    b")",
    b"{",
    b"}",
    b"\"",
    b"\\",
    b";",
    b"\n",
    b"\r\n",
    b"=",
    b",",
    b":",
    b"&\"",
    b"#",
    b"-",
    b"0",
    b"1.5e",
    b"inf",
    b"nan",
    b"\\u",
    b"\\ud800",
    b"Vector2(",
    b"Array[",
    b"Dictionary[",
    b"ExtResource(",
    b"SubResource(",
    b"Object(",
    b"[node ",
    b"[gd_scene ",
    b"PackedByteArray(\"",
    b"\0",
    b"\xef\xbb\xbf",
    b"\xff",
    b"\xc3",
];

fn mutate(rng: &mut Rng, input: &[u8], corpus: &[Vec<u8>]) -> Vec<u8> {
    let mut data = input.to_vec();
    for _ in 0..=rng.below(4) {
        let len = data.len();
        match rng.below(6) {
            0 if len > 0 => {
                let i = rng.below(len);
                data[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let a = rng.below(len);
                let b = (a + 1 + rng.below(64)).min(len);
                data.drain(a..b);
            }
            2 if len > 0 => {
                let a = rng.below(len);
                let b = (a + 1 + rng.below(64)).min(len);
                let chunk = data[a..b].to_vec();
                let at = rng.below(data.len() + 1);
                data.splice(at..at, chunk);
            }
            3 => {
                let tok = TOKENS[rng.below(TOKENS.len())];
                let at = rng.below(len + 1);
                data.splice(at..at, tok.iter().copied());
            }
            4 if !corpus.is_empty() => {
                let other = &corpus[rng.below(corpus.len())];
                let at = rng.below(len + 1);
                let from = rng.below(other.len());
                let to = (from + rng.below(256)).min(other.len());
                data.splice(at..at, other[from..to].iter().copied());
            }
            _ if len > 0 => {
                let keep = rng.below(len);
                data.truncate(keep);
            }
            _ => {}
        }
    }
    data
}

fn check(data: &[u8]) {
    if let Ok(doc) = Document::parse_bytes(data) {
        assert_eq!(
            doc.to_text().as_bytes(),
            data,
            "accepted input must round-trip"
        );
    }
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = parse_value_str(s);
        let _ = rewrite_refs(s, |_, id| Some(format!("{id}_x")));
    }
}

#[test]
fn mutation_fuzz_never_panics() {
    let seconds: u64 = std::env::var("FUZZ_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut corpus = Vec::new();
    for sub in ["demo", "synthetic", "invalid"] {
        for e in fs::read_dir(dir.join(sub)).unwrap().flatten() {
            let bytes = fs::read(e.path()).unwrap();
            if bytes.len() <= 16 * 1024 {
                corpus.push(bytes);
            }
        }
    }
    assert!(!corpus.is_empty());
    let seed = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5eed_cafe_f00d_u64);
    let mut rng = Rng(seed);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut iterations = 0u64;
    let mut accepted = 0u64;
    while Instant::now() < deadline {
        for _ in 0..64 {
            let base = &corpus[rng.below(corpus.len())];
            let input = mutate(&mut rng, base, &corpus);
            let result = catch_unwind(AssertUnwindSafe(|| check(&input)));
            if result.is_err() {
                let out = std::env::temp_dir().join("scenemerge-fuzz-crash.bin");
                let _ = fs::write(&out, &input);
                panic!("panic on mutated input, saved to {}", out.display());
            }
            if Document::parse_bytes(&input).is_ok() {
                accepted += 1;
            }
            iterations += 1;
        }
    }
    eprintln!("fuzz: {iterations} inputs ({accepted} accepted) in {seconds}s, seed {seed}");
}
