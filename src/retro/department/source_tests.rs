// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Source-boundary oracles: native HTTP projections and configured files enter production normalizers.
use super::*;
use crate::retro::sources::{GithubSource, MainCommit, PullRequest, PullRequestInfo};
use crate::retro::{Frame, Status};
use ix_trace_rs::trace;
use std::collections::{BTreeMap, BTreeSet};
const END: f64 = 1_800_000_000.0;
const EXPORT: &str = include_str!("../../../tests/fixtures/retro/source-events.json");
fn at(offset: f64) -> String {
    crate::drift::format_timestamp(END + offset)
}
fn config() -> Config {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/retro/department-config.json"
    ))
    .unwrap()
}
fn native_pr() -> PullRequest {
    PullRequest {
        repo: "fixture/repo".into(),
        closed_at: None,
        reopen_events: None,
        number: 1,
        open: false,
        draft: false,
        created_at: END - 100000.0,
        merged_at: Some(END),
        updated_at: END,
        additions: 1,
        deletions: 1,
        review_decision: None,
        force_pushes: Some(0),
    }
}
fn http(endpoint: &str, projection: &str) -> Result<String, String> {
    match projection{
        PR_DETAILS=>Ok(serde_json::json!({"base_sha":"a".repeat(40),"head_sha":"b".repeat(40),"requested_reviewers":0,"requested_teams":0}).to_string()),
        PR_REVIEWS=>Ok("[]".into()),
        PR_TIMELINE=>Ok(serde_json::json!([{"id":10,"event":"ready_for_review","created_at":at(-25200.0)}]).to_string()),
        PR_FILES=>Ok("[{\"filename\":\"Cargo.toml\",\"status\":\"modified\"}]".into()),
        RAW_CONTENT=>{let rev=if endpoint.contains(&"a".repeat(40)){"a".repeat(40)}else{"b".repeat(40)};Ok(format!("[package]\nname='fixture'\nversion='1.0.0'\n[dependencies]\ndep={{git='https://github.com/fixture/dependency',rev='{rev}'}}\n"))},
        other=>Err(format!("unexpected projection {other}"))
    }
}
struct MergeFixture;
impl GithubSource for MergeFixture {
    fn pull_requests(&self, _: &str, _: f64) -> Result<Vec<PullRequest>, String> {
        Err("unused native listing".into())
    }
    fn pull_request(&self, repo: &str, number: u64) -> Result<PullRequestInfo, String> {
        assert_eq!((repo, number), ("fixture/repo", 1));
        Ok(PullRequestInfo {
            merged: true,
            updated_at: END,
        })
    }
    fn main_commits(&self, _: &str, _: &str, _: f64) -> Result<Vec<MainCommit>, String> {
        Err("unused checks".into())
    }
}
fn native_batches(c: &Config, pr: &PullRequest) -> Vec<Evidence> {
    let workspace: crate::WorkspaceSnapshot = serde_json::from_str(include_str!(
        "../../../tests/fixtures/retro/native-workspace.json"
    ))
    .unwrap();
    vec![
        workspace_evidence(&workspace, &c.scope),
        Evidence {
            source: "github".into(),
            observed_at: Some(at(0.0)),
            complete: ["pull_request".into()].into(),
            observations: vec![collect_pull_request(c, pr, &at(0.0), http)],
            ..Default::default()
        },
    ]
}
fn report(c: &Config, body: &str, pr: &PullRequest) -> Report {
    let mut batches = native_batches(c, pr);
    enrich_merged_attachments(c, &mut batches, &MergeFixture);
    read_source_logs(
        c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &mut batches,
        |path| {
            assert_eq!(path, c.parameters.event_logs[0].path);
            Ok(body.into())
        },
    );
    collect(
        c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &batches,
        &BTreeMap::new(),
    )
}
fn scalar(r: &Report, id: &str) -> f64 {
    let Some(Value::Scalar(v)) = &r.measurements[id].value else {
        panic!("{id}: {:?}", r.measurements[id])
    };
    *v
}

