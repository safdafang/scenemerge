//! Human-readable rendering of a [`FileDiff`].

use std::fmt::Write as _;

use anstyle::{AnsiColor, Style};

use crate::change::{Change, ComponentChange, ConnectionKey, FieldChange, Target};
use crate::diff::FileDiff;

/// Options for [`render_human`].
#[derive(Debug, Clone)]
pub struct HumanOptions {
    /// Emit ANSI colours.
    pub color: bool,
    /// Values longer than this many characters are shortened with `…`.
    pub max_value_len: usize,
    /// Name of the root node in the old version (used to display the root path `.`).
    pub old_root: String,
    /// Name of the root node in the new version.
    pub new_root: String,
}

impl Default for HumanOptions {
    fn default() -> Self {
        HumanOptions {
            color: false,
            max_value_len: 80,
            old_root: ".".into(),
            new_root: ".".into(),
        }
    }
}

#[derive(Clone, Copy)]
enum Mark {
    Added,
    Removed,
    Changed,
    Renamed,
    Moved,
}

impl Mark {
    fn symbol(self) -> &'static str {
        match self {
            Mark::Added => "+",
            Mark::Removed => "-",
            Mark::Changed => "~",
            Mark::Renamed => "✎",
            Mark::Moved => "→",
        }
    }

    fn style(self) -> Style {
        let c = match self {
            Mark::Added => AnsiColor::Green,
            Mark::Removed => AnsiColor::Red,
            Mark::Changed => AnsiColor::Yellow,
            Mark::Renamed | Mark::Moved => AnsiColor::Cyan,
        };
        Style::new().fg_color(Some(c.into()))
    }
}

struct Group {
    key: String,
    head: Option<(Mark, String)>,
    default_head: String,
    details: Vec<(Option<Mark>, String)>,
}

/// `(group key, default headline, headline, detail line)` for one change.
type GroupParts = (
    String,
    String,
    Option<(Mark, String)>,
    Option<(Option<Mark>, String)>,
);

struct Renderer<'a> {
    opts: &'a HumanOptions,
}

