// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only acceptance snapshot for SWM-261 / SWM-255 role metric views.
//!
//! The runner backs up the live SQLite ledger through SQLite's online backup API, migrates only
//! that copy, and reads relay and Prometheus sources without issuing writes to either service.

use ix_board::fleet::FleetConfig;
use ix_board::fleet::ledger::{self, Ledger};
use reqwest::blocking::Client;
use rusqlite::{Connection, OpenFlags, backup::Backup};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RETENTION_MS: i64 = 8 * 24 * 60 * 60 * 1_000;
const ORDER_VIEWS: &[&str] = &[
    "lane_orders_due",
    "lane_orders_not_taken_up",
    "minder_orders_unconfirmed_at_deadline",
    "unconfirmed_orders_not_resent_or_escalated",
];
const IDENTITY_COLUMNS: &[&str] = &[
    "action_id",
    "assignment_id",
    "defect_id",
    "finding_id",
    "gap_id",
    "host",
    "id",
    "lane",
    "lead_addr",
    "order_id",
    "project",
    "recipient",
    "sender",
    "sender_addr",
    "session_addr",
    "snapshot_id",
    "subject",
    "ticket",
    "tick_id",
];
const REASON_COLUMNS: &[&str] = &[
    "incomplete_reason",
    "unavailable_reason",
    "reason",
    "source_reason",
];
const SAFE_LABEL_COLUMNS: &[&str] = &[
    "class",
    "ended_reason",
    "kind",
    "outcome",
    "rule",
    "session_role",
    "state",
    "status",
];

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn create() -> Result<Self, Box<dyn Error>> {
        let root = env::temp_dir();
        for attempt in 0..10 {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = root.join(format!(
                "ix-board-swm261-acceptance-{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err("could not create a unique temporary acceptance directory".into())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Args {
    ledger: Option<PathBuf>,
    prometheus_url: Option<String>,
    output: Option<PathBuf>,
}

struct OrderSource {
    rows: usize,
    pages: usize,
    complete: bool,
    reason: Option<String>,
    skipped: usize,
    truncated: bool,
    oldest_available_at: Option<i64>,
    window_since: i64,
}

struct WorkSource {
    events: usize,
    truncated: bool,
    changes: BTreeMap<String, usize>,
}

struct ExpiredSource {
    rows: usize,
    indexed_since_ms: Option<i64>,
    skipped: usize,
    truncated: bool,
    error: Option<String>,
}

struct PrometheusSource {
    families: BTreeMap<String, usize>,
    error: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("SWM-261 acceptance runner failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    let live_ledger = args.ledger.unwrap_or_else(default_ledger_path);
    let output = args
        .output
        .unwrap_or_else(|| env::temp_dir().join("swm261-acceptance.md"));
    let prometheus_url = args
        .prometheus_url
        .or_else(read_config_prometheus_url)
        .unwrap_or_else(|| "http://localhost:9090".to_owned());
    let now_ms = unix_ms()?;
    let since_ms = now_ms.saturating_sub(RETENTION_MS);
    let temp = TempWorkspace::create()?;
    let copied_ledger = temp.path("fleet-ledger.sqlite3");
    let temp_config = temp.path("config.toml");

    backup_read_only(&live_ledger, &copied_ledger)?;
    let source_version = sqlite_version(&copied_ledger)?;
    let config = write_temp_config(&temp_config, &copied_ledger, &prometheus_url)?;
    let source_counts = table_counts(&copied_ledger)?;

    // Ledger::open applies migrations and writer setup only to the isolated copy.
    drop(Ledger::open(&copied_ledger, &config)?);
    let copied_version = sqlite_version(&copied_ledger)?;
    let order_source = read_orders(since_ms, now_ms)?;
    let expired_source = read_expired_orders()?;
    let work_source = read_work_events()?;
    let prometheus_source = read_prometheus(&prometheus_url);

    let report = render_report(
        &live_ledger,
        source_version,
        copied_version,
        now_ms,
        &source_counts,
        &order_source,
        &expired_source,
        &work_source,
        &prometheus_url,
        &prometheus_source,
        &copied_ledger,
    )?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, report)?;
    println!("wrote {}", output.display());
    Ok(())
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut args = Args::default();
    let mut iter = env::args().skip(1);
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--ledger" => args.ledger = Some(value.into()),
            "--prometheus-url" => args.prometheus_url = Some(value),
            "--output" => args.output = Some(value.into()),
            _ => return Err(format!("unknown argument {flag}").into()),
        }
    }
    Ok(args)
}

fn default_ledger_path() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/ix-board/fleet-ledger.sqlite3")
}

