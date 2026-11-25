// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Source traits, the report model and the builder that joins the sources on the lane address.

use super::classify::{
    ClassKind, Evidence, HookEvent, HookKind, LaneInput, Thresholds, WeakSource, age, classify,
};
use super::config::{
    RELAY_DEFAULT_MAX_TOOL_RUNTIME, RELAY_DEFAULT_NO_EVENTS_AFTER, RELAY_DEFAULT_WORKING_WINDOW,
};
use super::live::series;
use super::{FleetConfig, SourceError};
use relay_core::fleet::{HostLaneList, fleet_registry};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Version of the JSON evidence schema.
pub const SCHEMA_VERSION: u32 = 1;

/// One relay registration, reduced to what classification reads.
#[derive(Clone, Debug)]
pub struct RegInfo {
    /// `server:workspace:tab`.
    pub addr: String,
    /// The relay queue identity of the session.
    pub queue: String,
    /// Relay-clock milliseconds the registration was last seen.
    pub last_seen_ms: i64,
    /// Whether a herdr pane hosts it.
    pub has_pane: bool,
}

/// One activity event from the relay stream or last-activity hash.
#[derive(Clone, Debug)]
pub struct RawEvent {
    /// Lane address.
    pub addr: String,
    /// Event.
    pub event: HookEvent,
}

/// Parses a stored relay activity event (`addr`, `server_ms`, `kind`); `None` when unreadable.
pub fn parse_event(json: &str) -> Option<RawEvent> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(RawEvent {
        addr: value.get("addr")?.as_str()?.to_owned(),
        event: HookEvent {
            server_ms: value.get("server_ms")?.as_i64()?,
            kind: HookKind::parse(value.get("kind")?.as_str()?)?,
        },
    })
}

/// Everything read from the relay in one pass.
#[derive(Clone, Debug, Default)]
pub struct RelaySnapshot {
    /// Relay server time, Unix milliseconds.
    pub now_ms: i64,
    /// Leased per-host lane lists.
    pub lists: Vec<HostLaneList>,
    /// Lane lists that did not decode.
    pub unreadable_lists: usize,
    /// Relay registrations.
    pub registrations: Vec<RegInfo>,
    /// Activity events, stream and last-activity records.
    pub events: Vec<RawEvent>,
    /// Stored events that did not parse.
    pub unreadable_events: usize,
    /// One reason per registration that could not become a lane address.
    pub registration_defects: Vec<String>,
    /// The activity stream is absent or empty.
    pub activity_absent: bool,
    /// No host lane list exists.
    pub lane_registry_absent: bool,
}

/// The relay `ix.fleet.config` values of one host.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RelayConfig {
    /// `working_window`.
    pub working_window: Option<Duration>,
    /// `max_tool_runtime`.
    pub max_tool_runtime: Option<Duration>,
    /// `no_events_after`.
    pub no_events_after: Option<Duration>,
}

/// One `ix_fleet_probe_ok{host,source}` reading.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeReading {
    /// Probe source.
    pub source: String,
    /// Whether it read.
    pub ok: bool,
}

/// One host's watcher series.
#[derive(Clone, Debug, Default)]
pub struct HostSeries {
    /// The watcher-reported busy ratio for the host.
    pub busy_ratio: Option<f64>,
    /// Why the busy ratio could not be used even though the series was readable.
    pub busy_ratio_error: Option<String>,
    /// Age of the busy ratio sample, independently of the relay configuration series.
    pub busy_sample_age: Option<Duration>,
    /// Lane (lower case) to relay-resolved blocked reason.
    pub blocked: BTreeMap<String, String>,
    /// Probe readings.
    pub probes: Vec<ProbeReading>,
    /// Relay thresholds.
    pub config: RelayConfig,
    /// Cumulative compile seconds per lane (lower case).
    pub compile_seconds: BTreeMap<String, f64>,
    /// Age of the newest pushed sample, if any series exists.
    pub sample_age: Option<Duration>,
    /// Series that returned nothing for this host.
    pub absent: Vec<String>,
}

/// An order whose `AckExpired` receipt the relay recorded (FR-067, FR-071). The relay applies the
/// order deadline; ix-board recomputes none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiredOrder {
    /// Recipient lane address.
    pub addr: String,
    /// Whether a `Delivered` receipt exists; `false` is `timed_out`.
    pub delivered: bool,
}

