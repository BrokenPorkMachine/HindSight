#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Xcode build phases use a deliberately minimal PATH. Add conventional Rust/Homebrew
# locations without replacing the paths Xcode already supplied.
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:${PATH:-/usr/bin:/bin:/usr/sbin:/sbin}"

rew_phase_banner() {
    local name="$1"
    echo
    echo "=== HindSight Research Tool: ${name} ==="
    echo "root: ${ROOT}"
    echo "configuration: ${CONFIGURATION:-<unset>}"
    echo "arch: ${CURRENT_ARCH:-${NATIVE_ARCH_ACTUAL:-<unset>}}"
    echo "sdkroot: ${SDKROOT:-<unset>}"
    echo "target build dir: ${TARGET_BUILD_DIR:-<unset>}"
}

rew_die() {
    echo "error: HindSight Research Tool: $*" >&2
    exit 1
}

rew_install_error_trap() {
    local phase="$1"
    trap 'rc=$?; echo "error: HindSight Research Tool phase failed: '"${phase}"' (exit ${rc})" >&2; echo "error: command: ${BASH_COMMAND}" >&2; echo "error: source: ${BASH_SOURCE[0]}:${LINENO}" >&2; exit ${rc}' ERR
}

resolve_cargo() {
    if [[ -n "${VECTORGLASS_CARGO:-}" ]]; then
        CARGO="$VECTORGLASS_CARGO"
    elif [[ -n "${REWORKBENCH_CARGO:-}" ]]; then
        CARGO="$REWORKBENCH_CARGO"
    elif command -v cargo >/dev/null 2>&1; then
        CARGO="$(command -v cargo)"
    elif [[ -x "$HOME/.cargo/bin/cargo" ]]; then
        CARGO="$HOME/.cargo/bin/cargo"
    else
        rew_die "Cargo not found. Install Rust with rustup or set VECTORGLASS_CARGO to the cargo executable."
    fi
    [[ -x "$CARGO" ]] || rew_die "Cargo is not executable: $CARGO"
}

resolve_rustup() {
    if command -v rustup >/dev/null 2>&1; then
        RUSTUP="$(command -v rustup)"
    elif [[ -x "$HOME/.cargo/bin/rustup" ]]; then
        RUSTUP="$HOME/.cargo/bin/rustup"
    else
        RUSTUP=""
    fi
}

resolve_macos_sdk() {
    local candidate="${SDKROOT:-}"
    if [[ -z "$candidate" || ! -d "$candidate" ]]; then
        candidate="$(/usr/bin/xcrun --sdk macosx --show-sdk-path)"
    fi
    [[ -d "$candidate" ]] || rew_die "Unable to resolve macOS SDKROOT (got '$candidate')."
    SDKROOT="$candidate"
    export SDKROOT
}