fn read_config_prometheus_url() -> Option<String> {
    let path = env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".config/ix-board/config.toml");
    let source = fs::read_to_string(path).ok()?;
    let value = toml::from_str::<toml::Value>(&source).ok()?;
    value
        .get("fleet")?
        .get("prometheus_url")?
        .as_str()
        .map(str::to_owned)
}

fn unix_ms() -> Result<i64, Box<dyn Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(i64::try_from(millis)?)
}

fn backup_read_only(source_path: &Path, destination_path: &Path) -> Result<(), Box<dyn Error>> {
    let source = Connection::open_with_flags(
        source_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut destination = Connection::open(destination_path)?;
    let backup = Backup::new(&source, &mut destination)?;
    backup.run_to_completion(128, Duration::from_millis(25), None)?;
    Ok(())
}

fn sqlite_version(path: &Path) -> Result<i64, Box<dyn Error>> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn table_counts(path: &Path) -> Result<BTreeMap<String, i64>, Box<dyn Error>> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let tables = [
        "run",
        "lane_tick",
        "tick",
        "action",
        "assignment_history",
        "session_turn",
        "cycle_event",
        "cycle_missed",
        "plan_coverage_tick",
        "plan_coverage_gap",
        "drift_finding",
        "defect",
    ];
    let mut counts = BTreeMap::new();
    for table in tables {
        let count = connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })?;
        counts.insert(table.to_owned(), count);
    }
    for (name, sql) in [
        (
            "lane_tick_ready_work_observed",
            "SELECT COUNT(*) FROM lane_tick WHERE ready_work_available IS NOT NULL",
        ),
        (
            "lane_tick_assignment_observed",
            "SELECT COUNT(*) FROM lane_tick WHERE remaining_assigned IS NOT NULL",
        ),
        (
            "lane_tick_blocked_rows",
            "SELECT COUNT(*) FROM lane_tick WHERE class='BLOCKED'",
        ),
        (
            "probe_defect_rows",
            "SELECT COUNT(*) FROM defect WHERE source LIKE 'probe:%'",
        ),
    ] {
        let count = connection.query_row(sql, [], |row| row.get::<_, i64>(0))?;
        counts.insert(name.to_owned(), count);
    }
    Ok(counts)
}

fn write_temp_config(
    path: &Path,
    copied_ledger: &Path,
    prometheus_url: &str,
) -> Result<FleetConfig, Box<dyn Error>> {
    let mut fleet = toml::Table::new();
    fleet.insert(
        "ledger_path".to_owned(),
        toml::Value::String(copied_ledger.to_string_lossy().into_owned()),
    );
    fleet.insert(
        "prometheus_url".to_owned(),
        toml::Value::String(prometheus_url.to_owned()),
    );
    let mut root = toml::Table::new();
    root.insert("fleet".to_owned(), toml::Value::Table(fleet));
    let value = toml::Value::Table(root);
    let encoded = toml::to_string(&value)?;
    fs::write(path, &encoded)?;
    let decoded = toml::from_str(&encoded)?;
    let mut config = FleetConfig::from_toml(Some(&decoded), None)?;
    // Never allow an inherited IX_BOARD_LEDGER_PATH to redirect the copy migration to live data.
    config.ledger_path = Some(copied_ledger.to_owned());
    Ok(config)
}

