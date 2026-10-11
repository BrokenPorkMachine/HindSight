#!/usr/bin/env python3
from __future__ import annotations
from pathlib import Path
import re, tomllib

ROOT = Path(__file__).resolve().parents[1]
ENGINE = ROOT / "Engines/VectorEmu"
EXPECTED = "1.90"

# Integer convenience APIs introduced after Rust 1.90 and known to be tempting in
# low-level bit manipulation. This is a source regression guard; the authoritative
# compatibility gate remains compiling the workspace with the pinned 1.90 toolchain.
POST_190_TOKENS = {
    ".isolate_lowest_one(": "1.97",
    ".isolate_highest_one(": "1.97",
    ".bit_width(": "1.97",
}

errors: list[str] = []
for manifest in sorted((ENGINE / "crates").glob("*/Cargo.toml")):
    data = tomllib.loads(manifest.read_text())
    got = str(data.get("package", {}).get("rust-version", ""))
    if got != EXPECTED:
        errors.append(f"{manifest.relative_to(ROOT)}: rust-version={got!r}, expected {EXPECTED!r}")

for source in sorted((ENGINE / "crates").rglob("*.rs")):
    text = source.read_text()
    for token, stabilized in POST_190_TOKENS.items():
        if token in text:
            errors.append(
                f"{source.relative_to(ROOT)}: uses {token[:-1]} stabilized in Rust {stabilized}; "
                f"VectorEmu MSRV is Rust {EXPECTED}"
            )

rt = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())
channel = str(rt.get("toolchain", {}).get("channel", ""))
if channel != "1.90.0":
    errors.append(f"rust-toolchain.toml pins {channel!r}, expected '1.90.0'")

if errors:
    for error in errors:
        print(f"error: {error}")
    raise SystemExit(2)
print("VectorEmu Rust 1.90 MSRV source preflight passed.")
