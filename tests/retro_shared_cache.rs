// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Real retro adapters share centrally coordinated detail reads and quota metering.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Barrier, mpsc};
use std::time::Duration;

use ix_board::LinearGraphQlSource;
use ix_board::quota::{QuotaConfig, QuotaGate};
use ix_board::retro::live::LiveLinear;
use ix_board::retro::sources::LinearSource;
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn read_request(stream: &mut TcpStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("fixture read bound");
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let (start, length) = loop {
        let read = stream.read(&mut buffer).expect("request headers");
        assert_ne!(read, 0, "request must complete");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let length = String::from_utf8_lossy(&bytes[..end])
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .expect("request content length");
            break (end + 4, length);
        }
        assert!(
            bytes.len() < 64 * 1024,
            "fixture request headers are bounded"
        );
    };
    while bytes.len() - start < length {
        let read = stream.read(&mut buffer).expect("request body");
        assert_ne!(read, 0, "request body must complete");
        bytes.extend_from_slice(&buffer[..read]);
    }
    serde_json::from_slice(&bytes[start..]).expect("GraphQL request")
}

fn fixture() -> (String, mpsc::Receiver<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("offline fixture");
    let endpoint = format!(
        "http://{}/graphql",
        listener.local_addr().expect("fixture address")
    );
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..7 {
            let (mut stream, _) = listener.accept().expect("fixture request");
            let request = read_request(&mut stream);
            let query = request["query"].as_str().expect("query string");
            let page = json!({"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}});
            let response = if query.contains("WorkspaceProjects") {
                json!({"data":{"projects":page}})
            } else if query.contains("WorkspaceIssues") {
                json!({"data":{"issues":page}})
            } else if query.contains("SummaryInitiatives") {
                json!({"data":{"initiatives":page}})
            } else if query.contains("IssueHistory") {
                json!({"data":{"issue":{"id":"fixture-issue","history":page}}})
            } else {
                panic!(
                    "retro must not issue per-worker update probes or unrelated queries: {query}"
                );
            };
            requests.push(request);
            let body = response.to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).expect("fixture response");
        }
        sender.send(requests).expect("request observations");
    });
    (endpoint, receiver)
}

fn worker(endpoint: &str, path: &Path) -> LiveLinear {
    let source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("retro-shared-fixture")),
        || None,
        endpoint,
    )
    .expect("explicit fixture credential");
    LiveLinear::with_source(source, path).expect("real retro worker")
}

fn workspaces(left: &LiveLinear, right: &LiveLinear) -> ix_board::WorkspaceSnapshot {
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            left.workspace(0.0).expect("first retro workspace")
        });
        let second = scope.spawn(|| {
            barrier.wait();
            right.workspace(0.0).expect("second retro workspace")
        });
        let first = first.join().expect("first worker completes");
        let second = second.join().expect("second worker completes");
        assert_eq!(
            first, second,
            "both workers must consume one complete publication"
        );
        first
    })
}

#[trace("FR-030-AC-5", "FR-031-AC-4")]
#[test]
fn two_live_retro_workers_reuse_full_and_delta_publications_and_meter_histories() {
    let directory = tempfile::tempdir().expect("isolated cache");
    let path = directory.path().join("linear.sqlite");
    let (endpoint, requests) = fixture();
    let left = worker(&endpoint, &path);
    let right = worker(&endpoint, &path);
    let baseline = workspaces(&left, &right);
    let quota = QuotaGate::open(&path, &QuotaConfig::default()).expect("shared meter");
    assert_eq!(
        quota.status().expect("cold meter").requests_used,
        2,
        "two workers share one project/issue baseline, with no update probes"
    );
    assert_eq!(workspaces(&left, &right), baseline);
    assert_eq!(
        quota.status().expect("warm meter").requests_used,
        2,
        "warm retro workers must not probe or force full reads"
    );

    // Expire actual successful fetch provenance without sleeping or making the
    // 24-hour full-reconciliation deadline due. This changes only fixture state.
    let mut expired = baseline.clone();
    expired.fetched_at -= 3600;
    let payload = serde_json::to_vec(&expired).expect("expired fixture baseline");
    let digest = format!("{:x}", Sha256::digest(&payload));
    rusqlite::Connection::open(&path).expect("fixture database").execute(
        "UPDATE workspace_snapshot SET fetched_at=?1,payload=?2,read_started_at=?1,read_started_for=?1,read_started_digest=?3",
        rusqlite::params![expired.fetched_at,payload,digest],
    ).expect("expire shared detail baseline");
    let refreshed = workspaces(&left, &right);
    assert!(refreshed.fetched_at > expired.fetched_at);
    assert_eq!(
        quota.status().expect("delta meter").requests_used,
        5,
        "two workers share one three-query delta instead of independently probing/full-scanning"
    );
    assert!(
        left.state_history("fixture-issue")
            .expect("first metered history")
            .is_empty()
    );
    assert!(
        right
            .state_history("fixture-issue")
            .expect("second metered history")
            .is_empty()
    );
    let metered = quota.status().expect("history meter");
    assert_eq!(
        metered.requests_used, 7,
        "both direct history requests consume shared allowance"
    );
    assert_eq!(
        metered.complexity_used, 70_000,
        "unknown history complexity is charged conservatively"
    );

    let requests = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("all fixture requests");
    assert_eq!(requests.len(), 7);
    for request in &requests[..2] {
        assert_eq!(request["variables"]["includeArchived"], false);
        assert!(request["variables"]["since"].is_null());
    }
    let expected = chrono::DateTime::from_timestamp(expired.fetched_at - 60, 0)
        .expect("watermark")
        .to_rfc3339();
    for request in &requests[2..5] {
        assert_eq!(request["variables"]["includeArchived"], true);
        let since = if request["query"]
            .as_str()
            .expect("query")
            .contains("SummaryInitiatives")
        {
            &request["variables"]["filter"]["updatedAt"]["gte"]
        } else {
            &request["variables"]["since"]
        };
        assert_eq!(
            since,
            &Value::String(expected.clone()),
            "shared delta must retain the last successful watermark overlap"
        );
    }
    for request in &requests[5..] {
        assert!(
            request["query"]
                .as_str()
                .expect("query")
                .contains("IssueHistory")
        );
    }
}
