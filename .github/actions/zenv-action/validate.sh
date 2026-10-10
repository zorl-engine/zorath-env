#!/usr/bin/env bash
set -euo pipefail

# Inputs arrive as data, never as shell source. Arrays preserve spaces, quotes,
# dollar substitutions and leading dashes in paths without evaluating them.
CMD=(zenv check "--schema=$ZENV_SCHEMA" "--env=$ZENV_ENV_FILE"
  "--allow-missing-env=$ZENV_ALLOW_MISSING_ENV"
  "--detect-secrets=$ZENV_DETECT_SECRETS"
  "--no-cache=$ZENV_NO_CACHE" "--format=$ZENV_FORMAT")
if [ -n "$ZENV_VERIFY_HASH" ]; then
  CMD+=("--verify-hash=$ZENV_VERIFY_HASH")
fi
if [ -n "$ZENV_CA_CERT" ]; then
  CMD+=("--ca-cert=$ZENV_CA_CERT")
fi

STDERR_FILE=$(mktemp)
trap 'rm -f "$STDERR_FILE"' EXIT
if OUTPUT=$("${CMD[@]}" 2>"$STDERR_FILE"); then
  EXIT_CODE=0
else
  EXIT_CODE=$?
fi
printf '%s\n' "$OUTPUT"
cat "$STDERR_FILE" >&2

if [ "$EXIT_CODE" -eq 0 ]; then
  printf 'valid=true\nerrors=[]\n' >> "$GITHUB_OUTPUT"
else
  printf 'valid=false\n' >> "$GITHUB_OUTPUT"
  # JSON mode keeps stdout separate from diagnostics and extracts actual
  # validation messages. File/schema failures fall back to their diagnostics.
  if [ "$ZENV_FORMAT" = json ] && ERRORS=$(printf '%s' "$OUTPUT" | jq -ce '.errors | select(type == "array" and length > 0) | map(if type == "object" then .key + ": " + .message else . end)'); then
    :
  else
    ERRORS=$( { cat "$STDERR_FILE"; printf '\n%s\n' "$OUTPUT"; } | jq -Rsc 'split("\n") | map(select(length > 0))')
  fi
  printf 'errors=%s\n' "$ERRORS" >> "$GITHUB_OUTPUT"
  printf '::error::Environment validation failed (exit %s)\n' "$EXIT_CODE"
  exit 1
fi
