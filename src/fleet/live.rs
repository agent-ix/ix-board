// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Live readers: relay Redis through relay-core, Prometheus over its HTTP API, and the order
//! receipt reader and the weak-signal seam.

use super::SourceError;
use super::classify::WeakSource;
use super::report::{
    ExpiredOrder, HostSeries, OrderReceipts, ProbeReading, ReceiptRead, RegInfo, RelayConfig,
    RelaySnapshot, SeriesSource, UnresolvedOrder, WeakSignals, parse_event,
};
use relay_core::config::{CoreFlags, load_core};
use relay_core::receipts::{
    ExpiredOrders, RECEIPT_EVENT_CAP, RECEIPT_PAGE_CAP, ReceiptCoverageReason, ReceiptEvent,
    ReceiptExport,
};
use relay_core::{ACTIVITY_MAX_EVENTS, Address, Relay};
use std::time::Duration;

/// Reads the relay snapshot and the order receipts over one bounded connection. The URL and key
/// prefix resolve like every ix-relay binary (`IX_RELAY_URL`, then `~/.ix/relay/config.toml`).
/// The receipts are matched against the snapshot's registrations, so neither is read twice.
pub fn read_relay(timeout: Duration) -> (Result<RelaySnapshot, SourceError>, ReadReceipts) {
    let connected = load_core(CoreFlags::default())
        .map_err(|e| SourceError(e.to_string()))
        .and_then(|settings| {
            Ok(Relay::connect(&settings.url.value, timeout)
                .map_err(|e| SourceError(format!("relay connect: {e}")))?
                .with_prefix(settings.prefix.value.clone()))
        });
    let mut relay = match connected {
        Ok(relay) => relay,
        Err(error) => return (Err(error.clone()), ReadReceipts(Err(error))),
    };
    read_relay_on(&mut relay, timeout)
}

/// [`read_relay`] over a connection the caller made.
pub fn read_relay_on(
    relay: &mut Relay,
    timeout: Duration,
) -> (Result<RelaySnapshot, SourceError>, ReadReceipts) {
    let snapshot = read_snapshot(relay, timeout);
    let receipts = match &snapshot {
        Ok(s) => read_receipts(relay, &s.registrations),
        Err(error) => Err(error.clone()),
    };
    (snapshot, ReadReceipts(receipts))
}

fn read_snapshot(relay: &mut Relay, timeout: Duration) -> Result<RelaySnapshot, SourceError> {
    let fail = |what: &str, e: relay_core::Error| SourceError(format!("relay {what}: {e}"));
    let lists = relay.lane_lists().map_err(|e| fail("lane lists", e))?;
    let agents = relay.agents().map_err(|e| fail("registrations", e))?;
    let mut registrations = Vec::new();
    let mut registration_defects = Vec::new();
    for reg in &agents.registrations {
        match reg.full_address() {
            Some(addr) => registrations.push(RegInfo {
                addr: addr.to_string(),
                queue: reg.name.as_str().to_owned(),
                last_seen_ms: reg.last_seen_ms,
                has_pane: reg.herdr_pane.is_some(),
            }),
            None => registration_defects.push(format!(
                "registration {}: no resolvable server:workspace:tab address (no verified place)",
                reg.name
            )),
        }
    }
    for skipped in &agents.skipped {
        registration_defects.push(format!(
            "registration {}: record did not decode: {}",
            skipped.field, skipped.error
        ));
    }
    let lane_registry_absent = lists.lists.is_empty();
    let mut snapshot = RelaySnapshot {
        now_ms: lists.now_ms,
        lists: lists.lists,
        unreadable_lists: lists.unreadable,
        lane_registry_absent,
        registrations,
        registration_defects,
        ..RelaySnapshot::default()
    };
    snapshot.activity_absent = relay
        .activity_len()
        .map_err(|e| fail("activity length", e))?
        == 0;
    let stream = relay
        .activity_events(ACTIVITY_MAX_EVENTS)
        .map_err(|e| fail("activity stream", e))?;
    for line in &stream {
        match parse_event(line) {
            Some(raw) => snapshot.events.push(raw),
            None => snapshot.unreadable_events += 1,
        }
    }
    let mut addresses: Vec<String> = relay_core::fleet::fleet_registry(
        &snapshot.lists,
        &snapshot
            .registrations
            .iter()
            .map(|r| r.addr.clone())
            .collect::<Vec<_>>(),
        snapshot.now_ms,
    )
    .into_iter()
    .map(|lane| lane.address)
    .collect();
    addresses.sort();
    let parsed: Vec<Address> = addresses.iter().filter_map(|a| a.parse().ok()).collect();
    let lasts = relay
        .activity_last_many(&parsed, timeout)
        .map_err(|e| fail("last activity", e))?;
    for last in lasts {
        for (field, json) in last {
            if field.starts_with("kind:") || field == "any" {
                match parse_event(&json) {
                    Some(raw) => snapshot.events.push(raw),
                    None => snapshot.unreadable_events += 1,
                }
            }
        }
    }
    Ok(snapshot)
}

