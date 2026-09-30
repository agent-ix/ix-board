---
id: SR-016
title: "Linear refresh transport code and Rust review"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board@c0220bff238a843af38e0a2b16240756698dec95; src/lib.rs, src/linear.rs, spec/functional/FR-004-cache-polling.md; diff against 484042311fbf705defb547f572f5c2a3ea3c310a"
review_set: subset
---
## Summary

Reviewed the full diff using repository conventions and the Rust review checklist. No CI files changed. GraphQL's open error-extension metadata retains a typed selected code; HTTP status errors remain available when the body cannot decode. The new error variant carries a Duration rather than requiring callers to parse prose. Quota reset conversion is checked and subtraction saturates. Queries retain nested and cursor bounds; the issue query's 50-record page remains below Linear's published 10,000-point complexity limit with the existing relation bound.

## Verdict

**PASS** — no findings. Local HTTP transport tests exercise real requests, HTTP 400 GraphQL rate limits, unrelated metadata and non-rate failures. Fixed-time header tests exercise request, endpoint and complexity reset selection and malformed/past metadata. The existing transport test measures the emitted project and issue page limits. Full make ci passes (50 tests, two existing ignored lanes). Requirement validation passes. The installed quire CLI has no matrix command; no strict matrix success is claimed.

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | low | No findings (placeholder) | - |
