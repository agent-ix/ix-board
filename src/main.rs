// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Command-line interface for deterministic cached Linear reads.

use clap::{Parser, Subcommand};
use ix_board::{
    Error, LinearGraphQlSource, NextStep, ProjectChunk, ReadService, ReadyItem, SearchResult,
};
use ix_cli_kit::exit::Outcome;
use ix_cli_kit::streams::{
    ColorChoice, Diagnostics, DiagnosticsFormat, emit_diagnostic, emit_result,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "ix-board",
    version,
    about = "Deterministic, cached Linear reads for ready work, project context, and search."
)]
struct Cli {
    /// Emit machine-readable JSON on stdout instead of human-readable text.
    #[arg(long, global = true)]
    json: bool,

    /// Diagnostic stream format on stderr.
    #[arg(long, value_name = "FORMAT", default_value = "human", global = true)]
    diagnostics_format: DiagnosticsFormat,

    /// Colorize human diagnostics on stderr: auto (TTY only), always, or never.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    color: ColorChoice,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show the deterministic ready queue for one Linear team.
    Next {
        /// Linear team key.
        #[arg(long)]
        team: String,
    },
    /// Return a project's goal, milestones, open tickets, and blocker layers.
    ProjectChunk {
        /// Linear project identifier or UUID.
        project: String,
    },
    /// Search structured project and issue fields.
    Search {
        /// Case-insensitive substring query.
        query: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum Output {
    Queue(Vec<ReadyItem>),
    Chunk(ProjectChunk),
    Search(Vec<SearchResult>),
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let diagnostics = Diagnostics::resolved(cli.diagnostics_format, cli.color);
    if let Err(error) = validate_command(&cli.command) {
        emit_diagnostic(diagnostics, "error", &error.to_string());
        return std::process::ExitCode::from(Outcome::Invalid);
    }
    match run(cli.command) {
        Ok(output) => {
            let rendered = if cli.json {
                ix_cli_kit::json::encode_canonical(&output, true).map_err(|error| error.to_string())
            } else {
                Ok(render_human(&output))
            };
            match rendered {
                Ok(text) => {
                    emit_result(&text);
                    std::process::ExitCode::from(Outcome::Ok)
                }
                Err(error) => {
                    emit_diagnostic(
                        diagnostics,
                        "error",
                        &format!("could not encode result: {error}"),
                    );
                    std::process::ExitCode::from(Outcome::Internal)
                }
            }
        }
        Err(error) => {
            emit_diagnostic(diagnostics, "error", &error.to_string());
            std::process::ExitCode::from(match error {
                Error::InvalidTeam | Error::InvalidQuery | Error::ProjectNotFound { .. } => {
                    Outcome::Invalid
                }
                _ => Outcome::Internal,
            })
        }
    }
}

fn validate_command(command: &Command) -> Result<(), Error> {
    match command {
        Command::Next { team } if team.trim().is_empty() => Err(Error::InvalidTeam),
        Command::Search { query } if query.trim().is_empty() => Err(Error::InvalidQuery),
        Command::ProjectChunk { project } if project.trim().is_empty() => {
            Err(Error::ProjectNotFound {
                identifier: project.clone(),
            })
        }
        _ => Ok(()),
    }
}

fn run(command: Command) -> Result<Output, Error> {
    let source = LinearGraphQlSource::from_env()?;
    let cache_path = cache_path()?;
    let mut service = ReadService::open(source, &cache_path)?;
    match command {
        Command::Next { team } => service.next(&team).map(Output::Queue),
        Command::ProjectChunk { project } => service.project_chunk(&project).map(Output::Chunk),
        Command::Search { query } => service.search(&query).map(Output::Search),
    }
}

fn cache_path() -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("IX_BOARD_CACHE_PATH") {
        return Ok(PathBuf::from(path));
    }
    let Some(project_dirs) = directories::ProjectDirs::from("com", "agent-ix", "ix-board") else {
        return Err(Error::CacheDirectoryUnavailable);
    };
    std::fs::create_dir_all(project_dirs.cache_dir()).map_err(Error::CacheDirectory)?;
    Ok(project_dirs.cache_dir().join("linear.sqlite3"))
}

fn render_human(output: &Output) -> String {
    match output {
        Output::Queue(items) => render_queue(items),
        Output::Chunk(chunk) => render_chunk(chunk),
        Output::Search(results) => results
            .iter()
            .map(|record| format!("{:?} {} — {}", record.kind, record.identifier, record.name))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn render_queue(items: &[ReadyItem]) -> String {
    items
        .iter()
        .map(|item| {
            let step = match item.next_step {
                NextStep::Specify => "Specify",
                NextStep::Coding => "Coding",
            };
            format!(
                "{} [{}] {} — {}",
                item.identifier, step, item.title, item.reason
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_chunk(chunk: &ProjectChunk) -> String {
    let mut lines = vec![
        format!("{} — {}", chunk.identifier, chunk.name),
        format!("Goal: {}", chunk.goal),
        "Milestones:".to_owned(),
    ];
    if chunk.milestones.is_empty() {
        lines.push("  (none)".to_owned());
    } else {
        lines.extend(
            chunk
                .milestones
                .iter()
                .map(|milestone| format!("  - {}", milestone.name)),
        );
    }
    lines.push("Open tickets:".to_owned());
    if chunk.open_tickets.is_empty() {
        lines.push("  (none)".to_owned());
    } else {
        lines.extend(
            chunk
                .open_tickets
                .iter()
                .map(|issue| format!("  - {} {} — {}", issue.identifier, issue.state, issue.title)),
        );
    }
    lines.push("Blocker layers:".to_owned());
    if chunk.blocker_layers.is_empty() {
        lines.push("  (none)".to_owned());
    } else {
        lines.extend(
            chunk
                .blocker_layers
                .iter()
                .enumerate()
                .map(|(index, layer)| format!("  {}: {}", index, layer.join(", "))),
        );
    }
    lines.join("\n")
}
