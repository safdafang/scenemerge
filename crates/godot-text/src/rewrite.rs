use crate::error::ParseError;
use crate::lexer::{Lexer, Punct, TokKind};
use crate::value::{escape_multiline, RefKind};

/// Rewrites the ids inside `ExtResource(...)` / `SubResource(...)` in `raw`
/// (the source text of one value or header), leaving every other byte
/// untouched. `map` returns the new id, or `None` to keep the old one.
///
/// ```
/// use godot_text::{rewrite_refs, RefKind};
/// let out = rewrite_refs("[ExtResource(\"1_a\"), SubResource(\"B_1\")]", |kind, id| {
///     (kind == RefKind::Ext && id == "1_a").then(|| "7_z".to_string())
/// }).unwrap();
/// assert_eq!(out, "[ExtResource(\"7_z\"), SubResource(\"B_1\")]");
/// ```
pub fn rewrite_refs(
    raw: &str,
    mut map: impl FnMut(RefKind, &str) -> Option<String>,
) -> Result<String, ParseError> {
    let mut lx = Lexer::new(raw);
    let mut out = String::with_capacity(raw.len());
    let mut copied = 0;
    let mut pending: Option<RefKind> = None;
    let mut saw_paren = false;
    loop {
        let t = lx.next_token()?;
        match &t.kind {
            TokKind::Eof => break,
            TokKind::Ident("ExtResource") => {
                pending = Some(RefKind::Ext);
                saw_paren = false;
                continue;
            }
            TokKind::Ident("SubResource") => {
                pending = Some(RefKind::Sub);
                saw_paren = false;
                continue;
            }
            TokKind::Punct(Punct::ParenOpen) if pending.is_some() && !saw_paren => {
                saw_paren = true;
                continue;
            }
            TokKind::Str(id) if saw_paren => {
                if let Some(kind) = pending {
                    if let Some(new_id) = map(kind, id) {
                        out.push_str(raw.get(copied..t.start).unwrap_or(""));
                        out.push('"');
                        out.push_str(&escape_multiline(&new_id));
                        out.push('"');
                        copied = t.end;
                    }
                }
            }
            TokKind::Number(n) if saw_paren => {
                if let Some(kind) = pending {
                    if let Some(new_id) = map(kind, n) {
                        out.push_str(raw.get(copied..t.start).unwrap_or(""));
                        out.push('"');
                        out.push_str(&escape_multiline(&new_id));
                        out.push('"');
                        copied = t.end;
                    }
                }
            }
            _ => {}
        }
        pending = None;
        saw_paren = false;
    }
    out.push_str(raw.get(copied..).unwrap_or(""));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_formatting_and_strings() {
        let raw = "{\n\"ExtResource\": ExtResource( \"1\" ), ; c\n\"k\": SubResource(\"A\")\n}";
        let out = rewrite_refs(raw, |k, id| Some(format!("{k:?}-{id}"))).unwrap();
        assert_eq!(
            out,
            "{\n\"ExtResource\": ExtResource( \"Ext-1\" ), ; c\n\"k\": SubResource(\"Sub-A\")\n}"
        );
        assert_eq!(
            rewrite_refs("ExtResource(3)", |_, _| Some("3_abc".into())).unwrap(),
            "ExtResource(\"3_abc\")"
        );
    }
}
