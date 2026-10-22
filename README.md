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

Note: git intercepts `git multi-repo --help` (it looks for a man page). Use
`git multi-repo -h` or `git-multi-repo --help` instead.

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

Repository names must be plain directory names (no path separators, `.` or
`..`, no leading `-`). Review a `.git-multi-repo` file from someone else
before running `clone` or `--tasks` with it.

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

**Security:** variable values are inserted into the shell command literally,
without escaping. A `.git-multi-repo` file or repository state from someone
else can therefore inject shell code (for example through a branch name).
Only use variables with files and repositories you trust; quoting such as
`'%branch%'` reduces but does not remove the risk.

Windows (`cmd /C`) is supported in principle but untested; commands containing
`"` may not be passed correctly to cmd.exe.

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

## Agent skill

`skills/git-multi-repo/` contains an [Agent Skill](https://agentskills.io)
that teaches AI coding agents how to use `git multi-repo` safely: which
commands to run, how to read the output and exit codes, and when to ask
before changing many repositories at once. The agent still needs
`git-multi-repo` installed (see above).

Install it into the shared `.agents/skills/` directory that agents
supporting Agent Skills look in. For your user, so it is available in every
project:

    mkdir -p ~/.agents/skills
    cp -r skills/git-multi-repo ~/.agents/skills/

or only for one workspace, in the directory that contains your repositories:

    mkdir -p .agents/skills
    cp -r /path/to/git-multi-repo/skills/git-multi-repo .agents/skills/

On Windows (PowerShell):

    New-Item -ItemType Directory -Force "$HOME\.agents\skills" | Out-Null
    Copy-Item -Recurse skills\git-multi-repo "$HOME\.agents\skills\"

Some agents use their own directory instead; copy the same folder there,
for example `~/.claude/skills/` (or `.claude/skills/`) for Claude Code. Check
your agent's documentation if the skill is not picked up.

Start a new agent session afterwards. To update the skill, copy the folder
again.

## Packaging

    ./deploy.sh

Checks formatting, lints and tests, builds a release binary and writes
`dist/git-multi-repo-<version>-<target>.tar.gz` (binary, README, LICENSE) for
the host platform, plus a `.sha256` file that can be verified with
`sha256sum -c`. The working tree must be clean.

If [cargo-deb](https://github.com/kornelski/cargo-deb) is installed
(`cargo install cargo-deb`), it also writes a Debian package
`dist/git-multi-repo_<version>-1_<arch>.deb` (plus `.sha256`), which installs
the binary to `/usr/bin` and depends on `git`:

    sudo apt install ./dist/git-multi-repo_0.1.0-1_amd64.deb

On Windows, use the PowerShell script instead:

    powershell -ExecutionPolicy Bypass -File deploy.ps1

It runs the same checks and writes
`dist/git-multi-repo-<version>-<target>.zip` plus a `.sha256` file in the same
format.

If the [WiX Toolset](https://wixtoolset.org) v5 is installed
(`dotnet tool install --global wix --version 5.0.2`), it also writes an MSI
installer `dist/git-multi-repo-<version>-<target>.msi` (plus `.sha256`), which
installs the binary to `C:\Program Files\git-multi-repo` and adds that
directory to the system `PATH`:

    msiexec /i dist\git-multi-repo-0.9.0-x86_64-pc-windows-msvc.msi

The GitHub Actions workflow `.github/workflows/build.yml` runs the same
checks and builds archives for Linux (`.tar.gz`, `.deb`) and Windows (`.zip`, `.msi`) on every
push to `main`, every tag `v*` and every pull request. The archives are
attached to the workflow run as artifacts. For a tag `v*`, they are also
published to a GitHub release with generated release notes:

    git tag v0.1.0 && git push origin v0.1.0

## License

This software is available under the MIT license.
