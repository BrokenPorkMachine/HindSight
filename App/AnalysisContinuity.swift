import AppKit
import Foundation

private func vgBoundedUTF8(_ text: String, maxBytes: Int) -> String {
    guard maxBytes > 0 else { return "" }
    var result = ""
    result.reserveCapacity(min(text.utf8.count, maxBytes))
    for character in text {
        let piece = String(character)
        if result.utf8.count + piece.utf8.count > maxBytes { break }
        result.append(character)
    }
    return result
}

func vgContinuityFingerprint(_ text: String) -> String {
    var value: UInt64 = 0xcbf29ce484222325
    for byte in text.utf8 {
        value ^= UInt64(byte)
        value &*= 0x100000001b3
    }
    return String(format: "%016llx", value)
}

struct VGAnalysisPin: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    let source: String
    let workspace: String
    let kind: String
    let text: String
    let address: UInt64?
    let symbol: String?
    let createdAt: Date

    init(recordID: String, hit: VGAnalysisEvidenceHit, createdAt: Date = Date()) {
        self.id = "\(recordID)|\(hit.source)|\(vgContinuityFingerprint(hit.text))"
        self.recordID = recordID
        self.source = hit.source
        self.workspace = hit.workspace
        self.kind = hit.kind.rawValue
        self.text = String(hit.text.prefix(3_072))
        self.address = hit.address
        self.symbol = hit.symbol.map { String($0.prefix(512)) }
        self.createdAt = createdAt
    }

    var hit: VGAnalysisEvidenceHit {
        VGAnalysisEvidenceHit(
            id: id,
            source: source,
            workspace: workspace,
            kind: VGAnalysisEvidenceKind(rawValue: kind) ?? .other,
            text: text,
            address: address,
            symbol: symbol,
            ordinal: 0,
            focusPriority: 0
        )
    }
}

enum VGAnalysisConfidence: String, Codable, CaseIterable, Identifiable, Hashable {
    case unclassified = "Unclassified"
    case confirmed = "Confirmed"
    case likely = "Likely"
    case uncertain = "Uncertain"

    var id: String { rawValue }
}

struct VGAnalysisAnnotation: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    let evidence: String
    var note: String
    var tags: [String]
    var confidence: VGAnalysisConfidence
    var updatedAt: Date

    init(
        recordID: String,
        evidence: String,
        note: String,
        tags: [String],
        confidence: VGAnalysisConfidence,
        updatedAt: Date = Date()
    ) {
        self.id = "\(recordID)|\(vgContinuityFingerprint(evidence))"
        self.recordID = recordID
        self.evidence = String(evidence.prefix(3_072))
        self.note = vgBoundedUTF8(note.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 2_048)
        self.tags = Array(tags.map { vgBoundedUTF8($0.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 64) }.filter { !$0.isEmpty }.prefix(16))
        self.confidence = confidence
        self.updatedAt = updatedAt
    }
}

struct VGDecompilerFunctionOverride: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    let address: UInt64
    var analystName: String
    var signatureHypothesis: String
    var note: String
    var confidence: VGAnalysisConfidence
    var updatedAt: Date

    init(recordID: String, address: UInt64, analystName: String, signatureHypothesis: String, note: String, confidence: VGAnalysisConfidence, updatedAt: Date = Date()) {
        self.id = "\(recordID)|function|\(String(address, radix: 16))"
        self.recordID = recordID
        self.address = address
        self.analystName = vgBoundedUTF8(analystName.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 128)
        self.signatureHypothesis = vgBoundedUTF8(signatureHypothesis.split(whereSeparator: \.isNewline).joined(separator: " ").trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 512)
        self.note = vgBoundedUTF8(note.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 1_024)
        self.confidence = confidence
        self.updatedAt = updatedAt
    }
}

struct VGDecompilerFunctionOverrideRevision: Codable, Hashable {
    let analystName: String
    let signatureHypothesis: String
    let note: String
    let confidence: VGAnalysisConfidence
    let updatedAt: Date

    init(_ value: VGDecompilerFunctionOverride) {
        self.init(analystName: value.analystName, signatureHypothesis: value.signatureHypothesis, note: value.note, confidence: value.confidence, updatedAt: value.updatedAt)
    }

    init(analystName: String, signatureHypothesis: String, note: String, confidence: VGAnalysisConfidence, updatedAt: Date) {
        self.analystName = analystName
        self.signatureHypothesis = signatureHypothesis
        self.note = note
        self.confidence = confidence
        self.updatedAt = updatedAt
    }

    var isEmpty: Bool { analystName.isEmpty && signatureHypothesis.isEmpty && note.isEmpty && confidence == .unclassified }
}

struct VGAnalysisSavedSearch: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    var name: String
    var query: String
    let createdAt: Date
    var updatedAt: Date

    init(recordID: String, name: String, query: String, createdAt: Date = Date()) {
        self.id = UUID().uuidString
        self.recordID = recordID
        self.name = vgBoundedUTF8(name.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 96)
        self.query = vgBoundedUTF8(query.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 256)
        self.createdAt = createdAt
        self.updatedAt = createdAt
    }
}

struct VGAnalysisVisit: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    let source: String
    let workspace: String
    let kind: String
    let text: String
    let address: UInt64?
    let symbol: String?
    let destination: String
    let visitedAt: Date

    init(recordID: String, hit: VGAnalysisEvidenceHit, destination: String, visitedAt: Date = Date()) {
        self.id = UUID().uuidString
        self.recordID = recordID
        self.source = hit.source
        self.workspace = hit.workspace
        self.kind = hit.kind.rawValue
        self.text = String(hit.text.prefix(3_072))
        self.address = hit.address
        self.symbol = hit.symbol.map { String($0.prefix(512)) }
        self.destination = String(destination.prefix(64))
        self.visitedAt = visitedAt
    }

    var hit: VGAnalysisEvidenceHit {
        VGAnalysisEvidenceHit(
            id: id,
            source: source,
            workspace: workspace,
            kind: VGAnalysisEvidenceKind(rawValue: kind) ?? .other,
            text: text,
            address: address,
            symbol: symbol,
            ordinal: 0,
            focusPriority: 0
        )
    }
}


struct VGInvestigationCollection: Codable, Identifiable, Hashable {
    let id: String
    let recordID: String
    var name: String
    var pinIDs: [String]
    let createdAt: Date
    var updatedAt: Date

    init(recordID: String, name: String, createdAt: Date = Date()) {
        self.id = UUID().uuidString
        self.recordID = recordID
        self.name = String(name.trimmingCharacters(in: .whitespacesAndNewlines).prefix(96))
        self.pinIDs = []
        self.createdAt = createdAt
        self.updatedAt = createdAt
    }
}

struct VGInvestigationCollectionHealth: Hashable {
    let total: Int
    let current: Int?

    var preserved: Int { max(0, total - (current ?? total)) }
    var label: String {
        guard let current else { return "analysis not loaded" }
        if total == 0 { return "empty" }
        if current == total { return "\(current)/\(total) current" }
        return "\(current)/\(total) current · \(preserved) preserved"
    }
}

struct VGInvestigationReviewBaseline: Codable, Hashable {
    let collectionID: String
    let recordID: String
    let sourceSHA256: String?
    let reviewedAt: Date
    let memberPinIDs: [String]
    let currentPinIDs: [String]?
}

struct VGInvestigationReviewDrift: Hashable {
    let reviewedAt: Date
    let addedMembers: Int
    let removedMembers: Int
    let becameCurrent: Int?
    let becamePreserved: Int?

    var hasMembershipDrift: Bool { addedMembers > 0 || removedMembers > 0 }
    var hasPresenceDrift: Bool { (becameCurrent ?? 0) > 0 || (becamePreserved ?? 0) > 0 }
    var label: String {
        var parts: [String] = []
        if addedMembers > 0 { parts.append("+\(addedMembers) findings") }
        if removedMembers > 0 { parts.append("−\(removedMembers) findings") }
        if let becameCurrent, becameCurrent > 0 { parts.append("\(becameCurrent) now current") }
        if let becamePreserved, becamePreserved > 0 { parts.append("\(becamePreserved) now preserved") }
        return parts.isEmpty ? "no exact-identity drift" : parts.joined(separator: " · ")
    }
}

