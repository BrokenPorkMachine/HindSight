from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def test_external_process_hosts_bound_descendant_cleanup_to_process_groups():
    sources = [
        ROOT / "PlatformServices/CoordinatorService/ScriptingHost.swift",
        ROOT / "PlatformServices/CoordinatorService/REAAgentHost.swift",
        ROOT / "PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift",
    ]
    for source_path in sources:
        source = source_path.read_text(encoding="utf-8")
        assert "setpgid" in source, source_path
        assert "kill(-pid" in source, source_path
        assert "SIGKILL" in source, source_path
