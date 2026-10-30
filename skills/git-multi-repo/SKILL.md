---
name: git-multi-repo
description: Run git commands, clones and scripted tasks across many git repositories at once with the `git multi-repo` CLI, and manage its `.git-multi-repo` manifest (persist, pin, unpin, remove). Use this skill whenever the user wants to do something in several repositories that sit side by side in one directory — fetch/pull/status/checkout all of them, find which repos are dirty or behind, clone a set of repos from a manifest, pin or freeze the commits of a multi-repo workspace, or run a build/test command in every repo — even if they don't name the tool, and whenever a `.git-multi-repo` file or a git-multi-repo tasks JSON file is involved.
---

# git-multi-repo

`git multi-repo` (binary `git-multi-repo`) runs one action in every git
repository that is a direct sub-directory of the current working directory.
Always run it from the **parent** directory that contains the repositories,
not from inside one of them.

Check that it is installed with `git-multi-repo --version`. If it is missing,
tell the user rather than looping over repositories by hand: the tool handles
the manifest, ordering and exit codes that an ad-hoc loop would get wrong.
(`git multi-repo --help` opens a man-page lookup in git; use
`git-multi-repo --help` for help text.)

## Which repositories are used

- Without a `.git-multi-repo` file: every sub-directory that is a git repository.
- With a `.git-multi-repo` file: only the repositories listed in it, in file
  order. If that file is invalid, nothing runs (exit code 2).
- `--omit <regex>` skips repositories. Every regex in this tool must match
  the **whole** directory name: `--omit test` skips `test` but not
  `my-test-repo`; write `'.*test.*'` for that. Quote regexes for the shell.

## Commands

Options always come **before** the git command; everything after the first
non-option word is passed to git unchanged.

| Goal | Command |
|---|---|
| Run a git command everywhere | `git multi-repo fetch --prune` |
| Skip some repos | `git multi-repo --omit 'legacy-.*' status --short` |
| Run concurrently | `git multi-repo --parallel [--jobs 4] pull --ff-only` |
| Clone everything in the manifest | `git multi-repo --clone` |
| Clone onto a specific branch | `git multi-repo --clone --branch <name>` |
| Clone from a given manifest file | `git multi-repo --clone --from <file>` |
| Hard-reset to manifest commits/branches | `git multi-repo --reset` |
| Hard-reset to a specific branch | `git multi-repo --reset --branch <name>` |
| Run a tasks file in every repo | `git multi-repo --tasks tasks.json` |
| Write manifest (url + branch) | `git multi-repo --persist ['<regex>']` |
| Write manifest with commits pinned | `git multi-repo --pin ['<regex>']` |
| Drop pins from manifest entries | `git multi-repo --unpin ['<regex>']` |
| Remove manifest entries | `git multi-repo --remove ['<regex>']` |

`--omit` cannot be combined with `--remove` or `--unpin`; `--parallel` and
`--jobs` only apply to git commands, `--clone`, `--reset` and `--tasks`.
`--branch` is only valid with `--clone` or `--reset`; `--from` is only valid
with `--clone`.

### Running as an agent

- Prefer `--parallel` for read-only or non-interactive commands. In parallel
  mode the commands cannot read from the terminal, so a credential or editor
  prompt fails instead of hanging, and each repository's output arrives as
  one block, which is easier to attribute.
- Avoid commands that open an editor or pager without arguments that disable
  it (for example use `commit -m`, `merge --no-edit`, `--no-pager log`).
- Each repository's output starts with a `── <name> ──` header line, and the
  run ends with a `── summary ──` block listing every repository as `ok` or
  failed. Use the summary to report per-repository results, especially which
  ones failed, and the blocks above it for the details.
- Exit codes: `0` everything succeeded, `1` at least one repository command or
  task failed (the others still ran), `2` usage error, invalid regex, invalid
  `.git-multi-repo` or tasks file. On `1`, find the failing repositories in the
  output instead of re-running everything.

