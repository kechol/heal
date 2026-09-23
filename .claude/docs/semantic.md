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
| `concept` (C12) | code | `choice` over `.heal/concepts.toml` (+ `other`) per outer function ≥3 lines; state = numbered file, or ±40-line windows when the file exceeds the state budget | `concept_mix` (≥2 concepts ≥25% of a file's classified LOC, file ≥60 LOC; High at ≥3), `concept_misplaced` (function's concept ≠ file home, another file's home = that concept; Medium, `fix_hint` = move target), `concept_scatter` (concept in ≥5 files, none ≥40%; Medium, `locations` = other files) |
| `term_drift` (C13) | code | depends on `concept`. Per concept, frequent non-verb words of function names (≥2 names, top 8) that never co-occur → `noul` "same thing?" | `term_drift` (p ≥ 0.7; location = first file of the minority word, `locations` = its other files) |
| `name_mismatch` (C13) | code | functions in files with a hotspot Code finding, plus functions with a ≥Medium CCN / Cognitive finding → 4-level `score` (n/a · matches · arguable · contradicts / hides an effect) | `name_mismatch` (level 3, confidence ≥ 0.5; Medium) |
| `name_choice` (C13, on demand) | code | `--focus` JSON `{"candidates":[{file, symbol, names[]}]}` → `choice` current vs candidates | `report`: `choices[] {best, p, p_current, rename}`; `rename` iff best ≠ current, `p − p_current ≥ 0.2`, confidence ≥ 0.5 |
| `split_points` (C14) | code | High / Critical CCN / Cognitive functions; body statements merged into ≥3-line segments (≤30) → `noul` per adjacent pair "same step?" | note `split_points` on those findings: `lines` = boundaries with p ≤ 0.3, `detail` = step ranges |
| `fix_pattern` (H2) | code | T0 / T1 CCN / Cognitive / duplication findings → `choice` over the patch allow-list + `none` | note `fix_pattern` |
| `verify_patch` (V1 + V3, on demand) | code | `--diff <range>` → 4 `noul`: relocate, guard_clause, behaviour_change, message_mismatch; whole diff as one state, or one per file when too large | `report`: `checks` (max p per check), `flags` (≥ 0.7, excluding behaviour_change), `pass` |
| `verify_proposal` (V4, on demand) | code | `--focus` JSON `{"proposals":[{id, text, files[]}]}` → the five readability questions, phrased so true = good | `report`: per proposal `checks`, `pass` (all ≥ 0.5) |

Tasks run in registry order, and `heal semantic ask` sends one task's
batches before planning the next, so a task with `depends_on` reads
the answers of the same run through `TaskContext::prior_answer`.
On-demand tasks (`on_demand() == true`) only run with `--task <id>`,
never during `heal status`; their `report` is `tasks[].result` in
`--json`.

Semantic Findings never exceed `High` (`tasks::common::finding`): a
classifier is wrong often enough that a verdict alone must not put a
Finding in the Critical-driven T0 tier.

`.heal/concepts.toml` (`core::concepts`) — `[[concept]] { id,
description }`, ids lowercase `[a-z0-9_-]`, 1–254 entries, `other`
appended automatically. Written by `/heal-concepts-setup`; an
observation input of `config_hash` while the family is enabled.

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
