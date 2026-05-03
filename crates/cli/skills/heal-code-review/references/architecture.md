# Architecture vocabulary for refactor proposals

Reference loaded by `heal-code-review` when proposing refactors. The
skill reaches for **established** vocabulary so suggestions land in
concepts the user can verify against the literature, not phrases
invented in the moment.

The reference is organised in increasing scope. Pick the smallest
layer that fits the finding — a single high-CCN function rarely
needs DDD vocabulary; a multi-file coupling cluster sometimes does.

1. **Module-level depth** — Ousterhout. The unit at which most
   findings act.
2. **Layered / hexagonal architecture** — Cockburn, Evans. The unit
   at which cross-module findings (`change_coupling`, hub files)
   often act.
3. **Domain-Driven Design** — Evans, Vernon. The unit for cross-
   cutting hotspots and bounded-context split candidates.

A fourth section, **Respecting the codebase**, is the contract any
proposal must pass before being recommended.

---

## 1. Module-level depth (Ousterhout)

From *A Philosophy of Software Design*. The most precise vocabulary
for the per-file refactors that drive most findings.

**Module** — anything with an interface and an implementation:
function, class, package, file. The unit of compositional thinking.

**Interface** — *everything callers must know* to use the module:
types, invariants, errors, ordering, side-effects, configuration.
The signature is part of the interface; so are documented
preconditions and the panics it can raise.

**Depth** — leverage at the interface. A *deep* module hides a lot
of behaviour behind a small interface; a *shallow* module's
interface is nearly as wide as its implementation. Deep modules are
the goal.

**Shallow-module test.** "Would deleting this module concentrate
complexity at the call sites (good — the module was hiding
something), or just relocate the same code (the module was
shallow)?" Any "Extract Function" proposal must pass this test.

**Information leakage.** When two modules share a non-trivial
design decision through their interfaces, change in one forces a
change in the other. This is what `change_coupling` measures
empirically.

**Pull complexity downward.** Given a choice between making the
caller's life easier (one extra parameter, one more error case) and
the implementer's life easier, pick the caller. Code is read more
than written; depth is a small interface paying for a complex
implementation.

**Mapping findings → depth.**
- A high-CCN / high-Cognitive function with mixed responsibilities
  → currently shallow (interface-by-side-effect). Extract Function
  deepens it: the new helper has a single name, a typed signature,
  and hides one coherent step.
- A pair flagged by `change_coupling` with no static dependency →
  there is a hidden interface between them. Naming it (Extract
  Class / Service Layer) makes the dependency visible.
- A class flagged by `lcom` → the module bundles two interfaces
  under one name. Splitting deepens both.

---

## 2. Layered & hexagonal architecture

From Evans (*Domain-Driven Design*) and Cockburn ("Hexagonal
Architecture", 2005), with later refinements (Onion — Palermo;
Clean — Martin). Vocabulary for the *cross-file* refactors
`change_coupling` findings often surface.

**The dependency rule.** Inner layers must not know about outer
layers. Domain depends on nothing; application depends on domain;
infrastructure depends on application; the entrypoint (CLI / HTTP
handler / job) depends on infrastructure.

**Domain layer.** Pure business rules, no I/O, no framework
imports. Tests run in milliseconds because there is nothing to mock.

**Application layer.** Orchestrates domain operations to fulfil a
use case. Knows *what* to do, not *how* to persist or render it.

**Infrastructure layer.** Concrete adapters for I/O — filesystem,
network, database, parser. Implements the interfaces the
application layer depends on.

**Interface / entrypoint layer.** CLI, HTTP handler, hook script,
job runner. The thinnest layer; parses input, dispatches one
application call, formats output.

**Ports & adapters (hexagonal).** A *port* is the interface the
inner layers depend on (`trait Storage`); an *adapter* is the
concrete implementation in the outer layer (`struct FsStorage`).
The seam runs through the trait / interface.

**Anti-corruption layer (ACL).** When two contexts must
communicate but use incompatible models, the ACL is the
translator. Protects the inner domain from leaking outer
terminology.

