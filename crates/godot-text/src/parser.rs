//! Recursive-descent parser for Variant literals, mirroring
//! `VariantParser::parse_value` in Godot.

use crate::error::ParseError;
use crate::lexer::{Lexer, Punct, TokKind, Token};
use crate::value::{ContainerType, Number, Value};

/// Maximum nesting depth of arrays/dictionaries/objects. Real files nest only a
/// few levels; the limit protects against stack overflow on hostile input.
pub(crate) const MAX_DEPTH: usize = 128;

/// Argument counts of fixed-size math constructors (aliases included).
fn fixed_arity(name: &str) -> Option<usize> {
    Some(match name {
        "Vector2" | "Vector2i" => 2,
        "Vector3" | "Vector3i" => 3,
        "Rect2" | "Rect2i" | "Vector4" | "Vector4i" | "Plane" | "Quaternion" | "Quat" | "Color" => {
            4
        }
        "Transform2D" | "Matrix32" | "AABB" | "Rect3" => 6,
        "Basis" | "Matrix3" => 9,
        "Transform3D" | "Transform" => 12,
        "Projection" => 16,
        _ => return None,
    })
}

fn is_numeric_packed_array(name: &str) -> bool {
    matches!(
        name,
        "PackedInt32Array"
            | "PackedIntArray"
            | "PoolIntArray"
            | "IntArray"
            | "PackedInt64Array"
            | "PackedFloat32Array"
            | "PackedRealArray"
            | "PoolRealArray"
            | "FloatArray"
            | "PackedFloat64Array"
            | "PackedVector2Array"
            | "PoolVector2Array"
            | "Vector2Array"
            | "PackedVector3Array"
            | "PoolVector3Array"
            | "Vector3Array"
            | "PackedVector4Array"
            | "PoolVector4Array"
            | "Vector4Array"
            | "PackedColorArray"
            | "PoolColorArray"
            | "ColorArray"
    )
}

fn special_float(id: &str) -> bool {
    matches!(id, "inf" | "-inf" | "inf_neg" | "nan")
}

