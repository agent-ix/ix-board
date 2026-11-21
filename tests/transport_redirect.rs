// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Redirects cannot create transport attempts outside shared quota admission.

use ix_board::quota::{QuotaConfig, QuotaGate};
use ix_board::{LinearGraphQlSource, WorkspaceSource};
use ix_cli_kit::secrets::SecretStore;
use ix_trace_rs::trace;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn read_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("request timeout");
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let size = stream.read(&mut buffer).expect("request");
        assert_ne!(size, 0, "complete request");
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("length"))
                })
                .expect("body length");
            if bytes.len() >= end + 4 + length {
                return;
            }
        }
    }
}

#[trace("FR-031-AC-4")]
#[test]
fn a_redirect_target_receives_no_unreserved_request() {
    let origin = TcpListener::bind("127.0.0.1:0").expect("origin");
    let target = TcpListener::bind("127.0.0.1:0").expect("target");
    let endpoint = format!(
        "http://{}/graphql",
        origin.local_addr().expect("origin address")
    );
    let destination = format!(
        "http://{}/redirected",
        target.local_addr().expect("target address")
    );
    target.set_nonblocking(true).expect("observe destination");
    let (done, completed) = mpsc::channel();
    let destination_server = std::thread::spawn(move || {
        let started = Instant::now();
        loop {
            match target.accept() {
                Ok((mut stream, _)) => {
                    read_request(&mut stream);
                    let body = r#"{"data":{"initiatives":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}"#;
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).expect("target response");
                    return 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if completed.try_recv().is_ok() {
                        return 0;
                    }
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "fixture completes"
                    );
                    std::thread::yield_now();
                }
                Err(error) => panic!("target listener: {error}"),
            }
        }
    });
    let origin_server = std::thread::spawn(move || {
        let (mut stream, _) = origin.accept().expect("metered origin request");
        read_request(&mut stream);
        write!(stream,"HTTP/1.1 307 Temporary Redirect\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("redirect response");
        1
    });
    let directory = tempfile::tempdir().expect("quota cache");
    let path = directory.path().join("cache.sqlite3");
    let mut source = LinearGraphQlSource::from_credential_store(
        &SecretStore::system(),
        Some(OsString::from("redirect-fixture")),
        || None,
        &endpoint,
    )
    .expect("source");
    source.configure_cache(&path).expect("quota binding");
    let result = source.fetch_initiatives();
    let _ = done.send(());
    let actual_sends =
        origin_server.join().expect("origin") + destination_server.join().expect("destination");
    let attempts = QuotaGate::open(&path, &QuotaConfig::default())
        .expect("quota")
        .status()
        .expect("status")
        .requests_used;
    assert!(
        result.is_err(),
        "redirect is returned instead of silently followed"
    );
    assert_eq!(
        actual_sends, 1,
        "redirect destination receives no implicit request"
    );
    assert_eq!(attempts, u64::try_from(actual_sends).expect("send count"));
}
