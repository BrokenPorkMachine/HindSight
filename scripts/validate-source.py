#!/usr/bin/env python3
import hashlib, json, os, pathlib, plistlib, re, subprocess, sys, tomllib
import xml.etree.ElementTree as ET
sys.dont_write_bytecode = True
from openstep import load_project
R=pathlib.Path(__file__).resolve().parents[1]
for path in R.rglob('*.json'):
    if 'build' not in path.relative_to(R).parts: json.loads(path.read_text())
for path in R.rglob('*.plist'): plistlib.loads(path.read_bytes())
for path in (R/'rust').rglob('Cargo.toml'): tomllib.loads(path.read_text())
tomllib.loads((R/'rust/Cargo.lock').read_text())
vector_emu = R/'Engines/VectorEmu'
for path in vector_emu.rglob('Cargo.toml'): tomllib.loads(path.read_text())
tomllib.loads((vector_emu/'Cargo.lock').read_text())
subprocess.run([sys.executable, str(R/'scripts/check-lockfiles.py')], check=True)
for suffix in ['*.xcscheme','*.xcworkspacedata']:
    for path in R.rglob(suffix): ET.parse(path)
topology=json.loads((R/'Config/project-topology.json').read_text())
for path in topology['files']: assert (R/path).is_file(),path
pbx=(R/'HindSight.xcodeproj/project.pbxproj').read_bytes()
assert hashlib.sha256(pbx).hexdigest()==topology['project_sha256'],'Regenerate project after topology edits'
project=load_project(pbx.decode())
objects=project['objects']
assert project['rootObject'] in objects
references={'buildConfigurationList','mainGroup','productRefGroup','productReference','fileRef','target','targetProxy','containerPortal','remoteGlobalIDString'}
arrays={'children','buildConfigurations','buildPhases','dependencies','targets','files'}
for item in objects.values():
    for key,val in item.items():
        if key in references: assert val in objects,(key,val)
        if key in arrays:
            for reference in val: assert reference in objects,(key,reference)
for fragment in [b'PBXTargetDependency' ,b'PBXCopyFilesBuildPhase',b'XPCServices',b'Build Rust bridge',b'Build VectorEmu']:
    assert fragment in pbx

# Xcode/Rust artifact handoff must be configuration-wide and macOS-path safe.
build_rust=(R/'scripts/build-rust.sh').read_text()
embed_worker=(R/'scripts/embed-worker.sh').read_text()
generator=(R/'scripts/generate-project.py').read_text()
for script in (R/'scripts').glob('*.sh'):
    assert '/usr/bin/mkdir' not in script.read_text(), f'macOS-unsafe mkdir path in {script}'
assert 'CONFIGURATION_TEMP_DIR/HindSightRustArtifacts' in build_rust
assert 'CONFIGURATION_TEMP_DIR/HindSightRustArtifacts/reworker' in embed_worker
assert '$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts' in generator
assert '$(TARGET_TEMP_DIR)/rew-rust-artifacts' not in generator
assert b'$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts/libreworkbench_bridge.a' in pbx
assert b'$(CONFIGURATION_TEMP_DIR)/HindSightRustArtifacts/reworker' in pbx
assert b'$(TARGET_TEMP_DIR)/rew-rust-artifacts' not in pbx

# Virtualization service compile-boundary regression guards.
svc_topology = topology['targets']['CoordinatorService']
assert 'Shared/VirtualizationMessages.swift' in svc_topology, 'virtualization messages missing from CoordinatorService target'
virt_host=(R/'PlatformServices/CoordinatorService/VirtualizationHost.swift').read_text()
assert 'private var vm: OpaquePointer?' in virt_host
assert 'UnsafeMutablePointer<rw_hv_vm_t>' not in virt_host
assert 'read(base:length)' not in virt_host
assert 'read(base:minLength' not in virt_host


# Scripting host compile-regression guards for the Swift 6 diagnostics fixed in 0.2.4-alpha.
scripting_host=(R/'PlatformServices/CoordinatorService/ScriptingHost.swift').read_text()
assert 'DispatchTime.now() + DispatchTimeInterval.milliseconds(timeout)' in scripting_host
assert 'DispatchTime.now() + DispatchTimeInterval.seconds(1)' in scripting_host
assert '== .orderedAscending' in scripting_host
assert '[[String: Any]]' in scripting_host
assert 'timeout:.now()+.milliseconds' not in scripting_host
assert 'timeout:.now()+.seconds' not in scripting_host
assert '==.orderedAscending' not in scripting_host
assert '$0.address ?? NSNull()' not in scripting_host

# Hypervisor.framework Apple-silicon API regression guards.
hv_shim=(R/'native/virtualization/hv_vm.c').read_text()
assert 'hv_vm_create(NULL)' in hv_shim, 'Apple-silicon hv_vm_create must receive a nil config'
assert 'hv_vm_create(HV_VM_DEFAULT)' not in hv_shim, 'HV_VM_DEFAULT is the Intel-style VM flag'
assert 'hv_vcpu_get_reg(vm->vcpu, HV_REG_SP' not in hv_shim, 'Apple-silicon Hypervisor has no HV_REG_SP general register'
assert 'HV_SYS_REG_SP_EL0' in hv_shim and 'HV_SYS_REG_SP_EL1' in hv_shim
assert 'hv_vcpu_get_sys_reg(vm->vcpu, sp_reg, sp)' in hv_shim
assert (R/'scripts/test-hypervisor-shim-compile.sh').is_file()
assert (R/'Tests/stubs/Hypervisor/Hypervisor.h').is_file()

# Firmware runtime compile-regression guards for the Encodable/.init contextual-type
# failure fixed in 0.2.5-alpha. The encoder must be concrete so shorthand reply
# initializers resolve to FirmwareRuntimeReply rather than the Encodable protocol.
firmware_host=(R/'PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift').read_text()
assert 'private func encode(_ reply: FirmwareRuntimeReply)' in firmware_host
assert 'func encode<T:Encodable>' not in firmware_host
assert 'encode(.init(' in firmware_host

# Main app Swift compile-regression guards fixed in 0.2.9-alpha.
workbench=(R/'App/WorkbenchModel.swift').read_text()
target_workspace=(R/'App/TargetWorkspace.swift').read_text()
assert '==.' not in workbench, 'Swift parser-hostile equality shorthand remains in WorkbenchModel'
assert 'JSONEncoder.vectorGlassPretty' not in target_workspace, 'undefined JSONEncoder helper remains'
assert 'encoder.outputFormatting = [.prettyPrinted, .sortedKeys]' in target_workspace
assert 'private func firstHexAddress(in text: String) -> UInt64?' in workbench
assert '#"(?i)\\b0x[0-9a-f]{1,16}\\b"#' in workbench
assert 'DispatchTime.now() + DispatchTimeInterval.milliseconds(250)' in workbench
assert (R/'scripts/test-app-typecheck.sh').is_file()
assert (R/'scripts/list-target-swift-sources.py').is_file()
app_typecheck=(R/'scripts/test-app-typecheck.sh').read_text()
coordinator_typecheck=(R/'scripts/test-coordinator-typecheck.sh').read_text()
assert 'list-target-swift-sources.py" HindSight' in app_typecheck
assert 'list-target-swift-sources.py" CoordinatorService' in coordinator_typecheck
expected_app_swift=[str((R/path).resolve()) for path in topology['targets']['HindSight'] if path.endswith('.swift')]
expected_service_swift=[str((R/path).resolve()) for path in topology['targets']['CoordinatorService'] if path.endswith('.swift')]
actual_app_swift=subprocess.run([sys.executable,str(R/'scripts/list-target-swift-sources.py'),'HindSight'],check=True,capture_output=True,text=True).stdout.splitlines()
actual_service_swift=subprocess.run([sys.executable,str(R/'scripts/list-target-swift-sources.py'),'CoordinatorService'],check=True,capture_output=True,text=True).stdout.splitlines()
assert actual_app_swift == expected_app_swift, 'app type-check coverage drifted from generated topology'
assert actual_service_swift == expected_service_swift, 'Coordinator type-check coverage drifted from generated topology'
assert str((R/'App/AnalysisNavigation.swift').resolve()) in actual_app_swift
assert str((R/'App/AnalysisContinuity.swift').resolve()) in actual_app_swift
assert 'test-app-typecheck.sh' in (R/'scripts/test-macos.sh').read_text()

# Universal macOS / Intel compatibility contract. Static ARM64 analysis remains
# cross-host; only host-dependent Apple-Silicon execution features are gated.
host_platform=(R/'Shared/HostPlatform.swift').read_text()
assert 'case arm64' in host_platform and 'case x86_64' in host_platform
assert 'supportsAppleSiliconHypervisor' in host_platform and 'supportsArm64NativeDebugger' in host_platform
assert 'supportsVectorEmu' in host_platform and 'isAppleSiliconProcess' in host_platform
assert 'Shared/HostPlatform.swift' in topology['targets']['HindSight']
assert 'Shared/HostPlatform.swift' in topology['targets']['CoordinatorService']
assert "'ARCHS':'arm64 x86_64'" in generator
assert "ARCHS='arm64 x86_64'" in (R/'scripts/build-app.sh').read_text()
assert "aarch64-apple-darwin" in build_rust and "x86_64-apple-darwin" in build_rust
assert '/usr/bin/lipo -create' in build_rust
vector_build=(R/'scripts/build-vector-emu.sh').read_text()
assert 'aarch64-apple-darwin' in vector_build and 'x86_64-apple-darwin' in vector_build
assert '/usr/bin/lipo -create' in vector_build
assert '-target "${arch}-apple-macos14.0"' in app_typecheck
assert '-target "${arch}-apple-macos14.0"' in coordinator_typecheck
assert 'for arch in arm64 x86_64' in app_typecheck and 'for arch in arm64 x86_64' in coordinator_typecheck
assert "generic/platform=macOS" in (R/'scripts/build-app.sh').read_text()
assert "ARCHS='arm64 x86_64'" in (R/'scripts/build-app.sh').read_text()
content_host=(R/'App/ContentView.swift').read_text()
assert 'hostUnavailableWorkspace("Native ARM64 Debugger"' in content_host
assert 'hostUnavailableWorkspace("Hypervisor RE"' in content_host
assert 'hostUnavailableWorkspace("SoC Virtualization"' in content_host
assert 'workspaceRequiresAppleSilicon' in content_host
debug_swift=(R/'PlatformServices/CoordinatorService/DebugServer.swift').read_text()
assert 'supportsArm64NativeDebugger' in debug_swift
assert 'hostUnavailableMessage' in debug_swift
assert 'supportsAppleSiliconHypervisor' in virt_host
debug_c=(R/'native/debug/debug_server.c').read_text()
assert '#if !defined(__APPLE__) || !defined(__arm64__)' in debug_c
assert 'Native ARM64 debugging requires the arm64 HindSight slice on Apple Silicon.' in debug_c
bridge_caps=(R/'rust/crates/bridge/src/lib.rs').read_text()
for capability in ['universal_macos_binary','intel_macos_host_support','host_architecture_feature_gating','cross_host_arm64_analysis','vector_emu_cross_host']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability


