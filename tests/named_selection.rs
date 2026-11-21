// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX
//! The version 3 named selection document, its decoder and its schema (FR-016).

use ix_board::{
    Error, FOCUS_RECORD_SCHEMA_VERSION, FocusRecord, Initiative, Issue, IssueId, MemberOutcome,
    Milestone, NAMED_SELECTION_SCHEMA_VERSION, NamedMember, NamedMemberInput, NamedSelection,
    NamedSelectionInput, Project, ProjectId, TeamKey, WorkflowStateType, WorkspaceSnapshot,
    canonicalize_named_selection, decode_named_selection,
};
use ix_trace_rs::trace;
use serde_json::{Value, json};

fn member(kind: &str, id: &str) -> NamedMember {
    match kind {
        "initiative" => NamedMember::Initiative { id: id.into() },
        "project" => NamedMember::Project { id: id.into() },
        _ => NamedMember::Issue { id: id.into() },
    }
}

fn milestone(id: &str, project_id: &str) -> NamedMember {
    NamedMember::Milestone {
        id: id.into(),
        project_id: project_id.into(),
    }
}

fn decode(value: &Value) -> Result<NamedSelection, Error> {
    decode_named_selection(value.to_string().as_bytes())
}

fn valid() -> Value {
    json!({
        "schema_version": 3,
        "team": "SWM",
        "include": [{"kind": "project", "id": "p1"}],
        "exclude": []
    })
}

#[trace("FR-016-AC-6")]
#[test]
fn a_named_selection_round_trips_and_encodes_canonically_in_any_member_order() {
    let team = TeamKey("SWM".into());
    let include = vec![
        member("issue", "i2"),
        milestone("m1", "p1"),
        member("project", "p2"),
        member("initiative", "n1"),
        member("project", "p1"),
        member("project", "p1"),
    ];
    let exclude = vec![member("issue", "i2"), member("project", "p1")];
    let forward = NamedSelection::new(team.clone(), include.clone(), exclude.clone());
    let mut reversed_include = include;
    reversed_include.reverse();
    let mut reversed_exclude = exclude;
    reversed_exclude.reverse();
    let backward = NamedSelection::new(team, reversed_include, reversed_exclude);
    let bytes = forward.encode().expect("encodes");
    assert_eq!(bytes, backward.encode().expect("encodes"));
    assert_eq!(
        bytes,
        concat!(
            r#"{"schema_version":3,"team":"SWM","include":["#,
            r#"{"kind":"initiative","id":"n1"},{"kind":"project","id":"p1"},"#,
            r#"{"kind":"project","id":"p2"},{"kind":"milestone","id":"m1","project_id":"p1"},"#,
            r#"{"kind":"issue","id":"i2"}],"exclude":["#,
            r#"{"kind":"project","id":"p1"},{"kind":"issue","id":"i2"}]}"#
        )
    );
    // A member in both lists stays in both; the exclusion wins at evaluation.
    let decoded = decode_named_selection(bytes.as_bytes()).expect("round trips");
    assert_eq!(decoded, forward);
    assert!(decoded.include.contains(&member("project", "p1")));
    assert!(decoded.exclude.contains(&member("project", "p1")));
    assert_eq!(decoded.schema_version, NAMED_SELECTION_SCHEMA_VERSION);
}

