// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The public credential construction seam feeds the real cached Linear reader.

use ix_board::retro::department::Scope;
use ix_board::retro::live::LiveLinear;
use ix_board::retro::sources::LinearSource;
use ix_board::{Error, LinearGraphQlSource};
use ix_cli_kit::secrets::{
    AppScope, CredentialBackend, DeleteStatus, Presence, SecretError, SecretKey, SecretStore,
    SecretValue,
};
use ix_trace_rs::trace;
use std::cell::Cell;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::time::Duration;

struct SavedProvider {
    value: Result<Option<&'static str>, SecretError>,
    reads: Rc<Cell<usize>>,
}

impl CredentialBackend for SavedProvider {
    fn get(&self, scope: &AppScope, key: &SecretKey) -> Result<Option<SecretValue>, SecretError> {
        assert_eq!(scope.as_str(), "ix-board");
        assert_eq!(key.as_str(), "linear-api-key");
        self.reads.set(self.reads.get() + 1);
        self.value.map(|value| value.map(SecretValue::new))
    }

    fn set(&self, _: &AppScope, _: &SecretKey, _: &SecretValue) -> Result<(), SecretError> {
        panic!("source construction must not write credentials")
    }

    fn delete(&self, _: &AppScope, _: &SecretKey) -> Result<DeleteStatus, SecretError> {
        panic!("source construction must not delete credentials")
    }

    fn status(&self, _: &AppScope, _: &SecretKey) -> Result<Presence, SecretError> {
        panic!("source construction must not probe credential status")
    }
}

fn provider(
    value: Result<Option<&'static str>, SecretError>,
) -> (SecretStore<SavedProvider>, Rc<Cell<usize>>) {
    let reads = Rc::new(Cell::new(0));
    let store = SecretStore::new(SavedProvider {
        value,
        reads: Rc::clone(&reads),
    });
    (store, reads)
}

fn no_fallback() -> Option<String> {
    panic!("CLI fallback must not run for a saved key or a credential failure")
}

#[trace("FR-015-AC-2", "FR-029-AC-4", "FR-029-AC-8")]
#[test]
fn saved_provider_without_override_reaches_real_department_collection() {
    // A subprocess supplies a conflicting environment without mutating the
    // parallel test process. The None snapshot must still select the saved key.
    const CHILD: &str = "IX_BOARD_SAVED_PROVIDER_FIXTURE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "saved_provider_without_override_reaches_real_department_collection",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("LINEAR_API_KEY", "fixture-global-env-must-not-be-read")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "saved-provider child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/graphql", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut operations = Vec::new();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let (header_end, length) = loop {
                let size = stream.read(&mut buffer).unwrap();
                assert_ne!(size, 0, "complete GraphQL request headers required");
                request.extend_from_slice(&buffer[..size]);
                if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    assert!(
                        headers.lines().any(|line| {
                            line.eq_ignore_ascii_case("authorization: fixture-saved-key")
                        }),
                        "real HTTP request must use the saved provider's key"
                    );
                    break (end + 4, length);
                }
            };
            while request.len() < header_end + length {
                let size = stream.read(&mut buffer).unwrap();
                assert_ne!(size, 0, "complete GraphQL body required");
                request.extend_from_slice(&buffer[..size]);
            }
            let request: serde_json::Value =
                serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
            let query = request["query"].as_str().unwrap();
            let resource = if query.contains("WorkspaceProjects") {
                "projects"
            } else {
                assert!(query.contains("issues("), "expected real issue queries");
                "issues"
            };
            operations.push(query.to_owned());
            let body = serde_json::json!({"data":{(resource):{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        operations
    });
    let (store, reads) = provider(Ok(Some("fixture-saved-key")));
    let source = LinearGraphQlSource::from_credential_store(&store, None, no_fallback, &endpoint)
        .expect("saved key must satisfy real source construction without environment override");
    let cache = tempfile::tempdir().unwrap();
    let cache_path = cache.path().join("isolated.sqlite");
    let linear = LiveLinear::with_source(source, &cache_path).unwrap();
    let evidence = linear.department_evidence(&Scope::default(), 0.0);
    assert_eq!(evidence.source, "linear");
    assert!(
        evidence.complete.contains("issue"),
        "real workspace must be collected: {:?}",
        evidence.reasons
    );
    assert!(evidence.complete.contains("state"));
    assert!(cache_path.exists(), "explicit isolated cache must be used");
    assert_eq!(
        reads.get(),
        1,
        "reader must not resolve credentials a second time"
    );
    let operations = server.join().unwrap();
    assert_eq!(operations.len(), 3);
    assert!(operations[0].contains("WorkspaceProjects"));
    assert!(
        operations[2].contains("updatedAt"),
        "real freshness read must run"
    );
}

#[trace("FR-015-AC-2")]
#[test]
fn explicit_environment_snapshot_precedes_provider() {
    let (store, reads) = provider(Err(SecretError::Locked));
    LinearGraphQlSource::from_credential_store(
        &store,
        Some(OsString::from("fixture-environment-key")),
        no_fallback,
        "http://127.0.0.1:1/graphql",
    )
    .expect("environment override must avoid the denied provider");
    assert_eq!(reads.get(), 0);
}

#[trace("FR-015-AC-3")]
#[test]
fn invalid_environment_snapshot_never_falls_through() {
    let (store, reads) = provider(Ok(Some("fixture-saved-key")));
    let mut invalid = vec![OsString::new()];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        invalid.push(OsString::from_vec(vec![0xff]));
    }
    for value in invalid {
        assert!(matches!(
            LinearGraphQlSource::from_credential_store(
                &store,
                Some(value),
                no_fallback,
                "http://127.0.0.1:1/graphql"
            ),
            Err(Error::Credential(SecretError::InvalidEnvironment))
        ));
    }
    assert!(matches!(
        LinearGraphQlSource::from_credential_store(
            &store,
            Some(OsString::from("   ")),
            no_fallback,
            "http://127.0.0.1:1/graphql"
        ),
        Err(Error::MissingApiKey)
    ));
    assert_eq!(reads.get(), 0);
}

#[trace("FR-015-AC-2")]
#[test]
fn provider_access_failures_propagate_without_cli_fallback() {
    for error in [
        SecretError::Locked,
        SecretError::Unavailable,
        SecretError::BackendFailure,
    ] {
        let (store, reads) = provider(Err(error));
        assert!(matches!(
            LinearGraphQlSource::from_credential_store(
                &store, None, no_fallback, "http://127.0.0.1:1/graphql"
            ),
            Err(Error::Credential(actual)) if actual == error
        ));
        assert_eq!(reads.get(), 1);
    }
}

#[trace("FR-015-AC-2", "FR-015-AC-3")]
#[test]
fn cli_fallback_runs_only_after_absent_provider_and_missing_stays_error() {
    let (store, reads) = provider(Ok(None));
    let fallback_calls = Cell::new(0);
    LinearGraphQlSource::from_credential_store(
        &store,
        None,
        || {
            fallback_calls.set(fallback_calls.get() + 1);
            Some("fixture-cli-key".to_owned())
        },
        "http://127.0.0.1:1/graphql",
    )
    .unwrap();
    assert_eq!(fallback_calls.get(), 1);
    assert!(matches!(
        LinearGraphQlSource::from_credential_store(
            &store,
            None,
            || None,
            "http://127.0.0.1:1/graphql"
        ),
        Err(Error::MissingApiKey)
    ));
    assert_eq!(reads.get(), 2);
}
