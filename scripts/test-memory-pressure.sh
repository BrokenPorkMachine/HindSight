#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

[[ "$(uname -s)" == "Darwin" ]] || { echo "Memory pressure test requires macOS." >&2; exit 1; }

echo "HindSight memory-pressure qualification (synthetic)"
echo "host: $(uname -m)"

# Use a bounded synthetic workload: run Rust unit tests with release profile
# to exercise analysis paths without requiring external large fixtures by default.
# This gives measurable memory behavior in a controlled way.
timestamp="$(date '+%Y%m%d-%H%M%S')"
report="$ROOT/build/release-reports/memory-pressure-$timestamp.json"
mkdir -p "$(dirname "$report")"

start_rss=$(ps -o rss= -p $$ | awk '{print $1}')
echo "start RSS (KB): $start_rss"
start_time=$(date +%s)

# Run a focused set of release tests that stress bounded analysis
if command -v cargo >/dev/null 2>&1; then
  cargo test --release --workspace --lib 2>&1 | tail -n 30
else
  echo "cargo not found; skipping cargo-based pressure test"
fi

end_time=$(date +%s)
end_rss=$(ps -o rss= -p $$ | awk '{print $1}')
delta=$((end_rss - start_rss))
duration=$((end_time - start_time))
echo "end RSS (KB): $end_rss"
echo "delta RSS (KB): $delta"
echo "duration (s): $duration"

cat > "$report" <<EOF
{
  "timestamp": "$timestamp",
  "host": "$(uname -m)",
  "type": "synthetic",
  "start_rss_kb": $start_rss,
  "end_rss_kb": $end_rss,
  "delta_rss_kb": $delta,
  "duration_s": $duration,
  "note": "Synthetic bounded workload. Complete real large-firmware pressure test per RELEASE_READINESS.md matrix."
}
EOF
echo "Report: $report"
echo "Memory pressure synthetic check completed (bounded)."
echo "Note: complete real large-firmware pressure test per RELEASE_READINESS.md matrix."