struct VGInvestigationCollectionComparison: Hashable {
    let leftName: String
    let rightName: String
    let shared: [VGAnalysisEvidenceHit]
    let leftOnly: [VGAnalysisEvidenceHit]
    let rightOnly: [VGAnalysisEvidenceHit]

    var summary: String {
        "\(shared.count) shared · \(leftOnly.count) only in \(leftName) · \(rightOnly.count) only in \(rightName)"
    }
}

enum VGInvestigationReviewState: String, Codable, CaseIterable, Hashable {
    case open = "Open"
    case inReview = "In Review"
    case followUp = "Follow Up"
    case reviewed = "Reviewed"
    case approved = "Approved"
    case rejected = "Rejected"

    var icon: String {
        switch self {
        case .open: return "circle"
        case .inReview: return "clock"
        case .followUp: return "arrow.triangle.branch"
        case .reviewed: return "checkmark.circle"
        case .approved: return "checkmark.seal.fill"
        case .rejected: return "xmark.seal.fill"
        }
    }
}

struct VGInvestigationReviewEntry: Codable, Identifiable, Hashable {
    let id: String
    let collectionID: String
    let recordID: String
    let state: VGInvestigationReviewState
    let note: String
    let createdAt: Date
    let memberPinIDs: [String]

    init(collectionID: String, recordID: String, state: VGInvestigationReviewState, note: String, memberPinIDs: [String], createdAt: Date = Date()) {
        self.id = UUID().uuidString
        self.collectionID = collectionID
        self.recordID = recordID
        self.state = state
        self.note = vgBoundedUTF8(note.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: VGAnalysisContinuityStore.maximumReviewNoteBytes)
        self.createdAt = createdAt
        self.memberPinIDs = Array(memberPinIDs.prefix(VGAnalysisContinuityStore.maximumPinsPerCollection))
    }
}

enum VGInvestigationHandoffReadiness: String, Hashable {
    case ready = "Ready for handoff"
    case followUpRequired = "Follow-up required"
    case reviewRequired = "Review required"
    case conflict = "Review conflict"

    var icon: String {
        switch self {
        case .ready: return "checkmark.seal.fill"
        case .followUpRequired: return "checklist"
        case .reviewRequired: return "exclamationmark.circle"
        case .conflict: return "exclamationmark.triangle.fill"
        }
    }

    static func evaluate(state: VGInvestigationReviewState,
                         hasBaseline: Bool,
                         baselineMatchesRecord: Bool,
                         baselineMatchesSource: Bool,
                         hasDrift: Bool,
                         openFollowUps: Int) -> Self {
        guard hasBaseline, baselineMatchesRecord, baselineMatchesSource, !hasDrift else { return .conflict }
        guard state == .approved || state == .rejected else { return .reviewRequired }
        return openFollowUps > 0 ? .followUpRequired : .ready
    }
}



enum VGInvestigationFollowUpState: String, Codable, CaseIterable, Hashable {
    case open = "Open"
    case completed = "Completed"

    var icon: String {
        switch self {
        case .open: return "circle"
        case .completed: return "checkmark.circle.fill"
        }
    }
}

struct VGInvestigationFollowUp: Codable, Identifiable, Hashable {
    let id: String
    let collectionID: String
    let recordID: String
    var title: String
    var note: String
    var state: VGInvestigationFollowUpState
    let createdAt: Date
    var completedAt: Date?
    let relatedPinIDs: [String]

    init(collectionID: String, recordID: String, title: String, note: String, relatedPinIDs: [String], createdAt: Date = Date()) {
        self.id = UUID().uuidString
        self.collectionID = collectionID
        self.recordID = recordID
        self.title = vgBoundedUTF8(title.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: VGAnalysisContinuityStore.maximumFollowUpTitleBytes)
        self.note = vgBoundedUTF8(note.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: VGAnalysisContinuityStore.maximumFollowUpNoteBytes)
        self.state = .open
        self.createdAt = createdAt
        self.completedAt = nil
        self.relatedPinIDs = Array(relatedPinIDs.prefix(VGAnalysisContinuityStore.maximumFollowUpEvidenceIDs))
    }
}

struct VGAnalysisChannelFingerprint: Codable, Hashable, Sendable {
    let name: String
    let lineCount: Int
    let fingerprint: String
}

struct VGAnalysisSnapshot: Codable, Hashable, Sendable {
    let recordID: String
    let sourceSHA256: String
    let engine: String
    let capturedAt: Date
    let channels: [VGAnalysisChannelFingerprint]
}

struct VGAnalysisDelta: Codable, Hashable, Sendable {
    let recordID: String
    let observedAt: Date
    let summary: String
    let changedChannels: [String]
    let unchangedChannels: Int
}

struct VGAnalysisContinuityDocument: Codable {
    var schema: Int = 7
    var pins: [VGAnalysisPin] = []
    var recent: [VGAnalysisVisit] = []
    var collections: [VGInvestigationCollection] = []
    var reviewBaselines: [String: VGInvestigationReviewBaseline] = [:]
    var reviewEntries: [VGInvestigationReviewEntry] = []
    var followUps: [VGInvestigationFollowUp] = []
    var snapshots: [String: VGAnalysisSnapshot] = [:]
    var deltas: [String: VGAnalysisDelta] = [:]
    var annotations: [String: VGAnalysisAnnotation] = [:]
    var decompilerFunctionOverrides: [String: VGDecompilerFunctionOverride] = [:]
    var decompilerFunctionOverrideHistory: [String: [VGDecompilerFunctionOverrideRevision]] = [:]
    var savedSearches: [VGAnalysisSavedSearch] = []

    enum CodingKeys: String, CodingKey { case schema, pins, recent, collections, reviewBaselines, reviewEntries, followUps, snapshots, deltas, annotations, decompilerFunctionOverrides, decompilerFunctionOverrideHistory, savedSearches }

    init() {}

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        schema = try container.decodeIfPresent(Int.self, forKey: .schema) ?? 1
        pins = try container.decodeIfPresent([VGAnalysisPin].self, forKey: .pins) ?? []
        recent = try container.decodeIfPresent([VGAnalysisVisit].self, forKey: .recent) ?? []
        collections = try container.decodeIfPresent([VGInvestigationCollection].self, forKey: .collections) ?? []
        reviewBaselines = try container.decodeIfPresent([String: VGInvestigationReviewBaseline].self, forKey: .reviewBaselines) ?? [:]
        reviewEntries = try container.decodeIfPresent([VGInvestigationReviewEntry].self, forKey: .reviewEntries) ?? []
        followUps = try container.decodeIfPresent([VGInvestigationFollowUp].self, forKey: .followUps) ?? []
        snapshots = try container.decodeIfPresent([String: VGAnalysisSnapshot].self, forKey: .snapshots) ?? [:]
        deltas = try container.decodeIfPresent([String: VGAnalysisDelta].self, forKey: .deltas) ?? [:]
        annotations = try container.decodeIfPresent([String: VGAnalysisAnnotation].self, forKey: .annotations) ?? [:]
        decompilerFunctionOverrides = try container.decodeIfPresent([String: VGDecompilerFunctionOverride].self, forKey: .decompilerFunctionOverrides) ?? [:]
        decompilerFunctionOverrideHistory = try container.decodeIfPresent([String: [VGDecompilerFunctionOverrideRevision]].self, forKey: .decompilerFunctionOverrideHistory) ?? [:]
        savedSearches = try container.decodeIfPresent([VGAnalysisSavedSearch].self, forKey: .savedSearches) ?? []
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(7, forKey: .schema)
        try container.encode(pins, forKey: .pins)
        try container.encode(recent, forKey: .recent)
        try container.encode(collections, forKey: .collections)
        try container.encode(reviewBaselines, forKey: .reviewBaselines)
        try container.encode(reviewEntries, forKey: .reviewEntries)
        try container.encode(followUps, forKey: .followUps)
        try container.encode(snapshots, forKey: .snapshots)
        try container.encode(deltas, forKey: .deltas)
        try container.encode(annotations, forKey: .annotations)
        try container.encode(decompilerFunctionOverrides, forKey: .decompilerFunctionOverrides)
        try container.encode(decompilerFunctionOverrideHistory, forKey: .decompilerFunctionOverrideHistory)
        try container.encode(savedSearches, forKey: .savedSearches)
    }

    static var empty: VGAnalysisContinuityDocument { VGAnalysisContinuityDocument() }
}

