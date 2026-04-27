#!/usr/bin/env bash
# Claude Code PostToolUse(Edit|Write|MultiEdit) hook — record event in HEAL.
# Hook contract: Claude pipes a JSON payload on stdin. We forward it to
# `heal hook edit` which appends to .heal/logs/YYYY-MM.jsonl.
set -euo pipefail

if ! command -v heal >/dev/null 2>&1; then
  exit 0
fi

heal hook edit || true
