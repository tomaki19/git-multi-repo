# git-multi-repo Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `git-multi-repo`, a Rust git plugin that applies git commands, clones and JSON-defined tasks to multiple repositories in sub-directories, managed through a `.git-multi-repo` TOML file.

**Architecture:** A binary crate with a thin `main.rs` over a library. `cli` parses arguments into one `Action`; `config` owns the `.git-multi-repo` file; `select` decides which repositories an action applies to; `git` reads repository state by calling the `git` executable; `tasks` parses and orders task files and substitutes variables; `runner` executes per-repository jobs sequentially (live output) or in parallel (buffered output); `actions` wires everything together, one function per action.

**Tech Stack:** Rust (stable, edition 2024), `clap` (derive), `regex`, `serde`, `toml` (with `preserve_order`), `serde_json`, `anyhow`; tests with `tempfile`, `assert_cmd`, `predicates`.

**Spec:** `docs/superpowers/specs/2026-10-21-git-multi-repo-design.md`

## Global Constraints

- Binary name: `git-multi-repo` (so `git multi-repo …` works when it is on `PATH`).
- All git interaction goes through the `git` executable as a subprocess. No libgit2 / gix.
- Runtime dependencies are limited to: `clap` (derive), `regex`, `serde` (derive), `toml` (preserve_order), `serde_json`, `anyhow`. Dev dependencies: `assert_cmd`, `predicates`, `tempfile`.
- Parallelism uses `std::thread` only. No async runtime, no rayon.
- Exit codes: `0` everything succeeded; `1` at least one repository command or task failed (runs always continue past failures); `2` usage error, invalid regex, invalid `.git-multi-repo`, invalid tasks file.
- Regexes are full-match: every user pattern is compiled as `^(?:<pattern>)$`.
- Every source file starts with:
  ```rust
  // SPDX-FileCopyrightText: 2026 Thomas Killer
  // SPDX-License-Identifier: MIT
  ```
- All commands below run from the repository root `git-multi-repo/` unless stated otherwise.

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | crate metadata and dependencies |
| `src/main.rs` | parse CLI, call `actions::run`, map result to exit code |
| `src/lib.rs` | module declarations |
| `src/cli.rs` | `Cli` (clap derive), `Action`, `Cli::action()`, `Cli::parallelism()` |
| `src/config.rs` | `Config`, `Entry`, `FILE_NAME`: parse, validate, serialize, load, atomic save, delete |
| `src/git.rs` | `remote_url`, `current_branch`, `head_commit` via `git` subprocess |
| `src/select.rs` | `full_match`, `scan`, `Repo`, `select` |
| `src/tasks.rs` | `Task`, `Vars`, `load`, `parse` (validate + order), `substitute` |
| `src/runner.rs` | `Step`, `Job`, `Outcome`, `Report`, `run`, `summarize` |
| `src/actions.rs` | `run` dispatch plus one function per action |
| `tests/common/mod.rs` | integration test fixture: temporary workspace with real repositories and bare remotes |
| `tests/passthrough.rs` | git command passthrough, `--omit`, `--parallel`, exit codes |
| `tests/clone.rs` | `clone` with and without `.git-multi-repo` |
| `tests/config_file.rs` | `--persist`, `--pin`, `--remove`, `--unpin` |
| `tests/tasks.rs` | `--tasks` |
| `README.md`, `LICENSE` | documentation, MIT license |

---

### Task 1: Scaffold the crate and the command-line interface

**Files:**
- Create: `Cargo.toml`, `.gitignore` (via `cargo init`), `src/main.rs` (cargo template, replaced in Task 6), `src/lib.rs`, `src/cli.rs`, `LICENSE`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `git_multi_repo::cli::Cli` — clap parser with pub fields `omit: Option<String>`, `parallel: bool`, `jobs: Option<NonZeroUsize>`, `persist/pin/remove/unpin: Option<Option<String>>`, `tasks: Option<PathBuf>`, `command: Vec<String>`.
  - `pub enum Action { Git(Vec<String>), Clone(Vec<String>), Tasks(PathBuf), Persist(Option<String>), Pin(Option<String>), Remove(Option<String>), Unpin(Option<String>) }` — `Git` holds the git command and its arguments; `Clone` holds only the arguments after `clone`.
  - `Cli::action(&self) -> Action`
  - `Cli::parallelism(&self) -> Option<usize>` — `None` = sequential, `Some(n)` = parallel with `n` workers.

- [ ] **Step 1: Initialise the crate and add dependencies**

```bash
cargo init --name git-multi-repo --edition 2024
cargo add anyhow regex serde_json
cargo add clap --features derive
cargo add serde --features derive
cargo add toml --features preserve_order
cargo add --dev assert_cmd predicates tempfile
cp ../git-multi-repo-old/LICENSE LICENSE
```

Then edit the `[package]` section of `Cargo.toml` so it reads (keep the generated `[dependencies]` / `[dev-dependencies]`):

```toml
[package]
name = "git-multi-repo"
version = "0.1.0"
edition = "2024"
license = "MIT"
description = "Apply git commands and tasks to multiple git repositories"
```

Update the copyright line in `LICENSE` to `Copyright (c) 2025-2026 Thomas Killer`.

- [ ] **Step 2: Write `src/lib.rs`**

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! `git-multi-repo` applies git commands, clones and tasks to the git
//! repositories found in the sub-directories of the working directory,
//! optionally restricted and pinned by a `.git-multi-repo` file.

pub mod cli;
```

- [ ] **Step 3: Write the failing CLI tests**

Create `src/cli.rs` with the tests only:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Command-line interface: argument definitions and their translation into
//! exactly one [`Action`].

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
        assert_eq!(cli.action(), Action::Git(strings(&["log", "--oneline", "-n", "1"])));
    }

    #[test]
    fn options_before_the_git_command_are_parsed() {
        let cli = parse(&["--omit", "x", "--parallel", "--jobs", "3", "fetch", "--prune"]).unwrap();
        assert_eq!(cli.omit.as_deref(), Some("x"));
        assert_eq!(cli.parallelism(), Some(3));
        assert_eq!(cli.action(), Action::Git(strings(&["fetch", "--prune"])));
    }

    #[test]
    fn clone_is_recognised() {
        let cli = parse(&["clone", "--depth", "1"]).unwrap();
        assert_eq!(cli.action(), Action::Clone(strings(&["--depth", "1"])));
    }

    #[test]
    fn file_actions_take_an_optional_regex() {
        assert_eq!(parse(&["--persist"]).unwrap().action(), Action::Persist(None));
        assert_eq!(
            parse(&["--pin", "lib-.*"]).unwrap().action(),
            Action::Pin(Some("lib-.*".into()))
        );
        assert_eq!(parse(&["--remove"]).unwrap().action(), Action::Remove(None));
        assert_eq!(parse(&["--unpin", "a"]).unwrap().action(), Action::Unpin(Some("a".into())));
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
        assert_eq!(parse(&["--jobs", "3", "fetch"]).unwrap().parallelism(), None);
        assert!(parse(&["--parallel", "fetch"]).unwrap().parallelism().unwrap() >= 1);
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --lib cli`
Expected: compile errors `cannot find type Cli` / `cannot find type Action`.

