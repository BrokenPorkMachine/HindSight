import Foundation
import SwiftUI

enum VGAnalysisEvidenceKind: String, CaseIterable, Hashable, Sendable {
    case synthesis = "Synthesis"
    case quality = "Quality"
    case function = "Function"
    case controlFlow = "Control Flow"
    case data = "Data"
    case linkage = "Linkage"
    case runtime = "Runtime"
    case semantic = "Semantic"
    case other = "Evidence"

    var icon: String {
        switch self {
        case .synthesis: return "sparkles.rectangle.stack"
        case .quality: return "checkmark.seal.text.page"
        case .function: return "function"
        case .controlFlow: return "point.3.connected.trianglepath.dotted"
        case .data: return "arrow.turn.down.right"
        case .linkage: return "link"
        case .runtime: return "bolt.horizontal.circle"
        case .semantic: return "text.badge.checkmark"
        case .other: return "doc.text.magnifyingglass"
        }
    }
}

struct VGAnalysisEvidenceSource: Hashable, Sendable {
    let name: String
    let workspace: String
    let kind: VGAnalysisEvidenceKind
    let text: String
    let focusPriority: Int
    let focusLimit: Int
}

struct VGAnalysisEvidenceHit: Identifiable, Hashable, Sendable {
    let id: String
    let source: String
    let workspace: String
    let kind: VGAnalysisEvidenceKind
    let text: String
    let address: UInt64?
    let symbol: String?
    let ordinal: Int
    let focusPriority: Int

    var addressLabel: String? {
        address.map { String(format: "0x%llx", $0) }
    }
}

enum VGAnalysisHitDestination {
    case source
    case disassembly
    case cfg
    case selectedSymbol
}

enum VGAnalysisNavigation {
    private static let maxLineBytes = 16 * 1024

    static func sources(_ report: AnalysisReport) -> [VGAnalysisEvidenceSource] {
        [
            .init(name: "Analysis Insights", workspace: "Analysis Insights", kind: .synthesis, text: report.analysis_insights, focusPriority: 100, focusLimit: 48),
            .init(name: "Analysis Quality", workspace: "Analysis Quality", kind: .quality, text: report.analysis_quality, focusPriority: 95, focusLimit: 28),
            .init(name: "Function Intelligence", workspace: "Function Intelligence", kind: .function, text: report.function_intelligence, focusPriority: 90, focusLimit: 48),
            .init(name: "Function Neighborhoods", workspace: "Function Neighborhoods", kind: .function, text: report.function_neighborhoods, focusPriority: 82, focusLimit: 36),
            .init(name: "Symbol Callers", workspace: "Symbol Callers", kind: .linkage, text: report.symbol_callers, focusPriority: 82, focusLimit: 32),
            .init(name: "Data References", workspace: "Data References", kind: .data, text: report.data_references, focusPriority: 80, focusLimit: 40),
            .init(name: "Indirect Calls", workspace: "Indirect Calls", kind: .controlFlow, text: report.indirect_calls, focusPriority: 78, focusLimit: 32),
            .init(name: "Tail-Call Candidates", workspace: "Tail-Call Candidates", kind: .controlFlow, text: report.tail_call_candidates, focusPriority: 76, focusLimit: 28),
            .init(name: "Import/Export Collisions", workspace: "Import/Export Collisions", kind: .linkage, text: report.import_export_collisions, focusPriority: 74, focusLimit: 24),
            .init(name: "Functions", workspace: "Functions", kind: .function, text: report.functions, focusPriority: 62, focusLimit: 24),
            .init(name: "Xrefs", workspace: "Xrefs", kind: .controlFlow, text: report.xrefs, focusPriority: 64, focusLimit: 28),
            .init(name: "Calls", workspace: "Call Graph", kind: .controlFlow, text: report.call_graph, focusPriority: 64, focusLimit: 28),
            .init(name: "Strings", workspace: "Strings", kind: .data, text: report.strings, focusPriority: 56, focusLimit: 24),
            .init(name: "Relocations", workspace: "Relocations", kind: .linkage, text: report.relocations, focusPriority: 58, focusLimit: 24),
            .init(name: "Jump Tables", workspace: "Jump Tables", kind: .controlFlow, text: report.jump_tables, focusPriority: 58, focusLimit: 24),
            .init(name: "Metadata", workspace: "Metadata", kind: .semantic, text: report.metadata, focusPriority: 48, focusLimit: 20),
            .init(name: "Runtime Records", workspace: "Runtime Records", kind: .runtime, text: report.runtime_records, focusPriority: 56, focusLimit: 24),
            .init(name: "Annotated Code", workspace: "Annotated Code", kind: .controlFlow, text: report.annotated_disassembly, focusPriority: 52, focusLimit: 20),
            .init(name: "SSA", workspace: "SSA", kind: .controlFlow, text: report.ssa, focusPriority: 42, focusLimit: 16),
            .init(name: "Dominators", workspace: "Dominators", kind: .controlFlow, text: report.dominators, focusPriority: 42, focusLimit: 16),
            .init(name: "Dominance Frontiers", workspace: "Dominance Frontiers", kind: .controlFlow, text: report.dominance_frontiers, focusPriority: 42, focusLimit: 16),
            .init(name: "Phi", workspace: "Phi Nodes", kind: .controlFlow, text: report.phi_nodes, focusPriority: 40, focusLimit: 16),
            .init(name: "Runtime Graph", workspace: "Runtime Graph", kind: .runtime, text: report.runtime_graph, focusPriority: 52, focusLimit: 20),
            .init(name: "Swift Types", workspace: "Swift Types", kind: .semantic, text: report.swift_types, focusPriority: 54, focusLimit: 20),
            .init(name: "Selector Xrefs", workspace: "Selector Xrefs", kind: .semantic, text: report.selector_xrefs, focusPriority: 58, focusLimit: 20),
            .init(name: "ObjC Classes", workspace: "ObjC Classes", kind: .semantic, text: report.objc_classes, focusPriority: 54, focusLimit: 20),
            .init(name: "Swift Contexts", workspace: "Swift Contexts", kind: .semantic, text: report.swift_contexts, focusPriority: 54, focusLimit: 20),
            .init(name: "ObjC Protocols", workspace: "ObjC Protocols", kind: .semantic, text: report.objc_protocols, focusPriority: 52, focusLimit: 20),
            .init(name: "Swift Fields", workspace: "Swift Fields", kind: .semantic, text: report.swift_fields, focusPriority: 52, focusLimit: 20),
            .init(name: "Chained Fixups", workspace: "Chained Fixups", kind: .linkage, text: report.chained_fixups, focusPriority: 54, focusLimit: 20),
            .init(name: "Chained Pointers", workspace: "Chained Pointers", kind: .linkage, text: report.chained_pointers, focusPriority: 54, focusLimit: 20),
            .init(name: "ARM64e Pointers", workspace: "ARM64e Pointers", kind: .linkage, text: report.arm64e_pointers, focusPriority: 56, focusLimit: 20),
            .init(name: "Dyld Imports", workspace: "Dyld Imports", kind: .linkage, text: report.dyld_imports, focusPriority: 62, focusLimit: 24),
            .init(name: "Dyld Linking", workspace: "Dyld Linking", kind: .linkage, text: report.dyld_linking, focusPriority: 66, focusLimit: 28),
            .init(name: "Soft Linking", workspace: "Soft Linking", kind: .linkage, text: report.softlinking, focusPriority: 68, focusLimit: 28),
            .init(name: "Symbolication", workspace: "Symbolication", kind: .linkage, text: report.symbolication, focusPriority: 72, focusLimit: 32),
            .init(name: "Symbol Resolver", workspace: "Symbol Resolver", kind: .linkage, text: report.symbol_resolver, focusPriority: 76, focusLimit: 36),
            .init(name: "Apple Symbolication", workspace: "Apple Symbolication", kind: .linkage, text: report.apple_symbolication, focusPriority: 70, focusLimit: 28),
            .init(name: "Resolved Imports", workspace: "Resolved Imports", kind: .linkage, text: report.resolved_imports, focusPriority: 68, focusLimit: 28),
            .init(name: "Authenticated Targets", workspace: "Authenticated Targets", kind: .linkage, text: report.authenticated_bind_targets, focusPriority: 66, focusLimit: 24),
            .init(name: "Indirect Symbols", workspace: "Indirect Symbols", kind: .linkage, text: report.indirect_symbols, focusPriority: 58, focusLimit: 20),
            .init(name: "Pointer Slots", workspace: "Pointer Slots", kind: .linkage, text: report.pointer_slots, focusPriority: 60, focusLimit: 24),
            .init(name: "Exports", workspace: "Exports", kind: .linkage, text: report.export_trie, focusPriority: 58, focusLimit: 20),
            .init(name: "Symbol Provenance", workspace: "Symbol Provenance", kind: .linkage, text: report.symbol_provenance, focusPriority: 68, focusLimit: 28),
            .init(name: "Linkage Records", workspace: "Linkage Records", kind: .linkage, text: report.linkage_records, focusPriority: 66, focusLimit: 24),
            .init(name: "GOT/Auth-GOT", workspace: "GOT/Auth-GOT", kind: .linkage, text: report.got_auth_slots, focusPriority: 62, focusLimit: 24)
        ]
    }

