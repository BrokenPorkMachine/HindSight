import SwiftUI

struct VGBinaryDiffWorkspace: View {
    let records: [ProjectRecord]
    @Binding var baselineID: String
    @Binding var candidateID: String
    let reports: [String: AnalysisReport]
    let status: String
    let busy: Bool
    let onAnalyzePair: () -> Void
    let onSelectAddress: (String) -> Void

    @State private var selectedDiffID = ""
    @State private var filter = "Changed"
    @State private var search = ""

    private var baselineRecord: ProjectRecord? { records.first { $0.id == baselineID } }
    private var candidateRecord: ProjectRecord? { records.first { $0.id == candidateID } }
    private var report: VGBinaryDiffReport? {
        guard let lhsRecord = baselineRecord, let rhsRecord = candidateRecord,
              let lhs = reports[baselineID], let rhs = reports[candidateID] else { return nil }
        return VGBinaryDiffEngine.compare(
            baselineName: lhsRecord.name, baseline: lhs,
            candidateName: rhsRecord.name, candidate: rhs
        )
    }

    private var visibleFunctions: [VGFunctionDiff] {
        guard let report else { return [] }
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return report.functions.filter { item in
            let statusOK: Bool
            switch filter {
            case "Changed": statusOK = item.status != .unchanged
            case "Added": statusOK = item.status == .added
            case "Removed": statusOK = item.status == .removed
            case "Semantic": statusOK = item.semanticsChanged && item.status == .changed
            case "CFG": statusOK = item.cfgChanged && item.status == .changed
            default: statusOK = true
            }
            let queryOK = query.isEmpty || item.displayName.lowercased().contains(query)
                || item.baselineAddress.lowercased().contains(query)
                || item.candidateAddress.lowercased().contains(query)
            return statusOK && queryOK
        }
    }

