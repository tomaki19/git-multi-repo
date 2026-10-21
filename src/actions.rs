// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! One function per command-line action. Each returns `Ok(true)` when
//! everything succeeded, `Ok(false)` when at least one repository failed,
//! and `Err` for usage or configuration errors (which abort before anything
//! runs).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::cli::{Action, Cli};
use crate::config::{Config, Entry, FILE_NAME};
use crate::git;
use crate::runner::{self, Job, Step};
use crate::select::{self, Repo};
use crate::tasks::{self, Task, Vars};

pub fn run(cli: &Cli, root: &Path) -> Result<bool> {
    let omit = cli.omit.as_deref().map(select::full_match).transpose()?;
    let omit = omit.as_ref();
    let parallelism = cli.parallelism();
    match cli.action() {
        Action::Git(args) => git_command(root, &args, omit, parallelism),
        Action::Clone(args) => clone(root, &args, omit, parallelism),
        Action::Persist(filter) => write_config(root, filter.as_deref(), omit, false),
        Action::Pin(filter) => write_config(root, filter.as_deref(), omit, true),
        Action::Remove(filter) => remove(root, filter.as_deref()),
        Action::Unpin(filter) => unpin(root, filter.as_deref()),
        Action::Tasks(path) => run_tasks(root, &path, omit, parallelism),
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
                argv: std::iter::once("git".to_owned())
                    .chain(args.iter().cloned())
                    .collect(),
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
    let jobs = repos
        .into_iter()
        .map(|repo| clone_job(root, repo, args))
        .collect();
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
            argv: [
                "git",
                "-C",
                repo.name.as_str(),
                "checkout",
                "--detach",
                commit.as_str(),
            ]
            .map(String::from)
            .to_vec(),
            cwd: root.to_path_buf(),
            depends_on: Some(0),
        });
    }
    Job {
        name: repo.name,
        steps,
        skip: None,
    }
}

fn compile(filter: Option<&str>) -> Result<Option<Regex>> {
    filter.map(select::full_match).transpose()
}

fn matches(filter: Option<&Regex>, name: &str) -> bool {
    filter.is_none_or(|re| re.is_match(name))
}

/// `--persist` / `--pin`: rebuild the file from the live sub-directories.
/// The file is overwritten, so entries not matched this time disappear.
fn write_config(
    root: &Path,
    filter: Option<&str>,
    omit: Option<&Regex>,
    pin: bool,
) -> Result<bool> {
    let filter = compile(filter)?;
    let previous = Config::load(root)?;
    let mut config = Config::default();
    for name in select::scan(root)? {
        if !matches(filter.as_ref(), &name) || omit.is_some_and(|re| re.is_match(&name)) {
            continue;
        }
        let dir = root.join(&name);
        let Some(url) = git::remote_url(&dir) else {
            eprintln!("warning: skipping '{name}': no remote configured");
            continue;
        };
        let branch = git::current_branch(&dir).or_else(|| {
            previous
                .as_ref()
                .and_then(|c| c.get(&name))
                .and_then(|e| e.branch.clone())
        });
        let commit = if pin {
            let Some(commit) = git::head_commit(&dir) else {
                eprintln!("warning: skipping '{name}': no commit to pin");
                continue;
            };
            Some(commit)
        } else {
            None
        };
        println!("{} {name}", if pin { "pinning" } else { "persisting" });
        config.entries.push((
            name,
            Entry {
                url,
                branch,
                commit,
            },
        ));
    }
    if config.entries.is_empty() {
        bail!("no repositories matched; {FILE_NAME} left unchanged");
    }
    config.save(root)?;
    println!("wrote {} entries to {FILE_NAME}", config.entries.len());
    Ok(true)
}

fn load_existing(root: &Path) -> Result<Config> {
    match Config::load(root)? {
        Some(config) => Ok(config),
        None => bail!("no {FILE_NAME} file in {}", root.display()),
    }
}

/// `--remove`: drop matching entries; an empty result deletes the file.
fn remove(root: &Path, filter: Option<&str>) -> Result<bool> {
    let filter = compile(filter)?;
    let mut config = load_existing(root)?;
    config.entries.retain(|(name, _)| {
        let matched = matches(filter.as_ref(), name);
        if matched {
            println!("removing {name}");
        }
        !matched
    });
    if config.entries.is_empty() {
        Config::delete(root)?;
        println!("no entries left; deleted {FILE_NAME}");
    } else {
        config.save(root)?;
    }
    Ok(true)
}

/// `--unpin`: drop the commit of matching entries.
fn unpin(root: &Path, filter: Option<&str>) -> Result<bool> {
    let filter = compile(filter)?;
    let mut config = load_existing(root)?;
    for (name, entry) in &mut config.entries {
        if matches(filter.as_ref(), name) && entry.commit.take().is_some() {
            println!("unpinning {name}");
        }
    }
    config.save(root)?;
    Ok(true)
}

fn run_tasks(
    root: &Path,
    path: &Path,
    omit: Option<&Regex>,
    parallelism: Option<usize>,
) -> Result<bool> {
    let tasks = tasks::load(path)?;
    let config = Config::load(root)?;
    let repos = select::select(root, config.as_ref(), omit, true)?;
    let jobs = repos
        .into_iter()
        .map(|repo| task_job(root, repo, &tasks))
        .collect();
    Ok(execute(jobs, parallelism, true))
}

/// Builds one job per repository: every task becomes a shell step with its
/// variables substituted. `tasks` is already in execution order, so a
/// dependency always refers to an earlier step.
fn task_job(root: &Path, repo: Repo, tasks: &[Task]) -> Job {
    let dir: PathBuf = root.join(&repo.name);
    let entry = repo.entry.unwrap_or_default();
    let vars = Vars {
        name: repo.name.clone(),
        url: Some(entry.url)
            .filter(|url| !url.is_empty())
            .or_else(|| git::remote_url(&dir)),
        branch: entry.branch.or_else(|| git::current_branch(&dir)),
        commit: entry.commit.or_else(|| git::head_commit(&dir)),
    };
    let mut warned = Vec::new();
    let steps = tasks
        .iter()
        .map(|task| {
            let (command, missing) = tasks::substitute(&task.command, &vars);
            for variable in missing {
                if !warned.contains(&variable) {
                    eprintln!(
                        "warning: {}: %{variable}% has no value, using an empty string",
                        repo.name
                    );
                    warned.push(variable);
                }
            }
            Step {
                label: task.task.clone(),
                argv: shell(&command),
                cwd: dir.clone(),
                depends_on: task.depends_on.as_ref().map(|dependency| {
                    tasks
                        .iter()
                        .position(|t| &t.task == dependency)
                        .expect("dependencies are validated by tasks::parse")
                }),
            }
        })
        .collect();
    Job {
        name: repo.name,
        steps,
        skip: None,
    }
}

fn shell(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), command.into()]
    } else {
        vec!["sh".into(), "-c".into(), command.into()]
    }
}
