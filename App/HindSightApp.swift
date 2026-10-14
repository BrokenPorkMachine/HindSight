import SwiftUI

@main
@MainActor
struct HindSightApp: App {
    @StateObject private var model = WorkbenchModel()
    var body: some Scene {
        WindowGroup(VGProduct.displayName) {
            ContentView(model: model)
                .frame(minWidth: 900, minHeight: 680)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1480, height: 920)
        Settings {
            HindSightSettingsView(model: model)
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Project…", action: model.newProject)
                    .keyboardShortcut("n")
                    .disabled(model.busy || !model.connected || model.connectingToCoordinator || model.restoringProjectSession)
                Button("Open Project…", action: model.openProject)
                    .keyboardShortcut("o")
                    .disabled(model.busy || !model.connected || model.connectingToCoordinator || model.restoringProjectSession)
                Button("Import Binary…") { model.selectBinary() }
                    .keyboardShortcut("i")
                    .disabled(!model.canImportBinary)
            }
            CommandMenu("Support") {
                Button("Reconnect Coordinator", action: model.reconnect)
                    .disabled(model.busy || model.connectingToCoordinator || model.restoringProjectSession)
                Button("Restore Project Session", action: model.retryProjectRestore)
                    .disabled(!model.connected || model.project == nil || model.projectSessionReady || model.busy || model.restoringProjectSession)
                Divider()
                Button("Reopen Last Project", action: model.reopenSavedLaunchSession)
                    .disabled(!model.savedLaunchSessionAvailable || model.project != nil || !model.connected || model.busy || model.connectingToCoordinator || model.restoringProjectSession)
                Button("Forget Saved Project", action: model.forgetSavedLaunchSession)
                    .disabled(!model.savedLaunchSessionAvailable || model.project != nil || model.busy || model.restoringProjectSession)
                Divider()
                Button("Copy Diagnostics", action: model.copyDiagnostics)
            }
            CommandMenu("Workspace") {
                Button("Investigation Home") { model.workspace = "Overview" }
                    .keyboardShortcut("1", modifiers: .command)
                Button("Targets") { model.workspace = "Targets" }
                    .keyboardShortcut("2", modifiers: .command)
                Button("Analysis Triage") { model.workspace = "Analysis Triage" }
                Button("Resume Latest Evidence", action: model.resumeMostRecentAnalysisVisit)
                    .keyboardShortcut("r", modifiers: [.command, .option])
                    .disabled(model.analysisRecentVisits.isEmpty)
                Button("Previous Evidence", action: model.navigateAnalysisHistoryBackward)
                    .keyboardShortcut("[", modifiers: [.command, .shift])
                    .disabled(!model.canNavigateAnalysisHistoryBackward)
                Button("Next Evidence", action: model.navigateAnalysisHistoryForward)
                    .keyboardShortcut("]", modifiers: [.command, .shift])
                    .disabled(!model.canNavigateAnalysisHistoryForward)
                Button("Search & Navigate") { model.workspace = "Navigator" }
                    .keyboardShortcut("3", modifiers: .command)
                Button("Debugger") { model.workspace = "Debugger" }
                    .keyboardShortcut("4", modifiers: .command)
                    .disabled(!VGHostPlatform.supportsArm64NativeDebugger)
                    .help(VGHostPlatform.supportsArm64NativeDebugger ? "Open the native ARM64 debugger." : VGHostPlatform.unavailableReason(for: "Native ARM64 debugging"))
                Button("VectorEmu") { model.workspace = "VectorEmu Lab" }
                    .keyboardShortcut("5", modifiers: .command)
                Button("REA Agent") { model.workspace = "REA Agent" }
                Button("Terminal") { model.workspace = "Terminal" }
                Divider()
                Button("Notes") { model.workspace = "Notes" }
                    .keyboardShortcut("0", modifiers: .command)
            }
        }
    }
}

private struct HindSightSettingsView: View {
    @ObservedObject var model: WorkbenchModel