#[trace("FR-016-AC-7")]
#[test]
fn explicit_empty_decodes_but_malformed_documents_are_invalid_never_empty() {
    let mut empty = valid();
    empty["include"] = json!([]);
    let decoded = decode(&empty).expect("an empty include list is the explicit empty selection");
    assert!(decoded.include.is_empty());
    let mut missing_include = valid();
    missing_include.as_object_mut().unwrap().remove("include");
    let mut missing_exclude = valid();
    missing_exclude.as_object_mut().unwrap().remove("exclude");
    let mut missing_team = valid();
    missing_team.as_object_mut().unwrap().remove("team");
    let mut milestone_without_project = valid();
    milestone_without_project["include"] = json!([{"kind": "milestone", "id": "m1"}]);
    let mut project_on_other_kind = valid();
    project_on_other_kind["include"] = json!([{"kind": "project", "id": "p1", "project_id": "p1"}]);
    let mut duplicate = valid();
    duplicate["include"] = json!([
        {"kind": "project", "id": "p1"},
        {"kind": "project", "id": "p1"}
    ]);
    let mut unknown_member = valid();
    unknown_member["include"] = json!([{"kind": "project", "id": "p1", "label": "Beta"}]);
    let mut unknown_field = valid();
    unknown_field["status"] = json!("healthy");
    let mut ticket_kind = valid();
    ticket_kind["include"] = json!([{"kind": "ticket", "id": "i1"}]);
    let mut blank_id = valid();
    blank_id["include"] = json!([{"kind": "project", "id": " "}]);
    for (name, document) in [
        ("missing include", missing_include),
        ("missing exclude", missing_exclude),
        ("missing team", missing_team),
        ("milestone without project_id", milestone_without_project),
        ("project_id on a non-milestone", project_on_other_kind),
        ("duplicate member", duplicate),
        ("unknown member field", unknown_member),
        ("unknown document field", unknown_field),
        ("ticket is not a wire kind", ticket_kind),
        ("blank identifier", blank_id),
    ] {
        assert!(
            matches!(decode(&document), Err(Error::InvalidNamedSelection { .. })),
            "{name} must be an invalid document"
        );
    }
    assert!(matches!(
        decode_named_selection(b"not json"),
        Err(Error::InvalidNamedSelection { .. })
    ));
}

#[trace("FR-016-AC-8", "FR-016-AC-9")]
#[test]
fn other_versions_are_unsupported_and_checked_before_any_other_member() {
    let record = json!({
        "schema_version": 2,
        "selection": [{"kind": "project", "id": "p1"}],
        "reported_at": 1,
        "status": {
            "fetched_at": 1, "work_milestones": [], "excluded_ready_count": 0,
            "projects": [], "blocker_layers": [], "ready_queue": []
        }
    });
    // The legacy record still deserializes exactly as before.
    let legacy: FocusRecord = serde_json::from_value(record.clone()).expect("version 2 record");
    assert_eq!(legacy.schema_version, FOCUS_RECORD_SCHEMA_VERSION);
    match decode(&record) {
        Err(Error::UnsupportedSchemaVersion { found, supported }) => {
            assert_eq!((found, supported), (2, 3));
        }
        other => panic!("a version 2 record must be unsupported, got {other:?}"),
    }
    for version in [0, 1, 4] {
        let mut document = valid();
        document["schema_version"] = json!(version);
        assert!(matches!(
            decode(&document),
            Err(Error::UnsupportedSchemaVersion { found, supported: 3 }) if found == version
        ));
    }
    // The version is read first: a version 4 document full of unknown members reports it.
    let future = json!({"schema_version": 4, "anything": [1], "team": 7});
    assert!(matches!(
        decode(&future),
        Err(Error::UnsupportedSchemaVersion { found: 4, .. })
    ));
    // Unsupported and invalid are different errors.
    let mut invalid = valid();
    invalid["include"] = json!("nope");
    assert!(matches!(
        decode(&invalid),
        Err(Error::InvalidNamedSelection { .. })
    ));
    // A missing or non-integer version is an undecodable document, not an unsupported one.
    for version in [json!(null), json!("3"), json!(3.5)] {
        let mut document = valid();
        document["schema_version"] = version;
        assert!(matches!(
            decode(&document),
            Err(Error::InvalidNamedSelection { .. })
        ));
    }
    // The legacy record type is unchanged for a version 3 document: it still refuses it.
    assert!(serde_json::from_value::<FocusRecord>(valid()).is_err());
}

fn kind_literal(variant: &Value) -> String {
    let kind = &variant["properties"]["kind"];
    kind.get("const")
        .or_else(|| kind["enum"].get(0))
        .and_then(Value::as_str)
        .expect("a kind literal")
        .to_owned()
}

