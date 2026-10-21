// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

mod common;

use std::fs;

use assert_cmd::prelude::*;
use common::{Fixture, git, git_ok};
use predicates::prelude::*;

#[test]
fn clones_all_entries_and_checks_out_pinned_commits() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    let pinned = git(&a, &["rev-parse", "HEAD"]);
    git(&a, &["commit", "-q", "--allow-empty", "-m", "second"]);
    git(&a, &["push", "-q", "origin", "main"]);
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = \"{}\"\nbranch = \"main\"\ncommit = \"{pinned}\"\n\n[b]\nurl = \"{}\"\nbranch = \"main\"\n",
            fx.remote_path("a").display(),
            fx.remote_path("b").display()
        ),
    );
    fs::remove_dir_all(&a).unwrap();
    fs::remove_dir_all(&b).unwrap();

    fx.cmd(&["clone"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a: ok\nb: ok\n"));

    assert_eq!(git(&a, &["rev-parse", "HEAD"]), pinned);
    assert!(
        !git_ok(&a, &["symbolic-ref", "-q", "HEAD"]),
        "a must be on a detached HEAD"
    );
    assert_eq!(git(&b, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[test]
fn existing_directories_are_skipped() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.write(
        ".git-multi-repo",
        &format!("[a]\nurl = \"{}\"\n", fx.remote_path("a").display()),
    );
    fx.cmd(&["clone"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "a: skipped (directory already exists)",
        ));
}

#[test]
fn omit_applies_to_clone() {
    let fx = Fixture::new();
    fx.remote("a");
    fx.remote("b");
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = \"{}\"\n\n[b]\nurl = \"{}\"\n",
            fx.remote_path("a").display(),
            fx.remote_path("b").display()
        ),
    );
    fx.cmd(&["--omit", "b", "clone"]).assert().success();
    assert!(fx.work.join("a").join(".git").is_dir());
    assert!(!fx.work.join("b").exists());
}

#[test]
fn a_failing_clone_gives_exit_code_1() {
    let fx = Fixture::new();
    fx.write(".git-multi-repo", "[a]\nurl = \"/does/not/exist.git\"\n");
    fx.cmd(&["clone"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("a: failed"));
}

#[test]
fn without_a_file_clone_is_forwarded_to_git() {
    let fx = Fixture::new();
    let remote = fx.remote("a");
    fx.cmd(&["clone", "-q", remote.to_str().unwrap(), "copy"])
        .assert()
        .success();
    assert!(fx.work.join("copy").join(".git").is_dir());
}

#[test]
fn rejects_a_repository_name_that_escapes_the_work_directory() {
    let fx = Fixture::new();
    fx.remote("x");
    fx.write(
        ".git-multi-repo",
        &format!(
            "[\"../escaped\"]\nurl = \"{}\"\n",
            fx.remote_path("x").display()
        ),
    );
    fx.cmd(&["clone"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "invalid repository name '../escaped'",
        ));
    assert!(!fx.base.join("escaped").exists());
}

#[test]
fn a_url_starting_with_a_dash_is_not_treated_as_an_option() {
    let fx = Fixture::new();
    let marker = fx.base.join("PWNED");
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = \"--upload-pack=touch {};git-upload-pack\"\n",
            marker.display()
        ),
    );
    fx.cmd(&["clone"]).assert().code(1);
    assert!(!marker.exists());
    assert!(!fx.work.join("PWNED").exists());
}

#[test]
fn a_pinned_commit_starting_with_a_dash_is_not_treated_as_an_option() {
    let fx = Fixture::new();
    fx.remote("a");
    fx.write(
        ".git-multi-repo",
        &format!(
            "[a]\nurl = \"{}\"\ncommit = \"--help\"\n",
            fx.remote_path("a").display()
        ),
    );
    fx.cmd(&["clone"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("a: failed"));
}
