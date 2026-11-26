// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Command-line interface for deterministic cached Linear reads.

use clap::{Parser, Subcommand};
use ix_board::agents::{AgentsReport, ReadyTicket, parse_duration, render_text};
use ix_board::chain::{ChainReport, evaluate_chain, open_scope_issues, resolve_issue};
use ix_board::orphans::{OrphansReport, find_orphans};
#[cfg(test)]
use ix_board::program_review::ProgramRoadmap;
use ix_board::program_review::{ProgramScope, ScopeCheckReport};
use ix_board::reconcile::{
    DEFAULT_STALE_DAYS, ReconcileReport, collect_github_evidence, evaluate_reconcile,
    repositories_for_scope, resolve_project,
};
use ix_board::retro;
use ix_board::{
    DriftConfig, DriftError, DriftEvidenceFile, DriftReport, Error, FocusExpansion,
    FocusHygieneReport, FocusPlanChunk, FocusProjectMembership, FocusRecord, FocusSelection,
    FocusStatus, IssueBatch, LinearGraphQlSource, NextStep, ProjectChunk, ProjectPriority,
    ProjectRunStatus, ReadService, ReadyItem, ReferenceWarning, SearchResult, evaluate_drift,
    evaluate_drift_file, render_drift_text,
};
use ix_cli_kit::exit::Outcome;
use ix_cli_kit::secrets::{AppScope, SecretKey, SecretStore, SecretValue};
use ix_cli_kit::streams::{
    ColorChoice, Diagnostics, DiagnosticsFormat, emit_diagnostic, emit_result, emit_warning,
};
use ix_trace_rs::implements;
use serde::Serialize;
use std::collections::BTreeSet;
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
    /// Inspect shared cache and quota state without resolving credentials.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    /// Internal read-only lifecycle probe protocol.
    #[command(name = "__agent-probe", hide = true)]
    AgentProbe,
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
        /// Prometheus text exposition URL containing locked-build running-owner metrics.
        #[arg(long)]
        lease_metrics_url: Option<String>,
    },
    /// Fleet lane commands.
    Fleet {
        #[command(subcommand)]
        command: FleetCommand,
    },
    /// Check and run recurring program jobs.
    Cadence {
        #[command(subcommand)]
        command: CadenceCommand,
    },
    /// Inspect or clean build-host worktree output.
    Hygiene {
        #[command(subcommand)]
        command: HygieneCommand,
    },
    /// Compare build-host capacity telemetry.
    Build {
        #[command(subcommand)]
        command: BuildCommand,
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
    /// Measure whether a team's work is converging and keep the measurements.
    Retro {
        #[command(subcommand)]
        command: RetroCommand,
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
enum CacheCommand {
    /// Show dataset freshness and shared allowance.
    Status,
}

#[derive(Subcommand, Debug)]
enum HygieneCommand {
    /// Report disk space, target directories, and process use for one host.
    Check(HygieneArgs),
    /// Plan cleanup, applying it only when --apply is supplied.
    Clean {
        #[command(flatten)]
        args: HygieneArgs,
        /// Remove only target directories proven idle by complete process evidence.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(clap::Args, Debug)]
struct HygieneArgs {
    /// Host label.
    #[arg(long)]
    host: String,
    /// Worktree root on this host.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Configured minimum free space in GiB.
    #[arg(long)]
    free_floor_gb: Option<u64>,
    /// Configured minimum target age in minutes.
    #[arg(long)]
    idle_minutes: Option<u64>,
}

#[derive(Subcommand, Debug)]
enum BuildCommand {
    /// Render host load, memory, swap, queue and lease metrics.
    Status {
        /// URL returning Prometheus text exposition for configured hosts.
        #[arg(long)]
        metrics_url: String,
        /// Host policy in `host:max_load1:max_running_leases` form (repeatable).
        #[arg(long = "policy")]
        policies: Vec<String>,
    },
}

fn default_worktree_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dev/worktrees")
}

#[derive(Subcommand, Debug)]
enum FleetCommand {
    /// Classify every registered agent lane from relay, Prometheus and order evidence.
    Status,
    /// Send a lane its ordered tickets as a relay order, store the list, and wait for the outcome.
    Assign {
        /// Lane address server:workspace:tab.
        lane: String,
        /// Ticket identifiers in order; the first is the expected ticket.
        #[arg(required = true, num_args = 1..)]
        tickets: Vec<String>,
        /// Acknowledgement deadline (for example 90s, 5m).
        #[arg(long, default_value = "5m", value_parser = parse_duration)]
        deadline: u64,
    },
    /// Evaluate the fleet once and emit the minder's action list; records the tick in the ledger.
    Tick {
        /// Address of the session running the tick, recorded with it.
        #[arg(long)]
        session: Option<String>,
    },
    /// Read live supply, duty and ready-buffer deficit for configured projects.
    Saturation {
        #[command(subcommand)]
        command: Option<SaturationCommand>,
    },
    /// Read the fleet ledger.
    Ledger {
        #[command(subcommand)]
        command: LedgerCommand,
    },
    /// Record the beginning or end of an orchestrator loop cycle.
    Cycle {
        #[command(subcommand)]
        command: CycleCommand,
    },
    /// Read capacity discovery runs, measurements and sustainable levels.
    Capacity {
        #[command(subcommand)]
        command: CapacityCommand,
    },
}

#[derive(Subcommand, Debug)]
enum CadenceCommand {
    /// Report due and overdue jobs for one program and team.
    Due {
        #[arg(long)]
        program: String,
        #[arg(long)]
        team: String,
        /// Emit an alert when the cadence checker job is overdue.
        #[arg(long)]
        alert: bool,
        /// Override the configured cadence manifest.
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Run one uniquely identified cadence job.
    Run {
        job: String,
        /// Override the configured cadence manifest.
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum SaturationCommand {
    /// Collect one live saturation sample into the local fleet ledger.
    Collect,
}

#[derive(Subcommand, Debug)]
enum CycleCommand {
    /// Start a cycle for one relay session.
    Start {
        /// Session address that owns the cycle.
        #[arg(long)]
        session: String,
        /// Configured cycle interval, for example 10m.
        #[arg(long, value_parser = parse_duration)]
        interval: Option<u64>,
    },
    /// End the newest open cycle for one relay session.
    End {
        /// Session address that owns the cycle.
        #[arg(long)]
        session: String,
    },
}

#[derive(Subcommand, Debug)]
enum LedgerCommand {
    /// Print the rows of one ledger view without writing.
    Query {
        /// View name, for example idle_minutes or defect_rows.
        view: String,
        /// Only rows within this window before the newest run (for example 90s, 20m, 4h).
        #[arg(long, value_parser = parse_duration)]
        since: Option<u64>,
    },
    /// Record the Linear ticket that owns an open probe defect.
    RouteDefect {
        /// Ledger defect id.
        defect_id: i64,
        /// Ticket identifier that was filed for the defect.
        ticket: String,
    },
}

#[derive(Subcommand, Debug)]
enum CapacityCommand {
    /// Print capacity discovery state and retained measurement history.
    Status,
    /// Start a discovery run at relay time in milliseconds.
    Start {
        host: String,
        #[arg(long)]
        trigger: String,
        #[arg(long)]
        at_ms: i64,
    },
    /// Record that a new lane count is held after ramp-up.
    Step {
        host: String,
        #[arg(long)]
        lanes: u32,
        #[arg(long)]
        lane_added_at_ms: i64,
        #[arg(long)]
        rampup_end_ms: i64,
        #[arg(long)]
        repeat_of: Option<u32>,
    },
    /// Close and measure the active hold window from live sources.
    Window {
        host: String,
        #[arg(long)]
        end_ms: i64,
    },
    /// End the open run and record its valid sustainable level, when established.
    End {
        host: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        at_ms: i64,
    },
    /// Mark a lane draining and lower the six-hour lane ceiling.
    Drain {
        host: String,
        lane: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        at_ms: i64,
    },
    /// Record an orchestrator-directed lane removal.
    Remove {
        host: String,
        lane: String,
        #[arg(long)]
        at_ms: i64,
    },
}

#[derive(Subcommand, Debug)]
enum RetroCommand {
    /// Append one record of the metric catalog to the team's retro store.
    Snapshot {
        /// JSON retro configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Trailing window for window metrics: seconds, minutes, hours or days (90s, 20m, 4h, 2d).
        #[arg(long, default_value = retro::DEFAULT_WINDOW)]
        window: String,
    },
    /// Compare the latest snapshot with the previous one and the baseline, reading only the store.
    Delta {
        /// JSON retro configuration file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Append one record per UTC day from Linear and GitHub history.
    Backfill {
        /// JSON retro configuration file.
        #[arg(long)]
        config: PathBuf,
        /// First UTC day, YYYY-MM-DD.
        #[arg(long)]
        since: String,
        /// Last UTC day, YYYY-MM-DD; defaults to the last complete UTC day.
        #[arg(long)]
        until: Option<String>,
        /// Trailing window for window metrics: seconds, minutes, hours or days (90s, 20m, 4h, 2d).
        #[arg(long, default_value = retro::DEFAULT_WINDOW)]
        window: String,
    },
    /// Evaluate configured role rules over an event-time window.
    Eval {
        /// JSON retro configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Override the configured team.
        #[arg(long)]
        team: Option<String>,
        /// Trailing window (for example 24h).
        #[arg(long, default_value = "24h")]
        since: String,
        /// Limit presentation to a role.
        #[arg(long)]
        role: Option<String>,
        /// Limit presentation to a session.
        #[arg(long)]
        session: Option<String>,
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
        /// Report closures and PR events inside this window (for example 90m, 48h).
        #[arg(long, default_value = "48h", value_parser = parse_duration)]
        closed_since: u64,
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
        /// Report closures and PR events inside this window (for example 90m, 48h).
        #[arg(long, default_value = "48h", value_parser = parse_duration)]
        closed_since: u64,
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

/// The ready queue. Its JSON is the bare list of rows; the warnings are diagnostics.
#[derive(Serialize)]
#[serde(transparent)]
struct ReadyQueue {
    items: Vec<ReadyItem>,
    #[serde(skip)]
    warnings: Vec<ReferenceWarning>,
}

/// The ready-queue warnings of a result, whether or not it is labeled cached.
fn queue_warnings(output: &Output) -> &[ReferenceWarning] {
    match output {
        Output::Cached { result, .. } => queue_warnings(result),
        Output::Queue(queue) => &queue.warnings,
        _ => &[],
    }
}

/// The first ready row of one team's queue read, keeping the read's warnings.
fn first_ready(
    (queue, warnings): (Vec<ReadyItem>, Vec<ReferenceWarning>),
    collected: &mut BTreeSet<ReferenceWarning>,
) -> Option<ReadyTicket> {
    collected.extend(warnings);
    queue.into_iter().next().map(|item| ReadyTicket {
        identifier: item.identifier,
        title: item.title,
    })
}

#[derive(Serialize)]
#[serde(untagged)]
enum Output {
    Cached {
        result: Box<Output>,
        #[serde(flatten)]
        freshness: ix_board::CacheFreshness,
    },
    CacheStatus(ix_board::cache_status::EvaluatedCacheStatus),
    SummaryRoadmap(ix_board::program_review::SummaryRoadmap),
    Auth {
        status: String,
        source: Option<String>,
    },
    Queue(ReadyQueue),
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
    Fleet(ix_board::fleet::FleetReport),
    FleetLedger(ix_board::fleet::ledger::LedgerRows),
    FleetDefectRoute {
        defect_id: i64,
        ticket: String,
        routed_at_ms: i64,
    },
    FleetCycle(FleetCycleReport),
    FleetAssign(ix_board::fleet::assign::AssignReport),
    FleetTick(ix_board::fleet::tick::TickOutput),
    CadenceDue(ix_board::cadence::DueReport),
    CadenceRun(ix_board::cadence::JobRunReport),
    FleetSaturation(ix_board::fleet::saturation::SaturationReport),
    Retro(Box<retro::SnapshotOutput>),
    FleetCapacity(ix_board::fleet::ledger::CapacityStatus),
    FleetCapacityRun(i64),
    FleetCapacityStep(ix_board::fleet::ledger::capacity::OpenCapacityWindow),
    FleetCapacityMeasurement(Box<ix_board::fleet::capacity::WindowMeasurements>),
    FleetCapacityLevel {
        recorded_level: Option<u32>,
    },
    Hygiene(ix_board::host_hygiene::HygieneReport),
    BuildStatus(
        Vec<(
            ix_board::host_hygiene::BuildHostMetrics,
            ix_board::host_hygiene::Eligibility,
        )>,
    ),
    RetroDelta(retro::DeltaReport),
    RetroBackfill(retro::BackfillReport),
    RetroEval(retro::eval::EvalReport),
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
    #[cfg(test)]
    ProgramRoadmap(ProgramRoadmap),
    Reconcile(ReconcileReport),
    Chain(ChainReport),
    Orphans(OrphansReport),
}

#[derive(Serialize)]
struct FleetCycleReport {
    operation: String,
    session_addr: String,
    started_at_ms: Option<i64>,
    ended_at_ms: Option<i64>,
    interval_s: Option<i64>,
    cycle_id: Option<i64>,
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
    if matches!(&cli.command, Command::AgentProbe) {
        let result = std::env::var_os("HOME")
            .ok_or_else(|| "home_unavailable".to_owned())
            .and_then(|home| {
                ix_board::agents::serve_probe(std::io::stdin().lock(), &PathBuf::from(home))
                    .map_err(|e| e.to_string())
            });
        return match result {
            Ok(text) => {
                emit_result(&text);
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                emit_diagnostic(diagnostics, "error", &error);
                std::process::ExitCode::from(Outcome::Invalid)
            }
        };
    }
    if let Err(error) = validate_command(&cli.command) {
        emit_diagnostic(diagnostics, "error", &error.to_string());
        return std::process::ExitCode::from(Outcome::Invalid);
    }
    match run(cli.command) {
        Ok(output) => {
            if let Output::Cached { freshness, .. } = &output
                && let Some(reason) = &freshness.failure_reason
            {
                eprintln!("{reason}");
            }
            for warning in queue_warnings(&output) {
                emit_warning(diagnostics, warning.kind(), &warning.to_string());
            }
            let rendered = if cli.json {
                encode_output_json(&output)
            } else {
                Ok(render_human(&output))
            };
            match rendered {
                Ok(text) => {
                    emit_result(&text);
                    match &output {
                        Output::Hygiene(report) => std::process::ExitCode::from(report.exit_code()),
                        _ => std::process::ExitCode::from(output_outcome(&output)),
                    }
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
            if matches!(
                error,
                Error::LinearRateLimited { .. }
                    | Error::LinearLocalBudgetExhausted { .. }
                    | Error::QuotaAdmissionRefused { .. }
            ) {
                eprintln!("{error}");
            } else {
                emit_diagnostic(diagnostics, "error", &format_cli_error(&error));
            }
            std::process::ExitCode::from(match error {
                Error::Assign(ref assign) if assign.operational() => Outcome::Internal,
                Error::Fleet(ix_board::fleet::FleetError::Tick(_))
                | Error::Fleet(ix_board::fleet::FleetError::Relay(_)) => Outcome::Internal,
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
                | Error::Drift(_)
                | Error::Cadence(_)
                | Error::Fleet(_)
                | Error::Assign(_)
                | Error::Retro(_) => Outcome::Invalid,
                Error::EmptyCredentialInput => Outcome::Invalid,
                _ => Outcome::Internal,
            })
        }
    }
}

#[implements("FR-047-AC-7")]
fn encode_output_json(output: &Output) -> Result<String, String> {
    match output {
        Output::RetroEval(report) => {
            ix_cli_kit::json::encode_canonical(report, true).map_err(|error| error.to_string())
        }
        _ => ix_cli_kit::json::encode_canonical(output, true).map_err(|error| error.to_string()),
    }
}

#[implements("FR-049-AC-1")]
fn output_outcome(output: &Output) -> Outcome {
    match output {
        Output::Cached { result, .. } => output_outcome(result),
        Output::Issues(batch) if batch.issues.is_empty() && !batch.missing.is_empty() => {
            Outcome::Invalid
        }
        Output::Fleet(report) => match report.coverage() {
            ix_board::fleet::Coverage::Complete => Outcome::Ok,
            ix_board::fleet::Coverage::Partial => Outcome::Partial,
            ix_board::fleet::Coverage::Failed => Outcome::Internal,
        },
        Output::FleetAssign(report) if report.confirmed() => Outcome::Ok,
        Output::FleetAssign(report) if report.operational_failure() => Outcome::Internal,
        Output::FleetAssign(_) => Outcome::Partial,
        Output::FleetTick(tick) if !tick.complete => Outcome::Partial,
        Output::CadenceDue(report) if report.exit_code() == 1 => Outcome::Partial,
        Output::CadenceDue(report) if report.exit_code() == 2 => Outcome::Refused,
        Output::CadenceRun(report) if !report.succeeded => Outcome::Internal,
        Output::FleetSaturation(report) if !report.complete => Outcome::Partial,
        Output::Agents(report) => match report.coverage() {
            ix_board::agents::Coverage::Complete => Outcome::Ok,
            ix_board::agents::Coverage::Partial => Outcome::Partial,
            ix_board::agents::Coverage::Failed => Outcome::Internal,
        },
        Output::BuildStatus(rows)
            if rows
                .iter()
                .any(|(_, verdict)| *verdict == ix_board::host_hygiene::Eligibility::Unknown) =>
        {
            Outcome::Partial
        }
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
        return "Linear credential store unavailable or access denied for this binary; if another ix-board build works, grant this build access to the saved ix-board item in Keychain Access (see docs/setup.md)".to_owned();
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
        Command::Cache {
            command: CacheCommand::Status,
        } => ix_board::cache_status::read_status_with_decision(&ix_board::default_cache_path()?)
            .map(Output::CacheStatus),
        Command::Auth { command } => run_auth(command),
        Command::Drift {
            config,
            evidence,
            live,
        } => run_drift(&config, evidence.as_ref(), live),
        Command::Agents {
            teams,
            idle_after,
            lease_metrics_url,
        } => run_agents(&teams, idle_after, lease_metrics_url.as_deref()).map(Output::Agents),
        Command::Hygiene { command } => match command {
            HygieneCommand::Check(args) => run_hygiene(&args, false).map(Output::Hygiene),
            HygieneCommand::Clean { args, apply } => run_hygiene(&args, apply).map(Output::Hygiene),
        },
        Command::Build {
            command:
                BuildCommand::Status {
                    metrics_url,
                    policies,
                },
        } => run_build_status(&metrics_url, &policies).map(Output::BuildStatus),
        Command::Fleet {
            command: FleetCommand::Status,
        } => run_fleet_status().map(Output::Fleet),
        Command::Fleet {
            command: FleetCommand::Tick { session },
        } => ix_board::fleet::tick::run(ix_board::settings_value()?.as_ref(), session.as_deref())
            .map(Output::FleetTick)
            .map_err(Error::Fleet),
        Command::Cadence { command } => run_cadence(command),
        Command::Fleet {
            command: FleetCommand::Saturation { command },
        } => run_fleet_saturation(matches!(command, Some(SaturationCommand::Collect)))
            .map(Output::FleetSaturation),
        Command::Fleet {
            command:
                FleetCommand::Assign {
                    lane,
                    tickets,
                    deadline,
                },
        } => run_fleet_assign(lane, tickets, deadline).map(Output::FleetAssign),
        Command::Fleet {
            command:
                FleetCommand::Ledger {
                    command: LedgerCommand::Query { view, since },
                },
        } => run_fleet_ledger_query(&view, since).map(Output::FleetLedger),
        Command::Fleet {
            command:
                FleetCommand::Ledger {
                    command: LedgerCommand::RouteDefect { defect_id, ticket },
                },
        } => run_fleet_ledger_route_defect(defect_id, &ticket).map(|routed_at_ms| {
            Output::FleetDefectRoute {
                defect_id,
                ticket,
                routed_at_ms,
            }
        }),
        Command::Fleet {
            command: FleetCommand::Cycle { command },
        } => match command {
            CycleCommand::Start { session, interval } => {
                run_fleet_cycle_start(&session, interval).map(Output::FleetCycle)
            }
            CycleCommand::End { session } => run_fleet_cycle_end(&session).map(Output::FleetCycle),
        },
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command: CapacityCommand::Status,
                },
        } => run_fleet_capacity_status().map(Output::FleetCapacity),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command:
                        CapacityCommand::Start {
                            host,
                            trigger,
                            at_ms,
                        },
                },
        } => run_fleet_capacity_start(&host, &trigger, at_ms).map(Output::FleetCapacityRun),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command:
                        CapacityCommand::Step {
                            host,
                            lanes,
                            lane_added_at_ms,
                            rampup_end_ms,
                            repeat_of,
                        },
                },
        } => run_fleet_capacity_step(&host, lanes, repeat_of, lane_added_at_ms, rampup_end_ms)
            .map(Output::FleetCapacityStep),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command: CapacityCommand::Window { host, end_ms },
                },
        } => run_fleet_capacity_window(&host, end_ms)
            .map(|measurement| Output::FleetCapacityMeasurement(Box::new(measurement))),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command:
                        CapacityCommand::End {
                            host,
                            reason,
                            at_ms,
                        },
                },
        } => run_fleet_capacity_end(&host, &reason, at_ms)
            .map(|recorded_level| Output::FleetCapacityLevel { recorded_level }),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command:
                        CapacityCommand::Drain {
                            host,
                            lane,
                            reason,
                            at_ms,
                        },
                },
        } => run_fleet_capacity_drain(&host, &lane, &reason, at_ms).map(Output::FleetCapacity),
        Command::Fleet {
            command:
                FleetCommand::Capacity {
                    command: CapacityCommand::Remove { host, lane, at_ms },
                },
        } => run_fleet_capacity_remove(&host, &lane, at_ms).map(Output::FleetCapacity),
        Command::Retro {
            command: RetroCommand::Snapshot { config, window },
        } => run_retro_snapshot(&config, &window),
        Command::Retro {
            command: RetroCommand::Delta { config },
        } => run_retro_delta(&config).map(Output::RetroDelta),
        Command::Retro {
            command:
                RetroCommand::Backfill {
                    config,
                    since,
                    until,
                    window,
                },
        } => run_retro_backfill(&config, &since, until.as_deref(), &window)
            .map(Output::RetroBackfill),
        Command::Retro {
            command:
                RetroCommand::Eval {
                    config,
                    team,
                    since,
                    role,
                    session,
                },
        } => run_retro_eval(
            &config,
            team.as_deref(),
            &since,
            role.as_deref(),
            session.as_deref(),
        )
        .map(Output::RetroEval),
        Command::Scope {
            command: ProgramScopeCommand::Check { scope },
        } => {
            let scope = ProgramScope::load(scope).map_err(|error| Error::ProgramReview {
                message: error.to_string(),
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let read = source.summary_snapshot_cached()?;
            let report = ix_board::program_review::check_summary_scope(&scope, &read.data)?;
            Ok(Output::Cached {
                result: Box::new(Output::ProgramScopeCheck(report)),
                freshness: read.freshness,
            })
        }
        Command::Roadmap { scope } => {
            let scope = scope.map(ProgramScope::load).transpose().map_err(|error| {
                Error::ProgramReview {
                    message: error.to_string(),
                }
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let read = source.summary_snapshot_cached()?;
            let mut report = ix_board::program_review::build_summary_roadmap(
                &read.data,
                scope.as_ref(),
                &format_utc(read.last_successful_sync),
            );
            count_roadmap_from_cache(&mut report, source);
            Ok(Output::Cached {
                result: Box::new(Output::SummaryRoadmap(report)),
                freshness: read.freshness,
            })
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
            let mut service = ReadService::open_default(source)?;
            let workspace = service.workspace_snapshot()?;
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
            .map(|report| label_cached(Output::Reconcile(report), service.cache_freshness()))
            .map_err(Error::from)
        }
        Command::Chain { issue, scope } => {
            let scope = scope.map(ProgramScope::load).transpose().map_err(|error| {
                Error::ProgramReview {
                    message: error.to_string(),
                }
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let mut service = ReadService::open_default(source)?;
            let workspace = service.workspace_snapshot()?;
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
                .map(|report| label_cached(Output::Chain(report), service.cache_freshness()))
                .map_err(Error::from)
        }
        Command::Orphans { scope } => {
            let scope = ProgramScope::load(scope).map_err(|error| Error::ProgramReview {
                message: error.to_string(),
            })?;
            let source = LinearGraphQlSource::from_env()?;
            let mut service = ReadService::open_default(source)?;
            let workspace = service.workspace_snapshot()?;
            find_orphans(&scope, &workspace)
                .map(|report| label_cached(Output::Orphans(report), service.cache_freshness()))
                .map_err(Error::from)
        }
        other => run_linear(other),
    }
}

/// Adds ticket counts from the issue snapshot already in the local cache. It makes no
/// Linear request; without a readable snapshot the counts stay unavailable (FR-020).
fn count_roadmap_from_cache(
    report: &mut ix_board::program_review::SummaryRoadmap,
    source: LinearGraphQlSource,
) {
    match ReadService::open_default(source)
        .and_then(|service| service.cached_workspace_snapshot())
    {
        Ok(Some(detail)) => report.count_from_issues(&detail, &format_utc(detail.fetched_at)),
        Ok(None) => report.counts_unavailable(
            "no cached issue snapshot; roadmap makes no issue requests, so run a focus command first",
        ),
        Err(error) => {
            report.counts_unavailable(&format!("cached issue snapshot is unreadable: {error}"));
        }
    }
}

#[implements("FR-039-AC-14")]
fn run_fleet_status() -> Result<ix_board::fleet::FleetReport, Error> {
    use ix_board::fleet::live;
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let series: Option<std::sync::Arc<dyn ix_board::fleet::SeriesSource>> = config
        .prometheus_url
        .as_ref()
        .map(|url| match live::PromHttp::new(url, config.host_timeout) {
            Ok(query) => std::sync::Arc::new(live::PrometheusSeries { query })
                as std::sync::Arc<dyn ix_board::fleet::SeriesSource>,
            Err(error) => std::sync::Arc::new(live::UnavailablePrometheus(error.to_string())),
        });
    let (relay, receipts) = live::read_relay(config.host_timeout);
    let role_observations = relay
        .as_ref()
        .ok()
        .map(|snapshot| snapshot.events.clone())
        .unwrap_or_default();
    let sources = ix_board::fleet::Sources {
        relay,
        series,
        receipts: &receipts,
        weak: &live::NoWeakSignals,
    };
    let local_now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        });
    let mut report = ix_board::fleet::build_report(&config, &sources, local_now_ms);
    let tick_settings = match ix_board::fleet::tick::TickSettings::from_toml(settings.as_ref()) {
        Ok(settings) => settings,
        Err(error) => {
            report.add_unreadable_source("config:fleet_tick", error.to_string());
            ix_board::fleet::tick::TickSettings::default()
        }
    };
    record_run(&config, &tick_settings, &role_observations, &mut report);
    Ok(report)
}

/// Commits the run, completing the `assignment_history` rows a killed `fleet assign` left pending, or
/// confirmed before their deadline, in the same pass (FR-035 behavior 10). A relay that cannot be
/// reached is already a defect of the report, and the run is recorded without the completion.
#[implements("FR-039-AC-14")]
fn record_run(
    config: &ix_board::fleet::FleetConfig,
    tick_settings: &ix_board::fleet::tick::TickSettings,
    role_observations: &[ix_board::fleet::RawEvent],
    report: &mut ix_board::fleet::FleetReport,
) {
    ix_board::fleet::live::record_session_turn_page(
        config,
        tick_settings,
        role_observations,
        report,
    );
    let relay = relay_core::config::load_core(relay_core::config::CoreFlags::default())
        .ok()
        .and_then(|settings| {
            let relay =
                relay_core::Relay::connect(&settings.url.value, config.host_timeout).ok()?;
            Some(relay.with_prefix(settings.prefix.value.clone()))
        });
    match relay {
        Some(mut relay) => {
            ix_board::fleet::ledger::record_status_completing(
                config,
                tick_settings,
                report,
                &mut relay,
            );
        }
        None => ix_board::fleet::ledger::record_status(config, report),
    }
}

fn run_fleet_ledger_query(
    view: &str,
    since: Option<u64>,
) -> Result<ix_board::fleet::ledger::LedgerRows, Error> {
    use ix_board::fleet::{FleetError, ledger};
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    ledger::query(&path, view, since.map(std::time::Duration::from_secs))
        .map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))
}

#[implements("FR-039-AC-20")]
fn run_fleet_ledger_route_defect(defect_id: i64, ticket: &str) -> Result<i64, Error> {
    use ix_board::fleet::{FleetError, ledger};
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let (relay, _receipts) = ix_board::fleet::live::read_relay(config.host_timeout);
    let relay = relay.map_err(|error| Error::Fleet(FleetError::Relay(error.to_string())))?;
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let mut store = ledger::Ledger::open(&path, &config)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))?;
    store
        .record_defect_route(defect_id, ticket, relay.now_ms)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))?;
    Ok(relay.now_ms)
}

