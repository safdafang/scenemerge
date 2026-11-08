use godot_text::{BodyItem, Header, ParseError, Property, Section};

const NODE_FIELD_ORDER: &[&str] = &[
    "name",
    "type",
    "parent",
    "parent_id_path",
    "owner",
    "owner_uid_path",
    "index",
    "unique_id",
    "node_paths",
    "groups",
    "instance_placeholder",
    "instance",
];

const CONNECTION_FIELD_ORDER: &[&str] = &[
    "signal",
    "from",
    "to",
    "method",
    "flags",
    "from_uid_path",
    "to_uid_path",
    "unbinds",
    "binds",
];

/// Converts line breaks in `text` to `eol`.
pub(crate) fn with_eol(text: &str, eol: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    if eol == "\n" {
        lf
    } else {
        lf.replace('\n', eol)
    }
}

/// Sets header field `key` to `raw`, inserting a missing field at the
/// position Godot's saver would use.
pub(crate) fn set_header_field(h: &mut Header, key: &str, raw: String) -> Result<(), ParseError> {
    if h.field(key).is_some() {
        return h.set_field(key, raw);
    }
    let order: &[&str] = match h.name.as_str() {
        "node" => NODE_FIELD_ORDER,
        "connection" => CONNECTION_FIELD_ORDER,
        _ => &[],
    };
    let rank = |k: &str| order.iter().position(|o| *o == k);
    let my_rank = rank(key);
    let insert_at = my_rank.and_then(|r| {
        h.fields
            .iter()
            .position(|f| rank(&f.key).is_some_and(|fr| fr > r))
    });
    h.set_field(key, raw)?;
    if let Some(pos) = insert_at {
        if let Some(f) = h.fields.pop() {
            h.fields.insert(pos, f);
        }
        h.rebuild_raw();
    }
    Ok(())
}

fn prop_index(s: &Section, key: &str) -> Option<usize> {
    s.body
        .iter()
        .rposition(|b| matches!(b, BodyItem::Property(p) if p.key == key))
}

/// Sets the value of property `key` (which must exist).
pub(crate) fn set_property(
    s: &mut Section,
    key: &str,
    raw: String,
    eol: &str,
) -> Result<bool, ParseError> {
    let Some(i) = prop_index(s, key) else {
        return Ok(false);
    };
    if let Some(BodyItem::Property(p)) = s.body.get_mut(i) {
        p.set_raw_value(raw)?;
    }
    if matches!(s.body.get(i + 1), Some(BodyItem::Property(_))) {
        separate_after(s, i, eol);
    }
    Ok(true)
}

fn insert_line(s: &mut Section, after: Option<&str>, item: BodyItem, eol: &str) {
    let at = after.and_then(|k| prop_index(s, k)).map_or(0, |i| i + 1);
    s.body.insert(at, item);
    s.body.insert(at, BodyItem::Trivia(eol.to_string()));
    separate_after(s, at + 1, eol);
}

fn separate_after(s: &mut Section, i: usize, eol: &str) {
    let needs_break = match s.body.get(i + 1) {
        None => true,
        Some(BodyItem::Property(_)) => true,
        Some(BodyItem::Trivia(t)) => !t.contains('\n'),
    };
    if needs_break {
        s.body.insert(i + 1, BodyItem::Trivia(eol.to_string()));
    }
}

fn separate_before(s: &mut Section, i: usize, eol: &str) -> usize {
    if i > 0 && matches!(s.body.get(i - 1), Some(BodyItem::Property(_))) {
        s.body.insert(i, BodyItem::Trivia(eol.to_string()));
        return i + 1;
    }
    i
}

/// Adds property `key = raw` after property `after`.
pub(crate) fn add_property(
    s: &mut Section,
    key: &str,
    raw: String,
    after: Option<&str>,
    eol: &str,
) -> Result<(), ParseError> {
    let p = Property::new(key, raw)?;
    insert_line(s, after, BodyItem::Property(p), eol);
    Ok(())
}

/// Removes property `key` together with the line break that belongs to it.
pub(crate) fn remove_property(s: &mut Section, key: &str) -> bool {
    let Some(i) = prop_index(s, key) else {
        return false;
    };
    let next_is_trivia = matches!(s.body.get(i + 1), Some(BodyItem::Trivia(_)));
    let has_later_prop = s
        .body
        .iter()
        .skip(i + 1)
        .any(|b| matches!(b, BodyItem::Property(_)));
    if next_is_trivia && has_later_prop {
        s.body.drain(i..=i + 1);
    } else if i > 0 && matches!(s.body.get(i - 1), Some(BodyItem::Trivia(_))) {
        s.body.drain(i - 1..=i);
    } else {
        s.body.remove(i);
    }
    true
}

/// Labels and size of conflict markers.
#[derive(Debug, Clone)]
pub struct MarkerStyle {
    /// Number of `<`, `=`, `>` characters (Git's `%L`, default 7).
    pub size: usize,
    /// Label after `<<<<<<<`.
    pub ours: String,
    /// Label after `>>>>>>>`.
    pub theirs: String,
}

impl Default for MarkerStyle {
    fn default() -> Self {
        MarkerStyle {
            size: 7,
            ours: "ours".into(),
            theirs: "theirs".into(),
        }
    }
}

