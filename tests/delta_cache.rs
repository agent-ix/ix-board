// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Actual detailed-cache refreshes against an offline GraphQL service (FR-030).

use ix_board::{
    Clock, LinearGraphQlSource, ReadService, WorkspaceSnapshot, WorkspaceSource,
    ready_from_snapshot,
};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone)]
struct ManualClock(Arc<AtomicI64>);
impl Clock for ManualClock {
    fn now_epoch_seconds(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    fn monotonic_now(&self) -> Instant {
        Instant::now()
    }
}
impl ManualClock {
    fn new() -> Self {
        Self(Arc::new(AtomicI64::new(1_800_000_000)))
    }
    fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

fn page(nodes: Value) -> Value {
    json!({"nodes":nodes,"pageInfo":{"hasNextPage":false,"endCursor":null}})
}
fn projects(nodes: Value) -> Value {
    json!({"data":{"projects":page(nodes)}})
}
fn issues(nodes: Value) -> Value {
    json!({"data":{"issues":page(nodes)}})
}
fn initiatives() -> Value {
    json!({"data":{"initiatives":page(json!([]))}})
}
fn initiative_members(name: &str, ids: &[&str]) -> Value {
    let members = ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>();
    json!({"data":{"initiatives":page(json!([{
        "id":"initiative-1", "slugId":"slug-1", "name":name,"updatedAt":"2026-11-21T00:00:00Z",
        "archivedAt":null,"trashed":false,"status":"Active","health":"onTrack",
        "projects":page(json!(members))
    }]))}})
}
fn project(id: &str) -> Value {
    json!({"id":id,"name":id,"description":"Goal","content":null,"priority":2,"sortOrder":1.0,
      "targetDate":null,"updatedAt":"2026-11-20T00:00:00Z","archivedAt":null,"trashed":false,
      "status":{"name":"Started","type":"started"},"teams":page(json!([{"key":"SWM"}])),
      "projectMilestones":page(json!([])),"initiatives":page(json!([]))})
}
fn issue(id: &str) -> Value {
    json!({"id":id,"identifier":id,"title":id,"description":null,"priority":2.0,"sortOrder":1.0,
      "createdAt":"2026-11-19T00:00:00Z","updatedAt":"2026-11-20T00:00:00Z","archivedAt":null,"trashed":false,
      "state":{"name":"Backlog","type":"backlog"},"team":{"key":"SWM"},"assignee":null,
      "project":{"id":"p1"},"projectMilestone":null,"parent":null,
      "relations":page(json!([])),"inverseRelations":page(json!([])),"attachments":page(json!([]))})
}
fn source(endpoint: &str, credential: &str) -> LinearGraphQlSource {
    LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from(credential)),
        || None,
        endpoint,
    )
    .expect("explicit fixture credential")
}
fn fixture(responses: Vec<Value>) -> (String, mpsc::Receiver<Vec<Value>>) {
    fixture_with_cost(responses, None)
}

fn fixture_with_cost(
    responses: Vec<Value>,
    actual_cost: Option<u64>,
) -> (String, mpsc::Receiver<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback fixture");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (index, response) in responses.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().expect("fixture request");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            let (start, length) = loop {
                let read = stream.read(&mut buffer).expect("request headers");
                assert_ne!(read, 0, "complete request required");
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
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
                        .expect("request length");
                    break (end + 4, length);
                }
            };
            while bytes.len() - start < length {
                let read = stream.read(&mut buffer).expect("request body");
                assert_ne!(read, 0, "complete body required");
                bytes.extend_from_slice(&buffer[..read]);
            }
            requests.push(serde_json::from_slice(&bytes[start..]).expect("typed JSON request"));
            let body = response.to_string();
            let accounting = actual_cost.map_or_else(String::new,|cost| {
                let used = u64::try_from(index+1).expect("fixture request count")*cost;
                let reset = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("epoch").as_millis()+3_600_000;
                format!("X-Complexity: {cost}\r\nX-RateLimit-Requests-Limit: 100000\r\nX-RateLimit-Complexity-Limit: 6000000\r\nX-RateLimit-Complexity-Remaining: {}\r\nX-RateLimit-Complexity-Reset: {reset}\r\n",6_000_000-used)
            });
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{accounting}Content-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).expect("fixture response");
        }
        sender.send(requests).expect("observations");
    });
    (endpoint, receiver)
}

