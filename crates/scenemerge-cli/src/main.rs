//! `scenemerge` — semantic diff and three-way merge for Godot 4 scenes.

mod check;
mod diff_cmd;
mod git;
mod io;
mod merge_cmd;

use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

/// Semantic diff and three-way merge for Godot 4 scenes and resources (.tscn/.tres).
#[derive(Debug, Parser)]
#[command(name = "scenemerge", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// When to use colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorWhen {
    /// Colour when writing to a terminal (respects `NO_COLOR`).
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

/// Output format of `diff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DiffFormat {
    /// Coloured tree of semantic changes.
    Human,
    /// Machine-readable JSON (see docs/json-schema.md).
    Json,
    /// Unified diff of the canonical text form (what `git diff` shows with textconv).
    Text,
}

/// Options shared by commands that compare files.
#[derive(Debug, Clone, clap::Args)]
pub struct CompareArgs {
    /// Ignore `metadata/_edit_*` properties (editor-only state).
    #[arg(long)]
    pub ignore_editor_metadata: bool,
    /// Treat floats that differ by at most this much as equal.
    #[arg(long, value_name = "EPSILON", default_value_t = 0.0)]
    pub epsilon: f64,
    /// Minimum similarity (0..1) for a removed + added node pair to count as a rename/move.
    #[arg(long, value_name = "SCORE", default_value_t = 0.65)]
    pub rename_threshold: f64,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the semantic differences between two files.
    Diff(diff_cmd::DiffArgs),
    /// Three-way merge BASE, OURS and THEIRS into OUT.
    Merge(merge_cmd::MergeArgs),
    /// Print the canonical text form of a file (for Git's `textconv`).
    Textconv {
        /// The .tscn/.tres file.
        file: std::path::PathBuf,
    },
    /// Parse and validate files, and check that they round-trip losslessly.
    Check(check::CheckArgs),
    /// Register scenemerge as Git diff and merge driver for *.tscn and *.tres.
    Install(git::InstallArgs),
    /// Remove what `install` added.
    Uninstall(git::UninstallArgs),
    /// Entry point for GIT_EXTERNAL_DIFF / diff.<driver>.command
    /// (path old-file old-hex old-mode new-file new-hex new-mode).
    #[command(name = "git-diff", hide = true)]
    GitDiff(diff_cmd::GitDiffArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Diff(args) => diff_cmd::run(&args),
        Command::Merge(args) => merge_cmd::run(&args),
        Command::Textconv { file } => diff_cmd::textconv(&file),
        Command::Check(args) => check::run(&args),
        Command::Install(args) => git::install(&args),
        Command::Uninstall(args) => git::uninstall(&args),
        Command::GitDiff(args) => diff_cmd::git_diff(&args),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("scenemerge: error: {e:#}");
            ExitCode::from(2)
        }
    }
}
