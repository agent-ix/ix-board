// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Offline native summary transport, merge and roadmap contracts.

use ix_board::linear_summary::{SummarySnapshot, merge_summary};
use ix_board::program_review::build_summary_roadmap;
use ix_board::{Error, LinearGraphQlSource, WorkspaceSource};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

struct ElapsedRefreshClock {
    before: i64,
    after: i64,
    reads: std::sync::atomic::AtomicUsize,
}

impl ix_board::Clock for ElapsedRefreshClock {
    fn now_epoch_seconds(&self) -> i64 {
        if self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
            self.before
        } else {
            self.after
        }
    }
}

#[trace("FR-004-AC-3")]
#[test]
fn failed_summary_age_uses_return_clock_and_cooldown_matches_absolute_transport_state() {
    use ix_board::Clock;
    let (source, requests, cache, _) = fixture_responses(vec![
        (
            200,
            vec![],
            json!({"data":{"projects":page(json!([project("p1")]),None)}}),
        ),
        (
            200,
            vec![],
            json!({"data":{"initiatives":page(json!([initiative()]),None)}}),
        ),
        (
            500,
            vec![],
            json!({"errors":[{"message":"failed after elapsed refresh"}]}),
        ),
        (
            429,
            vec![("Retry-After", "60".to_owned())],
            json!({"errors":[{"message":"delayed quota response","extensions":{"code":"RATELIMITED"}}]}),
        ),
    ]);
    let first = source.summary_snapshot_cached().expect("complete baseline");
    let path = cache.path().join("linear.sqlite3");
    let connection = rusqlite::Connection::open(&path).expect("cache");
    let sync = first.last_successful_sync - 1000;
    connection
        .execute("UPDATE summary_snapshot SET synced_at=?1", [sync])
        .expect("expired summary");
    for rate_limited in [false, true] {
        let after = ix_board::SystemClock.now_epoch_seconds();
        let clock = ElapsedRefreshClock {
            before: after - 20,
            after,
            reads: std::sync::atomic::AtomicUsize::new(0),
        };
        let fallback = source
            .summary_snapshot_cached_with_clock(&clock)
            .expect("last complete fallback");
        assert!(fallback.freshness.stale);
        assert_eq!(fallback.data, first.data);
        assert_eq!(
            fallback.freshness.age_seconds,
            Some(after - sync),
            "elapsed refresh time belongs to source age"
        );
        assert_eq!(fallback.last_successful_sync, sync);
        if rate_limited {
            let millis = ix_board::quota::QuotaGate::open(&path, &Default::default())
                .expect("quota")
                .status()
                .expect("stored cooldown")
                .rate_limit_until
                .expect("observed cooldown");
            let expected = millis.div_euclid(1000) + i64::from(millis.rem_euclid(1000) > 0);
            assert_eq!(
                fallback.freshness.rate_limit_until,
                Some(expected),
                "absolute persisted retry deadline is authoritative"
            );
            assert!(
                expected > clock.before + 60,
                "pre-refresh clock plus remaining duration would understate cooldown"
            );
        } else {
            assert!(fallback.freshness.rate_limit_until.is_none());
        }
    }
    assert_eq!(
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("four requests")
            .len(),
        4
    );
}