- [ ] **Step 5: Implement the CLI**

Insert above the `#[cfg(test)]` module in `src/cli.rs`:

```rust
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
        .args(["persist", "pin", "remove", "unpin", "tasks", "command"]))
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
    #[arg(value_name = "GIT_COMMAND", trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// The single thing one invocation does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run `git <args…>` in every selected repository.
    Git(Vec<String>),
    /// `clone`, holding the arguments that followed `clone`.
    Clone(Vec<String>),
    Tasks(PathBuf),
    Persist(Option<String>),
    Pin(Option<String>),
    Remove(Option<String>),
    Unpin(Option<String>),
}

impl Cli {
    pub fn action(&self) -> Action {
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
        match self.command.split_first() {
            Some((first, rest)) if first == "clone" => Action::Clone(rest.to_vec()),
            _ => Action::Git(self.command.clone()),
        }
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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib cli`
Expected: 8 tests pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore LICENSE src
git commit -m "feat: scaffold crate and command-line interface"
```

---

### Task 2: The `.git-multi-repo` file

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs` (add `pub mod config;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const FILE_NAME: &str = ".git-multi-repo";`
  - `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct Entry { pub url: String, pub branch: Option<String>, pub commit: Option<String> }`
  - `#[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct Config { pub entries: Vec<(String, Entry)> }` (file order)
  - `Config::parse(text: &str) -> anyhow::Result<Config>`
  - `Config::to_toml(&self) -> String`
  - `Config::load(dir: &Path) -> anyhow::Result<Option<Config>>` (`None` if the file does not exist)
  - `Config::save(&self, dir: &Path) -> anyhow::Result<()>` (atomic)
  - `Config::delete(dir: &Path) -> anyhow::Result<()>`
  - `Config::get(&self, name: &str) -> Option<&Entry>`

- [ ] **Step 1: Register the module**

Append to `src/lib.rs`:

```rust
pub mod config;
```

- [ ] **Step 2: Write the failing tests**

Create `src/config.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! The `.git-multi-repo` file: an ordered list of repositories, each with a
//! url, an optional branch and an optional pinned commit.

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, branch: Option<&str>, commit: Option<&str>) -> Entry {
        Entry {
            url: url.into(),
            branch: branch.map(Into::into),
            commit: commit.map(Into::into),
        }
    }

    fn error(text: &str) -> String {
        format!("{:#}", Config::parse(text).unwrap_err())
    }

    #[test]
    fn parses_entries_in_file_order() {
        let config = Config::parse(
            "[zeta]\nurl = \"u1\"\n\n[alpha]\nurl = \"u2\"\nbranch = \"main\"\ncommit = \"abc\"\n",
        )
        .unwrap();
        assert_eq!(
            config.entries,
            vec![
                ("zeta".to_owned(), entry("u1", None, None)),
                ("alpha".to_owned(), entry("u2", Some("main"), Some("abc"))),
            ]
        );
        assert_eq!(config.get("alpha"), Some(&entry("u2", Some("main"), Some("abc"))));
        assert_eq!(config.get("missing"), None);
    }

    #[test]
    fn writes_the_documented_format() {
        let config = Config {
            entries: vec![
                ("my-lib".into(), entry("git@host:me/my-lib.git", Some("main"), Some("3d193e9"))),
                ("other".into(), entry("u", None, None)),
            ],
        };
        assert_eq!(
            config.to_toml(),
            "[my-lib]\nurl = \"git@host:me/my-lib.git\"\nbranch = \"main\"\ncommit = \"3d193e9\"\n\n[other]\nurl = \"u\"\n"
        );
    }

    #[test]
    fn round_trips_names_and_values_that_need_quoting() {
        let config = Config {
            entries: vec![
                ("my.lib".into(), entry("C:\\repos\\\"x\"\t", None, None)),
                ("b".into(), entry("u", Some("feature/x"), None)),
            ],
        };
        assert_eq!(Config::parse(&config.to_toml()).unwrap(), config);
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(Config::parse("[a\nurl = \"u\"").is_err());
    }

    #[test]
    fn rejects_non_table_entries() {
        assert!(error("a = \"x\"\n").contains("'a' must be a table"));
    }

    #[test]
    fn rejects_missing_or_empty_url() {
        assert!(error("[a]\nbranch = \"main\"\n").contains("[a] missing or empty 'url'"));
        assert!(error("[a]\nurl = \"\"\n").contains("[a] missing or empty 'url'"));
    }

    #[test]
    fn rejects_non_string_fields() {
        assert!(error("[a]\nurl = \"u\"\nbranch = 1\n").contains("[a] 'branch' must be a string"));
        assert!(error("[a]\nurl = 1\n").contains("[a] 'url' must be a string"));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(error("[a]\nurl = \"u\"\nfoo = \"x\"\n").contains("[a] unknown key 'foo'"));
    }

    #[test]
    fn load_returns_none_without_a_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), None);
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config { entries: vec![("a".into(), entry("u", Some("main"), None))] };
        config.save(dir.path()).unwrap();
        assert_eq!(Config::load(dir.path()).unwrap(), Some(config));
        assert!(!dir.path().join(format!("{FILE_NAME}.tmp")).exists());
        Config::delete(dir.path()).unwrap();
        assert!(!dir.path().join(FILE_NAME).exists());
    }

    #[test]
    fn load_reports_an_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "[a]\n").unwrap();
        let message = format!("{:#}", Config::load(dir.path()).unwrap_err());
        assert!(message.contains("invalid .git-multi-repo"), "{message}");
        assert!(message.contains("missing or empty 'url'"), "{message}");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib config`
Expected: compile errors `cannot find type Config` / `Entry`.

- [ ] **Step 4: Implement the config module**

Insert above the `#[cfg(test)]` module in `src/config.rs`:

```rust
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

pub const FILE_NAME: &str = ".git-multi-repo";

/// One repository: where to clone it from and, optionally, which branch and
/// which exact commit to check out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub url: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
}

/// The whole file. Entries keep the order in which they appear in the file,
/// because that order is the order in which repositories are processed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub entries: Vec<(String, Entry)>,
}

impl Config {
    /// Parses and validates the file content. Validation is strict: an
    /// unexpected key is far more likely a typo than an intention.
    pub fn parse(text: &str) -> Result<Config> {
        let table: toml::Table = toml::from_str(text)?;
        let mut entries = Vec::new();
        for (name, value) in table {
            let toml::Value::Table(fields) = value else {
                bail!("'{name}' must be a table");
            };
            let mut entry = Entry::default();
            for (key, value) in fields {
                match (key.as_str(), value) {
                    ("url", toml::Value::String(s)) => entry.url = s,
                    ("branch", toml::Value::String(s)) => entry.branch = Some(s),
                    ("commit", toml::Value::String(s)) => entry.commit = Some(s),
                    ("url" | "branch" | "commit", _) => bail!("[{name}] '{key}' must be a string"),
                    _ => bail!("[{name}] unknown key '{key}'"),
                }
            }
            if entry.url.is_empty() {
                bail!("[{name}] missing or empty 'url'");
            }
            entries.push((name, entry));
        }
        Ok(Config { entries })
    }

    /// Serializes in a fixed, diff-friendly layout: one table per entry,
    /// keys in the order url, branch, commit, a blank line between tables.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        for (name, entry) in &self.entries {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", toml_key(name)));
            out.push_str(&format!("url = {}\n", toml_string(&entry.url)));
            if let Some(branch) = &entry.branch {
                out.push_str(&format!("branch = {}\n", toml_string(branch)));
            }
            if let Some(commit) = &entry.commit {
                out.push_str(&format!("commit = {}\n", toml_string(commit)));
            }
        }
        out
    }

    /// Returns `Ok(None)` when there is no file, and an error when the file
    /// exists but is invalid — callers must not silently fall back to
    /// scanning in that case.
    pub fn load(dir: &Path) -> Result<Option<Config>> {
        let path = dir.join(FILE_NAME);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        Config::parse(&text)
            .map(Some)
            .with_context(|| format!("invalid {FILE_NAME}"))
    }

    /// Writes to a temporary file next to the target and renames it, so an
    /// interrupted write never leaves a truncated file behind.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(FILE_NAME);
        let temporary = dir.join(format!("{FILE_NAME}.tmp"));
        fs::write(&temporary, self.to_toml())
            .with_context(|| format!("cannot write {}", temporary.display()))?;
        fs::rename(&temporary, &path)
            .with_context(|| format!("cannot replace {}", path.display()))
    }

    pub fn delete(dir: &Path) -> Result<()> {
        let path = dir.join(FILE_NAME);
        fs::remove_file(&path).with_context(|| format!("cannot delete {}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(entry_name, _)| entry_name == name)
            .map(|(_, entry)| entry)
    }
}

/// Bare keys are written as-is; anything else (e.g. names containing dots)
/// is quoted so it stays a single table name.
fn toml_key(name: &str) -> String {
    let bare = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare { name.to_owned() } else { toml_string(name) }
}

fn toml_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib config`
Expected: 11 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/config.rs
git commit -m "feat: read, validate and write the .git-multi-repo file"
```

---

### Task 3: Git queries and repository selection

**Files:**
- Create: `src/git.rs`, `src/select.rs`
- Modify: `src/lib.rs` (add `pub mod git;` and `pub mod select;`)

**Interfaces:**
- Consumes: `config::{Config, Entry}` from Task 2.
- Produces:
  - `git::remote_url(dir: &Path) -> Option<String>` — `origin`, else first remote.
  - `git::current_branch(dir: &Path) -> Option<String>` — `None` on detached HEAD.
  - `git::head_commit(dir: &Path) -> Option<String>` — full SHA, `None` without commits.
  - `select::full_match(pattern: &str) -> anyhow::Result<regex::Regex>`
  - `select::scan(root: &Path) -> anyhow::Result<Vec<String>>` — sorted names of sub-directories containing `.git`.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Repo { pub name: String, pub entry: Option<Entry> }`
  - `select::select(root: &Path, config: Option<&Config>, omit: Option<&Regex>, require_existing: bool) -> anyhow::Result<Vec<Repo>>`

- [ ] **Step 1: Register the modules**

Append to `src/lib.rs`:

```rust
pub mod git;
pub mod select;
```

- [ ] **Step 2: Write the failing git tests**

Create `src/git.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Reads repository state by asking the `git` executable, so the answers
//! match exactly what the user's own git would report.

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
        git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "first"]);
        assert_eq!(head_commit(dir.path()).unwrap().len(), 40);
        assert_eq!(current_branch(dir.path()).as_deref(), Some("main"));
    }

    #[test]
    fn detached_head_has_no_branch() {
        let dir = repository();
        git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "first"]);
        git(dir.path(), &["checkout", "-q", "--detach"]);
        assert_eq!(current_branch(dir.path()), None);
    }

    #[test]
    fn prefers_origin_and_falls_back_to_the_first_remote() {
        let dir = repository();
        assert_eq!(remote_url(dir.path()), None);
        git(dir.path(), &["remote", "add", "upstream", "https://example.com/up.git"]);
        assert_eq!(remote_url(dir.path()).as_deref(), Some("https://example.com/up.git"));
        git(dir.path(), &["remote", "add", "origin", "https://example.com/origin.git"]);
        assert_eq!(remote_url(dir.path()).as_deref(), Some("https://example.com/origin.git"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib git::`
Expected: compile errors `cannot find function head_commit` etc.

- [ ] **Step 4: Implement the git module**

Insert above the `#[cfg(test)]` module in `src/git.rs`:

```rust
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
```

- [ ] **Step 5: Run the git tests to verify they pass**

Run: `cargo test --lib git::`
Expected: 3 tests pass.

- [ ] **Step 6: Write the failing selection tests**

Create `src/select.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Decides which repositories an action applies to: the entries of a valid
//! `.git-multi-repo` file if there is one, otherwise every git sub-directory,
//! minus whatever `--omit` matches.

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// `a` and `c` contain a `.git` directory, `b` a `.git` file (worktree
    /// style), `plain` no `.git` at all, and `f` is a regular file.
    fn workspace() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in ["c", "a"] {
            fs::create_dir_all(dir.path().join(name).join(".git")).unwrap();
        }
        fs::create_dir(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("b").join(".git"), "gitdir: elsewhere\n").unwrap();
        fs::create_dir(dir.path().join("plain")).unwrap();
        fs::write(dir.path().join("f"), "").unwrap();
        dir
    }

    fn names(repos: &[Repo]) -> Vec<&str> {
        repos.iter().map(|repo| repo.name.as_str()).collect()
    }

    #[test]
    fn full_match_requires_the_whole_name() {
        let test = full_match("test").unwrap();
        assert!(test.is_match("test"));
        assert!(!test.is_match("my-test-repo"));
        assert!(full_match(".*test.*").unwrap().is_match("my-test-repo"));
        let either = full_match("a|b").unwrap();
        assert!(either.is_match("b"));
        assert!(!either.is_match("ab"));
    }

    #[test]
    fn full_match_reports_invalid_patterns() {
        let message = format!("{:#}", full_match("(").unwrap_err());
        assert!(message.contains("invalid regex '('"), "{message}");
    }

    #[test]
    fn scan_finds_git_subdirectories_sorted_by_name() {
        let ws = workspace();
        assert_eq!(scan(ws.path()).unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn without_a_file_all_scanned_repositories_minus_omitted_are_selected() {
        let ws = workspace();
        let omit = full_match("b").unwrap();
        let repos = select(ws.path(), None, Some(&omit), true).unwrap();
        assert_eq!(names(&repos), ["a", "c"]);
        assert!(repos.iter().all(|repo| repo.entry.is_none()));
    }

    #[test]
    fn with_a_file_entries_are_selected_in_file_order() {
        let ws = workspace();
        let config =
            Config::parse("[c]\nurl = \"u\"\n[ghost]\nurl = \"u\"\n[a]\nurl = \"u\"\n").unwrap();
        let existing = select(ws.path(), Some(&config), None, true).unwrap();
        assert_eq!(names(&existing), ["c", "a"]);
        assert_eq!(existing[0].entry.as_ref().unwrap().url, "u");
        let all = select(ws.path(), Some(&config), None, false).unwrap();
        assert_eq!(names(&all), ["c", "ghost", "a"]);
        let omit = full_match("c").unwrap();
        let omitted = select(ws.path(), Some(&config), Some(&omit), true).unwrap();
        assert_eq!(names(&omitted), ["a"]);
    }
}
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cargo test --lib select`
Expected: compile errors `cannot find function full_match` etc.

- [ ] **Step 8: Implement the select module**

Insert above the `#[cfg(test)]` module in `src/select.rs`:

```rust
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;

use crate::config::{Config, Entry};

/// Compiles a user pattern so it must match the whole directory name:
/// `test` matches `test` but not `my-test-repo`.
pub fn full_match(pattern: &str) -> Result<Regex> {
    Regex::new(&format!("^(?:{pattern})$")).with_context(|| format!("invalid regex '{pattern}'"))
}

/// Names of the immediate sub-directories that contain a `.git` directory or
/// file, sorted. Names that are not valid UTF-8 are ignored.
pub fn scan(root: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let entries =
        fs::read_dir(root).with_context(|| format!("cannot read directory {}", root.display()))?;
    for dirent in entries {
        let dirent = dirent?;
        let path = dirent.path();
        if path.is_dir() && path.join(".git").exists() {
            if let Some(name) = dirent.file_name().to_str() {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// A selected repository; `entry` is its `.git-multi-repo` entry if a file
/// is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub name: String,
    pub entry: Option<Entry>,
}

/// With `require_existing`, file entries whose directory does not exist are
/// skipped with a warning (everything except `clone` wants that).
pub fn select(
    root: &Path,
    config: Option<&Config>,
    omit: Option<&Regex>,
    require_existing: bool,
) -> Result<Vec<Repo>> {
    let omitted = |name: &str| omit.is_some_and(|re| re.is_match(name));
    let mut repos = Vec::new();
    match config {
        Some(config) => {
            for (name, entry) in &config.entries {
                if omitted(name) {
                    continue;
                }
                if require_existing && !root.join(name).is_dir() {
                    eprintln!("warning: skipping '{name}': directory does not exist");
                    continue;
                }
                repos.push(Repo { name: name.clone(), entry: Some(entry.clone()) });
            }
        }
        None => {
            for name in scan(root)? {
                if !omitted(&name) {
                    repos.push(Repo { name, entry: None });
                }
            }
        }
    }
    Ok(repos)
}
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test --lib`
Expected: all tests (cli, config, git, select) pass.

- [ ] **Step 10: Commit**

```bash
git add src/lib.rs src/git.rs src/select.rs
git commit -m "feat: query git state and select repositories"
```

---

### Task 4: Task files

**Files:**
- Create: `src/tasks.rs`
- Modify: `src/lib.rs` (add `pub mod tasks;`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq, Deserialize)] pub struct Task { pub task: String, pub depends_on: Option<String>, pub command: String }` (JSON field `dependsOn`)
  - `tasks::load(path: &Path) -> anyhow::Result<Vec<Task>>` — validated, in execution order.
  - `tasks::parse(text: &str) -> anyhow::Result<Vec<Task>>` — validated, in execution order.
  - `#[derive(Debug, Clone, Default)] pub struct Vars { pub name: String, pub url: Option<String>, pub branch: Option<String>, pub commit: Option<String> }`
  - `tasks::substitute(command: &str, vars: &Vars) -> (String, Vec<&'static str>)` — second element lists referenced variables without a value (`"url"`, `"branch"`, `"commit"`), each once.

