// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! One HTTP attempt per admission, with a locally resolved, bound credential.
use super::github::{Request, Response, Transport};
use ix_cli_kit::secrets::SecretValue;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, time::Duration};

pub(super) struct Http {
    client: Client,
    endpoint: reqwest::Url,
    credential: SecretValue,
    namespace: String,
}
impl Http {
    pub(super) fn local() -> Result<Self, String> {
        #[cfg(unix)]
        {
            // This local provider resolves gh's environment/config/keyring precedence.
            // Its stdout is captured only inside this process, never printed or stored.
            let mut command = std::process::Command::new("gh");
            command.args(["auth", "token", "--hostname", "github.com"]);
            let output =
                super::history::command_output(&mut command, Duration::from_secs(3), 16 * 1024)
                    .map_err(|_| "GitHub local credential provider unavailable")?;
            if !output.status.success() {
                return Err("GitHub local credential provider unavailable".into());
            }
            let token =
                String::from_utf8(output.bytes).map_err(|_| "GitHub credential is not UTF-8")?;
            let token = token.trim();
            if token.is_empty() || !token.bytes().all(|b| b.is_ascii_graphic()) {
                return Err("GitHub local credential is invalid".into());
            }
            Self::new("https://api.github.com/graphql", SecretValue::new(token))
        }
        #[cfg(not(unix))]
        {
            Err("GitHub local credential provider requires Unix/WSL".into())
        }
    }
    fn new(endpoint: &str, credential: SecretValue) -> Result<Self, String> {
        let endpoint =
            reqwest::Url::parse(endpoint).map_err(|_| "invalid GitHub provider endpoint")?;
        if endpoint.scheme() != "https"
            && !(endpoint.scheme() == "http"
                && endpoint
                    .host_str()
                    .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                    .is_some_and(|a| a.is_loopback()))
        {
            return Err("GitHub provider requires HTTPS".into());
        }
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .pool_max_idle_per_host(0)
            .no_proxy()
            .timeout(Duration::from_secs(20))
            .user_agent("ix-team-exporter")
            .build()
            .map_err(|_| "GitHub HTTP client unavailable")?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"ix-board/github/credential/v2\0");
        fingerprint.update(endpoint.as_str().as_bytes());
        fingerprint.update(b"\0");
        fingerprint.update(credential.expose_secret().as_bytes());
        let namespace = format!("{:x}", fingerprint.finalize());
        Ok(Self {
            client,
            endpoint,
            credential,
            namespace,
        })
    }
}
impl Transport for Http {
    fn namespace(&self) -> &str {
        &self.namespace
    }
    fn send(&self, request: &Request, etag: Option<&str>) -> Result<Response, String> {
        let body = request.encoded()?;
        let method = if request.conditional {
            reqwest::Method::GET
        } else {
            reqwest::Method::POST
        };
        let mut builder = self
            .client
            .request(method, self.endpoint.clone())
            .bearer_auth(self.credential.expose_secret())
            .header("Accept", "application/vnd.github+json");
        if request.conditional {
            let variables = serde_json::to_string(&request.variables)
                .map_err(|_| "invalid GitHub variables")?;
            builder = builder.query(&[
                ("query", request.query.as_str()),
                ("variables", variables.as_str()),
            ]);
        } else {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        if let Some(etag) = etag {
            builder = builder.header("If-None-Match", etag);
        }
        let outgoing = builder.build().map_err(|_| "invalid GitHub HTTP request")?;
        if request.conditional && outgoing.url().as_str().len() > super::github::MAX_QUERY {
            return Err("GitHub encoded query URL byte limit exceeded".into());
        }
        let mut response = self
            .client
            .execute(outgoing)
            .map_err(|_| "GitHub HTTP attempt failed")?;
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for name in [
            "etag",
            "retry-after",
            "x-ratelimit-remaining",
            "x-ratelimit-reset",
        ] {
            if let Some(value) = response.headers().get(name) {
                let value = value
                    .to_str()
                    .map_err(|_| "invalid GitHub response metadata")?;
                if value.len() > 1024 {
                    return Err("oversize GitHub response metadata".into());
                }
                headers.insert(name.into(), value.into());
            }
        }
        let mut bytes = Vec::new();
        // Preserve status and cooldown metadata even if a body is invalid, oversized
        // or interrupted. An empty 200 body cannot pass the typed payload validator.
        let body = match response
            .by_ref()
            .take(
                u64::try_from(super::github::MAX_BODY).map_err(|_| "GitHub body bound overflow")?
                    + 1,
            )
            .read_to_end(&mut bytes)
        {
            Ok(_) if bytes.len() <= super::github::MAX_BODY => {
                String::from_utf8(bytes).unwrap_or_default()
            }
            _ => String::new(),
        };
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
impl Http {
    pub(super) fn fixture(endpoint: &str, token: &str) -> Result<Self, String> {
        if token.is_empty() {
            return Err("fixture credential unavailable".into());
        }
        Self::new(endpoint, SecretValue::new(token))
    }
}
#[cfg(test)]
pub(in crate::team_metrics) mod tests {
    use super::*;
    use ix_trace_rs::trace;
    use std::{io::Write, net::TcpListener, sync::mpsc, thread, time::Instant};

    pub(in crate::team_metrics) struct Fixture {
        pub(in crate::team_metrics) endpoint: String,
        stop: mpsc::Sender<()>,
        worker: thread::JoinHandle<Vec<String>>,
    }
    impl Fixture {
        pub(in crate::team_metrics) fn start(
            handler: impl Fn(usize, &str) -> String + Send + 'static,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/graphql", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let (stop, rx) = mpsc::channel();
            let worker = thread::spawn(move || {
                let start = Instant::now();
                let mut requests = Vec::new();
                while rx.try_recv().is_err() && start.elapsed() < Duration::from_secs(10) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream.set_nonblocking(false).unwrap();
                            stream
                                .set_read_timeout(Some(Duration::from_secs(2)))
                                .unwrap();
                            let mut bytes = Vec::new();
                            loop {
                                let mut chunk = [0; 4096];
                                let n = stream.read(&mut chunk).unwrap();
                                if n == 0 {
                                    break;
                                }
                                bytes.extend_from_slice(&chunk[..n]);
                                assert!(bytes.len() < 32 * 1024);
                                let text = String::from_utf8_lossy(&bytes);
                                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                                    let length = head
                                        .lines()
                                        .find_map(|line| {
                                            line.to_ascii_lowercase()
                                                .strip_prefix("content-length:")
                                                .and_then(|v| v.trim().parse::<usize>().ok())
                                        })
                                        .unwrap_or(0);
                                    if body.len() >= length {
                                        break;
                                    }
                                }
                            }
                            let request = String::from_utf8(bytes).unwrap();
                            requests.push(request);
                            let reply = handler(requests.len(), requests.last().unwrap());
                            stream.write_all(reply.as_bytes()).unwrap();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::yield_now(),
                        Err(e) => panic!("offline fixture listener: {e}"),
                    }
                }
                requests
            });
            Self {
                endpoint,
                stop,
                worker,
            }
        }
        pub(in crate::team_metrics) fn finish(self) -> Vec<String> {
            self.stop.send(()).unwrap();
            self.worker.join().unwrap()
        }
    }
    pub(in crate::team_metrics) fn reply(status: u16, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    #[trace("FR-033-AC-12")]
    #[test]
    fn controlled_http_never_follows_redirects_or_retries_error_responses() {
        for status in [302, 307, 429, 503] {
            let server = Fixture::start(move |_, _| reply(status, "Location: /again\r\n", "{}"));
            let transport = Http::fixture(&server.endpoint, "offline-credential").unwrap();
            let request = Request {
                query: "query { fixture }".into(),
                variables: BTreeMap::new(),
                conditional: false,
            };
            assert_eq!(transport.send(&request, None).unwrap().status, status);
            assert_eq!(
                server.finish().len(),
                1,
                "each admission must cause exactly one HTTP request"
            );
        }
    }

    #[trace("FR-033-AC-12", "FR-033-AC-13")]
    #[test]
    fn controlled_http_preserves_error_metadata_and_bounds_large_bodies() {
        let server = Fixture::start(|_, _| {
            reply(
                429,
                "Retry-After: 7200\r\nX-RateLimit-Remaining: 0\r\n",
                &"x".repeat(super::super::github::MAX_BODY + 1),
            )
        });
        let transport = Http::fixture(&server.endpoint, "offline-credential").unwrap();
        let request = Request {
            query: "query { fixture }".into(),
            variables: BTreeMap::new(),
            conditional: false,
        };
        let response = transport.send(&request, None).unwrap();
        assert_eq!(response.status, 429);
        assert_eq!(response.headers["retry-after"], "7200");
        assert_eq!(response.headers["x-ratelimit-remaining"], "0");
        assert!(response.body.is_empty());
        assert_eq!(server.finish().len(), 1);
    }
}
