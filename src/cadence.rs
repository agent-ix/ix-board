// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic recurring jobs backed by the fleet ledger (FR-049).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use ix_trace_rs::implements;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::fleet::FleetConfig;
use crate::fleet::ledger::{CadenceRun, Ledger, LedgerError};

const CHECKER_JOB_ID: &str = "cadence-checker";

/// Errors reading cadence configuration or recording/running a job.
#[derive(Debug, Error)]
pub enum CadenceError {
    /// Manifest I/O failed.
    #[error("cadence manifest {path}: {source}")]
    Read {
        /// Manifest path.
        path: PathBuf,
        /// I/O source.
        source: std::io::Error,
    },
    /// Manifest data failed validation.
    #[error("cadence manifest: {0}")]
    Manifest(String),
    /// A requested job is missing or ambiguous.
    #[error("cadence job {0:?} is missing or ambiguous")]
    Job(String),
    /// Ledger read/write failed.
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    /// The executable could not be started or its status could not be read.
    #[error("cadence job {job}: {message}")]
    Execute {
        /// Job identifier.
        job: String,
        /// Process error.
        message: String,
    },
    /// The alert notification could not be delivered.
    #[error("cadence alert notification: {0}")]
    Notification(String),
}

/// A configured cadence job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// Program identifier.
    pub program: String,
    /// Team identifier.
    pub team: String,
    /// Stable job identifier.
    pub id: String,
    /// Interval between completed runs, in seconds.
    pub interval_secs: u64,
    /// Deterministic job category.
    pub kind: JobKind,
    /// Arguments passed to this ix-board executable, without shell expansion.
    pub command: Vec<String>,
    /// Optional `retro delta --notify` command run only after this command succeeds.
    #[serde(default)]
    pub after_success: Option<Vec<String>>,
    /// Role responsible for this job.
    pub owner_role: String,
}

/// Supported cadence job categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Run a configured retro snapshot.
    RetroSnapshot,
    /// Run one fleet tick.
    FleetTick,
    /// Run the hygiene command.
    Hygiene,
    /// Run the local no-argument host check.
    HostCheck,
}

/// Cadence manifest containing program/team job entries.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Single relay address notified when `cadence due --alert` finds the checker needs attention.
    #[serde(default)]
    pub alert_recipient: Option<String>,
    /// Jobs in the manifest.
    pub jobs: Vec<Job>,
}

/// Due state of one job.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DueState {
    /// The interval has not elapsed.
    NotDue,
    /// The interval has elapsed, but less than two intervals have elapsed.
    Due,
    /// Two or more intervals have elapsed.
    Overdue,
}

/// One job's computed due state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DueJob {
    /// Job identifier.
    pub id: String,
    /// Job kind.
    pub kind: JobKind,
    /// Job owner role.
    pub owner_role: String,
    /// Due classification.
    pub state: DueState,
    /// Latest ledger run, if present.
    pub last_run: Option<CadenceRun>,
}

/// Due report for one program and team.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DueReport {
    /// Program identifier.
    pub program: String,
    /// Team identifier.
    pub team: String,
    /// Matching jobs in stable id order.
    pub jobs: Vec<DueJob>,
    /// Whether `--alert` found the checker missing or due/overdue.
    pub alert: bool,
}

impl DueReport {
    /// Required process exit code: 0 clear, 1 due, 2 overdue.
    #[implements("FR-049-AC-1")]
    pub fn exit_code(&self) -> u8 {
        if self.jobs.iter().any(|job| job.state == DueState::Overdue) {
            2
        } else if self.jobs.iter().any(|job| job.state == DueState::Due) {
            1
        } else {
            0
        }
    }

    /// Renders a stable human-readable report.
    #[implements("FR-049-AC-1")]
    pub fn render_text(&self) -> String {
        let mut lines = self
            .jobs
            .iter()
            .map(|job| {
                format!(
                    "{} {} owner={}",
                    job.id,
                    state_name(job.state),
                    job.owner_role
                )
            })
            .collect::<Vec<_>>();
        if self.alert {
            lines.push("alert cadence checker is missing or due".to_owned());
        }
        lines.join("\n")
    }
}

