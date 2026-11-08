# scene-merge

Three-way semantic merge of Godot 4 scenes and resources, part of
[scenemerge](https://github.com/safdafang/scenemerge).

```rust
let out = scene_merge::merge(&base, &ours, &theirs, &scene_merge::MergeOptions::default())?;
if out.is_clean() { std::fs::write("merged.tscn", &out.text)?; }
```

* Works on nodes, properties, resources and connections; the result is built
  from OURS so that untouched sections are copied byte for byte.
* Conflicts are reported per property/attribute/connection/node with
  `markers`, `ours` or `theirs` resolution and a JSON report.
* Colliding local ids are regenerated Godot-style and references rewritten;
  unused resources are removed; the result is validated before it is returned.

License: MIT.