/// Runs one PromQL instant query and returns the response body.
pub trait PromQuery: Send + Sync {
    /// Evaluates `expr`.
    ///
    /// # Errors
    ///
    /// The transport or HTTP failure.
    fn query(&self, expr: &str) -> Result<String, SourceError>;
}

/// The Prometheus HTTP API over a bounded blocking client.
#[derive(Clone)]
pub struct PromHttp {
    base: String,
    client: reqwest::blocking::Client,
}

impl PromHttp {
    /// A client for `base` whose every request is bounded by `timeout`.
    ///
    /// # Errors
    ///
    /// The client could not be built.
    pub fn new(base: &str, timeout: Duration) -> Result<Self, SourceError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| SourceError(format!("prometheus client: {e}")))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            client,
        })
    }
}

impl PromQuery for PromHttp {
    fn query(&self, expr: &str) -> Result<String, SourceError> {
        let response = self
            .client
            .get(format!("{}/api/v1/query", self.base))
            .query(&[("query", expr)])
            .send()
            .map_err(|e| SourceError(format!("prometheus: {e}")))?;
        if !response.status().is_success() {
            return Err(SourceError(format!(
                "prometheus: HTTP {}",
                response.status()
            )));
        }
        response
            .text()
            .map_err(|e| SourceError(format!("prometheus: {e}")))
    }
}

/// Prometheus series names, derived in this one place from ix-relay FR-045's declared kinds and
/// units and Prometheus 3's default OTLP translation (`UnderscoreEscapingWithSuffixes`): dots
/// become underscores, unit `s` adds `_seconds` unless the name already ends so, unit `1` on a
/// gauge adds `_ratio`, and a monotonic sum adds `_total`. Live evidence of the rule is
/// `herdr_pane_status_ratio` for `herdr.pane.status` (gauge, unit `1`).
pub mod series {
    /// `ix.fleet.lane.blocked`, gauge, unit `1`. Exported only while a lane is blocked, so an
    /// empty result is normal and is not a `series_absent` defect.
    pub const LANE_BLOCKED: &str = "ix_fleet_lane_blocked_ratio";
    /// `ix.fleet.probe_ok`, gauge, unit `1`.
    pub const PROBE_OK: &str = "ix_fleet_probe_ok_ratio";
    /// `ix.fleet.config`, gauge, unit `s`.
    pub const CONFIG: &str = "ix_fleet_config_seconds";
    /// `ix.fleet.lane.compile_seconds`, monotonic sum, unit `s`.
    pub const COMPILE_SECONDS: &str = "ix_fleet_lane_compile_seconds_total";
    /// `ix.fleet.host.busy_ratio`, gauge, unit `1`.
    pub const HOST_BUSY_RATIO: &str = "ix_fleet_host_busy_ratio";
}

/// A series source that cannot read; the reason is the defect text of every host.
pub struct UnavailablePrometheus(pub String);

impl SeriesSource for UnavailablePrometheus {
    fn host_series(&self, _host: &str) -> Result<HostSeries, SourceError> {
        Err(SourceError(self.0.clone()))
    }
}

/// The watcher series read through a [`PromQuery`].
pub struct PrometheusSeries<Q: PromQuery> {
    /// The query transport.
    pub query: Q,
}

type Sample = (std::collections::BTreeMap<String, String>, f64);

fn samples(body: &str) -> Result<Vec<Sample>, SourceError> {
    let bad = |what: &str| SourceError(format!("prometheus response: {what}"));
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| bad("not JSON"))?;
    if value["status"] != "success" {
        return Err(bad("status is not success"));
    }
    let result = value["data"]["result"]
        .as_array()
        .ok_or_else(|| bad("no result vector"))?;
    result
        .iter()
        .map(|item| {
            let metric = item["metric"]
                .as_object()
                .ok_or_else(|| bad("sample without labels"))?
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect();
            let number = item["value"][1]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .ok_or_else(|| bad("sample without a numeric value"))?;
            Ok((metric, number))
        })
        .collect()
}

