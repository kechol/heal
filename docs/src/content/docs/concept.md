---
title: Concept
description: Why heal exists, what problem it solves, and how it approaches your codebase.
---

This page explains the _why_. To start using heal directly, see
[Quick Start](/heal/quick-start/) and come back later.

## The problem

AI coding agents are great at the next change you ask for. But the
codebase keeps moving in the background: each fix or feature adds a
little complexity, the same files get touched over and over,
duplicated blocks slip in. The agent doesn't watch for that — and on
a real codebase, by the time _you_ notice that a file has become
hard to work with, the regressions are already shipping.

## The idea

> **Turn codebase health into agent triggers.**

Instead of asking the human to remember to run a linter, heal lets
the codebase emit signals on its own.

- **Every commit**, a post-commit hook re-runs every observer and
  prints any Critical / High item right inside the commit output —
  the next problem stays visible without a daemon.
- **On demand**, `heal status` lays out the same items as a
  Severity-grouped TODO list, which the bundled `/heal-code-patch`
  Claude skill works through one fix per commit.

The result is a loop where the codebase wakes the agent up, rather
than waiting for the human to do so.

## Codebase-relative Severity

A naïve threshold ("CCN ≥ 10 is high") works poorly across projects
— a 200-line script and a 200kloc service operate in different
worlds. heal calibrates each metric to **your codebase's own
distribution**: the top decile of _your_ complexity becomes High, the
top 5% becomes Critical. Recalibration is manual (`heal calibrate
--force`) — a refactor that genuinely improves the codebase shouldn't
silently move the goalposts.

Two literature-grade absolute floors bracket the percentile
classifier so a uniformly-bad codebase still surfaces its worst
cases, and a uniformly-clean codebase isn't held hostage by the "top
10% is always red" loop. See [Code › Metrics](/heal/code/metrics/)
for the full ladder.

## Hotspot — the file most likely to break next

**A Hotspot is a file that's both hard to read and frequently
edited.** heal ranks every file by `commits × complexity` (the
"code as a crime scene" idea from Adam Tornhill's *Your Code as a
Crime Scene*); the top 10% of that distribution gets the `🔥`
flag in `heal status`.

The intuition: a high-complexity file that nobody touches is debt
— interesting, but not urgent. A high-complexity file the team
edits every other day is where the next bug ships from. Hotspot is
the intersection: high score = often touched **and** hard to read
= where regressions historically concentrate.

That's why **Hotspot is the one signal to watch first** out of the
Code family. A `Critical 🔥` item isn't twice as bad as a plain
`Critical` — it's the one that actually pays back the time you
spend fixing it. The `/heal-code-patch` skill works through the
`🔥` queue first by default for the same reason.

The opt-in Test and Docs families ship their own family-specific
Hotspot composers — Test Hotspot (`commits × uncovered %`) and Doc
Hotspot (`paired-source churn × doc debt`) — so the same `🔥` flag
also points at the next file most worth testing or re-documenting.
Same idea, family-appropriate inputs.

## Two halves: heal surfaces the debt, the skills work it down

The design splits the work in two. The `heal` CLI is the
**measurement half** — it observes, calibrates, and surfaces the
code-debt signals worth acting on, but never edits a single source
file. The bundled Claude skills are the **repair half** — they read
what `heal` produced and turn it into commits.

A measurement tool that also "helpfully" applies fixes blurs the
line between *what is wrong* and *how this team chooses to address
it*. heal keeps those two questions in separate programs so each
side stays answerable on its own terms.

Inside the repair half, two skills further split _thinking_ from
_doing_:

- **`/heal-code-review`** is the _thinking_ skill. Read-only. It
  reads the TODO list as a system, deep-reads the flagged code, and
  proposes architectural moves — the calls a human still has to make.
  Use this when you want to _understand_ what HEAL is telling you
  before changing anything.
- **`/heal-code-patch`** is the _doing_ skill. Mechanical only — it
  works through the TODO list one fix per commit using established
  refactor patterns whose application doesn't require domain judgment.
  Refuses to start on a dirty worktree, never pushes, never amends.
  When the next item needs an architectural decision, it stops and
  hands back to `/heal-code-review`.

This split is the contract that lets you trust autonomy. The boring
fixes happen on their own; the interesting calls stay with you.

## Three feature families

heal observes three orthogonal slices of code health. Each family
follows the same loop — observe, surface what's worth fixing, hand
the list to a dedicated review ↔ patch skill pair — but each
answers a different question.

### Code — where is this codebase hard to change?

The always-on family. heal looks for the files that quietly cost
the team time: deeply branched functions, classes whose methods
have drifted apart, blocks copy-pasted across the tree, and the
hubs where every change ripples out. Then it ranks them by how
often the team is actually editing them, so the queue points at
today's friction, not yesterday's debt.

`/heal-code-review` and `/heal-code-patch` walk that queue with
classic refactoring moves — Extract Function, decompose
conditionals, pull a duplicate up into a shared helper. Mechanical
once you know which file to touch; tedious to chase down by hand.

### Test (opt-in) — where is production code dark to the test suite?

`[features.test]` reads the `lcov.info` your existing reporter
already produces and joins it back to the file-change history.
The result points at three things: production code that's been
edited recently but stays uncovered, tests that have stopped
tracking the source they cover, and tests that are silently
skipped — the kind nobody notices until the bug they were
guarding ships.

`/heal-test-review` and `/heal-test-patch` write the missing
tests one commit at a time and re-align the drifted ones. heal
never runs your tests itself; it just turns "we should have more
coverage here" into a ranked, file-specific TODO.

### Docs (opt-in) — where has documentation drifted from the code?

`[features.docs]` compares your paired documentation against the
source it describes — a small mapping file says "this doc
explains this file" — and surfaces the places where the doc has
fallen behind: paragraphs whose example identifiers no longer
exist, internal links that don't resolve, pages reachable from
nowhere, sections quietly accumulating TODO markers.

`/heal-doc-review` and `/heal-doc-patch` fix the mechanical
breakage automatically — broken links, dangling identifiers,
orphans — and frame the rest through the **Diátaxis** lens, so a
confused first-time reader gets attention before a mostly-stable
reference page.

For the full picture see [Features](/heal/features/).

## Further reading

- [Quick Start](/heal/quick-start/) — install and try it on a real
  repository
- [Features](/heal/features/) — the Code / Test / Docs family
  overview
- [CLI](/heal/cli/) — every subcommand
- [Code › Metrics](/heal/code/metrics/) — what each code metric
  measures and how Severity is assigned
- [Code › Configuration](/heal/code/configuration/) — `.heal/config.toml`
  reference for the always-on family
- [Code › Skills](/heal/code/skills/) — `/heal-code-review`,
  `/heal-code-patch`, `/heal-cli`, and `/heal-setup`
