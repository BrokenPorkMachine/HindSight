import SwiftUI

private enum VGBinaryCenterMode: String, CaseIterable, Identifiable {
    case split = "Split"
    case code = "Code"
    case lifted = "Lifted"
    var id: String { rawValue }
}

private enum VGBinarySemanticMode: String, CaseIterable, Identifiable {
    case pseudocode = "Pseudocode"
    case lifted = "Lifted"
    case ir = "IR"
    case cfg = "CFG"
    var id: String { rawValue }
}

private enum VGBinaryRightMode: String, CaseIterable, Identifiable {
    case references = "References"
    case data = "Data"
    var id: String { rawValue }
}

struct VGBinaryWorkspace: View {
    let functionsText: String
    let dyldImageIdentity: String?
    let symbolResolverText: String
    let objcClassesText: String
    let disassemblyText: String
    let cfgText: String
    let irText: String
    let dataflowText: String
    let semanticPreviewText: String?
    let xrefsText: String
    let callGraphText: String
    let functionOverrides: [UInt64: VGDecompilerFunctionOverride]
    let functionOverrideUndoCounts: [UInt64: Int]
    let selectedAddress: String
    let debugPC: String?
    let onSelectAddress: (String) -> Void
    let onSaveFunctionOverride: (UInt64, String, String, String, VGAnalysisConfidence) -> Void
    let onUndoFunctionOverride: (UInt64) -> Void

    @State private var functionQuery = ""
    @State private var selectedFunctionStart: UInt64?
    @State private var selectedInstruction: UInt64?
    @State private var showFunctions = true
    @State private var showRightPane = true
    @State private var followPC = true
    @State private var centerMode: VGBinaryCenterMode = .split
    @State private var semanticMode: VGBinarySemanticMode = .pseudocode
    @State private var rightMode: VGBinaryRightMode = .references
    @State private var showFunctionOverride = false
    @State private var overrideName = ""
    @State private var overrideSignature = ""
    @State private var overrideNote = ""
    @State private var overrideConfidence: VGAnalysisConfidence = .unclassified

    private var functions: [VGFunctionRecord] {
        VGGraphParser.functions(functionsText, symbolResolver: symbolResolverText, objcClasses: objcClassesText).map { function in
            guard let name = functionOverrides[function.start]?.analystName, !name.isEmpty else { return function }
            return VGFunctionRecord(start: function.start, end: function.end, byteCount: function.byteCount, name: name)
        }
    }
    private var rows: [VGDisassemblyRow] { VGDisassemblyParser.parse(disassemblyText) }
    private var references: [VGBinaryReference] { VGBinaryWorkspaceParser.references(xrefs: xrefsText, callGraph: callGraphText) }
    private var selectedAddressValue: UInt64? { VGBinaryWorkspaceParser.address(selectedAddress) }
    private var debugPCValue: UInt64? { debugPC.flatMap(VGBinaryWorkspaceParser.address) }

    private var activeAddress: UInt64? {
        if followPC, let debugPCValue { return debugPCValue }
        return selectedInstruction ?? selectedAddressValue
    }

    private var selectedFunction: VGFunctionRecord? {
        if let start = selectedFunctionStart, let exact = functions.first(where: { $0.start == start }) { return exact }
        if let activeAddress, let containing = functions.first(where: { $0.contains(activeAddress) }) { return containing }
        return functions.first
    }

