// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Task files: a JSON array of named shell commands, each optionally
//! depending on one other task, plus `%variable%` substitution.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Task {
    pub task: String,
    #[serde(default)]
    pub depends_on: Option<String>,
    pub command: String,
}

pub fn load(path: &Path) -> Result<Vec<Task>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read tasks file {}", path.display()))?;
    parse(&text).with_context(|| format!("invalid tasks file {}", path.display()))
}

/// Parses, validates and returns the tasks in the order they will run.
/// Everything is checked up front so a broken file never runs half-way.
pub fn parse(text: &str) -> Result<Vec<Task>> {
    let tasks: Vec<Task> = serde_json::from_str(text)?;
    if tasks.is_empty() {
        bail!("no tasks defined");
    }
    let mut names = HashSet::new();
    for task in &tasks {
        if !names.insert(task.task.as_str()) {
            bail!("duplicate task '{}'", task.task);
        }
    }
    for task in &tasks {
        if let Some(dependency) = &task.depends_on
            && !names.contains(dependency.as_str())
        {
            bail!(
                "task '{}' depends on unknown task '{dependency}'",
                task.task
            );
        }
    }
    order(tasks)
}

/// Topological order that stays as close to file order as dependencies
/// allow: repeatedly take the first task whose dependency is already placed.
/// If no task can be placed, the remaining ones form a cycle.
fn order(mut remaining: Vec<Task>) -> Result<Vec<Task>> {
    let mut placed = HashSet::new();
    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .position(|task| task.depends_on.as_ref().is_none_or(|d| placed.contains(d)));
        let Some(index) = ready else {
            let names: Vec<&str> = remaining.iter().map(|task| task.task.as_str()).collect();
            bail!("dependency cycle among tasks: {}", names.join(", "));
        };
        let task = remaining.remove(index);
        placed.insert(task.task.clone());
        ordered.push(task);
    }
    Ok(ordered)
}

/// Values for `%name%`, `%url%`, `%branch%` and `%commit%`.
#[derive(Debug, Clone, Default)]
pub struct Vars {
    pub name: String,
    pub url: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
}

/// Replaces known `%variable%` tokens in a single left-to-right pass, so
/// substituted values are never themselves expanded. Unknown tokens and
/// stray `%` signs are left untouched. Returns the referenced variables that
/// had no value (each listed once); they are replaced by an empty string.
pub fn substitute(command: &str, vars: &Vars) -> (String, Vec<&'static str>) {
    let mut out = String::with_capacity(command.len());
    let mut missing = Vec::new();
    let mut rest = command;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let token = after.find('%').and_then(|end| {
            let (label, value): (&'static str, Option<&str>) = match &after[..end] {
                "name" => ("name", Some(vars.name.as_str())),
                "url" => ("url", vars.url.as_deref()),
                "branch" => ("branch", vars.branch.as_deref()),
                "commit" => ("commit", vars.commit.as_deref()),
                _ => return None,
            };
            Some((end, label, value))
        });
        match token {
            Some((end, label, value)) => {
                if value.is_none() && !missing.contains(&label) {
                    missing.push(label);
                }
                out.push_str(value.unwrap_or(""));
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    (out, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_of(text: &str) -> Vec<String> {
        parse(text)
            .unwrap()
            .into_iter()
            .map(|task| task.task)
            .collect()
    }

    fn error(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    #[test]
    fn parses_tasks_with_optional_dependency() {
        let tasks =
            parse(r#"[{"task":"a","command":"x"},{"task":"b","dependsOn":"a","command":"y"}]"#)
                .unwrap();
        assert_eq!(
            tasks,
            vec![
                Task {
                    task: "a".into(),
                    depends_on: None,
                    command: "x".into()
                },
                Task {
                    task: "b".into(),
                    depends_on: Some("a".into()),
                    command: "y".into()
                },
            ]
        );
    }

    #[test]
    fn independent_tasks_keep_file_order() {
        assert_eq!(
            order_of(
                r#"[{"task":"x","command":""},{"task":"y","command":""},{"task":"z","command":""}]"#
            ),
            ["x", "y", "z"]
        );
    }

    #[test]
    fn dependencies_move_tasks_only_as_far_as_needed() {
        assert_eq!(
            order_of(
                r#"[{"task":"b","dependsOn":"a","command":""},{"task":"a","command":""},{"task":"c","command":""}]"#
            ),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn rejects_invalid_task_files() {
        assert!(error("[]").contains("no tasks defined"));
        assert!(error(r#"{"task":"a","command":"x"}"#).contains("invalid type"));
        assert!(error(r#"[{"task":"a","command":"x","extra":1}]"#).contains("unknown field"));
        assert!(error(r#"[{"task":"a"}]"#).contains("missing field `command`"));
        assert!(
            error(r#"[{"task":"a","command":"x"},{"task":"a","command":"y"}]"#)
                .contains("duplicate task 'a'")
        );
        assert!(
            error(r#"[{"task":"a","dependsOn":"nope","command":"x"}]"#)
                .contains("task 'a' depends on unknown task 'nope'")
        );
        assert!(
            error(r#"[{"task":"a","dependsOn":"b","command":""},{"task":"b","dependsOn":"a","command":""}]"#)
                .contains("dependency cycle among tasks: a, b")
        );
        assert!(
            error(r#"[{"task":"a","dependsOn":"a","command":""}]"#)
                .contains("dependency cycle among tasks: a")
        );
    }

    #[test]
    fn load_names_the_file_in_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        std::fs::write(&path, "[]").unwrap();
        let message = format!("{:#}", load(&path).unwrap_err());
        assert!(message.contains("invalid tasks file"), "{message}");
        assert!(message.contains("tasks.json"), "{message}");
    }

    fn vars() -> Vars {
        Vars {
            name: "lib".into(),
            url: Some("git@host:lib.git".into()),
            branch: Some("main".into()),
            commit: Some("abc123".into()),
        }
    }

    #[test]
    fn substitutes_all_variables() {
        let (command, missing) = substitute("echo %name% %url% %branch% %commit% %name%", &vars());
        assert_eq!(command, "echo lib git@host:lib.git main abc123 lib");
        assert!(missing.is_empty());
    }

    #[test]
    fn missing_values_become_empty_and_are_reported_once() {
        let vars = Vars {
            name: "lib".into(),
            ..Vars::default()
        };
        let (command, missing) = substitute("[%url%][%url%][%commit%]", &vars);
        assert_eq!(command, "[][][]");
        assert_eq!(missing, ["url", "commit"]);
    }

    #[test]
    fn unknown_tokens_and_lone_percent_signs_are_kept() {
        let (command, _) = substitute("50% of %foo%bar%name% %", &vars());
        assert_eq!(command, "50% of %foo%barlib %");
    }

    #[test]
    fn substituted_values_are_not_substituted_again() {
        let vars = Vars {
            name: "%url%".into(),
            ..vars()
        };
        let (command, _) = substitute("%name%", &vars);
        assert_eq!(command, "%url%");
    }
}
