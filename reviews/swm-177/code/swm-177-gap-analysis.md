---
id: SR-6065
title: "Gap analysis — native project status preservation"
type: SpecReview
analysis: gap-analysis
scope: "agent-ix/ix-board; SWM-177 PR73; repository matrix and reverse inventory with changed normalization/model/cache inspected in detail"
review_set: subset
relationships:
  - {target: "ix://agent-ix/ix-board", type: references}
  - {target: "ix://agent-ix/ix-board/FR-005", type: references}
---
## Summary

Planless audit for SWM-177 frozen head 6a43edb82fafa4721fea84d6efffe3cf061393b3 against base 2154f2d5b1f2cc540772dfdf4e6ab27fd0479658. Consumer: https://linear.app/agent-ix/issue/SWM-176. The repository retains the accepted legacy traceability baseline; the scoped code defect is separately recorded in SR-6064, without duplicating it here.

## Verdict

**FAIL; repository assurance INCOMPLETE** — the computed matrix retains177 untagged legacy criteria. The standing dispatch explicitly accepts this unchanged baseline outside SWM-177 scope; it is not a newly introduced source gap. Merge still requires disposition of SR-6064 FND-001 and the lead's final aggregate gate.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | high | The computed repository matrix retains177 untagged legacy criteria, including FR-001-AC-1; existing gaps prevent full repository assurance. The category repair adds no criterion and does not repair this accepted baseline. | spec/functional/FR-001-ready-queue.md:52; matrix.json |

## Coverage

Plan completion: not assessed

Reconciliation: quire matrix --scope /Users/peter/dev/ix-board-swm-177 --format json exit0; quire0.52.0 engine0.62.2, quoin0.36.7, spec-artifacts-process@v0.29.2. matrix.json records229 criteria:44 tagged,177 untagged,8 method-without-symbol, zero ignored-only. No execution-evidence store was read. Counts match the accepted release baseline SR-6063; no grep fallback or baseline repair was used. Existing conditional-symbol collisions, legacy unrecognized comment tags and NFR heading extraction diagnostics remain. The matrix is not a full assurance PASS and tags alone are not behavioral evidence.

quire coverage at the same explicit repo scope exit0; coverage.json records Rust330 candidates,269 tagged,106 bound in rust-trace-attribute form, zero untracked_symbols. Compared with the retained release baseline327 candidates,269 tagged,106 bound, the three new test functions add no trace bindings. No new criterion was authored; current FR-001/004/005 own this normalization repair. Literal existing ACs do not separately specify category serialization/completion. Do not claim the three new tests close the legacy matrix or manufacture an inaccurate binding.

Reverse inventory used source/test/spec paths, public-inventory.txt and stub-scan.txt. Public families ready/project/search/cache, project writes, focus, agents, drift, program scope/roadmap/reconcile/chain/orphans, retro store/catalog/snapshot/delta/backfill and department sources/assessment map to FR001–029 and NFR001/master scope. The new public category/completion and retained fields preserve already-fetched structured source evidence under FR005/004 and preserve FR001 readiness. No new unowned behavior, confirmed new source stub, confirmed new test stub, new ignored test or gate weakening was found. Optional gap-analysis semantic fan-out was skipped; ordinary code-review inspected the changed behavior and finite failure oracles. Full legacy body-by-body reverse assurance remains incomplete, as in the accepted baseline; this inventory is not a semantic reacceptance of unchanged modules. No line coverage was collected, and passing tests do not establish absence of coverage inflation.

No plan was supplied or selected. No applicable AssuranceProfile exists. No compiler, formatter, tests, live ix-board command, real credentials, install, branch mutation, additional worktree or target, aggregate gate, push or merge occurred. BUILDQUIESCENT. Tool versions are newer than the historical rollout notice, not silently substituted older binaries. Artifact validation is performed at these explicit scratch paths against the sole repository.

## Examined units

```yaml
scope:
  - {id: FR-001-AC-1, path: spec/functional/FR-001-ready-queue.md, role: examined, excerpt: "Given fixture projects in `Started` and `Paused`, `next` includes eligible tickets only from `Started` projects."}
  - {id: FR-004, path: spec/functional/FR-004-cache-polling.md, role: examined, excerpt: "The ix-board read library SHALL use SQLite to cache structured Linear read data."}
  - {id: FR-005, path: spec/functional/FR-005-structured-data-boundary.md, role: examined, excerpt: "The ix-board read commands SHALL base filtering, readiness, ordering, and other command decisions on structured Linear fields."}
```

## Dispositions

| FND | outcome | reason |
| --- | --- | --- |
| FND-001 | accepted-no-change | Round1 standing dispatch authority explicitly excludes the accepted prior177 legacy trace gaps from this bounded normalization repair. Retain repository FAIL/INCOMPLETE; no broad repair or duplicate functional gate. |
