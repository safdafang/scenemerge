# scene-diff

Semantic diff of Godot 4 scenes and resources, part of
[scenemerge](https://github.com/OWNER/scenemerge).

* `diff(old, new, &options)` returns typed `Change`s (`NodeAdded`,
  `NodeRenamed`, `PropertyChanged` with component breakdown, `ConnectionAdded`, …).
* `render_human` prints a coloured change tree; `to_json` produces the
  documented JSON schema (`docs/json-schema.md`, `schema_version` 1);
  `canonical_text` is the normalised form used for Git `textconv`.
* Id renumbering, section order, `load_steps`, `unique_id` and whitespace are
  never reported.

License: MIT.
