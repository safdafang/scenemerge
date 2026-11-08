#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Absolute path of the built `scenemerge` binary, as Git should invoke it.
pub fn exe() -> String {
    format!(
        "\"{}\"",
        env!("CARGO_BIN_EXE_scenemerge").replace('\\', "/")
    )
}

/// A throw-away repository with an isolated Git configuration.
pub struct Repo {
    pub dir: tempfile::TempDir,
    pub home: tempfile::TempDir,
}

impl Repo {
    pub fn new() -> Repo {
        let repo = Repo {
            dir: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "core.autocrlf", "false"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.current_dir(self.dir.path())
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path().join(".config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_EXTERNAL_DIFF")
            .env("GIT_PAGER", "cat")
            .env("NO_COLOR", "1");
        c
    }

    pub fn run(&self, program: &str, args: &[&str]) -> Output {
        self.command(program).args(args).output().unwrap()
    }

    pub fn run_env(&self, program: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut c = self.command(program);
        for (k, v) in env {
            c.env(k, v);
        }
        c.args(args).output().unwrap()
    }

    /// Runs git and asserts success; returns stdout.
    pub fn git(&self, args: &[&str]) -> String {
        let out = self.run("git", args);
        assert!(
            out.status.success(),
            "git {args:?} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Runs scenemerge in the repository.
    pub fn scenemerge(&self, args: &[&str]) -> Output {
        self.run(env!("CARGO_BIN_EXE_scenemerge"), args)
    }

    pub fn write(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, text).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }

    pub fn commit_all(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }
}

pub fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}
