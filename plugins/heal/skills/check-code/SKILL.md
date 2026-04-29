---
name: check-code
description: Read-only review of code health metrics (hotspot, complexity, duplication). Trigger when the user asks "what's unhealthy in the codebase", "any hotspots", or after `heal status` shows new findings. Output is a prioritized list grouped by hotspot rank — never modify code.
---

# check-code

Read `.heal/snapshots/*.jsonl` (latest snapshot) via `heal status --json`
and report the top findings by **hotspot score = churn × complexity**.

## Procedure

1. Run `heal status --json` to load the latest metric summary.
2. Surface in this order:
   - New top-3 hotspot entries (rank change since last snapshot).
   - CCN delta > 30% on any file present in both snapshots.
   - Duplication rate increase > 1pt.
3. For each finding, print:
   - File path
   - Metric (hotspot / ccn / duplication)
   - Magnitude (current value, delta vs previous)
   - One-sentence rationale ("complex + frequently changed" / "newly introduced cluster" / etc.)

## Constraints

- This skill is **read-only**. Do not edit files, do not run formatters.
- Don't propose fixes inline; the user invokes `run-code-*` skills (v0.2+) when ready.
- Cap output at 10 findings; if more exist, summarize the rest.
