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
        Err(Unread::Unwired("SWM-281"))
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
        series,
        receipts: &receipts,
        weak: &live::NoWeakSignals,
    };
    let report = build_report(&config, &sources, host_now_ms());
    if report.relay_unavailable || report.coverage() == Coverage::Failed {
        let why = report
            .defects
            .first()
            .map_or_else(|| "no lane could be read".to_owned(), |d| d.reason.clone());
        return Err(FleetError::Tick(why));
    }
    let facts = TickFacts::read(&LiveSources {
        timeout: config.host_timeout,
    });
    let orders = StandingOrders::load(settings.standing_orders_path.as_deref());
    let resolved;
    let session = match session {
        Some(given) => Some(given),
        None => {
            resolved = resolve_caller(config.host_timeout);
            resolved.as_deref()
        }
    };
    let path = ledger::resolve_path(&config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    let mut ledger = Ledger::open(&path, &config).map_err(|e| FleetError::Ledger(e.to_string()))?;
    ledger
        .commit_tick(&TickCommit {
            report: &report,
            settings: &settings,
            facts: &facts,
            orders: &orders,
            session,
        })
        .map_err(|e| FleetError::Ledger(e.to_string()))
}
