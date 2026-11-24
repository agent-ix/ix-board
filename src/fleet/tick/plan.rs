// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The pure planner of `fleet tick` (FR-038): conditions from evidence, ids and ladder rungs
//! from the open actions of the ledger.

use super::{
    Action, ActionKind, Severity, StandingOrders, Suppressed, Target, TickFacts, TickSettings,
};
use crate::fleet::{ClassKind, DefectKind, FleetReport, LaneRow};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// One unresolved action row of the ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenAction {
    /// Ledger id.
    pub id: i64,
    /// Kind.
    pub kind: ActionKind,
    /// The subject stored in the lane column.
    pub subject: String,
    /// Relay time the condition started.
    pub started_at: i64,
    /// Relay time the minder had to execute it by.
    pub due_by: i64,
    /// Rung of the row.
    pub rung: Option<u8>,
    /// Condition action of a rung row.
    pub parent: Option<i64>,
}

/// Capacity state the ledger holds for `add_lane` and `escalate_drain` (FR-041).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Capacity {
    /// Hosts (lower case) with an open capacity run.
    pub open_runs: BTreeSet<String>,
    /// Host to its current recorded sustainable level.
    pub levels: BTreeMap<String, u32>,
    /// Host to the end of its cool-off after a step-down.
    pub cool_off_until: BTreeMap<String, i64>,
    /// Draining lane (lower case) to its host and the relay time of its mark.
    pub draining: BTreeMap<String, (String, i64)>,
}

/// Everything one plan reads.
pub struct PlanInput<'a> {
    /// The `fleet status` report of this tick.
    pub report: &'a FleetReport,
    /// First-seen relay time per report defect, in report order.
    pub first_seen: &'a [Option<i64>],
    /// Tick settings.
    pub settings: &'a TickSettings,
    /// Tick interval in milliseconds.
    pub interval_ms: i64,
    /// Facts from the other sources.
    pub facts: &'a TickFacts,
    /// Capacity state.
    pub capacity: &'a Capacity,
    /// Standing-order file state.
    pub orders: &'a StandingOrders,
    /// Unresolved actions of earlier ticks.
    pub open: &'a [OpenAction],
}

/// A planned action and its condition key.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    /// The action; `id` is 0 until the ledger assigns it.
    pub action: Action,
    /// The condition's key; a rung names its parent's.
    pub key: (ActionKind, String),
    /// The action is not yet in the ledger.
    pub is_new: bool,
    /// The row is recorded but not sent (a rung 2 with no lead).
    pub record_only: bool,
}

/// The plan of one tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Actions in emission order; rung rows follow their condition's order by severity.
    pub actions: Vec<Planned>,
    /// Open actions whose condition cleared in this tick, with the condition's current state.
    pub resolved: Vec<OpenAction>,
    /// Suppressions.
    pub suppressed: Vec<Suppressed>,
}

struct Cond {
    kind: ActionKind,
    subject: String,
    lane: Option<String>,
    target: Target,
    reason: String,
    severity: Severity,
    evidence: Vec<String>,
    detail: Value,
}

fn relay_reason(row: &LaneRow) -> Option<String> {
    row.evidence.iter().find_map(|e| {
        e.item
            .strip_prefix("ix_fleet_lane_blocked{reason=")
            .and_then(|r| r.strip_suffix('}'))
            .map(str::to_owned)
    })
}

fn severity_of(kind: ActionKind) -> Severity {
    match kind {
        ActionKind::EscalateBlock
        | ActionKind::EscalateStuck
        | ActionKind::EscalateStalePr
        | ActionKind::EscalateDrain => Severity::High,
        ActionKind::FixSource | ActionKind::ActOnUndetermined | ActionKind::ResendOrder => {
            Severity::Medium
        }
        _ => Severity::Low,
    }
}

struct Ctx<'a> {
    input: &'a PlanInput<'a>,
    conds: Vec<Cond>,
}

