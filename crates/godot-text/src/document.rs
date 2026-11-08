//! Lossless document model: every byte of the input belongs to exactly one
//! piece (leading trivia, a section header, a property or trivia), so writing
//! the pieces back in order reproduces the input byte for byte.

use std::fmt;

use crate::error::ParseError;
use crate::lexer::{Lexer, Punct, TokKind};
use crate::parser::{parse_value, parse_value_str};
use crate::value::Value;

/// Line terminator style of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`
    #[default]
    Lf,
    /// `\r\n`
    CrLf,
}

impl LineEnding {
    /// The terminator text.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// A `key=value` field of a section header, e.g. `name="Player"`.
#[derive(Debug, Clone)]
pub struct Field {
    /// Field name.
    pub key: String,
    /// Parsed value.
    pub value: Value,
    /// The value exactly as written.
    pub raw_value: String,
}

/// A section header such as `[node name="Player" type="CharacterBody2D" parent="."]`.
#[derive(Debug, Clone)]
pub struct Header {
    /// Tag name: `gd_scene`, `gd_resource`, `ext_resource`, `sub_resource`,
    /// `node`, `connection`, `editable` or `resource`.
    pub name: String,
    /// Fields in source order.
    pub fields: Vec<Field>,
    /// Exact source text from `[` to `]` inclusive.
    pub raw: String,
    /// 1-based line of the `[`.
    pub line: usize,
}

impl Header {
    /// Builds a header from already-formatted field values, in Godot's style
    /// (`[name key=value key2=value2]`).
    pub fn from_raw_fields<'a>(
        name: &str,
        fields: impl IntoIterator<Item = (&'a str, String)>,
    ) -> Result<Header, ParseError> {
        let mut parsed = Vec::new();
        let mut raw = format!("[{name}");
        for (key, raw_value) in fields {
            let value = parse_value_str(&raw_value)?;
            raw.push(' ');
            raw.push_str(key);
            raw.push('=');
            raw.push_str(&raw_value);
            parsed.push(Field {
                key: key.to_string(),
                value,
                raw_value,
            });
        }
        raw.push(']');
        Ok(Header {
            name: name.to_string(),
            fields: parsed,
            raw,
            line: 0,
        })
    }

    /// The last field with the given key (Godot keeps the last duplicate).
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().rev().find(|f| f.key == key)
    }

    /// The value of field `key`, if present.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.field(key).map(|f| &f.value)
    }

    /// The value of field `key` if it is a string (or string name).
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::String(s) | Value::StringName(s) => Some(s),
            _ => None,
        }
    }

    /// Rebuilds the header text from its fields (used after modification).
    pub fn rebuild_raw(&mut self) {
        let mut raw = format!("[{}", self.name);
        for f in &self.fields {
            raw.push(' ');
            raw.push_str(&f.key);
            raw.push('=');
            raw.push_str(&f.raw_value);
        }
        raw.push(']');
        self.raw = raw;
    }

    /// Sets (or appends) field `key` to the given raw value text and rebuilds the header.
    pub fn set_field(&mut self, key: &str, raw_value: String) -> Result<(), ParseError> {
        let value = parse_value_str(&raw_value)?;
        if let Some(f) = self.fields.iter_mut().rev().find(|f| f.key == key) {
            f.value = value;
            f.raw_value = raw_value;
        } else {
            self.fields.push(Field {
                key: key.to_string(),
                value,
                raw_value,
            });
        }
        self.rebuild_raw();
        Ok(())
    }

    /// Removes every field named `key`. Returns whether anything was removed.
    pub fn remove_field(&mut self, key: &str) -> bool {
        let before = self.fields.len();
        self.fields.retain(|f| f.key != key);
        let removed = self.fields.len() != before;
        if removed {
            self.rebuild_raw();
        }
        removed
    }
}

/// A `key = value` property line inside a section.
#[derive(Debug, Clone)]
pub struct Property {
    /// Decoded property name, e.g. `surface_material_override/0`.
    pub key: String,
    /// Exact source text from the start of the key up to the start of the
    /// value (typically `key = `).
    pub lead: String,
    /// The value exactly as written (may span several lines).
    pub raw_value: String,
    /// Parsed value.
    pub value: Value,
    /// 1-based line where the property starts.
    pub line: usize,
}

impl Property {
    /// Creates a property in Godot's style: `key = value`.
    pub fn new(key: &str, raw_value: String) -> Result<Property, ParseError> {
        let value = parse_value_str(&raw_value)?;
        Ok(Property {
            key: key.to_string(),
            lead: format!("{} = ", encode_property_name(key)),
            raw_value,
            value,
            line: 0,
        })
    }

    /// Replaces the value, keeping the key spelling.
    pub fn set_raw_value(&mut self, raw_value: String) -> Result<(), ParseError> {
        self.value = parse_value_str(&raw_value)?;
        self.raw_value = raw_value;
        Ok(())
    }

