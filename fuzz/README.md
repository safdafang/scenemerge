# Fuzzing

Requires a nightly toolchain and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz):

```sh
cargo install cargo-fuzz
mkdir -p fuzz/corpus/parse && cp tests/fixtures/*/*.t* fuzz/corpus/parse/
cargo +nightly fuzz run parse -- -max_total_time=600
cargo +nightly fuzz run model -- -max_total_time=600
```

Targets:

* `parse`: `godot_text::Document::parse_bytes` never panics, and every accepted
  input is written back byte for byte.
* `model`: building the semantic model, diffing and merging never panic; a
  file never differs from itself; merging valid inputs never fails.

On platforms without libFuzzer support there is a stable-Rust mutation fuzzer
in `crates/godot-text/tests/fuzz_stable.rs`:

```sh
FUZZ_SECONDS=600 cargo test --release -p godot-text --test fuzz_stable -- --nocapture
FUZZ_SECONDS=600 cargo test --release -p scene-merge --test fuzz_merge_stable -- --nocapture
```

The second one mutates fixtures structurally (values, lines, node names,
sections) into OURS and THEIRS and checks the whole pipeline: no panics, valid
inputs always merge into a valid file in every conflict mode.
