import SwiftUI


/// Deliberately limited structural pseudocode. No type, condition or call
/// semantics are asserted without lifting evidence. Rendering is capped.
private func vgStructuralPseudocode(_ ir: String) -> String {
    var result = ["// Evidence-backed structural C sketch; types and values are unknown"]
    var bytes = 0
    for line in ir.split(separator: "\n").prefix(120) {
        let raw = String(line.prefix(240))
        bytes += raw.utf8.count
        if bytes > 32_768 { result.append("// truncated: byte limit"); break }
        // IR layout: address, kind, assembly. Do not infer target or condition
        // from a free-text substring of the assembly mnemonic.
        let fields = raw.split(whereSeparator: { $0.isWhitespace })
        guard fields.count >= 3 else { result.append("/* malformed IR */ // " + raw); continue }
        let kind = String(fields[1]).lowercased()
        let assembly = fields.dropFirst(2).joined(separator: " ")
        let mnemonic = assembly.split(separator: " ").first.map(String.init) ?? ""
        let operandText = String(assembly.dropFirst(mnemonic.count)).trimmingCharacters(in: .whitespaces)
        switch kind {
        case "cbranch": result.append("if (/* condition unresolved */) goto /* branch target unresolved */; // " + raw)
        case "branch": result.append("goto /* branch target unresolved */; // " + raw)
        case "term":
            if ["ret", "retq", "retn"].contains(mnemonic.lowercased()) {
                result.append("return /* return value unresolved */; // " + raw)
            } else { result.append("/* terminal transfer */ // " + raw) }
        case "op":
            // Recognize effects only where supported by the mnemonic itself.
            // No register value, inferred type, or side effect is fabricated.
            let op = mnemonic.lowercased()
            if ["bl", "blr", "call", "callq"].contains(op) {
                let target = operandText.isEmpty ? "target unresolved" : operandText
                result.append("call_target(\(target)); /* signature and side effects unresolved */ // " + raw)
            } else if ["ldr", "ldrb", "ldrh", "ldrsw", "mov", "movz", "movk", "lea"].contains(op) {
                result.append("value_flow(\(operandText.isEmpty ? "operands unresolved" : operandText)); /* direction and types unresolved */ // " + raw)
            } else if ["str", "strb", "strh", "stp"].contains(op) {
                result.append("memory_write(\(operandText.isEmpty ? "address/value unresolved" : operandText)); /* layout and aliasing unresolved */ // " + raw)
            } else if ["cmp", "cmn", "test", "tst"].contains(op) {
                result.append("compare_flags(\(operandText.isEmpty ? "operands unresolved" : operandText)); /* predicate unresolved */ // " + raw)
            } else if ["adr", "adrp", "add", "sub", "and", "orr", "eor", "xor", "imul"].contains(op) {
                result.append("compute(\(operandText.isEmpty ? "operands unresolved" : operandText)); /* result and types unresolved */ // " + raw)
            } else {
                result.append("/* instruction semantics not lifted */ // " + raw)
            }
        case "data": result.append("/* data-in-code boundary */ // " + raw)
        default: result.append("/* opaque barrier */ // " + raw)
        }
    }
    return result.joined(separator: "\n")
}

@MainActor
private struct VGBoundedEvidenceList: View {
    @ObservedObject var model: WorkbenchModel
    let text: String
    let maxRows: Int
    let batchSize: Int
    private let rows: [Substring]
    @State private var visibleCount: Int

    init(model: WorkbenchModel, text: String, maxRows: Int = 4000, batchSize: Int = 200) {
        self.model = model
        self.text = text
        self.maxRows = max(1, maxRows)
        self.batchSize = max(25, batchSize)
        let bounded = Array(text.split(separator: "\n", omittingEmptySubsequences: true).prefix(max(1, maxRows)))
        self.rows = bounded
        _visibleCount = State(initialValue: min(max(25, batchSize), bounded.count))
    }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 4) {
                ForEach(Array(rows.prefix(visibleCount).enumerated()), id: \.offset) { _, raw in
                    let line = String(raw)
                    Button { model.selectEvidence(line) } label: {
                        Text(line)
                            .font(.system(.caption, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }
                    .buttonStyle(.plain)
                    .padding(.vertical, 2)
                    .contextMenu {
                        Button("Select Evidence") { model.selectEvidence(line) }
                        Button("Send Evidence to Terminal") { model.sendContextToTerminal(line) }
                        Divider()
                        Button("OpenCode: Explain") { model.runOpenCodePrompt("Explain this evidence conservatively and separate observations from hypotheses.", context: line) }
                        Button("OpenCode: Find implications") { model.runOpenCodePrompt("Identify useful reverse-engineering implications and follow-up checks for this evidence.", context: line) }
                        Button("OpenCode: Draft note") { model.runOpenCodePrompt("Draft a concise analyst note grounded only in this evidence.", context: line) }
                        if let address = model.analysisEvidenceAddress(line) {
                            Button("Open 0x\(String(address, radix: 16)) in Disassembly") {
                                model.selectEvidence(line)
                                model.addressText = String(format: "0x%llx", address)
                                model.workspace = "Disassembly"
                                if model.currentPage != nil { model.jumpToAddress() } else { model.loadPage() }
                            }
                            Button("Focus 0x\(String(address, radix: 16)) in CFG") {
                                let value = String(format: "0x%016llx", address)
                                model.selectEvidence(line)
                                model.addressText = value
                                model.cfgFocus = value
                                model.workspace = "CFG"
                            }
                        }
                    }
                }

                if visibleCount < rows.count {
                    HStack(spacing: 10) {
                        Button("Show next \(min(batchSize, rows.count - visibleCount))") {
                            visibleCount = min(rows.count, visibleCount + batchSize)
                        }
                        .buttonStyle(.bordered)
                        Text("Showing \(visibleCount) of \(rows.count) evidence lines")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        Spacer()
                    }
                    .padding(.vertical, 8)
                } else if rows.count >= maxRows {
                    Text("Display capped at \(maxRows) evidence lines. Use Analysis Search or Triage to narrow the corpus.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 8)
                }
            }
            .padding(6)
        }
        .onChange(of: text) { _, _ in
            visibleCount = min(batchSize, rows.count)
        }
    }
}

@MainActor
struct ContentView: View {
    @ObservedObject var model: WorkbenchModel
    @State private var commandPalettePresented = false
    @State private var sidebarQuery = ""
    @State private var showInspector = true
    @State private var inspectorMode: VGInspectorMode = .context
    @State private var workspaceTabs = ["Overview"]
    @State private var focusMode = false
    @State private var inspectorBeforeFocus = true
    @State private var quickStartPresented = false


    private func parsedShortcut(_ spec: String?) -> (KeyEquivalent, EventModifiers)? {
        guard let spec, !spec.isEmpty else { return nil }
        let parts = spec.lowercased().split(separator: "+").map(String.init)
        guard let keyPart = parts.last, keyPart.count == 1, let char = keyPart.first else { return nil }
        var modifiers: EventModifiers = []
        for part in parts.dropLast() {
            switch part {
            case "cmd", "command": modifiers.insert(.command)
            case "shift": modifiers.insert(.shift)
            case "opt", "option", "alt": modifiers.insert(.option)
            case "ctrl", "control": modifiers.insert(.control)
            default: return nil
            }
        }
        return (KeyEquivalent(char), modifiers)
    }

    @ViewBuilder private func pluginCommandButton(_ plugin: PluginInfo, _ command: PluginCommand) -> some View {
        let button = Button(command.title) { model.selectedPluginID = plugin.id; model.runPluginCommand(command) }
        if let parsed = parsedShortcut(command.shortcut) { button.keyboardShortcut(parsed.0, modifiers: parsed.1) } else { button }
    }

    private func rememberWorkspace(_ name: String) {
        guard !name.isEmpty else { return }
        workspaceTabs.removeAll(where: { $0 == name })
        workspaceTabs.append(name)
        if workspaceTabs.count > 8 {
            let protected = model.workspace
            while workspaceTabs.count > 8, let candidate = workspaceTabs.first, candidate != protected {
                workspaceTabs.removeFirst()
            }
        }
    }

    private func closeWorkspaceTab(_ name: String) {
        guard workspaceTabs.count > 1, let index = workspaceTabs.firstIndex(of: name) else { return }
        let wasActive = model.workspace == name
        workspaceTabs.remove(at: index)
        if wasActive {
            let replacementIndex = min(index, workspaceTabs.count - 1)
            model.workspace = workspaceTabs[replacementIndex]
        }
    }

    private func toggleFocusMode() {
        if focusMode {
            focusMode = false
            showInspector = inspectorBeforeFocus
        } else {
            inspectorBeforeFocus = showInspector
            focusMode = true
            showInspector = false
        }
    }

    private var hasNextPage: Bool {
        guard let page = model.currentPage, let start = UInt64(page.start_word), let total = UInt64(page.total_words), start < total else { return false }
        return UInt64(page.word_count) < total - start
    }

