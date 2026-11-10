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
assert '"snapshot": {"selection": studio.selection, "analysis": studio.analysis}' in main_source
assert "supported_node" not in main_source
host_source = (root / "PlatformServices/CoordinatorService/ScriptingHost.swift").read_text()
project_source = (root / "scripts/generate-project.py").read_text()
assert 'resources.appendingPathComponent("BundledPlugins"' in host_source
assert "bundled_plugins_build" in project_source
extension_source = (bundle / "runtime/cli/extension.js").read_text()
assert "hindsight_context" in extension_source
assert "new Registry([snapshot, todo])" in extension_source
assert "BEGIN_HINDSIGHT_EVIDENCE" not in extension_source

# The packaged setup helpers must resolve the MCP server within the bundle,
# rather than relying on a source-checkout-only path.
mcp_server = bundle / "runtime/mcp/hindsight-mcp.mjs"
assert mcp_server.is_file()
mcp_source = mcp_server.read_text()
assert mcp_source == (root / "mcp/hindsight-mcp.mjs").read_text(), "bundled MCP server drifted from source"
subprocess.run(["node", "--check", str(mcp_server)], check=True, capture_output=True, text=True)
for tool in ["hindsight_capabilities", "hindsight_project", "hindsight_analysis_submit",
             "hindsight_analysis_status", "hindsight_analysis_result", "hindsight_analysis_cancel"]:
    assert tool in mcp_source
assert "HINDSIGHT_CERT_SHA256" in mcp_source and "checkServerIdentity" in mcp_source
assert "HINDSIGHT_CA_PEM" in mcp_source
for setup in [bundle / "setup/setup-warhead-mcp-macos.sh", bundle / "setup/setup-warhead-mcp-linux.sh"]:
    assert 'runtime/mcp/hindsight-mcp.mjs' in setup.read_text()
assert 'runtime/mcp/hindsight-mcp.mjs' in (bundle / "setup/setup-warhead-mcp-windows.ps1").read_text()

hindsight_client = bundle / "runtime/rea/hindsight-mcp-client.js"
assert hindsight_client.is_file()
hindsight_client_source = hindsight_client.read_text()
assert "Library', 'Caches', 'HindSight', 'local-gateway.sock'" in hindsight_client_source
assert '{"action":"ensure"}' in hindsight_client_source
assert "HINDSIGHT_CA_PEM" in hindsight_client_source and "HINDSIGHT_CERT_SHA256" in hindsight_client_source
assert "HINDSIGHT_URL" in hindsight_client_source and "HINDSIGHT_TOKEN" in hindsight_client_source
assert "certificateSHA256" in hindsight_client_source and "BEGIN CERTIFICATE" in hindsight_client_source
repl_source = (bundle / "runtime/cli/repl.js").read_text()
assert "HindsightMcpClient" in repl_source and "HindSight MCP" in repl_source
assert "hindsightMcp?.close" in repl_source
extension_source = (bundle / "runtime/cli/extension.js").read_text()
assert "hindsight-mcp-client.js" in extension_source or "hindsightStatus" in extension_source or "HindSight" in extension_source

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
