import Foundation

/// Canonical, typed evidence record used by the 0.5 Evidence Core.
/// The worker's v1 66-field text report remains the wire/cache compatibility format;
/// this structure is the indexed in-process representation consumed by SwiftUI.
struct VGEvidenceRecord: Identifiable, Hashable, Sendable {
    let id: String
    let source: String
    let workspace: String
    let kind: VGAnalysisEvidenceKind
    let text: String
    let normalizedText: String
    let address: UInt64?
    let symbol: String?
    let ordinal: Int
    let focusPriority: Int

    var hit: VGAnalysisEvidenceHit {
        VGAnalysisEvidenceHit(
            id: id,
            source: source,
            workspace: workspace,
            kind: kind,
            text: text,
            address: address,
            symbol: symbol,
            ordinal: ordinal,
            focusPriority: focusPriority
        )
    }
}

/// Immutable evidence snapshot. It is Sendable so construction can happen away
/// from the main actor and publication can be a single cheap state transition.
struct VGAnalysisEvidenceIndex: Sendable {
    static let maxRecords = 50_000

    let records: [VGEvidenceRecord]
    let focusOrder: [Int]
    let addressIndex: [UInt64: [Int]]
    let symbolIndex: [String: [Int]]
    let sourceRanges: [String: Range<Int>]
    let sourceFocusLimits: [String: Int]
    let truncated: Bool

    static func build(from sources: [VGAnalysisEvidenceSource]) -> VGAnalysisEvidenceIndex {
        var records: [VGEvidenceRecord] = []
        records.reserveCapacity(min(8_192, maxRecords))
        var addressIndex: [UInt64: [Int]] = [:]
        var symbolIndex: [String: [Int]] = [:]
        var sourceRanges: [String: Range<Int>] = [:]
        var sourceFocusLimits: [String: Int] = [:]
        var truncated = false

        sourceLoop: for source in sources {
            sourceFocusLimits[source.name] = source.focusLimit
            let start = records.count
            var ordinal = 0
            for raw in source.text.split(separator: "\n", omittingEmptySubsequences: true) {
                if records.count >= maxRecords { truncated = true; break sourceLoop }
                let line = String(raw).trimmingCharacters(in: .whitespacesAndNewlines)
                guard !line.isEmpty, line.utf8.count <= 16 * 1024 else { continue }
                let address = VGAnalysisNavigation.firstAddress(in: line)
                let symbol = VGAnalysisNavigation.symbol(in: line)
                let semanticID = semanticIdentity(source: source.name, address: address, symbol: symbol, text: line, ordinal: ordinal)
                let record = VGEvidenceRecord(
                    id: semanticID,
                    source: source.name,
                    workspace: source.workspace,
                    kind: source.kind,
                    text: line,
                    normalizedText: line.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current),
                    address: address,
                    symbol: symbol,
                    ordinal: ordinal,
                    focusPriority: source.focusPriority
                )
                let index = records.count
                records.append(record)
                if let address { addressIndex[address, default: []].append(index) }
                if let symbol, !symbol.isEmpty { symbolIndex[symbol.lowercased(), default: []].append(index) }
                ordinal += 1
            }
            let end = records.count
            if end > start { sourceRanges[source.name] = start..<end }
        }

