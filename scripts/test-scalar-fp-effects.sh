#!/usr/bin/env bash
# Measure how scalar floating-point instructions affect the upper bits of their
# destination vector register on this host.
#
# The result is a property of the machine, so the measurement runs natively on
# an AArch64 host. The evidence and the model it supports are recorded in
# docs/AARCH64_SCALAR_FP_EFFECTS.md.
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
work_dir="$(mktemp -d /private/tmp/hindsight-scalar-fp.XXXXXX)"
cleanup() { rm -f "$work_dir/probe"; rmdir "$work_dir" 2>/dev/null || true; }
trap cleanup EXIT

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Scalar FP upper-bit effects need an AArch64 host; skipped on $(uname -m)." >&2
  exit 0
fi

clang -O0 -o "$work_dir/probe" "$project_root/scripts/probe-scalar-fp-effects.c"
"$work_dir/probe"