#[trace("FR-029-AC-3", "FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn native_http_and_source_export_make_required_collectors_available_with_exact_values() {
    let c = config();
    assert_eq!(c.validate(), Ok(()));
    let r = report(&c, EXPORT, &native_pr());
    for (id, value) in [
        ("claims_without_movement", 1.0),
        ("merged_open", 1.0),
        ("oldest_gated_hours", 1.0),
        ("idle_with_ready", 1.0),
        ("repin_only_prs", 1.0),
        ("tag_repin_events", 2.0),
        ("focused_check_runs", 1.0),
        ("workspace_check_runs", 1.0),
    ] {
        assert_eq!(scalar(&r, id), value, "{id}");
    }
    assert_eq!(
        r.measurements["gates_per_pr"].value,
        Some(Value::Keyed([("fixture/repo#1".into(), 1.0)].into()))
    );
    assert_eq!(
        r.loops["tag_repin_loop"].findings[0].event_ids,
        ["tag-1", "repin-1", "tag-2", "repin-2"]
    );
    assert!(
        r.measurements["claims_without_movement"]
            .evidence_refs
            .contains(&"native-export:claim".into())
    );
    let mut open = native_pr();
    open.open = true;
    open.merged_at = None;
    let r = report(&c, EXPORT, &open);
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
}

#[trace("FR-029-AC-3", "FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn source_export_missing_partial_stale_or_unattributed_evidence_is_isolated() {
    let c = config();
    let original: serde_json::Value = serde_json::from_str(EXPORT).unwrap();
    for which in [
        "partial-movement",
        "missing-channel",
        "missing-attribution",
        "stale",
        "bad-version",
    ] {
        let mut changed = original.clone();
        match which {
            "partial-movement" => changed["coverage"][1]["complete"] = false.into(),
            "missing-channel" => {
                changed["coverage"][1]["movement_sources"] =
                    serde_json::json!(["pr_head", "commit"])
            }
            "missing-attribution" => changed["events"][8]["attribution"] = serde_json::Value::Null,
            "stale" => changed["observed_at"] = at(-61.0).into(),
            "bad-version" => changed["version"] = 99.into(),
            _ => unreachable!(),
        }
        let r = report(&c, &changed.to_string(), &native_pr());
        assert_eq!(scalar(&r, "wip_active"), 1.0);
        if which == "missing-attribution" {
            assert_eq!(
                r.measurements["focused_check_runs"].status,
                Status::Unavailable
            );
            assert_eq!(scalar(&r, "merged_open"), 1.0);
        } else {
            assert_eq!(
                r.measurements["claims_without_movement"].status,
                Status::Unavailable,
                "{which}"
            );
        }
    }
    let mut batches = native_batches(&c, &native_pr());
    read_source_logs(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &mut batches,
        |_| Err("fixture read permission denied".into()),
    );
    let r = collect(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &batches,
        &BTreeMap::new(),
    );
    assert!(
        r.measurements["claims_without_movement"]
            .reason
            .as_ref()
            .unwrap()
            .contains("permission denied")
    );
    assert_eq!(scalar(&r, "wip_active"), 1.0);
}