**Mapping findings → layers.**
- Cross-module `change_coupling` between `application/` and
  `infrastructure/` → the inner layer is depending on the wrong
  side of a port. The fix is to push the trait into the inner
  layer and have the outer side implement it.
- Coupling between `domain/` and `infrastructure/` → dependency-
  rule violation. Introduce a port in the domain layer and have
  the adapter implement it.
- A god-file at the centre of many couplings → almost always a
  facade that has accreted both application and infrastructure
  concerns. Split along the layer boundary.

**When this vocabulary does *not* apply.** Small projects, scripts,
single-purpose tools, libraries that have no I/O of their own. The
codebase you are reviewing may legitimately have only one or two
"layers" — don't propose a four-layer rewrite for a 5kloc tool. The
vocabulary is descriptive, not prescriptive.

---

## 3. Domain-Driven Design (Evans, Vernon)

From Evans (*Domain-Driven Design*, 2003) and Vernon
(*Implementing Domain-Driven Design*, 2013). Vocabulary for the
*architectural* findings — when the question is not "extract this
helper" but "is the boundary between A and B in the right place?"

**Ubiquitous language.** Names in the code match the names domain
experts use. A rename from `OrderProcessor` to `Checkout` because
that is what the business calls it is valid DDD work, not churn.

**Bounded context.** A region of the codebase where one model is
authoritative. Two contexts can use the same word (`User`) and
mean different things — the boundary is what stops the two models
leaking into each other.

**Aggregate.** A cluster of objects treated as a single unit for
the purpose of consistency. The aggregate root is the only entry
point; inner objects are reached through it. Aggregates draw a
transactional boundary — changes inside one aggregate are atomic;
changes across aggregates are eventually consistent.

**Entity vs value object.** An entity has identity that survives
mutation (`Order(id=42)` stays the same order even when its lines
change). A value object is defined by its attributes
(`Money(100, USD)` equals any other `Money(100, USD)`); replacing
it is conceptually free.

**Domain service.** Behaviour that does not naturally belong to
any single entity or value object. *Stateless*. If you find
yourself making it stateful, it probably belongs on an aggregate.

**Repository.** The collection-like interface for retrieving and
storing aggregates. Hides the persistence mechanism.

**Bounded-context split.** When two halves of a context evolve at
different rates, are touched by different teams, or use overlapping
language with subtly different rules, that is the seam. A
`change_coupling` finding spanning two would-be contexts is a
candidate signal — but confirm with the user before proposing it,
because the answer is often "yes, the boundary is correct and we
just have one cross-cutting story going through it."

**Strategic vs tactical patterns.** Tactical = aggregate, entity,
value object, service, repository. Strategic = bounded context,
context map, anti-corruption layer, shared kernel. Findings mostly
drive *tactical* changes; architectural findings (a hub file, a
multi-pair coupling cluster) sometimes raise strategic questions.
Frame those as questions, not refactors.

**When DDD vocabulary does *not* apply.** Codebases without a
non-trivial domain (a build script, a CLI tool, a parser library,
a UI shell). Don't impose `Aggregate` and `Repository` where there
is no business rule to encode. Stick to module-level vocabulary
(§1) instead. The presence of `controller` / `service` /
`repository` directories already in the tree is a strong hint that
DDD vocabulary fits; their absence is a strong hint that it does
not.

---

## 4. Respecting the codebase

A proposal is only useful if it fits the codebase as it exists.
Before recommending a refactor, satisfy these checks:

**Read the code first.** Open every file you intend to talk about.
The metric summary is a hint, not the diagnosis. A function might
be high-CCN because of an exhaustive `match` on a closed enum, in
which case the type-checker is the reason and decomposition would
hurt clarity.

**Match the existing style.** If the codebase is procedural, do
not propose introducing classes; if it is functional, do not
propose introducing inheritance; if it is OO with explicit DI, do
not propose ambient module-level singletons. The team's style is
load-bearing — fights against it produce churn, not improvement.

**Match the existing layering.** If the codebase already has
`domain/` `application/` `infrastructure/` directories, fit
proposals into that grammar. If it has a flat `src/` with a few
subfolders, do not invent a multi-layer hierarchy on the basis of
two findings.

