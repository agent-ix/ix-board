// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Deterministic operational fixture oracles with explicit source completeness.

use super::*;
use crate::retro::department::{Parameters, Selector};
use ix_trace_rs::trace;

const END: f64 = 1_800_000_000.0;
const LANE: &str = "host:team:code";
fn at(seconds: f64) -> String { crate::drift::format_timestamp(END + seconds) }
fn config() -> Config {
    Config { version: 1, policy_id: "ops-policy".into(),
        scope: Scope { teams: vec![Selector { id: "independent-team".into(),label: "Team".into() }],
            hosts: vec![Selector { id: "host".into(),label: "Host".into() }],
            lanes: vec![Selector { id: LANE.into(),label: "Code".into() }],..Default::default() },
        freshness_seconds: [("fixture".into(),60),("scheduler:host".into(),60)].into(),
        clock_tolerance_seconds: 2,
        parameters: Parameters { loop_repeat_count: Some(2),loop_lookback_seconds: Some(3600),..Default::default() },
        policies: Vec::new(), classes: Vec::new() }
}
fn operations() -> OperationalConfig {
    OperationalConfig { disk_reserve_bytes: [("host".into(),1000)].into(),
        filesystems: [("host".into(),"fs-native-id".into())].into(),..Default::default() }
}
fn row(id: &str, entity: &str, time: f64, fact: OperationFact) -> Observation {
    Observation { event_id: id.into(),entity_id: entity.into(),event_at: Some(at(time)),
        observed_at: Some(at(0.0)),sequence: Some(1),scope: config().scope,complete: true,
        source_ref: format!("fixture:{id}"),attribution: Some(Attribution { actor: "audit-tool".into(),
            classifier_version: "audit-v1".into(),offset_start: 0,offset_end: 10 }),fact: Fact::Operation(fact) }
}
fn batch(rows: Vec<Observation>) -> Evidence {
    Evidence { source: "fixture".into(),observed_at: Some(at(0.0)),
        complete: OPERATIONAL_FAMILIES.iter().map(|s| (*s).into()).collect(),
        reasons: BTreeMap::new(),observations: rows }
}
fn collect(config: &Config, evidence: &[Evidence]) -> (BTreeMap<String,Measurement>,LoopReport) {
    collect_operations(config,&operations(),Frame { now: END,window_secs: 3600,end_exclusive: false },evidence)
}
fn measure(rows: Vec<Observation>) -> BTreeMap<String,Measurement> {
    collect(&config(),&[batch(rows)]).0
}
fn host() -> OperationFact {
    OperationFact::Host(HostFact { host: "host".into(),capacity_units: Some(3),used_units: Some(4),
        load_one: Some(6.0),disk_available_bytes: Some(800),filesystem: Some("fs-native-id".into()),source_error: None,
        sample_observed_at: Some(at(0.0)) })
}
fn scalar(metrics: &BTreeMap<String,Measurement>,id: &str) -> f64 {
    match metrics[id].value.as_ref().expect(id) { Value::Scalar(n) => *n,other => panic!("{id}: {other:?}") }
}
fn keyed(metrics: &BTreeMap<String,Measurement>,id: &str,key: &str) -> f64 {
    match metrics[id].value.as_ref().expect(id) { Value::Keyed(rows) => rows[key],other => panic!("{id}: {other:?}") }
}
fn duration(metrics: &BTreeMap<String,Measurement>,id: &str,key: &str) -> Distribution {
    match metrics[id].value.as_ref().expect(id) { Value::Durations(rows) => rows[key].clone(),other => panic!("{id}: {other:?}") }
}
fn request(phase: RequestPhase,enqueue: f64,start: Option<f64>,end: Option<f64>) -> OperationFact {
    OperationFact::Request(RequestFact { host: "host".into(),lane: LANE.into(),phase,
        enqueued_at: at(enqueue),started_at: start.map(at),ended_at: end.map(at),
        wait_seconds: start.map_or(-enqueue,|s| s-enqueue),hold_seconds: start.map(|s| end.unwrap_or(0.0)-s) })
}
fn relay(id: &str,transition: RelayKind) -> OperationFact {
    OperationFact::Relay(RelayFact { message_id: id.into(),lane: LANE.into(),transition,order: true })
}
fn order(id: &str,previous: Option<&str>,direction: &str,revision: &str) -> OperationFact {
    OperationFact::Order(OrderFact { order_id: id.into(),lane: LANE.into(),action_key: Some("build-admission".into()),
        direction: Some(direction.into()),reverses_order_id: previous.map(str::to_owned),
        supersedes_order_id: previous.map(str::to_owned),evidence_revision: Some(revision.into()) })
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn capacity_load_and_signed_disk_headroom_use_configured_native_hosts() {
    for (team,capacity,used) in [("healthy",5,2),("problem",3,4)] {
        let mut c = config(); c.scope.teams[0].id = team.into();
        let mut fact = host();
        if let OperationFact::Host(f) = &mut fact { f.capacity_units = Some(capacity); f.used_units = Some(used); }
        let evidence = batch(vec![row("host","host",0.0,fact)]);
        let metrics = collect(&c,std::slice::from_ref(&evidence)).0;
        assert_eq!(keyed(&metrics,"host_slots_used","host"),used as f64);
        assert_eq!(keyed(&metrics,"host_slots_free","host"),capacity.saturating_sub(used) as f64);
        assert_eq!(keyed(&metrics,"host_overcommit_units","host"),used.saturating_sub(capacity) as f64);
        assert_eq!(keyed(&metrics,"host_load_per_slot","host"),6.0/capacity as f64);
        assert_eq!(keyed(&metrics,"disk_available_bytes","host"),800.0);
        assert_eq!(keyed(&metrics,"disk_headroom_bytes","host"),-200.0);
        assert_eq!(collect(&c,std::slice::from_ref(&evidence)).0,metrics);
    }
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn missing_remote_host_capacity_reserve_and_samples_do_not_fabricate_zero() {
    let mut c = config();
    c.scope.hosts.push(Selector { id: "remote".into(),label: "Remote".into() });
    let metrics = collect(&c,&[batch(vec![row("host","host",0.0,host())])]).0;
    assert!(metrics["host_slots_used"].reason.as_deref().unwrap().contains("remote"));
    let mut f = host();
    if let OperationFact::Host(f) = &mut f { f.capacity_units = None; }
    let metrics = measure(vec![row("host","host",0.0,f)]);
    assert_eq!(metrics["host_slots_free"].value,None);
    assert_eq!(keyed(&metrics,"disk_available_bytes","host"),800.0);
    let mut f = host();
    if let OperationFact::Host(f) = &mut f { f.capacity_units = Some(0); f.source_error = Some("sample read failed".into()); }
    let metrics = measure(vec![row("host","host",0.0,f)]);
    assert_eq!(metrics["host_load_per_slot"].status,Status::Unavailable);
    assert_eq!(metrics["disk_headroom_bytes"].reason.as_deref(),Some("sample read failed"));
    assert_eq!(keyed(&metrics,"host_slots_used","host"),4.0);
    let (metrics,_) = collect_operations(&config(),&OperationalConfig::default(),
        Frame { now: END,window_secs: 3600,end_exclusive: false },&[batch(vec![row("h","host",0.0,host())])]);
    assert!(metrics["disk_headroom_bytes"].reason.as_deref().unwrap().contains("reserve"));
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn scheduler_distributions_separate_completed_samples_from_current_ages_and_window_start() {
    let metrics = measure(vec![
        row("queued","q",0.0,request(RequestPhase::Queued,-420.0,None,None)),
        row("running","r",0.0,request(RequestPhase::Running,-720.0,Some(-600.0),None)),
        row("finished","f",0.0,request(RequestPhase::Succeeded,-1200.0,Some(-1080.0),Some(-480.0))),
        row("excluded","x",0.0,request(RequestPhase::Succeeded,-4800.0,Some(-4200.0),Some(-3600.0))),
    ]);
    assert_eq!(duration(&metrics,"queue_wait_minutes","host/queued_age"),Distribution { count: 1,median: 7.0,p90: 7.0 });
    assert_eq!(duration(&metrics,"queue_wait_minutes","host/started_wait").count,2);
    assert_eq!(duration(&metrics,"lock_hold_minutes","host/running_age").median,10.0);
    assert_eq!(duration(&metrics,"lock_hold_minutes","host/finished_hold"),Distribution { count: 1,median: 10.0,p90: 10.0 });
    assert_eq!(measure(Vec::new())["queue_wait_minutes"].status,Status::Unavailable);
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn partial_stale_missing_and_future_sources_invalidate_dependent_measurements() {
    let original = batch(vec![row("host","host",0.0,host())]);
    for observed in [None,Some(at(-61.0)),Some(at(3.0))] {
        let mut evidence = original.clone(); evidence.observed_at = observed;
        assert_eq!(collect(&config(),&[evidence]).0["host_slots_used"].value,None);
    }
    let mut partial = original.clone(); partial.complete.remove("host");
    assert_eq!(collect(&config(),&[partial]).0["host_slots_used"].value,None);
    let mut stale_row = original.clone(); stale_row.observations[0].observed_at = Some(at(-61.0));
    assert_eq!(collect(&config(),&[stale_row]).0["host_slots_used"].value,None);
    let mut error = Evidence { source: "remote".into(),..Default::default() };
    error.reasons.insert("host".into(),"remote host disconnected".into());
    assert!(collect(&config(),&[original,error]).0["host_slots_used"].reason.as_deref().unwrap().contains("disconnected"));
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn relay_enqueue_delivery_ack_and_redelivery_are_distinct_original_message_counts() {
    let mut delivered = row("d1","message",-120.0,relay("m1",RelayKind::Delivered));
    delivered.sequence = Some(2);
    let metrics = measure(vec![
        row("q1","message",-180.0,relay("m1",RelayKind::Queued)),delivered,
        row("redelivery","message",-60.0,relay("m1",RelayKind::Delivered)),
        row("q2","second",-60.0,relay("m2",RelayKind::Queued)),
        row("timeout","second",-30.0,relay("m2",RelayKind::PromptTimeout)),
    ]);
    assert_eq!(scalar(&metrics,"relay_queued"),2.0);
    assert_eq!(scalar(&metrics,"relay_delivered"),1.0);
    assert_eq!(scalar(&metrics,"relay_acknowledged"),0.0);
    assert_eq!(scalar(&metrics,"prompt_timeout_unconfirmed"),1.0);
    assert_eq!(duration(&metrics,"order_unacked_age_minutes",&format!("{LANE}/since_delivery")).median,2.0);
    assert_eq!(duration(&metrics,"order_unacked_age_minutes",&format!("{LANE}/since_enqueue_delivery_unknown")).median,1.0);
    let metrics = measure(vec![row("q","a",-60.0,relay("m",RelayKind::Queued)),
        row("t","a",-45.0,relay("m",RelayKind::PromptTimeout)),
        row("a","a",-30.0,relay("m",RelayKind::Acknowledged))]);
    assert_eq!(scalar(&metrics,"prompt_timeout_unconfirmed"),0.0);
    assert_eq!(metrics["order_unacked_age_minutes"].value,None);
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn denial_duration_unions_overlaps_intersects_window_and_requires_attribution() {
    let denial = |order: &str,start: f64,end: Option<f64>| OperationFact::Denial(DenialFact {
        lane: LANE.into(),order_id: order.into(),denied_at: at(start),resolved_at: end.map(at) });
    let rows = vec![row("d1","d1",-4000.0,denial("o1",-4000.0,Some(-1800.0))),
        row("d2","d2",-2000.0,denial("o2",-2000.0,Some(-900.0))),
        row("d3","d3",-600.0,denial("o3",-600.0,None))];
    let metrics = measure(rows.clone());
    assert_eq!(keyed(&metrics,"refusals_by_lane",LANE),2.0);
    assert_eq!(keyed(&metrics,"frozen_denial_hours",LANE),3300.0/3600.0);
    let mut missing = rows; missing[0].attribution = None;
    assert_eq!(measure(missing)["frozen_denial_hours"].status,Status::Unavailable);
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn movement_uses_latest_verified_input_and_missing_remote_lane_remains_unknown() {
    let signal = OperationFact::LaneSignal(LaneSignal { lane: LANE.into(),login_blocked: Some(true),
        movement_complete: true,movement_at: vec![at(-7200.0),at(-1800.0)],reasons: BTreeMap::new() });
    let rows = vec![row("lane",LANE,0.0,signal)];
    let metrics = measure(rows.clone());
    assert_eq!(scalar(&metrics,"login_blocked_lanes"),1.0);
    assert_eq!(keyed(&metrics,"lane_no_movement_hours",LANE),0.5);
    let mut c = config(); c.scope.lanes.push(Selector { id: "remote:team:code".into(),label: "Remote".into() });
    assert!(collect(&c,&[batch(rows)]).0["lane_no_movement_hours"].reason.as_deref().unwrap().contains("remote"));
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn reconciliation_preserves_missing_side_and_refuses_skew_or_zero_denominator() {
    let pair = |tool: Option<&str>,truth: Option<&str>,offset: f64| OperationFact::Reconciliation(ReconciliationFact {
        tool_value: tool.map(str::to_owned),truth_value: truth.map(str::to_owned),tool_at: Some(at(0.0)),truth_at: Some(at(offset)) });
    let metrics = measure(vec![row("one","one",0.0,pair(Some("WORKING"),Some("WORKING"),0.0)),
        row("two","two",0.0,pair(Some("WORKING"),Some("IDLE"),0.0)),
        row("three","three",0.0,pair(Some("WORKING"),None,0.0))]);
    assert_eq!(scalar(&metrics,"reconciliation_agreement_ratio"),0.5);
    assert_eq!(metrics["reconciliation_agreement_ratio"].denominator,Some(2));
    assert_eq!(scalar(&metrics,"reconciliation_unpaired"),1.0);
    assert_eq!(measure(Vec::new())["reconciliation_agreement_ratio"].value,None);
    assert!(measure(vec![row("skew","s",0.0,pair(Some("a"),Some("a"),-3.0))])["reconciliation_agreement_ratio"]
        .reason.as_deref().unwrap().contains("skew"));
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn installed_instruction_drift_and_unknown_release_are_distinct() {
    let instruction = |actual: Option<&str>| OperationFact::Instruction(InstructionFact { lane: LANE.into(),
        component: "coder".into(),claimed: Some("0.4.0".into()),actual: actual.map(str::to_owned) });
    assert_eq!(scalar(&measure(vec![row("i","coder",0.0,instruction(Some("0.4.1")))]),"instruction_version_mismatches"),1.0);
    assert_eq!(measure(vec![row("i","coder",0.0,instruction(None))])["instruction_version_mismatches"].value,None);
}

#[trace("FR-029-AC-2", "FR-029-AC-4")]
#[test]
fn attributed_messages_deduplicate_redelivery_and_unknown_semantics_do_not_become_healthy() {
    let message = |id: &str,correction,status| OperationFact::Message(MessageFact { message_id: id.into(),
        sender: LANE.into(),correction,status_only: status });
    let mut rows = vec![row("m1","m1",-100.0,message("m1",Some(true),Some(false))),
        row("m2","m2",-50.0,message("m2",Some(false),Some(true))),
        row("repeat","m1",-20.0,message("m1",Some(true),Some(false)))];
    let metrics = measure(rows.clone());
    assert_eq!(keyed(&metrics,"messages_per_sender",LANE),2.0);
    assert_eq!(scalar(&metrics,"correction_rate"),0.5);
    assert_eq!(metrics["correction_rate"].denominator,Some(2));
    assert_eq!(metrics["correction_rate"].classification_coverage,Some(ClassificationCoverage {
        classified: 2,total: 2,basis: CoverageBasis::CompletePopulation }));
    assert_eq!(scalar(&metrics,"status_only_messages"),1.0);
    rows[0].attribution = None;
    let metrics = measure(rows);
    assert_eq!(scalar(&metrics,"correction_rate"),0.0);
    assert_eq!(metrics["correction_rate"].classification_coverage,Some(ClassificationCoverage {
        classified: 1,total: 2,basis: CoverageBasis::LabeledSubset }));
    assert_eq!(keyed(&metrics,"messages_per_sender",LANE),2.0);
}

#[trace("FR-029-AC-3", "FR-029-AC-9")]
#[test]
fn unknown_message_population_never_claims_classification_coverage_or_rate() {
    let mut evidence = batch(vec![row("m","m",0.0,OperationFact::Message(MessageFact {
        message_id: "m".into(),sender: LANE.into(),correction: Some(false),status_only: Some(false) }))]);
    evidence.complete.remove("message");
    let metrics = collect(&config(),&[evidence]).0;
    assert_eq!(metrics["correction_rate"].value,None);
    assert_eq!(metrics["correction_rate"].classification_coverage,None);
}

#[trace("FR-029-AC-2", "FR-029-AC-5", "FR-029-AC-6")]
#[test]
fn independent_teams_assess_native_host_rows_as_healthy_or_overcommitted_without_combined_score() {
    use crate::retro::department::{Basis,Comparator,KeyReduction,Policy,Severity,Statistic,assess_metric};
    let policy = Policy { metric_id: "host_overcommit_units".into(),statistic: Statistic::Value,
        unit: "units".into(),comparator: Comparator::Gte,amber: 1.0,red: 2.0,basis: Basis::Current,
        window_seconds: 3600,freshness_seconds: 60,expected_keys: ["host".into()].into(),
        key_reduction: Some(KeyReduction::WorstSeverity) };
    for (team,capacity,used,severity) in [("team-alpha",5,2,Severity::Green),("team-beta",2,4,Severity::Red)] {
        let mut c = config(); c.scope.teams[0].id = team.into();
        let mut f = host();
        if let OperationFact::Host(f) = &mut f { f.capacity_units = Some(capacity); f.used_units = Some(used); }
        let metrics = collect(&c,&[batch(vec![row("h","host",0.0,f)])]).0;
        let assessment = assess_metric(Some(&metrics["host_overcommit_units"]),Some(&policy),None);
        assert_eq!(assessment.summary.severity,severity);
        assert!(assessment.summary.complete);
    }
    let mut c = config();
    c.scope.hosts.push(Selector { id: "remote".into(),label: "Remote".into() });
    let metrics = collect(&c,&[batch(vec![row("h","host",0.0,host())])]).0;
    let assessment = assess_metric(Some(&metrics["host_overcommit_units"]),Some(&policy),None);
    assert_eq!(assessment.summary.severity,Severity::Unknown);
    assert!(!assessment.summary.complete);
}

#[trace("FR-029-AC-2", "FR-029-AC-9")]
#[test]
fn unrelated_host_source_does_not_contaminate_configured_host_population() {
    let mut unrelated = host();
    if let OperationFact::Host(f) = &mut unrelated { f.host = "unselected".into(); }
    let mut stale = batch(vec![row("remote","unselected",0.0,unrelated)]);
    stale.observed_at = None;
    let metrics = collect(&config(),&[batch(vec![row("local","host",0.0,host())]),stale]).0;
    assert_eq!(keyed(&metrics,"host_slots_used","host"),4.0);
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn explicit_order_reversals_have_exact_evidence_and_changed_authority_suppresses_pairs() {
    let mut rows = vec![row("first","o1",-300.0,order("o1",None,"go","e1")),
        row("second","o2",-200.0,order("o2",Some("o1"),"stop","e1")),
        row("third","o3",-100.0,order("o3",Some("o2"),"go","e1"))];
    rows.push(rows[1].clone());
    let (_,report) = collect(&config(),&[batch(rows.clone())]);
    assert_eq!(report.status,Status::Ok);
    assert_eq!(report.findings[0].event_ids,["first","second","third"]);
    if let Fact::Operation(OperationFact::Order(f)) = &mut rows[2].fact { f.evidence_revision = Some("new-authority".into()); }
    assert!(collect(&config(),&[batch(rows)]).1.findings.is_empty());
    let mut missing = row("unknown","unknown",0.0,order("o",None,"go","e1")); missing.attribution = None;
    assert_eq!(collect(&config(),&[batch(vec![missing])]).1.status,Status::Unavailable);
}

#[trace("FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn repeated_order_counts_sends_after_first_only_for_unchanged_evidence() {
    let metrics = measure(vec![row("first","o1",-4000.0,order("o1",None,"go","e1")),
        row("second","o1",-100.0,order("o1",None,"go","e1")),
        row("new","o1",-50.0,order("o1",None,"go","e2"))]);
    assert_eq!(scalar(&metrics,"repeated_orders"),1.0);
}

#[trace("FR-029-AC-4")]
#[test]
fn planner_plan_scope_and_tick_audits_count_only_attributed_actionable_sends() {
    let assignment = |id: &str,in_plan,in_scope,new_evidence| OperationFact::Assignment(AssignmentFact {
        message_id: id.into(),lane: LANE.into(),tick_id: "tick-1".into(),plan_revision: Some("wave-2".into()),
        in_plan: Some(in_plan),in_scope: Some(in_scope),evidence_checked: Some(true),prior_orders_checked: Some(true),
        end_tick_audit_ref: Some("audit:tick-1".into()),new_blocking_evidence: Some(new_evidence) });
    let rows = vec![row("one","one",-300.0,assignment("one",true,true,false)),
        row("two","two",-200.0,assignment("two",false,false,false)),
        row("three","three",-100.0,assignment("three",true,true,true))];
    let metrics = measure(rows.clone());
    assert_eq!(scalar(&metrics,"planner_ad_hoc_assignments"),1.0);
    assert_eq!(scalar(&metrics,"planner_scope_violations"),1.0);
    assert_eq!(scalar(&metrics,"planner_unbatched_sends"),1.0);
    let mut missing = rows;
    if let Fact::Operation(OperationFact::Assignment(f)) = &mut missing[0].fact { f.end_tick_audit_ref = None; }
    assert_eq!(measure(missing)["planner_scope_violations"].status,Status::Unavailable);
}

#[trace("FR-029-AC-10")]
#[test]
fn operational_file_adapter_reads_actual_normalized_exports_and_retains_read_errors() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("operational.json");
    let original = batch(vec![row("host","host",0.0,host())]);
    std::fs::write(&path,serde_json::to_vec(&original).unwrap()).unwrap();
    let mut op = operations(); op.evidence_files.push(path);
    let evidence = read_operational_files(&op,&config().scope);
    assert_eq!(evidence,[original]);
    assert_eq!(keyed(&collect(&config(),&evidence).0,"host_slots_used","host"),4.0);
    op.evidence_files.push(directory.path().join("missing.json"));
    assert_eq!(collect(&config(),&read_operational_files(&op,&config().scope)).0["host_slots_used"].value,None);
}

#[trace("FR-029-AC-4", "FR-029-AC-9", "FR-029-AC-10")]
#[test]
fn released_scheduler_status_export_is_consumed_without_reading_private_state_or_running_a_command() {
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
    std::fs::write(&path,serde_json::to_vec(&export).unwrap()).unwrap();
    let mut op = operations(); op.scheduler_files.insert("host".into(),path.clone());
    let run = || collect_operations(&config(),&op,Frame { now: END,window_secs: 3600,end_exclusive: false },
        &read_operational_files(&op,&config().scope)).0;
    let metrics = run();
    assert_eq!(keyed(&metrics,"host_slots_used","host"),1.0);
    assert_eq!(keyed(&metrics,"host_load_per_slot","host"),0.5);
    assert_eq!(keyed(&metrics,"disk_headroom_bytes","host"),2000.0);
    assert_eq!(duration(&metrics,"queue_wait_minutes","host/started_wait").median,2.0);
    assert_eq!(duration(&metrics,"lock_hold_minutes","host/running_age").median,10.0);
    export["sample"]["observed_at_ms"] = serde_json::json!(1799999939000_u64);
    std::fs::write(&path,serde_json::to_vec(&export).unwrap()).unwrap();
    let metrics = run();
    assert_eq!(metrics["host_load_per_slot"].status,Status::Unavailable);
    assert_eq!(keyed(&metrics,"host_slots_used","host"),1.0);
    export["pruned_events"] = serde_json::json!(1);
    std::fs::write(&path,serde_json::to_vec(&export).unwrap()).unwrap();
    assert_eq!(run()["queue_wait_minutes"].status,Status::Unavailable);
    export["schema_version"] = serde_json::json!(2);
    std::fs::write(&path,serde_json::to_vec(&export).unwrap()).unwrap();
    assert!(run()["host_slots_used"].reason.as_deref().unwrap().contains("schema_version"));
}
