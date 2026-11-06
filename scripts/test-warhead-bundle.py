#!/usr/bin/env python3
"""Static contract for the governed Warhead extension staged in HindSight."""
import json
import pathlib
import re
import subprocess

root = pathlib.Path(__file__).resolve().parents[1]
bundle = root / "BundledPlugins" / "Warhead.vgplugin"
entries = list(bundle.rglob("*"))
assert len(entries) <= 2048
assert not any(entry.is_symlink() for entry in entries)
assert sum(entry.stat().st_size for entry in entries if entry.is_file()) <= 16 * 1024 * 1024
manifest = json.loads((bundle / "plugin.json").read_text())
package = json.loads((bundle / "package.json").read_text())

assert manifest["api_version"] == 2
assert manifest["id"] == "com.brokenporkmachine.hindsight.warhead"
assert (bundle / manifest["entry"]).is_file()
assert package["type"] == "module"
assert package["dependencies"]["undici"] == "7.30.1"
assert (bundle / "node_modules/undici/package.json").is_file()
assert {"selection.read", "analysis.read", "settings.read", "state.read", "state.write"} <= set(manifest["capabilities"])

main_source = (bundle / manifest["entry"]).read_text()
compile(main_source, str(bundle / manifest["entry"]), "exec")
assert 'studio.context.get("rea_evidence", [])' in main_source
assert "supported_node" in main_source
host_source = (root / "PlatformServices/CoordinatorService/ScriptingHost.swift").read_text()
project_source = (root / "scripts/generate-project.py").read_text()
assert 'resources.appendingPathComponent("BundledPlugins"' in host_source
assert "bundled_plugins_build" in project_source
extension_source = (bundle / "runtime/cli/extension.js").read_text()
assert "new Registry([])" in extension_source
assert "BEGIN_HINDSIGHT_EVIDENCE" in extension_source

runtime = bundle / "runtime"
javascript = sorted(runtime.rglob("*.js"))
assert javascript
for source in javascript:
    subprocess.run(["node", "--check", str(source)], check=True, capture_output=True, text=True)
    text = source.read_text()
    assert "rea/scripts/rea.mjs" not in text
    assert "npx -y" not in text
    for specifier in re.findall(r"(?:from\s+|import\s*\()['\"](\.{1,2}/[^'\"]+)['\"]", text):
        target = (source.parent / specifier).resolve()
        assert target.is_file(), f"missing local import: {source.relative_to(bundle)} -> {specifier}"

print(f"Warhead bundle contract passed ({len(javascript)} JavaScript modules)")
