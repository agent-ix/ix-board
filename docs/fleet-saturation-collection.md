# Fleet saturation sample collection

`ix-board fleet saturation --json` is a read-only live report. It does not write
the fleet ledger. Capacity windows receive samples from
`ix-board fleet saturation collect --json`, which reads the live saturation
sources and appends one sample per configured host to the local ledger. The
collector does not send relay messages or issue lane orders.

The included macOS LaunchAgent runs the collector every 900 seconds to reduce
source reads. Install it on draco after ix-board is
installed and the fleet configuration, Prometheus URL, and read credentials
are available:

```sh
mkdir -p "$HOME/Library/LaunchAgents"
cp deploy/launchd/com.agentix.ix-board-saturation-collector.plist \
  "$HOME/Library/LaunchAgents/"
launchctl bootstrap "gui/$(id -u)" \
  "$HOME/Library/LaunchAgents/com.agentix.ix-board-saturation-collector.plist"
```

The plist prepends `$HOME/.local/bin` and `$HOME/.cargo/bin` to the supplied
Homebrew or system `PATH` before starting `ix-board`; it logs to `/tmp`. Install
`ix-board` and `agent-msg` in those directories or a standard `PATH` directory.
The logged-in account also needs its ix-relay and Linear credentials available
to the LaunchAgent. To inspect the job, use
`launchctl print gui/$(id -u)/com.agentix.ix-board-saturation-collector`.
To unload it, use
`launchctl bootout gui/$(id -u)/com.agentix.ix-board-saturation-collector`.

Collection checks ledger schema compatibility before reading live sources.
Install a compatible binary when the ledger schema exceeds the installed writer version.

Capacity windows weight saturation samples at the configured tick interval.
At the default 300-second tick, each sample covers at most five minutes.
The 900-second collection schedule therefore leaves hourly coverage incomplete.
Current deficit and discovery-start evidence expires after 600 seconds, so those values
are unavailable for about five minutes between scheduled collections.
Capacity calculations requiring 90% coverage remain unavailable until enough evidence exists.
Eleven complete five-minute samples would cover 90% of an hour; this eligibility threshold is not an operational SLA.
Missed or unavailable samples retain their unavailable state and do not count as zero.
The panel reads status/history and does not run the collector.

## GitHub evidence

The team exporter collects GitHub evidence every 3,600 seconds by default,
independently of its tracker and activity intervals. Set `github_interval_seconds`
between 3,600 and 86,400 seconds in the exporter configuration.
The persisted collection deadline survives restarts and serializes scope sweeps.

HTTP reads return cached evidence with original last-success timestamps and source ages.
GitHub values expire after the configured interval plus a 15-minute allowance.
This expiry allowance is a cache validity setting.
Response-cache reuse retains its separate 15-minute TTL.
Failed refreshes never advance success timestamps; incomplete populations remain unavailable.

API admission ceilings apply to scheduled and manually invoked readers alike:
40 attempts and 1,000 reserved points per rolling hour, with 25 points reserved per attempt.
Each scope retains its 45-second collection admission bound.
Cooldown deadlines survive restart and suppress requests until expiry.
An urgent status request receives cached evidence and its age; the HTTP interface offers no force-refresh route.

Each HTTP attempt logs bounded status, cost, remaining quota, reset, backoff, duration,
and local rolling-hour reservations. The journal retains the latest attempt in
`github_health`, cumulative cache hits, and the next collection deadline in `github_collection`.
Inspect these local records to distinguish server limits, local headroom, permission,
transport, and validation failures. Credentials, repository contents, and response bodies are excluded from diagnostics.