    static func firstAddress(in text: String) -> UInt64? {
        guard let range = text.range(of: #"(?i)\b0x[0-9a-f]{1,16}\b"#, options: .regularExpression) else { return nil }
        return UInt64(text[range].dropFirst(2), radix: 16)
    }

    static func symbol(in text: String) -> String? {
        for key in ["symbol=", "name="] {
            guard let range = text.range(of: key, options: [.caseInsensitive]) else { continue }
            let tail = text[range.upperBound...]
            let value = String(tail.prefix { character in
                !(character.isWhitespace || character == "," || character == ";" || character == "]" || character == ")")
            })
            if !value.isEmpty, value.utf8.count <= 1024 { return value }
        }
        if text.hasPrefix("symbol ") {
            let value = String(text.dropFirst(7).prefix { !$0.isWhitespace })
            if !value.isEmpty, value.utf8.count <= 1024 { return value }
        }
        return nil
    }

    private static func lines(_ source: VGAnalysisEvidenceSource) -> [String] {
        source.text.split(separator: "\n", omittingEmptySubsequences: true).compactMap { raw in
            let line = String(raw).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !line.isEmpty, line.utf8.count <= maxLineBytes else { return nil }
            return line
        }
    }

    private static func makeHit(_ line: String, source: VGAnalysisEvidenceSource, ordinal: Int) -> VGAnalysisEvidenceHit {
        let address = firstAddress(in: line)
        let symbolName = symbol(in: line)
        return VGAnalysisEvidenceHit(
            id: "\(source.name)|\(ordinal)|\(line)",
            source: source.name,
            workspace: source.workspace,
            kind: source.kind,
            text: line,
            address: address,
            symbol: symbolName,
            ordinal: ordinal,
            focusPriority: source.focusPriority
        )
    }

    static func search(_ report: AnalysisReport, query: String, limit: Int = 500) -> [VGAnalysisEvidenceHit] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return [] }
        let boundedLimit = min(max(limit, 1), 1000)
        var result: [VGAnalysisEvidenceHit] = []
        for source in sources(report) {
            for (ordinal, line) in lines(source).enumerated() where line.localizedCaseInsensitiveContains(needle) {
                result.append(makeHit(line, source: source, ordinal: ordinal))
                if result.count >= boundedLimit { return result }
            }
        }
        return result
    }

    static func focusQueue(_ report: AnalysisReport, filter: String = "", limit: Int = 180) -> [VGAnalysisEvidenceHit] {
        let needle = filter.trimmingCharacters(in: .whitespacesAndNewlines)
        let boundedLimit = min(max(limit, 1), 300)
        var result: [VGAnalysisEvidenceHit] = []
        for source in sources(report).sorted(by: { lhs, rhs in
            lhs.focusPriority == rhs.focusPriority ? lhs.name < rhs.name : lhs.focusPriority > rhs.focusPriority
        }) {
            var admitted = 0
            for (ordinal, line) in lines(source).enumerated() {
                if !needle.isEmpty, !line.localizedCaseInsensitiveContains(needle) { continue }
                result.append(makeHit(line, source: source, ordinal: ordinal))
                admitted += 1
                if admitted >= source.focusLimit || result.count >= boundedLimit { break }
            }
            if result.count >= boundedLimit { break }
        }
        return result
    }

    static func corpus(_ report: AnalysisReport, limit: Int = 25_000) -> [VGAnalysisEvidenceHit] {
        let boundedLimit = min(max(limit, 1), 50_000)
        var result: [VGAnalysisEvidenceHit] = []
        for source in sources(report) {
            for (ordinal, line) in lines(source).enumerated() {
                result.append(makeHit(line, source: source, ordinal: ordinal))
                if result.count >= boundedLimit { return result }
            }
        }
        return result
    }

    static func related(_ report: AnalysisReport, evidence: String, selectedSymbol: String, limit: Int = 120) -> [VGAnalysisEvidenceHit] {
        let symbolNeedle = selectedSymbol.trimmingCharacters(in: .whitespacesAndNewlines)
        let addressNeedle = firstAddress(in: evidence)
        guard !symbolNeedle.isEmpty || addressNeedle != nil else { return [] }
        let boundedLimit = min(max(limit, 1), 300)
        var result: [VGAnalysisEvidenceHit] = []
        for source in sources(report) {
            for (ordinal, line) in lines(source).enumerated() {
                let symbolMatch = !symbolNeedle.isEmpty && line.localizedCaseInsensitiveContains(symbolNeedle)
                let addressMatch = addressNeedle != nil && firstAddress(in: line) == addressNeedle
                guard symbolMatch || addressMatch else { continue }
                result.append(makeHit(line, source: source, ordinal: ordinal))
                if result.count >= boundedLimit { return result }
            }
        }
        return result
    }
}

