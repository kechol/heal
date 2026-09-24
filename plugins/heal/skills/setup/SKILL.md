---
name: setup
description: Set up heal in this repository, or re-check an existing setup — safe to run any time. Starts from `heal doctor --json` and does only what is missing or stale — initialize `.heal/`, recalibrate when the drift rules fire, tune `.heal/config.toml` to a strictness level, enable the optional docs / test / semantic families and build their inputs (doc pairs, coverage reporter, API key, concept vocabulary), and remove skill folders older heal versions copied into the project. Trigger on "set up heal", "configure heal", "check my heal setup", "tune heal thresholds", "make heal stricter / more lenient", "enable heal docs / coverage / semantic", "heal doctor shows todo", "/heal:setup".
argument-hint: "[config | docs | tests | semantic]"
---

# /heal:setup

One entry point for everything heal needs before `/heal:refactor`,
`/heal:docs`, and `/heal:tests` are useful. Every run starts by asking
`heal doctor` what is already in place, so re-running it changes only
what is missing or stale, and a fully set-up project finishes in one
report with no questions.

Load `${CLAUDE_PLUGIN_ROOT}/references/cli.md` before the first `heal`
command: it holds the JSON contracts and the output-language rules this
skill follows.

## 1. Diagnose

1. Run `heal --version`. When `heal` is missing, stop and point the
   user at the install commands (`cargo install heal-cli` or
   `brew install kechol/tap/heal-cli`). When `heal doctor` is an
   unknown subcommand, the CLI predates this plugin: stop and ask the
   user to upgrade it.
2. Run `heal doctor --json` and keep the result. Show it as a short
   table — one row per section with its `status` and `detail` — before
   asking anything.

## 2. Plan

Turn the report into steps. **Required** steps come from sections whose
status is `todo` or `error`; **optional** steps are things the user may
want but doctor does not require.

| Doctor section | Status | Step | Guide |
|---|---|---|---|
| `config` | `todo` (not initialized) | **Initialize**: `heal init --json`, then **Tune** | `references/tuning.md` |
| `config` | `error` | Show the loader error; fix the named key in `.heal/config.toml` with the user | `references/config.md` |
| `calibration` | `todo`, file missing | `heal calibrate --json` (creates it) | — |
| `calibration` | `todo`, `reasons` non-empty | **Recalibrate** — only after the user agrees | — |
| `post_commit_hook` | `todo` | `heal init --json` (keeps config and calibration, reinstalls the hook) | — |
| `post_commit_hook` | `warn` | Show the one line to add to their own hook: `command -v heal >/dev/null 2>&1 && heal hook commit || true`. Never overwrite it | — |
| `docs` | `todo` / `error` | **Docs** Part 2 (doc pairs) | `references/docs.md` |
| `test` | `todo` | **Tests** Part 2 (coverage reporter) | `references/tests.md` |
| `semantic` | `todo` / `error` | **Semantic** Part 2 (no key) or Part 3 (no concepts) | `references/semantic.md` |
| `legacy_skills` | `todo` | **Clean up**: `heal skills uninstall --json` | — |

Optional steps:

- **Tune thresholds** (strictness Strict / Default / Lenient,
  workspaces, excludes) — always offered right after Initialize;
  otherwise offered only when the user asks to tune.
- **Enable** each family whose section is `off` (`docs`, `test`,
  `semantic`) — Part 1 of its guide, then the rest.
- **Response language** — when `config.response_language` is absent
  and the user writes in a language other than English, offer to set
  `[project].response_language` so every heal skill answers in it.

When an argument names a step (`/heal:setup docs`), skip the menu and
run that step — after the diagnosis, and including its prerequisites.

Otherwise ask once with `AskUserQuestion`: one `multiSelect` question
for the required steps (recommend all of them) and, in the same call,
one for the optional steps. Use at most four options per question;
group related steps into one option when there are more. When nothing
is required and the user asked for nothing specific, skip the questions:
report that the setup is complete, list the `off` families as things
`/heal:setup` can enable later, and stop.

## 3. Run the selected steps

Run them in this order — each one depends on the ones before it:

1. **Initialize** — `heal init --json`. Read `monorepo_signals` for the
   workspace phase of tuning.
2. **Clean up** — `heal skills uninstall --json`. The removed folders
   were tracked in git; tell the user to commit the deletion.
3. **Recalibrate** — name the drift rule and the numbers from doctor,
   ask, and only on a yes run `heal calibrate --force --json`. Never
   recalibrate on your own.
4. **Tune** — follow `references/tuning.md`.
5. **Docs** — follow `references/docs.md` (Part 1 only when the family
   is being enabled).
6. **Tests** — follow `references/tests.md` (Part 1 only when the
   family is being enabled).
7. **Semantic** — follow `references/semantic.md` (Part 1 only when the
   family is being enabled).

Each guide asks for its own approvals before writing. When the user
declines a step, skip it and carry on with the rest.

## 4. Confirm

Run `heal doctor --json` again and show the table next to the first
one. When the config or calibration changed, also run
`heal status --refresh --json` and report the new `severity_counts`.

```
Setup  (heal 0.7.0)
  config            ok
  calibration       todo → ok     recalibrated (240 commits since the last one)
  post-commit hook  ok
  docs              off → ok      enabled; 12 doc pairs
  test              off           skipped
  semantic          off           skipped
  legacy skills     todo → ok     removed 12 folders — commit the deletion

Findings: critical 3 · high 11 · medium 22
Commit .heal/config.toml, .heal/calibration.toml, .heal/doc_pairs.json.
Next: /heal:refactor
```

## What this skill may write

- `.heal/config.toml` — merge, never replace: keep every key the step
  does not own. After writing, `heal doctor --json` must report
  `config` as `ok`; if it reports `error`, restore the previous file
  and show the loader message.
- `.heal/doc_pairs.json`, `.heal/concepts.toml`.
- `.heal/calibration.toml` — only through `heal calibrate`.
- The post-commit hook — only through `heal init`.
- Legacy skill folders — removed only through `heal skills uninstall`.
- Reporter installs, reporter runs, and `lcov.info` — each approved
  step by step in `references/tests.md`. CI files stay a printed
  proposal unless the user explicitly chooses to apply them.

Never edit source files or `.heal/findings/*`, never paste or store an
API key, never run the paid `heal semantic ask` unless the user asks
for it in this session, and never commit — `.heal/` files are the
team's shared setup, so tell the user which ones to commit.
