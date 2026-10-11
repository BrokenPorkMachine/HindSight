#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
"$ROOT/scripts/doctor.sh"
"$ROOT/scripts/test-portable.sh"
"$ROOT/scripts/test-app-typecheck.sh"
"$ROOT/scripts/test-coordinator-typecheck.sh"
"$ROOT/scripts/test-scripting-host-typecheck.sh"
"$ROOT/scripts/test-firmware-runtime-typecheck.sh"
CONFIGURATION=Debug ARCHS='arm64 x86_64' ONLY_ACTIVE_ARCH=NO "$ROOT/scripts/build-rust.sh"
host_arch="$(uname -m)"
xcrun clang -arch "$host_arch" -mmacosx-version-min=14.0 -I "$ROOT/Bridge" "$ROOT/Tests/bridge_smoke.c" "$ROOT/build/bridge/Debug/libreworkbench_bridge.a" -framework Security -framework CoreFoundation -liconv -o "$ROOT/build/bridge-smoke"
"$ROOT/build/bridge-smoke"
xcrun swiftc "$ROOT/Shared/WorkbenchMessages.swift" "$ROOT/Tests/page_contract.swift" -o "$ROOT/build/page-contract-smoke"; "$ROOT/build/page-contract-smoke"
xcrun swiftc -import-objc-header "$ROOT/Bridge/HindSightBridge.h" "$ROOT/Shared/WorkbenchMessages.swift" "$ROOT/PlatformServices/CoordinatorService/VerifiedPageCache.swift" "$ROOT/Tests/cache_contract.swift" "$ROOT/build/bridge/Debug/libreworkbench_bridge.a" -o "$ROOT/build/cache-contract-smoke" -framework Security -framework CoreFoundation -liconv; "$ROOT/build/cache-contract-smoke"
"$ROOT/scripts/build-app.sh"
app="$ROOT/build/DerivedData/Build/Products/Debug/HindSight.app"; "$ROOT/scripts/verify-app-signing.sh" "$app"
service="$app/Contents/XPCServices/CoordinatorService.xpc"; [[ -d "$service" ]] || { echo "missing service" >&2; exit 1; }
for executable in "$app/Contents/MacOS/HindSight" "$service/Contents/MacOS/CoordinatorService" "$service/Contents/MacOS/reworker" "$service/Contents/MacOS/vg-emu-server"; do
  [[ -x "$executable" ]] || { echo "missing executable: $executable" >&2; exit 1; }
  archs="$(/usr/bin/lipo -archs "$executable")"; [[ "$archs" == *arm64* && "$archs" == *x86_64* ]] || { echo "not universal: $executable ($archs)" >&2; exit 1; }
  /usr/bin/codesign --verify --strict --verbose=2 "$executable"
done
"$ROOT/scripts/test-embedded-worker.py" "$service/Contents/MacOS/reworker"
"$service/Contents/MacOS/vg-emu-server" --version | /usr/bin/grep -F "0.5.7"
printf '%s\n' "Universal macOS build/signature/bundle gates completed on $(uname -m)."
