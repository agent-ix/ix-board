// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Emit the JSON Schema of an ix-board public wire type for downstream generation.

use ix_board::{
    DriftReport, FocusHygieneReport, FocusPlanChunk, FocusRecord, FocusStatus, Project,
    ProjectChunk, ReadyItem,
};

fn main() {
    let name = std::env::args().nth(1).unwrap_or_default();
    let schema = match name.as_str() {
        "Project" => schemars::schema_for!(Project),
        "ProjectChunk" => schemars::schema_for!(ProjectChunk),
        "ReadyItem" => schemars::schema_for!(ReadyItem),
        "DriftReport" => schemars::schema_for!(DriftReport),
        "FocusStatus" => schemars::schema_for!(FocusStatus),
        "FocusRecord" => schemars::schema_for!(FocusRecord),
        "FocusHygieneReport" => schemars::schema_for!(FocusHygieneReport),
        "FocusPlanChunk" => schemars::schema_for!(FocusPlanChunk),
        _ => {
            eprintln!(
                "usage: cargo run --example export_schema -- <Project|ProjectChunk|ReadyItem|DriftReport|FocusStatus|FocusRecord|FocusHygieneReport|FocusPlanChunk>"
            );
            std::process::exit(2);
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&schema).expect("schema is JSON")
    );
}