impl Renderer<'_> {
    fn short(&self, s: &str) -> String {
        let one_line: String = s
            .split('\n')
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join(" ");
        let n = one_line.chars().count();
        if n <= self.opts.max_value_len {
            one_line
        } else {
            let keep = self.opts.max_value_len.saturating_sub(1);
            let mut out: String = one_line.chars().take(keep).collect();
            out.push('…');
            out
        }
    }

    fn new_path(&self, p: &str) -> String {
        if p == "." {
            self.opts.new_root.clone()
        } else {
            p.to_string()
        }
    }

    fn old_path(&self, p: &str) -> String {
        if p == "." {
            self.opts.old_root.clone()
        } else {
            p.to_string()
        }
    }

    fn field(&self, f: &FieldChange) -> String {
        let show =
            |v: &Option<String>| v.as_deref().map_or("(none)".to_string(), |s| self.short(s));
        format!("{}: {} → {}", f.field, show(&f.old), show(&f.new))
    }

    fn signal(&self, k: &ConnectionKey) -> String {
        format!(
            "signal {}.{} → {}.{}",
            self.new_path(&k.from),
            k.signal,
            self.new_path(&k.to),
            k.method
        )
    }

    fn component(&self, property: &str, c: &ComponentChange) -> String {
        let sep = if c.name.starts_with('[') { "" } else { "." };
        let show =
            |v: &Option<String>| v.as_deref().map_or("(none)".to_string(), |s| self.short(s));
        format!(
            "{property}{sep}{}: {} → {}",
            c.name,
            show(&c.old),
            show(&c.new)
        )
    }

    fn target_key(&self, t: &Target) -> (String, String) {
        match t {
            Target::Node { path } => (format!("node:{path}"), self.new_path(path)),
            Target::SubResource { id, type_name } => (
                format!("sub:{id}"),
                format!("sub_resource {id} ({type_name})"),
            ),
            Target::Resource => ("resource".into(), "[resource]".into()),
        }
    }

    fn groups(&self, changes: &[Change]) -> Vec<Group> {
        let mut groups: Vec<Group> = Vec::new();
        for c in changes {
            let (key, default_head, head, detail): GroupParts = match c {
                Change::HeaderChanged(f) => (
                    "header".into(),
                    "file descriptor".into(),
                    None,
                    Some((None, self.field(f))),
                ),
                Change::ExtResourceAdded {
                    path, type_name, ..
                } => (
                    format!("ext+:{path}"),
                    String::new(),
                    Some((Mark::Added, format!("ext_resource {path} ({type_name})"))),
                    None,
                ),
                Change::ExtResourceRemoved {
                    path, type_name, ..
                } => (
                    format!("ext-:{path}"),
                    String::new(),
                    Some((Mark::Removed, format!("ext_resource {path} ({type_name})"))),
                    None,
                ),
                Change::ExtResourceChanged { path, changes } => {
                    let mut lines = changes.iter().map(|f| self.field(f));
                    let first = lines.next().unwrap_or_default();
                    let key = format!("ext~:{path}");
                    let mut g = Group {
                        key: key.clone(),
                        head: Some((Mark::Changed, format!("ext_resource {path}"))),
                        default_head: String::new(),
                        details: vec![(None, first)],
                    };
                    g.details.extend(lines.map(|l| (None, l)));
                    groups.push(g);
                    continue;
                }
                Change::SubResourceAdded { id, type_name } => (
                    format!("sub:{id}"),
                    String::new(),
                    Some((Mark::Added, format!("sub_resource {id} ({type_name})"))),
                    None,
                ),
                Change::SubResourceRemoved { id, type_name } => (
                    format!("sub-:{id}"),
                    String::new(),
                    Some((Mark::Removed, format!("sub_resource {id} ({type_name})"))),
                    None,
                ),
                Change::SubResourceChanged {
                    id,
                    old_type,
                    new_type,
                } => (
                    format!("sub:{id}"),
                    format!("sub_resource {id} ({new_type})"),
                    None,
                    Some((None, format!("type: {old_type} → {new_type}"))),
                ),
                Change::NodeAdded {
                    path,
                    type_name,
                    instance,
                } => {
                    let what = match (type_name, instance) {
                        (Some(t), Some(i)) => format!(" ({t}, instance of {i})"),
                        (Some(t), None) => format!(" ({t})"),
                        (None, Some(i)) => format!(" (instance of {i})"),
                        (None, None) => String::new(),
                    };
                    (
                        format!("node:{path}"),
                        String::new(),
                        Some((Mark::Added, format!("{}{what}", self.new_path(path)))),
                        None,
                    )
                }
                Change::NodeRemoved { path, type_name } => (
                    format!("node-:{path}"),
                    String::new(),
                    Some((
                        Mark::Removed,
                        match type_name {
                            Some(t) => format!("{} ({t})", self.old_path(path)),
                            None => self.old_path(path),
                        },
                    )),
                    None,
                ),
                Change::NodeRenamed {
                    path,
                    new_name,
                    new_path,
                } => (
                    format!("node:{new_path}"),
                    String::new(),
                    Some((
                        Mark::Renamed,
                        format!("{} renamed to {new_name}", self.old_path(path)),
                    )),
                    None,
                ),
                Change::NodeMoved { path, new_path } => (
                    format!("node:{new_path}"),
                    String::new(),
                    Some((
                        Mark::Moved,
                        format!(
                            "{} moved to {}",
                            self.old_path(path),
                            self.new_path(new_path)
                        ),
                    )),
                    None,
                ),
                Change::NodeReordered {
                    path,
                    old_position,
                    new_position,
                } => (
                    format!("node:{path}"),
                    self.new_path(path),
                    None,
                    Some((
                        None,
                        format!("position among siblings: {old_position} → {new_position}"),
                    )),
                ),
                Change::NodeTypeChanged { path, old, new } => (
                    format!("node:{path}"),
                    self.new_path(path),
                    None,
                    Some((
                        None,
                        format!(
                            "type: {} → {}",
                            old.as_deref().unwrap_or("(none)"),
                            new.as_deref().unwrap_or("(none)")
                        ),
                    )),
                ),
                Change::InstanceChanged { path, old, new } => (
                    format!("node:{path}"),
                    self.new_path(path),
                    None,
                    Some((
                        None,
                        format!(
                            "instance: {} → {}",
                            old.as_deref().unwrap_or("(none)"),
                            new.as_deref().unwrap_or("(none)")
                        ),
                    )),
                ),
                Change::GroupsChanged {
                    path,
                    added,
                    removed,
                } => {
                    let mut parts: Vec<String> = added.iter().map(|g| format!("+{g}")).collect();
                    parts.extend(removed.iter().map(|g| format!("-{g}")));
                    (
                        format!("node:{path}"),
                        self.new_path(path),
                        None,
                        Some((None, format!("groups: {}", parts.join(" ")))),
                    )
                }
                Change::NodeAttributeChanged { path, change } => (
                    format!("node:{path}"),
                    self.new_path(path),
                    None,
                    Some((None, self.field(change))),
                ),
                Change::PropertyAdded {
                    target,
                    property,
                    new,
                } => {
                    let (key, head) = self.target_key(target);
                    (
                        key,
                        head,
                        None,
                        Some((
                            Some(Mark::Added),
                            format!("{property} = {}", self.short(new)),
                        )),
                    )
                }
                Change::PropertyRemoved {
                    target,
                    property,
                    old,
                } => {
                    let (key, head) = self.target_key(target);
                    (
                        key,
                        head,
                        None,
                        Some((
                            Some(Mark::Removed),
                            format!("{property} (was {})", self.short(old)),
                        )),
                    )
                }
                Change::PropertyChanged {
                    target,
                    property,
                    old,
                    new,
                    components,
                } => {
                    let (key, head) = self.target_key(target);
                    if !components.is_empty() && components.len() <= 6 {
                        let lines: Vec<(Option<Mark>, String)> = components
                            .iter()
                            .map(|c| (None, self.component(property, c)))
                            .collect();
                        push(&mut groups, key, head, None, lines);
                        continue;
                    }
                    (
                        key,
                        head,
                        None,
                        Some((
                            None,
                            format!("{property}: {} → {}", self.short(old), self.short(new)),
                        )),
                    )
                }
                Change::ConnectionAdded(k) => (
                    format!("conn+:{}", self.signal(k)),
                    String::new(),
                    Some((Mark::Added, self.signal(k))),
                    None,
                ),
                Change::ConnectionRemoved(k) => (
                    format!("conn-:{}", self.signal(k)),
                    String::new(),
                    Some((Mark::Removed, self.signal(k))),
                    None,
                ),
                Change::ConnectionChanged {
                    connection,
                    changes,
                } => {
                    let lines = changes.iter().map(|f| (None, self.field(f))).collect();
                    push(
                        &mut groups,
                        format!("conn~:{}", self.signal(connection)),
                        String::new(),
                        Some((Mark::Changed, self.signal(connection))),
                        lines,
                    );
                    continue;
                }
                Change::EditableAdded { path } => (
                    format!("edit+:{path}"),
                    String::new(),
                    Some((Mark::Added, format!("editable {}", self.new_path(path)))),
                    None,
                ),
                Change::EditableRemoved { path } => (
                    format!("edit-:{path}"),
                    String::new(),
                    Some((Mark::Removed, format!("editable {}", self.old_path(path)))),
                    None,
                ),
            };
            push(
                &mut groups,
                key,
                default_head,
                head,
                detail.into_iter().collect(),
            );
        }
        groups
    }
}

