//! The Variant value AST.

use std::fmt::{self, Write as _};

/// A numeric literal. The original spelling is kept so that values can be
/// written back unchanged.
#[derive(Debug, Clone)]
pub struct Number {
    /// The literal exactly as written, e.g. `-1.5e3` or `42`.
    pub raw: String,
}

impl Number {
    /// Creates a number from its textual form.
    pub fn new(raw: impl Into<String>) -> Self {
        Number { raw: raw.into() }
    }

    /// Whether Godot reads this literal as a float (it contains `.`, `e`/`E`,
    /// or is one of `inf`, `-inf`, `inf_neg`, `nan`).
    pub fn is_float(&self) -> bool {
        self.raw.contains(['.', 'e', 'E'])
            || matches!(self.raw.as_str(), "inf" | "-inf" | "inf_neg" | "nan")
    }

    /// The value as `f64` (`inf`, `-inf`, `nan` included).
    pub fn as_f64(&self) -> f64 {
        match self.raw.as_str() {
            "inf" => f64::INFINITY,
            "-inf" | "inf_neg" => f64::NEG_INFINITY,
            "nan" => f64::NAN,
            s => parse_float_prefix(s),
        }
    }

    /// The value as `i64`, if it is an integer literal (saturating on overflow).
    pub fn as_i64(&self) -> Option<i64> {
        if self.is_float() {
            return None;
        }
        match self.raw.parse::<i64>() {
            Ok(v) => Some(v),
            Err(_) => Some(if self.raw.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            }),
        }
    }

    /// Numeric equality with an absolute tolerance. NaN equals NaN and
    /// `-0.0` equals `0.0`, which is what a human expects in a diff.
    pub fn approx_eq(&self, other: &Number, epsilon: f64) -> bool {
        if self.raw == other.raw {
            return true;
        }
        if let (Some(a), Some(b)) = (self.as_i64(), other.as_i64()) {
            return a == b;
        }
        let (a, b) = (self.as_f64(), other.as_f64());
        if a.is_nan() && b.is_nan() {
            return true;
        }
        if a == b {
            return true;
        }
        epsilon > 0.0 && (a - b).abs() <= epsilon
    }
}

/// Parses the longest valid float prefix, like Godot's `String::to_float`
/// does for slightly malformed literals such as `1e` or `1.`.
fn parse_float_prefix(s: &str) -> f64 {
    if let Ok(v) = s.parse::<f64>() {
        return v;
    }
    let mut end = s.len();
    while end > 0 {
        if let Some(Ok(v)) = s.get(..end).map(str::parse::<f64>) {
            return v;
        }
        end -= 1;
    }
    0.0
}

/// The element type of a typed `Array[...]` or key/value type of a typed `Dictionary[...]`.
#[derive(Debug, Clone)]
pub enum ContainerType {
    /// A built-in or class name, e.g. `int`, `Node`, `Resource`.
    Named(String),
    /// A script-defined type, given as `ExtResource(...)`/`SubResource(...)`/`Resource(...)`.
    Script(Box<Value>),
}

/// A parsed Variant literal as it appears in `.tscn`/`.tres` files.
#[derive(Debug, Clone)]
pub enum Value {
    /// `null` (or `nil`).
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// Integer or float literal, also `inf`, `-inf`, `nan`.
    Number(Number),
    /// `"..."`
    String(String),
    /// `&"..."`
    StringName(String),
    /// `#rrggbb` colour shorthand (raw text including `#`).
    HexColor(String),
    /// `NodePath("...")`
    NodePath(String),
    /// A constructor with literal arguments: math types (`Vector2(1, 2)`,
    /// `Transform3D(...)`, `Color(...)`, ...), packed arrays, `RID()`,
    /// `Signal()`, `Callable()`. Arguments are numbers or strings.
    Construct {
        /// Constructor name as written (aliases such as `Transform` are kept).
        name: String,
        /// Arguments.
        args: Vec<Value>,
    },
    /// `ExtResource("id")` — reference to an `[ext_resource]` by local id.
    ExtResource(String),
    /// `SubResource("id")` — reference to a `[sub_resource]` by local id.
    SubResource(String),
    /// `Resource("uid://...", "res://...")` — direct reference by uid and/or path.
    Resource(Vec<String>),
    /// `[a, b, ...]`
    Array(Vec<Value>),
    /// `Array[Type]([a, b])`
    TypedArray {
        /// Element type.
        elem: ContainerType,
        /// Elements.
        items: Vec<Value>,
    },
    /// `{ k: v, ... }`
    Dictionary(Vec<(Value, Value)>),
    /// `Dictionary[K, V]({ ... })`
    TypedDictionary {
        /// Key type.
        key: ContainerType,
        /// Value type.
        value: ContainerType,
        /// Entries.
        entries: Vec<(Value, Value)>,
    },
    /// `Object(Class, "prop": value, ...)`
    Object {
        /// Class name.
        class: String,
        /// Properties in file order.
        props: Vec<(String, Value)>,
    },
}