fn escape(host: &str) -> String {
    host.replace('\\', "\\\\").replace('"', "\\\"")
}

fn secs(value: f64) -> Option<Duration> {
    (value.is_finite() && value >= 0.0).then(|| Duration::from_secs_f64(value))
}

impl<Q: PromQuery> SeriesSource for PrometheusSeries<Q> {
    fn host_series(&self, host: &str) -> Result<HostSeries, SourceError> {
        let host = escape(host);
        let read = |expr: String| -> Result<Vec<Sample>, SourceError> {
            samples(&self.query.query(&expr)?)
        };
        let mut series = HostSeries::default();
        let busy = read(format!("{}{{host=\"{host}\"}}", series::HOST_BUSY_RATIO))?;
        match busy.as_slice() {
            [] => series.absent.push(series::HOST_BUSY_RATIO.to_owned()),
            [(_, value)] if value.is_finite() && (0.0..=1.0).contains(value) => {
                series.busy_ratio = Some(*value);
            }
            [_] => {
                series.busy_ratio_error =
                    Some("host busy ratio must be finite and between 0 and 1".to_owned());
            }
            _ => {
                series.busy_ratio_error =
                    Some("multiple host busy ratio samples for one host".to_owned());
            }
        }
        let busy_ages = read(format!(
            "time() - timestamp({}{{host=\"{host}\"}})",
            series::HOST_BUSY_RATIO
        ))?;
        series.busy_sample_age = busy_ages
            .into_iter()
            .filter_map(|(_, value)| secs(value))
            .max();
        for (labels, value) in read(format!("{}{{host=\"{host}\"}}", series::LANE_BLOCKED))? {
            if value >= 1.0
                && let (Some(lane), Some(reason)) = (labels.get("lane"), labels.get("reason"))
            {
                series.blocked.insert(lane.to_lowercase(), reason.clone());
            }
        }
        let probes = read(format!("{}{{host=\"{host}\"}}", series::PROBE_OK))?;
        if probes.is_empty() {
            series.absent.push(series::PROBE_OK.to_owned());
        }
        for (labels, value) in probes {
            if let Some(source) = labels.get("source") {
                series.probes.push(ProbeReading {
                    source: source.clone(),
                    ok: value >= 1.0,
                });
            }
        }
        series.probes.sort_by(|a, b| a.source.cmp(&b.source));
        let mut config = RelayConfig::default();
        let configs = read(format!("{}{{host=\"{host}\"}}", series::CONFIG))?;
        if configs.is_empty() {
            series.absent.push(series::CONFIG.to_owned());
        }
        for (labels, value) in configs {
            match labels.get("name").map(String::as_str) {
                Some("working_window") => config.working_window = secs(value),
                Some("max_tool_runtime") => config.max_tool_runtime = secs(value),
                Some("no_events_after") => config.no_events_after = secs(value),
                _ => {}
            }
        }
        series.config = config;
        let compiles = read(format!("{}{{host=\"{host}\"}}", series::COMPILE_SECONDS))?;
        if compiles.is_empty() {
            series.absent.push(series::COMPILE_SECONDS.to_owned());
        }
        for (labels, value) in compiles {
            if let Some(lane) = labels.get("lane") {
                series.compile_seconds.insert(lane.to_lowercase(), value);
            }
        }
        let ages = read(format!(
            "time() - timestamp({}{{host=\"{host}\"}})",
            series::CONFIG
        ))?;
        series.sample_age = ages.into_iter().filter_map(|(_, value)| secs(value)).max();
        Ok(series)
    }
}

/// The most pages the retained receipt history can fill: its event cap over the page size, plus
/// the final partial page.
const RECEIPT_MAX_PAGES: usize = RECEIPT_EVENT_CAP / RECEIPT_PAGE_CAP + 1;

