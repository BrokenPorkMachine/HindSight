#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
if [[ "$(uname -s)" != "Darwin" ]]; then printf '%s\n' "HindSight app type-check skipped: macOS SDK required."; exit 0; fi
sdk="$(xcrun --sdk macosx --show-sdk-path)"
sources=(); while IFS= read -r source; do [[ -n "$source" ]] && sources+=("$source"); done < <(python3 "$ROOT/scripts/list-target-swift-sources.py" HindSight)
((${#sources[@]} > 0)) || { echo "error: generated HindSight target has no Swift sources" >&2; exit 1; }
compiler_flags=(-typecheck)
# Some managed environments cannot nest the compiler's plugin subprocess sandbox.
# This opt-in affects local verification only; ordinary builds retain the default.
if [[ "${HINDSIGHT_SWIFT_DISABLE_SANDBOX:-0}" == "1" ]]; then compiler_flags+=(-disable-sandbox); fi
for arch in arm64 x86_64; do
  xcrun swiftc -swift-version 5 -strict-concurrency=targeted -module-name HindSight -target "${arch}-apple-macos14.0" -sdk "$sdk" "${compiler_flags[@]}" "${sources[@]}"
  printf '%s\n' "HindSight app-target Swift type-check passed for $arch (${#sources[@]} topology sources)."
done
