# Decisions and format notes

A log of architectural decisions and of facts about the Godot text formats that
were verified against the engine source. Engine references are to the `master`
branch of `godotengine/godot` as of 2026-11 (Godot 4.6+):

* `core/variant/variant_parser.cpp` — tokenizer (`get_token`), value parser
  (`parse_value`), tag parser (`_parse_tag`), property reader
  (`parse_tag_assign_eof`) and writer (`VariantWriter::write`);
* `scene/resources/resource_format_text.cpp` — `ResourceLoaderText` /
  `ResourceFormatSaverTextInstance`;
* `core/string/ustring.cpp` (`property_name_encode`, `c_escape`,
  `c_escape_multiline`), `core/io/resource.cpp` (`generate_scene_unique_id`),
  `core/io/resource_uid.cpp` (`id_to_text`/`text_to_id`).

Each entry: **Context → Decision → Consequences**.

---

## D1. Hand-written lexer and parser instead of `winnow`/`nom`

* **Context.** The brief suggests `winnow` or `nom`. The grammar is token-based,
  and Godot's own reader is a hand-written tokenizer plus a recursive-descent
  parser with several quirks (see the format notes below).
* **Decision.** Port `VariantParser::get_token`/`parse_value` structurally to a
  small hand-written lexer and parser (`crates/godot-text/src/{lexer,parser}.rs`).
* **Consequences.** Behaviour parity with Godot is easy to review side by side
  with the C++; byte spans for lossless output and precise error positions come
  for free; no parser-combinator dependency. Cost: ~700 lines of straightforward code.

## D2. Lossless document = ordered pieces with raw text

* **Context.** `write(parse(x)) == x` must hold byte for byte for any valid input.
* **Decision.** A `Document` is `bom` + `leading` trivia + `Section`s. A section
  is a `Header` (raw text from `[` to `]` plus parsed fields with their raw value
  text) and a body of `Property` (raw `lead` = key up to the start of the value,
  raw value text, parsed `Value`) and `Trivia` (whitespace, newlines, comments)
  items. Writing concatenates the raw pieces. Modified pieces are re-rendered in
  Godot's style; untouched pieces keep their original bytes.
* **Consequences.** CRLF, BOM, comments, odd spacing and multi-line values survive.
  The parse tree is the single source of truth for both comparison (AST) and output (raw).
  Verified on the 160 bundled fixtures and on the complete 533-file
  godot-demo-projects corpus; a mutation fuzzer additionally checks that *every*
  input the parser accepts round-trips.

## D3. Stricter than Godot on a few malformed inputs

Godot silently tolerates some malformed input; `scenemerge` reports an error
instead, because silently dropping data during a merge is worse than refusing:

| Input | Godot | scenemerge |
|---|---|---|
| trailing `key` without `=` at EOF | ignored (`ERR_FILE_EOF`) | error |
| `= value` with an empty key | confuses the loader | error |
| NUL character | treated as end of file | error |
| invalid UTF-8 | replacement characters | error |
| `#xyz` colour with length ≠ 3/4/6/8 digits | `Color::html` error, empty colour | error |
| invalid base64 in `PackedByteArray("...")` | error | error |
| > 128 levels of nested containers | up to its own recursion limit | error (protects the stack) |

## D4. Rust toolchain and MSRV

MSRV is 1.88 (the oldest toolchain available on the development machine, so
the MSRV is actually tested); CI builds it in a separate job.

## D5. Fuzzing on stable as well as with `cargo-fuzz`

`cargo-fuzz` needs nightly and libFuzzer, which is not available on every
development machine (notably Windows/MSVC without nightly). The `fuzz/` crate
holds the libFuzzer targets used by CI (Linux, nightly; 2 minutes per push,
10 minutes weekly). In addition `crates/godot-text/tests/fuzz_stable.rs` is a
dependency-free mutation fuzzer over the fixture corpus that runs on stable on
all platforms (3 s in normal test runs, `FUZZ_SECONDS=600` for long runs).

## D6. Accept `format=3` and `format=4`; reject `format<=2` with a clear message

* **Context.** The brief targets `format=3`. The engine source says otherwise:
  `ResourceLoaderText::FORMAT_VERSION = 4`, `FORMAT_VERSION_COMPAT = 3`. Since
  Godot 4.3 the saver writes `format=3` *unless* the file contains a
  `PackedVector4Array` or a `PackedByteArray` longer than 64 bytes (written as
  base64), in which case it writes `format=4`. 13 of the 533 demo files are `format=4`.
