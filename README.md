# heal

> **h**ook-driven **e**valuation & **a**utonomous **l**oop — a code-health
> harness that turns codebase decay signals into work for AI coding agents.

LLM coding agents are usually reactive: a human files a task before the
agent moves. Codebases, meanwhile, decay continuously — complexity
creeps, hotspots shift, duplicates accumulate. heal closes that gap
with an **observe → calibrate → status → drain** loop, turning codebase
state changes into agent triggers.

Documentation: <https://kechol.github.io/heal/>

## Supported languages

| Metric                                          | Languages                                                  |
| ----------------------------------------------- | ---------------------------------------------------------- |
| LOC                                             | Every language [`tokei`](https://github.com/XAMPPRocky/tokei) recognizes. |
| Churn / Change Coupling / Hotspot               | Language-agnostic — driven by `git log`, applies everywhere. |
| Complexity (CCN + Cognitive) / Duplication       | TypeScript / JavaScript / Python / Go / Scala / Rust.      |
| LCOM                                            | TypeScript / JavaScript / Python / Rust. (Go has no class scope; Scala awaits the LSP backend.) |
| Docs (drift / freshness / coverage / link health / orphans / TODO density / hotspot) | Markdown / RST docs paired against the same set of source languages. Off by default — enable via `[features.docs]` in `.heal/config.toml`. |
| Test (coverage / skip ratio / hotspot)         | Any language whose reporter emits an `lcov.info`. Off by default — enable via `[features.test]` in `.heal/config.toml`. |

Hotspot composes complexity with churn, so on a language without a
tree-sitter grammar enabled it falls back to a churn-only signal.

> ⚠️ **Status: v0.3 — `[features.docs]` and `[features.test]` are
> opt-in beta.** macOS / Linux only.

## Install

Pick whichever fits your environment.

```sh
brew install kechol/tap/heal-cli                # macOS / Linux
cargo install heal-cli                          # Rust toolchain
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/kechol/heal/releases/latest/download/heal-cli-installer.sh | sh
```

Details: [Installation](https://kechol.github.io/heal/installation/).

## Quick Start

Three commands inside any git repository:

```sh
heal init                     # set up .heal/, calibrate, install hook, offer skills for each detected agent
heal status                    # render the Severity-grouped TODO list
claude /heal-code-patch        # work through it, one fix per commit (substitute `codex` if you use Codex)
```

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
