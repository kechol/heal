# heal

> **h**ook-driven **e**valuation & **a**utonomous **l**oop — a code-health
> harness that turns codebase decay signals into work for AI coding agents.

LLM coding agents are usually reactive: a human files a task before the
agent moves. Codebases, meanwhile, decay continuously — complexity
creeps, hotspots shift, duplicates accumulate. heal closes that gap
with an **observe → calibrate → check → fix** loop, turning codebase
state changes into agent triggers.

Documentation: <https://kechol.github.io/heal/>

> ⚠️ **Status: v0.2 in progress.** macOS / Linux only.

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
heal init                     # set up .heal/, calibrate, install hook, offer Claude plugin
heal check                    # render the Severity-grouped TODO list
claude /heal-code-patch         # drain it, one finding per commit
```

Full walkthrough: [Quick Start](https://kechol.github.io/heal/quick-start/).

## Documentation

Topical pages on the docs site:

- [Concept](https://kechol.github.io/heal/concept/) — design idea in three minutes
- [Metrics](https://kechol.github.io/heal/metrics/) — what each metric measures, how Severity is assigned
- [CLI](https://kechol.github.io/heal/cli/) — every subcommand
- [Configuration](https://kechol.github.io/heal/configuration/) — thresholds, toggles, calibration
- [Claude plugin](https://kechol.github.io/heal/claude-plugin/) — `/heal-code-review` + `/heal-code-patch` contracts
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
