# Security Policy

## Supported versions

Only the latest release of git-multi-repo receives security fixes. Please
check that a problem still exists in the latest release before you report it.

| Version | Supported |
|---|---|
| latest release (currently 0.9.x) | yes |
| older releases | no |

## Reporting a vulnerability

Please do **not** report security problems in public issues, pull requests or
discussions.

Report them privately through GitHub instead:
[Report a vulnerability](https://github.com/tomaki19/git-multi-repo/security/advisories/new)
(or the **Security** tab, then **Report a vulnerability**).

Please include:

- the git-multi-repo version (`git-multi-repo --version`) and your operating system
- what an attacker can do, and what they must control to do it (for example a
  `.git-multi-repo` file, a tasks file, a cloned repository or a branch name)
- steps or files to reproduce the problem

## What to expect

git-multi-repo is maintained by one person in their spare time, so these are
goals, not guarantees:

- a first reply within 7 days
- a fix or a plan within 30 days of a confirmed report
- a published GitHub security advisory once a fixed release is available, crediting
  you unless you prefer not to be named

## Scope

In scope, for example:

- running a command or shell code that the user did not write or approve, other
  than the known risk described below
- writing, overwriting or deleting files outside the working directory and its
  repositories, for example through a repository name in a `.git-multi-repo` file
- problems in the release packages (`.tar.gz`, `.zip`, `.deb`, `.msi`) or the
  release workflow

Known and documented, so not a vulnerability by itself:

- tasks files (`--tasks`) run arbitrary shell commands. That is their purpose,
  so only run tasks files you trust.
- variables such as `%branch%` or `%url%` are inserted into task commands
  without escaping. A `.git-multi-repo` file or repository from someone else
  can inject shell code through them (see **Security** under *Tasks* in the
  [README](../README.md)).
- git itself runs hooks, credential helpers and other configured programs.
  Vulnerabilities in git belong to the [git project](https://git-scm.com/community).
