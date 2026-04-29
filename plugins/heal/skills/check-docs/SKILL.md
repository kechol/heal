---
name: check-docs
description: Read-only audit of documentation coverage and freshness. Trigger when the user asks "are the docs up to date", "doc skew", or before a release where docs need to track recent changes. Reports pairs of (source, doc) where the doc is older than configured `max_skew_days` — never modify docs.
---

# check-docs

Read `.heal/snapshots/*.jsonl` for the latest doc-coverage and doc-update-skew
observations and report which docs are stale or missing.

## Procedure

1. Run `heal status --json` to load the latest metric summary.
2. Find files whose source was modified after the linked doc by more than
   `metrics.doc_update_skew.max_skew_days` (config setting).
3. For each finding, print:
   - Doc path
   - Linked source path
   - Skew in days
   - Link to the corresponding source mtime / commit
4. Separately list public APIs that have no doc at all (Doc Coverage misses).

## Constraints

- This skill is **read-only**. Do not edit docs, do not generate new content.
- Don't fabricate links: if the config doesn't define a src↔doc mapping for a
  file, flag it as "no mapping configured" instead of guessing.
- The corresponding *fix* skill (`run-doc-dedupe`, doc-patch) lands in v0.3+.
