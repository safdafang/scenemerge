//! `diff`, `textconv` and `git-diff` commands.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use scene_diff::{canonical_text, diff, render_human, to_json, DiffOptions, HumanOptions};
use scene_model::{MatchOptions, SceneFile};

use crate::io::{empty_like, load, load_optional};
use crate::{ColorWhen, CompareArgs, DiffFormat};

/// `scenemerge diff`
#[derive(Debug, clap::Args)]
pub struct DiffArgs {
    /// Old version (`/dev/null` for "no file").
    pub old: PathBuf,
    /// New version (`/dev/null` for "no file").
    pub new: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = DiffFormat::Human)]
    pub format: DiffFormat,
    /// When to use colours (human format).
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto)]
    pub color: ColorWhen,
    /// Exit with 1 if there are semantic differences (like `git diff --exit-code`).
    #[arg(long)]
    pub exit_code: bool,
    /// Name to show for the file (defaults to the new path).
    #[arg(long, value_name = "NAME")]
    pub label: Option<String>,
    #[command(flatten)]
    pub compare: CompareArgs,
}

/// Arguments Git passes to an external diff program.
#[derive(Debug, clap::Args)]
pub struct GitDiffArgs {
    /// `path old-file old-hex old-mode new-file new-hex new-mode [new-path info]`
    /// (or just `path` for an unmerged file).
    #[arg(num_args = 1.., allow_hyphen_values = true)]
    pub args: Vec<String>,
}

pub fn diff_options(c: &CompareArgs) -> DiffOptions {
    DiffOptions {
        matching: MatchOptions {
            rename_threshold: c.rename_threshold,
            epsilon: c.epsilon,
        },
        ignore_editor_metadata: c.ignore_editor_metadata,
    }
}

fn root_name(f: &SceneFile) -> String {
    f.nodes
        .first()
        .map_or_else(|| ".".into(), |n| n.name.clone())
}

fn set_color(when: ColorWhen) {
    let choice = match when {
        ColorWhen::Auto => anstream::ColorChoice::Auto,
        ColorWhen::Always => anstream::ColorChoice::Always,
        ColorWhen::Never => anstream::ColorChoice::Never,
    };
    choice.write_global();
}

/// Loads both sides; a missing side becomes an empty file of the same kind.
fn load_pair(
    old: &Path,
    new: &Path,
    old_name: &Path,
    new_name: &Path,
) -> Result<(SceneFile, SceneFile, &'static str)> {
    let a = load_optional(old, old_name)?;
    let b = load_optional(new, new_name)?;
    Ok(match (a, b) {
        (Some(a), Some(b)) => (a, b, ""),
        (None, Some(b)) => (empty_like(&b), b, " (new file)"),
        (Some(a), None) => {
            let e = empty_like(&a);
            (a, e, " (deleted)")
        }
        (None, None) => anyhow::bail!("both sides are /dev/null"),
    })
}

pub fn run(args: &DiffArgs) -> Result<u8> {
    set_color(args.color);
    let (a, b, note) = load_pair(&args.old, &args.new, &args.old, &args.new)?;
    let title = args
        .label
        .clone()
        .unwrap_or_else(|| args.new.display().to_string().replace('\\', "/"));
    let opts = diff_options(&args.compare);
    let d = diff(&a, &b, &opts);
    let mut out = anstream::stdout();
    match args.format {
        DiffFormat::Human => {
            let text = render_human(
                &format!("{title}{note}"),
                &d,
                &HumanOptions {
                    color: args.color != ColorWhen::Never,
                    old_root: root_name(&a),
                    new_root: root_name(&b),
                    ..HumanOptions::default()
                },
            );
            out.write_all(text.as_bytes())?;
        }
        DiffFormat::Json => {
            let old_name = args.old.display().to_string().replace('\\', "/");
            let new_name = args.new.display().to_string().replace('\\', "/");
            writeln!(out, "{}", to_json(&old_name, &new_name, &d))?;
        }
        DiffFormat::Text => {
            let (ta, tb) = (canonical_text(&a), canonical_text(&b));
            let old_name = format!("a/{}", args.old.display()).replace('\\', "/");
            let new_name = format!("b/{}", args.new.display()).replace('\\', "/");
            let text = similar::TextDiff::from_lines(&ta, &tb)
                .unified_diff()
                .context_radius(3)
                .header(&old_name, &new_name)
                .to_string();
            out.write_all(text.as_bytes())?;
        }
    }
    out.flush()?;
    Ok(u8::from(args.exit_code && !d.is_empty()))
}

pub fn textconv(file: &Path) -> Result<u8> {
    let f = load(file, file)?;
    let mut out = std::io::stdout().lock();
    out.write_all(canonical_text(&f).as_bytes())?;
    out.flush()?;
    Ok(0)
}

/// GIT_EXTERNAL_DIFF protocol. Must exit 0 unless
/// `GIT_EXTERNAL_DIFF_TRUST_EXIT_CODE` is true (then 1 means "different").
pub fn git_diff(args: &GitDiffArgs) -> Result<u8> {
    let a = &args.args;
    let (Some(path), Some(old), Some(new)) = (a.first(), a.get(1), a.get(4)) else {
        // Unmerged path: Git passes only the path. Nothing sensible to show.
        if let Some(p) = a.first() {
            println!("* Unmerged path {p}");
        }
        return Ok(0);
    };
    let new_path = a.get(7).unwrap_or(path);
    // Git pipes our output into its pager; colour like Git does then.
    let colour = if std::env::var_os("GIT_PAGER_IN_USE").is_some() {
        ColorWhen::Always
    } else {
        ColorWhen::Auto
    };
    set_color(colour);
    let (fa, fb, note) = load_pair(
        Path::new(old),
        Path::new(new),
        Path::new(path),
        Path::new(new_path),
    )?;
    let d = diff(&fa, &fb, &DiffOptions::default());
    let title = if new_path != path {
        format!("{path} → {new_path}{note}")
    } else {
        format!("{path}{note}")
    };
    let text = render_human(
        &title,
        &d,
        &HumanOptions {
            color: true,
            old_root: root_name(&fa),
            new_root: root_name(&fb),
            ..HumanOptions::default()
        },
    );
    let mut out = anstream::stdout();
    out.write_all(text.as_bytes())?;
    out.flush()?;
    let trust = std::env::var("GIT_EXTERNAL_DIFF_TRUST_EXIT_CODE")
        .is_ok_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));
    Ok(u8::from(trust && !d.is_empty()))
}
