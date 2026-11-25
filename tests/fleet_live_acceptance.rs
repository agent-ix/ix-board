// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Explicit live acceptance runner for SWM-252/217/282.
//!
//! The runner never calls `fleet tick` and never writes to live relay or source systems. It copies
//! the operator-supplied config and optional ledger into a temporary directory, then runs the CLI
//! against read-only Prometheus, relay, Linear, and GitHub sources. Capacity windows require real
//! host/relay-time facts supplied in the scenario file; the runner rejects future or too-short
//! windows instead of substituting current samples.

use ix_trace_rs::trace;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[derive(Debug, Deserialize)]
struct Scenario {
    hosts: Vec<HostWindow>,
}

#[derive(Debug, Deserialize)]
struct HostWindow {
    host: String,
    lane_address: String,
    ticket: Option<String>,
    expect_work_event: bool,
    trigger: String,
    run_started_at_ms: i64,
    lanes: u32,
    lane_add_evidence: String,
    lane_added_at_ms: i64,
    rampup_end_ms: i64,
    window_end_ms: i64,
    repeat_window_end_ms: i64,
    end_reason: String,
}

#[test]
#[trace("FR-041-AC-11")]
#[ignore = "requires explicit live-source acceptance config and measured host windows"]
fn live_acceptance_uses_temporary_state_and_prints_source_evidence() {
    let config_source = required_path("IX_BOARD_LIVE_ACCEPTANCE_CONFIG");
    let scenario_source = required_path("IX_BOARD_LIVE_ACCEPTANCE_SCENARIO");
    let ledger_source = std::env::var_os("IX_BOARD_LIVE_ACCEPTANCE_LEDGER").map(PathBuf::from);
    let prometheus_url = std::env::var("IX_PROMETHEUS_URL")
        .expect("set IX_PROMETHEUS_URL to the read-only Prometheus endpoint");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_millis();
    let now_ms = i64::try_from(now_ms).expect("current timestamp must fit i64 milliseconds");

    let scenario: Scenario =
        serde_json::from_slice(&fs::read(&scenario_source).expect("read acceptance scenario"))
            .expect("scenario must be JSON");
    assert!(
        scenario
            .hosts
            .iter()
            .any(|host| host.host.eq_ignore_ascii_case("draco")),
        "scenario must include draco"
    );
    assert!(
        scenario
            .hosts
            .iter()
            .any(|host| host.host.eq_ignore_ascii_case("titan")),
        "scenario must include titan"
    );
    for host in &scenario.hosts {
        assert_work_event_coverage(host);
    }

    let temporary = tempfile::tempdir().expect("create temporary acceptance directory");
    let config_path = temporary.path().join("ix-board-config.toml");
    let ledger_path = temporary.path().join("fleet-ledger.sqlite3");
    let cache_path = temporary.path().join("cache");
    fs::create_dir_all(&cache_path).expect("create temporary cache");
    fs::copy(&config_source, &config_path).expect("copy config into temporary directory");
    if let Some(ledger_source) = ledger_source {
        fs::copy(ledger_source, &ledger_path).expect("copy ledger into temporary directory");
    }

    let saturation = print_json_command(
        &config_path,
        &ledger_path,
        &cache_path,
        &prometheus_url,
        &["fleet", "saturation", "--json"],
    );
    for host_name in ["draco", "titan"] {
        let host = saturation["hosts"]
            .as_array()
            .expect("saturation report lists hosts")
            .iter()
            .find(|host| host["host"].as_str() == Some(host_name))
            .unwrap_or_else(|| panic!("saturation report includes {host_name}"));
        assert!(
            host["supplied_lanes"].is_number() && host["unsupplied_lanes"].is_number(),
            "{host_name} assignment evidence must come from the relay assignment and work-event readers"
        );
        assert_eq!(
            host["ready_share"]["availability"], "available",
            "{host_name} must have a real ready share independent of duty and deficit"
        );
    }
    print_json_command(
        &config_path,
        &ledger_path,
        &cache_path,
        &prometheus_url,
        &["fleet", "saturation", "collect", "--json"],
    );

    for host in scenario.hosts {
        assert!(host.lanes > 0, "{} lane count must be positive", host.host);
        assert!(
            !host.lane_add_evidence.trim().is_empty(),
            "{} must name the temporary ledger evidence basis",
            host.host
        );
        assert!(
            host.run_started_at_ms <= host.lane_added_at_ms
                && host.lane_added_at_ms < host.rampup_end_ms
                && host.rampup_end_ms < host.window_end_ms
                && host.window_end_ms < host.repeat_window_end_ms
                && host.repeat_window_end_ms <= now_ms,
            "{} scenario timestamps must be ordered and cannot be in the future",
            host.host
        );

        print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "start",
                &host.host,
                "--trigger",
                &host.trigger,
                "--at-ms",
                &host.run_started_at_ms.to_string(),
                "--json",
            ],
        );
        print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "step",
                &host.host,
                "--lanes",
                &host.lanes.to_string(),
                "--lane-added-at-ms",
                &host.lane_added_at_ms.to_string(),
                "--rampup-end-ms",
                &host.rampup_end_ms.to_string(),
                "--json",
            ],
        );
        let first_window = print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "window",
                &host.host,
                "--end-ms",
                &host.window_end_ms.to_string(),
                "--json",
            ],
        );
        if host.host.eq_ignore_ascii_case("draco") && host.expect_work_event {
            assert_eq!(
                first_window["host_output_per_hour"]["availability"], "available",
                "a Linear-linked PR without a ticket id in its title/branch must not void host output"
            );
            assert!(
                first_window["derived_sources"]["output_attribution_basis"]
                    .as_str()
                    .is_some_and(|basis| basis.contains("linear_pull_request_attachment")),
                "host output must record Linear attachment attribution"
            );
        }
        print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "step",
                &host.host,
                "--lanes",
                &host.lanes.to_string(),
                "--lane-added-at-ms",
                &host.lane_added_at_ms.to_string(),
                "--rampup-end-ms",
                &host.rampup_end_ms.to_string(),
                "--repeat-of",
                "1",
                "--json",
            ],
        );
        print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "window",
                &host.host,
                "--end-ms",
                &host.repeat_window_end_ms.to_string(),
                "--json",
            ],
        );
        print_json_command(
            &config_path,
            &ledger_path,
            &cache_path,
            &prometheus_url,
            &[
                "fleet",
                "capacity",
                "end",
                &host.host,
                "--reason",
                &host.end_reason,
                "--at-ms",
                &host.repeat_window_end_ms.to_string(),
                "--json",
            ],
        );
    }

    print_json_command(
        &config_path,
        &ledger_path,
        &cache_path,
        &prometheus_url,
        &["fleet", "capacity", "status", "--json"],
    );
}

