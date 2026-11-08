# Test fixtures

All files in this directory are test inputs and are checked out byte-for-byte
(`-text` in `.gitattributes`): never let an editor or Git re-encode them.

## `demo/`

160 `.tscn`/`.tres` files copied unchanged from
[godotengine/godot-demo-projects](https://github.com/godotengine/godot-demo-projects),
commit `b761b4cd5718a7118f2c95eb55f181d0a5f004f1` (2026-10-08), saved by Godot 4.6+.

* License: MIT, see [`demo/LICENSE.godot-demo-projects.md`](demo/LICENSE.godot-demo-projects.md)
  (Copyright (c) 2014-present Godot Engine contributors).
* File names are the original repository paths with `/` replaced by `--`,
  e.g. `3d--platformer--player--player.tscn` is `3d/platformer/player/player.tscn`.
* The selection was made by a script that guarantees, for each of these features,
  at least the two smallest files that contain it: `unique_id`, `load_steps`,
  `parent_id_path`, `[editable]`, `node_paths`, `groups`, `index`, `script_class`,
  `binds`, typed `Dictionary[...]`/`Array[...]`, base64 `PackedByteArray`, BBCode
  lines starting with `[` inside multi-line strings, scene instances, inherited
  scenes (override nodes without `type`), `Animation`/`AnimationLibrary`,
  connections, `StringName`, `Transform3D`, `Color`, shaders, `TileSet`, `Theme`,
  numeric (pre-4.0 style) ext-resource ids, `NodePath`, packed arrays, escaped
  quotes and multi-line strings. The rest was filled up with small files picked
  evenly from every top-level category (2D, 3D, GUI, audio, networking, XR, ...).

The whole upstream corpus (533 files at that commit) also round-trips; run it with

```sh
SCENEMERGE_CORPUS=/path/to/godot-demo-projects cargo test -p godot-text --test roundtrip
```

## `synthetic/`

Hand-written or derived files that cover what the demo corpus does not:

| File | Purpose |
|---|---|
| `all-variant-types.tres` | every Variant literal type Godot's parser accepts |
| `comments-and-spacing.tscn` | `;` comments in headers/values, odd spacing, quoted keys, indented keys |
| `no-unique-id.tscn` | pre-4.6 scene: `load_steps`, no `unique_id`, groups, flags/unbinds/binds |
| `inherited-scene.tscn` | inherited scene: `parent_id_path`, `owner_uid_path`, `instance_placeholder`, `[editable]` |
| `crlf--*` | demo files converted to CRLF line endings |
| `bom--*` | demo files with a UTF-8 byte order mark prepended |

## `invalid/`

Inputs that must be rejected with a precise location (or, for
`godot3-format2.tscn`, parse but be rejected as an unsupported format).
