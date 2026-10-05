// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Command-line interface for deterministic cached Linear reads.

use clap::{Parser, Subcommand};
use ix_board::{
    DriftConfig, DriftError, DriftEvidenceFile, DriftReport, Error, FocusExpansion,
    FocusHygieneReport, FocusPlanChunk, FocusSelection, FocusStatus, LinearGraphQlSource, NextStep,
    ProjectChunk, ProjectPriority, ProjectRunStatus, ReadService, ReadyItem, SearchResult,
    evaluate_drift_file, render_drift_text,
};
use ix_cli_kit::exit::Outcome;
use ix_cli_kit::secrets::{AppScope, SecretKey, SecretStore, SecretValue};
use ix_cli_kit::streams::{
    ColorChoice, Diagnostics, DiagnosticsFormat, emit_diagnostic, emit_result,
};
use serde::Serialize;
use std::io::Read;
use std::num::NonZeroUsize;
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
    /// Manage the Linear API key in the operating system credential store.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
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
enum AuthCommand {
    /// Save a Linear API key; prompt without echoing by default.
    Login {
        /// Read the key from stdin for scripts instead of prompting.
        #[arg(long)]
        stdin: bool,
    },
    /// Report the active key source without displaying the key.
    Status,
    /// Remove the saved key from the OS credential store.
    Logout,
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
    /// Prepare a bounded dependency-ordered handoff chunk for a focus.
    PlanChunk {
        /// Linear initiative UUID or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
        /// Maximum number of open focus issues to emit.
        #[arg(long, value_parser = parse_positive_size)]
        size: Option<NonZeroUsize>,
    },
    /// Report hygiene findings and changes for a focus.
    Hygiene {
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
        /// Stale threshold in positive whole days.
        #[arg(long, default_value_t = 30, value_parser = parse_positive_days)]
        stale_days: u32,
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
    Auth {
        status: String,
        source: Option<String>,
    },
    Queue(Vec<ReadyItem>),
    Chunk(ProjectChunk),
    Focus(FocusExpansion),
    FocusStatus(FocusStatus),
    FocusPlanChunk(FocusPlanChunk),
    FocusHygiene(Box<FocusHygieneReport>),
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

fn parse_positive_days(value: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .ok()
        .filter(|days| *days > 0)
        .ok_or_else(|| "stale days must be a positive whole number".to_owned())
}

fn parse_positive_size(value: &str) -> Result<NonZeroUsize, String> {
    value
        .parse::<NonZeroUsize>()
        .map_err(|_| "size must be a positive whole number".to_owned())
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
                Error::EmptyCredentialInput => Outcome::Invalid,
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
                FocusCommand::Hygiene {
                    initiatives,
                    projects,
                    issues,
                    ..
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
        Command::Auth { command } => run_auth(command),
        Command::Drift { config, evidence } => run_drift(&config, &evidence).map(Output::Drift),
        other => run_linear(other),
    }
}

fn run_auth(command: AuthCommand) -> Result<Output, Error> {
    let store = SecretStore::system();
    let scope = AppScope::try_from("ix-board")?;
    let key = SecretKey::try_from("linear-api-key")?;
    match command {
        AuthCommand::Login { stdin } => {
            let value = if stdin {
                let mut value = String::new();
                std::io::stdin()
                    .read_to_string(&mut value)
                    .map_err(Error::CredentialInput)?;
                value.trim_end_matches(['\r', '\n']).to_owned()
            } else {
                rpassword::prompt_password("Linear API key: ").map_err(Error::CredentialInput)?
            };
            if value.trim().is_empty() {
                return Err(Error::EmptyCredentialInput);
            }
            store.set(&scope, &key, &SecretValue::new(value))?;
            Ok(Output::Auth {
                status: "saved".to_owned(),
                source: Some("credential_store".to_owned()),
            })
        }
        AuthCommand::Status => {
            let source = store
                .resolve(None, Some("LINEAR_API_KEY"), &scope, &key)?
                .map(|resolved| resolved.source.as_str().to_owned());
            Ok(Output::Auth {
                status: if source.is_some() { "ready" } else { "missing" }.to_owned(),
                source,
            })
        }
        AuthCommand::Logout => {
            store.delete(&scope, &key)?;
            Ok(Output::Auth {
                status: "removed".to_owned(),
                source: None,
            })
        }
    }
}

fn run_linear(command: Command) -> Result<Output, Error> {
    let source = LinearGraphQlSource::from_env()?;
    let mut service = ReadService::open_default(source)?;
    match command {
        Command::Auth { .. } => unreachable!("auth commands are handled before Linear dispatch"),
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
        Command::Focus {
            command:
                FocusCommand::PlanChunk {
                    initiatives,
                    projects,
                    issues,
                    size,
                },
        } => service
            .focus_plan_chunk(
                &FocusSelection {
                    initiatives,
                    projects,
                    issues,
                },
                size,
            )
            .map(Output::FocusPlanChunk),
        Command::Focus {
            command:
                FocusCommand::Hygiene {
                    initiatives,
                    projects,
                    issues,
                    stale_days,
                },
        } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| {
                    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
                });
            service
                .focus_hygiene(
                    &FocusSelection {
                        initiatives,
                        projects,
                        issues,
                    },
                    &format_utc(now),
                    stale_days,
                )
                .map(|report| Output::FocusHygiene(Box::new(report)))
        }
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
        Output::Auth { status, source } => match source {
            Some(source) => format!("Linear credential {status} ({source})"),
            None => format!("Linear credential {status}"),
        },
        Output::Queue(items) => render_queue(items),
        Output::Chunk(chunk) => render_chunk(chunk),
        Output::Focus(expansion) => render_focus(expansion),
        Output::FocusStatus(status) => render_focus_status(status),
        Output::FocusPlanChunk(chunk) => render_focus_plan_chunk(chunk),
        Output::FocusHygiene(report) => render_focus_hygiene(report),
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
        format_snapshot_time(expansion.fetched_at)
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
    let mut lines = vec![format!(
        "Focus snapshot fetched at {}",
        format_snapshot_time(status.fetched_at)
    )];
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
        lines.push("  No milestone:".to_owned());
        render_status_group(
            &mut lines,
            "    ",
            &project.unmilestoned.open_by_state,
            &project.unmilestoned.in_progress,
            project.unmilestoned.progress,
        );
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

fn render_focus_plan_chunk(chunk: &FocusPlanChunk) -> String {
    let mut lines = vec![format!(
        "Focus snapshot fetched at {}",
        format_snapshot_time(chunk.fetched_at)
    )];
    lines.push("Plan chunk:".to_owned());
    if chunk.issues.is_empty() {
        lines.push("  (no open focus issues)".to_owned());
    }
    for issue in &chunk.issues {
        let milestone = issue.milestone.as_deref().unwrap_or("unmilestoned");
        lines.push(format!(
            "  {} [layer {}] — {} ({milestone}; ready: {})",
            issue.identifier, issue.dependency_layer, issue.title, issue.ready
        ));
        lines.push(format!("    reasons: {}", issue.reasons.join("; ")));
        lines.push(format!("    unblocks: {}", issue.unblocks.join(", ")));
        if let Some(excerpt) = &issue.acceptance_criteria {
            lines.push(format!("    acceptance criteria: {excerpt}"));
        }
    }
    lines.join("\n")
}

fn render_focus_hygiene(report: &FocusHygieneReport) -> String {
    let mut lines = vec![format!(
        "Focus hygiene report at {} (stale > {} days)",
        report.report_time, report.stale_days
    )];
    macro_rules! group_header {
        ($name:literal, $group:expr) => {
            lines.push(format!("{}: {:?}", $name, $group.availability));
            for reason in &$group.reasons {
                lines.push(format!("  unavailable: {reason}"));
            }
        };
    }
    group_header!("Missing scope", report.missing_scope);
    for finding in &report.missing_scope.findings {
        lines.push(format!(
            "  {}: missing {}",
            issue_label(report, &finding.issue_id),
            finding.missing_fields.join(", ")
        ));
    }
    group_header!("Overdue milestones", report.overdue_milestones);
    for finding in &report.overdue_milestones.findings {
        lines.push(format!(
            "  {} {} due {}: {}",
            finding.project_id.0,
            finding.name,
            finding.target_date,
            finding
                .open_issue_ids
                .iter()
                .map(|id| issue_label(report, id))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    group_header!("Pull requests", report.pull_requests);
    for finding in &report.pull_requests.findings {
        lines.push(format!(
            "  {} {}#{} {}",
            issue_label(report, &finding.issue_id),
            finding.repository,
            finding.number,
            finding.kind.as_str()
        ));
    }
    group_header!("Stale issues", report.stale_issues);
    for finding in &report.stale_issues.findings {
        lines.push(format!(
            "  {} last updated {} ({} days)",
            issue_label(report, &finding.issue_id),
            finding.updated_at,
            finding.elapsed_days
        ));
    }
    group_header!("Near-duplicate titles", report.near_duplicate_titles);
    for finding in &report.near_duplicate_titles.findings {
        lines.push(format!(
            "  {}",
            finding
                .issue_ids
                .iter()
                .map(|id| issue_label(report, id))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    group_header!("Diff", report.diff);
    for finding in &report.diff.findings {
        lines.push(format!(
            "  {}: {finding:?}",
            issue_label(report, change_issue_id(finding))
        ));
    }
    lines.join("\n")
}

fn format_utc(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let seconds = epoch.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

fn format_snapshot_time(epoch: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok());
    format_snapshot_time_at(epoch, now)
}

fn format_snapshot_time_at(epoch: i64, now: Option<i64>) -> String {
    match now.and_then(|now| now.checked_sub(epoch)) {
        Some(age) if age >= 0 => format!("{} ({}s old)", format_utc(epoch), age),
        Some(_) => format!("{} (clock ahead of local time)", format_utc(epoch)),
        None => format!("{} (age unavailable)", format_utc(epoch)),
    }
}

fn issue_label(report: &FocusHygieneReport, id: &ix_board::IssueId) -> String {
    format_issue_ref(&report.issue_refs, id)
}

fn format_issue_ref(
    refs: &std::collections::BTreeMap<ix_board::IssueId, ix_board::IssueReference>,
    id: &ix_board::IssueId,
) -> String {
    refs.get(id).map_or_else(
        || id.0.clone(),
        |reference| format!("{} — {}", reference.identifier, reference.title),
    )
}

fn change_issue_id(change: &ix_board::FocusChange) -> &ix_board::IssueId {
    use ix_board::FocusChange;
    match change {
        FocusChange::Membership { issue_id, .. }
        | FocusChange::State { issue_id, .. }
        | FocusChange::Project { issue_id, .. }
        | FocusChange::Milestone { issue_id, .. }
        | FocusChange::Blockers { issue_id, .. }
        | FocusChange::Created { issue_id }
        | FocusChange::Closed { issue_id } => issue_id,
    }
}

fn render_status_group(
    lines: &mut Vec<String>,
    indent: &str,
    open_by_state: &[ix_board::FocusStateCount],
    in_progress: &[ix_board::FocusIssue],
    progress: ix_board::FocusProgress,
) {
    lines.push(format!(
        "{indent}Progress: {}/{} leaf tasks, excluding parents ({})",
        progress.done,
        progress.total,
        progress
            .completed_percent
            .map_or_else(|| "n/a".to_owned(), |percent| format!("{percent:.1}%"))
    ));
    lines.push(format!("{indent}Open by state, including parents:"));
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
    use super::{Cli, Command, FocusCommand, format_issue_ref, format_snapshot_time_at};
    use clap::Parser;
    use ix_board::{IssueId, IssueReference};
    use std::collections::BTreeMap;

    /// Trace: FR-012-AC-7
    #[test]
    fn snapshot_time_is_readable_and_reports_age() {
        assert_eq!(
            format_snapshot_time_at(1_791_232_484, Some(1_791_232_514)),
            "2026-10-05T20:34:44Z (30s old)"
        );
        assert!(format_snapshot_time_at(10, Some(9)).contains("clock ahead"));
    }

    /// Trace: FR-013-AC-12
    #[test]
    fn hygiene_issue_label_uses_identifier_and_title_with_uuid_fallback() {
        let id = IssueId("issue-uuid".to_owned());
        let refs = BTreeMap::from([(
            id.clone(),
            IssueReference {
                identifier: "SAPHO-57".to_owned(),
                title: "Add Extractor port".to_owned(),
            },
        )]);
        assert_eq!(
            format_issue_ref(&refs, &id),
            "SAPHO-57 — Add Extractor port"
        );
        assert_eq!(
            format_issue_ref(&refs, &IssueId("missing-uuid".to_owned())),
            "missing-uuid"
        );
    }

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

    /// Trace: FR-014-AC-3
    #[test]
    fn focus_plan_chunk_accepts_repeated_selectors_and_positive_size_only() {
        let cli = Cli::try_parse_from([
            "ix-board",
            "focus",
            "plan-chunk",
            "--initiative",
            "Delivery",
            "--project",
            "IR-P1",
            "--issue",
            "IR-317",
            "--size",
            "4",
        ])
        .expect("parse plan chunk");
        assert!(matches!(
            cli.command,
            Command::Focus {
                command: FocusCommand::PlanChunk {
                    initiatives,
                    projects,
                    issues,
                    size: Some(size),
                }
            } if initiatives == ["Delivery"]
                && projects == ["IR-P1"]
                && issues == ["IR-317"]
                && size.get() == 4
        ));
        for value in ["0", "-1", "abc"] {
            assert!(
                Cli::try_parse_from([
                    "ix-board",
                    "focus",
                    "plan-chunk",
                    "--project",
                    "IR-P1",
                    "--size",
                    value,
                ])
                .is_err()
            );
        }
    }

    /// Trace: FR-013-AC-4
    #[test]
    fn focus_hygiene_accepts_positive_stale_days_and_rejects_zero() {
        let cli = Cli::try_parse_from([
            "ix-board",
            "focus",
            "hygiene",
            "--project",
            "IR-P1",
            "--stale-days",
            "45",
        ])
        .expect("positive stale days");
        assert!(
            matches!(cli.command, Command::Focus { command: FocusCommand::Hygiene { stale_days: 45, projects, .. } } if projects == ["IR-P1"])
        );
        assert!(
            Cli::try_parse_from([
                "ix-board",
                "focus",
                "hygiene",
                "--project",
                "IR-P1",
                "--stale-days",
                "0",
            ])
            .is_err()
        );
    }
}