@MainActor
extension WorkbenchModel {
    func analysisSearchHits(limit: Int = 500) -> [VGAnalysisEvidenceHit] {
        currentEvidenceIndex?.search(analysisSearch, limit: limit) ?? []
    }

    func analysisFocusHits(filter: String = "", limit: Int = 180) -> [VGAnalysisEvidenceHit] {
        currentEvidenceIndex?.focus(filter: filter, limit: limit) ?? []
    }

    func relatedAnalysisHits(limit: Int = 120) -> [VGAnalysisEvidenceHit] {
        currentEvidenceIndex?.related(evidence: selectedEvidence, selectedSymbol: selectedSymbol, limit: limit) ?? []
    }

    func analysisEvidenceAddress(_ line: String) -> UInt64? {
        VGAnalysisNavigation.firstAddress(in: line)
    }

    func selectAnalysisHit(_ hit: VGAnalysisEvidenceHit) {
        selectEvidence(hit.text)
        if let symbol = hit.symbol { selectedSymbol = symbol }
    }

    func openAnalysisHit(_ hit: VGAnalysisEvidenceHit, destination: VGAnalysisHitDestination) {
        applyAnalysisHit(hit, destination: destination, recordVisit: true)
    }

    func resumeAnalysisVisit(_ visit: VGAnalysisVisit) {
        let destination: VGAnalysisHitDestination
        switch visit.destination {
        case "disassembly": destination = .disassembly
        case "cfg": destination = .cfg
        case "symbol": destination = .selectedSymbol
        default: destination = .source
        }
        applyAnalysisHit(visit.hit, destination: destination, recordVisit: false)
        status = "Resumed evidence from \(visit.source)."
    }

    func resumeMostRecentAnalysisVisit() {
        guard let visit = analysisRecentVisits.first else { status = "No recent evidence pivot is available for this artifact."; return }
        analysisHistoryCursor = 0
        resumeAnalysisVisit(visit)
    }

    var canNavigateAnalysisHistoryBackward: Bool {
        !analysisRecentVisits.isEmpty && analysisHistoryCursor + 1 < analysisRecentVisits.count
    }

    var canNavigateAnalysisHistoryForward: Bool {
        analysisHistoryCursor > 0 && analysisHistoryCursor < analysisRecentVisits.count
    }

    func navigateAnalysisHistoryBackward() {
        guard canNavigateAnalysisHistoryBackward else { status = "No earlier evidence pivot is available."; return }
        analysisHistoryCursor += 1
        resumeAnalysisVisit(analysisRecentVisits[analysisHistoryCursor])
        status = "Moved to previous evidence pivot (\(analysisHistoryCursor + 1)/\(analysisRecentVisits.count))."
    }

    func navigateAnalysisHistoryForward() {
        guard canNavigateAnalysisHistoryForward else { status = "No newer evidence pivot is available."; return }
        analysisHistoryCursor -= 1
        resumeAnalysisVisit(analysisRecentVisits[analysisHistoryCursor])
        status = "Moved to newer evidence pivot (\(analysisHistoryCursor + 1)/\(analysisRecentVisits.count))."
    }

    private func applyAnalysisHit(_ hit: VGAnalysisEvidenceHit, destination: VGAnalysisHitDestination, recordVisit: Bool) {
        selectAnalysisHit(hit)
        switch destination {
        case .source:
            if recordVisit { recordAnalysisVisit(hit, destination: destination) }
            workspace = hit.workspace
        case .disassembly:
            guard let address = hit.address else { status = "This evidence row has no navigable virtual address."; return }
            if recordVisit { recordAnalysisVisit(hit, destination: destination) }
            addressText = String(format: "0x%llx", address)
            workspace = "Disassembly"
            if currentPage != nil { jumpToAddress() } else { loadPage() }
        case .cfg:
            guard let address = hit.address else { status = "This evidence row has no navigable virtual address."; return }
            if recordVisit { recordAnalysisVisit(hit, destination: destination) }
            let text = String(format: "0x%016llx", address)
            addressText = text
            cfgFocus = text
            workspace = "CFG"
        case .selectedSymbol:
            guard let symbol = hit.symbol, !symbol.isEmpty else { status = "This evidence row has no explicit symbol identity."; return }
            if recordVisit { recordAnalysisVisit(hit, destination: destination) }
            selectedSymbol = symbol
            workspace = "Selected Symbol"
        }
    }
}

