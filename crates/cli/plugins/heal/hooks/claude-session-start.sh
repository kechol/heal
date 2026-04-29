#!/usr/bin/env bash
# Claude Code SessionStart hook — surface unresolved code-health findings.
#
# Anything written to stdout here is shown to the user as additional
# context at session boot, so HEAL emits a brief nudge listing findings
# whose cool-down has elapsed. The heavy lifting (finding derivation +
# cool-down bookkeeping) lives inside `heal hook session-start`.
set -euo pipefail

if ! command -v heal >/dev/null 2>&1; then
  exit 0
fi

heal hook session-start || true
