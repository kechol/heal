---
title: Code · Skills
description: The bundled skills for the always-on Code family — /heal-cli, /heal-setup, /heal-code-review, /heal-code-patch — for Claude Code and OpenAI Codex.
---

heal ships a bundled set of skills so the metrics it collects flow
into your AI agent sessions. The same skill bodies serve every
supported agent:

| Agent | Project install path | Discovery doc |
|---|---|---|
| Claude Code | `.claude/skills/` | <https://code.claude.com/docs/en/skills> |
| OpenAI Codex | `.agents/skills/` | <https://developers.openai.com/codex/skills> |

`heal init` installs them automatically for every agent it detects
on your `PATH` (you'll get one Y/N prompt per agent in TTY mode;
`--yes` accepts all, `--no-skills` skips all). To run the install
explicitly later:

```sh
heal init --force --yes      # refreshes both targets in lockstep
heal skills install          # Claude target only (v0.4 limitation)
```

This page covers the four Code-family skills. The doc-family
skills live under [Docs › Skills](/heal/docs/skills/); the
test-family skills under [Test › Skills](/heal/test/skills/).

The skill set is shipped inside the `heal` binary, so the version
installed always matches the binary in use. After upgrading
`heal`, the simplest refresh path is `heal init --force --yes` —
that re-extracts every detected agent's tree (the
`heal skills update` subcommand currently only refreshes the
Claude target).

## `/heal-code-review` — the audit skill

Read-only. Reads `heal status --all --json`, deep-reads the
flagged code, and returns:

1. An **architectural reading** — what the findings say
   _as a system_, not as a list (the dominant axis: complexity,
   duplication, coupling, hub).
2. A **prioritized TODO list** drawn from T0 only by default. T1
   gets an "if bandwidth permits" section; Advisory is summarized
   as a count.

Never edits source. Can recommend `heal mark accept` for findings
the team has decided are intrinsic — a deliberately complex tax
engine, a procedurally cohesive parser combinator.

After reading a review, you can act on any item right away — just
ask the agent in the same session ("apply the first three", "let's
fix the extract-function items"). Mechanical fixes get routed
through `/heal-code-patch`; judgment-call items wait for your
direction.

### Why review and patch are split

**Patch** handles the mechanical class — a long function that
wants Extract Function, a duplicate that wants a shared helper, a
drifted test that needs realignment. Steps that don't require
domain knowledge.

**Review** also surfaces the items that *do* need a human call —
should this hub be split? is this duplication two different
concepts that grew the same shape? is this complex function
intrinsic to the problem or accidental? — so review proposes and
stops. Mixing them into one auto-driver would either rush
judgment calls or refuse to touch the mechanical pile.

Trigger phrases: "review the codebase health", "what does heal
say?", "where should we refactor?", "/heal-code-review".

## `/heal-code-patch` — the write skill

Drains `.heal/findings/latest.json` one finding at a time, in
Severity order, committing once per fix. The loop drains **T0
(`must`) only**; T1 / Advisory are surfaced for review but never
auto-drained.

**Pre-flight** (refuses to start otherwise):

- Clean worktree.
- Cache exists (runs `heal status --json` to populate if missing).
- Calibration exists (without it every Finding is `Severity::Ok`
  — nothing to act on).

**Per-metric moves** (Fowler / Tornhill vocabulary):

| Metric | Common move |
|---|---|
| `ccn` / `cognitive` | Extract Function, Guard Clauses, Decompose Conditional |
| `duplication` | Extract Function / Method, Pull Up Method, Rule of Three |
| `change_coupling` (incl. `.symmetric`) | Surface the architectural seam — patch never auto-fixes coupling |
| `lcom` | Extract Class along the cluster boundary |
| `hotspot` | Hotspot is a flag, not a problem — act on the underlying metric |

**Constraints** (enforced by the skill): one finding = one
commit, Conventional Commit subject + `Refs: F#<finding_id>`
trailer, never push / amend / `--no-verify`. Findings whose
metric belongs to the docs or test families are skipped — those
go through `/heal-doc-patch` / `/heal-test-patch`.

Trigger phrases: "fix the heal findings", "drain the cache",
"work through the TODO list", "/heal-code-patch".

## `/heal-cli` — CLI reference

A concise, complete reference for the `heal` CLI — every
subcommand, every `--json` shape, the `.heal/` files each command
reads or writes. Loaded by every other skill before shelling out
to `heal` so the CLI surface is treated as a stable contract.

## `/heal-setup` — setup wizard

One-shot setup wizard. Calibrates the project, surveys the
codebase, asks for a strictness level (Strict / Default /
Lenient), writes or updates `.heal/config.toml`, then offers to
turn on `[features.docs]` / `[features.test]` and chain into the
matching setup skill (`/heal-doc-pair-setup` /
`/heal-test-reporter-setup`).

Re-run when the codebase shifts enough that the bar should move,
or when every Critical has been drained for a sustained run —
the skill recommends `heal calibrate --force` in those cases.

## Maintenance

```sh
heal skills update     # refresh after upgrading heal (Claude target, drift-aware)
heal skills status     # list drifted files (Claude target)
heal skills uninstall  # remove every bundled skill (Claude target)
```

`update` leaves hand-edited files in place with a warning; pass
`--force` to overwrite. `uninstall` removes every
`.claude/skills/heal-*` directory; sibling skills you authored
survive, and project data under `.heal/` is untouched.

For the Codex target (`.agents/skills/`), `heal init --force --yes`
is the supported refresh path in v0.4 — multi-target support for
the explicit `heal skills *` group is tracked as follow-up.
