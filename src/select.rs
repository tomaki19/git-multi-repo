// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Decides which repositories an action applies to: the entries of a valid
//! `.git-multi-repo` file if there is one, otherwise every git sub-directory,
//! minus whatever `--omit` matches.

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
        if path.is_dir()
            && path.join(".git").exists()
            && let Some(name) = dirent.file_name().to_str()
        {
            names.push(name.to_owned());
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
                repos.push(Repo {
                    name: name.clone(),
                    entry: Some(entry.clone()),
                });
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
