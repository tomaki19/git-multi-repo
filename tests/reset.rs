// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

mod common;

use std::fs;

use assert_cmd::prelude::*;
use common::{Fixture, git};
use predicates::prelude::*;

fn dirty(repo: &std::path::Path) {
    fs::write(repo.join("file"), "x").unwrap();
    git(repo, &["add", "file"]);
    git(repo, &["commit", "-q", "-m", "local"]);
}

#[test]
fn resets_to_the_pinned_commit() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let pinned = git(&a, &["rev-parse", "HEAD"]);
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = '{}'\nbranch = \"main\"\ncommit = \"{pinned}\"\n",
            fx.remote_path("a").display()
        ),
    );
    dirty(&a);
    fx.cmd(&["--reset"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a: ok"));
    assert_eq!(git(&a, &["rev-parse", "HEAD"]), pinned);
}

#[test]
fn resets_to_the_branch_head_without_a_commit() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    git(&a, &["branch", "other"]);
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = '{}'\nbranch = \"other\"\n",
            fx.remote_path("a").display()
        ),
    );
    dirty(&a);
    fx.cmd(&["--reset"]).assert().success();
    assert_eq!(git(&a, &["rev-parse", "HEAD"]), head);
}

#[test]
fn without_a_file_reset_is_an_error() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.cmd(&["--reset"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no .git-multi-repo file"));
}

#[test]
fn reset_takes_no_extra_arguments() {
    let fx = Fixture::new();
    fx.cmd(&["--reset", "--soft", "HEAD~1"]).assert().code(2);
}