- [ ] **Step 1: Register the module**

Append to `src/lib.rs`:

```rust
pub mod tasks;
```

- [ ] **Step 2: Write the failing tests**

Create `src/tasks.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Task files: a JSON array of named shell commands, each optionally
//! depending on one other task, plus `%variable%` substitution.

#[cfg(test)]
mod tests {
    use super::*;

    fn order_of(text: &str) -> Vec<String> {
        parse(text).unwrap().into_iter().map(|task| task.task).collect()
    }

    fn error(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    #[test]
    fn parses_tasks_with_optional_dependency() {
        let tasks = parse(r#"[{"task":"a","command":"x"},{"task":"b","dependsOn":"a","command":"y"}]"#)
            .unwrap();
        assert_eq!(
            tasks,
            vec![
                Task { task: "a".into(), depends_on: None, command: "x".into() },
                Task { task: "b".into(), depends_on: Some("a".into()), command: "y".into() },
            ]
        );
    }

    #[test]
    fn independent_tasks_keep_file_order() {
        assert_eq!(
            order_of(r#"[{"task":"x","command":""},{"task":"y","command":""},{"task":"z","command":""}]"#),
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
        let vars = Vars { name: "lib".into(), ..Vars::default() };
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
        let vars = Vars { name: "%url%".into(), ..vars() };
        let (command, _) = substitute("%name%", &vars);
        assert_eq!(command, "%url%");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib tasks`
