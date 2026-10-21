// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

mod common;

use assert_cmd::prelude::*;
use common::{Fixture, git_ok};
use predicates::prelude::*;

fn has_tag(dir: &std::path::Path) -> bool {
    git_ok(dir, &["rev-parse", "-q", "--verify", "refs/tags/v1"])
}

#[test]
fn runs_in_every_git_subdirectory() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    std::fs::create_dir(fx.work.join("not-a-repo")).unwrap();
    fx.cmd(&["tag", "v1"]).assert().success().stdout(
        predicate::str::contains("── a ──")
            .and(predicate::str::contains("── b ──"))
            .and(predicate::str::contains("a: ok\nb: ok\n")),
    );
    assert!(has_tag(&a));
    assert!(has_tag(&b));
}

#[test]
fn forwards_hyphenated_arguments() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.cmd(&["log", "--oneline", "-n", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("first"));
}

#[test]
fn omit_skips_fully_matching_repositories() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let ab = fx.repo("ab");
    fx.cmd(&["--omit", "a", "tag", "v1"]).assert().success();
    assert!(!has_tag(&a));
    assert!(has_tag(&ab));
}

#[test]
fn a_file_limits_the_repositories_and_missing_ones_are_skipped() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    fx.write(".git-multi-repo", "[a]\nurl = \"u\"\n\n[ghost]\nurl = \"u\"\n");
    fx.cmd(&["tag", "v1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("skipping 'ghost'"));
    assert!(has_tag(&a));
    assert!(!has_tag(&b));
}

#[test]
fn a_failing_repository_gives_exit_code_1_and_the_run_continues() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    common::git(&b, &["tag", "v1"]);
    fx.cmd(&["tag", "v1"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("a: ok\nb: failed (exit"));
    assert!(has_tag(&a));
}

#[test]
fn an_invalid_file_aborts_with_exit_code_2() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    fx.write(".git-multi-repo", "[a]\nbranch = \"main\"\n");
    fx.cmd(&["tag", "v1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("missing or empty 'url'"));
    assert!(!has_tag(&a));
}

#[test]
fn an_invalid_omit_regex_is_a_usage_error() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.cmd(&["--omit", "(", "status"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid regex"));
}

#[test]
fn parallel_runs_in_every_repository() {
    let fx = Fixture::new();
    let repos: Vec<_> = ["a", "b", "c"].iter().map(|name| fx.repo(name)).collect();
    fx.cmd(&["--parallel", "--jobs", "2", "tag", "v1"]).assert().success();
    assert!(repos.iter().all(|dir| has_tag(dir)));
}

#[test]
fn conflicting_actions_are_usage_errors() {
    let fx = Fixture::new();
    fx.cmd(&["--persist", "--pin"]).assert().code(2);
    fx.cmd(&[]).assert().code(2);
}
