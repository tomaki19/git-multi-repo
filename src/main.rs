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
