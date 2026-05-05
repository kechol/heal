---
name: heal-doc-patch
description: Drain `[features.docs]` findings from the cache, applying mechanical fixes (broken internal links, dangling identifier deletions, orphan-page registration, resolvable TODO markers) one finding per commit. Refuses to start on a dirty worktree. Does NOT push or open PRs. Trigger on "fix the doc findings", "drain the doc cache", "patch stale docs", "/heal-doc-patch".
---

# heal-doc-patch

Drain the `doc_*` findings that `heal status` produced. One finding
per commit, in Severity order, until the docs slice of the cache is
empty (or the user stops). This is the **write** counterpart to
`/heal-doc-review`.

## Output language

Write the per-finding narration, prompts, and end-of-loop summary in
the user's language. Resolution order:

1. Explicit instruction in the current conversation.
2. The language the user is writing in (Claude Code's conversation
   language).
3. `[project].response_language` in `.heal/config.toml` (free-form:
   `"Japanese"`, `"日本語"`, `"ja"`, `"français"`).
4. English (fallback).

Identifiers stay verbatim — file paths, `Finding.metric` strings
(`doc_drift`, `doc_link_health`, …), command names (`heal status`),
and finding ids are part of the contract. Doc edits themselves keep
the language already used in the doc tree (don't translate a stale
identifier in an English doc into Japanese, even if the user is
writing in Japanese). Commit subject lines follow the project's
existing convention (in this repo: English, per Conventional
Commits).

## Mental model

