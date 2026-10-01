// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Command-line interface for deterministic cached Linear reads.

use clap::{Parser, Subcommand};
use ix_board::{
    DriftConfig, DriftError, DriftEvidenceFile, DriftReport, Error, FocusExpansion, FocusSelection,
    FocusStatus, LinearGraphQlSource, NextStep, ProjectChunk, ProjectPriority, ProjectRunStatus,
    ReadService, ReadyItem, SearchResult, evaluate_drift_file, render_drift_text,
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
    about = "Deterministic Linear project controls and cached reads for ready work and project context."
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
    /// Render a deterministic drift report from versioned structured evidence.
    Drift {
        /// JSON file containing thresholds and configured leads.
        #[arg(long)]
        config: PathBuf,
        /// JSON file containing source records and completeness attestations.
        #[arg(long)]
        evidence: PathBuf,
    },
    /// Resolve an inline Linear initiative/project/issue focus.
    Focus {
        #[command(subcommand)]
        command: FocusCommand,
    },
    /// Update one of the two supported Linear project fields.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
}

#[derive(Subcommand, Debug)]
enum FocusCommand {
    /// Expand selectors into focus issues and prerequisite layers.
    Expand {
        /// Linear initiative UUID or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
    },
    /// Report project and milestone status, blockers, and ready focus issues.
    Status {
        /// Linear initiative UUID or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ProjectCommand {
    /// Set the project run state to Started or Paused.
    SetStatus {
        /// Linear project identifier or UUID.
        project: String,
        /// One of: Started, Paused.
        #[arg(value_parser = parse_project_status)]
        status: ProjectRunStatus,
    },
    /// Set the project priority to a Linear priority value.
    SetPriority {
        /// Linear project identifier or UUID.
        project: String,
        /// 0=No priority, 1=Urgent, 2=High, 3=Medium, 4=Low.
        #[arg(value_parser = parse_project_priority)]
        priority: ProjectPriority,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum Output {
    Queue(Vec<ReadyItem>),
    Chunk(ProjectChunk),
    Focus(FocusExpansion),
    FocusStatus(FocusStatus),
    Search(Vec<SearchResult>),
    Drift(DriftReport),
    ProjectStatusUpdated {
        project: String,
        status: String,
    },
    ProjectPriorityUpdated {
        project: String,
        priority: i32,
        label: String,
    },
}

fn parse_project_status(value: &str) -> Result<ProjectRunStatus, String> {
    value.parse().map_err(|error: Error| error.to_string())
}

fn parse_project_priority(value: &str) -> Result<ProjectPriority, String> {
    value.parse().map_err(|error: Error| error.to_string())
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
                Error::InvalidTeam
                | Error::InvalidQuery
                | Error::ProjectNotFound { .. }
                | Error::InvalidProjectStatus { .. }
                | Error::InvalidProjectPriority { .. }
                | Error::EmptyFocus
                | Error::ScopeSelection { .. }
                | Error::DriftInputRead { .. }
                | Error::DriftInputDecode { .. }
                | Error::Drift(_) => Outcome::Invalid,
                _ => Outcome::Internal,
            })
        }
    }
}

fn validate_command(command: &Command) -> Result<(), Error> {
    match command {
        Command::Next { team } if team.trim().is_empty() => Err(Error::InvalidTeam),
        Command::Search { query } if query.trim().is_empty() => Err(Error::InvalidQuery),
        Command::Focus {
            command:
                FocusCommand::Expand {
                    initiatives,
                    projects,
                    issues,
                },
        } if initiatives.is_empty() && projects.is_empty() && issues.is_empty() => {
            Err(Error::EmptyFocus)
        }
        Command::Focus {
            command:
                FocusCommand::Status {
                    initiatives,
                    projects,
                    issues,
                },
        } if initiatives.is_empty() && projects.is_empty() && issues.is_empty() => {
            Err(Error::EmptyFocus)
        }
        Command::ProjectChunk { project } if project.trim().is_empty() => {
            Err(Error::ProjectNotFound {
                identifier: project.clone(),
            })
        }
        Command::Project {
            command:
                ProjectCommand::SetStatus { project, .. } | ProjectCommand::SetPriority { project, .. },
        } if project.trim().is_empty() => Err(Error::ProjectNotFound {
            identifier: project.clone(),
        }),
        _ => Ok(()),
    }
}

fn run(command: Command) -> Result<Output, Error> {
    match command {
        Command::Drift { config, evidence } => run_drift(&config, &evidence).map(Output::Drift),
        other => run_linear(other),
    }
}

