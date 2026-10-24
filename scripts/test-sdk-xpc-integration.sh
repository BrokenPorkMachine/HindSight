#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
[[ "$(uname -s)" == "Darwin" ]] || { echo "SDK XPC integration requires macOS." >&2; exit 1; }

app="$ROOT/build/DerivedData/Build/Products/Debug/HindSight.app"
service="$app/Contents/XPCServices/CoordinatorService.xpc"
[[ -x "$service/Contents/MacOS/reworker" ]] || { echo "Build the app before the SDK XPC integration gate." >&2; exit 1; }
[[ -f "$service/Contents/Resources/sdk_declarations.h" ]] || { echo "CoordinatorService is missing its SDK lookup header." >&2; exit 1; }

work="$(mktemp -d /private/tmp/hindsight-sdk-xpc.XXXXXX)"
harness="$app/Contents/MacOS/hindsight-sdk-xpc-contract"
[[ ! -e "$harness" ]] || { echo "Temporary SDK XPC harness path already exists." >&2; exit 1; }
trap 'rm -f "$harness"; rm -rf "$work"' EXIT

xcrun --sdk macosx clang -target arm64-apple-macos14.0 -O0 \
  "$ROOT/Tests/Fixtures/Decompilation/sdk_import_xpc.c" -o "$work/sdk-import-fixture"
python3 "$ROOT/Tests/sdk_xpc_baseline.py" "$service/Contents/MacOS/reworker" "$work/sdk-import-fixture"
env CLANG_MODULE_CACHE_PATH="$work/swift-module-cache" xcrun swiftc \
  "$ROOT/Shared/CoordinatorProtocol.swift" "$ROOT/Tests/sdk_xpc_reconstruction_contract.swift" \
  -o "$harness"
"$harness" "$work/sdk-import-fixture" "$work/project"
