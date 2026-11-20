// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic operational fixture oracles with explicit source completeness.

use super::*;
use crate::retro::department::{Parameters, Selector};
use ix_trace_rs::trace;

const END: f64 = 1_800_000_000.0;
const LANE: &str = "host:team:code";
fn at(seconds: f64) -> String {
    crate::drift::format_timestamp(END + seconds)
}
fn config() -> Config {
    Config {
        version: 1,
        policy_id: "ops-policy".into(),
        scope: Scope {
            teams: vec![Selector {
                id: "independent-team".into(),
                label: "Team".into(),
            }],
            hosts: vec![Selector {
                id: "host".into(),
                label: "Host".into(),
            }],
            lanes: vec![Selector {
                id: LANE.into(),
                label: "Code".into(),
            }],
            ..Default::default()
        },
        freshness_seconds: [("fixture".into(), 60), ("scheduler:host".into(), 60)].into(),
        clock_tolerance_seconds: 2,
        parameters: Parameters {
            loop_repeat_count: Some(2),
            loop_lookback_seconds: Some(3600),
            ..Default::default()
        },
        operations: None,
        policies: Vec::new(),
        classes: Vec::new(),
    }
}
fn operations() -> OperationalConfig {
    OperationalConfig {
        disk_reserve_bytes: [("host".into(), 1000)].into(),
        filesystems: [("host".into(), "fs-native-id".into())].into(),
        ..Default::default()
    }
}
fn row(id: &str, entity: &str, time: f64, mut fact: OperationFact) -> Observation {
    if let OperationFact::Order(order) = &mut fact {
        order.send_id = id.into();
    }
    Observation {
        event_id: id.into(),
        entity_id: entity.into(),
        event_at: Some(at(time)),
        observed_at: Some(at(0.0)),
        sequence: Some(1),
        scope: config().scope,
        complete: true,
        source_ref: format!("fixture:{id}"),
        attribution: Some(Attribution {
            actor: "audit-tool".into(),
            classifier_version: "audit-v1".into(),
            offset_start: 0,
            offset_end: 10,
        }),
        fact: Fact::Operation(fact),
    }
}
fn batch(rows: Vec<Observation>) -> Evidence {
    Evidence {
        source: "fixture".into(),
        observed_at: Some(at(0.0)),
        complete: OPERATIONAL_FAMILIES.iter().map(|s| (*s).into()).collect(),
        reasons: BTreeMap::new(),
        observations: rows,
    }
}
fn collect(config: &Config, evidence: &[Evidence]) -> (BTreeMap<String, Measurement>, LoopReport) {
    collect_operations(
        config,
        &operations(),
        Frame {
            now: END,
            window_secs: 3600,
            end_exclusive: false,
        },
        evidence,
    )
}
fn read_files(operations: &OperationalConfig) -> Vec<Evidence> {
    read_operational_files(
        operations,
        &config(),
        Frame {
            now: END,
            window_secs: 3600,
            end_exclusive: false,
        },
    )
}
fn measure(rows: Vec<Observation>) -> BTreeMap<String, Measurement> {
    collect(&config(), &[batch(rows)]).0
}
fn host() -> OperationFact {
    OperationFact::Host(HostFact {
        host: "host".into(),
        capacity_units: Some(3),
        used_units: Some(4),
        load_one: Some(6.0),
        disk_available_bytes: Some(800),
        filesystem: Some("fs-native-id".into()),
        source_error: None,
        sample_observed_at: Some(at(0.0)),
    })
}
fn scalar(metrics: &BTreeMap<String, Measurement>, id: &str) -> f64 {
    match metrics[id].value.as_ref().expect(id) {
        Value::Scalar(n) => *n,
        other => panic!("{id}: {other:?}"),
    }
}
fn keyed(metrics: &BTreeMap<String, Measurement>, id: &str, key: &str) -> f64 {
    match metrics[id].value.as_ref().expect(id) {
        Value::Keyed(rows) => rows[key],
        other => panic!("{id}: {other:?}"),
    }
}
fn duration(metrics: &BTreeMap<String, Measurement>, id: &str, key: &str) -> Distribution {
    match metrics[id].value.as_ref().expect(id) {
        Value::Durations(rows) => rows[key].clone(),
        other => panic!("{id}: {other:?}"),
    }
}
fn request(
    phase: RequestPhase,
    enqueue: f64,
    start: Option<f64>,
    end: Option<f64>,
) -> OperationFact {
    OperationFact::Request(RequestFact {
        host: "host".into(),
        lane: LANE.into(),
        ticket: "native-ticket".into(),
        units: 1,
        capacity_units: 3,
        priority: 0,
        suspect: false,
        reason: None,
        exit_code: None,
        signal: None,
        phase,
        enqueued_at: at(enqueue),
        started_at: start.map(at),
        ended_at: end.map(at),
        wait_seconds: start.map_or(-enqueue, |s| s - enqueue),
        hold_seconds: start.map(|s| end.unwrap_or(0.0) - s),
    })
}
fn relay(id: &str, transition: RelayKind) -> OperationFact {
    OperationFact::Relay(RelayFact {
        message_id: id.into(),
        lane: LANE.into(),
        transition,
        order: true,
    })
}
fn order(id: &str, previous: Option<&str>, direction: &str, revision: &str) -> OperationFact {
    OperationFact::Order(OrderFact {
        send_id: id.into(),
        order_id: id.into(),
        lane: LANE.into(),
        action_key: Some("build-admission".into()),
        direction: Some(direction.into()),
        reverses_order_id: previous.map(str::to_owned),
        supersedes_order_id: previous.map(str::to_owned),
        evidence_revision: Some(revision.into()),
    })
}