#[trace("FR-029-AC-3", "FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn native_review_pagination_and_diff_http_failures_preserve_independent_attributes() {
    let mut c = config();
    c.parameters.event_logs.clear();
    let mut pr = native_pr();
    pr.open = true;
    pr.merged_at = None;
    let mut calls = Vec::new();
    let observation = collect_pull_request(&c, &pr, &at(0.0), |endpoint, projection| {
        calls.push(endpoint.to_owned());
        if projection == PR_REVIEWS && endpoint.ends_with("page=1") {
            return Ok(serde_json::to_string(&(0..100).map(|id|serde_json::json!({"id":id,"state":"COMMENTED","submitted_at":at(-1.0)})).collect::<Vec<_>>()).unwrap());
        }
        http(endpoint, projection)
    });
    let Fact::PullRequest(p) = &observation.fact else {
        panic!("native PR")
    };
    assert_eq!(p.qualifying_reviews, Some(0));
    assert_eq!(p.repin_only, Some(true));
    assert!(
        calls
            .iter()
            .any(|s| s.contains("/reviews?") && s.ends_with("page=2"))
    );
    for failure in ["second-page", "missing-ready", "files"] {
        let observation = collect_pull_request(&c, &pr, &at(0.0), |endpoint, projection| {
            if projection == PR_REVIEWS && failure == "second-page" {
                if endpoint.ends_with("page=1") {
                    return Ok(serde_json::to_string(&(0..100).map(|id|serde_json::json!({"id":id,"state":"COMMENTED","submitted_at":at(-1.0)})).collect::<Vec<_>>()).unwrap());
                }
                return Err("denied second page".into());
            }
            if projection == PR_TIMELINE && failure == "missing-ready" {
                return Ok("[]".into());
            }
            if projection == PR_FILES && failure == "files" {
                return Err("files endpoint unavailable".into());
            }
            http(endpoint, projection)
        });
        let Fact::PullRequest(p) = &observation.fact else {
            panic!("native PR")
        };
        match failure {
            "second-page" => {
                assert_eq!(p.qualifying_reviews, None);
                assert_eq!(p.repin_only, Some(true));
            }
            "missing-ready" => {
                assert_eq!(p.review_ready_at, None);
                assert_eq!(p.assigned_reviewers, Some(0));
            }
            "files" => {
                assert_eq!(p.repin_only, None);
                assert_eq!(p.qualifying_reviews, Some(0));
            }
            _ => unreachable!(),
        }
    }
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn native_claim_restart_and_attachment_truncation_never_use_update_time_as_claim() {
    use crate::WorkflowStateType::{Started, Unstarted};
    use crate::retro::sources::StateChange;
    let c = config();
    let mut batches = native_batches(&c, &native_pr());
    let row = &mut batches[0].observations[0];
    let history = [
        StateChange {
            at: END - 7200.0,
            from: Some(Unstarted),
            to: Some(Started),
        },
        StateChange {
            at: END - 20.0,
            from: Some(Started),
            to: Some(Started),
        },
        StateChange {
            at: END - 10.0,
            from: Some(Unstarted),
            to: Some(Started),
        },
    ];
    enrich_claim_history(row, Ok(&history), Some(&at(0.0)));
    let Fact::Issue(i) = &row.fact else {
        panic!("native issue")
    };
    assert_eq!(i.claim_at.as_deref(), Some(at(-10.0).as_str()));
    if let Fact::Issue(i) = &mut row.fact {
        i.attachments_complete = false;
    }
    enrich_merged_attachments(&c, &mut batches, &MergeFixture);
    let Fact::Issue(i) = &batches[0].observations[0].fact else {
        panic!("native issue")
    };
    assert_eq!(i.merged_pr, None);
    assert!(!i.provenance["merged_pr"].complete);
    let mut ambiguous = history.to_vec();
    ambiguous.push(StateChange {
        at: END - 10.0,
        from: Some(Started),
        to: Some(Unstarted),
    });
    enrich_claim_history(
        &mut batches[0].observations[0],
        Ok(&ambiguous),
        Some(&at(0.0)),
    );
    let Fact::Issue(i) = &batches[0].observations[0].fact else {
        panic!("native issue")
    };
    assert_eq!(i.claim_at, None);
}

#[trace("FR-029-AC-5", "FR-029-AC-7")]
#[test]
fn configured_source_exports_reject_overlapping_owners_and_normalize_input_order() {
    let mut c = config();
    let mut other = c.parameters.event_logs[0].clone();
    other.source = "other".into();
    other.kinds = [LogKind::Gates].into();
    c.parameters.event_logs[0].kinds.remove(&LogKind::Gates);
    c.parameters.event_logs.push(other);
    assert_eq!(c.validate(), Ok(()));
    let id = c.identity();
    c.parameters.event_logs.reverse();
    assert_eq!(c.identity(), id);
    c.parameters.event_logs[0].kinds.insert(LogKind::Claims);
    assert!(
        c.validate()
            .unwrap_err()
            .contains("duplicate capability owner")
    );
    let _ = BTreeSet::<String>::new();
}

