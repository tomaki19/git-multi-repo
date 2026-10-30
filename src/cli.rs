// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Command-line interface: argument definitions and their translation into
//! exactly one [`Action`].

use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::{ArgGroup, Parser};

/// Raw command-line arguments. Exactly one member of the `action` group must
/// be given; [`Cli::action`] turns it into an [`Action`].
#[derive(Parser, Debug)]
#[command(
    name = "git-multi-repo",
    version,
    about = "Apply git commands and tasks to multiple git repositories",
    group(ArgGroup::new("action")
        .required(true)
        .args(["clone", "reset", "persist", "pin", "remove", "unpin", "tasks", "command"]))
)]
pub struct Cli {
    /// Omit repositories whose directory name fully matches REGEX
    #[arg(long, value_name = "REGEX", conflicts_with_all = ["remove", "unpin"])]
    pub omit: Option<String>,

    /// Process repositories in parallel; output is buffered per repository
    #[arg(long, conflicts_with_all = ["persist", "pin", "remove", "unpin"])]
    pub parallel: bool,

    /// Number of parallel workers (default: number of CPUs); used with --parallel
    #[arg(long, value_name = "N", conflicts_with_all = ["persist", "pin", "remove", "unpin"])]
    pub jobs: Option<NonZeroUsize>,

    /// Clone the repositories listed in .git-multi-repo
    #[arg(long, conflicts_with_all = ["persist", "pin", "remove", "unpin"])]
    pub clone: bool,

    /// Hard-reset the repositories to the commit or branch in .git-multi-repo
    #[arg(long, conflicts_with_all = ["persist", "pin", "remove", "unpin"])]
    pub reset: bool,

    /// Branch to clone or reset to instead of the branch and commit in .git-multi-repo
    #[arg(long, value_name = "NAME", conflicts_with_all = ["persist", "pin", "remove", "unpin", "tasks", "command"])]
    pub branch: Option<String>,

    /// Clone the repositories listed in the given .git-multi-repo file
    #[arg(long, value_name = "FILE", conflicts_with_all = ["reset", "persist", "pin", "remove", "unpin", "tasks", "command"])]
    pub from: Option<PathBuf>,

    /// Write .git-multi-repo with url and branch of all (or matching) repositories
    #[arg(long, value_name = "REGEX", num_args = 0..=1)]
    pub persist: Option<Option<String>>,

    /// Write .git-multi-repo with url, branch and commit of all (or matching) repositories
    #[arg(long, value_name = "REGEX", num_args = 0..=1)]
    pub pin: Option<Option<String>>,

    /// Remove all (or matching) entries from .git-multi-repo
    #[arg(long, value_name = "REGEX", num_args = 0..=1)]
    pub remove: Option<Option<String>>,

    /// Remove the commit of all (or matching) entries in .git-multi-repo
    #[arg(long, value_name = "REGEX", num_args = 0..=1)]
    pub unpin: Option<Option<String>>,

    /// Run the tasks defined in JSON_FILE in every repository
    #[arg(long, value_name = "JSON_FILE")]
    pub tasks: Option<PathBuf>,

    /// Git command and arguments to run in every repository
    #[arg(
        value_name = "GIT_COMMAND",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub command: Vec<String>,
}

/// The single thing one invocation does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run `git <args…>` in every selected repository.
    Git(Vec<String>),
    Clone,
    Reset,
    Tasks(PathBuf),
    Persist(Option<String>),
    Pin(Option<String>),
    Remove(Option<String>),
    Unpin(Option<String>),
}

impl Cli {
    pub fn action(&self) -> Action {
        if self.clone {
            return Action::Clone;
        }
        if self.reset {
            return Action::Reset;
        }
        if let Some(filter) = &self.persist {
            return Action::Persist(filter.clone());
        }
        if let Some(filter) = &self.pin {
            return Action::Pin(filter.clone());
        }
        if let Some(filter) = &self.remove {
            return Action::Remove(filter.clone());
        }
        if let Some(filter) = &self.unpin {
            return Action::Unpin(filter.clone());
        }
        if let Some(path) = &self.tasks {
            return Action::Tasks(path.clone());
        }
        Action::Git(self.command.clone())
    }

    /// `None` runs repositories one after another with live output;
    /// `Some(n)` runs them on `n` worker threads.
    pub fn parallelism(&self) -> Option<usize> {
        self.parallel
            .then(|| self.jobs.map_or_else(default_workers, NonZeroUsize::get))
    }
}

fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}