    /// The exact source text of the property.
    pub fn raw(&self) -> String {
        format!("{}{}", self.lead, self.raw_value)
    }
}

/// Quotes a property name if Godot would (`String::property_name_encode`).
pub fn encode_property_name(name: &str) -> String {
    let needs_quotes = name
        .chars()
        .any(|c| matches!(c, '=' | '"' | ';' | '[' | ']') || (c as u32) < 33 || (c as u32) > 126);
    if needs_quotes {
        format!("\"{}\"", crate::value::escape_multiline(name))
    } else {
        name.to_string()
    }
}

/// One element of a section body.
#[derive(Debug, Clone)]
pub enum BodyItem {
    /// Whitespace, line breaks and `;` comments, exactly as written.
    Trivia(String),
    /// A property assignment.
    Property(Property),
}

/// A section: a header followed by its body.
#[derive(Debug, Clone)]
pub struct Section {
    /// The `[...]` header.
    pub header: Header,
    /// Properties and trivia following the header, up to the next header.
    pub body: Vec<BodyItem>,
}

impl Section {
    /// Creates a section with no body.
    pub fn new(header: Header) -> Self {
        Section {
            header,
            body: Vec::new(),
        }
    }

    /// Iterates over the properties of the section.
    pub fn properties(&self) -> impl Iterator<Item = &Property> {
        self.body.iter().filter_map(|b| match b {
            BodyItem::Property(p) => Some(p),
            BodyItem::Trivia(_) => None,
        })
    }

    /// Mutable iteration over the properties of the section.
    pub fn properties_mut(&mut self) -> impl Iterator<Item = &mut Property> {
        self.body.iter_mut().filter_map(|b| match b {
            BodyItem::Property(p) => Some(p),
            BodyItem::Trivia(_) => None,
        })
    }

    /// The last property with the given key.
    pub fn property(&self, key: &str) -> Option<&Property> {
        self.properties().filter(|p| p.key == key).last()
    }

    /// Writes the section's exact text.
    pub fn write_to(&self, out: &mut String) {
        out.push_str(&self.header.raw);
        for item in &self.body {
            match item {
                BodyItem::Trivia(t) => out.push_str(t),
                BodyItem::Property(p) => {
                    out.push_str(&p.lead);
                    out.push_str(&p.raw_value);
                }
            }
        }
    }

    /// The section's exact text.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        self.write_to(&mut s);
        s
    }
}

/// A parsed `.tscn`/`.tres` file.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Whether the file started with a UTF-8 byte order mark.
    pub bom: bool,
    /// Trivia before the first section.
    pub leading: String,
    /// Sections in file order.
    pub sections: Vec<Section>,
}

impl Document {
    /// Parses UTF-8 bytes (an optional BOM is recognised and preserved).
    pub fn parse_bytes(bytes: &[u8]) -> Result<Document, ParseError> {
        match std::str::from_utf8(bytes) {
            Ok(s) => Document::parse(s),
            Err(e) => {
                let valid = bytes.get(..e.valid_up_to()).unwrap_or(&[]);
                let prefix = String::from_utf8_lossy(valid);
                let mut err = ParseError::at(&prefix, prefix.len(), "File is not valid UTF-8");
                if prefix.starts_with('\u{feff}') {
                    err.column = err.column.saturating_sub(usize::from(err.line == 1));
                }
                Err(err)
            }
        }
    }

    /// Parses a document from text.
    pub fn parse(src: &str) -> Result<Document, ParseError> {
        let (bom, text) = match src.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, src),
        };
        DocParser::new(text).parse().map(|mut d| {
            d.bom = bom;
            d
        })
    }

    /// Writes the document back to text. For an unmodified document this is
    /// byte-for-byte identical to the input.
    pub fn to_text(&self) -> String {
        let cap = self.leading.len()
            + self
                .sections
                .iter()
                .map(|s| s.header.raw.len() + 64)
                .sum::<usize>();
        let mut out = String::with_capacity(cap);
        if self.bom {
            out.push('\u{feff}');
        }
        out.push_str(&self.leading);
        for s in &self.sections {
            s.write_to(&mut out);
        }
        out
    }

    /// The line ending used by the document (decided by its first line break).
    pub fn line_ending(&self) -> LineEnding {
        let mut buf = String::new();
        buf.push_str(&self.leading);
        for s in &self.sections {
            s.write_to(&mut buf);
            if let Some(i) = buf.find('\n') {
                return if i > 0 && buf.as_bytes().get(i - 1) == Some(&b'\r') {
                    LineEnding::CrLf
                } else {
                    LineEnding::Lf
                };
            }
        }
        LineEnding::Lf
    }

    /// Iterates over sections with the given header name.
    pub fn sections_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Section> + 'a {
        self.sections.iter().filter(move |s| s.header.name == name)
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

