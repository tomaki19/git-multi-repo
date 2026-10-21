// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Integration test fixture: a temporary directory with a `work` directory
//! (where `git-multi-repo` runs) and bare remotes outside of it, so tests
//! need no network.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use git_multi_repo::config::Config;
use tempfile::TempDir;

/// Runs git with a fixed identity and returns trimmed stdout; panics on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs")
        .status
        .success()
}

pub struct Fixture {
    _tmp: TempDir,
    pub base: PathBuf,
    pub work: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let work = base.join("work");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(base.join("remotes")).unwrap();
        Fixture {
            _tmp: tmp,
            base,
            work,
        }
    }

    pub fn remote_path(&self, name: &str) -> PathBuf {
        self.base.join("remotes").join(format!("{name}.git"))
    }

    /// Creates a bare remote with one commit on `main`.
    pub fn remote(&self, name: &str) -> PathBuf {
        let seed = self.base.join("seeds").join(name);
        fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q", "-b", "main"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let remote = self.remote_path(name);
        git(
            &self.base,
            &[
                "clone",
                "-q",
                "--bare",
                seed.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        );
        remote
    }

    /// Creates a remote and clones it into `work/<name>`.
    pub fn repo(&self, name: &str) -> PathBuf {
        let remote = self.remote(name);
        git(&self.work, &["clone", "-q", remote.to_str().unwrap(), name]);
        self.work.join(name)
    }

    /// `git-multi-repo <args…>` running in `work`.
    pub fn cmd(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_git-multi-repo"));
        command.current_dir(&self.work).args(args);
        command
    }

    pub fn write(&self, file: &str, contents: &str) {
        fs::write(self.work.join(file), contents).unwrap();
    }

    pub fn read(&self, file: &str) -> String {
        fs::read_to_string(self.work.join(file)).unwrap()
    }

    pub fn config(&self) -> Option<Config> {
        Config::load(&self.work).unwrap()
    }
}