/// An `AckExpired` receipt whose recipient queue has no registered lane address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedOrder {
    /// The order's original message identity.
    pub order_id: String,
    /// The recipient queue identity the receipt names.
    pub target: String,
}

/// What one read of the relay receipt history found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReceiptRead {
    /// Orders with a recorded `AckExpired` receipt, per lane address.
    pub orders: Vec<ExpiredOrder>,
    /// Expired orders whose recipient has no lane address.
    pub unresolved: Vec<UnresolvedOrder>,
    /// The relay says qualifying expired orders may be missing from `orders` (more exist than it
    /// returned, its scan bound was hit or its index dropped live rows).
    pub truncated: bool,
    /// Rows the relay could not decode; they are not in `orders`.
    pub skipped: u64,
    /// The relay has not run its first expiry scan, so it lists no order yet.
    pub index_not_started: bool,
}

/// Watcher series by host (Prometheus HTTP API).
pub trait SeriesSource: Send + Sync {
    /// Reads one host's series.
    ///
    /// # Errors
    ///
    /// The reason the host could not be read.
    fn host_series(&self, host: &str) -> Result<HostSeries, SourceError>;
}

/// The FR-035 order receipts (`Acted`, `AckExpired`) read from the relay.
pub trait OrderReceipts {
    /// The orders with a recorded `AckExpired` receipt and the limits of the history read.
    ///
    /// # Errors
    ///
    /// The reason the receipts could not be read.
    fn expired_orders(&self) -> Result<ReceiptRead, SourceError>;
}

/// Weak signals (herdr title, pane text, self-report) that a lane is working.
pub trait WeakSignals {
    /// The distinct sources that say `addr` is working.
    fn signals(&self, addr: &str) -> Vec<WeakSource>;
}

/// Every source a report reads.
pub struct Sources<'a> {
    /// The relay read, or why it failed.
    pub relay: Result<RelaySnapshot, SourceError>,
    /// Watcher series; `None` when no Prometheus URL is configured.
    pub series: Option<Arc<dyn SeriesSource>>,
    /// Order receipts.
    pub receipts: &'a dyn OrderReceipts,
    /// Weak signals.
    pub weak: &'a dyn WeakSignals,
}

/// Why a row or source is incomplete.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefectKind {
    /// A configured lane has no registry record.
    MissingLane,
    /// A configured host has no lanes.
    MissingHost,
    /// A registered lane whose last state was not quiet emitted no event for `no_events_after`.
    NoEvents,
    /// The relay has no activity stream, so no lane has hook evidence.
    RelayActivityAbsent,
    /// The relay has no host lane list.
    LaneRegistryAbsent,
    /// A queried Prometheus series returned nothing on any host.
    SeriesAbsent,
    /// Prometheus is readable but the always-present series are absent under the assumed names.
    SeriesNamingUnverified,
    /// A host never published a lane list.
    HostLaneListAbsent,
    /// A host has registered lanes but no hook event from any of them.
    HostHookEventsAbsent,
    /// A source is older than twice its push interval.
    StaleSource,
    /// The relay and ix-board clocks differ by more than `max_clock_skew`.
    ClockSkew,
    /// A source or probe could not be read.
    UnreadableSource,
    /// A lane's host registry lease expired.
    StaleHostRegistry,
    /// The fleet ledger could not be opened or written; the rest of the report stands.
    LedgerUnwritable,
}

/// A source-defect row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Defect {
    /// Defect kind.
    pub kind: DefectKind,
    /// Source naming the defect.
    pub source: String,
    /// Host, if the defect is per host.
    pub host: Option<String>,
    /// Lane, if the defect is per lane.
    pub lane: Option<String>,
    /// Why.
    pub reason: String,
    /// Relay server time of the run that observed it.
    pub observed_ms: i64,
    /// Relay server time of the run that first observed it, from the ledger; absent when the
    /// ledger could not be written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen_ms: Option<i64>,
}