    @ViewBuilder private func monospaced(_ text: String) -> some View {
        ScrollView([.horizontal, .vertical]) {
            Text(text).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading).padding(8)
        }
    }

    @ViewBuilder private func analysisToolbar(_ record: ProjectRecord) -> some View {
        HStack(spacing: 10) {
            Button(model.currentAnalysis == nil ? "Analyze binary" : "Analyze again", action: model.analyzeSelected)
                .disabled(model.busy || !model.projectServiceReady || !record.result.captured)
            Button("Clear cache…", action: model.clearAnalysisCache)
                .disabled(model.busy || !model.projectServiceReady)
            if let report = model.currentAnalysis {
                Label("\(report.function_count) functions", systemImage: "function")
                Label("\(report.basic_block_count) blocks", systemImage: "point.3.connected.trianglepath.dotted")
                if report.truncated { Label("bounded scan", systemImage: "gauge.with.dots.needle.50percent") }
            }
            Spacer()
        }.font(.caption)
    }

    @ViewBuilder private func instructionBrowser(_ record: ProjectRecord) -> some View {
        if record.result.captured {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Button("From start") { model.loadPage() }.disabled(model.busy || !model.projectServiceReady)
                    Button("Previous", action: model.previousPage).disabled(model.busy || !model.projectServiceReady || model.currentPage == nil || model.currentPage?.start_word == "0")
                    Button("Next", action: model.nextPage).disabled(model.busy || !model.projectServiceReady || !hasNextPage)
                    Button("Verify", action: model.verifyPage).disabled(model.busy || !model.projectServiceReady)
                    Spacer()
                    if let page = model.currentPage { Text("\(page.word_count) \(page.unitBytes == 1 ? "bytes" : page.unitBytes == 2 ? "halfwords" : "words") • start \(page.start_word)").font(.caption).foregroundStyle(.secondary) }
                }
                HStack {
                    TextField("Virtual address (hex)", text: $model.addressText).frame(maxWidth: 260)
                    Button("Go", action: model.jumpToAddress).disabled(model.busy || !model.projectServiceReady || model.currentPage == nil)
                }
                VGRichDisassemblyView(
                    text: model.currentPage?.text ?? record.result.disassembly ?? "",
                    selectedAddress: model.addressText,
                    emptyMessage: "Choose From start to load a verified instruction page.",
                    showFileOffset: true,
                    onSelectAddress: { value in
                        model.addressText = value.hasPrefix("0x") ? value : "0x" + value
                        model.selectEvidence("address=\(model.addressText)")
                    }
                )
                .frame(minHeight: 420)
            }
        } else { Text("This legacy record has no captured bytes.").foregroundStyle(.secondary) }
    }

    @ViewBuilder private func clickableEvidence(_ text: String) -> some View {
        VGBoundedEvidenceList(model: model, text: text)
    }


    @ViewBuilder private func pythonWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Python Console • API v2") {
                VStack(alignment: .leading, spacing: 10) {
                    VStack(alignment: .leading, spacing: 7) {
                        VGFlowLayout {
                            Button("Run", action: model.runPython).disabled(model.busy)
                            Stepper("Timeout \(model.pythonTimeoutMS) ms", value: $model.pythonTimeoutMS, in: 250...30000, step: 250)
                        }
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 8) {
                                TextField("Python 3 interpreter (blank = env python3)", text: $model.pythonInterpreter)
                                Button("Choose…", action: model.choosePythonInterpreter)
                            }
                            VStack(alignment: .leading, spacing: 8) {
                                TextField("Python 3 interpreter (blank = env python3)", text: $model.pythonInterpreter)
                                Button("Choose…", action: model.choosePythonInterpreter)
                            }
                        }
                    }
                    Text("Runs out-of-process through the Coordinator. No shell is used. Source ≤256 KiB, context/output ≤1 MiB, timeout ≤30 s. API v2: `from hindsight import api as vg`; read vg.selection/analysis/debugger and use capability-gated vg.navigate(), breakpoint_set/remove(), debugger_continue/interrupt/step(), note_append(), vg.emit().")
                        .font(.caption).foregroundStyle(.secondary)
                    TextEditor(text: $model.pythonSource).font(.system(.caption, design: .monospaced)).frame(minHeight: 260).border(.separator)
                }.padding(8)
            }
            VGAdaptiveTwoPane {
                GroupBox("stdout") { monospaced(model.pythonOutput.isEmpty ? "Script output appears here." : model.pythonOutput) }
            } secondary: {
                GroupBox("stderr / result") { VStack(alignment: .leading, spacing: 8) { monospaced(model.pythonError.isEmpty ? "No stderr." : model.pythonError); if !model.pythonResult.isEmpty { Divider(); Text("result").font(.caption).fontWeight(.semibold); monospaced(model.pythonResult) } }.padding(4) }
            }.frame(minHeight: 220)
        }
    }

    @ViewBuilder private func pluginsWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("HindSight Extensions • API v1/v2") {
                VStack(alignment: .leading, spacing: 10) {
                    VGFlowLayout {
                        Button("Scan", action: model.refreshPlugins)
                        Button("Install Folder…", action: model.installPlugin)
                        Button("Run Selected", action: model.runSelectedPlugin).disabled(model.selectedPluginID.isEmpty || model.busy)
                    }
                    Text(model.pluginStatus).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    Text("Plugin roots: ~/Library/Application Support/HindSight/Plugins and <project>/Plugins. Legacy VectorGlass plugins remain discoverable. A plugin is a folder with plugin.json and a Python entry file. Manifests declare id, name, version, api_version, entry, capabilities, optional event hooks, commands, and panels. Project-local plugins override same-ID user plugins.").font(.caption).foregroundStyle(.secondary)
                    if model.plugins.isEmpty { Text("No plugins discovered. Scan, install a plugin folder, or add one to a plugin root.").foregroundStyle(.secondary) }
                    else {
                        Picker("Plugin", selection: $model.selectedPluginID) { ForEach(model.plugins) { p in Text("\(p.name) \(p.version)").tag(p.id) } }.pickerStyle(.menu).frame(maxWidth: 420)
                        if let p=model.plugins.first(where: {$0.id == model.selectedPluginID}) {
                            VGFlowLayout {
                                Label(p.enabled ? "Enabled" : "Disabled", systemImage: p.enabled ? "checkmark.circle.fill" : "pause.circle")
                                Button(p.enabled ? "Disable" : "Enable") { model.setSelectedPluginEnabled(!p.enabled) }
                            }.font(.caption)
                            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 5) {
                                GridRow { Text("ID").foregroundStyle(.secondary); Text(p.id).font(.system(.caption,design:.monospaced)) }
                                GridRow { Text("Source").foregroundStyle(.secondary); Text(p.source) }
                                GridRow { Text("Entry").foregroundStyle(.secondary); Text(p.entry).font(.system(.caption,design:.monospaced)) }
                                GridRow { Text("Capabilities").foregroundStyle(.secondary); Text(p.capabilities.isEmpty ? "none" : p.capabilities.joined(separator: ", ")).font(.system(.caption,design:.monospaced)) }
                                GridRow { Text("Events").foregroundStyle(.secondary); Text(p.events.isEmpty ? "none" : p.events.joined(separator: ", ")).font(.system(.caption,design:.monospaced)) }
                                GridRow { Text("Package").foregroundStyle(.secondary); Text(p.package_format) }
                                GridRow { Text("Settings").foregroundStyle(.secondary); Text(p.settings.isEmpty ? "none" : p.settings.map { "\($0.id)=\(p.settings_values[$0.id] ?? $0.default_value ?? "null")" }.joined(separator: ", ")).font(.system(.caption,design:.monospaced)) }
                            }.font(.caption)
                            if !p.commands.isEmpty {
                                VStack(alignment: .leading, spacing: 6) {
                                    Text("Commands").font(.caption).fontWeight(.semibold)
                                    VGFlowLayout { ForEach(p.commands) { command in Button(command.title) { model.runPluginCommand(command) }.help(command.shortcut ?? "") } }
                                }
                            }
                            if !p.panels.isEmpty {
                                VStack(alignment: .leading, spacing: 7) {
                                    Text("Plugin Panels").font(.caption).fontWeight(.semibold)
                                    ForEach(p.panels) { panel in
                                        VStack(alignment: .leading, spacing: 3) {
                                            Button(panel.title) { model.runPluginPanel(panel) }
                                            if let description = panel.description { Text(description).font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                                        }
                                    }
                                }
                            }
                            if !p.analysis_passes.isEmpty {
                                VStack(alignment: .leading, spacing: 7) {
                                    Text("Custom Analysis Passes").font(.caption).fontWeight(.semibold)
                                    ForEach(p.analysis_passes) { pass in
                                        VStack(alignment: .leading, spacing: 3) {
                                            Button(pass.title) { model.runPluginAnalysisPass(pass) }
                                            if let description = pass.description { Text(description).font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                                        }
                                    }
                                }
                            }
                            if let filter=p.debugger_filter { Text("Debugger filter • signals=\(filter.signals?.map(String.init).joined(separator: ",") ?? "any") • reasons=\(filter.reasons?.joined(separator: ",") ?? "any")").font(.caption2).foregroundStyle(.secondary) }
                        }
                    }
                }.padding(8)
            }
            VGAdaptiveTwoPane {
                GroupBox("Plugin stdout") { monospaced(model.pythonOutput.isEmpty ? "Plugin output appears here." : model.pythonOutput) }
            } secondary: {
                GroupBox("Plugin stderr / result") { VStack(alignment:.leading,spacing:8){ monospaced(model.pythonError.isEmpty ? "No stderr." : model.pythonError); if !model.pythonResult.isEmpty { Divider(); monospaced(model.pythonResult) } }.padding(4) }
            }.frame(minHeight: 240)
        }.onAppear { if model.plugins.isEmpty { model.refreshPlugins() } }
    }

    @ViewBuilder private func automationSDKWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Automation SDK • Python API v2") {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Extensions remain out-of-process. Read and mutation authority are separate manifest capabilities, and every returned action is bounded before the app applies it.").font(.caption).foregroundStyle(.secondary)
                    Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 6) {
                        GridRow { Text("Read").fontWeight(.semibold); Text("selection.read • analysis.read • debugger.read").font(.system(.caption, design: .monospaced)) }
                        GridRow { Text("Write").fontWeight(.semibold); Text("navigation.write • breakpoint.write • debugger.control • notes.write").font(.system(.caption, design: .monospaced)) }
                        GridRow { Text("Persistent").fontWeight(.semibold); Text("settings.read/write • state.read/write").font(.system(.caption, design: .monospaced)) }
                        GridRow { Text("Analysis").fontWeight(.semibold); Text("analysis.custom • manifest-defined passes").font(.system(.caption, design: .monospaced)) }
                        GridRow { Text("Hooks").fontWeight(.semibold); Text("on_analysis_complete • on_stop • on_symbol_selected").font(.system(.caption, design: .monospaced)) }
                    }.font(.caption)
                    Text("Manifest commands and panels are surfaced by the Plugins workspace. SDK templates live under Examples/Plugins and Scripts/create-plugin.py can scaffold API-v2 plugins.").font(.caption).foregroundStyle(.secondary)
                }.padding(8)
            }
            if let plugin = model.plugins.first(where: { $0.id == model.selectedPluginID }) {
                GroupBox("Selected Extension • \(plugin.name)") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("API \(plugin.api_version) • \(plugin.source) • \(plugin.enabled ? "enabled" : "disabled")").font(.caption).foregroundStyle(.secondary)
                        Text("Capabilities: \(plugin.capabilities.joined(separator: ", "))").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        Text("Hooks: \(plugin.events.joined(separator: ", "))").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        VGFlowLayout {
                            ForEach(plugin.commands) { command in Button(command.title) { model.runPluginCommand(command) } }
                            ForEach(plugin.panels) { panel in Button(panel.title) { model.runPluginPanel(panel) } }
                        }
                    }.padding(8)
                }
            } else { Text("Select a plugin in Plugins to inspect its automation surface.").foregroundStyle(.secondary) }
            Spacer()
        }.onAppear { if model.plugins.isEmpty { model.refreshPlugins() } }
    }

    @ViewBuilder private func reaAgentWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("REA Agent • Local MCP") {
                VStack(alignment: .leading, spacing: 9) {
                    Text("Run an explicit, local REA operation against one target. HindSight sends the selected path and provider directly to REA; it does not infer a broader target scope.")
                        .font(.caption).foregroundStyle(.secondary)
                    TextField("Absolute target path", text: $model.reaTargetPath)
                        .textFieldStyle(.roundedBorder)
                    HStack {
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
                    }
                    HStack {
                        Button("Probe provider") { model.probeREAAgents() }
                        if model.reaOperationActive {
                            Button("Cancel") { model.cancelREAOperation() }
                        } else {
                            Button("Run operation") { model.runREAOperation() }
                                .buttonStyle(.borderedProminent)
                                .disabled(model.reaTargetPath.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        }
                    }
                    Text(model.reaStatus)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                }.padding(8)
            }
            GroupBox("Safety boundary") {
                    Text("Operations are opt-in, local, bounded, and provider-scoped. Results are retained with a SHA-256 identity in the current case export. Live provider progress is request-scoped and bounded.")
                    .font(.caption).foregroundStyle(.secondary).padding(8)
            }
            if !model.reaProgressHistory.isEmpty {
                GroupBox("Live operation history") {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(model.reaProgressHistory.enumerated()), id: \.offset) { _, event in
                                Label(event, systemImage: "arrow.right.circle")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                            }
                        }.frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(maxHeight: 150).padding(8)
                }
            }
            if !model.reaEvidence.isEmpty {
                GroupBox("Retained REA evidence") {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(model.reaEvidence.reversed()) { evidence in
                            VStack(alignment: .leading, spacing: 2) {
                                Text("\(evidence.operation) · \(evidence.provider) · \(URL(fileURLWithPath: evidence.targetPath).lastPathComponent)")
                                    .font(.caption).fontWeight(.semibold)
                                Text("SHA-256 \(evidence.responseSHA256) · \(evidence.response.utf8.count) bytes")
                                    .font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary)
                            }
                        }
                    }.padding(8)
                }
            }
            Spacer()
        }
    }

    @ViewBuilder private func extensionsPlatformWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Extensions Platform • API v2") {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Extensions can declare dockable panels, persistent typed settings/state, custom analysis passes, debugger stop filters, commands/hotkeys, and .vgplugin directory bundles. Python remains isolated in the Coordinator service.").font(.caption).foregroundStyle(.secondary)
                    VGFlowLayout {
                        Button("Scan Extensions", action: model.refreshPlugins)
                        Button("Install Folder / .vgplugin…", action: model.installPlugin)
                    }
                    Text(model.pluginStatus).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }.padding(8)
            }
            if let plugin = model.plugins.first(where: { $0.id == model.selectedPluginID }) {
                VGAdaptiveTwoPane(breakpoint: 900) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(plugin.name).font(.title3).fontWeight(.semibold)
                        Text("\(plugin.id) • v\(plugin.version) • API \(plugin.api_version) • \(plugin.package_format)").font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                        Text("Capabilities").font(.caption).fontWeight(.semibold); Text(plugin.capabilities.joined(separator: "\n")).font(.system(.caption2, design: .monospaced)).textSelection(.enabled)
                        if !plugin.settings.isEmpty {
                            Divider(); Text("Persistent Settings").font(.caption).fontWeight(.semibold)
                            ForEach(plugin.settings) { setting in
                                VStack(alignment: .leading, spacing: 3) {
                                    ViewThatFits(in: .horizontal) {
                                        HStack(spacing: 8) {
                                            Text(setting.title).font(.caption)
                                            Text("[\(setting.type)]").font(.caption2).foregroundStyle(.secondary)
                                            Spacer(minLength: 8)
                                            TextField("JSON value", text: Binding(get: { model.pluginSettingDrafts[setting.id] ?? plugin.settings_values[setting.id] ?? setting.default_value ?? "null" }, set: { model.pluginSettingDrafts[setting.id] = $0 }))
                                                .font(.system(.caption2, design: .monospaced))
                                                .frame(minWidth: 130, idealWidth: 180, maxWidth: 240)
                                            Button("Save") { model.setPluginSetting(setting, valueJSON: model.pluginSettingDrafts[setting.id] ?? plugin.settings_values[setting.id] ?? setting.default_value ?? "null") }
                                        }
                                        VStack(alignment: .leading, spacing: 6) {
                                            HStack { Text(setting.title).font(.caption); Text("[\(setting.type)]").font(.caption2).foregroundStyle(.secondary) }
                                            TextField("JSON value", text: Binding(get: { model.pluginSettingDrafts[setting.id] ?? plugin.settings_values[setting.id] ?? setting.default_value ?? "null" }, set: { model.pluginSettingDrafts[setting.id] = $0 }))
                                                .font(.system(.caption2, design: .monospaced))
                                            Button("Save") { model.setPluginSetting(setting, valueJSON: model.pluginSettingDrafts[setting.id] ?? plugin.settings_values[setting.id] ?? setting.default_value ?? "null") }
                                        }
                                    }
                                    if let d=setting.description { Text(d).font(.caption2).foregroundStyle(.secondary) }
                                }
                            }
                        }
                    }.padding(8)
                } secondary: {
                    VStack(alignment: .leading, spacing: 10) {
                        if !plugin.commands.isEmpty { GroupBox("Commands & Hotkeys") { VGFlowLayout { ForEach(plugin.commands) { command in pluginCommandButton(plugin, command) } }.padding(6) } }
                        if !plugin.panels.isEmpty { GroupBox("Dockable Views") { VStack(alignment: .leading, spacing: 7) { ForEach(plugin.panels) { panel in VStack(alignment: .leading, spacing: 3) { VGFlowLayout { Button(panel.title) { model.runPluginPanel(panel) }; Text("dock: \(panel.dock ?? "inspector")").font(.caption2).foregroundStyle(.secondary) }; if let description = panel.description { Text(description).font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) } } } }.padding(6) } }
                        if !plugin.analysis_passes.isEmpty { GroupBox("Custom Analysis Passes") { VStack(alignment: .leading, spacing: 7) { ForEach(plugin.analysis_passes) { pass in VStack(alignment: .leading, spacing: 3) { Button(pass.title) { model.runPluginAnalysisPass(pass) }; Text(pass.description ?? "").font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) } } }.padding(6) } }
                        if let filter=plugin.debugger_filter { GroupBox("Debugger Event Filter") { Text("Signals: \(filter.signals?.map(String.init).joined(separator: ", ") ?? "any")\nReasons: \(filter.reasons?.joined(separator: ", ") ?? "any")").font(.system(.caption, design: .monospaced)).padding(6) } }
                        Spacer()
                    }.padding(8)
                }
                .frame(minHeight: 360)
            } else { Text("Select an extension in Plugins to inspect its platform surface.").foregroundStyle(.secondary) }
        }.onAppear { if model.plugins.isEmpty { model.refreshPlugins() } }
    }

    @ViewBuilder private func debuggerWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Native Debug Server") {
                VStack(alignment: .leading, spacing: 10) {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            TextField("PID", text: $model.debugPIDText)
                                .frame(minWidth: 90, idealWidth: 120, maxWidth: 140)
                            Button("Attach", action: model.debugAttachPID).disabled(model.debugAttached)
                            Spacer(minLength: 0)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("PID", text: $model.debugPIDText)
                            Button("Attach", action: model.debugAttachPID).disabled(model.debugAttached)
                        }
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            TextField("Executable path", text: $model.debugLaunchPath)
                            Button("Choose…", action: model.chooseDebugExecutable)
                            Button("Launch", action: model.debugLaunch).disabled(model.debugAttached || model.debugLaunchPath.isEmpty)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("Executable path", text: $model.debugLaunchPath)
                            VGFlowLayout {
                                Button("Choose…", action: model.chooseDebugExecutable)
                                Button("Launch", action: model.debugLaunch).disabled(model.debugAttached || model.debugLaunchPath.isEmpty)
                            }
                        }
                    }
                    Text("Native Mach/ptrace debugger hosted by the Coordinator XPC service. Dynamic reads are bounded and macOS debugging permissions are respected.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(8)
            }

            GroupBox("Execution Control") {
                VStack(alignment: .leading, spacing: 10) {
                    VGFlowLayout {
                        Button("Continue", action: model.debugContinue).disabled(!model.debugAttached)
                        Button("Interrupt", action: model.debugInterrupt).disabled(!model.debugAttached)
                        Button("Step", action: model.debugStep).disabled(!model.debugAttached)
                        Button("Refresh", action: model.debugRefreshSnapshot).disabled(!model.debugAttached)
                        Button("Detach", action: model.debugDetach).disabled(!model.debugAttached)
                        Toggle("Follow PC", isOn: $model.debugFollowPC).toggleStyle(.button)
                    }
                    VGFlowLayout {
                        Button("PC → Code", action: model.debugOpenPCInAnnotatedCode).disabled(model.debugSnapshot == nil || model.selected == nil)
                        Button("PC → CFG", action: model.debugOpenPCInCFG).disabled(model.debugSnapshot == nil || model.selected == nil)
                        Button("PC → Correlation", action: model.debugOpenPCInRuntimeCorrelation).disabled(model.debugSnapshot == nil || model.selected == nil)
                        Button("PC → Trace", action: model.debugOpenPCInTraceExplorer).disabled(model.debugSnapshot == nil || model.selected == nil)
                    }
                    Text(model.debugEvent)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let snapshot = model.debugSnapshot {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 8)], alignment: .leading, spacing: 7) {
                            Label("PID \(snapshot.pid)", systemImage: "cpu")
                            Label(snapshot.stopped ? "Stopped" : "Running", systemImage: snapshot.stopped ? "pause.circle" : "play.circle")
                            Label("\(snapshot.thread_count) threads", systemImage: "point.3.filled.connected.trianglepath.dotted")
                            Text("TID \(snapshot.selected_thread_id ?? "?")").font(.system(.caption, design: .monospaced))
                            Text("PC \(snapshot.pc)").font(.system(.caption, design: .monospaced))
                        }
                        .font(.caption)
                    }
                }
                .padding(8)
            }

            VGAdaptiveTwoPane {
                GroupBox("Threads / Registers") {
                    VStack(alignment: .leading, spacing: 8) {
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 8) {
                                TextField("Thread ID (hex)", text: $model.debugThreadID)
                                Button("Select", action: model.debugSelectThread).disabled(!model.debugAttached)
                                Button("Refresh") { model.debugQuery("threads") { $0.debugThreads = $1 } }
                                    .disabled(!model.debugAttached)
                            }
                            VStack(alignment: .leading, spacing: 8) {
                                TextField("Thread ID (hex)", text: $model.debugThreadID)
                                VGFlowLayout {
                                    Button("Select", action: model.debugSelectThread).disabled(!model.debugAttached)
                                    Button("Refresh") { model.debugQuery("threads") { $0.debugThreads = $1 } }
                                        .disabled(!model.debugAttached)
                                }
                            }
                        }
                        monospaced(model.debugThreads.isEmpty ? "Interrupt the target to enumerate threads." : model.debugThreads)
                            .frame(minHeight: 100)
                        if let snapshot = model.debugSnapshot {
                            ScrollView {
                                LazyVGrid(columns: [GridItem(.adaptive(minimum: 160), alignment: .leading)], alignment: .leading, spacing: 6) {
                                    Text("pc  \(snapshot.pc)")
                                    Text("sp  \(snapshot.sp)")
                                    Text("fp  \(snapshot.fp)")
                                    Text("lr  \(snapshot.lr)")
                                    ForEach(snapshot.registers.keys.sorted(), id: \.self) { key in
                                        Text("\(key)  \(snapshot.registers[key] ?? "")")
                                    }
                                }
                                .font(.system(.caption, design: .monospaced))
                                .textSelection(.enabled)
                                .padding(8)
                            }
                        }
                    }
                    .padding(8)
                }
            } secondary: {
                GroupBox("Live Code / Backtrace") {
                    VStack(alignment: .leading, spacing: 8) {
                        VGFlowLayout {
                            Button("Refresh live code") { model.debugQuery("disassembly") { $0.debugDisassembly = $1 } }
                                .disabled(!model.debugAttached)
                            Button("Refresh backtrace") { model.debugQuery("backtrace") { $0.debugBacktrace = $1 } }
                                .disabled(!model.debugAttached)
                        }
                        monospaced(model.debugDisassembly.isEmpty ? "Live ARM64 disassembly follows the selected thread PC." : model.debugDisassembly)
                            .frame(minHeight: 180)
                        monospaced(model.debugBacktrace.isEmpty ? "Frame-pointer backtrace appears here." : model.debugBacktrace)
                            .frame(minHeight: 130)
                    }
                    .padding(8)
                }
            }
            .frame(minHeight: 410)

            VGAdaptiveTwoPane {
                GroupBox("Memory / Breakpoints") {
                    VStack(alignment: .leading, spacing: 10) {
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 8) {
                                TextField("0x address", text: $model.debugAddress)
                                Button("Use PC") { model.debugAddress = model.debugSnapshot?.pc ?? model.addressText }
                            }
                            VStack(alignment: .leading, spacing: 8) {
                                TextField("0x address", text: $model.debugAddress)
                                Button("Use PC") { model.debugAddress = model.debugSnapshot?.pc ?? model.addressText }
                            }
                        }
                        VGFlowLayout {
                            Stepper("Read \(model.debugMemoryLength) bytes", value: $model.debugMemoryLength, in: 16...4096, step: 16)
                            Button("◀ Prev", action: { model.debugMoveMemory(-1) }).disabled(!model.debugAttached)
                            Button("Read", action: model.debugReadMemory).disabled(!model.debugAttached)
                            Button("Next ▶", action: { model.debugMoveMemory(1) }).disabled(!model.debugAttached)
                            Button("Set BRK", action: model.debugSetBreakpoint).disabled(!model.debugAttached)
                            Button("Remove BRK", action: model.debugRemoveBreakpoint).disabled(!model.debugAttached)
                        }
                        monospaced(model.debugMemory.isEmpty ? "Read up to 4096 bytes from target memory." : model.debugMemory)
                    }
                    .padding(8)
                }
            } secondary: {
                GroupBox("Process Map / Images") {
                    VStack(alignment: .leading, spacing: 8) {
                        VGFlowLayout {
                            Button("Memory Map", action: model.debugRefreshMemoryMap).disabled(!model.debugAttached)
                            Button("Loaded Images", action: model.debugRefreshModules).disabled(!model.debugAttached)
                        }
                        monospaced(model.debugMemoryMap.isEmpty ? (model.debugModules.isEmpty ? "Query VM regions or loaded images." : model.debugModules) : model.debugMemoryMap)
                    }
                    .padding(8)
                }
            }
            .frame(minHeight: 260)
        }
    }

    @ViewBuilder private func targetsWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 14) {
            GroupBox("Unified Target Model • v0.2") {
                VStack(alignment: .leading, spacing: 10) {
                    VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8) {
                        Button("Refresh Targets", action: model.refreshUnifiedTargets)
                        Button("Open Primary View", action: model.openSelectedTargetPrimaryView)
                            .disabled(model.selectedUnifiedTarget == nil)
                        Button("Copy Session JSON", action: model.copyTargetSessionJSON)
                            .disabled(model.selectedUnifiedTarget == nil)
                        Button("Export Session…", action: model.exportTargetSessionJSON)
                            .disabled(model.selectedUnifiedTarget == nil)
                    }
                    Text(model.targetStatus)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("Static artifacts, native processes, deterministic VectorEmu machines, and Hypervisor guests share one target identity. Operations are exposed only when the selected backend declares the required capability.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(8)
            }

            VGAdaptiveTwoPane(breakpoint: 980) {
                GroupBox("Targets") {
                    if model.unifiedTargets.isEmpty {
                        ContentUnavailableView(
                            "No targets",
                            systemImage: "scope",
                            description: Text("Load a binary, attach a process, configure VectorEmu, or start a Hypervisor guest.")
                        )
                        .frame(minHeight: 260)
                    } else {
                        LazyVStack(spacing: 7) {
                            ForEach(model.unifiedTargets) { target in
                                Button { model.chooseUnifiedTarget(target.id) } label: {
                                    HStack(alignment: .top, spacing: 10) {
                                        Image(systemName: targetIcon(target.kind))
                                            .font(.system(size: 15, weight: .semibold))
                                            .symbolRenderingMode(.hierarchical)
                                            .frame(width: 20)
                                        VStack(alignment: .leading, spacing: 4) {
                                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                                Text(target.name)
                                                    .fontWeight(.semibold)
                                                    .lineLimit(2)
                                                Spacer(minLength: 8)
                                                Text(target.state)
                                                    .font(.caption2)
                                                    .foregroundStyle(.secondary)
                                            }
                                            Text("\(target.kind.rawValue) • \(target.architecture) • \(target.backend)")
                                                .font(.caption)
                                                .foregroundStyle(.secondary)
                                                .fixedSize(horizontal: false, vertical: true)
                                            Text(target.id)
                                                .font(.system(.caption2, design: .monospaced))
                                                .foregroundStyle(.tertiary)
                                                .lineLimit(2)
                                                .textSelection(.enabled)
                                        }
                                    }
                                    .padding(9)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .contentShape(Rectangle())
                                    .background(
                                        target.id == model.selectedUnifiedTarget?.id
                                            ? Color.accentColor.opacity(0.14)
                                            : Color.primary.opacity(0.025),
                                        in: RoundedRectangle(cornerRadius: 9, style: .continuous)
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(6)
                    }
                }
            } secondary: {
                if let target = model.selectedUnifiedTarget {
                    VStack(alignment: .leading, spacing: 12) {
                        GroupBox("Capabilities") {
                            VStack(alignment: .leading, spacing: 8) {
                                VGFlowLayout(horizontalSpacing: 6, verticalSpacing: 6) {
                                    ForEach(target.capabilities.map(\.rawValue).sorted(), id: \.self) { capability in
                                        VGTagPill(text: capability)
                                    }
                                }
                                Text("Capability discovery is authoritative. Missing capabilities are treated as unavailable rather than inferred from target kind.")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            .padding(6)
                        }

                        GroupBox("Address Spaces") {
                            if target.addressSpaces.isEmpty {
                                Text("No address spaces reported.")
                                    .foregroundStyle(.secondary)
                                    .padding(6)
                            } else {
                                LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 8)], alignment: .leading, spacing: 8) {
                                    ForEach(target.addressSpaces) { space in
                                        VStack(alignment: .leading, spacing: 8) {
                                            HStack(alignment: .firstTextBaseline) {
                                                Text(space.name).fontWeight(.semibold)
                                                Spacer(minLength: 6)
                                                Text(space.kind)
                                                    .font(.system(.caption2, design: .monospaced))
                                                    .foregroundStyle(.secondary)
                                            }
                                            VGKeyValueField(label: "Base", value: space.base ?? "—", monospaced: true)
                                            VGKeyValueField(label: "Size", value: space.size ?? "—", monospaced: true)
                                            VGKeyValueField(label: "Evidence", value: space.source)
                                        }
                                        .padding(9)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                                    }
                                }
                                .padding(6)
                            }
                        }

                        GroupBox("Session Identity") {
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 210), spacing: 10)], alignment: .leading, spacing: 10) {
                                VGKeyValueField(label: "Schema", value: String(target.session.schemaVersion), monospaced: true)
                                VGKeyValueField(label: "Product", value: target.session.productVersion, monospaced: true)
                                VGKeyValueField(label: "Target", value: target.session.targetFingerprint, monospaced: true)
                                VGKeyValueField(label: "Engine", value: target.session.engineFingerprint, monospaced: true)
                                VGKeyValueField(label: "Model", value: target.session.modelFingerprint, monospaced: true)
                                VGKeyValueField(label: "Source", value: target.session.sourceFingerprint ?? "not content-hashed by this backend", monospaced: true)
                            }
                            .padding(6)
                        }
                    }
                } else {
                    ContentUnavailableView(
                        "No target selected",
                        systemImage: "scope",
                        description: Text("Select or create a target to inspect its capabilities, address spaces, and session identity.")
                    )
                    .frame(minHeight: 260)
                }
            }
            .frame(minHeight: 390)

            GroupBox("Evidence-backed Address Pivot") {
                VStack(alignment: .leading, spacing: 10) {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            TextField("0x address", text: $model.targetPivotInput)
                                .textFieldStyle(.roundedBorder)
                                .frame(minWidth: 180, idealWidth: 280, maxWidth: 360)
                            Button("Resolve", action: model.pivotUnifiedAddress)
                                .keyboardShortcut(.return, modifiers: [])
                            if let target = model.selectedUnifiedTarget {
                                Text("in \(target.name)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                            Spacer(minLength: 0)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("0x address", text: $model.targetPivotInput)
                                .textFieldStyle(.roundedBorder)
                            VGFlowLayout {
                                Button("Resolve", action: model.pivotUnifiedAddress)
                                    .keyboardShortcut(.return, modifiers: [])
                                if let target = model.selectedUnifiedTarget {
                                    Text("Target: \(target.name)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }

                    if let pivot = model.targetPivot {
                        Text("\(pivot.confidence.uppercased()) • \(pivot.explanation)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 230), spacing: 8)], alignment: .leading, spacing: 8) {
                            ForEach(pivot.translations) { row in
                                VStack(alignment: .leading, spacing: 7) {
                                    HStack(alignment: .firstTextBaseline) {
                                        Text(row.space)
                                            .font(.system(.caption, design: .monospaced))
                                            .fontWeight(.semibold)
                                        Spacer(minLength: 6)
                                        Text(row.address)
                                            .font(.system(.caption, design: .monospaced))
                                            .textSelection(.enabled)
                                    }
                                    VGKeyValueField(label: "Mapping", value: row.detail)
                                    VGKeyValueField(label: "Evidence", value: row.evidence)
                                }
                                .padding(9)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                            }
                        }
                    } else {
                        Text("Resolve an address across file offsets, preferred VM addresses, runtime slides, emulated bus regions, or guest physical regions only where loaded evidence proves the relationship.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .padding(8)
            }
        }
        .onAppear { model.refreshUnifiedTargets() }
    }

    private func targetIcon(_ kind: VGTargetKind) -> String {
        switch kind {
        case .staticArtifact: return "doc"
        case .nativeProcess: return "ladybug"
        case .emulatedMachine: return "cpu"
        case .hypervisorGuest: return "memorychip"
        }
    }

    @ViewBuilder private func extensionManagerWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 14) {
            GroupBox("Extension Manager • Trust & Annotations") {
                VStack(alignment: .leading, spacing: 9) {
                    VGFlowLayout {
                        Button("Scan", action: model.refreshPlugins)
                        Button("Install…", action: model.installPlugin)
                    }
                    Text(model.pluginStatus)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("Execution requires trust. Verified Ed25519 .vgplugin packages are trusted automatically; unsigned packages require an explicit local trust decision; invalid signatures remain blocked. Dependencies are checked again when a plugin runs.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Picker("Extension", selection: $model.selectedPluginID) {
                        ForEach(model.plugins) { plugin in
                            Text("\(plugin.name) \(plugin.version)").tag(plugin.id)
                        }
                    }
                    .pickerStyle(.menu)
                    .frame(maxWidth: 460)
                }
                .padding(6)
            }

            if let plugin = model.plugins.first(where: { $0.id == model.selectedPluginID }) {
                VGAdaptiveTwoPane(breakpoint: 860) {
                    GroupBox("Trust & Permissions") {
                        VStack(alignment: .leading, spacing: 8) {
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 190), spacing: 9)], alignment: .leading, spacing: 9) {
                                VGKeyValueField(label: "Signature", value: plugin.signature_status, monospaced: true)
                                VGKeyValueField(label: "Local policy", value: plugin.policy_status, monospaced: true)
                                VGKeyValueField(label: "Trusted", value: plugin.trusted ? "yes" : "no", monospaced: true)
                                VGKeyValueField(label: "Package", value: "\(plugin.package_format) • \(plugin.source)")
                                VGKeyValueField(label: "Capabilities", value: plugin.capabilities.joined(separator: ", "))
                            }
                            VGFlowLayout {
                                Button(plugin.trusted ? "Revoke Trust" : "Trust Locally") {
                                    model.setSelectedPluginTrusted(!plugin.trusted)
                                }
                                .disabled(plugin.signature_status == "invalid")
                                Button(plugin.enabled ? "Disable" : "Enable") {
                                    model.setSelectedPluginEnabled(!plugin.enabled)
                                }
                                .disabled(plugin.policy_status != "allowed" && !plugin.enabled)
                            }
                        }
                        .padding(6)
                    }
                } secondary: {
                    GroupBox("Dependencies") {
                        VStack(alignment: .leading, spacing: 6) {
                            if plugin.dependencies.isEmpty {
                                Text("No declared dependencies.").foregroundStyle(.secondary)
                            } else {
                                ForEach(plugin.dependencies) { dependency in
                                    Text("\(dependency.plugin_id) >= \(dependency.minimum_version ?? "any")")
                                        .font(.system(.caption, design: .monospaced))
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                            }
                        }
                        .padding(6)
                    }
                }
                .frame(minHeight: 180)

                GroupBox("Extension-owned Annotations") {
                    VStack(alignment: .leading, spacing: 7) {
                        Text("\(plugin.annotations.count) persisted annotation(s) • custom-data keys: \(plugin.custom_data_keys.joined(separator: ", "))")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                        ForEach(plugin.annotations.suffix(24)) { annotation in
                            VStack(alignment: .leading, spacing: 3) {
                                Text("[\(annotation.category)] \(annotation.address ?? "no address")")
                                    .font(.system(.caption2, design: .monospaced))
                                Text(annotation.text)
                                    .font(.caption)
                                    .textSelection(.enabled)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            .padding(.vertical, 2)
                        }
                    }
                    .padding(6)
                }
            } else {
                ContentUnavailableView(
                    "No extension selected",
                    systemImage: "puzzlepiece.extension",
                    description: Text("Select an extension to review trust, permissions, dependencies, and annotations.")
                )
                .frame(minHeight: 220)
            }
        }
        .onAppear { if model.plugins.isEmpty { model.refreshPlugins() } }
    }
    // Keep the workspace switch behind a concrete return type so callers do
    // not expand every tool's view tree during Swift SIL generation.
    private func workspace(_ record: ProjectRecord) -> AnyView {
        AnyView(Group {
            switch model.workspace {
            case "Targets":
                targetsWorkspace()
            case "Binary Diff":
                VGBinaryDiffWorkspace(
                    records: model.project?.records ?? [],
                    baselineID: $model.diffBaselineID,
                    candidateID: $model.diffCandidateID,
                    reports: model.analysisReportsByRecordID,
                    status: model.diffStatus,
                    busy: model.busy,
                    onAnalyzePair: model.prepareBinaryDiff,
                    onSelectAddress: { value in
                        model.addressText = value
                        model.analysisSearch = value
                        model.selectEvidence("binary diff address=\(value)")
                    }
                )
                .frame(minHeight: 650)
            case "Binary Workspace":
                if let analysis = model.currentAnalysis {
                    VGBinaryWorkspace(
                        functionsText: analysis.functions,
                        dyldImageIdentity: model.dyldAnalysisImageByRecordID[record.id],
                        symbolResolverText: analysis.symbol_resolver,
                        objcClassesText: analysis.objc_classes,
                        disassemblyText: analysis.annotated_disassembly,
                        irText: analysis.ir,
                        dataflowText: analysis.dataflow,
                        semanticPreviewText: analysis.semantic_preview,
                        xrefsText: analysis.xrefs,
                        callGraphText: analysis.call_graph,
                        selectedAddress: model.addressText,
                        debugPC: model.debugSnapshot?.pc,
                        onSelectAddress: { value in
                            model.addressText = value
                            model.analysisSearch = value
                            model.selectEvidence("binary workspace address=\(value)")
                        }
                    )
                    .frame(minHeight: 640)
                } else {
                    GroupBox("Binary Workspace") {
                        VStack(alignment: .leading, spacing: 12) {
                            analysisToolbar(record)
                            ContentUnavailableView(
                                "No analysis yet",
                                systemImage: "rectangle.3.group",
                                description: Text("Run Analyze binary to populate synchronized functions, disassembly, lifted IR, references, and instruction data.")
                            )
                            .frame(minHeight: 460)
                        }
                        .padding(8)
                    }
                }
            case "Investigation Ledger":
                if let ledger = model.investigationLedger {
                    VGInvestigationLedgerWorkspace(
                        ledger: ledger,
                        status: "\(model.ledgerStatus)\n\(model.caseIntegrityStatus)",
                        selectedEntryID: model.selectedLedgerEntryID,
                        onSelectEntry: { model.selectedLedgerEntryID = $0 },
                        onOpenContext: model.openLedgerContext,
                        onCreateHypothesis: model.createHypothesis,
                        onSetHypothesisStatus: model.setHypothesisStatus,
                        onAddBookmark: model.addLedgerBookmark,
                        onCaptureSnapshot: model.captureInvestigationSnapshot,
                        onRestoreSnapshot: model.restoreInvestigationSnapshot,
                        onExportCase: model.exportInvestigationCase,
                        onImportCase: model.importInvestigationCase,
                        onExportMarkdown: model.exportInvestigationMarkdown,
                        onExportPDF: model.exportInvestigationPDF,
                        onExportJSON: model.exportInvestigationJSON
                    )
                    .frame(minHeight: 650)
                } else {
                    ContentUnavailableView(
                        "Ledger unavailable",
                        systemImage: "book.pages",
                        description: Text("Open a HindSight project to create or resume its persistent investigation ledger.")
                    )
                    .frame(minHeight: 520)
                }
            case "Evidence Graph":
                if let analysis = model.currentAnalysis {
                    let functions = VGGraphParser.functions(analysis.functions)
                    let runtimeFacts = model.runtimeSamples.map { sample -> VGEvidenceRuntimeFact in
                        let correlation = VGRuntimeCorrelationEngine.correlate(
                            sample,
                            binaryPath: record.path,
                            modulesText: model.debugModules,
                            functions: functions
                        )
                        return VGEvidenceRuntimeFact(
                            id: sample.id,
                            timestamp: sample.timestamp,
                            runtimePC: sample.pc,
                            preferredPC: correlation.preferredPC,
                            functionName: correlation.functionName,
                            imageName: correlation.modulePath.map { URL(fileURLWithPath: $0).lastPathComponent },
                            reason: sample.reason,
                            confidence: correlation.confidence
                        )
                    }
                    let memoryFacts = model.runtimeMemoryObservations.map { observation in
                        VGEvidenceMemoryFact(
                            id: observation.id,
                            timestamp: observation.timestamp,
                            address: observation.address,
                            preview: observation.preview
                        )
                    }
                    VGEvidenceGraphWorkspace(
                        graph: VGEvidenceGraphBuilder.build(
                            functionsText: analysis.functions,
                            xrefsText: analysis.xrefs,
                            symbolProvenanceText: analysis.symbol_provenance,
                            runtime: runtimeFacts,
                            memory: memoryFacts,
                            note: model.noteText,
                            assertions: (model.investigationLedger?.hypotheses ?? []).map { hypothesis in
                                VGEvidenceAnalystAssertion(
                                    id: hypothesis.id,
                                    title: hypothesis.title,
                                    detail: hypothesis.detail,
                                    status: hypothesis.status.title,
                                    address: hypothesis.linkedAddresses.first.flatMap(VGTargetUtilities.parseHex)
                                )
                            },
                            selectedAddress: VGTargetUtilities.parseHex(model.addressText),
                            reaEvidence: model.reaEvidence,
                            imageName: URL(fileURLWithPath: record.path).lastPathComponent
                        ),
                        onSelectAddress: { value in
                            model.addressText = value
                            model.analysisSearch = value
                            model.cfgFocus = value
                            model.selectEvidence("evidence graph address=\(value)")
                        },
                        onOpenWorkspace: { model.workspace = $0 }
                    )
                    .frame(minHeight: 650)
                } else {
                    ContentUnavailableView(
                        "Analyze the binary first",
                        systemImage: "point.3.connected.trianglepath.dotted",
                        description: Text("Evidence Graph joins bounded static analysis with captured runtime observations and analyst notes while preserving provenance.")
                    )
                    .frame(minHeight: 520)
                }
            case "Trace Explorer":
                if let analysis = model.currentAnalysis {
                    VGTraceExplorerWorkspace(
                        binaryPath: record.path,
                        functionsText: analysis.functions,
                        annotatedDisassembly: analysis.annotated_disassembly,
                        modulesText: model.debugModules,
                        samples: model.runtimeSamples,
                        memoryObservations: model.runtimeMemoryObservations,
                        attached: model.debugAttached,
                        onSelectStaticAddress: { value in
                            model.addressText = value
                            model.analysisSearch = value
                            model.cfgFocus = value
                            model.selectEvidence("trace explorer static=\(value)")
                        },
                        onStep: model.debugStep,
                        onContinue: model.debugContinue,
                        onInterrupt: model.debugInterrupt,
                        onOpenCorrelation: { model.workspace = "Runtime Correlation" }
                    )
                    .frame(minHeight: 650)
                } else {
                    ContentUnavailableView(
                        "Analyze the binary first",
                        systemImage: "point.topleft.down.to.point.bottomright.curvepath",
                        description: Text("Trace Explorer correlates bounded debugger observations with the analyzed binary; run Analyze binary, then attach or launch a target.")
                    )
                    .frame(minHeight: 520)
                }
            case "Runtime Correlation":
                if let analysis = model.currentAnalysis {
                    VGRuntimeCorrelationWorkspace(
                        binaryPath: record.path,
                        functionsText: analysis.functions,
                        annotatedDisassembly: analysis.annotated_disassembly,
                        modulesText: model.debugModules,
                        liveDisassembly: model.debugDisassembly,
                        samples: model.runtimeSamples,
                        memoryObservations: model.runtimeMemoryObservations,
                        selectedAddress: model.addressText,
                        attached: model.debugAttached,
                        onSelectStaticAddress: { value in
                            model.addressText = value
                            model.analysisSearch = value
                            model.cfgFocus = value
                            model.selectEvidence("runtime correlation static=\(value)")
                        },
                        onStep: model.debugStep,
                        onContinue: model.debugContinue,
                        onInterrupt: model.debugInterrupt,
                        onReadMemory: { address in
                            model.debugAddress = address
                            model.debugReadMemory()
                        },
                        onRefreshModules: model.debugRefreshModules
                    )
                    .frame(minHeight: 650)
                    .onAppear {
                        if model.debugAttached && model.debugModules.isEmpty { model.debugRefreshModules() }
                    }
                } else {
                    GroupBox("Runtime Correlation") {
                        VStack(alignment: .leading, spacing: 12) {
                            analysisToolbar(record)
                            ContentUnavailableView(
                                "Static analysis required",
                                systemImage: "point.3.filled.connected.trianglepath.dotted",
                                description: Text("Run Analyze binary first, then attach the debugger to correlate observed runtime stops with proven static addresses.")
                            )
                            .frame(minHeight: 460)
                        }.padding(8)
                    }
                }
            case "Disassembly":
                GroupBox("Disassembly") { instructionBrowser(record).padding(8) }
            case "Annotated Code":
                GroupBox("Symbol-aware disassembly") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Bounded disassembly annotated with verified __text symbols, direct branch symbol targets, and Mach-O relocation evidence. An annotation is omitted when the target cannot be proven.")
                            .font(.caption).foregroundStyle(.secondary)
                        VGRichDisassemblyView(
                            text: model.currentAnalysis?.annotated_disassembly ?? "",
                            selectedAddress: model.addressText,
                            emptyMessage: "Run Analyze binary to build annotated code.",
                            showFileOffset: false,
                            onSelectAddress: { value in
                                model.addressText = value
                                model.selectEvidence("address=\(value)")
                            }
                        )
                        .frame(minHeight: 460)
                    }.padding(8)
                }
            case "Analysis Triage":
                GroupBox("Analysis triage") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        VGAnalysisTriageView(model: model)
                        DisclosureGroup("Structural insights") {
                            Text("Rankings use bounded structural evidence; they are not measured performance results.")
                                .font(.caption).foregroundStyle(.secondary)
                            clickableEvidence(model.currentAnalysis?.analysis_insights ?? "Run Analyze binary to synthesize structural evidence.")
                                .frame(minHeight: 160)
                        }
                        DisclosureGroup("Coverage and analysis limits") {
                            monospaced(model.currentAnalysis?.analysis_quality ?? "Run Analyze binary to review code coverage, truncation, and evidence limits.")
                                .frame(minHeight: 120)
                        }
                    }.padding(8)
                }
            case "Analysis Insights":
                GroupBox("Analysis insights") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Evidence-first synthesis of structural function connectivity and validated PC-relative data references. Rankings are transparent navigation heuristics, never semantic or vulnerability scores.")
                            .font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.analysis_insights ?? "Run Analyze binary to synthesize high-value structural evidence.")
                    }.padding(8)
                }
            case "Analysis Quality":
                GroupBox("Analysis quality ledger") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Shows code admission, scan/decoder coverage, truncation, dominator budgets, and evidence classes so missing output is not mistaken for negative evidence.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.analysis_quality ?? "Run Analyze binary to measure analysis coverage and evidence quality.")
                    }.padding(8)
                }
            case "Function Intelligence":
                GroupBox("Function intelligence") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Ranks discovered functions with an explicit formula based on incoming/outgoing direct calls, non-fallthrough branches, basic blocks, and bounded size. The score is for triage only; it does not claim importance, reachability, risk, or intent.")
                            .font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.function_intelligence ?? "Run Analyze binary to build function intelligence.")
                    }.padding(8)
                }
            case "Function Neighborhoods":
                GroupBox("Function neighborhoods") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("One-hop callers and callees built from direct BL evidence plus separately labelled cross-function B tail-call candidates. External targets stay external instead of being attributed to an internal function.")
                            .font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.function_neighborhoods ?? "Run Analyze binary to build one-hop function neighborhoods.")
                    }.padding(8)
                }
            case "Tail-Call Candidates":
                GroupBox("Cross-function tail-call candidates") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Conservative direct-B transfers that cross discovered function boundaries and land exactly on another admitted function entry. They remain candidates rather than being silently promoted to calls.")
                            .font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.tail_call_candidates ?? "Run Analyze binary to recover cross-function branch candidates.")
                    }.padding(8)
                }
            case "Functions":
                GroupBox("Function discovery") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Seeds: LC_FUNCTION_STARTS, __text symbols, and a checked code-entry fallback. No speculative function boundaries are promoted as facts.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.functions ?? "Run Analyze binary to discover function boundaries.")
                    }.padding(8)
                }
            case "CFG":
                if let analysis = model.currentAnalysis {
                    VGGraphWorkspace(
                        imageName: URL(fileURLWithPath: record.path).lastPathComponent,
                        cfgText: analysis.cfg,
                        functionsText: analysis.functions,
                        disassemblyText: analysis.annotated_disassembly,
                        irText: analysis.ir,
                        dataflowText: analysis.dataflow,
                        selectedAddress: model.addressText.isEmpty ? model.cfgFocus : model.addressText,
                        debugPC: model.debugSnapshot?.pc,
                        onSelectAddress: { value in
                            model.addressText = value
                            model.cfgFocus = value
                            model.selectEvidence("cfg address=\(value)")
                        }
                    )
                    .frame(minHeight: 620)
                } else {
                    GroupBox("Control-flow graph") {
                        VStack(alignment: .leading, spacing: 12) {
                            analysisToolbar(record)
                            ContentUnavailableView(
                                "No flow graph yet",
                                systemImage: "point.3.connected.trianglepath.dotted",
                                description: Text("Run Analyze binary to build the bounded control-flow graph.")
                            )
                            .frame(minHeight: 420)
                        }
                        .padding(8)
                    }
                }
            case "Navigator":
                GroupBox("Analysis navigator") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Search the connected analysis result across functions, annotated code, xrefs, calls, strings, relocations, SSA/dominance structure, indirect branches, and recovered Apple runtime records.")
                            .font(.caption).foregroundStyle(.secondary)
                        HStack {
                            TextField("Symbol, address, string, relocation, register…", text: $model.analysisSearch).textFieldStyle(.roundedBorder)
                            if !model.analysisSearch.isEmpty { Button("Clear") { model.analysisSearch = "" } }
                        }.frame(maxWidth: 720)
                        HStack {
                            Button("Open address in Disassembly", action: model.openSearchInDisassembly).disabled(model.searchAddress() == nil || model.busy)
                            Button("Focus address in CFG", action: model.focusSearchInCFG).disabled(model.searchAddress() == nil)
                        }.font(.caption)
                        VGAdaptiveTwoPane(breakpoint: 820) {
                            VGAnalysisSearchResults(model: model).frame(minHeight: 320)
                        } secondary: {
                            GroupBox("Context inspector") {
                                monospaced(model.contextInspector()).frame(minHeight: 180)
                            }
                        }
                        .frame(minHeight: 340)
                    }.padding(8)
                }
            case "Dominators":
                GroupBox("Dominators") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Block-level dominator sets derived from the bounded CFG. Function-entry blocks seed the fixed-point calculation; unreachable blocks do not gain invented ancestry.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.dominators ?? "Run Analyze binary to compute structural dominators.")
                    }.padding(8)
                }
            case "Dominance Frontiers":
                GroupBox("Dominance frontiers") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Immediate-dominator and dominance-frontier facts derived per discovered function. These frontiers are the structural basis for principled phi placement.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.dominance_frontiers ?? "Run Analyze binary to compute dominance frontiers.")
                    }.padding(8)
                }
            case "Phi Nodes":
                GroupBox("Phi placement") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Join blocks with multiple predecessors are surfaced as phi-placement sites. Operand versions remain conservative until predecessor register states are fully merged.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.phi_nodes ?? "Run Analyze binary to identify phi-placement sites.")
                    }.padding(8)
                }
            case "IR":
                GroupBox("Structured IR") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Conservative first-stage IR: op, branch, conditional branch, terminal, or opaque. This is the substrate for later SSA and pseudocode.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.ir ?? "Run Analyze binary to generate IR.")
                        if let semantic = model.currentAnalysis?.semantic_preview, semantic.hasPrefix("AArch64 register semantics") {
                            DisclosureGroup("Instruction-derived ARM64 semantics (bounded)") {
                                Text("Machine-register statements with instruction provenance. Opaque instructions stop value flow.")
                                    .font(.caption).foregroundStyle(.secondary)
                                clickableEvidence(semantic)
                            }
                        }
                        DisclosureGroup("Structural pseudocode preview (bounded, experimental)") {
                            Text("Control-flow sketch only; unknown machine semantics are never reconstructed as C statements.").font(.caption).foregroundStyle(.secondary)
                            monospaced(vgStructuralPseudocode(model.currentAnalysis?.ir ?? "No IR available"))
                        }
                        if let pseudo = VGBinaryWorkspaceParser.reconstructedPseudocode(model.currentAnalysis?.semantic_preview) {
                            DisclosureGroup("Reconstructed pseudocode with signatures (bounded)") {
                                Text("Function headers use validated metadata or exact API declarations. Direct calls retain instruction addresses; unknown arguments and effects remain explicit.").font(.caption).foregroundStyle(.secondary)
                                monospaced(pseudo)
                            }
                        }
                    }.padding(8)
                }
            case "Pseudocode":
                GroupBox("Reconstructed pseudocode") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Select a function in Binary Workspace for its local sketch. Declarations come from validated Objective-C metadata, supported C++ mangling, or exact C/CoreFoundation API names; unresolved source details stay marked.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(VGBinaryWorkspaceParser.reconstructedPseudocode(model.currentAnalysis?.semantic_preview) ?? "Analyze binary to reconstruct bounded function sketches.")
                        if let signatures = VGBinaryWorkspaceParser.signatureEvidence(model.currentAnalysis?.call_values ?? "") {
                            DisclosureGroup("Recovered function signatures") { monospaced(signatures) }
                        }
                    }.padding(8)
                }
            case "SSA":
                GroupBox("SSA scaffolding") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Register values are versioned for modeled ARM64 operations. Unknown instruction semantics invalidate versions conservatively. Structural dominance and phi-placement sites are now computed separately so unsupported semantics are not hidden by fake SSA precision.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.ssa ?? "Run Analyze binary to build SSA scaffolding.")
                    }.padding(8)
                }
            case "SSA Rename":
                GroupBox("Block-aware SSA rename trace") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Modeled register definitions/uses receive stable versions inside the bounded analysis. Opaque instructions are explicit barriers; HindSight does not invent phi operands across unknown semantics.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.ssa_renamed ?? "Run Analyze binary to build the SSA rename trace.")
                        Text("Phi operand provenance").font(.headline)
                        monospaced(model.currentAnalysis?.phi_operands ?? "No phi operand analysis available.")
                    }.padding(8)
                }
            case "Strings":
                GroupBox("Strings & constants") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Printable literals recovered from the bounded captured image. Locations are reported as captured-file offsets unless a virtual mapping is explicitly known; no semantic type is guessed.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.strings ?? "Run Analyze binary to recover strings and constants.")
                    }.padding(8)
                }
            case "Xrefs":
                GroupBox("Cross references") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Direct code references are resolved from decoded branch immediates. Indirect references remain unresolved by design.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.xrefs ?? "Run Analyze binary to build cross references.")
                    }.padding(8)
                }
            case "Data References":
                GroupBox("Validated data references") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Resolves explicit ARM64 ADR, bounded ADRP+ADD pairs, and LDR-literal sites only when their resulting address lands inside a parsed Mach-O section. String text is attached only for validated string-bearing sections.")
                            .font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.data_references ?? "Run Analyze binary to recover PC-relative data references.")
                    }.padding(8)
                }
            case "Relocations":
                GroupBox("Relocation-aware references") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("References sourced from Mach-O __text relocation records, including relocation type, PC-relative flag, encoded width, and external symbol name when available.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.relocations ?? "Run Analyze binary to inspect text relocations.")
                    }.padding(8)
                }
            case "Jump Tables":
                GroupBox("Indirect branches / jump-table candidates") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("BR sites are surfaced as unresolved candidates. HindSight does not claim a jump table until value provenance and table bounds can be demonstrated.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.jump_tables ?? "Run Analyze binary to find indirect branch candidates.")
                    }.padding(8)
                }
            case "Call Graph":
                GroupBox("Call graph") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Direct BL call edges are assigned to the containing discovered function. Cross-function direct-B transfers are kept separately as tail-call candidates; BLR and other indirect calls remain unresolved unless stronger provenance exists.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.call_graph ?? "Run Analyze binary to build the direct call graph.")
                    }.padding(8)
                }
            case "Data Flow":
                GroupBox("Data-flow / SSA foundation") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Register-definition/use facts for decoded integer instructions. This is intentionally pre-SSA: opaque instructions form conservative barriers until their semantics are known.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.dataflow ?? "Run Analyze binary to derive bounded register data-flow facts.")
                    }.padding(8)
                }
            case "Metadata":
                GroupBox("Apple runtime metadata") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Mach-O sections associated with Objective-C and Swift runtime/reflection metadata are reported with verified section ranges. Type/class reconstruction is not claimed until individual records are validated.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.metadata ?? "Run Analyze binary to inspect Objective-C/Swift metadata sections.")
                    }.padding(8)
                }
            case "Runtime Records":
                GroupBox("Recovered Apple runtime records") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Validated strings from Objective-C class/method/type-name sections and Swift reflection strings, plus resolved ObjC selector/class/super references when their pointers land inside a verified section mapping.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.runtime_records ?? "Run Analyze binary to reconstruct bounded runtime records.")
                    }.padding(8)
                }
            case "Runtime Graph":
                GroupBox("Validated Objective-C runtime reference graph") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Selector, class, and superclass reference edges are shown only when both the pointer site and its target validate against mapped Mach-O sections.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.runtime_graph ?? "Run Analyze binary to reconstruct runtime reference edges.")
                    }.padding(8)
                }
            case "Swift Types":
                GroupBox("Swift nominal-type descriptors") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("__swift5_types relative references are validated against mapped sections; descriptor names are shown only when the descriptor's relative name target resolves to a bounded C string.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.swift_types ?? "Run Analyze binary to recover Swift nominal type descriptors.")
                    }.padding(8)
                }
            case "Selector Xrefs":
                GroupBox("Objective-C selector / message-send xrefs") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Message-send and selector references proven by symbols or Mach-O relocation records. Unproven data-flow associations are deliberately omitted.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.selector_xrefs ?? "Run Analyze binary to build selector/message-send xrefs.")
                    }.padding(8)
                }
            case "ObjC Classes":
                GroupBox("Objective-C class / method reconstruction") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Class records are promoted only when class, RO data, name, and method-list pointers validate inside mapped Mach-O sections. Encoded or unresolved pointers remain candidates.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.objc_classes ?? "Run Analyze binary to reconstruct Objective-C classes and methods.")
                    }.padding(8)
                }
            case "Swift Contexts":
                GroupBox("Swift context-descriptor graph") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Validated Swift context descriptors show kind, name, and relative parent edges. Unmapped parent targets stay explicitly unresolved.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.swift_contexts ?? "Run Analyze binary to build the Swift context graph.")
                    }.padding(8)
                }
            case "Call Values":
                GroupBox("Arguments, parameters, and casts") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Shows AArch64 argument evidence and source signatures recovered from validated Objective-C type encodings, supported C++ mangling, and exact C/CoreFoundation API declarations. Unknown types remain unresolved.")
                            .font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.call_values ?? "Run Analyze binary to detect argument, parameter, and explicit-cast evidence.")
                    }.padding(8)
                }

            case "Chained Fixups":
                GroupBox("Dyld chained fixups") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Validates LC_DYLD_CHAINED_FIXUPS container/header bounds and import/symbol table offsets. Pointer chains are not promoted until segment/page records validate.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.chained_fixups ?? "Run Analyze binary to inspect chained fixups.")
                    }.padding(8)
                }
            case "Chained Starts":
                GroupBox("Dyld chained-fixup page starts") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Validated per-segment/page chain-start topology. Multi-start overflow tables are labeled but not expanded unless fully validated.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.chained_starts ?? "Run Analyze binary to recover chained page starts.")
                    }.padding(8)
                }
            case "Swift Field Records":
                GroupBox("Swift field records") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Decodes individual Swift field records (flags, field name, mangled type) only when descriptor bounds and relative targets validate.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.swift_field_records ?? "Run Analyze binary to decode Swift field records.")
                    }.padding(8)
                }
            case "Predecessor Values":
                GroupBox("Predecessor reaching values") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Shows modeled x0–x7 definitions reaching a block from each CFG predecessor. Conflicting paths remain separate evidence instead of being collapsed into guessed values.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.predecessor_values ?? "Run Analyze binary to compute predecessor-aware reaching definitions.")
                    }.padding(8)
                }
            case "ObjC Protocols":
                GroupBox("Objective-C protocols / categories") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Shows validated pointer edges from protocol, protocol-reference, and category sections without inferring unproven runtime structure.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.objc_protocols ?? "Run Analyze binary to inspect protocol/category edges.")
                    }.padding(8)
                }
            case "Swift Fields":
                GroupBox("Swift field descriptors") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Parses bounded __swift5_fieldmd descriptors and relative type/superclass references. Unvalidated names and records remain unresolved.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.swift_fields ?? "Run Analyze binary to inspect Swift field metadata.")
                    }.padding(8)
                }
            case "Inter-block Values":
                GroupBox("Inter-block value provenance") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Reports modeled reaching definitions for x0–x7 at direct call sites, scoped to the containing CFG block. This is provenance, not runtime value prediction.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.interblock_values ?? "Run Analyze binary to derive reaching call-value provenance.")
                    }.padding(8)
                }
            case "Chained Pointers":
                GroupBox("Decoded dyld chained pointers") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Walks individually validated nodes for supported 64-bit dyld chained-pointer formats. Bind/rebase payloads and next strides are shown as evidence; unsupported formats and unproven multi-start chains stay unresolved.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.chained_pointers ?? "Run Analyze binary to decode supported chained-pointer nodes.")
                    }.padding(8)
                }
            case "Multi-Start Chains":
                GroupBox("Expanded dyld multi-start chains") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Expands bounded DYLD_CHAINED_PTR_START_MULTI overflow entries only when the complete starts-in-segment structure proves the index and terminator are in range.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.chained_multistarts ?? "Run Analyze binary to expand validated multi-start entries.")
                    }.padding(8)
                }
            case "ARM64e Pointers":
                GroupBox("ARM64e authenticated pointer fields") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Decodes ARM64e auth/bind/rebase field structure, including diversity, addrDiv, key, ordinal/target and next stride. PAC verification is not claimed; these are encoded pointer fields, not authenticated runtime values.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.arm64e_pointers ?? "Run Analyze binary to decode supported ARM64e chained-pointer fields.")
                    }.padding(8)
                }
            case "Swift Ownership":
                GroupBox("Swift field ownership") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Associates validated Swift field descriptors with their owner type reference and individual field names when relative references resolve inside mapped sections.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.swift_field_ownership ?? "Run Analyze binary to recover Swift field ownership relationships.")
                    }.padding(8)
                }
            case "Propagated Values":
                GroupBox("Iterative inter-block values") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Propagates modeled x0–x7 definition sets through CFG predecessors to a fixed point. Conflicting definitions remain sets rather than being collapsed into invented values.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.propagated_values ?? "Run Analyze binary to propagate modeled values across the CFG.")
                    }.padding(8)
                }
            case "Dylib Ordinals":
                GroupBox("Dylib ordinal map") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Maps positive dyld library ordinals to validated dylib load-command names; special ordinals are preserved symbolically.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.dylib_ordinals ?? "Run Analyze binary to resolve dylib ordinals.") }.padding(8) }
            case "Resolved Imports":
                GroupBox("Resolved dyld imports") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Joins chained-import ordinals to dylib names and symbols without guessing unresolved ordinals.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.resolved_imports ?? "Run Analyze binary to resolve imports.") }.padding(8) }
            case "Authenticated Targets":
                GroupBox("Authenticated bind targets") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Composes ARM64e bind ordinals with imported symbols and combines import/node addends conservatively.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.authenticated_bind_targets ?? "Run Analyze binary to compose authenticated targets.") }.padding(8) }
            case "Dyld Imports":
                GroupBox("Dyld chained imports") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Decodes supported uncompressed chained-import formats and preserves ordinal, library ordinal, weak-import state, addend, and symbol name.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.dyld_imports ?? "Run Analyze binary to decode dyld chained imports.")
                    }.padding(8)
                }
            case "Dyld Linking":
                GroupBox("dyld bind / rebase opcodes") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Decodes bounded LC_DYLD_INFO / LC_DYLD_INFO_ONLY rebase, regular bind, weak-bind, and lazy-bind opcode streams, including segment offsets, target addresses, dylib ordinals, addends, and weak-import state.").font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.dyld_linking ?? "Run Analyze binary to decode legacy dyld linking opcodes.")
                    }.padding(8)
                }
            case "Soft Linking":
                GroupBox("Apple soft-linking evidence") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Surfaces LC_LOAD_WEAK_DYLIB dependencies, weak imports from chained and opcode-based binds, references to SoftLinking.framework, and common runtime soft-link helpers such as dlopen/dlsym and NSClassFromString. Evidence is classified; runtime availability is not guessed.").font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.softlinking ?? "Run Analyze binary to decode soft-linking evidence.")
                    }.padding(8)
                }
            case "Symbolication":
                GroupBox("Static symbolication evidence") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Reports LC_UUID identity, LC_FUNCTION_STARTS coverage, direct nlist-backed function symbols, symbol/function ranges, and DWARF/unwind section inventory. Address-to-name results remain unslid and file/line data is never invented without matching debug information.").font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.symbolication ?? "Run Analyze binary to recover symbolication evidence.")
                    }.padding(8)
                }
            case "Symbol Resolver":
                GroupBox("Unified symbol resolver") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Ranks exact nlist code symbols above exact indirect-symbol stub/pointer names and validated function-start ranges. Imports and exports without a proven address remain name-only evidence, so the resolver does not invent addresses or source lines.").font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.symbol_resolver ?? "Run Analyze binary to build the unified symbol index.")
                    }.padding(8)
                }
            case "Apple Symbolication":
                GroupBox("Apple Symbolication framework decoding") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Identifies static references to Apple Symbolication/CoreSymbolication-style framework and API names from validated load commands, symbol tables, and bounded strings. The analyzer does not load or call private frameworks, so Intel/Apple-Silicon host behavior remains deterministic.").font(.caption).foregroundStyle(.secondary)
                        clickableEvidence(model.currentAnalysis?.apple_symbolication ?? "Run Analyze binary to inspect Apple Symbolication framework evidence.")
                    }.padding(8)
                }
            case "ARM64e Bind Symbols":
                GroupBox("ARM64e bind-symbol resolution") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Joins validated ARM64e bind ordinals to the decoded dyld chained-import table. Unmapped ordinals stay explicitly unresolved.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.arm64e_bind_symbols ?? "Run Analyze binary to resolve ARM64e bind symbols.")
                    }.padding(8)
                }
            case "Indirect Calls":
                GroupBox("Indirect-call resolution") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Finds ARM64 BLR sites and resolves them only when the modeled reaching definition has relocation-backed target evidence. Otherwise the exact unresolved provenance is retained.").font(.caption).foregroundStyle(.secondary)
                        monospaced(model.currentAnalysis?.indirect_calls ?? "Run Analyze binary to inspect indirect call targets.")
                    }.padding(8)
                }
            case "Indirect Symbols":
                GroupBox("Indirect symbol table") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Resolves LC_DYSYMTAB indirect-symbol entries through stub and pointer-section reserved indices. LOCAL/ABS entries remain explicit and unknown indices are never guessed.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.indirect_symbols ?? "Run Analyze binary to recover indirect symbols.") }.padding(8) }
            case "Pointer Slots":
                GroupBox("Lazy / non-lazy pointer slots") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Maps validated lazy and non-lazy symbol pointer slots to their indirect-symbol entries and preserves the exact slot virtual address.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.pointer_slots ?? "Run Analyze binary to recover symbol pointer slots.") }.padding(8) }
            case "Exports":
                GroupBox("Dyld export trie") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Bounded LC_DYLD_EXPORTS_TRIE traversal with re-export and stub/resolver terminal evidence. Malformed or cyclic nodes are bounded rather than followed indefinitely.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.export_trie ?? "Run Analyze binary to decode the export trie.") }.padding(8) }
            case "Symbol Provenance":
                GroupBox("Symbol provenance") {
                    VStack(alignment: .leading, spacing: 10) {
                        analysisToolbar(record)
                        Text("Joins recovered stub and pointer sites by symbol so imported linkage evidence can be followed from one semantic surface.").font(.caption).foregroundStyle(.secondary)
                        HStack(spacing: 8) {
                            Button("Pointer Slots") { model.workspace = "Pointer Slots" }
                            Button("Indirect Symbols") { model.workspace = "Indirect Symbols" }
                            Button("Resolved Imports") { model.workspace = "Resolved Imports" }
                            Button("Exports") { model.workspace = "Exports" }
                            if !model.addressText.isEmpty { Button("Open Code") { model.workspace = "Disassembly"; model.jumpToAddress() }.disabled(model.busy) }
                        }.font(.caption)
                        clickableEvidence(model.currentAnalysis?.symbol_provenance ?? "Run Analyze binary to build symbol provenance.")
                    }.padding(8)
                }
            case "Linkage Records":
                GroupBox("Joined linkage records") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Joins each symbol's recovered stubs and pointer slots with chained-import ownership and bounded export-trie presence.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.linkage_records ?? "Run Analyze binary to build joined linkage records.") }.padding(8) }
            case "Symbol Callers":
                GroupBox("Stub caller provenance") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Maps direct BL sites that target validated Mach-O stubs back to the imported symbol and containing discovered function.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.symbol_callers ?? "Run Analyze binary to recover stub callers.") }.padding(8) }
            case "GOT/Auth-GOT":
                GroupBox("GOT / authenticated GOT") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Surfaces validated __got, __auth_got, __auth_ptr, lazy and non-lazy symbol-pointer slots through LC_DYSYMTAB.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.got_auth_slots ?? "Run Analyze binary to recover GOT slots.") }.padding(8) }
            case "Import/Export Collisions":
                GroupBox("Import / export collisions") { VStack(alignment: .leading, spacing: 10) { analysisToolbar(record); Text("Reports symbol names present in both the supported chained-import table and bounded export trie; this is name-collision evidence, not a runtime-interposition claim.").font(.caption).foregroundStyle(.secondary); clickableEvidence(model.currentAnalysis?.import_export_collisions ?? "Run Analyze binary to compare imports and exports.") }.padding(8) }
            case "Selected Symbol":
                VStack(alignment: .leading, spacing: 12) {
                    analysisToolbar(record)
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 10) {
                            Text("Selected Symbol").font(.title3).fontWeight(.semibold)
                            TextField("Select a symbol-bearing row or type a symbol", text: $model.selectedSymbol)
                                .textFieldStyle(.roundedBorder)
                                .frame(minWidth: 220, idealWidth: 420, maxWidth: 520)
                            Spacer(minLength: 0)
                        }
                        VStack(alignment: .leading, spacing: 7) {
                            Text("Selected Symbol").font(.title3).fontWeight(.semibold)
                            TextField("Select a symbol-bearing row or type a symbol", text: $model.selectedSymbol)
                                .textFieldStyle(.roundedBorder)
                        }
                    }
                    VGAdaptiveThreePane(leadingTitle: "Linkage", trailingTitle: "GOT / Slots", leadingIdealWidth: 280, trailingIdealWidth: 280) {
                        GroupBox("Linkage") { monospaced(model.symbolEvidence(model.currentAnalysis?.linkage_records ?? "")) }
                    } center: {
                        GroupBox("Callers") { monospaced(model.symbolEvidence(model.currentAnalysis?.symbol_callers ?? "")) }
                    } trailing: {
                        GroupBox("GOT / Slots") { monospaced(model.symbolEvidence(model.currentAnalysis?.got_auth_slots ?? "")) }
                    }.frame(minHeight: 300)
                    VGAdaptiveTwoPane {
                        GroupBox("Relocations") { monospaced(model.symbolEvidence(model.currentAnalysis?.relocations ?? "")) }
                    } secondary: {
                        GroupBox("Import / Export") { monospaced(model.symbolEvidence((model.currentAnalysis?.resolved_imports ?? "") + "\n" + (model.currentAnalysis?.export_trie ?? "") + "\n" + (model.currentAnalysis?.import_export_collisions ?? ""))) }
                    }.frame(minHeight: 220)
                }
            case "Threads":
                GroupBox("Debug Threads") {
                    VStack(alignment: .leading, spacing: 8) {
                        ViewThatFits(in: .horizontal) {
                            HStack(spacing: 8) {
                                TextField("Thread ID", text: $model.debugThreadID)
                                Button("Select", action: model.debugSelectThread)
                                Button("Refresh") { model.debugQuery("threads") { $0.debugThreads = $1 } }
                            }
                            VStack(alignment: .leading, spacing: 8) {
                                TextField("Thread ID", text: $model.debugThreadID)
                                VGFlowLayout {
                                    Button("Select", action: model.debugSelectThread)
                                    Button("Refresh") { model.debugQuery("threads") { $0.debugThreads = $1 } }
                                }
                            }
                        }
                        monospaced(model.debugThreads.isEmpty ? "Attach and interrupt a process to enumerate threads." : model.debugThreads)
                    }
                    .padding(8)
                }
            case "Memory Map":
                GroupBox("Target Memory Map") { VStack(alignment: .leading, spacing: 8) { Button("Refresh", action: model.debugRefreshMemoryMap); monospaced(model.debugMemoryMap.isEmpty ? "Attach to query Mach VM regions." : model.debugMemoryMap) }.padding(8) }
            case "Modules":
                GroupBox("Loaded Images") { VStack(alignment: .leading, spacing: 8) { Button("Refresh", action: model.debugRefreshModules); monospaced(model.debugModules.isEmpty ? "Attach to enumerate dyld images." : model.debugModules) }.padding(8) }
            case "Backtrace":
                GroupBox("Selected Thread Backtrace") { VStack(alignment: .leading, spacing: 8) { Button("Refresh") { model.debugQuery("backtrace") { $0.debugBacktrace = $1 } }; monospaced(model.debugBacktrace.isEmpty ? "Interrupt the target to unwind the selected thread." : model.debugBacktrace) }.padding(8) }
            case "Live Disassembly":
                GroupBox("Live ARM64 Disassembly") {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Button("Refresh") { model.debugQuery("disassembly") { $0.debugDisassembly = $1 } }
                            Toggle("Follow PC", isOn: $model.debugFollowPC).toggleStyle(.button)
                            Button("PC → Code", action: model.debugOpenPCInAnnotatedCode).disabled(model.debugSnapshot == nil || model.selected == nil)
                            Button("PC → CFG", action: model.debugOpenPCInCFG).disabled(model.debugSnapshot == nil || model.selected == nil); Button("PC → Correlation", action: model.debugOpenPCInRuntimeCorrelation).disabled(model.debugSnapshot == nil || model.selected == nil); Button("PC → Trace", action: model.debugOpenPCInTraceExplorer).disabled(model.debugSnapshot == nil || model.selected == nil)
                            Spacer()
                            if let snapshot = model.debugSnapshot {
                                Text("PC \(snapshot.pc)")
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(.secondary)
                            }
                        }
                        VGRichDisassemblyView(
                            text: model.debugDisassembly,
                            selectedAddress: model.debugSnapshot?.pc ?? model.addressText,
                            emptyMessage: "Interrupt the target to decode instructions around PC.",
                            showFileOffset: false,
                            onSelectAddress: { value in
                                model.debugAddress = value
                                model.addressText = value
                            }
                        )
                        .frame(minHeight: 460)
                    }.padding(8)
                }
            case "Stop Context":
                VStack(alignment: .leading, spacing: 12) {
                    GroupBox("Stop / Exception Context") {
                        VStack(alignment: .leading, spacing: 8) {
                            VGFlowLayout {
                                Button("Refresh Snapshot", action: model.debugRefreshSnapshot).disabled(!model.debugAttached)
                                Toggle("Follow PC", isOn: $model.debugFollowPC).toggleStyle(.button)
                                Button("PC → Code", action: model.debugOpenPCInAnnotatedCode).disabled(model.debugSnapshot == nil || model.selected == nil)
                                Button("PC → CFG", action: model.debugOpenPCInCFG).disabled(model.debugSnapshot == nil || model.selected == nil)
                                Button("PC → Correlation", action: model.debugOpenPCInRuntimeCorrelation).disabled(model.debugSnapshot == nil || model.selected == nil)
                                Button("PC → Trace", action: model.debugOpenPCInTraceExplorer).disabled(model.debugSnapshot == nil || model.selected == nil)
                            }
                            Text(model.debugEvent).font(.system(.caption, design: .monospaced)).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                            if let snap = model.debugSnapshot {
                                Text("thread \(snap.selected_thread_id ?? "?") • PC \(snap.pc) • SP \(snap.sp) • LR \(snap.lr)")
                                    .font(.system(.caption, design: .monospaced)).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                            }
                        }.padding(8)
                    }
                    VGAdaptiveTwoPane { GroupBox("Live Code") { monospaced(model.debugDisassembly.isEmpty ? "Live code appears after a stop." : model.debugDisassembly).padding(8) } } secondary: { GroupBox("Module-aware Backtrace") { monospaced(model.debugBacktrace.isEmpty ? "Backtrace appears after a stop." : model.debugBacktrace).padding(8) } }.frame(minHeight: 360)
                }
            case "Debugger":
                if VGHostPlatform.supportsArm64NativeDebugger { debuggerWorkspace() }
                else { hostUnavailableWorkspace("Native ARM64 Debugger", feature: "Native ARM64 debugging") }
            case "Python Console":
                pythonWorkspace()
            case "Plugins":
                pluginsWorkspace()
            case "Automation SDK":
                automationSDKWorkspace()
            case "REA Agent":
                reaAgentWorkspace()
            case "Extensions Platform":
                extensionsPlatformWorkspace()
            case "Extension Manager":
                extensionManagerWorkspace()
            case "Firmware Runtime Lab", "dyld Cache Browser", "IPSW Catalog", "Firmware Keys", "VectorEmu Lab", "VectorEmu Registers", "VectorEmu Code", "VectorEmu Trace", "Full-System iPhemeral":
                firmwareRuntimeWorkspace(model.workspace)
            case "Hypervisor RE Suite", "Guest Memory", "Guest Disassembly", "Firmware Loader", "VM Snapshots", "Guest Search", "vCPU Monitor", "VM Breakpoints", "VM Trace", "GPA Map", "DART Inspector", "Interrupt Trace":
                if VGHostPlatform.supportsAppleSiliconHypervisor { hypervisorREWorkspace(model.workspace) }
                else { hostUnavailableWorkspace("Hypervisor RE", feature: "Apple-Silicon Hypervisor RE") }
            case "SoC Virtualization":
                if VGHostPlatform.supportsAppleSiliconHypervisor { virtualizationWorkspace() }
                else { hostUnavailableWorkspace("SoC Virtualization", feature: "Apple-Silicon Hypervisor RE") }
            case "Notes":
                GroupBox("Project note") {
                    VStack(alignment: .leading) {
                        TextEditor(text: Binding(get: { model.noteText }, set: { model.noteText = $0 }))
                            .font(.body).frame(minHeight: 260).disabled(model.busy)
                        HStack {
                            Button("Save note", action: model.saveNote).disabled(model.busy || !model.projectServiceReady || !model.noteDirty)
                            Text("\(model.noteText.utf8.count)/4096 bytes • revision \(record.revision)\(model.noteDirty ? " • unsaved" : "")")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }.padding(8)
                }
            default:
                VStack(alignment: .leading, spacing: 16) {
                    VStack(alignment:.leading,spacing:8) {
                        Text("Start with the question you need to answer.").font(.largeTitle).fontWeight(.semibold)
                        Text("Inspect a binary, compare builds, review runtime observations, or preserve evidence. HindSight keeps measured observations distinct from structural analysis and hypotheses.").font(.title3).foregroundStyle(.secondary).frame(maxWidth:900,alignment:.leading)
                        if model.mentorMode { Text("Mentor: use Binary Workspace for code and references, Binary Diff for build changes, and Trace Explorer for debugger observations. Use Triage to preserve evidence and record what remains unknown.").font(.caption).foregroundStyle(.secondary) }
                        Button("Quick Start…", systemImage: "questionmark.circle") { quickStartPresented = true }
                            .buttonStyle(.bordered)
                    }.padding(.bottom,4)
                    investigationPath(record)
                    performanceReadiness()
                    handoffSummary(record)
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 190), spacing: 10)], spacing: 10) {
                        VGMetricTile(title: "Functions", value: String(model.currentAnalysis?.function_count ?? 0), detail: model.currentAnalysis == nil ? "Run analysis" : "bounded discoveries", icon: "function")
                        VGMetricTile(title: "Basic Blocks", value: String(model.currentAnalysis?.basic_block_count ?? 0), detail: model.currentAnalysis?.truncated == true ? "bounded scan" : "control-flow units", icon: "point.3.connected.trianglepath.dotted")
                        VGMetricTile(title: "Targets", value: String(model.unifiedTargets.count), detail: model.selectedUnifiedTarget?.state ?? "none active", icon: "scope")
                    }
                    LazyVGrid(columns:[GridItem(.adaptive(minimum:210),spacing:10)],spacing:10) {
                        reToolCard("Binary Workspace","Functions, code, flow, and references in one place","rectangle.3.group")
                        reToolCard("Binary Diff","Compare two captured builds","arrow.left.arrow.right.square")
                        reToolCard("Analysis Triage","Review, pin, and annotate findings","scope")
                        reToolCard("Trace Explorer","Inspect debugger observations and static correlation","point.topleft.down.to.point.bottomright.curvepath")
                    }
                    GroupBox("Evidence doctrine") {
                        Text("Observed bytes, decoded structure, inferred semantics, modeled hardware and runtime observations remain separately attributable. HindSight should make uncertainty visible instead of laundering it into confident-looking output.").font(.caption).foregroundStyle(.secondary).padding(8)
                    }
                    GroupBox("Source") {
                        VStack(alignment: .leading, spacing: 7) {
                            Text(record.name).font(.headline)
                            Text(record.path).font(.caption).textSelection(.enabled)
                            Text("\(record.result.file_kind) • \(record.result.architecture) • \(record.result.size) bytes")
                            if let digest = record.result.source_sha256 { Text("SHA-256  \(digest)").font(.system(.caption, design: .monospaced)).textSelection(.enabled) }
                            Text(record.result.captured ? "Captured object is immutable-by-identity and reverified before analysis." : "Legacy observation; source bytes were not captured.")
                                .font(.caption).foregroundStyle(.secondary)
                        }.frame(maxWidth: .infinity, alignment: .leading).padding(8)
                    }
                    GroupBox("Analysis overview") {
                        VStack(alignment: .leading, spacing: 10) {
                            analysisToolbar(record)
                            if let report = model.currentAnalysis { monospaced(report.summary) }
                            else { Text("Analyze once, then pivot between code, control-flow, data, and notes layouts without rerunning analysis.").foregroundStyle(.secondary) }
                        }.padding(8)
                    }
                    if let structure = record.result.structure {
                        GroupBox("Mach-O layout • \(record.result.structure_status ?? "unknown")") { monospaced(structure) }
                    }
                }
            }
        })
    }

    @ViewBuilder private func virtualizationWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Apple SoC Virtualization Kit") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("CPU execution uses Hypervisor.framework on Apple Silicon where compatible; Apple-specific SoC blocks are modeled as explicit emulated devices. SEP/T2 profiles are research models, not claims of hardware-equivalent secure execution.").font(.caption).foregroundStyle(.secondary)
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 10) {
                            Picker("Profile", selection: $model.virtualizationProfileID) { ForEach(model.virtualizationProfiles) { Text("\($0.name) · \($0.maturity.capitalized)").tag($0.id) } }
                                .frame(minWidth: 220, idealWidth: 360, maxWidth: 440)
                            Stepper("RAM \(model.virtualizationRAMMiB) MiB", value: $model.virtualizationRAMMiB, in: 16...16384, step: 256)
                            Button("Refresh Profiles", action: model.refreshVirtualizationProfiles)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            Picker("Profile", selection: $model.virtualizationProfileID) { ForEach(model.virtualizationProfiles) { Text("\($0.name) · \($0.maturity.capitalized)").tag($0.id) } }
                            Stepper("RAM \(model.virtualizationRAMMiB) MiB", value: $model.virtualizationRAMMiB, in: 16...16384, step: 256)
                            Button("Refresh Profiles", action: model.refreshVirtualizationProfiles)
                        }
                    }
                    VGFlowLayout {
                        Button("Start", action: model.startVirtualMachine).disabled(model.busy || model.virtualizationProfiles.isEmpty)
                        Button("Reset") { model.virtualizationCommand("reset") }.disabled(model.virtualizationState?.running != true)
                        Button("Stop") { model.virtualizationCommand("stop") }.disabled(model.virtualizationState?.running != true)
                        Button("Status") { model.virtualizationCommand("status") }
                    }
                    GroupBox("Guest firmware / ROM") {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Select a legally obtained image, start the VM, then load it into guest RAM at the specified GPA.")
                                .font(.caption).foregroundStyle(.secondary)
                            VGFlowLayout {
                                Button("Choose Image…", action: model.chooseVirtualizationImage)
                                TextField("Guest GPA (hex)", text: $model.virtualizationFirmwareGPA)
                                    .frame(minWidth: 150, idealWidth: 190, maxWidth: 240)
                                Button("Load into VM", action: model.loadVirtualizationImage)
                                    .disabled(model.virtualizationImagePath.isEmpty || model.virtualizationState?.running != true)
                            }
                            Text(model.virtualizationImagePath.isEmpty ? "No firmware image selected." : model.virtualizationImagePath)
                                .font(.caption).textSelection(.enabled)
                        }.padding(8)
                    }
                    Text(model.virtualizationStatus).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                }.padding(8)
            }
            if let selected = model.virtualizationProfiles.first(where: { $0.id == model.virtualizationProfileID }) {
                VGAdaptiveTwoPane(breakpoint: 860) {
                    GroupBox("Profile") { VStack(alignment: .leading, spacing: 8) { HStack { Text(selected.name).font(.headline); Text(selected.maturity.uppercased()).font(.caption2).fontWeight(.bold).padding(.horizontal, 7).padding(.vertical, 3).background(.orange.opacity(0.16), in: Capsule()).foregroundStyle(.orange) }; Text(selected.description); Text("CPU: \(selected.cpuMode)").font(.caption); Divider(); Text("Modeled devices").font(.caption).fontWeight(.semibold); ForEach(selected.devices, id: \.self) { Label($0, systemImage: "cpu") } }.padding(8) }
                } secondary: {
                    GroupBox("Truth / limitations") { VStack(alignment: .leading, spacing: 8) { ForEach(selected.limitations, id: \.self) { Label($0, systemImage: "exclamationmark.triangle") }; if let state = model.virtualizationState { Divider(); Text("Backend: \(state.cpuBackend)").font(.caption); Text("Device graph: \(state.deviceBackend)").font(.caption); ForEach(state.topology, id: \.self) { Text("• \($0)").font(.caption) } } }.padding(8) }
                }.frame(minHeight: 320)
            }
        }.onAppear { if model.virtualizationProfiles.isEmpty { model.refreshVirtualizationProfiles() } }
    }


    private func isFirmwareRuntimeWorkspace(_ name: String) -> Bool {
        ["Firmware Runtime Lab","dyld Cache Browser","IPSW Catalog","Firmware Keys","VectorEmu Lab","VectorEmu Registers","VectorEmu Code","VectorEmu Trace","Full-System iPhemeral"].contains(name)
    }

    @ViewBuilder private func firmwareRuntimeWorkspace(_ tool: String) -> some View {
        VStack(alignment:.leading,spacing:12) {
            GroupBox("Firmware & Runtime Lab") { VStack(alignment:.leading,spacing:10) {
                Text("HindSight keeps acquisition, static analysis, deterministic execution and full-system research as separate evidence layers. The built-in VectorEmu backend is local-only and ships without firmware, device secrets, restore automation, or exploit tooling.").font(.caption).foregroundStyle(.secondary)
                VGFlowLayout {
                    Button("VectorEmu"){model.workspace="VectorEmu Lab"}
                    Button("dyld Cache"){model.workspace="dyld Cache Browser"}
                    Button("IPSW"){model.workspace="IPSW Catalog"}
                    Button("Keys"){model.workspace="Firmware Keys"}
                    Button("iPhemeral Advanced"){model.workspace="Full-System iPhemeral"}
                    Button("HV RE"){model.workspace="Hypervisor RE Suite"}
                        .disabled(!VGHostPlatform.supportsAppleSiliconHypervisor)
                        .help(VGHostPlatform.supportsAppleSiliconHypervisor ? "Open Hypervisor RE." : VGHostPlatform.unavailableReason(for: "Apple-Silicon Hypervisor RE"))
                }
                Text(model.firmwareStatus).font(.system(.caption,design:.monospaced)).textSelection(.enabled)
            }.padding(8) }
            if tool == "Firmware Runtime Lab" {
                LazyVGrid(columns:[GridItem(.adaptive(minimum:220),spacing:10)],spacing:10) {
                    reToolCard("VectorEmu Lab","Deterministic CPU/MMU/device execution with live state","cpu")
                    reToolCard("VectorEmu Registers","Inspect architectural state without leaving the app","list.number")
                    reToolCard("VectorEmu Code","Live ARM64 decode around any guest address","text.alignleft")
                    reToolCard("VectorEmu Trace","Correlate recent executed instructions and stops","waveform.path.ecg")
                    reToolCard("dyld Cache Browser","Load cache and select frameworks/dylibs","shippingbox")
                    reToolCard("IPSW Catalog","Browse/download/import IPSWs by SoC/device","arrow.down.circle")
                    reToolCard("Firmware Keys","Fetch public IV/key records from The Apple Wiki","key")
                    reToolCard("Full-System iPhemeral","Experimental optional external backend","memorychip")
                    reToolCard("Hypervisor RE Suite","Experimental hardware-assisted guest analysis","bolt.horizontal.circle")
                }
            } else if tool == "VectorEmu Lab" || tool == "VectorEmu Registers" || tool == "VectorEmu Code" || tool == "VectorEmu Trace" {
                GroupBox("VectorEmu • deterministic execution microscope") { VStack(alignment:.leading,spacing:10) {
                    if model.mentorMode {
                        Text("Mentor: start with a legally obtained research image, attach while paused, inspect registers/code, then step or run a bounded number of instructions. Static facts and runtime observations remain separate so the tool never turns a guess into evidence.").font(.caption).foregroundStyle(.secondary)
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            TextField("Built-in vg-emu-server (optional override)",text:$model.vectorEmuExecutable)
                            Button("Choose…",action:model.chooseVectorEmuExecutable)
                            Button("Probe",action:model.vectorEmuStatusCheck)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("Built-in vg-emu-server (optional override)",text:$model.vectorEmuExecutable)
                            VGFlowLayout { Button("Choose…",action:model.chooseVectorEmuExecutable); Button("Probe",action:model.vectorEmuStatusCheck) }
                        }
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            Picker("SoC", selection:$model.vectorEmuSoC) {
                                ForEach(["t8020","t8027","t8030","t8101","t8015","t8010","s8000","a8","a7"],id:\.self){Text($0).tag($0)}
                            }.frame(minWidth: 130, idealWidth: 160, maxWidth: 190)
                            TextField("Research ROM / firmware image",text:$model.vectorEmuROM)
                            Button("Choose Image…",action:model.chooseVectorEmuROM)
                            TextField("Port", value:$model.vectorEmuPort, format:.number).frame(minWidth: 70, idealWidth: 90, maxWidth: 110)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            Picker("SoC", selection:$model.vectorEmuSoC) {
                                ForEach(["t8020","t8027","t8030","t8101","t8015","t8010","s8000","a8","a7"],id:\.self){Text($0).tag($0)}
                            }
                            TextField("Research ROM / firmware image",text:$model.vectorEmuROM)
                            VGFlowLayout {
                                Button("Choose Image…",action:model.chooseVectorEmuROM)
                                TextField("Port", value:$model.vectorEmuPort, format:.number).frame(width: 100)
                            }
                        }
                    }
                    VGFlowLayout {
                        Button("Start",action:model.startVectorEmu).disabled(model.vectorEmuRunning || model.vectorEmuROM.isEmpty)
                        Button("Stop",action:model.stopVectorEmu).disabled(!model.vectorEmuRunning)
                        Button("Attach"){model.vectorEmuDebug("attach")}.disabled(!model.vectorEmuRunning || model.vectorEmuAttached)
                        Button("Pause"){model.vectorEmuDebug("pause")}.disabled(!model.vectorEmuAttached)
                        Button("Run"){model.vectorEmuDebug("run")}.disabled(!model.vectorEmuAttached)
                        Button("Step"){model.vectorEmuDebug("step")}.disabled(!model.vectorEmuAttached)
                        Button("Reset"){model.vectorEmuDebug("reset")}.disabled(!model.vectorEmuAttached)
                        Label(model.vectorEmuRunning ? (model.vectorEmuAttached ? "attached" : "running") : "stopped", systemImage:model.vectorEmuRunning ? "circle.fill" : "circle")
                            .font(.caption).foregroundStyle(model.vectorEmuRunning ? .green : .secondary)
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) {
                            TextField("Guest address",text:$model.vectorEmuAddress).frame(minWidth: 160, idealWidth: 220, maxWidth: 260)
                            Stepper("Count \(model.vectorEmuCount)",value:$model.vectorEmuCount,in:1...1024)
                            Stepper("Run steps \(model.vectorEmuSteps)", value: $model.vectorEmuSteps, in: UInt64(1)...UInt64(1_000_000_000), step: 1000)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("Guest address",text:$model.vectorEmuAddress)
                            Stepper("Count \(model.vectorEmuCount)",value:$model.vectorEmuCount,in:1...1024)
                            Stepper("Run steps \(model.vectorEmuSteps)", value: $model.vectorEmuSteps, in: UInt64(1)...UInt64(1_000_000_000), step: 1000)
                        }
                    }
                    VGFlowLayout {
                        Button("Registers"){model.vectorEmuDebug("registers")}.disabled(!model.vectorEmuAttached)
                        Button("Decode"){model.vectorEmuDebug("disassemble")}.disabled(!model.vectorEmuAttached)
                        Button("Trace"){model.vectorEmuDebug("trace")}.disabled(!model.vectorEmuAttached)
                        Button("Regions"){model.vectorEmuDebug("regions")}.disabled(!model.vectorEmuAttached)
                        Button("Set Breakpoint"){model.vectorEmuDebug("breakpoint_add")}.disabled(!model.vectorEmuAttached)
                        Button("Remove Breakpoint"){model.vectorEmuDebug("breakpoint_remove")}.disabled(!model.vectorEmuAttached)
                        Button("List Breakpoints"){model.vectorEmuDebug("breakpoints")}.disabled(!model.vectorEmuAttached)
                        Button("Refresh Context",action:model.vectorEmuRefreshContext).disabled(!model.vectorEmuAttached)
                    }
                    Text(model.vectorEmuStatus).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }.padding(8) }
                if tool == "VectorEmu Registers" {
                    GroupBox("Architectural State") { monospaced(model.vectorEmuRegisters.isEmpty ? "Attach, then request Registers or CPU State." : model.vectorEmuRegisters).frame(minHeight:360) }
                } else if tool == "VectorEmu Code" {
                    GroupBox("Live ARM64") { monospaced(model.vectorEmuDisassembly.isEmpty ? "Choose an address and Decode." : model.vectorEmuDisassembly).frame(minHeight:360) }
                } else if tool == "VectorEmu Trace" {
                    GroupBox("Execution Timeline") { monospaced(model.vectorEmuTrace.isEmpty ? "Run or step the target, then request Trace." : model.vectorEmuTrace).frame(minHeight:360) }
                } else {
                    VGAdaptiveTwoPane {
                        GroupBox("Registers") { monospaced(model.vectorEmuRegisters.isEmpty ? "Registers appear here." : model.vectorEmuRegisters) }
                    } secondary: {
                        GroupBox("Live Code") { monospaced(model.vectorEmuDisassembly.isEmpty ? "Decoded instructions appear here." : model.vectorEmuDisassembly) }
                    }.frame(minHeight:300)
                    VGAdaptiveTwoPane {
                        GroupBox("Regions") { monospaced(model.vectorEmuRegions.isEmpty ? "Physical bus/device map appears here." : model.vectorEmuRegions) }
                    } secondary: {
                        GroupBox("Engine / Protocol") { monospaced(model.vectorEmuRaw.isEmpty ? model.vectorEmuStatus : model.vectorEmuRaw) }
                    }.frame(minHeight:220)
                }
            } else if tool == "dyld Cache Browser" {
                GroupBox("dyld Shared Cache") { VStack(alignment:.leading,spacing:10) {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) { Button("Open Cache…",action:model.chooseDyldCache); TextField("Filter frameworks / dylibs",text:$model.dyldFilter) }
                        VStack(alignment: .leading, spacing: 8) { Button("Open Cache…",action:model.chooseDyldCache); TextField("Filter frameworks / dylibs",text:$model.dyldFilter) }
                    }
                    Text(model.dyldCachePath.isEmpty ? "No cache selected." : model.dyldCachePath).font(.caption).textSelection(.enabled)
                    if let c=model.dyldCache {
                        Text("\(c.magic) • \(c.imageCount) images • \(c.mappingCount) mappings").font(.caption)
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 2) {
                                ForEach(model.filteredDyldImages) { img in
                                    Button { model.selectedDyldImageID = img.id } label: {
                                        VStack(alignment: .leading) {
                                            Text(img.path).lineLimit(1)
                                            Text("\(img.loadAddress) • __TEXT \(img.textSize) bytes")
                                                .font(.caption).foregroundStyle(.secondary)
                                        }
                                        .padding(7)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(img.id == model.selectedDyldImageID ? Color.accentColor.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 7))
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                            .padding(8)
                        }
                        .frame(minHeight: 300)
                        .onChange(of: model.selectedDyldImageID) { _, _ in model.useSelectedDyldImageAddress() }
                        Divider()
                        Text("Bounded cache read").font(.headline)
                        Text("Translates a cache virtual address through the primary mapping table and reads only validated file-backed bytes. Addresses in subcaches remain explicitly unresolved until that subcache is opened.").font(.caption).foregroundStyle(.secondary)
                        Button("Analyze Selected Image") { model.analyzeSelectedDyldImage() }
                            .disabled(model.busy || model.selectedDyldImageID.isEmpty || model.project == nil)
                        Text("Imports the matching installed image only after its Mach-O UUID is verified against the selected cache entry.").font(.caption).foregroundStyle(.secondary)
                        ViewThatFits(in:.horizontal) {
                            HStack(spacing:8) { TextField("VM address",text:$model.dyldReadAddress).textFieldStyle(.roundedBorder); Stepper("\(model.dyldReadCount) bytes",value:$model.dyldReadCount,in:16...65536,step:16); Button("Read",action:model.readDyldCacheBytes); Button("Selected Header",action:model.readSelectedDyldImageHeader).disabled(model.selectedDyldImageID.isEmpty) }
                            VStack(alignment:.leading,spacing:8) { TextField("VM address",text:$model.dyldReadAddress).textFieldStyle(.roundedBorder); Stepper("\(model.dyldReadCount) bytes",value:$model.dyldReadCount,in:16...65536,step:16); HStack { Button("Read",action:model.readDyldCacheBytes); Button("Selected Header",action:model.readSelectedDyldImageHeader).disabled(model.selectedDyldImageID.isEmpty) } }
                        }
                        if !model.dyldReadText.isEmpty { monospaced(model.dyldReadText).frame(minHeight:180,maxHeight:320) }
                    }
                }.padding(8) }
            } else if tool == "IPSW Catalog" {
                GroupBox("IPSW / SoC Catalog") { VStack(alignment:.leading,spacing:10) {
                    TextField("SoC / platform filter (a12, t8020, a14…)",text:$model.firmwareSoCFilter)
                    VGFlowLayout {
                        Button("Load Devices",action:model.refreshFirmwareDevices)
                        Button("Import Local IPSW…",action:model.chooseIPSW)
                    }
                    Picker("Device",selection:$model.firmwareDeviceID){ForEach(model.firmwareDevices){d in Text("\(d.name) [\(d.identifier)] • \(d.apple_cpu ?? d.platform ?? "?")").tag(d.identifier)}}
                    VGFlowLayout {
                        Button("Firmware Releases",action:model.refreshFirmwareReleases).disabled(model.firmwareDeviceID.isEmpty)
                        Button("Use in VectorEmu",action:model.useSelectedDeviceForVectorEmu).disabled(model.selectedFirmwareDevice?.platform == nil)
                        Button("Use in iPhemeral",action:model.useSelectedDeviceForEmulator).disabled(model.selectedFirmwareDevice?.platform == nil)
                    }
                    Picker("Build",selection:$model.firmwareBuildID){ForEach(model.firmwareReleases){f in Text("iOS \(f.version) • \(f.buildid)\(f.signed == true ? " • signed" : "")").tag(f.buildid)}}
                    VGFlowLayout {
                        Button("Download IPSW…",action:model.downloadSelectedIPSW).disabled(model.firmwareBuildID.isEmpty)
                        Button("Fetch Public Keys",action:model.fetchSelectedFirmwareKeys).disabled(model.firmwareBuildID.isEmpty)
                    }
                    Text(model.ipswPath.isEmpty ? "No local IPSW selected/downloaded." : model.ipswPath).font(.caption).textSelection(.enabled)
                    if !model.firmwareText.isEmpty { monospaced(model.firmwareText).frame(minHeight:220) }
                }.padding(8) }
            } else if tool == "Firmware Keys" {
                GroupBox("Public Firmware Keys") { VStack(alignment:.leading,spacing:8) {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 190), spacing: 8)], alignment: .leading, spacing: 6) {
                        VGKeyValueField(label: "Device", value: model.firmwareDeviceID.isEmpty ? "select in IPSW Catalog" : model.firmwareDeviceID, monospaced: true)
                        VGKeyValueField(label: "Build", value: model.firmwareBuildID.isEmpty ? "select a build" : model.firmwareBuildID, monospaced: true)
                    }
                    Button("Fetch from The Apple Wiki",action:model.fetchSelectedFirmwareKeys).disabled(model.firmwareBuildID.isEmpty)
                    Text("Only publicly published firmware IV/key material is imported. HindSight does not generate or infer UID/GID/device-unique secrets.").font(.caption).foregroundStyle(.secondary)
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 8) {
                            ForEach(model.firmwareKeys) { k in
                                VStack(alignment: .leading, spacing: 3) {
                                    Text(k.filename).font(.headline)
                                    Text("IV  \(k.iv)").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                                    Text("KEY \(k.key)").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                                    Text(k.sourcePage).font(.caption2).foregroundStyle(.secondary)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .padding(8)
                    }
                    .frame(minHeight: 360)
                }.padding(8) }
            } else if tool == "Full-System iPhemeral" {
                GroupBox("Full-System iPhemeral • advanced external backend") { VStack(alignment:.leading,spacing:10) {
                    Text("External iPhemeral 1.9-compatible full-system backend. It supports real-firmware boot, IPSW/DFU restore research, SpringBoard experiments, UART capture, and a loopback iphem-re/2 debugger. HindSight does not bundle firmware, restore credentials, device secrets, or key material.").font(.caption).foregroundStyle(.secondary)
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 8) { TextField("iphem-cli",text:$model.iphemeralExecutable); Button("Choose…",action:model.chooseIPhemeralExecutable); Button("Probe",action:model.iphemeralStatusCheck) }
                        VStack(alignment: .leading, spacing: 8) { TextField("iphem-cli",text:$model.iphemeralExecutable); VGFlowLayout { Button("Choose…",action:model.chooseIPhemeralExecutable); Button("Probe",action:model.iphemeralStatusCheck) } }
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Picker("Boot mode", selection:$model.iphemeralMode) {
                            Text("Raw SecureROM / firmware").tag("raw")
                            Text("IPSW boot assessment").tag("ipsw")
                            Text("System boot (directory or IPSW)").tag("system_boot")
                            Text("DFU restore").tag("dfu_restore")
                            Text("SpringBoard runloop").tag("springboard")
                            Text("IPSW install").tag("ipsw_install")
                        }
                        TextField("SoC (t8010, t8015, t8020, t8027, t8030, t8101, s8000, a8, a7)",text:$model.iphemeralSoC)
                        if model.iphemeralMode == "raw" {
                            TextField("SecureROM / iBoot image",text:$model.iphemeralROM)
                            Button("Choose ROM…",action:model.chooseIPhemeralROM)
                        } else {
                            TextField("IPSW / firmware source",text:$model.iphemeralBootSource)
                            Button("Choose IPSW / firmware…",action:model.chooseIPhemeralBootSource)
                            TextField("Optional matching ROM",text:$model.iphemeralROM)
                            Button("Choose ROM…",action:model.chooseIPhemeralROM)
                        }
                        HStack {
                            Stepper("RE port \(model.iphemeralPort)",value:$model.iphemeralPort,in:1024...65535)
                            Stepper("Steps",value:$model.iphemeralSteps,in:0...1_000_000_000,step:1000)
                            Text(model.iphemeralSteps == 0 ? "unbounded" : String(model.iphemeralSteps)).font(.caption)
                        }
                    }
                    VGFlowLayout {
                        Button("Start",action:model.startIPhemeral).disabled(model.iphemeralRunning)
                        Button("Stop",action:model.stopIPhemeral).disabled(!model.iphemeralRunning)
                        Button("Attach") { model.iphemeralDebug("attach") }.disabled(!model.iphemeralRunning)
                        Button("Registers") { model.iphemeralDebug("registers") }.disabled(!model.iphemeralRunning)
                        Button("Pause") { model.iphemeralDebug("pause") }.disabled(!model.iphemeralRunning)
                        Button("Run") { model.iphemeralDebug("run") }.disabled(!model.iphemeralRunning)
                        Text(model.iphemeralRunning ? "running" : "stopped").foregroundStyle(model.iphemeralRunning ? .green : .secondary)
                    }
                    monospaced(model.iphemeralOutput.isEmpty ? model.firmwareStatus : model.iphemeralOutput).frame(minHeight:260)
                }.padding(8) }
            }
        }
    }

    private func isHypervisorREWorkspace(_ name: String) -> Bool {
        ["Hypervisor RE Suite", "Guest Memory", "Guest Disassembly", "Firmware Loader", "VM Snapshots", "Guest Search", "vCPU Monitor", "VM Breakpoints", "VM Trace", "GPA Map", "DART Inspector", "Interrupt Trace"].contains(name)
    }

    @ViewBuilder private func hypervisorREWorkspace(_ tool: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Hypervisor-assisted Reverse Engineering") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("These tools inspect the isolated guest VM through the Coordinator. Guest RAM accesses are bounded, VM breakpoints patch only guest memory, and modeled Apple-SoC devices are shown as modeled evidence rather than physical-hardware claims.")
                        .font(.caption).foregroundStyle(.secondary)
                    VGFlowLayout {
                        Button("VM") { model.workspace = "SoC Virtualization" }
                        Button("Memory") { model.workspace = "Guest Memory" }
                        Button("Code") { model.workspace = "Guest Disassembly" }
                        Button("Trace") { model.workspace = "VM Trace" }
                        Button("GPA Map") { model.workspace = "GPA Map" }
                        Button("vCPU") { model.workspace = "vCPU Monitor" }
                        Button("Breakpoints") { model.workspace = "VM Breakpoints" }
                    }.font(.caption)
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 10) {
                            TextField("Guest GPA (hex)", text: $model.virtualizationREAddress).frame(minWidth: 150, idealWidth: 220, maxWidth: 260)
                            Stepper("Length \(model.virtualizationRELength)", value: $model.virtualizationRELength, in: 4...16777216, step: 16)
                            Spacer(minLength: 8)
                            Text(model.virtualizationState?.running == true ? "VM active" : "VM not active")
                                .font(.caption).foregroundStyle(model.virtualizationState?.running == true ? .green : .secondary)
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            TextField("Guest GPA (hex)", text: $model.virtualizationREAddress)
                            Stepper("Length \(model.virtualizationRELength)", value: $model.virtualizationRELength, in: 4...16777216, step: 16)
                            Text(model.virtualizationState?.running == true ? "VM active" : "VM not active")
                                .font(.caption).foregroundStyle(model.virtualizationState?.running == true ? .green : .secondary)
                        }
                    }
                }.padding(8)
            }

            if tool == "Hypervisor RE Suite" {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 210), spacing: 10)], spacing: 10) {
                    reToolCard("Guest Memory", "Hex/ASCII RAM inspection", "memorychip")
                    reToolCard("Guest Disassembly", "Decode live guest ARM64", "text.alignleft")
                    reToolCard("Firmware Loader", "Map ROM/firmware into GPA", "square.and.arrow.down")
                    reToolCard("VM Snapshots", "Bounded RAM snapshots + diff", "camera")
                    reToolCard("Guest Search", "Strings and byte patterns", "magnifyingglass")
                    reToolCard("vCPU Monitor", "PC/SP and HVF exits", "cpu")
                    reToolCard("VM Breakpoints", "Guest-only ARM64 BRK patches", "stop.circle")
                    reToolCard("VM Trace", "Timeline of RE/VM operations", "waveform.path.ecg")
                    reToolCard("GPA Map", "RAM/MMIO/device topology", "map")
                    reToolCard("DART Inspector", "IOMMU model evidence", "arrow.triangle.branch")
                    reToolCard("Interrupt Trace", "AIC/doorbell model surface", "bolt")
                }
            } else {
                GroupBox(tool) {
                    VStack(alignment: .leading, spacing: 10) {
                        switch tool {
                        case "Guest Memory":
                            VGFlowLayout { Button("Read") { model.virtualizationRE("memory") }; Button("Snapshot") { model.virtualizationRE("snapshot", length: min(model.virtualizationRELength, 1_048_576)) } }
                        case "Guest Disassembly":
                            VGFlowLayout { Button("Decode") { model.virtualizationRE("disassemble", length: min(model.virtualizationRELength, 4096)) }; Button("Set PC") { model.virtualizationRE("set_pc") }; Button("Run to HV Exit") { model.virtualizationRE("run_once") } }
                        case "Firmware Loader":
                            VGFlowLayout { Button("Choose Image…", action: model.chooseVirtualizationImage); Button("Load at GPA", action: model.loadVirtualizationImage).disabled(model.virtualizationImagePath.isEmpty) }
                            Text(model.virtualizationImagePath.isEmpty ? "No image selected." : model.virtualizationImagePath).font(.caption).textSelection(.enabled)
                        case "VM Snapshots":
                            VGFlowLayout { Button("Take Snapshot") { model.virtualizationRE("snapshot", length: min(model.virtualizationRELength, 1_048_576)) }; Button("Diff Current RAM") { model.virtualizationRE("diff") } }
                            Text("Snapshots are intentionally bounded to 1 MiB so reverse-engineering experiments do not duplicate multi-gigabyte guest RAM in the service.").font(.caption).foregroundStyle(.secondary)
                        case "Guest Search":
                            VStack(alignment: .leading, spacing: 8) {
                                TextField("Hex pattern", text: $model.virtualizationREPattern)
                                VGFlowLayout {
                                    Button("ASCII Strings") { model.virtualizationRE("strings", length: min(model.virtualizationRELength, 16_777_216)) }
                                    Button("Find Bytes") { model.virtualizationRE("pattern", value: model.virtualizationREPattern, length: min(model.virtualizationRELength, 16_777_216)) }
                                }
                            }
                        case "vCPU Monitor":
                            VGFlowLayout { Button("Registers") { model.virtualizationRE("registers") }; Button("Set PC") { model.virtualizationRE("set_pc") }; Button("Run to Next Exit") { model.virtualizationRE("run_once") } }
                            Text("Run-to-exit executes the vCPU until Hypervisor.framework returns an exit; it is not presented as instruction-level single-step.").font(.caption).foregroundStyle(.secondary)
                        case "VM Breakpoints":
                            VGFlowLayout { Button("Set BRK") { model.virtualizationRE("breakpoint_set") }; Button("Remove") { model.virtualizationRE("breakpoint_remove") }; Button("List") { model.virtualizationRE("breakpoints") } }
                        case "VM Trace":
                            Button("Refresh Timeline") { model.virtualizationRE("trace") }
                        case "GPA Map":
                            Button("Refresh Device Map") { model.virtualizationRE("device_map") }
                        case "DART Inspector":
                            Button("Inspect DART Model") { model.virtualizationRE("dart") }
                        case "Interrupt Trace":
                            Button("Inspect Interrupt Model") { model.virtualizationRE("interrupts") }
                        default: EmptyView()
                        }
                        Text(model.virtualizationStatus).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        Divider()
                        Text(model.virtualizationRETitle).font(.headline)
                        monospaced(model.virtualizationREText).frame(minHeight: 280)
                    }.padding(8)
                }
            }
        }
    }

    private func workspaceRequiresAppleSilicon(_ name: String) -> Bool {
        name == "Debugger" || name == "SoC Virtualization" || isHypervisorREWorkspace(name)
    }

    private func workspaceAvailableOnThisHost(_ name: String) -> Bool {
        !workspaceRequiresAppleSilicon(name) || VGHostPlatform.isAppleSiliconProcess
    }

    @ViewBuilder private func hostUnavailableWorkspace(_ title: String, feature: String) -> some View {
        ContentUnavailableView {
            Label("\(title) unavailable on this host", systemImage: "cpu")
        } description: {
            VStack(alignment: .leading, spacing: 6) {
                Text(VGHostPlatform.unavailableReason(for: feature))
                Text("Host capability: \(feature == "Native ARM64 debugging" ? VGHostPlatform.nativeDebuggerStatus : VGHostPlatform.hypervisorStatus)")
                    .font(.caption)
            }
        } actions: {
            Button("Open VectorEmu") { model.workspace = "VectorEmu Lab" }
            Button("Open Navigator") { model.workspace = "Navigator" }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder private func reToolCard(_ name: String, _ subtitle: String, _ icon: String) -> some View {
        let available = workspaceAvailableOnThisHost(name)
        Button { if available { model.workspace = name } } label: {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    ZStack {
                        RoundedRectangle(cornerRadius: 9, style: .continuous)
                            .fill(Color.accentColor.opacity(0.13))
                        Image(systemName: icon)
                            .font(.system(size: 17, weight: .semibold))
                            .symbolRenderingMode(.hierarchical)
                    }
                    .frame(width: 34, height: 34)
                    Spacer()
                    Image(systemName: available ? "arrow.up.right" : "lock.fill")
                        .font(.caption)
                        .foregroundStyle(available ? .tertiary : .secondary)
                }
                Text(name)
                    .font(.system(.headline, design: .rounded))
                Text(available ? subtitle : "Requires Apple Silicon • unavailable on \(VGHostPlatform.architecture.rawValue)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, minHeight: 126, alignment: .leading)
            .padding(14)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 13, style: .continuous)
                    .strokeBorder(.primary.opacity(0.08), lineWidth: 1)
            }
            .contentShape(RoundedRectangle(cornerRadius: 13, style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(!available)
        .help(available ? subtitle : VGHostPlatform.unavailableReason(for: name))
    }

    private var currentDescriptor: VGWorkspaceDescriptor {
        VGWorkspaceCatalog.descriptor(for: model.workspace)
    }

    private var projectDisplayName: String {
        guard let project = model.project else { return "No project open" }
        return URL(fileURLWithPath: project.path).lastPathComponent
    }

    @ViewBuilder private func sidebarWorkspaceButton(_ descriptor: VGWorkspaceDescriptor) -> some View {
        let available = workspaceAvailableOnThisHost(descriptor.name)
        Button {
            if available { model.workspace = descriptor.name }
        } label: {
            VGWorkspaceRow(descriptor: descriptor, selected: model.workspace == descriptor.name)
        }
        .buttonStyle(.plain)
        .disabled(!available)
        .help(available ? descriptor.helpText : VGHostPlatform.unavailableReason(for: descriptor.name))
    }

    private func sidebarSectionHeading(_ title: String) -> some View {
        Text(title.uppercased())
            .font(.system(size: 10, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 7)
    }

    @ViewBuilder private func workbenchSidebar() -> some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(Color.accentColor.opacity(0.15))
                        Image(systemName: "scope")
                            .font(.system(size: 18, weight: .bold))
                            .symbolRenderingMode(.hierarchical)
                    }
                    .frame(width: 36, height: 36)
                    VStack(alignment: .leading, spacing: 1) {
                        Text("HindSight")
                            .font(.system(size: 14, weight: .bold, design: .rounded))
                            .tracking(1.0)
                        Text("Reverse-engineering studio")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                }

                Button {
                    commandPalettePresented = true
                } label: {
                    HStack {
                        Image(systemName: "magnifyingglass")
                        Text("Find anything")
                        Spacer()
                        Text("⌘K")
                            .font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                    .background(.primary.opacity(0.055), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                .buttonStyle(.plain)
                .keyboardShortcut("k", modifiers: .command)
            }
            .padding(14)

            Divider()

            ScrollView {
                LazyVStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 8) {
                    sidebarSectionHeading("Investigation")
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(alignment: .center) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(projectDisplayName)
                                    .font(.system(size: 13, weight: .semibold))
                                    .lineLimit(1)
                                Text(model.project == nil ? "Create or open an investigation" : "\(model.project?.records.count ?? 0) captured source\((model.project?.records.count ?? 0) == 1 ? "" : "s")")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Menu {
                                Button("New Project…", action: model.newProject)
                                    .disabled(model.busy)
                                Button("Open Project…", action: model.openProject)
                                    .disabled(model.busy)
                                if !model.recentProjects.isEmpty {
                                    Divider()
                                    Section("Recent Projects") {
                                        ForEach(model.recentProjects) { recent in
                                            Button(recent.displayName) { model.openRecentProject(recent) }
                                                .help(recent.path)
                                        }
                                    }
                                }
                                Divider()
                                Button("Import Binary…", action: model.selectBinary)
                                    .disabled(!model.canImportBinary)
                                Button("Refresh", action: model.refreshProject)
                                    .disabled(model.project == nil || model.busy)
                            } label: {
                                Image(systemName: "ellipsis.circle")
                            }
                        }

                        if let records = model.project?.records, !records.isEmpty {
                            Divider()
                            ForEach(records.prefix(12)) { record in
                                Button {
                                    model.selectedID = record.id
                                } label: {
                                    HStack(spacing: 8) {
                                        Image(systemName: model.selectedID == record.id ? "doc.fill" : "doc")
                                            .foregroundStyle(model.selectedID == record.id ? Color.accentColor : Color.secondary)
                                        VStack(alignment: .leading, spacing: 1) {
                                            Text(record.name).lineLimit(1)
                                            Text("\(record.result.architecture) • \(record.result.captured ? "captured" : "legacy")")
                                                .font(.caption2)
                                                .foregroundStyle(.secondary)
                                        }
                                        Spacer()
                                    }
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                            if records.count > 12 {
                                Button { model.workspace = "Targets" } label: {
                                    Label("\(records.count - 12) more artifact\(records.count - 12 == 1 ? "" : "s")…", systemImage: "ellipsis")
                                        .font(.caption)
                                }
                                .buttonStyle(.plain)
                                .help("Open Targets to browse the complete project artifact list.")
                            }
                        }
                    }
                    .padding(.vertical, 4)
                }

                TextField("Filter tools", text: $sidebarQuery)
                    .textFieldStyle(.roundedBorder)

                if sidebarQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    ForEach(VGWorkspaceGroup.allCases) { group in
                        VStack(alignment: .leading, spacing: 5) {
                            sidebarSectionHeading(group.rawValue)
                            ForEach(VGWorkspaceCatalog.sidebar(in: group)) { descriptor in
                                sidebarWorkspaceButton(descriptor)
                            }
                        }
                    }
                } else {
                    VStack(alignment: .leading, spacing: 5) {
                        sidebarSectionHeading("Matching Tools")
                        let matches = Array(VGWorkspaceCatalog.search(sidebarQuery).prefix(24))
                        if matches.isEmpty {
                            Text("No tools match ‘\(sidebarQuery)’.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } else {
                            ForEach(matches) { descriptor in
                                sidebarWorkspaceButton(descriptor)
                            }
                        }
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        quickStartPresented = true
                    } label: {
                        Label("Quick Start…", systemImage: "questionmark.circle")
                    }
                    .buttonStyle(.plain)
                    Button {
                        commandPalettePresented = true
                    } label: {
                        Label("All Tools…", systemImage: "square.grid.3x3.square")
                    }
                    .buttonStyle(.plain)
                }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 12)
            }
        }
    }

    @ViewBuilder private func workspaceHeader(_ record: ProjectRecord?) -> some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                HStack(spacing: 4) {
                    Button(action: model.navigateBack) { Image(systemName: "chevron.left") }
                        .disabled(!model.canNavigateBack)
                        .help("Back")
                    Button(action: model.navigateForward) { Image(systemName: "chevron.right") }
                        .disabled(!model.canNavigateForward)
                        .help("Forward")
                }
                .buttonStyle(.borderless)

                VGWorkspaceTitle(
                    descriptor: currentDescriptor,
                    recordName: record?.name,
                    architecture: record?.result.architecture
                )
                .layoutPriority(1)

                Spacer(minLength: 8)

                if record != nil {
                    Button(model.currentAnalysis == nil ? "Analyze" : "Re-analyze", action: model.analyzeSelected)
                        .buttonStyle(.borderedProminent)
                        .disabled(model.busy || !model.projectServiceReady || !(record?.result.captured ?? false))
                }
            }
            .padding(.leading, focusMode ? 82 : 14)
            .padding(.trailing, 14)
            .padding(.top, 10)
            .padding(.bottom, 7)

            HStack(spacing: 8) {
                if record != nil {
                    HStack(spacing: 6) {
                        Image(systemName: "arrow.right.to.line.compact")
                            .foregroundStyle(.secondary)
                        TextField("0x address", text: $model.addressText)
                            .textFieldStyle(.plain)
                            .font(.system(.caption, design: .monospaced))
                            .frame(minWidth: 100, idealWidth: 140, maxWidth: 180)
                            .onSubmit { model.jumpToAddress() }
                        Button("Go", action: model.jumpToAddress)
                            .disabled(model.busy || model.currentPage == nil)
                    }
                    .padding(.horizontal, 9)
                    .padding(.vertical, 5)
                    .background(.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }

                Menu {
                    Section("Analyze") {
                        Button("Review findings") { model.workspace = "Analysis Triage" }
                        Button("Inspect code") { model.workspace = "Binary Workspace" }
                        Button("Compare builds") { model.workspace = "Binary Diff" }
                        Button("Search analysis") { model.workspace = "Navigator" }
                    }
                    Section("Runtime") {
                        Button("Runtime session") { model.workspace = "Trace Explorer" }
                        Button("Debugger") { model.workspace = "Debugger" }
                    }
                    Section("Evidence") {
                        Button("Evidence graph") { model.workspace = "Evidence Graph" }
                        Button("Investigation handoff") { model.workspace = "Investigation Ledger" }
                    }
                    Divider()
                    ForEach(model.plugins.filter { $0.enabled }) { plugin in
                        if !plugin.commands.isEmpty {
                            Section(plugin.name) {
                                ForEach(plugin.commands) { command in
                                    pluginCommandButton(plugin, command)
                                }
                            }
                        }
                    }
                } label: {
                    Label("Workflows", systemImage: "rectangle.3.group")
                }
                .fixedSize()

                Spacer(minLength: 4)

                Toggle(isOn: $model.mentorMode) { Image(systemName: "graduationcap") }
                    .toggleStyle(.button)
                    .help("Mentor mode")

                Button { showInspector.toggle() } label: { Image(systemName: "sidebar.right") }
                    .buttonStyle(.borderless)
                    .help(showInspector ? "Hide Inspector" : "Show Inspector")

                Button(action: toggleFocusMode) {
                    Image(systemName: focusMode ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right")
                }
                .buttonStyle(.borderless)
                .keyboardShortcut("f", modifiers: [.command, .shift])
                .help(focusMode ? "Exit Focus Mode" : "Focus Mode (⇧⌘F)")

                Button { commandPalettePresented = true } label: { Image(systemName: "command") }
                    .buttonStyle(.borderless)
                    .help("Command Palette (⌘K)")

                if model.activeRequest != nil {
                    Button("Cancel", action: model.cancel)
                        .controlSize(.small)
                }
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 9)
            .background(.regularMaterial)

            Divider()
        }
        .background(.regularMaterial)
    }

    @ViewBuilder private func contextualInspector() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label("Inspector", systemImage: "sidebar.right")
                    .font(.headline)
                Spacer()
                Toggle("Pin", isOn: $model.inspectorPinned)
                    .toggleStyle(.button)
                    .controlSize(.small)
            }

            Picker("Inspector", selection: $inspectorMode) {
                ForEach(VGInspectorMode.allCases) { mode in
                    Label(mode.rawValue, systemImage: mode.icon).tag(mode)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            switch inspectorMode {
            case .context:
                GroupBox("Current Context") {
                    Text(model.contextInspector())
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                if !model.selectedSymbol.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("SYMBOL").font(.system(size: 9, weight: .bold, design: .rounded)).foregroundStyle(.secondary).tracking(0.6)
                        Text(model.selectedSymbol).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                    }
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                }

            case .evidence:
                if !model.selectedEvidence.isEmpty {
                    GroupBox("Selected Evidence") {
                        Text(model.selectedEvidence)
                            .font(.system(.caption, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    GroupBox("Analyst annotation") {
                        VStack(alignment: .leading, spacing: 8) {
                            TextEditor(text: $model.annotationDraft)
                                .font(.caption)
                                .frame(minHeight: 72, maxHeight: 120)
                                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.separator))
                            TextField("Tags, separated by commas", text: $model.annotationTagsText)
                                .textFieldStyle(.roundedBorder)
                            HStack {
                                Picker("Confidence", selection: $model.annotationConfidence) {
                                    ForEach(VGAnalysisConfidence.allCases) { value in
                                        Text(value.rawValue).tag(value)
                                    }
                                }
                                .labelsHidden()
                                .frame(maxWidth: 170)
                                Spacer()
                                Button("Save annotation", action: model.saveSelectedEvidenceAnnotation)
                                    .buttonStyle(.borderedProminent)
                                    .controlSize(.small)
                            }
                            Text("Stored with this project and attached to the exact selected evidence identity.")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        .padding(6)
                    }
                }
                GroupBox("Related Evidence") {
                    VGRelatedAnalysisEvidenceView(model: model)
                }

            case .target:
                if let target = model.selectedUnifiedTarget {
                    GroupBox("\(target.name)") {
                        VStack(alignment: .leading, spacing: 7) {
                            Label(target.state, systemImage: "circle.fill")
                                .font(.caption)
                            Text("\(target.kind.rawValue) • \(target.architecture)")
                                .font(.system(.caption, design: .monospaced))
                            Text(target.backend).font(.caption).foregroundStyle(.secondary)
                            Divider()
                            Text("CAPABILITIES").font(.system(size: 9, weight: .bold, design: .rounded)).foregroundStyle(.secondary).tracking(0.6)
                            LazyVGrid(columns: [GridItem(.adaptive(minimum: 105), spacing: 5)], alignment: .leading, spacing: 5) {
                                ForEach(target.capabilities.sorted(by: { $0.rawValue < $1.rawValue }), id: \.self) { capability in
                                    Text(capability.rawValue)
                                        .font(.system(size: 9, weight: .medium, design: .monospaced))
                                        .padding(.horizontal, 6).padding(.vertical, 3)
                                        .background(.primary.opacity(0.055), in: Capsule())
                                }
                            }
                        }
                    }
                    GroupBox("Address Spaces") {
                        VStack(alignment: .leading, spacing: 7) {
                            ForEach(target.addressSpaces.prefix(12)) { space in
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(space.name).font(.caption).fontWeight(.semibold)
                                    Text("\(space.kind)  \(space.base ?? "—")  +  \(space.size ?? "—")")
                                        .font(.system(.caption2, design: .monospaced))
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                } else {
                    ContentUnavailableView("No active target", systemImage: "cpu", description: Text("Load a source, attach a process, or start a machine backend."))
                }
            }

            if !model.activePluginPanelTitle.isEmpty && model.activePluginPanelDock != "bottom" {
                GroupBox(model.activePluginPanelTitle) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack { Spacer(); Button("Close", action: model.closePluginPanel).controlSize(.small) }
                        ScrollView {
                            Text(model.activePluginPanelOutput.isEmpty ? "Panel returned no output." : model.activePluginPanelOutput)
                                .font(.system(.caption2, design: .monospaced))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
            }

            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(minWidth: 250)
        .background(.thinMaterial)
    }

    @ViewBuilder private func emptyInvestigation() -> some View {
        if model.project == nil {
            VStack(spacing: 18) {
                ZStack {
                    RoundedRectangle(cornerRadius: 22, style: .continuous)
                        .fill(Color.accentColor.opacity(0.12))
                    Image(systemName: "scope")
                        .font(.system(size: 42, weight: .semibold))
                        .symbolRenderingMode(.hierarchical)
                }
                .frame(width: 86, height: 86)
                Text("Start an investigation")
                    .font(.system(size: 26, weight: .semibold, design: .rounded))
                Text("Create a project or open an existing one. HindSight keeps captured artifacts, static evidence, live state, emulation, and notes in one investigation context.")
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 580)
                HStack(spacing: 10) {
                    if model.savedLaunchSessionAvailable {
                        Button("Reopen Last Project", action: model.reopenSavedLaunchSession)
                            .buttonStyle(.borderedProminent)
                            .disabled(model.busy || !model.connected || model.connectingToCoordinator || model.restoringProjectSession)
                    }
                    Button("New Project…", action: model.newProject)
                        .buttonStyle(.bordered)
                        .disabled(model.busy || !model.connected || model.connectingToCoordinator || model.restoringProjectSession)
                    Button("Open Project…", action: model.openProject)
                        .buttonStyle(.bordered)
                        .disabled(model.busy || !model.connected || model.connectingToCoordinator || model.restoringProjectSession)
                    Button("Quick Start…") { quickStartPresented = true }
                        .buttonStyle(.bordered)
                    if !model.connected && !model.connectingToCoordinator {
                        Button("Reconnect Coordinator", action: model.reconnect)
                            .buttonStyle(.bordered)
                    }
                }
                if model.savedLaunchSessionAvailable && !model.savedLaunchProjectName.isEmpty {
                    Text("Previous session: \(model.savedLaunchProjectName). HindSight will reopen it automatically when the Coordinator is available; use Support → Forget Saved Project to clear it.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }
                Text(model.restoringProjectSession ? "Restoring the previous project session…" : (model.connectingToCoordinator ? "Connecting to the local analysis service…" : (model.connected ? "Coordinator ready." : "The local Coordinator is offline; reconnect before creating or opening an investigation.")))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(40)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            VStack(spacing: 18) {
                ZStack {
                    RoundedRectangle(cornerRadius: 22, style: .continuous)
                        .fill(Color.accentColor.opacity(0.12))
                    Image(systemName: "doc.badge.plus")
                        .font(.system(size: 42, weight: .semibold))
                        .symbolRenderingMode(.hierarchical)
                }
                .frame(width: 86, height: 86)
                Text("Add your first artifact")
                    .font(.system(size: 26, weight: .semibold, design: .rounded))
                Text("‘\(projectDisplayName)’ is open and ready. Import a Mach-O binary to capture its identity and begin analysis.")
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 560)
                HStack(spacing: 10) {
                    Button("Import Binary…", action: model.selectBinary)
                        .buttonStyle(.borderedProminent)
                        .disabled(!model.canImportBinary)
                    Button("Refresh Project", action: model.refreshProject)
                        .buttonStyle(.bordered)
                        .disabled(model.busy || model.restoringProjectSession)
                    Button("Quick Start…") { quickStartPresented = true }
                        .buttonStyle(.bordered)
                    if !model.connected && !model.connectingToCoordinator {
                        Button("Reconnect", action: model.reconnect)
                            .buttonStyle(.bordered)
                    } else if model.connected && !model.projectSessionReady && !model.restoringProjectSession {
                        Button("Restore Project", action: model.retryProjectRestore)
                            .buttonStyle(.bordered)
                    }
                }
                if model.restoringProjectSession {
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Coordinator restarted; restoring the open project session before project-bound actions resume.")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                } else if !model.projectSessionReady {
                    Text(model.connectingToCoordinator ? "Connecting to the Coordinator…" : (model.connected ? "The Coordinator is online, but this project session still needs to be restored." : "Import is unavailable until the local Coordinator reconnects."))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(40)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    @ViewBuilder private func quickStartGuide() -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "scope")
                    .font(.system(size: 24, weight: .semibold))
                    .foregroundStyle(Color.accentColor)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Quick Start")
                        .font(.title2.weight(.semibold))
                    Text("Choose a task, follow the evidence, and keep the limits of each result visible.")
                        .foregroundStyle(.secondary)
                }
            }

            VStack(alignment: .leading, spacing: 12) {
                quickStartGuideStep(
                    "1", "Create or open a project",
                    "Projects keep captured sources, notes, evidence pins, and review state together.",
                    actionTitle: "New Project…", action: model.newProject,
                    enabled: model.connected && !model.busy
                )
                quickStartGuideStep(
                    "2", "Import an artifact",
                    "Use Auto for Mach-O or composite images. Raw firmware requires an explicit architecture and load address.",
                    actionTitle: "Import Binary…", action: model.selectBinary,
                    enabled: model.projectSessionReady && !model.busy
                )
                quickStartGuideStep(
                    "3", "Run bounded analysis",
                    "Start with Analysis Triage, then follow address- and symbol-backed evidence into code or CFG.",
                    actionTitle: "Open Triage", action: { model.workspace = "Analysis Triage" },
                    enabled: model.currentAnalysis != nil
                )
                quickStartGuideStep(
                    "4", "Document and export",
                    "Keep conclusions separate from findings, then create a case, Markdown, PDF, or JSON handoff from the Investigation Ledger.",
                    actionTitle: "Open Ledger", action: { model.workspace = "Investigation Ledger" },
                    enabled: model.project != nil
                )
            }

            GroupBox("Evidence boundaries") {
                VStack(alignment: .leading, spacing: 6) {
                    Label("Verified structure and decoded observations are kept separate from inferred semantics.", systemImage: "checkmark.seal")
                    Label("Unresolved branches, indirect targets, source lines, and types remain explicitly unresolved.", systemImage: "questionmark.diamond")
                    Label("Encrypted or compressed firmware containers must be unpacked before import.", systemImage: "lock.doc")
                    Label("Debugger stop history is not CPU profiling; HindSight does not yet import sampling, latency, throughput, or allocation profiles.", systemImage: "speedometer")
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(8)
            }

            GroupBox("Bundled tutorial fixture") {
                VStack(alignment: .leading, spacing: 7) {
                    Text("Try Tests/Fixtures/arm64-integer-sample.macho for a deterministic first investigation. It is synthetic, non-runnable test data with visible ARM64 instructions and deliberate unsupported words.")
                    Text("Expected path: import → capture → analyze → review Triage (including its insights and coverage disclosures) → inspect code → annotate a finding → export the ledger.")
                    Text("Full walkthrough: docs/HINDSIGHT_TUTORIAL.md")
                        .font(.system(.caption, design: .monospaced))
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .padding(8)
            }

            Divider()
            HStack {
                Text("Tip: turn on Mentor mode in the workspace header for contextual guidance.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Button("Done") { quickStartPresented = false }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(minWidth: 520, idealWidth: 620, maxWidth: 700)
    }

    @ViewBuilder private func quickStartGuideStep(
        _ number: String,
        _ title: String,
        _ detail: String,
        actionTitle: String,
        action: @escaping () -> Void,
        enabled: Bool
    ) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(number)
                .font(.system(.body, design: .rounded).weight(.bold))
                .frame(width: 28, height: 28)
                .background(Color.accentColor.opacity(0.14), in: Circle())
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.subheadline.weight(.semibold))
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 8)
            Button(actionTitle, action: action)
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(!enabled)
        }
    }

    @ViewBuilder private func investigationPath(_ record: ProjectRecord) -> some View {
        let analyzed = model.currentAnalysis != nil
        let hasNotes = !record.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let hasReviewContext = !model.analysisPins.isEmpty || !model.analysisCollections.isEmpty || model.investigationLedger != nil

        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline) {
                    Label("Investigation path", systemImage: "arrow.right.circle")
                        .font(.headline)
                    Spacer()
                    Text(analyzed ? "Analysis loaded" : "Ready for analysis")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text("Follow the evidence from the imported artifact to an analyst-owned conclusion. Each step keeps uncertainty and provenance visible.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 190), spacing: 8)], spacing: 8) {
                    investigationPathStep(
                        number: "1", title: "Import source",
                        detail: record.result.captured ? "Captured and identity-verified" : "Capture a binary before analysis",
                        icon: record.result.captured ? "checkmark.circle.fill" : "doc.badge.plus",
                        tint: record.result.captured ? .green : .accentColor,
                        actionTitle: "Targets", action: { model.workspace = "Targets" }, enabled: true
                    )
                    investigationPathStep(
                        number: "2", title: "Analyze",
                        detail: analyzed ? "Report and evidence index are available" : "Recover functions, flow, symbols, and data",
                        icon: analyzed ? "checkmark.circle.fill" : "waveform.path.ecg",
                        tint: analyzed ? .green : .accentColor,
                        actionTitle: analyzed ? "Analyze again" : "Analyze binary", action: model.analyzeSelected,
                        enabled: !model.busy && model.projectServiceReady && record.result.captured
                    )
                    investigationPathStep(
                        number: "3", title: "Triage evidence",
                        detail: model.analysisPins.isEmpty ? "Search, correlate, and pin exact findings" : "\(model.analysisPins.count) pinned finding\(model.analysisPins.count == 1 ? "" : "s")",
                        icon: hasReviewContext ? "checkmark.circle.fill" : "scope",
                        tint: hasReviewContext ? .green : .accentColor,
                        actionTitle: "Open Triage", action: { model.workspace = "Analysis Triage" }, enabled: analyzed
                    )
                    investigationPathStep(
                        number: "4", title: "Document conclusion",
                        detail: hasNotes ? "Project note saved" : "Record notes, hypotheses, and follow-ups",
                        icon: hasNotes ? "checkmark.circle.fill" : "note.text",
                        tint: hasNotes ? .green : .accentColor,
                        actionTitle: "Open Notes", action: { model.workspace = "Notes" }, enabled: true
                    )
                    investigationPathStep(
                        number: "5", title: "Export handoff",
                        detail: "Share a bounded case, Markdown, PDF, or JSON record",
                        icon: "square.and.arrow.up", tint: .accentColor,
                        actionTitle: "Open Ledger", action: { model.workspace = "Investigation Ledger" },
                        enabled: hasReviewContext
                    )
                }
            }
            .padding(8)
        }
    }

    @ViewBuilder private func performanceReadiness() -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Label("Performance workload support", systemImage: "speedometer")
                        .font(.headline)
                    Spacer()
                    Text("PROFILE IMPORT NOT AVAILABLE")
                        .font(.system(size: 9, weight: .bold, design: .rounded))
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(.quaternary, in: Capsule())
                }
                Text("HindSight can provide static code structure and debugger stop observations, but it does not currently collect or import CPU-time, sampling, latency, throughput, or allocation profiles. Debugger stops are not performance samples.")
                    .font(.caption).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                VGFlowLayout {
                    Button("Review static structure") { model.workspace = "Analysis Triage" }
                        .disabled(model.currentAnalysis == nil)
                    Button("Open runtime observations") { model.workspace = "Trace Explorer" }
                        .disabled(model.currentAnalysis == nil)
                    Button("Compare builds") { model.workspace = "Binary Diff" }
                }
                .buttonStyle(.borderless)
            }
            .padding(8)
        }
    }

    @ViewBuilder private func handoffSummary(_ record: ProjectRecord) -> some View {
        let openFollowUps = model.analysisFollowUps.filter { $0.state == .open }.count
        GroupBox("Handoff readiness") {
            VStack(alignment: .leading, spacing: 10) {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 8)], spacing: 8) {
                    VGMetricTile(title: "Pinned", value: "\(model.analysisPins.count)", detail: "preserved findings", icon: "pin")
                    VGMetricTile(title: "Annotated", value: "\(model.analysisAnnotationCount)", detail: "analyst context", icon: "note.text")
                    VGMetricTile(title: "Open tasks", value: "\(openFollowUps)", detail: "follow-ups", icon: "checklist")
                }
                Text(openFollowUps == 0 ? "No open follow-ups are recorded. Review the preserved evidence, then export the handoff when the conclusion is ready." : "Resolve or explain the open follow-ups before treating this investigation as ready for handoff.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                VGFlowLayout {
                    Button("Review annotations") { model.workspace = "Analysis Triage" }
                    Button("Open Ledger") { model.workspace = "Investigation Ledger" }
                    Button("Open Notes") { model.workspace = "Notes" }
                }
                .buttonStyle(.borderless)
                Text("Artifact: \(record.name) · SHA-256: \(record.result.source_sha256 ?? "unavailable")")
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.tertiary)
                    .textSelection(.enabled)
            }
            .padding(8)
        }
    }

    @ViewBuilder private func investigationPathStep(
        number: String,
        title: String,
        detail: String,
        icon: String,
        tint: Color,
        actionTitle: String,
        action: @escaping () -> Void,
        enabled: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(alignment: .top, spacing: 8) {
                Text(number)
                    .font(.system(.caption, design: .rounded).weight(.bold))
                    .frame(width: 22, height: 22)
                    .background(tint.opacity(0.16), in: Circle())
                    .foregroundStyle(tint)
                Image(systemName: icon)
                    .foregroundStyle(tint)
                Spacer()
            }
            Text(title).font(.subheadline.weight(.semibold))
            Text(detail)
                .font(.caption2)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(actionTitle, action: action)
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(!enabled)
        }
        .padding(10)
        .frame(maxWidth: .infinity, minHeight: 132, alignment: .topLeading)
        .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
    }

    @ViewBuilder private func selectedWorkspaceContent(_ record: ProjectRecord) -> some View {
        if model.workspace == "CFG" || model.workspace == "Binary Workspace" || model.workspace == "Runtime Correlation" || model.workspace == "Trace Explorer" || model.workspace == "Evidence Graph" || model.workspace == "Investigation Ledger" {
            workspace(record)
                .background(Color(nsColor: .controlBackgroundColor).opacity(0.22))
        } else {
            ScrollView {
                workspace(record)
                    .padding(22)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(Color(nsColor: .controlBackgroundColor).opacity(0.30))
        }
    }

    @ViewBuilder private func selectedWorkspaceAndInspector(_ record: ProjectRecord) -> some View {
        GeometryReader { proxy in
            if showInspector && !focusMode {
                if proxy.size.width >= 1040 {
                    HSplitView {
                        selectedWorkspaceContent(record)
                            .frame(minWidth: 480)
                        contextualInspector()
                            .frame(minWidth: 270, idealWidth: 330, maxWidth: 420)
                    }
                } else {
                    VSplitView {
                        selectedWorkspaceContent(record)
                            .frame(minHeight: 360)
                        contextualInspector()
                            .frame(maxWidth: .infinity)
                            .frame(minHeight: 180, idealHeight: 250, maxHeight: 360)
                    }
                }
            } else {
                selectedWorkspaceContent(record)
            }
        }
    }

    private func workspaceCanvas() -> AnyView {
        AnyView(Group {
            if let record = model.selected {
                VStack(spacing: 0) {
                    selectedWorkspaceAndInspector(record)

                    if !model.activePluginPanelTitle.isEmpty && model.activePluginPanelDock == "bottom" {
                        Divider()
                        GroupBox(model.activePluginPanelTitle) {
                            HStack(alignment: .top) {
                                ScrollView {
                                    Text(model.activePluginPanelOutput.isEmpty ? "Panel returned no output." : model.activePluginPanelOutput)
                                        .font(.system(.caption2, design: .monospaced))
                                        .textSelection(.enabled)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                }
                                Button("Close", action: model.closePluginPanel)
                            }
                            .padding(2)
                        }
                        .frame(minHeight: 120, idealHeight: 180, maxHeight: 260)
                        .padding(.horizontal, 14)
                        .padding(.bottom, 10)
                    }
                }
            } else if model.workspace == "Targets" {
                ScrollView { targetsWorkspace().padding(22) }
            } else if model.workspace == "Debugger" {
                if VGHostPlatform.supportsArm64NativeDebugger {
                    ScrollView { debuggerWorkspace().padding(22) }
                } else {
                    hostUnavailableWorkspace("Native ARM64 Debugger", feature: "Native ARM64 debugging")
                }
            } else if model.workspace == "Python Console" {
                ScrollView { pythonWorkspace().padding(22) }
            } else if model.workspace == "Terminal" {
                terminalWorkspace().padding(22)
            } else if model.workspace == "Plugins" {
                ScrollView { pluginsWorkspace().padding(22) }
            } else if model.workspace == "Automation SDK" {
                ScrollView { automationSDKWorkspace().padding(22) }
            } else if model.workspace == "Extensions Platform" {
                ScrollView { extensionsPlatformWorkspace().padding(22) }
            } else if model.workspace == "Extension Manager" {
                ScrollView { extensionManagerWorkspace().padding(22) }
            } else if isFirmwareRuntimeWorkspace(model.workspace) {
                ScrollView { firmwareRuntimeWorkspace(model.workspace).padding(22) }
            } else if isHypervisorREWorkspace(model.workspace) {
                if VGHostPlatform.supportsAppleSiliconHypervisor {
                    ScrollView { hypervisorREWorkspace(model.workspace).padding(22) }
                } else {
                    hostUnavailableWorkspace("Hypervisor RE", feature: "Apple-Silicon Hypervisor RE")
                }
            } else if model.workspace == "SoC Virtualization" {
                if VGHostPlatform.supportsAppleSiliconHypervisor {
                    ScrollView { virtualizationWorkspace().padding(22) }
                } else {
                    hostUnavailableWorkspace("SoC Virtualization", feature: "Apple-Silicon Hypervisor RE")
                }
            } else {
                emptyInvestigation()
            }
        })
    }

    @ViewBuilder private func terminalWorkspace() -> some View {
        VStack(alignment: .leading, spacing: 12) {
            GroupBox("Terminal") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Interactive local shell hosted by the Coordinator. OpenCode, debuggers, build tools, and ordinary shell commands can run here.")
                        .font(.caption).foregroundStyle(.secondary)
                    HStack {
                        TextField("Working directory", text: $model.terminalWorkingDirectory)
                        Button("Start", action: model.startTerminal).disabled(model.terminalRunning)
                        Button("Stop", action: model.stopTerminal).disabled(!model.terminalRunning)
                        Button("OpenCode") { model.terminalInput = "opencode"; model.sendTerminalInput() }
                            .disabled(!model.terminalRunning)
                        Button("Interrupt", action: model.terminalInterrupt).disabled(!model.terminalRunning)
                        Button("EOF", action: model.terminalEOF).disabled(!model.terminalRunning)
                        Button("Clear", action: model.clearTerminalOutput)
                    }
                    ScrollViewReader { proxy in
                        ScrollView {
                            Text(model.terminalDisplayOutput.isEmpty ? "Terminal output will appear here." : model.terminalDisplayOutput)
                                .font(.system(.body, design: .monospaced))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(10)
                                .id("terminal-output")
                        }
                        .frame(minHeight: 420, maxHeight: .infinity)
                        .background(Color.black.opacity(0.9), in: RoundedRectangle(cornerRadius: 6))
                        .foregroundStyle(.green)
                        .onChange(of: model.terminalDisplayOutput) { _, _ in proxy.scrollTo("terminal-output", anchor: .bottom) }
                        .background {
                            GeometryReader { geometry in
                                Color.clear
                                    .onAppear { model.resizeTerminal(columns: Int(geometry.size.width / 8), rows: Int(geometry.size.height / 18)) }
                                    .onChange(of: geometry.size) { _, size in
                                        model.resizeTerminal(columns: Int(size.width / 8), rows: Int(size.height / 18))
                                    }
                            }
                        }
                    }
                    HStack {
                        TextField("Command", text: $model.terminalInput)
                            .font(.system(.body, design: .monospaced))
                            .onSubmit(model.sendTerminalInput)
                        Button("↑", action: model.terminalHistoryUp)
                            .help("Previous terminal command")
                            .disabled(!model.terminalRunning)
                        Button("↓", action: model.terminalHistoryDown)
                            .help("Next terminal command")
                            .disabled(!model.terminalRunning)
                        Button("Send", action: model.sendTerminalInput).disabled(!model.terminalRunning || model.terminalInput.isEmpty)
                    }
                    Text(model.terminalStatus).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                }.padding(8)
            }
        }
        .onAppear {
            if model.terminalWorkingDirectory.isEmpty { model.terminalWorkingDirectory = model.project?.path ?? FileManager.default.homeDirectoryForCurrentUser.path }
        }
    }

    @ViewBuilder private func statusBar() -> some View {
        VStack(spacing: 3) {
            HStack(spacing: 8) {
                VGHorizontalOverflowBar {
                    HStack(spacing: 8) {
                        if model.connectingToCoordinator || model.restoringProjectSession {
                            ProgressView().controlSize(.small)
                        }
                        VGStatusPill(
                            label: model.coordinatorStateLabel,
                            systemImage: model.projectServiceReady ? "checkmark.circle.fill" : (model.connected ? "arrow.triangle.2.circlepath" : "exclamationmark.triangle"),
                            emphasized: model.projectServiceReady
                        )
                        if !model.connected && !model.connectingToCoordinator {
                            Button("Reconnect", action: model.reconnect)
                                .buttonStyle(.borderless)
                                .font(.caption2)
                        } else if model.connected && model.project != nil && !model.projectSessionReady && !model.restoringProjectSession {
                            Button("Restore", action: model.retryProjectRestore)
                                .buttonStyle(.borderless)
                                .font(.caption2)
                        }
                        if let target = model.selectedUnifiedTarget {
                            VGStatusPill(label: target.name, systemImage: "scope", emphasized: false)
                        }
                        if model.busy {
                            if let progress = model.analysisProgressValue {
                                ProgressView(value: progress, total: 100)
                                    .controlSize(.small)
                                    .frame(width: 72)
                                    .accessibilityLabel("Analysis progress")
                                    .accessibilityValue("\(Int(progress)) percent")
                            } else {
                                ProgressView().controlSize(.small)
                            }
                            Text(model.analysisProgressLabel ?? "Working…")
                                .font(.caption2)
                                .help(model.analysisJobStatus?.detail ?? "A bounded worker request is active.")
                            if model.analysisJobStatus?.operation == "analysis" {
                                Button("Cancel", action: model.cancel)
                                    .buttonStyle(.borderless)
                                    .font(.caption2)
                            }
                        }
                    }
                }
                Spacer(minLength: 0)
                Text(VGProduct.version)
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.tertiary)
                    .fixedSize()
            }

            HStack(spacing: 6) {
                Image(systemName: "text.bubble")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                Text(model.status)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .help(model.status)
                    .textSelection(.enabled)
                Spacer(minLength: 0)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 5)
        .background(.regularMaterial)
    }

    var body: some View {
        HSplitView {
            if !focusMode {
                workbenchSidebar()
                    .padding(.top, 30)
                    .frame(minWidth: 245, idealWidth: 285, maxWidth: 330)
            }
            VStack(spacing: 0) {
                // Bound generic expansion at the three top-level regions. Swift
                // 6.4 otherwise recurses through every workspace's opaque view
                // type while lowering this variadic ViewBuilder to SIL.
                AnyView(workspaceHeader(model.selected))
                VGWorkspaceTabStrip(
                    workspaces: workspaceTabs,
                    activeWorkspace: model.workspace,
                    onSelect: { model.workspace = $0 },
                    onClose: closeWorkspaceTab
                )
                Divider()
                workspaceCanvas()
                Divider()
                AnyView(statusBar())
            }
            .groupBoxStyle(VGPanelGroupBoxStyle())
        }
        .ignoresSafeArea(.container, edges: .top)
        .frame(minWidth: 900, minHeight: 680)
        .sheet(isPresented: $commandPalettePresented) {
            VGCommandPalette(
                isPresented: $commandPalettePresented,
                currentWorkspace: model.workspace,
                onSelect: { model.workspace = $0 }
            )
        }
        .sheet(isPresented: $quickStartPresented) {
            quickStartGuide()
        }
        .onAppear {
            rememberWorkspace(model.workspace)
            model.scheduleColdStart()
        }
        .onChange(of: model.workspace) { _, newValue in rememberWorkspace(newValue) }
    }
}
