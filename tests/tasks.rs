// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

#![cfg(unix)]

mod common;

use std::fs;

use assert_cmd::prelude::*;
use common::{Fixture, git};
use predicates::prelude::*;

#[test]
fn runs_tasks_in_dependency_order_with_variables() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    fx.write(
        "tasks.json",
        r#"[
            {"task": "second", "dependsOn": "first", "command": "echo second >> log.txt"},
            {"task": "first", "command": "echo %name% %branch% %url% > log.txt"}
        ]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a: ok\nb: ok\n"));
    for (dir, name) in [(&a, "a"), (&b, "b")] {
        assert_eq!(
            fs::read_to_string(dir.join("log.txt")).unwrap(),
            format!("{name} main {}\nsecond\n", fx.remote_path(name).display())
        );
    }
}

#[test]
fn file_values_take_precedence_over_live_values() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    fx.write(
        ".git-multi-repo",
        "[a]\nurl = \"u\"\ncommit = \"pinned123\"\n",
    );
    fx.write(
        "tasks.json",
        r#"[{"task": "t", "command": "echo %commit% %url% %branch% > out.txt"}]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"]).assert().success();
    assert_eq!(
        fs::read_to_string(a.join("out.txt")).unwrap(),
        "pinned123 u main\n"
    );
}

#[test]
fn commit_falls_back_to_head() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    fx.write(
        "tasks.json",
        r#"[{"task": "t", "command": "echo %commit% > out.txt"}]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"]).assert().success();
    assert_eq!(
        fs::read_to_string(a.join("out.txt")).unwrap(),
        format!("{head}\n")
    );
}

#[test]
fn a_failed_task_skips_only_its_dependents() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    fx.write(
        "tasks.json",
        r#"[
            {"task": "broken", "command": "exit 3"},
            {"task": "after", "dependsOn": "broken", "command": "touch after.txt"},
            {"task": "other", "command": "touch other.txt"}
        ]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "a: failed (exit 3) [failed: broken] [skipped: after]",
        ));
    assert!(!a.join("after.txt").exists());
    assert!(a.join("other.txt").exists());
}

#[test]
fn parallel_tasks_run_in_every_repository() {
    let fx = Fixture::new();
    let repos: Vec<_> = ["a", "b", "c"].iter().map(|name| fx.repo(name)).collect();
    fx.write(
        "tasks.json",
        r#"[{"task": "t", "command": "touch done.txt"}]"#,
    );
    fx.cmd(&["--parallel", "--tasks", "tasks.json"])
        .assert()
        .success();
    assert!(repos.iter().all(|dir| dir.join("done.txt").exists()));
}

#[test]
fn omit_applies_to_tasks() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    fx.write(
        "tasks.json",
        r#"[{"task": "t", "command": "touch done.txt"}]"#,
    );
    fx.cmd(&["--omit", "b", "--tasks", "tasks.json"])
        .assert()
        .success();
    assert!(a.join("done.txt").exists());
    assert!(!b.join("done.txt").exists());
}

#[test]
fn an_invalid_tasks_file_is_a_usage_error() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    fx.write(
        "tasks.json",
        r#"[
            {"task": "x", "dependsOn": "y", "command": "touch x.txt"},
            {"task": "y", "dependsOn": "x", "command": "touch y.txt"}
        ]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("dependency cycle"));
    assert!(!a.join("x.txt").exists());
}

#[test]
fn a_missing_variable_warns_and_becomes_empty() {
    let fx = Fixture::new();
    let solo = fx.work.join("solo");
    fs::create_dir(&solo).unwrap();
    git(&solo, &["init", "-q", "-b", "main"]);
    fx.write(
        "tasks.json",
        r#"[{"task": "t", "command": "echo [%url%] > out.txt"}]"#,
    );
    fx.cmd(&["--tasks", "tasks.json"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: solo: %url% has no value",
        ));
    assert_eq!(fs::read_to_string(solo.join("out.txt")).unwrap(), "[]\n");
}