/// One lane row.
#[derive(Clone, Debug, Serialize)]
pub struct LaneRow {
    /// Lane address.
    pub lane: String,
    /// Host.
    pub host: String,
    /// Class.
    pub class: ClassKind,
    /// Class reason, for example `dialog` in `BLOCKED(dialog)`.
    pub reason: Option<String>,
    /// Age of the newest event, seconds on the relay clock.
    pub state_age_secs: Option<u64>,
    /// Relay server time of the newest event.
    pub last_event_ms: Option<i64>,
    /// Evidence items used.
    pub evidence: Vec<Evidence>,
    /// Cumulative compile seconds, when the series reported it.
    pub compile_seconds: Option<f64>,
    /// The lane focus (FR-037); not derived by this command.
    pub focus: Option<String>,
    /// The class used the documented relay defaults because the relay thresholds were not read.
    pub thresholds_defaulted: bool,
    /// Evidence schema version.
    pub schema_version: u32,
}

/// Capacity figures of one host: lanes total, working, and not working (UNREACHABLE counts as
/// not working).
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HostSummary {
    /// Host.
    pub host: String,
    /// Lanes registered on the host.
    pub lanes: usize,
    /// Lanes classed WORKING.
    pub working: usize,
    /// Every other lane, including UNREACHABLE.
    pub not_working: usize,
}

/// How completely a run read the fleet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    /// No defect.
    Complete,
    /// Defects, with rows.
    Partial,
    /// Defects and no row.
    Failed,
}

/// The `fleet status` report.
#[derive(Clone, Debug, Serialize)]
pub struct FleetReport {
    /// Evidence schema version.
    pub schema_version: u32,
    /// `false` when any defect exists.
    pub complete: bool,
    /// Relay server time of the run, Unix milliseconds.
    pub run_ms: i64,
    /// One row per registered lane.
    pub rows: Vec<LaneRow>,
    /// Per-host capacity figures.
    pub hosts: Vec<HostSummary>,
    /// Source defects.
    pub defects: Vec<Defect>,
    /// Usable per-host busy ratios retained from the same bounded read as the status report.
    #[serde(skip)]
    pub host_busy: BTreeMap<String, f64>,
    /// The relay could not be read, so `run_ms` is the host clock, not relay time. Not part of
    /// the wire report.
    #[serde(skip)]
    pub relay_unavailable: bool,
}

impl FleetReport {
    /// Adds the `ledger_unwritable` defect for a run whose ledger write failed.
    pub fn add_ledger_defect(&mut self, reason: String) {
        self.defects.push(Defect {
            kind: DefectKind::LedgerUnwritable,
            source: "ledger".to_owned(),
            host: None,
            lane: None,
            reason,
            observed_ms: self.run_ms,
            first_seen_ms: None,
        });
        self.defects.sort();
        self.complete = false;
    }

    /// The coverage that sets the exit status.
    pub fn coverage(&self) -> Coverage {
        if self.defects.is_empty() {
            Coverage::Complete
        } else if self.rows.is_empty() {
            Coverage::Failed
        } else {
            Coverage::Partial
        }
    }
}

fn host_of(addr: &str) -> String {
    addr.split(':').next().unwrap_or(addr).to_lowercase()
}

type HostReads = BTreeMap<String, Result<HostSeries, SourceError>>;

/// Reads every host in parallel, each bounded by `timeout`; a host that misses it is an error.
fn read_hosts(
    series: &Arc<dyn SeriesSource>,
    hosts: &BTreeSet<String>,
    timeout: Duration,
) -> HostReads {
    let (tx, rx) = mpsc::channel();
    for host in hosts {
        let (tx, series, host) = (tx.clone(), Arc::clone(series), host.clone());
        std::thread::spawn(move || {
            let result = series.host_series(&host);
            let _ = tx.send((host, result));
        });
    }
    drop(tx);
    let started = Instant::now();
    let mut reads = HostReads::new();
    while reads.len() < hosts.len() {
        let left = timeout.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Ok((host, result)) => {
                reads.insert(host, result);
            }
            Err(_) => break,
        }
    }
    for host in hosts {
        reads.entry(host.clone()).or_insert_with(|| {
            Err(SourceError(format!(
                "read exceeded the {}s host bound",
                timeout.as_secs()
            )))
        });
    }
    reads
}

struct Ctx<'a> {
    now_ms: i64,
    defects: Vec<Defect>,
    cfg: &'a FleetConfig,
}