fn exported_report(
    c: &Config,
    export: &OperationalExport,
    families: &[&str],
) -> crate::retro::department::Report {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("export.json");
    std::fs::write(&path, serde_json::to_vec(export).unwrap()).unwrap();
    let mut op = operations();
    op.evidence_files.push(OperationalInput {
        path,
        source: "fixture".into(),
        families: families.iter().map(|f| (*f).into()).collect(),
    });
    let mut c = c.clone();
    c.operations = Some(op.clone());
    let frame = Frame {
        now: END,
        window_secs: 3600,
        end_exclusive: false,
    };
    crate::retro::department::collect(
        &c,
        frame,
        &read_operational_files(&op, &c, frame),
        &BTreeMap::new(),
    )
}
fn coverage(family: &str, entities: &[&str]) -> OperationalCoverage {
    OperationalCoverage {
        family: family.into(),
        entities: entities.iter().map(|s| (*s).into()).collect(),
        from: at(-7200.0),
        until: at(0.0),
        complete: true,
        full_history: true,
    }
}

#[trace("FR-029-AC-2", "FR-029-AC-8")]
#[test]
fn review_round1_redelivery_event_names_never_replace_original_order_context() {
    let original = row("z-original", "o1", -300.0, order("o1", None, "go", "e1"));
    let second = row(
        "b-second",
        "o2",
        -200.0,
        order("o2", Some("o1"), "stop", "e1"),
    );
    let third = row("c-third", "o3", -100.0, order("o3", Some("o2"), "go", "e1"));
    let expected = collect(
        &config(),
        &[batch(vec![original.clone(), second.clone(), third.clone()])],
    )
    .1;
    assert_eq!(expected.findings.len(), 1);
    let mut redelivery = original.clone();
    redelivery.event_id = "a-redelivery".into();
    redelivery.event_at = Some(at(-20.0));
    redelivery.sequence = Some(99);
    let rows = vec![
        redelivery.clone(),
        third.clone(),
        original.clone(),
        second.clone(),
    ];
    assert_eq!(collect(&config(), &[batch(rows.clone())]).1, expected);
    assert_eq!(
        collect(&config(), &[batch(rows.into_iter().rev().collect())]).1,
        expected
    );
    redelivery.attribution.as_mut().unwrap().actor = "conflicting-order-actor".into();
    assert_eq!(
        collect(
            &config(),
            &[batch(vec![original, second, third, redelivery])]
        )
        .1
        .status,
        Status::Unavailable
    );
}

