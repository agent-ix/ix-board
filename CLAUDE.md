# ix-board

ix-board is a command-line tool and Rust library that reads a Linear workspace and answers planning questions (ready queue, focus reports, program review, agent activity, drift) from structured data. Ticket prose is data and never drives a decision. See [README.md](README.md) and [docs/](docs/how-it-works.md).

## Commands

```bash
make fmt            # format with rustfmt
make fmt-check      # verify formatting (CI gate)
make lint           # clippy with -D warnings
make test           # cargo test
make build          # release build
make clean          # cargo clean
make deny           # cargo deny check licenses
make audit-unsafe   # check that every unsafe block has a // SAFETY: comment
make ci             # fmt-check + lint + test + deny + audit-unsafe
```

## Safety scaffolding

Backported from `agent-ix/ecaz`:

- `clippy.toml` pins MSRV to `1.98.1` and caps cognitive complexity / arg count
- `deny.toml` allow-lists licenses and denies unknown registries/git sources
- `scripts/check_unsafe_comments.sh` runs in CI and locally via `make audit-unsafe`. Every `unsafe {` block must have a `// SAFETY:` comment within the 3 preceding lines, or be listed in `scripts/unsafe_comment_baseline.txt`. Update the baseline with `bash scripts/check_unsafe_comments.sh --update-baseline`.
- `rustfmt.toml` uses 100-char width and `StdExternalCrate` import grouping. CI fails on drift.
- `rust-toolchain.toml` pins to stable + rustfmt + clippy.

## Layout

```
src/lib.rs             # read service, ready queue, focus, project chunk
src/main.rs            # clap command tree and text renderers
src/cache.rs           # SQLite snapshot cache (five-minute TTL) and hygiene baseline
src/linear.rs          # Linear GraphQL client
src/scope.rs           # selector resolution and leaf progress
src/hygiene.rs         # focus hygiene report and diff
src/program_review.rs  # scope file, scope check, roadmap
src/{chain,orphans,reconcile}.rs   # program review commands
src/drift.rs           # drift evidence, signals and alerts
src/agents/            # agents verdicts, Herdr and harness adapters
examples/              # export_schema (JSON Schema for wire types)
tests/                 # integration tests and fixtures
spec/                  # requirements artifacts
docs/                  # user documentation and figures (docs/images/figures.py)
plugin/                # Claude Code and Codex plugin skills
scripts/               # local tooling
```
