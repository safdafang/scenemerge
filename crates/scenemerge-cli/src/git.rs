//! `install` / `uninstall`: register scenemerge as Git diff and merge driver.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Name of the diff/merge driver in `.gitattributes` and git config.
pub const DRIVER: &str = "godot-scene";
const MARKER_COMMENT: &str = "# scenemerge: semantic diff and merge for Godot scenes";
const PATTERNS: &[&str] = &["*.tscn", "*.tres"];

/// How `git diff` should show scene changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DiffMode {
    /// Line diff of the canonical text form (`diff.<driver>.textconv`); works
    /// with every Git tool, `git log -p`, blame, etc.
    Textconv,
    /// Semantic change tree (`diff.<driver>.command`).
    Semantic,
}

/// `scenemerge install`
#[derive(Debug, clap::Args)]
pub struct InstallArgs {
    /// Configure the user's global Git config and global attributes file
    /// instead of the current repository.
    #[arg(long)]
    pub global: bool,
    /// What `git diff` shows for scenes.
    #[arg(long, value_enum, default_value_t = DiffMode::Textconv)]
    pub diff: DiffMode,
    /// Command Git should run (default: `scenemerge` if it is on PATH,
    /// otherwise the absolute path of this executable).
    #[arg(long, value_name = "CMD")]
    pub command: Option<String>,
}

/// `scenemerge uninstall`
#[derive(Debug, clap::Args)]
pub struct UninstallArgs {
    /// Undo a `--global` installation.
    #[arg(long)]
    pub global: bool,
}