**Avoid speculative future-proofing.** Don't add a port because a
second backend "might exist someday". Don't add a service layer
because the controller "might call multiple services". Each
abstraction must be paid for by the variation already in the code,
not by hypothetical variation.

**Trust internal invariants.** Validate at system boundaries (user
input, external APIs); do not propose adding defensive checks
between functions in the same module. A proposal that wraps a
typed call in `if value.is_valid()` is almost always wrong.

**No backwards-compatibility shims unless asked.** Renaming a
type or removing a field need not carry the old name as an alias.
If the change has no external consumer, just change it. If it
does, surface that as a question for the user.

**Three uses, not two.** Apply the *Rule of Three* (Fowler) to
duplication findings. The second occurrence is "wait"; only the
third has enough variation to inform a sound abstraction.
Premature extraction couples things that should evolve separately
and is more expensive to undo than the duplication is to live with.

**Respect generated and vendored code.** Parser tables,
schema-derived types, vendored dependencies, snapshot fixtures,
license headers — these are *not* defects when they score high.
Propose excluding them from observation rather than refactoring
them.

**Test ↔ implementation pairs are healthy.** A `change_coupling`
finding between `foo.rs` and `tests/foo.rs` is the metric working
as designed; do not "fix" it.

**Surface, don't decide.** When the right answer is a judgement
call (a coupling that could go either way; a renaming that the
team needs to align on; a context split that depends on roadmap),
present the trade-off and let the user decide. Do not auto-recommend.

---

## 5. Pattern leverage hierarchy

Patterns differ sharply in how much they shrink the global heal score.
Rule of thumb: patterns targeting `duplication` are *Goodhart-safe* (the
metric is the real target); patterns targeting `ccn` / `cognitive` alone
often only relocate complexity. When ranking proposals, pick the highest
tier that fits — leverage drops between tiers.

Names follow Fowler's *Refactoring* (2nd ed., 2018), with additions from
Hammant, Newman, and Evans for cross-module and strategic moves.

### Tier 1 — Duplication elimination (Goodhart-safe, highest leverage)

These patterns directly remove the thing the metric measures. heal's score
genuinely shrinks; new findings rarely appear in the helpers.

- **Form Template Method.** N call sites share the same shape; vary only a
  predicate, transform, or message. Collapse into one helper that takes the
  varying part as a parameter. Each caller becomes a declarative spec.
  Reduces global token count, CCN, and Cognitive simultaneously.
- **Pull Up Method / Pull Up Field / Pull Up Constructor Body.** Two
  parallel classes / files / sibling components have diverged accidentally.
  Hoist the common body to a shared parent or shared module. Removes
  connascence-by-replication.
- **Replace Conditional with Lookup Table / Map.** A cascading
  `if (x === "A") return 1; if (x === "B") return 2; …` becomes
  `LOOKUP[x]` with one fallback branch. Each entry's evolution is
  independent; CCN drops to ~3 regardless of table size.
- **Substitute Algorithm.** Two implementations achieve the same intent
  via different code paths — heal's `duplication` observer ships only
  Type-1 (verbatim) detection, so Type-2 / Type-3 clones go unflagged.
  When you spot one during exploration, replace one side with the other.
- **Consolidate Duplicate Conditional Fragments.** The same side-effect
  appears in every branch of an if-else. Hoist it outside the conditional.
- **Consolidate Conditional Expression.** Multiple if-checks return the
  same result. Combine into one boolean expression, then often Decompose
  Conditional with a named helper.

### Tier 2 — Structural division (moderate metric movement, large qualitative win)

These produce *deeper* modules (Ousterhout §1) — interface stays small while
implementation absorbs the variant behaviour. Metric improvement is moderate
but maintainability improvement is large.

- **Replace Conditional with Polymorphism.** A function branches on a type
  tag and each branch has meaningfully different behaviour. Split per type
  into separate components / classes / strategies. Each variant becomes a
  deep module hidden behind a thin dispatcher.
- **Replace Type Code with Subclasses / Strategy / State.** A class
  switches behaviour on a `kind: string` field. Extract one class per
  kind, polymorphic over the operation. Often reveals which fields belong
  with which kind (LCOM clusters become visible).
