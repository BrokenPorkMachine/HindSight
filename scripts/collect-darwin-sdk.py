#!/usr/bin/env python3
"""Import Apple SDK C prototypes, optionally as a hash-bound worker catalog."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from darwin_sdk_catalog import DEFAULT_HEADER, collect_declarations, worker_prototypes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--function", action="append", required=True, dest="functions")
    parser.add_argument("--header", type=Path, default=DEFAULT_HEADER)
    parser.add_argument("--sdk", choices=["macosx", "iphoneos", "iphonesimulator"], default="macosx")
    parser.add_argument("--target", default="arm64-apple-macos14.0")
    parser.add_argument("--prototype-source", type=Path, help="emit worker TSV bound to this binary's SHA-256")
    args = parser.parse_args()
    catalog = collect_declarations(args.functions, args.header, args.sdk, args.target)
    if args.prototype_source:
        with args.prototype_source.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        print(worker_prototypes(catalog, digest), end="")
    else:
        print(json.dumps(catalog, sort_keys=True, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Apple SDK declaration import failed: {error}", file=sys.stderr)
        raise SystemExit(1)
