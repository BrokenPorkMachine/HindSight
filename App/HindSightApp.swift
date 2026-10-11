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
        Form {
            Section("Startup") {
                Picker("Preferred layout", selection: $model.preferredLayoutPreset) {
                    ForEach(VGWorkspaceCatalog.layouts, id: \.self) { Text($0).tag($0) }
                }
                Button("Apply to current project") { model.layoutPreset = model.preferredLayoutPreset }
                    .disabled(model.layoutPreset == model.preferredLayoutPreset)
                Text("The last project view is restored when available; this preference is used as the default for a new session.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("Investigation") {
                Toggle("Show mentor guidance", isOn: $model.mentorMode)
                Text("Mentor mode adds short explanations around investigation controls without changing analysis results.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("Support & Readiness") {
                VGReadinessRow(label: "Build", value: buildIdentity)
                VGReadinessRow(label: "Host", value: hostIdentity)
                VGReadinessRow(label: "Coordinator", value: coordinatorIdentity)
                VGReadinessRow(label: "Project session", value: model.projectSessionReady ? "Ready" : "Not ready")
                VGReadinessRow(label: "Native debugger", value: VGHostPlatform.supportsArm64NativeDebugger ? "Available" : "Unavailable")
                VGReadinessRow(label: "Hypervisor", value: VGHostPlatform.supportsAppleSiliconHypervisor ? "Available" : "Unavailable")
                Divider()
                HStack {
                    Button("Copy diagnostics") { model.copyDiagnostics() }
                    Button("Export diagnostics…") { model.exportDiagnostics() }
                    Button("Reconnect Coordinator") { model.reconnect() }
                        .disabled(model.busy || model.connectingToCoordinator || model.restoringProjectSession)
                }
                Text("Diagnostics contain runtime and capability state, not target-file contents. Share them only with your support team.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .padding()
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