Expected: compile errors `cannot find function parse` etc.

- [ ] **Step 4: Implement the tasks module**

Insert above the `#[cfg(test)]` module in `src/tasks.rs`:

```rust
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
        if let Some(dependency) = &task.depends_on {
            if !names.contains(dependency.as_str()) {
                bail!("task '{}' depends on unknown task '{dependency}'", task.task);
            }
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib tasks`
Expected: 9 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/tasks.rs
git commit -m "feat: parse, validate and order task files with variable substitution"
```

---

### Task 5: The runner

**Files:**
- Create: `src/runner.rs`
- Modify: `src/lib.rs` (add `pub mod runner;`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `#[derive(Debug, Clone)] pub struct Step { pub label: String, pub argv: Vec<String>, pub cwd: PathBuf, pub depends_on: Option<usize> }` — `depends_on` is the index of an earlier step in the same job.
  - `#[derive(Debug, Clone)] pub struct Job { pub name: String, pub steps: Vec<Step>, pub skip: Option<String> }` — `skip = Some(reason)` runs nothing.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub enum Outcome { Ok, Failed(Option<i32>), Skipped }`
  - `#[derive(Debug, Clone)] pub struct Report { pub name: String, pub skip: Option<String>, pub steps: Vec<(String, Outcome)> }` with `Report::failed(&self) -> bool`
  - `runner::run(jobs: Vec<Job>, parallelism: Option<usize>) -> Vec<Report>` — reports in input order.
  - `runner::summarize(reports: &[Report], show_steps: bool) -> String`

- [ ] **Step 1: Register the module**

Append to `src/lib.rs`:

```rust
pub mod runner;
```

- [ ] **Step 2: Write the failing tests**