#[trace("FR-016-AC-2", "FR-016-AC-5", "FR-016-AC-10")]
#[test]
fn the_named_selection_schema_is_strict_and_the_version_two_schema_is_unchanged() {
    let schema = serde_json::to_value(schemars::schema_for!(NamedSelection)).expect("schema");
    let required = schema["required"]
        .as_array()
        .expect("required members")
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    for name in ["schema_version", "team", "include", "exclude"] {
        assert!(required.contains(name), "{name} must be required");
    }
    assert_eq!(schema["additionalProperties"], json!(false));
    let variants = schema["$defs"]["NamedMember"]["oneOf"]
        .as_array()
        .expect("one schema per member kind");
    let mut kinds = variants.iter().map(kind_literal).collect::<Vec<_>>();
    kinds.sort();
    assert_eq!(kinds, ["initiative", "issue", "milestone", "project"]);
    for variant in variants {
        let carries_project = variant["properties"].get("project_id").is_some();
        assert_eq!(carries_project, kind_literal(variant) == "milestone");
        assert_eq!(variant["additionalProperties"], json!(false));
        if carries_project {
            let required = variant["required"].as_array().expect("required");
            assert!(required.contains(&json!("project_id")));
        }
    }
    // The version 2 schema is exported separately and keeps exactly its own members.
    let record = serde_json::to_value(schemars::schema_for!(FocusRecord)).expect("schema");
    let mut members = record["required"]
        .as_array()
        .expect("required")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    members.sort_unstable();
    assert_eq!(
        members,
        ["reported_at", "schema_version", "selection", "status"]
    );
    assert!(record["properties"].get("team").is_none());
    assert!(record["properties"].get("include").is_none());
    assert_eq!(FOCUS_RECORD_SCHEMA_VERSION, 2);
    assert_ne!(schema, record);
}

fn project(id: &str, identifier: &str, name: &str, team: &str) -> Project {
    Project {
        id: ProjectId(id.into()),
        identifier: Some(identifier.into()),
        name: name.into(),
        updated_at: None,
        description: String::new(),
        content: None,
        status: "Started".into(),
        status_name: None,
        status_category: Default::default(),
        priority: 2,
        sort_order: 0.0,
        target_date: None,
        team_keys: vec![TeamKey(team.into())],
        milestones: vec![],
        initiatives: vec![Initiative {
            id: "uuid-init".into(),
            identifier: "I-10".into(),
            name: "Delivery".into(),
        }],
    }
}

fn milestone_record(id: &str, name: &str) -> Milestone {
    Milestone {
        id: id.into(),
        name: name.into(),
        description: None,
        sort_order: 1.0,
        target_date: None,
    }
}

fn ticket(id: &str, identifier: &str, project: &str) -> Issue {
    Issue {
        id: IssueId(id.into()),
        identifier: identifier.into(),
        title: "A label that must never be stored".into(),
        description: None,
        state: "Todo".into(),
        state_name: "Todo".into(),
        state_type: WorkflowStateType::Unstarted,
        parent_id: None,
        team_key: TeamKey(identifier.split('-').next().unwrap_or("SWM").into()),
        assignee_id: None,
        assignee_name: None,
        project_id: Some(ProjectId(project.into())),
        project_milestone_id: None,
        priority: 2.0,
        sort_order: 0.0,
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
        attachment_urls: vec![],
        attachments_complete: true,
    }
}

fn naming_snapshot() -> WorkspaceSnapshot {
    let mut alpha = project("uuid-alpha", "PRJ-1", "Project / Alpha", "SWM");
    alpha.milestones = vec![
        milestone_record("uuid-m1", "Derived W/M operators"),
        milestone_record("uuid-shared-a", "Beta"),
    ];
    let mut other = project("uuid-other", "PRJ-2", "Other", "SWM");
    other.milestones = vec![milestone_record("uuid-shared-b", "Beta")];
    let mut foreign = project("uuid-foreign", "PRJ-3", "Foreign", "AGE");
    foreign.initiatives.clear();
    WorkspaceSnapshot {
        projects: vec![alpha, other, foreign],
        issues: vec![
            ticket("uuid-t1", "SWM-1", "uuid-alpha"),
            ticket("uuid-t2", "AGE-1", "uuid-foreign"),
        ],
        fetched_at: 0,
    }
}

