import SwiftUI

private enum VGRuntimeCenterMode: String, CaseIterable, Identifiable {
    case code = "Code"
    case live = "Live"
    case split = "Split"
    var id: String { rawValue }
}

struct VGRuntimeCorrelationWorkspace: View {
    let binaryPath: String
    let functionsText: String
    let annotatedDisassembly: String
    let modulesText: String
    let liveDisassembly: String
    let samples: [VGRuntimeSample]
    let memoryObservations: [VGRuntimeMemoryObservation]
    let selectedAddress: String
    let attached: Bool
    let onSelectStaticAddress: (String) -> Void
    let onStep: () -> Void
    let onContinue: () -> Void
    let onInterrupt: () -> Void
    let onReadMemory: (String) -> Void
    let onRefreshModules: () -> Void

    @State private var selectedSampleID: Int?
    @State private var centerMode: VGRuntimeCenterMode = .code
    @State private var showTimeline = true
    @State private var showState = true

    private var functions: [VGFunctionRecord] { VGGraphParser.functions(functionsText) }
    private var rows: [VGDisassemblyRow] { VGDisassemblyParser.parse(annotatedDisassembly) }
    private var heat: [UInt64: Int] {
        VGRuntimeCorrelationEngine.heat(samples: samples, binaryPath: binaryPath, modulesText: modulesText)
    }
    private var selectedSample: VGRuntimeSample? {
        if let selectedSampleID, let sample = samples.first(where: { $0.id == selectedSampleID }) { return sample }
        return samples.last
    }
    private var correlation: VGRuntimeCorrelation? {
        selectedSample.map { VGRuntimeCorrelationEngine.correlate($0, binaryPath: binaryPath, modulesText: modulesText, functions: functions) }
    }
    private var selectedFunction: VGFunctionRecord? {
        guard let address = correlation?.preferredPC else { return nil }
        return functions.first(where: { $0.contains(address) })
    }
    private var codeRows: [VGDisassemblyRow] {
        if let function = selectedFunction {
            return rows.filter { row in
                guard row.kind == .instruction, let address = VGTargetUtilities.parseHex(row.address) else { return false }
                return function.contains(address)
            }
        }
        guard let address = correlation?.preferredPC else { return Array(rows.filter { $0.kind == .instruction }.prefix(96)) }
        let instructions = rows.filter { $0.kind == .instruction }
        guard let index = instructions.firstIndex(where: { VGTargetUtilities.parseHex($0.address) == address }) else { return Array(instructions.prefix(96)) }
        let lower = max(0, index - 40)
        let upper = min(instructions.count, index + 56)
        return Array(instructions[lower..<upper])
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            correlationBanner
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Stops",
                trailingTitle: "Runtime State",
                leadingVisible: showTimeline,
                trailingVisible: showState,
                leadingIdealWidth: 245,
                trailingIdealWidth: 320
            ) {
                timelinePane
            } center: {
                centerPane
            } trailing: {
                statePane
            }
        }
        .background(Color.primary.opacity(0.012))
        .onAppear {
            if selectedSampleID == nil { selectedSampleID = samples.last?.id }
        }
        .onChange(of: samples.count) { _, _ in
            if let last = samples.last { selectedSampleID = last.id }
        }
    }

    private var toolbar: some View {
        VStack(spacing: 6) {
            HStack(spacing: 9) {
                Label("Runtime Correlation", systemImage: "point.3.filled.connected.trianglepath.dotted")
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                Spacer(minLength: 8)
                Text(samples.isEmpty ? "No runtime observations" : "\(samples.count) stop observations")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            VGFlowLayout {
                Button { showTimeline.toggle() } label: { Label("Stops", systemImage: "sidebar.left") }
                    .buttonStyle(.borderless).help(showTimeline ? "Hide stop history" : "Show stop history")
                Button { showState.toggle() } label: { Label("State", systemImage: "sidebar.right") }
                    .buttonStyle(.borderless).help(showState ? "Hide runtime state" : "Show runtime state")
                Picker("Center", selection: $centerMode) { ForEach(VGRuntimeCenterMode.allCases) { Text($0.rawValue).tag($0) } }
                    .labelsHidden().pickerStyle(.segmented).frame(width: 180)
                Button("Step", action: onStep).disabled(!attached)
                Button("Continue", action: onContinue).disabled(!attached)
                Button("Interrupt", action: onInterrupt).disabled(!attached)
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 8).background(.bar)
    }

    @ViewBuilder private var correlationBanner: some View {
        if let sample = selectedSample, let correlation {
            VGHorizontalOverflowBar {
                HStack(spacing: 8) {
                    runtimePill("RUN", value: sample.pcText, icon: "bolt.fill")
                    runtimePill("STOP", value: sample.event, icon: sample.stopped ? "pause.circle.fill" : "play.circle")
                    Image(systemName: "arrow.right").font(.caption).foregroundStyle(.tertiary)
                    runtimePill("STATIC", value: correlation.preferredText, icon: correlation.preferredPC == nil ? "questionmark" : "doc.text")
                    runtimePill("FUNCTION", value: correlation.functionName ?? "unresolved", icon: "function")
                    if let modulePath = correlation.modulePath {
                        runtimePill("IMAGE", value: URL(fileURLWithPath: modulePath).lastPathComponent, icon: "shippingbox")
                    }
                    if let slide = correlation.slide { runtimePill("SLIDE", value: String(format: "%+lld", slide), icon: "arrow.left.and.right") }
                    Text(correlation.confidence.uppercased())
                        .font(.system(size: 9, weight: .bold, design: .rounded))
                        .padding(.horizontal, 7).padding(.vertical, 4)
                        .background(Capsule().fill(correlation.preferredPC == nil ? Color.secondary.opacity(0.12) : Color.accentColor.opacity(0.14)))
                    if !sample.reason.isEmpty {
                        Text(sample.reason)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
                .padding(.horizontal, 12).padding(.vertical, 7)
            }
            .background(Color.primary.opacity(0.025))
            .help(correlation.explanation)
        } else {
            HStack {
                Image(systemName: "info.circle")
                Text("Attach the native debugger and stop execution to correlate runtime state with the analyzed binary.")
                Spacer()
                Button("Refresh Images", action: onRefreshModules).disabled(!attached)
            }
            .font(.caption).foregroundStyle(.secondary)
            .padding(.horizontal, 12).padding(.vertical, 8)
        }
    }

    private func runtimePill(_ label: String, value: String, icon: String) -> some View {
        HStack(spacing: 5) {
            Image(systemName: icon).font(.system(size: 9, weight: .semibold))
            Text(label).font(.system(size: 8, weight: .bold, design: .rounded)).foregroundStyle(.secondary)
            Text(value).font(.system(size: 10, weight: .medium, design: .monospaced)).lineLimit(1)
        }
        .padding(.horizontal, 7).padding(.vertical, 4)
        .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Color.primary.opacity(0.055)))
    }

    private var timelinePane: some View {
        VStack(spacing: 0) {
            paneTitle("STOP HISTORY", detail: "observed debugger snapshots", icon: "clock.arrow.circlepath")
            Divider()
            if samples.isEmpty {
                ContentUnavailableView("No stops captured", systemImage: "pause.circle", description: Text("Step, break, or interrupt a debug target to populate runtime evidence."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 3) {
                        ForEach(samples.reversed()) { sample in
                            let corr = VGRuntimeCorrelationEngine.correlate(sample, binaryPath: binaryPath, modulesText: modulesText, functions: functions)
                            Button {
                                selectedSampleID = sample.id
                                if let address = corr.preferredPC { onSelectStaticAddress(VGTargetUtilities.hex(address)) }
                            } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    HStack(spacing: 6) {
                                        Circle().fill(sample.stopped ? Color.orange : Color.secondary).frame(width: 6, height: 6)
                                        Text("#\(sample.id)").font(.system(size: 9, weight: .bold, design: .monospaced))
                                        Text(sample.event).font(.caption2).foregroundStyle(.secondary)
                                        Spacer()
                                        Text("Δ\(sample.changedRegisters.count)").font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                                    }
                                    Text(sample.pcText).font(.system(size: 10, weight: .medium, design: .monospaced))
                                    if !sample.reason.isEmpty {
                                        Text(sample.reason).font(.system(size: 9, design: .monospaced)).foregroundStyle(.tertiary).lineLimit(1)
                                    }
                                    VStack(alignment: .leading, spacing: 1) {
                                        Text(corr.functionName ?? "Unresolved function")
                                        if let modulePath = corr.modulePath {
                                            Text(URL(fileURLWithPath: modulePath).lastPathComponent)
                                                .foregroundStyle(.tertiary)
                                        }
                                    }
                                        .font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                                }
                                .padding(.horizontal, 8).padding(.vertical, 7)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background {
                                    if selectedSample?.id == sample.id {
                                        RoundedRectangle(cornerRadius: 6).fill(Color.accentColor.opacity(0.11))
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
        .frame(minWidth: 210, idealWidth: 245, maxWidth: 310)
        .background(.thinMaterial)
    }

    @ViewBuilder private var centerPane: some View {
        switch centerMode {
        case .code: staticCodePane
        case .live: liveCodePane
        case .split:
            VSplitView {
                staticCodePane.frame(minHeight: 280)
                liveCodePane.frame(minHeight: 210)
            }
        }
    }

    private var staticCodePane: some View {
        VStack(spacing: 0) {
            paneTitle("STATIC ↔ RUNTIME CODE", detail: selectedFunction?.name ?? "bounded static window", icon: "arrow.triangle.2.circlepath")
            Divider()
            if codeRows.isEmpty {
                ContentUnavailableView("No correlated static code", systemImage: "doc.text.magnifyingglass", description: Text(correlation?.explanation ?? "Run binary analysis and stop the debug target."))
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(codeRows) { row in staticInstructionRow(row) }
                    }
                    .padding(.vertical, 4)
                }
            }
        }
    }

    private func staticInstructionRow(_ row: VGDisassemblyRow) -> some View {
        let address = VGTargetUtilities.parseHex(row.address)
        let current = address == correlation?.preferredPC
        let hits = address.flatMap { heat[$0] } ?? 0
        return Button {
            if let address { onSelectStaticAddress(VGTargetUtilities.hex(address)) }
        } label: {
            HStack(spacing: 8) {
                Rectangle().fill(current ? Color.orange : Color.clear).frame(width: 3, height: 24)
                Text(row.address).font(.system(size: 10, weight: current ? .bold : .regular, design: .monospaced)).frame(width: 142, alignment: .trailing)
                Text(row.word).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary).frame(width: 72, alignment: .leading)
                Text(row.mnemonic).font(.system(size: 11, weight: .semibold, design: .monospaced)).frame(width: 62, alignment: .leading)
                Text(row.operands).font(.system(size: 11, design: .monospaced)).lineLimit(1)
                Spacer(minLength: 8)
                if hits > 0 {
                    Text("×\(hits)").font(.system(size: 9, weight: .bold, design: .monospaced))
                        .padding(.horizontal, 5).padding(.vertical, 2)
                        .background(Capsule().fill(Color.orange.opacity(min(0.32, 0.08 + Double(hits) * 0.035))))
                        .help("Observed at this static address in \(hits) captured debugger snapshot(s).")
                }
            }
            .padding(.horizontal, 8).padding(.vertical, 2)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(current ? Color.orange.opacity(0.09) : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private var liveCodePane: some View {
        VStack(spacing: 0) {
            paneTitle("LIVE DISASSEMBLY", detail: "bytes read from stopped process", icon: "bolt.horizontal")
            Divider()
            ScrollView {
                Text(liveDisassembly.isEmpty ? "No live disassembly. Stop the target to read code around PC." : liveDisassembly)
                    .font(.system(size: 10.5, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    .padding(10)
            }
        }
    }

    private var statePane: some View {
        VStack(spacing: 0) {
            paneTitle("RUNTIME STATE", detail: "deltas and observations", icon: "waveform.path.ecg")
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    registerDeltas
                    breakpoints
                    memoryHistory
                    moduleEvidence
                }
                .padding(10)
            }
        }
        .frame(minWidth: 255, idealWidth: 300, maxWidth: 390)
        .background(.thinMaterial)
    }

    @ViewBuilder private var registerDeltas: some View {
        VStack(alignment: .leading, spacing: 6) {
            sectionLabel("REGISTER DELTAS")
            if let sample = selectedSample, !sample.changedRegisters.isEmpty {
                ForEach(sample.changedRegisters) { delta in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack { Text(delta.name).fontWeight(.semibold); Spacer(); Text(delta.after) }
                            .font(.system(size: 10, design: .monospaced))
                        if let before = delta.before {
                            Text("from \(before)").font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                        }
                    }
                    .padding(6).background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(0.04)))
                }
            } else { Text("No changed registers in this observation.").font(.caption).foregroundStyle(.secondary) }
        }
    }

    @ViewBuilder private var breakpoints: some View {
        VStack(alignment: .leading, spacing: 6) {
            sectionLabel("BREAKPOINTS")
            if let sample = selectedSample, !sample.breakpoints.isEmpty {
                ForEach(sample.breakpoints, id: \.self) { address in
                    Text(VGTargetUtilities.hex(address)).font(.system(size: 10, design: .monospaced))
                }
            } else { Text("No software breakpoints in the selected snapshot.").font(.caption).foregroundStyle(.secondary) }
        }
    }

    @ViewBuilder private var memoryHistory: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack { sectionLabel("MEMORY OBSERVATIONS"); Spacer(); if let sample = selectedSample { Button("Read SP") { if let sp = sample.sp { onReadMemory(VGTargetUtilities.hex(sp)) } }.controlSize(.small) } }
            if memoryObservations.isEmpty {
                Text("Memory reads made during this session will appear here as explicit observations.").font(.caption).foregroundStyle(.secondary)
            } else {
                ForEach(memoryObservations.suffix(12).reversed()) { item in
                    Button { onReadMemory(item.addressText) } label: {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("\(item.addressText) • \(item.bytes.count) B").font(.system(size: 9, weight: .semibold, design: .monospaced))
                            Text(item.preview).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary).lineLimit(2)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }.buttonStyle(.plain)
                }
            }
        }
    }

    @ViewBuilder private var moduleEvidence: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack { sectionLabel("MODULE EVIDENCE"); Spacer(); Button("Refresh", action: onRefreshModules).controlSize(.small).disabled(!attached) }
            if let correlation, let path = correlation.modulePath {
                Text(path).font(.system(size: 9, design: .monospaced)).textSelection(.enabled)
                Text(correlation.explanation).font(.caption2).foregroundStyle(.secondary)
            } else {
                Text("A unique loaded-image match is required before HindSight translates runtime addresses into static preferred addresses.").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func paneTitle(_ title: String, detail: String, icon: String) -> some View {
        HStack(spacing: 7) {
            Image(systemName: icon).font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
            Text(title).font(.system(size: 9, weight: .bold, design: .rounded)).tracking(0.65)
            Text(detail).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
            Spacer()
        }
        .padding(.horizontal, 10).padding(.vertical, 7).background(Color.primary.opacity(0.025))
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text).font(.system(size: 8.5, weight: .bold, design: .rounded)).tracking(0.7).foregroundStyle(.secondary)
    }
}
