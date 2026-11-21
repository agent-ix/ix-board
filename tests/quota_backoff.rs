// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Actual server cooldown dominates ordinary, direct, history and forced transport.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ix_board::quota::{QuotaClock, QuotaConfig, QuotaGate, QuotaState};
use ix_board::{Error, LinearGraphQlSource, ReadService, WorkspaceSource};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use reqwest::header::HeaderMap;

struct Clock(i64);
impl QuotaClock for Clock {
    fn now_epoch_millis(&self) -> i64 {
        self.0
    }
}

#[trace("FR-032-AC-7", "FR-031-AC-1", "FR-031-AC-2")]
#[test]
fn actual_http_and_graphql_backoff_survives_resets_high_observations_reopen_and_all_paths() {
    for graphql in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.sqlite");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let fixture = listener.try_clone().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = fixture.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "complete HTTP request");
                input.extend_from_slice(&chunk[..count]);
                assert!(input.len() < 65536, "fixture request bounded");
                if let Some(end) = input.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&input[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    if input.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let (status, body) = if graphql {
                (
                    "400 Bad Request",
                    r#"{"errors":[{"message":"overloaded","extensions":{"code":"RATELIMITED","retryAfter":90}}]}"#,
                )
            } else {
                ("429 Too Many Requests", "not json")
            };
            write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nRetry-After: 60\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            1
        });
        let make_source = || {
            let mut source = LinearGraphQlSource::from_credential_store(
                &SecretStore::system(),
                Some(OsString::from("fixture-key")),
                || None,
                &endpoint,
            )
            .unwrap();
            source.configure_cache(&path).unwrap();
            source
        };
        let source = make_source();
        assert!(
            matches!(
                source.fetch_workspace(),
                Err(Error::LinearRateLimited { .. })
            ),
            "legacy actual-server typed contract"
        );
        assert_eq!(worker.join().unwrap(), 1);
        let gate = QuotaGate::open(&path, &QuotaConfig::default()).unwrap();
        let until = gate.status().unwrap().rate_limit_until.unwrap();
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        assert!(until >= now + if graphql { 88000 } else { 58000 });
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("x-ratelimit-requests-limit", "10000".to_owned()),
            ("x-ratelimit-requests-remaining", "10000".to_owned()),
            ("x-ratelimit-requests-reset", (until + 60_000).to_string()),
        ] {
            headers.insert(name, value.parse().unwrap());
        }
        gate.observe_headers(200, &headers, None).unwrap();
        rusqlite::Connection::open(&path).unwrap().execute("UPDATE linear_shared_quota SET local_reset=?1,complexity_reset=?1 WHERE singleton=1",[now-1]).unwrap();
        let reopened = QuotaGate::open(&path, &QuotaConfig::default()).unwrap();
        let state = reopened.status().unwrap();
        assert_eq!(state.rate_limit_until, Some(until));
        assert_eq!(state.requests_used, 0);
        let before = reopened.decision(10).unwrap();
        assert_eq!(before.state, QuotaState::ServerBackoff);
        let second = make_source();
        for _ in 0..2 {
            assert!(matches!(
                second.fetch_workspace(),
                Err(Error::LinearRateLimited { .. })
            ));
            assert!(matches!(
                second.fetch_summary(),
                Err(Error::LinearRateLimited { .. })
            ));
            for result in [
                second
                    .fetch_state_changes("00000000-0000-0000-0000-000000000001")
                    .map(|_| ()),
                second.fetch_issue_updates(0.0).map(|_| ()),
                second.fetch_marker_lines("x", "y").map(|_| ()),
            ] {
                assert!(
                    result
                        .unwrap_err()
                        .contains("Linear rate limited; retry in ")
                );
            }
        }
        let mut service = ReadService::open(make_source(), &path).unwrap();
        assert!(matches!(
            service.force_refresh_workspace(),
            Err(Error::LinearRateLimited { .. })
        ));
        assert!(
            matches!(service.refresh_outcome(),ix_board::RefreshOutcome::QuotaRefused {decision} if decision.state==QuotaState::ServerBackoff)
        );
        assert_eq!(reopened.status().unwrap().requests_used, 0);
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "zero protected requests while cooldown active"
        );
        let at_expiry =
            QuotaGate::open_with_clock(&path, &QuotaConfig::default(), Arc::new(Clock(until)))
                .unwrap();
        assert_eq!(at_expiry.decision(10).unwrap().state, QuotaState::Available);
        assert_eq!(
            at_expiry.status().unwrap().requests_used,
            0,
            "expiry itself sends and reserves nothing"
        );
        at_expiry.reserve_evaluated(10, None).unwrap();
        assert_eq!(
            at_expiry.status().unwrap().requests_used,
            1,
            "only an ordinary protected admission changes accounting"
        );
    }
}
