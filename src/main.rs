// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! `ix-board` binary entry point.
//!
//! Scaffold only: the root `clap::Parser` wires the global stream flags
//! (`--json`, `--diagnostics-format`, `--color`) through `ix_cli_kit::streams`
//! and reports the process exit status through `ix_cli_kit::exit::Outcome`.
//! No subcommand exists yet.

use clap::{Parser, Subcommand};
use ix_cli_kit::exit::Outcome;
use ix_cli_kit::streams::{ColorChoice, Diagnostics, DiagnosticsFormat};

#[derive(Parser, Debug)]
#[command(
    name = "ix-board",
    version,
    about = "Keeps the Linear dependency graph honest: resolves dependencies written in ticket prose, reports every reference it cannot resolve, and never silently drops one."
)]
struct Cli {
    /// Emit machine-readable JSON on stdout instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    /// Diagnostic stream format on stderr.
    #[arg(long, value_name = "FORMAT", default_value = "human", global = true)]
    diagnostics_format: DiagnosticsFormat,

    /// Colorize human diagnostics on stderr: auto (TTY only, honours
    /// NO_COLOR), always, or never.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    color: ColorChoice,

    #[command(subcommand)]
    command: Option<Command>,
}

/// No subcommands exist yet. This is scaffold only.
#[derive(Subcommand, Debug)]
enum Command {}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let _diagnostics = Diagnostics::resolved(cli.diagnostics_format, cli.color);
    let _json = cli.json;

    match cli.command {
        // No subcommands exist yet; a bare invocation is a no-op success.
        None => std::process::ExitCode::from(Outcome::Ok),
    }
}