#[trace("FR-029-AC-4", "FR-029-AC-9")]
#[test]
fn source_export_conflicts_and_impossible_watermarks_are_not_overrides() {
    let c = config();
    let original: serde_json::Value = serde_json::from_str(EXPORT).unwrap();
    let mut conflict = original.clone();
    conflict["events"][11]["record"]["repin_only"] = false.into();
    let r = report(&c, &conflict.to_string(), &native_pr());
    assert!(
        r.measurements["repin_only_prs"]
            .reason
            .as_ref()
            .unwrap()
            .contains("conflicting native/exported")
    );
    assert_eq!(scalar(&r, "merged_open"), 1.0);
    let mut watermark = original;
    watermark["observed_at"] = at(-30.0).into();
    let r = report(&c, &watermark.to_string(), &native_pr());
    assert_eq!(
        r.measurements["claims_without_movement"].status,
        Status::Unavailable
    );
    assert_eq!(scalar(&r, "wip_active"), 1.0);
}

#[trace("FR-029-AC-4")]
#[test]
fn complete_reviewer_markers_join_repository_identity_and_incomplete_rows_remain_unknown() {
    let c = config();
    let mut other = native_pr();
    other.repo = "second/repo".into();
    let mut batches = native_batches(&c, &native_pr());
    batches[1]
        .observations
        .push(collect_pull_request(&c, &other, &at(0.0), http));
    let lines = vec![
        "<!-- reviewer repo=fixture/repo id=SR-1 pr=repo#1 -->".into(),
        "<!-- reviewer-dispositions repo=fixture/repo id=SR-1 round=2 -->".into(),
        "<!-- reviewer repo=second/repo id=SR-1 pr=repo#1 -->".into(),
        "<!-- reviewer-dispositions repo=second/repo id=SR-1 round=1 -->".into(),
    ];
    enrich_reviewer_rounds(
        &mut batches,
        &lines,
        &["fixture/repo".into(), "second/repo".into()],
    );
    for (row, expected) in batches[1].observations.iter().zip([Some(2), Some(1)]) {
        let Fact::PullRequest(p) = &row.fact else {
            panic!("PR")
        };
        assert_eq!(p.fix_rounds, expected);
    }
    let mut incomplete = lines;
    incomplete[1] = "<!-- reviewer-dispositions repo=fixture/repo id=SR-1 -->".into();
    enrich_reviewer_rounds(
        &mut batches,
        &incomplete,
        &["fixture/repo".into(), "second/repo".into()],
    );
    let Fact::PullRequest(p) = &batches[1].observations[0].fact else {
        panic!("PR")
    };
    assert_eq!(p.fix_rounds, None);
    let Fact::PullRequest(p) = &batches[1].observations[1].fact else {
        panic!("PR")
    };
    assert_eq!(p.fix_rounds, Some(1));
}

#[trace("FR-029-AC-3", "FR-029-AC-4")]
#[test]
fn documented_source_input_is_read_by_the_production_file_normalizer() {
    let doc = include_str!("../../../docs/retro.md");
    let examples: Vec<&str> = doc
        .split("```json\n")
        .skip(1)
        .filter_map(|block| block.split_once("```").map(|(body, _)| body))
        .collect();
    let parameters: Parameters = examples
        .iter()
        .filter_map(|body| serde_json::from_str::<Parameters>(body).ok())
        .find(|p| !p.event_logs.is_empty())
        .expect("documented event-log parameters");
    let body = examples
        .iter()
        .find(|body| serde_json::from_str::<SourceLog>(body).is_ok())
        .expect("documented source export");
    let mut c = config();
    c.parameters.event_logs = parameters.event_logs;
    c.freshness_seconds.insert("build-audit".into(), 60);
    let mut batches = native_batches(&c, &native_pr());
    batches[0].observations[0].entity_id = "issue-uuid".into();
    let history = [crate::retro::sources::StateChange {
        at: END - 7200.0,
        from: Some(crate::WorkflowStateType::Unstarted),
        to: Some(crate::WorkflowStateType::Started),
    }];
    enrich_claim_history(
        &mut batches[0].observations[0],
        Ok(&history),
        Some(&at(0.0)),
    );
    read_source_logs(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &mut batches,
        |_| Ok((*body).into()),
    );
    let r = collect(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &batches,
        &BTreeMap::new(),
    );
    assert_eq!(scalar(&r, "claims_without_movement"), 1.0);
}

