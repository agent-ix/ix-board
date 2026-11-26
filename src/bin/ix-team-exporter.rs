// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! Cached Prometheus metrics for explicitly configured operational teams.
#![forbid(unsafe_code)]
use clap::Parser;
use ix_board::team_metrics::{Collector, Config, Error, now_ms};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, RwLock, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(about = "Export bounded operational-team flow and effort metrics")]
struct Args {
    /// Version-one configuration JSON, containing only credential references/paths.
    #[arg(long)]
    config: PathBuf,
    /// Validate configuration and exit without source reads or persistent writes.
    #[arg(long)]
    check_config: bool,
}
struct Cache {
    metrics: String,
    updated: Instant,
}
fn serve(mut stream: TcpStream, cache: &RwLock<Cache>) -> Result<(), Error> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::with_capacity(1024);
    let deadline = Instant::now() + Duration::from_secs(2);
    let line_end = loop {
        if let Some(end) = bytes.windows(2).position(|part| part == b"\r\n") {
            break end;
        }
        if bytes.len() >= 4096 || Instant::now() >= deadline {
            return Err(Error::Invalid("HTTP request line exceeded budget"));
        }
        stream.set_read_timeout(Some(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ))?;
        let mut part = [0; 1024];
        let n = stream.read(&mut part)?;
        if n == 0 {
            return Err(Error::Invalid("incomplete HTTP request line"));
        }
        bytes.extend_from_slice(&part[..n]);
    };
    let first = std::str::from_utf8(&bytes[..line_end]).unwrap_or("");
    let cache = cache
        .read()
        .map_err(|_| Error::Invalid("metrics cache poisoned"))?;
    let (status, body, content) = match first {
        "GET /metrics HTTP/1.1" | "GET /metrics HTTP/1.0"
            if cache.updated.elapsed() >= Duration::from_secs(120) =>
        {
            (
                "503 Service Unavailable",
                "collection worker stale\n".into(),
                "text/plain",
            )
        }
        "GET /metrics HTTP/1.1" | "GET /metrics HTTP/1.0" => (
            "200 OK",
            cache.metrics.clone(),
            "text/plain; version=0.0.4; charset=utf-8",
        ),
        "GET /healthz HTTP/1.1" | "GET /healthz HTTP/1.0" => {
            ("200 OK", "ok\n".into(), "text/plain")
        }
        "GET /readyz HTTP/1.1" | "GET /readyz HTTP/1.0"
            if cache.updated.elapsed() < Duration::from_secs(120) =>
        {
            ("200 OK", "ready\n".into(), "text/plain")
        }
        "GET /readyz HTTP/1.1" | "GET /readyz HTTP/1.0" => (
            "503 Service Unavailable",
            "collection worker stale\n".into(),
            "text/plain",
        ),
        _ => ("404 Not Found", "not found\n".into(), "text/plain"),
    };
    drop(cache);
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    Ok(())
}
/// The cached collector metrics plus the last committed `fleet tick`, when a ledger is configured.
fn metrics_text(collector: &Collector, config: &Config, now: i64) -> Result<String, Error> {
    let mut text = collector.metrics(now)?;
    if let Some(path) = &config.fleet_ledger {
        let interval = i64::try_from(config.fleet_tick_interval_seconds * 1000).unwrap_or(i64::MAX);
        match ix_board::fleet::ledger::tick_metrics(path, now, interval) {
            Ok(tick) => text.push_str(&tick),
            Err(error) => eprintln!("fleet tick metrics: {error}"),
        }
        match ix_board::fleet::ledger::capacity::metrics(path) {
            Ok(capacity) => text.push_str(&capacity),
            Err(error) => eprintln!("fleet capacity metrics: {error}"),
        }
    }
    Ok(text)
}

fn tracker_worker_configs(config: &Config) -> [Config; 1] {
    [config.clone()]
}