#[implements("FR-044-AC-1")]
fn run_fleet_cycle_start(
    session_addr: &str,
    interval_s: Option<u64>,
) -> Result<FleetCycleReport, Error> {
    use ix_board::fleet::{FleetError, ledger};
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let (relay, _receipts) = ix_board::fleet::live::read_relay(config.host_timeout);
    let relay = relay.map_err(|error| Error::Fleet(FleetError::Relay(error.to_string())))?;
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let mut store = ledger::Ledger::open(&path, &config)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))?;
    let interval_s = interval_s
        .map(i64::try_from)
        .transpose()
        .map_err(|_| {
            Error::Fleet(FleetError::Config(
                "cycle interval overflows i64".to_owned(),
            ))
        })?
        .unwrap_or_else(|| i64::try_from(config.cycle_interval.as_secs()).unwrap_or(i64::MAX));
    let cycle_id = store
        .cycle_start(session_addr, relay.now_ms, interval_s)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e)))?;
    Ok(FleetCycleReport {
        operation: "start".to_owned(),
        session_addr: session_addr.to_owned(),
        started_at_ms: Some(relay.now_ms),
        ended_at_ms: None,
        interval_s: Some(interval_s),
        cycle_id: Some(cycle_id),
    })
}

#[implements("FR-044-AC-1")]
fn run_fleet_cycle_end(session_addr: &str) -> Result<FleetCycleReport, Error> {
    use ix_board::fleet::{FleetError, ledger};
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let (relay, _receipts) = ix_board::fleet::live::read_relay(config.host_timeout);
    let relay = relay.map_err(|error| Error::Fleet(FleetError::Relay(error.to_string())))?;
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let mut store = ledger::Ledger::open(&path, &config)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))?;
    store
        .cycle_end(session_addr, relay.now_ms)
        .map_err(|e| Error::Fleet(FleetError::Ledger(e)))?;
    Ok(FleetCycleReport {
        operation: "end".to_owned(),
        session_addr: session_addr.to_owned(),
        started_at_ms: None,
        ended_at_ms: Some(relay.now_ms),
        interval_s: None,
        cycle_id: None,
    })
}

#[implements("FR-041-AC-17")]
fn run_fleet_capacity_status() -> Result<ix_board::fleet::ledger::CapacityStatus, Error> {
    use ix_board::fleet::{FleetError, ledger};
    let (_, path) = fleet_capacity_config()?;
    ledger::capacity::status(&path).map_err(|e| Error::Fleet(FleetError::Ledger(e.to_string())))
}

#[derive(serde::Serialize)]
struct SaturationSampleSourceBasis {
    ready_buffer: ix_board::fleet::saturation::Evidence<u32>,
    active_lanes: ix_board::fleet::saturation::Evidence<u32>,
    supplied_lanes: ix_board::fleet::saturation::Evidence<u32>,
    unsupplied_lanes: ix_board::fleet::saturation::Evidence<u32>,
    ready_share: ix_board::fleet::saturation::Evidence<u32>,
    queued_behind_first: ix_board::fleet::saturation::Evidence<u32>,
    bootstrap_ticket_pool: ix_board::fleet::saturation::Evidence<u32>,
    registry_current: ix_board::fleet::saturation::Evidence<bool>,
    deficit: ix_board::fleet::saturation::Evidence<u32>,
}

#[implements("FR-041-AC-5")]
#[implements("FR-041-AC-7")]
#[implements("FR-041-AC-11")]
fn run_fleet_saturation(
    collect_sample: bool,
) -> Result<ix_board::fleet::saturation::SaturationReport, Error> {
    use ix_board::fleet::tick::TickSettings;
    use ix_board::fleet::{FleetError, ledger};
    let settings_value = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings_value.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let tick_settings = TickSettings::from_toml(settings_value.as_ref())?;
    let report =
        ix_board::fleet::saturation::read_live(&config, &tick_settings).map_err(Error::Fleet)?;
    if collect_sample {
        let path = ledger::resolve_path(&config)
            .map_err(|error| Error::Fleet(FleetError::Ledger(error.to_string())))?;
        let mut samples = Vec::with_capacity(report.hosts.len());
        for host in &report.hosts {
            let assignment_reason = host
                .supplied_lanes
                .is_none()
                .then(|| host.ready_share.reason.clone())
                .flatten();
            let source_basis = SaturationSampleSourceBasis {
                ready_buffer: report.ready_buffer.clone(),
                active_lanes: ix_board::fleet::saturation::Evidence::available(
                    host.active_lanes,
                    "relay.current_lane_state",
                    1.0,
                ),
                supplied_lanes: ix_board::fleet::saturation::Evidence {
                    value: host.supplied_lanes,
                    source: "relay.assignments+agent-msg.work.events+linear.issue_state".into(),
                    reason: assignment_reason.clone(),
                    coverage: host.ready_share.coverage,
                },
                unsupplied_lanes: ix_board::fleet::saturation::Evidence {
                    value: host.unsupplied_lanes,
                    source: "relay.assignments+agent-msg.work.events+relay.registry".into(),
                    reason: assignment_reason,
                    coverage: host.ready_share.coverage,
                },
                ready_share: host.ready_share.clone(),
                queued_behind_first: host.queued_behind_first.clone(),
                bootstrap_ticket_pool: host.bootstrap_ticket_pool.clone(),
                registry_current: ix_board::fleet::saturation::Evidence::available(
                    host.registry_current,
                    "relay.registry",
                    1.0,
                ),
                deficit: host.ready_buffer_deficit.clone(),
            };
            let source_basis = serde_json::to_string(&source_basis)
                .map_err(|error| Error::Fleet(FleetError::Ledger(error.to_string())))?;
            samples.push(ledger::capacity::SaturationSampleInput {
                host: &host.host,
                active_lanes: host.active_lanes,
                supplied_lanes: host.supplied_lanes,
                unsupplied_lanes: host.unsupplied_lanes,
                registry_current: host.registry_current,
                ready_buffer: report.ready_buffer.value,
                ready_share: host.ready_share.value,
                queued_behind_first: host.queued_behind_first.value,
                bootstrap_ticket_pool: host.bootstrap_ticket_pool.value,
                deficit: host.ready_buffer_deficit.value,
                source_basis,
                reason: host.ready_buffer_deficit.reason.as_deref(),
                coverage: host.ready_share.coverage,
            });
        }
        ledger::capacity::record_saturation_samples(
            &path,
            &config,
            report.observed_at_ms,
            &samples,
        )
        .map_err(|error| Error::Fleet(FleetError::Ledger(error.to_string())))?;
    }
    Ok(report)
}

