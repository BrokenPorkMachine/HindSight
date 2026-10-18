#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
[[ "$(uname -s)" == Darwin ]] || { echo "Server transactional tests require macOS." >&2; exit 1; }
mode="${1:---full}"
[[ "$mode" == --full || "$mode" == --quick ]] || {
  echo "Usage: $0 [--full|--quick]" >&2; exit 2;
}
test_dir="$(mktemp -d /private/tmp/hindsight-server-transactions.XXXXXX)"
cleanup() {
  rm -f "$test_dir/transactions"
  rmdir "$test_dir" 2>/dev/null || true
}
trap cleanup EXIT

echo "[1/4] Compiling and running production analysis transaction state machine"
CLANG_MODULE_CACHE_PATH=/private/tmp/hindsight-remote-module-cache swiftc \
  -swift-version 5 -strict-concurrency=targeted -emit-executable -o "$test_dir/transactions" \
  "$project_root/Shared/WorkbenchMessages.swift" \
  "$project_root/App/RemoteGateway.swift" \
  "$project_root/App/RemoteAnalysisTransactions.swift" \
  "$project_root/Tests/RemoteTransactions.swift"
"$test_dir/transactions"

echo "[2/4] Exercising real loopback TLS, authorization, Host validation, and JSON POST"
bash "$project_root/scripts/test-remote-gateway.sh"

echo "[3/4] Checking project-store security contracts"
python3 -m unittest discover -s "$project_root/python/tests" -p test_project_store_security.py

if [[ "$mode" == --quick ]]; then
  echo "[4/4] Type-checking both app target architectures (quick mode)"
  HINDSIGHT_SWIFT_DISABLE_SANDBOX=1 CLANG_MODULE_CACHE_PATH=/private/tmp/hindsight-remote-module-cache \
    bash "$project_root/scripts/test-app-typecheck.sh"
  echo "Server transactional quick gate passed. Full build and client interoperability remain separate."
else
  echo "[4/4] Building the app, embedded coordinator, and native worker"
  xcodebuild -quiet -project "$project_root/HindSight.xcodeproj" -scheme HindSight \
    -configuration Debug -derivedDataPath /private/tmp/hindsight-server-build \
    build CODE_SIGNING_ALLOWED=NO
  echo "Server transactional full gate passed. Client interoperability and release security remain separate."
fi
