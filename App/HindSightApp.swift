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
                Button("Binary Workspace") { model.workspace = "Binary Workspace" }
                Button("Pseudocode") { model.workspace = "Pseudocode" }
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
    @State private var remoteReviewRevision = 0

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
                    TextField("Keychain TLS identity subject", text: $model.remoteCertificateLabel)
                    TextField("Bind IPv4 address (127.0.0.1 or private LAN IP)", text: $model.remoteBindAddress)
                    TextField("Allowed HTTPS browser origin (LAN only)", text: $model.remoteAllowedOrigin)
                    TextField("HTTPS port", text: $model.remotePort)
                    VGReadinessRow(label: "Remote gateway", value: model.remoteStatus)
                    Button("Start HTTPS gateway") { model.startRemoteGateway() }
                    Button("Stop gateway") { model.stopRemoteGateway() }
                    if !model.remoteToken.isEmpty {
                        Text("Session token (copy now; regenerated on start)")
                        Text(model.remoteToken).textSelection(.enabled).font(.system(.caption, design: .monospaced))
                    }
                    settingsNote("Remote access requires a private LAN address, a trusted TLS certificate whose SAN matches the client address, and one exact HTTPS browser origin. Analysis jobs are available; uploads, event streaming, debugger and VM controls are not exposed yet.")
                    DisclosureGroup("Remote identities and grants") {
                        HStack {
                            TextField("New identity label", text: $model.remoteIdentityName)
                            Button("Enroll") {
                                model.enrollRemoteIdentity()
                                remoteReviewRevision &+= 1
                            }
                        }
                        if !model.remoteEnrollmentToken.isEmpty {
                            Text("One-time bearer token — copy now; not stored on disk")
                                .font(.caption2).foregroundStyle(.secondary)
                            Text(model.remoteEnrollmentToken)
                                .textSelection(.enabled).font(.system(size: 9, design: .monospaced))
                        }
                        settingsNote("Enrollment creates an identity and temporary bearer token, but no access grant. Grant access to one record or the whole open project. Project grants include project snapshot reads and analysis for every captured record. Stopping the gateway revokes all live tokens.")
                        let identities = model.remoteIdentitiesForSettings
                        if identities.isEmpty {
                            Text("No remote identities have been recorded.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        ForEach(identities, id: \.id) { identity in
                            VStack(alignment: .leading, spacing: 5) {
                                HStack {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(identity.name ?? identity.subject).font(.caption.weight(.semibold))
                                        Text(identity.id).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    Text(identity.isActive ? "Active" : "Disabled")
                                        .font(.caption2).foregroundStyle(identity.isActive ? .green : .secondary)
                                    Button(identity.isActive ? "Disable" : "Enable") {
                                        model.setRemoteIdentityActive(identity.id, active: !identity.isActive)
                                        remoteReviewRevision &+= 1
                                    }
                                    .buttonStyle(.borderless)
                                    if identity.isActive {
                                        Button("Issue token") {
                                            model.issueRemoteIdentityToken(identity.id)
                                            remoteReviewRevision &+= 1
                                        }
                                        .buttonStyle(.borderless)
                                    }
                                    Menu("Grant record access") {
                                        if let project = model.project, !project.records.isEmpty {
                                            ForEach(project.records) { record in
                                                Button(record.name) {
                                                    model.grantRemoteRecord(record.id, to: identity.id)
                                                    remoteReviewRevision &+= 1
                                                }
                                            }
                                        } else {
                                            Text("Open a project with imported records first")
                                        }
                                    }
                                    .disabled(!identity.isActive || model.project?.records.isEmpty != false)
                                    .menuStyle(.borderlessButton)
                                    Button("Grant project access") {
                                        model.grantRemoteProject(to: identity.id)
                                        remoteReviewRevision &+= 1
                                    }
                                    .disabled(!identity.isActive || model.project?.records.isEmpty != false)
                                    .buttonStyle(.borderless)
                                }
                                ForEach(Array(model.remoteGrantsForSettings(identity.id).enumerated()), id: \.offset) { grantIndex, grant in
                                    HStack(alignment: .top) {
                                        VStack(alignment: .leading, spacing: 2) {
                                            Text(grant.projectPath).font(.system(size: 9, design: .monospaced)).textSelection(.enabled)
                                            Text("\(grant.recordID.map { "Record \($0) · " } ?? "Project-wide · ")\(grant.capabilities.sorted().joined(separator: ", "))\(grant.isExpired ? " · expired" : "")")
                                                .font(.system(size: 9)).foregroundStyle(.secondary)
                                        }
                                        Spacer(minLength: 4)
                                        Button("Revoke", role: .destructive) {
                                            model.revokeRemoteGrant(identity.id, index: grantIndex)
                                            remoteReviewRevision &+= 1
                                        }
                                        .font(.caption2)
                                        .buttonStyle(.borderless)
                                    }
                                    .padding(.leading, 10)
                                }
                            }
                            .padding(.vertical, 4)
                        }
                    }
                    .font(.caption)
                    .id(remoteReviewRevision)
                    DisclosureGroup("Recent remote audit (up to 100 requests)") {
                        Button("Refresh audit entries") { remoteReviewRevision &+= 1 }
                            .buttonStyle(.borderless)
                        let entries = model.remoteAuditForSettings
                        if entries.isEmpty {
                            Text("No remote requests have been recorded.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 7) {
                                ForEach(entries.reversed(), id: \.id) { entry in
                                    VStack(alignment: .leading, spacing: 2) {
                                        HStack {
                                            Text("\(entry.method) \(entry.path)").font(.system(size: 10, weight: .semibold, design: .monospaced))
                                            Spacer()
                                            Text("\(entry.statusCode)").font(.system(size: 10, design: .monospaced))
                                                .foregroundStyle(entry.statusCode >= 400 ? .orange : .secondary)
                                        }
                                        Text("\(entry.timestamp.formatted(date: .abbreviated, time: .standard)) · user \(entry.userID ?? "unknown") · \(entry.latencyMs) ms · \(entry.bytesIn) in / \(entry.bytesOut) out")
                                            .font(.system(size: 9)).foregroundStyle(.secondary)
                                        if let projectPath = entry.projectPath {
                                            Text(projectPath).font(.system(size: 9, design: .monospaced)).textSelection(.enabled)
                                        }
                                    }
                                    .padding(.vertical, 3)
                                    Divider()
                                }
                            }
                        }
                        .frame(maxHeight: 260)
                    }
                    .font(.caption)
                    .id(remoteReviewRevision)
                    Divider()
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
