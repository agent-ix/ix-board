// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Command-line interface for deterministic cached Linear reads.

use clap::{Parser, Subcommand};
use ix_board::agents::{
    AgentsReport, Host, ReadyTicket, SystemHost, build_report, parse_duration, parse_machines,
    render_text,
};
use ix_board::chain::{ChainReport, evaluate_chain, open_scope_issues, resolve_issue};
use ix_board::orphans::{OrphansReport, find_orphans};
use ix_board::program_review::{
    ProgramRoadmap, ProgramScope, ScopeCheckReport, build_roadmap, check_scope,
};
use ix_board::reconcile::{
    DEFAULT_STALE_DAYS, ReconcileReport, collect_github_evidence, evaluate_reconcile,
    repositories_for_scope, resolve_project,
};
use ix_board::{
    DriftConfig, DriftError, DriftEvidenceFile, DriftReport, Error, FocusExpansion,
    FocusHygieneReport, FocusPlanChunk, FocusProjectMembership, FocusRecord, FocusSelection,
    FocusStatus, IssueBatch, LinearGraphQlSource, NextStep, ProjectChunk, ProjectPriority,
    ProjectRunStatus, ReadService, ReadyItem, SearchResult, WorkspaceSource, evaluate_drift,
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
        /// Linear project identifier, UUID, or exact name.
        project: String,
    },
    /// Search structured project and issue fields.
    Search {
        /// Case-insensitive substring query.
        query: String,
    },
    /// Read multiple exact Linear issues in one cached workspace request.
    Issues {
        /// Linear issue identifiers or UUIDs, in desired output order.
        #[arg(required = true, num_args = 1..)]
        identifiers: Vec<String>,
    },
    /// Say whether each agent in a team's herdr workspaces is working right now.
    Agents {
        /// Team names: herdr workspace labels, on any machine.
        #[arg(required = true, num_args = 1..)]
        teams: Vec<String>,
        /// Quiet time after which a ready agent counts as idle (for example 90s, 20m, 2h).
        #[arg(long, default_value = "20m", value_parser = parse_duration)]
        idle_after: u64,
    },
    /// Render a deterministic drift report from versioned structured evidence.
    Drift {
        /// JSON file containing thresholds and configured leads.
        #[arg(long)]
        config: PathBuf,
        /// JSON file containing source records and completeness attestations.
        #[arg(long, required_unless_present = "live", conflicts_with = "live")]
        evidence: Option<PathBuf>,
        /// Collect issue and state-history evidence directly from Linear.
        #[arg(long, conflicts_with = "evidence")]
        live: bool,
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
    /// Validate a program scope file against Linear and GitHub.
    Scope {
        #[command(subcommand)]
        command: ProgramScopeCommand,
    },
    /// List initiatives and projects with current progress and issue counts.
    Roadmap {
        /// Restrict the report to the initiatives and projects in this scope file.
        #[arg(long)]
        scope: Option<PathBuf>,
    },
    /// Compare a project's Linear state with its linked GitHub pull requests and branches.
    Reconcile {
        /// Linear project identifier, UUID, or exact name.
        project: String,
        /// Program scope file whose repositories should be searched for branches.
        #[arg(long)]
        scope: Option<PathBuf>,
        /// Staleness threshold in positive whole days.
        #[arg(long, default_value_t = DEFAULT_STALE_DAYS, value_parser = parse_positive_days)]
        stale_days: u32,
    },
    /// Expand blocker chains from one issue or every open ticket in a program scope.
    Chain {
        /// Linear issue identifier or UUID.
        issue: Option<String>,
        /// Program scope file; when issue is omitted, expands all open project tickets in it.
        #[arg(long, required_unless_present = "issue")]
        scope: Option<PathBuf>,
    },
    /// List unprojected open and completed tickets in the scope's project teams.
    Orphans {
        /// TOML program scope file that defines the projects and their teams.
        #[arg(long, required = true)]
        scope: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ProgramScopeCommand {
    /// Check listed records and report unplaced projects.
    Check {
        /// TOML program scope file.
        #[arg(long, required = true)]
        scope: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCommand {
    /// Save a Linear API key; prompt with masked feedback by default.
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
    /// Emit a versioned focus record suitable for shared storage and dashboards.
    Report {
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        #[arg(long = "milestone", action = clap::ArgAction::Append)]
        milestones: Vec<String>,
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
    },
    /// Expand selectors into focus issues and prerequisite layers.
    Expand {
        /// Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear milestone UUID or exact globally unique name; may be repeated.
        #[arg(long = "milestone", action = clap::ArgAction::Append)]
        milestones: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
    },
    /// Report project and milestone status, blockers, and ready focus issues.
    Status {
        /// Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear milestone UUID or exact globally unique name; may be repeated.
        #[arg(long = "milestone", action = clap::ArgAction::Append)]
        milestones: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
        /// Limit blocker roots and ready issues to these exact milestone IDs or names; repeatable.
        #[arg(long = "work-milestone", action = clap::ArgAction::Append)]
        work_milestones: Vec<String>,
    },
    /// Prepare a bounded dependency-ordered handoff chunk for a focus.
    PlanChunk {
        /// Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        /// Linear project UUID, identifier, or exact name; may be repeated.
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        /// Linear milestone UUID or exact globally unique name; may be repeated.
        #[arg(long = "milestone", action = clap::ArgAction::Append)]
        milestones: Vec<String>,
        /// Linear issue UUID or identifier; may be repeated.
        #[arg(long = "issue", action = clap::ArgAction::Append)]
        issues: Vec<String>,
        /// Limit handoff roots to these exact milestone IDs or names; repeatable.
        #[arg(long = "work-milestone", action = clap::ArgAction::Append)]
        work_milestones: Vec<String>,
        /// Maximum number of open focus issues to emit.
        #[arg(long, value_parser = parse_positive_size)]
        size: Option<NonZeroUsize>,
    },
    /// Report hygiene findings and changes for a focus.
    Hygiene {
        /// Linear initiative UUID, identifier (for example I-10), or exact name; may be repeated.
        #[arg(long = "initiative", action = clap::ArgAction::Append)]
        initiatives: Vec<String>,
        #[arg(long = "project", action = clap::ArgAction::Append)]
        projects: Vec<String>,
        #[arg(long = "milestone", action = clap::ArgAction::Append)]
        milestones: Vec<String>,
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
    FocusRecord(FocusRecord),
    FocusPlanChunk(FocusPlanChunk),
    FocusHygiene(Box<FocusHygieneReport>),
    Search(Vec<SearchResult>),
    Issues(IssueBatch),
    Drift(DriftReport),
    Agents(AgentsReport),
    ProjectStatusUpdated {
        project: String,
        status: String,
    },
    ProjectPriorityUpdated {
        project: String,
        priority: i32,
        label: String,
    },
    ProgramScopeCheck(ScopeCheckReport),
    ProgramRoadmap(ProgramRoadmap),
    Reconcile(ReconcileReport),
    Chain(ChainReport),
    Orphans(OrphansReport),
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
                    std::process::ExitCode::from(output_outcome(&output))
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
            emit_diagnostic(diagnostics, "error", &format_cli_error(&error));
            std::process::ExitCode::from(match error {
                Error::InvalidTeam
                | Error::InvalidQuery
                | Error::ProjectNotFound { .. }
                | Error::InvalidProjectStatus { .. }
                | Error::InvalidProjectPriority { .. }
                | Error::EmptyFocus
                | Error::WorkMilestoneNotFound { .. }
                | Error::ScopeSelection { .. }
                | Error::AmbiguousMilestone { .. }
                | Error::DriftInputRead { .. }
                | Error::DriftInputDecode { .. }
                | Error::Drift(_) => Outcome::Invalid,
                Error::EmptyCredentialInput => Outcome::Invalid,
                _ => Outcome::Internal,
            })
        }
    }
}

fn output_outcome(output: &Output) -> Outcome {
    match output {
        Output::Issues(batch) if batch.issues.is_empty() && !batch.missing.is_empty() => {
            Outcome::Invalid
        }
        Output::Agents(report) if !report.errors.is_empty() => Outcome::Internal,
        Output::ProgramScopeCheck(report) if !report.is_valid() => Outcome::Partial,
        _ => Outcome::Ok,
    }
}

fn format_cli_error(error: &Error) -> String {
    #[cfg(target_os = "macos")]
    if matches!(
        error,
        Error::Credential(ix_cli_kit::secrets::SecretError::Unavailable)
    ) {
        return "Linear credential store unavailable or access denied for this binary; if another ix-board build works, grant this build access to the saved ix-board item in Keychain Access (see setup.md)".to_owned();
    }
    error.to_string()
}

fn validate_command(command: &Command) -> Result<(), Error> {
    match command {
        Command::Next { team } if team.trim().is_empty() => Err(Error::InvalidTeam),
        Command::Search { query } if query.trim().is_empty() => Err(Error::InvalidQuery),
        Command::Issues { identifiers } if identifiers.iter().any(|id| id.trim().is_empty()) => {
            Err(Error::ScopeSelection {
                kind: "issue",
                identifier: String::new(),
                matches: 0,
            })
        }
        Command::Focus {
            command:
                FocusCommand::Report {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
        } if initiatives.is_empty()
            && projects.is_empty()
            && milestones.is_empty()
            && issues.is_empty() =>
        {
            Err(Error::EmptyFocus)
        }
        Command::Focus {
            command:
                FocusCommand::Expand {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
        } if initiatives.is_empty()
            && projects.is_empty()
            && milestones.is_empty()
            && issues.is_empty() =>
        {
            Err(Error::EmptyFocus)
        }
        Command::Focus {
            command:
                FocusCommand::Hygiene {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                    ..
                },
        } if initiatives.is_empty()
            && projects.is_empty()
            && milestones.is_empty()
            && issues.is_empty() =>
        {
            Err(Error::EmptyFocus)
        }
        Command::Focus {
            command:
                FocusCommand::Status {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                    ..
                },
        } if initiatives.is_empty()
            && projects.is_empty()
            && milestones.is_empty()
            && issues.is_empty() =>
        {
            Err(Error::EmptyFocus)
        }
        Command::ProjectChunk { project } if project.trim().is_empty() => {
            Err(Error::ProjectNotFound {
                identifier: project.clone(),
            })
        }
        Command::Reconcile { project, .. } if project.trim().is_empty() => {
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
        Command::Drift {
            config,
            evidence,
            live,
        } => run_drift(&config, evidence.as_ref(), live).map(Output::Drift),
        Command::Agents { teams, idle_after } => run_agents(&teams, idle_after).map(Output::Agents),
        Command::Scope {
            command: ProgramScopeCommand::Check { scope },
        } => {
            let scope = ProgramScope::load(scope).map_err(|error| Error::ProgramReview {
                message: error.to_string(),
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let initiatives = source.fetch_initiatives()?;
            let workspace = ix_board::WorkspaceSource::fetch_workspace(&source)?;
            check_scope(&scope, &initiatives, &workspace)
                .map(Output::ProgramScopeCheck)
                .map_err(Error::from)
        }
        Command::Roadmap { scope } => {
            let scope = scope.map(ProgramScope::load).transpose().map_err(|error| {
                Error::ProgramReview {
                    message: error.to_string(),
                }
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let initiatives = source.fetch_initiatives()?;
            let workspace = ix_board::WorkspaceSource::fetch_workspace(&source)?;
            Ok(Output::ProgramRoadmap(build_roadmap(
                &initiatives,
                &workspace,
                scope.as_ref(),
            )))
        }
        Command::Reconcile {
            project,
            scope,
            stale_days,
        } => {
            let scope = scope.map(ProgramScope::load).transpose().map_err(|error| {
                Error::ProgramReview {
                    message: error.to_string(),
                }
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let workspace = source.fetch_workspace()?;
            let project_record = resolve_project(&workspace, &project).map_err(Error::from)?;
            let project_issues = workspace
                .issues
                .iter()
                .filter(|issue| issue.project_id.as_ref() == Some(&project_record.id))
                .cloned()
                .collect::<Vec<_>>();
            let repositories = repositories_for_scope(scope.as_ref(), &project_issues);
            let evidence =
                collect_github_evidence(repositories.iter().cloned()).map_err(Error::from)?;
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| Error::ProgramReview {
                    message: format!("system clock is before Unix epoch: {error}"),
                })?
                .as_secs_f64();
            evaluate_reconcile(
                project_record,
                &project_issues,
                &evidence,
                repositories.into_iter().collect(),
                stale_days,
                now_epoch,
            )
            .map(Output::Reconcile)
            .map_err(Error::from)
        }
        Command::Chain { issue, scope } => {
            let scope = scope.map(ProgramScope::load).transpose().map_err(|error| {
                Error::ProgramReview {
                    message: error.to_string(),
                }
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let workspace = source.fetch_workspace()?;
            let starts = if let Some(issue) = issue.as_deref() {
                vec![
                    resolve_issue(&workspace, issue)
                        .map_err(Error::from)?
                        .id
                        .clone(),
                ]
            } else if let Some(scope) = scope.as_ref() {
                open_scope_issues(&workspace, scope)
                    .map_err(Error::from)?
                    .into_iter()
                    .map(|issue| issue.id.clone())
                    .collect::<Vec<_>>()
            } else {
                return Err(Error::EmptyFocus);
            };
            if starts.is_empty() {
                return Err(Error::EmptyFocus);
            }
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| Error::ProgramReview {
                    message: format!("system clock is before Unix epoch: {error}"),
                })?
                .as_secs_f64();
            evaluate_chain(&workspace, &starts, scope.as_ref(), now_epoch)
                .map(Output::Chain)
                .map_err(Error::from)
        }
        Command::Orphans { scope } => {
            let scope = ProgramScope::load(scope).map_err(|error| Error::ProgramReview {
                message: error.to_string(),
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let workspace = source.fetch_workspace()?;
            find_orphans(&scope, &workspace)
                .map(Output::Orphans)
                .map_err(Error::from)
        }
        other => run_linear(other),
    }
}

fn run_agents(teams: &[String], idle_after: u64) -> Result<AgentsReport, Error> {
    let local = SystemHost::local();
    let machines = parse_machines(&local.herdr(&["machine", "list", "--json"])?)?;
    let mut hosts: Vec<Box<dyn Host>> = vec![Box::new(local)];
    hosts.extend(
        machines
            .into_iter()
            .filter(|machine| machine.enabled)
            .map(|machine| {
                Box::new(SystemHost::remote(machine.label, machine.target)) as Box<dyn Host>
            }),
    );
    let mut service = None;
    let mut next_ready = |team: &str| -> Result<Option<ReadyTicket>, String> {
        if service.is_none() {
            let source = LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
            service = Some(ReadService::open_default(source).map_err(|error| error.to_string())?);
        }
        let queue = service
            .as_mut()
            .ok_or_else(|| "ready queue unavailable".to_owned())?
            .next(team)
            .map_err(|error| error.to_string())?;
        Ok(queue.into_iter().next().map(|item| ReadyTicket {
            identifier: item.identifier,
            title: item.title,
        }))
    };
    Ok(build_report(&hosts, teams, idle_after, &mut next_ready))
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
                let prompt = rpassword::ConfigBuilder::new()
                    .password_feedback_mask('*')
                    .build();
                rpassword::prompt_password_with_config("Linear API key: ", prompt)
                    .map_err(Error::CredentialInput)?
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
            let source = LinearGraphQlSource::credential_source()?;
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
        Command::Agents { .. } => unreachable!("agents is handled before Linear dispatch"),
        Command::Scope { .. }
        | Command::Roadmap { .. }
        | Command::Reconcile { .. }
        | Command::Chain { .. } => {
            unreachable!("program review commands are handled before Linear dispatch")
        }
        Command::Orphans { .. } => {
            unreachable!("program review commands are handled before Linear dispatch")
        }
        Command::Next { team } => service.next(&team).map(Output::Queue),
        Command::ProjectChunk { project } => service.project_chunk(&project).map(Output::Chunk),
        Command::Search { query } => service.search(&query).map(Output::Search),
        Command::Issues { identifiers } => service.issues(&identifiers).map(Output::Issues),
        Command::Focus {
            command:
                FocusCommand::Report {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
        } => service
            .focus_record(&FocusSelection {
                initiatives,
                projects,
                milestones,
                issues,
            })
            .map(Output::FocusRecord),
        Command::Focus {
            command:
                FocusCommand::Expand {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
        } => service
            .focus_expand(&FocusSelection {
                initiatives,
                projects,
                milestones,
                issues,
            })
            .map(Output::Focus),
        Command::Focus {
            command:
                FocusCommand::Status {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                    work_milestones,
                },
        } => service
            .focus_status_for_milestones(
                &FocusSelection {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
                &work_milestones,
            )
            .map(Output::FocusStatus),
        Command::Focus {
            command:
                FocusCommand::PlanChunk {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                    work_milestones,
                    size,
                },
        } => service
            .focus_plan_chunk_for_milestones(
                &FocusSelection {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
                size,
                &work_milestones,
            )
            .map(Output::FocusPlanChunk),
        Command::Focus {
            command:
                FocusCommand::Hygiene {
                    initiatives,
                    projects,
                    milestones,
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
                        milestones,
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

fn run_drift(
    config_path: &PathBuf,
    evidence_path: Option<&PathBuf>,
    live: bool,
) -> Result<DriftReport, Error> {
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
    if live {
        let source = LinearGraphQlSource::from_env()?;
        let evidence = source.collect_drift_evidence(&config)?;
        return Ok(evaluate_drift(&config, &evidence)?);
    }
    let evidence_path = evidence_path.ok_or_else(|| {
        Error::Drift(DriftError::InvalidConfig {
            reason: "drift requires --evidence or --live".to_owned(),
        })
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
        Output::FocusRecord(record) => format!(
            "Focus record v{} at {}\nSelection: {}\n{}",
            record.schema_version,
            format_utc(record.reported_at),
            record
                .selection
                .iter()
                .map(|member| match member {
                    ix_board::FocusMember::Initiative(id) => format!("initiative:{id}"),
                    ix_board::FocusMember::Project(id) => format!("project:{id}"),
                    ix_board::FocusMember::Milestone(id) => format!("milestone:{id}"),
                    ix_board::FocusMember::Issue(id) => format!("issue:{id}"),
                })
                .collect::<Vec<_>>()
                .join(", "),
            render_focus_status(&record.status)
        ),
        Output::FocusPlanChunk(chunk) => render_focus_plan_chunk(chunk),
        Output::FocusHygiene(report) => render_focus_hygiene(report),
        Output::Search(results) => results
            .iter()
            .map(|record| format!("{:?} {} — {}", record.kind, record.identifier, record.name))
            .collect::<Vec<_>>()
            .join("\n"),
        Output::Issues(batch) => {
            let mut lines = batch
                .issues
                .iter()
                .map(|issue| {
                    format!(
                        "{} [{}] {}",
                        issue.identifier, issue.state_name, issue.title
                    )
                })
                .collect::<Vec<_>>();
            if !batch.missing.is_empty() {
                lines.push(format!("Missing: {}", batch.missing.join(", ")));
            }
            lines.join("\n")
        }
        Output::Drift(report) => render_drift_text(report),
        Output::Agents(report) => render_text(report),
        Output::ProgramScopeCheck(report) => {
            let mut lines = Vec::new();
            for name in &report.missing_initiatives {
                lines.push(format!("missing initiative: {name}"));
            }
            for name in &report.missing_projects {
                lines.push(format!("missing project: {name}"));
            }
            for name in &report.missing_repositories {
                lines.push(format!("missing repository: {name}"));
            }
            for name in &report.unplaced_projects {
                lines.push(format!("unplaced project: {name}"));
            }
            if lines.is_empty() {
                lines.push("scope valid".to_owned());
            }
            lines.join("\n")
        }
        Output::ProgramRoadmap(report) => render_roadmap(report),
        Output::Reconcile(report) => render_reconcile(report),
        Output::Chain(report) => render_chain(report),
        Output::Orphans(report) => render_orphans(report),
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

fn render_roadmap(report: &ProgramRoadmap) -> String {
    let mut lines = Vec::new();
    for initiative in &report.initiatives {
        lines.push(initiative.name.clone());
        for project in &initiative.projects {
            lines.push(format!(
                "  {} | {} | {}% | open {} done {} canceled {} | last activity {}{}",
                project.name,
                project.status,
                project.progress_percent,
                project.open,
                project.done,
                project.canceled,
                project.last_activity.as_deref().unwrap_or("unknown"),
                if project.also_in.is_empty() {
                    String::new()
                } else {
                    format!(" | also in: {}", project.also_in.join(", "))
                }
            ));
        }
    }
    for project in &report.unplaced_projects {
        lines.push(format!(
            "unplaced project: {} | {} | {}%",
            project.name, project.status, project.progress_percent
        ));
    }
    for initiative in &report.initiatives_without_projects {
        lines.push(format!("initiative without projects: {initiative}"));
    }
    for project in &report.projects_in_multiple_initiatives {
        lines.push(format!("project in multiple initiatives: {project}"));
    }
    for project in &report.projects_missing_from_scope {
        lines.push(format!(
            "project in a scoped initiative but missing from scope file: {project}"
        ));
    }
    for initiative in &report.missing_scope_initiatives {
        lines.push(format!(
            "initiative listed in scope file but missing from Linear: {initiative}"
        ));
    }
    for project in &report.missing_scope_projects {
        lines.push(format!(
            "project listed in scope file but missing from Linear: {project}"
        ));
    }
    for mismatch in &report.project_membership_mismatches {
        if !mismatch.missing_from_linear.is_empty() {
            lines.push(format!(
                "scope project missing Linear initiative membership: {} | {}",
                mismatch.project,
                mismatch.missing_from_linear.join(", ")
            ));
        }
        if !mismatch.missing_from_scope.is_empty() {
            lines.push(format!(
                "Linear initiative membership missing from scope file: {} | {}",
                mismatch.project,
                mismatch.missing_from_scope.join(", ")
            ));
        }
    }
    lines.join("\n")
}

fn render_reconcile(report: &ReconcileReport) -> String {
    let mut lines = vec![format!(
        "Reconcile {} (stale threshold: {} days)",
        report.project, report.stale_days
    )];
    if !report.searched_repositories.is_empty() {
        lines.push(format!(
            "Searched repositories: {}",
            report.searched_repositories.join(", ")
        ));
    }
    for warning in &report.warnings {
        lines.push(format!("warning: {warning}"));
    }
    let ticket_groups = [
        (
            "Merged PR, ticket still open",
            &report.findings.merged_pr_open_ticket,
        ),
        (
            "Done without merged PR",
            &report.findings.done_without_merged_pr,
        ),
        (
            "In Progress without PR or branch",
            &report.findings.in_progress_without_pr_or_branch,
        ),
    ];
    for (title, findings) in ticket_groups {
        lines.push(format!("{title} ({}):", findings.len()));
        if findings.is_empty() {
            lines.push("  (none)".to_owned());
        }
        for finding in findings {
            let age = finding
                .age_days
                .map(|days| format!(" | {} days", days))
                .unwrap_or_default();
            let prs = if finding.pull_requests.is_empty() {
                String::new()
            } else {
                format!(" | PRs: {}", finding.pull_requests.join(", "))
            };
            lines.push(format!(
                "  {} [{}] {}{}{}",
                finding.identifier, finding.state, finding.title, age, prs
            ));
        }
    }
    for (title, findings) in [
        ("Stale open PR", &report.findings.stale_open_pr),
        (
            "Draft PR with closed ticket",
            &report.findings.draft_pr_closed_ticket,
        ),
    ] {
        lines.push(format!("{title} ({}):", findings.len()));
        if findings.is_empty() {
            lines.push("  (none)".to_owned());
        }
        for finding in findings {
            lines.push(format!(
                "  {} [{}] {} | {} | {} days",
                finding.identifier, finding.state, finding.title, finding.url, finding.age_days
            ));
        }
    }
    lines.join("\n")
}

fn render_chain(report: &ChainReport) -> String {
    let nodes = report
        .nodes
        .iter()
        .map(|node| (node.identifier.as_str(), node))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut lines = vec![format!("Blocker chain for {}", report.starts.join(", "))];
    lines.push(format!(
        "Root blockers without assignee or lane ({}):",
        report.unowned_root_blockers.len()
    ));
    if report.unowned_root_blockers.is_empty() {
        lines.push("  (none)".to_owned());
    }
    for identifier in &report.unowned_root_blockers {
        if let Some(node) = nodes.get(identifier.as_str()) {
            lines.push(format!("  {}", format_chain_node(node)));
        }
    }
    lines.push(format!(
        "All root blockers ({}):",
        report.root_blockers.len()
    ));
    if report.root_blockers.is_empty() {
        lines.push("  (none; cycles or unresolved blockers may prevent a root)".to_owned());
    }
    for identifier in &report.root_blockers {
        if let Some(node) = nodes.get(identifier.as_str()) {
            lines.push(format!("  {}", format_chain_node(node)));
        }
    }
    lines.push(format!("Blocker paths ({}):", report.paths.len()));
    if report.paths.is_empty() {
        lines.push("  (none)".to_owned());
    }
    for path in &report.paths {
        lines.push(format!("  {}", path.issues.join(" -> ")));
        for identifier in &path.issues {
            if let Some(node) = nodes.get(identifier.as_str()) {
                lines.push(format!("    {}", format_chain_node(node)));
            }
        }
    }
    lines.push(format!("Cycles ({}):", report.cycles.len()));
    if report.cycles.is_empty() {
        lines.push("  (none)".to_owned());
    }
    for cycle in &report.cycles {
        lines.push(format!("  {}", cycle.join(" -> ")));
        for identifier in cycle.iter().take(cycle.len().saturating_sub(1)) {
            if let Some(node) = nodes.get(identifier.as_str()) {
                lines.push(format!("    {}", format_chain_node(node)));
            }
        }
    }
    for warning in &report.warnings {
        lines.push(format!("warning: {warning}"));
    }
    lines.join("\n")
}

fn render_orphans(report: &OrphansReport) -> String {
    let mut lines = vec![format!("Unprojected tickets in {}", report.scope)];
    for warning in &report.warnings {
        lines.push(format!("warning: {warning}"));
    }
    for (heading, findings) in [("Open", &report.open), ("Done", &report.done)] {
        lines.push(format!("{heading} ({}):", findings.len()));
        if findings.is_empty() {
            lines.push("  (none)".to_owned());
        }
        for finding in findings {
            lines.push(format!(
                "  {} [{}] team={} — {}",
                finding.identifier, heading, finding.team, finding.title
            ));
            if finding.suggestions.is_empty() {
                lines.push("    suggested project: none (no matching scoped project)".to_owned());
            }
            for suggestion in &finding.suggestions {
                lines.push(format!(
                    "    suggested project: {} ({}){}",
                    suggestion.project,
                    suggestion.reason,
                    if suggestion.team_mismatch {
                        format!(
                            "; warning: ticket team {} is not associated with this project",
                            finding.team
                        )
                    } else {
                        String::new()
                    }
                ));
            }
        }
    }
    lines.join("\n")
}

fn format_chain_node(node: &ix_board::chain::ChainNode) -> String {
    let owner = node
        .assignee_name
        .as_deref()
        .or(node.assignee_id.as_deref())
        .unwrap_or("unassigned");
    let lane = node.owner_lane.as_deref().unwrap_or("unknown");
    let age = node
        .age_days
        .map(|days| format!("{days} days old"))
        .unwrap_or_else(|| "age unknown".to_owned());
    format!(
        "{} [{}] team={} lane={} owner={} age={} — {}",
        node.identifier, node.state, node.team, lane, owner, age, node.title
    )
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
    if !status.work_milestones.is_empty() {
        lines.push(format!(
            "Work milestones (ready queue and blocker roots): {}",
            status.work_milestones.join(", ")
        ));
    }
    for project in &status.projects {
        let scope_label = match project.membership {
            FocusProjectMembership::FullProject => "",
            FocusProjectMembership::PartialFocus => " (selected focus only)",
        };
        lines.push(format!(
            "Project {} — {}{}",
            project.identifier, project.name, scope_label
        ));
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
        if project.membership == FocusProjectMembership::FullProject
            || project.unmilestoned.progress.total > 0
            || !project.unmilestoned.open_by_state.is_empty()
        {
            lines.push("  No milestone:".to_owned());
            render_status_group(
                &mut lines,
                "    ",
                &project.unmilestoned.open_by_state,
                &project.unmilestoned.in_progress,
                project.unmilestoned.progress,
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
    if status.ready_queue.is_empty() {
        lines.push(if status.work_milestones.is_empty() {
            "  (none)".to_owned()
        } else {
            format!(
                "  (none in {}; {} ready in other milestones excluded)",
                status.work_milestones.join(", "),
                status.excluded_ready_count
            )
        });
    }
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
    if !chunk.work_milestones.is_empty() {
        lines.push(format!(
            "Work milestones: {}",
            chunk.work_milestones.join(", ")
        ));
    }
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
            lines.push(format!(
                "{}: {} ({} findings)",
                $name,
                if matches!($group.availability, ix_board::GroupStatus::Available) {
                    "checked"
                } else {
                    "incomplete"
                },
                $group.findings.len()
            ));
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
    use super::{
        Cli, Command, FocusCommand, Output, ProgramRoadmap, ProgramScopeCommand, format_issue_ref,
        format_snapshot_time_at, output_outcome, render_chain, render_focus_status, render_orphans,
        render_reconcile, render_roadmap,
    };
    use clap::Parser;
    use ix_board::program_review::ScopeCheckReport;
    use ix_board::{FocusStatus, IssueBatch, IssueId, IssueReference};
    use ix_cli_kit::exit::Outcome;

    /// Trace: FR-012-AC-9
    #[test]
    fn empty_filtered_ready_queue_explains_excluded_work() {
        let status = FocusStatus {
            fetched_at: 1_791_232_484,
            work_milestones: vec!["M1 Spec".to_owned(), "M2 Core on Qwen".to_owned()],
            excluded_ready_count: 15,
            projects: Vec::new(),
            blocker_layers: Vec::new(),
            ready_queue: Vec::new(),
        };
        assert!(
            render_focus_status(&status).contains(
                "(none in M1 Spec, M2 Core on Qwen; 15 ready in other milestones excluded)"
            )
        );
    }
    /// Trace: FR-017-AC-4
    #[test]
    fn all_missing_issue_batch_exits_nonzero_after_rendering() {
        assert_eq!(
            output_outcome(&Output::Issues(IssueBatch {
                issues: Vec::new(),
                missing: vec!["SAPHO-9999".to_owned()],
                complete: false,
            })) as u8,
            Outcome::Invalid as u8
        );
    }

    /// Trace: FR-021-AC-3, FR-023-AC-1
    #[test]
    fn program_review_commands_accept_the_scope_file_option() {
        let scope = Cli::try_parse_from(["ix-board", "scope", "check", "--scope", "qsl.toml"])
            .expect("parse scope check");
        assert!(
            matches!(scope.command, Command::Scope { command: ProgramScopeCommand::Check { scope } } if scope.to_str() == Some("qsl.toml"))
        );

        let roadmap = Cli::try_parse_from(["ix-board", "roadmap", "--scope", "qsl.toml"])
            .expect("parse scoped roadmap");
        assert!(
            matches!(roadmap.command, Command::Roadmap { scope: Some(scope) } if scope.to_str() == Some("qsl.toml"))
        );
        assert!(Cli::try_parse_from(["ix-board", "roadmap"]).is_ok());

        let reconcile = Cli::try_parse_from([
            "ix-board",
            "reconcile",
            "SWM",
            "--scope",
            "qsl.toml",
            "--stale-days",
            "21",
        ])
        .expect("parse scoped reconcile");
        assert!(matches!(
            reconcile.command,
            Command::Reconcile {
                project,
                scope: Some(scope),
                stale_days: 21
            } if project == "SWM" && scope.to_str() == Some("qsl.toml")
        ));
        assert!(Cli::try_parse_from(["ix-board", "reconcile", "SWM"]).is_ok());
        assert!(
            Cli::try_parse_from(["ix-board", "reconcile", "SWM", "--stale-days", "0"]).is_err()
        );

        let chain =
            Cli::try_parse_from(["ix-board", "chain", "QSL-42"]).expect("parse single issue chain");
        assert!(
            matches!(chain.command, Command::Chain { issue: Some(issue), scope: None } if issue == "QSL-42")
        );
        let scoped_chain = Cli::try_parse_from(["ix-board", "chain", "--scope", "program.toml"])
            .expect("parse scoped chain");
        assert!(
            matches!(scoped_chain.command, Command::Chain { issue: None, scope: Some(scope) } if scope.to_str() == Some("program.toml"))
        );
        assert!(Cli::try_parse_from(["ix-board", "chain"]).is_err());
        let orphans = Cli::try_parse_from(["ix-board", "orphans", "--scope", "program.toml"])
            .expect("parse scoped orphans");
        assert!(
            matches!(orphans.command, Command::Orphans { scope } if scope.to_str() == Some("program.toml"))
        );
        assert!(Cli::try_parse_from(["ix-board", "orphans"]).is_err());
    }

    /// Trace: FR-021-AC-3
    #[test]
    fn reconcile_human_and_json_outputs_expose_all_five_finding_groups() {
        let report = ix_board::reconcile::ReconcileReport {
            project: "SWM".to_owned(),
            project_id: ix_board::ProjectId("project-1".to_owned()),
            stale_days: 14,
            searched_repositories: vec!["agent-ix/ix-board".to_owned()],
            warnings: Vec::new(),
            findings: ix_board::reconcile::ReconcileFindings::default(),
        };
        let human = render_reconcile(&report);
        for heading in [
            "Merged PR, ticket still open",
            "Done without merged PR",
            "In Progress without PR or branch",
            "Stale open PR",
            "Draft PR with closed ticket",
        ] {
            assert!(human.contains(heading));
        }
        let json = serde_json::to_value(Output::Reconcile(report)).unwrap();
        for group in [
            "merged_pr_open_ticket",
            "done_without_merged_pr",
            "in_progress_without_pr_or_branch",
            "stale_open_pr",
            "draft_pr_closed_ticket",
        ] {
            assert!(json["findings"][group].is_array());
        }
    }

    #[test]
    /// Trace: FR-022-AC-1
    fn chain_human_and_json_outputs_include_graph_details() {
        let report = ix_board::chain::ChainReport {
            starts: vec!["QSL-1".to_owned()],
            nodes: vec![ix_board::chain::ChainNode {
                id: IssueId("issue-1".to_owned()),
                identifier: "QSL-1".to_owned(),
                title: "Review API".to_owned(),
                state: "In Progress".to_owned(),
                team: "QSL".to_owned(),
                owner_lane: Some("terra:QSL:code".to_owned()),
                assignee_id: Some("user-1".to_owned()),
                assignee_name: Some("Quill Dev".to_owned()),
                age_days: Some(12),
                blocked_by: vec!["IR-2".to_owned()],
                root_blocker: false,
                unowned_root_blocker: false,
            }],
            paths: vec![ix_board::chain::ChainPath {
                start: "QSL-1".to_owned(),
                issues: vec!["QSL-1".to_owned(), "IR-2".to_owned()],
            }],
            root_blockers: vec!["IR-2".to_owned()],
            unowned_root_blockers: vec!["IR-2".to_owned()],
            cycles: vec![vec![
                "QSL-4".to_owned(),
                "QSL-5".to_owned(),
                "QSL-4".to_owned(),
            ]],
            warnings: Vec::new(),
        };
        let human = render_chain(&report);
        for detail in [
            "QSL-1",
            "In Progress",
            "team=QSL",
            "terra:QSL:code",
            "Quill Dev",
            "12 days old",
            "QSL-4 -> QSL-5 -> QSL-4",
        ] {
            assert!(
                human.contains(detail),
                "human output should include {detail}"
            );
        }
        let json = serde_json::to_value(Output::Chain(report)).unwrap();
        assert!(json["nodes"].is_array());
        assert_eq!(json["nodes"][0]["assignee_name"], "Quill Dev");
        assert_eq!(json["cycles"][0][1], "QSL-5");
        assert_eq!(json["unowned_root_blockers"][0], "IR-2");
    }

    /// Trace: FR-023-AC-3
    #[test]
    fn orphans_human_and_json_outputs_expose_both_groups_and_warnings() {
        let report = ix_board::orphans::OrphansReport {
            scope: "Review".to_owned(),
            open: vec![ix_board::orphans::OrphanFinding {
                identifier: "IR-21".to_owned(),
                title: "QLS: integration".to_owned(),
                team: "IR".to_owned(),
                suggestions: vec![ix_board::orphans::ProjectSuggestion {
                    project: "QSL Language Spec".to_owned(),
                    reason: "title begins with project prefix \"QLS\"".to_owned(),
                    team_mismatch: true,
                }],
            }],
            done: Vec::new(),
            warnings: Vec::new(),
        };
        let human = render_orphans(&report);
        for value in [
            "Open (1)",
            "Done (0)",
            "QSL Language Spec",
            "title begins",
            "team IR is not associated",
        ] {
            assert!(human.contains(value), "human output should include {value}");
        }
        let json = serde_json::to_value(Output::Orphans(report)).unwrap();
        assert_eq!(json["open"][0]["suggestions"][0]["team_mismatch"], true);
        assert!(json["done"].is_array());
    }

    #[test]
    fn roadmap_reports_scope_coverage_and_initiative_membership_breaks() {
        let report = ProgramRoadmap {
            initiatives: Vec::new(),
            unplaced_projects: Vec::new(),
            initiatives_without_projects: Vec::new(),
            projects_in_multiple_initiatives: Vec::new(),
            projects_missing_from_scope: vec!["Runtime".to_owned()],
            project_membership_mismatches: vec![
                ix_board::program_review::ProjectInitiativeMismatch {
                    project: "Compiler".to_owned(),
                    missing_from_linear: vec!["QSL Core".to_owned()],
                    missing_from_scope: vec!["QSL Ecosystem".to_owned()],
                },
            ],
            missing_scope_initiatives: vec!["Missing Initiative".to_owned()],
            missing_scope_projects: vec!["Missing Project".to_owned()],
        };
        let human = render_roadmap(&report);
        for message in [
            "missing from scope file: Runtime",
            "scope project missing Linear initiative membership: Compiler | QSL Core",
            "Linear initiative membership missing from scope file: Compiler | QSL Ecosystem",
            "initiative listed in scope file but missing from Linear: Missing Initiative",
            "project listed in scope file but missing from Linear: Missing Project",
        ] {
            assert!(
                human.contains(message),
                "human output should include {message}"
            );
        }
        let json = serde_json::to_value(Output::ProgramRoadmap(report)).unwrap();
        assert_eq!(json["projects_missing_from_scope"][0], "Runtime");
        assert_eq!(
            json["project_membership_mismatches"][0]["project"],
            "Compiler"
        );
        assert_eq!(json["missing_scope_initiatives"][0], "Missing Initiative");
        assert_eq!(json["missing_scope_projects"][0], "Missing Project");
    }

    #[test]
    fn scope_findings_produce_a_nonzero_partial_outcome() {
        assert_eq!(
            output_outcome(&Output::ProgramScopeCheck(ScopeCheckReport {
                missing_projects: vec!["Missing project".to_owned()],
                ..Default::default()
            })) as u8,
            Outcome::Partial as u8
        );
    }
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
            "--milestone",
            "M1 Spec",
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
                    milestones,
                    issues,
                }
            } if initiatives == ["Delivery"]
                && projects == ["SWM-P1", "SWM-P2"]
                && milestones == ["M1 Spec"]
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
            "--work-milestone",
            "M1",
        ])
        .expect("parse focus status selectors");
        assert!(matches!(
            cli.command,
            Command::Focus {
                command: FocusCommand::Status {
                    initiatives,
                    projects,
                    issues,
                    work_milestones,
                    ..
                }
            } if initiatives == ["Delivery"]
                && projects == ["SWM-P1"]
                && issues == ["SWM-1"]
                && work_milestones == ["M1"]
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
            "--work-milestone",
            "M1",
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
                    work_milestones,
                    size: Some(size),
                    ..
                }
            } if initiatives == ["Delivery"]
                && projects == ["IR-P1"]
                && issues == ["IR-317"]
                && work_milestones == ["M1"]
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
