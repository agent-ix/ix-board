---
id: FR-048
title: Report build-host hygiene and admission status
type: FR
relationships:
  - target: "ix://agent-ix/ix-board/US-011"
    type: satisfies
  - target: "ix://agent-ix/ix-board/FR-043"
    type: depends_on
  - target: "ix://agent-ix/ix-board/FR-041"
    type: depends_on
---
# FR-048: Report build-host hygiene and admission status

## Description

The `ix-board hygiene check` and `ix-board hygiene clean` commands SHALL inspect build-host disk
and worktree hygiene using explicit host policy and process evidence. The `ix-board build status`
command SHALL report comparable load, memory, swap, locked-build queue, and active lease data for
all configured hosts and apply each host's admission policy. Running-agent build detection SHALL
use locked-build lease evidence instead of identifying a shell wrapper by its name.

## Behavior

- **Host policy.** Disk free-space floors, worktree roots and clean idle age are configuration
  fields. The default fleet policy uses 150 GB for terra and 100 GB for other hosts. Policy values
  are not embedded in check or clean logic.
- **Check.** `hygiene check --host H` reports free bytes and floor, target directories in worktrees,
  worktree age, active process use, prune status and orphan processes. Exit status is stable:
  0 when all configured checks pass, 1 when policy findings exist, 2 when the host or a required
  source is unavailable, and 3 on Windows where this feature is unsupported.
- **Process use.** On Linux, cwd, cmdline, environment, file descriptors and memory maps determine
  whether a process uses a target directory. On macOS, lsof and proc_pidinfo-equivalent APIs provide
  the same evidence. Incomplete process evidence is reported as unknown and is never treated as
  unused. Windows is unsupported.
  Unprivileged inspection reconciles `lsof` paths and `ps` commands for the invoking
  user's sampled live PIDs. Build environments are required for cargo/rustc processes.
  The report labels this evidence `current_user`; cleanup requires targets and
  containing worktrees owned by that user, in a root reserved for that account.
- **Clean.** `hygiene clean --host H` is a dry run unless `--apply` is passed. A target directory
  is eligible only if it is older than configured idle age, belongs to a worktree, is not in use by
  any process, and is not under a source, `.git`, `dev_bak`, or `dev_archive` path. Apply removes
  only eligible target directories. The decision logic accepts an injected process table.
- **Mount safety.** Hygiene reads the host mount table before scanning or cleaning. Worktrees and
  targets on a different filesystem from the configured worktree root are excluded, and a target
  containing a nested mount point is reported as ineligible and is never counted or removed across
  that boundary. An unreadable mount table makes the affected result unknown and prevents cleanup.
- **Build status.** The all-host table reports one-minute load per core, memory and swap use,
  `ix_build_queue_depth`, active `ix_build_lane_running` leases and an eligibility verdict from
  explicit per-host policy. A host with missing or stale evidence is ineligible. A heavy gate is
  eligible on luna only when fewer than one heavy gate is running and one-minute system load is
  below 8;
  policy may set different limits for other hosts.
- **Lease detection.** Agent activity reports a build as active when the current owner has a
  running locked-build lease. The process name or command text `locked-build.sh` does not establish
  a lease.

## Acceptance Criteria

| ID | Criteria | Verification |
| --- | --- | --- |
| FR-048-AC-1 | Host policy values control the free-space result; a fixture below its configured floor yields exit status 1 and a fixture above the floor yields 0. | Test |
| FR-048-AC-2 | Linux process evidence considers cwd, cmdline, environment, file descriptors and maps; a process reference to a target marks it in use, and incomplete evidence is unknown. | Test |
| FR-048-AC-3 | macOS process inspection reports process use through the platform source; unsupported Windows reports status 3. | Test |
| FR-048-AC-4 | Check reports target worktrees, prunable worktrees and orphan processes deterministically. | Test |
| FR-048-AC-5 | Clean is dry-run by default, removes only eligible idle targets under `--apply`, and preserves active targets, source, `.git`, `dev_bak` and `dev_archive`. | Test |
| FR-048-AC-6 | An injected process table produces the same cleanup decisions independent of the host's live process list. | Test |
| FR-048-AC-7 | Build status parses Prometheus text exposition and reports per-host load per core, memory, swap, queue depth and running leases in one deterministic table. | Test |
| FR-048-AC-8 | Eligibility uses the host policy and missing or stale telemetry is ineligible; luna is eligible for one heavy gate only when no heavy gate is running and system load is below 8. | Test |
| FR-048-AC-9 | Agent activity recognizes locked-build ownership from lease evidence and does not infer it from `locked-build.sh` command text. | Test |
| FR-048-AC-10 | An injected mount table proves hygiene ignores worktrees on an unexpected filesystem and refuses to scan or clean a target containing a nested mount point; unreadable mount evidence prevents cleanup. | Test |
| FR-048-AC-11 | Target discovery includes worktree targets at depths one through three under the configured root, deterministically, without following symlinks, traversing backups, or crossing filesystem boundaries; directories without a containing worktree are excluded. | Test |
| FR-048-AC-12 | Unprivileged lsof and ps reconcile cwd evidence for the invoking user's live processes on macOS and Linux. A missing live process cwd, failed inventory, or missing build environment leaves evidence incomplete; exited processes do not. Cleanup is restricted to worktrees owned by that user, preserves live references, and rechecks process evidence and the configured idle age (default one hour) before applying. | Test |

## Traceability

SWM-326, SWM-328 and SWM-334.

## Dependencies

- FR-043 provides the agent activity and evidence model extended by lease-aware build detection.
- FR-041 provides shared workspace and host configuration conventions used by the host commands.