/// Sends the checker alert only when the due report requests one.
///
/// # Errors
/// Returns an error when the notification transport refuses the alert.
#[implements("FR-049-AC-5")]
pub fn notify_if_alert(
    report: &DueReport,
    recipient: Option<&str>,
    notify: impl FnOnce(&str, &str) -> Result<(), String>,
) -> Result<(), CadenceError> {
    if report.alert {
        let recipient = recipient
            .filter(|address| crate::relay_address::is_valid(address))
            .ok_or_else(|| {
                CadenceError::Notification(
                    "manifest must configure one alert_recipient as server:workspace:tab"
                        .to_owned(),
                )
            })?;
        notify(
            recipient,
            &format!(
                "ix-board cadence checker needs attention for {}/{}",
                report.program, report.team
            ),
        )
        .map_err(CadenceError::Notification)?;
    }
    Ok(())
}

/// Resolves the manifest from an explicit override, environment or user config directory.
pub fn manifest_path(override_path: Option<&Path>) -> Result<PathBuf, CadenceError> {
    if let Some(path) = override_path {
        return Ok(path.to_owned());
    }
    if let Some(path) = std::env::var_os("IX_BOARD_CADENCE_MANIFEST") {
        return Ok(PathBuf::from(path));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        })
        .ok_or_else(|| CadenceError::Manifest("cannot resolve config directory".to_owned()))?;
    Ok(base.join("ix-board").join("cadence.json"))
}