struct VGAnalysisEvidenceHitCard: View {
    @ObservedObject var model: WorkbenchModel
    let hit: VGAnalysisEvidenceHit
    var compact = false
    @State private var annotationPresented = false
    @State private var annotationNote = ""
    @State private var annotationTags = ""
    @State private var annotationConfidence: VGAnalysisConfidence = .unclassified

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 6 : 8) {
            HStack(spacing: 7) {
                Label(hit.source, systemImage: hit.kind.icon)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .lineLimit(1)
                Spacer(minLength: 6)
                if let address = hit.addressLabel {
                    Text(address)
                        .font(.system(.caption2, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Text(hit.text)
                .font(.system(compact ? .caption2 : .caption, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .lineLimit(compact ? 4 : 7)
            if let annotation = model.analysisAnnotation(for: hit) {
                VGFlowLayout(horizontalSpacing: 5, verticalSpacing: 4) {
                    VGTagPill(text: annotation.confidence.rawValue, systemImage: "checkmark.seal")
                    ForEach(annotation.tags, id: \.self) { tag in
                        VGTagPill(text: tag, systemImage: "tag")
                    }
                    if !annotation.note.isEmpty {
                        VGTagPill(text: "annotated", systemImage: "note.text")
                    }
                }
            }
            if annotationPresented {
                VStack(alignment: .leading, spacing: 7) {
                    TextEditor(text: $annotationNote)
                        .font(.caption)
                        .frame(minHeight: 58, maxHeight: 100)
                        .overlay(RoundedRectangle(cornerRadius: 6).stroke(.separator))
                    TextField("Tags, separated by commas", text: $annotationTags)
                        .textFieldStyle(.roundedBorder)
                    HStack {
                        Picker("Confidence", selection: $annotationConfidence) {
                            ForEach(VGAnalysisConfidence.allCases) { value in
                                Text(value.rawValue).tag(value)
                            }
                        }
                        .labelsHidden()
                        .frame(maxWidth: 150)
                        Spacer()
                        Button("Save") {
                            model.selectAnalysisHit(hit)
                            model.annotationDraft = annotationNote
                            model.annotationTagsText = annotationTags
                            model.annotationConfidence = annotationConfidence
                            model.saveSelectedEvidenceAnnotation()
                            annotationPresented = false
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.small)
                    }
                }
                .padding(7)
                .background(.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
            }
            VGFlowLayout(horizontalSpacing: 6, verticalSpacing: 5) {
                Button("Select") { model.selectAnalysisHit(hit) }
                Button(annotationPresented ? "Close annotation" : "Annotate") {
                    if !annotationPresented {
                        let annotation = model.analysisAnnotation(for: hit)
                        annotationNote = annotation?.note ?? ""
                        annotationTags = annotation?.tags.joined(separator: ", ") ?? ""
                        annotationConfidence = annotation?.confidence ?? .unclassified
                    }
                    annotationPresented.toggle()
                }
                Button(model.isAnalysisHitPinned(hit) ? "Unpin" : "Pin") { model.toggleAnalysisPin(hit) }
                if !model.analysisCollections.isEmpty {
                    Menu("Collect") {
                        ForEach(model.analysisCollections, id: \.id) { collection in
                            Button {
                                model.toggleAnalysisCollectionMembership(hit, collectionID: collection.id)
                            } label: {
                                Label(collection.name, systemImage: model.analysisCollectionContains(hit, collectionID: collection.id) ? "checkmark.circle.fill" : "circle")
                            }
                        }
                    }
                }
                Button("Source") { model.openAnalysisHit(hit, destination: .source) }
                if hit.address != nil {
                    Button("Code") { model.openAnalysisHit(hit, destination: .disassembly) }
                        .disabled(model.busy || !model.projectServiceReady)
                    Button("CFG") { model.openAnalysisHit(hit, destination: .cfg) }
                }
                if hit.symbol != nil {
                    Button("Symbol") { model.openAnalysisHit(hit, destination: .selectedSymbol) }
                }
            }
            .font(.caption2)
            .buttonStyle(.borderless)
        }
        .padding(compact ? 8 : 10)
        .background(.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 9, style: .continuous)
                .strokeBorder(.primary.opacity(0.06), lineWidth: 1)
        }
        .onAppear {
            let annotation = model.analysisAnnotation(for: hit)
            annotationNote = annotation?.note ?? ""
            annotationTags = annotation?.tags.joined(separator: ", ") ?? ""
            annotationConfidence = annotation?.confidence ?? .unclassified
        }
    }
}

struct VGAnalysisTriageView: View {
    @ObservedObject var model: WorkbenchModel
    @State private var filter = ""
    @State private var mode = "Focus"
    @State private var newCollectionName = ""
    @State private var newSavedSearchName = ""
    @State private var collectionFilter = ""
    @State private var renamingCollectionID: String?
    @State private var renamingCollectionName = ""

    private var focusHits: [VGAnalysisEvidenceHit] {
        let indexed = model.analysisFocusHits(filter: filter)
        guard !filter.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return indexed }
        let annotated = model.analysisFocusHits().filter { model.analysisHitMatchesFilter($0, query: filter) }
        var seen = Set<String>()
        return (indexed + annotated).filter { seen.insert($0.id).inserted }.prefix(180).map { $0 }
    }
    private var pinnedHits: [VGAnalysisEvidenceHit] {
        model.analysisPins.map(\.hit).filter {
            filter.isEmpty || $0.text.localizedCaseInsensitiveContains(filter) || $0.source.localizedCaseInsensitiveContains(filter) || model.analysisHitMatchesFilter($0, query: filter)
        }
    }
    private var recentVisits: [VGAnalysisVisit] {
        model.analysisRecentVisits.filter {
            filter.isEmpty || $0.text.localizedCaseInsensitiveContains(filter) || $0.source.localizedCaseInsensitiveContains(filter) || model.analysisHitMatchesFilter($0.hit, query: filter)
        }
    }
    private var annotatedHits: [VGAnalysisEvidenceHit] {
        model.analysisAnnotatedHits().filter {
            filter.isEmpty || $0.text.localizedCaseInsensitiveContains(filter) || $0.source.localizedCaseInsensitiveContains(filter) || model.analysisHitMatchesFilter($0, query: filter)
        }
    }
    private var filteredCollections: [VGInvestigationCollection] { model.filteredAnalysisCollections(query: collectionFilter) }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("A durable, source-prioritized investigation queue built from validated analysis channels. Focus ordering is for navigation efficiency only; it is not a vulnerability, severity, exploitability, or semantic-importance score. Pins, recents, collections, review baselines, and change summaries preserve analyst context without inventing semantic meaning.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            VGFlowLayout {
                if let report = model.currentAnalysis {
                    VGTagPill(text: "\(report.function_count) functions", systemImage: "function")
                    VGTagPill(text: "\(report.basic_block_count) blocks", systemImage: "point.3.connected.trianglepath.dotted")
                    VGTagPill(text: "\(report.analyzed_instructions) instructions", systemImage: "text.alignleft")
                    if report.truncated { VGTagPill(text: "bounded scan", systemImage: "gauge.with.dots.needle.50percent") }
                } else {
                    VGTagPill(text: "analysis not loaded", systemImage: "tray")
                }
                VGTagPill(text: "\(model.analysisPins.count) pinned", systemImage: "pin")
                VGTagPill(text: "\(model.analysisAnnotationCount) annotated", systemImage: "note.text")
                VGTagPill(text: "\(model.analysisRecentVisits.count) recent", systemImage: "clock.arrow.circlepath")
                VGTagPill(text: "\(model.analysisCollections.count) collections", systemImage: "folder")
            }

            ViewThatFits(in: .horizontal) {
                Picker("Triage view", selection: $mode) {
                    Text("Focus").tag("Focus")
                    Text("Pinned").tag("Pinned")
                    Text("Annotated").tag("Annotated")
                    Text("Recent").tag("Recent")
                    Text("Collections").tag("Collections")
                    Text("Review").tag("Review")
                    Text("Changes").tag("Changes")
                }
                .pickerStyle(.segmented)
                .frame(minWidth: 720, idealWidth: 780, maxWidth: 820)

                Picker("Triage view", selection: $mode) {
                    Text("Focus").tag("Focus")
                    Text("Pinned").tag("Pinned")
                    Text("Annotated").tag("Annotated")
                    Text("Recent").tag("Recent")
                    Text("Collections").tag("Collections")
                    Text("Review").tag("Review")
                    Text("Changes").tag("Changes")
                }
                .pickerStyle(.menu)
                .frame(maxWidth: 260, alignment: .leading)
            }

            if mode != "Changes" && mode != "Collections" && mode != "Review" {
                HStack(spacing: 8) {
                    Image(systemName: "line.3.horizontal.decrease.circle").foregroundStyle(.secondary)
                    TextField(mode == "Focus" ? "Filter the focus queue…" : mode == "Annotated" ? "Filter annotations…" : "Filter preserved evidence…", text: $filter)
                        .textFieldStyle(.roundedBorder)
                    if !filter.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        TextField("Saved search name", text: $newSavedSearchName)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 150)
                        Button("Save") {
                            model.saveAnalysisSearch(named: newSavedSearchName, query: filter)
                            newSavedSearchName = ""
                        }
                        .disabled(newSavedSearchName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    if !model.analysisSavedSearches.isEmpty {
                        Menu("Saved") {
                            ForEach(model.analysisSavedSearches) { search in
                                Button(search.name) { filter = search.query }
                            }
                            Divider()
                            ForEach(model.analysisSavedSearches) { search in
                                Button("Delete \(search.name)", role: .destructive) {
                                    model.deleteAnalysisSearch(search.id)
                                }
                            }
                        }
                    }
                    if !filter.isEmpty { Button("Clear") { filter = "" } }
                    if mode == "Recent", !model.analysisRecentVisits.isEmpty {
                        Button("Previous", action: model.navigateAnalysisHistoryBackward)
                            .disabled(!model.canNavigateAnalysisHistoryBackward)
                        Button("Next", action: model.navigateAnalysisHistoryForward)
                            .disabled(!model.canNavigateAnalysisHistoryForward)
                        Button("Resume Latest", action: model.resumeMostRecentAnalysisVisit)
                        Button("Clear Recent", action: model.clearAnalysisRecents)
                    }
                }
                .frame(maxWidth: 760)
            }

            Group {
                switch mode {
                case "Pinned":
                    if pinnedHits.isEmpty {
                        ContentUnavailableView("No pinned evidence", systemImage: "pin", description: Text("Pin evidence from Triage, Search, or Related Evidence to preserve it with the project."))
                    } else {
                        ScrollView { LazyVStack(alignment: .leading, spacing: 8) { ForEach(pinnedHits) { VGAnalysisEvidenceHitCard(model: model, hit: $0) } } }
                    }
                case "Annotated":
                    if annotatedHits.isEmpty {
                        ContentUnavailableView("No annotated evidence", systemImage: "note.text", description: Text("Select an evidence row and add an analyst annotation from the Inspector to collect it here."))
                    } else {
                        ScrollView { LazyVStack(alignment: .leading, spacing: 8) { ForEach(annotatedHits) { VGAnalysisEvidenceHitCard(model: model, hit: $0) } } }
                    }
                case "Recent":
                    if recentVisits.isEmpty {
                        ContentUnavailableView("No recent evidence navigation", systemImage: "clock.arrow.circlepath", description: Text("Opening evidence in Source, Code, CFG, or Symbol records a bounded project-local history."))
                    } else {
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 8) {
                                ForEach(recentVisits) { visit in
                                    VStack(alignment: .leading, spacing: 5) {
                                        HStack {
                                            Text(visit.destination.capitalized).font(.caption2).fontWeight(.semibold)
                                            Spacer()
                                            Button("Resume") { model.resumeAnalysisVisit(visit) }.buttonStyle(.borderless).font(.caption2)
                                            Text(visit.visitedAt, style: .relative).font(.caption2).foregroundStyle(.secondary)
                                        }
                                        VGAnalysisEvidenceHitCard(model: model, hit: visit.hit, compact: true)
                                    }
                                }
                            }
                        }
                    }
                case "Collections":
                    VStack(alignment: .leading, spacing: 10) {
                        VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8) {
                            TextField("New investigation collection…", text: $newCollectionName)
                                .textFieldStyle(.roundedBorder)
                                .frame(minWidth: 220, idealWidth: 320, maxWidth: 420)
                            Button("Create") {
                                model.createAnalysisCollection(named: newCollectionName)
                                newCollectionName = ""
                            }
                            .disabled(newCollectionName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            TextField("Filter collections or preserved evidence…", text: $collectionFilter)
                                .textFieldStyle(.roundedBorder)
                                .frame(minWidth: 220, idealWidth: 340, maxWidth: 460)
                            if !collectionFilter.isEmpty { Button("Clear") { collectionFilter = "" } }
                        }
                        if model.analysisCollections.isEmpty {
                            ContentUnavailableView("No investigation collections", systemImage: "folder.badge.plus", description: Text("Create a named collection, then use Collect on evidence cards. Collection membership automatically preserves the underlying evidence as a project-local pin."))
                        } else if filteredCollections.isEmpty {
                            ContentUnavailableView("No matching collections", systemImage: "line.3.horizontal.decrease.circle", description: Text("The collection filter searches collection names and preserved evidence text, source labels, and symbols."))
                        } else {
                            let currentPinIDs = model.currentAnalysisPinIDs()
                            ScrollView {
                                LazyVStack(alignment: .leading, spacing: 10) {
                                    ForEach(filteredCollections, id: \.id) { collection in
                                        let hits = model.analysisCollectionHits(collection)
                                        let health = model.analysisCollectionHealth(collection, currentPinIDs: currentPinIDs)
                                        let drift = model.analysisCollectionReviewDrift(collection, currentPinIDs: currentPinIDs)
                                        GroupBox {
                                            VStack(alignment: .leading, spacing: 8) {
                                                HStack(alignment: .firstTextBaseline) {
                                                    Label(collection.name, systemImage: "folder")
                                                        .font(.headline)
                                                        .lineLimit(2)
                                                    Spacer(minLength: 8)
                                                    VGTagPill(text: "\(hits.count) findings", systemImage: "pin")
                                                    VGTagPill(text: health.label, systemImage: health.current == health.total ? "checkmark.circle" : "clock.badge.questionmark")
                                                    if let drift {
                                                        VGTagPill(text: drift.label, systemImage: (drift.hasMembershipDrift || drift.hasPresenceDrift) ? "arrow.triangle.2.circlepath" : "checkmark.seal")
                                                    }
                                                    Menu("More") {
                                                        Button(model.analysisReviewBaselines[collection.id] == nil ? "Mark Reviewed" : "Refresh Review Baseline") {
                                                            model.markAnalysisCollectionReviewed(collection.id)
                                                        }
                                                        Divider()
                                                        Button("Rename…") {
                                                            renamingCollectionID = collection.id
                                                            renamingCollectionName = collection.name
                                                        }
                                                        Button("Delete", role: .destructive) { model.deleteAnalysisCollection(collection.id) }
                                                    }
                                                }
                                                if hits.isEmpty {
                                                    Text("No pinned findings in this collection yet.").font(.caption).foregroundStyle(.secondary)
                                                } else {
                                                    ForEach(hits.prefix(64)) { hit in VGAnalysisEvidenceHitCard(model: model, hit: hit, compact: true) }
                                                }
                                            }.padding(4)
                                        }
                                    }
                                }
                            }
                            Text("Collection health compares preserved pin identities with a bounded corpus of the currently loaded validated report. ‘Preserved’ means the exact evidence line is not present in that bounded corpus; it does not mean the finding is invalid or semantically obsolete. Review drift compares exact pin identities with the last explicit review baseline; neither preserved status nor review drift means a finding is fixed, important, vulnerable, reachable, or semantically significant.")
                                .font(.caption2).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                case "Review":
                    VGCollectionReviewView(model: model)
                case "Changes":
                    VGAnalysisDeltaView(model: model)
                default:
                    if model.currentAnalysis == nil {
                        ContentUnavailableView("Analysis not loaded", systemImage: "scope", description: Text("Pinned, Recent, Collections, Review, and Changes remain available now. Run Analyze binary to load or reuse the validated report for the live Focus queue."))
                    } else if model.analysisEvidenceIndexing {
                        VStack(spacing: 10) {
                            ProgressView()
                            Text("Indexing typed evidence…").font(.headline)
                            Text("Search, triage, and correlation are being prepared off the UI thread.").font(.caption).foregroundStyle(.secondary)
                        }.frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else if focusHits.isEmpty {
                        ContentUnavailableView(filter.isEmpty ? "No triage evidence" : "No matching triage evidence", systemImage: "scope", description: Text(filter.isEmpty ? "The current analysis did not publish evidence into the triage channels." : "Clear or broaden the filter."))
                    } else {
                        ScrollView { LazyVStack(alignment: .leading, spacing: 8) { ForEach(focusHits) { VGAnalysisEvidenceHitCard(model: model, hit: $0) } } }
                    }
                }
            }
            .frame(minHeight: 420)
        }
        .alert("Rename Investigation Collection", isPresented: Binding(
            get: { renamingCollectionID != nil },
            set: { if !$0 { renamingCollectionID = nil } }
        )) {
            TextField("Collection name", text: $renamingCollectionName)
            Button("Rename") {
                if let id = renamingCollectionID { model.renameAnalysisCollection(id, to: renamingCollectionName) }
                renamingCollectionID = nil
            }
            Button("Cancel", role: .cancel) { renamingCollectionID = nil }
        } message: {
            Text("Collection names are organizational labels only; renaming does not alter the preserved evidence or its provenance.")
        }
    }
}


struct VGCollectionReviewView: View {
    @ObservedObject var model: WorkbenchModel
    @State private var reviewCollectionID = ""
    @State private var reviewState: VGInvestigationReviewState = .open
    @State private var reviewNote = ""
    @State private var followUpTitle = ""
    @State private var followUpNote = ""
    @State private var leftCollectionID = ""
    @State private var rightCollectionID = ""

    private var reviewCollection: VGInvestigationCollection? {
        model.analysisCollections.first(where: { $0.id == reviewCollectionID })
    }

    private var leftCollection: VGInvestigationCollection? {
        model.analysisCollections.first(where: { $0.id == leftCollectionID })
    }

    private var rightCollection: VGInvestigationCollection? {
        model.analysisCollections.first(where: { $0.id == rightCollectionID })
    }

    private var comparison: VGInvestigationCollectionComparison? {
        model.compareAnalysisCollections(leftCollectionID, rightCollectionID)
    }

    var body: some View {
        if model.analysisCollections.isEmpty {
            ContentUnavailableView(
                "No collections to review",
                systemImage: "text.book.closed",
                description: Text("Create an investigation collection first. Review journal entries and handoff summaries are bound to preserved collection evidence.")
            )
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    reviewJournal

                    if let collection = reviewCollection {
                        followUpQueue(collection)
                    }

                    Divider()

                    if model.analysisCollections.count < 2 {
                        ContentUnavailableView(
                            "Create a second collection to compare",
                            systemImage: "rectangle.split.2x1",
                            description: Text("The review journal and handoff work with one collection. Pairwise exact-identity comparison becomes available when a second collection exists.")
                        )
                    } else {
                        comparisonControls
                        comparisonBody
                    }

                    Text("Review notes and workflow states are analyst-authored project context. Follow-ups are analyst-authored workflow tasks, not evidence. Review, health, drift, and comparison remain exact-identity/provenance tools and do not imply vulnerability, severity, correctness, runtime reachability, or semantic importance. They also do not imply exploitability.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.bottom, 4)
            }
            .onAppear(perform: seedCollections)
            .onChange(of: reviewCollectionID) { _, newValue in
                reviewState = model.latestAnalysisReviewState(for: newValue)
                reviewNote = ""
                followUpTitle = ""
                followUpNote = ""
            }
        }
    }

    @ViewBuilder
    private var reviewJournal: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Label("Review Journal & Handoff", systemImage: "text.book.closed")
                    .font(.headline)

                VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8) {
                    Picker("Collection", selection: $reviewCollectionID) {
                        ForEach(model.analysisCollections, id: \.id) { collection in
                            Text(collection.name).tag(collection.id)
                        }
                    }
                    .frame(minWidth: 220, idealWidth: 300, maxWidth: 380)

                    Picker("Workflow", selection: $reviewState) {
                        ForEach(VGInvestigationReviewState.allCases, id: \.self) { state in
                            Label(state.rawValue, systemImage: state.icon).tag(state)
                        }
                    }
                    .frame(minWidth: 170, idealWidth: 220, maxWidth: 280)

                    Button("Record Review") {
                        model.appendAnalysisReviewEntry(collectionID: reviewCollectionID, state: reviewState, note: reviewNote)
                        reviewNote = ""
                    }
                    .disabled(reviewCollectionID.isEmpty || (reviewNote.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && reviewState == model.latestAnalysisReviewState(for: reviewCollectionID)))

                    Button("Copy Handoff") {
                        model.copyAnalysisCollectionHandoff(reviewCollectionID)
                    }
                    .disabled(reviewCollectionID.isEmpty)

                    if let collection = reviewCollection {
                        Button(model.analysisReviewBaselines[collection.id] == nil ? "Mark Reviewed" : "Refresh Review Baseline") {
                            model.markAnalysisCollectionReviewed(collection.id)
                        }
                    }
                }

                TextEditor(text: $reviewNote)
                    .font(.body)
                    .frame(minHeight: 64, idealHeight: 84, maxHeight: 120)
                    .overlay {
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(.quaternary, lineWidth: 1)
                    }
                    .accessibilityLabel("Review note")

                if let collection = reviewCollection {
                    let entries = model.analysisReviewEntries(for: collection.id)
                    let health = model.analysisCollectionHealth(collection, currentPinIDs: model.currentAnalysisPinIDs())
                    let drift = model.analysisCollectionReviewDrift(collection, currentPinIDs: model.currentAnalysisPinIDs())

                    VGFlowLayout {
                        VGTagPill(text: model.latestAnalysisReviewState(for: collection.id).rawValue, systemImage: model.latestAnalysisReviewState(for: collection.id).icon)
                        VGTagPill(text: health.label, systemImage: "heart.text.square")
                        if let drift { VGTagPill(text: drift.label, systemImage: "arrow.triangle.2.circlepath") }
                        VGTagPill(text: "\(entries.count) journal entries", systemImage: "list.bullet.rectangle")
                        let openFollowUps = model.analysisFollowUps(for: collection.id, includeCompleted: false).count
                        VGTagPill(text: "\(openFollowUps) open follow-ups", systemImage: openFollowUps == 0 ? "checkmark.circle" : "checklist")
                    }

                    if entries.isEmpty {
                        Text("No review journal entries yet. Record a workflow state change or a concise note to establish a durable analyst timeline for this collection.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(entries.prefix(12)) { entry in
                                VStack(alignment: .leading, spacing: 3) {
                                    HStack(spacing: 8) {
                                        Label(entry.state.rawValue, systemImage: entry.state.icon)
                                            .font(.caption)
                                            .fontWeight(.semibold)
                                        Spacer(minLength: 8)
                                        Text(entry.createdAt.formatted(date: .abbreviated, time: .shortened))
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                    }
                                    Text(entry.note.isEmpty ? "Workflow state update." : entry.note)
                                        .font(.caption)
                                        .fixedSize(horizontal: false, vertical: true)
                                    Text("\(entry.memberPinIDs.count) exact evidence identities captured with this journal entry")
                                        .font(.caption2)
                                        .foregroundStyle(.tertiary)
                                }
                                .padding(8)
                                .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 8))
                            }
                            if entries.count > 12 {
                                Text("\(entries.count - 12) older review entries omitted from this bounded surface; they remain in project continuity state within the configured limits.")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func followUpQueue(_ collection: VGInvestigationCollection) -> some View {
        let items = model.analysisFollowUps(for: collection.id)
        let openCount = items.filter { $0.state == .open }.count
        let completedCount = items.count - openCount

        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Label("Follow-Up Queue", systemImage: "checklist")
                        .font(.headline)
                    Spacer(minLength: 8)
                    Text("\(openCount) open · \(completedCount) completed")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text("Follow-ups are analyst-authored workflow tasks bound to a bounded snapshot of exact collection evidence identities. They are not findings, severity labels, or semantic conclusions.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)

                VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8) {
                    TextField("Follow-up title…", text: $followUpTitle)
                        .textFieldStyle(.roundedBorder)
                        .frame(minWidth: 220, idealWidth: 320, maxWidth: 440)
                    TextField("Optional context…", text: $followUpNote)
                        .textFieldStyle(.roundedBorder)
                        .frame(minWidth: 220, idealWidth: 360, maxWidth: 520)
                    Button("Add Follow-Up") {
                        if model.createAnalysisFollowUp(collectionID: collection.id, title: followUpTitle, note: followUpNote) {
                            followUpTitle = ""
                            followUpNote = ""
                        }
                    }
                    .disabled(followUpTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                if items.isEmpty {
                    ContentUnavailableView("No follow-ups", systemImage: "checklist", description: Text("Add explicit next actions here instead of overloading review notes or evidence with workflow meaning."))
                        .frame(minHeight: 120)
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(items.prefix(32)) { item in
                            HStack(alignment: .top, spacing: 10) {
                                Button { model.toggleAnalysisFollowUpCompleted(item.id) } label: {
                                    Image(systemName: item.state.icon)
                                        .imageScale(.large)
                                }
                                .buttonStyle(.plain)
                                .help(item.state == .open ? "Mark completed" : "Reopen follow-up")

                                VStack(alignment: .leading, spacing: 3) {
                                    Text(item.title)
                                        .font(.subheadline)
                                        .fontWeight(.semibold)
                                        .strikethrough(item.state == .completed, color: .secondary)
                                    if !item.note.isEmpty {
                                        Text(item.note)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .fixedSize(horizontal: false, vertical: true)
                                    }
                                    Text("\(item.relatedPinIDs.count) bounded evidence identities · created \(item.createdAt.formatted(date: .abbreviated, time: .shortened))")
                                        .font(.caption2)
                                        .foregroundStyle(.tertiary)
                                }
                                Spacer(minLength: 8)
                                Menu {
                                    Button(item.state == .open ? "Mark Completed" : "Reopen") { model.toggleAnalysisFollowUpCompleted(item.id) }
                                    Divider()
                                    Button("Remove Follow-Up", role: .destructive) { model.deleteAnalysisFollowUp(item.id) }
                                } label: {
                                    Image(systemName: "ellipsis.circle")
                                }
                                .menuStyle(.borderlessButton)
                            }
                            .padding(8)
                            .background(.quaternary.opacity(item.state == .open ? 0.48 : 0.28), in: RoundedRectangle(cornerRadius: 8))
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private var comparisonControls: some View {
        VGFlowLayout(horizontalSpacing: 8, verticalSpacing: 8) {
            Picker("Collection A", selection: $leftCollectionID) {
                ForEach(model.analysisCollections, id: \.id) { collection in
                    Text(collection.name).tag(collection.id)
                }
            }
            .frame(minWidth: 220, idealWidth: 300, maxWidth: 380)

            Picker("Collection B", selection: $rightCollectionID) {
                ForEach(model.analysisCollections, id: \.id) { collection in
                    Text(collection.name).tag(collection.id)
                }
            }
            .frame(minWidth: 220, idealWidth: 300, maxWidth: 380)

            Button("Swap") {
                let value = leftCollectionID
                leftCollectionID = rightCollectionID
                rightCollectionID = value
            }
            .disabled(leftCollectionID.isEmpty || rightCollectionID.isEmpty)
        }
    }

    @ViewBuilder
    private var comparisonBody: some View {
        if leftCollectionID == rightCollectionID {
            ContentUnavailableView("Choose two different collections", systemImage: "rectangle.split.2x1", description: Text("Collection comparison uses exact preserved evidence identities."))
        } else if let comparison {
            let currentPinIDs = model.currentAnalysisPinIDs()
            VGFlowLayout {
                VGTagPill(text: "\(comparison.shared.count) shared", systemImage: "link")
                VGTagPill(text: "\(comparison.leftOnly.count) only in \(comparison.leftName)", systemImage: "a.circle")
                VGTagPill(text: "\(comparison.rightOnly.count) only in \(comparison.rightName)", systemImage: "b.circle")
            }

            if let leftCollection, let drift = model.analysisCollectionReviewDrift(leftCollection, currentPinIDs: currentPinIDs) {
                Text("\(leftCollection.name) review: \(drift.label) · reviewed \(drift.reviewedAt.formatted(date: .abbreviated, time: .shortened))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let rightCollection, let drift = model.analysisCollectionReviewDrift(rightCollection, currentPinIDs: currentPinIDs) {
                Text("\(rightCollection.name) review: \(drift.label) · reviewed \(drift.reviewedAt.formatted(date: .abbreviated, time: .shortened))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            VStack(alignment: .leading, spacing: 12) {
                VGCollectionComparisonSection(title: "Shared evidence", icon: "link", hits: comparison.shared, model: model)
                VGCollectionComparisonSection(title: "Only in \(comparison.leftName)", icon: "a.circle", hits: comparison.leftOnly, model: model)
                VGCollectionComparisonSection(title: "Only in \(comparison.rightName)", icon: "b.circle", hits: comparison.rightOnly, model: model)
            }
        }
    }

    private func seedCollections() {
        let ids = model.analysisCollections.map(\.id)
        guard let first = ids.first else { return }
        if !ids.contains(reviewCollectionID) {
            reviewCollectionID = first
            reviewState = model.latestAnalysisReviewState(for: first)
        }
        if ids.count >= 2 {
            if !ids.contains(leftCollectionID) { leftCollectionID = ids[0] }
            if !ids.contains(rightCollectionID) || rightCollectionID == leftCollectionID {
                rightCollectionID = ids.first(where: { $0 != leftCollectionID }) ?? ids[1]
            }
        }
    }
}

private struct VGCollectionComparisonSection: View {
    let title: String
    let icon: String
    let hits: [VGAnalysisEvidenceHit]
    @ObservedObject var model: WorkbenchModel

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                Label(title, systemImage: icon)
                    .font(.headline)
                if hits.isEmpty {
                    Text("No evidence in this exact-identity set.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(hits.prefix(24)) { hit in
                        VGAnalysisEvidenceHitCard(model: model, hit: hit, compact: true)
                    }
                    if hits.count > 24 {
                        Text("\(hits.count - 24) additional findings omitted from this bounded review surface.")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct VGAnalysisDeltaView: View {
    @ObservedObject var model: WorkbenchModel
    var body: some View {
        if let delta = model.analysisDelta {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    Label("Last analysis comparison", systemImage: "arrow.triangle.2.circlepath")
                        .font(.headline)
                    Text(delta.summary).font(.body)
                    Text("Observed \(delta.observedAt.formatted(date: .abbreviated, time: .shortened))")
                        .font(.caption).foregroundStyle(.secondary)
                    if delta.changedChannels.isEmpty {
                        ContentUnavailableView("No changed channels", systemImage: "checkmark.circle", description: Text("The last comparison did not detect analysis-channel changes, or this is the first recorded baseline."))
                    } else {
                        Text("Changed channels").font(.subheadline).fontWeight(.semibold)
                        VGFlowLayout {
                            ForEach(delta.changedChannels, id: \.self) { name in VGTagPill(text: name, systemImage: "waveform.path.ecg") }
                        }
                    }
                    Text("\(delta.unchangedChannels) channels unchanged")
                        .font(.caption).foregroundStyle(.secondary)
                    Divider()
                    Text("Change detection compares bounded per-channel fingerprints and line counts. It is a continuity signal, not semantic diffing or a statement that a changed channel is important.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        } else {
            ContentUnavailableView("No analysis baseline", systemImage: "arrow.triangle.2.circlepath", description: Text("Run analysis once to record a project-local baseline. Future analyses can then report which evidence channels changed."))
        }
    }
}

struct VGAnalysisSearchResults: View {
    @ObservedObject var model: WorkbenchModel

    private var hits: [VGAnalysisEvidenceHit] { model.analysisSearchHits() }

    var body: some View {
        if model.currentAnalysis == nil {
            ContentUnavailableView("No analysis yet", systemImage: "magnifyingglass", description: Text("Analyze the selected artifact first."))
                .frame(minHeight: 320)
        } else if model.analysisEvidenceIndexing {
            VStack(spacing: 10) {
                ProgressView()
                Text("Indexing typed evidence…").font(.headline)
                Text(model.analysisEvidenceStatus).font(.caption).foregroundStyle(.secondary)
            }.frame(minHeight: 320)
        } else if model.analysisSearch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            ContentUnavailableView("Search the analysis", systemImage: "magnifyingglass", description: Text("Enter a symbol, address, string, relocation, register, or analysis term."))
                .frame(minHeight: 320)
        } else if hits.isEmpty {
            ContentUnavailableView("No analysis hits", systemImage: "magnifyingglass", description: Text("No validated analysis channel contains ‘\(model.analysisSearch)’."))
                .frame(minHeight: 320)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 7) {
                    ForEach(hits) { hit in
                        VGAnalysisEvidenceHitCard(model: model, hit: hit, compact: true)
                    }
                    if hits.count >= 500 {
                        Text("Search result cap reached (500). Narrow the query for a smaller evidence set.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .padding(.vertical, 8)
                    }
                }
            }
            .frame(minHeight: 320)
        }
    }
}

struct VGRelatedAnalysisEvidenceView: View {
    @ObservedObject var model: WorkbenchModel

    private var hits: [VGAnalysisEvidenceHit] { model.relatedAnalysisHits() }

    var body: some View {
        if model.currentAnalysis == nil {
            Text("Analyze the selected source to populate correlated evidence.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else if model.selectedEvidence.isEmpty && model.selectedSymbol.isEmpty {
            Text("Select an address- or symbol-bearing evidence row to correlate it across analysis channels.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else if hits.isEmpty {
            Text("No additional analysis channel shares the selected address or symbol identity.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 6) {
                    ForEach(hits.prefix(120)) { hit in
                        VGAnalysisEvidenceHitCard(model: model, hit: hit, compact: true)
                    }
                }
            }
            .frame(minHeight: 150)
        }
    }
}