    private var filteredFunctions: [VGFunctionRecord] {
        let query = functionQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return Array(functions.prefix(2000)) }
        return functions.filter { function in
            function.name.lowercased().contains(query) || function.startText.lowercased().contains(query)
        }
    }

    private var functionRows: [VGDisassemblyRow] {
        guard let function = selectedFunction else { return [] }
        return rows.filter { row in
            if row.kind == .metadata { return false }
            guard let address = VGBinaryWorkspaceParser.address(row.address) else { return false }
            return function.contains(address)
        }
    }

    private var liftedLines: [VGLiftedLine] {
        VGBinaryWorkspaceParser.lifted(irText, in: selectedFunction)
    }

    private var rawIRLines: [(UInt64, String, String)] {
        VGBinaryWorkspaceParser.irLines(irText, in: selectedFunction)
    }

    private var functionCFGLines: [VGBinaryCFGLine] {
        VGBinaryWorkspaceParser.cfgLines(cfgText, in: selectedFunction)
    }

    private var functionPseudocode: String? {
        guard let pseudocode = VGBinaryWorkspaceParser.pseudocode(semanticPreviewText, in: selectedFunction) else { return nil }
        guard let function = selectedFunction, let override = functionOverrides[function.start] else { return pseudocode }
        var provenance = ["// Analyst override; binary evidence is unchanged."]
        if !override.analystName.isEmpty { provenance.append("// Analyst function name: \(commentSafe(override.analystName))") }
        if !override.signatureHypothesis.isEmpty { provenance.append("// Analyst signature hypothesis [\(override.confidence.rawValue)]: \(commentSafe(override.signatureHypothesis))") }
        if !override.note.isEmpty { provenance.append("// Analyst note: \(commentSafe(override.note))") }
        return (provenance + [pseudocode]).joined(separator: "\n")
    }

    private func commentSafe(_ value: String) -> String {
        value.split(whereSeparator: \.isNewline).joined(separator: " ").replacingOccurrences(of: "*/", with: "* /")
    }

    private var functionReferences: [VGBinaryReference] {
        guard let function = selectedFunction else { return [] }
        return references.filter { function.contains($0.source) || function.contains($0.target) }
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Functions",
                trailingTitle: "Context",
                leadingVisible: showFunctions,
                trailingVisible: showRightPane,
                leadingIdealWidth: 245,
                trailingIdealWidth: 350
            ) {
                functionNavigator
            } center: {
                centerDesk
            } trailing: {
                rightInspector
            }
        }
        .background(Color.primary.opacity(0.012))
        .onAppear { synchronizeExternalSelection() }
        .onChange(of: selectedAddress) { _, _ in synchronizeExternalSelection() }
        .onChange(of: debugPC ?? "") { _, _ in
            if followPC { synchronizeExternalSelection() }
        }
        .onChange(of: followPC) { _, enabled in
            if enabled { synchronizeExternalSelection() }
        }
        .sheet(isPresented: $showFunctionOverride) { functionOverrideEditor }
    }

    private var functionOverrideEditor: some View {
        NavigationStack {
            Form {
                Section("Function at \(selectedFunction?.startText ?? "unavailable")") {
                    TextField("Analyst function name", text: $overrideName)
                    TextField("Signature hypothesis", text: $overrideSignature)
                    Picker("Confidence", selection: $overrideConfidence) {
                        ForEach(VGAnalysisConfidence.allCases) { confidence in Text(confidence.rawValue).tag(confidence) }
                    }
                    TextEditor(text: $overrideNote).frame(minHeight: 100)
                    Text("These project-local annotations are shown as analyst hypotheses. They do not alter decoded instructions or recovered binary evidence.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Function Override")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { showFunctionOverride = false } }
                ToolbarItem(placement: .automatic) {
                    if let function = selectedFunction, (functionOverrideUndoCounts[function.start] ?? 0) > 0 {
                        Button("Undo Last Edit") {
                            onUndoFunctionOverride(function.start)
                            showFunctionOverride = false
                        }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if let function = selectedFunction {
                            onSaveFunctionOverride(function.start, overrideName, overrideSignature, overrideNote, overrideConfidence)
                        }
                        showFunctionOverride = false
                    }
                }
            }
            .frame(minWidth: 480, minHeight: 360)
        }
    }

    private var toolbar: some View {
        VStack(spacing: 6) {
            HStack(spacing: 10) {
                Label("Binary Workspace", systemImage: "rectangle.3.group")
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                if let dyldImageIdentity {
                    Label(dyldImageIdentity, systemImage: "shippingbox")
                        .font(.system(size: 10, design: .monospaced))
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(dyldImageIdentity)
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                if let function = selectedFunction {
                    Text(function.name)
                        .font(.system(size: 11, weight: .semibold, design: .monospaced))
                        .lineLimit(1)
                    Text("\(function.extentLabel)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else {
                    Text("No function selected").font(.caption2).foregroundStyle(.secondary)
                }
            }
            VGFlowLayout {
                Button { showFunctions.toggle() } label: { Label("Functions", systemImage: "sidebar.left") }
                    .buttonStyle(.borderless)
                    .help(showFunctions ? "Hide function browser" : "Show function browser")
                Button { showRightPane.toggle() } label: { Label("Context", systemImage: "sidebar.right") }
                    .buttonStyle(.borderless)
                    .help(showRightPane ? "Hide context pane" : "Show context pane")
                if let function = selectedFunction {
                    Button {
                        let existing = functionOverrides[function.start]
                        overrideName = existing?.analystName ?? ""
                        overrideSignature = existing?.signatureHypothesis ?? ""
                        overrideNote = existing?.note ?? ""
                        overrideConfidence = existing?.confidence ?? .unclassified
                        showFunctionOverride = true
                    } label: { Label("Function Override", systemImage: "pencil.and.outline") }
                    .buttonStyle(.borderless)
                    .help("Record a project-local analyst name, signature hypothesis, confidence, and note")
                }
                Picker("Center", selection: $centerMode) {
                    ForEach(VGBinaryCenterMode.allCases) { mode in Text(mode.rawValue).tag(mode) }
                }
                .labelsHidden().pickerStyle(.segmented)
                .frame(width: 190)
                if debugPCValue != nil {
                    Toggle("Follow PC", isOn: $followPC).toggleStyle(.button).controlSize(.small)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private var functionNavigator: some View {
        VStack(spacing: 0) {
            HStack {
                Text("FUNCTIONS")
                    .font(.system(size: 9, weight: .bold, design: .rounded))
                    .tracking(0.7)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("\(functions.count)").font(.caption2).foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 10)
            .padding(.top, 10)
            .padding(.bottom, 7)

            TextField("Filter name or address", text: $functionQuery)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 8)
                .padding(.bottom, 8)

            Divider()

            if filteredFunctions.isEmpty {
                ContentUnavailableView("No matching functions", systemImage: "function", description: Text("Change the filter or run binary analysis."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 2) {
                        ForEach(filteredFunctions) { function in
                            Button {
                                selectFunction(function)
                            } label: {
                                VStack(alignment: .leading, spacing: 3) {
                                    HStack(spacing: 6) {
                                        Image(systemName: selectedFunction?.start == function.start ? "scope" : "function")
                                            .font(.system(size: 9, weight: .semibold))
                                            .foregroundStyle(selectedFunction?.start == function.start ? Color.accentColor : Color.secondary)
                                        Text(function.name)
                                            .font(.system(size: 11, weight: .medium, design: .monospaced))
                                            .lineLimit(1)
                                    }
                                    HStack(spacing: 5) {
                                        Text(function.startText)
                                        Text("•")
                                        Text("\(function.extentLabel)")
                                    }
                                    .font(.system(size: 9, design: .monospaced))
                                    .foregroundStyle(.secondary)
                                }
                                .padding(.horizontal, 8)
                                .padding(.vertical, 6)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                                .background {
                                    if selectedFunction?.start == function.start {
                                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                                            .fill(Color.accentColor.opacity(0.10))
                                    }
                                }
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(5)
                }
            }
        }
        .frame(minWidth: 205, idealWidth: 245, maxWidth: 320)
        .background(.thinMaterial)
    }

    @ViewBuilder private var centerDesk: some View {
        switch centerMode {
        case .split:
            VSplitView {
                codePane.frame(minHeight: 290)
                semanticPane.frame(minHeight: 220)
            }
        case .code:
            codePane
        case .lifted:
            semanticPane
        }
    }

    private var codePane: some View {
        VStack(spacing: 0) {
            paneHeader("DISASSEMBLY", detail: functionRows.isEmpty ? "no rows" : "\(functionRows.filter { $0.kind == .instruction }.count) instructions", icon: "text.alignleft")
            Divider()
            if functionRows.isEmpty {
                ContentUnavailableView("No function code", systemImage: "text.alignleft", description: Text("Select a discovered function after analysis."))
            } else {
                ScrollViewReader { proxy in
                    ScrollView([.horizontal, .vertical]) {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(functionRows) { row in
                                binaryCodeRow(row)
                                    .id(assemblyScrollID(row))
                            }
                        }
                        .padding(.vertical, 5)
                        .frame(minWidth: 760, alignment: .leading)
                    }
                    .onAppear { scrollAssembly(to: activeAddress, using: proxy) }
                    .onChange(of: activeAddress) { _, address in scrollAssembly(to: address, using: proxy) }
                }
            }
        }
        .background(Color.primary.opacity(0.016))
    }

    private var semanticPane: some View {
        VStack(spacing: 0) {
            HStack {
                paneHeader("SEMANTIC VIEW", detail: semanticMode == .pseudocode ? "reconstructed sketch" : semanticMode == .lifted ? "conservative lift" : "worker IR", icon: "curlybraces")
                Spacer()
                Picker("Semantic", selection: $semanticMode) {
                    ForEach(VGBinarySemanticMode.allCases) { mode in Text(mode.rawValue).tag(mode) }
                }
                .labelsHidden()
                .pickerStyle(.segmented)
                .frame(width: 320)
                .padding(.trailing, 8)
            }
            Divider()
            ScrollViewReader { proxy in
                ScrollView([.horizontal, .vertical]) {
                    if semanticMode == .pseudocode {
                        pseudocodeContent
                    } else if semanticMode == .lifted {
                        liftedContent
                    } else if semanticMode == .ir {
                        irContent
                    } else {
                        cfgContent
                    }
                }
                .onAppear { scrollPseudocode(to: activeAddress, using: proxy) }
                .onChange(of: activeAddress) { _, address in scrollPseudocode(to: address, using: proxy) }
                .onChange(of: semanticMode) { _, mode in
                    if mode == .pseudocode { scrollPseudocode(to: activeAddress, using: proxy) }
                }
            }
        }
        .background(Color.primary.opacity(0.012))
    }

    private var pseudocodeContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let functionPseudocode {
                LazyVStack(alignment: .leading, spacing: 1) {
                    ForEach(Array(functionPseudocode.split(separator: "\n", omittingEmptySubsequences: false).enumerated()), id: \.offset) { item in
                        let line = String(item.element)
                        if let address = VGBinaryWorkspaceParser.pseudocodeSourceAddress(line) {
                            Button { selectAddress(address) } label: {
                                Text(line)
                                    .font(.system(size: 11, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(.vertical, 1)
                                    .background(activeAddress == address ? Color.accentColor.opacity(0.14) : Color.clear)
                                    .contentShape(Rectangle())
                            }
                            .id("pseudo-\(String(address, radix: 16))")
                            .buttonStyle(.plain)
                            .help(String(format: "Select instruction at 0x%016llx", address))
                        } else {
                            Text(line)
                                .font(.system(size: 11, design: .monospaced))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .textSelection(.enabled)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text("No reconstructed sketch for this function in the bounded preview. Analyze again to refresh cached results, or use Lifted and IR for instruction evidence.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .frame(minWidth: 760, alignment: .leading)
    }

    private var liftedContent: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            if liftedLines.isEmpty {
                Text("No lifted statements intersect this function.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(12)
            } else {
                ForEach(liftedLines) { line in
                    Button {
                        selectAddress(line.address)
                    } label: {
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(line.addressText)
                                .frame(width: 148, alignment: .leading)
                                .foregroundStyle(.secondary)
                            confidenceBadge(line.confidence)
                            Text(line.text)
                                .font(.system(size: 11, design: .monospaced))
                                .foregroundStyle(line.confidence == .opaque ? .secondary : .primary)
                            Spacer(minLength: 20)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .contentShape(Rectangle())
                        .background(selectionBackground(line.address))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(.vertical, 4)
        .frame(minWidth: 760, alignment: .leading)
    }

    private var irContent: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            if rawIRLines.isEmpty {
                Text("No IR records intersect this function.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(12)
            } else {
                ForEach(Array(rawIRLines.enumerated()), id: \.offset) { _, item in
                    Button {
                        selectAddress(item.0)
                    } label: {
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(String(format: "0x%016llx", item.0))
                                .frame(width: 148, alignment: .leading)
                                .foregroundStyle(.secondary)
                            Text(item.1.uppercased())
                                .font(.system(size: 9, weight: .bold, design: .rounded))
                                .foregroundStyle(.secondary)
                                .frame(width: 62, alignment: .leading)
                            Text(item.2)
                                .font(.system(size: 11, design: .monospaced))
                            Spacer(minLength: 20)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .contentShape(Rectangle())
                        .background(selectionBackground(item.0))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(.vertical, 4)
        .frame(minWidth: 760, alignment: .leading)
    }

    private var cfgContent: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            if functionCFGLines.isEmpty {
                Text("No CFG blocks intersect this function in the bounded analysis.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(12)
            } else {
                ForEach(functionCFGLines) { line in
                    if let address = line.address {
                        Button { selectAddress(address) } label: {
                            Text(line.text)
                                .font(.system(size: 10, design: .monospaced))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .contentShape(Rectangle())
                                .background(selectionBackground(address))
                        }
                        .buttonStyle(.plain)
                    } else {
                        Text(line.text)
                            .font(.system(size: 10, design: .monospaced))
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                    }
                }
            }
        }
        .padding(.vertical, 4)
        .frame(minWidth: 760, alignment: .leading)
    }

    private var rightInspector: some View {
        VStack(spacing: 0) {
            HStack {
                Text("CONTEXT")
                    .font(.system(size: 9, weight: .bold, design: .rounded))
                    .tracking(0.7)
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.top, 10)
            .padding(.bottom, 7)

            Picker("Context", selection: $rightMode) {
                ForEach(VGBinaryRightMode.allCases) { mode in Text(mode.rawValue).tag(mode) }
            }
            .labelsHidden()
            .pickerStyle(.segmented)
            .padding(.horizontal, 8)
            .padding(.bottom, 8)

            Divider()

            switch rightMode {
            case .references: referencePane
            case .data: dataPane
            }
        }
        .frame(minWidth: 300, idealWidth: 350, maxWidth: 460)
        .background(.thinMaterial)
    }

    private var referencePane: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 4) {
                if functionReferences.isEmpty {
                    Text("No direct references intersect this function.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(10)
                } else {
                    ForEach(functionReferences.prefix(1000)) { ref in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 6) {
                                Image(systemName: ref.isCall ? "arrow.turn.up.right" : "arrow.triangle.branch")
                                    .font(.caption2)
                                    .foregroundStyle(ref.isCall ? Color.orange : Color.accentColor)
                                Text(ref.kind.uppercased())
                                    .font(.system(size: 9, weight: .bold, design: .rounded))
                                Spacer()
                            }
                            HStack(spacing: 5) {
                                Button(ref.sourceText) { selectAddress(ref.source) }.buttonStyle(.link)
                                Image(systemName: "arrow.right").font(.caption2).foregroundStyle(.secondary)
                                Button(ref.targetText) { selectAddress(ref.target) }.buttonStyle(.link)
                            }
                            .font(.system(size: 9, design: .monospaced))
                            Text(ref.detail)
                                .font(.system(size: 9, design: .monospaced))
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                                .textSelection(.enabled)
                        }
                        .padding(8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    }
                }
            }
            .padding(6)
        }
    }

    private var dataPane: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let data = VGBinaryWorkspaceParser.instructionData(rows: rows, address: activeAddress) {
                    dataSection("Instruction") {
                        dataLine("Address", data.addressText)
                        dataLine("Opcode", data.wordText)
                        dataLine("Bytes (LE)", data.bytesText)
                        dataLine("Unsigned", data.unsignedText)
                        dataLine("Signed", data.signedText)
                        if !data.fileOffset.isEmpty { dataLine("File offset", data.fileOffset) }
                    }
                    dataSection("Decode") {
                        dataLine("Mnemonic", data.mnemonic)
                        if !data.operands.isEmpty { dataLine("Operands", data.operands) }
                        if !data.annotation.isEmpty { dataLine("Evidence", data.annotation) }
                    }
                } else {
                    Text("Select an instruction to inspect its word and bytes.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                let flow = VGBinaryWorkspaceParser.dataflowLines(dataflowText, address: activeAddress, function: selectedFunction)
                dataSection("Modeled data flow") {
                    if flow.isEmpty {
                        Text("No modeled data-flow record for the current selection.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(Array(flow.prefix(64).enumerated()), id: \.offset) { _, line in
                            Text(line)
                                .font(.system(size: 9, design: .monospaced))
                                .textSelection(.enabled)
                        }
                    }
                }

                Text("Instruction bytes come from the analyzed ARM64 word. Data-flow text is modeled evidence; unsupported instruction semantics remain explicit barriers.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(10)
        }
    }

    private func binaryCodeRow(_ row: VGDisassemblyRow) -> some View {
        Group {
            if row.kind == .symbol {
                HStack(spacing: 7) {
                    Image(systemName: "bookmark.fill").font(.caption2).foregroundStyle(Color.accentColor)
                    Text(row.address).foregroundStyle(.secondary)
                    Text(row.mnemonic).fontWeight(.semibold)
                    Spacer()
                }
                .font(.system(size: 10, design: .monospaced))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(Color.accentColor.opacity(0.05))
            } else {
                Button {
                    if let address = VGBinaryWorkspaceParser.address(row.address) { selectAddress(address) }
                } label: {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Image(systemName: row.isCall ? "arrow.turn.up.right" : row.isControlFlow ? "arrow.triangle.branch" : "circle.fill")
                            .font(.system(size: row.isControlFlow ? 8 : 3.5, weight: .semibold))
                            .foregroundStyle(row.isCall ? AnyShapeStyle(Color.orange) : row.isControlFlow ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(Color.secondary.opacity(0.5)))
                            .frame(width: 12)
                        Text(row.address)
                            .frame(width: 150, alignment: .leading)
                            .foregroundStyle(.secondary)
                        Text(row.word)
                            .frame(width: 78, alignment: .leading)
                            .foregroundStyle(.tertiary)
                        Text(row.mnemonic)
                            .fontWeight(.semibold)
                            .frame(width: 58, alignment: .leading)
                        Text(row.operands)
                        if !row.annotation.isEmpty { Text("; \(row.annotation)").foregroundStyle(.secondary) }
                        Spacer(minLength: 20)
                    }
                    .font(.system(size: 10.5, design: .monospaced))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4.5)
                    .contentShape(Rectangle())
                    .background {
                        if let address = VGBinaryWorkspaceParser.address(row.address) {
                            selectionBackground(address)
                        }
                    }
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func paneHeader(_ title: String, detail: String, icon: String) -> some View {
        HStack(spacing: 7) {
            Image(systemName: icon).font(.caption).foregroundStyle(.secondary)
            Text(title)
                .font(.system(size: 9, weight: .bold, design: .rounded))
                .tracking(0.7)
                .foregroundStyle(.secondary)
            Spacer()
            Text(detail).font(.caption2).foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }

    private func confidenceBadge(_ confidence: VGLiftedLine.Confidence) -> some View {
        Text(confidence.rawValue.uppercased())
            .font(.system(size: 8, weight: .bold, design: .rounded))
            .foregroundStyle(confidence == .opaque ? Color.secondary : confidence == .modeled ? Color.accentColor : Color.primary.opacity(0.65))
            .padding(.horizontal, 5)
            .padding(.vertical, 2)
            .background(Color.primary.opacity(0.055), in: Capsule())
            .frame(width: 68)
    }

    @ViewBuilder private func dataSection<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title.uppercased())
                .font(.system(size: 9, weight: .bold, design: .rounded))
                .tracking(0.5)
                .foregroundStyle(.secondary)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(9)
        .background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }

    private func dataLine(_ label: String, _ value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(label).font(.caption2).foregroundStyle(.secondary).frame(width: 76, alignment: .leading)
            Text(value).font(.system(size: 9.5, design: .monospaced)).textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }

    @ViewBuilder private func selectionBackground(_ address: UInt64) -> some View {
        if activeAddress == address {
            Rectangle().fill(Color.accentColor.opacity(0.11))
        } else if debugPCValue == address {
            Rectangle().fill(Color.red.opacity(0.08))
        } else {
            Color.clear
        }
    }

    private func selectFunction(_ function: VGFunctionRecord) {
        selectedFunctionStart = function.start
        followPC = false
        selectAddress(function.start)
    }

    private func selectAddress(_ address: UInt64) {
        followPC = false
        selectedInstruction = address
        if let function = functions.first(where: { $0.contains(address) }) { selectedFunctionStart = function.start }
        onSelectAddress(String(format: "0x%016llx", address))
    }

    private func scrollAssembly(to address: UInt64?, using proxy: ScrollViewProxy) {
        guard let address else { return }
        let text = String(format: "0x%016llx", address)
        withAnimation(.easeInOut(duration: 0.16)) { proxy.scrollTo("assembly-\(text)", anchor: .center) }
    }

    private func assemblyScrollID(_ row: VGDisassemblyRow) -> String {
        if row.kind == .symbol { return "assembly-symbol-\(row.id)" }
        guard let address = VGBinaryWorkspaceParser.address(row.address) else { return "assembly-\(row.address)" }
        return "assembly-\(String(format: "0x%016llx", address))"
    }

    private func scrollPseudocode(to address: UInt64?, using proxy: ScrollViewProxy) {
        guard semanticMode == .pseudocode, let address else { return }
        withAnimation(.easeInOut(duration: 0.16)) { proxy.scrollTo("pseudo-\(String(address, radix: 16))", anchor: .center) }
    }

    private func synchronizeExternalSelection() {
        let candidate = followPC ? (debugPCValue ?? selectedAddressValue) : selectedAddressValue
        guard let candidate else {
            if selectedFunctionStart == nil { selectedFunctionStart = functions.first?.start }
            return
        }
        selectedInstruction = candidate
        if let function = functions.first(where: { $0.contains(candidate) }) { selectedFunctionStart = function.start }
    }
}
