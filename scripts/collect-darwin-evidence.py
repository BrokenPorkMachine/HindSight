#!/usr/bin/env python3
"""Collect image-qualified libobjc/dyld metadata from an isolated native helper."""

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE = ROOT / "native/reconstruction/darwin_catalog.m"


def compile_helper(output, extra_sources=()):
    command = ["xcrun", "--sdk", "macosx", "clang", "-std=c17", "-fobjc-arc", "-fblocks", "-O2",
               "-Wall", "-Wextra", "-Werror", "-mmacosx-version-min=14.0", str(SOURCE),
               *map(str, extra_sources), "-framework", "Foundation", "-framework", "CoreFoundation",
               "-framework", "CoreGraphics", "-lobjc", "-o", str(output)]
    subprocess.run(command, check=True, capture_output=True, text=True, timeout=60)


def collect(helper, classes, symbols):
    if len(classes) > 32 or len(symbols) > 128:
        raise ValueError("native catalog request exceeds class/symbol budget")
    for name in classes + symbols:
        if not name or len(name.encode()) > 1024 or any(ord(ch) < 32 for ch in name):
            raise ValueError("invalid native catalog name")
    command = [str(helper)]
    for name in classes:
        command.extend(["--class", name])
    for name in symbols:
        command.extend(["--symbol", name])
    # The helper enforces an 8 MiB output cap and never loads a requested file.
    result = subprocess.run(command, check=True, capture_output=True, timeout=30)
    if len(result.stdout) > 8 * 1024 * 1024:
        raise ValueError("native catalog output exceeds byte budget")
    document = json.loads(result.stdout)
    document["helper_source_sha256"] = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
    document["helper_sha256"] = hashlib.sha256(pathlib.Path(helper).read_bytes()).hexdigest()
    document["sdk_path"] = subprocess.run(["xcrun", "--sdk", "macosx", "--show-sdk-path"], check=True,
                                           capture_output=True, text=True, timeout=10).stdout.strip()
    return document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--class", dest="classes", action="append")
    parser.add_argument("--symbol", dest="symbols", action="append")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("Darwin native catalog collection requires macOS")
    with tempfile.TemporaryDirectory(prefix="hindsight-darwin-catalog-") as directory:
        helper = pathlib.Path(directory) / "catalog"
        compile_helper(helper)
        document = collect(helper, args.classes or ["NSObject", "NSString", "NSNumber"],
                           args.symbols or ["malloc", "objc_msgSend", "_dyld_image_count"])
        print(json.dumps(document, sort_keys=True, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        detail = getattr(error, "stderr", None)
        print(f"Darwin evidence collection failed: {detail or error}", file=sys.stderr)
        raise SystemExit(1)
