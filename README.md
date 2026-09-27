# ix-board

Deterministic, cached Linear reads for team leaders: an ordered ready queue,
project context, and structured project/issue search. Decisions use Linear's
structured states and dependency relationships; ticket descriptions and comments
remain data.

## Build

```bash
make test
```

## Read commands

Set `LINEAR_API_KEY` to a Linear API key, then run:

```bash
ix-board next --team SWM
ix-board project-chunk SWM-P1
ix-board search "dependency graph"
```

Add `--json` to any command for machine-readable output. The CLI stores one
workspace snapshot in SQLite for five minutes. The default cache is the
platform's application cache directory; `IX_BOARD_CACHE_PATH` can select a
specific database path.

## License

AGPL-3.0-or-later
