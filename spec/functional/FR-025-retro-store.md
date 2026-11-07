---
id: FR-025
title: Keep retro snapshots in a versioned append-only store outside git
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-009"
    type: implements
---
# FR-025: Keep retro snapshots in a versioned append-only store outside git

## Description

The retro commands SHALL keep their records in one append-only JSONL file per
team at `$TEAM_RETRO_HOME/<team>/snapshots.jsonl`, defaulting `TEAM_RETRO_HOME`
to `~/.local/state/team-retro`. Each line SHALL be one record carrying
`schema_version`, `kind` (`snapshot` or `backfill`), `taken_at` (UTC), `team`,
`window`, and the metric map of [FR-024](./FR-024-retro-snapshot.md).

When a retro command appends, it SHALL write the whole line with a single write to a
file opened in append mode, and SHALL create the directory when absent. The retro
command SHALL NOT rewrite or truncate an existing line.

While a retro command is about to write, it SHALL resolve the store path, following
symbolic links, and SHALL refuse the write if the resolved path lies inside a
git work tree or below a directory that contains a `.git` entry. If the path is refused, then the retro command SHALL name the offending path, exit non-zero and create no file.

When a command reads a line that does not parse, it SHALL stop with an error
naming the line number and exit non-zero. If the final byte of the file is not
a newline, the append command SHALL refuse to write and name the file.

When a retro command reads a record with a `schema_version` newer than it
understands, it SHALL stop with an error naming that version. The command SHALL
read an older version without rewriting it.

The retro commands SHALL NOT write retro data to Linear, GitHub or any
repository; the store is the only write this feature adds. The store orders
records by `taken_at`, with file order breaking ties.

## Acceptance Criteria

| ID | Criteria | Verification |
|----|----------|--------------|
| FR-025-AC-1 | With `TEAM_RETRO_HOME` unset, the store path is `~/.local/state/team-retro/<team>/snapshots.jsonl`; with it set, the path is under the given directory. | Test |
| FR-025-AC-2 | Two consecutive snapshots add two lines and leave the first line byte-identical. | Test |
| FR-025-AC-3 | A store path inside a git work tree, including through a symbolic link, is refused with a message naming the path, a non-zero exit, and no file or directory created. | Test |
| FR-025-AC-4 | A record with a newer `schema_version` stops the command with an error naming that version; a record with an older version is read and left unchanged. | Test |
| FR-025-AC-5 | Every record carries `schema_version`, `kind`, `taken_at`, `team`, `window` and a metric map, and no line contains a credential value. | Test |
| FR-025-AC-6 | A store with an unparsable line makes read commands stop naming that line number, and a store whose last byte is not a newline makes the append command refuse; neither command alters the file. | Test |
| FR-025-AC-7 | Records appended out of `taken_at` order are read back in `taken_at` order, with file order breaking ties. | Test |

## Dependencies

- **Upstream**: [US-009](../usecase/US-009-review-work-convergence.md); [FR-015](./FR-015-protected-credential-value.md) for credential handling; [FR-024](./FR-024-retro-snapshot.md) for the record content.
- **Downstream**: [FR-026](./FR-026-retro-unavailable-not-zero.md), [FR-027](./FR-027-retro-delta.md), [FR-028](./FR-028-retro-backfill.md).