fn read_orders(since_ms: i64, until_ms: i64) -> Result<OrderSource, Box<dyn Error>> {
    let mut source = OrderSource {
        rows: 0,
        pages: 0,
        complete: true,
        reason: None,
        skipped: 0,
        truncated: false,
        oldest_available_at: None,
        window_since: since_ms,
    };
    let mut cursor: Option<Value> = None;
    let mut retried_boundary = false;
    for _ in 0..20 {
        let requested_since = source.window_since;
        let mut command = Command::new("agent-msg");
        command.args([
            "orders",
            "--since",
            &requested_since.to_string(),
            "--until",
            &until_ms.to_string(),
            "--limit",
            "500",
            "--json",
        ]);
        if let Some(value) = cursor.as_ref() {
            command.arg("--cursor").arg(value.to_string());
        }
        let output = command.output()?;
        if !output.status.success() {
            source.complete = false;
            source.reason = Some(format!(
                "agent-msg orders exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return Ok(source);
        }
        let page: Value = serde_json::from_slice(&output.stdout)?;
        source.pages += 1;
        source.rows = source.rows.saturating_add(
            page.get("orders")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        );
        source.skipped = source
            .skipped
            .saturating_add(json_count(page.get("skipped")));
        let page_truncated = page
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        source.oldest_available_at = page
            .get("oldest_available_at")
            .and_then(Value::as_i64)
            .or(source.oldest_available_at);
        if page.get("complete").and_then(Value::as_bool) != Some(true) {
            source.truncated = page_truncated;
            let reason = page
                .get("incomplete_reason")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| "orders page omitted complete=true".to_owned());
            let boundary = page.get("oldest_available_at").and_then(Value::as_i64);
            if !retried_boundary
                && source.rows == 0
                && source.skipped == 0
                && cursor.is_none()
                && [
                    "before_index_start",
                    "before_complete_coverage",
                    "outside_retention",
                ]
                .contains(&reason.as_str())
                && boundary.is_some_and(|value| value > requested_since)
            {
                source.window_since = boundary.unwrap_or(requested_since);
                source.pages = 0;
                source.oldest_available_at = None;
                retried_boundary = true;
                continue;
            }
            source.complete = false;
            source.reason = Some(reason);
            break;
        }
        let next = page
            .get("next_cursor")
            .filter(|value| !value.is_null())
            .cloned();
        if let Some(next) = next {
            if !page_truncated {
                source.complete = false;
                source.reason = Some("relay cursor appeared without truncated=true".to_owned());
                break;
            }
            cursor = Some(next);
            source.truncated = false;
            continue;
        }
        if page_truncated {
            source.complete = false;
            source.truncated = true;
            source.reason =
                Some("relay page was truncated without a continuation cursor".to_owned());
        }
        break;
    }
    if source.pages == 20 && cursor.is_some() {
        source.complete = false;
        source.truncated = true;
        source.reason = Some("acceptance reader reached its 20-page safety bound".to_owned());
    }
    if source.skipped > 0 {
        source.complete = false;
        source.reason = Some("unreadable_order_records".to_owned());
    }
    if source.truncated && source.reason.is_none() {
        source.complete = false;
        source.reason = Some("relay orders page was truncated".to_owned());
    }
    Ok(source)
}

fn read_work_events() -> Result<WorkSource, Box<dyn Error>> {
    let output = Command::new("agent-msg")
        .args(["work", "events", "--limit", "1000"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "agent-msg work events exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let page: Value = serde_json::from_slice(&output.stdout)?;
    let events = page
        .get("events")
        .and_then(Value::as_array)
        .ok_or("agent-msg work events response has no events array")?;
    let mut changes = BTreeMap::new();
    for event in events {
        let label = event
            .get("change")
            .and_then(|change| change.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let count: &mut usize = changes.entry(label).or_insert(0);
        *count = (*count).saturating_add(1);
    }
    Ok(WorkSource {
        events: events.len(),
        truncated: page
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || page
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        changes,
    })
}

fn read_expired_orders() -> Result<ExpiredSource, Box<dyn Error>> {
    let output = Command::new("agent-msg")
        .args(["orders", "expired", "--json"])
        .output()?;
    if !output.status.success() {
        return Ok(ExpiredSource {
            rows: 0,
            indexed_since_ms: None,
            skipped: 0,
            truncated: false,
            error: Some(format!(
                "agent-msg orders expired exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        });
    }
    let page: Value = serde_json::from_slice(&output.stdout)?;
    Ok(ExpiredSource {
        rows: page
            .get("orders")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
        indexed_since_ms: page.get("indexed_since_ms").and_then(Value::as_i64),
        skipped: json_count(page.get("skipped")),
        truncated: page
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        error: None,
    })
}

fn json_count(value: Option<&Value>) -> usize {
    match value {
        None => 0,
        Some(value) => value
            .as_u64()
            .and_then(|count| usize::try_from(count).ok())
            .unwrap_or(usize::MAX),
    }
}

fn read_prometheus(base_url: &str) -> PrometheusSource {
    let result = (|| -> Result<BTreeMap<String, usize>, Box<dyn Error>> {
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        let url = format!("{}/api/v1/query", base_url.trim_end_matches('/'));
        let response: Value = client
            .get(url)
            .query(&[("query", r#"{__name__=~"ix_team_.*"}"#)])
            .send()?
            .error_for_status()?
            .json()?;
        if response.get("status").and_then(Value::as_str) != Some("success") {
            return Err(format!("Prometheus query status was not success: {response}").into());
        }
        let series = response
            .get("data")
            .and_then(|data| data.get("result"))
            .and_then(Value::as_array)
            .ok_or("Prometheus query response omitted data.result")?;
        let mut families = BTreeMap::new();
        for sample in series {
            let Some(name) = sample
                .get("metric")
                .and_then(|metric| metric.get("__name__"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            *families.entry(name.to_owned()).or_insert(0) += 1;
        }
        Ok(families)
    })();
    match result {
        Ok(families) => PrometheusSource {
            families,
            error: None,
        },
        Err(error) => PrometheusSource {
            families: BTreeMap::new(),
            error: Some(error.to_string()),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn render_report(
    live_ledger: &Path,
    source_version: i64,
    copied_version: i64,
    observed_at_ms: i64,
    source_counts: &BTreeMap<String, i64>,
    orders: &OrderSource,
    expired: &ExpiredSource,
    work: &WorkSource,
    prometheus_url: &str,
    prometheus: &PrometheusSource,
    copied_ledger: &Path,
) -> Result<String, Box<dyn Error>> {
    let mut report = String::from(
        "# SWM-261 / SWM-255 live-source acceptance snapshot\n\n\
         This report was generated by `src/bin/swm261_acceptance.rs`. The live SQLite source was\n\
         opened read-only, copied with SQLite's online backup API, and only the temporary copy was\n\
         migrated to the branch schema. Relay and Prometheus access used read-only requests.\n\n",
    );
    report.push_str(&format!(
        "- Observed at: `{observed_at_ms}` UTC epoch milliseconds\n- Live ledger: `{}`\n- Live ledger schema: `v{source_version}`; temporary copy schema after migration: `v{copied_version}`\n- Temporary config: `[temporary, removed after run]` (contains only the copied ledger path and Prometheus URL)\n- Temporary copied ledger: `[temporary, removed after run]`\n- Relay order window: `{}..{}` (effective lower bound through observation time)\n\n",
        home_relative_path(live_ledger),
        orders.window_since,
        observed_at_ms
    ));

    report.push_str("## Live source shapes\n\n");
    report.push_str("### Ledger snapshot\n\n");
    report.push_str(&format!("- Schema: `v{source_version}`\n"));
    for (table, count) in source_counts {
        report.push_str(&format!("- `{table}`: `{count}` rows\n"));
    }
    report.push_str("\n### Relay orders\n\n");
    report.push_str(&format!(
        "- Pages read: `{}`; order rows: `{}`; complete: `{}`; skipped: `{}`; truncated: `{}`; oldest coverage boundary: `{}`\n",
        orders.pages,
        orders.rows,
        orders.complete,
        orders.skipped,
        orders.truncated,
        orders
            .oldest_available_at
            .map_or_else(|| "null".to_owned(), |value| value.to_string())
    ));
    if let Some(reason) = orders.reason.as_deref() {
        report.push_str(&format!("- Exact unavailable reason: `{reason}`\n"));
    }
    report.push_str("\n### Relay expired-order index\n\n");
    if let Some(error) = expired.error.as_deref() {
        report.push_str(&format!("- Exact unavailable reason: `{error}`\n"));
    } else {
        report.push_str(&format!(
            "- Expired rows: `{}`; indexed since: `{}`; skipped: `{}`; truncated: `{}`. This separate expiry index does not provide the all-orders population required by the four order metric views.\n",
            expired.rows,
            expired.indexed_since_ms.map_or_else(|| "null".to_owned(), |value| value.to_string()),
            expired.skipped,
            expired.truncated
        ));
    }
    report.push_str("\n### Relay work events\n\n");
    report.push_str(&format!(
        "- Events read: `{}`; truncated: `{}`; event `change.kind` counts: `{}`\n",
        work.events,
        work.truncated,
        serde_json::to_string(&work.changes)?
    ));
    report.push_str("\n### Prometheus\n\n");
    report.push_str(&format!("- Endpoint: `{}`\n", prometheus_url));
    if let Some(error) = prometheus.error.as_deref() {
        report.push_str(&format!("- Exact unavailable reason: `{error}`\n"));
    } else {
        report.push_str(&format!(
            "- `ix_team_*` metric families with current samples: `{}`\n",
            prometheus.families.len()
        ));
        for (name, count) in &prometheus.families {
            report.push_str(&format!("- `{name}`: `{count}` current series\n"));
        }
    }

    report.push_str("\n## SWM-261 / SWM-255 ledger metric views\n\n");
    report.push_str(
        "The runner queries every role view before the capacity-only `discovery_*` views.\n\n",
    );
    report.push_str("| View | Result from copied live ledger |\n|---|---|\n");
    let mut defects = Vec::new();
    for view in ledger::QUERYABLE
        .iter()
        .take_while(|view| !view.starts_with("discovery_"))
    {
        let result = ledger::query(copied_ledger, view, None);
        let ready_work_missing = source_counts
            .get("lane_tick_ready_work_observed")
            .copied()
            .unwrap_or(0)
            == 0
            && source_counts.get("lane_tick").copied().unwrap_or(0) > 0;
        let description = if *view == "drift_lane_minutes_share"
            && source_counts.get("tick").copied().unwrap_or(0) == 0
        {
            let rows = result?;
            let summary = summarize_rows(&rows.rows)?;
            let reason = format!(
                "DEFECT: drift share is `0.0` although the live ledger has no fleet tick evaluations or drift findings; zero is not evidence of complete drift coverage. `{}` rows; values: `{summary}`",
                rows.rows.len()
            );
            defects.push(format!("`{view}`: {reason}"));
            reason
        } else if *view == "idle_minutes" && ready_work_missing {
            let rows = result?;
            let summary = summarize_rows(&rows.rows)?;
            let reason = format!(
                "DEFECT: `minutes_with_ready_work` is reported as zero although all {} live lane_tick rows have `ready_work_available=NULL`; the ready-work metric needs an unavailable value and reason. `{}` rows; values: `{summary}`",
                source_counts.get("lane_tick").copied().unwrap_or(0),
                rows.rows.len()
            );
            defects.push(format!("`{view}`: {reason}"));
            reason
        } else if view.contains("ready_work") && ready_work_missing {
            format!(
                "unavailable: `ready_work_available` is NULL in all {} live lane_tick rows; no ready-work observations exist",
                source_counts.get("lane_tick").copied().unwrap_or(0)
            )
        } else if ORDER_VIEWS.contains(view) && !orders.complete {
            format!(
                "unavailable: live relay order page incomplete: {} (skipped={}, truncated={}); copied snapshot cannot stand in for current complete coverage",
                orders
                    .reason
                    .as_deref()
                    .unwrap_or("unknown_incomplete_page"),
                orders.skipped,
                orders.truncated
            )
        } else {
            match result {
                Ok(rows) if !rows.rows.is_empty() => {
                    let summary = summarize_rows(&rows.rows)?;
                    let reasons = unavailable_reasons(&rows.rows);
                    if reasons.is_empty() {
                        format!("`{}` rows; values: `{summary}`", rows.rows.len())
                    } else {
                        format!(
                            "unavailable: {}; `{}` rows; values: `{summary}`",
                            reasons.join("; "),
                            rows.rows.len()
                        )
                    }
                }
                Ok(_) => {
                    let reason = empty_view_reason(view, source_counts, orders, work);
                    if reason.starts_with("DEFECT:") {
                        defects.push(format!("`{view}`: {reason}"));
                    }
                    reason
                }
                Err(error) => {
                    let reason = format!("unavailable: {error}");
                    defects.push(format!("`{view}`: {reason}"));
                    reason
                }
            }
        };
        report.push_str(&format!("| `{view}` | {description} |\n"));
    }
    report.push_str("\n## Defects requiring follow-up\n\n");
    if defects.is_empty() {
        report.push_str("None observed by the query runner.\n");
    } else {
        for defect in defects {
            report.push_str(&format!("- {defect}\n"));
        }
    }
    Ok(report)
}

fn summarize_rows(rows: &[serde_json::Map<String, Value>]) -> Result<String, Box<dyn Error>> {
    let mut columns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        for (name, value) in row {
            if IDENTITY_COLUMNS.contains(&name.as_str()) {
                continue;
            }
            let safe = if REASON_COLUMNS.contains(&name.as_str())
                || SAFE_LABEL_COLUMNS.contains(&name.as_str())
                || value.is_number()
                || value.is_boolean()
                || value.is_null()
            {
                Some(value.to_string())
            } else {
                None
            };
            if let Some(value) = safe {
                columns.entry(name.clone()).or_default().insert(value);
            }
        }
    }
    let compact = columns
        .into_iter()
        .map(|(name, values)| {
            let all = values.into_iter().collect::<Vec<_>>();
            let distinct = all.len();
            let samples = all.into_iter().take(5).collect::<Vec<_>>();
            (
                name,
                serde_json::json!({"distinct": distinct, "samples": samples}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(serde_json::to_string(&compact)?)
}

fn unavailable_reasons(rows: &[serde_json::Map<String, Value>]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| {
            row.get("unavailable_reason")
                .or_else(|| row.get("incomplete_reason"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn home_relative_path(path: &Path) -> String {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return path.display().to_string();
    };
    path.strip_prefix(&home).map_or_else(
        |_| path.display().to_string(),
        |relative| format!("~/{}", relative.display()),
    )
}

fn empty_view_reason(
    view: &str,
    counts: &BTreeMap<String, i64>,
    orders: &OrderSource,
    work: &WorkSource,
) -> String {
    if view == "idle_lane_minutes_during_build_pause" {
        return "unavailable: no pause producer".to_owned();
    }
    if view.contains("ready_work")
        && counts
            .get("lane_tick_ready_work_observed")
            .copied()
            .unwrap_or(0)
            == 0
        && counts.get("lane_tick").copied().unwrap_or(0) > 0
    {
        return format!(
            "unavailable: `ready_work_available` is NULL in all {} live lane_tick rows; no ready-work observations exist",
            counts.get("lane_tick").copied().unwrap_or(0)
        );
    }
    if ORDER_VIEWS.contains(&view) && !orders.complete {
        return format!(
            "unavailable: relay order page incomplete: {} (skipped={}, truncated={})",
            orders
                .reason
                .as_deref()
                .unwrap_or("unknown_incomplete_page"),
            orders.skipped,
            orders.truncated
        );
    }
    if ["session_turn_rows", "cycle_events", "cycle_missed"].contains(&view)
        && counts.get("tick").copied().unwrap_or(0) == 0
    {
        return format!(
            "no fleet tick source rows; the live relay work-history stream ({} transitions, kinds {:?}) is not the prompt-activity source required by the session/cycle writer",
            work.events,
            work.changes.keys().collect::<Vec<_>>()
        );
    }
    if view == "blocked_lane_minutes"
        && counts.get("lane_tick_blocked_rows").copied().unwrap_or(0) == 0
    {
        return "no BLOCKED lane observations in the live lane_tick source; there is no blocked-minute population".to_owned();
    }
    if ["failed_probe_ticks", "probe_failure_without_routed_defect"].contains(&view)
        && counts.get("probe_defect_rows").copied().unwrap_or(0) == 0
    {
        return "no live probe defect rows; the probe failure population is empty".to_owned();
    }
    if view == "minder_turns_without_committed_tick"
        && counts.get("session_turn").copied().unwrap_or(0) == 0
    {
        return "no session_turn source rows in the live ledger; no minder prompt turns were recorded".to_owned();
    }
    if view.starts_with("need_ready_tickets_") && counts.get("action").copied().unwrap_or(0) == 0 {
        return "no live action rows; no need_ready_tickets action was emitted".to_owned();
    }
    if (view.contains("assignment")
        || view == "assignments_sent"
        || view == "unconfirmed_assign_results")
        && counts.get("assignment_history").copied().unwrap_or(0) == 0
    {
        return "no live assignment_history rows; assignment metrics have no observed population"
            .to_owned();
    }
    if (view.contains("action") || view.contains("rung"))
        && counts.get("action").copied().unwrap_or(0) == 0
    {
        return "no live action rows; action and rung metrics have no observed population"
            .to_owned();
    }
    if (view.contains("plan_coverage") || view.contains("short_queue") || view == "drift_findings")
        && counts.get("tick").copied().unwrap_or(0) == 0
    {
        return "no live fleet tick source rows; planner and drift evaluations have not run"
            .to_owned();
    }
    if view == "drift_finding_open_cycles_max"
        && counts.get("drift_finding").copied().unwrap_or(0) == 0
    {
        return "no live drift_finding rows; maximum open cycles is undefined".to_owned();
    }
    if view == "plan_coverage_gap_open_cycles_max"
        && counts.get("plan_coverage_gap").copied().unwrap_or(0) == 0
    {
        return "no live plan_coverage_gap rows; maximum open cycles is undefined".to_owned();
    }
    if view == "short_queue_open_cycles_max"
        && counts.get("plan_coverage_gap").copied().unwrap_or(0) == 0
    {
        return "no live short-queue plan_coverage_gap rows; maximum open cycles is undefined"
            .to_owned();
    }
    if (view.contains("cycle") || view.contains("session"))
        && counts.get("tick").copied().unwrap_or(0) == 0
    {
        return "no live fleet tick source rows; no cycle or session attribution was written"
            .to_owned();
    }
    "no rows in the copied live ledger; zero was not substituted for missing observations"
        .to_owned()
}
