#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

[[ "$(uname -s)" == "Darwin" ]] || { echo "Release preflight requires macOS." >&2; exit 1; }

echo "HindSight release preflight"
echo "host architecture: $(uname -m)"
xcodebuild -version | sed -n '1,2p'
rustc --version
python3 "$ROOT/scripts/test-rea-bridge.py"
echo "REA bridge fixture contract: passed"

if [[ -n "${VECTORGLASS_SIGN_IDENTITY:-}" ]]; then
    identities="$(security find-identity -v -p codesigning 2>/dev/null || true)"
    if grep -Fq "${VECTORGLASS_SIGN_IDENTITY}" <<<"$identities"; then
        echo "Developer ID identity: present"
    else
        echo "Developer ID identity: configured but not present in the login keychain"
    fi
else
    echo "Developer ID identity: not configured"
fi

if [[ -n "${VECTORGLASS_DEVELOPMENT_TEAM:-}" ]]; then
    echo "Developer Team: configured"
else
    echo "Developer Team: not configured"
fi

if [[ -n "${VECTORGLASS_NOTARIZE_PROFILE:-}" ]]; then
    echo "Notary profile: configured"
else
    echo "Notary profile: not configured"
fi

app="$ROOT/build/DerivedData/Build/Products/Release/HindSight.app"
if [[ -d "$app" ]]; then
    echo "Release bundle: present"
    for executable in \
        "$app/Contents/MacOS/HindSight" \
        "$app/Contents/XPCServices/CoordinatorService.xpc/Contents/MacOS/CoordinatorService" \
        "$app/Contents/XPCServices/CoordinatorService.xpc/Contents/MacOS/reworker" \
        "$app/Contents/XPCServices/CoordinatorService.xpc/Contents/MacOS/vg-emu-server"; do
        [[ -x "$executable" ]] || { echo "Release executable missing: $executable" >&2; exit 1; }
        archs="$(/usr/bin/lipo -archs "$executable")"
        [[ "$archs" == *arm64* ]] || {
            echo "Release executable is missing the official arm64 slice: $executable ($archs)" >&2
            exit 1
        }
        echo "official arm64 slice: $(basename "$executable") ($archs)"
    done
else
    echo "Release bundle: not built"
fi

echo "Preflight does not submit notarization or replace the manual Apple-Silicon qualification matrix. Intel qualification is experimental and optional."
