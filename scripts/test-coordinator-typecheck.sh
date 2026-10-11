#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "CoordinatorService type-check requires macOS." >&2; exit 1; }
sdk="$(xcrun --sdk macosx --show-sdk-path)"
sources=(); while IFS= read -r source; do [[ -n "$source" ]] && sources+=("$source"); done < <(python3 "$ROOT/scripts/list-target-swift-sources.py" CoordinatorService)
((${#sources[@]} > 0)) || { echo "error: generated CoordinatorService target has no Swift sources" >&2; exit 1; }
for arch in arm64 x86_64; do
  xcrun swiftc -typecheck -swift-version 5 -strict-concurrency=targeted -module-name CoordinatorService -target "${arch}-apple-macos14.0" -sdk "$sdk" -import-objc-header "$ROOT/Bridge/HindSightBridge.h" "${sources[@]}"
  printf '%s\n' "CoordinatorService full Swift type-check passed for $arch (${#sources[@]} topology sources)."
done
