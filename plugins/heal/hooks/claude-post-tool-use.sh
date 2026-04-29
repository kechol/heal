#!/usr/bin/env bash
# Claude Code PostToolUse(Edit|Write|MultiEdit) hook — record event in HEAL.
#
# Hook contract: Claude pipes the tool-use payload as JSON on stdin. We
# forward the byte stream unchanged to `heal hook edit`, which appends a
# record to .heal/logs/YYYY-MM.jsonl. This hook is intentionally cheap
# (no observer scan) because it fires on every edit during a session.
set -euo pipefail

if ! command -v heal >/dev/null 2>&1; then
  exit 0
fi

heal hook edit || true
