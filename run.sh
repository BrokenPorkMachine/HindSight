#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIGURATION="${CONFIGURATION:-Debug}"

"$ROOT/build.sh" "$@"

APP="$ROOT/build/DerivedData/Build/Products/$CONFIGURATION/HindSight.app"
[[ -d "$APP" ]] || {
    echo "error: built app not found at $APP" >&2
    exit 1
}

exec open -n "$APP"