enum VGAnalysisContinuityStore {
    static let engine = "arm64-analysis-v77-schema120"
    static let maximumBytes = 1 * 1024 * 1024
    static let maximumPins = 128
    static let maximumRecent = 96
    static let maximumCollections = 32
    static let maximumPinsPerCollection = 64
    static let maximumReviewBaselines = 256
    static let maximumReviewEntries = 256
    static let maximumReviewEntriesPerCollection = 64
    static let maximumReviewNoteBytes = 2_048
    static let maximumFollowUps = 128
    static let maximumFollowUpsPerCollection = 32
    static let maximumFollowUpTitleBytes = 256
    static let maximumFollowUpNoteBytes = 1_024
    static let maximumFollowUpEvidenceIDs = 16
    static let maximumAnnotations = 512
    static let maximumDecompilerFunctionOverrides = 32
    static let maximumDecompilerFunctionOverrideHistory = 10
    static let maximumSavedSearches = 64
    private static let directoryName = ".vectorglass"
    private static let fileName = "analysis-continuity-v1.json"

    static func load(projectPath: String) -> VGAnalysisContinuityDocument {
        do {
            guard let directory = try storageDirectory(projectPath: projectPath, create: false) else { return .empty }
            let url = directory.appendingPathComponent(fileName, isDirectory: false)
            guard FileManager.default.fileExists(atPath: url.path) else { return .empty }
            try validateStateFile(url)
            let data = try Data(contentsOf: url, options: [.mappedIfSafe])
            guard !data.isEmpty, data.count <= maximumBytes else { return .empty }
            let decoder = JSONDecoder()
            decoder.dateDecodingStrategy = .iso8601
            var document = try decoder.decode(VGAnalysisContinuityDocument.self, from: data)
            guard [1, 2, 3, 4, 5, 6, 7].contains(document.schema) else { return .empty }
            document.schema = 7
            document.pins = Array(document.pins.filter(validPin).prefix(maximumPins))
            document.recent = Array(document.recent.filter(validVisit).prefix(maximumRecent))
            let validPinIDs = Set(document.pins.map(\.id))
            var collectionsPerRecord: [String: Int] = [:]
            var sanitizedCollections: [VGInvestigationCollection] = []
            for value in document.collections where validCollection(value) {
                let count = collectionsPerRecord[value.recordID, default: 0]
                guard count < maximumCollections, sanitizedCollections.count < 256 else { continue }
                var sanitized = value
                sanitized.pinIDs = Array(value.pinIDs.filter { validPinIDs.contains($0) }.prefix(maximumPinsPerCollection))
                sanitizedCollections.append(sanitized)
                collectionsPerRecord[value.recordID] = count + 1
            }
            document.collections = sanitizedCollections
            let validCollectionIDs = Set(document.collections.map(\.id))
            let collectionRecordIDs = Dictionary(uniqueKeysWithValues: document.collections.map { ($0.id, $0.recordID) })
            document.reviewBaselines = Dictionary(uniqueKeysWithValues: document.reviewBaselines
                .filter { validCollectionIDs.contains($0.key) && validReviewBaseline(key: $0.key, value: $0.value) }
                .prefix(maximumReviewBaselines)
                .map { ($0.key, $0.value) })
            var reviewCounts: [String: Int] = [:]
            document.reviewEntries = Array(document.reviewEntries
                .filter { entry in
                    guard validCollectionIDs.contains(entry.collectionID), validReviewEntry(entry) else { return false }
                    let count = reviewCounts[entry.collectionID, default: 0]
                    guard count < maximumReviewEntriesPerCollection else { return false }
                    reviewCounts[entry.collectionID] = count + 1
                    return true
                }
                .prefix(maximumReviewEntries))
            var followUpCounts: [String: Int] = [:]
            document.followUps = Array(document.followUps
                .filter { item in
                    guard validCollectionIDs.contains(item.collectionID),
                          collectionRecordIDs[item.collectionID] == item.recordID,
                          validFollowUp(item) else { return false }
                    let count = followUpCounts[item.collectionID, default: 0]
                    guard count < maximumFollowUpsPerCollection else { return false }
                    followUpCounts[item.collectionID] = count + 1
                    return true
                }
                .prefix(maximumFollowUps))
            document.snapshots = Dictionary(uniqueKeysWithValues: document.snapshots.filter { validSnapshot(key: $0.key, value: $0.value) }.prefix(256).map { ($0.key, $0.value) })
            document.deltas = Dictionary(uniqueKeysWithValues: document.deltas.filter { validDelta(key: $0.key, value: $0.value) }.prefix(256).map { ($0.key, $0.value) })
            document.annotations = Dictionary(uniqueKeysWithValues: document.annotations
                .filter { validAnnotation(key: $0.key, value: $0.value) }
                .prefix(maximumAnnotations)
                .map { ($0.key, $0.value) })
            document.decompilerFunctionOverrides = Dictionary(uniqueKeysWithValues: document.decompilerFunctionOverrides
                .filter { validDecompilerOverride(key: $0.key, value: $0.value) }
                .prefix(maximumDecompilerFunctionOverrides)
                .map { ($0.key, $0.value) })
            document.decompilerFunctionOverrideHistory = Dictionary(uniqueKeysWithValues: document.decompilerFunctionOverrideHistory
                .filter { $0.key.utf8.count <= 320 && $0.value.count <= maximumDecompilerFunctionOverrideHistory && $0.value.allSatisfy(validDecompilerOverrideRevision) }
                .prefix(maximumDecompilerFunctionOverrides * 2)
                .map { ($0.key, $0.value) })
            document.savedSearches = Array(document.savedSearches.filter(validSavedSearch).prefix(maximumSavedSearches))
            return document
        } catch { return .empty }
    }

    static func save(_ document: VGAnalysisContinuityDocument, projectPath: String) throws {
        guard let directory = try storageDirectory(projectPath: projectPath, create: true) else {
            throw NSError(domain: "HindSight", code: 70, userInfo: [NSLocalizedDescriptionKey: "Analysis continuity storage is unavailable."])
        }
        let target = directory.appendingPathComponent(fileName, isDirectory: false)
        try validateStateFile(target)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(document)
        guard data.count <= maximumBytes else {
            throw NSError(domain: "HindSight", code: 72, userInfo: [NSLocalizedDescriptionKey: "Analysis continuity state exceeds its 1 MiB project-local budget."])
        }
        try data.write(to: target, options: .atomic)
    }

    static func snapshot(recordID: String, sourceSHA256: String, report: AnalysisReport, engineIdentity: String = engine) -> VGAnalysisSnapshot {
        let channels = VGAnalysisNavigation.sources(report).prefix(96).map { source -> VGAnalysisChannelFingerprint in
            let lines = source.text.split(separator: "\n", omittingEmptySubsequences: true).count
            return .init(name: source.name, lineCount: lines, fingerprint: vgContinuityFingerprint(source.text))
        }
        return .init(recordID: recordID, sourceSHA256: sourceSHA256, engine: engineIdentity, capturedAt: Date(), channels: channels)
    }

    static func delta(previous: VGAnalysisSnapshot?, current: VGAnalysisSnapshot) -> VGAnalysisDelta {
        guard let previous else {
            return .init(recordID: current.recordID, observedAt: Date(), summary: "Baseline recorded for this artifact; there is no earlier analysis snapshot to compare.", changedChannels: [], unchangedChannels: 0)
        }
        let old = Dictionary(uniqueKeysWithValues: previous.channels.map { ($0.name, $0) })
        let now = Dictionary(uniqueKeysWithValues: current.channels.map { ($0.name, $0) })
        let names = Set(old.keys).union(now.keys).sorted()
        let changed = names.filter { old[$0] != now[$0] }
        let changedDetails = changed.map { name -> String in
            let before = old[name]?.lineCount ?? 0
            let after = now[name]?.lineCount ?? 0
            return "\(name): \(before) → \(after) lines"
        }
        let unchanged = names.count - changed.count
        let summary: String
        if previous.sourceSHA256 != current.sourceSHA256 {
            summary = "Source identity changed; \(changed.count) analysis channels differ and \(unchanged) are unchanged."
        } else if previous.engine != current.engine {
            summary = "Analysis engine changed from \(previous.engine) to \(current.engine); \(changed.count) channels differ."
        } else if changed.isEmpty {
            summary = "No analysis-channel changes detected for the same captured source and engine."
        } else {
            summary = "\(changed.count) analysis channels changed; \(unchanged) are unchanged for the same source identity."
        }
        return .init(recordID: current.recordID, observedAt: Date(), summary: summary, changedChannels: Array(changedDetails.prefix(96)), unchangedChannels: unchanged)
    }

