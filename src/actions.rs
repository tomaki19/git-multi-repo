// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! One function per command-line action. Each returns `Ok(true)` when
//! everything succeeded, `Ok(false)` when at least one repository failed,
//! and `Err` for usage or configuration errors (which abort before anything
//! runs).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::cli::{Action, Cli};
use crate::config::Config;
use crate::runner::{self, Job, Step};
use crate::select::{self, Repo};

pub fn run(cli: &Cli, root: &Path) -> Result<bool> {
    let omit = cli.omit.as_deref().map(select::full_match).transpose()?;
    let omit = omit.as_ref();
    let parallelism = cli.parallelism();
    match cli.action() {
        Action::Git(args) => git_command(root, &args, omit, parallelism),
        Action::Clone(args) => clone(root, &args, omit, parallelism),
        Action::Tasks(_)
        | Action::Persist(_)
        | Action::Pin(_)
        | Action::Remove(_)
        | Action::Unpin(_) => bail!("this action is not available yet"),
    }
}

/// Runs the jobs, prints the summary and reports whether all succeeded.
fn execute(jobs: Vec<Job>, parallelism: Option<usize>, show_steps: bool) -> bool {
    let reports = runner::run(jobs, parallelism);
    print!("{}", runner::summarize(&reports, show_steps));
    !reports.iter().any(runner::Report::failed)
}

fn git_command(
    root: &Path,
    args: &[String],
    omit: Option<&Regex>,
    parallelism: Option<usize>,
) -> Result<bool> {
    let config = Config::load(root)?;
    let repos = select::select(root, config.as_ref(), omit, true)?;
    let jobs = repos
        .into_iter()
        .map(|repo| Job {
            steps: vec![Step {
                label: "git".into(),
                argv: std::iter::once("git".to_owned()).chain(args.iter().cloned()).collect(),
                cwd: root.join(&repo.name),
                depends_on: None,
            }],
            name: repo.name,
            skip: None,
        })
        .collect();
    Ok(execute(jobs, parallelism, false))
}

/// With a file, clones every selected entry (and checks out pinned commits);
/// without one, behaves exactly like a single `git clone`.
fn clone(
    root: &Path,
    args: &[String],
    omit: Option<&Regex>,
    parallelism: Option<usize>,
) -> Result<bool> {
    let Some(config) = Config::load(root)? else {
        let status = Command::new("git")
            .arg("clone")
            .args(args)
            .current_dir(root)
            .status()
            .context("cannot run git")?;
        return Ok(status.success());
    };
    let repos = select::select(root, Some(&config), omit, false)?;
    let jobs = repos.into_iter().map(|repo| clone_job(root, repo, args)).collect();
    Ok(execute(jobs, parallelism, false))
}

fn clone_job(root: &Path, repo: Repo, args: &[String]) -> Job {
    if root.join(&repo.name).exists() {
        return Job {
            name: repo.name,
            steps: Vec::new(),
            skip: Some("directory already exists".into()),
        };
    }
    let entry = repo.entry.unwrap_or_default();
    let mut argv = vec!["git".to_owned(), "clone".to_owned()];
    if let Some(branch) = &entry.branch {
        argv.extend(["--branch".to_owned(), branch.clone()]);
    }
    argv.extend(args.iter().cloned());
    argv.extend([entry.url.clone(), repo.name.clone()]);
    let mut steps = vec![Step {
        label: "clone".into(),
        argv,
        cwd: root.to_path_buf(),
        depends_on: None,
    }];
    if let Some(commit) = &entry.commit {
        steps.push(Step {
            label: "checkout".into(),
            argv: ["git", "-C", repo.name.as_str(), "checkout", "--detach", commit.as_str()]
                .map(String::from)
                .to_vec(),
            cwd: root.to_path_buf(),
            depends_on: Some(0),
        });
    }
    Job { name: repo.name, steps, skip: None }
}
