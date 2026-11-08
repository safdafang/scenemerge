use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use scene_merge::{merge, ConflictStyle, MarkerStyle, MergeOptions, Report, REPORT_SCHEMA_VERSION};
use scene_model::MatchOptions;

use crate::io::{load, write_atomic};
use crate::CompareArgs;

/// How conflicts are resolved in the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ConflictArg {
    /// Git conflict markers around the smallest conflicting unit.
    Markers,
    /// Take our side for every conflict.
    Ours,
    /// Take their side for every conflict.
    Theirs,
}

/// What to do when a file cannot be merged semantically (e.g. it does not parse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OnError {
    /// Fail with exit code 2 and leave the output untouched.
    Fail,
    /// Fall back to a line-based merge with `git merge-file` (what Git would do without scenemerge).
    Text,
}

/// `scenemerge merge`
#[derive(Debug, clap::Args)]
pub struct MergeArgs {
    /// Common ancestor (BASE, Git's %O).
    pub base: PathBuf,
    /// Our version (OURS, Git's %A).
    pub ours: PathBuf,
    /// Their version (THEIRS, Git's %B).
    pub theirs: PathBuf,
    /// Output file (default: standard output). Git uses `-o %A`.
    #[arg(short = 'o', long = "output", value_name = "OUT")]
    pub output: Option<PathBuf>,
    /// The file's path in the repository (Git's %P), used in messages.
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
    /// How to resolve conflicts.
    #[arg(long, value_enum, default_value_t = ConflictArg::Markers)]
    pub conflict: ConflictArg,
    /// Write a JSON conflict report to this file.
    #[arg(long, value_name = "FILE")]
    pub report: Option<PathBuf>,
    /// Conflict marker length (Git's %L).
    #[arg(long, value_name = "N", default_value_t = 7)]
    pub marker_size: usize,
    /// Label for our side of conflict markers (Git's %X).
    #[arg(long, value_name = "LABEL", default_value = "ours")]
    pub label_ours: String,
    /// Label for the common ancestor (Git's %S); shown in messages only.
    #[arg(long, value_name = "LABEL", default_value = "base")]
    pub label_base: String,
    /// Label for their side of conflict markers (Git's %Y).
    #[arg(long, value_name = "LABEL", default_value = "theirs")]
    pub label_theirs: String,
    /// What to do if the files cannot be merged semantically.
    #[arg(long, value_enum, default_value_t = OnError::Fail)]
    pub on_error: OnError,
    /// Do not print the conflict summary.
    #[arg(short, long)]
    pub quiet: bool,
    #[command(flatten)]
    pub compare: CompareArgs,
}

fn display_name(args: &MergeArgs) -> String {
    args.path
        .clone()
        .unwrap_or_else(|| args.ours.display().to_string())
        .replace('\\', "/")
}

pub fn run(args: &MergeArgs) -> Result<u8> {
    match semantic_merge(args) {
        Ok(code) => Ok(code),
        Err(e) if args.on_error == OnError::Text => {
            eprintln!(
                "scenemerge: {}: semantic merge not possible ({e:#}); falling back to a line-based merge",
                display_name(args)
            );
            text_merge(args)
        }
        Err(e) => Err(e),
    }
}