#[trace("FR-029-AC-8")]
#[test]
fn review_round1_flip_lookback_retains_predecessor_and_counts_only_inside_reversals() {
    let rows = vec![
        row("old", "o1", -4000.0, order("o1", None, "go", "e1")),
        row(
            "first-reversal",
            "o2",
            -3000.0,
            order("o2", Some("o1"), "stop", "e1"),
        ),
        row(
            "second-reversal",
            "o3",
            -2000.0,
            order("o3", Some("o2"), "go", "e1"),
        ),
    ];
    let report = collect(&config(), &[batch(rows.clone())]).1;
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].event_ids,
        ["old", "first-reversal", "second-reversal"]
    );
    assert_eq!(report.findings[0].start, at(-4000.0));
    let mut boundary = rows;
    boundary[1].event_at = Some(at(-3600.0));
    assert!(
        collect(&config(), &[batch(boundary)]).1.findings.is_empty(),
        "reversal at the open boundary is context, not a counted event"
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-4")]
#[test]
fn review_round1_later_attributed_message_labels_merge_without_changing_send_identity() {
    let mut raw = row(
        "a-raw",
        "message",
        -100.0,
        OperationFact::Message(MessageFact {
            message_id: "message".into(),
            sent_at: at(-100.0),
            sender: LANE.into(),
            correction: None,
            status_only: None,
        }),
    );
    raw.attribution = None;
    let mut classified = raw.clone();
    classified.event_id = "z-classified".into();
    classified.event_at = Some(at(-20.0));
    classified.attribution = row("a", "b", 0.0, host()).attribution;
    if let Fact::Operation(OperationFact::Message(f)) = &mut classified.fact {
        f.correction = Some(true);
        f.status_only = Some(false);
    }
    let metrics = measure(vec![raw.clone(), classified.clone()]);
    assert_eq!(scalar(&metrics, "correction_rate"), 1.0);
    assert_eq!(keyed(&metrics, "messages_per_sender", LANE), 1.0);
    assert_eq!(scalar(&metrics, "status_only_messages"), 0.0);
    assert_eq!(
        metrics["correction_rate"].classification_coverage,
        Some(ClassificationCoverage {
            classified: 1,
            total: 1,
            basis: CoverageBasis::CompletePopulation
        })
    );
    assert_eq!(metrics, measure(vec![classified.clone(), raw.clone()]));
    let mut conflict = classified.clone();
    conflict.event_id = "conflict".into();
    if let Fact::Operation(OperationFact::Message(f)) = &mut conflict.fact {
        f.correction = Some(false);
    }
    assert_eq!(
        measure(vec![raw.clone(), classified.clone(), conflict])["correction_rate"].value,
        None
    );
    if let Fact::Operation(OperationFact::Message(f)) = &mut classified.fact {
        f.sent_at = at(-99.0);
    }
    assert_eq!(
        measure(vec![raw, classified])["messages_per_sender"].value,
        None,
        "classification must not replace original send time"
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn review_round1_gauge_watermarks_follow_source_observation_and_configured_freshness() {
    let mut observation = row("host", "host", -10.0, host());
    observation.observed_at = Some(at(-10.0));
    let mut evidence = batch(vec![observation]);
    evidence.observed_at = Some(at(-10.0));
    let mut cov = coverage("host", &["host"]);
    cov.until = at(-10.0);
    let mut export = OperationalExport {
        version: 1,
        evidence,
        coverage: vec![cov],
    };
    let report = exported_report(&config(), &export, &["host"]);
    assert_eq!(keyed(&report.measurements, "host_slots_used", "host"), 4.0);
    assert_eq!(
        report.measurements["host_slots_used"].source_age_seconds,
        Some(10.0)
    );
    export.evidence.observed_at = Some(at(-61.0));
    export.evidence.observations[0].observed_at = Some(at(-61.0));
    export.evidence.observations[0].event_at = Some(at(-61.0));
    export.coverage[0].until = at(-61.0);
    assert_eq!(
        exported_report(&config(), &export, &["host"]).measurements["host_slots_used"].value,
        None
    );
    let mut event_export = OperationalExport {
        version: 1,
        evidence: batch(Vec::new()),
        coverage: vec![coverage("message", &[LANE])],
    };
    event_export.evidence.observed_at = Some(at(-10.0));
    event_export.coverage[0].until = at(-10.0);
    assert_eq!(
        exported_report(&config(), &event_export, &["message"]).measurements["messages_per_sender"]
            .value,
        None,
        "event populations still require the complete window end"
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn review_round1_malformed_family_keeps_independent_valid_facts_and_source_reasons() {
    let mut instruction = row(
        "instruction",
        LANE,
        0.0,
        OperationFact::Instruction(InstructionFact {
            lane: LANE.into(),
            component: "coder".into(),
            claimed: Some("v1".into()),
            actual: Some("v2".into()),
        }),
    );
    instruction.event_at = Some("malformed-timestamp".into());
    let mut export = OperationalExport {
        version: 1,
        evidence: batch(vec![row("host", "host", 0.0, host()), instruction]),
        coverage: vec![
            coverage("host", &["host"]),
            coverage("instruction", &[LANE]),
        ],
    };
    let report = exported_report(&config(), &export, &["host", "instruction"]);
    assert_eq!(keyed(&report.measurements, "host_slots_used", "host"), 4.0);
    assert_eq!(
        report.measurements["instruction_version_mismatches"].value,
        None
    );
    assert!(
        report.measurements["instruction_version_mismatches"]
            .reason
            .is_some()
    );
    assert!(report.observations.iter().any(|r| r.event_id == "host"));
    export.evidence.observations.pop();
    export.coverage[1].until = "bad-watermark".into();
    assert_eq!(
        keyed(
            &exported_report(&config(), &export, &["host", "instruction"]).measurements,
            "host_slots_used",
            "host"
        ),
        4.0
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn review_round1_decision_waiters_have_nonzero_scoped_deduplicated_native_oracles() {
    let decision = |open, classification, ruling: &[&str], spec: &[&str]| {
        OperationFact::Decision(DecisionFact {
            open,
            decision: classification,
            opened_at: Some(at(-7200.0)),
            ruling_waiters: ruling.iter().map(|s| (*s).into()).collect(),
            spec_waiters: spec.iter().map(|s| (*s).into()).collect(),
        })
    };
    let mut closed = row(
        "closed",
        "closed",
        0.0,
        decision(false, None, &["closed-ruling"], &["closed-spec"]),
    );
    closed.attribution = None;
    let mut outside = row(
        "outside",
        "outside",
        0.0,
        decision(true, Some(true), &["outside-ruling"], &["outside-spec"]),
    );
    outside.scope.teams[0].id = "other-team".into();
    outside.scope.lanes.clear();
    let a = row(
        "owner-a",
        "a",
        0.0,
        decision(
            true,
            Some(true),
            &["wait-a", "wait-b"],
            &["wait-b", "wait-c"],
        ),
    );
    let mut export = OperationalExport {
        version: 1,
        evidence: batch(vec![
            a.clone(),
            a,
            row(
                "owner-b",
                "b",
                0.0,
                decision(
                    true,
                    Some(true),
                    &["wait-b", "wait-c"],
                    &["wait-c", "wait-d"],
                ),
            ),
            row(
                "spec-owner",
                "spec",
                0.0,
                decision(true, Some(false), &["not-a-ruling"], &["wait-d"]),
            ),
            closed,
            outside,
        ]),
        coverage: vec![coverage("decision", &["independent-team"])],
    };
    let report = exported_report(&config(), &export, &["decision"]);
    assert_eq!(scalar(&report.measurements, "ruling_waiting_issues"), 3.0);
    assert_eq!(
        scalar(&report.measurements, "spec_merge_waiting_issues"),
        3.0
    );
    export.coverage[0].complete = false;
    assert_eq!(
        exported_report(&config(), &export, &["decision"]).measurements["ruling_waiting_issues"]
            .value,
        None
    );
    export.coverage[0].complete = true;
    export.evidence.observations[0].attribution = None;
    assert_eq!(exported_report(&config(),&export,&["decision"]).measurements["spec_merge_waiting_issues"].value,None);
    export.evidence.observations.remove(0);
    if let Fact::Operation(OperationFact::Decision(f)) = &mut export.evidence.observations[0].fact {
        f.decision = None;
    }
    assert_eq!(
        exported_report(&config(), &export, &["decision"]).measurements["ruling_waiting_issues"]
            .value,
        None
    );
    let legacy: BTreeMap<_, _> = [
        ("decisions_open", 7.0, "issues"),
        ("decision_oldest_hours", 11.0, "hours"),
    ]
    .into_iter()
    .map(|(id, value, unit)| {
        (
            id.into(),
            crate::retro::Metric {
                id: id.into(),
                status: Status::Ok,
                value: Some(value),
                unit: unit.into(),
                direction: Direction::Down,
                source: "legacy".into(),
                query: "unchanged legacy formula".into(),
                reason: None,
            },
        )
    })
    .collect();
    let mut c = config();
    c.operations = Some(operations());
    let preserved = crate::retro::department::collect(
        &c,
        Frame {
            now: END,
            window_secs: 3600,
            end_exclusive: false,
        },
        &[],
        &legacy,
    );
    assert_eq!(scalar(&preserved.measurements, "decisions_open"), 7.0);
    assert_eq!(
        scalar(&preserved.measurements, "decision_oldest_hours"),
        11.0
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn capacity_load_and_signed_disk_headroom_use_configured_native_hosts() {
    for (team, capacity, used) in [("healthy", 5, 2), ("problem", 3, 4)] {
        let mut c = config();
        c.scope.teams[0].id = team.into();
        let mut fact = host();
        if let OperationFact::Host(f) = &mut fact {
            f.capacity_units = Some(capacity);
            f.used_units = Some(used);
        }
        let evidence = batch(vec![row("host", "host", 0.0, fact)]);
        let metrics = collect(&c, std::slice::from_ref(&evidence)).0;
        assert_eq!(keyed(&metrics, "host_slots_used", "host"), used as f64);
        assert_eq!(
            keyed(&metrics, "host_slots_free", "host"),
            capacity.saturating_sub(used) as f64
        );
        assert_eq!(
            keyed(&metrics, "host_overcommit_units", "host"),
            used.saturating_sub(capacity) as f64
        );
        assert_eq!(
            keyed(&metrics, "host_load_per_slot", "host"),
            6.0 / capacity as f64
        );
        assert_eq!(keyed(&metrics, "disk_available_bytes", "host"), 800.0);
        assert_eq!(keyed(&metrics, "disk_headroom_bytes", "host"), -200.0);
        assert_eq!(collect(&c, std::slice::from_ref(&evidence)).0, metrics);
    }
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn missing_remote_host_capacity_reserve_and_samples_do_not_fabricate_zero() {
    let mut c = config();
    c.scope.hosts.push(Selector {
        id: "remote".into(),
        label: "Remote".into(),
    });
    let metrics = collect(&c, &[batch(vec![row("host", "host", 0.0, host())])]).0;
    assert!(
        metrics["host_slots_used"]
            .reason
            .as_deref()
            .unwrap()
            .contains("remote")
    );
    let mut f = host();
    if let OperationFact::Host(f) = &mut f {
        f.capacity_units = None;
    }
    let metrics = measure(vec![row("host", "host", 0.0, f)]);
    assert_eq!(metrics["host_slots_free"].value, None);
    assert_eq!(keyed(&metrics, "disk_available_bytes", "host"), 800.0);
    let mut f = host();
    if let OperationFact::Host(f) = &mut f {
        f.capacity_units = Some(0);
        f.source_error = Some("sample read failed".into());
    }
    let metrics = measure(vec![row("host", "host", 0.0, f)]);
    assert_eq!(metrics["host_load_per_slot"].status, Status::Unavailable);
    assert_eq!(
        metrics["disk_headroom_bytes"].reason.as_deref(),
        Some("sample read failed")
    );
    assert_eq!(keyed(&metrics, "host_slots_used", "host"), 4.0);
    let (metrics, _) = collect_operations(
        &config(),
        &OperationalConfig::default(),
        Frame {
            now: END,
            window_secs: 3600,
            end_exclusive: false,
        },
        &[batch(vec![row("h", "host", 0.0, host())])],
    );
    assert!(
        metrics["disk_headroom_bytes"]
            .reason
            .as_deref()
            .unwrap()
            .contains("reserve")
    );
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn scheduler_distributions_separate_completed_samples_from_current_ages_and_window_start() {
    let metrics = measure(vec![
        row(
            "queued",
            "q",
            0.0,
            request(RequestPhase::Queued, -420.0, None, None),
        ),
        row(
            "running",
            "r",
            0.0,
            request(RequestPhase::Running, -720.0, Some(-600.0), None),
        ),
        row(
            "finished",
            "f",
            0.0,
            request(
                RequestPhase::Succeeded,
                -1200.0,
                Some(-1080.0),
                Some(-480.0),
            ),
        ),
        row(
            "excluded",
            "x",
            0.0,
            request(
                RequestPhase::Succeeded,
                -4800.0,
                Some(-4200.0),
                Some(-3600.0),
            ),
        ),
    ]);
    assert_eq!(
        duration(&metrics, "queue_wait_minutes", "host/queued_age"),
        Distribution {
            count: 1,
            median: 7.0,
            p90: 7.0
        }
    );
    assert_eq!(
        duration(&metrics, "queue_wait_minutes", "host/started_wait").count,
        2
    );
    assert_eq!(
        duration(&metrics, "lock_hold_minutes", "host/running_age").median,
        10.0
    );
    assert_eq!(
        duration(&metrics, "lock_hold_minutes", "host/finished_hold"),
        Distribution {
            count: 1,
            median: 10.0,
            p90: 10.0
        }
    );
    assert_eq!(
        measure(Vec::new())["queue_wait_minutes"].status,
        Status::Unavailable
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn partial_stale_missing_and_future_sources_invalidate_dependent_measurements() {
    let original = batch(vec![row("host", "host", 0.0, host())]);
    for observed in [None, Some(at(-61.0)), Some(at(3.0))] {
        let mut evidence = original.clone();
        evidence.observed_at = observed;
        assert_eq!(
            collect(&config(), &[evidence]).0["host_slots_used"].value,
            None
        );
    }
    let mut partial = original.clone();
    partial.complete.remove("host");
    assert_eq!(
        collect(&config(), &[partial]).0["host_slots_used"].value,
        None
    );
    let mut stale_row = original.clone();
    stale_row.observations[0].observed_at = Some(at(-61.0));
    assert_eq!(
        collect(&config(), &[stale_row]).0["host_slots_used"].value,
        None
    );
    let mut error = Evidence {
        source: "remote".into(),
        ..Default::default()
    };
    error
        .reasons
        .insert("host".into(), "remote host disconnected".into());
    assert!(
        collect(&config(), &[original, error]).0["host_slots_used"]
            .reason
            .as_deref()
            .unwrap()
            .contains("disconnected")
    );
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn relay_enqueue_delivery_ack_and_redelivery_are_distinct_original_message_counts() {
    let mut delivered = row("d1", "message", -120.0, relay("m1", RelayKind::Delivered));
    delivered.sequence = Some(2);
    let metrics = measure(vec![
        row("q1", "message", -180.0, relay("m1", RelayKind::Queued)),
        delivered,
        row(
            "redelivery",
            "message",
            -60.0,
            relay("m1", RelayKind::Delivered),
        ),
        row("q2", "second", -60.0, relay("m2", RelayKind::Queued)),
        row(
            "timeout",
            "second",
            -30.0,
            relay("m2", RelayKind::PromptTimeout),
        ),
    ]);
    assert_eq!(scalar(&metrics, "relay_queued"), 2.0);
    assert_eq!(scalar(&metrics, "relay_delivered"), 1.0);
    assert_eq!(scalar(&metrics, "relay_acknowledged"), 0.0);
    assert_eq!(scalar(&metrics, "prompt_timeout_unconfirmed"), 1.0);
    assert_eq!(
        duration(
            &metrics,
            "order_unacked_age_minutes",
            &format!("{LANE}/since_delivery")
        )
        .median,
        2.0
    );
    assert_eq!(
        duration(
            &metrics,
            "order_unacked_age_minutes",
            &format!("{LANE}/since_enqueue_delivery_unknown")
        )
        .median,
        1.0
    );
    let metrics = measure(vec![
        row("q", "a", -60.0, relay("m", RelayKind::Queued)),
        row("t", "a", -45.0, relay("m", RelayKind::PromptTimeout)),
        row("a", "a", -30.0, relay("m", RelayKind::Acknowledged)),
    ]);
    assert_eq!(scalar(&metrics, "prompt_timeout_unconfirmed"), 0.0);
    assert_eq!(metrics["order_unacked_age_minutes"].value, None);
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn denial_duration_unions_overlaps_intersects_window_and_requires_attribution() {
    let denial = |order: &str, start: f64, end: Option<f64>| {
        OperationFact::Denial(DenialFact {
            lane: LANE.into(),
            order_id: order.into(),
            denied_at: at(start),
            resolved_at: end.map(at),
        })
    };
    let rows = vec![
        row("d1", "d1", -4000.0, denial("o1", -4000.0, Some(-1800.0))),
        row("d2", "d2", -2000.0, denial("o2", -2000.0, Some(-900.0))),
        row("d3", "d3", -600.0, denial("o3", -600.0, None)),
    ];
    let metrics = measure(rows.clone());
    assert_eq!(keyed(&metrics, "refusals_by_lane", LANE), 2.0);
    assert_eq!(
        keyed(&metrics, "frozen_denial_hours", LANE),
        3300.0 / 3600.0
    );
    let mut missing = rows;
    missing[0].attribution = None;
    assert_eq!(
        measure(missing)["frozen_denial_hours"].status,
        Status::Unavailable
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn movement_uses_latest_verified_input_and_missing_remote_lane_remains_unknown() {
    let signal = OperationFact::LaneSignal(LaneSignal {
        lane: LANE.into(),
        login_blocked: Some(true),
        movement_complete: true,
        movement_at: vec![at(-7200.0), at(-1800.0)],
        reasons: BTreeMap::new(),
    });
    let rows = vec![row("lane", LANE, 0.0, signal)];
    let metrics = measure(rows.clone());
    assert_eq!(scalar(&metrics, "login_blocked_lanes"), 1.0);
    assert_eq!(keyed(&metrics, "lane_no_movement_hours", LANE), 0.5);
    let mut c = config();
    c.scope.lanes.push(Selector {
        id: "remote:team:code".into(),
        label: "Remote".into(),
    });
    assert!(
        collect(&c, &[batch(rows)]).0["lane_no_movement_hours"]
            .reason
            .as_deref()
            .unwrap()
            .contains("remote")
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn reconciliation_preserves_missing_side_and_refuses_skew_or_zero_denominator() {
    let pair = |tool: Option<&str>, truth: Option<&str>, offset: f64| {
        OperationFact::Reconciliation(ReconciliationFact {
            tool_value: tool.map(str::to_owned),
            truth_value: truth.map(str::to_owned),
            tool_at: Some(at(0.0)),
            truth_at: Some(at(offset)),
        })
    };
    let metrics = measure(vec![
        row(
            "one",
            "one",
            0.0,
            pair(Some("WORKING"), Some("WORKING"), 0.0),
        ),
        row("two", "two", 0.0, pair(Some("WORKING"), Some("IDLE"), 0.0)),
        row("three", "three", 0.0, pair(Some("WORKING"), None, 0.0)),
    ]);
    assert_eq!(scalar(&metrics, "reconciliation_agreement_ratio"), 0.5);
    assert_eq!(
        metrics["reconciliation_agreement_ratio"].denominator,
        Some(2)
    );
    assert_eq!(scalar(&metrics, "reconciliation_unpaired"), 1.0);
    assert_eq!(
        measure(Vec::new())["reconciliation_agreement_ratio"].value,
        None
    );
    assert!(
        measure(vec![row(
            "skew",
            "s",
            0.0,
            pair(Some("a"), Some("a"), -3.0)
        )])["reconciliation_agreement_ratio"]
            .reason
            .as_deref()
            .unwrap()
            .contains("skew")
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn installed_instruction_drift_and_unknown_release_are_distinct() {
    let instruction = |actual: Option<&str>| {
        OperationFact::Instruction(InstructionFact {
            lane: LANE.into(),
            component: "coder".into(),
            claimed: Some("0.4.0".into()),
            actual: actual.map(str::to_owned),
        })
    };
    assert_eq!(
        scalar(
            &measure(vec![row("i", "coder", 0.0, instruction(Some("0.4.1")))]),
            "instruction_version_mismatches"
        ),
        1.0
    );
    assert_eq!(
        measure(vec![row("i", "coder", 0.0, instruction(None))])["instruction_version_mismatches"]
            .value,
        None
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn stale_inner_observation_times_and_future_semantic_timestamps_remain_unknown() {
    let pair = OperationFact::Reconciliation(ReconciliationFact {
        tool_value: Some("working".into()),
        truth_value: Some("working".into()),
        tool_at: Some(at(-61.0)),
        truth_at: Some(at(-61.0)),
    });
    assert!(
        measure(vec![row("pair", "entity", 0.0, pair)])["reconciliation_agreement_ratio"]
            .reason
            .as_deref()
            .unwrap()
            .contains("stale")
    );
    let denial = OperationFact::Denial(DenialFact {
        lane: LANE.into(),
        order_id: "order".into(),
        denied_at: at(-60.0),
        resolved_at: Some(at(3.0)),
    });
    assert_eq!(
        measure(vec![row("denial", "order", 0.0, denial)])["frozen_denial_hours"].value,
        None
    );
    let message = OperationFact::Message(MessageFact {
        message_id: "message".into(),
        sent_at: at(3.0),
        sender: LANE.into(),
        correction: Some(false),
        status_only: Some(false),
    });
    assert_eq!(
        measure(vec![row("message", "message", 0.0, message)])["correction_rate"].value,
        None
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-4")]
#[test]
fn attributed_messages_deduplicate_redelivery_and_unknown_semantics_do_not_become_healthy() {
    let message = |id: &str, correction, status| {
        OperationFact::Message(MessageFact {
            message_id: id.into(),
            sent_at: at(if id == "m1" { -100.0 } else { -50.0 }),
            sender: LANE.into(),
            correction,
            status_only: status,
        })
    };
    let mut rows = vec![
        row("m1", "m1", -100.0, message("m1", Some(true), Some(false))),
        row("m2", "m2", -50.0, message("m2", Some(false), Some(true))),
        row(
            "repeat",
            "m1",
            -20.0,
            message("m1", Some(true), Some(false)),
        ),
    ];
    let metrics = measure(rows.clone());
    assert_eq!(keyed(&metrics, "messages_per_sender", LANE), 2.0);
    assert_eq!(scalar(&metrics, "correction_rate"), 0.5);
    assert_eq!(metrics["correction_rate"].denominator, Some(2));
    assert_eq!(
        metrics["correction_rate"].classification_coverage,
        Some(ClassificationCoverage {
            classified: 2,
            total: 2,
            basis: CoverageBasis::CompletePopulation
        })
    );
    assert_eq!(scalar(&metrics, "status_only_messages"), 1.0);
    rows.pop();
    rows[0].attribution = None;
    let metrics = measure(rows);
    assert_eq!(scalar(&metrics, "correction_rate"), 0.0);
    assert_eq!(
        metrics["correction_rate"].classification_coverage,
        Some(ClassificationCoverage {
            classified: 1,
            total: 2,
            basis: CoverageBasis::LabeledSubset
        })
    );
    assert_eq!(keyed(&metrics, "messages_per_sender", LANE), 2.0);
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn unknown_message_population_never_claims_classification_coverage_or_rate() {
    let mut evidence = batch(vec![row(
        "m",
        "m",
        0.0,
        OperationFact::Message(MessageFact {
            message_id: "m".into(),
            sent_at: at(0.0),
            sender: LANE.into(),
            correction: Some(false),
            status_only: Some(false),
        }),
    )]);
    evidence.complete.remove("message");
    let metrics = collect(&config(), &[evidence]).0;
    assert_eq!(metrics["correction_rate"].value, None);
    assert_eq!(metrics["correction_rate"].classification_coverage, None);
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn known_source_age_survives_unavailable_capabilities_but_missing_source_clock_stays_unknown() {
    let mut c = config();
    c.freshness_seconds = [("fixture".into(), 60)].into();
    let mut evidence = batch(Vec::new());
    evidence.observed_at = Some(at(-30.0));
    evidence
        .reasons
        .insert("order".into(), "receipt instrumentation unavailable".into());
    let metrics = collect(&c, std::slice::from_ref(&evidence)).0;
    assert_eq!(
        keyed(&metrics, "source_age_hours", "fixture"),
        30.0 / 3600.0
    );
    assert_eq!(metrics["repeated_orders"].value, None);
    evidence.observed_at = None;
    assert_eq!(collect(&c, &[evidence]).0["source_age_hours"].value, None);
}

#[trace("FR-029-AC-2", "FR-029-AC-5", "FR-029-AC-6")]
#[test]
fn independent_teams_assess_native_host_rows_as_healthy_or_overcommitted_without_combined_score() {
    use crate::retro::department::{
        Basis, Comparator, KeyReduction, Policy, Severity, Statistic, assess_metric,
    };
    let policy = Policy {
        metric_id: "host_overcommit_units".into(),
        statistic: Statistic::Value,
        unit: "units".into(),
        comparator: Comparator::Gte,
        amber: 1.0,
        red: 2.0,
        basis: Basis::Current,
        window_seconds: 3600,
        freshness_seconds: 60,
        expected_keys: ["host".into()].into(),
        key_reduction: Some(KeyReduction::WorstSeverity),
    };
    for (team, capacity, used, severity) in [
        ("team-alpha", 5, 2, Severity::Green),
        ("team-beta", 2, 4, Severity::Red),
    ] {
        let mut c = config();
        c.scope.teams[0].id = team.into();
        let mut f = host();
        if let OperationFact::Host(f) = &mut f {
            f.capacity_units = Some(capacity);
            f.used_units = Some(used);
        }
        let metrics = collect(&c, &[batch(vec![row("h", "host", 0.0, f)])]).0;
        let assessment =
            assess_metric(Some(&metrics["host_overcommit_units"]), Some(&policy), None);
        assert_eq!(assessment.summary.severity, severity);
        assert!(assessment.summary.complete);
    }
    let mut c = config();
    c.scope.hosts.push(Selector {
        id: "remote".into(),
        label: "Remote".into(),
    });
    let metrics = collect(&c, &[batch(vec![row("h", "host", 0.0, host())])]).0;
    let assessment = assess_metric(Some(&metrics["host_overcommit_units"]), Some(&policy), None);
    assert_eq!(assessment.summary.severity, Severity::Unknown);
    assert!(!assessment.summary.complete);
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn unrelated_host_source_does_not_contaminate_configured_host_population() {
    let mut unrelated = host();
    if let OperationFact::Host(f) = &mut unrelated {
        f.host = "unselected".into();
    }
    let mut stale = batch(vec![row("remote", "unselected", 0.0, unrelated)]);
    stale.observed_at = None;
    let metrics = collect(
        &config(),
        &[batch(vec![row("local", "host", 0.0, host())]), stale],
    )
    .0;
    assert_eq!(keyed(&metrics, "host_slots_used", "host"), 4.0);
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn explicit_order_reversals_have_exact_evidence_and_changed_authority_suppresses_pairs() {
    let mut rows = vec![
        row("first", "o1", -300.0, order("o1", None, "go", "e1")),
        row(
            "second",
            "o2",
            -200.0,
            order("o2", Some("o1"), "stop", "e1"),
        ),
        row("third", "o3", -100.0, order("o3", Some("o2"), "go", "e1")),
    ];
    rows.push(rows[1].clone());
    let (_, report) = collect(&config(), &[batch(rows.clone())]);
    assert_eq!(report.status, Status::Ok);
    assert_eq!(report.findings[0].event_ids, ["first", "second", "third"]);
    if let Fact::Operation(OperationFact::Order(f)) = &mut rows[2].fact {
        f.evidence_revision = Some("new-authority".into());
    }
    assert!(collect(&config(), &[batch(rows)]).1.findings.is_empty());
    let mut missing = row("unknown", "unknown", 0.0, order("o", None, "go", "e1"));
    missing.attribution = None;
    assert_eq!(
        collect(&config(), &[batch(vec![missing])]).1.status,
        Status::Unavailable
    );
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn repeated_order_counts_sends_after_first_only_for_unchanged_evidence() {
    let metrics = measure(vec![
        row("first", "o1", -4000.0, order("o1", None, "go", "e1")),
        row("second", "o1", -100.0, order("o1", None, "go", "e1")),
        row("new", "o1", -50.0, order("o1", None, "go", "e2")),
    ]);
    assert_eq!(scalar(&metrics, "repeated_orders"), 1.0);
}

#[trace("FR-029-AC-2", "FR-029-AC-8")]
#[test]
fn different_lanes_do_not_share_native_order_send_counters() {
    let mut c = config();
    c.scope.lanes.push(Selector {
        id: "remote:team:lead".into(),
        label: "Remote".into(),
    });
    let local = row(
        "local-send",
        "local-order",
        -100.0,
        order("order-1", None, "go", "e1"),
    );
    let mut remote = row(
        "remote-send",
        "remote-order",
        -50.0,
        order("order-1", None, "go", "e1"),
    );
    if let Fact::Operation(OperationFact::Order(f)) = &mut remote.fact {
        f.lane = "remote:team:lead".into();
        f.send_id = "local-send".into();
    }
    assert_eq!(
        scalar(
            &collect(&c, &[batch(vec![local, remote])]).0,
            "repeated_orders"
        ),
        0.0
    );
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn redelivered_orders_keep_send_identity_and_messages_keep_original_send_window() {
    let first = row("original", "o1", -100.0, order("o1", None, "go", "e1"));
    let mut redelivery = first.clone();
    redelivery.event_id = "redelivery".into();
    redelivery.event_at = Some(at(-50.0));
    assert_eq!(
        scalar(&measure(vec![first, redelivery]), "repeated_orders"),
        0.0
    );
    let message = OperationFact::Message(MessageFact {
        message_id: "old".into(),
        sent_at: at(-3600.0),
        sender: LANE.into(),
        correction: Some(true),
        status_only: Some(false),
    });
    let metrics = measure(vec![row("redelivered", "old", -100.0, message)]);
    assert_eq!(
        metrics["correction_rate"].classification_coverage,
        Some(ClassificationCoverage {
            classified: 0,
            total: 0,
            basis: CoverageBasis::CompletePopulation
        })
    );
    assert_eq!(metrics["correction_rate"].value, None);
}

#[trace("FR-029-AC-4")]
#[test]
fn planner_plan_scope_and_tick_audits_count_only_attributed_actionable_sends() {
    let assignment = |id: &str, in_plan, in_scope, new_evidence| {
        OperationFact::Assignment(AssignmentFact {
            message_id: id.into(),
            lane: LANE.into(),
            tick_id: "tick-1".into(),
            plan_revision: Some("wave-2".into()),
            in_plan: Some(in_plan),
            in_scope: Some(in_scope),
            evidence_checked: Some(true),
            prior_orders_checked: Some(true),
            end_tick_audit_ref: Some("audit:tick-1".into()),
            new_blocking_evidence: Some(new_evidence),
        })
    };
    let rows = vec![
        row("one", "one", -300.0, assignment("one", true, true, false)),
        row("two", "two", -200.0, assignment("two", false, false, false)),
        row(
            "three",
            "three",
            -100.0,
            assignment("three", true, true, true),
        ),
    ];
    let metrics = measure(rows.clone());
    assert_eq!(scalar(&metrics, "planner_ad_hoc_assignments"), 1.0);
    assert_eq!(scalar(&metrics, "planner_scope_violations"), 1.0);
    assert_eq!(scalar(&metrics, "planner_unbatched_sends"), 1.0);
    let mut missing = rows;
    if let Fact::Operation(OperationFact::Assignment(f)) = &mut missing[0].fact {
        f.end_tick_audit_ref = None;
    }
    assert_eq!(
        measure(missing)["planner_scope_violations"].status,
        Status::Unavailable
    );
}

#[trace("FR-029-AC-10")]
#[test]
fn operational_file_adapter_reads_actual_normalized_exports_and_retains_read_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("operational.json");
    let mut original = batch(vec![row("host", "host", 0.0, host())]);
    original.complete = ["host".into()].into();
    let export = OperationalExport {
        version: 1,
        evidence: original.clone(),
        coverage: vec![OperationalCoverage {
            family: "host".into(),
            entities: ["host".into()].into(),
            from: at(0.0),
            until: at(0.0),
            complete: true,
            full_history: false,
        }],
    };
    std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
    let mut op = operations();
    op.evidence_files.push(OperationalInput {
        path,
        source: "fixture".into(),
        families: ["host".into()].into(),
    });
    let evidence = read_files(&op);
    assert_eq!(evidence, [original]);
    assert_eq!(
        keyed(&collect(&config(), &evidence).0, "host_slots_used", "host"),
        4.0
    );
    op.evidence_files.push(OperationalInput {
        path: directory.path().join("missing.json"),
        source: "missing".into(),
        families: ["host".into()].into(),
    });
    assert_eq!(
        collect(&config(), &read_files(&op)).0["host_slots_used"].value,
        None
    );
}

#[trace("FR-029-AC-2", "FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn operational_export_requires_remote_population_watermarks_and_origin_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("relay.json");
    let mut c = config();
    c.scope.lanes.push(Selector {
        id: "remote:team:lead".into(),
        label: "Remote".into(),
    });
    let mut evidence = batch(Vec::new());
    evidence.complete = ["relay_event".into()].into();
    let mut export = OperationalExport {
        version: 1,
        evidence,
        coverage: vec![OperationalCoverage {
            family: "relay_event".into(),
            entities: [LANE.into()].into(),
            from: at(-3600.0),
            until: at(0.0),
            complete: true,
            full_history: true,
        }],
    };
    let mut op = operations();
    op.evidence_files.push(OperationalInput {
        path: path.clone(),
        source: "fixture".into(),
        families: ["relay_event".into()].into(),
    });
    let frame = Frame {
        now: END,
        window_secs: 3600,
        end_exclusive: false,
    };
    for state in [
        "missing_remote",
        "missing_origin",
        "short_watermark",
        "complete",
    ] {
        export.coverage[0].entities = [LANE.into(), "remote:team:lead".into()].into();
        export.coverage[0].full_history = true;
        export.coverage[0].until = at(0.0);
        match state {
            "missing_remote" => {
                export.coverage[0].entities.remove("remote:team:lead");
            }
            "missing_origin" => export.coverage[0].full_history = false,
            "short_watermark" => export.coverage[0].until = at(-1.0),
            _ => {}
        }
        std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
        let metrics = collect_operations(&c, &op, frame, &read_operational_files(&op, &c, frame)).0;
        if state == "complete" {
            assert_eq!(scalar(&metrics, "relay_queued"), 0.0);
        } else {
            assert_eq!(
                metrics["relay_queued"].status,
                Status::Unavailable,
                "{state}"
            );
        }
    }
}

#[trace("FR-029-AC-4", "FR-029-AC-9", "FR-029-AC-10")]
#[test]
fn released_scheduler_status_export_is_consumed_without_reading_private_state_or_running_a_command()
{
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scheduler-status.json");
    let mut export = serde_json::json!({
        "schema_version":1,"store_id":"store-a","host":"host","observed_at_ms":1800000000000_u64,
        "capacity_units":3,"used_units":1,"free_units":2,"overcommit_units":0,
        "sample":{"host":"host","observed_at_ms":1800000000000_u64,"memory_available_bytes":10000,
            "disk_available_bytes":3000,"load_one":1.5},
        "source_error":null,"pruned_events":0,"next_event_sequence":3,
        "requests":[{"request_id":"store-a:r1","owner":LANE,"ticket":"ticket-native-id","state":"running",
            "units":1,"priority":0,"enqueued_at_ms":1799999280000_u64,"started_at_ms":1799999400000_u64,
            "ended_at_ms":null,"wait_ms":120000,"hold_ms":600000,"suspect":false,"reason":null,"exit_code":null,"signal":null}],
        "events":[{"schema_version":1,"event_id":"store-a:e1","request_id":"store-a:r1","host":"host",
            "owner":LANE,"ticket":"ticket-native-id","observed_at_ms":1799999280000_u64,"state":"queued",
            "units":1,"priority":0,"capacity_units":3,"free_units":3,"wait_ms":0,"hold_ms":null,"reason":null},
            {"schema_version":1,"event_id":"store-a:e2","request_id":"store-a:r1","host":"host",
            "owner":LANE,"ticket":"ticket-native-id","observed_at_ms":1799999400000_u64,"state":"running",
            "units":1,"priority":0,"capacity_units":3,"free_units":2,"wait_ms":120000,"hold_ms":0,"reason":null}]
    });
    std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
    let mut op = operations();
    op.scheduler_files.insert("host".into(), path.clone());
    let run = || {
        collect_operations(
            &config(),
            &op,
            Frame {
                now: END,
                window_secs: 3600,
                end_exclusive: false,
            },
            &read_files(&op),
        )
        .0
    };
    let metrics = run();
    assert_eq!(keyed(&metrics, "host_slots_used", "host"), 1.0);
    assert_eq!(keyed(&metrics, "host_load_per_slot", "host"), 0.5);
    assert_eq!(keyed(&metrics, "disk_headroom_bytes", "host"), 2000.0);
    assert_eq!(
        duration(&metrics, "queue_wait_minutes", "host/started_wait").median,
        2.0
    );
    assert_eq!(
        duration(&metrics, "lock_hold_minutes", "host/running_age").median,
        10.0
    );
    export["sample"]["observed_at_ms"] = serde_json::json!(1799999939000_u64);
    std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
    let metrics = run();
    assert_eq!(metrics["host_load_per_slot"].status, Status::Unavailable);
    assert_eq!(keyed(&metrics, "host_slots_used", "host"), 1.0);
    export["pruned_events"] = serde_json::json!(1);
    std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
    assert_eq!(run()["queue_wait_minutes"].status, Status::Unavailable);
    export["schema_version"] = serde_json::json!(2);
    std::fs::write(&path, serde_json::to_vec(&export).unwrap()).unwrap();
    assert!(
        run()["host_slots_used"]
            .reason
            .as_deref()
            .unwrap()
            .contains("schema_version")
    );
}
