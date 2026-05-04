---
name: heal-doc-review
description: Read every finding from the `[features.docs]` observer family produced by `heal status --feature docs --json`, deeply investigate the user's docs and codebase through a Diátaxis lens, and return one architectural reading plus a prioritized doc-fix TODO list. Read-only — proposes only. The write counterpart is `/heal-doc-patch`. Trigger on "review the docs health", "what does heal say about my docs", "where should we fix documentation", "/heal-doc-review".
---

# heal-doc-review

Read-only skill that interprets the `[features.docs]` findings (`doc_freshness`,
`doc_drift`, `doc_coverage`, `doc_link_health`, `orphan_pages`,
`todo_density`) and produces a prioritized TODO list grounded in
Diátaxis, the doc-quality literature, and the user's actual codebase.

This is the **proposal** side of doc maintenance. The mechanical
counterpart — broken-link fixes, dangling-identifier deletions, etc.
— lives in `/heal-doc-patch`. Don't apply changes here.

## Mental model

`heal status --feature docs --json` (with `[features.docs] enabled = true`) returns
findings with `Finding.metric` strings prefixed with `doc_`. Each
metric measures a different axis of doc decay; the right
remediation depends on which axis fired and why. The reading layer
maps findings to one of four Diátaxis purposes (Tutorial / How-to /
Reference / Explanation) and recommends a fix that respects the
doc's purpose.

## When this skill is right

- Right after enabling `[features.docs]` and running
  `/heal-doc-pair-setup`: the first set of doc findings deserves an
  interpretation pass before mechanical fixes.
- After a major refactor: docs lag code, drift findings spike, and
  the user wants triage.
- The user wants a TODO list to hand to a writer / next agent loop.

## References (load on demand)

- `references/architecture.md` — Diátaxis primer, the four
  doc-quality traps to avoid (Coverage trap, autogen-only trap,
  link perfectionism, doc bloat), and per-metric reading rules.
- `references/metrics.md` — what each `[features.docs]` metric
  measures, how its severity is computed, and what a fix should
  preserve.

## Pre-flight

1. **Findings exist.** Run `heal status --feature docs --json`. The
   command exits 1 with a stderr message when
   `[features.docs].enabled = false` — bail and tell the user to
   enable the family (via `/heal-setup` or hand-edit
   `.heal/config.toml`) before retrying. On success the payload (or
   the cached `latest.json`) must contain at least one finding with
   a `doc_*` metric. If `doc_pairs.json` is missing, recommend
   `/heal-doc-pair-setup` and stop — there's nothing to review.
2. **Read the SSoT.** Open `.heal/doc_pairs.json`. The pair list
   tells you which docs claim to describe which srcs; the metric
   findings reference these via `Finding.location.file` and
   `Finding.locations[].file`.
3. **Capture the docs config.** Open `.heal/config.toml` and read
   `[features.docs]`. The thresholds (`doc_freshness.high_commits`
   / `critical_commits`) and `standalone.include` / `exclude` set
   the lens through which you should read severity.

## Procedure (Read → Investigate → Propose)

### Phase 1 — Read

For each `doc_*` finding:

1. Note `metric`, `severity`, `hotspot`, primary location, and
   secondary locations. Hotspot decoration matters — a stale doc
   on a hotspot file reads readers to wrong conclusions on the
   highest-leverage code.
2. Group by file: usually one doc has several findings (drift
   identifiers + freshness + link breaks). Walking the doc once
   beats walking it once per finding.
3. Open the doc + every paired src in the SSoT. The proposal needs
   to reflect what the doc *should* say after a fix, not just that
   something is broken.

### Phase 2 — Investigate (Diátaxis lens)

Classify each doc into one of four Diátaxis purposes (see
`references/architecture.md` §1):

- **Tutorial** — teaches a beginner by guiding them through a
  task. Strict requirements: every step works, the task completes.
  Drift / dangling identifiers here block learners cold.
- **How-to** — answers a specific user question. Drift-tolerant if
  the recipe still works; identifier drift in steps is critical.
- **Reference** — exhaustive description of an API surface or
  configuration. Drift here is **load-bearing** — readers come for
  truth, not narrative. `doc_drift` and `doc_freshness` matter
  most.