fn fleet_capacity_config() -> Result<(ix_board::fleet::FleetConfig, PathBuf), Error> {
    use ix_board::fleet::{FleetError, ledger};
    let settings = ix_board::settings_value()?;
    let config = ix_board::fleet::FleetConfig::from_toml(
        settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )?;
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    Ok((config, path))
}

#[implements("FR-041-AC-14")]
fn run_fleet_capacity_start(host: &str, trigger: &str, at_ms: i64) -> Result<i64, Error> {
    let (config, path) = fleet_capacity_config()?;
    ix_board::fleet::ledger::capacity::start(&path, &config, host, trigger, at_ms)
        .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))
}

#[implements("FR-041-AC-11")]
fn run_fleet_capacity_step(
    host: &str,
    lanes: u32,
    repeat_of: Option<u32>,
    lane_added_at_ms: i64,
    rampup_end_ms: i64,
) -> Result<ix_board::fleet::ledger::capacity::OpenCapacityWindow, Error> {
    let (config, path) = fleet_capacity_config()?;
    let status = ix_board::fleet::ledger::capacity::status(&path)
        .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))?;
    let run_id = status
        .hosts
        .iter()
        .find(|entry| entry.host == host)
        .and_then(|entry| entry.current_run)
        .ok_or_else(|| {
            Error::Fleet(ix_board::fleet::FleetError::Ledger(format!(
                "host {host} has no open capacity run"
            )))
        })?;
    ix_board::fleet::ledger::capacity::open_step(
        &path,
        &config,
        run_id,
        lanes,
        repeat_of,
        lane_added_at_ms,
        rampup_end_ms,
    )
    .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))
}

#[implements("FR-041-AC-13")]
fn run_fleet_capacity_end(host: &str, reason: &str, at_ms: i64) -> Result<Option<u32>, Error> {
    let (config, path) = fleet_capacity_config()?;
    ix_board::fleet::ledger::capacity::end(&path, &config, host, reason, at_ms)
        .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))
}

#[implements("FR-041-AC-14")]
fn run_fleet_capacity_drain(
    host: &str,
    lane: &str,
    reason: &str,
    at_ms: i64,
) -> Result<ix_board::fleet::ledger::CapacityStatus, Error> {
    let (config, path) = fleet_capacity_config()?;
    let settings_value = ix_board::settings_value()?;
    let tick_settings = ix_board::fleet::tick::TickSettings::from_toml(settings_value.as_ref())?;
    ix_board::fleet::ledger::capacity::mark_draining(
        &path,
        &config,
        host,
        lane,
        reason,
        at_ms,
        tick_settings.drain_cap,
    )
    .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))?;
    ix_board::fleet::ledger::capacity::status(&path)
        .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))
}

#[implements("FR-041-AC-19")]
fn run_fleet_capacity_remove(
    host: &str,
    lane: &str,
    at_ms: i64,
) -> Result<ix_board::fleet::ledger::CapacityStatus, Error> {
    use ix_board::fleet::ledger::capacity::CapacityEventInput;
    let (config, path) = fleet_capacity_config()?;
    ix_board::fleet::ledger::capacity::record_event(
        &path,
        &config,
        &CapacityEventInput {
            host,
            lane: Some(lane),
            kind: "lane_removed",
            reason: Some("fleet_capacity_remove"),
            evidence_kind: None,
            cycle_id: None,
            at_ms,
            ceiling_until: None,
        },
    )
    .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))?;
    ix_board::fleet::ledger::capacity::status(&path)
        .map_err(|error| Error::Fleet(ix_board::fleet::FleetError::Ledger(error.to_string())))
}

#[implements("FR-041-AC-10")]
fn run_fleet_capacity_window(
    host: &str,
    end_ms: i64,
) -> Result<ix_board::fleet::capacity::WindowMeasurements, Error> {
    use ix_board::fleet::{FleetError, capacity::LiveMeasurementSource, ledger, live::PromHttp};
    let (config, path) = fleet_capacity_config()?;
    let status = ledger::capacity::status(&path)
        .map_err(|error| Error::Fleet(FleetError::Ledger(error.to_string())))?;
    let run_id = status
        .hosts
        .iter()
        .find(|entry| entry.host.eq_ignore_ascii_case(host))
        .and_then(|entry| entry.current_run)
        .ok_or_else(|| {
            Error::Fleet(FleetError::Ledger(format!(
                "host {host} has no open capacity run"
            )))
        })?;
    let prometheus = config
        .prometheus_url
        .as_deref()
        .map(|url| PromHttp::new(url, config.host_timeout))
        .transpose()
        .map_err(|error| Error::Fleet(FleetError::Tick(error.to_string())))?;
    let source = LiveMeasurementSource {
        config: &config,
        prometheus: prometheus
            .as_ref()
            .map(|query| query as &dyn ix_board::fleet::live::PromQuery),
    };
    ledger::capacity::measure_open_step(&path, &config, run_id, end_ms, &source)
        .map_err(|error| Error::Fleet(FleetError::Ledger(error.to_string())))
}

fn run_fleet_assign(
    lane: String,
    tickets: Vec<String>,
    deadline_secs: u64,
) -> Result<ix_board::fleet::assign::AssignReport, Error> {
    use ix_board::fleet::{assign, live};
    let config_error = |e: String| Error::Fleet(ix_board::fleet::FleetError::Config(e));
    let file = relay_core::config::load_file()
        .map_err(|e| config_error(e.to_string()))?
        .unwrap_or(relay_core::config::FileConfig::default());
    let settings = relay_core::config::load_core(relay_core::config::CoreFlags::default())
        .map_err(|e| config_error(e.to_string()))?;
    let board = ix_board::settings_value()?;
    let fleet = ix_board::fleet::FleetConfig::from_toml(board.as_ref(), None)?;
    let source = LinearGraphQlSource::from_env()?;
    let mut service = ReadService::open_default(source)?;
    let batch = service.issues(&tickets)?;
    let mut relay =
        relay_core::Relay::connect(&settings.url.value, std::time::Duration::from_secs(5))
            .map_err(|e| assign::AssignError::Relay(format!("connect: {e}")))?
            .with_prefix(settings.prefix.value.clone());
    let caller = if file.assign.is_orchestrator().unwrap_or(false) {
        live::caller_lane(&mut relay, &settings.machine.value)
    } else {
        None
    };
    let writer = relay_core::assignment::AssignWriter::from_config(
        &file.assign,
        &settings.machine.value,
        caller,
    )
    .map_err(config_error)?;
    let request = assign::AssignRequest {
        lane,
        tickets,
        deadline: std::time::Duration::from_secs(deadline_secs),
        poll: std::time::Duration::from_secs(2),
        focus_window_ms: i64::try_from(fleet.focus_window.as_millis()).unwrap_or(i64::MAX),
        team_keys: fleet.team_keys.clone(),
    };
    let mut sink = ix_board::fleet::ledger::AssignSink::resolve(&fleet)
        .map_err(|e| assign::AssignError::Ledger(e.to_string()))?;
    Ok(assign::assign(
        &mut relay, &mut sink, &request, &batch, &writer,
    )?)
}

fn run_agents(
    teams: &[String],
    idle_after: u64,
    lease_metrics_url: Option<&str>,
) -> Result<AgentsReport, Error> {
    let hosts = ix_board::agents::system_hosts()?;
    let metrics_url = lease_metrics_url
        .map(str::to_owned)
        .or_else(|| std::env::var("IX_BUILD_METRICS_URL").ok());
    let (leases, lease_error) = match metrics_url.as_deref() {
        Some(url) => match fetch_metrics_text(url) {
            Ok(text) => (
                ix_board::host_hygiene::parse_build_metrics(&text)
                    .into_values()
                    .flat_map(|host| host.leases_by_lane.into_keys())
                    .map(|address| address.to_lowercase())
                    .collect(),
                None,
            ),
            Err(error) => (std::collections::BTreeSet::new(), Some(error.to_string())),
        },
        None => (
            std::collections::BTreeSet::new(),
            Some("locked-build metrics are unavailable; configure IX_BUILD_METRICS_URL".into()),
        ),
    };
    let mut service = None;
    let mut warnings = BTreeSet::new();
    let mut next_ready = |team: &str| -> Result<Option<ReadyTicket>, String> {
        if service.is_none() {
            let source = LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
            service = Some(ReadService::open_default(source).map_err(|error| error.to_string())?);
        }
        let read = service
            .as_mut()
            .ok_or_else(|| "ready queue unavailable".to_owned())?
            .next_with_warnings(team)
            .map_err(|error| error.to_string())?;
        Ok(first_ready(read, &mut warnings))
    };
    let mut report = ix_board::agents::build_report_with_leases(
        &hosts,
        teams,
        idle_after,
        ix_board::agents::HOST_TIMEOUT,
        &mut next_ready,
        &leases,
    );
    report.errors.extend(lease_error);
    report.ready_queue_warnings = warnings.into_iter().collect();
    Ok(report)
}

