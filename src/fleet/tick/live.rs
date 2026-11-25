// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The live `fleet tick` run: `fleet status` evidence, the FR-072 lists through relay-core, one
//! ledger commit.

use super::{
    LanePr, ReadyWork, StandingOrders, TickFacts, TickOutput, TickSettings, TickSources, Unread,
};
use crate::fleet::ledger::{self, Ledger, TickCommit};
use crate::fleet::{
    Coverage, FleetConfig, FleetError, FleetReport, SeriesSource, SourceError, Sources,
    build_report, live,
};
use ix_trace_rs::implements;
use relay_core::Relay;
use relay_core::config::{CoreFlags, load_core};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// The sources a live tick reads. The busy map comes from the same bounded host-series reads as
/// `fleet status`; each other unwired source reports why it is unread.
struct LiveSources {
    timeout: Duration,
    config: FleetConfig,
    settings: super::TickSettings,
    host_busy: BTreeMap<String, f64>,
    lanes: Vec<(String, String)>,
    host_lane_counts: BTreeMap<String, u32>,
}

fn live_sources(
    report: &FleetReport,
    timeout: Duration,
    config: &FleetConfig,
    settings: &super::TickSettings,
) -> LiveSources {
    LiveSources {
        timeout,
        config: config.clone(),
        settings: settings.clone(),
        host_busy: report.host_busy.clone(),
        lanes: report
            .rows
            .iter()
            .map(|row| (row.host.to_lowercase(), row.lane.to_lowercase()))
            .collect(),
        host_lane_counts: report
            .hosts
            .iter()
            .map(|host| {
                (
                    host.host.to_lowercase(),
                    u32::try_from(host.lanes).unwrap_or(u32::MAX),
                )
            })
            .collect(),
    }
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
        Ok(self.host_busy.clone())
    }

    #[implements("FR-038-AC-5")]
    fn lane_prs(&self) -> Result<Vec<LanePr>, Unread> {
        super::pr::read_live(self.timeout).map_err(|error| Unread::Failed(SourceError(error)))
    }

    fn ready_work(&self) -> Result<ReadyWork, Unread> {
        Err(Unread::Unwired("SWM-212"))
    }

    #[implements("FR-041-AC-5")]
    fn need_ready_tickets(&self) -> Result<Vec<super::NeedReadyTickets>, Unread> {
        if self.config.saturation_projects.is_empty() {
            return Err(Unread::Failed(SourceError(
                "fleet.saturation_projects is empty".to_owned(),
            )));
        }
        let report = crate::fleet::saturation::read_live(&self.config, &self.settings)
            .map_err(|error| Unread::Failed(SourceError(error.to_string())))?;
        if !report.complete {
            return Err(Unread::Failed(SourceError(
                "fleet saturation source coverage is incomplete".to_owned(),
            )));
        }
        let mut needs = Vec::new();
        for project in report
            .projects
            .into_iter()
            .filter(|project| project.need > 0)
        {
            let Some(planner) = project.planner else {
                return Err(Unread::Failed(SourceError(format!(
                    "no planner configured for project {} with ready-buffer deficit {}",
                    project.project, project.need
                ))));
            };
            needs.push(super::NeedReadyTickets {
                project: project.project,
                planner,
                need: project.need,
                ready: project.ready,
                lanes: project.lanes,
            });
        }
        Ok(needs)
    }

    #[implements("FR-041-AC-16")]
    fn capacity_throttle_samples(
        &self,
        at_ms: i64,
        interval_secs: u64,
    ) -> Vec<super::CapacityThrottleSample> {
        let prometheus = self
            .config
            .prometheus_url
            .as_deref()
            .map(|url| live::PromHttp::new(url, self.timeout));
        self.config
            .hosts
            .iter()
            .map(|host| {
                let evidence = match prometheus.as_ref() {
                    Some(Ok(query)) => crate::fleet::saturation::live_admission_blocked_share(
                        query,
                        host,
                        interval_secs,
                        self.config.push_interval.as_secs(),
                    ),
                    Some(Err(error)) => crate::fleet::saturation::Evidence::unavailable(
                        "prometheus.ix_build_admission_blocked_seconds_total",
                        error.to_string(),
                        None,
                    ),
                    None => crate::fleet::saturation::Evidence::unavailable(
                        "prometheus.ix_build_admission_blocked_seconds_total",
                        "prometheus_url_unconfigured",
                        None,
                    ),
                };
                super::CapacityThrottleSample {
                    host: host.to_lowercase(),
                    at_ms,
                    interval_secs,
                    share: evidence.value,
                    coverage: evidence.coverage,
                    unavailable_reason: evidence.reason,
                }
            })
            .collect()
    }

    #[implements("FR-041-AC-15")]
    fn drain_done(&self) -> Result<Vec<super::DrainDoneSample>, Unread> {
        let path = ledger::resolve_path(&self.config)
            .map_err(|error| Unread::Failed(SourceError(error.to_string())))?;
        let history = ledger::capacity::status(&path)
            .map_err(|error| Unread::Failed(SourceError(error.to_string())))?;
        let open_drains = history
            .hosts
            .iter()
            .flat_map(|host| {
                host.draining_lanes
                    .iter()
                    .map(|lane| (host.host.to_lowercase(), lane.to_lowercase()))
            })
            .filter(|(host, lane)| {
                self.lanes
                    .iter()
                    .any(|current| current == &(host.clone(), lane.clone()))
            })
            .collect::<Vec<_>>();
        if open_drains.is_empty() {
            return Ok(Vec::new());
        }
        let Some(url) = self.config.prometheus_url.as_deref() else {
            return Err(Unread::Failed(SourceError(
                "prometheus_url_unconfigured_for_drain_evidence".to_owned(),
            )));
        };
        let query = live::PromHttp::new(url, self.timeout)
            .map_err(|error| Unread::Failed(SourceError(error.to_string())))?;
        let mut completed = Vec::new();
        for (host, lane) in open_drains {
            let evidence = crate::fleet::saturation::live_drain_done(
                &query,
                &host,
                &lane,
                self.config.push_interval.as_secs(),
                self.settings.drain_build_window.as_secs(),
            );
            match evidence.value {
                Some(true) => completed.push(super::DrainDoneSample {
                    host,
                    lane,
                    source_basis: format!(
                        "ix_fleet_lane_state_ratio;increase(ix_fleet_lane_compile_seconds_total[{}s])",
                        self.settings.drain_build_window.as_secs()
                    ),
                }),
                Some(false) => {}
                None => {
                    return Err(Unread::Failed(SourceError(format!(
                        "drain evidence unavailable for {host}/{lane}: {}",
                        evidence.reason.unwrap_or_else(|| "unknown".to_owned())
                    ))));
                }
            }
        }
        Ok(completed)
    }

    #[implements("FR-041-AC-21")]
    fn host_output_samples(&self, at_ms: i64) -> Vec<super::HostOutputSample> {
        use crate::fleet::capacity::{LiveMeasurementSource, MeasurementSource, WindowRequest};
        let prometheus = self
            .config
            .prometheus_url
            .as_deref()
            .and_then(|url| live::PromHttp::new(url, self.timeout).ok());
        self.config
            .hosts
            .iter()
            .map(|host| {
                let source = LiveMeasurementSource {
                    config: &self.config,
                    prometheus: prometheus
                        .as_ref()
                        .map(|query| query as &dyn live::PromQuery),
                };
                let start_ms = at_ms.saturating_sub(3_600_000);
                let request = WindowRequest {
                    host: host.to_lowercase(),
                    lanes: self.host_lane_counts.get(&host.to_lowercase()).copied().unwrap_or(0),
                    start_ms,
                    end_ms: at_ms,
                    rampup_end_ms: start_ms,
                };
                let measured = source.measure(&request).ok();
                let basis = measured.as_ref().map_or_else(
                    || {
                        serde_json::json!({
                            "tickets_closed": {"availability":"unavailable", "reason":"measurement_failed"},
                            "prs_merged": {"availability":"unavailable", "reason":"measurement_failed"},
                            "commits_pushed": {"availability":"unavailable", "reason":"measurement_failed"}
                        })
                    },
                    |measurement| {
                        serde_json::json!({
                            "tickets_closed": measurement.tickets_closed,
                            "prs_merged": measurement.prs_merged,
                            "commits_pushed": measurement.commits_pushed
                        })
                    },
                );
                super::HostOutputSample {
                    host: host.to_lowercase(),
                    at_ms,
                    lanes: self.host_lane_counts.get(&host.to_lowercase()).copied(),
                    tickets_closed: measured.as_ref().and_then(|m| m.tickets_closed.value),
                    prs_merged: measured.as_ref().and_then(|m| m.prs_merged.value),
                    commits_pushed: measured.as_ref().and_then(|m| m.commits_pushed.value),
                    source_basis: basis.to_string(),
                }
            })
            .collect()
    }

    #[implements("FR-041-AC-16")]
    fn overload_events(&self, at_ms: i64) -> Vec<super::OverloadEventSample> {
        use crate::fleet::capacity::{LiveMeasurementSource, MeasurementSource, WindowRequest};
        let prometheus = self
            .config
            .prometheus_url
            .as_deref()
            .and_then(|url| live::PromHttp::new(url, self.timeout).ok());
        let window_secs = 300_i64;
        let start_ms = at_ms.saturating_sub(window_secs * 1000);
        let source = LiveMeasurementSource {
            config: &self.config,
            prometheus: prometheus
                .as_ref()
                .map(|query| query as &dyn live::PromQuery),
        };
        let mut events = Vec::new();
        for host in &self.config.hosts {
            let request = WindowRequest {
                host: host.to_lowercase(),
                lanes: self
                    .host_lane_counts
                    .get(&host.to_lowercase())
                    .copied()
                    .unwrap_or(0),
                start_ms,
                end_ms: at_ms,
                rampup_end_ms: start_ms,
            };
            let Ok(measurement) = source.measure(&request) else {
                continue;
            };
            if measurement
                .memory_headroom_min
                .value
                .is_some_and(|headroom| headroom < 0.0)
            {
                events.push(super::OverloadEventSample {
                    host: host.to_lowercase(),
                    cause: "memory_below_reserve".to_owned(),
                    source_basis: format!(
                        "{};fleet.capacity_limits.{}.memory_reserve_bytes",
                        measurement.memory_headroom_min.source,
                        host.to_lowercase()
                    ),
                });
            }
            if measurement
                .thermal_headroom_min
                .value
                .is_some_and(|headroom| headroom < 0.0)
            {
                events.push(super::OverloadEventSample {
                    host: host.to_lowercase(),
                    cause: "thermal_abort".to_owned(),
                    source_basis: format!(
                        "{};fleet.capacity_limits.{}.thermal_limit_celsius",
                        measurement.thermal_headroom_min.source,
                        host.to_lowercase()
                    ),
                });
            }
        }
        events
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
#[implements("FR-038-AC-24")]
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
    let facts = TickFacts::read(
        &live_sources(&report, config.host_timeout, &config, &settings),
        report.run_ms,
        config.tick_interval.as_secs(),
    );
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
#[path = "live_tests.rs"]
mod tests;