fn run() -> Result<(), Error> {
    // SystemHost's local lifecycle transport invokes the current executable.
    // Support the same bounded, read-only protocol as the ix-board CLI.
    if std::env::args().skip(1).collect::<Vec<_>>() == ["__agent-probe"] {
        let home = std::env::var_os("HOME").ok_or(Error::Invalid("home unavailable"))?;
        let response = ix_board::agents::serve_probe(std::io::stdin().lock(), &PathBuf::from(home))
            .map_err(|e| Error::Source(e.to_string()))?;
        println!("{response}");
        return Ok(());
    }
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    if args.check_config {
        println!("team metrics configuration valid");
        return Ok(());
    }
    let listener = TcpListener::bind(config.listen)?;
    let mut collector = Collector::open(config.clone())?;
    let cache = Arc::new(RwLock::new(Cache {
        metrics: metrics_text(&collector, &config, now_ms()?)?,
        updated: Instant::now(),
    }));
    let (sender, receiver) = mpsc::sync_channel(1);
    for tracker_config in tracker_worker_configs(&config) {
        let sender = sender.clone();
        thread::spawn(move || {
            loop {
                match now_ms() {
                    Ok(now) => {
                        if sender
                            .send(Collector::tracker(&tracker_config, now))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => eprintln!("tracker clock: {error}"),
                }
                thread::sleep(Duration::from_secs(tracker_config.tracker_interval_seconds));
            }
        });
    }
    drop(sender);
    let output = Arc::clone(&cache);
    let worker_config = config.clone();
    thread::spawn(move || {
        loop {
            let tick = Instant::now();
            let result = (|| -> Result<(), Error> {
                while let Ok(batch) = receiver.try_recv() {
                    collector.apply_tracker(batch)?;
                }
                if let Err(error) = collector.activity(now_ms()?) {
                    eprintln!("activity evidence: {error}");
                }
                let metrics = metrics_text(&collector, &worker_config, now_ms()?)?;
                let mut target = output
                    .write()
                    .map_err(|_| Error::Invalid("metrics cache poisoned"))?;
                target.metrics = metrics;
                target.updated = Instant::now();
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("team collection: {error}");
            }
            thread::sleep(
                Duration::from_secs(config.activity_interval_seconds)
                    .saturating_sub(tick.elapsed()),
            );
        }
    });
    // One bounded request at a time: no unbounded task/thread-per-scrape growth.
    for stream in listener.incoming() {
        if let Err(error) = serve(stream?, &cache) {
            eprintln!("metrics HTTP: {error}");
        }
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_board::team_metrics::{ScopeConfig, Selectors};
    fn two_scope_config(path: &std::path::Path) -> Config {
        let scope = |team_id: &str, scope_id: &str, team: &str| ScopeConfig {
            team_id: team_id.into(),
            scope_id: scope_id.into(),
            selectors: Selectors {
                team: Some(team.into()),
                ..Default::default()
            },
            leads: Vec::new(),
            herdr_teams: Vec::new(),
            retro_config: path.join(format!("{team_id}.json")),
        };
        Config {
            version: 1,
            listen: "127.0.0.1:0".parse().unwrap(),
            state_path: path.join("events.sqlite"),
            max_bytes: 1024 * 1024,
            retention_seconds: 604800,
            activity_interval_seconds: 30,
            tracker_interval_seconds: 300,
            fleet_ledger: None,
            fleet_tick_interval_seconds: 300,
            prometheus_url: Some("http://127.0.0.1:9090".into()),
            scopes: vec![
                scope("fsl", "completion", "QSL"),
                scope("swarm-tools", "department-monitoring", "SWM"),
            ],
        }
    }

    #[test]
    fn tracker_worker_keeps_all_scopes_behind_one_shared_gate() {
        let directory = tempfile::tempdir().unwrap();
        let config = two_scope_config(directory.path());
        let workers = tracker_worker_configs(&config);

        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].scopes.len(), 2);
        assert_eq!(
            workers[0]
                .scopes
                .iter()
                .map(|scope| format!("{}:{}", scope.team_id, scope.scope_id))
                .collect::<Vec<_>>(),
            ["fsl:completion", "swarm-tools:department-monitoring"]
        );
    }

    use ix_trace_rs::trace;
    #[trace("FR-033-AC-8")]
    #[test]
    fn fragmented_http_line_serves_the_cached_metrics() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let cache = Arc::new(RwLock::new(Cache {
            metrics: "test_metric 7\n".into(),
            updated: Instant::now(),
        }));
        let target = Arc::clone(&cache);
        let worker = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, &target).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(b"GET /met").unwrap();
        client
            .write_all(b"rics HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        worker.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("test_metric 7\n"));
    }
}