fn input(include: Vec<NamedMemberInput>) -> NamedSelectionInput {
    NamedSelectionInput {
        team: "SWM".into(),
        include,
        exclude: vec![],
    }
}

fn milestone_input(selector: &str, project: Option<&str>) -> NamedMemberInput {
    NamedMemberInput::Milestone {
        selector: selector.into(),
        project: project.map(str::to_owned),
    }
}

#[trace("FR-016-AC-11", "FR-010-AC-15")]
#[test]
fn names_canonicalize_to_ids_with_the_owning_project_and_failures_leave_no_document() {
    let snapshot = naming_snapshot();
    let document = canonicalize_named_selection(
        &snapshot,
        &NamedSelectionInput {
            exclude: vec![NamedMemberInput::Ticket("swm-1".into())],
            ..input(vec![
                NamedMemberInput::Initiative("i-10".into()),
                NamedMemberInput::Project("Project / Alpha".into()),
                milestone_input("Project / Alpha/Derived W/M operators", None),
                milestone_input("Derived W/M operators", Some("PRJ-1")),
                milestone_input("PRJ-1/Beta", None),
            ])
        },
    )
    .expect("canonicalizes");
    assert_eq!(
        document.include,
        [
            NamedMember::Initiative {
                id: "uuid-init".into()
            },
            NamedMember::Project {
                id: "uuid-alpha".into()
            },
            milestone("uuid-m1", "uuid-alpha"),
            milestone("uuid-shared-a", "uuid-alpha"),
        ]
    );
    assert_eq!(
        document.exclude,
        [NamedMember::Issue {
            id: "uuid-t1".into()
        }]
    );
    // The stored document keeps no label, expanded issue ID or status.
    let bytes = document.encode().expect("encodes");
    for forbidden in ["Alpha", "Delivery", "SWM-1", "label", "Todo", "Started"] {
        assert!(
            !bytes.contains(forbidden),
            "{forbidden} must not be stored: {bytes}"
        );
    }
    let refused =
        |members: Vec<NamedMemberInput>| canonicalize_named_selection(&snapshot, &input(members));
    // An ambiguous bare name lists candidates; a qualified one is unique.
    assert!(matches!(
        refused(vec![milestone_input("Beta", None)]),
        Err(Error::AmbiguousMilestone { .. })
    ));
    assert!(matches!(
        refused(vec![milestone_input("Missing", None)]),
        Err(Error::ScopeSelection {
            kind: "milestone",
            matches: 0,
            ..
        })
    ));
    assert!(matches!(
        refused(vec![NamedMemberInput::Project("Nothing".into())]),
        Err(Error::ScopeSelection {
            kind: "project",
            ..
        })
    ));
    // The recorded project must own the milestone.
    assert!(matches!(
        refused(vec![milestone_input("PRJ-1/Beta", Some("PRJ-2"))]),
        Err(Error::NamedMemberRejected {
            outcome: MemberOutcome::ProjectMismatch,
            ..
        })
    ));
    // Another team's ticket, project, or milestone owner is refused.
    for other_team in [
        NamedMemberInput::Ticket("AGE-1".into()),
        NamedMemberInput::Project("Foreign".into()),
    ] {
        assert!(matches!(
            refused(vec![other_team]),
            Err(Error::NamedMemberRejected {
                outcome: MemberOutcome::WrongTeam,
                ..
            })
        ));
    }
    let mut unknown_team = input(vec![]);
    unknown_team.team = "NOPE".into();
    assert!(matches!(
        canonicalize_named_selection(&snapshot, &unknown_team),
        Err(Error::ScopeSelection { kind: "team", .. })
    ));
}
