#!/bin/sh
# SessionStart hook for the heal plugin.
#
# Checks once per session that the heal CLI on PATH and this plugin come
# from the same release series, because the skills call CLI flags and
# read JSON shapes that change between releases. Silent unless the
# project uses heal (`.heal/` exists) and something needs attention, so
# a user-scope install adds nothing to sessions in other projects.
# Never fails the session: every path exits 0.

project="${CLAUDE_PROJECT_DIR:-$PWD}"
[ -d "$project/.heal" ] || exit 0

manifest="${CLAUDE_PLUGIN_ROOT:-$(dirname "$0")/..}/.claude-plugin/plugin.json"
plugin_version=$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" 2>/dev/null | head -n 1)
[ -n "$plugin_version" ] || exit 0

# 0.x releases may break on the minor version; 1.0 and later on the major.
series() {
  case "$1" in
    0.*) echo "$1" | cut -d. -f1,2 ;;
    *) echo "$1" | cut -d. -f1 ;;
  esac
}

# True when version $1 is older than version $2 (numeric, per component).
older() {
  for i in 1 2 3; do
    a=$(echo "$1" | cut -d. -f"$i" | sed 's/[^0-9].*//')
    b=$(echo "$2" | cut -d. -f"$i" | sed 's/[^0-9].*//')
    a=${a:-0}
    b=${b:-0}
    [ "$a" -lt "$b" ] && return 0
    [ "$a" -gt "$b" ] && return 1
  done
  return 1
}

if ! command -v heal >/dev/null 2>&1; then
  msg="The heal plugin $plugin_version is installed but the heal CLI is not on PATH. Install it (cargo install heal-cli, or brew install kechol/tap/heal-cli) before using the /heal: skills."
else
  cli_version=$(heal --version 2>/dev/null | awk '{print $2}')
  [ -n "$cli_version" ] || exit 0
  [ "$(series "$cli_version")" = "$(series "$plugin_version")" ] && exit 0
  if older "$cli_version" "$plugin_version"; then
    msg="heal CLI $cli_version is older than the heal plugin $plugin_version. Upgrade the CLI (cargo install heal-cli, or brew upgrade heal-cli) before using the /heal: skills."
  else
    msg="The heal plugin $plugin_version is older than heal CLI $cli_version. Update it with /plugin marketplace update heal, then /plugin update heal@heal."
  fi
fi

printf '{"systemMessage":"%s","hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"%s"}}\n' "$msg" "$msg"
exit 0
