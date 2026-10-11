#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "Native app builds require macOS." >&2; exit 1; }
host_arch="$(uname -m)"
[[ "$host_arch" == "arm64" || "$host_arch" == "x86_64" ]] || { echo "Unsupported Mac host architecture: $host_arch" >&2; exit 1; }
xcodebuild -version
xcrun --sdk macosx --show-sdk-path
resolve_cargo
"$CARGO" --version
python3 "$ROOT/scripts/check-lockfiles.py"
python3 -c 'import sys; assert sys.version_info >= (3, 11), "Python 3.11+ is required for source validation"; print(sys.version)'
if command -v rustup >/dev/null 2>&1; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target list --installed | grep -qx "$target" || { echo "Rust target $target is missing. Run: rustup target add aarch64-apple-darwin x86_64-apple-darwin" >&2; exit 1; }
  done
fi
[[ -f "$ROOT/Engines/VectorEmu/Cargo.toml" ]] || { echo "VectorEmu workspace is missing." >&2; exit 1; }
printf '%s\n' "Host architecture: $host_arch" "Rust targets: aarch64-apple-darwin + x86_64-apple-darwin" "VectorEmu workspace: present" "Doctor completed; universal build/test gates still need to run."