fn semantic_merge(args: &MergeArgs) -> Result<u8> {
    let name = display_name(args);
    let shown = |p: &Path, side: &str| -> PathBuf {
        if args.path.is_some() {
            PathBuf::from(format!("{name} ({side})"))
        } else {
            p.to_path_buf()
        }
    };
    let (base, ours, theirs) = std::thread::scope(|scope| {
        let failed = || Err(anyhow::anyhow!("internal error while loading"));
        let hb = scope.spawn(|| load(&args.base, &shown(&args.base, &args.label_base)));
        let ht = scope.spawn(|| load(&args.theirs, &shown(&args.theirs, &args.label_theirs)));
        let o = load(&args.ours, &shown(&args.ours, &args.label_ours));
        (
            hb.join().unwrap_or_else(|_| failed()),
            o,
            ht.join().unwrap_or_else(|_| failed()),
        )
    });
    let (base, ours, theirs) = (base?, ours?, theirs?);
    let opts = MergeOptions {
        conflict: match args.conflict {
            ConflictArg::Markers => ConflictStyle::Markers,
            ConflictArg::Ours => ConflictStyle::Ours,
            ConflictArg::Theirs => ConflictStyle::Theirs,
        },
        markers: MarkerStyle {
            size: args.marker_size.max(1),
            ours: args.label_ours.clone(),
            theirs: args.label_theirs.clone(),
        },
        matching: MatchOptions {
            rename_threshold: args.compare.rename_threshold,
            epsilon: args.compare.epsilon,
        },
    };
    let out =
        merge(&base, &ours, &theirs, &opts).with_context(|| format!("cannot merge {name}"))?;

    match &args.output {
        Some(p) => write_atomic(p, out.text.as_bytes())?,
        None => {
            use std::io::Write;
            let mut so = std::io::stdout().lock();
            so.write_all(out.text.as_bytes())?;
            so.flush()?;
        }
    }
    if let Some(report_path) = &args.report {
        let report = Report {
            schema_version: REPORT_SCHEMA_VERSION,
            path: Some(&name),
            clean: out.is_clean(),
            conflicts: &out.conflicts,
            resolutions: &out.resolutions,
        };
        let json = serde_json::to_string_pretty(&report)? + "\n";
        std::fs::write(report_path, json)
            .with_context(|| format!("cannot write {}", report_path.display()))?;
    }
    if !args.quiet {
        for r in &out.resolutions {
            eprintln!(
                "scenemerge: {name}: renumbered {} {} ({} → {})",
                r.kind.replace('_', " "),
                r.target,
                r.from,
                r.to
            );
        }
        for c in &out.conflicts {
            let what = match &c.property {
                Some(p) => format!("{} {p}", c.target),
                None => c.target.clone(),
            };
            let at = c.line.map(|l| format!(" at line {l}")).unwrap_or_default();
            let resolved = match c.resolution {
                ConflictStyle::Markers => String::new(),
                ConflictStyle::Ours => " (kept ours)".into(),
                ConflictStyle::Theirs => " (took theirs)".into(),
            };
            let kind = serde_json::to_value(&c.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            eprintln!("CONFLICT ({kind}): {name}: {what}{at}{resolved}");
        }
        if !out.conflicts.is_empty() {
            eprintln!(
                "scenemerge: {name}: {} conflict(s){}",
                out.conflicts.len(),
                if opts.conflict == ConflictStyle::Markers {
                    "; fix the marked lines, then open the scene in Godot"
                } else {
                    ""
                }
            );
        }
    }
    let code = u8::from(!out.is_clean());
    std::mem::forget((base, ours, theirs, out));
    Ok(code)
}

fn text_merge(args: &MergeArgs) -> Result<u8> {
    let out = Command::new("git")
        .arg("merge-file")
        .arg("-p")
        .arg(format!("--marker-size={}", args.marker_size.max(1)))
        .args([
            "-L",
            &args.label_ours,
            "-L",
            &args.label_base,
            "-L",
            &args.label_theirs,
        ])
        .arg(&args.ours)
        .arg(&args.base)
        .arg(&args.theirs)
        .output()
        .context("cannot run `git merge-file` for the line-based fallback")?;
    let code = out.status.code().unwrap_or(-1);
    if !(0..=127).contains(&code) {
        anyhow::bail!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    match &args.output {
        Some(p) => write_atomic(p, &out.stdout)?,
        None => {
            use std::io::Write;
            std::io::stdout().write_all(&out.stdout)?;
        }
    }
    Ok(u8::from(code > 0))
}