fn git(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .context("cannot run git (is it installed and on PATH?)")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git config` getter that returns `None` for unset keys.
fn git_config_get(scope: &str, key: &str) -> Option<String> {
    Command::new("git")
        .args(["config", scope, "--get", key])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn expand_tilde(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home_dir().map_or_else(|| PathBuf::from(p), |h| h.join(rest)),
        None => PathBuf::from(p),
    }
}

/// The attributes file to edit: `<repo>/.gitattributes`, or the global
/// attributes file (`core.attributesFile`, default `$XDG_CONFIG_HOME/git/attributes`
/// or `~/.config/git/attributes`).
fn attributes_file(global: bool) -> Result<PathBuf> {
    if !global {
        let root = git(&["rev-parse", "--show-toplevel"])
            .context("not inside a Git repository (use --global to configure all repositories)")?;
        return Ok(PathBuf::from(root).join(".gitattributes"));
    }
    if let Some(p) = git_config_get("--global", "core.attributesFile").filter(|p| !p.is_empty()) {
        return Ok(expand_tilde(&p));
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return Ok(PathBuf::from(xdg).join("git").join("attributes"));
    }
    home_dir()
        .map(|h| h.join(".config").join("git").join("attributes"))
        .context("cannot determine the home directory for the global attributes file")
}

fn on_path(exe: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{exe}.exe"), exe.to_string()]
    } else {
        vec![exe.to_string()]
    };
    std::env::split_paths(&paths).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// The command Git should run for scenemerge.
fn self_command(explicit: Option<&str>) -> String {
    if let Some(c) = explicit {
        return c.to_string();
    }
    if on_path("scenemerge") {
        return "scenemerge".into();
    }
    match std::env::current_exe() {
        Ok(p) => {
            // Git runs drivers through a POSIX shell, also on Windows.
            let s = p.display().to_string().replace('\\', "/");
            format!("\"{s}\"")
        }
        Err(_) => "scenemerge".into(),
    }
}

fn git_version() -> Option<(u32, u32)> {
    let v = git(&["--version"]).ok()?;
    let num = v.split_whitespace().nth(2)?;
    let mut it = num.split('.');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

fn attribute_lines() -> Vec<String> {
    PATTERNS
        .iter()
        .map(|p| format!("{p} diff={DRIVER} merge={DRIVER}"))
        .collect()
}

/// Adds the attribute lines that are missing. Returns whether the file changed.
fn add_attributes(path: &Path) -> Result<bool> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let present: Vec<&str> = existing.lines().map(str::trim).collect();
    let missing: Vec<String> = attribute_lines()
        .into_iter()
        .filter(|l| !present.contains(&l.as_str()))
        .collect();
    if missing.is_empty() {
        return Ok(false);
    }
    let mut text = existing.clone();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(eol);
    }
    if !present.contains(&MARKER_COMMENT) {
        if !text.is_empty() {
            text.push_str(eol);
        }
        text.push_str(MARKER_COMMENT);
        text.push_str(eol);
    }
    for l in missing {
        text.push_str(&l);
        text.push_str(eol);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(true)
}

/// Removes our attribute lines and comment. Returns whether the file changed.
fn remove_attributes(path: &Path) -> Result<bool> {
    let Ok(existing) = std::fs::read_to_string(path) else {
        return Ok(false);
    };
    let ours = attribute_lines();
    let eol = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let kept: Vec<&str> = existing
        .lines()
        .filter(|l| {
            let t = l.trim();
            t != MARKER_COMMENT && !ours.iter().any(|o| o == t)
        })
        .collect();
    if kept.len() == existing.lines().count() {
        return Ok(false);
    }
    // Drop trailing blank lines left behind by our block.
    let mut kept = kept;
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut text = kept.join(eol);
    if !text.is_empty() {
        text.push_str(eol);
    }
    std::fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(true)
}

fn set_config(scope: &str, key: &str, value: &str) -> Result<bool> {
    if git_config_get(scope, key).as_deref() == Some(value) {
        return Ok(false);
    }
    git(&["config", scope, key, value])?;
    Ok(true)
}

fn unset_config(scope: &str, key: &str) -> bool {
    Command::new("git")
        .args(["config", scope, "--unset-all", key])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn install(args: &InstallArgs) -> Result<u8> {
    let scope = if args.global { "--global" } else { "--local" };
    let attrs = attributes_file(args.global)?;
    let cmd = self_command(args.command.as_deref());
    let mut driver = format!("{cmd} merge %O %A %B -o %A --path %P --marker-size %L");
    if git_version().is_some_and(|v| v >= (2, 44)) {
        // Conflict labels (branch names) are available since Git 2.44.
        driver.push_str(" --label-base %S --label-ours %X --label-theirs %Y");
    }
    let mut changed = Vec::new();
    if add_attributes(&attrs)? {
        changed.push(format!("updated {}", attrs.display()));
    }
    let diff_key = format!("diff.{DRIVER}.textconv");
    let cmd_key = format!("diff.{DRIVER}.command");
    match args.diff {
        DiffMode::Textconv => {
            if set_config(scope, &diff_key, &format!("{cmd} textconv"))? {
                changed.push(format!("set {diff_key}"));
            }
            if unset_config(scope, &cmd_key) {
                changed.push(format!("unset {cmd_key}"));
            }
        }
        DiffMode::Semantic => {
            if set_config(scope, &cmd_key, &format!("{cmd} git-diff"))? {
                changed.push(format!("set {cmd_key}"));
            }
            if unset_config(scope, &diff_key) {
                changed.push(format!("unset {diff_key}"));
            }
        }
    }
    for (key, value) in [
        (
            format!("merge.{DRIVER}.name"),
            "Godot semantic scene merge".to_string(),
        ),
        (format!("merge.{DRIVER}.driver"), driver),
    ] {
        if set_config(scope, &key, &value)? {
            changed.push(format!("set {key}"));
        }
    }
    let where_ = if args.global {
        "globally"
    } else {
        "for this repository"
    };
    if changed.is_empty() {
        println!("scenemerge is already installed {where_}; nothing to do.");
    } else {
        println!("Installed scenemerge {where_}:");
        for c in changed {
            println!("  {c}");
        }
    }
    Ok(0)
}

pub fn uninstall(args: &UninstallArgs) -> Result<u8> {
    let scope = if args.global { "--global" } else { "--local" };
    let attrs = attributes_file(args.global)?;
    let mut changed = Vec::new();
    if remove_attributes(&attrs)? {
        changed.push(format!("updated {}", attrs.display()));
    }
    for section in [format!("diff.{DRIVER}"), format!("merge.{DRIVER}")] {
        let ok = Command::new("git")
            .args(["config", scope, "--remove-section", &section])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            changed.push(format!("removed [{section}]"));
        }
    }
    if changed.is_empty() {
        println!("scenemerge was not installed; nothing to do.");
    } else {
        println!("Uninstalled scenemerge:");
        for c in changed {
            println!("  {c}");
        }
    }
    Ok(0)
}
