// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use ix_board::{
    DriftReport, FocusHygieneReport, FocusPlanChunk, FocusRecord, FocusStatus, PrScopeSignal,
    Project, ReadyItem,
};
use ix_trace_rs::trace;

#[test]
fn public_api_models_have_structural_json_schemas() {
    let project = serde_json::to_value(schemars::schema_for!(Project)).unwrap();
    let ready = serde_json::to_value(schemars::schema_for!(ReadyItem)).unwrap();
    let drift = serde_json::to_value(schemars::schema_for!(DriftReport)).unwrap();
    let scope = serde_json::to_value(schemars::schema_for!(PrScopeSignal)).unwrap();
    let focus_status = serde_json::to_value(schemars::schema_for!(FocusStatus)).unwrap();
    let focus_record = serde_json::to_value(schemars::schema_for!(FocusRecord)).unwrap();
    let focus_hygiene = serde_json::to_value(schemars::schema_for!(FocusHygieneReport)).unwrap();
    let focus_chunk = serde_json::to_value(schemars::schema_for!(FocusPlanChunk)).unwrap();

    assert_eq!(project["type"], "object");
    assert!(project["properties"]["status"].is_object());
    assert!(ready["properties"]["next_step"].is_object());
    assert_eq!(drift["type"], "object");
    assert!(drift["properties"]["leads"].is_object());
    assert!(drift["$defs"]["LeadDriftReport"]["properties"]["signals"].is_object());
    assert!(scope["properties"]["availability"].is_object());
    assert!(scope["properties"]["missing_sources"].is_object());
    assert!(focus_status["properties"]["projects"].is_object());
    assert!(focus_record["properties"]["schema_version"].is_object());
    assert!(focus_record["properties"]["selection"].is_object());
    assert!(focus_record["properties"]["status"].is_object());
    assert!(focus_hygiene["properties"]["issue_refs"].is_object());
    assert!(focus_chunk["properties"]["issues"].is_object());
}

#[trace("FR-018-AC-7", "FR-018-AC-23")]
#[test]
fn lifecycle_schema_and_documented_example_preserve_nullable_counts_and_versions() {
    let report: ix_board::agents::AgentsReport =
        serde_json::from_str(include_str!("../docs/examples/agents-v2.json")).unwrap();
    assert_eq!(
        report.schema_version,
        ix_board::agents::AGENTS_SCHEMA_VERSION
    );
    let row = &report.agents[0];
    assert_eq!(row.live_subagents, row.active_direct_workers);
    assert_eq!(row.active_direct_workers, None);
    let schema =
        serde_json::to_value(schemars::schema_for!(ix_board::agents::AgentsReport)).unwrap();
    let fields = &schema["$defs"]["AgentRow"]["properties"];
    assert_eq!(
        fields["active_direct_workers"]["type"],
        serde_json::json!(["integer", "null"])
    );
    assert!(fields["workers"].is_object());
    assert!(fields["availability"].is_object());
    assert_eq!(
        schema,
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../docs/schema/agents-report.schema.json"
        ))
        .unwrap()
    );
}
