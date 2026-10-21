# git-multi-repo — Design

Date: 2026-10-21
Status: Approved design, pending implementation plan

## Purpose

`git-multi-repo` is a git plugin (invoked as `git multi-repo …`) that applies git
commands and user-defined tasks to multiple git repositories located in the
sub-directories of the current working directory. It replaces the Python script
in `git-multi-repo-old` and is written in Rust.

Compared to the old tool it adds: regex-based selection, a TOML repository file
with branch and optional commit pinning, pin/unpin management, a JSON task runner
with dependencies and variables, and optional parallel execution.

## Approach

- Single Rust binary named `git-multi-repo`, so git picks it up as the
  `multi-repo` subcommand when it is on `PATH`.
- All git interaction happens by invoking the `git` executable as a subprocess
  (`git -C <dir> …`). This honours the user's credentials, SSH config, hooks,
  aliases and git version. No libgit2 / gix.
- Parallelism uses a small `std::thread` worker pool. No async runtime.

## 1. CLI and repository selection

```
git-multi-repo [--omit <re>] [--parallel] [--jobs <n>] <git-command> [args...]
git-multi-repo [--omit <re>] [--parallel] [--jobs <n>] clone [args...]
git-multi-repo [--omit <re>] [--parallel] [--jobs <n>] --tasks <file.json>
git-multi-repo [--omit <re>] --persist [<re>]
git-multi-repo [--omit <re>] --pin [<re>]
git-multi-repo --remove [<re>]
git-multi-repo --unpin [<re>]
```

### Rules

- **One action per invocation.** `--persist`, `--pin`, `--remove`, `--unpin` and
  `--tasks` are mutually exclusive with each other and with a git command.
  Violations are usage errors (exit code 2).
- `--parallel` and `--jobs` are valid only with a git command, `clone` or
  `--tasks`. `--jobs` implies nothing on its own; it only sets the pool size when
  `--parallel` is given. Default pool size: `std::thread::available_parallelism()`.
- `--omit` is valid with every action except `--remove` and `--unpin`.
- **Regex matching is full-match.** Every user pattern is compiled as
  `^(?:<pattern>)$` and matched against the directory name. `--omit test`
  matches only `test`; use `.*test.*` for substring matching. An invalid regex is
  a usage error (exit code 2).

### Repository selection

1. **No `.git-multi-repo` file:** candidates are the immediate sub-directories of
   the working directory that contain a `.git` entry (directory or file, so
   worktrees and submodule checkouts count), sorted by name.
2. **Valid `.git-multi-repo` file:** candidates are the file's entries, in file
   order. For all actions except `clone`, entries whose directory does not exist
   are skipped with a warning.
3. **Invalid `.git-multi-repo` file:** abort with exit code 2 and an error naming
   the file and the problem. Nothing runs.
4. `--omit <re>` then removes matching candidates.

Exception: `--persist` and `--pin` always scan live sub-directories (rule 1),
regardless of whether a file exists, because they (re)create the file.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Everything succeeded |
| 1 | At least one repository command or task failed (the run always continues past failures) |
| 2 | Usage error, invalid regex, invalid `.git-multi-repo`, invalid tasks file |

## 2. The `.git-multi-repo` file

### Format

TOML, one table per repository, keyed by directory name. Entry order is preserved
on read and write.

```toml
["my-lib"]
url = "git@github.com:me/my-lib.git"
branch = "main"
commit = "3d193e9c0a5b…"   # optional, only written by --pin
```

### Validity

A file is valid if and only if:

- it parses as TOML;
- every top-level value is a table;
- every table has a non-empty string `url`;
- `branch` and `commit`, if present, are strings;
- no table contains any other key.

### Reading live repository state

| Field | Source | Fallback |
|---|---|---|
| `url` | `git remote get-url origin` | first remote from `git remote`; if none, the repo is skipped with a warning |
| `branch` | `git symbolic-ref --short HEAD` | detached HEAD: branch from the existing file entry if any, otherwise `branch` is omitted |
| `commit` | `git rev-parse HEAD` (full SHA) | — |

### Actions

- **`--persist [<re>]`**: scan live sub-directories, keep those matching `<re>`
  (all if omitted), apply `--omit`, and **overwrite** the file with `url` and
  `branch` for each. Existing `commit` pins are therefore dropped.
- **`--pin [<re>]`**: same as `--persist` but also writes `commit`. Also
  overwrites the file.
- **Empty-result guard:** if `--persist` or `--pin` would write zero entries, the
  existing file is left untouched and the command fails with exit code 2.
- **`--remove [<re>]`**: delete matching entries from the file. Without a regex,
  all entries are removed and the file is deleted. Repositories on disk are never
  touched. If no file exists: exit code 2.
- **`--unpin [<re>]`**: remove the `commit` key from matching entries (all if
  omitted). If no file exists: exit code 2.
- **Atomic writes:** the file is written to a temporary file in the same directory
  and renamed over the target.

### `clone`

