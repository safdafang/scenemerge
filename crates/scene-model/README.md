# scene-model

Semantic model of Godot 4 scenes and resources, part of
[scenemerge](https://github.com/OWNER/scenemerge).

* `SceneFile`: typed view of a parsed file — descriptor, `ext_resource`,
  `sub_resource`, node tree, connections, `[editable]`, `[resource]`.
* `SceneFile::validate`: the structural rules Godot enforces when loading
  (single root, existing parents, references defined before use, …).
* `Matching`: which entities of two versions are the same — nodes by
  `unique_id`, path or a rename/move heuristic, external resources by uid/path,
  sub-resources by id or content — and value normalisation so that renumbered
  local ids compare equal.

License: MIT.
