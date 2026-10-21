// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! The `.git-multi-repo` file: an ordered list of repositories, each with a
//! url, an optional branch and an optional pinned commit.

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
        let text =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
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
        fs::rename(&temporary, &path).with_context(|| format!("cannot replace {}", path.display()))
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
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        name.to_owned()
    } else {
        toml_string(name)
    }
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
        assert_eq!(
            config.get("alpha"),
            Some(&entry("u2", Some("main"), Some("abc")))
        );
        assert_eq!(config.get("missing"), None);
    }

    #[test]
    fn writes_the_documented_format() {
        let config = Config {
            entries: vec![
                (
                    "my-lib".into(),
                    entry("git@host:me/my-lib.git", Some("main"), Some("3d193e9")),
                ),
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
        let config = Config {
            entries: vec![("a".into(), entry("u", Some("main"), None))],
        };
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