/// Loads and validates a cadence manifest.
#[implements("FR-049-AC-4")]
pub fn load_manifest(path: &Path) -> Result<Manifest, CadenceError> {
    let content = std::fs::read_to_string(path).map_err(|source| CadenceError::Read {
        path: path.to_owned(),
        source,
    })?;
    let manifest: Manifest = serde_json::from_str(&content)
        .map_err(|error| CadenceError::Manifest(error.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Validates cadence job identity, interval and supported argv shape.
#[implements("FR-049-AC-4")]
pub fn validate_manifest(manifest: &Manifest) -> Result<(), CadenceError> {
    if manifest
        .alert_recipient
        .as_deref()
        .is_some_and(|address| !crate::relay_address::is_valid(address))
    {
        return Err(CadenceError::Manifest(
            "alert_recipient must be one server:workspace:tab address".to_owned(),
        ));
    }
    let mut identities = BTreeSet::new();
    for job in &manifest.jobs {
        if job.program.trim().is_empty()
            || job.team.trim().is_empty()
            || job.id.trim().is_empty()
            || job.owner_role.trim().is_empty()
        {
            return Err(CadenceError::Manifest(
                "program, team, id and owner_role must be non-empty".to_owned(),
            ));
        }
        if job.interval_secs == 0 || i64::try_from(job.interval_secs).is_err() {
            return Err(CadenceError::Manifest(format!(
                "job {} interval_secs must fit a positive signed timestamp interval",
                job.id
            )));
        }
        let key = (job.program.as_str(), job.team.as_str(), job.id.as_str());
        if !identities.insert(key) {
            return Err(CadenceError::Manifest(format!(
                "duplicate job {} for {}/{}",
                job.id, job.program, job.team
            )));
        }
        if !command_matches_kind(job.kind, &job.command) {
            return Err(CadenceError::Manifest(format!(
                "job {} command does not match kind {:?}",
                job.id, job.kind
            )));
        }
        let valid_delta = job.after_success.as_deref().is_some_and(|command| {
            command.len() >= 2
                && command[0] == "retro"
                && command[1] == "delta"
                && command
                    .windows(2)
                    .any(|pair| pair[0] == "--config" && !pair[1].is_empty())
                && command.iter().any(|arg| arg == "--notify")
        });
        if job.after_success.is_some() && (job.kind != JobKind::RetroSnapshot || !valid_delta) {
            return Err(CadenceError::Manifest(format!(
                "job {} after_success must be a retro delta command",
                job.id
            )));
        }
    }
    Ok(())
}

fn command_matches_kind(kind: JobKind, command: &[String]) -> bool {
    match (kind, command) {
        (JobKind::RetroSnapshot, [root, sub, ..]) => root == "retro" && sub == "snapshot",
        (JobKind::FleetTick, [root, sub, ..]) => root == "fleet" && sub == "tick",
        (JobKind::Hygiene, [root, sub, ..]) => root == "focus" && sub == "hygiene",
        (JobKind::HostCheck, [root, sub]) => root == "host" && sub == "check",
        _ => false,
    }
}

/// Computes due state for the exact requested program and team.
#[implements("FR-049-AC-1")]
pub fn due(
    manifest: &Manifest,
    ledger: &Ledger,
    program: &str,
    team: &str,
    now_ms: i64,
    alert: bool,
) -> Result<DueReport, CadenceError> {
    let mut jobs = manifest
        .jobs
        .iter()
        .filter(|job| job.program == program && job.team == team)
        .map(|job| {
            let last_run = ledger.cadence_last_run(program, team, &job.id)?;
            let state = classify_due(job.interval_secs, last_run.as_ref(), now_ms);
            Ok(DueJob {
                id: job.id.clone(),
                kind: job.kind,
                owner_role: job.owner_role.clone(),
                state,
                last_run,
            })
        })
        .collect::<Result<Vec<_>, CadenceError>>()?;
    jobs.sort_by(|left, right| left.id.cmp(&right.id));
    let checker_missing_or_due = jobs
        .iter()
        .find(|job| job.id == CHECKER_JOB_ID)
        .is_none_or(|job| job.state != DueState::NotDue);
    Ok(DueReport {
        program: program.to_owned(),
        team: team.to_owned(),
        jobs,
        alert: alert && checker_missing_or_due,
    })
}

fn classify_due(interval_secs: u64, last_run: Option<&CadenceRun>, now_ms: i64) -> DueState {
    let Some(reference_at_ms) = last_run.map(|run| run.finished_at_ms.unwrap_or(run.started_at_ms))
    else {
        return DueState::Due;
    };
    let interval_ms = i64::try_from(interval_secs)
        .unwrap_or(i64::MAX)
        .saturating_mul(1000);
    let elapsed_ms = now_ms.saturating_sub(reference_at_ms);
    if elapsed_ms >= interval_ms.saturating_mul(2) {
        DueState::Overdue
    } else if elapsed_ms >= interval_ms {
        DueState::Due
    } else {
        DueState::NotDue
    }
}

/// Runs one uniquely identified job and records its process outcome.
#[implements("FR-049-AC-3")]
#[implements("FR-049-AC-7")]
pub fn run_job(
    manifest: &Manifest,
    mut open_ledger: impl FnMut() -> Result<Ledger, CadenceError>,
    job_id: &str,
    now_ms: i64,
) -> Result<JobRunReport, CadenceError> {
    run_job_with_executor(manifest, open_ledger, job_id, now_ms, |job| {
        execute(job).map(|status| RunOutcome {
            succeeded: status.success(),
            exit_status: status.code(),
        })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunOutcome {
    succeeded: bool,
    exit_status: Option<i32>,
}

fn run_job_with_executor(
    manifest: &Manifest,
    mut open_ledger: impl FnMut() -> Result<Ledger, CadenceError>,
    job_id: &str,
    now_ms: i64,
    execute_job: impl FnOnce(&Job) -> Result<RunOutcome, std::io::Error>,
) -> Result<JobRunReport, CadenceError> {
    validate_manifest(manifest)?;
    let mut matching = manifest.jobs.iter().filter(|job| job.id == job_id);
    let Some(job) = matching.next() else {
        return Err(CadenceError::Job(job_id.to_owned()));
    };
    if matching.next().is_some() {
        return Err(CadenceError::Job(job_id.to_owned()));
    }
    let mut ledger = open_ledger()?;
    ledger.cadence_start(&job.program, &job.team, &job.id, now_ms)?;
    drop(ledger);
    let result = execute_job(job);
    let finished_at_ms = unix_time_ms();
    match result {
        Ok(outcome) => {
            let code = outcome.exit_status.map(|code| code.to_string());
            open_ledger()?.cadence_finish(
                &job.program,
                &job.team,
                &job.id,
                finished_at_ms,
                outcome.succeeded,
                code.as_deref(),
            )?;
            Ok(JobRunReport {
                id: job.id.clone(),
                succeeded: outcome.succeeded,
                exit_status: outcome.exit_status,
                finished_at_ms,
            })
        }
        Err(error) => {
            let detail = error.to_string();
            open_ledger()?.cadence_finish(
                &job.program,
                &job.team,
                &job.id,
                finished_at_ms,
                false,
                Some(&detail),
            )?;
            Err(CadenceError::Execute {
                job: job.id.clone(),
                message: detail,
            })
        }
    }
}

fn execute(job: &Job) -> Result<std::process::ExitStatus, std::io::Error> {
    let executable = std::env::current_exe()?;
    let status = Command::new(&executable).args(&job.command).status()?;
    if status.success()
        && let Some(after_success) = &job.after_success
    {
        return Command::new(executable).args(after_success).status();
    }
    Ok(status)
}

fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

/// Result of running a cadence job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobRunReport {
    /// Job identifier.
    pub id: String,
    /// Whether the process exited successfully.
    pub succeeded: bool,
    /// Process exit status, absent if terminated by signal.
    pub exit_status: Option<i32>,
    /// Completion time in Unix milliseconds.
    pub finished_at_ms: i64,
}

fn state_name(state: DueState) -> &'static str {
    match state {
        DueState::NotDue => "not_due",
        DueState::Due => "due",
        DueState::Overdue => "overdue",
    }
}

/// Opens the fleet ledger at its configured path or the default state path.
pub fn open_ledger(settings: Option<&toml::Value>) -> Result<Ledger, CadenceError> {
    let config = FleetConfig::from_toml(settings, std::env::var("IX_PROMETHEUS_URL").ok())
        .map_err(|error| CadenceError::Manifest(error.to_string()))?;
    let path = config
        .ledger_path
        .clone()
        .unwrap_or(crate::fleet::ledger::default_path()?);
    Ok(Ledger::open(&path, &config)?)
}

#[cfg(test)]
mod tests {
    use super::{
        DueState, Job, JobKind, Manifest, RunOutcome, classify_due, due, notify_if_alert,
        run_job_with_executor, validate_manifest,
    };
    use crate::fleet::FleetConfig;
    use crate::fleet::ledger::Ledger;
    use ix_trace_rs::trace;
    use serde_json::json;

    fn job(id: &str) -> Job {
        Job {
            program: "QSL".to_owned(),
            team: "qsl".to_owned(),
            id: id.to_owned(),
            interval_secs: 1800,
            kind: JobKind::RetroSnapshot,
            command: vec![
                "retro".to_owned(),
                "snapshot".to_owned(),
                "--config".to_owned(),
                "/tmp/retro.json".to_owned(),
            ],
            owner_role: "minder".to_owned(),
            after_success: None,
        }
    }

    #[test]
    #[trace("FR-049-AC-1")]
    fn due_classification_distinguishes_missing_due_and_overdue() {
        assert_eq!(classify_due(1800, None, 0), DueState::Due);
        let recent = crate::fleet::ledger::CadenceRun {
            started_at_ms: 0,
            finished_at_ms: Some(1000),
            outcome: "succeeded".to_owned(),
            detail: None,
        };
        assert_eq!(classify_due(1800, Some(&recent), 1_801_000), DueState::Due);
        assert_eq!(
            classify_due(1800, Some(&recent), 3_601_000),
            DueState::Overdue
        );
        assert_eq!(
            classify_due(1800, Some(&recent), 1_800_999),
            DueState::NotDue
        );
    }

    #[test]
    #[trace("FR-049-AC-1", "FR-049-AC-5")]
    fn due_report_exit_codes_and_checker_alert_are_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let checker = job("cadence-checker");
        let manifest = Manifest {
            alert_recipient: None,
            jobs: vec![checker.clone()],
        };
        let absent = due(&manifest, &ledger, "elsewhere", "qsl", 0, true).unwrap();
        assert_eq!(absent.exit_code(), 0);
        let first = due(&manifest, &ledger, "QSL", "qsl", 0, true).unwrap();
        assert_eq!(first.exit_code(), 1);
        assert!(first.alert);

        ledger
            .cadence_start(&checker.program, &checker.team, &checker.id, 0)
            .unwrap();
        ledger
            .cadence_finish(
                &checker.program,
                &checker.team,
                &checker.id,
                1000,
                true,
                None,
            )
            .unwrap();
        let overdue = due(&manifest, &ledger, "QSL", "qsl", 3_601_000, true).unwrap();
        assert_eq!(overdue.exit_code(), 2);
        assert!(overdue.alert);
    }

    #[test]
    #[trace("FR-049-AC-5")]
    fn checker_notification_is_sent_only_when_report_alerts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let manifest_path = directory.path().join("cadence.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&json!({
                "alert_recipient": "draco:SWM:minder",
                "jobs": [{
                    "program": "QSL",
                    "team": "qsl",
                    "id": "cadence-checker",
                    "interval_secs": 1800,
                    "kind": "retro_snapshot",
                    "command": ["retro", "snapshot", "--config", "/tmp/retro.json"],
                    "owner_role": "minder"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let manifest = super::load_manifest(&manifest_path).unwrap();
        let checker = &manifest.jobs[0];
        ledger
            .cadence_start(&checker.program, &checker.team, &checker.id, 0)
            .unwrap();
        ledger
            .cadence_finish(
                &checker.program,
                &checker.team,
                &checker.id,
                1000,
                true,
                None,
            )
            .unwrap();

        let on_time = due(&manifest, &ledger, "QSL", "qsl", 1001, true).unwrap();
        let mut on_time_notified = false;
        notify_if_alert(&on_time, manifest.alert_recipient.as_deref(), |_, _| {
            on_time_notified = true;
            Ok(())
        })
        .unwrap();
        assert!(!on_time_notified);

        let overdue = due(&manifest, &ledger, "QSL", "qsl", 3_601_000, true).unwrap();
        let mut sent = None;
        notify_if_alert(
            &overdue,
            manifest.alert_recipient.as_deref(),
            |recipient, message| {
                sent = Some((recipient.to_owned(), message.to_owned()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            sent,
            Some((
                "draco:SWM:minder".to_owned(),
                "ix-board cadence checker needs attention for QSL/qsl".to_owned()
            ))
        );
    }

    #[test]
    #[trace("FR-049-AC-2")]
    fn ledger_reopen_preserves_outcome_and_running_job_ages_to_overdue() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.sqlite3");
        {
            let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
            ledger.cadence_start("QSL", "qsl", "retro", 0).unwrap();
            ledger
                .cadence_finish("QSL", "qsl", "retro", 1000, false, Some("exit 4"))
                .unwrap();
        }
        let mut ledger = Ledger::open(&path, &FleetConfig::default()).unwrap();
        let last = ledger
            .cadence_last_run("QSL", "qsl", "retro")
            .unwrap()
            .unwrap();
        assert_eq!(last.finished_at_ms, Some(1000));
        assert_eq!(last.outcome, "failed");
        assert_eq!(last.detail.as_deref(), Some("exit 4"));

        ledger.cadence_start("QSL", "qsl", "stopped", 0).unwrap();
        let manifest = Manifest {
            alert_recipient: None,
            jobs: vec![job("stopped")],
        };
        let report = due(&manifest, &ledger, "QSL", "qsl", 3_600_000, false).unwrap();
        assert_eq!(report.jobs[0].state, DueState::Overdue);
    }

    #[test]
    #[trace("FR-049-AC-4")]
    fn manifest_rejects_duplicate_and_invalid_job_entries() {
        let mut manifest = Manifest {
            alert_recipient: None,
            jobs: vec![job("retro")],
        };
        manifest.jobs.push(job("retro"));
        assert!(validate_manifest(&manifest).is_err());
        manifest.jobs.pop();
        manifest.jobs[0].interval_secs = 0;
        assert!(validate_manifest(&manifest).is_err());
        manifest.jobs[0].interval_secs = 1800;
        manifest.alert_recipient = Some("all".to_owned());
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    #[trace("FR-049-AC-8", "FR-050-AC-5")]
    fn host_check_cadence_job_records_its_injected_child_outcome() {
        let directory = tempfile::tempdir().unwrap();
        let ledger_path = directory.path().join("ledger.sqlite3");
        let manifest = Manifest {
            alert_recipient: None,
            jobs: vec![Job {
                program: "QSL".to_owned(),
                team: "qsl".to_owned(),
                id: "host-check".to_owned(),
                interval_secs: 3600,
                kind: JobKind::HostCheck,
                command: vec!["host".to_owned(), "check".to_owned()],
                after_success: None,
                owner_role: "minder".to_owned(),
            }],
        };
        validate_manifest(&manifest).expect("host check command accepted");
        let report = run_job_with_executor(
            &manifest,
            || Ok(Ledger::open(&ledger_path, &FleetConfig::default())?),
            "host-check",
            1000,
            |job| {
                assert_eq!(job.command, ["host", "check"]);
                Ok(RunOutcome {
                    succeeded: false,
                    exit_status: Some(1),
                })
            },
        )
        .expect("record host-check result");
        assert!(!report.succeeded);
        assert_eq!(report.exit_status, Some(1));
        let ledger = Ledger::open(&ledger_path, &FleetConfig::default()).unwrap();
        let last = ledger
            .cadence_last_run("QSL", "qsl", "host-check")
            .unwrap()
            .unwrap();
        assert_eq!(last.outcome, "failed");
        assert_eq!(last.detail.as_deref(), Some("1"));
    }
}
