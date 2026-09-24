---
description: Product-scope guardrails for heal — what is in / out of scope and which design philosophies are non-negotiable.
---

# Scope rules

## R1. CCN, Cognitive, Duplication % are proxies, not targets

Drive findings down by removing real friction (hard-to-test, hard-to-
read, hard-to-change). Don't write code or prose that frames the
metric as the target. Goodhart's Law applies — driving metrics to
zero degrades the codebase.

The drain target is **Critical AND `hotspot=true`**. Just-Critical
without hotspot is signal, not target.

## R2. No persistent metrics history

`heal metrics` recomputes every invocation. There is no `snapshots/`,
no rolling buffer, no "delta vs. previous run" field. Drift is
served by `heal diff <ref>` on demand.

The motivation is per-team determinism: every teammate on the same
commit + config + calibration sees identical findings. Do not
introduce features that break that property.

## R3. Auto-recalibration is forbidden

`heal calibrate` runs only when the user asks. The post-commit hook
never triggers it. `heal status` never triggers it. If your change
wants newer percentiles, prompt the user to run
`heal calibrate --force` — don't run it for them.

## R4. The findings cache is single-record and tracked

Files under `.heal/findings/`:

- `latest.json` — one `FindingsRecord`.
- `fixed.json` — `BTreeMap<finding_id, FixedFinding>`.
- `regressed.jsonl` — append-only audit trail.

That's the full layout. No history rotation, no `YYYY-MM.jsonl`,
no archive directory.

All three are **git-tracked** (`heal init` no longer writes a
`.heal/.gitignore` — the template was empty so the file was just
noise). Two consequences agents must keep load-bearing:

- `FindingsRecord.id` is a **deterministic** FNV-1a digest of
  `(head_sha, config_hash, worktree_clean)` — never a ULID or a
  wall-clock value. Same triple → byte-identical `latest.json`,
  which is what keeps `git status` clean across teammates on the
  same commit.
- `heal status` cache reuse goes through `is_fresh_against`. A
  `latest.json` from a different commit, different config, or a
  dirty scan is auto-rescanned even without `--refresh`. Don't
  short-circuit this gate — the user fetching a teammate's
  `latest.json` at a different HEAD relies on it.

**This repository is the exception.** HEAL's own root `.gitignore`
ignores `/.heal/` entirely so dogfooding (`heal init --force`,
repeated rescans, throwaway verdicts) never dirties the tree. That
is a property of this repo, not of the product: user repositories
track `.heal/` as described here and in R6.

## R5. v0.x out-of-scope features

Don't propose these without explicit roadmap discussion:

- `heal run` / autonomous PR generation (v0.4+ target).
- LSP-based metrics.
- Multi-agent provider abstraction.
- Languages beyond TypeScript / JavaScript / Rust / Python / Go / Scala.
- Cloud sync, telemetry, network access (HEAL is local-only;
  network access = `git2` over the local repo only). The single
  exception is the opt-in `[features.semantic]` family: only
  `heal semantic ask` and `heal auth jev status` may connect, and
  only to the TypeSafe Jev API. No other command, hook, or observer
  may open a connection, with or without the feature enabled.
- `[features.docs]` extensions deferred past v0.4: external HTTP
  link checking, executable example verification, issue-tracker /
  Stack Overflow question coverage, readability scores, terminology
  consistency vs a glossary, structural-depth analysis. `doc_drift`
  Type 2 (signature mismatch via per-language code-block parsing)
  also defers. Type 3 (semantic drift) ships only through
  `[features.semantic]` (Jev verdicts), never as an LLM call from
  an observer.

## R6. New `.heal/` files require a decision

In user repositories everything under `.heal/` is tracked:
`.heal/*.toml` and `.heal/doc_pairs.json` are the team contract,
`.heal/findings/*` is shared per-commit state (R4), and
`.heal/concepts.toml` is the concept vocabulary the semantic tasks
classify against (team contract, like `config.toml`), and
`.heal/semantic/verdicts/*.jsonl` is the Jev verdict cache — tracked
so teammates without an API key read the same verdicts and
`latest.json` stays byte-identical across them. Verdicts of tasks that
judge one person's input (on-demand checks such as `verify_patch`,
and `focus`) are not team state: they live in the untracked
`.heal/cache/semantic/verdicts/` and stay out of `config_hash`, so a
patch session never dirties the worktree. Secrets never go
under `.heal/`; the Jev key lives in the environment or in the
per-user `credentials.toml`. (`.heal/cache/` is the one untracked,
machine-local directory.) New top-level files must state which side
they fall on and why. When in doubt, open an issue first.

## R7. No marketplace, no plugin distribution

Skills are bundled inside the binary (`include_dir!`). Users update
skills by upgrading `heal-cli`. There is no `heal skills add <url>`,
no separate registry, no per-skill version pinning.

## R8. heal-code-review and heal-code-patch are distinct

Review = read, propose, including architecture. Patch = mechanical
write, one commit per finding. Don't merge them, don't add an
architecture-decision step to patch, don't add a code-write step to
review.

## R9. Coupling noise is filtered, not surfaced raw

The PairClass demotion (Lockfile / Generated / Manifest / TestSrc /
DocSrc / Genuine) is load-bearing. Without it, lockfile bumps and
mass-renames would dominate the drain queue. If a "real" pair is
getting demoted incorrectly, fix the classifier, don't bypass it.

## R10. Internal docs target AI agents; user docs target juniors

`docs/` (Starlight, en + ja) and `README.md`: junior developer who
just installed HEAL. No internal jargon (`FindingsRecord`,
`config_hash`, `worktree mode`).

`.claude/docs/` and `.claude/rules/`: AI agent modifying this
codebase. Internal jargon expected.

If you're tempted to put architectural detail in user docs, redirect
to `.claude/docs/` and link from a brief user-doc mention.