# v0.5.x Evidence Core contract.
evidence_core=(R/'App/AnalysisEvidenceCore.swift').read_text()
for token in ['struct VGEvidenceRecord', 'struct VGAnalysisEvidenceIndex', 'static let maxRecords = 50_000', 'addressIndex', 'symbolIndex', 'semanticIdentity', 'normalizedText']:
    assert token in evidence_core, token
for capability in ['typed_evidence_core','off_main_evidence_index','semantic_evidence_identity','indexed_evidence_navigation','evidence_core_compat_adapter']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
nav_core=(R/'App/AnalysisNavigation.swift').read_text()
assert 'currentEvidenceIndex?.search' in nav_core and 'currentEvidenceIndex?.focus' in nav_core and 'currentEvidenceIndex?.related' in nav_core
workbench_core=(R/'App/WorkbenchModel.swift').read_text()
assert 'Task.detached(priority: .utility)' in workbench_core and 'rebuildEvidenceIndex(recordID:' in workbench_core
assert 'App/AnalysisEvidenceCore.swift' in topology['targets']['HindSight']
assert 'maxResidentAnalysisReports = 8' in workbench_core and 'analysisReportLRU' in workbench_core
assert 'Task.sleep(nanoseconds: 350_000_000)' in workbench_core and 'writeLaunchSessionNow()' in workbench_core
for capability in ['worker_native_typed_evidence','typed_evidence_sideband_v1','analysis_cache_envelope_v3','analysis_stage_fingerprints','typed_evidence_compatibility_merge']:
    assert capability in bridge_caps, capability
worker_051=(R/'rust/crates/worker/src/preview.rs').read_text()
assert 'VG_EVIDENCE\\t' in worker_051 and 'VG_EVIDENCE_MANIFEST\\t' in worker_051
assert 'WorkerEvidenceBundle' in (R/'Shared/WorkbenchMessages.swift').read_text()
assert 'AnalysisCacheEnvelopeV2' in (R/'PlatformServices/CoordinatorService/ProjectStore.swift').read_text()
assert 'build(worker: bundle, compatibilitySources: sources)' in workbench_core
for capability in ['bounded_resident_analysis_reports','debounced_launch_session_persistence']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability

# v0.3 workbench UX contract. Advanced tools must remain discoverable without
# returning to the old giant workspace picker.
content_ui=(R/'App/ContentView.swift').read_text()
chrome_ui=(R/'App/WorkbenchChrome.swift').read_text()
catalog_ui=(R/'App/WorkspaceCatalog.swift').read_text()
assert 'private let workspaces = [' not in content_ui, 'legacy monolithic workspace picker returned'
assert 'VGCommandPalette' in content_ui and 'keyboardShortcut("k", modifiers: .command)' in content_ui
assert 'workbenchSidebar()' in content_ui and 'contextualInspector()' in content_ui and 'statusBar()' in content_ui
assert 'VGPanelGroupBoxStyle' in chrome_ui and 'VGWorkspaceRow' in chrome_ui
assert 'enum VGWorkspaceGroup' in catalog_ui and 'static func search(_ query: String)' in catalog_ui
assert (R/'App/WorkbenchChrome.swift').is_file() and (R/'App/WorkspaceCatalog.swift').is_file() and (R/'App/AnalystSurface.swift').is_file()
assert (R/'docs/UX_SYSTEM.md').is_file()
assert 'Command palette' in (R/'docs/UX_SYSTEM.md').read_text() and 'Inspector' in (R/'docs/UX_SYSTEM.md').read_text()
# v0.3.1 Analyst Desk contract: code is structured, workspaces are tabbed, and deep analysis can enter focus mode.
analyst_ui=(R/'App/AnalystSurface.swift').read_text()
assert 'struct VGWorkspaceTabStrip' in analyst_ui
assert 'struct VGRichDisassemblyView' in analyst_ui
disassembly_model=(R/'App/DisassemblyModel.swift').read_text()
assert 'enum VGDisassemblyParser' in disassembly_model and 'struct VGDisassemblyRow' in disassembly_model
assert 'enum VGInspectorMode' in analyst_ui and 'struct VGMetricTile' in analyst_ui
assert 'VGWorkspaceTabStrip(' in content_ui and 'toggleFocusMode' in content_ui
assert 'VGRichDisassemblyView(' in content_ui and 'inspectorMode' in content_ui
assert 'workspace_tabs' in (R/'rust/crates/bridge/src/lib.rs').read_text()

# v0.4.0-beta.8 workbench polish contract: first-run service state and user actions
# must reflect actual prerequisites instead of relying on silent model guards.
app_shell=(R/'App/HindSightApp.swift').read_text()
assert '@Published var connectingToCoordinator = false' in workbench
assert 'func connect(force: Bool = false)' in workbench and 'func reconnect()' in workbench
assert 'self.connected = true' in workbench and 'Reconnect to restore the open project session.' in workbench
assert 'Create or open a project before importing a binary.' in workbench
assert 'self.connect()' in workbench
assert 'Reconnect Coordinator' in content_ui and 'Add your first artifact' in content_ui
assert 'model.canImportBinary' in content_ui
assert 'records.count > 12' in content_ui and 'more artifact' in content_ui
assert 'No tools match' in content_ui
assert 'No matching tool' in chrome_ui
assert '.disabled(!model.canImportBinary)' in app_shell

# v0.4.0-beta.9 Coordinator recovery contract: transport reachability, capability
# compatibility, and project-session readiness are separate states. A restarted
# XPC service must not expose project-bound actions until the prior project has
# been re-opened successfully.
coordinator_client=(R/'App/CoordinatorClient.swift').read_text()
for token in ['@Published var restoringProjectSession = false', '@Published var projectSessionReady = false',
              'var projectServiceReady: Bool', 'var canImportBinary: Bool',
              'private struct CoordinatorHandshake: Decodable', 'validateCoordinatorHandshake',
              'handshake.version == VGProduct.version', 'handshake.abi == 1',
              'func retryProjectRestore()', 'private func restoreProjectSession(path: String)',
              'Coordinator recovered and project session restored.', 'func diagnosticsText()', 'func copyDiagnostics()']:
    assert token in workbench, token
assert 'func resetConnection(reason:' in coordinator_client
assert 'Reconnect to restore the session.' in coordinator_client
assert 'model.coordinatorStateLabel' in content_ui
assert 'Restore Project' in content_ui
assert '!model.projectServiceReady' in content_ui
assert 'CommandMenu("Support")' in app_shell and 'Copy Diagnostics' in app_shell
for capability in ['coordinator_handshake_validation','coordinator_session_rehydration','beta_diagnostics']:
    assert f'\\"{capability}\\":true' in (R/'rust/crates/bridge/src/lib.rs').read_text(), capability

# v0.4.0-beta.10 launch/session continuity contract. A full app relaunch may
# recover only bounded navigation metadata; the project itself remains authoritative
# in CoordinatorService and stale saved paths must degrade to the normal open flow.
for token in ['private struct LaunchSession: Codable', 'maxLaunchSessionBytes = 32 * 1024',
              'private static func readSavedLaunchSession()', 'persistLaunchSessionIfPossible()',
              'private func restoreSavedLaunchSession(_ session: LaunchSession)',
              'func reopenSavedLaunchSession()', 'func forgetSavedLaunchSession()',
              'savedLaunchSessionAvailable', 'savedLaunchProjectName',
              'session.projectPath.utf8.count <= 4096', 'standardizedFileURL.path == session.projectPath', 'VGWorkspaceCatalog.allNames.contains(session.workspace)',
              'VGWorkspaceCatalog.layouts.contains(session.layoutPreset)',
              'The saved project no longer exists; choose another project.',
              'Reopening last project…', 'from the previous session.']:
    assert token in workbench, token
assert 'else if let session = self.savedLaunchSession' in workbench
assert 'persistLaunchSessionIfPossible()' in workbench.split('private func apply(_ result:',1)[1]
assert 'saved_launch_session=' in workbench and 'saved_launch_project=' in workbench
assert 'Reopen Last Project' in content_ui and 'Previous session:' in content_ui
assert 'Forget Saved Project' in app_shell and 'model.reopenSavedLaunchSession' in app_shell
for capability in ['launch_session_continuity','automatic_last_project_reopen','stale_launch_session_recovery']:
    assert f'\\"{capability}\\":true' in (R/'rust/crates/bridge/src/lib.rs').read_text(), capability
assert (R/'docs/SESSION_CONTINUITY_BETA10.md').is_file()
assert 'does **not** contain unsaved analyst-note text' in (R/'docs/SESSION_CONTINUITY_BETA10.md').read_text()