    private static func storageDirectory(projectPath: String, create: Bool) throws -> URL? {
        let fm = FileManager.default
        let root = URL(fileURLWithPath: projectPath, isDirectory: true).standardizedFileURL
        let rootValues = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard rootValues.isDirectory == true, rootValues.isSymbolicLink != true else {
            throw NSError(domain: "HindSight", code: 73, userInfo: [NSLocalizedDescriptionKey: "Project path must be a real directory for analysis continuity state."])
        }
        let directory = root.appendingPathComponent(directoryName, isDirectory: true)
        if !fm.fileExists(atPath: directory.path) {
            guard create else { return nil }
            try fm.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw NSError(domain: "HindSight", code: 74, userInfo: [NSLocalizedDescriptionKey: "Analysis continuity directory must be a real directory, not a symbolic link."])
        }
        return directory
    }

    private static func validateStateFile(_ url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true else {
            throw NSError(domain: "HindSight", code: 75, userInfo: [NSLocalizedDescriptionKey: "Analysis continuity state must be a regular file, not a symbolic link or special file."])
        }
    }

    private static func validPin(_ value: VGAnalysisPin) -> Bool {
        value.id.utf8.count <= 768 && value.recordID.utf8.count <= 256 && value.source.utf8.count <= 256 && value.workspace.utf8.count <= 128 && value.text.utf8.count <= 3_072 && (value.symbol?.utf8.count ?? 0) <= 512
    }

    private static func validVisit(_ value: VGAnalysisVisit) -> Bool {
        value.id.utf8.count <= 256 && value.recordID.utf8.count <= 256 && value.source.utf8.count <= 256 && value.workspace.utf8.count <= 128 && value.text.utf8.count <= 3_072 && value.destination.utf8.count <= 64 && (value.symbol?.utf8.count ?? 0) <= 512
    }

    private static func validCollection(_ value: VGInvestigationCollection) -> Bool {
        value.id.utf8.count <= 256 && value.recordID.utf8.count <= 256 && !value.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && value.name.utf8.count <= 96 && value.pinIDs.count <= maximumPinsPerCollection && value.pinIDs.allSatisfy { $0.utf8.count <= 768 }
    }

    private static func validReviewBaseline(key: String, value: VGInvestigationReviewBaseline) -> Bool {
        guard key == value.collectionID,
              key.utf8.count <= 256,
              value.recordID.utf8.count <= 256,
              (value.sourceSHA256?.utf8.count ?? 0) <= 128,
              value.memberPinIDs.count <= maximumPinsPerCollection,
              (value.currentPinIDs?.count ?? 0) <= maximumPinsPerCollection else { return false }
        guard value.memberPinIDs.allSatisfy({ $0.utf8.count <= 768 }),
              value.currentPinIDs?.allSatisfy({ $0.utf8.count <= 768 }) ?? true else { return false }
        if let currentPinIDs = value.currentPinIDs {
            return Set(currentPinIDs).isSubset(of: Set(value.memberPinIDs))
        }
        return true
    }

    private static func validReviewEntry(_ value: VGInvestigationReviewEntry) -> Bool {
        value.id.utf8.count <= 256 &&
            value.collectionID.utf8.count <= 256 &&
            value.recordID.utf8.count <= 256 &&
            value.note.utf8.count <= maximumReviewNoteBytes &&
            value.memberPinIDs.count <= maximumPinsPerCollection &&
            value.memberPinIDs.allSatisfy { $0.utf8.count <= 768 }
    }

    private static func validFollowUp(_ value: VGInvestigationFollowUp) -> Bool {
        value.id.utf8.count <= 256 &&
            value.collectionID.utf8.count <= 256 &&
            value.recordID.utf8.count <= 256 &&
            !value.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
            value.title.utf8.count <= maximumFollowUpTitleBytes &&
            value.note.utf8.count <= maximumFollowUpNoteBytes &&
            value.relatedPinIDs.count <= maximumFollowUpEvidenceIDs &&
            value.relatedPinIDs.allSatisfy { $0.utf8.count <= 768 } &&
            (value.state == .completed ? value.completedAt != nil : value.completedAt == nil)
    }

    private static func validSnapshot(key: String, value: VGAnalysisSnapshot) -> Bool {
        guard key == value.recordID, key.utf8.count <= 256, value.sourceSHA256.utf8.count <= 128, value.engine.utf8.count <= 128, value.channels.count <= 96 else { return false }
        return value.channels.allSatisfy { $0.name.utf8.count <= 256 && $0.fingerprint.utf8.count <= 64 && $0.lineCount >= 0 && $0.lineCount <= 10_000_000 }
    }

    private static func validDelta(key: String, value: VGAnalysisDelta) -> Bool {
        guard key == value.recordID, key.utf8.count <= 256, value.summary.utf8.count <= 2_048, value.changedChannels.count <= 96, value.unchangedChannels >= 0 else { return false }
        return value.changedChannels.allSatisfy { $0.utf8.count <= 256 }
    }

    private static func validAnnotation(key: String, value: VGAnalysisAnnotation) -> Bool {
        key == value.id &&
            key.utf8.count <= 768 &&
            value.recordID.utf8.count <= 256 &&
            value.evidence.utf8.count <= 3_072 &&
            value.note.utf8.count <= 2_048 &&
            value.tags.count <= 16 &&
            value.tags.allSatisfy { $0.utf8.count <= 64 }
    }

    private static func validDecompilerOverride(key: String, value: VGDecompilerFunctionOverride) -> Bool {
        key == value.id && key.utf8.count <= 320 && value.recordID.utf8.count <= 256 && value.address != 0
            && value.analystName.utf8.count <= 128 && value.signatureHypothesis.utf8.count <= 512
            && value.note.utf8.count <= 1_024
    }

    private static func validDecompilerOverrideRevision(_ value: VGDecompilerFunctionOverrideRevision) -> Bool {
        value.analystName.utf8.count <= 128 && value.signatureHypothesis.utf8.count <= 512 && value.note.utf8.count <= 1_024
    }

    private static func validSavedSearch(_ value: VGAnalysisSavedSearch) -> Bool {
        !value.name.isEmpty && !value.query.isEmpty && value.id.utf8.count <= 256 && value.recordID.utf8.count <= 256 && value.name.utf8.count <= 96 && value.query.utf8.count <= 256
    }

}

