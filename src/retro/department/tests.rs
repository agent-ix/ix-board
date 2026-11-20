// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Exact canonical fixture oracles, independent of credentials and ambient clocks.
use super::*;
use crate::retro::{Frame, Status};
use ix_trace_rs::trace;
use std::collections::{BTreeMap, BTreeSet};

const END: f64 = 1_800_000_000.0;
fn at(offset: f64) -> String {
    crate::drift::format_timestamp(END + offset)
}
fn scope(team: &str, lane: &str) -> Scope {
    Scope {
        teams: vec![Selector {
            id: team.into(),
            label: "Fixture team".into(),
        }],
        lanes: vec![Selector {
            id: lane.into(),
            label: "Fixture lane".into(),
        }],
        repositories: vec![Selector {
            id: "fixture/repo".into(),
            label: "Repo".into(),
        }],
        ..Default::default()
    }
}
fn config() -> Config {
    Config {
        version: 1,
        policy_id: "fixture-policy".into(),
        scope: scope("QSL", "host:workspace:code"),
        freshness_seconds: [("fixture".into(), 60)].into(),
        clock_tolerance_seconds: 2,
        parameters: Parameters {
            lane_capacity: [("host:workspace:code".into(), 2)].into(),
            issue_lanes: [
                ("issue-a".into(), "host:workspace:code".into()),
                ("issue-b".into(), "host:workspace:code".into()),
            ]
            .into(),
            completion_obligations: Some(["accepted".into()].into()),
            states: ["started".into(), "review".into()].into(),
            gates: ["issue-a".into()].into(),
            stale_seconds: Some(3600),
            repin_seconds: Some(120),
            diff_classifier: Some("diff-v1".into()),
            review_round_limit: Some(1),
            loop_lookback_seconds: Some(86400),
            loop_repeat_count: Some(2),
        },
        policies: Vec::new(),
        classes: Vec::new(),
    }
}
fn issue(started: bool) -> IssueFact {
    IssueFact {
        team: "QSL".into(),
        project: None,
        state: "started".into(),
        started,
        terminal: false,
        blockers: BTreeSet::new(),
        claim_at: Some(at(-7200.0)),
        movement_complete: true,
        movement_at: None,
        blocked_at: None,
        merged_pr: Some(false),
        obligations: Some(BTreeSet::new()),
    }
}
fn row(id: &str, entity: &str, offset: f64, fact: Fact) -> Observation {
    Observation {
        event_id: id.into(),
        entity_id: entity.into(),
        event_at: Some(at(offset)),
        observed_at: Some(at(0.0)),
        sequence: Some(1),
        scope: config().scope,
        complete: true,
        source_ref: format!("fixture:{id}"),
        attribution: None,
        fact,
    }
}
fn batch(rows: Vec<Observation>) -> Evidence {
    Evidence {
        source: "fixture".into(),
        observed_at: Some(at(0.0)),
        complete: [
            "issue",
            "state",
            "lane",
            "pull_request",
            "pin",
            "pin_event",
            "check",
        ]
        .map(str::to_owned)
        .into(),
        reasons: BTreeMap::new(),
        observations: rows,
    }
}
fn collect_rows(c: &Config, b: &Evidence) -> Report {
    collect(
        c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        std::slice::from_ref(b),
        &BTreeMap::new(),
    )
}
fn scalar(r: &Report, id: &str) -> f64 {
    match r.measurements[id].value.as_ref().expect(id) {
        Value::Scalar(v) => *v,
        other => panic!("{id}: {other:?}"),
    }
}
fn policy(id: &str) -> Policy {
    Policy {
        metric_id: id.into(),
        statistic: Statistic::Value,
        unit: definition(id).unwrap().unit.into(),
        comparator: Comparator::Gt,
        amber: 1.0,
        red: 2.0,
        basis: Basis::Current,
        window_seconds: 86400,
        freshness_seconds: 60,
        expected_keys: BTreeSet::new(),
        key_reduction: None,
    }
}
fn member(severity: Severity) -> KeyAssessment {
    KeyAssessment {
        value: None,
        severity,
        reason: (severity == Severity::Unknown).then(|| "missing fixture".into()),
    }
}

