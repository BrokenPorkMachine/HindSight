#!/usr/bin/env python3
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="hindsight-rea-fixture-") as directory:
    temp = pathlib.Path(directory)
    target = temp / "fixture.bin"
    target.write_bytes(b"deterministic REA fixture")
    executable = temp / "rea-agent-host-contract"
    subprocess.run([
        "swiftc", "-module-cache-path", str(temp / "module-cache"), "-O",
        str(root / "Shared/REAAgentMessages.swift"),
        str(root / "PlatformServices/CoordinatorService/REAAgentHost.swift"),
        str(root / "Tests/rea_agent_host_contract.swift"),
        "-o", str(executable),
    ], check=True)
    subprocess.run([str(executable), str(root / "Tests/Fixtures/rea_mcp_fixture.py"), str(target)], check=True)
