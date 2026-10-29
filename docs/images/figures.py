# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Agent-IX
"""ix-board's documentation figures, rendered by the docs-figures skill.

Regenerate from the repository root with the agent-ix/dev-tools skill:

    python3 -m pip install fonttools
    python3 <dev-tools>/skills/docs-figures/scripts/render.py docs/images/figures.py

Add --check to confirm the committed SVGs are current. The cache lifetime, the
ready-queue ordering keys, the spec-state mapping and the project run state are
read from the Rust sources in this repository.
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


def ordering_keys():
    """The ready-queue sort keys in order, from the reason string in `ready_queue_for`."""
    text = _source("src/lib.rs")
    tail = re.search(r"ordered by ([^\"]+)\"", text).group(1)
    keys = [part.replace(" {}", "").strip() for part in tail.split(", ")]
    return keys + ["identifier"]


def next_steps():
    """Spec state to next step, from `spec_step`."""
    body = _source("src/lib.rs").split("fn spec_step", 1)[1].split("\n}\n", 1)[0]
    steps = {}
    for line in body.splitlines():
        step = re.search(r"=> Some\(NextStep::(\w+)\)", line)
        if step:
            for state in re.findall(r'"(\w+)"', line):
                steps[state] = step.group(1)
    return steps


def running_status():
    """The project state that lets tickets into the queue, from `ready_queue_for`."""
    return re.search(r'project\.status != "(\w+)"', _source("src/lib.rs")).group(1)


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


def ready_queue():
    """A small blocker graph: which tickets are ready, which are not, and the queue order."""
    keys = ordering_keys()
    steps = next_steps()
    running = running_status()
    f = Figure(
        "ready-queue",
        820,
        408,
        "A ticket is ready when its project is Started, its spec state is Backlog, Specify or "
        "Specced, and every ticket in its chain of blockers is Completed. A blocker that is "
        "still in progress or Canceled keeps a ticket out. Ready tickets are ordered by "
        + ", ".join(keys[:-1])
        + f", then {keys[-1]}.",
    )
    bx, tx, qx = 0, 230, 560
    bw, tw, qw = 150, 240, 250
    row = 70

    def y_of(i):
        return 26 + i * row

    f.text(bx, 12, "BLOCKER", "muted", "mono", SMALL)
    f.text(tx, 12, "TICKET", "muted", "mono", SMALL)
    f.text(qx, 12, "READY QUEUE", "muted", "mono", SMALL)

    # (blocker, ticket, spec state, outcome, sub-line of the ticket)
    rows = [
        (None, "ENG-8", "Specced", "ready", f"Specced · next step {steps['Specced']}"),
        (("ENG-1", "Completed"), "ENG-2", "Backlog", "ready", f"Backlog · next step {steps['Backlog']}"),
        (("ENG-3", "In Progress"), "ENG-4", "Specced", "blocked", "Specced · blocker open"),
        (("ENG-5", "Canceled"), "ENG-6", "Specced", "blocked", "Specced · blocker Canceled"),
        (None, "ENG-9", "Specced", "excluded", f"project not {running}"),
    ]
    for i, entry in enumerate(rows):
        blocker, ticket, _state, outcome, sub = entry
        y = y_of(i)
        role = {"ready": "logic", "blocked": "evidence", "excluded": "muted"}[outcome]
        if blocker:
            f.box(bx, y, bw, 50, "ink", blocker[0], blocker[1])
            f.line([(bx + bw, y + 25), (tx - 3, y + 25)], "ink")
            f.note(bx + bw + 14, y + 17, "blocks", "ink")
        else:
            f.text(bx, y + 30, "no blockers", "muted", "mono", SMALL)
        f.box(tx, y, tw, 50, role, ticket, sub, dashed=(outcome == "excluded"))

    # The queue: ready tickets in order, level with their rows.
    ready_rows = [(0, "ENG-8", "Urgent"), (1, "ENG-2", "High")]
    for n, (i, ticket, prio) in enumerate(ready_rows, start=1):
        y = y_of(i)
        f.line([(tx + tw, y + 25), (qx - 3, y + 25)], "logic", weight=2.4)
        f.box(qx, y, qw, 50, "logic", f"{n}.  {ticket}", f"ticket priority {prio}")

    # Ordering keys, read from the ready-queue sort.
    top = y_of(2) + 6
    f.text(qx, top, "ORDERED BY", "muted", "mono", SMALL)
    for n, key in enumerate(keys, start=1):
        f.text(qx, top + 24 + (n - 1) * 24, f"{n}.  {key}", "ink", "mono", SMALL + 1)
    f.legend(0, 392, [
        ("logic", "ready", "box"),
        ("evidence", "blocked", "box"),
        ("muted", "excluded", "dash"),
        ("ink", "blocker", "box"),
    ])
    return f


def focus():
    """Selectors resolve to one focus issue set that feeds four reports."""
    f = Figure(
        "focus",
        860,
        262,
        "A focus is a union of initiative, project, milestone and issue selectors. ix-board "
        "resolves it to a deduplicated set of issues with their blocker layers. The status, "
        "hygiene, plan-chunk and report commands read that set.",
    )
    sel = [
        ("--initiative", "UUID, I-10 or name"),
        ("--project", "UUID, identifier or name"),
        ("--milestone", "name or project/name"),
        ("--issue", "identifier, with descendants"),
    ]
    ys = [i * 66 for i in range(4)]
    cy = (ys[0] + ys[-1] + 52) // 2
    for (name, sub), y in zip(sel, ys):
        f.box(0, y, 230, 52, "ink", name, sub)
    bus = 262
    f.line([(bus, ys[0] + 26), (bus, ys[-1] + 26)], "ink", arrow=False)
    for y in ys:
        f.line([(230, y + 26), (bus, y + 26)], "ink", arrow=False)
    f.line([(bus, cy), (297, cy)], "ink")
    f.box(300, cy - 39, 220, 78, "logic", "focus issues", "deduplicated union", "blocker layers")
    f.text(300, cy + 66, "--work-milestone narrows", "muted", "mono", SMALL)
    f.text(300, cy + 86, "ready work and handoff roots", "muted", "mono", SMALL)

    reports = [
        ("status", "progress · blockers · ready"),
        ("hygiene", "findings · diff"),
        ("plan-chunk", "ordered handoff"),
        ("report", "versioned JSON record"),
    ]
    rbus = 552
    f.line([(520, cy), (rbus, cy)], "logic", arrow=False)
    f.line([(rbus, ys[0] + 26), (rbus, ys[-1] + 26)], "logic", arrow=False)
    for (name, sub), y in zip(reports, ys):
        f.line([(rbus, y + 26), (597, y + 26)], "logic")
        f.box(600, y, 260, 52, "evidence", name, sub)
    return f


FIGURES = [overview, ready_queue, focus]