/// Which kind of local resource reference was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKind {
    /// `ExtResource("...")`
    Ext,
    /// `SubResource("...")`
    Sub,
}

impl Value {
    /// Semantic equality: numbers compare by value (with `epsilon` tolerance for
    /// floats), dictionaries ignore key order, everything else is structural.
    pub fn semantic_eq(&self, other: &Value, epsilon: f64) -> bool {
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Number(a), Number(b)) => a.is_float() == b.is_float() && a.approx_eq(b, epsilon),
            (String(a), String(b))
            | (StringName(a), StringName(b))
            | (NodePath(a), NodePath(b))
            | (ExtResource(a), ExtResource(b))
            | (SubResource(a), SubResource(b)) => a == b,
            (HexColor(a), HexColor(b)) => a.eq_ignore_ascii_case(b),
            (Resource(a), Resource(b)) => a == b,
            (Construct { name: n1, args: a1 }, Construct { name: n2, args: a2 }) => {
                canonical_construct_name(n1) == canonical_construct_name(n2)
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2).all(|(x, y)| match (x, y) {
                        // Constructor arguments are all reals/ints: `1` == `1.0`.
                        (Number(p), Number(q)) => p.approx_eq(q, epsilon),
                        _ => x.semantic_eq(y, epsilon),
                    })
            }
            (Array(a), Array(b)) => seq_eq(a, b, epsilon),
            (
                TypedArray {
                    elem: e1,
                    items: i1,
                },
                TypedArray {
                    elem: e2,
                    items: i2,
                },
            ) => container_type_eq(e1, e2, epsilon) && seq_eq(i1, i2, epsilon),
            (Dictionary(a), Dictionary(b)) => dict_eq(a, b, epsilon),
            (
                TypedDictionary {
                    key: k1,
                    value: v1,
                    entries: e1,
                },
                TypedDictionary {
                    key: k2,
                    value: v2,
                    entries: e2,
                },
            ) => {
                container_type_eq(k1, k2, epsilon)
                    && container_type_eq(v1, v2, epsilon)
                    && dict_eq(e1, e2, epsilon)
            }
            (
                Object {
                    class: c1,
                    props: p1,
                },
                Object {
                    class: c2,
                    props: p2,
                },
            ) => {
                c1 == c2
                    && p1.len() == p2.len()
                    && p1.iter().all(|(k, v)| {
                        p2.iter()
                            .any(|(k2, v2)| k == k2 && v.semantic_eq(v2, epsilon))
                    })
            }
            _ => false,
        }
    }

    /// Calls `f` for every `ExtResource`/`SubResource` reference inside the value.
    pub fn for_each_ref(&self, f: &mut impl FnMut(RefKind, &str)) {
        match self {
            Value::ExtResource(id) => f(RefKind::Ext, id),
            Value::SubResource(id) => f(RefKind::Sub, id),
            Value::Construct { args, .. } => args.iter().for_each(|v| v.for_each_ref(f)),
            Value::Array(items) => items.iter().for_each(|v| v.for_each_ref(f)),
            Value::TypedArray { elem, items } => {
                if let ContainerType::Script(v) = elem {
                    v.for_each_ref(f);
                }
                items.iter().for_each(|v| v.for_each_ref(f));
            }
            Value::Dictionary(entries) => entries.iter().for_each(|(k, v)| {
                k.for_each_ref(f);
                v.for_each_ref(f);
            }),
            Value::TypedDictionary {
                key,
                value,
                entries,
            } => {
                for t in [key, value] {
                    if let ContainerType::Script(v) = t {
                        v.for_each_ref(f);
                    }
                }
                entries.iter().for_each(|(k, v)| {
                    k.for_each_ref(f);
                    v.for_each_ref(f);
                });
            }
            Value::Object { props, .. } => props.iter().for_each(|(_, v)| v.for_each_ref(f)),
            _ => {}
        }
    }

    /// Returns a copy with every reference replaced by `f(kind, id)`.
    #[must_use]
    pub fn map_refs(&self, f: &mut impl FnMut(RefKind, &str) -> Value) -> Value {
        let map_ty = |t: &ContainerType, f: &mut dyn FnMut(&Value) -> Value| match t {
            ContainerType::Named(n) => ContainerType::Named(n.clone()),
            ContainerType::Script(v) => ContainerType::Script(Box::new(f(v))),
        };
        match self {
            Value::ExtResource(id) => f(RefKind::Ext, id),
            Value::SubResource(id) => f(RefKind::Sub, id),
            Value::Construct { name, args } => Value::Construct {
                name: name.clone(),
                args: args.iter().map(|v| v.map_refs(f)).collect(),
            },
            Value::Array(items) => Value::Array(items.iter().map(|v| v.map_refs(f)).collect()),
            Value::TypedArray { elem, items } => Value::TypedArray {
                elem: map_ty(elem, &mut |v| v.map_refs(f)),
                items: items.iter().map(|v| v.map_refs(f)).collect(),
            },
            Value::Dictionary(entries) => Value::Dictionary(
                entries
                    .iter()
                    .map(|(k, v)| (k.map_refs(f), v.map_refs(f)))
                    .collect(),
            ),
            Value::TypedDictionary {
                key,
                value,
                entries,
            } => Value::TypedDictionary {
                key: map_ty(key, &mut |v| v.map_refs(f)),
                value: map_ty(value, &mut |v| v.map_refs(f)),
                entries: entries
                    .iter()
                    .map(|(k, v)| (k.map_refs(f), v.map_refs(f)))
                    .collect(),
            },
            Value::Object { class, props } => Value::Object {
                class: class.clone(),
                props: props
                    .iter()
                    .map(|(k, v)| (k.clone(), v.map_refs(f)))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// A short name of the value's type, e.g. `Vector3`, `Array`, `String`.
    pub fn type_name(&self) -> &str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(n) => {
                if n.is_float() {
                    "float"
                } else {
                    "int"
                }
            }
            Value::String(_) => "String",
            Value::StringName(_) => "StringName",
            Value::HexColor(_) => "Color",
            Value::NodePath(_) => "NodePath",
            Value::Construct { name, .. } => canonical_construct_name(name),
            Value::ExtResource(_) => "ExtResource",
            Value::SubResource(_) => "SubResource",
            Value::Resource(_) => "Resource",
            Value::Array(_) | Value::TypedArray { .. } => "Array",
            Value::Dictionary(_) | Value::TypedDictionary { .. } => "Dictionary",
            Value::Object { .. } => "Object",
        }
    }
}

