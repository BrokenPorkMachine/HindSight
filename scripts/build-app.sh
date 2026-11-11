#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "Xcode requires macOS." >&2; exit 1; }
CONFIGURATION="${CONFIGURATION:-Debug}"
build_args=("$@")
# Terminal builds use one fixed DerivedData path. Serialize builds launched
# through this script, and detect an Xcode/other-process holder before starting.
# This lock never deletes build.db, and must not auto-break a stale lock: doing so
# could overlap a live build and corrupt Xcode's incremental state.
mkdir -p "$ROOT/build"
build_lock="$ROOT/build/.hindsight-build-app.lock"
if ! mkdir "$build_lock" 2>/dev/null; then
    echo "error: another HindSight build may be using $ROOT/build/DerivedData" >&2
    if [[ -f "$build_lock/owner" ]]; then
        echo "Lock owner: $(cat "$build_lock/owner")" >&2
    fi
    echo "Check running builds: pgrep -fl 'xcodebuild|XCBBuildService|swiftc|cargo'" >&2
    echo "If no build is active, remove only the stale $build_lock directory and retry." >&2
    exit 75
fi
printf 'PID %s started %s\n' "$" "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$build_lock/owner"
release_build_lock() {
    rm -f "$build_lock/owner"
    rmdir "$build_lock" 2>/dev/null || true
}
trap release_build_lock EXIT

build_db="$ROOT/build/DerivedData/Build/Intermediates.noindex/XCBuildData/build.db"
if [[ -f "$build_db" ]] && command -v lsof >/dev/null 2>&1; then
    holders="$(lsof -nP "$build_db" 2>/dev/null || true)"
    if [[ -n "$holders" ]]; then
        echo "error: HindSight Xcode build database is currently open by another process:" >&2
        printf '%s\n' "$holders" >&2
        echo "Wait for that build to finish or stop the specific stale process; do not delete build.db while it is in use." >&2
        exit 75
    fi
fi

# Managed environments may reject Swift macro-plugin subprocess sandboxes. Keep
# the workaround opt-in; ordinary builds retain Xcode's default sandboxing.
if [[ "${HINDSIGHT_SWIFT_DISABLE_SANDBOX:-0}" == "1" ]]; then
    build_args+=("OTHER_SWIFT_FLAGS=\$(inherited) -disable-sandbox")
fi
# Apple Silicon is the official release target. Intel/universal builds are
# opt-in experimental builds for development and research.
build_archs='arm64'
[[ "${HINDSIGHT_EXPERIMENTAL_INTEL:-0}" == "1" ]] && build_archs='arm64 x86_64'
build_command=(xcodebuild -project "$ROOT/HindSight.xcodeproj" -scheme HindSight -configuration "$CONFIGURATION" -destination 'generic/platform=macOS' -derivedDataPath "$ROOT/build/DerivedData" ARCHS="$build_archs" ONLY_ACTIVE_ARCH=NO build)
if (( ${#build_args[@]} > 0 )); then
    build_command+=("${build_args[@]}")
fi
"${build_command[@]}"