impl Ctx<'_> {
    fn defect(
        &mut self,
        kind: DefectKind,
        source: &str,
        host: Option<&str>,
        lane: Option<&str>,
        reason: impl Into<String>,
    ) {
        self.defects.push(Defect {
            kind,
            source: source.to_owned(),
            host: host.map(str::to_owned),
            lane: lane.map(str::to_owned),
            reason: reason.into(),
            observed_ms: self.now_ms,
            first_seen_ms: None,
        });
    }
}

fn host_defects(ctx: &mut Ctx<'_>, host: &str, read: &Result<HostSeries, SourceError>) {
    match read {
        Err(error) => ctx.defect(
            DefectKind::UnreadableSource,
            "prometheus",
            Some(host),
            None,
            error.0.clone(),
        ),
        Ok(series) => {
            let stale_after = ctx.cfg.push_interval * 2;
            match series.sample_age {
                None => ctx.defect(
                    DefectKind::StaleSource,
                    "prometheus",
                    Some(host),
                    None,
                    "no series pushed within the lookback",
                ),
                Some(sample_age) if sample_age > stale_after => ctx.defect(
                    DefectKind::StaleSource,
                    "prometheus",
                    Some(host),
                    None,
                    format!("newest sample is {}s old", sample_age.as_secs()),
                ),
                Some(_) => {}
            }
            match (
                &series.busy_ratio_error,
                series.busy_ratio,
                series.busy_sample_age,
            ) {
                (Some(reason), _, _) => ctx.defect(
                    DefectKind::UnreadableSource,
                    "prometheus.host_busy",
                    Some(host),
                    None,
                    reason.clone(),
                ),
                (None, None, _) => ctx.defect(
                    DefectKind::UnreadableSource,
                    "prometheus.host_busy",
                    Some(host),
                    None,
                    "ix_fleet_host_busy_ratio has no sample",
                ),
                (None, Some(_), None) => ctx.defect(
                    DefectKind::StaleSource,
                    "prometheus.host_busy",
                    Some(host),
                    None,
                    "busy ratio sample age is unavailable",
                ),
                (None, Some(_), Some(age)) if age > ctx.cfg.push_interval * 2 => ctx.defect(
                    DefectKind::StaleSource,
                    "prometheus.host_busy",
                    Some(host),
                    None,
                    format!("busy ratio sample is {}s old", age.as_secs()),
                ),
                _ => {}
            }
            for probe in series.probes.iter().filter(|p| !p.ok) {
                ctx.defect(
                    DefectKind::UnreadableSource,
                    &format!("probe:{}", probe.source),
                    Some(host),
                    None,
                    "ix_fleet_probe_ok is 0",
                );
            }
            let c = series.config;
            if c.working_window.is_none()
                || c.max_tool_runtime.is_none()
                || c.no_events_after.is_none()
            {
                ctx.defect(
                    DefectKind::UnreadableSource,
                    "prometheus",
                    Some(host),
                    None,
                    "relay thresholds (ix.fleet.config) not reported; documented defaults used",
                );
            }
        }
    }
}

/// Run-level defects for a source that is absent rather than failing.
fn absence_defects(ctx: &mut Ctx<'_>, relay: &RelaySnapshot, reads: &HostReads) {
    if relay.lane_registry_absent {
        ctx.defect(
            DefectKind::LaneRegistryAbsent,
            "relay.registry",
            None,
            None,
            "no host lane list exists; no watcher has published its lanes",
        );
    }
    if relay.activity_absent {
        ctx.defect(
            DefectKind::RelayActivityAbsent,
            "relay.activity",
            None,
            None,
            "the activity stream is absent or empty; no lane has hook evidence",
        );
    }
    for reason in &relay.registration_defects {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.registrations",
            None,
            None,
            reason.clone(),
        );
    }
    let read: Vec<&HostSeries> = reads.values().filter_map(|r| r.as_ref().ok()).collect();
    if read.is_empty() {
        return;
    }
    // The always-present series prove the naming scheme: when neither exists on any readable
    // host, the names are suspect, and one defect says so instead of a row per series.
    let always = [series::PROBE_OK, series::CONFIG];
    let missing = |name: &str| read.iter().all(|s| s.absent.iter().any(|a| a == name));
    let naming_unverified = always.iter().all(|name| missing(name));
    if naming_unverified {
        ctx.defect(
            DefectKind::SeriesNamingUnverified,
            "prometheus",
            None,
            None,
            format!(
                "Prometheus is readable but none of {} exists; the assumed OTLP translation of \
                 the series names may be wrong",
                always.join(", ")
            ),
        );
    }
    let mut names: BTreeSet<&String> = BTreeSet::new();
    for series in &read {
        names.extend(series.absent.iter());
    }
    for name in names {
        if naming_unverified && always.contains(&name.as_str()) {
            continue;
        }
        if read.iter().all(|series| series.absent.contains(name)) {
            ctx.defect(
                DefectKind::SeriesAbsent,
                "prometheus",
                None,
                None,
                format!("series {name} returned nothing on any host"),
            );
        }
    }
}

