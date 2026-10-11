#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "FirmwareRuntimeHost type-check requires macOS." >&2; exit 1; }
xcrun swiftc -typecheck \
  "$ROOT/Shared/FirmwareRuntimeMessages.swift" \
  "$ROOT/PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift"
printf '%s\n' "FirmwareRuntimeHost Swift type-check passed."
