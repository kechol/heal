---
title: Architecture
description: Where HEAL stores data, how the event log and snapshots work, and how the pieces fit together.
---

This page describes HEAL's internals — useful when debugging a
missing nudge, scripting against the JSON output, or wanting an
overview of how the components fit together. It is not required
reading for day-to-day use.

## The big picture

```
git commit
   │
   ▼
.git/hooks/post-commit  ──►  heal hook commit
                                 │
                                 ├──►  observers (LOC, complexity, churn, …)
                                 │
                                 ├──►  .heal/snapshots/YYYY-MM.jsonl   (heavy: MetricsSnapshot)
                                 │
                                 └──►  .heal/logs/YYYY-MM.jsonl        (lightweight: CommitInfo)

claude session opens
   │
   ▼
SessionStart hook  ──►  heal hook session-start
                            │
                            ├──►  read latest snapshot + delta
                            │
                            ├──►  read .heal/runtime/state.json (cool-downs)
                            │
                            └──►  print markdown nudge to stdout (Claude sees it)
```

`heal` is a single binary; both arrows go through it. There is no
daemon, no scheduler, no background process.

## On-disk layout

After `heal init`:

```
.heal/
├── config.toml                # you edit this; heal init writes the default
├── snapshots/
│   └── 2026-04.jsonl          # MetricsSnapshot per commit, append-only
├── logs/
│   └── 2026-04.jsonl          # CommitInfo + Claude hook events
├── runtime/
│   └── state.json             # cool-down timestamps for the SessionStart nudge
├── docs/                      # placeholder for v0.3 doc observers
└── reports/                   # placeholder for future weekly reports
```

Plus, outside of `.heal/`:

```
.git/hooks/post-commit         # one-line shim that calls `heal hook commit`
.claude/plugins/heal/          # bundled Claude plugin, after `heal skills install`
```

## The event log

Both `snapshots/` and `logs/` use the same on-disk format:

- **One directory per stream** (`snapshots/` and `logs/` are
  independent).
- **One file per month**: `YYYY-MM.jsonl` (UTC).
- **Append-only**: every record is one JSON object, one line.
- **Compressed segments** (`YYYY-MM.jsonl.gz`) are read transparently
  once compaction lands in v0.2+. v0.1 only writes plaintext.

Every record has the same outer shape:

```json
{
  "timestamp": "2026-04-29T05:14:22Z",
  "event": "commit",
  "data": {
    /* … shape depends on event … */
  }
}
```

The `event` field tells you what kind of payload `data` is.

### Snapshots vs. logs

The two streams are deliberately split:

- **`snapshots/`** holds the heavy `MetricsSnapshot` payload — the
  full per-metric report from every observer. `heal status` reads
  these. Larger and slower to scan than the log stream.
- **`logs/`** holds lightweight events:
  - `commit` — `CommitInfo` (sha, parent, author email, message
    summary, files_changed, insertions, deletions). Mirrors a
    snapshot's commit but without the heavy payload, so timeline
    queries are cheap.
  - `edit` — raw stdin from Claude's `PostToolUse` hook.
  - `stop` — raw stdin from Claude's `Stop` hook.
  - `session-start` — raw stdin from Claude's `SessionStart` hook,
    plus the rules that fired.
  - `init` — written once when `heal init` ran.

  `heal logs` reads these.

Both streams can be inspected with standard Unix tools; `jq` is
particularly well-suited because each line is a complete JSON
object.

## Snapshots

A `MetricsSnapshot` (one line in `snapshots/YYYY-MM.jsonl`) looks
roughly like:

```json
{
  "version": 1,
  "git_sha": "a0a6d1a…",
  "loc": {
    /* LocReport      */
  },
  "complexity": {
    /* or null if disabled */
  },
  "churn": {
    /* … */
  },
  "change_coupling": null,
  "duplication": {
    /* … */
  },
  "hotspot": {
    /* … */
  },
  "delta": {
    /* SnapshotDelta or null on the first snapshot */
  }
}
```

Each per-metric field is the JSON-serialised report from that
observer (see [Metrics](/heal/metrics/) for what each one contains)
or `null` when the metric is disabled.

`delta` summarises movement since the previous snapshot — new
entries in worst-N lists, changes in `max_ccn`, hotspot ranking
shifts. The SessionStart nudge consumes it.

The schema deliberately does **not** use `deny_unknown_fields` so an
older `heal` binary can still read a snapshot written by a newer
one — forward-compatibility for the persisted format.

## State

`.heal/runtime/state.json` holds:

```json
{
  "last_fired": {
    "complexity.spike": "2026-04-28T03:14:22Z",
    "hotspot.new_top": "2026-04-25T11:02:08Z"
  },
  "open_proposals": {}
}
```

- `last_fired[<rule_id>]` — when each rule last fired. The
  SessionStart hook uses this against the rule's `cooldown_hours` to
  decide whether to fire again.
- `open_proposals` — placeholder for v0.2's `heal run`. Empty in
  v0.1.

Writes are **atomic** (write-to-temp + rename) so a SIGINT mid-write
cannot leave the file half-truncated.

## The Claude plugin install manifest

`.claude/plugins/heal/.heal-install.json` records the fingerprint
of every file `heal skills install` extracted, plus the version of
`heal` that did the extraction. `heal skills update` reads this:

- File whose current fingerprint matches the recorded bundled
  fingerprint → safe to overwrite (refresh).
- File whose fingerprint differs → user has hand-edited it; leave
  alone (warn) unless `--force`.

## What is in the binary

The `heal` binary is a single Rust crate (`heal-cli`) with internal
modules organised as if the project still had a three-crate split:

```
crates/cli/src/
├── core/        # config, eventlog, snapshot, state — pure data types
├── observer/    # LOC, complexity, churn, coupling, duplication, hotspot
├── commands/    # one file per subcommand (init, status, hook, …)
├── cli.rs       # clap definitions
└── main.rs      # entrypoint
```

The Claude plugin tree (`crates/cli/plugins/heal/`) is embedded at
compile time via `include_dir!` so `cargo install heal-cli` ships
both the CLI and the plugin in one tarball.

## Design rationale

Key decisions:

- **Per-commit snapshots** — observers run on the post-commit hook
  rather than in CI. The data continues to flow without requiring a
  separate CI integration.
- **Append-only JSONL** — easy to read with shell tools, easy to
  rotate, and avoids schema-migration overhead during early
  development.
- **Snapshots separate from logs** — splitting the two streams keeps
  `heal logs` fast even as the snapshot store grows.
- **Atomic state writes** — `state.json` is the only mutable file;
  everything else is append-only or rewritten in full.
- **No daemon** — the post-commit hook and the SessionStart hook
  together cover the entire input surface. There is nothing to
  start or monitor.

For very large projects (on the order of millions of commits), HEAL
will need a more efficient store; SQLite is on the v0.2 list. In
v0.1, simplicity is intentional.