    private var selectedDiff: VGFunctionDiff? {
        let rows = visibleFunctions
        return rows.first { $0.id == selectedDiffID } ?? rows.first
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            if records.count < 2 {
                ContentUnavailableView(
                    "Two artifacts required",
                    systemImage: "arrow.left.arrow.right.square",
                    description: Text("Import two builds or firmware artifacts into the same project to compare them.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if report == nil {
                ContentUnavailableView(
                    "Analyze both sides",
                    systemImage: "waveform.path.ecg.rectangle",
                    description: Text("Binary Diff only compares bounded analysis evidence. Analyze the baseline and candidate; it will not infer changes from filenames or file size alone.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                summary
                Divider()
                VGAdaptiveTwoPane(breakpoint: 940) {
                    functionList
                } secondary: {
                    detail
                }
                .frame(minHeight: 520)
            }
        }
        .onChange(of: baselineID) { _, _ in selectedDiffID = "" }
        .onChange(of: candidateID) { _, _ in selectedDiffID = "" }
        .onChange(of: filter) { _, _ in selectedDiffID = "" }
    }

    private var toolbar: some View {
        VStack(spacing: 7) {
            HStack(spacing: 10) {
                Label("Binary Diff", systemImage: "arrow.left.arrow.right.square").font(.headline)
                Spacer(minLength: 8)
                Text(status).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 8) {
                    Picker("Baseline", selection: $baselineID) {
                        Text("Select baseline").tag("")
                        ForEach(records) { record in Text(record.name).tag(record.id) }
                    }.frame(minWidth: 160, idealWidth: 230, maxWidth: 300)
                    Picker("Candidate", selection: $candidateID) {
                        Text("Select candidate").tag("")
                        ForEach(records) { record in Text(record.name).tag(record.id) }
                    }.frame(minWidth: 160, idealWidth: 230, maxWidth: 300)
                    Button("Analyze Pair", action: onAnalyzePair)
                        .buttonStyle(.borderedProminent)
                        .disabled(busy || baselineID.isEmpty || candidateID.isEmpty || baselineID == candidateID)
                    if busy { ProgressView().controlSize(.small) }
                    Spacer(minLength: 0)
                }
                VStack(alignment: .leading, spacing: 7) {
                    Picker("Baseline", selection: $baselineID) {
                        Text("Select baseline").tag("")
                        ForEach(records) { record in Text(record.name).tag(record.id) }
                    }
                    Picker("Candidate", selection: $candidateID) {
                        Text("Select candidate").tag("")
                        ForEach(records) { record in Text(record.name).tag(record.id) }
                    }
                    VGFlowLayout {
                        Button("Analyze Pair", action: onAnalyzePair)
                            .buttonStyle(.borderedProminent)
                            .disabled(busy || baselineID.isEmpty || candidateID.isEmpty || baselineID == candidateID)
                        if busy { ProgressView().controlSize(.small) }
                    }
                }
            }
        }
        .padding(10)
        .background(.bar)
    }

    @ViewBuilder private var summary: some View {
        if let report {
            VGHorizontalOverflowBar {
                HStack(spacing: 10) {
                    metric("Changed", report.changedCount, "exclamationmark.triangle")
                    metric("Added", report.addedCount, "plus.circle")
                    metric("Removed", report.removedCount, "minus.circle")
                    metric("Semantic", report.semanticChangeCount, "text.badge.checkmark")
                    metric("CFG", report.cfgChangeCount, "point.3.connected.trianglepath.dotted")
                    metric("Unchanged", report.unchangedCount, "equal.circle")
                    Text("Matching: unique symbols → exact normalized code fingerprints. Weak matches remain added/removed.")
                        .font(.caption2).foregroundStyle(.secondary).frame(maxWidth: 390, alignment: .leading)
                }
                .padding(.horizontal, 10).padding(.vertical, 8)
            }
        }
    }

    private func metric(_ title: String, _ value: Int, _ icon: String) -> some View {
        HStack(spacing: 6) {
            Image(systemName: icon).foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 0) {
                Text("\(value)").font(.headline).monospacedDigit()
                Text(title).font(.caption2).foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
    }

    private var functionList: some View {
        VStack(spacing: 8) {
            HStack {
                TextField("Function or address", text: $search).textFieldStyle(.roundedBorder)
                Picker("Filter", selection: $filter) {
                    ForEach(["Changed", "All", "Added", "Removed", "Semantic", "CFG"], id: \.self, content: Text.init)
                }
                .labelsHidden().frame(width: 110)
            }
            .padding(.horizontal, 10).padding(.top, 10)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(visibleFunctions) { item in
                        Button { selectedDiffID = item.id } label: {
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    statusBadge(item.status)
                                    Text(item.displayName).font(.system(.body, design: .monospaced)).fontWeight(.semibold).lineLimit(1)
                                    Spacer()
                                }
                                Text(item.changeSummary).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                                Text("\(item.baselineAddress)  →  \(item.candidateAddress)")
                                    .font(.system(.caption2, design: .monospaced)).foregroundStyle(.tertiary)
                            }
                            .padding(7)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(item.id == selectedDiffID ? Color.accentColor.opacity(0.12) : Color.clear, in: RoundedRectangle(cornerRadius: 7))
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(8)
            }
        }
    }

    private func statusBadge(_ status: VGBinaryDiffStatus) -> some View {
        let icon: String
        switch status {
        case .unchanged: icon = "equal.circle"
        case .changed: icon = "exclamationmark.triangle.fill"
        case .added: icon = "plus.circle.fill"
        case .removed: icon = "minus.circle.fill"
        }
        return Image(systemName: icon).foregroundStyle(status == .unchanged ? .secondary : .primary)
    }

    @ViewBuilder private var detail: some View {
        if let item = selectedDiff {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    HStack(alignment: .top) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.displayName).font(.title3).fontWeight(.semibold)
                            Text(item.changeSummary).foregroundStyle(.secondary)
                            Text("Match evidence: \(item.matchReason)").font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if let baseline = item.baseline {
                            Button("Baseline") { onSelectAddress(baseline.startText) }
                        }
                        if let candidate = item.candidate {
                            Button("Candidate") { onSelectAddress(candidate.startText) }
                        }
                    }
                    changeMatrix(item)
                    sideBySide(item)
                }
                .padding(14)
            }
        } else {
            ContentUnavailableView("No diff row", systemImage: "arrow.left.arrow.right", description: Text("Choose a function change to inspect its evidence."))
        }
    }

    private func changeMatrix(_ item: VGFunctionDiff) -> some View {
        GroupBox("Evidence comparison") {
            Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 7) {
                GridRow { Text("Dimension").bold(); Text("Baseline").bold(); Text("Candidate").bold(); Text("Changed").bold() }
                row("Instructions", item.baselineInstructionCount, item.candidateInstructionCount, item.codeChanged)
                row("CFG blocks", item.baselineBlockCount, item.candidateBlockCount, item.cfgChanged)
                row("References", item.baselineReferenceCount, item.candidateReferenceCount, item.referencesChanged)
                row("Lifted lines", item.baselineLiftedCount, item.candidateLiftedCount, item.semanticsChanged)
            }
            .padding(6)
        }
    }

    private func row(_ title: String, _ lhs: Int, _ rhs: Int, _ changed: Bool) -> some View {
        GridRow {
            Text(title)
            Text("\(lhs)").monospacedDigit()
            Text("\(rhs)").monospacedDigit()
            Label(changed ? "Yes" : "No", systemImage: changed ? "exclamationmark.triangle" : "equal.circle")
                .foregroundStyle(changed ? .primary : .secondary)
        }
    }

    private func sideBySide(_ item: VGFunctionDiff) -> some View {
        VGAdaptiveTwoPane(breakpoint: 900) {
            codeEvidence(title: baselineRecord?.name ?? "Baseline", function: item.baseline, analysis: reports[baselineID])
        } secondary: {
            codeEvidence(title: candidateRecord?.name ?? "Candidate", function: item.candidate, analysis: reports[candidateID])
        }
        .frame(minHeight: 360)
    }

    @ViewBuilder private func codeEvidence(title: String, function: VGFunctionRecord?, analysis: AnalysisReport?) -> some View {
        GroupBox(title) {
            if let function, let analysis {
                let rows = VGDisassemblyParser.parse(analysis.annotated_disassembly).filter { row in
                    guard row.kind == .instruction, let address = VGGraphParser.address(from: row.address.lowercased().hasPrefix("0x") ? row.address : "0x" + row.address) else { return false }
                    return function.contains(address)
                }
                VStack(alignment: .leading, spacing: 8) {
                    Text("\(function.startText) • \(function.extentLabel)").font(.caption).foregroundStyle(.secondary)
                    ScrollView([.vertical, .horizontal]) {
                        LazyVStack(alignment: .leading, spacing: 2) {
                            ForEach(rows.prefix(600)) { row in
                                HStack(spacing: 10) {
                                    Text(row.address).foregroundStyle(.secondary).frame(width: 145, alignment: .leading)
                                    Text(row.word).foregroundStyle(.tertiary).frame(width: 74, alignment: .leading)
                                    Text(row.mnemonic).fontWeight(.semibold).frame(width: 58, alignment: .leading)
                                    Text(row.operands)
                                    if !row.annotation.isEmpty { Text(row.annotation).foregroundStyle(.secondary) }
                                }
                                .font(.system(.caption, design: .monospaced))
                                .contentShape(Rectangle())
                                .onTapGesture { onSelectAddress(row.address) }
                            }
                        }
                        .textSelection(.enabled)
                    }
                }
                .padding(6)
            } else {
                Text(function == nil ? "Function absent on this side." : "Analysis unavailable.")
                    .foregroundStyle(.secondary).padding(8)
            }
        }
    }
}
