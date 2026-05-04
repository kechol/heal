---
title: Code · Skills
description: The four bundled Claude Code skills for the always-on Code family — /heal-cli, /heal-setup, /heal-code-review, /heal-code-patch.
---

heal ships a bundled set of Claude Code skills so the metrics it
collects flow into Claude sessions. They're installed once per
repository:

```sh
heal skills install
```

This page covers the four Code-family skills. The doc-family skills
live under [Docs › Skills](/heal/docs/skills/); the test-family
skills under [Test › Skills](/heal/test/skills/).

The four code-family skills:

```
.claude/skills/
├── heal-cli/
├── heal-code-patch/
├── heal-code-review/
└── heal-setup/
```

The skill set is shipped inside the `heal` binary, so the version
installed always matches the binary in use. After upgrading `heal`,
run `heal skills update` to refresh.

## `/heal-code-review` — the audit skill

Read-only. Reads `heal status --all --json`, deep-reads the flagged
code, and returns:

1. An **architectural reading** of the codebase — what the findings
   say _as a system_, not as a list (the dominant axis: complexity,
   duplication, coupling, hub).
2. A **prioritized TODO list** — drawn from **T0 (`must`) only** by
   default. T1 (`should`) findings get an "If bandwidth permits"
   section; Advisory findings are surfaced as a count only.

`/heal-code-review` proposes only — it never edits source. It can
also recommend `heal mark accept` for findings the team has decided
are intrinsic (a deliberately complex tax engine, a procedurally
cohesive parser combinator).

The write counterpart is `/heal-code-patch`.

Trigger phrases: "review the codebase health", "what does heal
say?", "where should we refactor?", "/heal-code-review".

## `/heal-code-patch` — the write skill

Drains `.heal/findings/latest.json` one finding at a time, in
Severity order, committing once per fix.

Pre-flight (refuses to start when these fail):

1. **Clean worktree.** A dirty worktree means the cache's
   `worktree_clean = false` — the recorded numbers don't reflect the
   on-disk source. The skill asks you to commit or stash first.
2. **Cache exists.** If `latest.json` is missing, the skill runs
   `heal status --json` once to populate it.
3. **Calibration exists.** Without `calibration.toml`, every Finding
   is `Severity::Ok` — nothing to act on.

The loop drains **T0 (`must`) only**. T1 / Advisory are surfaced for
review but never auto-drained — the session ends when T0 is empty
rather than silently extending.

Per-metric, the patch skill maps to established refactoring moves
(Fowler, Tornhill):

| Metric                      | Common moves                                                                           |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `ccn` / `cognitive`         | Extract Function, Replace Nested Conditional with Guard Clauses, Decompose Conditional |
| `duplication`               | Extract Function / Method, Pull Up Method, Form Template Method, Rule of Three         |
| `change_coupling`           | Surface the architectural seam — patch never auto-fixes coupling                        |
| `change_coupling.symmetric` | Same — strong "responsibility mixing" signal needs a human call                          |
| `lcom`                      | Split the class along the cluster boundary (usually Extract Class)                      |
| `hotspot`                   | Hotspot is a flag, not a problem — act on the underlying CCN / dup / coupling          |

Constraints (enforced by the skill):

- One finding = one commit. No squashing across findings.
- Conventional Commit subject + body + `Refs: F#<finding_id>`
  trailer.
- Never push, never amend, never `--no-verify`.
- The loop stops at the cache boundary; new findings the user wants
  addressed go into a fresh `heal status` run.

`/heal-code-patch` skips findings whose metric belongs to the docs
or test families — those are owned by `/heal-doc-patch` and
`/heal-test-patch` respectively.

Trigger phrases: "fix the heal findings", "drain the cache",
"work through the TODO list heal produced", "/heal-code-patch".

## `/heal-cli` — CLI reference

A concise, complete reference for the `heal` CLI — every
subcommand, every `--json` shape, the `.heal/` files each command
reads or writes. Claude loads it before shelling out to `heal` from
any other skill so the CLI surface is treated as a stable contract,
not inferred from `--help` text.

## `/heal-setup` — setup wizard

One-shot setup wizard. It calibrates the project, surveys the
codebase, asks for a strictness level (Strict / Default / Lenient),
writes or updates `.heal/config.toml`, then asks whether to enable
each optional feature family (`[features.docs]`, `[features.test]`)
and chains to the companion setup skill if you opt in.

Use it when:

- Setting heal up for the first time.
- After a structural change to the codebase (a new vendored tree,
  a layer rewrite).
- When you want to shift the quality bar without remembering every
  threshold.
- When you want to turn on docs or coverage observers without
  hand-editing `[features.*]` blocks.

If you accept `[features.docs]`, `/heal-setup` populates the
`[features.docs.standalone]` include / exclude globs from the
project's actual doc layout and chains to `/heal-doc-pair-setup`
to generate `.heal/doc_pairs.json`. If you accept `[features.test]`,
it populates `test_paths` and `lcov_paths` from the detected
language stack and chains to `/heal-test-reporter-setup` for the
language-specific lcov reporter wiring.

`/heal-setup` also recommends `heal calibrate --force` when the
calibration baseline has drifted enough to matter — file count
moved significantly, the calibration is old relative to project
velocity, or every Critical has been drained for a sustained run.

## Updating

```sh
heal skills update
```

`update` is drift-aware: files that have been hand-edited are left
in place, with a warning. Pass `--force` to overwrite anyway.
`heal skills status` lists which files have drifted.

## Removing

```sh
heal skills uninstall
```

Removes every bundled skill directory under `.claude/skills/heal-*`
— including the doc and test families if they were extracted.
Sibling skills you authored survive, and project data under
`.heal/` is otherwise untouched.
