// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Read-only saved snapshot, JSONL history and delta decoding through shared version rules.

use super::{DeltaReport, Record, RetroError};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// A saved snapshot or shared comparison report, without collection or persistence.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SavedReport {
    /// One supported stored snapshot/backfill record.
    Record(Record),
    /// A saved delta, or the canonical comparison of a supplied history.
    Delta(DeltaReport),
}

/// Saved-data decoding failures; none cause collection or a store append.
#[derive(Debug, Error)]
pub enum SavedReportError {
    /// The supplied file could not be read.
    #[error("cannot read saved report: {0}")]
    Read(#[source] std::io::Error),
    /// Malformed input, unsupported version or mixed native team identities.
    #[error("invalid saved report: {0}")]
    Invalid(String),
    /// Shared comparison selection rejected the requested baseline.
    #[error(transparent)]
    Comparison(#[from] RetroError),
}

/// Reads a supplied path; never resolves a default store or invokes any collector.
pub fn read_saved_report(
    path: &Path,
    baseline_taken_at: Option<&str>,
) -> Result<SavedReport, SavedReportError> {
    let bytes = std::fs::read(path).map_err(SavedReportError::Read)?;
    decode_saved_report(&bytes, baseline_taken_at)
}

/// Decodes a record JSON, record JSONL history, or an existing serialized delta.
/// History uses the same ordering, previous and baseline selection as `retro delta`.
pub fn decode_saved_report(
    bytes: &[u8],
    baseline_taken_at: Option<&str>,
) -> Result<SavedReport, SavedReportError> {
    let invalid = |error: serde_json::Error| SavedReportError::Invalid(error.to_string());
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if value.get("schema_version").is_some() {
            return decode_record(value).map(SavedReport::Record);
        }
        let report: DeltaReport = serde_json::from_value(value).map_err(invalid)?;
        if let Some(department) = &report.department {
            super::store::validate_department_version(department.current.version)
                .map_err(SavedReportError::Invalid)?;
        }
        return Ok(SavedReport::Delta(report));
    }
    let mut records = Vec::new();
    for line in bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
    {
        records.push(decode_record(
            serde_json::from_slice(line).map_err(invalid)?,
        )?);
    }
    if records.is_empty() {
        return Err(SavedReportError::Invalid("empty input".into()));
    }
    records.sort_by(|a, b| {
        crate::drift::timestamp_epoch(&a.taken_at)
            .unwrap_or(0.0)
            .total_cmp(&crate::drift::timestamp_epoch(&b.taken_at).unwrap_or(0.0))
    });
    let team = records[0].team.clone();
    if records.iter().any(|r| r.team != team) {
        return Err(SavedReportError::Invalid(
            "mixed team identities in supplied history".into(),
        ));
    }
    if records.len() == 1 {
        return Ok(SavedReport::Record(records.remove(0)));
    }
    super::delta::compare_records(&team, baseline_taken_at, &records)
        .map(SavedReport::Delta)
        .map_err(SavedReportError::Comparison)
}

fn decode_record(value: serde_json::Value) -> Result<Record, SavedReportError> {
    let record: Record =
        serde_json::from_value(value).map_err(|e| SavedReportError::Invalid(e.to_string()))?;
    super::store::validate_record(&record).map_err(SavedReportError::Invalid)?;
    crate::drift::timestamp_epoch(&record.taken_at)
        .map_err(|e| SavedReportError::Invalid(e.to_string()))?;
    Ok(record)
}