/// Maps legacy constructor aliases to their Godot 4 names.
pub fn canonical_construct_name(name: &str) -> &str {
    match name {
        "Matrix32" => "Transform2D",
        "Quat" => "Quaternion",
        "Rect3" => "AABB",
        "Matrix3" => "Basis",
        "Transform" => "Transform3D",
        "PoolByteArray" | "ByteArray" => "PackedByteArray",
        "PackedIntArray" | "PoolIntArray" | "IntArray" => "PackedInt32Array",
        "PackedRealArray" | "PoolRealArray" | "FloatArray" => "PackedFloat32Array",
        "PoolStringArray" | "StringArray" => "PackedStringArray",
        "PoolVector2Array" | "Vector2Array" => "PackedVector2Array",
        "PoolVector3Array" | "Vector3Array" => "PackedVector3Array",
        "PoolVector4Array" | "Vector4Array" => "PackedVector4Array",
        "PoolColorArray" | "ColorArray" => "PackedColorArray",
        other => other,
    }
}

fn seq_eq(a: &[Value], b: &[Value], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.semantic_eq(y, eps))
}

fn dict_eq(a: &[(Value, Value)], b: &[(Value, Value)], eps: f64) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // Fast path: same order.
    if a.iter()
        .zip(b)
        .all(|((k1, v1), (k2, v2))| k1.semantic_eq(k2, eps) && v1.semantic_eq(v2, eps))
    {
        return true;
    }
    a.iter().all(|(k1, v1)| {
        b.iter()
            .find(|(k2, _)| k1.semantic_eq(k2, eps))
            .is_some_and(|(_, v2)| v1.semantic_eq(v2, eps))
    })
}

fn container_type_eq(a: &ContainerType, b: &ContainerType, eps: f64) -> bool {
    match (a, b) {
        (ContainerType::Named(x), ContainerType::Named(y)) => x == y,
        (ContainerType::Script(x), ContainerType::Script(y)) => x.semantic_eq(y, eps),
        _ => false,
    }
}

