#!/usr/bin/env bash
set -euo pipefail
# Python keeps the version update portable across GNU/BSD sed and Windows.
if command -v python3 >/dev/null 2>&1; then
  exec python3 "$(dirname "$0")/release.py" "$@"
fi
exec python "$(dirname "$0")/release.py" "$@"
