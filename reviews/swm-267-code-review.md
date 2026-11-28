---
id: SR-6114
title: "Code review — ix-board SWM-267 Linux x86_64 release binary"
type: SpecReview
analysis: code-review
scope: "agent-ix/ix-board; .github/workflows/release.yml"
review_set: subset
---

## Summary

Ticket: SWM-267. PR agent-ix/ix-board#91 at `2e147ddf6b63e63baab078075c3869608b9a7bff`, reviewed as `git diff origin/main...HEAD`. The diff adds a new `.github/workflows/release.yml` with one job, `linux-binary`. It builds `ix-board` for `x86_64-unknown-linux-gnu`, packs it with LICENSE, writes a `.sha256` and attaches both to the tag's GitHub release with `softprops/action-gh-release@v2`.

Evidence: `actionlint .github/workflows/release.yml` exits 0. glibc measured over ssh: titan 2.43, luna 2.39, terra WSL 2.35. The v0.7.2 darwin asset was downloaded and listed for comparison.

## Verdict

**CHANGES REQUESTED** for FND-001, or a measured disproof of it. FND-002 needs a fix or a documented release order.

Checked and clean:

- **Triggers.** `on: push: tags: ["v*.*.*"]` only. There is no PR, branch or dispatch trigger.
- **Toolchain.** `rustup toolchain install` with no argument installs the rust-toolchain.toml pin (1.98.1) on rustup 1.28 and later.
- **Reproducible build.** `cargo build --release --locked --bin ix-board`. All git dependencies (ix-relay, ix-cli-kit, ix-trace-rs) are public.
- **Asset naming and checksum.** `ix-board-${GITHUB_REF_NAME}-x86_64-unknown-linux-gnu.tar.gz` matches `ix-board-v0.7.2-aarch64-apple-darwin.tar.gz`. Both tarballs hold `ix-board` and `LICENSE` at the root. `sha256sum` run inside `dist` writes `<hash>  <basename>`, the same as the darwin `.sha256`.
- **Permissions.** Workflow-level `contents: write` only. That is the minimum for uploading release assets.
- **Existing manual release.** When the darwin release already exists for the tag, action-gh-release v2 finds it by tag, keeps its name, body, draft and prerelease values, and uploads the two files. That path works.
- **Action pins.** Major-version tags, matching ci.yml (`Swatinem/rust-cache@v2`) and ix-relay's release.yml (`actions/checkout@v5`).

## Findings

| ID | Severity | Summary | Refs |
| --- | --- | --- | --- |
| FND-001 | medium | The gnu binary is built on `ubuntu-latest` (24.04, glibc 2.39). terra's WSL, a rollout host in this ticket, has glibc 2.35. Bundled C code (libsqlite3-sys, ring) compiled against 2.39 headers can require symbols newer than 2.35 (for example `__isoc23_strtol@GLIBC_2.38`), and the binary then fails to start on terra. `ubuntu-latest` also moves the floor up without notice. Build on `ubuntu-22.04`, or check `objdump -T` of the artifact for symbols above GLIBC_2.35. | .github/workflows/release.yml:14 |
| FND-002 | medium | If the tag is pushed before the manual darwin release exists, action-gh-release creates the release itself. That release is titled with the bare tag (`v0.7.3`, not `ix-board 0.7.3`), has no notes, and is published as latest. The later manual `gh release create` then fails with "already exists", and the operator must switch to `gh release upload` and `gh release edit`. The header comment says both assets land on the same release, but nothing makes that order hold. Either document the order (create the release first, or use `gh release upload` afterwards) or set the release `name` in the action. | .github/workflows/release.yml:3-4,39-42 |

## Dispositions

| FND | Outcome | reason |
| --- | --- | --- |
| FND-001 | fixed | e9f3124: `linux-binary` now `runs-on: ubuntu-22.04` (glibc 2.35, terra WSL floor). actionlint exit 0. |
| FND-002 | accepted-no-change | e9f3124 sets `name: ix-board ${{ github.ref_name }}` and adds a README Releases section that documents the order (`gh release upload` when the release exists), so the failed-create path is resolved. The title is `ix-board v0.7.3` against the existing `ix-board 0.7.2` style, and the action also applies that name to a manually created release. The code lead accepted this cosmetic difference without change. |
