import SwiftUI

struct VGTraceExplorerWorkspace: View {
    let binaryPath: String
    let functionsText: String
    let annotatedDisassembly: String
    let modulesText: String
    let samples: [VGRuntimeSample]
    let memoryObservations: [VGRuntimeMemoryObservation]
    let attached: Bool
    let onSelectStaticAddress: (String) -> Void
    let onStep: () -> Void
    let onContinue: () -> Void
    let onInterrupt: () -> Void
    let onOpenCorrelation: () -> Void

    @State private var selectedEventID: Int?
    @State private var filter = ""
    @State private var showRegisterBand = true
    @State private var showMemoryBand = true

    private var functions: [VGFunctionRecord] { VGGraphParser.functions(functionsText) }
    private var rows: [VGDisassemblyRow] { VGDisassemblyParser.parse(annotatedDisassembly) }
    private var events: [VGTraceEvent] {
        VGTraceExplorerEngine.events(samples: samples, binaryPath: binaryPath, modulesText: modulesText, functions: functions, disassembly: rows)
    }
    private var filteredEvents: [VGTraceEvent] {
        let q = filter.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !q.isEmpty else { return events }
        return events.filter { event in
            event.runtimeText.lowercased().contains(q)
                || event.preferredText.lowercased().contains(q)
                || (event.functionName?.lowercased().contains(q) ?? false)
                || event.reason.lowercased().contains(q)
                || event.transition.rawValue.lowercased().contains(q)
        }
    }
    private var selectedEvent: VGTraceEvent? {
        if let selectedEventID, let event = events.first(where: { $0.id == selectedEventID }) { return event }
        return events.last
    }
    private var summary: VGTraceSessionSummary { VGTraceExplorerEngine.summary(events: events, memoryObservations: memoryObservations) }
    private var functionSummary: [VGTraceFunctionSummary] { VGTraceExplorerEngine.functionSummary(events) }
    private var registerLanes: [VGTraceRegisterLane] { VGTraceExplorerEngine.registerLanes(events) }
    private var nearbyMemory: [VGRuntimeMemoryObservation] { VGTraceExplorerEngine.nearbyMemory(memoryObservations, event: selectedEvent) }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            metrics
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Timeline",
                trailingTitle: "Evidence",
                leadingIdealWidth: 360,
                trailingIdealWidth: 320,
                wideBreakpoint: 1240,
                compactBreakpoint: 800
            ) {
                timeline
            } center: {
                eventInspector
            } trailing: {
                evidenceBands
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear { if selectedEventID == nil { selectedEventID = events.last?.id } }
        .onChange(of: samples.count) { _, _ in
            if selectedEventID == nil || selectedEventID == events.dropLast().last?.id { selectedEventID = events.last?.id }
        }
    }

    private var toolbar: some View {
        VStack(spacing: 7) {
            HStack(spacing: 10) {
                Label("Trace Explorer", systemImage: "point.topleft.down.to.point.bottomright.curvepath")
                    .font(.headline)
                TextField("Filter address, function, reason…", text: $filter)
                    .textFieldStyle(.roundedBorder)
                    .frame(minWidth: 180, idealWidth: 300, maxWidth: 420)
                Spacer(minLength: 0)
            }
            VGFlowLayout {
                Toggle("Registers", isOn: $showRegisterBand).toggleStyle(.button)
                Toggle("Memory", isOn: $showMemoryBand).toggleStyle(.button)
                Button("Correlation", action: onOpenCorrelation)
                Button("Step", action: onStep).disabled(!attached)
                Button("Continue", action: onContinue).disabled(!attached)
                Button("Interrupt", action: onInterrupt).disabled(!attached)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }

    private var metrics: some View {
        VGHorizontalOverflowBar {
            HStack(spacing: 18) {
                metric("OBS", "\(summary.eventCount)")
                metric("CORRELATED", "\(summary.correlatedCount)")
                metric("UNRESOLVED", "\(summary.unresolvedCount)")
                metric("STATIC PCs", "\(summary.uniqueStaticPCs)")
                metric("FUNCTIONS", "\(summary.functionCount)")
                metric("REG Δ", "\(summary.registerChangeCount)")
                metric("MEM", "\(summary.memoryObservationCount)")
                Text("Debugger observations ≠ full instruction trace")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
        .background(.thinMaterial)
    }

    private func metric(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label).font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            Text(value).font(.system(size: 13, weight: .semibold, design: .monospaced))
        }
    }

    private var timeline: some View {
        VStack(spacing: 0) {
            HStack {
                Text("OBSERVED TIMELINE").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                Spacer()
                Text("\(filteredEvents.count)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
            }
            .padding(10)
            Divider()
            if filteredEvents.isEmpty {
                ContentUnavailableView("No debugger observations", systemImage: "timeline.selection", description: Text("Attach or launch a target, then step/stop execution to populate the bounded runtime history."))
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 2) {
                        ForEach(filteredEvents) { event in
                            Button { selectedEventID = event.id } label: {
                                traceRow(event)
                                    .padding(7)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(event.id == selectedEventID ? Color.accentColor.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 7))
                            }
                            .buttonStyle(.plain)
                            .contextMenu {
                                if let preferred = event.preferredPC {
                                    Button("Open static address") { onSelectStaticAddress(VGTargetUtilities.hex(preferred)) }
                                }
                            }
                        }
                    }
                    .padding(8)
                }
            }
        }
    }

    private func traceRow(_ event: VGTraceEvent) -> some View {
        HStack(alignment: .top, spacing: 8) {
            VStack(spacing: 2) {
                Circle().fill(transitionColor(event.transition)).frame(width: 8, height: 8)
                Rectangle().fill(.secondary.opacity(0.2)).frame(width: 1, height: 30)
            }
            VStack(alignment: .leading, spacing: 3) {
                HStack {
                    Text("#\(event.sequence)").font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(.secondary)
                    Text(event.preferredText).font(.system(size: 10, weight: .semibold, design: .monospaced))
                    Spacer()
                    Text(event.timeText).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                }
                Text(event.functionName ?? event.transition.rawValue)
                    .font(.system(size: 11, weight: .medium))
                    .lineLimit(1)
                HStack(spacing: 6) {
                    Text(event.transition.rawValue).font(.system(size: 8, weight: .bold)).foregroundStyle(transitionColor(event.transition))
                    if !event.changedRegisters.isEmpty {
                        Text("Δ\(event.changedRegisters.count)").font(.system(size: 8, design: .monospaced)).foregroundStyle(.secondary)
                    }
                    if !event.reason.isEmpty { Text(event.reason).font(.system(size: 8)).foregroundStyle(.secondary).lineLimit(1) }
                }
            }
        }
        .padding(.vertical, 3)
    }

    private var eventInspector: some View {
        VStack(spacing: 0) {
            HStack {
                Text("EVENT EVIDENCE").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                Spacer()
                if let event = selectedEvent, let preferred = event.preferredPC {
                    Button("Open \(VGTargetUtilities.hex(preferred))") { onSelectStaticAddress(VGTargetUtilities.hex(preferred)) }
                        .buttonStyle(.borderless)
                }
            }
            .padding(10)
            Divider()
            if let event = selectedEvent {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        eventCard(event)
                        codeContext(event)
                        functionPath
                    }
                    .padding(12)
                }
            } else {
                ContentUnavailableView("Select an event", systemImage: "cursorarrow.click.2")
            }
        }
    }

    private func eventCard(_ event: VGTraceEvent) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(event.functionName ?? "Unresolved function").font(.title3.weight(.semibold))
                Spacer()
                Text(event.confidence.uppercased()).font(.system(size: 8, weight: .bold)).padding(.horizontal, 6).padding(.vertical, 3).background(.thinMaterial, in: Capsule())
            }
            Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 5) {
                GridRow { label("runtime"); value(event.runtimeText) }
                GridRow { label("static"); value(event.preferredText) }
                GridRow { label("thread"); value(event.threadID ?? "unknown") }
                GridRow { label("transition"); value(event.transition.rawValue) }
                GridRow { label("source"); value([event.sourceMnemonic, event.sourceOperands].compactMap { $0 }.joined(separator: " ").isEmpty ? "not established" : [event.sourceMnemonic, event.sourceOperands].compactMap { $0 }.joined(separator: " ")) }
            }
            Text(event.explanation).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
        }
        .padding(12)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
    }

    private func label(_ text: String) -> some View { Text(text.uppercased()).font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary) }
    private func value(_ text: String) -> some View { Text(text).font(.system(size: 10, design: .monospaced)).textSelection(.enabled) }

    private func codeContext(_ event: VGTraceEvent) -> some View {
        let instructions = rows.filter { $0.kind == .instruction }
        let index = event.preferredPC.flatMap { address in instructions.firstIndex(where: { VGTargetUtilities.parseHex($0.address) == address }) }
        let slice: ArraySlice<VGDisassemblyRow>
        if let index {
            let lower = max(0, index - 5)
            let upper = min(instructions.count, index + 6)
            slice = instructions[lower..<upper]
        } else {
            slice = instructions.prefix(0)
        }
        return VStack(alignment: .leading, spacing: 6) {
            Text("STATIC CONTEXT").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
            if slice.isEmpty {
                Text("No uniquely correlated static instruction context for this observation.").font(.caption).foregroundStyle(.secondary)
            } else {
                ForEach(Array(slice)) { row in
                    HStack(spacing: 10) {
                        Text(row.address).font(.system(size: 9, design: .monospaced)).frame(width: 142, alignment: .leading)
                        Text(row.mnemonic).font(.system(size: 10, weight: .semibold, design: .monospaced)).frame(width: 48, alignment: .leading)
                        Text(row.operands).font(.system(size: 10, design: .monospaced)).lineLimit(1)
                        Spacer()
                    }
                    .padding(.vertical, 2)
                    .padding(.horizontal, 6)
                    .background(VGTargetUtilities.parseHex(row.address) == event.preferredPC ? Color.accentColor.opacity(0.14) : Color.clear, in: RoundedRectangle(cornerRadius: 4))
                }
            }
        }
        .padding(10)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 10))
    }

    private var functionPath: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("OBSERVED FUNCTION PATH").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
            ForEach(functionSummary.prefix(12)) { item in
                HStack {
                    Text(item.name).font(.system(size: 10, weight: .medium)).lineLimit(1)
                    Spacer()
                    Text("\(item.observations) obs").font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                    Button(item.addressText) { onSelectStaticAddress(item.addressText) }.buttonStyle(.borderless).font(.system(size: 9, design: .monospaced))
                }
            }
            if functionSummary.isEmpty { Text("No correlated function observations yet.").font(.caption).foregroundStyle(.secondary) }
        }
        .padding(10)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 10))
    }

    private var evidenceBands: some View {
        VStack(spacing: 0) {
            if showRegisterBand {
                evidenceSection("REGISTER CHANGE BAND") {
                    if registerLanes.isEmpty {
                        Text("No changed registers captured.").font(.caption).foregroundStyle(.secondary)
                    } else {
                        ForEach(registerLanes.prefix(18)) { lane in
                            HStack {
                                Text(lane.name).font(.system(size: 10, weight: .semibold, design: .monospaced)).frame(width: 54, alignment: .leading)
                                ProgressView(value: Double(lane.changes), total: Double(max(1, registerLanes.first?.changes ?? 1))).frame(maxWidth: .infinity)
                                Text("\(lane.changes)").font(.system(size: 9, design: .monospaced)).frame(width: 26, alignment: .trailing)
                            }
                            Text(lane.latestValue).font(.system(size: 8, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                }
            }
            if showMemoryBand {
                Divider()
                evidenceSection("NEARBY MEMORY OBSERVATIONS") {
                    if nearbyMemory.isEmpty {
                        Text("No explicit memory reads near this observation.").font(.caption).foregroundStyle(.secondary)
                    } else {
                        ForEach(nearbyMemory) { item in
                            VStack(alignment: .leading, spacing: 2) {
                                Text("\(item.addressText) • \(item.bytes.count) B").font(.system(size: 9, weight: .semibold, design: .monospaced))
                                Text(item.preview).font(.system(size: 8, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                            }.padding(.vertical, 4)
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
    }

    private func evidenceSection<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                Text(title).font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                content()
            }
            .padding(10)
        }
        .frame(maxHeight: .infinity)
    }

    private func transitionColor(_ kind: VGTraceEvent.TransitionKind) -> Color {
        switch kind {
        case .origin: return .secondary
        case .sequential: return .green
        case .callSite: return .blue
        case .branchSite: return .orange
        case .returnSite: return .purple
        case .observedTransfer: return .yellow
        case .unresolved: return .red
        }
    }
}