#[derive(Debug, Deserialize)]
struct WorkEventsPage {
    available_from_ms: i64,
    has_more: bool,
    next_cursor: Option<String>,
    truncated: bool,
    events: Vec<WorkEvent>,
}

#[derive(Debug, Deserialize)]
struct WorkEvent {
    at_ms: i64,
    binding: WorkBinding,
    change: WorkChange,
}

#[derive(Debug, Deserialize)]
struct WorkBinding {
    address: String,
}

#[derive(Debug, Deserialize)]
struct WorkChange {
    agent: Option<WorkAgent>,
}

#[derive(Debug, Deserialize)]
struct WorkAgent {
    ticket: String,
    status: String,
}

fn assert_work_event_coverage(host: &HostWindow) {
    let window_start_ms = host.rampup_end_ms;
    let mut cursor: Option<String> = None;
    let mut available_from_ms: Option<i64> = None;
    let mut has_match = false;
    let mut active_at_window_start = false;
    let mut pages = 0usize;
    loop {
        let mut command = Command::new("agent-msg");
        command.args(["work", "events", "--limit", "400"]);
        if let Some(cursor) = &cursor {
            command.args(["--after", cursor]);
        }
        let output = command
            .output()
            .expect("run read-only agent-msg work events");
        assert!(
            output.status.success(),
            "agent-msg work events failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let page: WorkEventsPage = serde_json::from_slice(&output.stdout)
            .expect("agent-msg work events returned invalid JSON");
        assert!(!page.truncated, "work-event page is truncated");
        if let Some(boundary) = available_from_ms {
            assert_eq!(
                boundary, page.available_from_ms,
                "work-event coverage boundary changed during pagination"
            );
        } else {
            available_from_ms = Some(page.available_from_ms);
        }
        for event in page.events {
            if event.at_ms > host.window_end_ms
                || event.binding.address != host.lane_address
                || !event
                    .change
                    .agent
                    .as_ref()
                    .zip(host.ticket.as_ref())
                    .is_some_and(|(agent, ticket)| agent.ticket.eq_ignore_ascii_case(ticket))
            {
                continue;
            }
            let status = event.change.agent.as_ref().unwrap().status.as_str();
            if event.at_ms < window_start_ms {
                active_at_window_start = status == "running";
            } else if status == "running" || status == "done" {
                has_match = true;
                active_at_window_start = status == "running";
            }
        }
        pages += 1;
        assert!(
            pages <= 10_000,
            "work-event pagination exceeded its safety bound"
        );
        if !page.has_more {
            break;
        }
        let next = page.next_cursor.expect("has_more requires next_cursor");
        assert_ne!(
            cursor.as_deref(),
            Some(next.as_str()),
            "work-event cursor stalled"
        );
        cursor = Some(next);
    }
    has_match |= active_at_window_start;
    let available_from_ms = available_from_ms.expect("at least one work-event page");
    assert!(
        available_from_ms <= window_start_ms,
        "{} work-event coverage begins after the capacity window: {} > {}",
        host.host,
        available_from_ms,
        window_start_ms
    );
    assert_eq!(
        has_match, host.expect_work_event,
        "{} work-event mapping expectation failed for {} / {:?}",
        host.host, host.lane_address, host.ticket
    );
    println!(
        "work-event coverage: host={} lane={} ticket={:?} available_from_ms={} window_start_ms={} pages={} matched={}",
        host.host,
        host.lane_address,
        host.ticket,
        available_from_ms,
        window_start_ms,
        pages,
        has_match
    );
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name} to a real acceptance input"))
}

