#!/usr/bin/env bash
# Claude Code Stop hook — record session end and emit nudge if findings exist.
# Hook contract: stdout is shown to the user; we keep it terse (no findings yet
# in v0.1 foundation, but the wiring is in place).
set -euo pipefail

if ! command -v heal >/dev/null 2>&1; then
  exit 0
fi

heal hook stop || true