fn expect_punct(lx: &mut Lexer<'_>, p: Punct, what: &str) -> Result<Token<'static>, ParseError> {
    let t = lx.next_token()?;
    if t.kind == TokKind::Punct(p) {
        Ok(Token {
            kind: TokKind::Punct(p),
            start: t.start,
            end: t.end,
        })
    } else {
        Err(lx.error(
            t.start,
            format!("Expected {} {what}, got {}", p.as_str(), t.kind.describe()),
        ))
    }
}

/// Parses `( number, number, ... )` (also accepting `inf`/`nan` identifiers).
fn parse_construct_args(lx: &mut Lexer<'_>, name: &str) -> Result<Vec<Value>, ParseError> {
    expect_punct(lx, Punct::ParenOpen, &format!("after '{name}'"))?;
    let mut args = Vec::new();
    let mut first = true;
    loop {
        if !first {
            let t = lx.next_token()?;
            match t.kind {
                TokKind::Punct(Punct::Comma) => {}
                TokKind::Punct(Punct::ParenClose) => break,
                k => {
                    return Err(lx.error(
                        t.start,
                        format!("Expected ',' or ')' in constructor, got {}", k.describe()),
                    ))
                }
            }
        }
        let t = lx.next_token()?;
        match t.kind {
            TokKind::Punct(Punct::ParenClose) if first => break,
            TokKind::Number(raw) => args.push(Value::Number(Number::new(raw))),
            TokKind::Ident(id) if special_float(id) => args.push(Value::Number(Number::new(id))),
            k => {
                return Err(lx.error(
                    t.start,
                    format!("Expected number in constructor, got {}", k.describe()),
                ))
            }
        }
        first = false;
    }
    Ok(args)
}

fn parse_string_args(lx: &mut Lexer<'_>, name: &str) -> Result<Vec<Value>, ParseError> {
    expect_punct(lx, Punct::ParenOpen, &format!("after '{name}'"))?;
    let mut args = Vec::new();
    let mut first = true;
    loop {
        if !first {
            let t = lx.next_token()?;
            match t.kind {
                TokKind::Punct(Punct::Comma) => {}
                TokKind::Punct(Punct::ParenClose) => break,
                k => {
                    return Err(lx.error(
                        t.start,
                        format!("Expected ',' or ')', got {}", k.describe()),
                    ))
                }
            }
        }
        let t = lx.next_token()?;
        match t.kind {
            TokKind::Punct(Punct::ParenClose) => break,
            TokKind::Str(s) => args.push(Value::String(s)),
            k => return Err(lx.error(t.start, format!("Expected string, got {}", k.describe()))),
        }
        first = false;
    }
    Ok(args)
}

fn parse_byte_array(lx: &mut Lexer<'_>) -> Result<Vec<Value>, ParseError> {
    expect_punct(lx, Punct::ParenOpen, "after 'PackedByteArray'")?;
    let t = lx.next_token()?;
    match t.kind {
        TokKind::Punct(Punct::ParenClose) => Ok(Vec::new()),
        TokKind::Str(s) => {
            if !is_base64(&s) {
                return Err(lx.error(t.start, "Invalid base64-encoded string"));
            }
            expect_punct(lx, Punct::ParenClose, "after base64 string")?;
            Ok(vec![Value::String(s)])
        }
        TokKind::Number(_) | TokKind::Ident(_) => {
            let mut args = Vec::new();
            let mut cur = t;
            loop {
                match cur.kind {
                    TokKind::Number(raw) => args.push(Value::Number(Number::new(raw))),
                    TokKind::Ident(id) if special_float(id) => {
                        args.push(Value::Number(Number::new(id)))
                    }
                    k => {
                        return Err(lx.error(
                            cur.start,
                            format!("Expected number in constructor, got {}", k.describe()),
                        ))
                    }
                }
                let sep = lx.next_token()?;
                match sep.kind {
                    TokKind::Punct(Punct::Comma) => {}
                    TokKind::Punct(Punct::ParenClose) => break,
                    k => {
                        return Err(lx.error(
                            sep.start,
                            format!("Expected ',' or ')' in constructor, got {}", k.describe()),
                        ))
                    }
                }
                cur = lx.next_token()?;
            }
            Ok(args)
        }
        k => Err(lx.error(
            t.start,
            format!(
                "Expected base64 string, or list of numbers in constructor, got {}",
                k.describe()
            ),
        )),
    }
}

fn is_base64(s: &str) -> bool {
    let trimmed = s.trim_end_matches('=');
    s.len() - trimmed.len() <= 2
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// Parses `("id")` or `(123)` after `ExtResource`/`SubResource`.
fn parse_ref_id(lx: &mut Lexer<'_>, name: &str) -> Result<String, ParseError> {
    expect_punct(lx, Punct::ParenOpen, &format!("after '{name}'"))?;
    let t = lx.next_token()?;
    let id = match t.kind {
        TokKind::Str(s) => s,
        TokKind::Number(n) => n.to_string(),
        k => {
            return Err(lx.error(
                t.start,
                format!(
                    "Expected string or number as argument of {name}(), got {}",
                    k.describe()
                ),
            ))
        }
    };
    expect_punct(lx, Punct::ParenClose, &format!("after {name} id"))?;
    Ok(id)
}

fn parse_resource_args(lx: &mut Lexer<'_>) -> Result<Vec<String>, ParseError> {
    expect_punct(lx, Punct::ParenOpen, "after 'Resource'")?;
    let t = lx.next_token()?;
    let TokKind::Str(first) = t.kind else {
        return Err(lx.error(t.start, "Expected string as argument for Resource()"));
    };
    let mut parts = vec![first];
    let t = lx.next_token()?;
    match t.kind {
        TokKind::Punct(Punct::ParenClose) => {}
        TokKind::Punct(Punct::Comma) => {
            let t2 = lx.next_token()?;
            let TokKind::Str(second) = t2.kind else {
                return Err(lx.error(t2.start, "Expected string in Resource reference"));
            };
            parts.push(second);
            expect_punct(lx, Punct::ParenClose, "in Resource reference")?;
        }
        k => return Err(lx.error(t.start, format!("Expected ')', got {}", k.describe()))),
    }
    Ok(parts)
}

/// Parses a container element type: a name, or a script reference.
fn parse_container_type<'a>(lx: &mut Lexer<'a>, depth: usize) -> Result<ContainerType, ParseError> {
    let t = lx.next_token()?;
    let TokKind::Ident(id) = t.kind else {
        return Err(lx.error(
            t.start,
            format!("Expected type identifier, got {}", t.kind.describe()),
        ));
    };
    if matches!(id, "Resource" | "SubResource" | "ExtResource") {
        let mut look = lx.clone();
        if look.next_token()?.kind == TokKind::Punct(Punct::ParenOpen) {
            let v = parse_value(lx, t, depth + 1)?;
            return Ok(ContainerType::Script(Box::new(v)));
        }
    }
    Ok(ContainerType::Named(id.to_string()))
}

fn check_depth(lx: &Lexer<'_>, at: usize, depth: usize) -> Result<(), ParseError> {
    if depth >= MAX_DEPTH {
        Err(lx.error(at, format!("Values nested deeper than {MAX_DEPTH} levels")))
    } else {
        Ok(())
    }
}

/// Parses a value whose first token is `tok`.
pub(crate) fn parse_value<'a>(
    lx: &mut Lexer<'a>,
    tok: Token<'a>,
    depth: usize,
) -> Result<Value, ParseError> {
    match tok.kind {
        TokKind::Punct(Punct::CurlyOpen) => {
            check_depth(lx, tok.start, depth)?;
            Ok(Value::Dictionary(parse_dict_body(lx, depth + 1)?))
        }
        TokKind::Punct(Punct::BracketOpen) => {
            check_depth(lx, tok.start, depth)?;
            Ok(Value::Array(parse_array_body(lx, depth + 1)?))
        }
        TokKind::Number(raw) => Ok(Value::Number(Number::new(raw))),
        TokKind::Str(s) => Ok(Value::String(s)),
        TokKind::StrName(s) => Ok(Value::StringName(s)),
        TokKind::Color(raw) => {
            if !matches!(raw.len(), 4 | 5 | 7 | 9) {
                return Err(lx.error(tok.start, format!("Invalid color code '{raw}'")));
            }
            Ok(Value::HexColor(raw.to_string()))
        }
        TokKind::Ident(id) => parse_ident_value(lx, id, tok.start, depth),
        k => Err(lx.error(tok.start, format!("Expected value, got {}", k.describe()))),
    }
}

