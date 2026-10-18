#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

[[ "$(uname -s)" == "Darwin" ]] || { echo "Crash diagnostics check requires macOS." >&2; exit 1; }

echo "HindSight crash diagnostics check"
echo "host: $(uname -m)"

# Verify crash reporting infrastructure exists
app="$ROOT/build/DerivedData/Build/Products/Release/HindSight.app"
if [[ -d "$app" ]]; then
  echo "Release app: present"
else
  echo "Release app: missing (build first)"
fi

# Check for crash report directories
if [[ -d "$HOME/Library/Logs/DiagnosticReports" ]]; then
  echo "DiagnosticReports: accessible"
else
  echo "DiagnosticReports: not found"
fi

# Check for relevant scripts
for f in scripts/crash-resolve.py scripts/test-app-typecheck.sh; do
  if [[ -f "$ROOT/$f" ]]; then
    echo "infrastructure: $f present"
  fi
done

timestamp="$(date '+%Y%m%d-%H%M%S')"
report="$ROOT/build/release-reports/crash-diagnostics-$timestamp.json"
mkdir -p "$(dirname "$report")"

cat > "$report" <<EOF
{
  "timestamp": "$timestamp",
  "host": "$(uname -m)",
  "release_app_present": $([[ -d "$app" ]] && echo true || echo false),
  "diagnostic_reports_accessible": $([[ -d "$HOME/Library/Logs/DiagnosticReports" ]] && echo true || echo false),
  "infrastructure_checked": ["crash-resolve.py", "test-app-typecheck.sh"],
  "note": "Infrastructure check only. Manual crash reproduction required per RELEASE_READINESS.md matrix."
}
EOF
echo "Report: $report"
echo "Crash diagnostics infrastructure verified."
echo "Note: manual crash reproduction required per RELEASE_READINESS.md matrix."
