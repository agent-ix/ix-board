// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Recently closed tickets and linked pull-request events for focus status.

use std::collections::BTreeSet;

use crate::model::{
    FocusActivity, FocusActivityKind, FocusActivityPr, FocusIssue, Issue, PullRequestFact,
    PullRequestStatus, WorkflowStateType,
};

/// Default `--closed-since` window: 48 hours.
pub const DEFAULT_CLOSED_SINCE_SECONDS: u64 = 48 * 60 * 60;
const RECENTLY_CLOSED_CAP: usize = 50;
const ACTIVITY_CAP: usize = 200;

fn epoch(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.timestamp())
}

/// The time window activity is reported over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityWindow {
    /// Unix seconds treated as now.
    pub now: i64,
    /// Window length in seconds.
    pub closed_since_seconds: u64,
}

impl ActivityWindow {
    fn cutoff(self) -> i64 {
        let span = i64::try_from(self.closed_since_seconds).unwrap_or(i64::MAX);
        self.now.saturating_sub(span)
    }

    fn contains(self, timestamp: Option<&str>) -> bool {
        timestamp
            .and_then(epoch)
            .is_some_and(|at| at >= self.cutoff() && at <= self.now)
    }
}

/// Builds the focus summary of an issue, with `state` as its displayed state.
pub(crate) fn focus_issue_with_state(issue: &Issue, state: String) -> FocusIssue {
    FocusIssue {
        id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        title: issue.title.clone(),
        state,
        assignee: issue.assignee_name.clone(),
        started_at: issue.started_at.clone(),
        completed_at: issue.completed_at.clone(),
        prs: issue.pull_requests.clone(),
    }
}

/// Completed issues whose completion falls inside the window, newest first, capped at 50.
pub(crate) fn recently_closed(issues: &[&Issue], window: ActivityWindow) -> Vec<FocusIssue> {
    let mut closed = issues
        .iter()
        .copied()
        .filter(|issue| {
            issue.state_type == WorkflowStateType::Completed
                && window.contains(issue.completed_at.as_deref())
        })
        .map(|issue| {
            let at = issue
                .completed_at
                .as_deref()
                .and_then(epoch)
                .unwrap_or(i64::MIN);
            (at, focus_issue_with_state(issue, issue.state_name.clone()))
        })
        .collect::<Vec<_>>();
    closed.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.identifier.cmp(&right.1.identifier))
    });
    closed
        .into_iter()
        .take(RECENTLY_CLOSED_CAP)
        .map(|(_, issue)| issue)
        .collect()
}

/// Dated events for the issues of one project, unsorted and not yet capped.
pub(crate) fn project_events(
    project_name: &str,
    issues: &[&Issue],
    window: ActivityWindow,
) -> Vec<(i64, FocusActivity)> {
    let mut events = Vec::new();
    let mut push = |kind, timestamp: &str, issue: &Issue, pr: Option<&PullRequestFact>| {
        if let Some(at_epoch) = epoch(timestamp) {
            events.push((
                at_epoch,
                FocusActivity {
                    at: timestamp.to_owned(),
                    kind,
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    project: project_name.to_owned(),
                    pr: pr.map(|pr| FocusActivityPr {
                        url: pr.url.clone(),
                        repo: pr.repo.clone(),
                        number: pr.number,
                    }),
                },
            ));
        }
    };
    for issue in issues {
        for pr in &issue.pull_requests {
            if let Some(at) = pr
                .merged_at
                .as_deref()
                .filter(|at| window.contains(Some(at)))
            {
                push(FocusActivityKind::Merged, at, issue, Some(pr));
            }
            if let Some(at) = pr
                .created_at
                .as_deref()
                .filter(|at| window.contains(Some(at)))
            {
                push(FocusActivityKind::Opened, at, issue, Some(pr));
            }
        }
        if issue.state_type == WorkflowStateType::Completed
            && let Some(at) = issue
                .completed_at
                .as_deref()
                .filter(|at| window.contains(Some(at)))
        {
            push(FocusActivityKind::Closed, at, issue, None);
        }
    }
    events
}

/// Orders events newest first, drops repeats of (kind, identifier, pr url), and caps at 200.
pub(crate) fn finish_activity(mut events: Vec<(i64, FocusActivity)>) -> Vec<FocusActivity> {
    events.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.identifier.cmp(&right.1.identifier))
            .then_with(|| left.1.kind.cmp(&right.1.kind))
    });
    let mut seen = BTreeSet::new();
    events
        .into_iter()
        .map(|(_, event)| event)
        .filter(|event| {
            seen.insert((
                event.kind,
                event.identifier.clone(),
                event.pr.as_ref().map(|pr| pr.url.clone()),
            ))
        })
        .take(ACTIVITY_CAP)
        .collect()
}

impl PullRequestStatus {
    /// Maps Linear attachment metadata status text to a status; anything else is unknown.
    pub(crate) fn from_metadata(status: Option<&str>) -> Self {
        match status {
            Some("open") => Self::Open,
            Some("draft") => Self::Draft,
            Some("merged") => Self::Merged,
            Some("closed") => Self::Closed,
            _ => Self::Unknown,
        }
    }
}