#[trace("FR-029-AC-3", "FR-029-AC-7", "FR-029-AC-9")]
#[test]
fn review_round2_claims_normalize_before_movement_in_any_input_or_name_order() {
    for (movement_path, claims_path, movement_source, claims_source) in [
        ("a-movement.json", "z-claims.json", "z-movement", "a-claims"),
        ("z-movement.json", "a-claims.json", "a-movement", "z-claims"),
    ] {
        let mut c = config();
        c.parameters.event_logs = vec![
            LogInput {
                path: movement_path.into(),
                source: movement_source.into(),
                kinds: [LogKind::Movement].into(),
            },
            LogInput {
                path: claims_path.into(),
                source: claims_source.into(),
                kinds: [LogKind::Claims].into(),
            },
        ];
        c.freshness_seconds.insert(movement_source.into(), 60);
        c.freshness_seconds.insert(claims_source.into(), 60);
        let claims = SourceLog {
            version: 1,
            source: claims_source.into(),
            observed_at: at(0.0),
            coverage: vec![Coverage {
                kind: LogKind::Claims,
                entities: ["issue-a".into()].into(),
                from: at(-300000.0),
                until: at(0.0),
                complete: true,
                full_history: true,
                movement_sources: Default::default(),
            }],
            events: vec![LogEvent {
                event_id: "claim-48h".into(),
                entity_id: "issue-a".into(),
                event_at: at(-172800.0),
                sequence: Some(1),
                source_ref: "native:claim-48h".into(),
                attribution: None,
                record: NativeRecord::Claim,
            }],
        };
        let mut movement = SourceLog {
            version: 1,
            source: movement_source.into(),
            observed_at: at(0.0),
            coverage: vec![Coverage {
                kind: LogKind::Movement,
                entities: ["issue-a".into()].into(),
                from: at(-86400.0),
                until: at(0.0),
                complete: true,
                full_history: false,
                movement_sources: [
                    MovementKind::PrHead,
                    MovementKind::Commit,
                    MovementKind::Worktree,
                ]
                .into(),
            }],
            events: Vec::new(),
        };
        let identity = c.identity();
        for complete_interval in [false, true] {
            if complete_interval {
                movement.coverage[0].from = at(-300000.0);
            }
            for reverse in [false, true] {
                let mut selected = c.clone();
                if reverse {
                    selected.parameters.event_logs.reverse();
                }
                assert_eq!(selected.identity(), identity);
                assert_eq!(selected.validate(), Ok(()));
                let mut workspace: crate::WorkspaceSnapshot = serde_json::from_str(include_str!(
                    "../../../tests/fixtures/retro/native-workspace.json"
                ))
                .unwrap();
                workspace.issues[0].created_at = Some(at(-300000.0));
                let mut batches = vec![workspace_evidence(&workspace, &selected.scope)];
                read_source_logs(
                    &selected,
                    Frame {
                        now: END,
                        window_secs: 86400,
                        end_exclusive: false,
                    },
                    &mut batches,
                    |path| {
                        Ok(
                            serde_json::to_string(if path == std::path::Path::new(movement_path) {
                                &movement
                            } else {
                                &claims
                            })
                            .unwrap(),
                        )
                    },
                );
                let report = collect(
                    &selected,
                    Frame {
                        now: END,
                        window_secs: 86400,
                        end_exclusive: false,
                    },
                    &batches,
                    &BTreeMap::new(),
                );
                if complete_interval {
                    assert_eq!(scalar(&report, "claims_without_movement"), 1.0);
                } else {
                    assert_eq!(
                        report.measurements["claims_without_movement"].status,
                        Status::Unavailable,
                        "{movement_path}/{claims_path}/reverse={reverse}"
                    );
                }
                let Fact::Issue(issue) = &batches[0].observations[0].fact else {
                    panic!("native issue")
                };
                assert_eq!(issue.claim_at.as_deref(), Some(at(-172800.0).as_str()));
                assert_eq!(issue.movement_complete, complete_interval);
                assert_eq!(scalar(&report, "wip_active"), 1.0);
            }
        }
    }
}

