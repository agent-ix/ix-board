---
id: FR-020
title: Build a live program roadmap
type: FR
provenance:
  source: SWM-103
---
# FR-020: Build a live program roadmap

## Description

The `roadmap` command SHALL build a deterministic program view from one
structured Linear workspace snapshot, with an optional program scope file.
It SHALL group projects under initiatives, place a shared project once, show
other initiative memberships, identify projects without an initiative, and
identify initiatives without projects. A scoped roadmap SHALL include only
the initiatives and projects declared by that scope.

The command SHALL use the shared initiative/project summary cache without
requesting issues or histories from Linear. Each project row SHALL use native Linear
`Project.progress` for progress and identify its provenance as `linear_native`.
Native progress is an estimate-weighted fraction with started work contributing
partial progress; it is not the completed-ticket percentage. Available native
count summaries SHALL identify their source and observation time; unsupported
open/completed/canceled counts or issue last-activity values SHALL be null with an
unavailable reason, never inferred from progress or fabricated as zero. Project
`updatedAt` SHALL be labeled project update time rather than issue last activity.
Persisted source summaries SHALL retain fetch provenance and SHALL NOT become
user-maintained scope tallies. Projects and initiatives SHALL have stable,
deterministic ordering. Existing count fields SHALL remain present as nullable
values when their native source is unavailable.

Each initiative and project entry SHALL carry its Linear UUID (`id`) and URL
slug (`identifier`, null when the source supplies none). Each initiative entry
SHALL carry `member_project_ids`: the sorted UUIDs of every listed project that
belongs to it, including projects listed under another primary initiative. The
slug is a roadmap output; focus selectors take the UUID or name.

Where the local cache holds an issue snapshot, however old, the command SHALL
fill each project's `done`, `total`, `open` and `canceled` from that snapshot
with the FR-010 leaf classification (`total` is `done` plus `open`), and SHALL
name the source (`counts_source`) and the snapshot's fetch time
(`counts_observed_at`), with an empty `counts_unavailable_reason`. The counting
makes no Linear request. Without a readable snapshot, or for a project the
snapshot does not hold, the counts SHALL be null with a reason.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-020-AC-1 | The roadmap groups projects under their initiative memberships, places a shared project exactly once under its declared primary initiative when scoped (or deterministic primary membership when unscoped), and reports remaining memberships as `also_in`. | Test |
| FR-020-AC-2 | A cold roadmap performs no issue/history requests, reports native progress with `linear_native` provenance and observation time, and distinguishes it from ticket completion; count fields without a cached issue snapshot and unsupported activity fields are present as null with unavailable reasons in JSON and text. Project update time is never mislabeled issue activity and no scope tally is used. | Test |
| FR-020-AC-3 | Scoped output contains only scope-declared initiatives and projects; the roadmap reports declared initiatives without projects and projects in multiple initiatives, and an unscoped roadmap reports projects with no initiative membership as unplaced. | Test |
| FR-020-AC-4 | Initiative, project, and diagnostic rows, including each `also_in` membership list, are ordered case-insensitively by name, with original spelling as a deterministic tie-breaker when names have the same case-folded value. | Test |
| FR-020-AC-5 | The roadmap uses a structured Linear snapshot and performs no Linear or GitHub writes. | Inspection |
| FR-020-AC-6 | A scoped roadmap reports scope-declared initiatives and projects absent from Linear in `missing_scope_initiatives` and `missing_scope_projects`; reports Linear projects assigned to a scope-declared initiative but absent from the scope file in `projects_missing_from_scope`; and reports each scope-listed project with differing declared and Linear initiative memberships, with declared memberships absent from Linear as `missing_from_linear` and Linear memberships absent from the scope file as `missing_from_scope`. | Test |
| FR-020-AC-7 | Every initiative and project entry carries its Linear UUID and slug, read in the summary queries without further requests; an initiative's `member_project_ids` holds every listed member project, including one listed under another initiative. | Test |
| FR-020-AC-8 | With a cached issue snapshot, each project's `done` and `total` equal the `focus status` leaf progress for that project over the same snapshot, `open` and `canceled` are leaf counts, and source and fetch time are named, with no source read; without a snapshot, or for a project it does not hold, counts are null with a reason. | Test |

## Dependencies

- Structured Linear initiative/project summary records with the freshness and coverage metadata in [FR-030](FR-030-incremental-linear-cache.md).
- Optional program scope file conforming to FR-019.