@MainActor
extension WorkbenchModel {
    func loadAnalysisContinuity(projectPath: String) {
        analysisContinuityDocument = VGAnalysisContinuityStore.load(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func refreshAnalysisContinuityProjection() {
        guard let recordID = selectedID else {
            analysisPins = []
            analysisRecentVisits = []
            analysisCollections = []
            analysisReviewBaselines = [:]
            analysisReviewEntries = []
            analysisFollowUps = []
            analysisSavedSearches = []
            analysisDelta = nil
            return
        }
        analysisPins = analysisContinuityDocument.pins.filter { $0.recordID == recordID }
        analysisRecentVisits = analysisContinuityDocument.recent.filter { $0.recordID == recordID }
        analysisCollections = analysisContinuityDocument.collections.filter { $0.recordID == recordID }.sorted { $0.updatedAt > $1.updatedAt }
        analysisReviewBaselines = analysisContinuityDocument.reviewBaselines.filter { $0.value.recordID == recordID }
        analysisReviewEntries = analysisContinuityDocument.reviewEntries.filter { $0.recordID == recordID }.sorted { $0.createdAt > $1.createdAt }
        analysisFollowUps = analysisContinuityDocument.followUps.filter { $0.recordID == recordID }.sorted { lhs, rhs in
            if lhs.state != rhs.state { return lhs.state == .open }
            return lhs.createdAt > rhs.createdAt
        }
        analysisSavedSearches = analysisContinuityDocument.savedSearches
            .filter { $0.recordID == recordID }
            .sorted { $0.updatedAt > $1.updatedAt }
        analysisDelta = analysisContinuityDocument.deltas[recordID]
    }

    func isAnalysisHitPinned(_ hit: VGAnalysisEvidenceHit) -> Bool {
        guard let recordID = selectedID else { return false }
        if analysisContinuityDocument.pins.contains(where: { $0.id == hit.id && $0.recordID == recordID }) { return true }
        let id = "\(recordID)|\(hit.source)|\(vgContinuityFingerprint(hit.text))"
        return analysisContinuityDocument.pins.contains(where: { $0.id == id })
    }

    func toggleAnalysisPin(_ hit: VGAnalysisEvidenceHit) {
        guard let recordID = selectedID, let projectPath = project?.path else { return }
        let pin = VGAnalysisPin(recordID: recordID, hit: hit)
        let existingIndex = analysisContinuityDocument.pins.firstIndex(where: { $0.id == hit.id && $0.recordID == recordID })
            ?? analysisContinuityDocument.pins.firstIndex(where: { $0.id == pin.id })
        if let index = existingIndex {
            let removedID = analysisContinuityDocument.pins[index].id
            analysisContinuityDocument.pins.remove(at: index)
            for collectionIndex in analysisContinuityDocument.collections.indices {
                analysisContinuityDocument.collections[collectionIndex].pinIDs.removeAll { $0 == removedID }
            }
            status = "Unpinned analysis evidence."
        } else {
            analysisContinuityDocument.pins.insert(pin, at: 0)
            if analysisContinuityDocument.pins.count > VGAnalysisContinuityStore.maximumPins {
                analysisContinuityDocument.pins.removeLast(analysisContinuityDocument.pins.count - VGAnalysisContinuityStore.maximumPins)
            }
            status = "Pinned analysis evidence to this project."
        }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func createAnalysisCollection(named rawName: String) {
        guard let recordID = selectedID, let projectPath = project?.path else { return }
        let trimmedName = rawName.trimmingCharacters(in: .whitespacesAndNewlines)
        let name = String(trimmedName.prefix(96))
        guard !name.isEmpty else { status = "Collection name cannot be empty."; return }
        guard analysisContinuityDocument.collections.filter({ $0.recordID == recordID }).count < VGAnalysisContinuityStore.maximumCollections else {
            status = "Collection limit reached (\(VGAnalysisContinuityStore.maximumCollections))."; return
        }
        guard !analysisContinuityDocument.collections.contains(where: { $0.recordID == recordID && $0.name.caseInsensitiveCompare(name) == .orderedSame }) else {
            status = "A collection named ‘\(name)’ already exists for this artifact."; return
        }
        analysisContinuityDocument.collections.insert(VGInvestigationCollection(recordID: recordID, name: name), at: 0)
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Created investigation collection ‘\(String(name.prefix(96)))’."
    }

    func renameAnalysisCollection(_ collectionID: String, to rawName: String) {
        guard let recordID = selectedID, let projectPath = project?.path,
              let index = analysisContinuityDocument.collections.firstIndex(where: { $0.id == collectionID && $0.recordID == recordID }) else { return }
        let name = String(rawName.trimmingCharacters(in: .whitespacesAndNewlines).prefix(96))
        guard !name.isEmpty else { status = "Collection name cannot be empty."; return }
        guard !analysisContinuityDocument.collections.contains(where: { $0.recordID == recordID && $0.id != collectionID && $0.name.caseInsensitiveCompare(name) == .orderedSame }) else {
            status = "A collection named ‘\(name)’ already exists for this artifact."; return
        }
        analysisContinuityDocument.collections[index].name = name
        analysisContinuityDocument.collections[index].updatedAt = Date()
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Renamed investigation collection to ‘\(name)’."
    }

    func deleteAnalysisCollection(_ collectionID: String) {
        guard let projectPath = project?.path else { return }
        analysisContinuityDocument.collections.removeAll { $0.id == collectionID }
        analysisContinuityDocument.reviewBaselines.removeValue(forKey: collectionID)
        analysisContinuityDocument.reviewEntries.removeAll { $0.collectionID == collectionID }
        analysisContinuityDocument.followUps.removeAll { $0.collectionID == collectionID }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Investigation collection removed; pinned evidence was preserved."
    }

    func analysisCollectionContains(_ hit: VGAnalysisEvidenceHit, collectionID: String) -> Bool {
        guard let recordID = selectedID else { return false }
        let pinID = VGAnalysisPin(recordID: recordID, hit: hit).id
        return analysisContinuityDocument.collections.first(where: { $0.id == collectionID && $0.recordID == recordID })?.pinIDs.contains(pinID) == true
    }

    func analysisAnnotation(for hit: VGAnalysisEvidenceHit) -> VGAnalysisAnnotation? {
        guard let recordID = selectedID else { return nil }
        let id = "\(recordID)|\(vgContinuityFingerprint(hit.text))"
        return analysisContinuityDocument.annotations[id]
    }

    func analysisHitMatchesFilter(_ hit: VGAnalysisEvidenceHit, query: String) -> Bool {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return true }
        guard let annotation = analysisAnnotation(for: hit) else { return false }
        return annotation.note.localizedCaseInsensitiveContains(needle) ||
            annotation.tags.contains { $0.localizedCaseInsensitiveContains(needle) } ||
            annotation.confidence.rawValue.localizedCaseInsensitiveContains(needle)
    }

    func saveAnalysisSearch(named rawName: String, query rawQuery: String) {
        guard let recordID = selectedID, let projectPath = project?.path else { return }
        let name = vgBoundedUTF8(rawName.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 96)
        let query = vgBoundedUTF8(rawQuery.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: 256)
        guard !name.isEmpty, !query.isEmpty else {
            status = "Saved search name and filter cannot be empty."
            return
        }
        guard analysisContinuityDocument.savedSearches.filter({ $0.recordID == recordID }).count < VGAnalysisContinuityStore.maximumSavedSearches else {
            status = "Saved search limit reached."
            return
        }
        guard !analysisContinuityDocument.savedSearches.contains(where: { $0.recordID == recordID && $0.name.caseInsensitiveCompare(name) == .orderedSame }) else {
            status = "A saved search with that name already exists for this artifact."
            return
        }
        analysisContinuityDocument.savedSearches.insert(VGAnalysisSavedSearch(recordID: recordID, name: name, query: query), at: 0)
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Saved Triage filter ‘\(name)’."
    }

    func deleteAnalysisSearch(_ searchID: String) {
        guard let projectPath = project?.path else { return }
        let before = analysisContinuityDocument.savedSearches.count
        analysisContinuityDocument.savedSearches.removeAll { $0.id == searchID }
        guard before != analysisContinuityDocument.savedSearches.count else { return }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Removed saved Triage filter."
    }

    func analysisAnnotatedHits(limit: Int = 180) -> [VGAnalysisEvidenceHit] {
        guard let index = currentEvidenceIndex else { return [] }
        return index.corpus(limit: 25_000)
            .filter { analysisAnnotation(for: $0) != nil }
            .prefix(max(1, limit))
            .map { $0 }
    }

    var analysisAnnotationCount: Int {
        guard let recordID = selectedID else { return 0 }
        return analysisContinuityDocument.annotations.values.filter { $0.recordID == recordID }.count
    }

    func toggleAnalysisCollectionMembership(_ hit: VGAnalysisEvidenceHit, collectionID: String) {
        guard let recordID = selectedID, let projectPath = project?.path,
              let collectionIndex = analysisContinuityDocument.collections.firstIndex(where: { $0.id == collectionID && $0.recordID == recordID }) else { return }
        let pin = VGAnalysisPin(recordID: recordID, hit: hit)
        if !analysisContinuityDocument.pins.contains(where: { $0.id == pin.id }) {
            analysisContinuityDocument.pins.insert(pin, at: 0)
            if analysisContinuityDocument.pins.count > VGAnalysisContinuityStore.maximumPins {
                let removed = analysisContinuityDocument.pins.removeLast()
                for index in analysisContinuityDocument.collections.indices { analysisContinuityDocument.collections[index].pinIDs.removeAll { $0 == removed.id } }
            }
        }
        var collection = analysisContinuityDocument.collections[collectionIndex]
        if collection.pinIDs.contains(pin.id) {
            collection.pinIDs.removeAll { $0 == pin.id }
            status = "Removed evidence from ‘\(collection.name)’."
        } else {
            collection.pinIDs.insert(pin.id, at: 0)
            if collection.pinIDs.count > VGAnalysisContinuityStore.maximumPinsPerCollection { collection.pinIDs.removeLast(collection.pinIDs.count - VGAnalysisContinuityStore.maximumPinsPerCollection) }
            status = "Added evidence to ‘\(collection.name)’."
        }
        collection.updatedAt = Date()
        analysisContinuityDocument.collections[collectionIndex] = collection
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func analysisCollectionHits(_ collection: VGInvestigationCollection) -> [VGAnalysisEvidenceHit] {
        let pinsByID = Dictionary(uniqueKeysWithValues: analysisPins.map { ($0.id, $0.hit) })
        return collection.pinIDs.compactMap { pinsByID[$0] }
    }

    func currentAnalysisPinIDs(limit: Int = 25_000) -> Set<String>? {
        guard let recordID = selectedID, let index = currentEvidenceIndex else { return nil }
        return Set(index.corpus(limit: limit).map { VGAnalysisPin(recordID: recordID, hit: $0).id })
    }

    func analysisCollectionHealth(_ collection: VGInvestigationCollection, currentPinIDs: Set<String>?) -> VGInvestigationCollectionHealth {
        guard let currentPinIDs else { return .init(total: collection.pinIDs.count, current: nil) }
        let current = collection.pinIDs.reduce(into: 0) { count, pinID in
            if currentPinIDs.contains(pinID) { count += 1 }
        }
        return .init(total: collection.pinIDs.count, current: current)
    }

    func filteredAnalysisCollections(query: String) -> [VGInvestigationCollection] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return analysisCollections }
        let pinsByID = Dictionary(uniqueKeysWithValues: analysisPins.map { ($0.id, $0) })
        return analysisCollections.filter { collection in
            if collection.name.localizedCaseInsensitiveContains(needle) { return true }
            return collection.pinIDs.contains { pinID in
                guard let pin = pinsByID[pinID] else { return false }
                return pin.text.localizedCaseInsensitiveContains(needle) ||
                    pin.source.localizedCaseInsensitiveContains(needle) ||
                    (pin.symbol?.localizedCaseInsensitiveContains(needle) ?? false)
            }
        }
    }

    func compareAnalysisCollections(_ leftID: String, _ rightID: String) -> VGInvestigationCollectionComparison? {
        guard leftID != rightID,
              let left = analysisCollections.first(where: { $0.id == leftID }),
              let right = analysisCollections.first(where: { $0.id == rightID }) else { return nil }
        let pinsByID = Dictionary(uniqueKeysWithValues: analysisPins.map { ($0.id, $0.hit) })
        let leftSet = Set(left.pinIDs)
        let rightSet = Set(right.pinIDs)
        let sharedIDs = left.pinIDs.filter { rightSet.contains($0) }
        let leftOnlyIDs = left.pinIDs.filter { !rightSet.contains($0) }
        let rightOnlyIDs = right.pinIDs.filter { !leftSet.contains($0) }
        return .init(
            leftName: left.name,
            rightName: right.name,
            shared: sharedIDs.compactMap { pinsByID[$0] },
            leftOnly: leftOnlyIDs.compactMap { pinsByID[$0] },
            rightOnly: rightOnlyIDs.compactMap { pinsByID[$0] }
        )
    }

    func markAnalysisCollectionReviewed(_ collectionID: String) {
        guard let recordID = selectedID,
              let projectPath = project?.path,
              let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return }
        let currentIDs = currentAnalysisPinIDs().map { current -> [String] in
            collection.pinIDs.filter { current.contains($0) }
        }
        analysisContinuityDocument.reviewBaselines[collectionID] = .init(
            collectionID: collectionID,
            recordID: recordID,
            sourceSHA256: selected?.result.source_sha256,
            reviewedAt: Date(),
            memberPinIDs: Array(collection.pinIDs.prefix(VGAnalysisContinuityStore.maximumPinsPerCollection)),
            currentPinIDs: currentIDs
        )
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Recorded an exact-identity review baseline for ‘\(collection.name)’."
    }

