//! Lossless reader and writer for Godot 4 text scenes and resources
//! (`.tscn`, `.tres`, `format=3`).
//!
//! The parser follows Godot's own `VariantParser` and `ResourceLoaderText`
//! (see `DECISIONS.md` in the repository for the details that were verified
//! against the engine source). Every byte of the input is kept: writing an
//! unmodified [`Document`] reproduces the input exactly, including line
//! endings, a byte order mark, comments and unusual spacing.
//!
//! ```
//! use godot_text::Document;
//!
//! let src = "[gd_scene format=3]\n\n[node name=\"Root\" type=\"Node2D\"]\nposition = Vector2(1, 2)\n";
//! let doc = Document::parse(src)?;
//! assert_eq!(doc.to_text(), src);
//! let root = &doc.sections[1];
//! assert_eq!(root.header.get_str("name"), Some("Root"));
//! assert_eq!(root.property("position").unwrap().value.to_string(), "Vector2(1, 2)");
//! # Ok::<(), godot_text::ParseError>(())
//! ```

mod document;
mod error;
mod lexer;
mod parser;
mod rewrite;
mod value;

pub use document::{
    encode_property_name, BodyItem, Document, Field, Header, LineEnding, Property, Section,
};
pub use error::ParseError;
pub use parser::parse_value_str;
pub use rewrite::rewrite_refs;
pub use value::{
    canonical_construct_name, escape_c, escape_multiline, ContainerType, Number, RefKind, Value,
};
