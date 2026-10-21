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
