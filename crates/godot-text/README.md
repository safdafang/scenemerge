# godot-text

Lossless lexer, parser and writer for Godot 4 text scenes and resources
(`.tscn`, `.tres`; `format=3` and `format=4`), part of
[scenemerge](https://github.com/OWNER/scenemerge).

* Ports Godot's `VariantParser` tokenizer and value grammar (every Variant
  literal, typed arrays/dictionaries, `Object(...)`, packed arrays, …).
* Keeps every byte: `Document::parse(x)?.to_text() == x` for any valid input,
  including CRLF, BOM, `;` comments and multi-line strings.
* Parse errors carry line, column and the offending source line.
* `rewrite_refs` rewrites `ExtResource`/`SubResource` ids at token level.

```rust
let doc = godot_text::Document::parse(text)?;
for section in &doc.sections {
    println!("[{}] {:?}", section.header.name, section.header.get_str("name"));
}
assert_eq!(doc.to_text(), text);
```

License: MIT.
