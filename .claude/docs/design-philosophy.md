# Design philosophy

Why HEAL is shaped the way it is. The other docs in this tree describe
**what** HEAL is today; this one captures the **why** behind the
load-bearing decisions, so future changes don't quietly undo them.

If you're tempted to add a feature that contradicts one of these
tenets, that's the signal to discuss it in an issue first — not to
write the PR.

---

## 1. Three threshold principles

Every metric in HEAL is shaped by three principles. They look
obvious in hindsight; they constrain a lot of design decisions in
practice.

### 1.1 Relative beats absolute

A fixed threshold ("CCN > 10 is a problem") fails two ways:

1. **Alert fatigue.** Legacy files trip the alarm on every run.
   Users learn to ignore the alarm, which removes its purpose.
2. **Domain insensitivity.** A tax engine, medical-decision rule
   set, or parser combinator is intrinsically branchy. Flagging
   every function in those modules tells the user nothing.

HEAL's response: **Calibration**. Each codebase's own distribution
defines its baseline (`p50 / p75 / p90 / p95`). Industry-standard
absolute floors stay as a hard ceiling (`floor_critical = 25` for
CCN, `30` for Duplication%, `50` for Cognitive — sourced from McCabe
and SonarQube literature). The classification is `floor_critical OR
percentile`, not one or the other. See `observer/calibration.rs` and
`.claude/docs/observers.md`.

The flip side: HEAL does **not** auto-recalibrate (`scope.md` R3).
Drift is a function of the user's intent ("I just refactored half
the codebase, recalibrate now") not the tool's bookkeeping. Users
opt in via `heal calibrate --force`.

### 1.2 Worst-N beats average

Aggregate scores ("average maintainability index = 72") hide the
files that hurt. A composite that says "your repo is 72/100" can't
be acted on; "these three files account for most of the friction"
can.

HEAL's response: **per-Finding output**, never a global score.
`heal status` lists Findings; `heal metrics` lists per-file
scores; nothing in HEAL emits a single "code health is N/100"
number. A maintainability index would be a regression
(`prior-art.md` §5).

The drain target follows the same principle: not "reduce average
CCN" but **"resolve Critical AND `hotspot=true` findings, in
order"** (`scope.md` R1). One concrete file at a time.

### 1.3 Composite beats single (the Hotspot axis)

A file with high CCN and zero churn is not a problem worth fixing
this quarter. A file with high churn and trivial CCN is also fine.
A file with **both** is where bugs and friction land.

HEAL's response: **Hotspot is a multiplicative composite**
(`(weight_complexity * ccn_sum) * (weight_churn * commits)`) and
**every Severity finding carries a `hotspot` decoration**
(true / false). The two attributes are **orthogonal** — never blur
them:

|             | Hotspot low      | Hotspot high              |
|-------------|------------------|---------------------------|
| Critical    | sleeping debt    | **burning debt** (drain)  |
| Ok          | healthy          | why are we touching this? |

