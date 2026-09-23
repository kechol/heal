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

## Live API behaviour (measured 2026-09-23, `jev-1.13.0`)

- `POST /v1/systemone` answers `{model, answers, usage}` exactly as
  `semantic::api::Response` expects; `model` echoes the pinned id.
- `score` is the probability-weighted mean of the 0-based levels
  (fractional, e.g. 2.38), not an index. Thresholds compare it as a
  real number. The extra `legend` field is dropped.
- `GET /v1/models` returns `{"models":[{"name","description",
  "release_date"}]}` and lists only `jev-latest` / `jev-preview`.
  Pinned ids are usable but unlisted, so the listing can confirm a
  model, never rule one out.
- An unknown model is HTTP 400 `api_usage_error` / `Unknown model:
  <id>` → `JevErrorKind::Setup`, which stops the run (exit 2).
  An empty `questions` map is HTTP 422, so there is no free probe.
- Every code-family task on this repository (505 requests, 2.24M
  input tokens) cost $0.094 and took 31 s at `concurrency = 8`.

## Tasks

| id | Family | Question | Output |
|---|---|---|---|
| `commit_intent` (Q7) | core | `choice` fix / feature / refactor / test / docs / chore per commit in the churn window (≤1000 newest) | note `fix_ratio` on every Code Finding of a touched file: `p` = fix commits / answered commits |
| `consequence` (Q1) | core | files with a non-Ok Finding → 4-level `score` on the first 200 lines: dev · internal · user_facing · critical | note `consequence` on every non-Ok Finding of the file |
| `triage` (H1 + Q2 + H4, H8) | core | T0 / T1 Findings (all families) → `choice` gate (mechanical / false_positive / escalate), `choice` effort (local / contained / cross_file), `choice` accept_reason (the family's categorical reasons from the patch skills + none); duplication adds `noul` duplication_real | notes `gate`, `effort`, `accept_reason`, `duplication_real` |
| `friction` (Q5 + H3) | core | High+ CCN / Cognitive / LCOM → `noul` × 3 (change / test / read) + `choice` triage_class (symptomatic / intrinsic / cohesive_procedural) | notes `friction.change`, `friction.test`, `friction.read`, `triage_class` |
| `focus` (Q4) | core | needs `--focus` text; files with a non-Ok Finding (≤300) → 4-level `score` none · read · touch · change | note `focus` (only rendered by `heal status --focus`, which rescans and never writes `latest.json`) |
| `concept` (C12) | code | `choice` over `.heal/concepts.toml` (+ `other`) per outer function ≥3 lines; state = numbered file, or ±40-line windows when the file exceeds the state budget | `concept_mix` (≥2 concepts ≥25% of a file's classified LOC, file ≥60 LOC; High at ≥3), `concept_misplaced` (function's concept ≠ file home, another file's home = that concept; Medium, `fix_hint` = move target), `concept_scatter` (concept in ≥5 files, none ≥40%; Medium, `locations` = other files) |
| `term_drift` (C13) | code | depends on `concept`. Per concept, frequent non-verb words of function names (≥2 names, top 8) that never co-occur → `noul` "same thing?" | `term_drift` (p ≥ 0.7; location = first file of the minority word, `locations` = its other files) |
| `name_mismatch` (C13) | code | functions in files with a hotspot Code finding, plus functions with a ≥Medium CCN / Cognitive finding → 4-level `score` (n/a · matches · arguable · contradicts / hides an effect) | `name_mismatch` (level 3, confidence ≥ 0.5; Medium) |
| `name_choice` (C13, on demand) | code | `--focus` JSON `{"candidates":[{file, symbol, names[]}]}` → `choice` current vs candidates | `report`: `choices[] {best, p, p_current, rename}`; `rename` iff best ≠ current, `p − p_current ≥ 0.2`, confidence ≥ 0.5 |
| `split_points` (C14) | code | High / Critical CCN / Cognitive functions; body statements merged into ≥3-line segments (≤30) → `noul` per adjacent pair "same step?" | note `split_points` on those findings: `lines` = boundaries with p ≤ 0.3, `detail` = step ranges |
| `fix_pattern` (H2) | code | T0 / T1 CCN / Cognitive / duplication findings → `choice` over the patch allow-list + `none` | note `fix_pattern` |
| `test_value` (T7) | test | needs `[features.test]`. Every non-skipped test case → `noul` regression, `noul` brittle, `choice` checks (behaviour / mock_values / framework / constants_or_types / nothing), `noul` has_logic; state = test file + the guessed source file when both fit | `test_value` (Medium) labelled `delete` (trivial checks with confidence ≥ 0.9 and regression < 0.5, or has_logic < 0.2 and regression < 0.5) or `rewrite` (regression ≥ 0.5 and brittle ≥ 0.7); note `test_value` carries the four answers |
| `mock_scope` (T8) | test | needs `[features.test]`. Every lexical mock site (`observer::test::cases::mock_sites`) → `choice` boundary / subject / internal_collaborator / pure_value | `mock_scope` for non-boundary kinds with p ≥ 0.6 (High for `subject`, else Medium) |
| `test_triage` (H7) | test | `coverage_pct` findings → `choice` pure_logic / coordination / io_boundary; skipped cases in `skip_ratio` files → `choice` environment / slow / broken / pending | notes `coverage_band`, `skip_reason` (majority label, detail per test) |
| `test_duplicate` (T9) | test | needs `[features.test]`. Pairs of non-skipped cases in one file with body word-set Jaccard ≥ 0.6 (≤30 per file) → `choice` same_case / parameterizable / different | `test_duplicate` (Medium) at the later case, `locations` = the earlier one |
| `verify_tests` (V2, on demand) | test | `--diff` → the `test_value` questions for test cases overlapping added lines, the `mock_scope` question for added mock lines | `report`: `tests[] {label}`, `mocks[] {kind, bad}`, `pass` |
| `doc_structure` (D7 + H6) | docs | needs `[features.docs]`. Every non-empty section of standalone + paired Markdown docs → `choice` kind (Diátaxis 4 + changelog / adr / runbook / glossary / other) and, from the second section on, `noul` "a separate document starts here"; short pages (≤60 lines) in one directory → `noul` merge per neighbouring pair | `doc_structure.split` (boundaries ≥ 0.7; note lists them, `detail` flags boundaries within ±0.1), `doc_structure.mixed_mode` (≥2 kinds ≥25% of lines in one document), `doc_structure.merge`; note `doc_kind` on the page's docs findings |
| `doc_placement` (D8 + H9) | docs | each doc → `choice` over the doc directories (described by their index page or page titles) | `doc_placement` when another section wins by ≥ 0.2 at p ≥ 0.6; note `placement` on `orphan_pages` findings (the link slot) |
| `doc_drift_semantic` (D1) | docs | paired doc sections (≤40 per doc) with the pair's sources; one state per pair when it fits, else per section → 4-level `score` n/a · accurate · partly outdated · wrong | `doc_drift.semantic` (level 3, confidence ≥ 0.5) |
| `doc_concept` (D9) | docs | depends on `concept`; needs `[features.docs]` and the vocabulary. Every non-empty doc section → `choice` over the concepts | `doc_concept.gap` (at the code file holding most of a concept that has ≥10% of classified code or ≥300 LOC and no doc section) |
| `doc_overlap` (D9) | docs | depends on `doc_concept`. Pairs of ≥5-line sections from different pages in one concept (≤10 per concept) → `noul` duplicate, `noul` conflict | `doc_concept.duplicate` (Medium), `doc_concept.conflict` (High), p ≥ 0.7; `locations` = the other section |
| `doc_pairs` (H5, on demand) | docs | unpaired docs → `choice` over ≤20 sources ranked by path / heading word overlap, plus `none` | `report`: `pairs[] {doc, src, confidence, source: "jev"}`; `/heal-doc-pair-setup` writes them with `PairSource::Jev` |
| `verify_patch` (V1 + V3, on demand) | code | `--diff <range>` → 4 `noul`: relocate, guard_clause, behaviour_change, message_mismatch; whole diff as one state, or one per file when too large | `report`: `checks` (max p per check), `flags` (≥ 0.7, excluding behaviour_change), `pass` |
| `verify_proposal` (V4, on demand) | code | `--focus` JSON `{"proposals":[{id, text, files[]}]}` → the five readability questions, phrased so true = good | `report`: per proposal `checks`, `pass` (all ≥ 0.5) |

Tasks run in registry order, and `heal semantic ask` sends one task's
batches before planning the next, so a task with `depends_on` reads
the answers of the same run through `TaskContext::prior_answer`.
On-demand tasks (`on_demand() == true`) only run with `--task <id>`,
never during `heal status`; their `report` is `tasks[].result` in
`--json`.

### Drain order axes (`core::order::within_severity`)

Inside one Tier + Severity bucket, before `hotspot_score`, compared
lexicographically (never summed): `focus` (0–6, 0 without a note) →
`consequence` (0 dev … 6 critical; neutral 3) → friction (2 if any
≥ 0.7, 0 if all < 0.3, neutral 1) → `fix_ratio` (quarters) → `effort`
(local 4, contained 2, cross_file 0; neutral 1). Notes with
confidence < 0.5 are ignored. With no notes every axis is neutral, so
the order equals the pre-semantic Tier → Severity → `hotspot_score`.

H9 (the doc-patch "is this applicable?" judgments) is covered by the
`triage` gate, which runs for docs findings too, plus `doc_placement`'s
`placement` note for orphan registration.

Test files for the semantic tasks are `tasks::common::TestMatcher`:
the naming heuristic (`is_test_path`) **or** `[features.test].test_paths`.
The default globs are root-anchored (`tests/**`) and miss nested
`crates/<x>/tests/`, so the heuristic always applies here (unlike the
`is_test_file` tag).

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