#[implements("FR-048-AC-7", "FR-048-AC-9")]
fn fetch_metrics_text(url: &str) -> Result<String, Error> {
    const MAX_METRICS_BYTES: u64 = 8 * 1024 * 1024;
    let mut response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?
        .get(url)
        .send()
        .map_err(|error| Error::ProgramReview {
            message: format!("metrics request failed: {}", error.without_url()),
        })?
        .error_for_status()
        .map_err(|error| Error::ProgramReview {
            message: format!("metrics response failed: {}", error.without_url()),
        })?;
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_METRICS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Error::ProgramReview {
            message: format!("cannot read metrics exposition: {error}"),
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_METRICS_BYTES {
        return Err(Error::ProgramReview {
            message: "metrics exposition exceeds 8 MiB".to_owned(),
        });
    }
    String::from_utf8(bytes).map_err(|error| Error::ProgramReview {
        message: format!("metrics exposition is not UTF-8: {error}"),
    })
}

fn run_retro_snapshot(config_path: &std::path::Path, window: &str) -> Result<Output, Error> {
    let config = retro::RetroConfig::load(config_path).map_err(retro::RetroError::from)?;
    let window_secs = retro::parse_window(window).map_err(retro::RetroError::from)?;
    let store = retro::Store::from_env(&config.team).map_err(retro::RetroError::from)?;
    // A refused store path must cost no reads, so check it before any source is started.
    store.check_writable().map_err(retro::RetroError::from)?;
    let linear = retro::live::linear_or_unreadable();
    let agents = retro::live::LiveAgents::start(config.herdr_teams.clone());
    let sources = retro::Sources {
        linear: linear.as_ref(),
        github: &retro::live::LiveGithub,
        agents: &agents,
        gate_log: &retro::live::FileGateLog,
        relay: &retro::live::RedisCliRelay,
        local: &retro::live::SystemLocal,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let record = retro::run_snapshot(&config, window_secs, now, &sources, &store)?;
    if let Some(notice) = retro::unavailable_notice(&record) {
        eprintln!("{notice}");
    }
    Ok(label_cached(
        Output::Retro(Box::new(retro::SnapshotOutput::try_from_record(record)?)),
        linear.cache_freshness().as_ref(),
    ))
}

fn run_retro_delta(config_path: &std::path::Path) -> Result<retro::DeltaReport, Error> {
    let config = retro::RetroConfig::load(config_path).map_err(retro::RetroError::from)?;
    let store = retro::Store::from_env(&config.team).map_err(retro::RetroError::from)?;
    Ok(retro::run_delta(&config, &store)?)
}

fn run_retro_backfill(
    config_path: &std::path::Path,
    since: &str,
    until: Option<&str>,
    window: &str,
) -> Result<retro::BackfillReport, Error> {
    let config = retro::RetroConfig::load(config_path).map_err(retro::RetroError::from)?;
    let window_secs = retro::parse_window(window).map_err(retro::RetroError::from)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let first = retro::parse_day(since)?;
    let last = until.map(retro::parse_day).transpose()?;
    let store = retro::Store::from_env(&config.team).map_err(retro::RetroError::from)?;
    // A refused store path must cost no reads, so check it before any source is started.
    store.check_writable().map_err(retro::RetroError::from)?;
    let linear = retro::live::linear_or_unreadable();
    Ok(retro::run_backfill(
        &config,
        window_secs,
        (first, last),
        now,
        (linear.as_ref(), &retro::live::LiveGithub),
        &store,
    )?)
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-2")]
#[implements("FR-047-AC-3")]
#[implements("FR-047-AC-7")]
#[implements("FR-047-AC-9")]
#[implements("FR-047-AC-4")]
#[implements("FR-047-AC-8")]
fn run_retro_eval(
    config_path: &std::path::Path,
    team_override: Option<&str>,
    since: &str,
    role_filter: Option<&str>,
    session_filter: Option<&str>,
) -> Result<retro::eval::EvalReport, Error> {
    let run_scope = retro::eval::EvalRunScope::start(
        std::time::Duration::from_secs(240),
        retro::eval::DEFAULT_EVAL_LINEAR_REQUEST_LIMIT,
    )
    .map_err(|error| retro::RetroError::Evaluation(error.to_string()))?;
    let request_start = retro::eval::request_count_snapshot();
    let mut config = retro_eval_stage("configuration", || retro::RetroConfig::load(config_path))
        .map_err(retro::RetroError::from)?;
    let team = team_override.unwrap_or(&config.team).to_owned();
    config.team = team.clone();
    let settings = retro::eval::load_settings(&team).map_err(retro::RetroError::Evaluation)?;
    let specs_dir = retro::eval::resolve_role_spec_dir(&team, &settings);
    let mut specs = retro_eval_stage("role_specs", || retro::eval::read_role_specs(&specs_dir))
        .map_err(retro::RetroError::Evaluation)?;
    let source_specs = retro::eval::specs_with_objective_metric_queries(&specs);
    let duration = retro::parse_window(since).map_err(retro::RetroError::from)?;
    const MAX_EVAL_WINDOW_SECONDS: u64 = 31 * 24 * 60 * 60;
    if duration > MAX_EVAL_WINDOW_SECONDS {
        return Err(retro::RetroError::Evaluation(format!(
            "--since exceeds the maximum retro-eval window of {MAX_EVAL_WINDOW_SECONDS} seconds"
        ))
        .into());
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_i64, |elapsed| {
            elapsed.as_millis().min(i64::MAX as u128) as i64
        });
    let duration_ms = duration.saturating_mul(1000).min(i64::MAX as u64) as i64;
    let window = retro::eval::EvalWindow {
        start_ms: now_ms.saturating_sub(duration_ms),
        end_ms: now_ms,
    };
    let previous_window = retro::eval::EvalWindow {
        start_ms: window.start_ms.saturating_sub(duration_ms),
        end_ms: window.start_ms,
    };
    if let Some(role) = role_filter {
        if !specs.iter().any(|spec| spec.role == role) {
            return Err(retro::RetroError::Evaluation(format!(
                "unknown role {role:?}; no matching role specification"
            ))
            .into());
        }
        specs.retain(|spec| spec.role == role);
    }
    let store_read = retro_eval_stage_bounded("retro_store", &run_scope, || {
        retro::Store::from_env(&team)
            .and_then(|store| store.read_all())
            .map_err(|error| error.to_string())
    })
    .unwrap_or_else(Err);
    let records = store_read
        .as_ref()
        .map(Vec::as_slice)
        .map_err(String::as_str);
    let current_live_record =
        retro_eval_stage_bounded("current.live_retro_record", &run_scope, || {
            collect_eval_retro_record(&config, &window, &source_specs, &settings)
        })
        .unwrap_or_else(Err);
    let fleet_settings = ix_board::settings_value().ok().flatten();
    let fleet_config = ix_board::fleet::FleetConfig::from_toml(
        fleet_settings.as_ref(),
        std::env::var("IX_PROMETHEUS_URL").ok(),
    )
    .ok();
    let prometheus_url = fleet_config
        .as_ref()
        .and_then(|config| config.prometheus_url.as_deref());
    let fleet_ledger_path = fleet_config
        .as_ref()
        .and_then(|config| ix_board::fleet::ledger::resolve_path(config).ok());
    let tick_settings =
        ix_board::fleet::tick::TickSettings::from_toml(fleet_settings.as_ref()).ok();
    let needs_relay = source_specs
        .iter()
        .flat_map(|spec| &spec.rules)
        .any(|rule| rule.method == "deterministic" && rule.query_or_rubric.starts_with("relay."));
    let relay_settings = needs_relay
        .then(|| relay_core::config::load_core(relay_core::config::CoreFlags::default()).ok())
        .flatten();
    let session_ledger_rows = retro_eval_stage_bounded("session_turn_ledger", &run_scope, || {
        fleet_ledger_path
            .as_deref()
            .ok_or_else(|| "fleet ledger path is unavailable for session_turn".to_owned())
            .and_then(|path| {
                let source_window = retro::eval::EvalWindow {
                    start_ms: previous_window.start_ms,
                    end_ms: window.end_ms,
                };
                retro::eval::session_turn_rows(path, &source_window)
            })
    })
    .unwrap_or_else(Err);
    let all_session_rows = session_ledger_rows
        .as_ref()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let scoped_session_rows = all_session_rows
        .iter()
        .filter(|row| {
            row.get("session_addr")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|address| address_belongs_to_team(address, &config.herdr_teams))
        })
        .cloned()
        .collect::<Vec<_>>();
    let scoped_session_ledger_rows = session_ledger_rows
        .as_ref()
        .map(|_| scoped_session_rows.clone())
        .map_err(Clone::clone);
    let session_rows = scoped_session_rows.as_slice();
    let mut sessions_by_role =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for row in session_rows {
        let Some(started_at) = row.get("started_at").and_then(serde_json::Value::as_i64) else {
            continue;
        };
        if started_at < window.start_ms || started_at >= window.end_ms {
            continue;
        }
        let (Some(address), Some(role)) = (
            row.get("session_addr").and_then(serde_json::Value::as_str),
            row.get("session_role").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if !address_belongs_to_team(address, &config.herdr_teams) {
            continue;
        }
        sessions_by_role
            .entry(role.to_owned())
            .or_default()
            .insert(address.to_owned());
    }
    let lane_order_scope = retro::eval::LaneOrderScope {
        team_id: Some(team.clone()),
        sessions_by_role,
    };
    let session_lane_order_adapters =
        retro_eval_stage_bounded("current.session_lane_orders", &run_scope, || {
            collect_session_lane_order_adapters(
                &source_specs,
                fleet_ledger_path.as_deref(),
                settings.ready_state.as_deref(),
                &config.projects,
                &window,
                &team,
                &config.herdr_teams,
                session_rows,
            )
        })
        .unwrap_or_else(Err)
        .unwrap_or_default();
    let mut current_adapters = retro::eval::declared_adapters(&source_specs);
    current_adapters.extend(retro::eval::retro_adapters(&source_specs, records, &window));
    current_adapters.extend(retro::eval::retro_record_adapters(
        &source_specs,
        current_live_record.as_ref().map_err(String::as_str),
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.prometheus",
        &run_scope,
        &source_specs,
        "prom.",
        || retro::eval::prometheus_adapters(&source_specs, prometheus_url, &window),
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.linear_reviewed_prs",
        &run_scope,
        &source_specs,
        "linear.",
        || {
            retro::eval::linear_reviewed_prs_adapters(
                &source_specs,
                &config.repos,
                &window,
                settings.github_guard_state_path.as_deref(),
                settings.github_guard_max_bytes,
            )
        },
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.linear_markers",
        &run_scope,
        &source_specs,
        "linear.",
        || retro::eval::linear_marker_adapters(&source_specs, &config.repos, &window),
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.github_merged_prs",
        &run_scope,
        &source_specs,
        "github.",
        || {
            retro::eval::github_merged_prs_adapters(
                &source_specs,
                &config.repos,
                &window,
                settings.github_guard_state_path.as_deref(),
                settings.github_guard_max_bytes,
            )
        },
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.linear_merge_without_review",
        &run_scope,
        &source_specs,
        "linear.",
        || {
            retro::eval::linear_merged_prs_without_review_adapters(
                &source_specs,
                &config.repos,
                &window,
                settings.github_guard_state_path.as_deref(),
                settings.github_guard_max_bytes,
            )
        },
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.linear_gate_records",
        &run_scope,
        &source_specs,
        "linear.",
        || {
            retro::eval::linear_gate_record_adapters(
                &source_specs,
                &config.repos,
                config.gate_log.as_deref(),
                &window,
            )
        },
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.gate_log",
        &run_scope,
        &source_specs,
        "linear.",
        || retro::eval::gate_log_adapters(&source_specs, config.gate_log.as_deref(), &window),
    ));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.fleet_ledger",
        &run_scope,
        &source_specs,
        "ledger.",
        || {
            retro::eval::ledger_adapters_scoped(
                &source_specs,
                fleet_ledger_path.as_deref(),
                settings.ready_state.as_deref(),
                &config.projects,
                &window,
                &lane_order_scope,
            )
        },
    ));
    let relay_timeout = fleet_config
        .as_ref()
        .map(|config| config.host_timeout)
        .unwrap_or_else(|| std::time::Duration::from_secs(20));
    current_adapters.extend(retro_eval_bounded_adapters(
        "current.relay",
        &run_scope,
        &source_specs,
        "relay.",
        || {
            retro::eval::relay::live_adapters(
                &source_specs,
                &window,
                fleet_ledger_path.as_deref(),
                relay_settings
                    .as_ref()
                    .map(|config| config.url.value.as_str()),
                relay_settings
                    .as_ref()
                    .map(|config| config.prefix.value.as_str()),
                tick_settings.as_ref(),
                fleet_config.as_ref().map(|config| config.tick_interval),
                relay_timeout,
            )
        },
    ));
    let transcript_turns = retro::eval::transcript::turn_rows_from_ledger(session_rows);
    let needs_transcript = source_specs.iter().any(|spec| {
        spec.rules.iter().any(|rule| {
            rule.method == "deterministic"
                && (rule.query_or_rubric.starts_with("transcript.")
                    || rule.query_or_rubric == "github.review_ready_prs_undispatched")
        }) || spec.metrics.iter().any(|metric| {
            metric.query.starts_with("transcript.")
                || metric.query == "github.ticket_start_to_pr_open_median_hours"
        })
    });
    let transcript_id_column_present = if needs_transcript {
        retro_eval_stage_bounded("session_turn.transcript_id_schema", &run_scope, || {
            fleet_ledger_path
                .as_deref()
                .ok_or_else(|| {
                    "fleet ledger path is unavailable for transcript identity".to_owned()
                })
                .and_then(retro::eval::session_turn_has_transcript_id)
        })
        .unwrap_or_else(Err)
    } else {
        Ok(false)
    };
    let transcript_inventory = if needs_transcript {
        retro_eval_stage_bounded("current.transcripts", &run_scope, || {
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| "transcript home directory is unavailable".to_owned())?;
            Ok(retro::eval::transcript::discover_transcripts(
                &home,
                &transcript_turns,
                retro::eval::transcript::TranscriptReadLimits::default(),
            ))
        })
        .unwrap_or_else(Err)
    } else {
        Ok(retro::eval::transcript::TranscriptInventory {
            turns: transcript_turns,
            transcripts: std::collections::BTreeMap::new(),
            complete: true,
            unavailable_reasons: Vec::new(),
        })
    };
    let transcript_context = retro::eval::transcript::AdapterContext {
        review_age_limit_hours: settings.review_age_limit_hours,
        ..Default::default()
    };
    let (role_transcript_adapters, session_transcript_adapters) = transcript_adapter_sets(
        &source_specs,
        &transcript_inventory,
        &window,
        &transcript_context,
        &transcript_id_column_present,
    );
    current_adapters.extend(role_transcript_adapters);
    let role_github_linear_adapters =
        retro_eval_stage_bounded("current.role_github_linear", &run_scope, || {
            collect_role_github_linear_adapters(
                &source_specs,
                &config.repos,
                config.decision_label.as_deref(),
                &window,
                &settings,
                &run_scope,
                &TranscriptDispatchContext {
                    inventory: &transcript_inventory,
                    transcript_id_column_present: &transcript_id_column_present,
                    coverage_start_ms: previous_window.start_ms,
                },
            )
        })
        .unwrap_or_else(|reason| {
            unavailable_adapters_for_prefixes(&source_specs, &["github.", "linear."], &reason)
        });
    current_adapters.extend(role_github_linear_adapters);
    let mut previous_adapters = retro::eval::declared_adapters(&source_specs);
    previous_adapters.extend(retro::eval::retro_adapters(
        &source_specs,
        records,
        &previous_window,
    ));
    // Previous-window source evidence is never recomputed from today's live state. Only the
    // append-only retro store can supply prior-window values; other sources stay unavailable.
    let mut sessions =
        retro::eval::evaluate_sessions(&specs, &current_adapters, session_rows, &window, None);
    let mut role_session_counts = std::collections::BTreeMap::<String, usize>::new();
    for session in &sessions {
        *role_session_counts.entry(session.role.clone()).or_default() += 1;
    }
    for session in &mut sessions {
        let Some(spec) = specs.iter().find(|spec| spec.role == session.role) else {
            continue;
        };
        let session_key = (session.role.clone(), session.id.clone());
        let mut session_source_adapters = session_transcript_adapters
            .get(&session_key)
            .cloned()
            .unwrap_or_default();
        if let Some(adapters) = session_lane_order_adapters.get(&session_key) {
            session_source_adapters.extend(adapters.clone());
        }
        if !session_source_adapters.is_empty() {
            let scoped_rules = spec
                .rules
                .iter()
                .filter(|rule| {
                    rule.method == "deterministic"
                        && (rule.query_or_rubric.starts_with("transcript.")
                            || is_lane_order_query(&rule.query_or_rubric))
                })
                .cloned()
                .collect::<Vec<_>>();
            let results = retro::eval::evaluate_rules(&scoped_rules, &session_source_adapters);
            for result in results {
                if let Some(existing) = session.rules.iter_mut().find(|row| row.id == result.id) {
                    *existing = result;
                }
            }
            session.score = retro::eval::score(&session.rules);
            session.failing_rules = session
                .rules
                .iter()
                .filter(|rule| rule.status == retro::eval::RuleStatus::Fail)
                .map(|rule| rule.id.clone())
                .collect();
        }
        session.objective_metrics = if role_session_counts.get(&session.role) == Some(&1) {
            retro::eval::evaluate_objective_metrics(&spec.metrics, &current_adapters)
        } else {
            retro::eval::unavailable_objective_metrics(
                &spec.metrics,
                "team-level objective metric is ambiguous for this role session",
            )
        };
        if !session_source_adapters.is_empty() {
            let scoped_metrics = spec
                .metrics
                .iter()
                .filter(|metric| {
                    metric.query.starts_with("transcript.") || is_lane_order_query(&metric.query)
                })
                .cloned()
                .collect::<Vec<_>>();
            for metric in
                retro::eval::evaluate_objective_metrics(&scoped_metrics, &session_source_adapters)
            {
                if let Some(existing) = session
                    .objective_metrics
                    .iter_mut()
                    .find(|row| row.id == metric.id)
                {
                    *existing = metric;
                } else {
                    session.objective_metrics.push(metric);
                }
            }
        }
    }
    let sessions = retro::eval::filter_sessions(sessions, session_filter);
    let roles = specs
        .into_iter()
        .map(|spec| {
            let mut current_role_adapters = current_adapters.clone();
            current_role_adapters.extend(retro::eval::session_turn_adapters(
                &spec,
                scoped_session_ledger_rows
                    .as_deref()
                    .map_err(String::as_str),
                &window,
            ));
            let mut previous_role_adapters = previous_adapters.clone();
            previous_role_adapters.extend(retro::eval::session_turn_adapters(
                &spec,
                scoped_session_ledger_rows
                    .as_deref()
                    .map_err(String::as_str),
                &previous_window,
            ));
            let rules = retro::eval::evaluate_rules(&spec.rules, &current_role_adapters);
            let previous_rules = retro::eval::evaluate_rules(&spec.rules, &previous_role_adapters);
            let score = retro::eval::score(&rules);
            let previous_score = retro::eval::score(&previous_rules);
            let (trend, trend_matched_rules) = retro::eval::trend_delta(&rules, &previous_rules);
            let failing_rules = rules
                .iter()
                .filter(|rule| rule.status == retro::eval::RuleStatus::Fail)
                .map(|rule| rule.id.clone())
                .collect();
            let objective_metrics =
                retro::eval::evaluate_objective_metrics(&spec.metrics, &current_role_adapters);
            let role_name = spec.role;
            retro::eval::RoleResult {
                role: role_name.clone(),
                objective_metrics,
                previous_score,
                score,
                trend,
                trend_matched_rules,
                rules,
                failing_rules,
                session_ids: sessions
                    .iter()
                    .filter(|session| session.role == role_name)
                    .map(|session| session.id.clone())
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    let report = retro::eval::EvalReport {
        schema_version: 1,
        team,
        window,
        previous_window,
        roles,
        sessions,
        source_requests: retro::eval::request_counts_since(request_start),
    };
    const MAX_EVAL_REPORT_BYTES: usize = 4 * 1024 * 1024;
    let report_bytes = serde_json::to_vec(&report)
        .map_err(|error| retro::RetroError::Evaluation(error.to_string()))?;
    if report_bytes.len() > MAX_EVAL_REPORT_BYTES {
        return Err(retro::RetroError::Evaluation(format!(
            "retro-eval report exceeds the {MAX_EVAL_REPORT_BYTES}-byte output limit"
        ))
        .into());
    }
    Ok(report)
}

#[implements("FR-047-AC-9")]
fn retro_eval_stage<T>(name: &str, operation: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    eprintln!("[retro-eval] stage={name} status=started");
    let result = operation();
    eprintln!(
        "[retro-eval] stage={name} status=finished duration_ms={}",
        started.elapsed().as_millis()
    );
    result
}

#[implements("FR-047-AC-9")]
fn retro_eval_stage_bounded<T>(
    name: &str,
    scope: &retro::eval::EvalRunScope,
    operation: impl FnOnce() -> T,
) -> Result<T, String> {
    scope.remaining().map_err(|error| {
        let reason = format!("evaluation deadline elapsed before {name}: {error}");
        eprintln!("[retro-eval] stage={name} status=skipped reason={reason}");
        reason
    })?;
    Ok(retro_eval_stage(name, operation))
}

#[implements("FR-047-AC-9")]
fn retro_eval_bounded_adapters(
    name: &str,
    scope: &retro::eval::EvalRunScope,
    specs: &[retro::eval::RoleSpec],
    query_prefix: &str,
    operation: impl FnOnce() -> std::collections::BTreeMap<String, retro::eval::QueryValue>,
) -> std::collections::BTreeMap<String, retro::eval::QueryValue> {
    match retro_eval_stage_bounded(name, scope, operation) {
        Ok(values) => values,
        Err(reason) => specs
            .iter()
            .flat_map(|spec| &spec.rules)
            .filter(|rule| {
                rule.method == "deterministic" && rule.query_or_rubric.starts_with(query_prefix)
            })
            .map(|rule| {
                (
                    rule.query_or_rubric.clone(),
                    retro::eval::QueryValue::Unavailable {
                        reason: reason.clone(),
                        evidence_refs: Vec::new(),
                    },
                )
            })
            .collect(),
    }
}

#[implements("FR-047-AC-1")]
fn address_belongs_to_team(address: &str, teams: &[String]) -> bool {
    let mut segments = address.split(':');
    let (Some(server), Some(workspace), Some(tab), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };
    if server.is_empty() || workspace.is_empty() || tab.is_empty() {
        return false;
    }
    teams
        .iter()
        .any(|team| workspace.eq_ignore_ascii_case(team.as_str()))
}

fn is_lane_order_query(query: &str) -> bool {
    matches!(
        query,
        "ledger.lane_orders_due" | "ledger.lane_orders_not_taken_up"
    )
}

type SessionQueryValues = std::collections::BTreeMap<String, retro::eval::QueryValue>;
type SessionAdapterValues = std::collections::BTreeMap<(String, String), SessionQueryValues>;
type TranscriptAdapterValues = (
    std::collections::BTreeMap<String, retro::eval::QueryValue>,
    SessionAdapterValues,
);

struct TranscriptDispatchContext<'a> {
    inventory: &'a Result<retro::eval::transcript::TranscriptInventory, String>,
    transcript_id_column_present: &'a Result<bool, String>,
    coverage_start_ms: i64,
}

#[implements("FR-047-AC-1")]
fn collect_session_lane_order_adapters(
    specs: &[retro::eval::RoleSpec],
    ledger_path: Option<&std::path::Path>,
    ready_state: Option<&str>,
    projects: &[String],
    window: &retro::eval::EvalWindow,
    team: &str,
    team_workspaces: &[String],
    session_rows: &[serde_json::Map<String, serde_json::Value>],
) -> Result<SessionAdapterValues, String> {
    let mut result = std::collections::BTreeMap::new();
    let mut identities = std::collections::BTreeSet::new();
    for row in session_rows {
        let (Some(at), Some(address), Some(role)) = (
            row.get("started_at").and_then(serde_json::Value::as_i64),
            row.get("session_addr").and_then(serde_json::Value::as_str),
            row.get("session_role").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if at >= window.start_ms
            && at < window.end_ms
            && address_belongs_to_team(address, team_workspaces)
        {
            identities.insert((role.to_owned(), address.to_owned()));
        }
    }
    if identities.len() > retro::eval::transcript::DEFAULT_MAX_TRANSCRIPT_SESSIONS {
        return Err("lane-order scope exceeds the bounded session limit".to_owned());
    }
    for (role, address) in identities {
        let Some(spec) = specs.iter().find(|spec| spec.role == role) else {
            continue;
        };
        let mut scoped_spec = spec.clone();
        scoped_spec.rules.retain(|rule| {
            rule.method == "deterministic" && is_lane_order_query(&rule.query_or_rubric)
        });
        if scoped_spec.rules.is_empty() {
            continue;
        }
        let scope = retro::eval::LaneOrderScope {
            team_id: Some(team.to_owned()),
            sessions_by_role: std::collections::BTreeMap::from([(
                role.clone(),
                std::collections::BTreeSet::from([address.clone()]),
            )]),
        };
        let adapters = retro::eval::ledger_adapters_scoped(
            &[scoped_spec],
            ledger_path,
            ready_state,
            projects,
            window,
            &scope,
        );
        result.insert((role, address), adapters);
    }
    Ok(result)
}

#[implements("FR-047-AC-5")]
fn transcript_adapter_sets(
    specs: &[retro::eval::RoleSpec],
    inventory: &Result<retro::eval::transcript::TranscriptInventory, String>,
    window: &retro::eval::EvalWindow,
    context: &retro::eval::transcript::AdapterContext,
    transcript_id_column_present: &Result<bool, String>,
) -> TranscriptAdapterValues {
    use retro::eval::QueryValue;
    let mut role_values = std::collections::BTreeMap::new();
    let mut session_values = std::collections::BTreeMap::new();
    for spec in specs {
        let queries = spec
            .rules
            .iter()
            .filter(|rule| {
                rule.method == "deterministic" && rule.query_or_rubric.starts_with("transcript.")
            })
            .map(|rule| rule.query_or_rubric.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if queries.is_empty() {
            continue;
        }
        let inventory = match inventory {
            Ok(inventory) => inventory,
            Err(reason) => {
                for query in &queries {
                    role_values.insert(
                        query.clone(),
                        QueryValue::Unavailable {
                            reason: format!("{query}: transcript collection unavailable: {reason}"),
                            evidence_refs: Vec::new(),
                        },
                    );
                }
                continue;
            }
        };
        let missing_id_reason = match transcript_id_column_present {
            Ok(true) => {
                "installed session_turn producer has not yet accumulated a transcript_id for this turn"
            }
            Ok(false) => "transcript_id writer is pending in unmerged SWM-261 PR #103",
            Err(reason) => reason.as_str(),
        };
        let mut values_by_session = std::collections::BTreeMap::<
            String,
            std::collections::BTreeMap<String, Vec<QueryValue>>,
        >::new();
        for turn in &inventory.turns {
            if turn.session_role == spec.role
                && turn.started_at_ms >= window.start_ms
                && turn.started_at_ms < window.end_ms
            {
                values_by_session
                    .entry(turn.session_addr.clone())
                    .or_default();
            }
        }
        for transcript in inventory.transcripts.values().filter(|transcript| {
            transcript.binding.session_role == spec.role
                && transcript.binding.started_at_ms >= window.start_ms
                && transcript.binding.started_at_ms < window.end_ms
        }) {
            let values = retro::eval::transcript::adapters(spec, transcript, window, context);
            let by_query = values_by_session
                .entry(transcript.binding.session_addr.clone())
                .or_default();
            for (query, value) in values {
                by_query.entry(query).or_default().push(value);
            }
        }
        let mut role_inputs = std::collections::BTreeMap::<String, Vec<QueryValue>>::new();
        for (address, by_query) in values_by_session {
            let mut adapters = std::collections::BTreeMap::new();
            for query in &queries {
                let value = combine_transcript_values(
                    query,
                    by_query.get(query).map(Vec::as_slice).unwrap_or_default(),
                    missing_id_reason,
                );
                role_inputs
                    .entry(query.clone())
                    .or_default()
                    .push(value.clone());
                adapters.insert(query.clone(), value);
            }
            session_values.insert((spec.role.clone(), address), adapters);
        }
        for query in &queries {
            let inputs = role_inputs
                .get(query)
                .map(Vec::as_slice)
                .unwrap_or_default();
            role_values.insert(
                query.clone(),
                combine_transcript_values(query, inputs, missing_id_reason),
            );
        }
    }
    (role_values, session_values)
}

#[implements("FR-047-AC-5")]
fn combine_transcript_values(
    query: &str,
    values: &[retro::eval::QueryValue],
    empty_reason: &str,
) -> retro::eval::QueryValue {
    use retro::eval::QueryValue;
    if values.is_empty() {
        return QueryValue::Unavailable {
            reason: format!("{query}: {empty_reason}"),
            evidence_refs: Vec::new(),
        };
    }
    if let Some(reason) = values.iter().find_map(|value| match value {
        QueryValue::Unavailable { reason, .. } => Some(reason.as_str()),
        _ => None,
    }) {
        return QueryValue::Unavailable {
            reason: format!("{query}: incomplete transcript population: {reason}"),
            evidence_refs: Vec::new(),
        };
    }
    if values
        .iter()
        .all(|value| matches!(value, QueryValue::NotApplicable { .. }))
    {
        let evidence_refs = values
            .iter()
            .flat_map(|value| match value {
                QueryValue::NotApplicable { evidence_refs, .. } => evidence_refs.clone(),
                _ => Vec::new(),
            })
            .collect();
        return QueryValue::NotApplicable {
            reason: format!("{query}: not applicable for every attributed transcript"),
            evidence_refs,
        };
    }
    let mut numbers = Vec::with_capacity(values.len());
    let mut evidence_refs = Vec::new();
    for value in values {
        let QueryValue::Value {
            value,
            evidence_refs: evidence,
        } = value
        else {
            return QueryValue::Unavailable {
                reason: format!("{query}: transcript population has mixed applicability"),
                evidence_refs: Vec::new(),
            };
        };
        numbers.push(*value);
        evidence_refs.extend(evidence.clone());
    }
    evidence_refs.sort();
    evidence_refs.dedup();
    let value = if query.ends_with("_max") {
        numbers.into_iter().reduce(f64::max).unwrap_or_default()
    } else {
        numbers.into_iter().sum()
    };
    QueryValue::Value {
        value,
        evidence_refs,
    }
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
fn collect_role_github_linear_adapters(
    specs: &[retro::eval::RoleSpec],
    repositories: &[String],
    decision_label: Option<&str>,
    window: &retro::eval::EvalWindow,
    settings: &retro::eval::EvalSettings,
    run_scope: &retro::eval::EvalRunScope,
    transcript: &TranscriptDispatchContext<'_>,
) -> std::collections::BTreeMap<String, retro::eval::QueryValue> {
    let has_role_github_queries = specs.iter().any(|spec| {
        spec.rules.iter().any(|rule| {
            rule.method == "deterministic"
                && rule.query_or_rubric.starts_with("github.")
                && matches!(
                    rule.query_or_rubric.as_str(),
                    "github.review_ready_prs_undispatched"
                        | "github.merges_beyond_reviewed_change"
                        | "github.merges_without_covering_gate"
                        | "github.ready_prs_unmerged_over_hour"
                )
        }) || spec
            .metrics
            .iter()
            .any(|metric| metric.query == "github.ticket_start_to_pr_open_median_hours")
    });
    let has_role_linear_queries = specs.iter().any(|spec| {
        spec.rules.iter().any(|rule| {
            rule.method == "deterministic"
                && matches!(
                    rule.query_or_rubric.as_str(),
                    "linear.redundant_gate_runs"
                        | "linear.findings_open_at_merge"
                        | "linear.gate_runs_before_clean_review"
                        | "linear.fix_round_findings_returned_still_open"
                        | "linear.open_decisions_without_owner"
                        | "linear.findings_without_outcome_after_disposition"
                        | "linear.mergeable_yes_with_open_finding"
                        | "linear.outside_diff_findings_without_failure"
                )
        }) || spec
            .metrics
            .iter()
            .any(|metric| metric.query == "linear.review_to_clean_median_hours")
    });
    if !has_role_github_queries && !has_role_linear_queries {
        return std::collections::BTreeMap::new();
    }

    let needs_pull_request_evidence = has_role_github_queries
        || specs.iter().any(|spec| {
            spec.rules.iter().any(|rule| {
                rule.method == "deterministic"
                    && matches!(
                        rule.query_or_rubric.as_str(),
                        "linear.redundant_gate_runs"
                            | "linear.findings_open_at_merge"
                            | "linear.fix_round_findings_returned_still_open"
                            | "linear.outside_diff_findings_without_failure"
                    )
            })
        });
    let evidence = if !needs_pull_request_evidence {
        Err("GitHub pull-request evidence is not required for selected role queries".to_owned())
    } else {
        match (
            settings.github_guard_state_path.as_deref(),
            repositories.is_empty(),
        ) {
            (Some(state_path), false) => retro::eval::github_rules::read_pull_request_evidence(
                state_path,
                settings.github_guard_max_bytes,
                repositories,
                &std::collections::BTreeMap::new(),
                run_scope.deadline(),
            ),
            (None, _) => Err("shared GitHub guard path is not configured".to_owned()),
            (_, true) => {
                Err("retro config has no repositories for role-specific GitHub evidence".to_owned())
            }
        }
    };
    match evidence {
        Ok(mut evidence) => {
            let mut join_error = None;
            if has_role_github_queries {
                let include_ticket_started_at = specs.iter().any(|spec| {
                    spec.metrics
                        .iter()
                        .any(|metric| metric.query == "github.ticket_start_to_pr_open_median_hours")
                });
                match retro::eval::linear_pull_request_joins_for_evidence(
                    repositories,
                    &evidence,
                    include_ticket_started_at,
                    settings.review_age_limit_hours,
                    window,
                ) {
                    Ok(joins) => {
                        for row in &mut evidence {
                            if let Some(join) = joins.get(&(row.repository.clone(), row.number)) {
                                row.reviewed_heads = join.reviewed_heads.clone();
                                row.gate_required = join.gate_required;
                                row.gate_covered_heads = join.gate_covered_heads.clone();
                                row.waived_for_gate = join.waived_for_gate;
                                row.waived_for_review = join.waived_for_review;
                                row.reviewer_or_dispatch_by_deadline =
                                    join.reviewer_or_dispatch_by_deadline;
                                row.reviewer_or_dispatch_by_window_end =
                                    join.reviewer_or_dispatch_by_window_end;
                                row.ready_merge_waived = join.ready_merge_waived;
                                row.ticket_started_at_ms = join.ticket_started_at_ms;
                            }
                        }
                        apply_transcript_dispatch_joins(
                            &mut evidence,
                            transcript.inventory,
                            transcript.transcript_id_column_present,
                            transcript.coverage_start_ms,
                            window,
                            settings.review_age_limit_hours,
                        );
                    }
                    Err(reason) => join_error = Some(reason),
                }
            }
            let mut values = retro::eval::github_rules::adapters(
                specs,
                join_error.as_deref().map_or(Ok(&evidence), Err),
                window,
                settings.review_age_limit_hours,
            );
            if let Some(retro::eval::QueryValue::Unavailable {
                reason,
                evidence_refs,
            }) = values.get_mut("github.review_ready_prs_undispatched")
                && reason.contains("reviewer and lead dispatch evidence is incomplete")
            {
                *reason = transcript_dispatch_unavailable_reason(
                    transcript.transcript_id_column_present,
                    transcript.inventory,
                    transcript.coverage_start_ms,
                );
                evidence_refs.clear();
            }
            values.extend(retro::eval::linear_class_a_adapters_with_github(
                specs,
                repositories,
                decision_label,
                window,
                Ok(&evidence),
            ));
            values
        }
        Err(reason) => {
            let mut values = retro::eval::github_rules::adapters(
                specs,
                Err(&reason),
                window,
                settings.review_age_limit_hours,
            );
            values.extend(retro::eval::linear_class_a_adapters_with_github(
                specs,
                repositories,
                decision_label,
                window,
                Err(&reason),
            ));
            values
        }
    }
}

#[implements("FR-047-AC-5")]
fn transcript_dispatch_unavailable_reason(
    transcript_id_column_present: &Result<bool, String>,
    inventory: &Result<retro::eval::transcript::TranscriptInventory, String>,
    coverage_start_ms: i64,
) -> String {
    match transcript_id_column_present {
        Ok(false) => {
            return "transcript_id writer is pending in unmerged SWM-261 PR #103".to_owned();
        }
        Err(reason) => return format!("session_turn.transcript_id schema unavailable: {reason}"),
        Ok(true) => {}
    }
    if let Ok(inventory) = inventory {
        let has_missing_code_lead_id = inventory.turns.iter().any(|turn| {
            turn.session_role == "code-lead"
                && turn.started_at_ms >= coverage_start_ms
                && turn.transcript_id.is_none()
        });
        if has_missing_code_lead_id || !inventory.complete {
            return "installed session_turn producer has not yet accumulated a transcript_id for this turn".to_owned();
        }
    }
    "installed session_turn producer must accumulate a full 48-hour transcript_id history before older ready PRs can be classified".to_owned()
}

#[implements("FR-047-AC-5")]
fn apply_transcript_dispatch_joins(
    evidence: &mut [retro::eval::github_rules::PullRequestEvidence],
    inventory: &Result<retro::eval::transcript::TranscriptInventory, String>,
    transcript_id_column_present: &Result<bool, String>,
    coverage_start_ms: i64,
    window: &retro::eval::EvalWindow,
    review_age_limit_hours: u64,
) {
    use retro::eval::transcript::TranscriptEventKind;

    let Ok(inventory) = inventory else {
        return;
    };
    let complete_code_lead_coverage = transcript_id_column_present == &Ok(true)
        && inventory.complete
        // `session_turn` has no separate coverage marker. Empty history therefore cannot
        // establish that missing dispatch evidence means no dispatch occurred.
        && inventory
            .turns
            .iter()
            .any(|turn| turn.session_role == "code-lead")
        && inventory.turns.iter().all(|turn| {
            turn.session_role != "code-lead"
                || turn.started_at_ms < window.start_ms
                || turn.started_at_ms >= window.end_ms
                || inventory
                    .transcripts
                    .get(turn.transcript_id.as_deref().unwrap_or_default())
                    .is_some_and(|transcript| transcript.complete)
        });
    let dispatches_for = |pr: &retro::eval::github_rules::PullRequestEvidence, through_ms: i64| {
        inventory.transcripts.values().any(|transcript| {
            transcript.binding.session_role == "code-lead"
                && transcript.complete
                && transcript.events.iter().any(|event| {
                    event.at_ms >= pr.ready_at_ms.unwrap_or(i64::MAX)
                        && event.at_ms <= through_ms
                        && transcript_event_names_pr_dispatch(
                            event,
                            &pr.repository,
                            pr.number,
                            pr.url.as_str(),
                        )
                })
        })
    };

    for pr in evidence {
        let Some(ready_at_ms) = pr.ready_at_ms else {
            continue;
        };
        let deadline_ms = ready_at_ms.saturating_add(
            review_age_limit_hours
                .saturating_mul(3_600_000)
                .min(i64::MAX as u64) as i64,
        );
        let deadline_complete = complete_code_lead_coverage
            && ready_at_ms >= coverage_start_ms
            && deadline_ms < window.end_ms;
        let window_end_complete = complete_code_lead_coverage && ready_at_ms >= coverage_start_ms;
        if deadline_complete {
            let transcript_dispatch = dispatches_for(pr, deadline_ms);
            pr.reviewer_or_dispatch_by_deadline =
                or_complete_evidence(pr.reviewer_or_dispatch_by_deadline, transcript_dispatch);
        } else if !has_positive(pr.reviewer_or_dispatch_by_deadline) {
            pr.reviewer_or_dispatch_by_deadline = None;
        }
        if window_end_complete {
            let transcript_dispatch = dispatches_for(pr, window.end_ms);
            pr.reviewer_or_dispatch_by_window_end =
                or_complete_evidence(pr.reviewer_or_dispatch_by_window_end, transcript_dispatch);
        } else if !has_positive(pr.reviewer_or_dispatch_by_window_end) {
            pr.reviewer_or_dispatch_by_window_end = None;
        }
    }

    fn has_positive(value: Option<bool>) -> bool {
        value == Some(true)
    }

    fn or_complete_evidence(existing: Option<bool>, transcript: bool) -> Option<bool> {
        Some(existing.unwrap_or(false) || transcript)
    }

    fn transcript_event_names_pr_dispatch(
        event: &retro::eval::transcript::TranscriptEvent,
        repository: &str,
        number: u64,
        url: &str,
    ) -> bool {
        if !matches!(
            event.kind,
            TranscriptEventKind::ToolCall | TranscriptEventKind::AssistantMessage
        ) {
            return false;
        }
        let Some(text) = event.command.as_deref() else {
            return false;
        };
        let lower = text.to_ascii_lowercase();
        let explicit_review = lower.contains("review")
            && (lower.contains("agent-msg send")
                || lower.contains("/reviewer")
                || lower.contains("please review")
                || lower.contains("review this"));
        explicit_review
            && (lower.contains(&url.to_ascii_lowercase())
                || lower.contains(&format!("{repository}#{}", number).to_ascii_lowercase())
                || lower.contains(&format!("{repository}/pull/{number}").to_ascii_lowercase()))
    }
}

#[implements("FR-047-AC-1")]
fn unavailable_adapters_for_prefixes(
    specs: &[retro::eval::RoleSpec],
    prefixes: &[&str],
    reason: &str,
) -> std::collections::BTreeMap<String, retro::eval::QueryValue> {
    let mut queries = std::collections::BTreeSet::new();
    for spec in specs {
        queries.extend(
            spec.rules
                .iter()
                .filter(|rule| rule.method == "deterministic")
                .map(|rule| rule.query_or_rubric.as_str())
                .filter(|query| prefixes.iter().any(|prefix| query.starts_with(prefix))),
        );
        queries.extend(
            spec.metrics
                .iter()
                .map(|metric| metric.query.as_str())
                .filter(|query| prefixes.iter().any(|prefix| query.starts_with(prefix))),
        );
    }
    queries
        .into_iter()
        .map(|query| {
            (
                query.to_owned(),
                retro::eval::QueryValue::Unavailable {
                    reason: format!("{query}: {reason}"),
                    evidence_refs: Vec::new(),
                },
            )
        })
        .collect()
}

#[implements("FR-047-AC-1")]
#[implements("FR-047-AC-9")]
fn collect_eval_retro_record(
    config: &retro::RetroConfig,
    window: &retro::eval::EvalWindow,
    specs: &[retro::eval::RoleSpec],
    settings: &retro::eval::EvalSettings,
) -> Result<retro::Record, String> {
    let requested = specs
        .iter()
        .flat_map(|spec| {
            spec.rules
                .iter()
                .filter(|rule| rule.method == "deterministic")
                .map(|rule| rule.query_or_rubric.as_str())
                .chain(spec.metrics.iter().map(|metric| metric.query.as_str()))
        })
        .filter_map(|query| query.strip_prefix("retro."))
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    if requested.is_empty() {
        return Err("no retro catalog query is selected for the current window".to_owned());
    }
    let window_secs = u64::try_from(window.end_ms.saturating_sub(window.start_ms) / 1000)
        .map_err(|_| "evaluation window has a negative duration".to_owned())?;
    let frame = retro::Frame {
        now: window.end_ms as f64 / 1000.0,
        window_secs,
        end_exclusive: true,
    };
    let linear = retro::live::linear_or_unreadable();
    let agents = retro::live::LiveAgents::start(config.herdr_teams.clone());
    let github = retro::eval::guarded_retro_github_source(
        settings.github_guard_state_path.clone(),
        settings.github_guard_max_bytes,
    );
    let sources = retro::Sources {
        linear: linear.as_ref(),
        github: &github,
        agents: &agents,
        gate_log: &retro::live::FileGateLog,
        relay: &retro::live::RedisCliRelay,
        local: &retro::live::SystemLocal,
    };
    let metrics = retro::metrics::collect_selected(config, frame, &sources, &requested);
    let needs_department = requested
        .iter()
        .any(|id| retro::department::definition(id).is_some() || id == "order_flip_loop");
    let department = needs_department
        .then(|| retro::collect_department_report(config, frame, &sources, &metrics, None, None))
        .flatten();
    Ok(retro::Record {
        schema_version: if department.is_some() {
            2
        } else {
            retro::SCHEMA_VERSION
        },
        kind: retro::RecordKind::Snapshot,
        taken_at: chrono::DateTime::<chrono::Utc>::from_timestamp(frame.now.floor() as i64, 0)
            .ok_or_else(|| "retro evaluation timestamp is out of range".to_owned())?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        team: config.team.clone(),
        window: retro::format_window(window_secs),
        metrics,
        department,
    })
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

#[implements("FR-049-AC-3")]
#[implements("FR-049-AC-5")]
fn run_cadence(command: CadenceCommand) -> Result<Output, Error> {
    let manifest_override = match &command {
        CadenceCommand::Due { manifest, .. } | CadenceCommand::Run { manifest, .. } => {
            manifest.as_deref()
        }
    };
    let path = ix_board::cadence::manifest_path(manifest_override)
        .map_err(|error| Error::Cadence(error.to_string()))?;
    let manifest = ix_board::cadence::load_manifest(&path)
        .map_err(|error| Error::Cadence(error.to_string()))?;
    let settings = ix_board::settings_value()?;
    match command {
        CadenceCommand::Due {
            program,
            team,
            alert,
            ..
        } => {
            let ledger = ix_board::cadence::open_ledger(settings.as_ref())
                .map_err(|error| Error::Cadence(error.to_string()))?;
            let report = ix_board::cadence::due(
                &manifest,
                &ledger,
                &program,
                &team,
                cadence_now_ms(),
                alert,
            )
            .map_err(|error| Error::Cadence(error.to_string()))?;
            ix_board::cadence::notify_if_alert(
                &report,
                manifest.alert_recipient.as_deref(),
                |recipient, message| {
                    let output = std::process::Command::new("agent-msg")
                        .args(["send", recipient, message])
                        .output()
                        .map_err(|error| format!("could not run agent-msg: {error}"))?;
                    if !output.status.success() {
                        return Err(format!(
                            "agent-msg send {recipient} exited {}: {}",
                            output.status,
                            String::from_utf8_lossy(&output.stderr).trim()
                        ));
                    }
                    Ok(())
                },
            )
            .map_err(|error| Error::Cadence(error.to_string()))?;
            Ok(Output::CadenceDue(report))
        }
        CadenceCommand::Run { job, .. } => {
            let report = ix_board::cadence::run_job(
                &manifest,
                || ix_board::cadence::open_ledger(settings.as_ref()),
                &job,
                cadence_now_ms(),
            )
            .map_err(|error| Error::Cadence(error.to_string()))?;
            Ok(Output::CadenceRun(report))
        }
    }
}

fn cadence_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn run_linear(command: Command) -> Result<Output, Error> {
    let source = LinearGraphQlSource::from_env()?;
    let mut service = ReadService::open_default(source)?;
    let result = match command {
        Command::Cache { .. } => unreachable!("cache commands are handled before Linear dispatch"),
        Command::Auth { .. } => unreachable!("auth commands are handled before Linear dispatch"),
        Command::AgentProbe => unreachable!("probe is handled before normal dispatch"),
        Command::Agents { .. } => unreachable!("agents is handled before Linear dispatch"),
        Command::Hygiene { .. } | Command::Build { .. } => {
            unreachable!("build-host commands are handled before Linear dispatch")
        }
        Command::Fleet { .. } => unreachable!("fleet is handled before Linear dispatch"),
        Command::Cadence { .. } => unreachable!("cadence is handled before Linear dispatch"),
        Command::Retro { .. } => unreachable!("retro is handled before Linear dispatch"),
        Command::Scope { .. }
        | Command::Roadmap { .. }
        | Command::Reconcile { .. }
        | Command::Chain { .. } => {
            unreachable!("program review commands are handled before Linear dispatch")
        }
        Command::Orphans { .. } => {
            unreachable!("program review commands are handled before Linear dispatch")
        }
        // The queue's JSON is a bare list, so its warnings are diagnostics (FR-046).
        Command::Next { team } => service
            .next_with_warnings(&team)
            .map(|(items, warnings)| Output::Queue(ReadyQueue { items, warnings })),
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
                    closed_since,
                },
        } => service
            .focus_record_windowed(
                &FocusSelection {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
                closed_since,
            )
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
                    closed_since,
                },
        } => service
            .focus_status_windowed(
                &FocusSelection {
                    initiatives,
                    projects,
                    milestones,
                    issues,
                },
                &work_milestones,
                closed_since,
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
    }?;
    match service
        .cache_freshness()
        .filter(|freshness| freshness.stale)
    {
        Some(freshness) => Ok(Output::Cached {
            result: Box::new(result),
            freshness: freshness.clone(),
        }),
        None => Ok(result),
    }
}

fn label_cached(result: Output, freshness: Option<&ix_board::CacheFreshness>) -> Output {
    match freshness.filter(|freshness| freshness.stale) {
        Some(freshness) => Output::Cached {
            result: Box::new(result),
            freshness: freshness.clone(),
        },
        None => result,
    }
}

fn run_drift(
    config_path: &PathBuf,
    evidence_path: Option<&PathBuf>,
    live: bool,
) -> Result<Output, Error> {
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
        let (evidence, freshness) = source.collect_drift_evidence_cached(&config)?;
        return Ok(label_cached(
            Output::Drift(evaluate_drift(&config, &evidence)?),
            freshness.as_ref(),
        ));
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
    Ok(Output::Drift(evaluate_drift_file(&config, &evidence)?))
}

#[implements("FR-048-AC-1", "FR-048-AC-4", "FR-048-AC-5")]
fn run_hygiene(
    args: &HygieneArgs,
    apply: bool,
) -> Result<ix_board::host_hygiene::HygieneReport, Error> {
    use ix_board::host_hygiene::{
        CleanDecision, HostPolicy, HygieneConfig, HygieneReport, apply_clean_decision,
        canonicalize_worktree_root, clean_decision, free_disk_bytes, newest_mtime_under,
        recheck_clean_decision, system_process_table,
    };
    use std::time::{Duration, SystemTime};

    if cfg!(target_os = "windows") {
        return Ok(HygieneReport {
            host: args.host.clone(),
            free_bytes: None,
            free_floor_bytes: 0,
            targets: Vec::new(),
            prunable_worktrees: Vec::new(),
            orphan_build_processes: Vec::new(),
            process_table_complete: false,
            unsupported: true,
        });
    }
    let settings = ix_board::settings_value()?;
    let config = HygieneConfig::from_settings(settings.as_ref())
        .map_err(|message| Error::ProgramReview { message })?;
    let configured_root = args
        .root
        .clone()
        .or(config.worktree_root)
        .unwrap_or_else(default_worktree_root);
    let root =
        canonicalize_worktree_root(&configured_root).map_err(|error| Error::ProgramReview {
            message: format!("cannot canonicalize worktree root: {error}"),
        })?;
    let host_policy = config.hosts.get(&args.host.to_lowercase());
    let floor_gb = args.free_floor_gb.or_else(|| host_policy.and_then(|policy| policy.free_floor_gb)).or(config.default_free_floor_gb)
        .ok_or_else(|| Error::ProgramReview {
            message: format!("no free-space floor configured for host {:?}; set hygiene.hosts.{}.free_floor_gb or pass --free-floor-gb", args.host, args.host),
        })?;
    let idle_minutes =
        args.idle_minutes
            .or(config.idle_minutes)
            .ok_or_else(|| Error::ProgramReview {
                message:
                    "no target idle age configured; set hygiene.idle_minutes or pass --idle-minutes"
                        .to_owned(),
            })?;
    if root.file_name().is_none_or(|name| name != "worktrees") {
        return Err(Error::ProgramReview {
            message: "hygiene root must be a worktrees directory".to_owned(),
        });
    }
    let floor = floor_gb
        .checked_mul(1024 * 1024 * 1024)
        .ok_or_else(|| Error::ProgramReview {
            message: "free-space floor overflows bytes".to_owned(),
        })?;
    let age = idle_minutes
        .checked_mul(60)
        .ok_or_else(|| Error::ProgramReview {
            message: "idle age overflows seconds".to_owned(),
        })?;
    let mtime_scan_entries = config.mtime_scan_entries.unwrap_or(100_000);
    let mtime_scan_duration = Duration::from_millis(config.mtime_scan_duration_ms.unwrap_or(2_000));
    let free_bytes = free_disk_bytes(&root).ok();
    let processes = system_process_table().unwrap_or_default();
    let prunable_worktrees = find_prunable_worktrees(&root);
    let orphan_build_processes = processes
        .processes
        .iter()
        .filter(|process| {
            process
                .cwd
                .as_deref()
                .is_some_and(|cwd| cwd.starts_with(&root))
                && process.cmdline.split(|byte| *byte == 0).any(|word| {
                    matches!(
                        PathBuf::from(String::from_utf8_lossy(word).as_ref())
                            .file_name()
                            .and_then(|name| name.to_str()),
                        Some("cargo" | "rustc")
                    )
                })
                && !process
                    .environ
                    .windows(b"HERDR_PANE_ID=".len())
                    .any(|window| window == b"HERDR_PANE_ID=")
        })
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    let now = SystemTime::now();
    let policy = HostPolicy {
        free_floor_bytes: floor,
        idle_age_secs: age,
    };
    let entries = std::fs::read_dir(&root).map_err(|error| Error::ProgramReview {
        message: format!("cannot read worktree root: {error}"),
    })?;
    let mut targets = Vec::new();
    for entry in entries.flatten() {
        let Ok(worktree) = std::fs::canonicalize(entry.path()) else {
            continue;
        };
        if !worktree.starts_with(&root) {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        if !worktree.join(".git").exists() {
            continue;
        }
        let target_path = worktree.join("target");
        let Ok(metadata) = std::fs::symlink_metadata(&target_path) else {
            continue;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(target) = std::fs::canonicalize(&target_path) else {
            continue;
        };
        if !target.starts_with(&worktree) {
            continue;
        }
        let modified = newest_mtime_under(&target, mtime_scan_entries, mtime_scan_duration)
            .unwrap_or_else(|_| SystemTime::now());
        let mut decision = clean_decision(&target, &worktree, now, modified, &policy, &processes);
        if apply && decision.eligible {
            let latest = system_process_table().unwrap_or_default();
            let newest_modified =
                newest_mtime_under(&target, mtime_scan_entries, mtime_scan_duration)
                    .unwrap_or_else(|_| SystemTime::now());
            decision = recheck_clean_decision(
                &decision,
                &worktree,
                SystemTime::now(),
                newest_modified,
                &policy,
                &latest,
            );
            if decision.eligible {
                decision = apply_clean_decision(&decision, true).unwrap_or_else(|error| {
                    let mut failed = decision.clone();
                    failed.reason = format!("remove_failed:{}", error.kind());
                    failed
                });
            }
        }
        targets.push(decision);
    }
    targets.sort_by(|left: &CleanDecision, right| left.target.cmp(&right.target));
    Ok(HygieneReport {
        host: args.host.clone(),
        free_bytes,
        free_floor_bytes: floor,
        targets,
        prunable_worktrees,
        orphan_build_processes,
        process_table_complete: processes.complete,
        unsupported: false,
    })
}

#[implements("FR-048-AC-4")]
fn find_prunable_worktrees(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = std::collections::BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for entry in entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
    {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(entry.path())
            .args(["worktree", "list", "--porcelain"])
            .output();
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let mut current = None;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                current = Some(PathBuf::from(path));
            } else if line.starts_with("prunable")
                && let Some(path) = current.as_ref().filter(|path| path.starts_with(root))
            {
                found.insert(path.clone());
            }
        }
    }
    found.into_iter().collect()
}

#[implements("FR-048-AC-7", "FR-048-AC-8")]
fn run_build_status(
    metrics_url: &str,
    policies: &[String],
) -> Result<
    Vec<(
        ix_board::host_hygiene::BuildHostMetrics,
        ix_board::host_hygiene::Eligibility,
    )>,
    Error,
> {
    use ix_board::host_hygiene::{
        Eligibility, HygieneConfig, parse_build_metrics, parse_build_policy,
    };
    let response = fetch_metrics_text(metrics_url)?;
    let configured = if policies.is_empty() {
        let settings = ix_board::settings_value()?;
        let config = HygieneConfig::from_settings(settings.as_ref())
            .map_err(|message| Error::ProgramReview { message })?;
        if config.hosts.is_empty() {
            return Err(Error::ProgramReview {
                message: "no hosts configured in [hygiene.hosts] and no --policy supplied"
                    .to_owned(),
            });
        }
        config
            .hosts
            .keys()
            .filter_map(|host| {
                config
                    .build_policy(host)
                    .map(|policy| (host.clone(), policy))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    } else {
        policies
            .iter()
            .map(|policy| {
                parse_build_policy(policy).map_err(|message| Error::ProgramReview { message })
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?
    };
    if configured.is_empty() {
        return Err(Error::ProgramReview {
            message: "no complete build policy configured; set max_load1 and heavy_gate_slots for each host".to_owned(),
        });
    }
    let metrics = parse_build_metrics(&response);
    Ok(configured
        .into_iter()
        .map(|(host, policy)| {
            let row = metrics.get(&host).cloned().unwrap_or_else(|| {
                ix_board::host_hygiene::BuildHostMetrics {
                    host,
                    ..Default::default()
                }
            });
            let verdict = if row.load1.is_none() || row.cores.is_none() {
                Eligibility::Unknown
            } else {
                ix_board::host_hygiene::eligibility(&row, policy)
            };
            (row, verdict)
        })
        .collect())
}

fn render_human(output: &Output) -> String {
    match output {
        Output::Cached { result, freshness } => format!(
            "{}\nstale: {}\nage_seconds: {}\nrate_limit_until: {}\nlast_successful_sync: {}\nlast_full_reconciliation: {}{}",
            render_human(result),
            freshness.stale,
            freshness
                .age_seconds
                .map_or_else(|| "unknown".to_owned(), |age| age.to_string()),
            freshness
                .rate_limit_until
                .map_or_else(|| "unknown".to_owned(), |until| until.to_string()),
            freshness
                .last_successful_sync
                .map_or_else(|| "unknown".to_owned(), |sync| sync.to_string()),
            freshness
                .last_full_reconciliation
                .map_or_else(|| "unknown".to_owned(), |full| full.to_string()),
            freshness
                .failure_reason
                .as_ref()
                .map_or_else(String::new, |reason| format!("\nfailure_reason: {reason}"))
        ),
        Output::CacheStatus(status) => ix_board::cache_status::render_evaluated_text(status),
        Output::SummaryRoadmap(report) => render_summary_roadmap(report),
        Output::Auth { status, source } => match source {
            Some(source) => format!("Linear credential {status} ({source})"),
            None => format!("Linear credential {status}"),
        },
        Output::Queue(queue) => render_queue(&queue.items),
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
        Output::Fleet(report) => ix_board::fleet::render_text(report),
        Output::Hygiene(report) => {
            format!(
                "{} hygiene: free={} floor={} targets={} process_evidence_complete={} unsupported={}",
                report.host,
                report
                    .free_bytes
                    .map_or_else(|| "unknown".to_owned(), |bytes| bytes.to_string()),
                report.free_floor_bytes,
                report.targets.len(),
                report.process_table_complete,
                report.unsupported,
            ) + &report
                .targets
                .iter()
                .map(|target| {
                    format!(
                        "\n{} age={}s {}",
                        target.reason,
                        target.age_secs,
                        target.target.display()
                    )
                })
                .collect::<Vec<_>>()
                .join("")
                + &report
                    .prunable_worktrees
                    .iter()
                    .map(|path| format!("\nprunable {}", path.display()))
                    .collect::<Vec<_>>()
                    .join("")
                + &report
                    .orphan_build_processes
                    .iter()
                    .map(|pid| format!("\norphan_build_process {pid}"))
                    .collect::<Vec<_>>()
                    .join("")
        }
        Output::BuildStatus(rows) => rows
            .iter()
            .map(|(row, verdict)| {
                format!(
                    "{} load/core={:.2} disk_free={:?} memory={:?}/{:?} swap={:?}/{:?} queue={} leases={} owners={:?} {:?}",
                    row.host,
                    row.load1
                        .zip(row.cores)
                        .filter(|(_, cores)| *cores > 0.0)
                        .map_or(f64::NAN, |(load, cores)| load / cores),
                    row.disk_available_bytes,
                    row.memory_available_bytes,
                    row.memory_total_bytes,
                    row.swap_free_bytes,
                    row.swap_total_bytes,
                    row.queue_depth,
                    row.running_leases,
                    row.leases_by_lane,
                    verdict,
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Output::FleetAssign(report) => ix_board::fleet::assign::render_text(report),
        Output::FleetLedger(rows) => rows.render_text(),
        Output::FleetDefectRoute {
            defect_id,
            ticket,
            routed_at_ms,
        } => {
            format!("routed defect {defect_id} to {ticket} at relay-ms {routed_at_ms}")
        }
        Output::FleetCycle(report) => format!(
            "{} cycle for {} at relay-ms {}",
            report.operation,
            report.session_addr,
            report
                .ended_at_ms
                .or(report.started_at_ms)
                .unwrap_or_default()
        ),
        Output::FleetTick(tick) => tick.render_text(),
        Output::CadenceDue(report) => report.render_text(),
        Output::CadenceRun(report) => format!(
            "{} {} at {} (exit status {})",
            report.id,
            if report.succeeded {
                "succeeded"
            } else {
                "failed"
            },
            report.finished_at_ms,
            report
                .exit_status
                .map_or_else(|| "signal".to_owned(), |status| status.to_string())
        ),
        Output::FleetSaturation(report) => serde_json::to_string_pretty(report)
            .unwrap_or_else(|error| format!("could not render fleet saturation JSON: {error}")),
        Output::Retro(output) => retro::render_text(output.record()),
        Output::FleetCapacity(status) => {
            let mut lines = Vec::new();
            for host in &status.hosts {
                let level = host
                    .recorded_level
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), |level| level.level.to_string());
                lines.push(format!(
                    "{} state={} recorded_level={} draining={}",
                    host.host,
                    host.discovery_state,
                    level,
                    host.draining_lanes.len()
                ));
            }
            for run in &status.runs {
                lines.push(format!(
                    "run {} host={} trigger={} started_at={} ended_at={:?} end_reason={:?} found_level={:?}",
                    run.run_id,
                    run.host,
                    run.trigger.as_deref().unwrap_or("unknown"),
                    run.started_at,
                    run.ended_at,
                    run.end_reason,
                    run.found_level
                ));
            }
            for step in &status.steps {
                lines.push(format!(
                    "step run={} no={} host={} lanes={:?} valid={:?} output_per_hour={:?} basis={:?} knee={:?} reason={:?}",
                    step.run_id,
                    step.step_no,
                    step.host,
                    step.lanes,
                    step.valid,
                    step.host_output_per_hour,
                    step.output_basis,
                    step.knee_signal,
                    step.knee_reason
                ));
            }
            for level in &status.levels {
                lines.push(format!(
                    "level host={} lanes={} recorded_at={} basis={:?} run={:?} reason={:?} superseded_at={:?}",
                    level.host,
                    level.level,
                    level.recorded_at,
                    level.output_basis,
                    level.run_id,
                    level.reason,
                    level.superseded_at
                ));
            }
            for event in &status.events {
                lines.push(format!(
                    "event {} host={} lane={:?} kind={} at={} reason={:?} evidence={:?}",
                    event.event_id,
                    event.host,
                    event.lane,
                    event.kind,
                    event.at,
                    event.reason,
                    event.evidence_kind
                ));
            }
            for window in &status.open_windows {
                lines.push(format!(
                    "open_window run={} step={} host={} lanes={} added_at={} rampup_end={}",
                    window.run_id,
                    window.step_no,
                    window.host,
                    window.lanes,
                    window.lane_added_at_ms,
                    window.rampup_end_ms
                ));
            }
            lines.join("\n")
        }
        Output::FleetCapacityRun(run_id) => format!("capacity run {run_id} started"),
        Output::FleetCapacityStep(window) => format!(
            "capacity run {} step {} holds {} lanes on {} after ramp-up at {}",
            window.run_id, window.step_no, window.lanes, window.host, window.rampup_end_ms
        ),
        Output::FleetCapacityMeasurement(measurement) => format!(
            "measured window {}..{} lanes={:?} output_per_hour={:?} basis={:?} throttle_basis=admission_only",
            measurement.window_start_ms,
            measurement.window_end_ms,
            measurement.lanes.value,
            measurement.host_output_per_hour.value,
            measurement.output_basis.value,
        ),
        Output::FleetCapacityLevel { recorded_level } => recorded_level.map_or_else(
            || "capacity run ended without a new level".to_owned(),
            |level| format!("recorded sustainable level: {level}"),
        ),
        Output::RetroDelta(report) => retro::render_delta_text(report),
        Output::RetroBackfill(report) => retro::render_backfill_text(report),
        Output::RetroEval(report) => retro::eval::render_text(report),
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
        #[cfg(test)]
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

#[cfg(test)]
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

fn render_summary_roadmap(report: &ix_board::program_review::SummaryRoadmap) -> String {
    let mut lines = Vec::new();
    for initiative in &report.initiatives {
        lines.push(format!(
            "{} | status {} | health {}",
            initiative.name,
            initiative.status.as_deref().unwrap_or("unknown"),
            initiative.health.as_deref().unwrap_or("unknown")
        ));
        for project in &initiative.projects {
            lines.push(format!("  {}", render_summary_project(project)));
        }
    }
    for project in &report.unplaced_projects {
        lines.push(format!(
            "unplaced project: {}",
            render_summary_project(project)
        ));
    }
    for name in &report.initiatives_without_projects {
        lines.push(format!("initiative without projects: {name}"));
    }
    for name in &report.missing_scope_initiatives {
        lines.push(format!(
            "initiative listed in scope file but missing from Linear: {name}"
        ));
    }
    for name in &report.missing_scope_projects {
        lines.push(format!(
            "project listed in scope file but missing from Linear: {name}"
        ));
    }
    for name in &report.projects_missing_from_scope {
        lines.push(format!("project missing from scope: {name}"));
    }
    for name in &report.projects_in_multiple_initiatives {
        lines.push(format!("project in multiple initiatives: {name}"));
    }
    for mismatch in &report.project_membership_mismatches {
        lines.push(format!("project membership mismatch: {mismatch:?}"));
    }
    lines.join("\n")
}

fn render_summary_project(project: &ix_board::program_review::SummaryRoadmapProject) -> String {
    let counts = match (
        project.done,
        project.total,
        project.open,
        project.canceled,
        &project.counts_observed_at,
    ) {
        (Some(done), Some(total), Some(open), Some(canceled), Some(observed)) => format!(
            "leaf tasks done {done}/{total}, open {open}, canceled {canceled} (issues observed {observed})"
        ),
        _ => format!("counts unavailable: {}", project.counts_unavailable_reason),
    };
    format!(
        "{} | {} | progress {} | source {} | observed {} | {counts} | issue last activity: {} ({}) | project updated {}",
        project.name,
        project.status,
        project.progress_percent.map_or_else(
            || "unknown".to_owned(),
            |progress| format!("{progress:.1}%")
        ),
        project.progress_source,
        project.observed_at,
        project.last_activity.as_deref().unwrap_or("unknown"),
        project.activity_unavailable_reason,
        project.project_updated_at.as_deref().unwrap_or("unknown")
    )
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
    join_with_warnings(lines, &expansion.warnings)
}

/// Joins rendered lines, ending with the unavailable references when there are any.
fn join_with_warnings(mut lines: Vec<String>, warnings: &[ReferenceWarning]) -> String {
    if !warnings.is_empty() {
        lines.push("Warnings:".to_owned());
        lines.extend(warnings.iter().map(|warning| format!("  {warning}")));
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
        render_closed(&mut lines, "  ", &project.recently_closed);
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
    if let Some(outside) = &status.outside_projects {
        lines.push("Outside any available project:".to_owned());
        render_status_group(
            &mut lines,
            "  ",
            &outside.open_by_state,
            &outside.in_progress,
            outside.progress,
        );
        render_closed(&mut lines, "  ", &outside.recently_closed);
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
    join_with_warnings(lines, &status.warnings)
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
    join_with_warnings(lines, &chunk.warnings)
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
    join_with_warnings(lines, &report.warnings)
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

fn render_closed(lines: &mut Vec<String>, indent: &str, closed: &[ix_board::FocusIssue]) {
    lines.push(format!("{indent}Closed in window:"));
    lines.extend(closed.iter().map(|issue| {
        let merged = issue
            .prs
            .iter()
            .find(|pr| pr.status == ix_board::PullRequestStatus::Merged)
            .map_or_else(String::new, |pr| format!(" ({}#{})", pr.repo, pr.number));
        format!("{indent}  {} {}{merged}", issue.identifier, issue.title)
    }));
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
    join_with_warnings(lines, &chunk.warnings)
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, FocusCommand, Output, ProgramRoadmap, ProgramScopeCommand,
        encode_output_json, format_issue_ref, format_snapshot_time_at, output_outcome,
        render_chain, render_closed, render_focus_status, render_orphans, render_reconcile,
        render_roadmap,
    };
    use clap::Parser;
    use ix_board::program_review::ScopeCheckReport;
    use ix_board::{FocusStatus, IssueBatch, IssueId, IssueReference};
    use ix_cli_kit::exit::Outcome;
    use ix_trace_rs::trace;

    #[trace("FR-047-AC-7")]
    #[trace("FR-047-AC-9")]
    #[test]
    fn retro_eval_json_is_the_report_object_not_an_enum_wrapper() {
        let report = ix_board::retro::eval::EvalReport {
            schema_version: 1,
            team: "TEST".into(),
            window: ix_board::retro::eval::EvalWindow {
                start_ms: 10,
                end_ms: 20,
            },
            previous_window: ix_board::retro::eval::EvalWindow {
                start_ms: 0,
                end_ms: 10,
            },
            roles: vec![ix_board::retro::eval::RoleResult {
                role: "coder".into(),
                objective_metrics: Vec::new(),
                score: ix_board::retro::eval::Score {
                    passed: 2,
                    evaluated: 3,
                    pass_rate: Some(2.0 / 3.0),
                },
                previous_score: ix_board::retro::eval::Score {
                    passed: 1,
                    evaluated: 3,
                    pass_rate: Some(1.0 / 3.0),
                },
                trend: Some(1.0 / 3.0),
                trend_matched_rules: 3,
                rules: Vec::new(),
                failing_rules: Vec::new(),
                session_ids: Vec::new(),
            }],
            sessions: Vec::new(),
            source_requests: ix_board::retro::eval::SourceRequestCounts {
                linear_http_requests: 4,
                github_cli_calls: 2,
                github_pages_observed: 3,
                github_guard_calls: 5,
                prometheus_http_requests: 1,
            },
        };
        let encoded = encode_output_json(&Output::RetroEval(report)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["team"], "TEST");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["roles"][0]["trend_matched_rules"], 3);
        assert_eq!(
            value["roles"][0]["objective_metrics"],
            serde_json::json!([])
        );
        assert_eq!(value["source_requests"]["linear_http_requests"], 4);
        assert_eq!(value["source_requests"]["github_cli_calls"], 2);
        assert_eq!(value["source_requests"]["github_pages_observed"], 3);
        assert_eq!(value["source_requests"]["github_guard_calls"], 5);
        assert_eq!(value["source_requests"]["prometheus_http_requests"], 1);
        assert!(value.get("RetroEval").is_none());
    }

    #[trace("FR-047-AC-8")]
    #[trace("FR-047-AC-5")]
    #[trace("FR-047-AC-6")]
    #[test]
    fn retro_eval_composes_scoped_ledger_relay_and_transcript_adapters_read_only() {
        use ix_board::retro::eval::{
            EvalWindow, EvaluationRule, ObjectiveMetricSpec, QueryValue, RoleSpec,
        };
        use std::collections::BTreeMap;
        use std::fs;

        let home = tempfile::tempdir().expect("temporary home");
        let ledger_path = home.path().join("ledger.sqlite3");
        let connection = rusqlite::Connection::open(&ledger_path).expect("create ledger fixture");
        connection
            .execute_batch(
                "PRAGMA user_version=9;
                 CREATE TABLE run(relay_ms INTEGER NOT NULL);
                 INSERT INTO run VALUES (1700000000000);
                 CREATE TABLE lane_orders_due(
                   order_id TEXT, lane TEXT, at_ms INTEGER, available INTEGER,
                   unavailable_reason TEXT);
                 INSERT INTO lane_orders_due VALUES
                   ('team-order','test:TEST:coder',1700000000100,1,NULL),
                   ('foreign-order','test:OTHER:coder',1700000000100,1,NULL);",
            )
            .expect("seed read-only ledger fixture");
        drop(connection);

        let transcript_path = home.path().join(".claude/projects/work/transcript-1.jsonl");
        fs::create_dir_all(transcript_path.parent().expect("transcript parent"))
            .expect("create transcript directory");
        fs::write(
            &transcript_path,
            r#"{"type":"assistant","timestamp":"2023-11-14T22:13:20Z","sessionId":"transcript-1","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"gh pr merge 42","description":"merge"}}]}}"#,
        )
        .expect("write transcript fixture");

        let rule = |id: &str, query: &str| EvaluationRule {
            id: id.to_owned(),
            rule: "fixture rule".to_owned(),
            evidence_source: query.to_owned(),
            trigger: "fixture".to_owned(),
            method: "deterministic".to_owned(),
            query_or_rubric: query.to_owned(),
            comparator: ">=".to_owned(),
            target: 1.0,
            unit: "count".to_owned(),
        };
        let specs = vec![RoleSpec {
            role: "coder".to_owned(),
            metrics: vec![ObjectiveMetricSpec {
                id: "CODE-M01".to_owned(),
                query: "transcript.coder_merge_commands".to_owned(),
                unit: "commands".to_owned(),
                target: 1.0,
                direction: "higher_is_better".to_owned(),
            }],
            rules: vec![
                rule("CODE-R11-E1", "transcript.coder_merge_commands"),
                rule("PLAN-R01-E1", "ledger.lane_orders_due"),
                rule("MIND-R01-E1", "relay.minder_orders_without_expect_ticket"),
                rule("LEAD-R02-E1", "github.review_ready_prs_undispatched"),
            ],
        }];
        let window = EvalWindow {
            start_ms: 1_699_999_000_000,
            end_ms: 1_700_001_000_000,
        };
        let session_row = serde_json::json!({
            "session_addr":"test:TEST:coder",
            "session_role":"coder",
            "started_at":1_700_000_000_000_i64,
            "transcript_id":"transcript-1"
        })
        .as_object()
        .expect("session row")
        .clone();
        let ledger_bytes_before = fs::read(&ledger_path).expect("read ledger before evaluation");
        let transcript_before =
            fs::read(&transcript_path).expect("read transcript before evaluation");

        let lane_adapters = super::collect_session_lane_order_adapters(
            &specs,
            Some(&ledger_path),
            None,
            &[],
            &window,
            "TEST",
            &["TEST".to_owned()],
            std::slice::from_ref(&session_row),
        )
        .expect("collect scoped lane-order adapter");
        let session_key = ("coder".to_owned(), "test:TEST:coder".to_owned());
        assert!(matches!(
            lane_adapters[&session_key]["ledger.lane_orders_due"],
            QueryValue::Value { value: 1.0, .. }
        ));

        let turns = ix_board::retro::eval::transcript::turn_rows_from_ledger(&[session_row]);
        let inventory = ix_board::retro::eval::transcript::discover_transcripts(
            home.path(),
            &turns,
            ix_board::retro::eval::transcript::TranscriptReadLimits::default(),
        );
        let (_, transcript_adapters) = super::transcript_adapter_sets(
            &specs,
            &Ok(inventory),
            &window,
            &Default::default(),
            &Ok(true),
        );
        assert!(matches!(
            transcript_adapters[&session_key]["transcript.coder_merge_commands"],
            QueryValue::Value { value: 1.0, .. }
        ));

        let relay_adapters = ix_board::retro::eval::relay::live_adapters(
            &specs,
            &window,
            Some(&ledger_path),
            None,
            None,
            None,
            None,
            std::time::Duration::from_millis(10),
        );
        assert!(matches!(
            relay_adapters["relay.minder_orders_without_expect_ticket"],
            QueryValue::Unavailable { .. }
        ));
        let github_adapters = ix_board::retro::eval::github_rules::adapters(
            &specs,
            Err("shared GitHub source is unavailable"),
            &window,
            1,
        );
        assert!(matches!(
            github_adapters["github.review_ready_prs_undispatched"],
            QueryValue::Unavailable { .. }
        ));

        let mut composed = BTreeMap::new();
        composed.extend(lane_adapters[&session_key].clone());
        composed.extend(transcript_adapters[&session_key].clone());
        composed.extend(relay_adapters);
        composed.extend(github_adapters);
        assert_eq!(composed.len(), 4);
        assert_eq!(
            fs::read(&ledger_path).expect("read ledger after evaluation"),
            ledger_bytes_before
        );
        assert_eq!(
            fs::read(&transcript_path).expect("read transcript after evaluation"),
            transcript_before
        );
    }

    #[trace("FR-047-AC-5")]
    #[test]
    fn retro_eval_joins_explicit_transcript_review_dispatch_to_exact_github_pr() {
        use super::apply_transcript_dispatch_joins;
        use ix_board::retro::eval::EvalWindow;
        use ix_board::retro::eval::github_rules::PullRequestEvidence;
        use ix_board::retro::eval::transcript::{
            HarnessFormat, SessionBinding, SessionTurnRow, TranscriptInventory, parse_lines,
        };
        use std::collections::BTreeMap;

        let at_ms = 1_700_000_000_000_i64;
        let json = br#"{"type":"assistant","timestamp":"2023-11-14T22:13:20Z","sessionId":"transcript-1","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"agent-msg send test:workspace:reviewer \"Please review https://github.com/agent-ix/ix-board/pull/42\"","description":"dispatch review"}}]}}"#;
        let transcript = parse_lines(
            HarnessFormat::Claude,
            SessionBinding {
                transcript_id: "transcript-1".to_owned(),
                session_addr: "test:workspace:code-lead".to_owned(),
                session_role: "code-lead".to_owned(),
                started_at_ms: at_ms,
            },
            &[json.as_slice()],
            4096,
            4096,
        );
        assert!(transcript.complete);
        let inventory = TranscriptInventory {
            turns: vec![SessionTurnRow {
                transcript_id: Some("transcript-1".to_owned()),
                session_addr: "test:workspace:code-lead".to_owned(),
                session_role: "code-lead".to_owned(),
                started_at_ms: at_ms,
            }],
            transcripts: BTreeMap::from([("transcript-1".to_owned(), transcript)]),
            complete: true,
            unavailable_reasons: Vec::new(),
        };
        let mut evidence = vec![PullRequestEvidence {
            repository: "agent-ix/ix-board".to_owned(),
            number: 42,
            url: "https://github.com/agent-ix/ix-board/pull/42".to_owned(),
            ready_at_ms: Some(at_ms),
            ..Default::default()
        }];
        let window = EvalWindow {
            start_ms: at_ms,
            end_ms: at_ms + 7_200_000,
        };

        apply_transcript_dispatch_joins(
            &mut evidence,
            &Ok(inventory),
            &Ok(true),
            at_ms - 1,
            &window,
            1,
        );

        assert_eq!(evidence[0].reviewer_or_dispatch_by_deadline, Some(true));
        assert_eq!(evidence[0].reviewer_or_dispatch_by_window_end, Some(true));
    }

    #[trace("FR-047-AC-5")]
    #[test]
    fn retro_eval_keeps_empty_transcript_history_unavailable_for_dispatch() {
        use super::apply_transcript_dispatch_joins;
        use ix_board::retro::eval::EvalWindow;
        use ix_board::retro::eval::github_rules::PullRequestEvidence;
        use ix_board::retro::eval::transcript::TranscriptInventory;
        use std::collections::BTreeMap;

        let at_ms = 1_700_000_000_000_i64;
        let mut evidence = vec![PullRequestEvidence {
            repository: "agent-ix/ix-board".to_owned(),
            number: 42,
            url: "https://github.com/agent-ix/ix-board/pull/42".to_owned(),
            ready_at_ms: Some(at_ms),
            ..Default::default()
        }];
        let window = EvalWindow {
            start_ms: at_ms,
            end_ms: at_ms + 7_200_000,
        };
        let empty_inventory = TranscriptInventory {
            turns: Vec::new(),
            transcripts: BTreeMap::new(),
            complete: true,
            unavailable_reasons: Vec::new(),
        };

        apply_transcript_dispatch_joins(
            &mut evidence,
            &Ok(empty_inventory),
            &Ok(true),
            at_ms - 1,
            &window,
            1,
        );

        assert_eq!(evidence[0].reviewer_or_dispatch_by_deadline, None);
        assert_eq!(evidence[0].reviewer_or_dispatch_by_window_end, None);
    }

    #[trace("FR-047-AC-9")]
    #[test]
    fn retro_eval_deadline_returns_partial_source_reason_without_starting_next_stage() {
        let scope = ix_board::retro::eval::EvalRunScope::start(
            std::time::Duration::from_millis(1),
            ix_board::retro::eval::DEFAULT_EVAL_LINEAR_REQUEST_LIMIT,
        )
        .expect("start bounded invocation");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let mut called = false;
        let result = super::retro_eval_stage_bounded("fixture", &scope, || {
            called = true;
            1
        });
        assert!(!called);
        assert!(
            result
                .unwrap_err()
                .contains("evaluation deadline elapsed before fixture")
        );
    }

    #[trace("FR-020-AC-2")]
    #[test]
    fn native_roadmap_text_reports_complete_provenance_for_placed_and_unplaced_projects() {
        use ix_board::program_review::{
            SummaryRoadmap, SummaryRoadmapInitiative, SummaryRoadmapProject,
        };
        let row = |name: &str, progress| SummaryRoadmapProject {
            id: name.to_owned(),
            identifier: None,
            total: None,
            counts_source: None,
            counts_observed_at: None,
            name: name.to_owned(),
            status: "Building".to_owned(),
            progress_percent: Some(progress),
            progress_source: "linear_native".to_owned(),
            observed_at: "2026-11-21T00:00:00Z".to_owned(),
            open: None,
            done: None,
            canceled: None,
            counts_unavailable_reason: "native current ticket count unavailable".to_owned(),
            last_activity: None,
            activity_unavailable_reason: "native summary does not expose issue activity".to_owned(),
            project_updated_at: Some("2026-11-19T00:00:00Z".to_owned()),
            also_in: Vec::new(),
        };
        let report = SummaryRoadmap {
            initiatives: vec![SummaryRoadmapInitiative {
                id: "program-id".to_owned(),
                identifier: None,
                member_project_ids: Vec::new(),
                name: "Program".to_owned(),
                status: Some("Active".to_owned()),
                health: None,
                projects: vec![row("Placed", 35.0)],
            }],
            unplaced_projects: vec![row("Unplaced", 75.0)],
            initiatives_without_projects: Vec::new(),
            projects_in_multiple_initiatives: Vec::new(),
            projects_missing_from_scope: Vec::new(),
            project_membership_mismatches: Vec::new(),
            missing_scope_initiatives: Vec::new(),
            missing_scope_projects: Vec::new(),
        };
        let text = super::render_summary_roadmap(&report);
        for (name, percentage) in [("Placed", "35.0%"), ("Unplaced", "75.0%")] {
            let line = text
                .lines()
                .find(|line| line.contains(name))
                .expect("project row");
            assert!(
                line.contains(percentage),
                "native progress is visible: {line}"
            );
            assert!(line.contains("source linear_native | observed 2026-11-21T00:00:00Z"));
            assert!(line.contains("counts unavailable: native current ticket count unavailable"));
            assert!(line.contains(
                "issue last activity: unknown (native summary does not expose issue activity)"
            ));
            assert!(line.contains("project updated 2026-11-19T00:00:00Z"));
        }
    }

    #[trace("FR-004-AC-3", "FR-004-AC-21")]
    #[test]
    fn fallback_json_and_text_preserve_unknown_age_cooldown_and_failure() {
        let output = Output::Cached {
            result: Box::new(Output::Queue(crate::ReadyQueue {
                items: Vec::new(),
                warnings: Vec::new(),
            })),
            freshness: ix_board::CacheFreshness {
                last_successful_sync: None,
                last_full_reconciliation: None,
                stale: true,
                age_seconds: None,
                rate_limit_until: Some(1_800_000_060),
                failure_reason: Some("Linear rate limited; retry in 3s".to_owned()),
            },
        };
        let json = serde_json::to_value(&output).expect("fallback JSON");
        assert_eq!(json["stale"], true);
        assert!(json["age_seconds"].is_null());
        assert_eq!(json["rate_limit_until"], 1_800_000_060);
        assert_eq!(json["failure_reason"], "Linear rate limited; retry in 3s");
        let text = super::render_human(&output);
        assert!(text.contains("stale: true\nage_seconds: unknown\nrate_limit_until: 1800000060"));
        assert!(text.contains("failure_reason: Linear rate limited; retry in 3s"));
    }

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
            outside_projects: None,
            warnings: Vec::new(),
            activity: Vec::new(),
        };
        assert!(
            render_focus_status(&status).contains(
                "(none in M1 Spec, M2 Core on Qwen; 15 ready in other milestones excluded)"
            )
        );
    }
    /// Trace: FR-046-AC-10
    #[test]
    fn the_agents_queue_lookup_keeps_each_warning_once_across_teams() {
        let warning = |issue: &str| ix_board::ReferenceWarning::UnavailableBlocker {
            issue: issue.to_owned(),
            blocker: "gone".to_owned(),
        };
        let mut collected = std::collections::BTreeSet::new();
        let empty = crate::first_ready((Vec::new(), vec![warning("ENG-2")]), &mut collected);
        assert!(empty.is_none());
        crate::first_ready(
            (Vec::new(), vec![warning("ENG-2"), warning("ENG-1")]),
            &mut collected,
        );
        assert_eq!(
            collected.into_iter().collect::<Vec<_>>(),
            [warning("ENG-1"), warning("ENG-2")]
        );
    }

    /// Trace: FR-046-AC-6
    #[test]
    fn focus_status_text_lists_warnings_and_the_outside_group() {
        let mut status = FocusStatus {
            fetched_at: 1_791_232_484,
            work_milestones: Vec::new(),
            excluded_ready_count: 0,
            projects: Vec::new(),
            blocker_layers: Vec::new(),
            ready_queue: Vec::new(),
            outside_projects: None,
            warnings: Vec::new(),
            activity: Vec::new(),
        };
        let clean = render_focus_status(&status);
        assert!(!clean.contains("Warnings:") && !clean.contains("Outside any available project"));
        status.outside_projects = Some(ix_board::FocusUnmilestonedStatus {
            open_by_state: Vec::new(),
            in_progress: Vec::new(),
            recently_closed: Vec::new(),
            progress: ix_board::FocusProgress {
                done: 1,
                total: 2,
                completed_percent: Some(50.0),
            },
        });
        status.warnings = vec![ix_board::ReferenceWarning::UnavailableMilestone {
            issue: "ENG-44".to_owned(),
            milestone: "m-gone".to_owned(),
            project: Some("p-1".to_owned()),
        }];
        let text = render_focus_status(&status);
        assert!(text.contains("Outside any available project:\n  Progress: 1/2 leaf tasks"));
        assert!(text.ends_with(
            "Warnings:\n  issue ENG-44 references unavailable milestone m-gone in project p-1"
        ));
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
        let scope =
            Cli::try_parse_from(["ix-board", "scope", "check", "--scope", "sample-team.toml"])
                .expect("parse scope check");
        assert!(
            matches!(scope.command, Command::Scope { command: ProgramScopeCommand::Check { scope } } if scope.to_str() == Some("sample-team.toml"))
        );

        let roadmap = Cli::try_parse_from(["ix-board", "roadmap", "--scope", "sample-team.toml"])
            .expect("parse scoped roadmap");
        assert!(
            matches!(roadmap.command, Command::Roadmap { scope: Some(scope) } if scope.to_str() == Some("sample-team.toml"))
        );
        assert!(Cli::try_parse_from(["ix-board", "roadmap"]).is_ok());

        let reconcile = Cli::try_parse_from([
            "ix-board",
            "reconcile",
            "SWM",
            "--scope",
            "sample-team.toml",
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
            } if project == "SWM" && scope.to_str() == Some("sample-team.toml")
        ));
        assert!(Cli::try_parse_from(["ix-board", "reconcile", "SWM"]).is_ok());
        assert!(
            Cli::try_parse_from(["ix-board", "reconcile", "SWM", "--stale-days", "0"]).is_err()
        );

        let chain = Cli::try_parse_from(["ix-board", "chain", "TEST-42"])
            .expect("parse single issue chain");
        assert!(
            matches!(chain.command, Command::Chain { issue: Some(issue), scope: None } if issue == "TEST-42")
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
            starts: vec!["TEST-1".to_owned()],
            nodes: vec![ix_board::chain::ChainNode {
                id: IssueId("issue-1".to_owned()),
                identifier: "TEST-1".to_owned(),
                title: "Review API".to_owned(),
                state: "In Progress".to_owned(),
                team: "TEST".to_owned(),
                owner_lane: Some("test:workspace:code".to_owned()),
                assignee_id: Some("user-1".to_owned()),
                assignee_name: Some("Quill Dev".to_owned()),
                age_days: Some(12),
                blocked_by: vec!["IR-2".to_owned()],
                root_blocker: false,
                unowned_root_blocker: false,
            }],
            paths: vec![ix_board::chain::ChainPath {
                start: "TEST-1".to_owned(),
                issues: vec!["TEST-1".to_owned(), "IR-2".to_owned()],
            }],
            root_blockers: vec!["IR-2".to_owned()],
            unowned_root_blockers: vec!["IR-2".to_owned()],
            cycles: vec![vec![
                "TEST-4".to_owned(),
                "TEST-5".to_owned(),
                "TEST-4".to_owned(),
            ]],
            warnings: Vec::new(),
        };
        let human = render_chain(&report);
        for detail in [
            "TEST-1",
            "In Progress",
            "team=TEST",
            "test:workspace:code",
            "Quill Dev",
            "12 days old",
            "TEST-4 -> TEST-5 -> TEST-4",
        ] {
            assert!(
                human.contains(detail),
                "human output should include {detail}"
            );
        }
        let json = serde_json::to_value(Output::Chain(report)).unwrap();
        assert!(json["nodes"].is_array());
        assert_eq!(json["nodes"][0]["assignee_name"], "Quill Dev");
        assert_eq!(json["cycles"][0][1], "TEST-5");
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
                    project: "TEST Language Spec".to_owned(),
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
            "TEST Language Spec",
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
    /// Trace: FR-020-AC-6
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
                    missing_from_linear: vec!["TEST Core".to_owned()],
                    missing_from_scope: vec!["TEST Ecosystem".to_owned()],
                },
            ],
            missing_scope_initiatives: vec!["Missing Initiative".to_owned()],
            missing_scope_projects: vec!["Missing Project".to_owned()],
        };
        let human = render_roadmap(&report);
        for message in [
            "missing from scope file: Runtime",
            "scope project missing Linear initiative membership: Compiler | TEST Core",
            "Linear initiative membership missing from scope file: Compiler | TEST Ecosystem",
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

    /// Trace: FR-045-AC-10
    #[test]
    fn closed_since_defaults_to_48h_and_closed_issues_render_with_merged_pr() {
        let parse = |extra: &[&str]| {
            let mut args = vec!["ix-board", "focus", "status", "--project", "SWM-P1"];
            args.extend_from_slice(extra);
            match Cli::try_parse_from(args)
                .expect("parse focus status")
                .command
            {
                Command::Focus {
                    command: FocusCommand::Status { closed_since, .. },
                } => closed_since,
                other => panic!("unexpected command {other:?}"),
            }
        };
        assert_eq!(parse(&[]), 48 * 3600);
        assert_eq!(parse(&["--closed-since", "90m"]), 90 * 60);

        let closed = ix_board::FocusIssue {
            id: IssueId("i1".to_owned()),
            identifier: "SWM-1".to_owned(),
            title: "Ship it".to_owned(),
            state: "Done".to_owned(),
            assignee: None,
            started_at: None,
            completed_at: Some("2026-11-24T10:05:00Z".to_owned()),
            prs: vec![ix_board::PullRequestFact {
                url: "https://github.com/agent-ix/ix-board/pull/7".to_owned(),
                repo: "ix-board".to_owned(),
                number: 7,
                status: ix_board::PullRequestStatus::Merged,
                draft: false,
                created_at: None,
                merged_at: Some("2026-11-24T10:00:00Z".to_owned()),
            }],
        };
        let mut lines = Vec::new();
        render_closed(&mut lines, "  ", &[closed]);
        assert_eq!(
            lines,
            ["  Closed in window:", "    SWM-1 Ship it (ix-board#7)"]
        );
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

#[cfg(test)]
mod fleet_assign_output {
    use super::*;
    use ix_board::fleet::assign::{AssignDefect, AssignReport, AssignResult};
    use ix_trace_rs::trace;

    fn report(result: AssignResult) -> AssignReport {
        AssignReport {
            sender_role: "orchestrator".into(),
            sender_addr: "test-server:test-workspace:test-lead".into(),
            lane: "test-server:test-workspace:test-lane".into(),
            tickets: vec!["SWM-211".into()],
            order_id: Some("order-1".into()),
            expect_ticket: "SWM-211".into(),
            started_at_ms: 1,
            sent_at_ms: Some(2),
            deadline_at_ms: Some(3),
            result,
            result_at_ms: Some(3),
            confirm_latency_s: None,
            focus_at_deadline: Some("SWM-999".into()),
            list_version: Some(1),
            previous_tickets: vec![],
            list_write_error: None,
            defects: vec![],
        }
    }

    #[trace("FR-035-AC-5")]
    #[test]
    fn only_confirmed_prints_confirmed_and_exits_zero() {
        let confirmed = report(AssignResult::Confirmed);
        assert!(render_human(&Output::FleetAssign(confirmed.clone())).starts_with("confirmed "));
        assert_eq!(
            output_outcome(&Output::FleetAssign(confirmed.clone())).code(),
            0
        );
        let acted = report(AssignResult::ActedNotFocused);
        assert!(
            render_human(&Output::FleetAssign(acted.clone())).starts_with("acted_not_focused ")
        );
        assert_eq!(
            output_outcome(&Output::FleetAssign(acted.clone())).code(),
            1
        );
        for result in [AssignResult::AckExpired, AssignResult::TimedOut] {
            let row = report(result);
            assert!(render_human(&Output::FleetAssign(row.clone())).starts_with(result.name()));
            assert_eq!(output_outcome(&Output::FleetAssign(row.clone())).code(), 1);
        }
    }

    #[trace("FR-035-AC-3")]
    #[trace("FR-035-AC-14")]
    #[test]
    fn a_list_or_ledger_failure_after_the_send_exits_with_the_internal_code() {
        let mut failed = report(AssignResult::Confirmed);
        failed.list_write_error = Some("order order-1 was accepted".into());
        assert_eq!(
            output_outcome(&Output::FleetAssign(failed.clone())).code(),
            4
        );
        assert!(render_human(&Output::FleetAssign(failed)).contains("error: order order-1"));
        let mut ledger = report(AssignResult::Confirmed);
        ledger.defects.push(AssignDefect {
            kind: "ledger_unwritable".into(),
            detail: "disk".into(),
        });
        assert_eq!(
            output_outcome(&Output::FleetAssign(ledger.clone())).code(),
            4
        );
        assert!(render_human(&Output::FleetAssign(ledger)).contains("defect: ledger_unwritable"));
    }
}
