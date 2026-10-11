#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
rew_install_error_trap "Embed Rust worker"
rew_phase_banner "Embed Rust worker"

configuration="${CONFIGURATION:-Debug}"
if [[ -n "${CONFIGURATION_TEMP_DIR:-}" ]]; then
    source_worker="$CONFIGURATION_TEMP_DIR/HindSightRustArtifacts/reworker"
else
    source_worker="$ROOT/build/bridge/$configuration/reworker"
fi
[[ -n "${TARGET_BUILD_DIR:-}" ]] || rew_die "TARGET_BUILD_DIR is not set by Xcode."
[[ -n "${EXECUTABLE_FOLDER_PATH:-}" ]] || rew_die "EXECUTABLE_FOLDER_PATH is not set by Xcode."
destination="$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH/reworker"

if [[ ! -x "$source_worker" ]]; then
    rew_die "Rust worker is missing or not executable: $source_worker. The preceding 'Build Rust bridge' phase must succeed first."
fi

/bin/mkdir -p "$(/usr/bin/dirname "$destination")"
/usr/bin/ditto --noqtn "$source_worker" "$destination"
/bin/chmod 755 "$destination"
/usr/bin/xattr -c "$destination" 2>/dev/null || true

echo "embedded worker: $destination"
/usr/bin/file "$destination"

if [[ "${CODE_SIGNING_ALLOWED:-YES}" != "NO" ]]; then
    identity="${EXPANDED_CODE_SIGN_IDENTITY:-}"
    if [[ -z "$identity" || "$identity" == "-" ]]; then
        identity='-'
        echo "worker signing: ad-hoc"
    else
        echo "worker signing identity: ${EXPANDED_CODE_SIGN_IDENTITY_NAME:-$identity}"
    fi

    sign_args=(--force --sign "$identity" --timestamp=none)
    if [[ "$identity" != "-" && "${ENABLE_HARDENED_RUNTIME:-NO}" == "YES" ]]; then
        sign_args+=(--options runtime)
    fi

    /usr/bin/codesign "${sign_args[@]}" "$destination"
    /usr/bin/codesign --verify --strict --verbose=4 "$destination"
else
    echo "worker signing: skipped because CODE_SIGNING_ALLOWED=NO"
fi

echo "=== HindSight Research Tool: worker embed completed ==="