fn print_json_command(
    config_path: &PathBuf,
    ledger_path: &PathBuf,
    cache_path: &PathBuf,
    prometheus_url: &str,
    args: &[&str],
) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ix-board"))
        .args(args)
        .env("IX_BOARD_CONFIG_PATH", config_path)
        .env("IX_BOARD_LEDGER_PATH", ledger_path)
        .env("IX_PROMETHEUS_URL", prometheus_url)
        .env("XDG_CACHE_HOME", cache_path)
        .output()
        .expect("run ix-board acceptance command");
    print_command_output(args, &output);
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{} did not print JSON: {error}", args.join(" ")));
    let incomplete_saturation = args.get(..2) == Some(["fleet", "saturation"].as_slice())
        && parsed["complete"] == serde_json::Value::Bool(false);
    assert!(
        output.status.success() || incomplete_saturation,
        "acceptance command failed ({}): {}\nstdout: {}\nstderr: {}",
        output.status,
        args.join(" "),
        serde_json::to_string_pretty(&parsed).expect("format failure output"),
        String::from_utf8_lossy(&output.stderr),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&parsed).expect("format JSON")
    );
    parsed
}

fn print_command_output(args: &[&str], output: &Output) {
    println!("\n=== ix-board {} ===", args.join(" "));
    if !output.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    }
}