/// Lanes whose host list lease lapsed, whatever a registration says.
fn lease_lapsed(relay: &RelaySnapshot, addr: &str) -> bool {
    let host = host_of(addr);
    let lowered = addr.to_lowercase();
    let listed = |list: &&HostLaneList| list.lanes.iter().any(|l| l.to_lowercase() == lowered);
    let lapsed_host = relay
        .lists
        .iter()
        .any(|l| l.machine.as_str().to_lowercase() == host && l.expired(relay.now_ms));
    lapsed_host
        && !relay
            .lists
            .iter()
            .filter(listed)
            .any(|l| !l.expired(relay.now_ms))
}

/// Resolves each threshold: the relay value, else the documented relay default. The flag is
/// `true` when any default was used.
fn thresholds(series: Option<&HostSeries>, cfg: &FleetConfig) -> (Thresholds, bool) {
    let relay = series.map(|s| s.config).unwrap_or_default();
    let defaulted = relay.working_window.is_none()
        || relay.max_tool_runtime.is_none()
        || relay.no_events_after.is_none();
    (
        Thresholds {
            working_window: Some(relay.working_window.unwrap_or(RELAY_DEFAULT_WORKING_WINDOW)),
            max_tool_runtime: Some(
                relay
                    .max_tool_runtime
                    .unwrap_or(RELAY_DEFAULT_MAX_TOOL_RUNTIME),
            ),
            no_events_after: Some(
                relay
                    .no_events_after
                    .unwrap_or(RELAY_DEFAULT_NO_EVENTS_AFTER),
            ),
            loop_stops: cfg.loop_stops,
            dead_after: cfg.dead_after,
        },
        defaulted,
    )
}

