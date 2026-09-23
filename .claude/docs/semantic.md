# `[features.semantic]` — Jev tasks

Descriptive reference for the opt-in family that asks TypeSafe's Jev
classifier typed questions. Rules that constrain it live in
`scope.md` R5 / R6 and `design-philosophy.md` §2.

## Shape

```
heal semantic ask                     heal status / heal diff
  base_observation()                    build_record()
    run_all + classify                    run_all + classify
  for task in registry():                 semantic::lower::apply
    task.plan(ctx) ──┐                      for task in registry():
    skip cached keys │ identical plan        task.plan(ctx) ──┐
    pack → price     │ ⇒ identical keys      look up verdicts │
    dispatch (HTTP)  │                       task.lower(...)  │
    VerdictStore::save                     merge notes + new Findings
```

- `semantic::task::Task` — `id`, `criteria_text` (hashed into every
  key), `plan` (subjects → `Group { state, items[] }`), `lower`
  (cached answers → `Lowered { findings, notes }`), `on_demand`.
- `Item.meta` carries what `lower` needs (file, symbol, finding id,
  lines). It is never sent.
- Keys: `verdict_key(task, criteria_hash, model, subject, state)`.
  Changing a criterion, the pinned model, or the judged text re-asks.
- `lower` must skip items without an answer — that is the normal state
  for a teammate without a key or for code nobody asked about yet.
- New Findings inherit per-family `hotspot` / `hotspot_score` and
  `is_test_file` from existing Findings on the same file
  (`semantic::lower::merge`). Notes land in `Finding.semantic`.
- A task failing to plan is logged and skipped; it never fails
  `heal status`.

## Tasks

| id | Family | Question | Output |
|---|---|---|---|
| `commit_intent` (Q7) | core | `choice` fix / feature / refactor / test / docs / chore per commit in the churn window (≤1000 newest) | note `fix_ratio` on every Code Finding of a touched file: `p` = fix commits / answered commits |

## Q9 backtest (dev only)

`cargo run -p heal-cli --example backtest -- --project . --points 4
--step-days 90 --horizon-days 180 [--with-verdicts]`

Ranks files at past commits (self-calibrated, `observers::
observe_self_calibrated`) by the HEAL drain order, churn, LOC, and a
pseudo-random order; labels files by later bug-fix commits
(Conventional `fix` or fix / bug / hotfix / regression words; `(heal)`
scope excluded); prints precision@k and fixes captured at 20% LOC.
Labels never come from Jev, so a semantic axis is not graded by its
own verdicts.

Axes the backtest cannot grade: Q1 (consequence) and Q2 (effort) —
validate with maintainer-labelled samples and fix-commit sizes.

### Baseline on this repository (2026-09-23)

`--points 2 --step-days 20 --horizon-days 30`, before any semantic axis:

| order | p@10 | p@20 | fixes@20%LOC |
|---|---:|---:|---:|
| heal | 0.60 | 0.40 | 0.29 |
| churn | 0.60 | 0.40 | 0.43 |
| loc | 0.70 | 0.45 | 0.45 |
| random | 0.30 | 0.25 | 0.43 |

One point had fixes (99 files, 56 fix touches); the other had none and
is excluded. A single short window on a small repository — treat as
the harness working, not as evidence about the ordering.
