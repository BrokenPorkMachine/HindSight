#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "Xcode requires macOS." >&2; exit 1; }
CONFIGURATION="${CONFIGURATION:-Debug}"
build_args=("$@")
# Managed environments may reject Swift macro-plugin subprocess sandboxes. Keep
# the workaround opt-in; ordinary builds retain Xcode's default sandboxing.
if [[ "${HINDSIGHT_SWIFT_DISABLE_SANDBOX:-0}" == "1" ]]; then
    build_args+=("OTHER_SWIFT_FLAGS=\$(inherited) -disable-sandbox")
fi
build_command=(xcodebuild -project "$ROOT/HindSight.xcodeproj" -scheme HindSight -configuration "$CONFIGURATION" -destination 'generic/platform=macOS' -derivedDataPath "$ROOT/build/DerivedData" ARCHS='arm64 x86_64' ONLY_ACTIVE_ARCH=NO build)
if (( ${#build_args[@]} > 0 )); then
    build_command+=("${build_args[@]}")
fi
"${build_command[@]}"