fn parse_ident_value<'a>(
    lx: &mut Lexer<'a>,
    id: &'a str,
    start: usize,
    depth: usize,
) -> Result<Value, ParseError> {
    let construct = |args| Value::Construct {
        name: id.to_string(),
        args,
    };
    match id {
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        "null" | "nil" => Ok(Value::Null),
        _ if special_float(id) => Ok(Value::Number(Number::new(id))),
        _ if fixed_arity(id).is_some() => {
            let args = parse_construct_args(lx, id)?;
            let want = fixed_arity(id).unwrap_or(0);
            if args.len() != want {
                return Err(lx.error(
                    start,
                    format!(
                        "Expected {want} arguments for {id} constructor, got {}",
                        args.len()
                    ),
                ));
            }
            Ok(construct(args))
        }
        _ if is_numeric_packed_array(id) => Ok(construct(parse_construct_args(lx, id)?)),
        "PackedByteArray" | "PoolByteArray" | "ByteArray" => Ok(construct(parse_byte_array(lx)?)),
        "PackedStringArray" | "PoolStringArray" | "StringArray" => {
            Ok(construct(parse_string_args(lx, id)?))
        }
        "NodePath" => {
            expect_punct(lx, Punct::ParenOpen, "after 'NodePath'")?;
            let t = lx.next_token()?;
            let TokKind::Str(s) = t.kind else {
                return Err(lx.error(t.start, "Expected string as argument for NodePath()"));
            };
            expect_punct(lx, Punct::ParenClose, "after NodePath string")?;
            Ok(Value::NodePath(s))
        }
        "RID" => {
            expect_punct(lx, Punct::ParenOpen, "after 'RID'")?;
            let t = lx.next_token()?;
            match t.kind {
                TokKind::Punct(Punct::ParenClose) => Ok(construct(Vec::new())),
                TokKind::Number(raw) => {
                    expect_punct(lx, Punct::ParenClose, "after RID number")?;
                    Ok(construct(vec![Value::Number(Number::new(raw))]))
                }
                k => Err(lx.error(
                    t.start,
                    format!("Expected number as argument or ')', got {}", k.describe()),
                )),
            }
        }
        "Signal" | "Callable" => {
            expect_punct(lx, Punct::ParenOpen, &format!("after '{id}'"))?;
            expect_punct(lx, Punct::ParenClose, &format!("after '{id}('"))?;
            Ok(construct(Vec::new()))
        }
        "ExtResource" => Ok(Value::ExtResource(parse_ref_id(lx, id)?)),
        "SubResource" => Ok(Value::SubResource(parse_ref_id(lx, id)?)),
        "Resource" => Ok(Value::Resource(parse_resource_args(lx)?)),
        "Object" => {
            check_depth(lx, start, depth)?;
            parse_object(lx, depth + 1)
        }
        "Array" => {
            check_depth(lx, start, depth)?;
            expect_punct(lx, Punct::BracketOpen, "after 'Array'")?;
            let elem = parse_container_type(lx, depth)?;
            expect_punct(lx, Punct::BracketClose, "after array element type")?;
            expect_punct(lx, Punct::ParenOpen, "after 'Array[...]'")?;
            expect_punct(lx, Punct::BracketOpen, "in typed array")?;
            let items = parse_array_body(lx, depth + 1)?;
            expect_punct(lx, Punct::ParenClose, "after typed array")?;
            Ok(Value::TypedArray { elem, items })
        }
        "Dictionary" => {
            check_depth(lx, start, depth)?;
            expect_punct(lx, Punct::BracketOpen, "after 'Dictionary'")?;
            let key = parse_container_type(lx, depth)?;
            expect_punct(lx, Punct::Comma, "after key type")?;
            let value = parse_container_type(lx, depth)?;
            expect_punct(lx, Punct::BracketClose, "after value type")?;
            expect_punct(lx, Punct::ParenOpen, "after 'Dictionary[...]'")?;
            expect_punct(lx, Punct::CurlyOpen, "in typed dictionary")?;
            let entries = parse_dict_body(lx, depth + 1)?;
            expect_punct(lx, Punct::ParenClose, "after typed dictionary")?;
            Ok(Value::TypedDictionary {
                key,
                value,
                entries,
            })
        }
        _ => Err(lx.error(start, format!("Unexpected identifier '{id}'"))),
    }
}

