# scenemerge

**Semantic diff and three-way merge for Godot 4 scenes and resources (`.tscn`, `.tres`).**
A single binary that plugs into Git as diff and merge driver — and works just as
well in a terminal or in CI.

[![CI](https://github.com/OWNER/scenemerge/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/scenemerge/actions/workflows/ci.yml)
[![Godot integration](https://github.com/OWNER/scenemerge/actions/workflows/godot-integration.yml/badge.svg)](https://github.com/OWNER/scenemerge/actions/workflows/godot-integration.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

> 🇷🇺 [Краткое описание на русском](README.ru.md)

<!-- DEMO GIF: record `git merge` of two branches with scenemerge installed, e.g.
     with vhs or asciinema, and put it here:
     ![scenemerge demo](docs/demo.gif) -->

## The problem

Godot's `.tscn` files are text, but Git only sees lines. The editor renumbers
resource ids (`ExtResource("1_abcde")` becomes `ExtResource("3_xyz12")`),
reorders sections and adds identifiers when it re-saves a scene, so a one-line
change shows up as a wall of diff, and two people touching the same scene get a
merge conflict even when they changed completely different nodes.

Worse, Godot cannot open a scene with `<<<<<<<` conflict markers in it, so
teams "resolve" conflicts by taking one side wholesale and losing the other
side's work. **scenemerge** understands the structure of scenes — nodes,
properties, resources, signal connections — and merges *that*: changes to
different nodes or different properties merge automatically, id renumbering is
invisible, and a real conflict is reduced to the one property line that
actually conflicts.

## What it looks like

```console
$ scenemerge diff level.tscn~ level.tscn
scenes/level.tscn
+ ext_resource res://enemies/boss.tscn (PackedScene)
~ Player/Camera3D
    transform.origin.y: 1 → 2.5
    fov: 75.0 → 90.0
✎ Enemies/Enemy renamed to Grunt
+ Enemies/Boss (instance of res://enemies/boss.tscn)
+ Coins (Node3D)
→ Pickups/Coin moved to Coins/Coin
- Enemies/OldSpawner (Node3D)
+ signal Player/Area3D.body_entered → Player._on_hit
```

The editor's renumbering of ids, reordered `[ext_resource]` sections,
`load_steps` and the `unique_id`s that Godot 4.6 adds are not changes, and are
not shown.

A merge of two branches that both edited `level.tscn`:

```console
$ git merge feature
Auto-merging level.tscn
Merge made by the 'ort' strategy.
```

…and when both branches really changed the same property:

```console
$ git merge feature
CONFLICT (property): level.tscn: node:Camera2D zoom at line 12
scenemerge: level.tscn: 1 conflict(s); fix the marked lines, then open the scene in Godot
Automatic merge failed; fix conflicts and then commit the result.
```

```ini
[node name="Camera2D" type="Camera2D" parent="."]
<<<<<<< HEAD
zoom = Vector2(3, 3)
=======
zoom = Vector2(2, 2)
>>>>>>> feature
```

Everything else in the file is already merged.

## Installation

**Pre-built binaries** for Linux (static), macOS (Intel and Apple Silicon) and
Windows are attached to every [release](https://github.com/OWNER/scenemerge/releases).
Put `scenemerge` (or `scenemerge.exe`) somewhere on your `PATH`.

**With Cargo** (Rust 1.88 or newer):

```sh
cargo install --git https://github.com/OWNER/scenemerge scenemerge
```

## Set up Git (one command)

In your Godot project's repository:

```sh
scenemerge install
```

This adds to `.gitattributes`

```gitattributes
*.tscn diff=godot-scene merge=godot-scene
*.tres diff=godot-scene merge=godot-scene
```

and registers the drivers in `.git/config`:

```ini
[diff "godot-scene"]
    textconv = scenemerge textconv
[merge "godot-scene"]
    name = Godot semantic scene merge
    driver = scenemerge merge %O %A %B -o %A --path %P --marker-size %L --on-error text --label-base %S --label-ours %X --label-theirs %Y
```

Commit `.gitattributes` so that the whole team uses it; every team member runs
`scenemerge install` once (Git configuration is not versioned).

* `scenemerge install --global` configures all your repositories instead (your
  global Git config and global attributes file).
* `scenemerge install --diff semantic` makes `git diff` print the semantic
  change tree instead of a cleaned-up line diff.
* Running `install` again changes nothing; `scenemerge uninstall [--global]`
  removes exactly what was added.
* If a file cannot be merged semantically (e.g. a Godot 3 file), the driver
  falls back to Git's normal line-based merge (`--on-error text`).

`git diff`, `git log -p`, `git merge`, `git rebase`, `git cherry-pick` and
`git pull` then all use scenemerge for scenes.

## Commands

| command | what it does |
|---|---|
| `scenemerge diff OLD NEW` | semantic diff; `--format human` (default), `json` or `text` |
| `scenemerge merge BASE OURS THEIRS -o OUT` | three-way merge; exit 0 = clean, 1 = conflicts, 2 = error |
| `scenemerge textconv FILE` | canonical text form (what `git diff` compares) |
| `scenemerge check FILE_OR_DIR...` | parse, validate and verify lossless round-trip — handy in CI |
| `scenemerge install` / `uninstall` | set up / remove the Git integration |

Useful options:

* `diff --ignore-editor-metadata` ignores `metadata/_edit_*` properties (editor
  locks and the like); `--epsilon 0.0001` treats nearly equal floats as equal;
  `--exit-code` exits with 1 when there are differences.
* `merge --conflict markers|ours|theirs` chooses how conflicts are written;
  `--report conflicts.json` writes a machine-readable report.
* `diff --format json` and the merge report follow a versioned, documented
  schema: [docs/json-schema.md](docs/json-schema.md).

### What merges automatically

* Changes to different nodes, or to different properties of the same node.
* The same change on both sides.
* Nodes added, deleted, renamed, moved or reordered on one side — children,
  `parent=` paths and signal connections follow renames and moves.
* External and internal resources added on both sides, even with colliding
  local ids (the second one gets a fresh Godot-style id and every reference is
  rewritten); resources nobody uses any more are removed.
* Signal connections added on either side, or changed on one side.

### What is a conflict

* The same property changed to different values.
* A node deleted on one side and changed (or given new children) on the other.
* Two nodes added at the same path with different types or content.
* A node moved to different parents, or renamed differently.
* The same connection changed differently, or deleted on one side and
  changed on the other.

Conflict markers are placed around the smallest unit — usually a single
property line — so that you edit one line, not half the file.
`--conflict ours` / `--conflict theirs` resolve every conflict to one side
instead (still reporting them).

## How it works

* **Lossless parser.** scenemerge ports Godot's own text-resource parser and
  keeps every byte: reading and writing an unchanged file reproduces it exactly
  (verified on the complete godot-demo-projects corpus, and by fuzzing).
* **Identity, not position.** Nodes are matched by `unique_id` (Godot 4.6+),
  else by path, else by a rename/move heuristic; external resources by `uid` or
  path; sub-resources by id or content. Local ids are labels, nothing more.
* **Minimal output.** The merge result starts from your side of the merge and
  only touches what the other side changed, so the merge commit's diff is small
  and readable, and Godot re-saving the file afterwards changes nothing.
* **Checked against Godot.** CI loads every merge result, and real demo
  projects merged through `git merge`, in headless Godot 4.6 and 4.7.

Design notes and every verified detail of the file format are in
[DECISIONS.md](DECISIONS.md).

## Limitations

* Godot 4 only (`format=3` and `format=4`). Godot 3 (`format=2`) files are
  reported as unsupported (the Git driver falls back to a line merge).
* `NodePath` property values (e.g. `NodePath("../Player")`) are compared as
  text and are not rewritten when the other branch renames a node.
* Arrays and dictionaries are compared as a whole: changing the same
  animation track or the same array on both branches is a conflict even if
  different elements changed.
* Binary scenes (`.scn`, `.res`) are not supported — use the text formats in
  version control.
* Conflict markers make the file unloadable in Godot until they are resolved
  (as with any Git conflict); use `--conflict ours|theirs` to avoid that.

## Roadmap

* A GitHub Action that posts the semantic diff of changed scenes as a pull
  request comment.
* A Godot editor add-on that calls the CLI and shows the diff in the scene tree.
* Smart animation diffs (changed keyframes instead of huge arrays) and
  element-wise merge of arrays.
* Unity YAML (`.unity`, `.prefab`) through the same model layer.
* Godot 3 (`format=2`).

## Contributing

Bug reports with a small example scene are the most useful thing you can
send. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and propose
changes, and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE). Test fixtures from
[godot-demo-projects](https://github.com/godotengine/godot-demo-projects) are
MIT-licensed by the Godot Engine contributors (see `tests/fixtures/README.md`).
