// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

use ix_board::{
    DriftReport, FocusHygieneReport, FocusPlanChunk, FocusRecord, FocusStatus, PrScopeSignal,
    Project, ReadyItem,
};

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