/// Escapes a string the way Godot's `String::c_escape_multiline` does
/// (used for `String` values): only `\` and `"` are escaped.
pub fn escape_multiline(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escapes a string the way Godot's `String::c_escape` does
/// (used for `StringName`, `NodePath` and header fields).
pub fn escape_c(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

fn write_container_type(f: &mut fmt::Formatter<'_>, t: &ContainerType) -> fmt::Result {
    match t {
        ContainerType::Named(n) => f.write_str(n),
        ContainerType::Script(v) => write!(f, "{v}"),
    }
}

fn write_dict(f: &mut fmt::Formatter<'_>, entries: &[(Value, Value)]) -> fmt::Result {
    if entries.is_empty() {
        return f.write_str("{}");
    }
    f.write_str("{\n")?;
    for (i, (k, v)) in entries.iter().enumerate() {
        write!(f, "{k}: {v}")?;
        f.write_str(if i + 1 < entries.len() { ",\n" } else { "\n" })?;
    }
    f.write_str("}")
}

fn write_list(f: &mut fmt::Formatter<'_>, items: &[Value]) -> fmt::Result {
    for (i, v) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{v}")?;
    }
    Ok(())
}

/// Writes the value in Godot's text syntax (following `VariantWriter`),
/// reusing the original spelling of numbers.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(b) => f.write_str(if *b { "true" } else { "false" }),
            Value::Number(n) => f.write_str(&n.raw),
            Value::String(s) => write!(f, "\"{}\"", escape_multiline(s)),
            Value::StringName(s) => write!(f, "&\"{}\"", escape_c(s)),
            Value::HexColor(s) => f.write_str(s),
            Value::NodePath(s) => write!(f, "NodePath(\"{}\")", escape_c(s)),
            Value::Construct { name, args } => {
                write!(f, "{name}(")?;
                write_list(f, args)?;
                f.write_char(')')
            }
            Value::ExtResource(id) => write!(f, "ExtResource(\"{}\")", escape_multiline(id)),
            Value::SubResource(id) => write!(f, "SubResource(\"{}\")", escape_multiline(id)),
            Value::Resource(parts) => {
                f.write_str("Resource(")?;
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "\"{}\"", escape_multiline(p))?;
                }
                f.write_char(')')
            }
            Value::Array(items) => {
                f.write_char('[')?;
                write_list(f, items)?;
                f.write_char(']')
            }
            Value::TypedArray { elem, items } => {
                f.write_str("Array[")?;
                write_container_type(f, elem)?;
                f.write_str("]([")?;
                write_list(f, items)?;
                f.write_str("])")
            }
            Value::Dictionary(entries) => write_dict(f, entries),
            Value::TypedDictionary {
                key,
                value,
                entries,
            } => {
                f.write_str("Dictionary[")?;
                write_container_type(f, key)?;
                f.write_str(", ")?;
                write_container_type(f, value)?;
                f.write_str("](")?;
                write_dict(f, entries)?;
                f.write_char(')')
            }
            Value::Object { class, props } => {
                writeln!(f, "Object({class},")?;
                for (i, (k, v)) in props.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",\n")?;
                    }
                    write!(f, "\"{}\": {v}", escape_multiline(k))?;
                }
                f.write_str("\n)")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_equality() {
        let n = |s: &str| Number::new(s);
        assert!(n("1.0").approx_eq(&n("1.00"), 0.0));
        assert!(n("-0.0").approx_eq(&n("0"), 0.0));
        assert!(n("nan").approx_eq(&n("nan"), 0.0));
        assert!(!n("1.0").approx_eq(&n("1.001"), 0.0));
        assert!(n("1.0").approx_eq(&n("1.001"), 0.01));
        // int vs float differ as values, but not as constructor args
        let int = Value::Number(n("1"));
        let float = Value::Number(n("1.0"));
        assert!(!int.semantic_eq(&float, 0.0));
        let v1 = Value::Construct {
            name: "Vector2".into(),
            args: vec![int.clone(), int],
        };
        let v2 = Value::Construct {
            name: "Vector2".into(),
            args: vec![float.clone(), float],
        };
        assert!(v1.semantic_eq(&v2, 0.0));
    }

    #[test]
    fn escaping() {
        assert_eq!(escape_multiline("a\"b\\c\nd"), "a\\\"b\\\\c\nd");
        assert_eq!(escape_c("a\"b\nc"), "a\\\"b\\nc");
    }
}
