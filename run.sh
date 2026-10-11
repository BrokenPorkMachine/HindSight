#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIGURATION="${CONFIGURATION:-Debug}"

# Swift macro plugins cannot create their nested sandbox in some managed
# environments. Keep the build script's default unchanged, but make the
# user-facing build-and-run entry point work there unless explicitly opted out.
HINDSIGHT_SWIFT_DISABLE_SANDBOX="${HINDSIGHT_SWIFT_DISABLE_SANDBOX:-1}" \
    "$ROOT/build.sh" "$@"

APP="$ROOT/build/DerivedData/Build/Products/$CONFIGURATION/HindSight.app"
[[ -d "$APP" ]] || {
    echo "error: built app not found at $APP" >&2
    exit 1
}

exec open -n "$APP"
