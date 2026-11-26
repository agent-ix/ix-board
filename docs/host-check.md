# Host check and monitoring command map

Run `ix-board host check` on each host. It takes no flags, uses that host's
non-login process environment, and returns 0 only when authentication, locked-
build telemetry, admission policy, and process inspection all pass. It emits
safe status labels and metrics only. A cadence manifest can run it locally:

```json
{
  "jobs": [{
    "program": "QSL",
    "team": "qsl",
    "id": "host-check",
    "interval_secs": 300,
    "kind": "host_check",
    "command": ["host", "check"],
    "owner_role": "minder"
  }]
}
```

The job runs wherever `ix-board cadence run host-check` runs. Install the same
cadence manifest and scheduler on each machine. Configure `IX_BOARD_HOST` when
the local Herdr server identity is not discoverable, and configure
`IX_PROMETHEUS_URL` (or `[fleet].prometheus_url`) plus
`[hygiene.hosts.<host>]` policy for locked-build capacity.

## `_lead-bin` script disposition

| Script | ix-board command or cadence job | Disposition |
| --- | --- | --- |
| `host-probe.sh` | `host check` / cadence `host_check` | Replaced by the local per-host report. |
| `host-watch.sh` | `fleet status` and cadence `fleet_tick` | Use structured host/lane coverage and the fleet ledger instead of local notifications. |
| `hosts-status.sh` | `build status --metrics-url <url>` | Use the all-configured-host capacity table. |
| `lane-status.sh` | `build status --metrics-url <url>` and `agents <TEAM>` | Split host metrics from evidence-backed lane verdicts. |
| `status-qsl.sh` | `agents QSL` | Read the QSL lane state directly; the legacy relay status-request wrapper is delete. |
| `monitor.sh` | cadence `host_check` and `fleet_tick`; `agents QSL`; `next --team QSL`; `reconcile <project>` | Replace its host, lane, ready-queue and PR views with their structured ix-board reads. |
| `watchdog.sh` | scheduled `cadence due --program <P> --team <T> --manifest <file>` | Use due/overdue status from the persistent cadence ledger. |
| `plan-check.sh` | `focus status`, `next --team QSL`, `reconcile <project>` | Run only the structured views needed for the plan. |
| `rss-watch.sh` | `host check` | Delete the polling/kill script; host check reports suspicious quire/quoin/rg working directories without killing processes. |
| `defect-rate.sh` | `retro eval --config <file>` | Use configured retro metrics and evidence sources. |
| `probe-linux.sh` | `host check` / cadence `host_check` | Replaced by the same host-local command on Linux, macOS and WSL. |

`status-qsl.sh` is a thin wrapper with no independent host or lane analysis and
is marked for deletion after the `agents QSL` view is adopted.
