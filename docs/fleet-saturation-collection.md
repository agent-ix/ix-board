# Fleet saturation sample collection

`ix-board fleet saturation --json` is a read-only live report. It does not write
the fleet ledger. Capacity windows receive samples from
`ix-board fleet saturation collect --json`, which reads the live saturation
sources and appends one sample per configured host to the local ledger. The
collector does not send relay messages or issue lane orders.

The included macOS LaunchAgent runs the collector every 300 seconds, matching
the default `fleet.tick_interval_secs`. Install it on draco after ix-board is
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

Capacity windows weight saturation samples at the configured tick interval.
With one 300-second sample every 300 seconds, an hourly window reaches full
coverage; at least 11 of 12 expected samples reach 90% coverage. Missed or
unavailable samples retain their unavailable state and do not count as zero.
The panel reads status/history and does not run the collector.
