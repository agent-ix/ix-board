// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The live `fleet tick` run: `fleet status` evidence, the FR-072 lists through relay-core, one
//! ledger commit.

use super::{
    LanePr, ReadyWork, StandingOrders, TickFacts, TickOutput, TickSettings, TickSources, Unread,
};
use crate::fleet::ledger::{self, Ledger, TickCommit};
use crate::fleet::{
    Coverage, FleetConfig, FleetError, SeriesSource, SourceError, Sources, build_report, live,
};
use relay_core::Relay;
use relay_core::config::{CoreFlags, load_core};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// The sources a live tick reads. Only the FR-072 lists are wired; each other source reports
/// why it is unread, so the tick names the missing seam instead of emitting from an empty one.
struct LiveSources {
    timeout: Duration,
    series: Option<Arc<dyn SeriesSource>>,
    hosts: Vec<String>,
}

impl TickSources for LiveSources {
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, Unread> {
        let failed = |e: String| Unread::Failed(SourceError(e));
        let mut relay = connect(self.timeout).map_err(failed)?;
        let lists = relay
            .assignments()
            .map_err(|e| failed(format!("relay assignment lists: {e}")))?;
        Ok(lists
            .into_iter()
            .map(|list| {
                (
                    list.lane.to_lowercase(),
                    list.tickets.into_iter().map(|t| t.reference).collect(),
                )
            })
            .collect())
    }

    fn host_busy(&self) -> Result<BTreeMap<String, f64>, Unread> {
        let series = self.series.as_ref().ok_or_else(|| {
            Unread::Failed(SourceError(
                "set IX_PROMETHEUS_URL or [fleet] prometheus_url to read host busy ratios"
                    .to_owned(),
            ))
        })?;
        self.hosts
            .iter()
            .map(|host| {
                let reading = series.host_series(host).map_err(Unread::Failed)?;
                let ratio = reading.busy_ratio.ok_or_else(|| {
                    Unread::Failed(SourceError(format!(
                        "Prometheus series {} returned no busy ratio for host {host}",
                        live::series::HOST_BUSY_RATIO
                    )))
                })?;
                Ok((host.to_lowercase(), ratio))
            })
            .collect()
    }

    fn lane_prs(&self) -> Result<Vec<LanePr>, Unread> {
        Err(Unread::Unwired("SWM-282"))
    }

    fn ready_work(&self) -> Result<ReadyWork, Unread> {
        Err(Unread::Unwired("SWM-212"))
    }
}

fn connect(timeout: Duration) -> Result<Relay, String> {
    let settings = load_core(CoreFlags::default()).map_err(|e| e.to_string())?;
    Ok(Relay::connect(&settings.url.value, timeout)
        .map_err(|e| format!("relay connect: {e}"))?
        .with_prefix(settings.prefix.value.clone()))
}

/// The address of the session that owns `pane` on `machine`, from the relay registrations given as
/// `(machine, herdr pane, verified address)`.
pub fn caller_from(
    registrations: &[(String, Option<String>, Option<String>)],
    machine: &str,
    pane: &str,
) -> Option<String> {
    registrations
        .iter()
        .find(|(m, p, _)| m == machine && p.as_deref() == Some(pane))
        .and_then(|(_, _, addr)| addr.clone())
}

/// The calling session resolved through the relay: this machine and `HERDR_PANE_ID` against the
/// registrations. `None` when the session is not under herdr or is not registered.
fn resolve_caller(timeout: Duration) -> Option<String> {
    let pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|p| !p.is_empty())?;
    let machine = relay_core::machine_name().ok()?;
    let mut relay = connect(timeout).ok()?;
    let agents = relay.agents().ok()?;
    let regs: Vec<_> = agents
        .registrations
        .iter()
        .map(|r| {
            (
                r.machine.as_str().to_owned(),
                r.herdr_pane.as_ref().map(|p| p.as_str().to_owned()),
                r.full_address().map(|a| a.to_string()),
            )
        })
        .collect();
    caller_from(&regs, machine.as_str(), &pane)
}