fn run_linear(command: Command) -> Result<Output, Error> {
    let source = LinearGraphQlSource::from_env()?;
    let mut service = ReadService::open_default(source)?;
    match command {
        Command::Next { team } => service.next(&team).map(Output::Queue),
        Command::ProjectChunk { project } => service.project_chunk(&project).map(Output::Chunk),
        Command::Search { query } => service.search(&query).map(Output::Search),
        Command::Focus {
            command:
                FocusCommand::Expand {
                    initiatives,
                    projects,
                    issues,
                },
        } => service
            .focus_expand(&FocusSelection {
                initiatives,
                projects,
                issues,
            })
            .map(Output::Focus),
        Command::Focus {
            command:
                FocusCommand::Status {
                    initiatives,
                    projects,
                    issues,
                },
        } => service
            .focus_status(&FocusSelection {
                initiatives,
                projects,
                issues,
            })
            .map(Output::FocusStatus),
        Command::Drift { .. } => Err(Error::Drift(DriftError::InvalidConfig {
            reason: "drift command must use its evidence input".to_owned(),
        })),
        Command::Project {
            command: ProjectCommand::SetStatus { project, status },
        } => service
            .set_project_status(&project, status)
            .map(|()| Output::ProjectStatusUpdated {
                project,
                status: status.as_str().to_owned(),
            }),
        Command::Project {
            command: ProjectCommand::SetPriority { project, priority },
        } => service.set_project_priority(&project, priority).map(|()| {
            Output::ProjectPriorityUpdated {
                project,
                priority: priority.linear_value(),
                label: priority.label().to_owned(),
            }
        }),
    }
}

fn run_drift(config_path: &PathBuf, evidence_path: &PathBuf) -> Result<DriftReport, Error> {
    let config_text =
        std::fs::read_to_string(config_path).map_err(|source| Error::DriftInputRead {
            path: config_path.display().to_string(),
            source,
        })?;
    let config: DriftConfig =
        serde_json::from_str(&config_text).map_err(|source| Error::DriftInputDecode {
            path: config_path.display().to_string(),
            source,
        })?;
    let evidence_text =
        std::fs::read_to_string(evidence_path).map_err(|source| Error::DriftInputRead {
            path: evidence_path.display().to_string(),
            source,
        })?;
    let evidence: DriftEvidenceFile =
        serde_json::from_str(&evidence_text).map_err(|source| Error::DriftInputDecode {
            path: evidence_path.display().to_string(),
            source,
        })?;
    Ok(evaluate_drift_file(&config, &evidence)?)
}

fn render_human(output: &Output) -> String {
    match output {
        Output::Queue(items) => render_queue(items),
        Output::Chunk(chunk) => render_chunk(chunk),
        Output::Focus(expansion) => render_focus(expansion),
        Output::FocusStatus(status) => render_focus_status(status),
        Output::Search(results) => results
            .iter()
            .map(|record| format!("{:?} {} — {}", record.kind, record.identifier, record.name))
            .collect::<Vec<_>>()
            .join("\n"),
        Output::Drift(report) => render_drift_text(report),
        Output::ProjectStatusUpdated { project, status } => {
            format!("Project {project} status set to {status}")
        }
        Output::ProjectPriorityUpdated {
            project,
            priority,
            label,
        } => {
            format!("Project {project} priority set to {priority} ({label})")
        }
    }
}

fn render_focus(expansion: &FocusExpansion) -> String {
    let mut lines = vec![format!(
        "Focus snapshot fetched at {}",
        expansion.fetched_at
    )];
    lines.push("Selected projects:".to_owned());
    if expansion.projects.is_empty() {
        lines.push("  (none)".to_owned());
    }
    for project in &expansion.projects {
        lines.push(format!("  {} — {}", project.identifier, project.name));
        for milestone in &project.milestones {
            lines.push(format!(
                "    Milestone {} — {}",
                milestone.name, milestone.id
            ));
            lines.extend(milestone.issues.iter().map(|issue| {
                format!(
                    "      {} [{}] {}",
                    issue.identifier, issue.state, issue.title
                )
            }));
        }
        lines.push("    Project-level issues:".to_owned());
        lines.extend(project.project_level_issues.iter().map(|issue| {
            format!(
                "      {} [{}] {}",
                issue.identifier, issue.state, issue.title
            )
        }));
    }
    lines.push("Focus issues:".to_owned());
    lines.extend(
        expansion
            .focus_issues
            .iter()
            .map(|issue| format!("  {} [{}] {}", issue.identifier, issue.state, issue.title)),
    );
    lines.push("Issue selector issues and descendants:".to_owned());
    lines.extend(
        expansion
            .issue_selector_issues
            .iter()
            .map(|issue| format!("  {} [{}] {}", issue.identifier, issue.state, issue.title)),
    );
    lines.push("Blocker layers:".to_owned());
    if expansion.blocker_layers.is_empty() {
        lines.push("  (none)".to_owned());
    } else {
        lines.extend(
            expansion
                .blocker_layers
                .iter()
                .enumerate()
                .map(|(index, layer)| format!("  {}: {}", index, layer.join(", "))),
        );
    }
    lines.join("\n")
}

