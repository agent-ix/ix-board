// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use super::super::LanePrRead;
use super::*;
use crate::fleet::live::{NoWeakSignals, PromQuery, PrometheusSeries, ReadReceipts};
use crate::fleet::report::{RegInfo, Sources};
use crate::fleet::tick::plan::{Capacity, PlanInput, plan};
use crate::fleet::{RelaySnapshot, SourceError};
use ix_trace_rs::trace;
use relay_core::fleet::HostLaneList;
use std::collections::BTreeSet;

struct CannedProm;

impl PromQuery for CannedProm {
    fn query(&self, expr: &str) -> Result<String, SourceError> {
        let result = if expr.starts_with("ix_fleet_host_busy_ratio{host=\"draco\"}") {
            r#"[{"metric":{"host":"draco"},"value":[1,"0.2"]}]"#
        } else if expr.starts_with("time() - timestamp(ix_fleet_host_busy_ratio") {
            r#"[{"metric":{"host":"draco"},"value":[1,"1"]}]"#
        } else {
            "[]"
        };
        Ok(format!(
            r#"{{"status":"success","data":{{"resultType":"vector","result":{result}}}}}"#
        ))
    }
}

struct TickSourcesWithReady<'a>(&'a LiveSources);

impl TickSources for TickSourcesWithReady<'_> {
    fn assignments(&self) -> Result<BTreeMap<String, Vec<String>>, Unread> {
        Ok(BTreeMap::new())
    }

    fn host_busy(&self) -> Result<BTreeMap<String, f64>, Unread> {
        self.0.host_busy()
    }

    fn lane_prs(&self) -> Result<LanePrRead, Unread> {
        Err(Unread::Unwired("test"))
    }

    fn ready_work(&self) -> Result<ReadyWork, Unread> {
        Ok(ReadyWork {
            unassigned: 1,
            lanes: BTreeSet::new(),
        })
    }
}

#[test]
fn live_ready_buffer_requires_configured_projects() {
    let sources = LiveSources {
        timeout: Duration::from_secs(1),
        config: FleetConfig::default(),
        settings: TickSettings::default(),
        host_busy: BTreeMap::new(),
        lanes: Vec::new(),
        host_lane_counts: BTreeMap::new(),
    };
    let error = sources.need_ready_tickets().unwrap_err();
    assert!(matches!(error, Unread::Failed(_)));
}

#[trace("FR-038-AC-6", "FR-038-AC-23")]
#[test]
fn live_busy_ratio_read_emits_add_lane_without_discarding_other_hosts() {
    let prometheus: Arc<dyn SeriesSource> = Arc::new(PrometheusSeries { query: CannedProm });
    let receipts = ReadReceipts(Ok(crate::fleet::ReceiptRead::default()));
    let weak = NoWeakSignals;
    let config = FleetConfig {
        push_interval: Duration::from_secs(30),
        ..FleetConfig::default()
    };
    let report = build_report(
        &config,
        &Sources {
            relay: Ok(RelaySnapshot {
                now_ms: 1_000,
                lists: vec![
                    HostLaneList {
                        machine: "draco".parse().unwrap(),
                        refreshed_ms: 1_000,
                        lanes: vec!["draco:w:1".to_owned()],
                    },
                    HostLaneList {
                        machine: "luna".parse().unwrap(),
                        refreshed_ms: 1_000,
                        lanes: vec!["luna:w:1".to_owned()],
                    },
                ],
                registrations: ["draco:w:1", "luna:w:1"]
                    .into_iter()
                    .map(|addr| RegInfo {
                        addr: addr.to_owned(),
                        queue: addr.to_owned(),
                        last_seen_ms: 1_000,
                        has_pane: true,
                    })
                    .collect(),
                ..RelaySnapshot::default()
            }),
            series: Some(prometheus),
            receipts: &receipts,
            weak: &weak,
        },
        1_000,
    );
    let mut settings = TickSettings::default();
    settings.lane_capacity.insert("draco".to_owned(), 2);
    settings.lane_capacity.insert("luna".to_owned(), 2);
    let facts = TickFacts::read(
        &TickSourcesWithReady(&live_sources(
            &report,
            Duration::from_secs(1),
            &config,
            &settings,
        )),
        report.run_ms,
        config.tick_interval.as_secs(),
    );
    let capacity = Capacity {
        open_runs: BTreeSet::new(),
        levels: BTreeMap::from([("draco".to_owned(), 2)]),
        cool_off_until: BTreeMap::new(),
        draining: BTreeMap::new(),
    };
    let plan = plan(&PlanInput {
        report: &report,
        first_seen: &[],
        settings: &settings,
        interval_ms: 300_000,
        facts: &facts,
        capacity: &capacity,
        orders: &StandingOrders::Missing,
        open: &[],
    });

    assert_eq!(
        facts.host_busy,
        Some(BTreeMap::from([("draco".to_owned(), 0.2)]))
    );
    let add_lanes: Vec<_> = plan
        .actions
        .iter()
        .filter(|action| action.action.kind == super::super::ActionKind::AddLane)
        .collect();
    assert_eq!(add_lanes.len(), 1);
    assert_eq!(add_lanes[0].action.subject, "draco");
}

#[trace("FR-041-AC-16")]
#[test]
fn live_tick_hosts_come_from_registry_lanes_without_fleet_hosts() {
    let mut sources = LiveSources {
        timeout: Duration::from_secs(1),
        config: FleetConfig::default(),
        settings: TickSettings::default(),
        host_busy: BTreeMap::new(),
        lanes: Vec::new(),
        host_lane_counts: BTreeMap::from([("draco".to_owned(), 2), ("terra".to_owned(), 1)]),
    };
    assert!(sources.config.hosts.is_empty());
    assert_eq!(sources.hosts(), ["draco", "terra"]);

    sources.config.hosts = vec!["Titan".to_owned()];
    assert_eq!(sources.hosts(), ["draco", "terra", "titan"]);
}