- **Explanation** — discusses *why*, not *what*. Identifier
  freshness matters less; the cost of churning these on every
  small refactor is high.

Misclassification inflates noise: applying Reference rigor to an
Explanation doc surfaces drift that's actually fine. Apply rigor
proportional to the doc's purpose.

### Phase 3 — Propose

Build a prioritized TODO list. The order matters — drain the
high-value, low-effort items first so the cache empties faster
under `/heal-doc-patch`:

1. **Mechanical wins (allow-list).** Findings whose fix is
   obviously deterministic — broken internal links, dangling
   identifiers in fenced code blocks (deleting them as obsolete
   examples), TODO marker resolutions where the answer is now in
   the code. Hand these to `/heal-doc-patch`.
2. **Interpretive fixes.** Findings whose fix needs judgment —
   re-explaining a concept after a refactor, deciding whether a
   stale doc should be deleted or rewritten, splitting a doc into
   Tutorial + Reference per Diátaxis. The user (or another agent
   loop) drives these.
3. **Architectural changes.** Recurring drift on the same doc
   signals the underlying code or doc structure has shifted in a
   way that no amount of patching will fix. Surface as a separate
   architectural recommendation: SSoT consolidation, transclusion
   (shared include), splitting a doc into Reference + Explanation,
   or retirement.

Avoid the four traps (`references/architecture.md` §4):

- **Coverage trap.** Don't recommend writing empty docstrings to
  raise `doc_coverage` numbers. Coverage 100% with empty bodies
  is worse than 80% with real explanations.
- **Autogen trap.** API reference produced by tooling is necessary
  but not sufficient. Recommend explanation alongside it, not in
  place of it.
- **Link perfectionism.** Every external link rots. Don't propose
  rewriting docs to remove all external links — propose archive
  URLs (Web Archive) for high-value references.
- **Doc bloat.** Always pair "write more" recommendations with
  "delete some" — the deletion-side metrics (`orphan_pages`,
  `duplication`) exist for this.

## Output format

End with three blocks:

```
Architectural reading:
  - docs/cli.md serves Reference; drift + freshness both fired
    (28 src commits since doc, 2 dangling identifiers). Propose:
    extract the auto-derivable parts (flag list, exit codes) into
    a generated section; keep the Explanation in the hand-written
    body.
  - docs/concept.md serves Explanation; only doc_freshness fired
    (12 commits). Skip-able for now — explanation drift bites
    less than reference drift.
  - 3 docs orphaned in docs/legacy/; recommend deletion or move
    to docs/archive/ (excluded from standalone walk).

Prioritized TODO:
  T1 Mechanical (hand to /heal-doc-patch):
    - docs/cli.md:42 broken link to ./old-flag.md
    - docs/api.md:18 dangling identifier `OldStruct`
    - docs/install.md FIXME: pin Rust version (use 1.85, see CI)
  T2 Interpretive (user drives):
    - docs/cli.md: rewrite Step 3 after observer rename
    - docs/concept.md: clarify what 'workspace' means after monorepo support
  T3 Architectural:
    - Three observer pages duplicate the calibration table —
      extract into a single Reference page and transclude.
    - docs/legacy/* (12 files) → recommend archive or deletion.

Counts:
  total findings:    37
  paired (Layer A):  18
  standalone (B):    19
  by metric:         drift=8 freshness=6 coverage=5 link=11 orphan=4 todo=3
```

## Constraints

- **Read-only.** Never edit a doc, src file, or `.heal/*` file
  from this skill.
- **Diátaxis-aware.** Don't recommend Reference rigor on
  Explanation docs (or vice versa). The lens matters.
- **Avoid the traps.** Coverage / autogen / link perfectionism /
  doc bloat are spelled out in `references/architecture.md` §4 —
  consult that section before writing any "write more docs"
  recommendation.
- **Don't moralize.** A doc that's been stale 30 commits is a
  signal, not a verdict. The user might have intentionally
  frozen it.
- **English output.** Skill writes English; underlying docs may
  be in any language. When suggesting a rewrite of a non-English
  doc, recommend the user (or a translator) handle it — don't
  auto-translate.
