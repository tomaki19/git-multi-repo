// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

mod common;

use std::path::Path;

use assert_cmd::prelude::*;
use common::{Fixture, git};
use git_multi_repo::config::{Config, Entry};
use predicates::prelude::*;

fn entry(url: &str, branch: &str, commit: Option<String>) -> Entry {
    Entry {
        url: url.into(),
        branch: Some(branch.into()),
        commit,
    }
}

/// The `origin` url as git stored it, which can differ from the path passed
/// to `git clone` on Windows (separators, short names).
fn origin(repo: &Path) -> String {
    git(repo, &["remote", "get-url", "origin"])
}

fn names(config: &Config) -> Vec<&str> {
    config
        .entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect()
}

#[test]
fn persist_writes_url_and_branch_of_every_repository() {
    let fx = Fixture::new();
    let b = fx.repo("b");
    let a = fx.repo("a");
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![
            ("a".to_owned(), entry(&origin(&a), "main", None)),
            ("b".to_owned(), entry(&origin(&b), "main", None)),
        ]
    );
}

#[test]
fn persist_regex_and_omit_select_repositories() {
    let fx = Fixture::new();
    for name in ["app", "lib-x", "lib-y"] {
        fx.repo(name);
    }
    fx.cmd(&["--omit", "lib-y", "--persist", "lib-.*"])
        .assert()
        .success();
    assert_eq!(names(&fx.config().unwrap()), ["lib-x"]);
}

#[test]
fn persist_overwrites_the_file_and_drops_pins() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    fx.write(
        ".git-multi-repo",
        "[old]\nurl = \"u\"\n\n[a]\nurl = \"u\"\ncommit = \"abc\"\n",
    );
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![("a".to_owned(), entry(&origin(&a), "main", None))]
    );
}

#[test]
fn persist_keeps_the_branch_from_the_file_for_a_detached_head() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    git(&a, &["checkout", "-q", "--detach"]);
    fx.write(
        ".git-multi-repo",
        "[a]\nurl = \"u\"\nbranch = \"develop\"\n",
    );
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(
        fx.config().unwrap().get("a").unwrap().branch.as_deref(),
        Some("develop")
    );
}

#[test]
fn pin_records_the_head_commit() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    fx.cmd(&["--pin"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![("a".to_owned(), entry(&origin(&a), "main", Some(head)))]
    );
}

#[test]
fn an_empty_match_leaves_the_file_untouched() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.write(".git-multi-repo", "[x]\nurl = \"u\"\n");
    fx.cmd(&["--persist", "nomatch"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no repositories matched"));
    assert_eq!(fx.read(".git-multi-repo"), "[x]\nurl = \"u\"\n");
}

#[test]
fn remove_deletes_matching_entries() {
    let fx = Fixture::new();
    fx.write(
        ".git-multi-repo",
        "[a]\nurl = \"u\"\n[b]\nurl = \"u\"\n[c]\nurl = \"u\"\n",
    );
    fx.cmd(&["--remove", "a|b"]).assert().success();
    assert_eq!(names(&fx.config().unwrap()), ["c"]);
}

#[test]
fn remove_without_regex_deletes_the_file() {
    let fx = Fixture::new();
    fx.write(".git-multi-repo", "[a]\nurl = \"u\"\n");
    fx.cmd(&["--remove"]).assert().success();
    assert!(fx.config().is_none());
}

#[test]
fn remove_and_unpin_need_a_file() {
    let fx = Fixture::new();
    fx.cmd(&["--remove"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no .git-multi-repo file"));
    fx.cmd(&["--unpin"]).assert().code(2);
}

#[test]
fn unpin_removes_commits_of_matching_entries() {
    let fx = Fixture::new();
    fx.write(
        ".git-multi-repo",
        "[a]\nurl = \"u\"\ncommit = \"c1\"\n\n[b]\nurl = \"u\"\ncommit = \"c2\"\n",
    );
    fx.cmd(&["--unpin", "a"]).assert().success();
    let config = fx.config().unwrap();
    assert_eq!(config.get("a").unwrap().commit, None);
    assert_eq!(config.get("b").unwrap().commit.as_deref(), Some("c2"));
}
