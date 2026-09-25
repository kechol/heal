---
title: Code · Skills
description: The heal Claude Code plugin's skills for the always-on Code family — /heal:setup and /heal:refactor.
---

heal's skills ship as a Claude Code plugin, so the findings heal
collects flow straight into your Claude sessions. Install it once, in
Claude Code:

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

The plugin lives in your Claude Code settings, not in your project —
nothing is added to your git tree. It is pinned to the heal release it
shipped with; when you upgrade the `heal` CLI, update the plugin too
(`/plugin marketplace update heal`, then `/plugin update heal@heal`).
At the start of a session in a project that uses heal, the plugin
prints one line if the CLI and the plugin come from different releases.

The plugin has four skills:

| Skill            | For                                                                    |
| ---------------- | ---------------------------------------------------------------------- |
| `/heal:setup`    | Set up heal, or re-check the setup. Covered on this page.              |
| `/heal:refactor` | Read the code findings, propose refactors, apply the ones you approve. |
| `/heal:docs`     | The same for docs — see [Docs › Skills](/heal/docs/skills/).           |
| `/heal:tests`    | The same for tests — see [Test › Skills](/heal/test/skills/).          |

None of the skills push, open a pull request, or change files before
you approve what they propose.

## `/heal:setup` — set up and re-check

Safe to run any time. It starts from `heal doctor`, which reports what
is already in place, and then does only what is missing or stale:

- **Initialize** the project (`heal init`) when `.heal/` does not exist.
- **Tune strictness** — asks Strict / Default / Lenient and writes
  `.heal/config.toml` from a survey of your codebase (excluded paths,
  workspaces in a monorepo, metrics with no signal). Offered on the
  first run and whenever you ask to tune.
- **Recalibrate** when the codebase has moved on — more than 200
  commits since calibration, the file count changed by more than 20%,
  or no Critical / High findings are left after ten or more fixes. It
  asks first; heal never recalibrates by itself.
- **Enable the optional families** and build what each needs: doc
  pairs for Docs, a coverage reporter for Test, and an API key plus a
  concept vocabulary for [Semantic (Jev)](/heal/semantic/).
- **Remove skill folders** that heal 0.6 and earlier copied into the
  project (`.claude/skills/heal-*`, `.agents/skills/heal-*`).

On a project that is already set up, it reports that everything is in
place and asks nothing. Pass a step to go straight to it:
`/heal:setup docs`, `/heal:setup tests`, `/heal:setup semantic`, or
`/heal:setup config`.

Trigger phrases: "set up heal", "check my heal setup", "make heal
stricter", "enable heal coverage", "/heal:setup".

## `/heal:refactor` — propose and apply refactors

One skill for understanding what heal found in the code and acting on
it. It runs in five steps:

1. **Diagnose.** Reads `heal status --all --feature code --json`,
   opens the flagged files, and reads them as a system: which files
   carry several findings, which pairs of files change together across
   module boundaries, which files are hubs, and how the codebase is
   layered.
2. **Propose.** Starts with a short architectural reading — the
   dominant problem (complexity, duplication, coupling, mixed concepts)
   — then numbered proposals, Critical findings on hotspot files first.
   Each proposal names the change, the friction it removes, the
   findings it resolves, the files it touches, and its risk.
3. **Choose.** You pick which proposals to apply.
4. **Apply.** One commit per proposal. Before each commit the skill
   runs your build and tests; if they fail, the attempt is discarded.
   After the commit it records the resolved findings (`heal mark fix`)
   and re-checks them with `heal status`.
5. **Report.** What was applied, what was skipped and why, and what is
   left in the queue.

**Structural proposals.** Splitting a file along its concepts, moving a
function to the module it belongs to, extracting a class, or
introducing an interface at a layer boundary are ordinary proposals
when at least two independent signals point at the same seam — for
example, LCOM clusters that match the concepts
[Semantic (Jev)](/heal/semantic/) found in a file, or a function whose
concept lives elsewhere and whose file changes together with that
other file. With a single signal, the skill raises it as a question
instead.

**Needs a decision.** Renaming public API (exported items, CLI flags,
JSON fields), choosing between two defensible module boundaries, and
domain-level moves get their own question with concrete options before
anything is applied.

**Accept instead of change.** Some findings measure something
intentional — a generated parser table, an exhaustive `match` over a
closed enum, a pipeline whose steps belong together. The skill proposes
recording those as accepted (`heal mark accept`, with a short reason)
so they leave the queue; it never accepts without your approval.

**With [Semantic (Jev)](/heal/semantic/) enabled**, proposals are
checked before you see them (`verify_proposal`), and each commit is
checked after it lands (`verify_patch`): when complexity only moved, a
condition was flipped into early returns, the commit message does not
match the change, or a refactor changed behaviour, the skill undoes
that commit and moves on. Without the family or a key, those checks
are skipped.

Arguments: a path (`/heal:refactor src/payments`) narrows the work to
findings under it, a finding id narrows it to that finding, and `plan`
stops after the proposals.

Trigger phrases: "what does heal say?", "where should we refactor?",
"fix the heal findings", "/heal:refactor".

## Upgrading from heal 0.6 or earlier

Earlier versions bundled twelve skills in the CLI and copied them into
each project. They map onto the plugin like this:

| Before                                                                                     | Now                     |
| ------------------------------------------------------------------------------------------ | ----------------------- |
| `/heal-setup`, `/heal-concepts-setup`, `/heal-doc-pair-setup`, `/heal-test-reporter-setup` | `/heal:setup`           |
| `/heal-code-review`, `/heal-code-patch`                                                    | `/heal:refactor`        |
| `/heal-doc-review`, `/heal-doc-patch`, `/heal-doc-scaffold`                                | `/heal:docs`            |
| `/heal-test-review`, `/heal-test-patch`                                                    | `/heal:tests`           |
| `/heal-cli`                                                                                | (built into the plugin) |

After installing the plugin, run `heal skills uninstall` in each
project to remove the old copies, then commit the deletion. Only the
folders heal wrote are removed; your own skills stay.
