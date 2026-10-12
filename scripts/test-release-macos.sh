#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

[[ "$(uname -s)" == "Darwin" ]] || { echo "Release qualification requires macOS." >&2; exit 1; }
: "${VECTORGLASS_NOTARIZE_PROFILE:?Set VECTORGLASS_NOTARIZE_PROFILE for stable release qualification.}"
: "${VECTORGLASS_SIGN_IDENTITY:?Set VECTORGLASS_SIGN_IDENTITY to a Developer ID Application identity.}"
: "${VECTORGLASS_DEVELOPMENT_TEAM:?Set VECTORGLASS_DEVELOPMENT_TEAM for stable release qualification.}"

"$ROOT/scripts/test-beta-macos.sh"
app="$ROOT/build/DerivedData/Build/Products/Release/HindSight.app"
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
build=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$app/Contents/Info.plist")
[[ "$version" == "0.5.10" ]] || { echo "error: expected stable version 0.5.10, got $version" >&2; exit 1; }
[[ "$build" == "5910" ]] || { echo "error: expected build 5910, got $build" >&2; exit 1; }
xcrun stapler validate "$app"
printf '%s\n' \
  "HindSight Research Tool 0.5.10 stable release-host gates passed." \
  "Complete the real-target and 8 GiB pressure matrix in docs/RELEASE_READINESS.md before distribution."