    var body: some View {
        // A SwiftUI Form is List-backed on macOS and shares the NSOutlineView
        // teardown path that crashed AttributeGraph during live window resize
        // (SwiftUI.OutlineListCoordinator / AG::Graph::resolve_offset). Settings
        // therefore uses the same crash-safe ScrollView primitive as the rest of
        // the workbench and keeps only plain controls inside it.
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                settingsSection("Startup") {
                    Picker("Preferred layout", selection: $model.preferredLayoutPreset) {
                        ForEach(VGWorkspaceCatalog.layouts, id: \.self) { Text($0).tag($0) }
                    }
                    Button("Apply to current project") { model.layoutPreset = model.preferredLayoutPreset }
                        .disabled(model.layoutPreset == model.preferredLayoutPreset)
                    settingsNote("The last project view is restored when available; this preference is used as the default for a new session.")
                }
                settingsSection("Investigation") {
                    Toggle("Show mentor guidance", isOn: $model.mentorMode)
                    settingsNote("Mentor mode adds short explanations around investigation controls without changing analysis results.")
                }
                settingsSection("REA Agents") {
                    LabeledContent("Integration", value: "Optional · local MCP bridge")
                    LabeledContent("Provider", value: "rea-agents")
                    settingsNote("REA support is designed to run as a separately versioned local process. HindSight will request an explicit target and preserve REA evidence provenance when the bridge is enabled.")
                    settingsNote("Upstream project: github.com/morluto/rea")
                    VGReadinessRow(label: "Probe", value: model.reaStatus)
                    Button("Probe local REA agent") { model.probeREAAgents() }
                        .disabled(!model.connected || model.busy)
                    TextField("Absolute target path", text: $model.reaTargetPath)
                        .textFieldStyle(.roundedBorder)
                    Picker("Operation", selection: $model.reaOperation) {
                        Text("Binary overview").tag("binary_overview")
                        Text("List procedures").tag("list_procedures")
                        Text("List names").tag("list_names")
                    }
                    Picker("Provider", selection: $model.reaProvider) {
                        Text("Automatic").tag("auto")
                        Text("Hopper").tag("hopper")
                        Text("Ghidra").tag("ghidra")
                        Text("IDA").tag("ida")
                    }
                    Button("Analyze selected target with REA") { model.runREAOperation() }
                        .disabled(!model.connected || model.busy || model.reaTargetPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                settingsSection("Support & Readiness") {
                    VGReadinessRow(label: "Build", value: buildIdentity)
                    VGReadinessRow(label: "Host", value: hostIdentity)
                    VGReadinessRow(label: "Coordinator", value: coordinatorIdentity)
                    VGReadinessRow(label: "Project session", value: model.projectSessionReady ? "Ready" : "Not ready")
                    VGReadinessRow(label: "Native debugger", value: VGHostPlatform.supportsArm64NativeDebugger ? "Available" : "Unavailable")
                    VGReadinessRow(label: "Hypervisor", value: VGHostPlatform.supportsAppleSiliconHypervisor ? "Available" : "Unavailable")
                    Divider()
                    VGFlowLayout {
                        Button("Copy diagnostics") { model.copyDiagnostics() }
                        Button("Export diagnostics…") { model.exportDiagnostics() }
                        Button("Reconnect Coordinator") { model.reconnect() }
                            .disabled(model.busy || model.connectingToCoordinator || model.restoringProjectSession)
                    }
                    if let error = model.operationErrors.last {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Last operation issue · \(error.code)").font(.caption).fontWeight(.semibold)
                            Text(error.message).font(.caption).foregroundStyle(.secondary)
                            Text("Recovery: \(error.recovery)").font(.caption2).foregroundStyle(.secondary)
                        }
                    }
                    settingsNote("Diagnostics contain runtime and capability state, not target-file contents. Share them only with your support team.")
                }
            }
            .padding()
        }
        .frame(width: 460)
    }

    @ViewBuilder private func settingsSection<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.headline)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }

    private func settingsNote(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var buildIdentity: String { "\(VGProduct.version) · \(VGProduct.releaseChannel)" }
    private var hostIdentity: String { "\(VGHostPlatform.displayName) · \(VGHostPlatform.architecture.rawValue)" }
    private var coordinatorIdentity: String {
        guard model.connected else { return "Disconnected" }
        return "Connected · \(model.coordinatorVersion.isEmpty ? "version pending" : model.coordinatorVersion)"
    }
}

private struct VGReadinessRow: View {
    let label: String
    let value: String

    var body: some View {
        LabeledContent(label, value: value)
            .textSelection(.enabled)
    }
}