fn render_focus_status(status: &FocusStatus) -> String {
    let mut lines = vec![format!("Focus snapshot fetched at {}", status.fetched_at)];
    for project in &status.projects {
        lines.push(format!("Project {} — {}", project.identifier, project.name));
        lines.push(format!(
            "  Target date: {}",
            project.target_date.as_deref().unwrap_or("none")
        ));
        render_status_group(
            &mut lines,
            "  ",
            &project.open_by_state,
            &project.in_progress,
            project.progress,
        );
        for milestone in &project.milestones {
            lines.push(format!("  Milestone {} — {}", milestone.name, milestone.id));
            lines.push(format!(
                "    Target date: {}",
                milestone.target_date.as_deref().unwrap_or("none")
            ));
            render_status_group(
                &mut lines,
                "    ",
                &milestone.open_by_state,
                &milestone.in_progress,
                milestone.progress,
            );
        }
    }
    lines.push("Blocker layers:".to_owned());
    for (index, layer) in status.blocker_layers.iter().enumerate() {
        lines.push(format!("  {index}:"));
        lines.extend(layer.iter().map(|blocker| {
            format!(
                "    {} [{} / {:?}] roots: {} — {}",
                blocker.identifier,
                blocker.state,
                blocker.classification,
                blocker.focus_root_ids.join(", "),
                blocker.title
            )
        }));
    }
    lines.push("Ready focus issues:".to_owned());
    lines.extend(
        status
            .ready_queue
            .iter()
            .map(|item| format!("  {} — {} ({})", item.identifier, item.title, item.reason)),
    );
    lines.join("\n")
}

fn render_status_group(
    lines: &mut Vec<String>,
    indent: &str,
    open_by_state: &[ix_board::FocusStateCount],
    in_progress: &[ix_board::FocusIssue],
    progress: ix_board::FocusProgress,
) {
    lines.push(format!(
        "{indent}Progress: {}/{} leaves ({})",
        progress.done,
        progress.total,
        progress
            .completed_percent
            .map_or_else(|| "n/a".to_owned(), |percent| format!("{percent:.1}%"))
    ));
    lines.push(format!("{indent}Open by state:"));
    lines.extend(
        open_by_state
            .iter()
            .map(|count| format!("{indent}  {}: {}", count.state, count.count)),
    );
    lines.push(format!("{indent}In progress:"));
    lines.extend(in_progress.iter().map(|issue| {
        format!(
            "{indent}  {} [{}] {}",
            issue.identifier, issue.state, issue.title
        )
    }));
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

#[cfg(test)]
mod tests {
    use super::{Cli, Command, FocusCommand};
    use clap::Parser;

    /// Trace: FR-011-AC-1
    #[test]
    fn focus_expand_accepts_repeated_combined_selectors() {
        let cli = Cli::try_parse_from([
            "ix-board",
            "focus",
            "expand",
            "--initiative",
            "Delivery",
            "--project",
            "SWM-P1",
            "--project",
            "SWM-P2",
            "--issue",
            "SWM-1",
            "--issue",
            "SWM-2",
        ])
        .expect("parse repeated selectors");
        assert!(matches!(
            cli.command,
            Command::Focus {
                command: FocusCommand::Expand {
                    initiatives,
                    projects,
                    issues,
                }
            } if initiatives == ["Delivery"]
                && projects == ["SWM-P1", "SWM-P2"]
                && issues == ["SWM-1", "SWM-2"]
        ));
    }

    /// Trace: FR-012-AC-1
    #[test]
    fn focus_status_accepts_repeated_combined_selectors() {
        let cli = Cli::try_parse_from([
            "ix-board",
            "focus",
            "status",
            "--initiative",
            "Delivery",
            "--project",
            "SWM-P1",
            "--issue",
            "SWM-1",
        ])
        .expect("parse focus status selectors");
        assert!(matches!(
            cli.command,
            Command::Focus {
                command: FocusCommand::Status {
                    initiatives,
                    projects,
                    issues,
                }
            } if initiatives == ["Delivery"]
                && projects == ["SWM-P1"]
                && issues == ["SWM-1"]
        ));
    }
}
