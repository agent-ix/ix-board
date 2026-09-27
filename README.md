# ix-board

Deterministic, cached Linear reads for team leaders: an ordered ready queue,
project context, and structured project/issue search. Decisions use Linear's
structured states and dependency relationships; ticket descriptions and comments
remain data.

## Project queue convention

Linear project status controls whether a project runs: `Started` projects
contribute tickets to `next`, while `Paused` projects are on hold and their
tickets are excluded. Project priority and manual order determine the global
queue position before ticket priority and manual order. ix-board may write only
Linear project status and priority; it does not write manual order or any other
Linear fields.

The project mutation commands are `project set-status <project> <Started|Paused>`
and `project set-priority <project> <0|1|2|3|4>`. The priority values map to
Linear as `0` = No priority, `1` = Urgent, `2` = High, `3` = Medium, and `4` =
Low. These make an explicit update to the requested project's corresponding
Linear field.

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