`heal status --feature docs --json` writes a `FindingsRecord` to
`.heal/findings/latest.json`. The seven docs metric strings:
`doc_freshness`, `doc_drift`, `doc_coverage`, `doc_link_health`,
`orphan_pages`, `todo_density`, plus the per-family decoration
carrier `doc_hotspot` (always `Severity::Ok`; flips
`hotspot=true` on the other six when the pair's churn × debt
sits in the project's top decile). Finding ids are deterministic
— same broken link keeps the same id, so disappearance from the
cache after a commit means it's genuinely fixed.

This skill mirrors `/heal-code-patch`'s loop — same pre-flight,
same per-commit `heal mark fix`, same constraints. The
allow-list / escalate-list below is what makes doc fixes
different.

## Pre-flight (refuse to start when these fail)

1. **Clean worktree.** `git status --porcelain` must be empty. Stop
   otherwise.
2. **`[features.docs]` enabled.** Probe with
   `heal status --feature docs --json`. When the docs family is
   disabled in `.heal/config.toml`, this command exits 1 with a
   stderr message naming the missing config switch — that's the
   early-exit contract. Bail and tell the user to run
   `/heal-setup` (or hand-edit `.heal/config.toml`) to enable the
   feature before retrying.
3. **`.heal/doc_pairs.json` present.** When missing, recommend
   `/heal-doc-pair-setup` first. The doc observers wouldn't have
   produced findings in any case.
4. **Cache exists.** `heal status --feature docs --json` returns
   at least one `doc_*` finding. If only `severity: "ok"` doc
   findings exist, say so — the calibration thresholds are loose
   enough that nothing is actionable.

## The loop

```
while there are non-Ok doc_* findings in the cache:
    pick the next one (Severity order: Critical → High → Medium)
        skip findings where `accepted == true`
    decide: allow-list (apply mechanically) or escalate-list (stop)?
    if allow-list:
        read the doc; apply the smallest fix
        run any verification (link recheck, syntax)
        git commit -m "<conventional doc message>"
        heal mark fix --finding-id <id> --commit-sha <sha>
        heal status --refresh --feature docs --json
    if escalate-list:
        end the session; surface remaining findings; recommend /heal-doc-review
```

Stop conditions: doc cache empty, user interrupts, or only
escalate-list findings remain.

## Allow-list (apply mechanically)

These transformations are deterministic enough to apply in-loop
after reading the doc to confirm the pattern fits.

### `doc_link_health` (MissingPath)

The link `[text](./old-path.md)` doesn't resolve. Apply when:

- The target was renamed in a recent commit and the new path is
  unambiguous (one candidate via `git log --diff-filter=R`).
- The target was deleted and there's a clear redirect candidate
  in the same directory (e.g. consolidated into another file).

Fix shape: replace the link target. Don't auto-pick when there are
multiple candidates — escalate.

### `doc_link_health` (MissingAnchor)

The `#anchor` doesn't match any heading slug in the doc. Apply
when:

- A heading with the right text exists but a different slug — fix
  the anchor.
- The heading was renamed and the new slug is obvious — update
  both target and anchor.

### `doc_drift` (dangling identifier — fenced examples)

The identifier appears inside a code-fence-style block in the doc
that is intended as a real example. Apply when the identifier is
clearly a leftover from a renamed symbol AND grep'ing the codebase
finds the new name (one candidate). Replace the identifier in the
example.

### `doc_drift` (dangling identifier — prose mention)

The doc mentions the identifier in inline prose ("`OldStruct`
handles X"). Apply when:

- A renamed equivalent exists (`NewStruct` is a clear successor),
  AND the surrounding sentence still makes sense after substitution.
- The identifier was removed entirely — the right fix is usually
  to delete the sentence (or paragraph). Apply when the deletion
  doesn't break the surrounding logic.

### `orphan_pages`

The doc isn't linked from anywhere. Apply when:

- The standalone TOC / index file (`docs/SUMMARY.md`,
  `docs/_sidebar.md`, Starlight's nav config) has an obvious slot
  for it. Add a link.
- The orphan is clearly archived content that should move to an
  excluded directory (`docs/archive/**`) — move it.

Don't auto-link from an arbitrary doc just to clear the finding.
Linking creates a meaning relationship; spurious links degrade
navigation.

### `todo_density` (resolvable markers)

A `TODO: pin Rust version` where the codebase now pins it.
A `[要確認] サポートする最小バージョン` where the answer is in
`Cargo.toml`.

Apply when the marker's question has a clear, verifiable answer
in the current code or config. Replace the marker line with the
answer. When the answer requires interpretation (e.g. a TODO
asking "should we deprecate X?"), escalate.

## Escalate-list (stop and surface)

These need judgment that lives in `/heal-doc-review` (or with the
user). When a finding's best-fit pattern is here, end the loop:

### `doc_freshness`

A doc lagging the src by 5+ commits is rarely a mechanical fix.
Either:

- The doc needs new content (interpretive — what to write?), or
- The doc needs to be retired (interpretive — should it be?).

Escalate. `/heal-doc-review` proposes; the user decides.

### `doc_coverage`

A pair entry's `doc` is missing. Writing the missing doc requires
deciding the doc's Diátaxis purpose, audience, and depth — none
of those are mechanical. Escalate.

### `doc_drift` requiring rewrite

The dangling identifier was a key concept whose successor doesn't
exist (system rearchitected, the concept was abolished). The fix
is to rewrite the surrounding section, not substitute the
identifier. Escalate.

### `doc_link_health` (ambiguous redirect)

Multiple plausible new targets exist. Picking one is a doc-design
choice, not a mechanical lookup. Escalate.

### `todo_density` (unanswerable markers)

`TODO: decide whether to deprecate this`. The marker's question
has no answer yet. Escalate (or convert to a tracked issue).

### Anything paired with a hotspot decoration

A `doc_*` finding on a hotspot file is high-leverage — but
hotspot files are usually structurally complex AND under
churn. Mechanical doc patches there often paper over
architectural drift. Escalate to `/heal-doc-review` for
proposal-level discussion before mechanical fixes.

## Anti-patterns to refuse

The four traps from `/heal-doc-review`'s reference apply here.
Two are load-bearing for the mechanical loop:

### Don't manufacture content to clear a finding

Writing `# CLI\n\nCLI documentation.\n` to satisfy
`doc_coverage` is forbidden — a blank stub is actively worse
than a missing doc (Coverage trap, `/heal-doc-review`'s
`references/architecture.md` §4). When `doc_coverage` is all
that's left, end the loop; the write is interpretive.

### Don't harmonise across intentional drift

Quotations, migration guides, deprecation notes, and CHANGELOG
entries deliberately reference old names. When a `doc_drift`
finding sits inside one, escalate — the drift is load-bearing.

## Verification per commit

Markdown / RST don't have a build step in most projects, but light
verification keeps the loop honest:

- After a link fix: re-grep for the broken target across all docs;
  ensure the fix didn't introduce a new break elsewhere.
- After a markdown edit: render with the project's preview tool if
  one is configured (Starlight, mdBook, mkdocs build) and confirm
  no syntax errors.
- After a TODO resolution: ensure the answer matches what the code
  / config actually says today, not what it might.

If verification fails, **revert the change** (`git restore .`) and
skip the finding.

## Commit message format

Conventional Commits with the finding id:

```
docs(heal): fix broken link in docs/cli.md → migration.md

The legacy ./old-flag.md was consolidated into migration.md in
e8a1f2c. Update the cross-reference.

Refs: F#doc_link_health:docs/cli.md:./old-flag.md:1234567890abcdef
```

Subject: `docs(heal): <verb> in <doc-path>`. Body: 2-3 sentences
naming the underlying cause (rename, removal, etc.) — the
audit-trail value is "why was this fix correct?".

## Marking the commit

```
heal mark fix \
  --finding-id "<finding_id>" \
  --commit-sha "$(git rev-parse HEAD)"

heal status --refresh --feature docs --json
```

Same pattern as `/heal-code-patch`.

## Output format

While running, narrate one short paragraph per finding:

```
[1/8] 🔴 Critical  doc_drift  docs/cli.md:42 `OldStruct`
  Renamed to `Cli` in 7d3a1c2. Updated the inline mention.
  Committed: a1b2c3d4. heal status confirms fixed.

[2/8] 🟡 High      doc_link_health  docs/api.md:18 → ./removed.md
  Target deleted in c4d5e6f; consolidated into ./reference.md. Updated.
  Committed: e7f8g9h0. heal status confirms fixed.
```

End with a session summary:

```
Doc cache drain: fixed 6 / skipped 1 / regressed 0 / 1 escalated.
Escalated: docs/concept.md doc_freshness (12 commits past doc) —
recommend running /heal-doc-review for proposal-level discussion.
```

## Constraints

- One finding = one commit. Don't bundle multiple findings.
- **Never push.** Local commits only; user runs `git push`.
- **Never amend.** New commit per finding is the contract.
- **Never `--no-verify`.** Fix the underlying issue or skip.
- **Never manufacture content.** Empty stubs to clear
  `doc_coverage` are forbidden (Coverage trap, §4).
- **Never harmonize at the cost of intentional drift.** Quotations,
  migration notes, CHANGELOG entries with old names stay (§5.6
  trap).
- **English commit messages.** The doc itself may be in any
  language; the commit message stays English (workflow.md R6.1).
