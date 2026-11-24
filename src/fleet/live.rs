// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Live readers: relay Redis through relay-core, Prometheus over its HTTP API, and the order
//! receipt reader and the weak-signal seam.

use super::SourceError;
use super::classify::WeakSource;
use super::report::{
    ExpiredOrder, HostSeries, OrderReceipts, ProbeReading, RegInfo, RelayConfig, RelaySnapshot,
    SeriesSource, WeakSignals, parse_event,
};
use relay_core::config::{CoreFlags, load_core};
use relay_core::receipts::{
    ORDER_ACK_MAX_MS, RECEIPT_PAGE_CAP, ReceiptCoverageReason, ReceiptEvent, ReceiptOutcome,
    ReceiptRequest,
};
use relay_core::{ACTIVITY_MAX_EVENTS, Address, Relay};
use std::collections::BTreeMap;
use std::time::Duration;

/// Reads the relay snapshot with one bounded connection. The URL and key prefix resolve like
/// every ix-relay binary (`IX_RELAY_URL`, then `~/.ix/relay/config.toml`).
///
/// # Errors
///
/// The reason the relay could not be read.
pub fn read_relay(timeout: Duration) -> Result<RelaySnapshot, SourceError> {
    let settings = load_core(CoreFlags::default()).map_err(|e| SourceError(e.to_string()))?;
    let mut relay = Relay::connect(&settings.url.value, timeout)
        .map_err(|e| SourceError(format!("relay connect: {e}")))?
        .with_prefix(settings.prefix.value.clone());
    let fail = |what: &str, e: relay_core::Error| SourceError(format!("relay {what}: {e}"));
    let lists = relay.lane_lists().map_err(|e| fail("lane lists", e))?;
    let agents = relay.agents().map_err(|e| fail("registrations", e))?;
    let mut registrations = Vec::new();
    let mut registration_defects = Vec::new();
    for reg in &agents.registrations {
        match reg.full_address() {
            Some(addr) => registrations.push(RegInfo {
                addr: addr.to_string(),
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

/// How far back the live reader asks the relay for receipts. This bounds the read only: whether an
/// order expired is the relay's `AckExpired` record, never a deadline computed here. Twice the
/// longest order ack deadline, so a `Delivered` receipt that precedes a recorded expiry is in view.
const RECEIPT_WINDOW_MS: i64 = 2 * ORDER_ACK_MAX_MS;

/// Most receipt pages one run reads.
const RECEIPT_MAX_PAGES: usize = 20;

/// Orders with a recorded `AckExpired` receipt (FR-067, FR-071), read from the relay's receipt
/// stream for the lanes in `lanes`, a map from a session queue identity to its lane address.
///
/// An order that later has an `Acted` receipt is not expired. An order with no `Delivered` receipt
/// is `timed_out` (`delivered` false).
#[must_use]
pub fn expired_orders_from(
    events: &[ReceiptEvent],
    lanes: &BTreeMap<String, String>,
) -> Vec<ExpiredOrder> {
    let seen = |id: &str, outcome: ReceiptOutcome| {
        events
            .iter()
            .any(|e| e.original_id == id && e.outcome == outcome)
    };
    let mut orders = Vec::new();
    for event in events {
        if event.outcome != ReceiptOutcome::AckExpired
            || seen(&event.original_id, ReceiptOutcome::Acted)
        {
            continue;
        }
        if let Some(addr) = lanes.get(&event.target) {
            orders.push(ExpiredOrder {
                addr: addr.clone(),
                delivered: seen(&event.original_id, ReceiptOutcome::Delivered),
            });
        }
    }
    orders
}

/// The order receipts read from the relay Redis through relay-core with one bounded connection.
pub struct RelayOrderReceipts {
    /// The connect and read bound.
    pub timeout: Duration,
}

impl OrderReceipts for RelayOrderReceipts {
    fn expired_orders(&self) -> Result<Vec<ExpiredOrder>, SourceError> {
        let settings = load_core(CoreFlags::default()).map_err(|e| SourceError(e.to_string()))?;
        let mut relay = Relay::connect(&settings.url.value, self.timeout)
            .map_err(|e| SourceError(format!("relay connect: {e}")))?
            .with_prefix(settings.prefix.value.clone());
        read_expired_orders(&mut relay)
    }
}

/// Reads the `AckExpired` orders of every registered lane from `relay`.
///
/// # Errors
///
/// The reason the receipts or registrations could not be read, or why the receipt history is
/// known to be incomplete.
pub fn read_expired_orders(relay: &mut Relay) -> Result<Vec<ExpiredOrder>, SourceError> {
    let fail = |what: &str, e: relay_core::Error| SourceError(format!("relay {what}: {e}"));
    let lanes: BTreeMap<String, String> = relay
        .agents()
        .map_err(|e| fail("registrations", e))?
        .registrations
        .iter()
        .filter_map(|reg| {
            Some((
                reg.name.as_str().to_owned(),
                reg.full_address()?.to_string(),
            ))
        })
        .collect();
    let now_ms = relay.lane_lists().map_err(|e| fail("clock", e))?.now_ms;
    let mut events = Vec::new();
    let mut after = None;
    for _ in 0..RECEIPT_MAX_PAGES {
        let page = relay
            .receipt_export(&ReceiptRequest {
                since_ms: now_ms - RECEIPT_WINDOW_MS,
                until_ms: now_ms,
                now_ms,
                page_size: RECEIPT_PAGE_CAP,
                after: after.take(),
            })
            .map_err(|e| fail("receipts", e))?;
        if let Some(reason) = page.reasons.iter().find(|r| {
            matches!(
                r,
                ReceiptCoverageReason::SourceUnavailable
                    | ReceiptCoverageReason::RecordingGap
                    | ReceiptCoverageReason::CursorEvicted
                    | ReceiptCoverageReason::CursorUnavailable
            )
        }) {
            return Err(SourceError(format!(
                "relay receipts incomplete: {reason:?}"
            )));
        }
        events.extend(page.events.into_iter().map(|row| row.event));
        if !page.has_more {
            return Ok(expired_orders_from(&events, &lanes));
        }
        after = page.next_cursor;
    }
    Err(SourceError(format!(
        "relay receipts exceed {RECEIPT_MAX_PAGES} pages"
    )))
}

/// The seam for herdr weak signals (title, pane text). The live command wires none yet, so only
/// hook evidence makes WORKING; live weak signals are SWM-264.
pub struct NoWeakSignals;

impl WeakSignals for NoWeakSignals {
    fn signals(&self, _addr: &str) -> Vec<WeakSource> {
        Vec::new()
    }
}
