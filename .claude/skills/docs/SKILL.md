---
name: docs
description: Sync the heal repo's documentation with the current code. Reads every tracked Markdown / MDX page under `docs/src/` (Starlight site, en + ja mirror), `.claude/` (internal AI-agent reference and rules), and `README.md`, compares each claim against the live source tree, and rewrites stale or missing sections in place — preserving voice and structure, sweeping en + ja in lockstep, honoring the canonical-names contract in `.claude/docs/glossary.md`. Read-only on code; writes only to the doc files. Does NOT commit. Not for HEAL-finding triage (`/heal-doc-review`, `/heal-doc-patch`) or generating a fresh `.heal/docs/` scaffold (`/heal-doc-scaffold`) — those handle the observer family, not code → doc drift. Trigger on "/docs", "update the docs", "sync docs with code", "docs are stale, fix them", "rewrite outdated docs", "freshen the documentation".
---

# docs

Walk every doc surface in this repo, find what disagrees with the
current code, rewrite it in place. Read-only on code, write-only on
docs. Never commits (workflow.md R3).

Two audiences, two languages, one pass:

- **User docs** — `docs/src/content/docs/**`, `README.md`. Junior
  engineer audience. English root, Japanese mirror under `ja/`.
- **Internal docs** — `.claude/docs/**`, `.claude/rules/**`,
  `.claude/skills/**/SKILL.md`, `CLAUDE.md`. AI-agent audience.
  Always English (workflow.md R6.1). Capture decision rationale.

## Drift to look for

1. **Renamed identifier** — flag, JSON key, type, canonical term
   moved in code; docs cite the old name. Cross-check against
   `.claude/docs/glossary.md`.
2. **Removed surface** — subcommand, observer, knob, skill is
   gone; docs still mention it. `terminology.md` R3 is the floor.
3. **New surface, undocumented** — landed in code, never made it
   into docs.
4. **Wrong shape** — JSON schemas, `.heal/` layout, exit codes,
   config keys diverged.
5. **en / ja parity drift** — English updated, Japanese mirror
   didn't; or canonical names got translated (`Severity` → 重大度
   is the textbook bite, see `terminology.md` R7).
6. **Bit-rotted examples** — printed command output / file trees
   no longer match what the binary produces. Re-run, paste new.

Fix the smallest region containing the drift. A 90% right page
gets a 10% rewrite, not a tone overhaul.

## When NOT to use

- New page from scratch — propose first, align on placement /
  audience / voice.
