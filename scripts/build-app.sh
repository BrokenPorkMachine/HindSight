#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "Xcode requires macOS." >&2; exit 1; }
CONFIGURATION="${CONFIGURATION:-Debug}"
xcodebuild -project "$ROOT/HindSight.xcodeproj" -scheme HindSight -configuration "$CONFIGURATION" -destination 'generic/platform=macOS' -derivedDataPath "$ROOT/build/DerivedData" ARCHS='arm64 x86_64' ONLY_ACTIVE_ARCH=NO build "$@"
