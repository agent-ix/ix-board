---
id: SR-6032
title: "ix-board retro snapshot spec edits review (SWM-142)"
type: SpecReview
analysis: base
scope: "agent-ix/ix-board@585c3f2c42c3dbffe68da3e07af6176fa8377c01; spec/functional/FR-024-retro-snapshot.md, spec/functional/FR-026-retro-unavailable-not-zero.md (diff only)"
review_set: subset
relationships:
  - target: "ix://agent-ix/ix-board/FR-024"
    type: reviews
  - target: "ix://agent-ix/ix-board/FR-026"
    type: reviews
---

# SR-6032: ix-board retro snapshot spec edits review (SWM-142)

## Summary

Ticket: SWM-142. A review of the spec edits this code PR makes to FR-024 (the
configuration rows, metric definitions, the history-read paragraph and
FR-024-AC-6/AC-11/AC-18) and FR-026 (the paired-field clause). Only the diff
was reviewed. The edits read as system behavior, not a build log. AC-18
is testable. The `orphan_slot_hours` and `message_stale_minutes` rows remove
an earlier ambiguity.

## Verdict

One medium defect in the edited fix_rounds definition. The history-read
paragraph justifies skipping reads with "an issue cannot change state without
being updated", which holds only for a fresh `updated_at`. SR-6030 FND-003
covers the cached-snapshot consequence on the code side.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The FR-024 fix_rounds definition keys pull requests by `<repo>#<n>` with no owner, and it matches dispositions to reviewer markers by `id=` alone. SR ids are allocated per repository, so a disposition for SR-002 in another repository counts toward this team's PR whose reviewer marker is also SR-002. The definition should also require the disposition's `repo=` to match the reviewer marker's `repo=`. | spec/functional/FR-024-retro-snapshot.md:118 |
| FND-002 | low | The `head_shas_per_pr_max` definition ("distinct head commits ... from that pull request's paginated timeline") does not say whether every commit counts or only the commits that were the head after a push, and the implementation counts every commit (SR-6030 FND-004). | spec/functional/FR-024-retro-snapshot.md:119 |
