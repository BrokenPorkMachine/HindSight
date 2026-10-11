#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

usage() {
    cat <<'USAGE'
Usage: ./scripts/qualify-release.sh [options]

Run HindSight's release checks and save an operator report under build/release-reports/.

Options:
  --candidate       Run the credential-free universal candidate gate (default).
  --stable          Run the signed, notarized stable-release gate.
  --report-dir DIR  Store the report in DIR instead of build/release-reports/.
  --open            Open the verified Release app after a successful gate.
  --help            Show this help.

Stable mode expects:
  VECTORGLASS_SIGN_IDENTITY
  VECTORGLASS_DEVELOPMENT_TEAM
  VECTORGLASS_NOTARIZE_PROFILE
USAGE
}

mode="candidate"
open_app=false
report_root="$ROOT/build/release-reports"

while (($#)); do
    case "$1" in
        --candidate) mode="candidate" ;;
        --stable) mode="stable" ;;
        --report-dir)
            (($# >= 2)) || rew_die "--report-dir requires a directory"
            report_root="$2"
            shift
            ;;
        --open) open_app=true ;;
        --help|-h) usage; exit 0 ;;
        *) rew_die "unknown option: $1 (use --help)" ;;
    esac
    shift
done

[[ "$(uname -s)" == "Darwin" ]] || rew_die "release qualification requires macOS"

timestamp="$(date '+%Y%m%d-%H%M%S')"
report_dir="$report_root/$timestamp-$mode"
mkdir -p "$report_dir"
transcript="$report_dir/transcript.log"
summary="$report_dir/summary.txt"
manual="$report_dir/manual-matrix.md"

# Keep Swift/Clang module artifacts inside the report workspace by default. This
# avoids permission surprises from a locked-down global cache and makes a report
# self-contained; callers may override either path.
export CLANG_MODULE_CACHE_PATH="${CLANG_MODULE_CACHE_PATH:-$report_dir/module-cache}"
export CLANG_CACHE_PATH="${CLANG_CACHE_PATH:-$report_dir/clang-cache}"
mkdir -p "$CLANG_MODULE_CACHE_PATH" "$CLANG_CACHE_PATH"

# Capture the complete build/signing output without relying on /dev/fd process
# substitution, which is unavailable in some managed shells. The transcript is
# replayed to the terminal after the gate finishes.
exec 3>&1
exec > "$transcript" 2>&1

started_at="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
git_revision="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
git_dirty="$(git -C "$ROOT" status --porcelain 2>/dev/null || true)"

echo "HindSight release qualification"
echo "mode: $mode"
echo "started: $started_at"
echo "revision: $git_revision"
if [[ -n "$git_dirty" ]]; then
    echo "warning: working tree is dirty; qualification is not from a clean checkout"
fi
echo "report: $report_dir"

set +e
./scripts/release-preflight.sh
preflight_rc=$?
if ((preflight_rc != 0)); then
    echo "preflight: FAILED ($preflight_rc)"
    gate_rc=$preflight_rc
else
    if [[ "$mode" == "stable" ]]; then
        ./scripts/test-release-macos.sh
    else
        ./scripts/test-beta-macos.sh
    fi
    gate_rc=$?
fi
set -e

app="$ROOT/build/DerivedData/Build/Products/Release/HindSight.app"
if [[ -d "$app" ]]; then
    app_status="present"
else
    app_status="missing"
fi

cat > "$manual" <<EOF
# HindSight manual release matrix

Generated: $started_at
Repository revision: $git_revision
Automated mode: $mode
Automated gate exit code: $gate_rc

The following checks require a real Intel Mac and an 8 GiB Apple-Silicon Mac. Record
results here after each run; do not treat this file as passing until every item is
verified on the required host.

- [ ] Large-firmware memory-pressure reproduction; record peak RSS and bounded failure/completion.
- [ ] Real stripped, symbol-rich, data-in-code, and encrypted-metadata arm64 Mach-O inputs.
- [ ] 0.4.1 schema-1 migration and 0.4.2 schema-2 cache reuse/invalidation.
- [ ] Monotonic progress, cancellation, and clean worker diagnostics.
- [ ] Binary Diff, responsive layouts, Function Neighborhoods, and continuity/reconnect flows.
- [ ] VectorEmu flows and Intel closed-out debugger/Hypervisor/SoC surfaces.
- [ ] Apple-Silicon debugger, Hypervisor, and SoC execution paths.
- [ ] Final Gatekeeper assessment after notarization.

See docs/RELEASE_READINESS.md for the complete numbered matrix.
EOF

cat > "$summary" <<EOF
HindSight release qualification summary
mode=$mode
started=$started_at
revision=$git_revision
preflight_exit=$preflight_rc
gate_exit=$gate_rc
release_app=$app_status
transcript=$transcript
manual_matrix=$manual
EOF

if ((gate_rc == 0)); then
    echo "automated result: PASS"
    echo "manual matrix: $manual"
    if $open_app; then
        [[ -d "$app" ]] || { echo "error: cannot open missing Release app: $app" >&2; exit 2; }
        open -n "$app"
    fi
else
    echo "automated result: FAIL ($gate_rc)" >&2
    echo "inspect transcript: $transcript" >&2
fi

echo "summary: $summary"
cat "$transcript" >&3
exit "$gate_rc"
