#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
rew_install_error_trap "Build Rust bridge + worker"
resolve_cargo
python3 "$ROOT/scripts/check-lockfiles.py"
resolve_rustup

[[ "$(uname -s)" == "Darwin" ]] || rew_die "Rust app artifacts require macOS."
configuration="${CONFIGURATION:-Debug}"
rew_phase_banner "Build Rust bridge + worker"
resolve_macos_sdk
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
export CC="$(/usr/bin/xcrun --sdk macosx -f clang)"
export AR="$(/usr/bin/xcrun --sdk macosx -f ar)"

if [[ -n "${CONFIGURATION_TEMP_DIR:-}" ]]; then work_root="$CONFIGURATION_TEMP_DIR/HindSightRust"
elif [[ -n "${DERIVED_FILE_DIR:-}" ]]; then work_root="$DERIVED_FILE_DIR/HindSightRust"
else work_root="$ROOT/build"; fi
export CARGO_TARGET_DIR="$work_root/target"
export CARGO_TERM_COLOR=never

arches=()
add_arch() {
  local a="$1" x
  [[ "$a" == "arm64" || "$a" == "x86_64" ]] || return 0
  for x in "${arches[@]:-}"; do [[ "$x" == "$a" ]] && return 0; done
  arches+=("$a")
}
if [[ "${ONLY_ACTIVE_ARCH:-NO}" == "YES" && ( "${CURRENT_ARCH:-}" == "arm64" || "${CURRENT_ARCH:-}" == "x86_64" ) ]]; then
  add_arch "$CURRENT_ARCH"
elif [[ -n "${ARCHS:-}" ]]; then
  for a in $ARCHS; do add_arch "$a"; done
fi
if [[ ${#arches[@]} -eq 0 ]]; then
  if [[ "${CURRENT_ARCH:-}" == "arm64" || "${CURRENT_ARCH:-}" == "x86_64" ]]; then add_arch "$CURRENT_ARCH"
  elif [[ "${NATIVE_ARCH_ACTUAL:-}" == "arm64" || "${NATIVE_ARCH_ACTUAL:-}" == "x86_64" ]]; then add_arch "$NATIVE_ARCH_ACTUAL"
  else add_arch "$(uname -m)"; fi
fi
[[ ${#arches[@]} -gt 0 ]] || rew_die "no supported macOS architectures resolved from ARCHS/CURRENT_ARCH"

if [[ "$configuration" == "Release" ]]; then profile=release; release_flag=1; else profile=debug; release_flag=0; fi
bridge_parts=(); worker_parts=()
for arch in "${arches[@]}"; do
  case "$arch" in arm64) target="aarch64-apple-darwin" ;; x86_64) target="x86_64-apple-darwin" ;; *) rew_die "unsupported architecture '$arch'" ;; esac
  target_env="$(/bin/echo "$target" | /usr/bin/tr '[:lower:]-' '[:upper:]_')"
  linker_key="CARGO_TARGET_${target_env}_LINKER"
  export "${linker_key}=$CC"
  if [[ -n "$RUSTUP" ]] && ! "$RUSTUP" target list --installed | /usr/bin/grep -qx "$target"; then
    rew_die "Rust target '$target' is not installed. Run: $RUSTUP target add $target"
  fi
  args=(build --manifest-path "$ROOT/rust/Cargo.toml" --locked --jobs 2 --target "$target" -p reworkbench-bridge -p reworkbench-worker)
  if [[ "${VECTORGLASS_CARGO_OFFLINE:-${REWORKBENCH_CARGO_OFFLINE:-1}}" != "0" ]]; then args+=(--offline); fi
  [[ $release_flag -eq 1 ]] && args+=(--release)
  echo "HindSight Rust build: configuration=$configuration arch=$arch target=$target deployment=$MACOSX_DEPLOYMENT_TARGET"
  echo "cargo command: $CARGO ${args[*]}"
  "$CARGO" "${args[@]}"
  bridge_src="$CARGO_TARGET_DIR/$target/$profile/libreworkbench_bridge.a"
  worker_src="$CARGO_TARGET_DIR/$target/$profile/reworker"
  [[ -f "$bridge_src" ]] || rew_die "bridge archive missing: $bridge_src"
  [[ -x "$worker_src" ]] || rew_die "worker executable missing: $worker_src"
  bridge_parts+=("$bridge_src"); worker_parts+=("$worker_src")
done

if [[ -n "${CONFIGURATION_TEMP_DIR:-}" ]]; then out="$CONFIGURATION_TEMP_DIR/HindSightRustArtifacts"; else out="$ROOT/build/bridge/$configuration"; fi
/bin/mkdir -p "$out"
if [[ ${#arches[@]} -eq 1 ]]; then
  /usr/bin/ditto --noqtn "${bridge_parts[0]}" "$out/libreworkbench_bridge.a"
  /usr/bin/ditto --noqtn "${worker_parts[0]}" "$out/reworker"
else
  /usr/bin/lipo -create "${bridge_parts[@]}" -output "$out/libreworkbench_bridge.a"
  /usr/bin/lipo -create "${worker_parts[@]}" -output "$out/reworker"
fi
/bin/chmod 755 "$out/reworker"
/usr/bin/xattr -c "$out/libreworkbench_bridge.a" "$out/reworker" 2>/dev/null || true

echo "bridge artifact: $out/libreworkbench_bridge.a"; /usr/bin/file "$out/libreworkbench_bridge.a"
echo "worker artifact: $out/reworker"; /usr/bin/file "$out/reworker"
if [[ ${#arches[@]} -gt 1 ]]; then
  [[ "$(/usr/bin/lipo -archs "$out/reworker")" == *arm64* && "$(/usr/bin/lipo -archs "$out/reworker")" == *x86_64* ]] || rew_die "universal worker is missing an architecture slice"
fi
echo "=== HindSight Research Tool: Rust build completed for ${arches[*]} ==="
