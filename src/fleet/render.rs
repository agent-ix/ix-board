// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Human text for the fleet report; it carries the same rows, defects and `complete` as JSON.

use super::report::{Defect, FleetReport, LaneRow};
use std::fmt::Write;

fn class_text(row: &LaneRow) -> String {
    let name = match serde_json::to_value(row.class) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    };
    match (&row.reason, name.as_str()) {
        (Some(reason), "BLOCKED") => format!("BLOCKED({reason})"),
        (Some(reason), _) => format!("{name}({reason})"),
        (None, _) => name,
    }
}

fn defect_text(defect: &Defect) -> String {
    let kind = match serde_json::to_value(defect.kind) {
        Ok(serde_json::Value::String(kind)) => kind,
        _ => String::new(),
    };
    let first_seen = defect
        .first_seen_ms
        .map_or_else(String::new, |ms| format!(" first_seen_ms={ms}"));
    format!(
        "{} source={} host={} lane={} observed_ms={}{} {}",
        kind,
        defect.source,
        defect.host.as_deref().unwrap_or("-"),
        defect.lane.as_deref().unwrap_or("-"),
        defect.observed_ms,
        first_seen,
        defect.reason
    )
}

/// Renders the report as text: one line per lane, one per defect, then `complete`.
pub fn render_text(report: &FleetReport) -> String {
    let mut out = String::new();
    for row in &report.rows {
        let age = row
            .state_age_secs
            .map_or_else(|| "-".to_owned(), |secs| format!("{secs}s"));
        let last = row
            .last_event_ms
            .map_or_else(|| "-".to_owned(), |ms| ms.to_string());
        let _ = writeln!(
            out,
            "{}  {}  {}  age={}  last_event_ms={}{}{}",
            row.lane,
            row.host,
            class_text(row),
            age,
            last,
            if row.thresholds_defaulted {
                "  thresholds_defaulted"
            } else {
                ""
            },
            if row.receipt_gap { "  receipt_gap" } else { "" }
        );
    }
    for host in &report.hosts {
        let _ = writeln!(
            out,
            "host {}: lanes={} working={} not_working={}",
            host.host, host.lanes, host.working, host.not_working
        );
    }
    for defect in &report.defects {
        let _ = writeln!(out, "defect: {}", defect_text(defect));
    }
    let _ = write!(out, "complete: {}", report.complete);
    out
}
