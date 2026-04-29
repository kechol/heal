#!/usr/bin/env bash
# Claude Code Stop hook — record session-turn end in HEAL.
#
# Stop fires every time Claude finishes responding within a session, which
# is too noisy for nudges (the underlying MetricsSnapshot only updates on
# commit). HEAL therefore uses Stop solely for log fidelity; the
# user-facing nudge lives in the post-commit path (TODO §post-commit
# nudge — to land alongside Calibration).
set -euo pipefail

if ! command -v heal >/dev/null 2>&1; then
  exit 0
fi

heal hook stop || true
