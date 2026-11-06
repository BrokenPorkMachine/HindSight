#!/usr/bin/env python3
"""Compile and run the real REA Coordinator bridge against an approved local target."""
import pathlib
import shutil
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
rea = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else (shutil.which("rea-agents") or ""))
target = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "/usr/bin/true")
if not rea.is_absolute() or not rea.is_file():
    raise SystemExit("rea-agents executable was not found")
if not target.is_absolute() or not target.is_file():
    raise SystemExit("qualification target must be an existing absolute file")

with tempfile.TemporaryDirectory(prefix="hindsight-rea-live-") as directory:
    temp = pathlib.Path(directory)
    executable = temp / "rea-agent-host-live"
    subprocess.run([
        "swiftc", "-module-cache-path", str(temp / "module-cache"), "-O",
        str(root / "Shared/REAAgentMessages.swift"),
        str(root / "PlatformServices/CoordinatorService/REAAgentHost.swift"),
        str(root / "Tests/rea_agent_host_live.swift"), "-o", str(executable),
    ], check=True)
    subprocess.run([str(executable), str(rea), str(target)], check=True)
