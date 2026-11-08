# Contributing to scenemerge

Thanks for helping! The most valuable contributions are **bug reports with a
small scene that reproduces the problem**, ideally the three files of a merge
(base, ours, theirs) or the two files of a diff.

## Reporting bugs

Please include:

* `scenemerge --version`, your OS and the Godot version that saved the files;
* the command you ran and its full output;
* minimal `.tscn`/`.tres` files (strip everything unrelated; files from your
  game are fine if you can share them).

If a file fails to parse, `scenemerge check FILE` prints the exact line and
column. That line is usually all we need.

## Building and testing

You need stable Rust (1.88 or newer) and Git.

```sh
cargo build
cargo test --workspace           # unit, snapshot, property and end-to-end tests
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Optional, slower checks:

```sh
# parser fuzzing on stable (any platform)
FUZZ_SECONDS=600 cargo test --release -p godot-text --test fuzz_stable -- --nocapture
FUZZ_SECONDS=600 cargo test --release -p scene-merge --test fuzz_merge_stable -- --nocapture
# libFuzzer (nightly)
cargo +nightly fuzz run parse
# round-trip a whole project
SCENEMERGE_CORPUS=/path/to/project cargo test -p godot-text --test roundtrip
# load merge results in a real Godot
cargo build --release
GODOT=/path/to/godot tests/godot/run.sh /path/to/godot-demo-projects
# benchmark (target: < 200 ms for 10 000 nodes)
cargo bench -p scene-diff
```

## Repository layout

| path | contents |
|---|---|
| `crates/godot-text` | lossless lexer, parser and writer |
| `crates/scene-model` | semantic model, validation, identity matching |
| `crates/scene-diff` | diff, human/JSON/canonical-text output |
| `crates/scene-merge` | three-way merge |
| `crates/scenemerge-cli` | the `scenemerge` binary |
| `tests/fixtures` | real and synthetic `.tscn`/`.tres` files (byte-exact, `-text`) |
| `tests/merge-cases` | merge scenarios: `base`, `ours`, `theirs`, `expected`, `expected-report.json` |
| `tests/godot` | headless-Godot integration |
| `DECISIONS.md` | design decisions and verified format details (read this first) |

## Adding a merge scenario

1. Create `tests/merge-cases/<name>/` with `base.tscn`, `ours.tscn`,
   `theirs.tscn` (and optionally `options.txt` with `conflict=ours|theirs`).
2. Run `SCENEMERGE_BLESS=1 cargo test -p scene-merge --test merge_cases`.
3. **Review** the generated `expected.tscn` and `expected-report.json`
   line by line: they define the expected behaviour.

Snapshot tests of the diff output use [insta](https://insta.rs):
`cargo insta review` (or `INSTA_UPDATE=always cargo test`) after a deliberate
output change.

## Rules of the code base

* Never break the round-trip: `write(parse(x)) == x` byte for byte.
* No `unwrap()`, `expect()`, `panic!` or unchecked indexing in library crates
  (enforced by clippy lints); tests may use them.
* Do not guess the file format: check Godot's `resource_format_text.cpp` /
  `variant_parser.cpp` or real files, and record findings in `DECISIONS.md`.
* Public API items have `///` docs.
* Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `test:`, `docs:`, `chore:`…).

## Releasing (maintainers)

1. Update `CHANGELOG.md` and the version in `Cargo.toml` (`[workspace.package]`).
2. Tag: `git tag v0.1.0 && git push --tags`.
3. `.github/workflows/release.yml` builds the binaries and creates the GitHub
   release with the changelog section.