        let focusOrder = records.indices.sorted { lhs, rhs in
            let a = records[lhs], b = records[rhs]
            if a.focusPriority != b.focusPriority { return a.focusPriority > b.focusPriority }
            if a.source != b.source { return a.source < b.source }
            return a.ordinal < b.ordinal
        }
        return VGAnalysisEvidenceIndex(
            records: records,
            focusOrder: focusOrder,
            addressIndex: addressIndex,
            symbolIndex: symbolIndex,
            sourceRanges: sourceRanges,
            sourceFocusLimits: sourceFocusLimits,
            truncated: truncated
        )
    }

    func search(_ query: String, limit: Int = 500) -> [VGAnalysisEvidenceHit] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
            .folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
        guard !needle.isEmpty else { return [] }
        let cap = min(max(limit, 1), 1_000)
        var out: [VGAnalysisEvidenceHit] = []
        out.reserveCapacity(min(cap, 128))
        for record in records where record.normalizedText.contains(needle) {
            out.append(record.hit)
            if out.count >= cap { break }
        }
        return out
    }

    func focus(filter: String = "", limit: Int = 180) -> [VGAnalysisEvidenceHit] {
        let needle = filter.trimmingCharacters(in: .whitespacesAndNewlines)
            .folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
        let cap = min(max(limit, 1), 300)
        var perSource: [String: Int] = [:]
        var out: [VGAnalysisEvidenceHit] = []
        out.reserveCapacity(min(cap, 128))
        for index in focusOrder {
            let record = records[index]
            if !needle.isEmpty, !record.normalizedText.contains(needle) { continue }
            let used = perSource[record.source, default: 0]
            let sourceCap = sourceFocusLimits[record.source, default: 24]
            if used >= sourceCap { continue }
            perSource[record.source] = used + 1
            out.append(record.hit)
            if out.count >= cap { break }
        }
        return out
    }

    func related(evidence: String, selectedSymbol: String, limit: Int = 120) -> [VGAnalysisEvidenceHit] {
        let cap = min(max(limit, 1), 300)
        var candidateIndices = Set<Int>()
        if let address = VGAnalysisNavigation.firstAddress(in: evidence) {
            candidateIndices.formUnion(addressIndex[address] ?? [])
        }
        let symbol = selectedSymbol.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if !symbol.isEmpty { candidateIndices.formUnion(symbolIndex[symbol] ?? []) }
        return candidateIndices.sorted().prefix(cap).map { records[$0].hit }
    }

    func corpus(limit: Int = 25_000) -> [VGAnalysisEvidenceHit] {
        records.prefix(min(max(limit, 1), 50_000)).map(\.hit)
    }

    private static func semanticIdentity(source: String, address: UInt64?, symbol: String?, text: String, ordinal: Int) -> String {
        // Stable FNV-1a over normalized evidence. This is an identity key, not a security digest.
        var hash: UInt64 = 0xcbf29ce484222325
        let normalized = "\(source)|\(ordinal)|\(address.map { String($0) } ?? "-")|\(symbol?.lowercased() ?? "-")|\(text)"
        for byte in normalized.utf8 {
            hash ^= UInt64(byte)
            hash &*= 0x100000001b3
        }
        return String(format: "ev-%016llx", hash)
    }
}

