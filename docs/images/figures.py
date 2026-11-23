# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Agent-IX
"""ix-board's documentation figures, rendered by the docs-figures skill.

Regenerate from the repository root with the agent-ix/dev-tools skill:

    python3 -m pip install fonttools
    python3 <dev-tools>/skills/docs-figures/scripts/render.py docs/images/figures.py

Add --check to confirm the committed SVGs are current. Only the overview is defined
here: it shows the cache lifetime in minutes, derived from `CACHE_TTL_SECONDS` in
seconds, which docs/images/figures.json cannot yet express (no arithmetic in value
bindings). The ready-queue and focus figures are defined in docs/images/figures.json
and rendered by `make figures`.
"""

import re
from pathlib import Path

from docs_figures import FONTS, SMALL, TITLE, Figure  # noqa: F401

STYLE = "pencil"

ROOT = Path(__file__).resolve().parents[2]

# One role per kind of thing, fixed across the figures in this repository.
ROLES = [
    ("code", "external source", "box"),
    ("model", "local cache", "box"),
    ("logic", "ix-board command", "box"),
    ("evidence", "output", "box"),
    ("ink", "input / agent plugin", "box"),
]


# ---- repository data ---------------------------------------------------------


def _source(path):
    return (ROOT / path).read_text(encoding="utf-8")


def cache_minutes():
    """The snapshot lifetime in minutes, from `CACHE_TTL_SECONDS`."""
    seconds = int(re.search(r"CACHE_TTL_SECONDS: i64 = (\d+);", _source("src/cache.rs")).group(1))
    return seconds // 60


# ---- figures -----------------------------------------------------------------


def overview():
    """README hero: sources, the cached snapshot, the command groups and the outputs."""
    minutes = cache_minutes()
    f = Figure(
        "overview",
        960,
        384,
        "ix-board reads Linear into a SQLite snapshot that expires after "
        f"{minutes} minutes. Four command groups read that snapshot: ready queue, focus, "
        "program review, and agents and drift. GitHub pull requests and Herdr agent panes are "
        "read directly by the commands that need them. Every command prints text or JSON, and "
        "the Claude Code and Codex plugin skills read the JSON.",
    )
    fx, fy, fw, fh = 220, 0, 560, 340
    f.rect(fx, fy, fw, fh, "faint", dashed=True, fill=False)
    f.text(fx + 20, fy + 24, "ix-board", "muted", "title", TITLE)

    cy = fy + fh // 2 + 8
    f.box(0, cy - 34, 140, 68, "code", "Linear", "projects", "issues")
    f.box(0, 40, 140, 56, "code", "GitHub", "pull requests")
    f.box(0, 270, 140, 56, "code", "Herdr", "agent panes")
    f.line([(140, cy), (fx + 17, cy)], "code")
    f.note(146, cy - 12, "GraphQL", "code")
    f.line([(140, 68), (fx - 3, 68)], "code", dashed=True)
    f.line([(140, 298), (fx - 3, 298)], "code", dashed=True)

    f.box(fx + 20, cy - 34, 140, 68, "model", "SQLite snapshot", f"expires after {minutes} min")
    groups = [
        ("Ready queue", "next · project-chunk", "search · issues"),
        ("Focus", "expand · status · report", "hygiene · plan-chunk"),
        ("Program review", "scope check · roadmap · orphans", "chain · reconcile"),
        ("Agents and drift", "agents · drift"),
    ]
    gx, gw, gh = fx + 220, 320, 62
    ys = [36 + i * 76 for i in range(4)]
    bus = fx + 190
    f.line([(fx + 160, cy), (bus, cy)], "model", arrow=False)
    f.line([(bus, ys[0] + gh // 2), (bus, ys[-1] + gh // 2)], "model", arrow=False)
    for (title, *subs), y in zip(groups, ys):
        h = gh if len(subs) > 1 else 56
        f.box(gx, y, gw, h, "logic", title, *subs)
        f.line([(bus, y + h // 2), (gx - 3, y + h // 2)], "model")

    ox, ow = 830, 130
    f.line([(fx + fw, cy), (ox - 3, cy)], "logic", weight=2.4)
    f.box(ox, cy - 34, ow, 68, "evidence", "output", "text or --json")
    f.box(ox, 270, ow, 56, "ink", "plugin skills", "Claude · Codex")
    f.line([(ox + ow // 2, cy + 34), (ox + ow // 2, 267)], "evidence")
    f.note(ox + ow // 2 + 8, 232, "--json", "evidence")
    f.legend(0, 372, ROLES)
    return f


FIGURES = [overview]
