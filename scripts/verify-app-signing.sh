#!/bin/bash
set -euo pipefail

app="${1:-${TARGET_BUILD_DIR:-}/${FULL_PRODUCT_NAME:-HindSight.app}}"
if [[ ! -d "$app" ]]; then
    echo "error: app bundle not found: $app" >&2
    exit 2
fi

echo "== Bundle root =="
find "$app" -mindepth 1 -maxdepth 1 -print

# A canonical macOS .app should contain Contents at its root and nothing else.
mapfile_cmd=false
if command -v mapfile >/dev/null 2>&1; then mapfile_cmd=true; fi
extra=()
while IFS= read -r entry; do
    [[ "$(basename "$entry")" == "Contents" ]] || extra+=("$entry")
done < <(find "$app" -mindepth 1 -maxdepth 1 -print)

if (( ${#extra[@]} )); then
    echo "error: unexpected item(s) in app bundle root:" >&2
    printf '  %s\n' "${extra[@]}" >&2
    exit 3
fi

echo "== Nested code signatures =="
while IFS= read -r nested; do
    echo "verify: $nested"
    /usr/bin/codesign --verify --strict --verbose=2 "$nested"
done < <(find "$app/Contents" -type d \( -name '*.xpc' -o -name '*.framework' -o -name '*.appex' \) -prune -print)

echo "== App signature =="
/usr/bin/codesign --verify --deep --strict --verbose=4 "$app"
/usr/bin/codesign -d --entitlements :- "$app" 2>/dev/null || true
echo "Signing verification passed: $app"