fn host_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Runs one tick against the live relay and commits it.
///
/// # Errors
///
/// A refused configuration, a relay that cannot be read, or a ledger failure; no tick is
/// committed then.
pub fn run(file: Option<&toml::Value>, session: Option<&str>) -> Result<TickOutput, FleetError> {
    let config = FleetConfig::from_toml(file, std::env::var("IX_PROMETHEUS_URL").ok())?;
    let settings = TickSettings::from_toml(file)?;
    let series: Option<Arc<dyn SeriesSource>> = config.prometheus_url.as_ref().map(|url| {
        match live::PromHttp::new(url, config.host_timeout) {
            Ok(query) => Arc::new(live::PrometheusSeries { query }) as Arc<dyn SeriesSource>,
            Err(error) => Arc::new(live::UnavailablePrometheus(error.to_string())),
        }
    });
    let (relay, receipts) = live::read_relay(config.host_timeout);
    let sources = Sources {
        relay,
        series: series.clone(),
        receipts: &receipts,
        weak: &live::NoWeakSignals,
    };
    let mut report = build_report(&config, &sources, host_now_ms());
    if report.relay_unavailable || report.coverage() == Coverage::Failed {
        let why = report
            .defects
            .first()
            .map_or_else(|| "no lane could be read".to_owned(), |d| d.reason.clone());
        return Err(FleetError::Tick(why));
    }
    let facts = TickFacts::read(&LiveSources {
        timeout: config.host_timeout,
        series: series.clone(),
        hosts: report.hosts.iter().map(|host| host.host.clone()).collect(),
    });
    let orders = StandingOrders::load(settings.standing_orders_path.as_deref());
    let mut bus = connect(config.host_timeout).map_err(FleetError::Tick)?;
    let resolved;
    let session = match session {
        Some(given) => Some(given),
        None => {
            resolved = resolve_caller(config.host_timeout);
            resolved.as_deref()
        }
    };
    commit_completing(
        &config,
        &mut report,
        &TickInputs {
            settings: &settings,
            facts: &facts,
            orders: &orders,
            session,
        },
        &mut bus,
    )
}

/// The FR-037 focus ticket of each WORKING lane that has an open `push_next_ticket` condition, read
/// from the lane's hook events. A lane whose events cannot be read has no focus, so no
/// `executed_at`.
fn pushed_lane_focus(
    config: &FleetConfig,
    report: &crate::fleet::FleetReport,
    ledger: &Ledger,
    facts: &TickFacts,
    bus: &mut impl crate::fleet::assign::AssignBus,
) -> Result<BTreeMap<String, String>, FleetError> {
    let open = ledger
        .open_push_lanes()
        .map_err(|e| FleetError::Ledger(e.to_string()))?;
    let window_ms = i64::try_from(config.focus_window.as_millis()).unwrap_or(i64::MAX);
    let mut focus = BTreeMap::new();
    for row in &report.rows {
        let key = row.lane.to_lowercase();
        if row.class != crate::fleet::ClassKind::Working || !open.contains(&key) {
            continue;
        }
        let tickets: Vec<String> = facts
            .assignments
            .as_ref()
            .and_then(|a| a.get(&key))
            .cloned()
            .unwrap_or_default();
        let Ok(events) = bus.lane_events(&row.lane) else {
            continue;
        };
        let keys = crate::fleet::assign::focus_keys(&config.team_keys, &tickets);
        let derived = crate::fleet::focus::derive(&events, report.run_ms, window_ms, None, &keys);
        if let Some(ticket) = derived.ticket() {
            focus.insert(key, ticket.to_owned());
        }
    }
    Ok(focus)
}

/// What a tick commits besides its report.
pub struct TickInputs<'a> {
    /// Tick settings.
    pub settings: &'a TickSettings,
    /// Facts from the other sources.
    pub facts: &'a TickFacts,
    /// Standing-order file state.
    pub orders: &'a StandingOrders,
    /// The session that ran the tick.
    pub session: Option<&'a str>,
}

/// Completes the pending assignment rows through the shared `fleet status` step, then commits the
/// tick, so a fleet that runs only ticks does not leave killed `fleet assign` rows `pending`.
///
/// # Errors
///
/// A ledger failure; no tick is committed then.
pub fn commit_completing(
    config: &FleetConfig,
    report: &mut crate::fleet::FleetReport,
    inputs: &TickInputs<'_>,
    bus: &mut impl crate::fleet::assign::AssignBus,
) -> Result<TickOutput, FleetError> {
    ledger::complete_assignments(config, report, bus);
    let path = ledger::resolve_path(config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let mut ledger = Ledger::open(&path, config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let focus = pushed_lane_focus(config, report, &ledger, inputs.facts, bus)?;
    ledger
        .commit_tick(&TickCommit {
            report,
            settings: inputs.settings,
            facts: inputs.facts,
            orders: inputs.orders,
            session: inputs.session,
            focus: &focus,
        })
        .map_err(|e| FleetError::Ledger(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::{HostSeries, SourceError};
    use ix_trace_rs::trace;

    struct BusySeries;

    impl SeriesSource for BusySeries {
        fn host_series(&self, host: &str) -> Result<HostSeries, SourceError> {
            let busy_ratio = match host {
                "draco" => 0.72,
                "titan" => 0.91,
                _ => return Err(SourceError(format!("unexpected host {host}"))),
            };
            Ok(HostSeries {
                busy_ratio: Some(busy_ratio),
                ..HostSeries::default()
            })
        }
    }

    #[trace("FR-038-AC-6")]
    #[test]
    fn live_tick_reads_busy_ratio_from_prometheus_for_each_host() {
        let sources = LiveSources {
            timeout: Duration::from_secs(1),
            series: Some(Arc::new(BusySeries)),
            hosts: vec!["draco".to_owned(), "titan".to_owned()],
        };

        assert_eq!(
            sources.host_busy().unwrap(),
            BTreeMap::from([("draco".to_owned(), 0.72), ("titan".to_owned(), 0.91)])
        );
    }
}
