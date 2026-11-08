# JSON output

`scenemerge` produces two JSON documents:

* the **diff report**: `scenemerge diff --format json OLD NEW`;
* the **conflict report**: `scenemerge merge … --report FILE`.

Both carry a top-level `schema_version` (currently **1**). Within a schema
version, fields are only ever *added*; consumers must ignore unknown fields
and unknown `kind` values. Removing or renaming a field, or changing its
meaning, increments `schema_version`.

Conventions:

* **Node paths** are relative to the scene root, exactly as in `parent=`
  attributes: the root is `"."`, its children are `"Player"`, grandchildren
  `"Player/Camera3D"`.
* **Values** are strings in Godot's text syntax (`"Vector3(0, 1, 0)"`,
  `"75.0"`, `"\"text\""`), except that `ExtResource(...)` shows the **path** of
  the referenced file instead of the file-local id:
  `ExtResource("res://player.gd")`. `SubResource(...)` keeps the id.
* Optional fields are omitted when empty/absent.

---

## Diff report

```json
{
  "schema_version": 1,
  "files": [
    {
      "old_path": "scenes/level.tscn",
      "new_path": "scenes/level.tscn",
      "changes": [ { "kind": "...", ... } ]
    }
  ]
}
```

`changes` is ordered: file descriptor, external resources, sub-resources,
nodes (in tree order of the new version, removed nodes last), the
`[resource]` section, connections, editable markers.

### Targets

Property changes carry a `target`:

| `target.kind` | other fields | meaning |
|---|---|---|
| `node` | `path` | a node (path in the new version) |
| `sub_resource` | `id`, `type` | a `[sub_resource]` (id in the new version) |
| `resource` | (none) | the `[resource]` section of a `.tres` file |

### Change kinds

| `kind` | fields |
|---|---|
| `HeaderChanged` | `field`, `old`?, `new`?: a descriptor field such as `uid`, `format`, `type`, `script_class` (`load_steps` is ignored) |
| `ExtResourceAdded` | `path`, `type`, `uid`? |
| `ExtResourceRemoved` | `path`, `type`, `uid`? |
| `ExtResourceChanged` | `path` (new), `changes`: [`FieldChange`] for `path` / `type` / `uid` |
| `SubResourceAdded` | `id`, `type` |
| `SubResourceRemoved` | `id`, `type` |
| `SubResourceChanged` | `id`, `old_type`, `new_type` (property changes are separate `Property*` entries) |
| `NodeAdded` | `path`, `type`?, `instance`? (path of the instanced scene) |
| `NodeRemoved` | `path` (old), `type`? |
| `NodeRenamed` | `path` (old), `new_name`, `new_path` |
| `NodeMoved` | `path` (old), `new_path`: the parent changed (the name may have changed too) |
| `NodeReordered` | `path`, `old_position`, `new_position`: 0-based position among siblings |
| `NodeTypeChanged` | `path`, `old`?, `new`? |
| `InstanceChanged` | `path`, `old`?, `new`? (paths of the instanced scenes) |
| `GroupsChanged` | `path`, `added`: [string], `removed`: [string] |
| `NodeAttributeChanged` | `path`, `field`, `old`?, `new`? (`owner`, `index`, `instance_placeholder`, `node_paths`, …) |
| `PropertyAdded` | `target`, `property`, `new` |
| `PropertyRemoved` | `target`, `property`, `old` (the property is back to its default) |
| `PropertyChanged` | `target`, `property`, `old`, `new`, `components`? |
| `ConnectionAdded` | `signal`, `from`, `to`, `method` |
| `ConnectionRemoved` | `signal`, `from`, `to`, `method` |
| `ConnectionChanged` | `signal`, `from`, `to`, `method`, `changes`: [`FieldChange`] for `flags` / `binds` / `unbinds` |
| `EditableAdded` | `path` |
| `EditableRemoved` | `path` |

`FieldChange` = `{ "field": string, "old"?: string, "new"?: string }`.

`components` lists the parts of a compound value that changed, when they can
be identified: `{ "name": string, "old"?: string, "new"?: string }`. Names:

