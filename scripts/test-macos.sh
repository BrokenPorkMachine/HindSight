#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
"$ROOT/scripts/doctor.sh"
"$ROOT/scripts/test-portable.sh"
python3 "$ROOT/scripts/test-prototype-abi.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-darwin-catalog.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-arm64-vector-extract.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-arm64-vector-mla.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-arm64-vector-float.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-arm64-nameflow.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-worker-reconstruction.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-machine-state.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-recovered-source.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-recovered-void.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-recovered-pointees.py" "$ROOT/build/rust/debug/reworker"
python3 "$ROOT/scripts/test-recovered-headers.py" "$ROOT/build/rust/debug/reworker"
"$ROOT/scripts/test-app-typecheck.sh"
"$ROOT/scripts/test-coordinator-typecheck.sh"
python3 "$ROOT/scripts/test-rea-bridge.py"
"$ROOT/scripts/test-scripting-host-typecheck.sh"
"$ROOT/scripts/test-firmware-runtime-typecheck.sh"
test_archs='arm64'
if [[ "${HINDSIGHT_EXPERIMENTAL_INTEL:-0}" == "1" ]]; then test_archs='arm64 x86_64'; fi
CONFIGURATION=Debug ARCHS="$test_archs" ONLY_ACTIVE_ARCH=NO "$ROOT/scripts/build-rust.sh"
host_arch="$(uname -m)"
xcrun clang -arch "$host_arch" -mmacosx-version-min=14.0 -I "$ROOT/Bridge" "$ROOT/Tests/bridge_smoke.c" "$ROOT/build/bridge/Debug/libreworkbench_bridge.a" -framework Security -framework CoreFoundation -liconv -o "$ROOT/build/bridge-smoke"
"$ROOT/build/bridge-smoke"
xcrun swiftc "$ROOT/Shared/WorkbenchMessages.swift" "$ROOT/Tests/page_contract.swift" -o "$ROOT/build/page-contract-smoke"; "$ROOT/build/page-contract-smoke"
xcrun swiftc -import-objc-header "$ROOT/Bridge/HindSightBridge.h" "$ROOT/Shared/WorkbenchMessages.swift" "$ROOT/PlatformServices/CoordinatorService/VerifiedPageCache.swift" "$ROOT/Tests/cache_contract.swift" "$ROOT/build/bridge/Debug/libreworkbench_bridge.a" -o "$ROOT/build/cache-contract-smoke" -framework Security -framework CoreFoundation -liconv; "$ROOT/build/cache-contract-smoke"
"$ROOT/scripts/build-app.sh"
app="$ROOT/build/DerivedData/Build/Products/Debug/HindSight.app"
"$ROOT/scripts/test-sdk-xpc-integration.sh"
"$ROOT/scripts/verify-app-signing.sh" "$app"
service="$app/Contents/XPCServices/CoordinatorService.xpc"; [[ -d "$service" ]] || { echo "missing service" >&2; exit 1; }
for executable in "$app/Contents/MacOS/HindSight" "$service/Contents/MacOS/CoordinatorService" "$service/Contents/MacOS/reworker" "$service/Contents/MacOS/vg-emu-server"; do
  [[ -x "$executable" ]] || { echo "missing executable: $executable" >&2; exit 1; }
  archs="$(/usr/bin/lipo -archs "$executable")"; [[ "$archs" == *arm64* ]] || { echo "missing official arm64 slice: $executable ($archs)" >&2; exit 1; }
  /usr/bin/codesign --verify --strict --verbose=2 "$executable"
done
"$ROOT/scripts/test-embedded-worker.py" "$service/Contents/MacOS/reworker"
"$service/Contents/MacOS/vg-emu-server" --version | /usr/bin/grep -F "0.5.13"
printf '%s\n' "Official arm64 macOS build/signature/bundle gates completed on $(uname -m). Intel support is experimental."
