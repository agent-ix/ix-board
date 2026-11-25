// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! Dangling Linear references that a read tolerates and reports (FR-046).

use std::collections::{BTreeSet, HashMap};
use std::fmt::{Display, Formatter};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::read_bound::Budget;
use crate::{Error, Issue, IssueId, Project, ProjectId, WorkspaceSnapshot};

/// A Linear record that a read needed and the snapshot does not hold.
///
/// `issue` is the human-readable identifier of the issue that holds the reference; the
/// other fields are Linear UUIDs. The issue stays in the result and in its counts.
#[derive(
    Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReferenceWarning {
    /// The issue's milestone is not one of its project's milestones. It is counted at
    /// project level, without a milestone.
    UnavailableMilestone {
        /// Issue holding the reference.
        issue: String,
        /// Referenced milestone.
        milestone: String,
        /// The issue's project; null when it has none.
        project: Option<String>,
    },
    /// The issue's project has no record. The issue belongs to no project row.
    UnavailableProject {
        /// Issue holding the reference.
        issue: String,
        /// Referenced project.
        project: String,
    },
    /// The issue's parent has no record. Ancestry stops at this issue.
    UnavailableParent {
        /// Issue holding the reference.
        issue: String,
        /// Referenced parent issue.
        parent: String,
    },
    /// A prerequisite of the issue has no record. The issue is not ready, and the
    /// prerequisite appears in no blocker layer.
    UnavailableBlocker {
        /// Issue holding the reference.
        issue: String,
        /// Referenced blocking issue.
        blocker: String,
    },
}

impl ReferenceWarning {
    /// The warning's `kind` as it appears in JSON and in diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnavailableMilestone { .. } => "unavailable_milestone",
            Self::UnavailableProject { .. } => "unavailable_project",
            Self::UnavailableParent { .. } => "unavailable_parent",
            Self::UnavailableBlocker { .. } => "unavailable_blocker",
        }
    }
}

impl Display for ReferenceWarning {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnavailableMilestone {
                issue,
                milestone,
                project: Some(project),
            } => write!(
                formatter,
                "issue {issue} references unavailable milestone {milestone} in project {project}"
            ),
            Self::UnavailableMilestone {
                issue,
                milestone,
                project: None,
            } => write!(
                formatter,
                "issue {issue} references milestone {milestone} and has no project"
            ),
            Self::UnavailableProject { issue, project } => write!(
                formatter,
                "issue {issue} references unavailable project {project}"
            ),
            Self::UnavailableParent { issue, parent } => write!(
                formatter,
                "issue {issue} references unavailable parent {parent}"
            ),
            Self::UnavailableBlocker { issue, blocker } => write!(
                formatter,
                "issue {issue} references unavailable blocker {blocker}"
            ),
        }
    }
}

struct Records<'a> {
    issues: HashMap<&'a IssueId, &'a Issue>,
    projects: HashMap<&'a ProjectId, &'a Project>,
}

impl<'a> Records<'a> {
    fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
        Self {
            issues: snapshot
                .issues
                .iter()
                .map(|issue| (&issue.id, issue))
                .collect(),
            projects: snapshot
                .projects
                .iter()
                .map(|project| (&project.id, project))
                .collect(),
        }
    }

    /// The issue's own project and milestone references.
    fn placement(&self, issue: &Issue, found: &mut BTreeSet<ReferenceWarning>) {
        let project = match &issue.project_id {
            Some(id) => match self.projects.get(id) {
                Some(project) => Some(*project),
                None => {
                    found.insert(ReferenceWarning::UnavailableProject {
                        issue: issue.identifier.clone(),
                        project: id.0.clone(),
                    });
                    // Without the project record its milestones cannot be judged.
                    return;
                }
            },
            None => None,
        };
        if let Some(milestone) = &issue.project_milestone_id
            && !project.is_some_and(|p| p.milestones.iter().any(|m| &m.id == milestone))
        {
            found.insert(ReferenceWarning::UnavailableMilestone {
                issue: issue.identifier.clone(),
                milestone: milestone.clone(),
                project: project.map(|p| p.id.0.clone()),
            });
        }
    }

    /// Every unavailable record in the transitive `blocked_by` closure of `roots`.
    fn blockers(
        &self,
        roots: impl IntoIterator<Item = &'a Issue>,
        found: &mut BTreeSet<ReferenceWarning>,
        budget: Budget<'_>,
    ) -> Result<(), Error> {
        let mut pending = roots.into_iter().collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        while let Some(issue) = pending.pop() {
            budget.check()?;
            if !visited.insert(&issue.id) {
                continue;
            }
            for blocker in &issue.blocked_by {
                match self.issues.get(blocker) {
                    Some(record) => pending.push(record),
                    None => {
                        found.insert(ReferenceWarning::UnavailableBlocker {
                            issue: issue.identifier.clone(),
                            blocker: blocker.0.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

/// Warnings for a focus: each member's project, milestone and ancestry references, and
/// the blocker closure the members root. Sorted and without duplicates.
pub(crate) fn focus_warnings(
    snapshot: &WorkspaceSnapshot,
    member_ids: &BTreeSet<IssueId>,
    budget: Budget<'_>,
) -> Result<Vec<ReferenceWarning>, Error> {
    let records = Records::new(snapshot);
    let members = snapshot
        .issues
        .iter()
        .filter(|issue| member_ids.contains(&issue.id))
        .collect::<Vec<_>>();
    let mut found = BTreeSet::new();
    let mut walked = BTreeSet::new();
    for member in &members {
        records.placement(member, &mut found);
        let mut next = Some(*member);
        while let Some(issue) = next {
            budget.check()?;
            if !walked.insert(&issue.id) {
                break;
            }
            next = match &issue.parent_id {
                Some(parent) => {
                    let record = records.issues.get(parent).copied();
                    if record.is_none() {
                        found.insert(ReferenceWarning::UnavailableParent {
                            issue: issue.identifier.clone(),
                            parent: parent.0.clone(),
                        });
                    }
                    record
                }
                None => None,
            };
        }
    }
    records.blockers(members, &mut found, budget)?;
    Ok(found.into_iter().collect())
}

/// Warnings for ready-queue candidates, whose project record and blocker closure decide
/// eligibility.
pub(crate) fn candidate_warnings(
    snapshot: &WorkspaceSnapshot,
    is_candidate: impl Fn(&Issue) -> bool,
) -> Result<Vec<ReferenceWarning>, Error> {
    let records = Records::new(snapshot);
    let candidates = snapshot
        .issues
        .iter()
        .filter(|issue| is_candidate(issue))
        .collect::<Vec<_>>();
    let mut found = BTreeSet::new();
    for issue in &candidates {
        if let Some(project) = &issue.project_id
            && !records.projects.contains_key(project)
        {
            found.insert(ReferenceWarning::UnavailableProject {
                issue: issue.identifier.clone(),
                project: project.0.clone(),
            });
        }
    }
    records.blockers(candidates, &mut found, Budget::unbounded())?;
    Ok(found.into_iter().collect())
}
