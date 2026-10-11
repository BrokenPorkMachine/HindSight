from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]

def read(path: str) -> str:
    return (ROOT / path).read_text()

def test_xcode_and_rust_are_universal():
    generator = read('scripts/generate-project.py')
    rust = read('scripts/build-rust.sh')
    emu = read('scripts/build-vector-emu.sh')
    assert "'ARCHS':'arm64 x86_64'" in generator
    for text in (rust, emu):
        assert 'aarch64-apple-darwin' in text
        assert 'x86_64-apple-darwin' in text
        assert '/usr/bin/lipo -create' in text

def test_host_capability_model_preserves_cross_host_analysis():
    host = read('Shared/HostPlatform.swift')
    assert 'case arm64' in host and 'case x86_64' in host
    assert 'supportsAppleSiliconHypervisor' in host
    assert 'supportsArm64NativeDebugger' in host
    assert 'supportsVectorEmu' in host
    # Static ARM64 analysis is intentionally not conditioned on host architecture.
    assert 'cross-host' in read('docs/INTEL_MAC_COMPATIBILITY.md')

def test_apple_silicon_features_are_gated_on_intel():
    ui = read('App/ContentView.swift')
    app = read('App/HindSightApp.swift')
    debug = read('PlatformServices/CoordinatorService/DebugServer.swift')
    hv = read('PlatformServices/CoordinatorService/VirtualizationHost.swift')
    native_debug = read('native/debug/debug_server.c')
    assert 'VGHostPlatform.supportsArm64NativeDebugger' in ui
    assert 'VGHostPlatform.supportsAppleSiliconHypervisor' in ui
    # Menu navigation must be closed out too; workspace rendering alone is not enough.
    assert '.disabled(!VGHostPlatform.supportsArm64NativeDebugger)' in app
    # No-artifact routes must use the same host gates instead of instantiating live workspaces directly.
    assert 'if VGHostPlatform.supportsArm64NativeDebugger' in ui
    assert 'if VGHostPlatform.supportsAppleSiliconHypervisor' in ui
    assert 'supportsArm64NativeDebugger' in debug
    assert 'supportsAppleSiliconHypervisor' in hv
    assert '#if !defined(__APPLE__) || !defined(__arm64__)' in native_debug

def test_host_diagnostics_explain_capability_and_runtime_boundaries():
    host = read('Shared/HostPlatform.swift')
    model = read('App/WorkbenchModel.swift')
    ui = read('App/ContentView.swift')
    assert 'nativeDebuggerStatus' in host
    assert 'hypervisorStatus' in host
    assert 'debugging permission is not probed until attach' in host
    assert 'entitlement/runtime availability is not probed until start' in host
    assert 'arm64_native_debugger_status=' in model
    assert 'apple_silicon_hypervisor_status=' in model
    assert 'unavailable on this host' in ui

def test_typecheck_and_release_gates_cover_both_architectures():
    for script in ('scripts/test-app-typecheck.sh', 'scripts/test-coordinator-typecheck.sh'):
        text = read(script)
        assert 'for arch in arm64 x86_64' in text
    assert 'ARCHS=\'arm64 x86_64\'' in read('scripts/build-app.sh')
    beta = read('scripts/test-beta-macos.sh')
    assert 'lipo -archs' in beta
    assert 'native Apple Silicon terminal' not in beta
    assert 'native Apple Silicon terminal' not in read('scripts/test-release-macos.sh')
    doctor = read('scripts/doctor.sh')
    assert 'aarch64-apple-darwin x86_64-apple-darwin' in doctor
    assert 'Use a native Apple Silicon terminal' not in doctor

def test_host_platform_is_in_both_targets():
    topology = json.loads(read('Config/project-topology.json'))
    assert 'Shared/HostPlatform.swift' in topology['targets']['HindSight']
    assert 'Shared/HostPlatform.swift' in topology['targets']['CoordinatorService']
    generator = read('scripts/generate-project.py')
    assert "'Shared/HostPlatform.swift'" in generator