#[trace("FR-031-AC-4", "FR-031-AC-5", "FR-030-AC-3")]
#[test]
fn a_complete_small_cost_catalog_can_exceed_270_transport_requests() {
    let mut responses = vec![projects(json!([]))];
    for index in 1..=300 {
        let mut response = issues(json!([issue(&format!("SWM-{index}"))]));
        if index < 300 {
            response["data"]["issues"]["pageInfo"] =
                json!({"hasNextPage":true,"endCursor":format!("issues-{index}")});
        }
        responses.push(response);
    }
    let (endpoint, requests) = fixture_with_cost(responses, Some(100));
    let cache = tempfile::tempdir().expect("cache");
    let path = cache.path().join("linear.sqlite3");
    let mut reader =
        ReadService::open(source(&endpoint, "actual-cost-full-catalog"), &path).expect("reader");
    let snapshot = reader
        .workspace_snapshot()
        .expect("every page fits the actual shared complexity budget");
    assert_eq!(
        snapshot.issues.len(),
        300,
        "full catalog cannot stop at conservative reservation capacity"
    );
    assert!(
        snapshot
            .issues
            .iter()
            .any(|issue| issue.identifier == "SWM-300")
    );
    let status = ix_board::cache_status::read_status(&path).expect("offline shared accounting");
    assert_eq!(status.quota.requests_used, 301);
    assert_eq!(
        status.quota.complexity_used, 30_100,
        "observed actual cost replaces each estimate exactly once"
    );
    let refresh = status.latest_refresh.expect("successful complete refresh");
    assert!(refresh.success);
    assert_eq!(refresh.requests_attempted, 301);
    assert_eq!(refresh.returned_complexity, Some(30_100));
    assert_eq!(refresh.estimated_complexity, 0);
    assert_eq!(
        requests
            .recv_timeout(Duration::from_secs(5))
            .expect("all real fixture sends")
            .len(),
        301
    );
}
fn is_ready(snapshot: &WorkspaceSnapshot, id: &str) -> bool {
    ready_from_snapshot(snapshot, "SWM")
        .expect("ready queue")
        .iter()
        .any(|row| row.identifier == id)
}
fn assert_delta(request: &Value, since: i64) {
    let query = request["query"].as_str().expect("query");
    assert_eq!(request["variables"]["includeArchived"], true);
    let expected = chrono::DateTime::from_timestamp(since - 60, 0)
        .expect("sync timestamp")
        .to_rfc3339();
    assert!(
        !query.contains("gte: $since"),
        "filters are typed whole inputs"
    );
    assert!(
        request["variables"].get("since").is_none(),
        "no undeclared legacy variable"
    );
    let observed = &request["variables"]["filter"]["updatedAt"]["gte"];
    assert_eq!(observed, &Value::String(expected));
}

