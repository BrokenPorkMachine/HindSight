#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/hindsight-dispatch.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
xcrun swiftc \
  "$ROOT/PlatformServices/CoordinatorService/BoundedProcessRunner.swift" \
  "$ROOT/Shared/ScriptingMessages.swift" \
  "$ROOT/PlatformServices/CoordinatorService/ScriptingHost.swift" \
  "$ROOT/Tests/process_dispatch_contract.swift" -o "$tmp/contract"
"$tmp/contract"
python3 - "$ROOT" "$tmp/lane.swift" <<'PY'
from pathlib import Path
import sys
source = (Path(sys.argv[1]) / 'PlatformServices/CoordinatorService/main.swift').read_text()
Path(sys.argv[2]).write_text(source.split('final class CoordinatorService:')[0])
PY
xcrun swiftc "$tmp/lane.swift" "$ROOT/Tests/service_lane_contract.swift" -o "$tmp/lane-contract"
"$tmp/lane-contract"
xcrun clang -Wall -Wextra -Werror "$ROOT/Tests/terminal_dispatch_contract.c" -o "$tmp/terminal-contract"
"$tmp/terminal-contract"
