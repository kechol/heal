# heal

> **h**ook-driven **e**valuation & **a**utonomous **l**oop — heal watches
> your codebase decay between commits and hands the next refactor to
> your AI coding agent, one commit per approved fix.

AI coding agents are reactive: they wait for a human to file the next
task. Meanwhile, codebases decay — complexity creeps, hotspots shift,
duplicates pile up. heal closes that gap. Every commit it re-measures
the codebase and produces a Tier/Severity/score-ranked TODO list your agent can
drain — no human in the polling path.

Documentation: <https://kechol.github.io/heal/>

## Supported languages

heal supports six languages out of the box —
**TypeScript / JavaScript / Python / Go / Scala / Rust**, all
bundled into the release binary.

| Metric                            | Languages                                                                                       |
| --------------------------------- | ----------------------------------------------------------------------------------------------- |
| LOC                               | Every language [`tokei`](https://github.com/XAMPPRocky/tokei) recognizes.                       |
| Churn / Change Coupling / Hotspot | Language-agnostic — driven by `git log`.                                                        |
| CCN / Cognitive / Duplication     | TypeScript / JavaScript / Python / Go / Scala / Rust.                                           |
| LCOM                              | TypeScript / JavaScript / Python / Rust. (Go has no class scope; Scala awaits the LSP backend.) |

The opt-in **Test** family runs on any language whose reporter
emits an `lcov.info`. The opt-in **Docs** family pairs Markdown /
RST docs against any of the six source languages above. Hotspot
composes complexity with churn, so on a language without a
tree-sitter grammar it falls back to a churn-only signal.

## Feature families

heal groups its observers into three families, plus an opt-in semantic layer. Each family carries
its own metrics, configuration block, and a Claude Code skill that
reads its findings, proposes changes, and applies the ones you
approve.

- **Code** (always on) — _"Where is the codebase hard to change?"_
  Seven metrics covering complexity, churn, duplication, and
  cohesion, plus a Hotspot decoration that highlights files that
  are both complex and frequently touched. Enabled by default after
  `heal init`.
- **Test** (opt-in via `[features.test]`) — _"Which production code
  is dark to the test suite, and which tests have drifted or are
  silently skipped?"_ Reads `lcov.info` from your existing reporter
  and adds a `test_hotspot` decoration so uncovered hot paths bubble
  to the top.
- **Docs** (opt-in via `[features.docs]`) — _"Which documentation
  has drifted from the code it describes?"_ Compares paired docs
  against their source files, flags broken internal links / orphan
  pages / TODO density, and adds a `doc_hotspot` decoration.

- **Semantic** (opt-in via `[features.semantic]`) — _"Does this code,
  test, or doc mean what it says?"_ Asks TypeSafe's
  [Jev](https://docs.typesafe.ai/) classifier typed questions — misnamed
  functions, files that mix concepts, tests that check nothing, docs
  that no longer match the code — and uses the answers to order the TODO
  list. The only part of heal that sends content over the network, and
  only when you run `heal semantic ask`. Details:
  [Semantic (Jev)](https://kechol.github.io/heal/semantic/).

Adoption order is usually Code first, then Test once you have (or
can produce) an `lcov.info`, then Docs once drift becomes a
recurring surprise. Details: [Features](https://kechol.github.io/heal/features/).

## Install

Pick whichever fits your environment.

```sh
# Homebrew (macOS / Linux)
brew install kechol/tap/heal-cli

# Cargo (Rust toolchain)
cargo install heal-cli

# Shell installer
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

The skills ship as a Claude Code plugin from this repository. In
Claude Code:

```
/plugin marketplace add kechol/heal
/plugin install heal@heal
```

Details: [Installation](https://kechol.github.io/heal/installation/).

## Quick Start

Inside any git repository:

```sh
heal init                      # set up .heal/, calibrate, install the post-commit hook
heal doctor                    # what is set up, what is left
heal status                    # render the Tier/Severity-ranked TODO list
```

Then, in Claude Code:

```
/heal:setup                    # tune strictness; optionally turn on Test / Docs / Semantic
/heal:refactor                 # propose refactors, apply the ones you approve — one commit each
/heal:docs                     # the same for docs ([features.docs])
/heal:tests                    # the same for tests ([features.test])
```

`/heal:setup` is safe to re-run: it starts from `heal doctor` and does
only what is missing — picks Strict / Default / Lenient, writes
`.heal/config.toml`, and builds what an opt-in family needs (doc pairs,
a coverage reporter, a concept list) when you turn it on.

When Test is enabled, HEAL distinguishes an explicit LCOV 0% result
from a production file the report never measured; the latter prompts a
reporter-scope check instead of creating a test-debt finding. Cache
freshness also includes enabled LCOV and doc-pair inputs, so changing an
ignored report invalidates the result even at the same commit. Within a
Tier and Severity, work is ordered by the relevant family Hotspot score;
scores are prioritization signals, not probabilities or guaranteed
payoff.

Full walkthrough: [Quick Start](https://kechol.github.io/heal/quick-start/).

## Documentation

Topical pages on the docs site:

- [Concept](https://kechol.github.io/heal/concept/) — design idea in three minutes
- [Features](https://kechol.github.io/heal/features/) — Code (always on), Test, Docs
- [CLI](https://kechol.github.io/heal/cli/) — every subcommand
- [Code › Metrics](https://kechol.github.io/heal/code/metrics/), [Code › Configuration](https://kechol.github.io/heal/code/configuration/), [Code › Skills](https://kechol.github.io/heal/code/skills/) — the always-on family
- [Test › Skills](https://kechol.github.io/heal/test/skills/), [Docs › Skills](https://kechol.github.io/heal/docs/skills/) — the opt-in families' skills
- [Architecture](https://kechol.github.io/heal/architecture/) — internals

Upgrading from heal 0.6 or earlier, which copied skills into each
project: install the plugin, then run `heal skills uninstall` in each
project and commit the deletion.

## Development

Standard workspace commands; CI runs all five on push / PR.

```sh
cargo build  --workspace
cargo test   --workspace
cargo fmt    --all
cargo clippy --workspace --all-targets -- -D warnings
cargo deny   check
```

Project conventions live in [`CLAUDE.md`](./CLAUDE.md).

## License

Dual-licensed under Apache-2.0 OR MIT
([LICENSE-APACHE](./LICENSE-APACHE), [LICENSE-MIT](./LICENSE-MIT)).
Contributions are dual-licensed unless stated otherwise.
