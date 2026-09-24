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
- **On demand**, `heal status` lays out the same items in effective
  Tier, Severity, then family-local score order, which the
  `/heal:refactor` Claude skill works through — proposing changes and
  committing the ones you approve, one at a time.

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

## Hotspot — where leverage concentrates

**A Hotspot is a file that's both hard to read and frequently
edited.** heal ranks every file by `commits × complexity` (the
"code as a crime scene" idea from Adam Tornhill's _Your Code as a
Crime Scene_). With 5+ finite candidates, the top decile that also
clears the family floor gets `🔥`; with only 1–4, the existing
absolute family floor is used alone. Non-finite values never flag.

The intuition: a high-complexity file that nobody touches is debt
— interesting, but not urgent. A high-complexity file the team edits
every other day deserves earlier scrutiny. Hotspot is the intersection:
high score = often touched **and** hard to read.

Hotspot is therefore a useful leverage signal, but `🔥` is not an
extra sort key. HEAL first applies the effective Drain Tier and
Severity, then orders the remaining same-family bucket by descending
`hotspot_score`. A plain finding with a higher score can precede a
flagged finding in that bucket.

The opt-in Test and Docs families ship their own family-specific
Hotspot composers — Test Hotspot (`commits × uncovered %`) and Doc
Hotspot (`paired-source churn × doc debt`) — so the same `🔥` flag
also points at the next file most worth testing or re-documenting.
Same idea, family-appropriate inputs.

HEAL uses these scores only to order work after drain Tier and Severity,
and only within the same family. They are heuristics, not defect
probabilities, cross-family units, or guarantees about the effect of a
fix.

## Two halves: heal surfaces the debt, the skills work it down

The design splits the work in two. The `heal` CLI is the
**measurement half** — it observes, calibrates, and surfaces the
code-debt signals worth acting on, but never edits a single source
file. The Claude skills in the heal plugin are the **repair half** —
they read what `heal` produced and turn it into commits.

A measurement tool that also "helpfully" applies fixes blurs the
line between _what is wrong_ and _how this team chooses to address
it_. heal keeps those two questions in separate programs so each
side stays answerable on its own terms.

Inside the repair half, each skill separates _thinking_ from _doing_
with one step in between — your approval:

- **Think.** `/heal:refactor` reads the TODO list as a system,
  deep-reads the flagged code, and proposes changes — from a renamed
  variable to splitting a file along the concepts it mixes, when the
  evidence for that split is strong. Each proposal says what friction
  it removes and what it touches.
- **Approve.** You pick which proposals to apply. Calls only you can
  make — a public name, which module owns a responsibility — come as
  questions with concrete options.
- **Do.** The skill applies what you picked, one commit per proposal,
  with your tests run before every commit. It refuses to start on a
  dirty worktree and never pushes or amends.

This checkpoint is the contract that lets you trust autonomy: nothing
changes before you say so, and everything you approved lands as a
reviewable commit. `/heal:refactor plan` stops after the proposals
when you only want to understand what heal is telling you.

## Three feature families

heal observes three orthogonal slices of code health. Each family
follows the same loop — observe, surface what's worth fixing, hand
the list to a dedicated skill — but each answers a different
question.

### Code — where is this codebase hard to change?

The always-on family. heal looks for the files that quietly cost
the team time: deeply branched functions, classes whose methods
have drifted apart, blocks copy-pasted across the tree, and the
hubs where every change ripples out. Then it ranks them by how
often the team is actually editing them, so the queue points at
today's friction, not yesterday's debt.

`/heal:refactor` walks that queue with classic refactoring moves —
Extract Function, decompose conditionals, pull a duplicate up into a
shared helper — and, when several signals agree, structural ones like
splitting a file or moving a function to where it belongs. Obvious once
you know which file to touch; tedious to chase down by hand.

### Test (opt-in) — where is production code dark to the test suite?

`[features.test]` reads the `lcov.info` your existing reporter
already produces and joins it back to the file-change history.
The result points at three things: production code that's been
edited recently but stays uncovered, tests that have stopped
tracking the source they cover, and tests that are silently
skipped — the kind nobody notices until the bug they were
guarding ships.

`/heal:tests` writes the missing tests one commit at a time and
re-aligns the drifted ones. heal
never runs your tests itself; it just turns "we should have more
coverage here" into a ranked, file-specific TODO.

### Docs (opt-in) — where has documentation drifted from the code?

`[features.docs]` compares your paired documentation against the
source it describes — a small mapping file says "this doc
explains this file" — and surfaces the places where the doc has
fallen behind: paragraphs whose example identifiers no longer
exist, internal links that don't resolve, pages reachable from
nowhere, sections quietly accumulating TODO markers.

`/heal:docs` fixes the mechanical breakage — broken links, dangling
identifiers, orphans — and frames the rest through the **Diátaxis**
lens, so a
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
- [Code › Skills](/heal/code/skills/) — the heal plugin,
  `/heal:setup`, and `/heal:refactor`
