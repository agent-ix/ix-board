---
id: SR-6023
title: "Spec integrity review — ix-board SWM-141 retro"
type: SpecReview
analysis: integrity
scope: "agent-ix/ix-board; spec/usecase/US-009-review-work-convergence.md, spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-025-retro-store.md, spec/functional/FR-026-retro-unavailable-not-zero.md, spec/functional/FR-027-retro-delta.md, spec/functional/FR-028-retro-backfill.md, spec/spec.md (diff only)"
review_set: all
---

## Summary

Ticket: SWM-141. The integrity lens of a review-set `all` spec review of the uncommitted SWM-141 retro spec change (US-009, FR-024..FR-028 and the spec/spec.md edits), checked against design source Deliverable 3 ("Code needed?") and SWM-132/SWM-133. Only the diff was reviewed; pre-existing defects in other files are out of scope. Units examined: FR-024, FR-024-AC-1, FR-024-AC-2, FR-024-AC-3, FR-024-AC-4, FR-024-AC-5, FR-024-AC-6, FR-025, FR-025-AC-1, FR-025-AC-2, FR-025-AC-3, FR-025-AC-4, FR-025-AC-5, FR-026, FR-026-AC-1, FR-026-AC-2, FR-026-AC-3, FR-026-AC-4, FR-026-AC-5, FR-027, FR-027-AC-1, FR-027-AC-2, FR-027-AC-3, FR-027-AC-4, FR-027-AC-5, FR-027-AC-6, FR-028, FR-028-AC-1, FR-028-AC-2, FR-028-AC-3, FR-028-AC-4, FR-028-AC-5, US-009-EX-1, US-009-EX-2, US-009-EX-3.

## Verdict

**FAIL** — FR-024 contradicts itself on reading comment text, and two metrics cite readers that do not compute them. FR-025, FR-026 and FR-027 are internally consistent.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | FR-024 says the command SHALL NOT read comment prose to derive a value. Its own `fix_rounds_*` metric (line 41) is derived from Linear comment bodies, and only AC-5 grants the exception; the Description does not. The marker grammar it parses is not defined. | spec/functional/FR-024-retro-snapshot.md:21-23 |
| FND-002 | medium | `issues_without_milestone` is attributed to FR-023, which has no milestone logic, so the 'reuse the FR-023 reader' claim is false for that id. | spec/functional/FR-024-retro-snapshot.md:43 |
| FND-003 | medium | `prs_ready_unmerged` depends on 'an approving structured Linear attachment state', a term defined nowhere in the spec. | spec/functional/FR-024-retro-snapshot.md:40 |
| FND-004 | medium | `net_closed_minus_new` adds 'other completed issues in the configured projects' with no window bound, and does not say how path issues outside the configured projects are counted. | spec/functional/FR-024-retro-snapshot.md:34 |
| FND-005 | low | `agents_working_share` counts 'verdict `working`', but FR-018 verdicts are `WORKING` and herdr state is `working`, two different sets. | spec/functional/FR-024-retro-snapshot.md:45 |

## Finding detail

- **FND-001** (FR-024): Description lines 21-23 forbid deriving a value from ticket or comment prose. Line 41 derives fix_rounds from `reviewer-dispositions` markers, which sit inside comment bodies; AC-5 (line 61) allows reading them, which the Description does not. FR-024 also leaves undefined which marker fields count (the `round=` value, or the number of comments), how a marker maps to a PR (`pr=<repo>#<n>`?), and whether duplicate or edited markers count. Peter's constraint is that ix-board does not interpret comment prose (FR-005). Failure: an implementer who follows the Description cannot compute the metric. One who follows AC-5 writes a free-text comment parser that a stray `reviewer-dispositions` string in a human comment inflates. Fix: state the exception and the exact marker grammar in the Description as a structured-field rule, or source rework from a structured field.
- **FND-002** (FR-024): FR-023 reports only issues with no project (FR-023:13-24). Failure: the implementer either adds milestone logic to orphans without a requirement or writes a new collector, which goes against the reuse constraint, and nothing specifies whether completed or canceled issues count (FR-023 excludes canceled and duplicate issues).
- **FND-003** (FR-024): Neither ix-board's readers nor any FR define an approval state for Linear GitHub attachments. GitHub's review decision is the structured approval source. Failure: two implementers read approval from different fields (Linear attachment metadata status, or GitHub reviewDecision) and report different counts for the same PRs.
- **FND-004** (FR-024): Line 34 does not say 'inside the window' for 'other completed issues'. The path set comes from cross-team FR-022 expansion and can include issues outside the configured projects. Failure: one reading counts every completed issue in the projects ever, which makes net hugely positive. Another counts path issues inside the projects twice.
- **FND-005** (FR-024): FR-018 rules 2, 3 and 7 assign `WORKING` to agents whose herdr state is not `working` (live subagent, recent turn). Failure: counting herdr state under-reports the share relative to the FR-018 verdict the spec cites.

## Review evidence

- src/ has no Linear comment reader (`grep -n comments src/*.rs` is empty); `src/linear.rs:82` has the IssueHistory query used by drift.
- FR-023 (spec/functional/FR-023-unprojected-issues.md:13-24) reports unprojected issues only; it has no milestone logic.
- FR-018 verdicts are upper-case (`WORKING`, `IDLE`...); herdr state is lower-case `working`.

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | The Description now names the exception (first-line `reviewer`/`reviewer-dispositions` markers parsed as key=value), line 63 defines the counting rule, and AC-6 (line 87) excludes body-embedded markers and human comments. Proof: spec/functional/FR-024-retro-snapshot.md:19-22. |
| FND-002 | fixed | `issues_without_milestone` is now read from the structured issue record and no longer credited to FR-023. FR-024:99 lists issue milestones as a new reader. Proof: spec/functional/FR-024-retro-snapshot.md:67-67. |
| FND-003 | fixed | `prs_ready_unmerged` now uses GitHub review decision `APPROVED`. Proof: spec/functional/FR-024-retro-snapshot.md:62-62. |
| FND-004 | fixed | The completions are now bounded to the window, and path issues are counted once. Proof: spec/functional/FR-024-retro-snapshot.md:56-56. |
| FND-005 | fixed | The metric now counts the FR-018 verdict `WORKING`, read for `herdr_teams`. Proof: spec/functional/FR-024-retro-snapshot.md:70-70. |

FND-001 after_excerpt:

> ending at the snapshot time, and state metrics at the snapshot time. The command SHALL NOT mutate Linear, GitHub or any repository. The
> command SHALL NOT derive a value from ticket or comment prose; the only comment
> text it reads is the first-line `reviewer` and `reviewer-dispositions` markers
> that `/reviewer` posts, parsed as the key=value fields defined below.

FND-002 after_excerpt:

> | scope | `issues_without_project`, `issues_without_milestone` | Open issues of `linear_teams` lacking a project, or lacking a milestone in the structured issue record. |

FND-003 after_excerpt:

> | flow | `prs_ready_unmerged` | Open non-draft pull requests whose GitHub review decision is `APPROVED`. |

FND-004 after_excerpt:

> | progress | `net_closed_minus_new` | Issues in the configured projects completed inside the window (path issues counted once) minus `intake_new`. |

FND-005 after_excerpt:

> | utilization | `agents_working_share` | Agents whose [FR-018](./FR-018-agent-activity-verdicts.md) verdict is `WORKING` divided by agents reported for `herdr_teams`. |

