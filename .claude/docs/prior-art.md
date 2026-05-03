# Prior art

HEAL stands on the shoulders of a small number of tools, papers, and books.
This page records what was borrowed, what was deliberately rebuilt, what
was considered and rejected, and what was deliberately left out — so future
changes know which lineage to respect and which to ignore.

If you are tempted to add a "while I'm here, let's also do X" feature,
check whether X is on the **deliberately out** list before opening a PR.

The split:

- §1 — runtime dependencies (we link the crate).
- §2 — conceptual ancestors we re-implemented.
- §3 — papers and books behind the metric definitions.
- §4 — refactoring / architecture vocabulary used by `heal-code-review`.
- §5 — tools we evaluated and chose **not** to adopt (the rejection
  is the load-bearing part — don't re-litigate without reason).
- §6 — deliberately out of scope.

---

## 1. Tools we depend on at runtime

### [`tokei`](https://github.com/XAMPPRocky/tokei) (crate)

Used directly via the `tokei` crate for LOC counting in
`observer/loc.rs`. We picked tokei over rolling our own because:

- Mature multi-language line classification (code / comment / blank) that
  we are not going to outdo.
- Honors `.gitignore` and exposes language statistics in one call.

What we layered on top: `is_tokei_substring_safe` (`loc.rs:178`) routes
exclude patterns either inline (cheap) or to a post-walk re-application
(when the pattern uses gitignore features tokei can't represent — globs,
anchors, negation). Preserves the full gitignore DSL without paying for
a second walk on simple patterns.

### [`tree-sitter`](https://github.com/tree-sitter/tree-sitter) (crate + grammars)

Direct dependency. Every parsing observer (Complexity, Duplication, LCOM)
shares one tree-sitter parse per file. Per-language `.scm` queries live
under `crates/cli/src/observer/complexity/queries/<lang>/`
(`functions.scm`, `ccn.scm`, `cognitive.scm`) and
`crates/cli/src/observer/lcom/queries/<lang>/lcom.scm`.

Why tree-sitter over LSP / language-server protocols:

- **Local, no daemon.** Determinism per commit + config + calibration
  (see `scope.md` R2) requires zero external state.
- **Cheap to add a language.** A new language is a grammar dep + four
  query files, not a server integration.
- **Good enough** for syntactic metrics. Type-aware analysis is reserved
  for the v0.5+ LSP backend (`scope.md` R5; `data-model.md` reserves
  `[metrics.lcom] backend = "lsp"`).

### [`git2`](https://docs.rs/git2) (libgit2 binding)

Underlying engine for Churn and Change Coupling revwalks. Single
notable convention: **diff each commit against its first parent only**
(`observer/churn.rs:137`) so merge commits don't double-count.

---

## 2. Conceptual prior art (we re-implemented, didn't import)

### [`code-maat`](https://github.com/adamtornhill/code-maat) — Adam Tornhill

The intellectual ancestor of HEAL's churn / change-coupling /
hotspot pipeline. We re-implemented in Rust against `git2` rather than
shelling out to code-maat (JVM dep, log-file workflow, single-repo
shape). What we took:

- **Hotspot = churn × complexity** as a multiplicative composite, not
  additive. A file high on one axis only gets a modest score; both
  axes high gets a large score. See `observer/hotspot.rs` and
  `observers.md` "Why multiplicative not additive".
- **Change coupling with lift, not raw co-change.**
  `lift = pair_count × commits / (count_a × count_b)`. Lift ≥ 2.0 is
  the conventional "interesting" threshold; 1.0 is the chance baseline.
  See `observer/change_coupling.rs` step 4.
- **First-parent revwalk** to keep merges from inflating churn.

What we did differently:

- **PairClass demotion** (Lockfile / Generated / Manifest / TestSrc /
  DocSrc / Genuine — `scope.md` R9) is HEAL's own. Without it,
  lockfile bumps and mass-renames dominate the drain queue. code-maat
  leaves filtering to the operator.
- **Bulk-commit cap** (`BULK_COMMIT_FILE_LIMIT = 50`) suppresses
  quadratic blow-up on mass renames. Churn still counts the commit;
  ChangeCoupling skips it.

### [`lizard`](https://github.com/terryyin/lizard) — Terry Yin

Multi-language CCN reference. Same shape of rule:
`CCN(scope) = 1 + count(decision points)`. We follow the spirit but:

- Parse via tree-sitter, not regex / lexer-per-language. Means new
  languages cost a `ccn.scm` query, not a parser fork.
- Logical operator handling is conservative: `&&`, `||`, `??` count
  **only** when the parent is `binary_expression` and the operator
  field matches (`complexity/ccn.rs`). Other binaries don't increment.

### [`rust-code-analysis`](https://github.com/mozilla/rust-code-analysis) — Mozilla

A peer in spirit (multi-language, tree-sitter-based, computes CCN +
Cognitive + Halstead + others). Confirms the design choice that **one
tree-sitter pass can yield CCN and Cognitive simultaneously**
(`observer/complexity/`).

We deliberately do **not** depend on it as a library — see §5.

### [`jscpd`](https://github.com/kucherenko/jscpd), PMD CPD, Simian

Reference implementations for type-1 token duplication via Rabin-Karp
fingerprinting. The 50-token minimum window
(`metrics.duplication.min_tokens` default) follows their consensus
default — small enough to catch real clones, large enough to filter
imports / type annotations / boilerplate one-liners. See
`observer/duplication.rs`.

---

## 3. Papers and books behind the metric definitions

### McCabe Cyclomatic Complexity (1976) + NIST SP 500-235

The original paper defines CCN over a control-flow graph. We use the
syntactic equivalent (`1 + decision-point count` over the AST) which
matches CCN under structured control flow. Difference is irrelevant
in practice for the languages we support. NIST SP 500-235 codifies
the threshold buckets (≤10 simple, 11–20 moderate, 21–50 high, >50
untestable) that informed `FLOOR_CCN = 25` and `FLOOR_OK_CCN = 11`
in `complexity/ccn.rs`.

### Sonar Cognitive Complexity (Campbell, 2017)

Source: <https://www.sonarsource.com/resources/cognitive-complexity/>

Our `complexity/cognitive.rs` implements the paper's B1/B2/B3 rules:

- B1 increment per control-flow break.
- B2 nesting bonus = current depth on breaks inside nesting.
- B3 no-bonus-for-bare-`else`; `else if` is +1 with no nesting bonus.
- Logical chain handling: +1 per chain, +1 per operator-kind switch.

We do **not** invent new categories on top — staying paper-faithful
keeps cross-tool comparison meaningful.

### LCOM family — Chidamber & Kemerer (1991), Henderson-Sellers, Hitz & Montazeri (LCOM4)

- Chidamber & Kemerer introduced LCOM as the original CK suite metric.
- Henderson-Sellers refined the formulation to address pathological
  zero / negative cases.
- **Hitz & Montazeri's LCOM4** — connected components in the
  field-and-call graph — is what HEAL approximates. See
  `observer/lcom.rs` and `observers.md` LCOM "Algorithm".

The shipped observer is a **syntactic approximation**: inherited
fields, dynamic property access (`this[name]`), and helper-mediated
state-sharing are invisible. Bias is toward over-reporting; treat
LCOM findings as candidates for review, not verdicts. The `lsp`
backend reserved in `data-model.md` exists precisely to close this
gap when needed.

### _Your Code as a Crime Scene_ — Adam Tornhill (Pragmatic Bookshelf, 2nd ed.)

Source of the "true risk = volatility × complexity" framing that
HEAL's hotspot decoration formalizes. The book also motivates the
churn-as-leading-indicator stance: **what changes a lot is what hurts
when it's wrong**. Roughly: empirically a small fraction of files
account for the majority of bug fixes — that's the population worth
attacking first.

### _Software Design X-Rays_ — Adam Tornhill (Pragmatic Bookshelf, 2018)

Companion volume. Source of the change-coupling-as-design-feedback
stance: pairs that change together but live apart are a coupling
smell. Drives the `Symmetric` vs. `OneWay` direction split in
`observer/change_coupling.rs` step 5. Tornhill also frames LCOM as
the **internal companion** to change coupling — coupling reveals
inter-file split candidates, LCOM reveals intra-file ones. That
framing is reflected verbatim in
`crates/cli/skills/heal-code-review/references/metrics.md`.

---

## 4. Refactoring and architecture vocabulary (heal-code-review)

`heal-code-review` proposes refactorings using a named vocabulary so
findings translate into actions a developer recognises. The
references live in
`crates/cli/skills/heal-code-review/references/architecture.md`
and `references/readability.md`. Sources:

### _Refactoring_ (2nd ed., 2018) — Martin Fowler

Source of nearly every Tier-1–4 pattern name in `architecture.md` §5
(Form Template Method, Pull Up Method, Replace Conditional with
Polymorphism, Extract Class, Decompose Conditional, Extract Function,
…) and the **Rule of Three** discipline applied to duplication
findings (don't extract on the second occurrence — wait for the
third).

### _A Philosophy of Software Design_ (Ousterhout, 2018)

Source of the **deep modules** framing in `readability.md` §2.2 and
the Tier-2 ranking ("structural division produces deeper modules").
Underpins the heuristic that interface width should stay small while
implementation absorbs variant behaviour — the inverse of the
"Extract Function for its own sake" relocate-trap.

### _Domain-Driven Design_ (Evans, 2003) + _Implementing DDD_ (Vernon, 2013)

Source of **Bounded Context**, **Anti-Corruption Layer**, ubiquitous
language, and the rule that DDD vocabulary applies only when the
codebase has a **non-trivial domain** (CRUD-shaped projects don't
benefit). See `architecture.md` §3 for the gating heuristic.

### Hexagonal Architecture (Cockburn)

Layered / ports-and-adapters terminology in `architecture.md` §2.
Used to describe domain-vs-infrastructure split when proposing module
moves.

### Strangler Fig (Fowler / Newman) and Branch by Abstraction (Hammant)

Tier-5 strategic patterns in `architecture.md` §5. Surfaced as
**questions**, not auto-applied — the gating decision is roadmap and
risk, not metrics.

### Parallel Change / Expand-Contract

Standard release-discipline pattern for large renames or signature
changes. The `/release` skill assumes this discipline (the trailing
`!` in commit types marks the contract change; the contract change is
allowed to land in one PR because the prior expand step has already
shipped).

---

## 5. Tools we evaluated and didn't adopt

The rejection is the load-bearing part — re-evaluating these is
fine, but don't reintroduce the dependency without explicitly
overturning the rationale.

### `rust-code-analysis` as a library dependency

**Decision:** consult, don't depend.

**Reason:** the crate has been at `0.0.25` since early 2023 with
sparse activity. As a *core* dependency feeding every parsing
observer, the bus factor and release cadence are too low. We use it
for cross-checking metric definitions in code review, not as a
runtime input. Re-implementing CCN + Cognitive + LCOM directly on
tree-sitter queries is also cheaper to extend per-language than
forking a fixed-language grammar set.

### Maintainability Index (`code-health-meter` and friends)

**Decision:** don't ship a composite MI score.

**Reason:** MI = `f(Halstead, CCN, SLOC)` collapses three weakly-
correlated proxies into one number that is less actionable than the
inputs. A user can't tell whether a low MI means "too long",
"branchy", or "high vocabulary". HEAL surfaces the inputs separately
and lets the drain target be **Critical AND `hotspot=true`** rather
than a derived index. Goodhart's Law is sharper for composites than
for raw proxies.

### Halstead volume / vocabulary / effort

**Decision:** don't ship.

**Reason:** Reading the Halstead formula does not predict whether a
file is hard to change. The three Halstead numbers are correlated
proxies for "code is large" — they add noise without lift over CCN
+ Cognitive + LCOM. Same Goodhart risk as MI without the composite
disguise.

### `scc` / `cloc` for LOC counting

**Decision:** stay on `tokei`.

**Reason:** all three are competent multi-language line counters.
`tokei` ships a stable Rust library API; `scc` and `cloc` are
primarily CLIs we'd shell out to. Library API + Rust dep is a
strictly better fit for an in-process pipeline.

### Type-2 / type-3 duplication detection

**Decision:** stay type-1.

**Reason:** parameterized / near-duplicate clones generate enough
false positives at scale that the value-vs-noise ratio collapses
below the type-1 baseline. PMD CPD's experience reflects this. See
`observers.md` Duplication "Quirks".

---

## 6. Deliberately out of scope

These look like obvious extensions but were considered and ruled out.
Don't propose them without explicit roadmap discussion (`scope.md` R5):

- **Doc-skew / doc-coverage observers.** Tracking comment / docstring
  freshness against code is interesting but generates a lot of "your
  doc is stale, but is it though?" noise.
- **LSP-based metrics.** Reserved for v0.5+ as an opt-in backend
  (`data-model.md` `[metrics.lcom] backend = "lsp"`). The current
  tree-sitter approximation is wrong on type-resolved patterns
  (inheritance, dynamic dispatch) and we accept that for now.
- **Persistent metrics history.** No `snapshots/`, no rolling
  delta-vs-previous-run field. Drift is served by `heal diff <ref>`
  on demand. See `scope.md` R2 for the determinism rationale and
  R4 for the single-record cache contract.
- **Cloud sync, telemetry, version-check pings.** HEAL is local-only.
  The only network access is `git2` against the local repo.
- **Multi-agent provider abstraction.** Skills target Claude Code in
  v0.x; non-Claude skill bodies are a v0.5+ discussion.
- **Plugin marketplace / per-skill version pinning.** Skills are
  bundled inside the binary (`include_dir!`). Users update skills by
  upgrading `heal-cli`. No `heal skills add <url>`, no registry, no
  pinning. (`scope.md` R7.)

---

## When adding a new observer

If you're proposing a new metric, the bar is roughly:

1. **Cite the source.** Paper, book, or established tool. Pure
   invention is a hard sell — too easy to optimize for the metric
   instead of the underlying friction (`scope.md` R1).
2. **Explain what friction it predicts** in the user's day:
   hard-to-test? hard-to-read? hard-to-change? If you can't name
   the friction, the metric is decoration.
3. **Re-implement, don't shell out.** New runtime deps are a license
   discussion (`deny.toml`, `CLAUDE.md` "Dependencies have license
   consequences"). Re-implementing in Rust against tree-sitter +
   git2 is the established pattern.
4. **Add the citation here**, with a short note on what was borrowed
   vs. what was changed.