#[cfg(test)]
mod tests {
    // `super::*` also brings `clap::Parser` into scope, needed for `try_parse_from`.
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("git-multi-repo").chain(args.iter().copied()))
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn git_command_keeps_hyphenated_arguments() {
        let cli = parse(&["log", "--oneline", "-n", "1"]).unwrap();
        assert_eq!(
            cli.action(),
            Action::Git(strings(&["log", "--oneline", "-n", "1"]))
        );
    }

    #[test]
    fn options_before_the_git_command_are_parsed() {
        let cli = parse(&[
            "--omit",
            "x",
            "--parallel",
            "--jobs",
            "3",
            "fetch",
            "--prune",
        ])
        .unwrap();
        assert_eq!(cli.omit.as_deref(), Some("x"));
        assert_eq!(cli.parallelism(), Some(3));
        assert_eq!(cli.action(), Action::Git(strings(&["fetch", "--prune"])));
    }

    #[test]
    fn clone_and_reset_are_recognised() {
        assert_eq!(parse(&["--clone"]).unwrap().action(), Action::Clone);
        assert_eq!(
            parse(&["--omit", "x", "--reset"]).unwrap().action(),
            Action::Reset
        );
    }

    #[test]
    fn clone_and_reset_take_no_arguments() {
        assert!(parse(&["--clone", "--depth", "1"]).is_err());
        assert!(parse(&["--reset", "--hard"]).is_err());
        assert!(parse(&["--clone", "--reset"]).is_err());
        assert!(parse(&["--reset", "--pin"]).is_err());
    }

    #[test]
    fn clone_and_reset_accept_a_branch() {
        let cli = parse(&["--clone", "--branch", "main"]).unwrap();
        assert_eq!(cli.action(), Action::Clone);
        assert_eq!(cli.branch.as_deref(), Some("main"));
        let cli = parse(&["--reset", "--branch", "develop"]).unwrap();
        assert_eq!(cli.action(), Action::Reset);
        assert_eq!(cli.branch.as_deref(), Some("develop"));
    }

    #[test]
    fn branch_is_not_valid_for_other_actions() {
        assert!(parse(&["--branch", "main"]).is_err());
        assert!(parse(&["--branch", "main", "--persist"]).is_err());
        assert!(parse(&["--branch", "main", "--tasks", "t.json"]).is_err());
        assert!(parse(&["--branch", "main", "fetch"]).is_err());
    }

    #[test]
    fn clone_from_takes_a_git_multi_repo_file() {
        let cli = parse(&["--clone", "--from", "release"]).unwrap();
        assert_eq!(cli.action(), Action::Clone);
        assert_eq!(cli.from.as_deref(), Some(std::path::Path::new("release")));
    }

    #[test]
    fn from_is_only_valid_for_clone() {
        assert!(parse(&["--from", "release", "--reset"]).is_err());
        assert!(parse(&["--from", "release", "--persist"]).is_err());
    }

    #[test]
    fn file_actions_take_an_optional_regex() {
        assert_eq!(
            parse(&["--persist"]).unwrap().action(),
            Action::Persist(None)
        );
        assert_eq!(
            parse(&["--pin", "lib-.*"]).unwrap().action(),
            Action::Pin(Some("lib-.*".into()))
        );
        assert_eq!(parse(&["--remove"]).unwrap().action(), Action::Remove(None));
        assert_eq!(
            parse(&["--unpin", "a"]).unwrap().action(),
            Action::Unpin(Some("a".into()))
        );
    }

    #[test]
    fn tasks_take_a_file() {
        assert_eq!(
            parse(&["--tasks", "t.json"]).unwrap().action(),
            Action::Tasks(PathBuf::from("t.json"))
        );
    }

    #[test]
    fn exactly_one_action_is_required() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--persist", "--pin"]).is_err());
        assert!(parse(&["--tasks", "t.json", "fetch"]).is_err());
    }

    #[test]
    fn invalid_combinations_are_rejected() {
        assert!(parse(&["--omit", "x", "--remove"]).is_err());
        assert!(parse(&["--omit", "x", "--unpin"]).is_err());
        assert!(parse(&["--parallel", "--persist"]).is_err());
        assert!(parse(&["--jobs", "2", "--pin"]).is_err());
        assert!(parse(&["--parallel", "--jobs", "0", "fetch"]).is_err());
    }

    #[test]
    fn parallelism_needs_the_parallel_flag() {
        assert_eq!(parse(&["fetch"]).unwrap().parallelism(), None);
        assert_eq!(
            parse(&["--jobs", "3", "fetch"]).unwrap().parallelism(),
            None
        );
        assert!(
            parse(&["--parallel", "fetch"])
                .unwrap()
                .parallelism()
                .unwrap()
                >= 1
        );
    }
}
