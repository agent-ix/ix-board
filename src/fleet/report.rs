// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Source traits, the report model and the builder that joins the sources on the lane address.

use super::classify::{
    ClassKind, Evidence, HookEvent, HookKind, LaneInput, Thresholds, WeakSource, age, classify,
};
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
}

/// An order with an `AckExpired` receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiredOrder {
    /// Recipient lane address.
    pub addr: String,
    /// Whether a `Delivered` receipt exists; `false` is `timed_out`.
    pub delivered: bool,
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
    /// Orders with an `AckExpired` receipt.
    ///
    /// # Errors
    ///
    /// The reason the receipts could not be read.
    fn expired_orders(&self) -> Result<Vec<ExpiredOrder>, SourceError>;
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
    /// Watcher series.
    pub series: Arc<dyn SeriesSource>,
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
    /// A registered lane emitted no event for `no_events_after`.
    NoEvents,
    /// A source is older than twice its push interval.
    StaleSource,
    /// The relay and ix-board clocks differ by more than `max_clock_skew`.
    ClockSkew,
    /// A source or probe could not be read.
    UnreadableSource,
    /// A lane's host registry lease expired.
    StaleHostRegistry,
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
    /// Relay server time of the run that first saw it.
    pub first_seen_ms: i64,
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
    /// Evidence schema version.
    pub schema_version: u32,
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
    /// Source defects.
    pub defects: Vec<Defect>,
}

impl FleetReport {
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
            first_seen_ms: self.now_ms,
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
                    "ix.fleet.config thresholds not reported",
                );
            }
        }
    }
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
            return finish(local_now_ms, Vec::new(), ctx.defects);
        }
    };
    let mut ctx = Ctx {
        now_ms: relay.now_ms,
        defects: Vec::new(),
        cfg,
    };
    if relay.now_ms.abs_diff(local_now_ms)
        > u64::try_from(cfg.max_clock_skew.as_millis()).unwrap_or(u64::MAX)
    {
        ctx.defect(
            DefectKind::ClockSkew,
            "relay",
            None,
            None,
            format!(
                "relay and local clocks differ by {}ms",
                relay.now_ms.abs_diff(local_now_ms)
            ),
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
    let lane_hosts: BTreeSet<String> = registry.iter().map(|lane| host_of(&lane.address)).collect();
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
    let reads = read_hosts(&sources.series, &hosts, cfg.host_timeout);
    for (host, read) in &reads {
        host_defects(&mut ctx, host, read);
    }
    let expired = match sources.receipts.expired_orders() {
        Ok(orders) => orders,
        Err(error) => {
            ctx.defect(
                DefectKind::UnreadableSource,
                "relay.receipts",
                None,
                None,
                error.0,
            );
            Vec::new()
        }
    };
    let mut events: BTreeMap<String, Vec<HookEvent>> = BTreeMap::new();
    for raw in &relay.events {
        events
            .entry(raw.addr.to_lowercase())
            .or_default()
            .push(raw.event);
    }
    let mut rows = Vec::new();
    for lane in &registry {
        let key = lane.address.to_lowercase();
        let host = host_of(&lane.address);
        let mut lane_events = events.remove(&key).unwrap_or_default();
        lane_events.sort();
        lane_events.dedup();
        let series = reads.get(&host).and_then(|read| read.as_ref().ok());
        let relay_cfg = series.map(|s| s.config).unwrap_or_default();
        let th = Thresholds {
            working_window: relay_cfg.working_window,
            max_tool_runtime: relay_cfg.max_tool_runtime,
            no_events_after: relay_cfg.no_events_after,
            loop_stops: cfg.loop_stops,
            dead_after: cfg.dead_after,
        };
        let registration = relay
            .registrations
            .iter()
            .find(|r| r.addr.to_lowercase() == key);
        let blocked = series.and_then(|s| s.blocked.get(&key).cloned());
        let input = LaneInput {
            addr: lane.address.clone(),
            stale: lane.stale,
            events: lane_events,
            registered_age: registration.map(|r| age(relay.now_ms, r.last_seen_ms)),
            has_pane: registration.map(|r| r.has_pane),
            blocked_reason: blocked.clone(),
            expired_order: expired
                .iter()
                .find(|o| o.addr.to_lowercase() == key)
                .map(|o| o.delivered),
            weak: sources.weak.signals(&lane.address),
        };
        let result = classify(&input, &th, relay.now_ms);
        let last = input.events.last().copied();
        if lane.stale {
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
        if !lane.stale
            && result.class.kind != ClassKind::Dead
            && th
                .no_events_after
                .is_some_and(|limit| last.is_none_or(|e| age(relay.now_ms, e.server_ms) > limit))
        {
            ctx.defect(
                DefectKind::NoEvents,
                "relay.activity",
                Some(&host),
                Some(&lane.address),
                "no hook event within no_events_after",
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
            schema_version: SCHEMA_VERSION,
        });
    }
    finish(relay.now_ms, rows, ctx.defects)
}

fn finish(run_ms: i64, mut rows: Vec<LaneRow>, mut defects: Vec<Defect>) -> FleetReport {
    rows.sort_by(|a, b| a.lane.cmp(&b.lane));
    defects.sort();
    FleetReport {
        schema_version: SCHEMA_VERSION,
        complete: defects.is_empty(),
        run_ms,
        rows,
        defects,
    }
}