- **Extract Class.** A class has two cohesion clusters (`lcom >= 2`).
  Splitting along the seam usually drops `change_coupling` on surrounding
  modules at the same time.
- **Move Function / Move Field.** `change_coupling` shows two files
  always editing together because a function lives on the wrong side.
  Moving it removes the coupling.
- **Introduce Parameter Object.** A function takes 5+ parameters and
  callers always pass the same coherent group. The group is a hidden
  type. Naming it deepens the function's interface and often unlocks
  further moves.
- **Combine Functions into Class / Combine Functions into Transform.**
  Several free functions take the same data shape and compute related
  derived values. Group them as a class or as a build-once transform
  pipeline.

### Tier 3 — Naming and intermediate structure (readability win, modest metric movement)

These improve the code's vocabulary without major restructuring. Metric
movement is small but reader load drops.

- **Decompose Conditional.** Pull boolean composites into named helpers
  (`isExpired`, `hasOpenSession`). Useful when the composite carries a
  domain concept. Beware: in TypeScript / JavaScript, `||` and `??` count as decisions —
  helper extraction can relocate CCN if the helper itself contains a
  non-trivial chain.
- **Extract Variable / Introduce Variable.** Give an intermediate
  computation a name. Reduces cognitive load even when CCN is unchanged.
- **Replace Magic Number / String with Named Constant.** Doesn't move
  CCN; helps reader and gives a single edit point for changes.
- **Split Phase.** A function does two coherent phases in sequence
  (parse, then transform; collect, then aggregate). Split with an
  intermediate data structure between the phases. Cognitive on the
  orchestrator drops without N-way splitting.
- **Replace Inline Code with Function Call.** A snippet exists already
  as a named function elsewhere; replace the inlined copy with a call.
  Subset of duplication elimination but less mechanical.

### Tier 4 — Procedural decomposition (low leverage; relocate-trap risk)

Use sparingly. Often relocates rather than reduces — see §6.

- **Extract Function.** Pull out a coherent sub-block. Justifiable only
  when the original mixes responsibilities (a real seam exists). When
  the original is a single coherent procedure, Extract Function moves
  CCN from the caller to the callee without reducing global count.