#[trace("FR-019-AC-5", "FR-030-AC-4", "FR-004-AC-21")]
#[test]
fn cold_and_warm_scope_and_roadmap_reuse_summaries_without_advancing_real_detail_reads() {
    let mut detail = project("p1");
    detail.as_object_mut().expect("project").remove("progress");
    detail["content"] = Value::Null;
    detail["teams"] = page(json!([]), None);
    detail["projectMilestones"] = page(json!([]), None);
    let (source, requests, cache, _) = fixture_responses(vec![
        (
            200,
            vec![],
            json!({"data":{"projects":page(json!([project("p1")]),None)}}),
        ),
        (
            200,
            vec![],
            json!({"data":{"initiatives":page(json!([initiative()]),None)}}),
        ),
        (
            200,
            vec![],
            json!({"data":{"projects":page(json!([detail]),None)}}),
        ),
        (200, vec![], json!({"data":{"issues":page(json!([]),None)}})),
        (
            200,
            vec![],
            json!({"data":{"projects":page(json!([]),None)}}),
        ),
        (
            200,
            vec![],
            json!({"data":{"initiatives":page(json!([]),None)}}),
        ),
        (
            429,
            vec![("Retry-After", "60".to_owned())],
            json!({"errors":[{"message":"quota","extensions":{"code":"RATELIMITED"}}]}),
        ),
    ]);
    let scope: ix_board::program_review::ProgramScope =
        serde_json::from_value(json!({"program":"fixture"})).expect("no repository probes");
    let cold = source.summary_snapshot_cached().expect("cold summary");
    ix_board::program_review::check_summary_scope(&scope, &cold.data).expect("cold scope");
    let cold_roadmap = build_summary_roadmap(&cold.data, None, "observed");
    let warm = source.summary_snapshot_cached().expect("warm catalogs");
    ix_board::program_review::check_summary_scope(&scope, &warm.data).expect("warm scope");
    assert_eq!(
        build_summary_roadmap(&warm.data, None, "observed"),
        cold_roadmap
    );
    let path = cache.path().join("linear.sqlite3");
    let mut reader = ix_board::ReadService::open(source.clone(), &path).expect("detail service");
    assert!(
        reader
            .next("SWM")
            .expect("ticket-dependent read")
            .is_empty()
    );
    assert!(reader.next("SWM").expect("warm detail reuse").is_empty());
    let connection = rusqlite::Connection::open(&path).expect("cache");
    let read_detail = || {
        connection
            .query_row(
                "SELECT fetched_at,payload,read_started_at FROM workspace_snapshot",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .expect("actual detail provenance")
    };
    let detail_before = read_detail();
    connection
        .execute("UPDATE summary_snapshot SET synced_at=synced_at-1000", [])
        .expect("expire summaries");
    source.summary_snapshot_cached().expect("summary delta");
    assert_eq!(
        read_detail(),
        detail_before,
        "summary refresh cannot certify detail freshness"
    );
    connection
        .execute("UPDATE summary_snapshot SET synced_at=synced_at-1000", [])
        .expect("expire for fallback");
    let fallback = source
        .summary_snapshot_cached()
        .expect("last complete summary");
    ix_board::program_review::check_summary_scope(&scope, &fallback.data)
        .expect("scope from stale summary");
    assert!(fallback.freshness.stale);
    assert!(fallback.freshness.age_seconds.expect("known age") >= 1000);
    assert!(fallback.freshness.rate_limit_until.is_some());
    assert_eq!(
        read_detail(),
        detail_before,
        "fallback does not update detail"
    );
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("complete fixture");
    assert_eq!(requests.len(), 7);
    for index in [0, 1, 4, 5, 6] {
        let query = requests[index]["query"].as_str().expect("summary query");
        assert!(!query.contains("WorkspaceIssues") && !query.contains("history"));
    }
    assert!(
        requests[2]["query"]
            .as_str()
            .expect("detail projects")
            .contains("WorkspaceProjects")
    );
    assert!(
        requests[3]["query"]
            .as_str()
            .expect("detail tickets")
            .contains("WorkspaceIssues")
    );
}

fn page(nodes: Value, cursor: Option<&str>) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage":cursor.is_some(), "endCursor":cursor}})
}

fn project(id: &str) -> Value {
    json!({"id":id,"name":id,"description":"Goal","updatedAt":"2026-11-20T00:00:00Z",
        "archivedAt":null,"trashed":null,"progress":0.375,"priority":2,"sortOrder":1.0,
        "targetDate":null,"status":{"name":"Building","type":"started"},
        "initiatives":page(json!([{"id":"i1","name":"Program"}]),None)})
}

fn initiative() -> Value {
    json!({"id":"i1","name":"Program","updatedAt":"2026-11-20T00:00:00Z",
        "archivedAt":null,"trashed":null,"status":"Active","health":"atRisk",
        "projects":page(json!([{"id":"p1"},{"id":"p2"}]),None)})
}

fn fixture(
    responses: Vec<Value>,
) -> (
    LinearGraphQlSource,
    mpsc::Receiver<Vec<Value>>,
    tempfile::TempDir,
) {
    let (source, requests, cache, _) = fixture_endpoint(responses);
    (source, requests, cache)
}

fn fixture_endpoint(
    responses: Vec<Value>,
) -> (
    LinearGraphQlSource,
    mpsc::Receiver<Vec<Value>>,
    tempfile::TempDir,
    String,
) {
    fixture_responses(
        responses
            .into_iter()
            .map(|value| (200, Vec::new(), value))
            .collect(),
    )
}

