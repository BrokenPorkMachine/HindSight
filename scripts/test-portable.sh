#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
# This is the full portable release gate, so fail before the longer source/native
# suite when the pinned Rust toolchain is unavailable. `validate-source.py` remains
# independently runnable on hosts that only need the source/contract audit.
resolve_cargo
python3 "$ROOT/scripts/validate-source.py"
"$ROOT/scripts/test-hypervisor-shim-compile.sh"
python3 "$ROOT/scripts/check-lockfiles.py"
python3 "$ROOT/scripts/check-rust-msrv.py"
export CARGO_TARGET_DIR="$ROOT/build/rust"
"$CARGO" test --manifest-path "$ROOT/rust/Cargo.toml" --locked --offline --jobs 2
"$CARGO" clippy --manifest-path "$ROOT/rust/Cargo.toml" --locked --offline --all-targets -- -D warnings
"$CARGO" build --manifest-path "$ROOT/rust/Cargo.toml" --locked --offline --jobs 2 -p reworkbench-worker
python3 "$ROOT/scripts/test-worker.py" "$CARGO_TARGET_DIR/debug/reworker"

# Compile the built-in VectorEmu workspace as a first-class release gate.
# v0.2.6 shipped four unresolved `hex::` call sites because only the main
# Rust workspace was compiled here. Keep the engine lockfile and server source
# under the same reproducibility standard as the bridge/worker.
export CARGO_TARGET_DIR="$ROOT/build/vector-emu-rust"
engine_cargo_flags=(--locked --jobs 2)
if [[ "${VECTORGLASS_CARGO_OFFLINE:-0}" == "1" ]]; then
    engine_cargo_flags+=(--offline)
fi
"$CARGO" check --manifest-path "$ROOT/Engines/VectorEmu/Cargo.toml" "${engine_cargo_flags[@]}" -p vg-emu-server
"$CARGO" test --manifest-path "$ROOT/Engines/VectorEmu/Cargo.toml" "${engine_cargo_flags[@]}" -p vg-emu-server