/// Maps byte offsets to 1-based line numbers.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn line(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

struct DocParser<'a> {
    lx: Lexer<'a>,
    lines: LineIndex,
}

impl<'a> DocParser<'a> {
    fn new(src: &'a str) -> Self {
        DocParser {
            lx: Lexer::new(src),
            lines: LineIndex::new(src),
        }
    }

    fn parse(mut self) -> Result<Document, ParseError> {
        // Leading trivia: whitespace and comments before the first '['.
        self.lx.skip_trivia();
        let leading = self.lx.slice(0, self.lx.pos).to_string();
        let mut sections = Vec::new();
        match self.lx.peek() {
            None => {}
            Some('[') => loop {
                let header = self.parse_header()?;
                let (body, more) = self.parse_body()?;
                sections.push(Section { header, body });
                if !more {
                    break;
                }
            },
            Some(_) => {
                return Err(self.lx.error(
                    self.lx.pos,
                    "Expected '[' (a section header such as [gd_scene ...])",
                ))
            }
        }
        Ok(Document {
            bom: false,
            leading,
            sections,
        })
    }

    /// Parses `[tag key=value ...]` starting at the `[`.
    fn parse_header(&mut self) -> Result<Header, ParseError> {
        let start = self.lx.pos;
        let open = self.lx.next_token()?;
        if open.kind != TokKind::Punct(Punct::BracketOpen) {
            return Err(self.lx.error(open.start, "Expected '['"));
        }
        let t = self.lx.next_token()?;
        let TokKind::Ident(name) = t.kind else {
            return Err(self.lx.error(
                t.start,
                format!("Expected identifier (tag name), got {}", t.kind.describe()),
            ));
        };
        let mut name = name.to_string();
        let mut fields = Vec::new();
        let mut parsing_tag = true;
        loop {
            let mut t = self.lx.next_token()?;
            match t.kind {
                TokKind::Punct(Punct::BracketClose) => break,
                TokKind::Eof => {
                    return Err(self
                        .lx
                        .error(start, format!("Unexpected EOF while parsing tag '{name}'")))
                }
                TokKind::Punct(p @ (Punct::Period | Punct::Colon)) if parsing_tag => {
                    name.push(if p == Punct::Period { '.' } else { ':' });
                    t = self.lx.next_token()?;
                }
                _ => parsing_tag = false,
            }
            let TokKind::Ident(key) = t.kind else {
                return Err(self.lx.error(
                    t.start,
                    format!(
                        "Expected field name in tag '{name}', got {}",
                        t.kind.describe()
                    ),
                ));
            };
            if parsing_tag {
                name.push_str(key);
                continue;
            }
            let eq = self.lx.next_token()?;
            if eq.kind != TokKind::Punct(Punct::Equal) {
                return Err(self.lx.error(
                    eq.start,
                    format!(
                        "Expected '=' after field '{key}', got {}",
                        eq.kind.describe()
                    ),
                ));
            }
            let vt = self.lx.next_token()?;
            let vstart = vt.start;
            let value = parse_value(&mut self.lx, vt, 0)?;
            fields.push(Field {
                key: key.to_string(),
                value,
                raw_value: self.lx.slice(vstart, self.lx.pos).to_string(),
            });
        }
        Ok(Header {
            name,
            fields,
            raw: self.lx.slice(start, self.lx.pos).to_string(),
            line: self.lines.line(start),
        })
    }

    /// Parses properties and trivia after a header. Returns the body and
    /// whether another section follows.
    fn parse_body(&mut self) -> Result<(Vec<BodyItem>, bool), ParseError> {
        let mut body = Vec::new();
        loop {
            let trivia_start = self.lx.pos;
            let next = loop {
                match self.lx.peek() {
                    None => break None,
                    Some(';') => self.lx.skip_comment(),
                    Some(c) if (c as u32) <= 32 && c != '\0' => {
                        self.lx.bump();
                    }
                    Some(c) => break Some(c),
                }
            };
            if self.lx.pos > trivia_start {
                body.push(BodyItem::Trivia(
                    self.lx.slice(trivia_start, self.lx.pos).to_string(),
                ));
            }
            match next {
                None => return Ok((body, false)),
                Some('[') => return Ok((body, true)),
                Some(_) => body.push(BodyItem::Property(self.parse_property()?)),
            }
        }
    }