* **Decision.** Both 3 and 4 are Godot 4 text formats and are fully supported
  (`scene_model::MAX_FORMAT = 4`). `format=2` (Godot 3) and lower produce
  `unsupported file format 2: Godot 3 (format=2) files are not supported…`;
  `format>4` produces "saved by a newer Godot version".
* **Consequences.** Merges keep the `format` value of OURS; if the merged
  content needs format 4 (it contains a base64 byte array or `PackedVector4Array`)
  the writer bumps it to 4.

## D7. Identity matching (spec §4)

Implemented in `scene_model::Matching`:

1. **ext_resource** — same `uid`; then same `path`. Ids are never compared.
2. **sub_resource** — same `id`; then same `type` + equal *normalised*
   content, processed in file order so that nested references
   (`GradientTexture1D` → `Gradient`) resolve first. Instead of a content hash
   the candidates are compared directly with `semantic_eq` (only unmatched
   entries are compared, so the cost is negligible and there are no hash collisions).
3. **node** — `unique_id` when present on both sides and unique in each file
   (duplicates disable the rule for that id); the roots always correspond; then
   *same name under the matched parent* (a path match that also works under a
   renamed ancestor); then the rename/move heuristic, alternating with the path
   pass until nothing changes, so that children of a renamed node match by path.
   Nodes whose parent is not in the file (overrides in inherited scenes) are
   matched by their `parent=` text.
4. **Heuristic** — only pairs with the same `type` and the same normalised
   `instance`. Score = `0.6·content + 0.25·[same parent] + 0.15·[same name]`;
   content = (Σ per property: 1 if equal, 0.5 if only the key matches, + number of
   common child names) / (union of property keys + union of child names), or 1
   when both nodes are empty. Default threshold `0.65`
   (`--rename-threshold`). Pairs whose parents are already matched are tried
   first (renames, moves between existing parents), then all remaining pairs
   (moves into new parents). Greedy by score, ties by file order → deterministic.
   Consequence: an *empty* node deleted and another empty node of the same type
   added under the same parent is reported as a rename; for scenes saved by
   Godot 4.6+ `unique_id` makes this exact anyway.
5. **Normalisation** — `ExtResource("id")`/`SubResource("id")` are replaced by
   tokens naming the matched entity before values are compared, so id
   renumbering never shows up as a property change.

---

# Format notes (verified)

### Tokens (`VariantParser::get_token`)

* Whitespace is **any** character with code `<= 32`, so `\r` is just whitespace;
  CRLF needs no special handling in the grammar.
* `;` starts a comment **anywhere outside a string** — in headers, between
  values, after a property — not only at the start of a line. Godot drops
  comments when it re-saves a file.
* Strings are raw and may contain literal newlines. Escapes: `\b \t \n \f \r`,
  `\uXXXX` (4 hex digits), `\UXXXXXX` (6), UTF-16 surrogate pairs are combined,
  and **any other escaped character stands for itself** (`\"`, `\\`, `\'`, `\q` → `q`).
* `&"..."` is a `StringName`; `@"..."` is accepted for 3.x compatibility.
* `#` followed by hex digits is a colour literal (`Color::html`).
* Numbers: optional `-`, digits, optional `.digits`, optional `e[+-]digits`; a
  literal is a float iff it contains `.` or `e`. There is no leading-dot form
  (`.5`) and no hex. `-` followed by a letter makes an identifier (`-inf`).
* Identifiers: `[A-Za-z_][A-Za-z0-9_]*`.

### Values (`parse_value`)

* Keywords: `true`, `false`, `null`, `nil`, `inf`, `-inf`, `inf_neg`, `nan`.
* Fixed-arity constructors (aliases): `Vector2(i)`/2, `Vector3(i)`/3,
  `Vector4(i)`/4, `Rect2(i)`/4, `Plane`/4, `Quaternion`(`Quat`)/4, `Color`/4,
  `Transform2D`(`Matrix32`)/6, `AABB`(`Rect3`)/6, `Basis`(`Matrix3`)/9,
  `Transform3D`(`Transform`)/12, `Projection`/16. Arguments are numbers (or `inf`/`nan`);
  a trailing comma is an error.
* Packed arrays and their 3.x aliases (`PoolIntArray`, `Vector2Array`, …) take
  any number of numeric arguments; `PackedStringArray` takes strings;
  `PackedByteArray` takes either a single base64 string or a list of numbers.
* `NodePath("...")`, `RID()`/`RID(n)`, `Signal()`, `Callable()`.
* `ExtResource(...)` / `SubResource(...)` take a string id or a number
  (old-style ids, `ExtResource(1)` and `ExtResource("1")` are the same id).