Create `src/runner.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Runs per-repository jobs. Sequential runs stream output live and let
//! commands talk to the terminal; parallel runs buffer each repository's
//! output and print it as one block when that repository is done.

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn step(label: &str, script: &str, depends_on: Option<usize>) -> Step {
        Step {
            label: label.into(),
            argv: vec!["sh".into(), "-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            depends_on,
        }
    }

    fn job(name: &str, steps: Vec<Step>) -> Job {
        Job { name: name.into(), steps, skip: None }
    }

    fn outcomes(report: &Report) -> Vec<Outcome> {
        report.steps.iter().map(|(_, outcome)| outcome.clone()).collect()
    }

    #[test]
    fn a_failed_step_skips_its_dependents_transitively() {
        let reports = run(
            vec![job(
                "r",
                vec![
                    step("a", "exit 3", None),
                    step("b", "true", Some(0)),
                    step("c", "true", Some(1)),
                    step("d", "true", None),
                ],
            )],
            None,
        );
        assert_eq!(
            outcomes(&reports[0]),
            [Outcome::Failed(Some(3)), Outcome::Skipped, Outcome::Skipped, Outcome::Ok]
        );
        assert!(reports[0].failed());
    }

    #[test]
    fn a_skipped_job_runs_nothing_and_is_not_a_failure() {
        let reports = run(
            vec![Job {
                name: "r".into(),
                steps: vec![step("a", "exit 1", None)],
                skip: Some("directory already exists".into()),
            }],
            None,
        );
        assert!(reports[0].steps.is_empty());
        assert!(!reports[0].failed());
    }

    #[test]
    fn parallel_reports_keep_input_order() {
        let jobs = (0..8)
            .map(|i| job(&format!("r{i}"), vec![step("s", if i % 2 == 0 { "true" } else { "exit 1" }, None)]))
            .collect();
        let reports = run(jobs, Some(3));
        let names: Vec<&str> = reports.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7"]);
        assert!(!reports[0].failed());
        assert!(reports[1].failed());
    }

    #[test]
    fn parallel_steps_get_no_stdin() {
        let reports = run(vec![job("r", vec![step("read", "read line", None)])], Some(1));
        assert!(reports[0].failed());
    }

    #[test]
    fn a_missing_program_is_a_failure_without_exit_code() {
        let mut missing = step("x", "", None);
        missing.argv = vec!["definitely-not-a-program-gmr".into()];
        let reports = run(vec![job("r", vec![missing])], None);
        assert_eq!(outcomes(&reports[0]), [Outcome::Failed(None)]);
    }

    #[test]
    fn summary_lists_status_and_optionally_steps() {
        let reports = vec![
            Report { name: "a".into(), skip: None, steps: vec![("x".into(), Outcome::Ok)] },
            Report {
                name: "b".into(),
                skip: None,
                steps: vec![("x".into(), Outcome::Failed(Some(2))), ("y".into(), Outcome::Skipped)],
            },
            Report { name: "c".into(), skip: Some("directory already exists".into()), steps: vec![] },
            Report { name: "d".into(), skip: None, steps: vec![("x".into(), Outcome::Failed(None))] },
        ];
        assert_eq!(
            summarize(&reports, true),
            "── summary ──\na: ok\nb: failed (exit 2) [failed: x] [skipped: y]\nc: skipped (directory already exists)\nd: failed [failed: x]\n"
        );
        assert_eq!(
            summarize(&reports, false),
            "── summary ──\na: ok\nb: failed (exit 2)\nc: skipped (directory already exists)\nd: failed\n"
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib runner`
Expected: compile errors `cannot find type Step` etc.

- [ ] **Step 4: Implement the runner**

Insert above the `#[cfg(test)]` module in `src/runner.rs`:

```rust
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;

/// One command within a job. `depends_on` points at an earlier step of the
/// same job; if that step did not succeed, this one is skipped.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub depends_on: Option<usize>,
}

/// Everything to do for one repository. A job with `skip` set runs nothing
/// and is reported as skipped with that reason.
#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub steps: Vec<Step>,
    pub skip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// Exit code, or `None` if the process could not start or was killed.
    Failed(Option<i32>),
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub name: String,
    pub skip: Option<String>,
    pub steps: Vec<(String, Outcome)>,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|(_, outcome)| matches!(outcome, Outcome::Failed(_)))
    }
}

/// Runs all jobs and returns their reports in input order.
pub fn run(jobs: Vec<Job>, parallelism: Option<usize>) -> Vec<Report> {
    match parallelism {
        None => jobs.into_iter().map(run_streaming).collect(),
        Some(workers) => run_parallel(jobs, workers),
    }
}

/// Executes the steps of a job in order, honouring dependencies; `execute`
/// decides how a single step is run (streaming or captured).
fn run_steps(job: Job, mut execute: impl FnMut(&Step) -> Outcome) -> Report {
    let mut results: Vec<(String, Outcome)> = Vec::with_capacity(job.steps.len());
    if job.skip.is_none() {
        for step in &job.steps {
            let blocked = step.depends_on.is_some_and(|dep| results[dep].1 != Outcome::Ok);
            let outcome = if blocked { Outcome::Skipped } else { execute(step) };
            results.push((step.label.clone(), outcome));
        }
    }
    Report { name: job.name, skip: job.skip, steps: results }
}

fn command(step: &Step) -> Command {
    let mut command = Command::new(&step.argv[0]);
    command.args(&step.argv[1..]).current_dir(&step.cwd);
    command
}

fn outcome(status: ExitStatus) -> Outcome {
    if status.success() { Outcome::Ok } else { Outcome::Failed(status.code()) }
}

fn run_streaming(job: Job) -> Report {
    println!("── {} ──", job.name);
    if let Some(reason) = &job.skip {
        println!("skipped: {reason}");
    }
    run_steps(job, |step| match command(step).status() {
        Ok(status) => outcome(status),
        Err(error) => {
            eprintln!("error: cannot run {}: {error}", step.argv[0]);
            Outcome::Failed(None)
        }
    })
}

fn execute_captured(step: &Step, out: &mut Vec<u8>, err: &mut Vec<u8>) -> Outcome {
    match command(step).stdin(Stdio::null()).output() {
        Ok(output) => {
            out.extend_from_slice(&output.stdout);
            err.extend_from_slice(&output.stderr);
            outcome(output.status)
        }
        Err(error) => {
            let _ = writeln!(err, "error: cannot run {}: {error}", step.argv[0]);
            Outcome::Failed(None)
        }
    }
}

/// A fixed pool of scoped worker threads pulls jobs from a shared queue.
/// Each job's output is collected in memory and written under a lock, so
/// blocks from different repositories never interleave.
fn run_parallel(jobs: Vec<Job>, workers: usize) -> Vec<Report> {
    let count = jobs.len();
    let queue = Mutex::new(jobs.into_iter().enumerate());
    let reports: Mutex<Vec<Option<Report>>> = Mutex::new((0..count).map(|_| None).collect());
    let print_lock = Mutex::new(());
    thread::scope(|scope| {
        for _ in 0..workers.clamp(1, count.max(1)) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, job)) = next else { break };
                    let mut out = Vec::new();
                    let mut err = Vec::new();
                    let _ = writeln!(out, "── {} ──", job.name);
                    if let Some(reason) = &job.skip {
                        let _ = writeln!(out, "skipped: {reason}");
                    }
                    let report = run_steps(job, |step| execute_captured(step, &mut out, &mut err));
                    {
                        let _guard = print_lock.lock().unwrap();
                        let mut stdout = io::stdout().lock();
                        let _ = stdout.write_all(&out);
                        let _ = stdout.flush();
                        let _ = io::stderr().write_all(&err);
                    }
                    reports.lock().unwrap()[index] = Some(report);
                }
            });
        }
    });
    reports
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|report| report.expect("every job produces a report"))
        .collect()
}

/// One line per repository. With `show_steps`, failed and skipped step
/// labels are appended (used for task runs).
pub fn summarize(reports: &[Report], show_steps: bool) -> String {
    let mut out = String::from("── summary ──\n");
    for report in reports {
        let first_failure = report.steps.iter().find_map(|(_, outcome)| match outcome {
            Outcome::Failed(code) => Some(*code),
            _ => None,
        });
        let status = match (&report.skip, first_failure) {
            (Some(reason), _) => format!("skipped ({reason})"),
            (None, Some(Some(code))) => format!("failed (exit {code})"),
            (None, Some(None)) => "failed".to_owned(),
            (None, None) => "ok".to_owned(),
        };
        out.push_str(&format!("{}: {status}", report.name));
        if show_steps {
            let failed = labels(report, |outcome| matches!(outcome, Outcome::Failed(_)));
            let skipped = labels(report, |outcome| *outcome == Outcome::Skipped);
            if !failed.is_empty() {
                out.push_str(&format!(" [failed: {failed}]"));
            }
            if !skipped.is_empty() {
                out.push_str(&format!(" [skipped: {skipped}]"));
            }
        }
        out.push('\n');
    }
    out
}

fn labels(report: &Report, pick: impl Fn(&Outcome) -> bool) -> String {
    report
        .steps
        .iter()
        .filter(|(_, outcome)| pick(outcome))
        .map(|(label, _)| label.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib runner`
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/runner.rs
git commit -m "feat: run jobs sequentially or in parallel and summarize results"
```

---

### Task 6: Binary entry point, git passthrough and clone

**Files:**
- Create: `src/actions.rs`, `tests/common/mod.rs`, `tests/passthrough.rs`, `tests/clone.rs`
- Modify: `src/lib.rs` (add `pub mod actions;`), `src/main.rs` (replace template)

**Interfaces:**
- Consumes: `cli::{Action, Cli}` (Task 1), `config::{Config, FILE_NAME}` (Task 2), `select::{self, Repo}` (Task 3), `runner::{self, Job, Step}` (Task 5).
- Produces:
  - `actions::run(cli: &Cli, root: &Path) -> anyhow::Result<bool>` — `Ok(true)` all succeeded, `Ok(false)` some repository failed, `Err` usage/config error.
  - Private helper used by Tasks 7 and 8: `fn execute(jobs: Vec<Job>, parallelism: Option<usize>, show_steps: bool) -> bool`.
  - Test fixture `tests/common/mod.rs`: `git(dir, args) -> String`, `git_ok(dir, args) -> bool`, `Fixture { base, work }` with `new()`, `remote(name) -> PathBuf`, `remote_path(name) -> PathBuf`, `repo(name) -> PathBuf`, `cmd(args) -> Command`, `write(file, contents)`, `read(file) -> String`, `config() -> Option<Config>`.

- [ ] **Step 1: Write the test fixture**

Create `tests/common/mod.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Integration test fixture: a temporary directory with a `work` directory
//! (where `git-multi-repo` runs) and bare remotes outside of it, so tests
//! need no network.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use git_multi_repo::config::Config;
use tempfile::TempDir;

/// Runs git with a fixed identity and returns trimmed stdout; panics on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(dir).args(args).output().expect("git runs").status.success()
}

pub struct Fixture {
    _tmp: TempDir,
    pub base: PathBuf,
    pub work: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let work = base.join("work");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(base.join("remotes")).unwrap();
        Fixture { _tmp: tmp, base, work }
    }

    pub fn remote_path(&self, name: &str) -> PathBuf {
        self.base.join("remotes").join(format!("{name}.git"))
    }

    /// Creates a bare remote with one commit on `main`.
    pub fn remote(&self, name: &str) -> PathBuf {
        let seed = self.base.join("seeds").join(name);
        fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q", "-b", "main"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "first"]);
        let remote = self.remote_path(name);
        git(&self.base, &["clone", "-q", "--bare", seed.to_str().unwrap(), remote.to_str().unwrap()]);
        remote
    }

    /// Creates a remote and clones it into `work/<name>`.
    pub fn repo(&self, name: &str) -> PathBuf {
        let remote = self.remote(name);
        git(&self.work, &["clone", "-q", remote.to_str().unwrap(), name]);
        self.work.join(name)
    }

    /// `git-multi-repo <args…>` running in `work`.
    pub fn cmd(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_git-multi-repo"));
        command.current_dir(&self.work).args(args);
        command
    }

    pub fn write(&self, file: &str, contents: &str) {
        fs::write(self.work.join(file), contents).unwrap();
    }

    pub fn read(&self, file: &str) -> String {
        fs::read_to_string(self.work.join(file)).unwrap()
    }

    pub fn config(&self) -> Option<Config> {
        Config::load(&self.work).unwrap()
    }
}
```

- [ ] **Step 2: Write the failing passthrough tests**

Create `tests/passthrough.rs`:

```rust
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
```

- [ ] **Step 3: Write the failing clone tests**

Create `tests/clone.rs`:

```rust
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
    assert!(!git_ok(&a, &["symbolic-ref", "-q", "HEAD"]), "a must be on a detached HEAD");
    assert_eq!(git(&b, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[test]
fn existing_directories_are_skipped() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.write(".git-multi-repo", &format!("[a]\nurl = \"{}\"\n", fx.remote_path("a").display()));
    fx.cmd(&["clone"])
        .assert()
        .success()
        .stdout(predicate::str::contains("a: skipped (directory already exists)"));
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
    fx.cmd(&["clone"]).assert().code(1).stdout(predicate::str::contains("a: failed"));
}

#[test]
fn without_a_file_clone_is_forwarded_to_git() {
    let fx = Fixture::new();
    let remote = fx.remote("a");
    fx.cmd(&["clone", "-q", remote.to_str().unwrap(), "copy"]).assert().success();
    assert!(fx.work.join("copy").join(".git").is_dir());
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --test passthrough --test clone`
Expected: tests fail — the binary is still the `cargo init` "Hello, world!" template, so assertions on exit codes, output and tags fail.

- [ ] **Step 5: Implement `src/actions.rs`**

Append to `src/lib.rs`:

```rust
pub mod actions;
```

Create `src/actions.rs`:

```rust
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
```

Note: the `bail!` arm for the remaining actions is intentionally temporary; Task 7 replaces the `Persist`/`Pin`/`Remove`/`Unpin` arms and Task 8 the `Tasks` arm.

- [ ] **Step 6: Replace `src/main.rs`**

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

use std::process::ExitCode;

use clap::Parser;
use git_multi_repo::{actions, cli::Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = std::env::current_dir()
        .map_err(anyhow::Error::from)
        .and_then(|root| actions::run(&cli, &root));
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test`
Expected: all unit tests plus 9 passthrough and 5 clone tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/lib.rs src/actions.rs src/main.rs tests
git commit -m "feat: forward git commands and clone repositories from the file"
```

---

### Task 7: Persist, pin, remove and unpin

**Files:**
- Modify: `src/actions.rs`
- Create: `tests/config_file.rs`

**Interfaces:**
- Consumes: `git::{remote_url, current_branch, head_commit}` (Task 3), `select::{scan, full_match}` (Task 3), `config::{Config, Entry, FILE_NAME}` (Task 2), fixture from Task 6.
- Produces: `actions::run` handles `Action::Persist`, `Action::Pin`, `Action::Remove`, `Action::Unpin`.

- [ ] **Step 1: Write the failing tests**

Create `tests/config_file.rs`:

```rust
// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

mod common;

use std::path::Path;

use assert_cmd::prelude::*;
use common::{Fixture, git};
use git_multi_repo::config::{Config, Entry};
use predicates::prelude::*;

fn entry(url: &Path, branch: &str, commit: Option<String>) -> Entry {
    Entry { url: url.to_str().unwrap().into(), branch: Some(branch.into()), commit }
}

fn names(config: &Config) -> Vec<&str> {
    config.entries.iter().map(|(name, _)| name.as_str()).collect()
}

#[test]
fn persist_writes_url_and_branch_of_every_repository() {
    let fx = Fixture::new();
    fx.repo("b");
    fx.repo("a");
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![
            ("a".to_owned(), entry(&fx.remote_path("a"), "main", None)),
            ("b".to_owned(), entry(&fx.remote_path("b"), "main", None)),
        ]
    );
}

#[test]
fn persist_regex_and_omit_select_repositories() {
    let fx = Fixture::new();
    for name in ["app", "lib-x", "lib-y"] {
        fx.repo(name);
    }
    fx.cmd(&["--omit", "lib-y", "--persist", "lib-.*"]).assert().success();
    assert_eq!(names(&fx.config().unwrap()), ["lib-x"]);
}

#[test]
fn persist_overwrites_the_file_and_drops_pins() {
    let fx = Fixture::new();
    fx.repo("a");
    fx.write(".git-multi-repo", "[old]\nurl = \"u\"\n\n[a]\nurl = \"u\"\ncommit = \"abc\"\n");
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![("a".to_owned(), entry(&fx.remote_path("a"), "main", None))]
    );
}