#[trace("FR-029-AC-2")]
#[test]
fn native_identity_deduplicates_populations_and_replays_two_independent_teams() {
    for (team, lane, capacity) in [
        ("QSL", "host:workspace:code", 2),
        ("NONQSL", "remote:second:build", 4),
    ] {
        let mut c = config();
        c.scope = scope(team, lane);
        c.parameters.lane_capacity = [(lane.into(), capacity)].into();
        c.parameters.issue_lanes = [("issue-a".into(), lane.into())].into();
        let mut i = issue(true);
        i.team = team.into();
        let o = row("claim-a", "issue-a", -7200.0, Fact::Issue(i));
        let b = batch(vec![o.clone(), o]);
        let report = collect_rows(&c, &b);
        assert_eq!(report, collect_rows(&c, &b));
        assert_eq!(report.scope, c.scope);
        assert_eq!(scalar(&report, "wip_active"), 1.0);
        assert_eq!(
            report.measurements["wip_capacity_ratio"].value,
            Some(Value::Keyed([(lane.into(), 1.0 / capacity as f64)].into()))
        );
        assert_eq!(report.observations.len(), 1);
        assert!(
            report.measurements["wip_active"]
                .evidence_refs
                .iter()
                .all(|id| report.observations.iter().any(|o| &o.event_id == id))
        );
    }
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn fresh_restarted_and_moved_claims_are_excluded_and_unknown_history_is_isolated() {
    let c = config();
    let old = issue(true);
    let mut fresh = old.clone();
    fresh.claim_at = Some(at(-3600.0));
    let mut restarted = old.clone();
    restarted.claim_at = Some(at(-30.0));
    let mut moved = old.clone();
    moved.movement_at = Some(at(-3599.0));
    let b = batch(vec![
        row("old", "issue-a", -7200.0, Fact::Issue(old)),
        row("fresh", "fresh", -3600.0, Fact::Issue(fresh)),
        row("restart", "restart", -30.0, Fact::Issue(restarted)),
        row("movement", "moved", -1.0, Fact::Issue(moved)),
    ]);
    let r = collect_rows(&c, &b);
    assert_eq!(scalar(&r, "claims_without_movement"), 1.0);
    assert_eq!(scalar(&r, "wip_active"), 4.0);
    let mut missing = b.clone();
    if let Fact::Issue(i) = &mut missing.observations[0].fact {
        i.claim_at = None;
    }
    let r = collect_rows(&c, &missing);
    assert!(
        r.measurements["claims_without_movement"]
            .reason
            .as_ref()
            .unwrap()
            .contains("claim")
    );
    assert_eq!(scalar(&r, "wip_active"), 4.0);
    let mut missing = c.clone();
    missing.parameters.completion_obligations = None;
    let r = collect_rows(&missing, &b);
    assert!(
        r.measurements["merged_open"]
            .reason
            .as_ref()
            .unwrap()
            .contains("completion_obligations")
    );
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn dependency_depth_fanin_cancelled_partial_and_cyclic_graphs_are_exact() {
    let mut a = issue(true);
    a.blockers = ["issue-b".into()].into();
    a.blocked_at = Some(at(-7200.0));
    let b = issue(false);
    let c = config();
    let evidence = batch(vec![
        row("a", "issue-a", -1.0, Fact::Issue(a)),
        row("b", "issue-b", -1.0, Fact::Issue(b)),
    ]);
    let r = collect_rows(&c, &evidence);
    assert_eq!(scalar(&r, "open_chain_length"), 1.0);
    assert_eq!(scalar(&r, "oldest_gated_hours"), 2.0);
    assert_eq!(
        r.measurements["lane_fanin"].value,
        Some(Value::Keyed([("host:workspace:code".into(), 1.0)].into()))
    );
    let mut canceled = evidence.clone();
    if let Fact::Issue(i) = &mut canceled.observations[1].fact {
        i.terminal = true;
    }
    assert_eq!(
        scalar(&collect_rows(&c, &canceled), "open_chain_length"),
        0.0
    );
    let mut partial = evidence.clone();
    partial.observations.pop();
    let r = collect_rows(&c, &partial);
    assert!(
        r.measurements["open_chain_length"]
            .reason
            .as_ref()
            .unwrap()
            .contains("partial dependency")
    );
    assert_eq!(scalar(&r, "wip_active"), 1.0);
    let mut cycle = evidence.clone();
    if let Fact::Issue(i) = &mut cycle.observations[1].fact {
        i.blockers = ["issue-a".into()].into();
    }
    assert!(
        collect_rows(&c, &cycle).measurements["open_chain_length"]
            .reason
            .as_ref()
            .unwrap()
            .contains("cycle")
    );
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn state_exit_boundaries_and_singleton_seven_hour_distributions_match_oracles() {
    let c = config();
    let b = batch(vec![
        row("i", "issue-a", 0.0, Fact::Issue(issue(true))),
        row(
            "entry",
            "issue-a",
            -7.0 * 3600.0,
            Fact::State(StateFact {
                state: "started".into(),
            }),
        ),
        row(
            "exit",
            "issue-a",
            0.0,
            Fact::State(StateFact {
                state: "review".into(),
            }),
        ),
    ]);
    let r = collect_rows(&c, &b);
    assert_eq!(
        r.measurements["state_cycle_hours"].value,
        Some(Value::Durations(
            [(
                "started".into(),
                Distribution {
                    count: 1,
                    median: 7.0,
                    p90: 7.0
                }
            )]
            .into()
        ))
    );
    let mut excluded = b.clone();
    excluded.observations[1].event_at = Some(at(-90000.0));
    excluded.observations[2].event_at = Some(at(-86400.0));
    assert_eq!(
        collect_rows(&c, &excluded).measurements["state_cycle_hours"].status,
        Status::Unavailable
    );
    assert_eq!(Distribution::from_samples(&[]), None);
    assert_eq!(
        Distribution::from_samples(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0])
            .unwrap()
            .p90,
        9.0
    );
}

#[trace("FR-029-AC-4")]
#[test]
fn completion_review_ready_diff_attribution_and_round_policy_use_explicit_evidence() {
    let mut i = issue(true);
    i.merged_pr = Some(true);
    i.obligations = Some(["accepted".into()].into());
    let pr = PrFact {
        repo: "fixture/repo".into(),
        open: true,
        draft: false,
        merged_at: None,
        review_ready_at: Some(at(-7.0 * 3600.0)),
        assigned_reviewers: Some(0),
        qualifying_reviews: Some(0),
        repin_only: Some(true),
        fix_rounds: Some(2),
        full_gates: Some(1),
    };
    let mut p = row("pr", "fixture/repo#1", -1.0, Fact::PullRequest(pr));
    p.attribution = Some(Attribution {
        actor: "review-tool".into(),
        classifier_version: "diff-v1".into(),
        offset_start: 0,
        offset_end: 12,
    });
    let mut b = batch(vec![row("i", "issue-a", 0.0, Fact::Issue(i)), p]);
    let c = config();
    let r = collect_rows(&c, &b);
    assert_eq!(scalar(&r, "merged_open"), 1.0);
    assert_eq!(scalar(&r, "repin_only_prs"), 1.0);
    assert_eq!(scalar(&r, "review_round_policy_excess"), 1.0);
    assert_eq!(
        r.measurements["pr_without_reviewer_age_hours"].value,
        Some(Value::Durations(
            [(
                "all".into(),
                Distribution {
                    count: 1,
                    median: 7.0,
                    p90: 7.0
                }
            )]
            .into()
        ))
    );
    b.observations[1].attribution = None;
    let r = collect_rows(&c, &b);
    assert!(
        r.measurements["repin_only_prs"]
            .reason
            .as_ref()
            .unwrap()
            .contains("attribution")
    );
    assert_eq!(scalar(&r, "merged_open"), 1.0);
    if let Fact::PullRequest(p) = &mut b.observations[1].fact {
        p.review_ready_at = None;
    }
    assert!(
        collect_rows(&c, &b).measurements["pr_without_reviewer_age_hours"]
            .reason
            .as_ref()
            .unwrap()
            .contains("review-ready")
    );
}

#[trace("FR-029-AC-5")]
#[test]
fn four_comparators_and_three_bases_respect_exact_threshold_boundaries() {
    let report = collect_rows(
        &config(),
        &batch(vec![row("i", "issue-a", 0.0, Fact::Issue(issue(true)))]),
    );
    let mut metric = report.measurements["wip_active"].clone();
    for (op, values) in [
        (
            Comparator::Gt,
            [Severity::Green, Severity::Amber, Severity::Red],
        ),
        (
            Comparator::Gte,
            [Severity::Amber, Severity::Red, Severity::Red],
        ),
        (
            Comparator::Lt,
            [Severity::Green, Severity::Amber, Severity::Red],
        ),
        (
            Comparator::Lte,
            [Severity::Amber, Severity::Red, Severity::Red],
        ),
    ] {
        for basis in [Basis::Current, Basis::SincePrevious, Basis::SinceBaseline] {
            let mut p = policy("wip_active");
            p.comparator = op;
            p.basis = basis;
            let descending = matches!(op, Comparator::Lt | Comparator::Lte);
            if descending {
                p.amber = 2.0;
                p.red = 1.0;
            }
            for (value, expected) in if descending {
                [2.0, 1.0, 0.0]
            } else {
                [1.0, 2.0, 3.0]
            }
            .into_iter()
            .zip(values)
            {
                let mut reference = metric.clone();
                reference.value = Some(Value::Scalar(10.0));
                metric.value = Some(Value::Scalar(
                    value + if basis == Basis::Current { 0.0 } else { 10.0 },
                ));
                assert_eq!(
                    assess_metric(Some(&metric), Some(&p), Some(&reference))
                        .summary
                        .severity,
                    expected,
                    "{op:?}/{basis:?}/{value}"
                );
            }
        }
    }
    assert_eq!(
        assess_metric(Some(&metric), None, None).summary.severity,
        Severity::Unknown
    );
}

#[trace("FR-029-AC-5", "FR-029-AC-11")]
#[test]
fn malformed_versions_units_statistics_keys_and_duplicates_name_policy_field() {
    let mut c = config();
    c.policies = vec![policy("wip_active")];
    assert_eq!(c.validate(), Ok(()));
    type ConfigMutation = (&'static str, Box<dyn Fn(&mut Config)>);
    let mutations: Vec<ConfigMutation> = vec![
        ("version", Box::new(|c| c.version = 99)),
        (
            "duplicate",
            Box::new(|c| c.policies.push(c.policies[0].clone())),
        ),
        ("unit", Box::new(|c| c.policies[0].unit = "hours".into())),
        ("threshold", Box::new(|c| c.policies[0].red = 0.0)),
        (
            "statistic",
            Box::new(|c| c.policies[0].statistic = Statistic::Median),
        ),
        (
            "unknown",
            Box::new(|c| c.policies[0].metric_id = "invented".into()),
        ),
        (
            "expected_keys",
            Box::new(|c| c.policies[0] = policy("wip_capacity_ratio")),
        ),
        (
            "categorical",
            Box::new(|c| c.policies[0] = policy("lane_execution_state")),
        ),
    ];
    for (field, mutate) in mutations {
        let mut bad = c.clone();
        mutate(&mut bad);
        assert!(bad.validate().unwrap_err().contains(field), "{field}");
    }
    let mut value = serde_json::to_value(&c).unwrap();
    value["policies"][0]
        .as_object_mut()
        .unwrap()
        .remove("statistic");
    assert!(
        serde_json::from_value::<Config>(value)
            .unwrap_err()
            .to_string()
            .contains("statistic")
    );
}

#[trace("FR-029-AC-6")]
#[test]
fn class_and_department_severity_preserve_breaches_and_independent_completeness() {
    for (known, expected) in [
        (Severity::Red, Severity::Red),
        (Severity::Amber, Severity::Amber),
        (Severity::Green, Severity::Unknown),
    ] {
        let s = summarize(
            &[
                ("known".into(), member(known)),
                ("unknown".into(), member(Severity::Unknown)),
            ]
            .into(),
        );
        assert_eq!(s.severity, expected);
        assert!(!s.complete);
        assert!(s.missing.contains_key("unknown"));
    }
    let s = summarize(&[("known".into(), member(Severity::Green))].into());
    assert_eq!(s.severity, Severity::Green);
    assert!(s.complete);
    assert!(!summarize(&BTreeMap::new()).complete);
    let mut c = config();
    let b = batch(vec![row("i", "issue-a", 0.0, Fact::Issue(issue(true)))]);
    c.policies = vec![policy("wip_active")];
    c.classes = CLASSES
        .iter()
        .map(|id| ClassConfig {
            id: (*id).into(),
            required: ["wip_active".into()].into(),
            optional: ["host_slots_used".into()].into(),
            route: None,
        })
        .collect();
    let r = collect_rows(&c, &b);
    assert_eq!(r.classes.len(), 10);
    assert_eq!(r.rollup.severity, Severity::Green);
    assert!(r.rollup.complete);
    assert_eq!(r.classes["flow_wip"].route, Route::Unrouted);
    c.classes[0].required.insert("host_slots_used".into());
    let r = collect_rows(&c, &b);
    assert_eq!(r.rollup.severity, Severity::Unknown);
    assert!(!r.rollup.complete);
}

#[trace("FR-029-AC-7")]
#[test]
fn normalized_configuration_identity_ignores_order_but_detects_semantic_changes() {
    let mut c = config();
    c.scope.teams.push(Selector {
        id: "other".into(),
        label: "Other".into(),
    });
    c.policies = vec![policy("wip_active"), policy("merged_open")];
    let original = c.measurement_identity("wip_active", 86400);
    let mut reordered = c.clone();
    reordered.scope.teams.reverse();
    reordered.policies.reverse();
    assert_eq!(
        original,
        reordered.measurement_identity("wip_active", 86400)
    );
    for changed in [
        {
            let mut v = c.clone();
            v.scope.teams[0].id = "changed".into();
            v
        },
        {
            let mut v = c.clone();
            v.parameters.stale_seconds = Some(99);
            v
        },
        {
            let mut v = c.clone();
            v.policies[0].red = 99.0;
            v
        },
    ] {
        assert_ne!(original, changed.measurement_identity("wip_active", 86400));
    }
    let a = collect_rows(
        &c,
        &batch(vec![row("i", "issue-a", 0.0, Fact::Issue(issue(true)))]),
    );
    let mut other = a.measurements["wip_active"].clone();
    other.query_identity = "changed-formula".into();
    let comparison = compare(&a.measurements["wip_active"], Some(&other));
    assert_eq!(comparison.state, ComparisonState::QueryChanged);
    assert!(comparison.changes.is_empty());
    assert_eq!(compare(&other, None).state, ComparisonState::Unavailable);
}

#[trace("FR-029-AC-8")]
#[test]
fn tag_repin_and_state_bounce_deduplicate_ordered_native_events_and_refuse_unknown_order() {
    let mut rows = vec![row("i", "issue-a", 0.0, Fact::Issue(issue(true)))];
    for (id, t, event) in [
        ("tag-1", -200.0, PinEventKind::Tag),
        ("repin-1", -150.0, PinEventKind::Repin),
        ("tag-2", -100.0, PinEventKind::Tag),
        ("repin-2", -50.0, PinEventKind::Repin),
    ] {
        rows.push(row(
            id,
            "dependency",
            t,
            Fact::PinEvent(PinEvent {
                dependency: "crate".into(),
                event,
            }),
        ));
    }
    for (n, state) in ["started", "review", "started", "review", "started"]
        .into_iter()
        .enumerate()
    {
        rows.push(row(
            &format!("state-{n}"),
            "issue-a",
            -1000.0 + n as f64,
            Fact::State(StateFact {
                state: state.into(),
            }),
        ));
    }
    rows.push(rows[1].clone());
    let c = config();
    let mut b = batch(rows);
    let r = collect_rows(&c, &b);
    assert_eq!(scalar(&r, "tag_repin_events"), 2.0);
    assert_eq!(
        r.loops["tag_repin_loop"].findings[0].event_ids,
        ["tag-1", "repin-1", "tag-2", "repin-2"]
    );
    assert_eq!(
        r.loops["state_bounce_loop"].findings[0].event_ids,
        ["state-0", "state-2", "state-4"]
    );
    let first = &r.loops["tag_repin_loop"].findings[0].deduplication_key;
    assert_eq!(
        first,
        &collect_rows(&c, &b).loops["tag_repin_loop"].findings[0].deduplication_key
    );
    b.observations[2].event_at = b.observations[1].event_at.clone();
    b.observations[2].sequence = Some(2);
    assert_eq!(
        collect_rows(&c, &b).loops["tag_repin_loop"].status,
        Status::Ok
    );
    b.observations[2].sequence = None;
    let r = collect_rows(&c, &b);
    assert_eq!(r.loops["tag_repin_loop"].status, Status::Unavailable);
    assert!(r.loops["tag_repin_loop"].findings.is_empty());
}

#[trace("FR-029-AC-9")]
#[test]
fn partial_pages_missing_timestamps_staleness_skew_and_zero_capacity_never_become_green() {
    let c = config();
    let b = batch(vec![row("i", "issue-a", 0.0, Fact::Issue(issue(true)))]);
    for observed in [None, Some(at(-61.0)), Some(at(3.0))] {
        let mut b = b.clone();
        b.observed_at = observed;
        let r = collect_rows(&c, &b);
        assert_eq!(r.measurements["wip_active"].status, Status::Unavailable);
        assert!(!r.rollup.complete);
    }
    let mut partial = b.clone();
    partial.complete.remove("issue");
    assert_eq!(
        collect_rows(&c, &partial).measurements["wip_active"].value,
        None
    );
    let mut zero = c.clone();
    zero.parameters
        .lane_capacity
        .insert("host:workspace:code".into(), 0);
    let r = collect_rows(&zero, &b);
    assert_eq!(r.measurements["wip_capacity_ratio"].value, None);
    assert_eq!(scalar(&r, "wip_active"), 1.0);
}

#[trace("FR-029-AC-11")]
#[test]
fn keyed_policy_selects_count_median_or_p90_per_expected_key_without_averaging() {
    let r = collect_rows(&config(), &batch(Vec::new()));
    let mut m = r.measurements["state_cycle_hours"].clone();
    m.status = Status::Ok;
    m.reason = None;
    m.source_age_seconds = Some(0.0);
    m.value = Some(Value::Durations(
        [
            (
                "state-a".into(),
                Distribution {
                    count: 1,
                    median: 7.0,
                    p90: 7.0,
                },
            ),
            (
                "state-b".into(),
                Distribution {
                    count: 3,
                    median: 0.0,
                    p90: 0.0,
                },
            ),
            (
                "unselected".into(),
                Distribution {
                    count: 100,
                    median: 100.0,
                    p90: 100.0,
                },
            ),
        ]
        .into(),
    ));
    let mut p = policy("state_cycle_hours");
    p.statistic = Statistic::Median;
    p.expected_keys = ["state-a".into(), "state-b".into()].into();
    p.key_reduction = Some(KeyReduction::WorstSeverity);
    let a = assess_metric(Some(&m), Some(&p), None);
    assert_eq!(a.summary.severity, Severity::Red);
    assert!(a.summary.complete);
    assert_eq!(a.keys["state-a"].value, Some(7.0));
    assert_eq!(a.keys["state-b"].severity, Severity::Green);
    p.expected_keys.insert("missing".into());
    let a = assess_metric(Some(&m), Some(&p), None);
    assert_eq!(a.summary.severity, Severity::Red);
    assert!(!a.summary.complete);
    assert!(a.summary.missing.contains_key("missing"));
    p.statistic = Statistic::Count;
    p.unit = "count".into();
    p.expected_keys.remove("missing");
    let a = assess_metric(Some(&m), Some(&p), None);
    assert_eq!(a.keys["state-a"].value, Some(1.0));
    assert_eq!(a.keys["state-b"].value, Some(3.0));
}

#[trace("FR-029-AC-10", "FR-029-AC-12")]
#[test]
fn categorical_roundtrip_is_unassessed_and_compares_equality_without_numeric_trends() {
    let r = collect_rows(&config(), &batch(Vec::new()));
    let mut m = r.measurements["lane_execution_state"].clone();
    m.status = Status::Ok;
    m.reason = None;
    m.value = Some(Value::SourceObservation(SourceObservation {
        category: "WORKING".into(),
        source_schema_version: "agents-1".into(),
        observation_ref: "lifecycle-row".into(),
    }));
    let encoded = serde_json::to_string(&m).unwrap();
    let decoded: Measurement = serde_json::from_str(&encoded).unwrap();
    assert_eq!(m, decoded);
    let a = assess_metric(Some(&m), None, None);
    assert_eq!(a.reason.as_deref(), Some("categorical evidence only"));
    assert!(!a.summary.complete);
    assert_eq!(compare(&m, Some(&decoded)).state, ComparisonState::Same);
    let mut changed = decoded.clone();
    if let Some(Value::SourceObservation(s)) = &mut changed.value {
        s.category = "IDLE".into();
    }
    let comparison = compare(&changed, Some(&decoded));
    assert_eq!(comparison.state, ComparisonState::Changed);
    assert!(comparison.changes.is_empty());
    if let Some(Value::SourceObservation(s)) = &mut changed.value {
        s.source_schema_version = "agents-2".into();
    }
    assert_eq!(
        compare(&changed, Some(&decoded)).state,
        ComparisonState::QueryChanged
    );
    changed.value = None;
    changed.status = Status::Unavailable;
    assert_eq!(
        compare(&changed, Some(&decoded)).state,
        ComparisonState::Unavailable
    );
    assert_eq!(
        serde_json::from_str::<Report>(&serde_json::to_string(&r).unwrap()).unwrap(),
        r
    );
}

#[trace("FR-029-AC-4")]
#[test]
fn injected_http_lock_and_reachability_boundary_keeps_unrelated_revision_counts_available() {
    let c = config();
    let lock = "version = 4\n[[package]]\nname = 'dep'\nversion = '1.0.0'\nsource = 'git+https://github.com/fixture/dependency?branch=main#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
    let evidence = collect_pins(&c.scope, &at(0.0), |endpoint, raw| {
        if raw {
            assert_eq!(endpoint, "repos/fixture/repo/contents/Cargo.lock");
            Ok(lock.into())
        } else {
            assert!(endpoint.contains("/compare/"));
            Ok("{\"status\":\"ahead\"}".into())
        }
    });
    let mut c = c;
    c.freshness_seconds.insert("github".into(), 60);
    let r = collect_rows(&c, &evidence);
    assert_eq!(
        r.measurements["crate_revision_count"].value,
        Some(Value::Keyed([("dep".into(), 1.0)].into()))
    );
    assert_eq!(scalar(&r, "pins_off_main"), 0.0);
    let missing = collect_pins(&c.scope, &at(0.0), |_, raw| {
        if raw {
            Ok(lock.into())
        } else {
            Err("dependency repository unavailable".into())
        }
    });
    let r = collect_rows(&c, &missing);
    assert_eq!(r.measurements["crate_revision_count"].status, Status::Ok);
    assert_eq!(r.measurements["pins_off_main"].status, Status::Unavailable);
}
