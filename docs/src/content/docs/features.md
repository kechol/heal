---
title: Features
description: heal observes three slices of code health — Code, Test, and Docs. Code ships on by default; Test and Docs are opt-in.
---

heal groups its observers into three feature families. **Code** is
always on — that's what `heal init` enables for every project.
**Test** and **Docs** are opt-in: turn them on in `.heal/config.toml`
when you're ready to surface those signals alongside the code
metrics. Each family has its own metrics, its own configuration
section, and a dedicated pair of Claude skills (one to review, one
to apply fixes).

## Code (always on)

> _"Where is the codebase hard to change?"_

The default observer family. Seven metrics — LOC, CCN, Cognitive
Complexity, Churn, Change Coupling, Duplication, Hotspot, LCOM —
calibrated to the codebase's own distribution and surfaced through
`heal status`. The `🔥` Hotspot decoration highlights files that are
both complex and frequently touched: the historical concentration of
regressions.

| Page | Read this when… |
|---|---|
| [Configuration](/heal/code/configuration/) | You want to tune thresholds, add monorepo workspaces, or change the drain policy. |
| [Metrics](/heal/code/metrics/) | You want to know what each metric means and how Severity is decided. |
| [Skills](/heal/code/skills/) | You want to drive heal from a Claude session — review, drain, configure. |

There's no flag to enable Code; `heal init` writes a config with
every observer turned on.

## Test (opt-in: `[features.test]`)

> _"Which production code is dark to the test suite, and which tests
> have drifted or are silently skipped?"_

Adds three test-quality observers and tags every Finding with an
`is_test_file` flag. The headline signal is **line coverage**, read
from an `lcov.info` produced by your existing reporter (`cargo
llvm-cov`, `pytest --cov`, `nyc`, `scoverage`). Hotspot scoring
gains a multiplier for uncovered files, so files that change a lot
**and** lack coverage **and** are complex bubble to the top of the
drain queue. The post-commit nudge gains an "N uncovered hotspot"
line so you know where the next test should land.

| Page | Read this when… |
|---|---|
| [Configuration](/heal/test/configuration/) | You're ready to enable the family or wire up an `lcov.info`. |
| [Metrics](/heal/test/metrics/) | You want to know what each test signal flags. |
| [Skills](/heal/test/skills/) | You want Claude to review your test suite or fill coverage gaps. |

Enable with:

```toml
[features.test]
enabled = true

[features.test.coverage]
enabled = true
```

If you don't have an `lcov.info` yet, run `/heal-test-reporter-setup`
— it inspects your stack and proposes the reporter wiring.

## Docs (opt-in: `[features.docs]`)

> _"Which documentation has drifted from the code it describes?"_

Adds six doc-quality observers that compare paired documentation
against its source: stale freshness, dangling identifiers, missing
pairs, broken internal links, orphan pages, TODO marker density. A
small JSON file (`.heal/doc_pairs.json`, generated once by
`/heal-doc-pair-setup`) maps each doc to the source it describes.
The Markdown / RST duplication pass turns on with this family too.
Hotspot scoring gains a multiplier when a file's paired doc is
stale.

| Page | Read this when… |
|---|---|
| [Configuration](/heal/docs/configuration/) | You're ready to enable the family or want to understand the pairs file. |
| [Metrics](/heal/docs/metrics/) | You want to know what each doc signal flags. |
| [Skills](/heal/docs/skills/) | You want Claude to detect pairs, audit your docs, or apply fixes. |

Enable with:

```toml
[features.docs]
enabled = true
```

External HTTP link checking and example-code execution stay out of
scope — heal is local-only. CI tools like `lychee` cover those.

## Picking what to enable

A typical adoption order:

1. **Start with Code.** Run `heal init`, audit with
   `/heal-code-review`, drain with `/heal-code-patch`. Once
   `Critical 🔥` is at zero, you have a baseline.
2. **Add Test next** if you have (or can produce) an `lcov.info`.
   `coverage_pct` and `skip_ratio` Findings turn "we should add
   tests" into a ranked queue.
3. **Add Docs last** when documentation drift is a recurring
   surprise. Layer A pairing needs one upfront pass through
   `/heal-doc-pair-setup`; after that, the doc family runs on
   every `heal status`.

Either opt-in family can be turned off later — set `enabled =
false` and the next `heal status --refresh` drops those findings
from the cache. Re-enabling brings them back without
re-calibration.
