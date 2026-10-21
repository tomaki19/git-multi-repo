// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Reads repository state by asking the `git` executable, so the answers
//! match exactly what the user's own git would report.

use std::path::Path;
use std::process::{Command, Stdio};

/// Runs `git -C <dir> <args…>` and returns trimmed stdout, or `None` if git
/// fails or prints nothing.
fn query(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

pub fn remote_url(dir: &Path) -> Option<String> {
    query(dir, &["remote", "get-url", "origin"]).or_else(|| {
        let first = query(dir, &["remote"])?.lines().next()?.to_owned();
        query(dir, &["remote", "get-url", first.as_str()])
    })
}

pub fn current_branch(dir: &Path) -> Option<String> {
    query(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
}

pub fn head_commit(dir: &Path) -> Option<String> {
    query(dir, &["rev-parse", "--verify", "-q", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn repository() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        dir
    }

    #[test]
    fn reads_branch_and_commit() {
        let dir = repository();
        assert_eq!(head_commit(dir.path()), None);
        git(
            dir.path(),
            &["commit", "-q", "--allow-empty", "-m", "first"],
        );
        assert_eq!(head_commit(dir.path()).unwrap().len(), 40);
        assert_eq!(current_branch(dir.path()).as_deref(), Some("main"));
    }

    #[test]
    fn detached_head_has_no_branch() {
        let dir = repository();
        git(
            dir.path(),
            &["commit", "-q", "--allow-empty", "-m", "first"],
        );
        git(dir.path(), &["checkout", "-q", "--detach"]);
        assert_eq!(current_branch(dir.path()), None);
    }

    #[test]
    fn prefers_origin_and_falls_back_to_the_first_remote() {
        let dir = repository();
        assert_eq!(remote_url(dir.path()), None);
        git(
            dir.path(),
            &["remote", "add", "upstream", "https://example.com/up.git"],
        );
        assert_eq!(
            remote_url(dir.path()).as_deref(),
            Some("https://example.com/up.git")
        );
        git(
            dir.path(),
            &["remote", "add", "origin", "https://example.com/origin.git"],
        );
        assert_eq!(
            remote_url(dir.path()).as_deref(),
            Some("https://example.com/origin.git")
        );
    }
}
