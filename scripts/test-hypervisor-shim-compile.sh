#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
src="$ROOT/native/virtualization/hv_vm.c"
if [[ "$(uname -s)" == "Darwin" ]]; then
  for arch in arm64 x86_64; do
    xcrun --sdk macosx clang -arch "$arch" -mmacosx-version-min=14.0 -std=c11 -Wall -Wextra -Werror=implicit-function-declaration -fsyntax-only "$src"
    printf '%s\n' "Hypervisor shim syntax check passed for $arch against the macOS SDK."
  done
  exit 0
fi
clang_bin="${CLANG:-$(command -v clang || true)}"; [[ -n "$clang_bin" ]] || { echo "HindSight: clang is required." >&2; exit 1; }
"$clang_bin" -D__APPLE__=1 -D__arm64__=1 -D_POSIX_C_SOURCE=200112L -std=c11 -Wall -Wextra -Werror=implicit-function-declaration -I "$ROOT/Tests/stubs" -fsyntax-only "$src"
"$clang_bin" -D__APPLE__=1 -D__x86_64__=1 -D_POSIX_C_SOURCE=200112L -std=c11 -Wall -Wextra -Werror=implicit-function-declaration -fsyntax-only "$src"
printf '%s\n' "Hypervisor shim compile checks passed for Apple-Silicon implementation and Intel fallback."