# v0.4.0-beta.11 responsive-workbench contract. Dense workspaces must change
# topology when width shrinks instead of preserving mutually incompatible
# minimum-width columns until controls disappear.
adaptive_workspaces = [
    'App/BinaryWorkspace.swift', 'App/GraphWorkspace.swift',
    'App/TraceExplorerWorkspace.swift', 'App/RuntimeCorrelationWorkspace.swift',
    'App/EvidenceGraphWorkspace.swift', 'App/InvestigationLedgerWorkspace.swift'
]
assert 'struct VGAdaptiveThreePane' in chrome_ui
assert 'wideBreakpoint' in chrome_ui and 'compactBreakpoint' in chrome_ui
assert 'VSplitView' in chrome_ui and 'auxiliaryDock' in chrome_ui
assert 'struct VGHorizontalOverflowBar' in chrome_ui
assert 'struct VGAdaptiveTwoPane' in chrome_ui
for path in adaptive_workspaces:
    assert 'VGAdaptiveThreePane(' in (R/path).read_text(), path
assert 'selectedWorkspaceAndInspector' in content_ui and 'proxy.size.width >= 1040' in content_ui
assert 'VGAdaptiveTwoPane(' in (R/'App/BinaryDiffWorkspace.swift').read_text()
assert '.frame(minWidth: 900, minHeight: 680)' in content_ui
assert '.frame(minWidth: 1180, minHeight: 760)' not in content_ui
for capability in ['adaptive_workspace_topology','responsive_workbench_chrome','overflow_safe_metric_bands']:
    assert f'\\"{capability}\\":true' in (R/'rust/crates/bridge/src/lib.rs').read_text(), capability
assert (R/'docs/UX_REARCHITECTURE_BETA11.md').is_file()

# v0.4.0-beta.12 VectorEmu Stepper compile-hotfix guard.
assert 'step:UInt64(1000)' not in content_ui
assert 'step: UInt64(1000)' not in content_ui
assert 'step: 1000' in content_ui and 'UInt64(1)...UInt64(1_000_000_000)' in content_ui

topology_ui=json.loads((R/'Config/project-topology.json').read_text()) if (R/'Config/project-topology.json').is_file() else {'targets':{'HindSight':[]}}
# project topology is regenerated later in validation; when present it must include the new shell sources.
if topology_ui['targets']['HindSight']:
    assert 'App/WorkbenchChrome.swift' in topology_ui['targets']['HindSight']
    assert 'App/WorkspaceCatalog.swift' in topology_ui['targets']['HindSight']
    assert 'App/AnalystSurface.swift' in topology_ui['targets']['HindSight']
    assert 'App/DisassemblyModel.swift' in topology_ui['targets']['HindSight']


# v0.3.2 Graph Workspace contract: CFG is a real canvas with typed graph data,
# deterministic layout, minimap, synchronized semantics, and debugger PC highlighting.
graph_model=(R/'App/GraphModel.swift').read_text()
graph_ui=(R/'App/GraphWorkspace.swift').read_text()
for token in ['struct VGCFGGraph','struct VGCFGBlock','struct VGCFGEdge','enum VGGraphParser','func layout(limit: Int = 240)']:
    assert token in graph_model, token
for token in ['struct VGGraphWorkspace','Canvas { context','MagnificationGesture','minimap','functionNavigator','semanticPane','debugPC','VGAdaptiveThreePane(']:
    assert token in graph_ui, token
assert 'VGGraphWorkspace(' in content_ui
assert 'model.workspace == "CFG"' in content_ui
for source in ['App/GraphModel.swift','App/GraphWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
bridge_caps=(R/'rust/crates/bridge/src/lib.rs').read_text()
analysis_source=(R/'rust/crates/worker/src/preview.rs').read_text()

# v0.4.0-beta.12 analysis-synthesis contract.
for token in ['fn function_intelligence(', 'fn pc_relative_data_references(', 'fn analysis_insights_report(',
              '(w&0x9f000000)==0x10000000', '(w&0x9f000000)==0x90000000',
              'w>>31!=0&&(w&0x7f000000)==0x11000000', 'provenance=paired-immediates',
              'i==born.saturating_add(1)', 'Interpretation guardrail']:
    assert token in analysis_source, token
for capability in ['function_intelligence','pc_relative_data_references','analysis_insights']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
for token in ['case "Analysis Insights"', 'case "Function Intelligence"', 'case "Data References"']:
    assert token in content_ui, token
assert (R/'docs/ANALYSIS_SYNTHESIS_BETA12.md').is_file()

# v0.4.0-beta.13 analysis-scalability contract.
for token in ['MAX_DOMINATOR_BLOCKS_PER_FUNCTION:usize=512', 'MAX_TOTAL_DOMINATOR_BLOCKS:usize=8_192', 'MAX_DOMINATOR_PASSES:usize=64', 'groups.entry(function_for(starts,leaders[b]))', 'dominator detail omitted', 'Dominator solver: bounded mode']:
    assert token in analysis_source, token
assert 'vec![all.clone();n]' not in analysis_source
fi_body=analysis_source[analysis_source.index('fn function_intelligence('):analysis_source.index('fn sign_extend_u64')]
assert 'collect::<std::collections::BTreeMap<_,_>>()' in fi_body
assert 'symbols.iter().find' not in fi_body
supervisor_source=(R/'PlatformServices/CoordinatorService/WorkerSupervisor.swift').read_text()
for token in ['process.terminationReason == .uncaughtSignal', 'The app remains isolated from the worker failure']:
    assert token in supervisor_source, token
for capability in ['bounded_dominator_solver','per_function_dominator_budget','analysis_worker_signal_diagnostics']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'docs/ANALYSIS_SCALABILITY_BETA13.md').is_file()

# v0.4.0-beta.14 evidence-quality / call-neighborhood contract.
for token in ['fn admitted_function_for(', 'fn cross_function_transfer_candidates(', 'fn function_neighborhood_report(', 'fn analysis_quality_report(', 'classification=tail-call-candidate', 'External BL targets remain visible']:
    assert token in analysis_source, token
fi_body=analysis_source[analysis_source.index('fn function_intelligence('):analysis_source.index('fn sign_extend_u64')]
assert 'admitted_function_for(starts,target,scan_end)' in fi_body
for capability in ['analysis_quality_ledger','function_neighborhoods','cross_function_tail_candidates']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
for token in ['case "Analysis Quality"', 'case "Function Neighborhoods"', 'case "Tail-Call Candidates"']:
    assert token in content_ui, token
assert (R/'docs/ANALYSIS_QUALITY_BETA14.md').is_file()

# v0.4.2 staged-analysis execution and patch-compatible analysis-cache contract.
target_model=(R/'Shared/TargetModel.swift').read_text()
assert 'static let version = "0.5.10"' in target_model
assert 'static let releaseChannel = "stable"' in target_model
assert 'static let isPrerelease = false' in target_model
assert r'\"release_channel\":\"stable\"' in bridge_caps
project_store_cache=(R/'PlatformServices/CoordinatorService/ProjectStore.swift').read_text()
coordinator_main=(R/'PlatformServices/CoordinatorService/main.swift').read_text()
for token in ['LegacyAnalysisCacheEnvelope', 'AnalysisCacheEnvelope', 'analysisCacheSchema = 3', 'analysis-cache', 'cachedAnalysis(', 'storeAnalysisCache(',
              'sourceSHA256', 'engineVersion', 'producerVersion', 'compatibleLegacyProductVersions',
              'Data(contentsOf: url, options: [.mappedIfSafe])', 'Set(rawReport.keys) == AnalysisReport.jsonFields',
              'data.write(to: url, options: [.atomic])', 'isSymbolicLinkKey']:
    assert token in project_store_cache, token
for token in ['analysisCacheEngineVersion = "arm64-analysis-v15-schema71"', 'analysisCacheProducerVersion = "0.5.10"',
              'compatibleLegacyAnalysisCacheVersions: Set<String> = ["0.4.1"]', '.checkingCache',
              'Compatible 0.4.1 analysis cache reused and migrated', 'storeAnalysisCache(report']:
    assert token in coordinator_main, token
messages_042=(R/'Shared/WorkbenchMessages.swift').read_text()
assert 'func forRequest(_ id: String) -> AnalysisReport?' in messages_042
for token in ['case checkingCache', 'progress_percent: Int', 'progress_stage: String']:
    assert token in messages_042, token
worker_supervisor=(R/'PlatformServices/CoordinatorService/WorkerSupervisor.swift').read_text()
for token in ['VG_PROGRESS\\t', 'progressPercent', 'progressStage', 'recordStatus(id:', 'analysis cache']:
    if token == 'analysis cache':
        continue
    assert token in worker_supervisor, token
analysis_worker=(R/'rust/crates/worker/src/preview.rs').read_text()
for token in ['analysis_progress(2,"verify"', 'analysis_progress(20,"metadata"', 'analysis_progress(44,"decode"',
              'analysis_progress(54,"cfg"', 'analysis_progress(86,"synthesis"', 'analysis_progress(92,"evidence"',
              'analysis_progress(97,"finalize"']:
    assert token in analysis_worker, token
assert 'ProgressView(value: progress, total: 100)' in content_ui
assert 'analysisProgressValue' in workbench
for capability in ['project_analysis_cache','analysis_cache_identity_binding','stable_release_channel',
                   'analysis_stage_progress','analysis_progress_percent','analysis_progress_diagnostic_separation',
                   'analysis_cache_patch_compatibility','analysis_cache_envelope_v2']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'docs/RELEASE_READINESS.md').is_file()
assert (R/'docs/ANALYSIS_EXECUTION_0.4.2.md').is_file()
assert (R/'scripts/test-release-macos.sh').is_file()