- **With a valid file:** for each selected entry run
  `git clone [--branch <branch>] [extra args…] <url> <dir>`; if the entry has
  `commit`, then run `git -C <dir> checkout --detach <commit>`. If `<dir>`
  already exists, the entry is skipped with a note (not a failure). Extra
  arguments (e.g. `--depth 1`) are passed to every clone; if a shallow clone
  makes the pinned checkout fail, that repository is reported as failed and the
  run continues.
- **Without a file:** `git clone <args…>` is forwarded unchanged, once, in the
  working directory. `--omit`, `--parallel` and `--jobs` have no effect.

### Other git commands

For each selected repository run `git -C <dir> <git-command> [args…]`. All
arguments after the git command are forwarded verbatim (no re-quoting through a
shell).

## 3. Tasks

### File format

A JSON array of task objects:

```json
[
  { "task": "fetch",  "command": "git fetch --prune" },
  { "task": "build",  "dependsOn": "fetch", "command": "cargo build" },
  { "task": "report", "dependsOn": "build", "command": "echo %name% %branch% %commit% %url%" }
]
```

| Field | Type | Required |
|---|---|---|
| `task` | string, unique | yes |
| `dependsOn` | string, name of another task | no |
| `command` | string | yes |

Unknown fields are rejected.

### Validation (before anything runs; violations → exit code 2)

- file is readable, valid JSON, and a non-empty array of task objects;
- task names are unique;
- every `dependsOn` refers to an existing task;
- the dependency graph has no cycles.

### Execution within a repository

- Tasks run sequentially in a topological order that preserves file order
  wherever dependencies allow.
- If a task fails (non-zero exit), all tasks that depend on it, transitively, are
  skipped for that repository and reported as `skipped`. Independent tasks still
  run.
- Commands run through the shell with the repository directory as the working
  directory: `sh -c <command>` on Unix, `cmd /C <command>` on Windows.

### Variables

Substituted in `command` before execution:

| Variable | Value |
|---|---|
| `%name%` | directory name |
| `%url%` | file entry `url` if present, else live `url` |
| `%branch%` | file entry `branch` if present, else live branch |
| `%commit%` | file entry `commit` if present, else live `HEAD` |

A value that cannot be determined becomes an empty string and emits a warning.
Unknown `%foo%` tokens are left unchanged. Values are substituted literally
(no shell escaping).

## 4. Execution model (git commands, clone, tasks)

- **Sequential (default):** print a header `── <name> ──` before each repository,
  stream its output live, and inherit stdin so prompts and editors work.
- **`--parallel [--jobs <n>]`:** a worker pool of `n` threads. Each repository's
  stdout/stderr is captured and printed as one block (header + output) when that
  repository finishes. stdin is null, so commands needing input fail rather than
  hang. Within one repository, tasks still run sequentially in dependency order.
- **Summary:** after the run, list every repository with `ok`,
  `failed (exit N)` or `skipped`, and for task runs the names of failed and
  skipped tasks.

## 5. Project layout

Binary crate with a thin `main.rs` over a library crate for testability.

| Module | Responsibility |
|---|---|
| `cli.rs` | `clap` derive definitions; action exclusivity validation |
| `config.rs` | `.git-multi-repo` model: load, validate, atomic save, order preservation |
| `git.rs` | git subprocess wrapper: query url/branch/commit, run commands |
| `select.rs` | sub-directory discovery, file-vs-scan selection, full-match regex and `--omit` |
| `tasks.rs` | tasks JSON parsing, validation, topological ordering, variable substitution |
| `runner.rs` | sequential/parallel execution, output buffering, results, summary |
| `actions.rs` | one function per action: passthrough, clone, persist, pin, remove, unpin, tasks |

Dependencies: `clap` (derive), `regex`, `serde`, `toml`, `serde_json`, `anyhow`.
Dev dependencies: `assert_cmd`, `predicates`, `tempfile`.

Also: `README.md` with usage and install instructions, MIT `LICENSE` (carried over
from `git-multi-repo-old`), SPDX headers in source files.

## 6. Testing

### Unit tests

- config: each validity rule (valid file, bad TOML, non-table value, missing or
  empty `url`, wrong types, unknown key); order preservation on round-trip.
- select: full-match regex semantics; `--omit`; file vs. scan; missing directories
  skipped.
- tasks: duplicate names, unknown `dependsOn`, cycles, empty array, unknown fields;
  topological order preserving file order; transitive skip after failure;
  variable substitution from file vs. live values; unknown tokens unchanged.

### Integration tests (`tests/`)

Run the real binary in temporary directories, using real repositories created
with `git init` and bare repositories as `file://` remotes (no network):

- passthrough on scanned repositories and on file-listed repositories;
- `--omit`;
- `--persist` / `--pin` overwrite semantics and the empty-result guard;
- `--remove` (partial and full, file deletion) and `--unpin`;
- `clone` with a file, with and without a pin (assert detached HEAD at the pinned
  SHA), existing directory skipped, and passthrough without a file;
- `--tasks` sequential and `--parallel`, including dependency skip on failure;
- exit codes 0, 1 and 2 (including invalid `.git-multi-repo` aborting the run).

## Out of scope

- Recursive discovery below the immediate sub-directories.
- Multiple `dependsOn` values per task.
- Shell escaping of substituted variables.
- Any network or hosting-provider integration beyond what `git` itself does.