fn fixture_responses(
    responses: Vec<(u16, Vec<(&'static str, String)>, Value)>,
) -> (
    LinearGraphQlSource,
    mpsc::Receiver<Vec<Value>>,
    tempfile::TempDir,
    String,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback fixture");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, headers, response) in responses {
            let (mut stream, _) = listener.accept().expect("fixture request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let (body_start, length) = loop {
                let read = stream.read(&mut buffer).expect("request read");
                assert_ne!(read, 0, "request must complete");
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .expect("content length");
                    break (end + 4, length);
                }
            };
            while bytes.len() - body_start < length {
                let read = stream.read(&mut buffer).expect("request body");
                assert_ne!(read, 0, "body must complete");
                bytes.extend_from_slice(&buffer[..read]);
            }
            requests.push(serde_json::from_slice(&bytes[body_start..]).expect("GraphQL request"));
            let body = response.to_string();
            let headers = headers
                .into_iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect::<String>();
            write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).expect("response");
        }
        sender.send(requests).expect("request observation");
    });
    let mut source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("summary-fixture")),
        || None,
        &endpoint,
    )
    .expect("fixture source");
    let cache = tempfile::tempdir().expect("isolated fixture cache");
    source
        .configure_cache(&cache.path().join("linear.sqlite3"))
        .expect("cache binding");
    (source, receiver, cache, endpoint)
}

/// Trace: FR-020-AC-2, FR-030-AC-1, FR-019-AC-5
#[trace("FR-020-AC-2")]
#[test]
fn native_catalog_pages_never_enumerate_tickets_and_keep_weighted_progress() {
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([project("p1")]),Some("next"))}}),
        json!({"data":{"projects":page(json!([project("p2")]),None)}}),
        json!({"data":{"initiatives":page(json!([initiative()]),None)}}),
    ]);
    let snapshot = source.fetch_summary().expect("complete native summary");
    assert_eq!(snapshot.projects.len(), 2);
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("observed requests");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["variables"]["after"], "next");
    assert_eq!(requests[0]["variables"]["first"], 20);
    for request in requests {
        let query = request["query"].as_str().expect("query text");
        assert!(!query.contains("issues") && !query.contains("history"));
        assert!(query.contains("includeArchived:$includeArchived"));
        assert_eq!(request["variables"]["includeArchived"], false);
    }
    let summary = merge_summary(&SummarySnapshot::default(), snapshot);
    let roadmap = build_summary_roadmap(&summary, None, "2026-11-21T00:00:00Z");
    let row = &roadmap.initiatives[0].projects[0];
    assert_eq!(row.progress_percent, Some(37.5));
    assert_eq!(row.progress_source, "linear_native");
    assert_eq!(row.open, None);
    assert_eq!(row.done, None);
    assert_eq!(row.canceled, None);
    assert_eq!(row.last_activity, None);
    assert!(!row.counts_unavailable_reason.is_empty());
    assert_eq!(roadmap.initiatives[0].health.as_deref(), Some("atRisk"));
    assert_eq!(
        row.project_updated_at.as_deref(),
        Some("2026-11-20T00:00:00Z")
    );
    let json = serde_json::to_value(&roadmap).expect("native JSON roadmap");
    let row = &json["initiatives"][0]["projects"][0];
    for field in ["open", "done", "canceled", "last_activity"] {
        assert!(row[field].is_null(), "unsupported {field} is explicit null");
    }
    assert_eq!(row["observed_at"], "2026-11-21T00:00:00Z");
    assert_eq!(row["progress_source"], "linear_native");
    assert!(
        row["activity_unavailable_reason"]
            .as_str()
            .expect("reason")
            .contains("issue")
    );
}

/// Trace: FR-030-AC-1, FR-030-AC-2
#[trace("FR-030-AC-1", "FR-030-AC-2")]
#[test]
fn delta_uses_overlap_for_both_catalogs_and_failed_pages_return_no_batch() {
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([]),None)}}),
        json!({"data":{"initiatives":page(json!([]),None)}}),
    ]);
    source
        .fetch_summary_changes(120)
        .expect("empty complete delta");
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("observed delta");
    for request in requests {
        assert_eq!(
            request["variables"]["filter"]["updatedAt"]["gte"],
            "1970-01-01T00:01:00+00:00"
        );
    }
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([project("p1")]),Some("next"))}}),
        json!({"errors":[{"message":"unavailable"}]}),
    ]);
    assert!(matches!(
        source.fetch_summary_changes(120),
        Err(Error::GraphQl { .. })
    ));
    assert_eq!(
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("failed pages")
            .len(),
        2
    );
}

fn baseline() -> SummarySnapshot {
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([project("p1"),project("p2")]),None)}}),
        json!({"data":{"initiatives":page(json!([initiative()]),None)}}),
    ]);
    let result = source.fetch_summary().expect("baseline");
    requests
        .recv_timeout(Duration::from_secs(5))
        .expect("fixture completed");
    merge_summary(&SummarySnapshot::default(), result)
}