impl Ctx<'_> {
    fn lead_target(&self, lane: &str) -> (Target, String) {
        match self.input.settings.leads.get(&lane.to_lowercase()) {
            Some(lead) => (
                Target {
                    role: "lead",
                    addr: Some(lead.clone()),
                },
                String::new(),
            ),
            None => (
                Target {
                    role: "orchestrator",
                    addr: self.input.settings.orchestrator.clone(),
                },
                "lead_unresolved".to_owned(),
            ),
        }
    }

    fn lane_cond(&mut self, kind: ActionKind, row: &LaneRow, reason: &str, detail: Value) {
        let (target, reason) =
            if matches!(kind, ActionKind::EscalateBlock | ActionKind::EscalateStuck) {
                let (target, unresolved) = self.lead_target(&row.lane);
                let reason = if unresolved.is_empty() {
                    reason.to_owned()
                } else {
                    unresolved
                };
                (target, reason)
            } else {
                (
                    Target {
                        role: "lane",
                        addr: Some(row.lane.clone()),
                    },
                    reason.to_owned(),
                )
            };
        self.push(Cond {
            kind,
            subject: row.lane.clone(),
            lane: Some(row.lane.clone()),
            target,
            reason,
            severity: severity_of(kind),
            evidence: row.evidence.iter().map(|e| e.item.clone()).collect(),
            detail,
        });
    }

    fn push(&mut self, cond: Cond) {
        self.conds.push(cond);
    }

    fn blocked(&mut self, row: &LaneRow) {
        let relay = relay_reason(row).unwrap_or_default();
        let class = row.reason.as_deref().unwrap_or("");
        let answerable = (class == "dialog") || (class == "question" && relay == "queued_question");
        let detail = |extra: Value| {
            let mut detail = json!({"class": class, "relay_reason": relay});
            if let (Some(base), Some(add)) = (detail.as_object_mut(), extra.as_object()) {
                base.extend(add.clone());
            }
            detail
        };
        if answerable {
            match self.input.orders {
                StandingOrders::Present => {
                    return self.lane_cond(
                        ActionKind::AnswerDialog,
                        row,
                        "standing_order",
                        detail(json!({"standing_order": null, "order_match": "swm_226_pending"})),
                    );
                }
                StandingOrders::Missing => {
                    return self.lane_cond(
                        ActionKind::EscalateBlock,
                        row,
                        "standing_orders_missing",
                        detail(json!({})),
                    );
                }
                StandingOrders::Unreadable(why) => {
                    return self.lane_cond(
                        ActionKind::EscalateBlock,
                        row,
                        "standing_orders_unreadable",
                        detail(json!({"error": why})),
                    );
                }
            }
        }
        let reason = match class {
            "dialog" => "no_standing_order",
            other => other,
        };
        self.lane_cond(ActionKind::EscalateBlock, row, reason, detail(json!({})));
    }

    fn undetermined(&mut self) {
        let report = self.input.report;
        let unreachable: BTreeSet<String> = report
            .rows
            .iter()
            .filter(|r| r.class == ClassKind::Unreachable)
            .map(|r| r.host.to_lowercase())
            .chain(
                report
                    .defects
                    .iter()
                    .filter(|d| d.kind == DefectKind::StaleHostRegistry)
                    .filter_map(|d| d.host.as_ref().map(|h| h.to_lowercase())),
            )
            .collect();
        let mut by_lane: BTreeMap<String, (Option<&LaneRow>, Vec<String>)> = BTreeMap::new();
        for defect in &report.defects {
            let lane_scoped = matches!(
                defect.kind,
                DefectKind::NoEvents
                    | DefectKind::MissingLane
                    | DefectKind::StaleSource
                    | DefectKind::UnreadableSource
            );
            let Some(lane) = defect.lane.as_ref().filter(|_| lane_scoped) else {
                continue;
            };
            let row = report
                .rows
                .iter()
                .find(|r| r.lane.eq_ignore_ascii_case(lane));
            let host = row
                .map(|r| r.host.clone())
                .or_else(|| defect.host.clone())
                .map(|h| h.to_lowercase());
            if host.is_some_and(|h| unreachable.contains(&h)) {
                continue;
            }
            if row.is_some_and(|r| r.class == ClassKind::Unreachable) {
                continue;
            }
            let entry = by_lane.entry(lane.clone()).or_default();
            entry.0 = row;
            entry.1.push(format!("{:?}", defect.kind));
        }
        for (lane, (row, mut defects)) in by_lane {
            defects.sort();
            defects.dedup();
            let ticket = self
                .input
                .facts
                .assignments
                .as_ref()
                .and_then(|a| a.get(&lane.to_lowercase()))
                .and_then(|t| t.first().cloned());
            self.push(Cond {
                kind: ActionKind::ActOnUndetermined,
                subject: lane.clone(),
                lane: Some(lane.clone()),
                target: Target {
                    role: "lane",
                    addr: Some(lane.clone()),
                },
                reason: "lane_evidence_defective".to_owned(),
                severity: severity_of(ActionKind::ActOnUndetermined),
                evidence: defects.clone(),
                detail: json!({
                    "defects": defects,
                    "treat_as": "idle",
                    "class": row.map(|r| r.class),
                    "remaining_ticket": ticket,
                }),
            });
        }
    }

    fn fix_source(&mut self) {
        let input = self.input;
        for (defect, seen) in input.report.defects.iter().zip(input.first_seen) {
            let Some(seen) = seen else { continue };
            let age_ms = input.report.run_ms.saturating_sub(*seen);
            if age_ms <= millis(input.settings.source_defect_age) {
                continue;
            }
            let subject = format!(
                "defect:{:?}/{}/{}/{}",
                defect.kind,
                defect.source,
                defect.host.as_deref().unwrap_or("-"),
                defect.lane.as_deref().unwrap_or("-")
            );
            self.push(Cond {
                kind: ActionKind::FixSource,
                subject,
                lane: None,
                target: Target {
                    role: "orchestrator",
                    addr: input.settings.orchestrator.clone(),
                },
                reason: "source_defect_persists".to_owned(),
                severity: severity_of(ActionKind::FixSource),
                evidence: vec![defect.reason.clone()],
                detail: json!({
                    "defect": format!("{:?}", defect.kind),
                    "source": defect.source,
                    "host": defect.host,
                    "lane": defect.lane,
                    "first_seen_ms": seen,
                }),
            });
        }
    }

    fn stale_prs(&mut self) {
        let input = self.input;
        let Some(prs) = &input.facts.prs else { return };
        let limit = millis(input.settings.stale_pr_after);
        let mut by_lane: BTreeMap<&str, Vec<&super::LanePr>> = BTreeMap::new();
        for pr in prs {
            if input.report.run_ms.saturating_sub(pr.last_activity_ms) > limit {
                by_lane.entry(pr.lane.as_str()).or_default().push(pr);
            }
        }
        for (lane, mut stale) in by_lane {
            stale.sort_by(|a, b| a.pr.cmp(&b.pr));
            let (target, unresolved) = self.lead_target(lane);
            let reason = if unresolved.is_empty() {
                "stale_pr".to_owned()
            } else {
                unresolved
            };
            self.push(Cond {
                kind: ActionKind::EscalateStalePr,
                subject: lane.to_owned(),
                lane: Some(lane.to_owned()),
                target,
                reason,
                severity: severity_of(ActionKind::EscalateStalePr),
                evidence: stale.iter().map(|p| p.pr.clone()).collect(),
                detail: json!({
                    "pull_requests": stale.iter().map(|p| json!({
                        "pr": p.pr, "last_activity_ms": p.last_activity_ms})).collect::<Vec<_>>(),
                }),
            });
        }
    }

    fn drains(&mut self) {
        let input = self.input;
        let cap = millis(input.settings.drain_cap);
        for (lane, (host, since)) in &input.capacity.draining {
            if input.report.run_ms.saturating_sub(*since) <= cap {
                continue;
            }
            let subject = input
                .report
                .rows
                .iter()
                .find(|r| r.lane.eq_ignore_ascii_case(lane))
                .map_or_else(|| lane.clone(), |r| r.lane.clone());
            let (target, unresolved) = self.lead_target(&subject);
            let reason = if unresolved.is_empty() {
                "drain_over_cap".to_owned()
            } else {
                unresolved
            };
            self.push(Cond {
                kind: ActionKind::EscalateDrain,
                subject: subject.clone(),
                lane: Some(subject),
                target,
                reason,
                severity: severity_of(ActionKind::EscalateDrain),
                evidence: vec![format!("draining_mark at {since}")],
                detail: json!({"host": host, "draining_since_ms": since}),
            });
        }
    }

    fn add_lanes(&mut self, suppressed: &mut Vec<Suppressed>) {
        let input = self.input;
        let (Some(busy), Some(ready)) = (&input.facts.host_busy, &input.facts.ready) else {
            return;
        };
        for host in &input.report.hosts {
            let key = host.host.to_lowercase();
            let host_unreachable = input.report.defects.iter().any(|d| {
                d.kind == DefectKind::StaleHostRegistry
                    && d.host
                        .as_deref()
                        .is_some_and(|h| h.eq_ignore_ascii_case(&key))
            });
            let (Some(ratio), Some(capacity)) =
                (busy.get(&key), input.settings.lane_capacity.get(&key))
            else {
                continue;
            };
            let target = input
                .settings
                .busy_targets
                .get(&key)
                .copied()
                .unwrap_or(input.settings.default_busy_target);
            let lanes = input
                .report
                .rows
                .iter()
                .filter(|r| r.host.eq_ignore_ascii_case(&key))
                .filter(|r| !input.capacity.draining.contains_key(&r.lane.to_lowercase()))
                .count();
            let lanes = u32::try_from(lanes).unwrap_or(u32::MAX);
            if host_unreachable || *ratio >= target || ready.unassigned == 0 || lanes >= *capacity {
                continue;
            }
            let reason = if input.capacity.open_runs.contains(&key) {
                Some("discovery_active")
            } else if !input.capacity.levels.contains_key(&key) {
                Some("no_recorded_level")
            } else if input
                .capacity
                .levels
                .get(&key)
                .is_some_and(|level| lanes >= *level)
            {
                Some("at_recorded_level")
            } else if input
                .capacity
                .cool_off_until
                .get(&key)
                .is_some_and(|until| *until > input.report.run_ms)
            {
                Some("cool_off")
            } else {
                None
            };
            if let Some(reason) = reason {
                suppressed.push(Suppressed {
                    kind: ActionKind::AddLane,
                    subject: host.host.clone(),
                    reason: reason.to_owned(),
                });
                continue;
            }
            self.push(Cond {
                kind: ActionKind::AddLane,
                subject: host.host.clone(),
                lane: None,
                target: Target {
                    role: "host",
                    addr: Some(host.host.clone()),
                },
                reason: "busy_below_target".to_owned(),
                severity: severity_of(ActionKind::AddLane),
                evidence: vec![format!("busy_ratio={ratio} target={target}")],
                detail: json!({
                    "busy_ratio": ratio, "target": target, "lanes": lanes,
                    "lane_capacity": capacity, "ready_unassigned": ready.unassigned,
                }),
            });
        }
    }
}

