// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! `git-multi-repo` applies git commands, clones and tasks to the git
//! repositories found in the sub-directories of the working directory,
//! optionally restricted and pinned by a `.git-multi-repo` file.

pub mod cli;
pub mod config;
