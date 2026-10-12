#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"
rew_install_error_trap "Build VectorEmu"
resolve_cargo
resolve_rustup
if [[ -n "${RUSTUP:-}" ]]; then
  export RUSTUP_TOOLCHAIN="$($RUSTUP show active-toolchain | /usr/bin/awk '{print $1}')"
  export CARGO="$($RUSTUP which cargo)"
fi
python3 "$ROOT/scripts/check-lockfiles.py"
python3 "$ROOT/scripts/check-rust-msrv.py"
[[ "$(uname -s)" == "Darwin" ]] || rew_die "VectorEmu app artifacts require macOS."
# Match the bridge/worker Mach-O layout workaround. Without explicit header
# padding, Xcode 27's linker can produce unloadable Rust proc-macro dylibs.
unset RUSTC RUSTC_WRAPPER RUSTFLAGS CARGO_ENCODED_RUSTFLAGS DYLD_LIBRARY_PATH DYLD_FALLBACK_LIBRARY_PATH
configuration="${CONFIGURATION:-Release}"
resolve_macos_sdk
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-14.0}"
export RUSTFLAGS="-C link-arg=-Wl,-headerpad_max_install_names"
macos_clang="$(/usr/bin/xcrun --sdk macosx -f clang)"
macos_ar="$(/usr/bin/xcrun --sdk macosx -f ar)"
unset CC AR
ENGINE="$ROOT/Engines/VectorEmu"
if [[ -n "${1:-}" ]]; then OUT="$1"; elif [[ -n "${TARGET_BUILD_DIR:-}" && -n "${EXECUTABLE_FOLDER_PATH:-}" ]]; then OUT="$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH"; else OUT="${TARGET_BUILD_DIR:-$ROOT/build}/VectorEmu"; fi
[[ -n "$OUT" && "$OUT" != "/" ]] || rew_die "refusing unsafe VectorEmu output directory: '$OUT'"
if [[ -n "${TARGET_BUILD_DIR:-}" && -n "${EXECUTABLE_FOLDER_PATH:-}" ]]; then
  case "$OUT" in
    "$TARGET_BUILD_DIR"/*) ;;
    *) rew_die "VectorEmu output must remain inside TARGET_BUILD_DIR when invoked by Xcode: $OUT" ;;
  esac
fi
if [[ -n "${TARGET_TEMP_DIR:-}" ]]; then work_root="$TARGET_TEMP_DIR/HindSightVectorEmu"; elif [[ -n "${DERIVED_FILE_DIR:-}" ]]; then work_root="$DERIVED_FILE_DIR/HindSightVectorEmu"; else work_root="$ROOT/build/vector-emu"; fi
export CARGO_TARGET_DIR="$work_root/target"; export CARGO_TERM_COLOR=never

arches=()
add_arch(){ local a="$1" x; [[ "$a" == "arm64" || "$a" == "x86_64" ]] || return 0; for x in "${arches[@]:-}"; do [[ "$x" == "$a" ]] && return 0; done; arches+=("$a"); }
if [[ "${ONLY_ACTIVE_ARCH:-NO}" == "YES" && ( "${CURRENT_ARCH:-}" == "arm64" || "${CURRENT_ARCH:-}" == "x86_64" ) ]]; then add_arch "$CURRENT_ARCH"; elif [[ -n "${ARCHS:-}" ]]; then for a in $ARCHS; do add_arch "$a"; done; fi
if [[ ${#arches[@]} -eq 0 ]]; then add_arch "${CURRENT_ARCH:-$(uname -m)}"; fi
[[ ${#arches[@]} -gt 0 ]] || rew_die "no supported VectorEmu architecture resolved"
if [[ "$configuration" == "Release" ]]; then
  profile=release
else
  profile=debug
fi
parts=()
for arch in "${arches[@]}"; do
  case "$arch" in arm64) target="aarch64-apple-darwin" ;; x86_64) target="x86_64-apple-darwin" ;; esac
  target_env="$(/bin/echo "$target" | /usr/bin/tr '[:lower:]-' '[:upper:]_')"; linker_key="CARGO_TARGET_${target_env}_LINKER"
  cc_key="CC_${target//-/_}"; ar_key="AR_${target//-/_}"
  export "${linker_key}=$macos_clang" "${cc_key}=$macos_clang" "${ar_key}=$macos_ar"
  if [[ -n "$RUSTUP" ]] && ! "$RUSTUP" target list --installed | /usr/bin/grep -qx "$target"; then rew_die "Rust target '$target' is not installed. Run: $RUSTUP target add $target"; fi
  args=(build --manifest-path "$ENGINE/Cargo.toml" --locked --jobs 2 --target "$target" -p vg-emu-server); [[ "$configuration" == "Release" ]] && args+=(--release)
  echo "VectorEmu build: configuration=$configuration arch=$arch target=$target"
  "$CARGO" "${args[@]}"
  src="$CARGO_TARGET_DIR/$target/$profile/vg-emu-server"; [[ -x "$src" ]] || rew_die "VectorEmu executable missing: $src"; parts+=("$src")
done
/bin/rm -rf "$OUT/vg-emu-server"
/bin/mkdir -p "$OUT"
if [[ ${#parts[@]} -eq 1 ]]; then /usr/bin/ditto --noqtn "${parts[0]}" "$OUT/vg-emu-server"; else /usr/bin/lipo -create "${parts[@]}" -output "$OUT/vg-emu-server"; fi
/bin/chmod 755 "$OUT/vg-emu-server"; /usr/bin/xattr -c "$OUT/vg-emu-server" 2>/dev/null || true

# Rust's linker leaves an ad-hoc "linker-signed" CodeDirectory that `codesign --verify`
# rejects outright ("code object is not signed at all"). Xcode's CodeSign step seals
# CoordinatorService.xpc bottom-up and aborts while a nested executable is not a valid
# signed code object, so sign the staged engine exactly like the Rust worker is signed.
if [[ "${CODE_SIGNING_ALLOWED:-YES}" != "NO" ]]; then
    identity="${EXPANDED_CODE_SIGN_IDENTITY:-}"
    if [[ -z "$identity" || "$identity" == "-" ]]; then
        identity='-'
        echo "VectorEmu signing: ad-hoc"
    else
        echo "VectorEmu signing identity: ${EXPANDED_CODE_SIGN_IDENTITY_NAME:-$identity}"
    fi

    sign_args=(--force --sign "$identity" --timestamp=none)
    if [[ "$identity" != "-" && "${ENABLE_HARDENED_RUNTIME:-NO}" == "YES" ]]; then
        sign_args+=(--options runtime)
    fi

    /usr/bin/codesign "${sign_args[@]}" "$OUT/vg-emu-server"
    /usr/bin/codesign --verify --strict --verbose=4 "$OUT/vg-emu-server"
else
    echo "VectorEmu signing: skipped because CODE_SIGNING_ALLOWED=NO"
fi

echo "VectorEmu staged at $OUT/vg-emu-server"; /usr/bin/file "$OUT/vg-emu-server"
