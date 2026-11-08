# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-08

First release.

### Added

- Lossless reader/writer for Godot 4 text scenes and resources (`format=3` and
  `format=4`): byte-for-byte round-trip including CRLF, BOM, comments and
  multi-line values; precise parse errors with file, line, column and snippet.
- Semantic model with validation and identity matching: nodes by `unique_id`,
  path or rename/move heuristic; external resources by uid/path; sub-resources
  by id or content. Local id renumbering is never a change.
- `scenemerge diff` with human (coloured tree), JSON (`schema_version` 1) and
  canonical-text output; component-wise changes of vectors, transforms,
  colours, arrays and dictionaries; `--ignore-editor-metadata`, `--epsilon`.
- `scenemerge merge`: three-way merge on the model with conflicts at property
  level, `--conflict markers|ours|theirs`, JSON conflict report, Godot-style
  regeneration of colliding ids, and a fallback to `git merge-file`.
- Git integration: `scenemerge install` / `uninstall` (repository or
  `--global`), `textconv`, external-diff protocol, merge driver.
- `scenemerge check` for CI: parse, validate, verify round-trip.
- Tests: 160 fixtures from godot-demo-projects plus synthetic edge cases,
  insta snapshots, 46 merge scenarios, property tests, fuzzing (stable and
  cargo-fuzz), end-to-end Git tests, headless Godot integration (4.6 and 4.7).

[Unreleased]: https://github.com/safdafang/scenemerge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/safdafang/scenemerge/releases/tag/v0.1.0