/// Builds the report: one row per registered lane, plus source defects.
pub fn build_report(cfg: &FleetConfig, sources: &Sources<'_>, local_now_ms: i64) -> FleetReport {
    let relay = match &sources.relay {
        Ok(relay) => relay,
        Err(error) => {
            let mut ctx = Ctx {
                now_ms: local_now_ms,
                defects: Vec::new(),
                cfg,
            };
            ctx.defect(
                DefectKind::UnreadableSource,
                "relay",
                None,
                None,
                error.0.clone(),
            );
            let mut report = finish(local_now_ms, Vec::new(), ctx.defects);
            report.relay_unavailable = true;
            return report;
        }
    };
    let mut ctx = Ctx {
        now_ms: relay.now_ms,
        defects: Vec::new(),
        cfg,
    };
    let skew = relay.now_ms.abs_diff(local_now_ms);
    if skew > u64::try_from(cfg.max_clock_skew.as_millis()).unwrap_or(u64::MAX) {
        ctx.defect(
            DefectKind::ClockSkew,
            "relay",
            None,
            None,
            format!("relay and local clocks differ by {skew}ms"),
        );
    }
    if relay.unreadable_lists > 0 {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.registry",
            None,
            None,
            format!("{} host lane lists did not decode", relay.unreadable_lists),
        );
    }
    if relay.unreadable_events > 0 {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.activity",
            None,
            None,
            format!("{} stored events did not parse", relay.unreadable_events),
        );
    }
    let registrations: Vec<String> = relay.registrations.iter().map(|r| r.addr.clone()).collect();
    let registry = fleet_registry(&relay.lists, &registrations, relay.now_ms);
    let lane_hosts: BTreeSet<String> = registry.iter().map(|l| host_of(&l.address)).collect();
    for host in cfg.hosts.iter().map(|h| h.to_lowercase()) {
        if !lane_hosts.contains(&host) {
            ctx.defect(
                DefectKind::MissingHost,
                "relay.registry",
                Some(&host),
                None,
                "configured host has no lanes",
            );
        }
    }
    let registered: BTreeSet<String> = registry.iter().map(|l| l.address.to_lowercase()).collect();
    for lane in &cfg.lanes {
        if !registered.contains(&lane.to_lowercase()) {
            ctx.defect(
                DefectKind::MissingLane,
                "relay.registry",
                Some(&host_of(lane)),
                Some(lane),
                "configured lane has no registry record",
            );
        }
    }
    let mut hosts = lane_hosts.clone();
    hosts.extend(cfg.hosts.iter().map(|h| h.to_lowercase()));
    let reads = match &sources.series {
        Some(series) => read_hosts(series, &hosts, cfg.host_timeout),
        None => {
            ctx.defect(
                DefectKind::UnreadableSource,
                "prometheus",
                None,
                None,
                "set IX_PROMETHEUS_URL or [fleet] prometheus_url; documented relay defaults used",
            );
            HostReads::new()
        }
    };
    for (host, read) in &reads {
        host_defects(&mut ctx, host, read);
    }
    absence_defects(&mut ctx, relay, &reads);
    let read = match sources.receipts.expired_orders() {
        Ok(read) => read,
        Err(error) => {
            ctx.defect(
                DefectKind::UnreadableSource,
                "relay.receipts",
                None,
                None,
                error.0,
            );
            ReceiptRead::default()
        }
    };
    let orders = &read.orders;
    if read.index_not_started {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.receipts",
            None,
            None,
            "the relay has not indexed expired orders yet; a STALE-ORDER lane may be missing"
                .to_owned(),
        );
    }
    if read.truncated {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.receipts",
            None,
            None,
            "the relay's expired-order list is truncated; a STALE-ORDER lane may be missing"
                .to_owned(),
        );
    }
    if read.skipped > 0 {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.receipts",
            None,
            None,
            format!(
                "{} expired-order rows could not be decoded by the relay; a STALE-ORDER lane may be missing",
                read.skipped
            ),
        );
    }
    let mut unresolved: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for order in &read.unresolved {
        unresolved
            .entry(order.target.as_str())
            .or_default()
            .push(order.order_id.as_str());
    }
    for (target, ids) in &unresolved {
        ctx.defect(
            DefectKind::UnreadableSource,
            "relay.receipts",
            None,
            None,
            format!(
                "AckExpired orders {} to {target} have no registered lane address",
                ids.join(", ")
            ),
        );
    }
    let mut events: BTreeMap<String, Vec<HookEvent>> = BTreeMap::new();
    for raw in &relay.events {
        events
            .entry(raw.addr.to_lowercase())
            .or_default()
            .push(raw.event);
    }
    let published: BTreeSet<String> = relay
        .lists
        .iter()
        .map(|l| l.machine.as_str().to_lowercase())
        .collect();
    let no_list_hosts: BTreeSet<String> = lane_hosts.difference(&published).cloned().collect();
    for host in &no_list_hosts {
        ctx.defect(
            DefectKind::HostLaneListAbsent,
            "relay.registry",
            Some(host),
            None,
            "the host never published a lane list; its lanes are unreachable",
        );
    }
    if !relay.activity_absent {
        let hosts_with_events: BTreeSet<String> =
            relay.events.iter().map(|raw| host_of(&raw.addr)).collect();
        for host in lane_hosts.difference(&hosts_with_events) {
            ctx.defect(
                DefectKind::HostHookEventsAbsent,
                "relay.activity",
                Some(host),
                None,
                "no hook event from any lane of the host; hooks are not installed or failing",
            );
        }
    }
    let mut rows = Vec::new();
    for lane in &registry {
        let key = lane.address.to_lowercase();
        let host = host_of(&lane.address);
        let lapsed = lane.stale || lease_lapsed(relay, &lane.address);
        let stale = lapsed || no_list_hosts.contains(&host);
        let mut lane_events = events.remove(&key).unwrap_or_default();
        lane_events.sort();
        lane_events.dedup();
        let series = reads.get(&host).and_then(|read| read.as_ref().ok());
        let (th, defaulted) = thresholds(series, cfg);
        let registration = relay
            .registrations
            .iter()
            .find(|r| r.addr.to_lowercase() == key);
        let blocked = series.and_then(|s| s.blocked.get(&key).cloned());
        let input = LaneInput {
            addr: lane.address.clone(),
            stale,
            events: lane_events,
            registered_age: registration.map(|r| age(relay.now_ms, r.last_seen_ms)),
            has_pane: registration.map(|r| r.has_pane),
            blocked_reason: blocked.clone(),
            expired_order: orders
                .iter()
                .filter(|o| o.addr.to_lowercase() == key)
                .map(|o| o.delivered)
                .next(),
            weak: sources.weak.signals(&lane.address),
        };
        let result = classify(&input, &th, relay.now_ms);
        let last = input.events.last().copied();
        if lapsed {
            ctx.defect(
                DefectKind::StaleHostRegistry,
                "relay.registry",
                Some(&host),
                Some(&lane.address),
                "host registry lease expired",
            );
        }
        if let Some(reason) = blocked.as_deref()
            && reason != "wait_loop"
            && super::classify::blocked_class(reason).is_none()
        {
            ctx.defect(
                DefectKind::UnreadableSource,
                "prometheus",
                Some(&host),
                Some(&lane.address),
                format!("unmapped blocked reason {reason}"),
            );
        }
        let quiet = last.is_none_or(|e| {
            matches!(
                e.kind,
                HookKind::Stop | HookKind::Idle | HookKind::SessionEnd | HookKind::Blocked
            )
        });
        if !stale
            && !quiet
            && th
                .no_events_after
                .is_some_and(|limit| last.is_some_and(|e| age(relay.now_ms, e.server_ms) > limit))
        {
            ctx.defect(
                DefectKind::NoEvents,
                "relay.activity",
                Some(&host),
                Some(&lane.address),
                "last state was not quiet and no hook event within no_events_after",
            );
        }
        rows.push(LaneRow {
            lane: lane.address.clone(),
            host,
            class: result.class.kind,
            reason: result.class.reason,
            state_age_secs: last.map(|e| age(relay.now_ms, e.server_ms).as_secs()),
            last_event_ms: last.map(|e| e.server_ms),
            evidence: result.evidence,
            compile_seconds: series.and_then(|s| s.compile_seconds.get(&key).copied()),
            focus: None,
            thresholds_defaulted: defaulted,
            schema_version: SCHEMA_VERSION,
        });
    }
    let mut report = finish(relay.now_ms, rows, ctx.defects);
    for (host, reading) in &reads {
        if let Ok(reading) = reading
            && reading.busy_ratio_error.is_none()
            && reading
                .busy_sample_age
                .is_some_and(|age| age <= cfg.push_interval * 2)
            && let Some(ratio) = reading.busy_ratio
        {
            report.host_busy.insert(host.to_lowercase(), ratio);
        }
    }
    report
}

fn summarize(rows: &[LaneRow]) -> Vec<HostSummary> {
    let mut by_host: BTreeMap<&str, HostSummary> = BTreeMap::new();
    for row in rows {
        let entry = by_host.entry(&row.host).or_insert_with(|| HostSummary {
            host: row.host.clone(),
            lanes: 0,
            working: 0,
            not_working: 0,
        });
        entry.lanes += 1;
        if row.class == ClassKind::Working {
            entry.working += 1;
        } else {
            entry.not_working += 1;
        }
    }
    by_host.into_values().collect()
}

fn finish(run_ms: i64, mut rows: Vec<LaneRow>, mut defects: Vec<Defect>) -> FleetReport {
    rows.sort_by(|a, b| a.lane.cmp(&b.lane));
    defects.sort();
    FleetReport {
        schema_version: SCHEMA_VERSION,
        complete: defects.is_empty(),
        run_ms,
        hosts: summarize(&rows),
        rows,
        defects,
        host_busy: BTreeMap::new(),
        relay_unavailable: false,
    }
}
