#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
resolve_cargo

echo "Regenerating HindSight core Cargo.lock…"
"$CARGO" generate-lockfile --manifest-path "$ROOT/rust/Cargo.toml"
echo "Regenerating VectorEmu Cargo.lock…"
"$CARGO" generate-lockfile --manifest-path "$ROOT/Engines/VectorEmu/Cargo.toml"
python3 "$ROOT/scripts/check-lockfiles.py"
echo "Review Cargo.lock diffs before committing or packaging."
