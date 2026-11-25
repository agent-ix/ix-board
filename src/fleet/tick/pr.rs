// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Live lane-to-ticket pull-request evidence for stale-PR actions (SWM-282).

use super::LanePr;
use crate::{LinearGraphQlSource, PullRequestStatus, ReadService};
use ix_trace_rs::implements;
use relay_core::assignment::AssignmentList;
use std::collections::BTreeMap;
use std::time::Duration;

trait PullRequestFacts {
    fn issues(&mut self, tickets: &[String]) -> Result<Vec<crate::Issue>, String>;
    fn updated_at(&self, url: &str) -> Result<i64, String>;
}

struct LivePullRequestFacts {
    timeout: Duration,
}

impl PullRequestFacts for LivePullRequestFacts {
    fn issues(&mut self, tickets: &[String]) -> Result<Vec<crate::Issue>, String> {
        let source = LinearGraphQlSource::from_env().map_err(|error| error.to_string())?;
        let mut service = ReadService::open_default(source).map_err(|error| error.to_string())?;
        let batch = service.issues(tickets).map_err(|error| error.to_string())?;
        if !batch.complete || !batch.missing.is_empty() {
            return Err(format!(
                "Linear ticket mapping incomplete: {:?}",
                batch.missing
            ));
        }
        Ok(batch.issues)
    }

    fn updated_at(&self, url: &str) -> Result<i64, String> {
        let api_url = github_api_url(url)?;
        let token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| std::env::var("GH_TOKEN"))
            .map_err(|_| "GITHUB_TOKEN_or_GH_TOKEN_unconfigured".to_owned())?;
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let response = client
            .get(api_url)
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let facts: serde_json::Value = response.json().map_err(|error| error.to_string())?;
        let updated_at = facts
            .get("updated_at")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "GitHub pull request response omitted updated_at".to_owned())?;
        chrono::DateTime::parse_from_rfc3339(updated_at)
            .map(|time| time.timestamp_millis())
            .map_err(|error| error.to_string())
    }
}

fn github_api_url(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://github.com/")
        .ok_or_else(|| "unsupported pull request URL".to_owned())?;
    let parts = rest.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "pull" || parts[3].parse::<u64>().is_err() {
        return Err("malformed pull request URL".to_owned());
    }
    Ok(format!(
        "https://api.github.com/repos/{}/{}/pulls/{}",
        parts[0], parts[1], parts[3]
    ))
}

#[implements("FR-038-AC-5")]
fn read_lane_prs(
    assignments: &[AssignmentList],
    facts: &mut dyn PullRequestFacts,
) -> Result<Vec<LanePr>, String> {
    let mut ticket_lanes = BTreeMap::<String, Vec<String>>::new();
    for list in assignments {
        for ticket in &list.tickets {
            ticket_lanes
                .entry(
                    ticket
                        .reference
                        .rsplit('/')
                        .next()
                        .unwrap_or(&ticket.reference)
                        .to_owned(),
                )
                .or_default()
                .push(list.lane.to_lowercase());
        }
    }
    if ticket_lanes.len() > 500 {
        return Err("assigned ticket count exceeds live PR reader limit 500".to_owned());
    }
    let tickets = ticket_lanes.keys().cloned().collect::<Vec<_>>();
    if tickets.is_empty() {
        return Ok(Vec::new());
    }
    let issues = facts.issues(&tickets)?;
    let mut result = Vec::new();
    for issue in issues {
        if !issue.attachments_complete {
            return Err(format!(
                "Linear PR attachments incomplete for {}",
                issue.identifier
            ));
        }
        let lanes = ticket_lanes
            .get(&issue.identifier)
            .ok_or_else(|| format!("Linear returned unexpected ticket {}", issue.identifier))?;
        for pull_request in issue.pull_requests.iter().filter(|pr| {
            matches!(
                pr.status,
                PullRequestStatus::Open | PullRequestStatus::Draft
            )
        }) {
            let last_activity_ms = facts.updated_at(&pull_request.url)?;
            for lane in lanes {
                result.push(LanePr {
                    lane: lane.clone(),
                    pr: pull_request.url.clone(),
                    last_activity_ms,
                });
            }
        }
    }
    Ok(result)
}

#[implements("FR-038-AC-5")]
pub(super) fn read_live(timeout: Duration) -> Result<Vec<LanePr>, String> {
    let settings = relay_core::config::load_core(relay_core::config::CoreFlags::default())
        .map_err(|error| error.to_string())?;
    let mut relay = relay_core::Relay::connect(&settings.url.value, timeout)
        .map_err(|error| format!("relay connect: {error}"))?
        .with_prefix(settings.prefix.value.clone());
    let assignments = relay
        .assignments()
        .map_err(|error| format!("relay assignments: {error}"))?;
    read_lane_prs(&assignments, &mut LivePullRequestFacts { timeout })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Issue;
    use crate::{IssueId, ProjectId, TeamKey, WorkflowStateType};
    use ix_trace_rs::trace;
    use relay_core::assignment::{AssignedTicket, AssignmentList};

    struct Facts(Vec<Issue>);

    impl PullRequestFacts for Facts {
        fn issues(&mut self, _: &[String]) -> Result<Vec<Issue>, String> {
            Ok(self.0.clone())
        }
        fn updated_at(&self, _: &str) -> Result<i64, String> {
            Ok(12_345)
        }
    }

    #[trace("FR-038-AC-5")]
    #[test]
    fn assigned_lane_open_pr_becomes_live_stale_pr_evidence() {
        let issue = crate::Issue {
            id: IssueId("uuid".to_owned()),
            identifier: "SWM-252".to_owned(),
            title: "test".to_owned(),
            description: None,
            state: "Started".to_owned(),
            state_name: "In Progress".to_owned(),
            state_type: WorkflowStateType::Started,
            parent_id: None,
            team_key: TeamKey("SWM".to_owned()),
            assignee_id: None,
            assignee_name: None,
            project_id: Some(ProjectId("project".to_owned())),
            project_milestone_id: None,
            priority: 0.0,
            sort_order: 0.0,
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
            attachment_urls: vec!["https://github.com/agent-ix/ix-board/pull/102".to_owned()],
            attachments_complete: true,
            started_at: None,
            completed_at: None,
            pull_requests: vec![crate::PullRequestFact {
                url: "https://github.com/agent-ix/ix-board/pull/102".to_owned(),
                repo: "agent-ix/ix-board".to_owned(),
                number: 102,
                status: PullRequestStatus::Open,
                draft: false,
                created_at: None,
                merged_at: None,
            }],
        };
        let assignments = vec![AssignmentList {
            lane: "draco:workspace:build".to_owned(),
            version: 1,
            written_at_ms: 1,
            tickets: vec![AssignedTicket {
                id: "SWM-252".to_owned(),
                reference: "SWM-252".to_owned(),
            }],
        }];
        let prs = read_lane_prs(&assignments, &mut Facts(vec![issue])).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].lane, "draco:workspace:build");
        assert_eq!(prs[0].last_activity_ms, 12_345);
    }
}
