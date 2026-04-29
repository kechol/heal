---
name: check-complexity
description: Find the most complex functions by Cyclomatic (CCN) and Cognitive Complexity, explain the risk in plain language, and propose specific decompositions grounded in established refactoring practice. Trigger on "any complex functions?", "what's hard to read?", "high CCN?", or splitting candidates. Read-only — proposes refactors without applying them.
---

# check-complexity

## Why these metrics matter

**Cyclomatic Complexity (CCN)** — McCabe (1976) — counts independent
paths through a function (decision points + 1). It maps directly to
the minimum number of test cases needed for branch coverage.
Industry rules of thumb (NIST SP 500-235, SonarQube defaults):

| CCN range | Risk |
|-----------|------|
| 1–10      | Simple, low risk |
| 11–20     | Moderate; review for clarity |
| 21–50     | High risk; difficult to test |
| 51+       | Untestable in practice; refactor |

**Cognitive Complexity** — G. Ann Campbell, SonarSource (2017) —
measures *how hard a function is to understand* by penalising nesting
and breaks in linear flow. Unlike CCN, a long flat `if/else if/else`
chain doesn't compound — but a deeply nested one does. Threshold ~15
flags review; ~25 flags refactor priority.

A function high in **both** is the strongest candidate: lots of paths
*and* hard to keep in your head while reading.

## Procedure

1. Run `heal status --metric complexity --json`.
2. Read `worst.ccn` and `worst.cognitive` (each precomputed to
   `metrics.top_n_complexity` from `.heal/config.toml`, exposed as
   `top_n` in the JSON). Default to top **3** even when `top_n` is
   larger — focus beats breadth for an unfamiliar reader. Mark any
   function appearing in **both** lists as the strongest candidate.
3. For each, **read** the function and:
   - Summarize what it does in 1–2 sentences.
   - Identify the **structural source** of complexity:
     - Nested control flow → cognitive load dominates
     - Long flat dispatch (switch / match) → CCN dominates, less harmful
     - Mixed responsibilities (parse + validate + execute + format)
       → Extract Function candidate
     - Flag-driven branches (`if (mode == X) ... else if (mode == Y)`)
       → polymorphism candidate
4. Propose a specific refactor, naming the pattern:
   - **Extract Function** (Fowler): pull a coherent sub-block into a
     named helper. Most common, lowest-risk.
   - **Replace Nested Conditional with Guard Clauses**: invert the
     leading `if`s and `return` early. Often halves cognitive
     complexity with no behavioural change.
   - **Decompose Conditional**: extract the predicate of a long `if`
     into a named boolean (`is_valid_input(x)`) — improves readability
     without changing structure.
   - **Replace Conditional with Polymorphism / Strategy**: for
     type-discriminating switches that grow per feature.
   - **Replace Conditional with Lookup Table**: for static value→value
     mappings hidden inside an `if` chain.
   - **Introduce Parameter Object**: when many parameters drive the
     branching.
5. Show numbers compactly: `file:line  function-name  CCN=22  Cog=31`.
6. If `delta.complexity.new_top_ccn` or `new_top_cognitive` is
   non-empty, those functions just entered the danger zone — call
   them out as fresh, since prevention is cheaper than cure.
7. If `complexity.spike` fired (max_ccn jumped beyond
   `ccn.warn_delta_pct`, default 30%), say so — a single commit
   regressed complexity meaningfully.
8. Close: "want me to draft a refactor diff for any of these?
   v0.2 `run-code-complexity` will automate this."

## When NOT to act

- Generated code (parser tables, AST visitors, code-gen output): high
  CCN is the cost of the abstraction, not a defect.
- Pattern-matching exhaustive `match` arms (especially in Rust): a
  large `match` over a closed set of variants is intentional and
  type-checked — splitting it can hurt clarity. CCN flags it; ignore.
- Mathematical / algorithmic kernels: a tight algorithm sometimes
  *needs* its conditionals (e.g. a state machine, a parser). Confirm
  with the user before recommending decomposition.

## Constraints

- Read freely; do not edit.
- Always pair function name with **file path** — names collide.
- Default 3 entries; expand on request.
- v0.1 measures only TS/JS and Rust. Other languages return empty
  reports; say so plainly when relevant.