impl MarkerStyle {
    /// A conflict block. `ours`/`theirs` are complete text without a final
    /// line break (or empty for "absent"). The block does not end with a line
    /// break either.
    pub(crate) fn block(&self, ours: &str, theirs: &str, eol: &str) -> String {
        let mut out = String::new();
        out.push_str(&"<".repeat(self.size));
        if !self.ours.is_empty() {
            out.push(' ');
            out.push_str(&self.ours);
        }
        out.push_str(eol);
        if !ours.is_empty() {
            out.push_str(ours);
            out.push_str(eol);
        }
        out.push_str(&"=".repeat(self.size));
        out.push_str(eol);
        if !theirs.is_empty() {
            out.push_str(theirs);
            out.push_str(eol);
        }
        out.push_str(&">".repeat(self.size));
        if !self.theirs.is_empty() {
            out.push(' ');
            out.push_str(&self.theirs);
        }
        out
    }
}

/// Replaces property `key` with a prebuilt conflict block (or inserts the
/// block on its own line after `after` when there is no such property).
pub(crate) fn put_block(s: &mut Section, key: &str, block: String, after: Option<&str>, eol: &str) {
    match prop_index(s, key) {
        Some(i) => {
            if let Some(item) = s.body.get_mut(i) {
                *item = BodyItem::Trivia(block);
            }
            let i = separate_before(s, i, eol);
            separate_after(s, i, eol);
        }
        None => insert_line(s, after, BodyItem::Trivia(block), eol),
    }
}

/// Splits a section's text into (content ending with its last line break,
/// separator = trailing blank lines/comments).
pub(crate) fn split_trailing(s: &Section) -> (String, String) {
    let text = s.to_text();
    let trimmed_len = text.trim_end_matches([' ', '\t', '\r', '\n']).len();
    let rest = text.get(trimmed_len..).unwrap_or("");
    let first_break = rest.find('\n').map_or(rest.len(), |i| i + 1);
    let content_end = trimmed_len + first_break;
    (
        text.get(..content_end).unwrap_or(&text).to_string(),
        text.get(content_end..).unwrap_or("").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use godot_text::Document;

    fn section(src: &str) -> Section {
        Document::parse(src).unwrap().sections.remove(0)
    }

    #[test]
    fn add_set_remove_properties() {
        let mut s = section("[node name=\"A\"]\na = 1\nb = 2\n\n");
        add_property(&mut s, "c", "3".into(), Some("a"), "\n").unwrap();
        assert_eq!(s.to_text(), "[node name=\"A\"]\na = 1\nc = 3\nb = 2\n\n");
        add_property(&mut s, "first", "0".into(), None, "\n").unwrap();
        assert_eq!(
            s.to_text(),
            "[node name=\"A\"]\nfirst = 0\na = 1\nc = 3\nb = 2\n\n"
        );
        set_property(
            &mut s,
            "c",
            "30".into(),
            "
",
        )
        .unwrap();
        assert!(remove_property(&mut s, "first"));
        assert!(remove_property(&mut s, "b"));
        assert_eq!(s.to_text(), "[node name=\"A\"]\na = 1\nc = 30\n\n");
        assert!(remove_property(&mut s, "a"));
        assert!(remove_property(&mut s, "c"));
        assert_eq!(s.to_text(), "[node name=\"A\"]\n\n");
        add_property(&mut s, "x", "1".into(), None, "\n").unwrap();
        assert_eq!(s.to_text(), "[node name=\"A\"]\nx = 1\n\n");
    }

    #[test]
    fn edits_keep_glued_properties_apart() {
        let mut s = section("[a]\nt = \"x\"u = 1");
        add_property(&mut s, "v", "2".into(), Some("t"), "\n").unwrap();
        set_property(&mut s, "t", "3".into(), "\n").unwrap();
        assert_eq!(s.to_text(), "[a]\nt = 3\nv = 2\nu = 1");
        let mut s = section("[a]\nt = \"x\"u = 1");
        set_property(&mut s, "t", "3".into(), "\n").unwrap();
        assert_eq!(s.to_text(), "[a]\nt = 3\nu = 1");
        let mut s = section("[a]");
        add_property(&mut s, "v", "2".into(), None, "\n").unwrap();
        assert_eq!(s.to_text(), "[a]\nv = 2\n");
    }

    #[test]
    fn header_fields_follow_godot_order() {
        let mut s = section("[node name=\"A\" type=\"Node\" parent=\".\" unique_id=5]\n");
        set_header_field(&mut s.header, "groups", "[\"g\"]".into()).unwrap();
        set_header_field(&mut s.header, "index", "\"2\"".into()).unwrap();
        assert_eq!(
            s.header.raw,
            "[node name=\"A\" type=\"Node\" parent=\".\" index=\"2\" unique_id=5 groups=[\"g\"]]"
        );
    }

    #[test]
    fn markers_and_split() {
        let mut s = section("[node name=\"A\"]\nfov = 75.0\nx = 1\n\n");
        let block = MarkerStyle::default().block("fov = 80.0", "fov = 90.0", "\n");
        put_block(&mut s, "fov", block, None, "\n");
        assert_eq!(
            s.to_text(),
            "[node name=\"A\"]\n<<<<<<< ours\nfov = 80.0\n=======\nfov = 90.0\n>>>>>>> theirs\nx = 1\n\n"
        );
        let (content, sep) = split_trailing(&section("[a]\nk = 1\r\n\r\n"));
        assert_eq!((content.as_str(), sep.as_str()), ("[a]\nk = 1\r\n", "\r\n"));
        let (content, sep) = split_trailing(&section("[a]"));
        assert_eq!((content.as_str(), sep.as_str()), ("[a]", ""));
    }
}