Mixing the axes (e.g. "promote High to Critical because it's a
hotspot") would collapse the table back into one number, which
defeats the point. See `terminology.md` R6 and
`crates/cli/src/observer/hotspot.rs`.

---

## 2. The four-layer model and HEAL's "no Executor" choice

A hook-driven code-health system has four conceptual layers:

| Layer       | Job                                                |
|-------------|----------------------------------------------------|
| Observer    | Compute raw metrics from source + git              |
| Aggregator  | Persist + classify (Severity, Hotspot decoration)  |
| Trigger     | Surface findings to the user (nudge / cache write) |
| Executor    | Act on the findings (refactor, commit, open PR)    |

**HEAL CLI implements Observer, Aggregator, and Trigger. It does
not implement Executor.** The CLI never calls an LLM, never opens
network connections (other than `git2` against the local repo),
never spawns `claude` / `codex` / `gh`.

Why the split:

- **API quota is the user's, not the tool's.** A CLI that silently
  burns the user's Anthropic quota on a post-commit hook is hostile.
  The user must be the one who decides "yes, spend tokens on this".
- **Local-only by default.** No telemetry, no version pings, no
  background uploads (`CLAUDE.md` "No telemetry, no network calls"
  + `scope.md` R5). HEAL works offline.
- **Determinism.** Same commit + config + calibration → byte-
  identical `latest.json` across teammates (`scope.md` R2,
  `invariants.md` R6, `R4`). LLM calls inside the pipeline would
  destroy this.

The Executor lives in user-invoked Claude skills:
`/heal-code-review` (read, propose architecture) and
`/heal-code-patch` (write, one commit per finding). Both are
explicitly user-triggered. See `skills-and-hooks.md` R7 and
`scope.md` R8.

This split is the reason the bundled skills are simple:
`heal-code-patch` doesn't have to handle "is HEAL allowed to spend
my quota?" — the user already answered yes by typing the command.

---

## 3. Why HEAL has no cool-down

A common pattern in monitoring tools is suppression: "don't fire
the same alert for 24 hours". HEAL deliberately does not have
this.

Reasoning:

- **Every commit is a real change.** If a Critical finding shows
  up in commit A and is still there in commit B, the user touched
  the codebase between them — repeating the nudge isn't
  duplicate-spam, it's a status report on what changed and what
  didn't. Suppression would hide the answer to "is my last commit
  better or worse?".
- **Cool-down state is durable surface area.** v0.1 had
  `.heal/state.json` with `last_fired` cool-down keys. It bred
  bugs (stale entries, machine-clock skew, "why isn't it
  firing?"). v0.2 retired it. The lesson: state that exists only
  to suppress signal is rarely worth its complexity.
- **Determinism wins.** A teammate fetching the same commit must
  see the same nudge output. Per-machine cool-down state breaks
  that. See `scope.md` R2 and `invariants.md` R4.

If you're tempted to add suppression "to reduce noise", first ask
whether the noise is actually a calibration problem
(`heal-config` skill should warn about flood) or a
classifier-demotion problem (`scope.md` R9 PairClass).

---

## 4. Single source of truth: the findings cache

`.heal/findings/` is a **single record**, not a history (`scope.md`
R4):

- `latest.json` — one `FindingsRecord`.
- `fixed.json` — `BTreeMap<finding_id, FixedFinding>`.
- `regressed.jsonl` — append-only audit trail.

That's the whole layout. No `snapshots/`, no `YYYY-MM.jsonl`, no
archive directory. Drift over time is served by `heal diff <ref>`,
which recomputes against the named git ref on demand — not by
reading a stored snapshot.

Why this shape:

- **Per-team determinism.** All three files are git-tracked. Same
  commit + config + calibration → byte-identical files. A teammate
  running `heal status` after `git pull` sees what you saw — no
  "works on my machine".
- **Bounded surface.** Recovering from corruption is `rm
  .heal/findings/* && heal status`. Snapshot trees grow, rotate,
  decay, and become a maintenance category of their own.
- **Drift is a query, not a state.** `heal diff main` answers
  "what changed since main?" in one command. A persistent
  delta-vs-previous-run field would have to define "previous when?"
  and would be wrong half the time.

The price is that HEAL can't answer "what was Critical six months
ago?" without `git checkout <old-sha>; heal metrics`. That's
intentional — questions that need history go through git, the
durable record we already trust.

---

## 5. Heuristics we follow

Three heuristics that show up in design reviews more than once.

### 5.1 If you can't name the friction, the metric is decoration

A new metric proposal answers: **what user pain does this
predict?** Hard-to-test? Hard-to-read? Hard-to-change?

A metric that doesn't tie to a friction is a number on a
dashboard, and dashboards no one reads are worse than no
dashboards at all. See `prior-art.md` "When adding a new
observer" for the full bar.

### 5.2 Context-rich proposals beat terse findings

`heal-code-review` and `heal-code-patch` always include **why**
this finding matters in this file: the hotspot score, the
change-coupling neighbourhood, the test-coverage shape. A finding
that says only "CCN = 23" gets refactored into something that
relocates the complexity instead of removing it (the
"relocate-trap" — see
`crates/cli/plugins/heal/skills/heal-code-review/references/architecture.md`
§6).

The cost is verbosity in skill output. The benefit is that the
proposed change actually drains the queue instead of bouncing the
finding to a neighbouring file.

### 5.3 Refuse on dirty worktree

`heal-code-patch` refuses to start if `git status` is dirty. The
audit trail (one finding per commit, `regressed.jsonl` recording
every fix-then-regress) only works if HEAL is the only thing
writing during a drain session. See `skills-and-hooks.md` R7.

The pattern generalises: any flow that builds a per-commit audit
trail must own the worktree for the duration. Don't add a "best
effort" mode that tolerates dirt — the audit trail's value
collapses the moment it's untrustworthy.

---

## 6. What this philosophy rules out

If a future PR proposes any of the following, it should explicitly
overturn the relevant section above before being accepted:

- **Single composite "code health" score.** §1.2.
- **Auto-recalibration.** §1.1, `scope.md` R3.
- **LLM calls inside `heal` CLI.** §2.
- **Cool-down / suppression state.** §3.
- **Persistent metrics history (`snapshots/`, rolling deltas).** §4.
- **Network access beyond `git2` on the local repo.** §2,
  `scope.md` R5.
- **Mixing Severity and Hotspot into one score.** §1.3,
  `terminology.md` R6.
- **Skills that act without explicit user invocation.** §2.

These are not arbitrary. Each one was tried, evaluated, and
rejected during v0.1 → v0.3. Re-relitigating the same trade-off
without new information is wasted PR cycles.