    func analysisCollectionReviewDrift(_ collection: VGInvestigationCollection, currentPinIDs: Set<String>?) -> VGInvestigationReviewDrift? {
        guard let baseline = analysisReviewBaselines[collection.id] else { return nil }
        let oldMembers = Set(baseline.memberPinIDs)
        let newMembers = Set(collection.pinIDs)
        let added = newMembers.subtracting(oldMembers).count
        let removed = oldMembers.subtracting(newMembers).count
        var becameCurrent: Int?
        var becamePreserved: Int?
        if let baselineCurrentIDs = baseline.currentPinIDs, let currentPinIDs {
            let commonMembers = oldMembers.intersection(newMembers)
            let oldCurrent = Set(baselineCurrentIDs).intersection(commonMembers)
            let nowCurrent = Set(collection.pinIDs.filter { currentPinIDs.contains($0) }).intersection(commonMembers)
            becameCurrent = nowCurrent.subtracting(oldCurrent).count
            becamePreserved = oldCurrent.subtracting(nowCurrent).count
        }
        return .init(
            reviewedAt: baseline.reviewedAt,
            addedMembers: added,
            removedMembers: removed,
            becameCurrent: becameCurrent,
            becamePreserved: becamePreserved
        )
    }

    func analysisReviewEntries(for collectionID: String) -> [VGInvestigationReviewEntry] {
        analysisReviewEntries.filter { $0.collectionID == collectionID }
    }

    func latestAnalysisReviewState(for collectionID: String) -> VGInvestigationReviewState {
        analysisReviewEntries.first(where: { $0.collectionID == collectionID })?.state ?? .open
    }