#[trace("FR-030-AC-3", "FR-030-AC-6", "FR-031-AC-4")]
#[test]
fn unfiltered_paged_baseline_replaces_poisoned_model_partition_without_resetting_shared_quota() {
    use sha2::{Digest, Sha256};
    let mut first_projects = projects(json!([project("p1")]));
    first_projects["data"]["projects"]["pageInfo"] =
        json!({"hasNextPage":true,"endCursor":"projects-2"});
    let mut first_issues = issues(json!([issue("first-page")]));
    first_issues["data"]["issues"]["pageInfo"] = json!({"hasNextPage":true,"endCursor":"issues-2"});
    let mut summary_project = project("native-summary");
    for field in ["content", "teams", "projectMilestones"] {
        summary_project
            .as_object_mut()
            .expect("project")
            .remove(field);
    }
    summary_project["progress"] = json!(0.5);
    let (endpoint, requests) = fixture(vec![
        first_projects,
        projects(json!([project("p2")])),
        first_issues,
        issues(json!([issue("second-page")])),
        projects(json!([])),
        issues(json!([issue("delta-added")])),
        initiatives(),
        projects(json!([summary_project])),
        initiatives(),
    ]);
    let cache = tempfile::tempdir().expect("cache");
    let path = cache.path().join("linear.sqlite3");
    let clock = ManualClock::new();
    let mut source = source(&endpoint, "poisoned-model-fixture");
    source.configure_cache(&path).expect("shared cache");
    let current = source.cache_namespace();
    assert!(current.starts_with("linear:v16:"));
    let poisoned = current.replacen("linear:v16:", "linear:v14:", 1);
    let mut reader = ReadService::with_clock(source.clone(), &path, clock.clone()).expect("reader");
    let db = Connection::open(&path).expect("seed former partition");
    let empty = serde_json::to_vec(&WorkspaceSnapshot {
        fetched_at: clock.now_epoch_seconds(),
        ..Default::default()
    })
    .expect("empty former baseline");
    db.execute("INSERT INTO workspace_snapshot(cache_key,fetched_at,payload,read_started_at,read_started_for,read_started_digest,full_reconciled_at)
        VALUES(?1,?2,?3,?2,?2,?4,?2)",rusqlite::params![poisoned,clock.now_epoch_seconds(),empty,format!("{:x}",Sha256::digest(&empty))]).expect("poisoned v14 detail baseline");
    db.execute_batch("CREATE TABLE summary_snapshot(cache_key TEXT PRIMARY KEY,payload BLOB NOT NULL,synced_at INTEGER NOT NULL,full_at INTEGER NOT NULL)").expect("former summaries");
    db.execute(
        "INSERT INTO summary_snapshot VALUES(?1,?2,?3,?3)",
        rusqlite::params![
            format!("{poisoned}:summary:v2"),
            serde_json::to_vec(&ix_board::linear_summary::SummarySnapshot::default())
                .expect("empty summary"),
            clock.now_epoch_seconds()
        ],
    )
    .expect("poisoned v14 summary baseline");
    let gate = ix_board::quota::QuotaGate::open(&path, &Default::default())
        .expect("existing independent quota");
    let _unobserved_legacy_charge = gate
        .reserve(7, None)
        .expect("prior admitted transport cost");
    let baseline = reader
        .workspace_snapshot()
        .expect("authoritative paged baseline");
    assert_eq!(baseline.projects.len(), 2);
    assert_eq!(baseline.issues.len(), 2);
    assert!(
        baseline
            .issues
            .iter()
            .any(|issue| issue.identifier == "second-page")
    );
    assert_eq!(gate.status().expect("preserved quota").requests_used, 5);
    clock.advance(1000);
    let delta = reader
        .force_refresh_workspace()
        .expect("delta on complete baseline");
    assert_eq!(
        delta.issues.len(),
        3,
        "unchanged baseline pages survive incremental merge"
    );
    let summary = source
        .summary_snapshot_cached()
        .expect("new model requires native full baseline too");
    assert_eq!(summary.data.projects[0].project.id.0, "native-summary");
    assert_eq!(
        gate.status().expect("all shared attempts").requests_used,
        10
    );
    assert_eq!(
        gate.status().expect("all shared costs").complexity_used,
        90_007
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM workspace_snapshot WHERE cache_key=?1",
            [poisoned],
            |row| row.get::<_, i64>(0)
        )
        .expect("legacy retention"),
        1
    );
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("complete pages and delta");
    assert_eq!(requests.len(), 9);
    for index in [0, 1, 2, 3] {
        let query = requests[index]["query"].as_str().expect("full query");
        assert!(
            !query.contains("gte:") && !query.contains("$since"),
            "full query cannot construct a null updatedAt lower bound"
        );
        assert!(query.contains("filter: $filter"));
        assert!(requests[index]["variables"].get("filter").is_none());
        assert!(requests[index]["variables"].get("since").is_none());
        assert_eq!(requests[index]["variables"]["includeArchived"], false);
    }
    assert_eq!(requests[1]["variables"]["after"], "projects-2");
    assert_eq!(requests[3]["variables"]["after"], "issues-2");
    for request in &requests[4..7] {
        assert_delta(request, baseline.fetched_at);
    }
    for request in &requests[7..] {
        assert!(
            request["variables"]["filter"].is_null(),
            "native summary root has no lower-bound object on full read"
        );
    }
}