/// Trace: FR-030-AC-1, FR-030-AC-3
#[trace("FR-030-AC-1", "FR-030-AC-3")]
#[test]
fn stable_id_merge_replaces_membership_removes_tombstones_and_is_idempotent() {
    let baseline = baseline();
    let mut changed = baseline.initiatives[0].clone();
    changed.initiative.name = "Renamed".to_owned();
    changed.project_ids.retain(|id| id.0 == "p2");
    let changes = SummarySnapshot {
        projects: Vec::new(),
        initiatives: vec![changed],
    };
    let merged = merge_summary(&baseline, changes.clone());
    assert!(merged.projects[0].project.initiatives.is_empty());
    assert_eq!(merged.projects[1].project.initiatives[0].name, "Renamed");
    assert_eq!(merge_summary(&merged, changes), merged);
    let mut archived = merged.projects[0].clone();
    archived.archived = true;
    let mut trashed = merged.initiatives[0].clone();
    trashed.trashed = true;
    let merged = merge_summary(
        &merged,
        SummarySnapshot {
            projects: vec![archived],
            initiatives: vec![trashed],
        },
    );
    assert_eq!(merged.projects.len(), 1);
    assert_eq!(merged.projects[0].project.id.0, "p2");
    assert!(merged.projects[0].project.initiatives.is_empty());
    assert!(merged.initiatives.is_empty());
    let reconciled = merge_summary(&SummarySnapshot::default(), SummarySnapshot::default());
    assert!(
        reconciled.projects.is_empty(),
        "authoritative empty scan removes absent IDs"
    );
}

/// Trace: FR-030-AC-1, FR-020-AC-1
#[trace("FR-030-AC-1", "FR-020-AC-1")]
#[test]
fn summary_memberships_paginate_without_loading_tickets() {
    let mut first_project = project("p1");
    first_project["initiatives"] = page(json!([{"id":"i1","name":"Program"}]), Some("members"));
    let mut first_initiative = initiative();
    first_initiative["projects"] = page(json!([{"id":"p1"}]), Some("projects"));
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([first_project]),None)}}),
        json!({"data":{"project":{"initiatives":page(json!([{"id":"i2","name":"Second"}]),None)}}}),
        json!({"data":{"initiatives":page(json!([first_initiative]),None)}}),
        json!({"data":{"initiative":{"projects":page(json!([{"id":"p2"}]),None)}}}),
    ]);
    let summary = source.fetch_summary().expect("complete memberships");
    assert_eq!(summary.projects[0].project.initiatives.len(), 2);
    assert_eq!(summary.initiatives[0].project_ids.len(), 2);
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("nested requests");
    assert_eq!(requests[1]["variables"]["after"], "members");
    assert_eq!(requests[3]["variables"]["after"], "projects");
    assert_eq!(requests[3]["variables"]["id"], "i1");
    for request in requests {
        let query = request["query"].as_str().expect("query text");
        assert!(!query.contains("issues") && !query.contains("history"));
    }
}

/// Trace: FR-030-AC-2
#[trace("FR-030-AC-2")]
#[test]
fn repeated_summary_cursor_is_refused_instead_of_publishing_partial_data() {
    let (source, requests, _cache) = fixture(vec![
        json!({"data":{"projects":page(json!([project("p1")]),Some("same"))}}),
        json!({"data":{"projects":page(json!([project("p2")]),Some("same"))}}),
    ]);
    assert!(matches!(
        source.fetch_summary(),
        Err(Error::InvalidCursor { .. })
    ));
    assert_eq!(
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("cursor pages")
            .len(),
        2
    );
}

