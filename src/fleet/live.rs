// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Live readers: relay Redis through relay-core, Prometheus over its HTTP API, and the order
//! receipt and weak-signal seams.

use super::SourceError;
use super::classify::WeakSource;
use super::report::{
    ExpiredOrder, HostSeries, OrderReceipts, ProbeReading, RegInfo, RelayConfig, RelaySnapshot,
    SeriesSource, WeakSignals, parse_event,
};
use relay_core::config::{CoreFlags, load_core};
use relay_core::{ACTIVITY_MAX_EVENTS, Address, Relay};
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
    let registrations: Vec<RegInfo> = agents
        .registrations
        .iter()
        .filter_map(|reg| {
            Some(RegInfo {
                addr: reg.full_address()?.to_string(),
                last_seen_ms: reg.last_seen_ms,
                has_pane: reg.herdr_pane.is_some(),
            })
        })
        .collect();
    let mut snapshot = RelaySnapshot {
        now_ms: lists.now_ms,
        lists: lists.lists,
        unreadable_lists: lists.unreadable,
        registrations,
        ..RelaySnapshot::default()
    };
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

/// A source that reads nothing because no Prometheus URL is configured.
pub struct NoPrometheus;

impl SeriesSource for NoPrometheus {
    fn host_series(&self, _host: &str) -> Result<HostSeries, SourceError> {
        Err(SourceError(
            "no Prometheus URL configured (IX_PROMETHEUS_URL or [fleet] prometheus_url)".to_owned(),
        ))
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
        for (labels, value) in read(format!("ix_fleet_lane_blocked{{host=\"{host}\"}}"))? {
            if value >= 1.0
                && let (Some(lane), Some(reason)) = (labels.get("lane"), labels.get("reason"))
            {
                series.blocked.insert(lane.to_lowercase(), reason.clone());
            }
        }
        for (labels, value) in read(format!("ix_fleet_probe_ok{{host=\"{host}\"}}"))? {
            if let Some(source) = labels.get("source") {
                series.probes.push(ProbeReading {
                    source: source.clone(),
                    ok: value >= 1.0,
                });
            }
        }
        series.probes.sort_by(|a, b| a.source.cmp(&b.source));
        let mut config = RelayConfig::default();
        for (labels, value) in read(format!("ix_fleet_config{{host=\"{host}\"}}"))? {
            match labels.get("name").map(String::as_str) {
                Some("working_window") => config.working_window = secs(value),
                Some("max_tool_runtime") => config.max_tool_runtime = secs(value),
                Some("no_events_after") => config.no_events_after = secs(value),
                _ => {}
            }
        }
        series.config = config;
        for (labels, value) in read(format!(
            "ix_fleet_lane_compile_seconds_total{{host=\"{host}\"}}"
        ))? {
            if let Some(lane) = labels.get("lane") {
                series.compile_seconds.insert(lane.to_lowercase(), value);
            }
        }
        let ages = read(format!(
            "time() - timestamp(ix_fleet_config{{host=\"{host}\"}})"
        ))?;
        series.sample_age = ages.into_iter().filter_map(|(_, value)| secs(value)).max();
        Ok(series)
    }
}

/// The seam for the FR-035 order receipt states. ix-relay main (0ef4efc) records `Delivered` and
/// `TimedOut` lease outcomes but no `Acted` or `AckExpired` (ix-relay PR #56), so the live
/// reader reports the receipts unavailable instead of an empty list.
pub struct PendingOrderReceipts;

impl OrderReceipts for PendingOrderReceipts {
    fn expired_orders(&self) -> Result<Vec<ExpiredOrder>, SourceError> {
        Err(SourceError(
            "AckExpired receipts are not on ix-relay main (ix-relay PR #56); STALE-ORDER unavailable"
                .to_owned(),
        ))
    }
}

/// The seam for herdr weak signals; the live command wires none, so only hook evidence works.
pub struct NoWeakSignals;

impl WeakSignals for NoWeakSignals {
    fn signals(&self, _addr: &str) -> Vec<WeakSource> {
        Vec::new()
    }
}
