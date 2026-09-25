---
title: Features
description: heal observes three slices of code health — Code, Test, and Docs. Code ships on by default; Test and Docs are opt-in.
---

heal groups its observers into three feature families. **Code** is
always on — that's what `heal init` enables for every project.
**Test** and **Docs** are opt-in: turn them on in `.heal/config.toml`
when you're ready to surface those signals alongside the code
metrics. Each family has its own metrics, its own configuration
section, and a dedicated Claude skill that proposes fixes and
applies the ones you approve.

## Code (always on)

> _"Where is the codebase hard to change?"_

The default observer family. Seven metrics — LOC, CCN, Cognitive
Complexity, Churn, Change Coupling, Duplication, LCOM —
calibrated to the codebase's own distribution and surfaced through
`heal status`. The `🔥` Hotspot decoration highlights files that are
both complex and frequently touched: the historical concentration of
regressions.

| Page                                       | Read this when…                                                                   |
| ------------------------------------------ | --------------------------------------------------------------------------------- |
| [Configuration](/heal/code/configuration/) | You want to tune thresholds, add monorepo workspaces, or change the drain policy. |
| [Metrics](/heal/code/metrics/)             | You want to know what each metric means and how Severity is decided.              |
| [Skills](/heal/code/skills/)               | You want to drive heal from a Claude session — set up, review, refactor.          |

There's no flag to enable Code; `heal init` writes a config with
every observer turned on.

## Test (opt-in: `[features.test]`)

> _"Which production code is dark to the test suite, and which tests
> have drifted or are silently skipped?"_

Adds three test-quality observers and tags every reported item
with an `is_test_file` flag. The headline signal is **line
coverage**, read
from an `lcov.info` produced by your existing reporter (`cargo
llvm-cov`, `pytest --cov`, `nyc`, `scoverage`). Hotspot scoring
gains a multiplier for uncovered files, so files that change a lot
**and** lack coverage **and** are complex bubble to the top of the
drain queue. The post-commit nudge gains an "N uncovered hotspot"
line so you know where the next test should land.

| Page                                       | Read this when…                                                  |
| ------------------------------------------ | ---------------------------------------------------------------- |
| [Configuration](/heal/test/configuration/) | You're ready to enable the family or wire up an `lcov.info`.     |
| [Metrics](/heal/test/metrics/)             | You want to know what each test signal flags.                    |
| [Skills](/heal/test/skills/)               | You want Claude to review your test suite or fill coverage gaps. |

Enable with:

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

Or let the setup skill do it: it turns the family on, inspects your
stack, and wires up a coverage reporter when you don't already have an
`lcov.info` (CI changes stay a proposal). In Claude Code:

```
/heal:setup tests
```

## Docs (opt-in: `[features.docs]`)

> _"Which documentation has drifted from the code it describes?"_

Adds seven doc-quality observers that compare paired documentation
against its source: stale freshness, dangling identifiers, missing
pairs, broken internal links, orphan pages, TODO marker density,
plus a docs-family Hotspot composer. A small JSON file
(`.heal/doc_pairs.json`, generated once by `/heal:setup docs`)
maps each doc to the source it describes. The Markdown / RST
duplication pass turns on with this family too. Hotspot scoring
gains a multiplier when a file's paired doc is stale.

| Page                                       | Read this when…                                                         |
| ------------------------------------------ | ----------------------------------------------------------------------- |
| [Configuration](/heal/docs/configuration/) | You're ready to enable the family or want to understand the pairs file. |
| [Metrics](/heal/docs/metrics/)             | You want to know what each doc signal flags.                            |
| [Skills](/heal/docs/skills/)               | You want Claude to detect pairs, audit your docs, or apply fixes.       |

Enable with:

```toml
[features.docs]
enabled = true
```

Then run the docs step of the setup skill once (it can also flip the
switch above for you). It scans your source and doc trees, infers the
doc ⇔ source pairings, and writes `.heal/doc_pairs.json`. Without that
mapping the docs family has nothing to compare paired docs against, so
this step is part of turning the family on, not an optional add-on. In
Claude Code:

```
/heal:setup docs
```

## Semantic (opt-in: `[features.semantic]`)

> _"Does this code, test, or doc mean what it says?"_

Asks TypeSafe's Jev classifier questions that metrics cannot answer
and adds the answers to the Code, Test, and Docs families. It is the
only part of heal that sends content over the network, and only when
you run `heal semantic ask`. See [Semantic (Jev)](/heal/semantic/)
for what is sent, the API key, and cost.

```toml
[features.semantic]
enabled = true
```

## Picking what to enable

A typical adoption order:

1. **Start with Code.** Run `heal init`, then work through the
   findings with `/heal:refactor`. Once `Critical 🔥` is at zero, you
   have a baseline.
2. **Add Test next** if you have (or can produce) an `lcov.info`.
   `coverage_pct` and `skip_ratio` reports turn "we should add
   tests" into a ranked queue.
3. **Add Docs last** when documentation drift is a recurring
   surprise. Layer A pairing needs one upfront pass through
   `/heal:setup docs`; after that, the doc family runs on
   every `heal status`.

Either opt-in family can be turned off later — set `enabled =
false` and the next `heal status --refresh` removes those items
from the TODO list. Re-enabling brings them back without
re-calibration.