# v0.4.2 UI-density hardening contract. Responsive parent panes are not enough
# if child controls still reserve desktop-only fixed widths.
assert 'struct VGFlowLayout: Layout' in chrome_ui
assert 'struct VGKeyValueField: View' in chrome_ui
assert 'struct VGTagPill: View' in chrome_ui
assert 'init(text: String, systemImage: String? = nil)' in chrome_ui
targets_start=content_ui.index('@ViewBuilder private func targetsWorkspace()')
targets_end=content_ui.index('private func targetIcon', targets_start)
targets_ui=content_ui[targets_start:targets_end]
for token in ['VGAdaptiveTwoPane(breakpoint: 980)', 'VGFlowLayout', 'LazyVGrid(columns: [GridItem(.adaptive(minimum: 220)', 'ViewThatFits(in: .horizontal)', 'VGKeyValueField(label: \"Evidence\"']:
    assert token in targets_ui, token
for token in ['.frame(width: 170', '.frame(width: 150', '.frame(width: 140']:
    assert token not in targets_ui, token
for path in ['App/BinaryWorkspace.swift','App/GraphWorkspace.swift','App/TraceExplorerWorkspace.swift','App/RuntimeCorrelationWorkspace.swift','App/EvidenceGraphWorkspace.swift','App/InvestigationLedgerWorkspace.swift']:
    assert 'VGFlowLayout' in (R/path).read_text(), path
assert 'VGHorizontalOverflowBar' in (R/'App/BinaryDiffWorkspace.swift').read_text()
assert (R/'python/tests/test_ui_density.py').is_file()
assert (R/'docs/UI_POLISH_0.4.2.md').is_file()

for capability in ['graph_workspace','cfg_canvas','graph_minimap','synchronized_semantics','debugger_graph_highlight','dockable_analysis_panes']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability


# v0.4.3 structured analysis navigation / triage contract.
analysis_navigation=(R/'App/AnalysisNavigation.swift').read_text()
for token in ['struct VGAnalysisEvidenceHit', 'enum VGAnalysisNavigation', 'struct VGAnalysisTriageView',
              'struct VGAnalysisSearchResults', 'struct VGRelatedAnalysisEvidenceView',
              'func analysisFocusHits(', 'func analysisSearchHits(', 'func relatedAnalysisHits(', 'func openAnalysisHit(']:
    assert token in analysis_navigation, token
for token in ['case "Analysis Triage"', 'VGAnalysisSearchResults(model: model)', 'VGRelatedAnalysisEvidenceView(model: model)']:
    assert token in content_ui, token
workspace_catalog=(R/'App/WorkspaceCatalog.swift').read_text()
assert '"Analysis Triage"' in workspace_catalog
assert 'App/AnalysisNavigation.swift' in topology['targets']['HindSight']
for capability in ['structured_analysis_navigation','analysis_triage_workspace','correlated_evidence_navigation']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_analysis_navigation.py').is_file()
assert (R/'docs/ANALYSIS_NAVIGATION_0.4.3.md').is_file()

# v0.4.4 investigation continuity contract.
analysis_continuity=(R/'App/AnalysisContinuity.swift').read_text()
for token in ['struct VGAnalysisPin', 'struct VGAnalysisVisit', 'struct VGAnalysisSnapshot', 'struct VGAnalysisDelta',
              'maximumBytes = 1 * 1024 * 1024', 'maximumPins = 128', 'maximumRecent = 96',
              'analysis-continuity-v1.json', 'options: .atomic', 'isSymbolicLinkKey', 'isRegularFileKey', 'decoder.dateDecodingStrategy = .iso8601', 'arm64-analysis-v15-schema71']:
    assert token in analysis_continuity, token
for token in ['Text("Pinned").tag("Pinned")', 'Text("Recent").tag("Recent")', 'Text("Changes").tag("Changes")',
              'VGAnalysisDeltaView(model: model)', 'toggleAnalysisPin(', 'recordAnalysisVisit(']:
    assert token in analysis_navigation, token
workbench_model=(R/'App/WorkbenchModel.swift').read_text()
for token in ['schema: 2', '[1, 2].contains(session.schema)', 'addressText: String(addressText.prefix(128))',
              'analysisSearch: String(analysisSearch.prefix(1024))', 'recordAnalysisSnapshot(', 'loadAnalysisContinuity(projectPath: snapshot.path)']:
    assert token in workbench_model, token
assert 'App/AnalysisContinuity.swift' in topology['targets']['HindSight']
for capability in ['persistent_analysis_pins','recent_evidence_history','analysis_change_ledger','launch_context_continuity']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_analysis_continuity.py').is_file()
assert (R/'docs/ANALYSIS_CONTINUITY_0.4.4.md').is_file()


# v0.4.5 investigation collections and resume contract. Named collections are
# project-local continuity metadata over existing pins; they do not alter evidence
# semantics or the 66-field worker report.
for token in ['struct VGInvestigationCollection', 'maximumCollections = 32', 'maximumPinsPerCollection = 64',
              'func createAnalysisCollection(', 'func toggleAnalysisCollectionMembership(', 'func analysisCollectionHits(']:
    assert token in analysis_continuity, token
for token in ['Text("Collections").tag("Collections")', 'Menu("Collect")', 'Resume Latest', 'resumeMostRecentAnalysisVisit(', 'resumeAnalysisVisit(']:
    assert token in analysis_navigation, token
for capability in ['investigation_collections','collection_aware_pins','evidence_resume']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_investigation_collections.py').is_file()
assert (R/'docs/INVESTIGATION_COLLECTIONS_0.4.5.md').is_file()

# v0.4.6 investigation workflow contract. Collections are searchable/renameable,
# expose bounded current-report health, and recent evidence supports back/forward traversal.
workflow=(R/'App/AnalysisContinuity.swift').read_text() + (R/'App/AnalysisNavigation.swift').read_text() + (R/'App/HindSightApp.swift').read_text()
for token in ['renameAnalysisCollection','filteredAnalysisCollections','analysisCollectionHealth','currentAnalysisPinIDs',
              'navigateAnalysisHistoryBackward','navigateAnalysisHistoryForward','Previous Evidence','Next Evidence']:
    assert token in workflow, token
for capability in ['collection_search','collection_health','collection_rename','evidence_history_navigation']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_investigation_workflow.py').is_file()
assert (R/'docs/INVESTIGATION_WORKFLOW_0.4.6.md').is_file()

# v0.4.7 collection review contract. Review baselines and pairwise comparison
# are bounded continuity/provenance operations and do not alter worker evidence semantics.
review_contract=(R/'App/AnalysisContinuity.swift').read_text() + (R/'App/AnalysisNavigation.swift').read_text()
for token in ['struct VGInvestigationReviewBaseline',
              'compareAnalysisCollections', 'markAnalysisCollectionReviewed', 'analysisCollectionReviewDrift',
              'Text("Review").tag("Review")', 'VGCollectionReviewView', 'exact-identity/provenance tools']:
    assert token in review_contract, token
for capability in ['collection_comparison','collection_review_baselines','collection_evidence_drift','investigation_review_mode']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_investigation_review.py').is_file()
assert (R/'docs/COLLECTION_REVIEW_0.4.7.md').is_file()


# v0.4.8 review-journal and handoff contract. Journal state is bounded, project-local,
# analyst-authored context; handoff generation preserves exact evidence identity without semantic scoring.
journal_contract=(R/'App/AnalysisContinuity.swift').read_text() + (R/'App/AnalysisNavigation.swift').read_text()
for token in ['struct VGInvestigationReviewEntry', 'enum VGInvestigationReviewState', 'var schema: Int = 5',
              '[1, 2, 3, 4, 5].contains(document.schema)', 'appendAnalysisReviewEntry',
              'analysisCollectionHandoff', 'Copy Handoff', 'Review Journal & Handoff']:
    assert token in journal_contract, token
for capability in ['collection_review_journal','collection_workflow_state','investigation_handoff','review_timeline']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_review_journal.py').is_file()
assert (R/'docs/REVIEW_JOURNAL_0.4.8.md').is_file()


# v0.4.9 explicit review follow-up contract. Follow-ups are bounded analyst-authored
# workflow tasks tied to exact evidence identities; they are not evidence or severity scores.
followup_contract=(R/'App/AnalysisContinuity.swift').read_text() + (R/'App/AnalysisNavigation.swift').read_text()
for token in ['struct VGInvestigationFollowUp', 'enum VGInvestigationFollowUpState', 'var schema: Int = 5',
              '[1, 2, 3, 4, 5].contains(document.schema)', 'createAnalysisFollowUp',
              'toggleAnalysisFollowUpCompleted', 'Follow-Up Queue', 'Follow-ups are analyst-authored workflow tasks']:
    assert token in followup_contract, token
for capability in ['review_follow_up_queue','follow_up_evidence_binding','collection_review_summary','handoff_follow_up_context']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'python/tests/test_review_followups.py').is_file()
assert (R/'docs/REVIEW_ACTIONS_0.4.9.md').is_file()

# v0.3.3 Binary Workspace contract: the high-frequency code-reading loop is a
# synchronized function/code/lift/reference/data desk backed by typed portable models.
binary_model=(R/'App/BinaryWorkspaceModel.swift').read_text()
binary_ui=(R/'App/BinaryWorkspace.swift').read_text()
for token in ['struct VGBinaryReference','struct VGLiftedLine','struct VGInstructionData','enum VGBinaryWorkspaceParser','static func references','static func lifted','static func instructionData']:
    assert token in binary_model, token
for token in ['struct VGBinaryWorkspace','functionNavigator','centerDesk','semanticPane','referencePane','dataPane','Follow PC']:
    assert token in binary_ui, token