    func appendAnalysisReviewEntry(collectionID: String, state: VGInvestigationReviewState, note rawNote: String) {
        guard let recordID = selectedID, let projectPath = project?.path,
              let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return }
        let note = vgBoundedUTF8(rawNote.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: VGAnalysisContinuityStore.maximumReviewNoteBytes)
        guard (state != .approved && state != .rejected) || !note.isEmpty else {
            status = "Approved and Rejected decisions require a bounded rationale note."
            return
        }
        guard !note.isEmpty || state != latestAnalysisReviewState(for: collectionID) else {
            status = "Add a note or change the workflow state before recording another review entry."
            return
        }
        let entry = VGInvestigationReviewEntry(collectionID: collectionID, recordID: recordID, state: state, note: note, memberPinIDs: collection.pinIDs)
        analysisContinuityDocument.reviewEntries.insert(entry, at: 0)
        var perCollection = 0
        analysisContinuityDocument.reviewEntries = analysisContinuityDocument.reviewEntries.filter { value in
            guard value.collectionID == collectionID else { return true }
            perCollection += 1
            return perCollection <= VGAnalysisContinuityStore.maximumReviewEntriesPerCollection
        }
        if analysisContinuityDocument.reviewEntries.count > VGAnalysisContinuityStore.maximumReviewEntries {
            analysisContinuityDocument.reviewEntries.removeLast(analysisContinuityDocument.reviewEntries.count - VGAnalysisContinuityStore.maximumReviewEntries)
        }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Recorded \(state.rawValue.lowercased()) review entry for ‘\(collection.name)’."
    }

    func analysisFollowUps(for collectionID: String, includeCompleted: Bool = true) -> [VGInvestigationFollowUp] {
        analysisFollowUps.filter { $0.collectionID == collectionID && (includeCompleted || $0.state == .open) }
    }

    @discardableResult
    func createAnalysisFollowUp(collectionID: String, title rawTitle: String, note rawNote: String) -> Bool {
        guard let recordID = selectedID, let projectPath = project?.path,
              let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return false }
        let title = vgBoundedUTF8(rawTitle.trimmingCharacters(in: .whitespacesAndNewlines), maxBytes: VGAnalysisContinuityStore.maximumFollowUpTitleBytes)
        guard !title.isEmpty else { status = "Follow-up title cannot be empty."; return false }
        guard analysisContinuityDocument.followUps.count < VGAnalysisContinuityStore.maximumFollowUps else {
            status = "The project has reached the bounded follow-up limit; complete or remove an existing task first."; return false
        }
        let existing = analysisContinuityDocument.followUps.filter { $0.collectionID == collectionID }
        guard existing.count < VGAnalysisContinuityStore.maximumFollowUpsPerCollection else {
            status = "This collection has reached the bounded follow-up limit."; return false
        }
        let openDuplicate = existing.contains { $0.state == .open && $0.title.caseInsensitiveCompare(title) == .orderedSame }
        guard !openDuplicate else { status = "An open follow-up with that title already exists in this collection."; return false }
        let related = Array(collection.pinIDs.prefix(VGAnalysisContinuityStore.maximumFollowUpEvidenceIDs))
        let item = VGInvestigationFollowUp(collectionID: collectionID, recordID: recordID, title: title, note: rawNote, relatedPinIDs: related)
        analysisContinuityDocument.followUps.insert(item, at: 0)
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Added follow-up to ‘\(collection.name)’ with \(related.count) bounded evidence identities."
        return true
    }

    func toggleAnalysisFollowUpCompleted(_ followUpID: String) {
        guard let projectPath = project?.path,
              let index = analysisContinuityDocument.followUps.firstIndex(where: { $0.id == followUpID }) else { return }
        if analysisContinuityDocument.followUps[index].state == .open {
            analysisContinuityDocument.followUps[index].state = .completed
            analysisContinuityDocument.followUps[index].completedAt = Date()
            status = "Follow-up marked completed."
        } else {
            analysisContinuityDocument.followUps[index].state = .open
            analysisContinuityDocument.followUps[index].completedAt = nil
            status = "Follow-up reopened."
        }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func deleteAnalysisFollowUp(_ followUpID: String) {
        guard let projectPath = project?.path else { return }
        let before = analysisContinuityDocument.followUps.count
        analysisContinuityDocument.followUps.removeAll { $0.id == followUpID }
        guard analysisContinuityDocument.followUps.count != before else { return }
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
        status = "Follow-up removed."
    }

    func analysisCollectionReviewSummary(_ collectionID: String) -> String {
        let state = latestAnalysisReviewState(for: collectionID)
        let items = analysisFollowUps(for: collectionID)
        let open = items.filter { $0.state == .open }.count
        let completed = items.count - open
        let journal = analysisReviewEntries(for: collectionID).count
        return "\(state.rawValue) · \(open) open follow-ups · \(completed) completed · \(journal) journal entries"
    }

    func analysisCollectionHandoffReadiness(_ collectionID: String) -> VGInvestigationHandoffReadiness? {
        guard let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return nil }
        let baseline = analysisReviewBaselines[collectionID]
        let source = selected?.result.source_sha256
        let baselineMatchesRecord = baseline.map { $0.recordID == selectedID } ?? false
        let baselineMatchesSource = baseline.map { $0.sourceSHA256 == source } ?? false
        let drift = analysisCollectionReviewDrift(collection, currentPinIDs: currentAnalysisPinIDs())
        let hasDrift = drift?.hasMembershipDrift == true || drift?.hasPresenceDrift == true
        let openFollowUps = analysisFollowUps(for: collectionID, includeCompleted: false).count
        return .evaluate(
            state: latestAnalysisReviewState(for: collectionID),
            hasBaseline: baseline != nil,
            baselineMatchesRecord: baselineMatchesRecord,
            baselineMatchesSource: baselineMatchesSource,
            hasDrift: hasDrift,
            openFollowUps: openFollowUps
        )
    }

    func analysisCollectionHandoffReadinessDetail(_ collectionID: String) -> String? {
        guard let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return nil }
        let baseline = analysisReviewBaselines[collectionID]
        guard let baseline else { return "Create an exact-identity review baseline before handoff." }
        if baseline.recordID != selectedID { return "The review baseline belongs to a different artifact record; refresh the review baseline." }
        if baseline.sourceSHA256 != selected?.result.source_sha256 { return "The artifact source SHA-256 changed since review; refresh the review baseline." }
        if let drift = analysisCollectionReviewDrift(collection, currentPinIDs: currentAnalysisPinIDs()), drift.hasMembershipDrift || drift.hasPresenceDrift {
            return "Exact collection membership or current/preserved presence drifted since review; refresh the review baseline."
        }
        let state = latestAnalysisReviewState(for: collectionID)
        guard state == .approved || state == .rejected else { return "Record an Approved or Rejected decision with a rationale before handoff." }
        let open = analysisFollowUps(for: collectionID, includeCompleted: false).count
        return open == 0 ? "Decision and evidence baseline match the current artifact." : "Resolve or explain the \(open) open follow-up(s) before treating the handoff as ready."
    }

    func analysisCollectionHandoff(_ collectionID: String) -> String? {
        guard let collection = analysisCollections.first(where: { $0.id == collectionID }) else { return nil }
        let currentPinIDs = currentAnalysisPinIDs()
        let health = analysisCollectionHealth(collection, currentPinIDs: currentPinIDs)
        let drift = analysisCollectionReviewDrift(collection, currentPinIDs: currentPinIDs)
        let entries = analysisReviewEntries(for: collectionID)
        let followUps = analysisFollowUps(for: collectionID)
        let hits = analysisCollectionHits(collection)
        let readiness = analysisCollectionHandoffReadiness(collectionID) ?? .reviewRequired
        var lines: [String] = [
            "# HindSight Investigation Handoff",
            "",
            "Collection: \(collection.name)",
            "Artifact: \(selected?.name ?? "Unknown")",
            "Source SHA-256: \(selected?.result.source_sha256 ?? "unknown")",
            "Workflow state: \(latestAnalysisReviewState(for: collectionID).rawValue)",
            "Review summary: \(analysisCollectionReviewSummary(collectionID))",
            "Collection health: \(health.label)",
            "Review drift: \(drift?.label ?? "no review baseline")",
            "Handoff readiness: \(readiness.rawValue)",
            "Readiness detail: \(analysisCollectionHandoffReadinessDetail(collectionID) ?? "unknown")",
            "Generated: \(Date().formatted(date: .abbreviated, time: .shortened))",
            "",
            "## Preserved Evidence"
        ]
        if hits.isEmpty {
            lines.append("- No preserved findings in this collection.")
        } else {
            for hit in hits.prefix(VGAnalysisContinuityStore.maximumPinsPerCollection) {
                let location = hit.addressLabel.map { " @ \($0)" } ?? ""
                lines.append("- [\(hit.source)]\(location) \(hit.text.replacingOccurrences(of: "\n", with: " "))")
            }
        }
        lines.append(contentsOf: ["", "## Analyst Annotations"])
        let annotationIDs = Set(hits.map { "\(selectedID ?? "")|\(vgContinuityFingerprint($0.text))" })
        let annotations = analysisContinuityDocument.annotations.values.filter { annotationIDs.contains($0.id) }.sorted { $0.updatedAt > $1.updatedAt }
        if annotations.isEmpty {
            lines.append("- No analyst annotations recorded.")
        } else {
            for annotation in annotations {
                let tags = annotation.tags.isEmpty ? "" : " · tags: \(annotation.tags.joined(separator: ", "))"
                lines.append("- [\(annotation.confidence.rawValue)\(tags)] \(annotation.note.isEmpty ? "(no note)" : annotation.note.replacingOccurrences(of: "\n", with: " "))")
                lines.append("  Evidence: \(annotation.evidence.replacingOccurrences(of: "\n", with: " "))")
            }
        }
        lines.append(contentsOf: ["", "## Follow-Up Queue"])
        let openFollowUps = followUps.filter { $0.state == .open }
        if openFollowUps.isEmpty {
            lines.append("- No open follow-ups.")
        } else {
            for item in openFollowUps.prefix(32) {
                let detail = item.note.isEmpty ? "" : " — \(item.note.replacingOccurrences(of: "\n", with: " "))"
                lines.append("- [ ] \(item.title)\(detail) (\(item.relatedPinIDs.count) bounded evidence identities)")
            }
        }
        let completedCount = followUps.filter { $0.state == .completed }.count
        if completedCount > 0 { lines.append("- \(completedCount) completed follow-up(s) retained in project continuity state.") }

        lines.append(contentsOf: ["", "## Review Journal"])
        if entries.isEmpty {
            lines.append("- No review journal entries.")
        } else {
            for entry in entries.prefix(16) {
                let note = entry.note.isEmpty ? "(state update)" : entry.note.replacingOccurrences(of: "\n", with: " ")
                lines.append("- \(entry.createdAt.formatted(date: .abbreviated, time: .shortened)) · \(entry.state.rawValue): \(note)")
            }
        }
        lines.append(contentsOf: [
            "",
            "---",
            "This handoff is project-local analyst context plus exact preserved evidence identity. Follow-ups are analyst-authored workflow tasks, not findings. Health and drift are provenance/continuity signals, not vulnerability, severity, correctness, reachability, or semantic-importance judgments."
        ])
        return lines.joined(separator: "\n")
    }

    func copyAnalysisCollectionHandoff(_ collectionID: String) {
        guard let text = analysisCollectionHandoff(collectionID) else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        status = "Investigation handoff copied to the clipboard."
    }

    func recordAnalysisVisit(_ hit: VGAnalysisEvidenceHit, destination: VGAnalysisHitDestination) {
        guard let recordID = selectedID, let projectPath = project?.path else { return }
        let name: String
        switch destination {
        case .source: name = "source"
        case .disassembly: name = "disassembly"
        case .cfg: name = "cfg"
        case .selectedSymbol: name = "symbol"
        }
        let visit = VGAnalysisVisit(recordID: recordID, hit: hit, destination: name)
        analysisContinuityDocument.recent.removeAll { $0.recordID == recordID && $0.source == visit.source && $0.text == visit.text && $0.destination == visit.destination }
        analysisContinuityDocument.recent.insert(visit, at: 0)
        if analysisContinuityDocument.recent.count > VGAnalysisContinuityStore.maximumRecent {
            analysisContinuityDocument.recent.removeLast(analysisContinuityDocument.recent.count - VGAnalysisContinuityStore.maximumRecent)
        }
        analysisHistoryCursor = -1
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func recordAnalysisSnapshot(recordID: String, sourceSHA256: String, report: AnalysisReport) {
        guard let projectPath = project?.path else { return }
        let engineIdentity = project?.records.first { $0.id == recordID }?.result.import_options?.cacheIdentity ?? (VGAnalysisContinuityStore.engine + "-patterns1")
        // Fingerprinting all bounded report channels can still touch several MiB.
        // Keep that deterministic CPU work off the SwiftUI main actor, then publish
        // the small snapshot/delta back into continuity state.
        Task { [weak self] in
            let current = await Task.detached(priority: .utility) {
                VGAnalysisContinuityStore.snapshot(recordID: recordID, sourceSHA256: sourceSHA256, report: report, engineIdentity: engineIdentity)
            }.value
            guard let self, self.project?.path == projectPath else { return }
            let previous = self.analysisContinuityDocument.snapshots[recordID]
            let delta = VGAnalysisContinuityStore.delta(previous: previous, current: current)
            self.analysisContinuityDocument.snapshots[recordID] = current
            self.analysisContinuityDocument.deltas[recordID] = delta
            self.persistAnalysisContinuity(projectPath: projectPath)
            self.refreshAnalysisContinuityProjection()
        }
    }

    func clearAnalysisRecents() {
        guard let recordID = selectedID, let projectPath = project?.path else { return }
        analysisContinuityDocument.recent.removeAll { $0.recordID == recordID }
        analysisHistoryCursor = -1
        persistAnalysisContinuity(projectPath: projectPath)
        refreshAnalysisContinuityProjection()
    }

    func persistAnalysisContinuity(projectPath: String) {
        do { try VGAnalysisContinuityStore.save(analysisContinuityDocument, projectPath: projectPath) }
        catch { status = "Analysis continuity state was not saved: \(error.localizedDescription)" }
    }

    func decompilerFunctionOverrides(for recordID: String) -> [UInt64: VGDecompilerFunctionOverride] {
        Dictionary(uniqueKeysWithValues: analysisContinuityDocument.decompilerFunctionOverrides.values
            .filter { $0.recordID == recordID }
            .map { ($0.address, $0) })
    }

    func saveDecompilerFunctionOverride(recordID: String, address: UInt64, analystName: String, signatureHypothesis: String, note: String, confidence: VGAnalysisConfidence) {
        guard selectedID == recordID, let projectPath = project?.path, address != 0 else { return }
        let value = VGDecompilerFunctionOverride(recordID: recordID, address: address, analystName: analystName, signatureHypothesis: signatureHypothesis, note: note, confidence: confidence)
        let existing = analysisContinuityDocument.decompilerFunctionOverrides[value.id]
        if existing == nil && VGDecompilerFunctionOverrideRevision(value).isEmpty { return }
        guard existing?.analystName != value.analystName || existing?.signatureHypothesis != value.signatureHypothesis
            || existing?.note != value.note || existing?.confidence != value.confidence else { return }
        guard existing != nil || analysisContinuityDocument.decompilerFunctionOverrides.count < VGAnalysisContinuityStore.maximumDecompilerFunctionOverrides else {
            status = "Function override limit reached for this project."
            return
        }
        var history = analysisContinuityDocument.decompilerFunctionOverrideHistory[value.id, default: []]
        history.append(existing.map(VGDecompilerFunctionOverrideRevision.init) ?? VGDecompilerFunctionOverrideRevision(analystName: "", signatureHypothesis: "", note: "", confidence: .unclassified, updatedAt: Date()))
        if history.count > VGAnalysisContinuityStore.maximumDecompilerFunctionOverrideHistory { history.removeFirst(history.count - VGAnalysisContinuityStore.maximumDecompilerFunctionOverrideHistory) }
        analysisContinuityDocument.decompilerFunctionOverrideHistory[value.id] = history
        if VGDecompilerFunctionOverrideRevision(value).isEmpty {
            analysisContinuityDocument.decompilerFunctionOverrides.removeValue(forKey: value.id)
            status = "Removed function override; the previous revision can be restored with Undo."
        } else {
            analysisContinuityDocument.decompilerFunctionOverrides[value.id] = value
            status = "Saved function override with analyst provenance."
        }
        persistAnalysisContinuity(projectPath: projectPath)
    }

    func undoDecompilerFunctionOverride(recordID: String, address: UInt64) {
        guard selectedID == recordID, let projectPath = project?.path, address != 0 else { return }
        let key = "\(recordID)|function|\(String(address, radix: 16))"
        guard var history = analysisContinuityDocument.decompilerFunctionOverrideHistory[key], let previous = history.popLast() else { return }
        if previous.isEmpty {
            analysisContinuityDocument.decompilerFunctionOverrides.removeValue(forKey: key)
        } else {
            analysisContinuityDocument.decompilerFunctionOverrides[key] = VGDecompilerFunctionOverride(recordID: recordID, address: address, analystName: previous.analystName, signatureHypothesis: previous.signatureHypothesis, note: previous.note, confidence: previous.confidence, updatedAt: previous.updatedAt)
        }
        if history.isEmpty { analysisContinuityDocument.decompilerFunctionOverrideHistory.removeValue(forKey: key) }
        else { analysisContinuityDocument.decompilerFunctionOverrideHistory[key] = history }
        status = "Restored the previous function override revision."
        persistAnalysisContinuity(projectPath: projectPath)
    }

    func decompilerFunctionOverrideUndoCounts(for recordID: String) -> [UInt64: Int] {
        let prefix = "\(recordID)|function|"
        return Dictionary(uniqueKeysWithValues: analysisContinuityDocument.decompilerFunctionOverrideHistory.compactMap { entry in
            let (key, history) = entry
            guard key.hasPrefix(prefix), let address = UInt64(key.dropFirst(prefix.count), radix: 16), !history.isEmpty else { return nil }
            return (address, history.count)
        })
    }
}
