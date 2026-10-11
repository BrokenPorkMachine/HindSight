#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "ScriptingHost type-check requires macOS." >&2; exit 1; }
xcrun swiftc -typecheck \
  "$ROOT/Shared/ScriptingMessages.swift" \
  "$ROOT/PlatformServices/CoordinatorService/ScriptingHost.swift"
printf '%s\n' "ScriptingHost Swift type-check passed."
