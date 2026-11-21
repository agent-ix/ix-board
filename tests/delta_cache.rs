// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Actual detailed-cache refreshes against an offline GraphQL service (FR-030).

use ix_board::{Clock, LinearGraphQlSource, ReadService, WorkspaceSnapshot, ready_from_snapshot};
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
        "id":"initiative-1", "name":name,"updatedAt":"2026-11-21T00:00:00Z",
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
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback fixture");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
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
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).expect("fixture response");
        }
        sender.send(requests).expect("observations");
    });
    (endpoint, receiver)
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
    let observed = if query.contains("SummaryInitiatives") {
        &request["variables"]["filter"]["updatedAt"]["gte"]
    } else {
        &request["variables"]["since"]
    };
    assert_eq!(observed, &Value::String(expected));
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
            requests[index]["variables"]["since"].is_null(),
            "full baseline requests have no delta watermark"
        );
    }
}