/// Reads every retained receipt through `fetch`, which returns the page after a cursor. The
/// request spans the relay's whole receipt retention. `fleet assign` and the assignment
/// completion read receipts through it.
///
/// # Errors
///
/// The page could not be read or continued, or the history needs more pages than it can hold.
pub fn collect_receipts(
    mut fetch: impl FnMut(Option<String>) -> Result<ReceiptExport, SourceError>,
) -> Result<(Vec<ReceiptEvent>, Vec<String>), SourceError> {
    let mut events = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    let mut after = None;
    for _ in 0..RECEIPT_MAX_PAGES {
        let page = fetch(after.take())?;
        for reason in &page.reasons {
            let name = match reason {
                ReceiptCoverageReason::SourceUnavailable
                | ReceiptCoverageReason::CursorEvicted
                | ReceiptCoverageReason::CursorUnavailable => {
                    return Err(SourceError(format!(
                        "relay receipts incomplete: {reason:?}"
                    )));
                }
                ReceiptCoverageReason::BeforeInstrumentation => continue,
                ReceiptCoverageReason::RecordingGap => "recording_gap",
                ReceiptCoverageReason::ObservationPending => "observation_pending",
                ReceiptCoverageReason::Retention => "retention",
            };
            if !gaps.iter().any(|g| g == name) {
                gaps.push(name.to_owned());
            }
        }
        if !page.complete && page.reasons.is_empty() && !gaps.iter().any(|g| g == "incomplete") {
            gaps.push("incomplete".to_owned());
        }
        events.extend(page.events.into_iter().map(|row| row.event));
        if !page.has_more {
            return Ok((events, gaps));
        }
        after = page.next_cursor;
    }
    Err(SourceError(format!(
        "relay receipts exceed {RECEIPT_MAX_PAGES} pages"
    )))
}

fn read_receipts(relay: &mut Relay, registrations: &[RegInfo]) -> Result<ReceiptRead, SourceError> {
    let listed = relay
        .expired_orders()
        .map_err(|e| SourceError(format!("relay expired orders: {e}")))?;
    let addrs: Vec<String> = registrations.iter().map(|reg| reg.addr.clone()).collect();
    Ok(receipt_read_from(&listed, &addrs))
}

/// Maps the relay's read-only expired-order list (`Relay::expired_orders`) to the report's
/// receipt read. The relay applies the deadline and reports whether a `Delivered` receipt came
/// before expiry, so `truncated`, `skipped` and an index that has not started are carried to the
/// report as defects. A recipient that is not a registered lane address is unresolved.
#[must_use]
pub fn receipt_read_from(listed: &ExpiredOrders, lanes: &[String]) -> ReceiptRead {
    let mut read = ReceiptRead {
        truncated: listed.truncated,
        skipped: listed.skipped,
        index_not_started: listed.indexed_since_ms.is_none(),
        ..ReceiptRead::default()
    };
    for order in &listed.orders {
        let recipient: Option<Address> = order.recipient.parse().ok();
        let lane = lanes
            .iter()
            .find(|lane| lane.parse::<Address>().ok() == recipient && recipient.is_some());
        match lane {
            Some(addr) => read.orders.push(ExpiredOrder {
                addr: addr.clone(),
                delivered: order.delivered_before_expiry,
            }),
            None => read.unresolved.push(UnresolvedOrder {
                order_id: order.original_id.clone(),
                target: order.recipient.clone(),
            }),
        }
    }
    read
}

/// The receipts read together with the relay snapshot, handed to the report as its order source.
pub struct ReadReceipts(pub Result<ReceiptRead, SourceError>);

impl OrderReceipts for ReadReceipts {
    fn expired_orders(&self) -> Result<ReceiptRead, SourceError> {
        self.0.clone()
    }
}

/// The seam for herdr weak signals (title, pane text). The live command wires none yet, so only
/// hook evidence makes WORKING; live weak signals are SWM-264.
pub struct NoWeakSignals;

impl WeakSignals for NoWeakSignals {
    fn signals(&self, _addr: &str) -> Vec<WeakSource> {
        Vec::new()
    }
}

/// The lane of the calling session when its harness session id (`CODEX_THREAD_ID`,
/// `CLAUDE_CODE_SESSION_ID` or `CLAUDE_SESSION_ID`) matches a registration with a verified place.
/// `None` when the caller is not a registered lane, so the own-lane guard has nothing to compare.
pub fn caller_lane(relay: &mut Relay, machine: &relay_core::MachineName) -> Option<Address> {
    let session = [
        "CODEX_THREAD_ID",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_SESSION_ID",
    ]
    .iter()
    .find_map(|name| std::env::var(name).ok())?;
    relay
        .agents()
        .ok()?
        .registrations
        .into_iter()
        .find(|r| {
            r.machine == *machine
                && r.session_id
                    .as_ref()
                    .is_some_and(|s| s.to_string() == session)
        })
        .and_then(|r| r.verified_place().map(|p| p.address(&r.machine)))
}