* `Resource("path")` or `Resource("uid://...", "path")` (either order).
* `Object(Class, "prop": value, ...)`.
* Typed containers: `Array[T]([ ... ])`, `Dictionary[K, V]({ ... })` where a
  type is an identifier or a script reference (`ExtResource("..")`, …); the bare
  identifier `Resource` followed by `]`/`,` is the class name `Resource`.

### Writer (`VariantWriter::write`) — used when we have to produce new text

* `String` → `"` + escape only `\` and `"` (`c_escape_multiline`) + `"`.
* `StringName`, `NodePath` and string header fields use full `c_escape`.
* Dictionaries are written one entry per line (`{\n"k": v,\n"k2": v2\n}`), `{}` when empty,
  with keys sorted; arrays inline with `", "`.
* Floats: `-0` is written as `0`; integral floats get `.0` appended, but not inside
  math constructors (`Vector2(1, 2)`).
* Property names are quoted (and escaped) only if they contain `= " ; [ ]`,
  a character `< 33` or a character `> 126` (`String::property_name_encode`).

### Sections (`resource_format_text.cpp`)

* `format=` is 3 or 4 in Godot 4 (see D6). The file descriptor is `[gd_scene format=3 uid="..."]` or
  `[gd_resource type="T" script_class="C" format=3 uid="..."]`; `script_class`
  (global class name of the resource's script) is not mentioned in the docs.
  `load_steps` was written before 4.6 and is ignored by current readers.
* Node header fields written by the saver, in this order: `name`, `type`,
  `parent`, `parent_id_path`, `owner`, `owner_uid_path`, `index` (written as a
  **string**, `index="3"`), `unique_id`, `node_paths`, `groups` (sorted),
  `instance_placeholder`, `instance`.
  `parent_id_path`, `owner_uid_path` (nodes) and `from_uid_path`, `to_uid_path`
  (connections) are 4.6 additions not described in the docs: `PackedInt32Array`s of
  `unique_id`s that locate the parent/owner/endpoint inside *instanced or inherited*
  scenes, so that the link survives renames in the base scene.
* A node without `type` is either an instance (`instance=`) or an override of a
  node that comes from an instanced/inherited scene.
* If the first node has `instance=`, the file is an **inherited scene**.
* `[connection]` requires `signal`, `from`, `to`, `method`; `flags` defaults to
  `CONNECT_PERSIST` (2) and is written only when different; `unbinds` only when
  `> 0`; `binds` is written as ` binds= [..]` (note the space).
* `[editable path="..."]` marks instanced children as editable.
* The loader accepts `node`, `connection` and `editable` sections in any order
  after the resources; the saver writes nodes → connections → editables.
* Sub-resources must be defined **before** they are referenced
  ("sub-resources stored out of order" error).
* An internal and an external resource may share the same local id because the
  reference syntax differs.
* Duplicate properties: the last one wins.

### Identifiers generated by the editor

* Ext-resource ids: `"<n>_<5 chars>"` where `<n>` is the 1-based index when first
  saved; sub-resource ids: `"<Type>_<5 chars>"`.
* The 5 characters come from `Resource::generate_scene_unique_id`, which uses
  `char_count = 'z' - 'a'` = 25 and `base = 25 + ('9' - '0')` = 34, so the
  alphabet is actually **`a`–`y` and `0`–`8`** (an off-by-one in the engine).
  Generated ids reproduce this alphabet.
* UIDs (`ResourceUID::id_to_text`) use the **same 34-character alphabet**
  (`uuid_characters` = `a`–`y`, `0`–`8`, base 34), most significant digit first,
  for a 63-bit value (at most 13 characters, e.g. `uid://d4n4ub6itg400`).
  `text_to_id` rejects any other character, so a UID containing `z` or `9` is invalid.

### Corpus observations (godot-demo-projects @ b761b4c, 533 files)

* 520 files are `format=3` and 13 are `format=4`; all have LF line endings, no BOM; 394 contain `unique_id`;
  only 3 still contain `load_steps`; 14 contain `parent_id_path`; none contain
  `owner_uid_path`, `from_uid_path`, `to_uid_path`, `instance_placeholder`, `flags`
  or `unbinds` (covered by synthetic fixtures instead).
* Several files contain lines starting with `[` (`[cell]`, `[ul]`, `[font_size=…]`)
  inside multi-line BBCode strings — a line-based "section starts with `[`"
  heuristic would corrupt them.