fn parse_object(lx: &mut Lexer<'_>, depth: usize) -> Result<Value, ParseError> {
    expect_punct(lx, Punct::ParenOpen, "after 'Object'")?;
    let t = lx.next_token()?;
    let TokKind::Ident(class) = t.kind else {
        return Err(lx.error(t.start, "Expected identifier with type of object"));
    };
    let class = class.to_string();
    expect_punct(lx, Punct::Comma, "after object type")?;
    let mut props = Vec::new();
    let mut need_comma = false;
    loop {
        let t = lx.next_token()?;
        match t.kind {
            TokKind::Punct(Punct::ParenClose) => break,
            TokKind::Eof => return Err(lx.error(t.start, "Unexpected EOF while parsing Object()")),
            TokKind::Punct(Punct::Comma) if need_comma => {
                need_comma = false;
                continue;
            }
            _ if need_comma => return Err(lx.error(t.start, "Expected ')' or ','")),
            TokKind::Str(key) => {
                expect_punct(lx, Punct::Colon, "after object property name")?;
                let vt = lx.next_token()?;
                let v = parse_value(lx, vt, depth)?;
                props.push((key, v));
                need_comma = true;
            }
            k => {
                return Err(lx.error(
                    t.start,
                    format!("Expected property name as string, got {}", k.describe()),
                ))
            }
        }
    }
    Ok(Value::Object { class, props })
}

fn parse_array_body(lx: &mut Lexer<'_>, depth: usize) -> Result<Vec<Value>, ParseError> {
    let mut items = Vec::new();
    let mut need_comma = false;
    loop {
        let t = lx.next_token()?;
        match t.kind {
            TokKind::Punct(Punct::BracketClose) => return Ok(items),
            TokKind::Eof => return Err(lx.error(t.start, "Unexpected EOF while parsing array")),
            TokKind::Punct(Punct::Comma) if need_comma => need_comma = false,
            _ if need_comma => {
                return Err(lx.error(
                    t.start,
                    format!("Expected ',' or ']' in array, got {}", t.kind.describe()),
                ))
            }
            _ => {
                items.push(parse_value(lx, t, depth)?);
                need_comma = true;
            }
        }
    }
}