extension VGAnalysisEvidenceIndex {
    static func build(worker bundle: WorkerEvidenceBundle) -> VGAnalysisEvidenceIndex? {
        guard let bundle = bundle.validated() else { return nil }
        var records: [VGEvidenceRecord] = []
        records.reserveCapacity(min(bundle.records.count, maxRecords))
        var addressIndex: [UInt64: [Int]] = [:]
        var symbolIndex: [String: [Int]] = [:]
        var sourceRanges: [String: Range<Int>] = [:]
        var sourceFocusLimits: [String: Int] = [:]
        var sourceStarts: [String: Int] = [:]
        var truncated = bundle.truncated

        for item in bundle.records {
            if records.count >= maxRecords { truncated = true; break }
            guard let kind = VGAnalysisEvidenceKind(rawValue: item.kind) else { continue }
            let address = item.address.flatMap { raw -> UInt64? in
                let text = raw.lowercased().hasPrefix("0x") ? String(raw.dropFirst(2)) : raw
                return UInt64(text, radix: 16)
            }
            let symbol = item.symbol?.isEmpty == false ? item.symbol : nil
            let identity = workerSemanticIdentity(source: item.source, ordinal: item.ordinal, address: address, symbol: symbol, text: item.text)
            let record = VGEvidenceRecord(
                id: identity,
                source: item.source,
                workspace: item.workspace,
                kind: kind,
                text: item.text,
                normalizedText: item.text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current),
                address: address,
                symbol: symbol,
                ordinal: item.ordinal,
                focusPriority: item.focus_priority
            )
            let index = records.count
            sourceStarts[item.source] = sourceStarts[item.source] ?? index
            sourceFocusLimits[item.source] = max(sourceFocusLimits[item.source, default: 0], 24)
            records.append(record)
            if let address { addressIndex[address, default: []].append(index) }
            if let symbol { symbolIndex[symbol.lowercased(), default: []].append(index) }
        }
        for (source, start) in sourceStarts {
            var end = start
            while end < records.count, records[end].source == source { end += 1 }
            if end > start { sourceRanges[source] = start..<end }
        }
        let focusOrder = records.indices.sorted { lhs, rhs in
            let a = records[lhs], b = records[rhs]
            if a.focusPriority != b.focusPriority { return a.focusPriority > b.focusPriority }
            if a.source != b.source { return a.source < b.source }
            return a.ordinal < b.ordinal
        }
        return VGAnalysisEvidenceIndex(records: records, focusOrder: focusOrder, addressIndex: addressIndex,
                                       symbolIndex: symbolIndex, sourceRanges: sourceRanges,
                                       sourceFocusLimits: sourceFocusLimits, truncated: truncated)
    }

    static func build(worker bundle: WorkerEvidenceBundle, compatibilitySources: [VGAnalysisEvidenceSource]) -> VGAnalysisEvidenceIndex? {
        guard let native = build(worker: bundle) else { return nil }
        let nativeSources = Set(native.records.map(\.source))
        let missingSources = compatibilitySources.filter { !nativeSources.contains($0.name) }
        guard !missingSources.isEmpty else { return native }
        let fallback = build(from: missingSources)
        var combined = native.records
        let remaining = max(0, maxRecords - combined.count)
        combined.append(contentsOf: fallback.records.prefix(remaining))
        var addressIndex: [UInt64: [Int]] = [:], symbolIndex: [String: [Int]] = [:]
        var sourceRanges: [String: Range<Int>] = [:], sourceFocusLimits = native.sourceFocusLimits
        for (name, value) in fallback.sourceFocusLimits where sourceFocusLimits[name] == nil { sourceFocusLimits[name] = value }
        var starts: [String: Int] = [:], ends: [String: Int] = [:]
        for (idx, record) in combined.enumerated() {
            starts[record.source] = starts[record.source] ?? idx
            ends[record.source] = idx + 1
            if let address = record.address { addressIndex[address, default: []].append(idx) }
            if let symbol = record.symbol { symbolIndex[symbol.lowercased(), default: []].append(idx) }
        }
        for (source, start) in starts { if let end = ends[source], end > start { sourceRanges[source] = start..<end } }
        let focusOrder = combined.indices.sorted { lhs, rhs in
            let a = combined[lhs], b = combined[rhs]
            if a.focusPriority != b.focusPriority { return a.focusPriority > b.focusPriority }
            if a.source != b.source { return a.source < b.source }
            return a.ordinal < b.ordinal
        }
        return VGAnalysisEvidenceIndex(records: combined, focusOrder: focusOrder, addressIndex: addressIndex,
                                       symbolIndex: symbolIndex, sourceRanges: sourceRanges,
                                       sourceFocusLimits: sourceFocusLimits,
                                       truncated: native.truncated || fallback.truncated || fallback.records.count > remaining)
    }

    private static func workerSemanticIdentity(source: String, ordinal: Int, address: UInt64?, symbol: String?, text: String) -> String {
        var hash: UInt64 = 0xcbf29ce484222325
        let normalized = "\(source)|\(ordinal)|\(address.map { String($0) } ?? "-")|\(symbol?.lowercased() ?? "-")|\(text)"
        for byte in normalized.utf8 { hash ^= UInt64(byte); hash &*= 0x100000001b3 }
        return String(format: "ev-%016llx", hash)
    }
}
