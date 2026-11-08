//! `check` command: parse, validate and verify lossless round-trip.

use std::path::PathBuf;

use anyhow::Result;
use godot_text::Document;
use scene_model::SceneFile;

/// `scenemerge check`
#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// Files to check (directories are searched recursively for .tscn/.tres).
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// Only print problems.
    #[arg(short, long)]
    pub quiet: bool,
}

fn collect(path: &std::path::Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(rd) = std::fs::read_dir(path) else {
            out.push(path.to_path_buf());
            return;
        };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let skip = p
                .file_name()
                .is_some_and(|n| n == ".git" || n == ".godot" || n == ".import");
            if p.is_dir() && !skip {
                collect(&p, out);
            } else if p
                .extension()
                .is_some_and(|e| e == "tscn" || e == "tres" || e == "escn")
            {
                out.push(p);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
}

/// Exit codes: 0 all files fine, 1 at least one file has problems.
pub fn run(args: &CheckArgs) -> Result<u8> {
    let mut files = Vec::new();
    for f in &args.files {
        collect(f, &mut files);
    }
    let mut bad = 0usize;
    for path in &files {
        let shown = path.display().to_string().replace('\\', "/");
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{shown}: cannot read: {e}");
                bad += 1;
                continue;
            }
        };
        let doc = match Document::parse_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}", e.with_path(&shown));
                bad += 1;
                continue;
            }
        };
        let mut problems: Vec<String> = Vec::new();
        if doc.to_text().as_bytes() != bytes.as_slice() {
            problems.push(
                "internal error: the file does not round-trip losslessly (please report this)"
                    .into(),
            );
        }
        match SceneFile::from_document(doc) {
            Ok(model) => {
                for issue in model.validate() {
                    problems.push(format!("{}: {}", issue.line, issue.message));
                }
            }
            Err(e) => problems.push(e.to_string()),
        }
        if problems.is_empty() {
            if !args.quiet {
                println!("ok    {shown}");
            }
        } else {
            bad += 1;
            for p in problems {
                // "12: message" → "file:12: message"; other messages get a space.
                let sep = if p.starts_with(|c: char| c.is_ascii_digit()) {
                    ":"
                } else {
                    ": "
                };
                eprintln!("{shown}{sep}{p}");
            }
        }
    }
    if !args.quiet || bad > 0 {
        let total = files.len();
        eprintln!("checked {total} file(s), {bad} with problems");
    }
    Ok(u8::from(bad > 0))
}