/// Trace: FR-030-AC-1, FR-030-AC-4
#[trace("FR-030-AC-1")]
#[test]
fn detailed_deltas_preserve_unchanged_records_and_update_blocking_edges_on_unchanged_dependents() {
    let mut b_blocks_c = issue("B");
    b_blocks_c["relations"] =
        page(json!([{"type":"blocks","issue":{"id":"B"},"relatedIssue":{"id":"C"}}]));
    let mut updated_project = project("p1");
    updated_project["name"] = json!("Updated project");
    let mut archived_project = project("obsolete-project");
    archived_project["archivedAt"] = json!("2026-11-21T00:00:00Z");
    let mut trashed_issue = issue("obsolete-issue");
    trashed_issue["trashed"] = json!(true);
    let (endpoint, requests) = fixture(vec![
        projects(json!([project("p1"), project("obsolete-project")])),
        issues(json!([
            issue("B"),
            issue("C"),
            issue("unchanged"),
            issue("obsolete-issue")
        ])),
        projects(json!([updated_project, project("new-project")])),
        issues(json!([b_blocks_c, issue("new-issue")])),
        initiative_members("Program", &["p1", "new-project"]),
        projects(json!([])),
        issues(json!([issue("B")])),
        initiative_members("Renamed Program", &["new-project"]),
        projects(json!([archived_project])),
        issues(json!([trashed_issue])),
        initiatives(),
    ]);
    let cache = tempfile::tempdir().expect("isolated cache");
    let clock = ManualClock::new();
    let mut reader = ReadService::with_clock(
        source(&endpoint, "delta-fixture"),
        &cache.path().join("linear.sqlite3"),
        clock.clone(),
    )
    .expect("reader");
    let baseline = reader.workspace_snapshot().expect("complete baseline");
    assert!(is_ready(&baseline, "C"));
    clock.advance(1000);
    let added = reader
        .force_refresh_workspace()
        .expect("add relation delta");
    assert_eq!(added.projects.len(), 3);
    assert!(added.projects.iter().any(|p| p.name == "Updated project"));
    assert_eq!(
        added
            .projects
            .iter()
            .find(|p| p.id.0 == "p1")
            .expect("updated project")
            .initiatives[0]
            .name,
        "Program"
    );
    assert!(
        added
            .projects
            .iter()
            .flat_map(|p| &p.initiatives)
            .all(|i| i.identifier.is_empty()),
        "a delta adds no initiative slug that a full detail read would not carry"
    );
    assert!(added.issues.iter().any(|i| i.identifier == "unchanged"));
    assert!(added.issues.iter().any(|i| i.identifier == "new-issue"));
    assert!(
        !is_ready(&added, "C"),
        "B's new outgoing edge must block C even though C was absent from the delta"
    );
    let c = added
        .issues
        .iter()
        .find(|i| i.identifier == "C")
        .expect("unchanged dependent");
    assert_eq!(
        c.blocked_by
            .iter()
            .map(|id| id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["B"]
    );
    clock.advance(1000);
    let removed = reader
        .force_refresh_workspace()
        .expect("remove relation delta");
    assert!(
        removed
            .projects
            .iter()
            .find(|p| p.id.0 == "p1")
            .expect("p1 retained")
            .initiatives
            .is_empty(),
        "initiative-only delta removes obsolete project membership without a changed project"
    );
    assert_eq!(
        removed
            .projects
            .iter()
            .find(|p| p.id.0 == "new-project")
            .expect("new project retained")
            .initiatives[0]
            .name,
        "Renamed Program"
    );
    assert!(
        is_ready(&removed, "C"),
        "removing B's outgoing edge must unblock unchanged C"
    );
    assert!(
        removed
            .issues
            .iter()
            .find(|i| i.identifier == "C")
            .expect("C retained")
            .blocked_by
            .is_empty()
    );
    clock.advance(1000);
    let tombstones = reader.force_refresh_workspace().expect("tombstone delta");
    assert!(
        !tombstones
            .projects
            .iter()
            .any(|p| p.id.0 == "obsolete-project")
    );
    assert!(
        !tombstones
            .issues
            .iter()
            .any(|i| i.identifier == "obsolete-issue")
    );
    assert!(
        tombstones
            .issues
            .iter()
            .any(|i| i.identifier == "unchanged")
    );
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("all fixture requests");
    assert_eq!(requests.len(), 11);
    for (start, sync) in [
        (2, baseline.fetched_at),
        (5, added.fetched_at),
        (8, removed.fetched_at),
    ] {
        for request in &requests[start..start + 3] {
            assert_delta(request, sync);
        }
    }
}

/// Trace: FR-030-AC-2, FR-030-AC-3, FR-030-AC-6
#[trace("FR-030-AC-2", "FR-030-AC-3", "FR-030-AC-6")]
#[test]
fn failed_detail_page_preserves_watermark_records_and_costs_then_retry_and_daily_full_are_correct()
{
    let (endpoint, requests) = fixture(vec![
        projects(json!([project("p1")])),
        issues(json!([issue("retained"), issue("deleted-later")])),
        projects(json!([project("unpublished")])),
        json!({"data":{"issues":{"nodes":[issue("unpublished")],"pageInfo":{"hasNextPage":true,"endCursor":"failed-page"}}}}),
        json!({"errors":[{"message":"fixture later-page failure"}]}),
        projects(json!([])),
        issues(json!([issue("added-after-retry")])),
        initiatives(),
        projects(json!([project("p1")])),
        issues(json!([issue("retained")])),
        json!({"errors":[{"message":"different authorization baseline unavailable"}]}),
    ]);
    let cache = tempfile::tempdir().expect("isolated cache");
    let path = cache.path().join("linear.sqlite3");
    let clock = ManualClock::new();
    let mut reader =
        ReadService::with_clock(source(&endpoint, "failure-fixture"), &path, clock.clone())
            .expect("reader");
    let baseline = reader.workspace_snapshot().expect("baseline");
    let db = Connection::open(&path).expect("inspect database");
    let before: (Vec<u8>, i64) = db
        .query_row(
            "SELECT payload,fetched_at FROM workspace_snapshot",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("baseline storage");
    clock.advance(1000);
    let fallback = reader.force_refresh_workspace().expect("complete fallback");
    assert_eq!(fallback, baseline);
    assert!(reader.cache_freshness().expect("fallback metadata").stale);
    let after: (Vec<u8>, i64) = db
        .query_row(
            "SELECT payload,fetched_at FROM workspace_snapshot",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("failed storage");
    assert_eq!(
        after, before,
        "failed later page cannot publish payload or successful watermark"
    );
    let costs = ix_board::cache_status::read_status(&path)
        .expect("offline status")
        .latest_refresh
        .expect("failed refresh costs");
    assert!(!costs.success);
    assert_eq!(costs.mode, "incremental");
    assert_eq!(costs.requests_attempted, 3);
    assert_eq!(costs.complexity, 30_000);
    assert_eq!(costs.estimated_complexity, 30_000);
    assert_eq!(costs.returned_complexity, None);
    clock.advance(1000);
    let retry = reader
        .force_refresh_workspace()
        .expect("retry against successful watermark");
    assert_eq!(retry.issues.len(), 3);
    assert!(!retry.issues.iter().any(|i| i.identifier == "unpublished"));
    clock.advance(86_401);
    let full = reader
        .force_refresh_workspace()
        .expect("daily full reconciliation");
    assert_eq!(full.issues.len(), 1);
    assert_eq!(full.issues[0].identifier, "retained");
    let costs = ix_board::cache_status::read_status(&path)
        .expect("full status")
        .latest_refresh
        .expect("full costs");
    assert!(costs.success);
    assert_eq!(costs.mode, "full");
    assert_eq!(costs.requests_attempted, 2);
    let mut different =
        ReadService::with_clock(source(&endpoint, "different-auth-fixture"), &path, clock)
            .expect("different authorization partition");
    assert!(
        different.workspace_snapshot().is_err(),
        "a failed incompatible baseline must not return the original authorization's cache"
    );
    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("fixture requests");
    assert_eq!(requests.len(), 11);
    for request in &requests[2..5] {
        assert_delta(request, baseline.fetched_at);
    }
    for request in &requests[5..8] {
        assert_delta(request, baseline.fetched_at);
    }
    for index in [0, 1, 8, 9, 10] {
        assert_eq!(
            requests[index]["variables"]["includeArchived"], false,
            "full scans enumerate only active records"
        );
        assert!(
            requests[index]["variables"].get("filter").is_none(),
            "full baseline omits the entire update filter"
        );
        let query = requests[index]["query"].as_str().expect("full query");
        assert!(
            !query.contains("$since") && !query.contains("gte:"),
            "no null lower-bound predicate in full queries"
        );
    }
}

/// Linear does not move a project's `updatedAt` when a milestone is added to it.
///
/// Trace: FR-030-AC-7
#[trace("FR-030-AC-7")]
#[test]
fn a_delta_adds_a_milestone_that_only_a_changed_issue_names() {
    let mut moved = issue("A");
    moved["projectMilestone"] = json!({
        "id":"milestone-new","name":"New milestone","sortOrder":2.0,
        "targetDate":"2026-12-01","project":{"id":"p1"}
    });
    let (endpoint, requests) = fixture(vec![
        projects(json!([project("p1")])),
        issues(json!([issue("A")])),
        projects(json!([])),
        issues(json!([moved])),
        initiatives(),
    ]);
    let cache = tempfile::tempdir().expect("isolated cache");
    let clock = ManualClock::new();
    let mut reader = ReadService::with_clock(
        source(&endpoint, "milestone-fixture"),
        &cache.path().join("linear.sqlite3"),
        clock.clone(),
    )
    .expect("reader");
    let baseline = reader.workspace_snapshot().expect("complete baseline");
    assert!(baseline.projects[0].milestones.is_empty());
    clock.advance(1000);
    let delta = reader.force_refresh_workspace().expect("delta");
    let milestones = &delta.projects[0].milestones;
    assert_eq!(milestones.len(), 1);
    assert_eq!(milestones[0].id, "milestone-new");
    assert_eq!(milestones[0].name, "New milestone");
    assert_eq!(milestones[0].target_date.as_deref(), Some("2026-12-01"));

    let expansion = reader
        .focus_expand(&ix_board::FocusSelection {
            milestones: vec!["New milestone".to_owned()],
            ..Default::default()
        })
        .expect("the new milestone resolves");
    assert_eq!(expansion.focus_issues.len(), 1);
    assert!(expansion.warnings.is_empty());
    assert_eq!(
        expansion.projects[0].milestones[0].issues[0].identifier,
        "A"
    );

    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("no request beyond the ordinary delta");
    assert_eq!(requests.len(), 5);
    assert!(
        requests[3]["query"]
            .as_str()
            .expect("issue query")
            .contains("projectMilestone { id name sortOrder targetDate project { id } }")
    );
}