assert 'case "Binary Workspace"' in content_ui
assert 'workspace = "Binary Workspace"' in (R/'App/WorkbenchModel.swift').read_text()
for source in ['App/BinaryWorkspaceModel.swift','App/BinaryWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['binary_workspace','synchronized_code_workspace','conservative_lifted_view','function_browser','xref_inspector','instruction_data_inspector']:
    assert f'\\\"{capability}\\\":true' in bridge_caps, capability

# v0.3.4 Runtime Correlation contract: debugger observations remain distinct from
# static evidence and are translated only through a unique loaded-image match.
runtime_model=(R/'App/RuntimeCorrelationModel.swift').read_text()
runtime_ui=(R/'App/RuntimeCorrelationWorkspace.swift').read_text()
for token in ['struct VGRuntimeSample','struct VGRuntimeMemoryObservation','struct VGRuntimeCorrelation','enum VGRuntimeCorrelationEngine','static func moduleForBinary','static func correlate','static func heat']:
    assert token in runtime_model, token
for token in ['struct VGRuntimeCorrelationWorkspace','STOP HISTORY','STATIC ↔ RUNTIME CODE','REGISTER DELTAS','MEMORY OBSERVATIONS','MODULE EVIDENCE']:
    assert token in runtime_ui, token
assert 'case "Runtime Correlation"' in content_ui
assert 'runtimeSamples: [VGRuntimeSample]' in workbench
assert 'runtimeMemoryObservations: [VGRuntimeMemoryObservation]' in workbench
assert 'recordRuntimeSample' in workbench and 'recordRuntimeMemory' in workbench
for source in ['App/RuntimeCorrelationModel.swift','App/RuntimeCorrelationWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['runtime_correlation_workspace','runtime_stop_history','register_delta_tracking','runtime_static_slide_correlation','runtime_memory_observations','runtime_execution_heat']:
    assert f'\\\"{capability}\\\":true' in bridge_caps, capability


# v0.3.5 Trace Explorer contract: ordered debugger observations can be reasoned
# across without being mislabeled as a complete instruction trace.
trace_model=(R/'App/TraceExplorerModel.swift').read_text()
trace_ui=(R/'App/TraceExplorerWorkspace.swift').read_text()
for token in ['struct VGTraceEvent','struct VGTraceSessionSummary','enum VGTraceExplorerEngine','static func events','static func functionSummary','static func registerLanes','static func nearbyMemory']:
    assert token in trace_model, token
for token in ['struct VGTraceExplorerWorkspace','OBSERVED TIMELINE','EVENT EVIDENCE','STATIC CONTEXT','OBSERVED FUNCTION PATH','REGISTER CHANGE BAND','NEARBY MEMORY OBSERVATIONS','Debugger observations ≠ full instruction trace']:
    assert token in trace_ui, token
assert 'case "Trace Explorer"' in content_ui
assert 'debugOpenPCInTraceExplorer' in workbench
for source in ['App/TraceExplorerModel.swift','App/TraceExplorerWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['trace_explorer','observed_transition_timeline','register_change_band','trace_function_summary','nearby_memory_evidence']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability

# v0.3.6 Evidence Graph contract: machine observations, static derivations,
# evidence-backed correlations, and analyst assertions must remain distinguishable.
evidence_model=(R/'App/EvidenceGraphModel.swift').read_text()
evidence_ui=(R/'App/EvidenceGraphWorkspace.swift').read_text()
for token in ['struct VGEvidenceNode','struct VGEvidenceEdge','struct VGEvidenceGraph','enum VGEvidenceGraphBuilder','func filtered(query: String','func layout(limit: Int = 180)']:
    assert token in evidence_model, token
for token in ['struct VGEvidenceGraphWorkspace','PROVENANCE','CONFIDENCE LANGUAGE','EVIDENCE INSPECTOR','Observed · Derived · Correlated · Asserted remain distinct','MagnificationGesture','miniMap']:
    assert token in evidence_ui, token
assert 'case "Evidence Graph"' in content_ui
assert 'case "Evidence Graph": workspace = "Evidence Graph"' in workbench
for source in ['App/EvidenceGraphModel.swift','App/EvidenceGraphWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['evidence_graph_workspace','provenance_graph','evidence_confidence_edges','analyst_note_nodes','runtime_evidence_links']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability


# v0.3.7 Investigation Ledger contract: project-local case state is bounded,
# provenance-aware, replayable at the workbench-context level, and exportable.
ledger_model=(R/'App/InvestigationLedgerModel.swift').read_text()
ledger_ui=(R/'App/InvestigationLedgerWorkspace.swift').read_text()
for token in ['struct VGInvestigationLedger','struct VGLedgerEntry','struct VGHypothesis','struct VGCaseBookmark','struct VGInvestigationSnapshot','struct VGCaseBundle','enum VGLedgerStore','static func pdf(_ bundle: VGCaseBundle) -> Data']:
    assert token in ledger_model, token
for token in ['struct VGInvestigationLedgerWorkspace','Timeline','Hypotheses','Bookmarks','Snapshots','HindSight Case (.vgcase)','PDF Report','LEDGER INSPECTOR']:
    assert token in ledger_ui, token
assert 'case "Investigation Ledger"' in content_ui
assert 'case "Investigation Ledger": workspace = "Investigation Ledger"' in workbench
for source in ['App/InvestigationLedgerModel.swift','App/InvestigationLedgerWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['investigation_ledger','persistent_case_state','hypothesis_tracking','case_bookmarks','session_snapshots','vgcase_export','case_markdown_export','case_json_export','case_pdf_export']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'docs/INVESTIGATION_LEDGER.md').is_file()

# v0.4.0-beta.7 Binary Diff contract: two bounded analysis reports are compared with
# conservative function identity and dimension-specific change evidence.
diff_model=(R/'App/BinaryDiffModel.swift').read_text()
diff_ui=(R/'App/BinaryDiffWorkspace.swift').read_text()
for token in ['enum VGBinaryDiffStatus','struct VGFunctionDiff','struct VGBinaryDiffReport','enum VGBinaryDiffEngine','unique symbol','exact normalized code fingerprint','Stable FNV-1a']:
    assert token in diff_model, token
for token in ['struct VGBinaryDiffWorkspace','Binary Diff','Analyze Pair','Evidence comparison','Matching: unique symbols','Function absent on this side.']:
    assert token in diff_ui, token
assert 'case "Binary Diff"' in content_ui
assert 'case "Binary Diff": workspace = "Binary Diff"' in workbench
assert 'prepareBinaryDiff' in workbench and 'analysisReportsByRecordID' in workbench
for source in ['App/BinaryDiffModel.swift','App/BinaryDiffWorkspace.swift']:
    if topology_ui['targets']['HindSight']:
        assert source in topology_ui['targets']['HindSight'], source
for capability in ['binary_diff_workspace','semantic_binary_diff','cfg_diff','xref_diff','layout_resilient_function_matching']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
for capability in ['analysis_job_status','analysis_progress_phases','bounded_analysis_deadline','nonfatal_status_polling']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'docs/BINARY_DIFF.md').is_file()


# v0.4.0-beta.7 Mach-O admission regression contract. Modern Apple binaries
# normally carry LC_FUNCTION_STARTS and may carry LC_DATA_IN_CODE; neither may
# veto an otherwise valid mapping. Truly encrypted code degrades to metadata-only.
inspect_source=(R/'native/macho/inspect.c').read_text()
for token in ['LC_FUNCTION_STARTS = 0x26','LC_DATA_IN_CODE = 0x29','LC_ENCRYPTION_INFO_64 = 0x2c']:
    assert token in inspect_source, token
assert 'cmd==0x26 && u32(p+12,0)!=0' not in inspect_source
assert 'LC_FUNCTION_STARTS and LC_DATA_IN_CODE are ordinary analysis metadata' in inspect_source
assert 'return 2;' in inspect_source and 'declared encrypted' in inspect_source
analysis_admission_worker=(R/'rust/crates/worker/src/preview.rs').read_text()
assert 'data_in_code_ranges' in analysis_admission_worker
assert '.data-in-code kind=' in analysis_admission_worker
assert 'Code bytes: declared encrypted' in analysis_admission_worker
analysis_admission_supervisor=(R/'PlatformServices/CoordinatorService/WorkerSupervisor.swift').read_text()
assert '["preview", "metadata-only", "unavailable"]' in analysis_admission_supervisor
assert 'case .staticArtifact: return "doc"' in content_ui
assert 'doc.binary' not in content_ui
for capability in ['modern_macho_metadata_admission','encrypted_metadata_analysis','data_in_code_barriers']:
    assert f'\\"{capability}\\":true' in bridge_caps, capability
assert (R/'docs/ANALYSIS_ADMISSION_BETA7.md').is_file()

# Release metadata must agree across every shipped component.
release = "0.5.10"
bundle_release = "0.5.10"
bundle_build = "5910"
app_info = plistlib.loads((R/'Config/App-Info.plist').read_bytes())
coordinator_info = plistlib.loads((R/'Config/Coordinator-Info.plist').read_bytes())
assert app_info['CFBundleShortVersionString'] == bundle_release
assert coordinator_info['CFBundleShortVersionString'] == bundle_release
assert app_info['CFBundleVersion'] == bundle_build
assert coordinator_info['CFBundleVersion'] == bundle_build
assert release in (R/'rust/crates/bridge/src/lib.rs').read_text()
assert 'Text(VGProduct.version)' in (R/'App/ContentView.swift').read_text()
assert 'analyzeRecord' in (R/'Shared/CoordinatorProtocol.swift').read_text()
assert 'debugAttach' in (R/'Shared/CoordinatorProtocol.swift').read_text()
assert (R/'PlatformServices/CoordinatorService/DebugServer.swift').is_file()
assert (R/'native/debug/debug_server.c').is_file()
assert '--analyze' in (R/'rust/crates/worker/src/main.rs').read_text()
assert 'analysis' in (R/'rust/crates/bridge/src/lib.rs').read_text() and 'true' in (R/'rust/crates/bridge/src/lib.rs').read_text()
for capability in ['ssa_rename','runtime_graph','swift_nominal_types','selector_xrefs','dyld_chained_page_starts','swift_field_records','predecessor_value_flow','evidence_layout','dyld_chained_pointer_decode','swift_field_ownership','iterative_value_propagation','indirect_call_resolution','resolution_layout','navigation_history','arm64e_pointer_format_recognition','arm64e_pointer_field_decode','dyld_multistart_expansion','context_navigation_history','context_inspector','indirect_symbol_table','lazy_nonlazy_pointer_resolution','export_trie','symbol_provenance','linkage_explorer','joined_linkage_records','symbol_caller_provenance','got_auth_got_reconstruction','import_export_collision_analysis','selected_symbol_workspace','native_debug_server','debug_attach_launch','debug_registers','debug_memory','software_breakpoints','debug_event_stream','debug_workspace','debug_stop_context','debug_module_slide_resolution','debug_pc_follow','debug_memory_browser','python_scripting','plugin_api_v1','plugin_discovery','plugin_capabilities','plugin_api_v2','plugin_dock_views','plugin_persistent_settings','plugin_persistent_state','plugin_analysis_passes','plugin_debug_filters','plugin_hotkeys','rewplugin_bundles','extensions_platform','hypervisor_vm','apple_soc_virtualization','soc_device_graph','hypervisor_re_suite','guest_memory_inspector','guest_disassembly','vm_snapshot_diff','guest_search','vm_trace','guest_breakpoints','gpa_device_map','unified_target_model','target_capability_discovery','address_pivot','session_headers','function_intelligence','pc_relative_data_references','analysis_insights']:
    assert f'\\\"{capability}\\\":true' in (R/'rust/crates/bridge/src/lib.rs').read_text(), capability
assert release in (R/'rust/crates/worker/src/main.rs').read_text()
analysis_source=(R/'rust/crates/worker/src/preview.rs').read_text()
# Rust worker compile-regression guards for the eight diagnostics fixed in 0.2.1-alpha.
assert 'fn string_at_va(' in analysis_source, 'missing string_at_va helper'
assert 'fn resolve(site:u64,rel:i64)->Option<u64>' in analysis_source, 'missing checked relative-address helper'
assert 'outs.insert(d);' in analysis_source, 'BTreeSet insertion must evaluate to unit'
assert 'outs.insert(d)}' not in analysis_source, 'BTreeSet insert bool leaked into a unit-valued branch'
messages=(R/'Shared/WorkbenchMessages.swift').read_text()
# beta.5 analysis transport contract: the modern report is a dedicated bounded bulk frame,
# not a legacy 256 KiB control message, and Swift/Rust must agree on its exact schema.
assert 'controlWorkerFrameBytes = 256 * 1024' in messages
assert 'analysisWorkerFrameBytes = 8 * 1024 * 1024' in messages
assert 'analysisXPCReplyBytes = 12 * 1024 * 1024' in messages
field_region=messages.split('static let jsonFields: Set<String> = [',1)[1].split(']\n    let `protocol`',1)[0]
swift_analysis_fields=set(re.findall(r'"([A-Za-z0-9_]+)"',field_region))
pos=analysis_source.index('\\"analysis_result\\"')
start=analysis_source.rfind('Ok(format!(',0,pos)
end=analysis_source.index('super::quote(&summary)))',pos)+len('super::quote(&summary)))')
rust_analysis_fields=set(re.findall(r'\\"([A-Za-z0-9_]+)\\":',analysis_source[start:end]))
assert swift_analysis_fields == rust_analysis_fields, (swift_analysis_fields-rust_analysis_fields, rust_analysis_fields-swift_analysis_fields)
assert len(swift_analysis_fields) == 71
for token in ['fn unified_symbol_records(', 'symbol_resolver', 'Precedence: exact nlist code symbol']:
    assert token in analysis_worker, token
assert r'\"unified_symbol_resolver\":true' in bridge_caps
assert (R/'docs/UNIFIED_SYMBOLS_0.5.5.md').is_file()
worker_supervisor_text=(R/'PlatformServices/CoordinatorService/WorkerSupervisor.swift').read_text()
assert 'Set(object.keys) == AnalysisReport.jsonFields' in worker_supervisor_text
assert 'maxFrameBytes: WorkbenchProtocolLimits.analysisWorkerFrameBytes' in worker_supervisor_text
assert 'process.waitUntilExit()' not in worker_supervisor_text
assert '.wait()' not in worker_supervisor_text
assert 'lifecycle.notify(queue: queue)' in worker_supervisor_text
assert 'func inspectionStatus(_ requestID: String' in (R/'Shared/CoordinatorProtocol.swift').read_text()
assert 'deadlineSeconds: 240' in worker_supervisor_text
assert 'recentStatuses' in worker_supervisor_text and 'while recentStatusOrder.count > 32' in worker_supervisor_text
assert 'active.update(.cancelling' in worker_supervisor_text
assert 'Worker diagnostic:' in worker_supervisor_text
assert 'if current.phase == .cancelled && phase == .failed { return }' in worker_supervisor_text
client_text=(R/'App/CoordinatorClient.swift').read_text()
assert 'maxReplyBytes: Int = WorkbenchProtocolLimits.defaultXPCReplyBytes' in client_text
assert 'invalidateConnectionOnTimeout: Bool = true' in client_text
assert 'failRequest(id: id, token: token' in client_text
assert 'invalidateConnectionOnTimeout: false' in workbench
assert 'startAnalysisMonitor(requestID:' in workbench
assert 'analysisProgressLabel' in workbench
assert 'client.request(timeout: 260, maxReplyBytes: WorkbenchProtocolLimits.analysisXPCReplyBytes' in workbench
assert 'maxReplyBytes: WorkbenchProtocolLimits.analysisXPCReplyBytes' in workbench
rust_protocol=(R/'rust/crates/protocol/src/lib.rs').read_text()
assert 'MAX_CONTROL_FRAME: usize = 256 * 1024' in rust_protocol
assert 'MAX_ANALYSIS_FRAME: usize = 8 * 1024 * 1024' in rust_protocol
assert 'write_frame_with_limit' in rust_protocol
rust_worker=(R/'rust/crates/worker/src/main.rs').read_text()
assert 'write_frame_with_limit(&mut io::stdout().lock(),response.as_bytes(),MAX_ANALYSIS_FRAME)' in rust_worker
content=(R/'App/ContentView.swift').read_text() + '\n' + (R/'App/WorkspaceCatalog.swift').read_text() + '\n' + (R/'App/WorkbenchChrome.swift').read_text()
for token in ['parse_text_relocations','ssa_hint','jump_tables','relocation_xrefs','structural_analysis','platform_metadata','classify_jump_candidate','runtime_records','annotated_disassembly','dominance_frontiers','swift_nominal_types','runtime_reference_graph','selector_call_xrefs','ssa_rename_trace','chained_page_starts','swift_field_records','predecessor_value_flow','chained_pointer_records','swift_field_ownership','iterative_value_flow','indirect_call_resolution','chained_multistart_records','arm64e_pointer_records','indirect_symbol_data','export_trie_records','symbol_provenance_records','joined_linkage_records','symbol_caller_records','got_auth_slot_records','import_export_collision_records','function_intelligence','pc_relative_data_references','cross_function_transfer_candidates','function_neighborhood_report','analysis_quality_report','analysis_insights']:
    assert token in analysis_source, token
for token in ['let ssa: String','let dominators: String','let dominance_frontiers: String','let phi_nodes: String','let relocations: String','let jump_tables: String','let metadata: String','let runtime_records: String','let annotated_disassembly: String','let ssa_renamed: String','let runtime_graph: String','let swift_types: String','let selector_xrefs: String','let chained_starts: String','let swift_field_records: String','let predecessor_values: String','let chained_pointers: String','let swift_field_ownership: String','let propagated_values: String','let indirect_calls: String','let chained_multistarts: String','let arm64e_pointers: String','let indirect_symbols: String','let pointer_slots: String','let export_trie: String','let symbol_provenance: String','let linkage_records: String','let symbol_callers: String','let got_auth_slots: String','let import_export_collisions: String','let function_intelligence: String','let data_references: String','let tail_call_candidates: String','let function_neighborhoods: String','let analysis_quality: String','let analysis_insights: String']:
    assert token in messages, token
for token in ['Analysis navigator','Dominators','Dominance frontiers','Phi placement','Apple runtime metadata','Recovered Apple runtime records','Symbol-aware disassembly','Relocation-aware references','Jump Tables','SSA Rename','Validated Objective-C runtime reference graph','Swift nominal-type descriptors','Objective-C selector / message-send xrefs','Dyld chained-fixup page starts','Swift field records','Predecessor reaching values','Decoded dyld chained pointers','Swift field ownership','Iterative inter-block values','Indirect-call resolution','Expanded dyld multi-start chains','ARM64e authenticated pointer fields','Context inspector','Dyld Imports','ARM64e Bind Symbols','Import Resolution','Dylib Ordinals','Resolved Imports','Authenticated Targets','Linker Semantics','Indirect symbol table','Lazy / non-lazy pointer slots','Dyld export trie','Symbol provenance','Linkage Explorer','Joined linkage records','Stub caller provenance','GOT / authenticated GOT','Import / export collisions','Selected Symbol','Native Debug Server','Execution Control','Memory / Breakpoints','Debug Threads','Target Memory Map','Loaded Images','Selected Thread Backtrace','Live ARM64 Disassembly','Stop / Exception Context','Follow PC','PC → Code','PC → CFG','Python Console','HindSight Extensions','Automation SDK','API v2','event hooks','Extensions Platform','Custom Analysis Passes','Dockable Views','Hypervisor-assisted Reverse Engineering','Guest Memory','Guest Disassembly','Firmware Loader','VM Snapshots','Guest Search','vCPU Monitor','VM Breakpoints','VM Trace','GPA Map','DART Inspector','Interrupt Trace','Firmware Runtime Lab','dyld Cache Browser','IPSW Catalog','Firmware Keys','VectorEmu Lab','VectorEmu Registers','VectorEmu Code','VectorEmu Trace','Full-System iPhemeral','Machine Lab','Mentor','Unified Target Model','Address Spaces','Session Identity','Evidence-backed Address Pivot','Binary Workspace','Runtime Correlation','Trace Explorer','Evidence Graph','Investigation Ledger','Binary Diff','Analysis Insights','Analysis Quality','Function Intelligence','Function Neighborhoods','Tail-Call Candidates','Data References']:
    assert token in content, token

# v0.4.0-beta.7 Rust compatibility contract: the Apple-Silicon release host
# currently qualifies VectorEmu with Rust 1.90. Keep every engine crate explicit
# about that MSRV and reject convenience APIs known to require newer stable Rust.
engine_manifests = sorted((R/'Engines/VectorEmu/crates').glob('*/Cargo.toml'))
for manifest in engine_manifests:
    package = tomllib.loads(manifest.read_text())['package']
    assert package.get('rust-version') == '1.90', f'VectorEmu MSRV drift in {manifest}'
hostmem=(R/'Engines/VectorEmu/crates/iphem-core/src/hostmem.rs').read_text()
assert '.isolate_lowest_one(' not in hostmem
assert 'bits & bits.wrapping_neg()' in hostmem
for source in (R/'Engines/VectorEmu/crates').rglob('*.rs'):
    text=source.read_text()
    for forbidden in ['.isolate_lowest_one(', '.isolate_highest_one(', '.bit_width(']:
        assert forbidden not in text, f'Rust >1.90 API {forbidden} in {source}'
assert (R/'scripts/check-rust-msrv.py').is_file()
assert tomllib.loads((R/'rust-toolchain.toml').read_text())['toolchain']['channel'] == '1.90.0'

# v0.4 beta-readiness contract: recover expected failures, preserve case data,
# and keep incomplete hardware models explicitly experimental.
production_swift = [p for base in [R/'App', R/'PlatformServices', R/'Shared'] for p in base.rglob('*.swift')]
for source in production_swift:
    text = source.read_text()
    for forbidden in ['fatalError(', 'preconditionFailure(', 'assertionFailure(', 'try!', ' as! ']:
        assert forbidden not in text, f'production Swift crash primitive {forbidden!r} in {source}'
    # Forced unwraps are also crash primitives. Match identifier/closing-delimiter bangs,
    # while excluding != and logical negation.
    assert not re.search(r'(?:[A-Za-z0-9_]|\)|\])!(?!=)', text), f'production Swift forced unwrap in {source}'
    # macOS 14+ SwiftUI compatibility: one-parameter onChange is deprecated,
    # and SwiftUI has no frame(width:minHeight:) overload.
    assert not re.search(r'\.onChange\(of:[^\n]+\)\s*\{\s*[^,\n{}]+\s+in', text), f'deprecated one-parameter onChange handler in {source}'
    assert not re.search(r'\.frame\([^\n)]*width\s*:[^\n)]*minHeight\s*:', text), f'invalid frame(width:minHeight:) overload in {source}'
worker_supervisor=(R/'PlatformServices/CoordinatorService/WorkerSupervisor.swift').read_text()
assert 'private let governor: OpaquePointer?' in worker_supervisor
assert 'var isAvailable: Bool { governor != nil }' in worker_supervisor
assert 'Rust admission governor is unavailable.' in worker_supervisor
assert 'acquireGlobalWorkerLock' in worker_supervisor and 'Another HindSight instance is already running a heavy analysis worker.' in worker_supervisor
project_store=(R/'PlatformServices/CoordinatorService/ProjectStore.swift').read_text()
assert 'PRAGMA quick_check' in project_store and 'cleanupAbandonedStaging' in project_store
assert '.isSymbolicLinkKey' in project_store and 'Project path must be a real directory, not a symbolic link.' in project_store
for token in ['private func validatedDirectory(', 'values.isDirectory == true, values.isSymbolicLink != true',
              'validatedDirectory(staging, createIfMissing: false, label: "Project staging")',
              'validatedDirectory(directory, createIfMissing: false, label: "Captured object storage")',
              'stagingValues.isRegularFile == true, stagingValues.isSymbolicLink != true',
              'Analysis cache destination must be a regular file, not a symbolic link or special file.']:
    assert token in project_store, token
assert 'try? FileManager.default.removeItem(at: destination)' in project_store, 'captured-object rollback cleanup missing'
ledger_model=(R/'App/InvestigationLedgerModel.swift').read_text()
assert 'investigation-ledger-v1.backup.json' in ledger_model
assert 'loadRecovering(projectPath:' in ledger_model
assert 'recoveredFromBackup' in ledger_model
assert '.isSymbolicLinkKey' in ledger_model and 'Case-storage path must be a real directory, not a symbolic link.' in ledger_model
virt_messages=(R/'Shared/VirtualizationMessages.swift').read_text()
virt_host=(R/'PlatformServices/CoordinatorService/VirtualizationHost.swift').read_text()
assert 'let maturity: String' in virt_messages
assert virt_host.count('maturity: "experimental"') >= 3
assert 'maturity.uppercased()' in content_ui
assert (R/'docs/BETA_READINESS.md').is_file()
assert (R/'scripts/test-beta-macos.sh').is_file()
assert (R/'scripts/test-coordinator-typecheck.sh').is_file()
for gate in ['scripts/test-macos.sh', 'scripts/test-beta-macos.sh']:
    gate_text=(R/gate).read_text()
    assert 'VectorGlassCoordinator.xpc' not in gate_text, f'stale XPC product name in {gate}'
    assert 'CoordinatorService.xpc' in gate_text, f'CoordinatorService bundle check missing in {gate}'
assert 'test-coordinator-typecheck.sh' in (R/'scripts/test-macos.sh').read_text()
dispatcher=(R/'PlatformServices/CoordinatorService/main.swift').read_text()
assert 'attributes: .concurrent' in dispatcher
assert 'running.count < maxConcurrent' in dispatcher
assert 'submittedAt: now' in dispatcher and '2_000_000_000' in dispatcher
assert 'firmwareLane.cancelQueued(); scriptingLane.cancelQueued()' in dispatcher
assert 'maxConcurrent: 1, maxPending: 16' in dispatcher
assert 'maxConcurrent: 4, maxPending: 64' in dispatcher
beta_gate=(R/'scripts/test-beta-macos.sh').read_text()
for token in ['VECTORGLASS_SIGN_IDENTITY','VECTORGLASS_DEVELOPMENT_TEAM','notarytool submit','stapler staple','stapler validate']:
    assert token in beta_gate, f'release signing/notarization gate missing: {token}'
assert (R/'scripts/generate-capabilities-schema.py').is_file()
subprocess.run([sys.executable, str(R/'scripts/generate-capabilities-schema.py'), '--check'], check=True)
cap_schema=json.loads((R/'schemas/capabilities.schema.json').read_text())
assert cap_schema['additionalProperties'] is False and len(cap_schema['required']) >= 160
assert cap_schema['properties']['release_channel']['enum'] == ['beta','rc','stable']
assert cap_schema['properties']['stable_release_channel']['const'] is True
assert cap_schema['properties']['runtime_governor']['const'] is True
assert 'CONFIGURATION="${CONFIGURATION:-Debug}"' in (R/'scripts/build-app.sh').read_text()
assert 'HINDSIGHT_SWIFT_DISABLE_SANDBOX' in (R/'scripts/build-app.sh').read_text()

# VectorGlass product split: public naming and built-in deterministic engine.
assert 'from hindsight import api as vg' in (R/'App/WorkbenchModel.swift').read_text()
scripting=(R/'PlatformServices/CoordinatorService/ScriptingHost.swift').read_text()
assert 'safePluginTree' in scripting and 'maxPluginPackageBytes' in scripting and 'maxPluginPackageEntries' in scripting
assert '.isSymbolicLinkKey' in scripting, 'plugin symlink boundary checks missing'
assert 'BoundedProcessOutput' in scripting, 'bounded concurrent scripting output missing'
assert 'types.ModuleType("hindsight")' in scripting
assert 'sys.modules["vectorglass"]=mod' in scripting  # existing extensions remain compatible
assert 'sys.modules["reworkbench"]=mod' in scripting  # migration compatibility only
assert '.vgplugin' in scripting
assert 'governedCapabilities' in scripting, 'extension capability policy is not explicit'
assert 'policy_status' in scripting and 'blocked by local policy' in scripting, 'extension policy is not fail-closed at execution'
assert 'HindSight.extensions.mode.v1' in scripting and 'verified-only' in scripting and 'disabled' in scripting, 'managed extension policy modes are not enforced'
assert 'Local policy' in (R/'App/ContentView.swift').read_text(), 'extension policy status is not visible in the UI'
continuity=(R/'App/AnalysisContinuity.swift').read_text()
assert 'Approved and Rejected decisions require a bounded rationale note.' in continuity
assert 'analysisCollectionHandoffReadinessDetail' in continuity and 'refresh the review baseline' in continuity
runtime_host=(R/'PlatformServices/CoordinatorService/FirmwareRuntimeHost.swift').read_text()
for token in ['SynchronousResult','BoundedOutputBuffer','readabilityHandler','SIGKILL','(200..<300).contains(http.statusCode)']:
    assert token in runtime_host, f'firmware/runtime hardening missing: {token}'
for token in ['vectoremu_start','vectoremu_stop','vectoremu_status','vectoremu_debug','127.0.0.1']:
    assert token in runtime_host, token
for required in [
    R/'Engines/VectorEmu/Cargo.toml',
    R/'Engines/VectorEmu/crates/vg-emu-server/src/main.rs',
    R/'Engines/VectorEmu/crates/vg-emu-server/src/debug.rs',
    R/'docs/PRODUCT_VISION.md',R/'docs/ARCHITECTURE_HINDSIGHT.md',
    R/'docs/VECTOREMU_PROTOCOL.md',R/'docs/COMMERCIAL_MODEL.md',R/'docs/ROADMAP.md',R/'docs/TARGET_MODEL.md',
    R/'Shared/TargetModel.swift',R/'App/TargetWorkspace.swift'
]: assert required.is_file(),required
emu_manifest=tomllib.loads((vector_emu/'Cargo.toml').read_text())
for member in emu_manifest['workspace']['members']:
    member_manifest=vector_emu/member/'Cargo.toml'
    assert member_manifest.is_file(),member_manifest
    cfg=tomllib.loads(member_manifest.read_text())
    for section in ('dependencies','dev-dependencies','build-dependencies'):
        for dep in cfg.get(section,{}).values():
            if isinstance(dep,dict) and 'path' in dep:
                assert (member_manifest.parent/dep['path']/'Cargo.toml').resolve().is_file(), (member_manifest,dep)
assert not (vector_emu/'crates/iphem-sdl').exists()
assert 'iphem-sdl' not in (vector_emu/'crates/iphem-soc/Cargo.toml').read_text()

# Current operational documentation must describe the actually shipped stable/hotfix line.
status_doc=(R/'docs/STATUS.md').read_text()
validation_doc=(R/'docs/VALIDATION.md').read_text()
macos_doc=(R/'docs/MACOS_VALIDATION.md').read_text()
readiness_doc=(R/'docs/RELEASE_READINESS.md').read_text()
assert status_doc.startswith('# HindSight 0.5.10 Status')
assert validation_doc.startswith('# Validation — HindSight 0.5.10')
for token in ['Hotfix 3', 'Function Neighborhoods', 'topology-derived']:
    assert token in status_doc or token in validation_doc, token
for token in ['Function Neighborhoods', 'AnalysisNavigation.swift', 'AnalysisContinuity.swift', 'objects', 'analysis-cache', 'staging']:
    assert token in macos_doc + readiness_doc, token

service=plistlib.loads((R/'Config/Coordinator-Info.plist').read_bytes())
assert service['CFBundleIdentifier']==topology['embedded_service']
assert topology['embedded_service'] in (R/'Shared/CoordinatorProtocol.swift').read_text()
assert service['XPCService']['ServiceType']=='Application'
assert (R/'Resources/schema.sql').is_file()
assert 'Resources/schema.sql' in topology['files']
assert b'Embed Rust worker' in pbx
for script in (R/'scripts').glob('*.sh'):
    subprocess.run(['bash','-n',str(script)],check=True)
    assert os.access(script,os.X_OK),script
manifest=tomllib.loads((R/'rust/Cargo.toml').read_text())
approved_external_dependencies={'flate2','lzvn'}
for member in manifest['workspace']['members']:
    config=tomllib.loads((R/'rust'/member/'Cargo.toml').read_text())
    for name, dep in config.get('dependencies',{}).items():
        if 'path' in dep:
            assert (R/'rust'/member/dep['path']/'Cargo.toml').is_file()
        else:
            assert name in approved_external_dependencies, f'Unapproved external dependency: {name}'
env=dict(os.environ,PYTHONPATH=str(R/'python'),PYTHONDONTWRITEBYTECODE='1')
subprocess.run([sys.executable,'-m','unittest','discover','-s',str(R/'python/tests'),'-v'],env=env,check=True)
# Portable Swift validation when a Swift compiler is available. Parsing catches syntax
# drift across the complete Swift surface; the target-model contract is Foundation-only
# and therefore runs on non-macOS CI as well.
import shutil, tempfile
swiftc=shutil.which('swiftc')
if swiftc:
    swift_sources=[str(p) for p in sorted(R.rglob('*.swift')) if 'build' not in p.relative_to(R).parts]
    for source in swift_sources:
        subprocess.run([swiftc,'-frontend','-parse',source],check=True)
    # Full type-check of the portable virtualization service surface. Unlike parse-only
    # validation, this catches missing target sources, opaque C-type importer mistakes,
    # and Swift argument-label/type errors before the Xcode build reaches CoordinatorService.
    subprocess.run([swiftc,'-typecheck','-import-objc-header',str(R/'Bridge/HindSightBridge.h'),
                    str(R/'Shared/VirtualizationMessages.swift'),
                    str(R/'Shared/HostPlatform.swift'),
                    str(R/'PlatformServices/CoordinatorService/VirtualizationHost.swift')],check=True)
    if sys.platform == 'darwin':
        subprocess.run([swiftc,'-typecheck',
                        str(R/'Shared/ScriptingMessages.swift'),
                        str(R/'PlatformServices/CoordinatorService/ScriptingHost.swift')],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-target-model-') as tmp:
        exe=pathlib.Path(tmp)/'target-model-contract'
        subprocess.run([swiftc,str(R/'Shared/TargetModel.swift'),str(R/'Tests/target_model_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-analysis-protocol-') as tmp:
        exe=pathlib.Path(tmp)/'analysis-protocol-contract'
        subprocess.run([swiftc,str(R/'Shared/WorkbenchMessages.swift'),str(R/'Tests/analysis_protocol_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-analysis-status-') as tmp:
        exe=pathlib.Path(tmp)/'analysis-status-contract'
        subprocess.run([swiftc,str(R/'Shared/WorkbenchMessages.swift'),str(R/'Tests/analysis_status_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-disassembly-model-') as tmp:
        exe=pathlib.Path(tmp)/'disassembly-presentation-contract'
        subprocess.run([swiftc,str(R/'App/DisassemblyModel.swift'),str(R/'Tests/disassembly_presentation_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-graph-model-') as tmp:
        exe=pathlib.Path(tmp)/'graph-model-contract'
        subprocess.run([swiftc,str(R/'App/GraphModel.swift'),str(R/'Tests/graph_model_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-binary-workspace-') as tmp:
        exe=pathlib.Path(tmp)/'binary-workspace-contract'
        subprocess.run([swiftc,str(R/'App/DisassemblyModel.swift'),str(R/'App/GraphModel.swift'),str(R/'App/BinaryWorkspaceModel.swift'),str(R/'Tests/binary_workspace_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-binary-diff-') as tmp:
        exe=pathlib.Path(tmp)/'binary-diff-contract'
        subprocess.run([swiftc,str(R/'Shared/WorkbenchMessages.swift'),str(R/'App/DisassemblyModel.swift'),str(R/'App/GraphModel.swift'),str(R/'App/BinaryWorkspaceModel.swift'),str(R/'App/BinaryDiffModel.swift'),str(R/'Tests/binary_diff_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-runtime-correlation-') as tmp:
        exe=pathlib.Path(tmp)/'runtime-correlation-contract'
        subprocess.run([swiftc,str(R/'Shared/TargetModel.swift'),str(R/'Shared/DebugMessages.swift'),str(R/'App/GraphModel.swift'),str(R/'App/InvestigationLedgerModel.swift'),str(R/'App/RuntimeCorrelationModel.swift'),str(R/'Tests/runtime_correlation_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-trace-explorer-') as tmp:
        exe=pathlib.Path(tmp)/'trace-explorer-contract'
        subprocess.run([swiftc,str(R/'Shared/TargetModel.swift'),str(R/'Shared/DebugMessages.swift'),str(R/'App/DisassemblyModel.swift'),str(R/'App/GraphModel.swift'),str(R/'App/InvestigationLedgerModel.swift'),str(R/'App/RuntimeCorrelationModel.swift'),str(R/'App/TraceExplorerModel.swift'),str(R/'Tests/trace_explorer_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-evidence-graph-') as tmp:
        exe=pathlib.Path(tmp)/'evidence-graph-contract'
        subprocess.run([swiftc,str(R/'App/GraphModel.swift'),str(R/'App/InvestigationLedgerModel.swift'),str(R/'App/EvidenceGraphModel.swift'),str(R/'Tests/evidence_graph_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
    with tempfile.TemporaryDirectory(prefix='vectorglass-investigation-ledger-') as tmp:
        exe=pathlib.Path(tmp)/'investigation-ledger-contract'
        subprocess.run([swiftc,str(R/'App/InvestigationLedgerModel.swift'),str(R/'Tests/investigation_ledger_contract.swift'),'-o',str(exe)],check=True)
        subprocess.run([str(exe)],check=True)
print('Source topology, metadata, local dependencies, shell syntax and Python tests passed.')
print('Portable Swift parsing/target-model contract ran when swiftc was available; full Rust/Xcode linking remains a macOS release gate.')

# VectorEmu must remain dependency-complete. The 0.2.6 server referenced the
# undeclared `hex` crate from four production call sites, which only surfaced
# once Cargo reached vg-emu-server on macOS. Keep the bounded local codec unless
# a future manifest intentionally adds a hex dependency and updates Cargo.lock.
vector_debug = (R/'Engines/VectorEmu/crates/vg-emu-server/src/debug.rs').read_text()
vector_manifest = (R/'Engines/VectorEmu/crates/vg-emu-server/Cargo.toml').read_text()
assert not ('hex::' in vector_debug and '\nhex' not in vector_manifest), 'vg-emu-server references hex:: without declaring a hex dependency'
for token in ['fn encode_hex(bytes: &[u8]) -> String', 'fn decode_hex(text: &str) -> Result<Vec<u8>, String>']:
    assert token in vector_debug, f'vg-emu-server missing internal codec: {token}'

# Xcode exports build settings as environment variables. Never place $(SETTING)
# syntax inside the generated shell body: /bin/sh treats it as command substitution.
generator_text = (R/'scripts/generate-project.py').read_text()
assert 'build-vector-emu.sh\" \"$(TARGET_BUILD_DIR)' not in generator_text, 'VectorEmu phase embeds Xcode $(...) syntax in shell source'
pbx_text = (R/'HindSight.xcodeproj/project.pbxproj').read_text()
assert '\\"${SRCROOT}/scripts/build-vector-emu.sh\\"\\n' in pbx_text, 'VectorEmu phase must invoke build script without a synthesized destination argument'
assert '\"$(TARGET_BUILD_DIR)/$(EXECUTABLE_FOLDER_PATH)\"' not in pbx_text, 'generated VectorEmu shell phase still contains command substitution syntax'
vector_build = (R/'scripts/build-vector-emu.sh').read_text()
for token in ['OUT="$TARGET_BUILD_DIR/$EXECUTABLE_FOLDER_PATH"', 'refusing unsafe VectorEmu output directory', 'VectorEmu output must remain inside TARGET_BUILD_DIR']:
    assert token in vector_build, f'VectorEmu staging safety guard missing: {token}'
