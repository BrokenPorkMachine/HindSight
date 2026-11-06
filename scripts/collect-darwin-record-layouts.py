#!/usr/bin/env python3
"""Emit target-qualified Apple Clang C record layout evidence as JSON."""

import argparse
import json
from pathlib import Path
import re
import sys

from darwin_sdk_catalog import collect_record_layouts, MAX_LAYOUT_SOURCE_BYTES


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("declarations", type=Path, help="C declaration context used to define the requested records")
    parser.add_argument("--record", action="append", required=True, help="record tag to qualify; repeat for multiple records")
    parser.add_argument("--sdk", choices=["macosx", "iphoneos", "iphonesimulator"], default="macosx")
    parser.add_argument("--target", default="arm64-apple-macos14.0")
    parser.add_argument("--project-source-sha256", help="also emit an importable one-candidate project type catalog bound to this captured source")
    args = parser.parse_args()
    with args.declarations.open("rb") as stream:
        source = stream.read(MAX_LAYOUT_SOURCE_BYTES + 1)
    if len(source) > MAX_LAYOUT_SOURCE_BYTES:
        raise ValueError("record declaration context exceeds its 1 MiB limit")
    declarations = source.decode("utf-8", errors="strict")
    result = collect_record_layouts(declarations, args.record, args.sdk, args.target)
    if args.project_source_sha256:
        if (len(args.record) != 1
                or not re.fullmatch(r"[0-9a-f]{64}", args.project_source_sha256)):
            raise ValueError("catalog export requires one selected record and a lowercase captured-source SHA-256")
        record = result["records"][0]
        layout = {
            "schema": result["schema"], "producer": result["producer"], "sdk": result["sdk"],
            "sdkPath": result["sdk_path"], "sdkBuild": result["sdk_build"], "target": result["target"],
            "compiler": result["compiler"], "declarationsSHA256": result["declarations_sha256"],
            "record": {
                "name": record["name"], "kind": record["kind"], "size": record["size"],
                "alignment": record["alignment"], "fieldOffsetsComplete": record["field_offsets_complete"],
                "fields": [{"name": field["name"], "declaration": field["declaration"],
                            "offsetBits": field["offset_bits"], "bitWidth": field["bit_width"]}
                           for field in record["fields"]],
            },
        }
        result = {"schema": 1, "candidates": [{
            "name": record["name"], "declaration": declarations, "target": args.target,
            "source": "analyst", "sourceSHA256": args.project_source_sha256,
            "layoutEvidence": layout,
        }]}
    print(json.dumps(result, sort_keys=True, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, UnicodeError, ValueError) as error:
        print(f"Apple Clang record layout failed: {error}", file=sys.stderr)
        raise SystemExit(1)