#[trace("FR-029-AC-3", "FR-029-AC-4", "FR-029-AC-10")]
#[test]
fn review_round2_gate_and_diff_exports_preserve_independent_field_attribution() {
    let mut c = config();
    c.parameters.event_logs[0].kinds = [LogKind::Gates].into();
    let mut gates: SourceLog = serde_json::from_str(EXPORT).unwrap();
    gates
        .events
        .retain(|e| matches!(e.record, NativeRecord::Gate { .. }));
    gates
        .coverage
        .retain(|coverage| coverage.kind == LogKind::Gates);
    let mut batches = native_batches(&c, &native_pr());
    let native_attribution = batches[1].observations[0].attribution.clone();
    assert_eq!(
        scalar(
            &collect(
                &c,
                Frame {
                    now: END,
                    window_secs: 86400,
                    end_exclusive: false
                },
                &batches,
                &BTreeMap::new()
            ),
            "repin_only_prs"
        ),
        1.0
    );
    read_source_logs(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &mut batches,
        |_| Ok(serde_json::to_string(&gates).unwrap()),
    );
    let report = collect(
        &c,
        Frame {
            now: END,
            window_secs: 86400,
            end_exclusive: false,
        },
        &batches,
        &BTreeMap::new(),
    );
    assert_eq!(scalar(&report, "repin_only_prs"), 1.0);
    assert_eq!(
        report.measurements["gates_per_pr"].value,
        Some(Value::Keyed([("fixture/repo#1".into(), 1.0)].into()))
    );
    assert_eq!(batches[1].observations[0].attribution, native_attribution);
    for reverse in [false, true] {
        let mut selected = c.clone();
        selected.parameters.event_logs = vec![
            LogInput {
                source: "gate-source".into(),
                path: "gate.json".into(),
                kinds: [LogKind::Gates].into(),
            },
            LogInput {
                source: "diff-source".into(),
                path: "diff.json".into(),
                kinds: [LogKind::Diffs].into(),
            },
        ];
        if reverse {
            selected.parameters.event_logs.reverse();
        }
        selected.freshness_seconds.insert("gate-source".into(), 60);
        selected.freshness_seconds.insert("diff-source".into(), 60);
        let mut gate_log = gates.clone();
        gate_log.source = "gate-source".into();
        gate_log.events[0].attribution.as_mut().unwrap().actor = "gate-recorder".into();
        let mut diffs: SourceLog = serde_json::from_str(EXPORT).unwrap();
        diffs.source = "diff-source".into();
        diffs
            .events
            .retain(|event| matches!(event.record, NativeRecord::Diff { .. }));
        diffs
            .coverage
            .retain(|coverage| coverage.kind == LogKind::Diffs);
        diffs.events[0].attribution.as_mut().unwrap().actor = "diff-recorder".into();
        let mut batches = native_batches(&selected, &native_pr());
        read_source_logs(
            &selected,
            Frame {
                now: END,
                window_secs: 86400,
                end_exclusive: false,
            },
            &mut batches,
            |path| {
                Ok(
                    serde_json::to_string(if path == std::path::Path::new("gate.json") {
                        &gate_log
                    } else {
                        &diffs
                    })
                    .unwrap(),
                )
            },
        );
        let report = collect(
            &selected,
            Frame {
                now: END,
                window_secs: 86400,
                end_exclusive: false,
            },
            &batches,
            &BTreeMap::new(),
        );
        assert_eq!(scalar(&report, "repin_only_prs"), 1.0);
        assert_eq!(
            report.measurements["gates_per_pr"].value,
            Some(Value::Keyed([("fixture/repo#1".into(), 1.0)].into()))
        );
        let Fact::PullRequest(pr) = &batches[1].observations[0].fact else {
            panic!("PR")
        };
        let json = serde_json::to_value(pr).unwrap();
        assert_eq!(
            json["provenance"]["repin_only"]["attribution"]["actor"],
            "diff-recorder"
        );
        assert_eq!(
            json["provenance"]["full_gates"]["attribution"]["actor"],
            "gate-recorder"
        );
        assert_eq!(
            serde_json::from_str::<Report>(&serde_json::to_string(&report).unwrap()).unwrap(),
            report
        );
    }
}