fn parse_dict_body(lx: &mut Lexer<'_>, depth: usize) -> Result<Vec<(Value, Value)>, ParseError> {
    let mut entries = Vec::new();
    let mut need_comma = false;
    loop {
        let t = lx.next_token()?;
        match t.kind {
            TokKind::Punct(Punct::CurlyClose) => return Ok(entries),
            TokKind::Eof => {
                return Err(lx.error(t.start, "Unexpected EOF while parsing dictionary"))
            }
            TokKind::Punct(Punct::Comma) if need_comma => need_comma = false,
            _ if need_comma => {
                return Err(lx.error(
                    t.start,
                    format!(
                        "Expected '}}' or ',' in dictionary, got {}",
                        t.kind.describe()
                    ),
                ))
            }
            _ => {
                let key = parse_value(lx, t, depth)?;
                expect_punct(lx, Punct::Colon, "after dictionary key")?;
                let vt = lx.next_token()?;
                let value = parse_value(lx, vt, depth)?;
                entries.push((key, value));
                need_comma = true;
            }
        }
    }
}

/// Parses a complete standalone value from `src` (trailing whitespace and
/// comments allowed, anything else is an error).
pub fn parse_value_str(src: &str) -> Result<Value, ParseError> {
    let mut lx = Lexer::new(src);
    let t = lx.next_token()?;
    let v = parse_value(&mut lx, t, 0)?;
    let rest = lx.next_token()?;
    if rest.kind != TokKind::Eof {
        return Err(lx.error(
            rest.start,
            format!("Unexpected {} after value", rest.kind.describe()),
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Value {
        parse_value_str(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn round_trips_through_display() {
        for s in [
            "null",
            "true",
            "-1.5e3",
            "\"a\\\"b\"",
            "&\"name\"",
            "NodePath(\"../A:position:x\")",
            "Vector2(1, 2)",
            "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2.5, 0)",
            "Color(1, 0.5, 0, 1)",
            "PackedStringArray(\"a\", \"b\")",
            "PackedByteArray(\"AAEC\")",
            "PackedFloat32Array()",
            "ExtResource(\"1_abc\")",
            "SubResource(\"BoxMesh_x1\")",
            "Resource(\"uid://abc\", \"res://a.tres\")",
            "[1, \"two\", [3]]",
            "Array[int]([1, 2])",
            "Array[ExtResource(\"2\")]([])",
            "Dictionary[String, Texture]({})",
            "RID()",
            "Callable()",
        ] {
            let v = p(s);
            assert_eq!(v.to_string(), s, "display of {s}");
            assert!(p(&v.to_string()).semantic_eq(&v, 0.0));
        }
    }

    #[test]
    fn dictionaries_are_order_insensitive() {
        let a = p("{\"a\": 1, \"b\": 2}");
        let b = p("{\n\"b\": 2,\n\"a\": 1\n}");
        assert!(a.semantic_eq(&b, 0.0));
    }

    #[test]
    fn typed_array_of_resource_class() {
        let v = p("Array[Resource]([])");
        assert!(matches!(
            v,
            Value::TypedArray {
                elem: ContainerType::Named(ref n),
                ..
            } if n == "Resource"
        ));
    }

    #[test]
    fn object_literal() {
        let v = p("Object(InputEventKey,\"resource_local_to_scene\":false,\"keycode\":65\n)");
        assert!(matches!(v, Value::Object { ref props, .. } if props.len() == 2));
    }

    #[test]
    fn errors() {
        for s in [
            "Vector2(1)",
            "Vector2(1,)",
            "[1 2]",
            "{1 2}",
            "Foo(1)",
            "NodePath(1)",
            "PackedByteArray(\"!!\")",
            "[",
        ] {
            assert!(parse_value_str(s).is_err(), "{s} should fail");
        }
        let deep = "[".repeat(1000);
        assert!(parse_value_str(&deep).is_err());
    }
}