- **Replace Nested Conditional with Guard Clauses.** Apply ONLY when the
  original is genuinely deeply nested (see `metrics.md`'s CCN-vs-Cognitive
  table and §6's reflexive guard-clause trap). On flat positive composites
  this refactoring is pure noise — or actively worse.
- **Replace Method with Method Object.** When a function has so many
  parameters that Introduce Parameter Object isn't enough, promote the
  whole function to a class. High cost; consider whether the
  parameter-list growth indicates a missing concept first.

### Tier 5 — Architectural / strategic (cross-module; not single-symbol)

When findings span layers, contexts, or a hub file, per-symbol patterns
don't fit. These operate at a coarser scope and require human judgement;
heal-code-review should *propose* them as questions, not auto-apply.

- **Strangler Fig** (Fowler / Newman). Replace a legacy subsystem
  incrementally by routing new functionality through a new
  implementation while old continues to serve existing flows. Used
  when in-place rewriting is too risky.
- **Branch by Abstraction** (Hammant). Introduce an interface,
  implement the new behaviour in parallel, switch call sites
  one-by-one, then remove the old implementation. Useful when changes
  span many files and continuous deployment is a constraint.
- **Parallel Change / Expand-Contract.** Add the new shape, migrate
  callers, remove the old shape — in three separate releases. The
  alternative to "big-bang" rename / signature changes.
- **Anti-Corruption Layer** (Evans). Two contexts must communicate but
  use incompatible models. The ACL is the translator; protects the
  inner domain from leaking outer terminology.
- **Bounded Context split** (Evans). A single context has accreted two
  domains. Draw the boundary, give each its own ubiquitous language.
  Surface as a question — the answer depends on roadmap and team
  ownership, not the code alone.
- **Split Hub File.** One file has 5+ `change_coupling` partners and
  acts as a facade. Decompose along the natural layer boundary
  (persistence vs application, view vs controller). The hub itself
  may become an empty re-export.
- **Introduce Port / push interface into the inner layer.** A
  `change_coupling` finding between `application/` and
  `infrastructure/` whose direction is wrong (inner depending on
  outer). The fix is hexagonal: the inner layer declares the trait;
  the outer layer implements it.

### Patterns that look helpful but rarely move heal score

These improve specific situations but do not address heal's metrics.
Don't propose them in response to a finding unless the user explicitly
asks.

- **Inline Function / Inline Variable.** The reverse of Extract; valid
  when an existing helper is shallow (interface ≈ implementation). The
  inlining reduces indirection but doesn't shrink the metric.
- **Slide Statements.** Reorders related statements to be adjacent.
  Reads cleaner; metric-neutral.
- **Encapsulate Field / Encapsulate Variable.** Replaces direct access
  with getter/setter. Modern languages with property syntax (Kotlin,
  C#, TypeScript accessors) make this largely automatic; heal won't
  notice.
- **Hide Delegate / Remove Middle Man.** Trade-off between exposing a
  collaborator and exposing its method. Doesn't directly affect
  duplication, CCN, or coupling at scale.
- **Rename Variable / Rename Function / Rename Field.** Improves
  ubiquitous-language alignment (DDD §3) but heal does not measure
  naming quality. Worth doing on the way past, not as a heal target.

---

## 6. Refactor traps to recognise

Three common failure modes when fixing findings mechanically. Recognise them
before they consume effort.

**The relocate trap.** Extract Function on a procedurally cohesive function
moves CCN from the original symbol to the new helper(s) without reducing
global count. Signal: after the refactor, the new helper itself appears as
critical or high in the cache, and the global severity counts barely move.
Diagnosis: the original complexity was *intrinsic* (a single coherent
pipeline / state machine / dispatcher), not symptomatic of mixed
responsibility. Action: stop splitting; accept the score; move to a
different finding.

**The reflexive guard-clause trap.** Converting `if (A && B && C) { ... }`
to `if (!A) return; if (!B) return; if (!C) return; ...` does **not** improve
Cognitive Complexity if the original was already flat (a single non-nested
`if`). It only inverts a positive composite predicate into a negative chain,
which often *increases* cognitive load — readers must mentally re-negate
each guard to reconstruct the rule. Apply guard clauses only when the
original is genuinely deeply-nested. Positive composite predicates are
usually clearer left as-is, optionally with a named boolean
(`const isRisky = ...`).

**The drain-to-zero trap.** Goodhart's Law: when a measure becomes a target,
it ceases to be a good measure. Do not aim to drain the cache to zero
critical findings. Beyond the symptomatic findings, the remainder are
intrinsic or cohesive — refactoring them would damage the code. Surface
them as deferred questions, propose `metrics.exclude_paths` for clear false
positives, and stop. ROI on heal-driven refactoring drops sharply after the
symptomatic findings are addressed.

**The data-shaped CCN false positive.** A function that exists to enumerate
fallback values — `String(row[a] ?? row[b] ?? "")` repeated for many fields,
or a `clsx(...)` call mapping booleans to class names — scores high CCN
because each `??` / `&&` is a decision point. But it is *data declaration*
shaped like control flow. Decomposing it into per-field helpers produces
shallow modules. Treat as intrinsic; consider excluding the symbol or
accepting the score.

---

## How `heal-code-review` should use this reference

When proposing a refactor:

1. Pick the **smallest** vocabulary layer that fits — module-level
   (§1) for per-file findings; layered (§2) for cross-module
   pairs; DDD (§3) only when the finding genuinely crosses a
   domain seam.
2. Validate against §4 *before* surfacing. A proposal that
   conflicts with the codebase's existing style is a question for
   the user, not a recommendation.
3. Apply §5 to rank between candidates — prefer high-leverage
   patterns (Form Template Method, Pull Up Method) over
   low-leverage ones (Extract Function), and warn the user when
   a §6 trap is likely.
4. Name the pattern with its established term (Extract Function,
   Anti-Corruption Layer, Aggregate split). Do not invent new
   words.
5. If the diagnosis is uncertain, present it as a *grilling
   question* — "Is `app/orders/service.ts` really one service, or
   has it accreted two unrelated workflows?" — and let the user
   answer before acting.