fn push(
    groups: &mut Vec<Group>,
    key: String,
    default_head: String,
    head: Option<(Mark, String)>,
    details: Vec<(Option<Mark>, String)>,
) {
    if let Some(last) = groups.last_mut() {
        if last.key == key {
            if last.head.is_none() {
                last.head = head;
            } else if let Some(h) = head {
                // A second headline for the same subject (e.g. moved + renamed).
                last.details.insert(0, (Some(h.0), h.1));
            }
            last.details.extend(details);
            return;
        }
    }
    groups.push(Group {
        key,
        head,
        default_head,
        details,
    });
}

/// Renders the diff as an indented, optionally coloured tree:
///
/// ```text
/// scenes/level.tscn
/// ~ Player/Camera3D
///     fov: 75.0 → 90.0
/// + Enemies/Boss (CharacterBody3D, instance of res://enemies/boss.tscn)
/// - Enemies/OldSpawner
/// ```
pub fn render_human(title: &str, diff: &FileDiff, opts: &HumanOptions) -> String {
    let r = Renderer { opts };
    let paint = |style: Style, text: &str| {
        if opts.color {
            format!("{}{text}{}", style.render(), style.render_reset())
        } else {
            text.to_string()
        }
    };
    let mut out = String::new();
    let _ = writeln!(out, "{}", paint(Style::new().bold(), title));
    if diff.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            paint(Style::new().dimmed(), "  (no semantic changes)")
        );
        return out;
    }
    for g in r.groups(&diff.changes) {
        let (mark, text) = g.head.unwrap_or((Mark::Changed, g.default_head));
        let _ = writeln!(
            out,
            "{} {}",
            paint(mark.style().bold(), mark.symbol()),
            paint(mark.style(), &text)
        );
        for (m, line) in g.details {
            match m {
                Some(m) => {
                    let _ = writeln!(
                        out,
                        "    {} {}",
                        paint(m.style(), m.symbol()),
                        paint(m.style(), &line)
                    );
                }
                None => {
                    let _ = writeln!(out, "    {line}");
                }
            }
        }
    }
    out
}
