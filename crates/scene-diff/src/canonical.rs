use std::fmt::Write as _;

use godot_text::{ContainerType, Number, Value};
use scene_model::{SceneFile, DEFAULT_CONNECTION_FLAGS};

fn canon_number(n: &Number, in_construct: bool) -> Number {
    if !n.is_float() {
        return match n.as_i64() {
            Some(0) => Number::new("0"),
            Some(v) => Number::new(v.to_string()),
            None => n.clone(),
        };
    }
    let v = n.as_f64();
    if v.is_nan() {
        return Number::new("nan");
    }
    if v.is_infinite() {
        return Number::new(if v > 0.0 { "inf" } else { "-inf" });
    }
    let v = if v == 0.0 { 0.0 } else { v };
    let mut s = format!("{v}");
    if !in_construct && !s.contains(['.', 'e', 'E']) {
        s.push_str(".0");
    }
    Number::new(s)
}

fn canon_type(t: &ContainerType, ext: &dyn Fn(&str) -> Option<String>) -> ContainerType {
    match t {
        ContainerType::Named(n) => ContainerType::Named(n.clone()),
        ContainerType::Script(v) => ContainerType::Script(Box::new(canon(v, false, ext))),
    }
}

fn canon(v: &Value, in_construct: bool, ext: &dyn Fn(&str) -> Option<String>) -> Value {
    match v {
        Value::Number(n) => Value::Number(canon_number(n, in_construct)),
        Value::ExtResource(id) => Value::ExtResource(ext(id).unwrap_or_else(|| id.clone())),
        Value::Construct { name, args } => Value::Construct {
            name: godot_text::canonical_construct_name(name).to_string(),
            args: args.iter().map(|a| canon(a, true, ext)).collect(),
        },
        Value::Array(items) => Value::Array(items.iter().map(|x| canon(x, false, ext)).collect()),
        Value::TypedArray { elem, items } => Value::TypedArray {
            elem: canon_type(elem, ext),
            items: items.iter().map(|x| canon(x, false, ext)).collect(),
        },
        Value::Dictionary(entries) => Value::Dictionary(
            entries
                .iter()
                .map(|(k, x)| (canon(k, false, ext), canon(x, false, ext)))
                .collect(),
        ),
        Value::TypedDictionary {
            key,
            value,
            entries,
        } => Value::TypedDictionary {
            key: canon_type(key, ext),
            value: canon_type(value, ext),
            entries: entries
                .iter()
                .map(|(k, x)| (canon(k, false, ext), canon(x, false, ext)))
                .collect(),
        },
        Value::Object { class, props } => Value::Object {
            class: class.clone(),
            props: props
                .iter()
                .map(|(k, x)| (k.clone(), canon(x, false, ext)))
                .collect(),
        },
        other => other.clone(),
    }
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", godot_text::escape_c(s))
}

/// Produces the canonical text of `file`.
pub fn canonical_text(file: &SceneFile) -> String {
    let ext_path = |id: &str| {
        file.ext_by_id(id)
            .and_then(|i| file.ext_resources.get(i))
            .map(|e| e.path.clone())
    };
    let value = |v: &Value| -> String { canon(v, false, &ext_path).to_string() };
    let mut out = String::new();

    if let Some(first) = file.doc.sections.first() {
        let _ = write!(out, "[{}", first.header.name);
        for f in &first.header.fields {
            if f.key != "load_steps" {
                let _ = write!(out, " {}={}", f.key, value(&f.value));
            }
        }
        out.push_str("]\n");
    }

    let mut exts: Vec<_> = file.ext_resources.iter().collect();
    exts.sort_by(|a, b| a.path.cmp(&b.path).then(a.uid.cmp(&b.uid)));
    if !exts.is_empty() {
        out.push('\n');
    }
    for e in exts {
        let _ = write!(out, "[ext_resource type={}", quoted(&e.type_name));
        if let Some(uid) = &e.uid {
            let _ = write!(out, " uid={}", quoted(uid));
        }
        let _ = writeln!(out, " path={}]", quoted(&e.path));
    }

    let props = |out: &mut String, props: &[scene_model::Prop]| {
        for p in props {
            let _ = writeln!(
                out,
                "{} = {}",
                godot_text::encode_property_name(&p.key),
                value(&p.value)
            );
        }
    };

    for s in &file.sub_resources {
        let _ = writeln!(
            out,
            "\n[sub_resource type={} id={}]",
            quoted(&s.type_name),
            quoted(&s.id)
        );
        props(&mut out, &s.props);
    }

    for n in &file.nodes {
        let _ = write!(
            out,
            "\n[node path={} name={}",
            quoted(&n.path),
            quoted(&n.name)
        );
        if let Some(t) = &n.type_name {
            let _ = write!(out, " type={}", quoted(t));
        }
        if let Some(i) = &n.instance {
            let _ = write!(out, " instance={}", value(i));
        }
        if let Some(p) = &n.instance_placeholder {
            let _ = write!(out, " instance_placeholder={}", quoted(p));
        }
        if let Some(o) = &n.owner {
            let _ = write!(out, " owner={}", quoted(o));
        }
        if let Some(i) = n.index {
            let _ = write!(out, " index={i}");
        }
        if !n.groups.is_empty() {
            let g: Vec<String> = n.groups.iter().map(|g| quoted(g)).collect();
            let _ = write!(out, " groups=[{}]", g.join(", "));
        }
        for (k, v) in &n.other_fields {
            if k != "parent_id_path" && k != "owner_uid_path" {
                let _ = write!(out, " {k}={}", value(v));
            }
        }
        out.push_str("]\n");
        props(&mut out, &n.props);
    }

    if file.resource_section.is_some() {
        out.push_str("\n[resource]\n");
        props(&mut out, &file.resource_props);
    }

    let mut conns: Vec<String> = file
        .connections
        .iter()
        .map(|c| {
            let mut s = format!(
                "[connection signal={} from={} to={} method={}",
                quoted(&c.signal),
                quoted(&c.from),
                quoted(&c.to),
                quoted(&c.method)
            );
            if c.flags != DEFAULT_CONNECTION_FLAGS {
                let _ = write!(s, " flags={}", c.flags);
            }
            if c.unbinds != 0 {
                let _ = write!(s, " unbinds={}", c.unbinds);
            }
            if let Some(b) = &c.binds {
                let _ = write!(s, " binds={}", value(b));
            }
            for (k, v) in &c.other_fields {
                if k != "from_uid_path" && k != "to_uid_path" {
                    let _ = write!(s, " {k}={}", value(v));
                }
            }
            s.push(']');
            s
        })
        .collect();
    conns.sort();
    if !conns.is_empty() {
        out.push('\n');
    }
    for c in conns {
        let _ = writeln!(out, "{c}");
    }

    let mut edits: Vec<&str> = file.editables.iter().map(|e| e.path.as_str()).collect();
    edits.sort_unstable();
    if !edits.is_empty() {
        out.push('\n');
    }
    for e in edits {
        let _ = writeln!(out, "[editable path={}]", quoted(e));
    }
    out
}