    /// Parses `key = value`, following Godot's `parse_tag_assign_eof`: the key
    /// is every non-whitespace character before `=` (or a quoted string).
    fn parse_property(&mut self) -> Result<Property, ParseError> {
        let start = self.lx.pos;
        let mut key = String::new();
        loop {
            match self.lx.peek() {
                None => {
                    return Err(self.lx.error(
                        start,
                        format!("Unexpected end of file: expected '=' after property '{key}'"),
                    ))
                }
                Some('\0') => return Err(self.lx.error(self.lx.pos, "Unexpected NUL character")),
                Some(';') => self.lx.skip_comment(),
                Some('"') => {
                    let t = self.lx.next_token()?;
                    match t.kind {
                        TokKind::Str(s) => key = s,
                        _ => return Err(self.lx.error(t.start, "Error reading quoted string")),
                    }
                }
                Some('=') => {
                    self.lx.bump();
                    break;
                }
                Some(c) => {
                    if (c as u32) > 32 {
                        key.push(c);
                    }
                    self.lx.bump();
                }
            }
        }
        if key.is_empty() {
            return Err(self.lx.error(start, "Empty property name before '='"));
        }
        let vt = self.lx.next_token()?;
        let vstart = vt.start;
        if vt.kind == TokKind::Eof {
            return Err(self
                .lx
                .error(vstart, format!("Expected value for property '{key}'")));
        }
        let value = parse_value(&mut self.lx, vt, 0)?;
        Ok(Property {
            key,
            lead: self.lx.slice(start, vstart).to_string(),
            raw_value: self.lx.slice(vstart, self.lx.pos).to_string(),
            value,
            line: self.lines.line(start),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[gd_scene format=3 uid=\"uid://abc\"]\n\n\
[ext_resource type=\"Script\" path=\"res://a.gd\" id=\"1_x\"]\n\n\
[node name=\"Root\" type=\"Node2D\"]\n\
script = ExtResource(\"1_x\")\n\
text = \"line1\n[not_a_section]\n\"\n\
metadata/_edit_lock_ = true\n\n\
[connection signal=\"ready\" from=\".\" to=\".\" method=\"_on_ready\" binds= [0]]\n";

    #[test]
    fn round_trip_and_structure() {
        let d = Document::parse(SAMPLE).unwrap();
        assert_eq!(d.to_text(), SAMPLE);
        assert_eq!(d.sections.len(), 4);
        let node = &d.sections[2];
        assert_eq!(node.header.get_str("name"), Some("Root"));
        assert_eq!(node.header.line, 5);
        let props: Vec<_> = node.properties().map(|p| p.key.as_str()).collect();
        assert_eq!(props, ["script", "text", "metadata/_edit_lock_"]);
        assert!(matches!(
            node.property("text").unwrap().value,
            Value::String(ref s) if s.contains("[not_a_section]")
        ));
        assert_eq!(
            d.sections[3].header.field("binds").unwrap().raw_value,
            "[0]"
        );
    }

    #[test]
    fn crlf_bom_and_comments() {
        let src = "\u{feff}; leading comment\r\n[gd_resource type=\"Resource\" format=3]\r\n\r\n[resource]\r\na = 1 ; trailing\r\n\"quoted key\" = 2\r\n";
        let d = Document::parse(src).unwrap();
        assert!(d.bom);
        assert_eq!(d.to_text(), src);
        assert_eq!(d.line_ending(), LineEnding::CrLf);
        let keys: Vec<_> = d.sections[1].properties().map(|p| p.key.clone()).collect();
        assert_eq!(keys, ["a", "quoted key"]);
    }

    #[test]
    fn errors_point_to_location() {
        let e = Document::parse("[gd_scene format=3]\n\n[node name=\"A\"]\nfoo = Vector2(1)\n")
            .unwrap_err();
        assert_eq!(e.line, 4);
        assert!(e.to_string().contains("Vector2"), "{e}");
        assert!(Document::parse("garbage").is_err());
        assert!(Document::parse("[gd_scene\n").is_err());
        assert!(Document::parse("[a]\nkey_without_value").is_err());
        assert!(Document::parse_bytes(b"[a]\n\xff").is_err());
    }

    #[test]
    fn empty_and_trivia_only() {
        for src in ["", "\n\n", "; only a comment"] {
            let d = Document::parse(src).unwrap();
            assert_eq!(d.to_text(), src);
        }
    }

    #[test]
    fn edit_property_and_header() {
        let mut d = Document::parse(SAMPLE).unwrap();
        let node = &mut d.sections[2];
        node.header.set_field("name", "\"Main\"".into()).unwrap();
        assert_eq!(node.header.raw, "[node name=\"Main\" type=\"Node2D\"]");
        for p in node.properties_mut() {
            if p.key == "metadata/_edit_lock_" {
                p.set_raw_value("false".into()).unwrap();
            }
        }
        assert!(d.to_text().contains("metadata/_edit_lock_ = false\n"));
        assert_eq!(encode_property_name("a b"), "\"a b\"");
        assert_eq!(encode_property_name("bones/1/position"), "bones/1/position");
    }
}
