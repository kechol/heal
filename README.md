# heal

> **h**ook-driven **e**valuation & **a**utonomous **l**oop — heal watches
> your codebase decay between commits and hands the next refactor to
> your AI coding agent, one fix per commit.

AI coding agents are reactive: they wait for a human to file the next
task. Meanwhile, codebases decay — complexity creeps, hotspots shift,
duplicates pile up. heal closes that gap. Every commit it re-measures
the codebase and produces a Severity-ranked TODO list your agent can
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

heal groups its observers into three families. Each family carries
its own metrics, configuration block, and pair of bundled skills
(one to review, one to patch).

- **Code** (always on) — _"Where is the codebase hard to change?"_
  Eight metrics covering complexity, churn, duplication, cohesion,
  and a Hotspot decoration that highlights files that are both
  complex and frequently touched. Enabled by default after
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

Details: [Installation](https://kechol.github.io/heal/installation/).

## Quick Start

Inside any git repository:

```sh
heal init                      # set up .heal/, calibrate, install hook, offer skills for each detected agent
claude /heal-setup             # tune strictness; optionally turn on Test / Docs
heal status                    # render the Severity-grouped TODO list
claude /heal-code-patch        # work through it, one fix per commit
```

`heal init` extracts the bundled skills into every supported agent
it detects on `PATH` — Claude Code (`.claude/skills/`) and OpenAI
Codex (`.agents/skills/`). Substitute `codex` for `claude` above if
that's your CLI.

`/heal-setup` is the first-run wizard: it surveys the codebase,
picks Strict / Default / Lenient, writes `.heal/config.toml`, and
chains into `/heal-doc-pair-setup` or `/heal-test-reporter-setup`
when you opt into either family.

Full walkthrough: [Quick Start](https://kechol.github.io/heal/quick-start/).

## Documentation

Topical pages on the docs site:

- [Concept](https://kechol.github.io/heal/concept/) — design idea in three minutes
- [Features](https://kechol.github.io/heal/features/) — Code (always on), Test, Docs
- [CLI](https://kechol.github.io/heal/cli/) — every subcommand
- [Code › Metrics](https://kechol.github.io/heal/code/metrics/), [Code › Configuration](https://kechol.github.io/heal/code/configuration/), [Code › Skills](https://kechol.github.io/heal/code/skills/) — the always-on family
- [Test › Skills](https://kechol.github.io/heal/test/skills/), [Docs › Skills](https://kechol.github.io/heal/docs/skills/) — the opt-in families' skills
- [Architecture](https://kechol.github.io/heal/architecture/) — internals

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
