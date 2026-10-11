#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

[[ "$(uname -s)" == "Darwin" ]] || { echo "Beta qualification requires macOS." >&2; exit 1; }

# Debug exercises the development instrumentation and all portable/native contracts.
"$ROOT/scripts/test-macos.sh"

# Beta promotion also requires a clean Release build and embedded-worker smoke test.
# Ad-hoc signing is sufficient for ordinary beta qualification. Notarization, however,
# requires an explicit Developer ID identity and team supplied by the release operator.
release_build_args=()
if [[ -n "${VECTORGLASS_NOTARIZE_PROFILE:-}" ]]; then
    : "${VECTORGLASS_SIGN_IDENTITY:?Set VECTORGLASS_SIGN_IDENTITY to a Developer ID Application identity when notarizing.}"
    : "${VECTORGLASS_DEVELOPMENT_TEAM:?Set VECTORGLASS_DEVELOPMENT_TEAM when notarizing.}"
    release_build_args+=(
        "CODE_SIGN_IDENTITY=$VECTORGLASS_SIGN_IDENTITY"
        "DEVELOPMENT_TEAM=$VECTORGLASS_DEVELOPMENT_TEAM"
        "CODE_SIGN_STYLE=Manual"
        "ENABLE_HARDENED_RUNTIME=YES"
    )
fi
CONFIGURATION=Release "$ROOT/scripts/build-app.sh" "${release_build_args[@]}"
app="$ROOT/build/DerivedData/Build/Products/Release/HindSight.app"
"$ROOT/scripts/verify-app-signing.sh" "$app"
service="$app/Contents/XPCServices/CoordinatorService.xpc"
[[ -d "$service" ]] || { echo "error: embedded CoordinatorService.xpc not found: $service" >&2; exit 1; }
service_exe="$service/Contents/MacOS/CoordinatorService"
worker="$service/Contents/MacOS/reworker"
emu="$service/Contents/MacOS/vg-emu-server"
for executable in "$app/Contents/MacOS/HindSight" "$service_exe" "$worker" "$emu"; do
    [[ -x "$executable" ]] || { echo "error: required embedded executable missing: $executable" >&2; exit 1; }
    archs=$(/usr/bin/lipo -archs "$executable")
    [[ "$archs" == *arm64* && "$archs" == *x86_64* ]] || { echo "error: universal arm64+x86_64 slices missing from $executable: $archs" >&2; exit 1; }
    /usr/bin/codesign --verify --strict --verbose=2 "$executable"
done
"$ROOT/scripts/test-embedded-worker.py" "$worker"
"$emu" --version | /usr/bin/grep -F "0.5.7"

# A notarization profile is optional for candidate qualification; stable distribution uses test-release-macos.sh and requires it.
if [[ -n "${VECTORGLASS_NOTARIZE_PROFILE:-}" ]]; then
    artifact="$ROOT/build/HindSight-0.5.7-notarization.zip"
    rm -f "$artifact"
    identity=$(/usr/bin/codesign -dv --verbose=4 "$app" 2>&1 | /usr/bin/sed -n 's/^Authority=//p' | /usr/bin/head -n 1)
    [[ -n "$identity" && "$identity" != "-" ]] || { echo "error: notarization requested but Release app is not Developer-ID signed" >&2; exit 1; }
    /usr/bin/ditto -c -k --keepParent "$app" "$artifact"
    xcrun notarytool submit "$artifact" --keychain-profile "$VECTORGLASS_NOTARIZE_PROFILE" --wait
    xcrun stapler staple "$app"
    xcrun stapler validate "$app"
    echo "Notarization submission and stapling completed."
else
    echo "Candidate gate: notarization skipped (set VECTORGLASS_NOTARIZE_PROFILE to exercise it)."
fi

printf '%s\n' \
  "HindSight candidate qualification gates passed on this host." \
  "Stable distribution still requires test-release-macos.sh plus the manual real-target and memory-pressure matrix in docs/RELEASE_READINESS.md."