fn millis(d: std::time::Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

/// Plans one tick.
pub fn plan(input: &PlanInput<'_>) -> Plan {
    let mut ctx = Ctx {
        input,
        conds: Vec::new(),
    };
    let mut out = Plan::default();
    for row in &input.report.rows {
        match row.class {
            ClassKind::Idle => {
                let ticket = input
                    .facts
                    .assignments
                    .as_ref()
                    .and_then(|a| a.get(&row.lane.to_lowercase()))
                    .and_then(|t| t.first().cloned());
                if let Some(ticket) = ticket {
                    ctx.lane_cond(
                        ActionKind::PushNextTicket,
                        row,
                        "idle_with_assignment",
                        json!({"ticket": ticket, "assignment_list": {
                            "source": "ix-relay FR-072", "position": 0}}),
                    );
                }
            }
            ClassKind::Blocked => ctx.blocked(row),
            ClassKind::Stuck | ClassKind::Loop => {
                let reason = if row.class == ClassKind::Stuck {
                    "stuck"
                } else {
                    "loop"
                };
                ctx.lane_cond(ActionKind::EscalateStuck, row, reason, json!({}));
            }
            ClassKind::StaleOrder => ctx.lane_cond(
                ActionKind::ResendOrder,
                row,
                row.reason.as_deref().unwrap_or("stale_order"),
                json!({}),
            ),
            _ => {}
        }
    }
    ctx.undetermined();
    ctx.fix_source();
    ctx.stale_prs();
    ctx.drains();
    ctx.add_lanes(&mut out.suppressed);
    let conds = std::mem::take(&mut ctx.conds);
    let now = input.report.run_ms;
    let interval = input.interval_ms;
    let open_by_key: BTreeMap<(ActionKind, &str), &OpenAction> = input
        .open
        .iter()
        .filter(|o| o.kind != ActionKind::LadderRung)
        .map(|o| ((o.kind, o.subject.as_str()), o))
        .collect();
    let mut live: BTreeSet<(ActionKind, String)> = BTreeSet::new();
    let mut rungs = Vec::new();
    for cond in conds {
        let key = (cond.kind, cond.subject.clone());
        let existing = open_by_key
            .get(&(cond.kind, cond.subject.as_str()))
            .copied();
        let (id, started, due_by) =
            existing.map_or((0, now, now + interval), |o| (o.id, o.started_at, o.due_by));
        live.insert(key.clone());
        let blocked_lane = cond.lane.as_ref().is_some_and(|lane| {
            input
                .report
                .rows
                .iter()
                .any(|r| r.lane.eq_ignore_ascii_case(lane) && r.class == ClassKind::Blocked)
        });
        let ladder = cond.kind.has_ladder();
        let lead_target = cond
            .lane
            .as_deref()
            .map(|lane| input.settings.leads.get(&lane.to_lowercase()));
        let planned = Planned {
            action: Action {
                id,
                kind: cond.kind,
                subject: cond.subject.clone(),
                target: cond.target,
                reason: cond.reason,
                severity: cond.severity,
                due_by,
                condition_started_at: started,
                rung: ladder.then_some(0),
                parent_action_id: None,
                evidence: cond.evidence,
                detail: cond.detail,
            },
            key: key.clone(),
            is_new: existing.is_none(),
            record_only: false,
        };
        out.actions.push(planned);
        let Some(lane) = cond.lane.filter(|_| ladder && existing.is_some()) else {
            continue;
        };
        let age = now - started;
        let sent: BTreeSet<u8> = input
            .open
            .iter()
            .filter(|o| o.parent == Some(id))
            .filter_map(|o| o.rung)
            .collect();
        let lead = lead_target.flatten();
        for rung in 1u8..=3 {
            let lead_missing = lead.is_none();
            let due = age >= i64::from(rung) * interval
                || (rung == 3 && lead_missing && age >= 2 * interval);
            if !due || sent.contains(&rung) || (rung == 1 && blocked_lane) {
                continue;
            }
            let (role, addr, reason, record_only) = match rung {
                1 => ("lane", Some(lane.clone()), "worker", false),
                2 => match lead {
                    Some(lead) => ("lead", Some(lead.clone()), "lead", false),
                    None => ("lead", None, "lead_unresolved", true),
                },
                _ if lead_missing => (
                    "orchestrator",
                    input.settings.orchestrator.clone(),
                    "lead_unresolved_fallthrough",
                    false,
                ),
                _ => (
                    "orchestrator",
                    input.settings.orchestrator.clone(),
                    "orchestrator",
                    false,
                ),
            };
            if record_only {
                out.suppressed.push(Suppressed {
                    kind: ActionKind::LadderRung,
                    subject: lane.clone(),
                    reason: reason.to_owned(),
                });
            }
            rungs.push(Planned {
                action: Action {
                    id: 0,
                    kind: ActionKind::LadderRung,
                    subject: lane.clone(),
                    target: Target { role, addr },
                    reason: reason.to_owned(),
                    severity: if rung == 3 {
                        Severity::High
                    } else {
                        Severity::Medium
                    },
                    due_by: now + interval,
                    condition_started_at: started,
                    rung: Some(rung),
                    parent_action_id: Some(id),
                    evidence: vec![format!("{} persists since {started}", key.0.name())],
                    detail: json!({"parent_kind": key.0.name()}),
                },
                key: key.clone(),
                is_new: true,
                record_only,
            });
        }
    }
    out.actions.extend(rungs);
    for open in input.open {
        let cleared = if open.kind == ActionKind::LadderRung {
            open.parent.is_some_and(|p| {
                !input
                    .open
                    .iter()
                    .any(|o| o.id == p && live.contains(&(o.kind, o.subject.clone())))
            })
        } else {
            !live.contains(&(open.kind, open.subject.clone()))
        };
        if cleared {
            out.resolved.push(open.clone());
        }
    }
    out.actions.sort_by(|a, b| {
        let key = |p: &Planned| {
            (
                p.action.severity,
                p.action.subject.to_lowercase(),
                p.action.kind,
                p.action.rung,
            )
        };
        key(a).cmp(&key(b))
    });
    out.suppressed
        .sort_by(|a, b| (&a.subject, &a.reason).cmp(&(&b.subject, &b.reason)));
    out
}