| value type | component names |
|---|---|
| `Vector2(i)` / `Vector3(i)` / `Vector4(i)` / `Quaternion` | `x`, `y`, `z`, `w` |
| `Color` | `r`, `g`, `b`, `a` |
| `Rect2(i)` / `AABB` | `position.x` …, `size.x` … |
| `Plane` | `normal.x`, `normal.y`, `normal.z`, `d` |
| `Transform2D` | `x.x`, `x.y`, `y.x`, `y.y`, `origin.x`, `origin.y` |
| `Basis` | `x.x` … `z.z` (axis, then component) |
| `Transform3D` | `basis.x.x` … `basis.z.z`, `origin.x`, `origin.y`, `origin.z` |
| `Projection` | `x.x` … `w.w` (column, then component) |
| arrays (same length) | `[index]` |
| dictionaries | `[key]` (`old` absent = key added, `new` absent = key removed) |

Arrays and dictionaries with more than 8 changed elements have no `components`.

### What is *not* reported

By design, these never produce changes: renumbered `ExtResource`/`SubResource`
ids, the order of `[ext_resource]`/`[sub_resource]`/`[connection]` sections,
`load_steps`, whitespace and comments, spelling of numbers (`1` vs `1.0` inside
constructors, `1.50` vs `1.5`), dictionary key order, `unique_id`,
`parent_id_path`, `owner_uid_path`, `from_uid_path` and `to_uid_path`
(see `DECISIONS.md`). With `--ignore-editor-metadata`, `metadata/_edit_*`
properties are skipped; with `--epsilon E`, floats that differ by at most `E`
are equal.

### Example

```json
{
  "schema_version": 1,
  "files": [
    {
      "old_path": "level.tscn",
      "new_path": "level.tscn",
      "changes": [
        {
          "kind": "PropertyChanged",
          "target": { "kind": "node", "path": "Player/Camera3D" },
          "property": "transform",
          "old": "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0)",
          "new": "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2.5, 0)",
          "components": [ { "name": "origin.y", "old": "1", "new": "2.5" } ]
        },
        {
          "kind": "NodeRenamed",
          "path": "Enemies/Enemy",
          "new_name": "Grunt",
          "new_path": "Enemies/Grunt"
        },
        {
          "kind": "ConnectionAdded",
          "signal": "body_entered",
          "from": "Player/Area3D",
          "to": "Player",
          "method": "_on_hit"
        }
      ]
    }
  ]
}
```

---

## Conflict report

Written by `scenemerge merge … --report FILE` (also when the merge is clean).

```json
{
  "schema_version": 1,
  "path": "scenes/player.tscn",
  "clean": false,
  "conflicts": [
    {
      "kind": "property",
      "target": "node:Camera3D",
      "property": "fov",
      "base": "75.0",
      "ours": "80.0",
      "theirs": "90.0",
      "resolution": "markers",
      "line": 6
    }
  ],
  "resolutions": [
    {
      "kind": "ext_resource_id",
      "target": "ext_resource:res://blue.png",
      "from": "2_xyz12",
      "to": "3_2ih0w"
    }
  ]
}
```

| field | meaning |
|---|---|
| `path` | the `--path` given (Git's `%P`), if any |
| `clean` | `true` when there are no conflicts (exit code 0) |
| `conflicts[].kind` | `property`, `node_attribute`, `rename`, `move`, `delete_modify`, `add_add`, `connection`, `ext_resource`, `header` |
| `conflicts[].target` | `node:<path>` (path in OURS, `.` = root), `sub_resource:<id>`, `resource`, `connection:<signal>:<from>-><to>::<method>`, `ext_resource:<path>`, `header` |
| `conflicts[].property` | the property, or the comma-separated header fields in conflict |
| `base` / `ours` / `theirs` | value text on each side; absent = did not exist or was deleted; for `delete_modify` the modified side is `"(modified)"` |
| `resolution` | `markers`, `ours` or `theirs` (the `--conflict` mode) |
| `line` | 1-based line of the `<<<<<<<` marker in the output (markers mode only) |
| `resolutions[]` | automatic adjustments: `kind` = `ext_resource_id`, `sub_resource_id` or `unique_id`, with `target`, `from` (the value in THEIRS) and `to` (the value written) |
