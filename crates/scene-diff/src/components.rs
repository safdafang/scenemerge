//! Component-wise comparison of compound values (vectors, transforms,
//! colours, arrays, dictionaries) for readable diffs.

use godot_text::{canonical_construct_name, Value};

use crate::change::ComponentChange;

const XYZ: [&str; 3] = ["x", "y", "z"];
const XYZW: [&str; 4] = ["x", "y", "z", "w"];

/// Names of the constructor arguments, following the order `VariantWriter` uses.
pub(crate) fn component_names(constructor: &str) -> Option<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Some(match canonical_construct_name(constructor) {
        "Vector2" | "Vector2i" => s(&["x", "y"]),
        "Vector3" | "Vector3i" => s(&XYZ),
        "Vector4" | "Vector4i" | "Quaternion" => s(&XYZW),
        "Color" => s(&["r", "g", "b", "a"]),
        "Rect2" | "Rect2i" => s(&["position.x", "position.y", "size.x", "size.y"]),
        "AABB" => s(&[
            "position.x",
            "position.y",
            "position.z",
            "size.x",
            "size.y",
            "size.z",
        ]),
        "Plane" => s(&["normal.x", "normal.y", "normal.z", "d"]),
        // columns[i][j] written column by column.
        "Transform2D" => s(&["x.x", "x.y", "y.x", "y.y", "origin.x", "origin.y"]),
        // Basis is written row by row: argument i*3+j is rows[i][j], i.e.
        // component i of column (axis) j.
        "Basis" => basis_names(""),
        "Transform3D" => {
            let mut v = basis_names("basis.");
            v.extend(XYZ.iter().map(|c| format!("origin.{c}")));
            v
        }
        "Projection" => {
            let mut v = Vec::new();
            for col in XYZW {
                for comp in XYZW {
                    v.push(format!("{col}.{comp}"));
                }
            }
            v
        }
        _ => return None,
    })
}

fn basis_names(prefix: &str) -> Vec<String> {
    let mut v = Vec::new();
    for row in XYZ {
        for axis in XYZ {
            v.push(format!("{prefix}{axis}.{row}"));
        }
    }
    v
}

/// Maximum number of element-level changes listed for arrays/dictionaries
/// before falling back to a summary.
const MAX_ELEMENT_CHANGES: usize = 8;

/// Computes component changes between `old` and `new` (both already
/// normalised for comparison). `show` renders a value for display, `eq`
/// compares two values. Returns an empty list when no readable breakdown exists.
pub(crate) fn components(
    old: &Value,
    new: &Value,
    show_old: &dyn Fn(&Value) -> String,
    show_new: &dyn Fn(&Value) -> String,
    eq: &dyn Fn(&Value, &Value) -> bool,
) -> Vec<ComponentChange> {
    match (old, new) {
        (Value::Construct { name: n1, args: a1 }, Value::Construct { name: n2, args: a2 })
            if canonical_construct_name(n1) == canonical_construct_name(n2) =>
        {
            if let Some(names) = component_names(n1) {
                if a1.len() == names.len() && a2.len() == names.len() {
                    return names
                        .iter()
                        .zip(a1.iter().zip(a2))
                        .filter(|(_, (x, y))| !eq(x, y))
                        .map(|(name, (x, y))| ComponentChange {
                            name: name.clone(),
                            old: Some(show_old(x)),
                            new: Some(show_new(y)),
                        })
                        .collect();
                }
                return Vec::new();
            }
            // Packed arrays: element-wise when the length is unchanged.
            sequence(a1, a2, show_old, show_new, eq)
        }
        (Value::Array(a1), Value::Array(a2))
        | (Value::TypedArray { items: a1, .. }, Value::TypedArray { items: a2, .. }) => {
            sequence(a1, a2, show_old, show_new, eq)
        }
        (Value::Dictionary(d1), Value::Dictionary(d2))
        | (
            Value::TypedDictionary { entries: d1, .. },
            Value::TypedDictionary { entries: d2, .. },
        ) => dictionary(d1, d2, show_old, show_new, eq),
        _ => Vec::new(),
    }
}

fn sequence(
    a1: &[Value],
    a2: &[Value],
    show_old: &dyn Fn(&Value) -> String,
    show_new: &dyn Fn(&Value) -> String,
    eq: &dyn Fn(&Value, &Value) -> bool,
) -> Vec<ComponentChange> {
    if a1.len() != a2.len() {
        return Vec::new();
    }
    let changed: Vec<ComponentChange> = a1
        .iter()
        .zip(a2)
        .enumerate()
        .filter(|(_, (x, y))| !eq(x, y))
        .map(|(i, (x, y))| ComponentChange {
            name: format!("[{i}]"),
            old: Some(show_old(x)),
            new: Some(show_new(y)),
        })
        .collect();
    if changed.len() > MAX_ELEMENT_CHANGES {
        Vec::new()
    } else {
        changed
    }
}

fn dictionary(
    d1: &[(Value, Value)],
    d2: &[(Value, Value)],
    show_old: &dyn Fn(&Value) -> String,
    show_new: &dyn Fn(&Value) -> String,
    eq: &dyn Fn(&Value, &Value) -> bool,
) -> Vec<ComponentChange> {
    let mut out = Vec::new();
    for (k2, v2) in d2 {
        match d1.iter().find(|(k1, _)| eq(k1, k2)) {
            Some((_, v1)) if eq(v1, v2) => {}
            Some((_, v1)) => out.push(ComponentChange {
                name: format!("[{}]", show_new(k2)),
                old: Some(show_old(v1)),
                new: Some(show_new(v2)),
            }),
            None => out.push(ComponentChange {
                name: format!("[{}]", show_new(k2)),
                old: None,
                new: Some(show_new(v2)),
            }),
        }
    }
    for (k1, v1) in d1 {
        if !d2.iter().any(|(k2, _)| eq(k1, k2)) {
            out.push(ComponentChange {
                name: format!("[{}]", show_old(k1)),
                old: Some(show_old(v1)),
                new: None,
            });
        }
    }
    if out.len() > MAX_ELEMENT_CHANGES {
        Vec::new()
    } else {
        out
    }
}