#[test]
fn persist_keeps_the_branch_from_the_file_for_a_detached_head() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    git(&a, &["checkout", "-q", "--detach"]);
    fx.write(".git-multi-repo", "[a]\nurl = \"u\"\nbranch = \"develop\"\n");
    fx.cmd(&["--persist"]).assert().success();
    assert_eq!(fx.config().unwrap().get("a").unwrap().branch.as_deref(), Some("develop"));
}

#[test]
fn pin_records_the_head_commit() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    fx.cmd(&["--pin"]).assert().success();
    assert_eq!(
        fx.config().unwrap().entries,
        vec![("a".to_owned(), entry(&fx.remote_path("a"), "main", Some(head)))]
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
    fx.write(".git-multi-repo", "[a]\nurl = \"u\"\n[b]\nurl = \"u\"\n[c]\nurl = \"u\"\n");
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test config_file`
Expected: tests fail with exit code 2 and `this action is not available yet`.

- [ ] **Step 3: Implement the file actions**

In `src/actions.rs`, change the imports to:

```rust
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use regex::Regex;

use crate::cli::{Action, Cli};
use crate::config::{Config, Entry, FILE_NAME};
use crate::git;
use crate::runner::{self, Job, Step};
use crate::select::{self, Repo};
```

Replace the `match` in `run` with:

```rust
    match cli.action() {
        Action::Git(args) => git_command(root, &args, omit, parallelism),
        Action::Clone(args) => clone(root, &args, omit, parallelism),
        Action::Persist(filter) => write_config(root, filter.as_deref(), omit, false),
        Action::Pin(filter) => write_config(root, filter.as_deref(), omit, true),
        Action::Remove(filter) => remove(root, filter.as_deref()),
        Action::Unpin(filter) => unpin(root, filter.as_deref()),
        Action::Tasks(_) => bail!("this action is not available yet"),
    }
```

Append to `src/actions.rs`:

```rust
fn compile(filter: Option<&str>) -> Result<Option<Regex>> {
    filter.map(select::full_match).transpose()
}

fn matches(filter: Option<&Regex>, name: &str) -> bool {
    filter.is_none_or(|re| re.is_match(name))
}

/// `--persist` / `--pin`: rebuild the file from the live sub-directories.
/// The file is overwritten, so entries not matched this time disappear.
fn write_config(root: &Path, filter: Option<&str>, omit: Option<&Regex>, pin: bool) -> Result<bool> {
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
            previous.as_ref().and_then(|c| c.get(&name)).and_then(|e| e.branch.clone())
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
        config.entries.push((name, Entry { url, branch, commit }));
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: all tests pass, including 10 in `config_file`.

- [ ] **Step 5: Commit**

```bash
git add src/actions.rs tests/config_file.rs
git commit -m "feat: persist, pin, remove and unpin .git-multi-repo entries"
```

---

### Task 8: Tasks action

**Files:**
- Modify: `src/actions.rs`
- Create: `tests/tasks.rs`

**Interfaces:**
- Consumes: `tasks::{load, substitute, Task, Vars}` (Task 4), `git::*` (Task 3), `execute` helper (Task 6).
- Produces: `actions::run` handles `Action::Tasks`; all actions are now implemented.

- [ ] **Step 1: Write the failing tests**

Create `tests/tasks.rs`:

```rust
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
    fx.write(".git-multi-repo", "[a]\nurl = \"u\"\ncommit = \"pinned123\"\n");
    fx.write("tasks.json", r#"[{"task": "t", "command": "echo %commit% %url% %branch% > out.txt"}]"#);
    fx.cmd(&["--tasks", "tasks.json"]).assert().success();
    assert_eq!(fs::read_to_string(a.join("out.txt")).unwrap(), "pinned123 u main\n");
}

#[test]
fn commit_falls_back_to_head() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let head = git(&a, &["rev-parse", "HEAD"]);
    fx.write("tasks.json", r#"[{"task": "t", "command": "echo %commit% > out.txt"}]"#);
    fx.cmd(&["--tasks", "tasks.json"]).assert().success();
    assert_eq!(fs::read_to_string(a.join("out.txt")).unwrap(), format!("{head}\n"));
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
        .stdout(predicate::str::contains("a: failed (exit 3) [failed: broken] [skipped: after]"));
    assert!(!a.join("after.txt").exists());
    assert!(a.join("other.txt").exists());
}

#[test]
fn parallel_tasks_run_in_every_repository() {
    let fx = Fixture::new();
    let repos: Vec<_> = ["a", "b", "c"].iter().map(|name| fx.repo(name)).collect();
    fx.write("tasks.json", r#"[{"task": "t", "command": "touch done.txt"}]"#);
    fx.cmd(&["--parallel", "--tasks", "tasks.json"]).assert().success();
    assert!(repos.iter().all(|dir| dir.join("done.txt").exists()));
}

#[test]
fn omit_applies_to_tasks() {
    let fx = Fixture::new();
    let a = fx.repo("a");
    let b = fx.repo("b");
    fx.write("tasks.json", r#"[{"task": "t", "command": "touch done.txt"}]"#);
    fx.cmd(&["--omit", "b", "--tasks", "tasks.json"]).assert().success();
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
    fx.write("tasks.json", r#"[{"task": "t", "command": "echo [%url%] > out.txt"}]"#);
    fx.cmd(&["--tasks", "tasks.json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: solo: %url% has no value"));
    assert_eq!(fs::read_to_string(solo.join("out.txt")).unwrap(), "[]\n");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test tasks`
Expected: tests fail with exit code 2 and `this action is not available yet`.

- [ ] **Step 3: Implement the tasks action**

In `src/actions.rs`, add to the imports:

```rust
use std::path::PathBuf;

use crate::tasks::{self, Task, Vars};
```

(merge `PathBuf` into the existing `use std::path::Path;` as `use std::path::{Path, PathBuf};`).

Replace the `Action::Tasks(_) => bail!(…)` arm with:

```rust
        Action::Tasks(path) => run_tasks(root, &path, omit, parallelism),
```

Append to `src/actions.rs`:

```rust
fn run_tasks(
    root: &Path,
    path: &Path,
    omit: Option<&Regex>,
    parallelism: Option<usize>,
) -> Result<bool> {
    let tasks = tasks::load(path)?;
    let config = Config::load(root)?;
    let repos = select::select(root, config.as_ref(), omit, true)?;
    let jobs = repos.into_iter().map(|repo| task_job(root, repo, &tasks)).collect();
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
        url: Some(entry.url).filter(|url| !url.is_empty()).or_else(|| git::remote_url(&dir)),
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
    Job { name: repo.name, steps, skip: None }
}

fn shell(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), command.into()]
    } else {
        vec!["sh".into(), "-c".into(), command.into()]
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test`
Expected: all tests pass, including 8 in `tasks`.

- [ ] **Step 5: Commit**

```bash
git add src/actions.rs tests/tasks.rs
git commit -m "feat: run task files across repositories"
```

---

### Task 9: README and final verification

**Files:**
- Create: `README.md`

**Interfaces:**
- Consumes: the finished binary.
- Produces: user documentation.

- [ ] **Step 1: Write `README.md`**

````markdown
# git-multi-repo

`git-multi-repo` is a git plugin written in Rust that applies git commands,
clones and tasks to all git repositories in the sub-directories of the
working directory.

If a `.git-multi-repo` file exists, only the repositories listed in it are
used, in file order. If the file exists but is invalid, nothing runs.

## Usage

    git multi-repo [--omit <regex>] [--parallel] [--jobs <n>] <git-command> [args...]
    git multi-repo [--omit <regex>] [--parallel] [--jobs <n>] clone [args...]
    git multi-repo [--omit <regex>] [--parallel] [--jobs <n>] --tasks <file.json>
    git multi-repo [--omit <regex>] --persist [<regex>]
    git multi-repo [--omit <regex>] --pin [<regex>]
    git multi-repo --remove [<regex>]
    git multi-repo --unpin [<regex>]

Options must come before the git command; everything after it is passed to git.

All regular expressions must match the **whole** directory name:
`--omit test` omits `test` but not `my-test-repo` (use `.*test.*` for that).

### Git commands

    git multi-repo fetch --prune
    git multi-repo --omit 'legacy-.*' status --short

Repositories are processed one after another with live output. With
`--parallel` they are processed concurrently (`--jobs <n>` workers, default:
number of CPUs); each repository's output is printed as one block, and
commands cannot read from the terminal.

### The `.git-multi-repo` file

    [my-lib]
    url = "git@github.com:me/my-lib.git"
    branch = "main"
    commit = "3d193e9c0a5b..."   # only after --pin

- `--persist [<regex>]` writes the file with `url` and `branch` of all (or
  matching) repositories. The file is **overwritten**; existing pins are dropped.
- `--pin [<regex>]` does the same and also records the current `commit`.
- `--remove [<regex>]` removes all (or matching) entries; when no entry is
  left, the file is deleted. Repositories on disk are not touched.
- `--unpin [<regex>]` removes the `commit` of all (or matching) entries.

### Cloning

    git multi-repo clone

With a `.git-multi-repo` file, every entry is cloned (on its `branch`) and
pinned entries are checked out at their `commit` (detached HEAD). Existing
directories are skipped. Extra arguments are passed to every `git clone`.
Without a file, `git multi-repo clone <args>` is a plain `git clone <args>`.

### Tasks

    git multi-repo --tasks tasks.json

```json
[
  { "task": "fetch", "command": "git fetch --prune" },
  { "task": "build", "dependsOn": "fetch", "command": "make" },
  { "task": "report", "dependsOn": "build", "command": "echo %name% %branch% %commit% %url%" }
]
```

Tasks run in each repository directory through the shell (`sh -c`, or
`cmd /C` on Windows), in dependency order. If a task fails, the tasks that
depend on it are skipped for that repository; independent tasks still run.

Variables: `%name%` (directory name), `%url%`, `%branch%`, `%commit%`. Values
come from the `.git-multi-repo` entry when present, otherwise from the
repository itself (`origin` url, current branch, `HEAD`).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | everything succeeded |
| 1 | at least one repository command or task failed |
| 2 | usage error, invalid regex, invalid `.git-multi-repo` or tasks file |

## Install

    cargo install --path .

This installs `git-multi-repo` into `~/.cargo/bin`; with that directory on
your `PATH`, git finds it as `git multi-repo`.

## License

This software is available under the MIT license.
````

- [ ] **Step 2: Run formatting, lints and the full test suite**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: `cargo fmt` may reformat files (that is fine — they are committed below); clippy reports no warnings; all tests pass. Fix any clippy findings before continuing.

- [ ] **Step 3: Smoke-test the installed plugin**

```bash
cargo install --path . --root /tmp/gmr-install
cd /tmp && rm -rf gmr-smoke && mkdir gmr-smoke && cd gmr-smoke
git init -q one && git init -q two
PATH=/tmp/gmr-install/bin:$PATH git multi-repo status --short
PATH=/tmp/gmr-install/bin:$PATH git multi-repo --help
```

Expected: a `── one ──` and `── two ──` header, a summary with `one: ok` and `two: ok`, and the help text listing all options.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: add README; apply formatting and lint fixes"
```