- The `.heal/docs/` scaffold (HEAL's *output*) — that's
  `/heal-doc-scaffold`.
- `[features.docs]` findings on the docs site itself — that's
  `/heal-doc-review` / `/heal-doc-patch`.

## Output language

Identifiers stay verbatim across all surfaces — file paths,
`Finding.metric` strings (`change_coupling`, `doc_drift`, …),
command names (`heal status`), config keys (`[features.docs]`),
canonical types (`Finding`, `FindingsRecord`, `Severity`). The
glossary is law.

Per-file language (workflow.md R6):

| Path | Language |
|---|---|
| `README.md`, `CLAUDE.md` | English |
| `CHANGELOG.md` | Don't touch — owned by `/release` |
| `docs/src/content/docs/*.md(x)` (root) | English |
| `docs/src/content/docs/ja/**` | Japanese (native) |
| `.claude/docs/**`, `.claude/rules/**`, `.claude/skills/**/SKILL.md` | English |

Japanese-mirror prose must read native (terminology.md R7):
translate **meaning** not structure; keep canonical names verbatim
(`Severity`, `Hotspot`, `Critical`, command names, file paths); no
ASCII spaces around CJK characters in prose; prefer 「〜できます」
over 「〜することができます」; drop pronouns; neutral 丁寧語 only.

## Pre-flight

Refuse to start when:

- `.claude/docs/glossary.md` is unreadable (term contract).
- Repo is mid-merge or has conflict markers — surface and bail.

Dirty worktree is **not** a blocker — the user reviews the diff
before committing. Print `git status` once so they can see what
was already in flight.

If the user passed scope ("ja only", "README only", "internal
only"), narrow accordingly. No scope → sweep everything.

## Phase 1 — Inventory

```sh
git ls-files \
  'README.md' 'CLAUDE.md' \
  'docs/src/content/docs/**/*.md' 'docs/src/content/docs/**/*.mdx' \
  '.claude/docs/**/*.md' '.claude/rules/**/*.md' \
  '.claude/skills/**/SKILL.md'
```

Skip `CHANGELOG.md` (owned by `/release`). For each page, frontmatter
plus the first ~100 lines is enough to triage; pull the full body
only when triage flags drift.

## Phase 2 — Snapshot the code

You need a current-state map of the things docs claim. Build it
from source, not from memory. Use the **Explore subagent** for
breadth — spawn it once with a list of questions, get a structured
answer back, use that as the source of truth for diffing.

Where each claim category lives:

| Claim | Source |
|---|---|
| CLI subcommands, flags, exit codes | `cargo run -q -- --help` (and per-subcommand `--help`); `crates/cli/src/cli.rs` and `crates/cli/src/commands/` |
| JSON output shapes | Type definitions in `crates/cli/src/{config,findings,observer,…}.rs` + `crates/cli/tests/core_*.rs` |
| Schema versions | `FINDINGS_RECORD_VERSION`, `CONFIG_VERSION`, `CALIBRATION_VERSION` constants |
| Observers, metric strings | `crates/cli/src/observer/`; emitted `Finding.metric` values |
| Bundled skills | `.claude/skills/*/SKILL.md` frontmatter |
| Config keys | `crates/cli/src/config.rs` and `feature_*` modules |
| Canonical names | `.claude/docs/glossary.md` |
| Retired names (must NOT appear) | `.claude/rules/terminology.md` R3 |

Capture the snapshot as terse notes. Don't paste large code blocks
into chat — the notes are for your own diff reasoning.

## Phase 3 — Diff each doc against the snapshot

For each Phase-1 file, ask:

1. **Identifiers** — every cited CLI command, flag, JSON key,
   type, observer name, metric string, config key exists in the
   snapshot with the same spelling. Retired names are drift to fix
   (except in deliberately historical migration notes).
2. **Coverage** — for reference pages (CLI, metrics, config, skills
   list), every snapshot entry has at least one mention.
3. **Shape** — printed JSON, file trees, command output match
   what the binary produces today. Re-run the command if in doubt.
4. **Audience** — internal jargon (`FindingsRecord`, `config_hash`,
   "worktree clean") leaking into user docs is drift; user-friendly
   summaries hiding the precise contract in `.claude/docs/` is also
   drift (scope.md R10).
5. **en / ja parity** — for every English page under
   `docs/src/content/docs/*.md(x)`, the JA mirror at the same path
   exists, covers the same headings in the same order, and keeps
   canonical names verbatim.
6. **Tone** — internal docs terse and precise; user docs plain
   and welcoming; Japanese mirror native (terminology.md R7).

Build a per-file change list before any write. Example shape:

```
README.md
  L18  "Critical / High" → "Critical AND hotspot=true" (scope.md R1)
  L57  Skills list missing /heal-doc-scaffold

docs/src/content/docs/cli.md
  Add --feature docs flag; refresh exit-code table

docs/src/content/docs/ja/cli.md
  Mirror the above; sweep CJK spacing on the touched lines

.claude/docs/observers.md
  Add test_hotspot / doc_hotspot rows for v4 schema
```

Pages with zero drift: note and move on. Goodhart applies to docs
too — don't rewrite for its own sake.

## Phase 4 — Apply minimal rewrites

Edit one file at a time. Default unit is the smallest contiguous
region containing the drift, not the whole section.

Preserve, in priority order:

1. **Frontmatter** — `title`, `description`, ordering keys. Touch
   only if the title itself is wrong (renamed page).
2. **Heading hierarchy** — don't reorganize H2 / H3 unless drift
   forces it; reorganization breaks deep links.
3. **Voice and length** — terse stays terse, chatty stays chatty.
   Don't smuggle style changes under cover of fact fixes.
4. **Working examples** — leave them; only paste new output when
   it actually differs.

By surface:

- **Internal docs** — state the current contract precisely. When
  changing an invariant, add a one-line **why** (the doc says what
  *is*; the rule says what you may not change).
- **User docs** — explain user benefit, not mechanism. Reaching for
  `FindingsRecord` / `config_hash` / "worktree mode" → redirect to
  `.claude/docs/` with a one-sentence user summary.
- **SKILL.md bodies** — `description` frontmatter is the trigger
  contract; keep it lean and externally readable. Body changes
  follow the minimal-region rule.

## Phase 5 — Sweep en / ja parity

Every change to an English page is mirrored to
`docs/src/content/docs/ja/<same-path>` in the same pass
(terminology.md R2 / workflow.md R5). The split-into-follow-up-PR
pattern is the bug, not the cure.

Verification loop after Japanese edits:

```sh
# CJK spacing artifacts (ASCII space adjacent to a Japanese char in prose)
rg -n '[A-Za-z0-9] [぀-ヿ一-鿿]|[぀-ヿ一-鿿] [A-Za-z0-9]' \
  docs/src/content/docs/ja
# Force-translated canonical names that should have stayed verbatim
rg -n '重大度|発見事項|変更結合|凝集度の欠如' docs/src/content/docs/ja
```

Hits inside ` `code spans` ` are fine; hits in prose are drift —
fix and re-run until clean.

## Phase 6 — Tone and voice review

After all edits, re-read each touched file end-to-end (not
diff-only):

- **Internal docs** — every claim sourced; retired names gone;
  rationale present where invariants live; no marketing language.
- **User docs** — a junior engineer arriving from Google can
  follow each page; no internal jargon outside the architecture
  page.
- **README.md** — install snippet runs as written; supported-
  languages table matches `crates/cli/src/observer/`; doc links
  resolve to pages that still exist.
- **Japanese mirrors** — read aloud in your head. Common smells:
  「〜することができます」where 「〜できます」 fits;
  「あなたの」/「私たち」 (drop the pronoun);
  過剰敬語 (use neutral ます調); machine-translation spaces.

Re-run the Phase-5 verification one more time on whatever you
changed in Phase 6.

## Final output

One block per touched file:

```
README.md
  +2 -1   skills list now includes /heal-doc-scaffold
          metrics table aligned with scope.md R1

docs/src/content/docs/cli.md
  +14 -3  added --feature docs; refreshed exit-code table

docs/src/content/docs/ja/cli.md
  +14 -3  same as cli.md; CJK spacing sweep on L22, L47

.claude/docs/observers.md
  +9 -2   added test_hotspot / doc_hotspot for v4 schema
```

Then in one or two lines, surface drift you spotted but
deliberately did **not** fix (out of scope, ambiguous, needs the
user's call).

End with: changes are unstaged. The user runs `git diff` to review
and `/commit` when ready.

## Constraints

- Read-only on code; never modify outside the Phase-1 doc set.
- Never touch `CHANGELOG.md` (owned by `/release`).
- No `git add`, `git commit`, push, or PR. Per workflow.md R3 / R9,
  the user reviews and runs `/commit` themselves.
- No new top-level docs without asking — propose first, then
  create both en and ja in one pass.
- Don't write Japanese in `.claude/**` (R6.1) or leave
  English-only paragraphs in `ja/**`.
- Glossary is law. If code and `.claude/docs/glossary.md`
  disagree, fix the glossary in the same pass and propagate
  (terminology.md R2).