### Useful recipes

- Which repos have local changes: `git multi-repo --parallel status --short`
  (repositories with no output lines after their name are clean).
- Which repos are behind or ahead: `git multi-repo --parallel fetch` then
  `git multi-repo --parallel status -sb`.
- Current branch everywhere: `git multi-repo --parallel branch --show-current`.
- Safe update: `git multi-repo --parallel pull --ff-only` (never creates merge
  commits; repos that cannot fast-forward fail and are reported).
- New release workspace from a previous one:
  `git multi-repo --clone --from <previous-workspace> --branch <release-branch>`
  (the first part is a `.git-multi-repo` file from an earlier release, the
  second the branch of the new release; `--branch` overrides each entry's
  branch and drops its pinned commit).

## The `.git-multi-repo` manifest

A TOML file in the parent directory, one table per repository directory:

```toml
[my-lib]
url = "git@github.com:me/my-lib.git"
branch = "main"
commit = "3d193e9c0a5b..."   # only present after --pin
```

Repository names must be plain directory names (no `/`, `\`, `.` or `..`, no
leading `-`).

- `--persist` and `--pin` **overwrite** the whole file from the current state
  of the repositories on disk; existing pins are dropped by `--persist`. If the
  user has hand-edited entries or pins they want to keep, say so before
  running these, or edit the file directly instead. Repositories without a
  remote are skipped with a warning, since a manifest entry needs a `url`.
- `--remove` deletes entries (and the file once it is empty); it never touches
  repositories on disk.
- `--clone` needs a manifest. It clones every entry on its `branch`, checks
  out pinned entries at their `commit` (detached HEAD) and skips directories
  that already exist. It takes no extra arguments.
  - With `--branch <name>`: clones every entry on that branch instead of the
    entry's branch and ignores the entry's pinned commit.
  - With `--from <file>`: reads the entries from the given `.git-multi-repo`
    file instead of the one in the working directory.
- `--reset` needs a manifest and runs `git reset --hard` in every entry: to its
  pinned `commit`, else to the head of its `branch`. This **discards local
  changes**, so ask before running it. It takes no extra arguments; for an
  ordinary reset use `git multi-repo reset <args>`.
  - With `--branch <name>`: resets every entry to that branch instead of the
    entry's commit or branch.

## Tasks files

`--tasks <file.json>` runs shell commands in every repository, in dependency
order:

```json
[
  { "task": "fetch", "command": "git fetch --prune" },
  { "task": "build", "dependsOn": "fetch", "command": "make" },
  { "task": "report", "dependsOn": "build", "command": "echo %name% %branch% %commit%" }
]
```

- Commands run through `sh -c` (Unix) or `cmd /C` (Windows) inside each
  repository directory.
- When a task fails, tasks that depend on it are skipped for that repository;
  independent tasks still run.
- Variables: `%name%`, `%url%`, `%branch%`, `%commit%`, taken from the
  manifest entry or else from the repository (`origin` url, current branch,
  `HEAD`).

When you write a tasks file for the user, put it in a temporary directory or a path
they choose, not inside a repository, and give each task a short `task` name.

## Safety

- A tasks file runs arbitrary shell code, and variable values are inserted
  **without escaping**, so a manifest or repository from someone else can
  inject commands through a branch name or URL. Before running `--clone` or
  `--tasks` with a `.git-multi-repo` or tasks file you did not write, read it
  and show the user anything unexpected (unfamiliar URLs, odd names, commands
  that delete or upload things).
- A git command runs in **every** selected repository. For anything that
  changes or discards state (`reset --hard`, `clean -fd`, `push --force`,
  `checkout -- .`, `branch -D`, `stash drop`), first show which repositories
  it would hit (for example with `status --short`) and get the user's
  confirmation; narrow it with `--omit` where possible.
- `push` publishes to remotes for every repository; confirm before running it.
