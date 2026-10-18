#!/usr/bin/env python3
"""Validate a source reconstruction artifact or inspect its evidence graph."""

import argparse
import json
import sys

from reconstruction_artifact import ArtifactError, ReconstructionArtifact, reconstruction_digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact")
    parser.add_argument("--node", help="content identity to query")
    parser.add_argument("--relation", help="restrict graph traversal to an edge relation")
    parser.add_argument("--direction", choices=["incoming", "outgoing"], default="incoming")
    parser.add_argument("--depth", type=int, default=8)
    parser.add_argument("--limit", type=int, default=1000)
    args = parser.parse_args()
    try:
        artifact = ReconstructionArtifact.load(args.artifact)
        if args.node:
            result = artifact.query(args.node, args.relation, args.direction, args.depth, args.limit)
        else:
            data = artifact.data
            result = {"valid": True, "candidate_sha256": reconstruction_digest(data),
                      "scope": data["scope"], "eligibility": data["eligibility"],
                      "nodes": len(data["nodes"]), "edges": len(data["edges"]),
                      "ast_nodes": len(data["ast"]["nodes"])}
        print(json.dumps(result, sort_keys=True, indent=2))
        return 0
    except (ArtifactError, OSError) as error:
        print(f"invalid reconstruction: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