/// Trace: FR-004-AC-1, FR-004-AC-21, FR-030-AC-1, FR-030-AC-2, FR-030-AC-3, FR-030-AC-4, FR-030-AC-6
#[trace(
    "FR-004-AC-1",
    "FR-004-AC-21",
    "FR-030-AC-2",
    "FR-030-AC-3",
    "FR-030-AC-6"
)]
#[test]
fn persistent_summary_cache_reuses_deltas_preserves_failures_and_reconciles_independently() {
    let mut only_p2 = initiative();
    only_p2["projects"] = page(json!([{"id":"p2"}]), None);
    let (source, requests, cache, endpoint) = fixture_endpoint(vec![
        json!({"data":{"projects":page(json!([project("p1")]),None)}}),
        json!({"data":{"initiatives":page(json!([initiative()]),None)}}),
        json!({"data":{"projects":page(json!([project("p2")]),None)}}),
        json!({"data":{"initiatives":page(json!([]),None)}}),
        json!({"data":{"projects":page(json!([project("unpublished")]),Some("later"))}}),
        json!({"errors":[{"message":"failed later page"}]}),
        json!({"data":{"projects":page(json!([project("p2")]),None)}}),
        json!({"data":{"initiatives":page(json!([only_p2]),None)}}),
        json!({"data":{"projects":page(json!([project("different-auth")]),None)}}),
        json!({"data":{"initiatives":page(json!([]),None)}}),
        json!({"errors":[{"message":"new authorization unavailable"}]}),
    ]);
    let path = cache.path().join("linear.sqlite3");
    let _detail_service =
        ix_board::ReadService::open(source.clone(), &path).expect("detail cache schema");
    let connection = rusqlite::Connection::open(&path).expect("cache inspection");
    connection.execute("INSERT INTO workspace_snapshot(cache_key,fetched_at,payload) VALUES('detail-sentinel',123,'{}')",[]).expect("preexisting detail");
    let first = source.summary_snapshot_cached().expect("cold summary");
    assert_eq!(first.data.projects.len(), 1);
    let warm = source.summary_snapshot_cached().expect("warm summary");
    assert_eq!(warm.data, first.data);
    assert_eq!(warm.last_successful_sync, first.last_successful_sync);
    let previous_sync = first.last_successful_sync - 4_000;
    connection
        .execute("UPDATE summary_snapshot SET synced_at=?1", [previous_sync])
        .expect("expire summary");
    let delta = source
        .summary_snapshot_cached()
        .expect("incremental summary");
    assert_eq!(
        delta.data.projects.len(),
        2,
        "unchanged project is retained"
    );
    let failed_sync = first.last_successful_sync - 5_000;
    connection
        .execute("UPDATE summary_snapshot SET synced_at=?1", [failed_sync])
        .expect("expire for failure");
    let before: Vec<u8> = connection
        .query_row("SELECT payload FROM summary_snapshot", [], |r| r.get(0))
        .expect("before payload");
    let fallback = source
        .summary_snapshot_cached()
        .expect("last complete fallback");
    assert!(fallback.freshness.stale);
    assert_eq!(fallback.data, delta.data);
    assert_eq!(fallback.last_successful_sync, failed_sync);
    let after: (Vec<u8>, i64) = connection
        .query_row("SELECT payload,synced_at FROM summary_snapshot", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("unchanged publication");
    assert_eq!(after, (before, failed_sync));
    connection
        .execute(
            "UPDATE summary_snapshot SET synced_at=?1,full_at=?2",
            rusqlite::params![failed_sync, first.last_successful_sync - 86_401],
        )
        .expect("daily full due");
    let reconciled = source
        .summary_snapshot_cached()
        .expect("authoritative reconciliation");
    assert_eq!(reconciled.data.projects.len(), 1);
    assert_eq!(reconciled.data.projects[0].project.id.0, "p2");
    assert!(reconciled.last_full_reconciliation > first.last_successful_sync - 86_401);
    let mut other = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("different-summary-credential")),
        || None,
        &endpoint,
    )
    .expect("different credential source");
    other.configure_cache(&path).expect("same shared cache");
    let isolated = other
        .summary_snapshot_cached()
        .expect("new authorization baseline");
    assert_eq!(isolated.data.projects.len(), 1);
    assert_eq!(isolated.data.projects[0].project.id.0, "different-auth");
    let mut unavailable = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("unavailable-credential")),
        || None,
        &endpoint,
    )
    .expect("unavailable authorization source");
    unavailable
        .configure_cache(&path)
        .expect("shared cache binding");
    assert!(
        matches!(
            unavailable.summary_snapshot_cached(),
            Err(Error::GraphQl { .. })
        ),
        "a failed new authorization baseline cannot expose another partition"
    );
    let detail_time: i64 = connection
        .query_row(
            "SELECT fetched_at FROM workspace_snapshot WHERE cache_key='detail-sentinel'",
            [],
            |r| r.get(0),
        )
        .expect("detail provenance");
    assert_eq!(
        detail_time, 123,
        "summary never advances detailed source freshness"
    );
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("coordinated requests");
    assert_eq!(requests.len(), 11, "warm read has no source request");
    let expected = chrono::DateTime::from_timestamp(previous_sync - 60, 0)
        .expect("sync timestamp")
        .to_rfc3339();
    assert_eq!(
        requests[2]["variables"]["filter"]["updatedAt"]["gte"],
        expected
    );
    assert_eq!(
        requests[3]["variables"]["filter"]["updatedAt"]["gte"],
        expected
    );
    for index in [0, 1, 6, 7, 8, 9] {
        assert!(
            requests[index]["variables"]["filter"].is_null(),
            "full reconciliation and changed authorization request full catalogs"
        );
    }
}
