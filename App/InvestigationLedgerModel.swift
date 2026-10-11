import CryptoKit
import Foundation

enum VGLedgerProvenance: String, Codable, CaseIterable, Hashable {
    case observed
    case derived
    case correlated
    case asserted

    var title: String { rawValue.capitalized }
}

enum VGLedgerEntryKind: String, Codable, CaseIterable, Hashable {
    case projectOpened = "project-opened"
    case recordSelected = "record-selected"
    case analysisCompleted = "analysis-completed"
    case workspaceVisited = "workspace-visited"
    case debuggerStop = "debugger-stop"
    case memoryObserved = "memory-observed"
    case breakpointChanged = "breakpoint-changed"
    case noteSaved = "note-saved"
    case hypothesisCreated = "hypothesis-created"
    case hypothesisUpdated = "hypothesis-updated"
    case bookmarkCreated = "bookmark-created"
    case snapshotCreated = "snapshot-created"
    case snapshotRestored = "snapshot-restored"
    case caseExported = "case-exported"
    case extensionTrustChanged = "extension-trust-changed"
    case extensionStateChanged = "extension-state-changed"
    case reaEvidenceCaptured = "rea-evidence-captured"

    var title: String {
        switch self {
        case .projectOpened: return "Project opened"
        case .recordSelected: return "Artifact selected"
        case .analysisCompleted: return "Analysis completed"
        case .workspaceVisited: return "Workspace visited"
        case .debuggerStop: return "Debugger observation"
        case .memoryObserved: return "Memory observed"
        case .breakpointChanged: return "Breakpoint changed"
        case .noteSaved: return "Note saved"
        case .hypothesisCreated: return "Hypothesis created"
        case .hypothesisUpdated: return "Hypothesis updated"
        case .bookmarkCreated: return "Bookmark created"
        case .snapshotCreated: return "Session snapshot"
        case .snapshotRestored: return "Snapshot restored"
        case .caseExported: return "Case exported"
        case .extensionTrustChanged: return "Extension trust changed"
        case .extensionStateChanged: return "Extension state changed"
        case .reaEvidenceCaptured: return "REA evidence captured"
        }
    }
}

enum VGHypothesisStatus: String, Codable, CaseIterable, Hashable {
    case proposed
    case supported
    case contradicted
    case confirmed
    case unresolved

    var title: String { rawValue.capitalized }
}

struct VGLedgerContext: Codable, Hashable {
    var recordID: String?
    var recordName: String?
    var sourceSHA256: String?
    var workspace: String
    var address: String?
    var symbol: String?
    var targetID: String?
}

struct VGLedgerEntry: Codable, Identifiable, Hashable {
    let id: String
    let timestamp: Date
    let kind: VGLedgerEntryKind
    let provenance: VGLedgerProvenance
    let title: String
    let detail: String
    let context: VGLedgerContext

    init(
        id: String = UUID().uuidString,
        timestamp: Date = Date(),
        kind: VGLedgerEntryKind,
        provenance: VGLedgerProvenance,
        title: String? = nil,
        detail: String,
        context: VGLedgerContext
    ) {
        self.id = id
        self.timestamp = timestamp
        self.kind = kind
        self.provenance = provenance
        self.title = title ?? kind.title
        self.detail = detail
        self.context = context
    }
}

struct VGHypothesis: Codable, Identifiable, Hashable {
    let id: String
    let createdAt: Date
    var updatedAt: Date
    var title: String
    var detail: String
    var status: VGHypothesisStatus
    var linkedAddresses: [String]
    var linkedEntryIDs: [String]

    init(
        id: String = UUID().uuidString,
        createdAt: Date = Date(),
        updatedAt: Date = Date(),
        title: String,
        detail: String,
        status: VGHypothesisStatus = .proposed,
        linkedAddresses: [String] = [],
        linkedEntryIDs: [String] = []
    ) {
        self.id = id
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.title = title
        self.detail = detail
        self.status = status
        self.linkedAddresses = linkedAddresses
        self.linkedEntryIDs = linkedEntryIDs
    }
}

struct VGCaseBookmark: Codable, Identifiable, Hashable {
    let id: String
    let timestamp: Date
    var title: String
    var note: String
    let workspace: String
    let address: String?
    let symbol: String?
    let recordID: String?

    init(
        id: String = UUID().uuidString,
        timestamp: Date = Date(),
        title: String,
        note: String,
        workspace: String,
        address: String?,
        symbol: String?,
        recordID: String?
    ) {
        self.id = id
        self.timestamp = timestamp
        self.title = title
        self.note = note
        self.workspace = workspace
        self.address = address
        self.symbol = symbol
        self.recordID = recordID
    }
}

struct VGInvestigationSnapshot: Codable, Identifiable, Hashable {
    let id: String
    let timestamp: Date
    var title: String
    let workspace: String
    let layoutPreset: String
    let address: String
    let cfgFocus: String
    let analysisSearch: String
    let selectedSymbol: String
    let selectedRecordID: String?
    let selectedTargetID: String
    let evidence: String

    init(
        id: String = UUID().uuidString,
        timestamp: Date = Date(),
        title: String,
        workspace: String,
        layoutPreset: String,
        address: String,
        cfgFocus: String,
        analysisSearch: String,
        selectedSymbol: String,
        selectedRecordID: String?,
        selectedTargetID: String,
        evidence: String
    ) {
        self.id = id
        self.timestamp = timestamp
        self.title = title
        self.workspace = workspace
        self.layoutPreset = layoutPreset
        self.address = address
        self.cfgFocus = cfgFocus
        self.analysisSearch = analysisSearch
        self.selectedSymbol = selectedSymbol
        self.selectedRecordID = selectedRecordID
        self.selectedTargetID = selectedTargetID
        self.evidence = evidence
    }
}

struct VGInvestigationLedger: Codable, Hashable {
    static let schemaVersion = 1

    let schema: Int
    let caseID: String
    let createdAt: Date
    var updatedAt: Date
    var projectName: String
    var entries: [VGLedgerEntry]
    var hypotheses: [VGHypothesis]
    var bookmarks: [VGCaseBookmark]
    var snapshots: [VGInvestigationSnapshot]

    init(projectName: String, caseID: String = UUID().uuidString, now: Date = Date()) {
        self.schema = Self.schemaVersion
        self.caseID = caseID
        self.createdAt = now
        self.updatedAt = now
        self.projectName = projectName
        self.entries = []
        self.hypotheses = []
        self.bookmarks = []
        self.snapshots = []
    }

    mutating func append(_ entry: VGLedgerEntry) {
        entries.append(entry)
        if entries.count > 4096 { entries.removeFirst(entries.count - 4096) }
        updatedAt = entry.timestamp
    }

    @discardableResult
    mutating func addHypothesis(title: String, detail: String, address: String?, linkedEntryID: String? = nil, now: Date = Date()) -> VGHypothesis {
        let hypothesis = VGHypothesis(
            createdAt: now,
            updatedAt: now,
            title: String(title.prefix(160)),
            detail: String(detail.prefix(4096)),
            linkedAddresses: address.map { [$0] } ?? [],
            linkedEntryIDs: linkedEntryID.map { [$0] } ?? []
        )
        hypotheses.append(hypothesis)
        if hypotheses.count > 512 { hypotheses.removeFirst(hypotheses.count - 512) }
        updatedAt = now
        return hypothesis
    }

    mutating func setHypothesisStatus(id: String, status: VGHypothesisStatus, now: Date = Date()) -> VGHypothesis? {
        guard let index = hypotheses.firstIndex(where: { $0.id == id }) else { return nil }
        hypotheses[index].status = status
        hypotheses[index].updatedAt = now
        updatedAt = now
        return hypotheses[index]
    }

    @discardableResult
    mutating func addBookmark(_ bookmark: VGCaseBookmark) -> VGCaseBookmark {
        bookmarks.append(bookmark)
        if bookmarks.count > 1024 { bookmarks.removeFirst(bookmarks.count - 1024) }
        updatedAt = bookmark.timestamp
        return bookmark
    }

    @discardableResult
    mutating func addSnapshot(_ snapshot: VGInvestigationSnapshot) -> VGInvestigationSnapshot {
        snapshots.append(snapshot)
        if snapshots.count > 128 { snapshots.removeFirst(snapshots.count - 128) }
        updatedAt = snapshot.timestamp
        return snapshot
    }
}

struct VGCaseArtifactIdentity: Codable, Hashable {
    let id: String
    let name: String
    let sourceSHA256: String?
    let architecture: String
    let size: String
}

struct VGCaseEvidenceSnapshot: Codable, Hashable {
    let summary: String
    let functions: String
    let xrefs: String
    let callGraph: String
    let cfg: String
    let ir: String
    let symbolProvenance: String
    let runtimeGraph: String
    let annotatedDisassembly: String
}

struct VGCaseRuntimeEvent: Codable, Hashable {
    let sequence: Int
    let timestamp: TimeInterval
    let event: String
    let reason: String
    let threadID: String?
    let stopped: Bool
    let pc: String
    let sp: String?
    let fp: String?
    let lr: String?
    let registers: [String: String]
    let breakpoints: [String]
}

struct VGCaseMemoryObservation: Codable, Hashable {
    let sequence: Int
    let timestamp: TimeInterval
    let address: String
    let bytesHex: String
}

struct VGREAAgentEvidence: Codable, Hashable, Identifiable {
    let id: String
    let capturedAt: Date
    let operation: String
    let targetPath: String
    let provider: String
    let responseSHA256: String
    let response: String
}

struct VGCaseIntegrityManifest: Codable, Hashable {
    let algorithm: String
    let payloadSHA256: String
    let artifactCount: Int
    let ledgerEntryCount: Int
    let runtimeEventCount: Int
    let memoryObservationCount: Int
}

struct VGCaseBundle: Codable, Hashable {
    let format: String
    let schema: Int
    let product: String
    let productVersion: String
    let exportedAt: Date
    let ledger: VGInvestigationLedger
    let artifacts: [VGCaseArtifactIdentity]
    let evidence: VGCaseEvidenceSnapshot?
    let runtime: [VGCaseRuntimeEvent]
    let memory: [VGCaseMemoryObservation]
    /// Optional so older case bundles remain decodable.
    var reaEvidence: [VGREAAgentEvidence]? = nil
    /// Optional for backward compatibility with schema-1 bundles exported before
    /// integrity manifests were introduced.
    let integrity: VGCaseIntegrityManifest?

    init(format: String, schema: Int, product: String, productVersion: String, exportedAt: Date,
         ledger: VGInvestigationLedger, artifacts: [VGCaseArtifactIdentity], evidence: VGCaseEvidenceSnapshot?,
         runtime: [VGCaseRuntimeEvent], memory: [VGCaseMemoryObservation],
         reaEvidence: [VGREAAgentEvidence]? = nil, integrity: VGCaseIntegrityManifest? = nil) {
        self.format = format; self.schema = schema; self.product = product; self.productVersion = productVersion
        self.exportedAt = exportedAt; self.ledger = ledger; self.artifacts = artifacts; self.evidence = evidence
        self.runtime = runtime; self.memory = memory; self.reaEvidence = reaEvidence; self.integrity = integrity
    }

    private enum CodingKeys: String, CodingKey {
        case format, schema, product, productVersion, exportedAt, ledger, artifacts, evidence, runtime, memory, reaEvidence, integrity
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        format = try c.decode(String.self, forKey: .format); schema = try c.decode(Int.self, forKey: .schema)
        product = try c.decode(String.self, forKey: .product); productVersion = try c.decode(String.self, forKey: .productVersion)
        exportedAt = try c.decode(Date.self, forKey: .exportedAt); ledger = try c.decode(VGInvestigationLedger.self, forKey: .ledger)
        artifacts = try c.decode([VGCaseArtifactIdentity].self, forKey: .artifacts); evidence = try c.decodeIfPresent(VGCaseEvidenceSnapshot.self, forKey: .evidence)
        runtime = try c.decode([VGCaseRuntimeEvent].self, forKey: .runtime); memory = try c.decode([VGCaseMemoryObservation].self, forKey: .memory)
        reaEvidence = try c.decodeIfPresent([VGREAAgentEvidence].self, forKey: .reaEvidence)
        integrity = try c.decodeIfPresent(VGCaseIntegrityManifest.self, forKey: .integrity)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(format, forKey: .format); try c.encode(schema, forKey: .schema); try c.encode(product, forKey: .product)
        try c.encode(productVersion, forKey: .productVersion); try c.encode(exportedAt, forKey: .exportedAt); try c.encode(ledger, forKey: .ledger)
        try c.encode(artifacts, forKey: .artifacts); try c.encodeIfPresent(evidence, forKey: .evidence); try c.encode(runtime, forKey: .runtime)
        try c.encode(memory, forKey: .memory); try c.encodeIfPresent(reaEvidence, forKey: .reaEvidence); try c.encodeIfPresent(integrity, forKey: .integrity)
    }
}

struct VGLedgerLoadResult {
    let ledger: VGInvestigationLedger
    let recoveredFromBackup: Bool
}

enum VGLedgerStore {
    static let relativeDirectory = ".vectorglass"
    static let filename = "investigation-ledger-v1.json"
    static let backupFilename = "investigation-ledger-v1.backup.json"
    static let maxLedgerBytes = 8 * 1024 * 1024

    static func ledgerURL(projectPath: String) -> URL {
        URL(fileURLWithPath: projectPath, isDirectory: true)
            .appendingPathComponent(relativeDirectory, isDirectory: true)
            .appendingPathComponent(filename, isDirectory: false)
    }

    static func backupURL(projectPath: String) -> URL {
        URL(fileURLWithPath: projectPath, isDirectory: true)
            .appendingPathComponent(relativeDirectory, isDirectory: true)
            .appendingPathComponent(backupFilename, isDirectory: false)
    }

    private static func storageDirectory(projectPath: String, create: Bool) throws -> URL? {
        let fm = FileManager.default
        let root = URL(fileURLWithPath: projectPath, isDirectory: true).standardizedFileURL
        let rootValues = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard rootValues.isDirectory == true, rootValues.isSymbolicLink != true else {
            throw NSError(domain: "HindSightLedger", code: 4, userInfo: [NSLocalizedDescriptionKey: "Project path must be a real directory, not a symbolic link."])
        }
        let directory = root.appendingPathComponent(relativeDirectory, isDirectory: true)
        if !fm.fileExists(atPath: directory.path) {
            guard create else { return nil }
            try fm.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        }
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw NSError(domain: "HindSightLedger", code: 5, userInfo: [NSLocalizedDescriptionKey: "Case-storage path must be a real directory, not a symbolic link."])
        }
        return directory
    }

    private static func validateLedgerFile(_ url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true else {
            throw NSError(domain: "HindSightLedger", code: 6, userInfo: [NSLocalizedDescriptionKey: "Case ledger must be a regular file, not a symbolic link."])
        }
    }

    private static func decodeLedger(_ data: Data) throws -> VGInvestigationLedger {
        guard data.count <= maxLedgerBytes else {
            throw NSError(domain: "HindSightLedger", code: 1, userInfo: [NSLocalizedDescriptionKey: "Ledger exceeds the 8 MiB safety limit."])
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let value = try decoder.decode(VGInvestigationLedger.self, from: data)
        guard value.schema == VGInvestigationLedger.schemaVersion else {
            throw NSError(domain: "HindSightLedger", code: 2, userInfo: [NSLocalizedDescriptionKey: "Unsupported ledger schema \(value.schema)."] )
        }
        return value
    }

    static func loadRecovering(projectPath: String, projectName: String) throws -> VGLedgerLoadResult {
        let fm = FileManager.default
        guard let directory = try storageDirectory(projectPath: projectPath, create: false) else {
            return VGLedgerLoadResult(ledger: VGInvestigationLedger(projectName: projectName), recoveredFromBackup: false)
        }
        let url = directory.appendingPathComponent(filename, isDirectory: false)
        let backup = directory.appendingPathComponent(backupFilename, isDirectory: false)
        try validateLedgerFile(url)
        try validateLedgerFile(backup)
        if fm.fileExists(atPath: url.path) {
            do {
                return VGLedgerLoadResult(ledger: try decodeLedger(Data(contentsOf: url, options: [.mappedIfSafe])), recoveredFromBackup: false)
            } catch {
                guard fm.fileExists(atPath: backup.path) else { throw error }
                let recovered = try decodeLedger(Data(contentsOf: backup, options: [.mappedIfSafe]))
                return VGLedgerLoadResult(ledger: recovered, recoveredFromBackup: true)
            }
        }
        if fm.fileExists(atPath: backup.path) {
            return VGLedgerLoadResult(ledger: try decodeLedger(Data(contentsOf: backup, options: [.mappedIfSafe])), recoveredFromBackup: true)
        }
        return VGLedgerLoadResult(ledger: VGInvestigationLedger(projectName: projectName), recoveredFromBackup: false)
    }

    static func load(projectPath: String, projectName: String) throws -> VGInvestigationLedger {
        try loadRecovering(projectPath: projectPath, projectName: projectName).ledger
    }

    static func save(_ ledger: VGInvestigationLedger, projectPath: String) throws {
        guard let directory = try storageDirectory(projectPath: projectPath, create: true) else {
            throw NSError(domain: "HindSightLedger", code: 7, userInfo: [NSLocalizedDescriptionKey: "Case-storage directory is unavailable."])
        }
        let url = directory.appendingPathComponent(filename, isDirectory: false)
        let backup = directory.appendingPathComponent(backupFilename, isDirectory: false)
        let fm = FileManager.default
        try validateLedgerFile(url)
        try validateLedgerFile(backup)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(ledger)
        guard data.count <= maxLedgerBytes else {
            throw NSError(domain: "HindSightLedger", code: 3, userInfo: [NSLocalizedDescriptionKey: "Ledger serialization exceeds the 8 MiB safety limit."])
        }
        // Preserve the last known-good primary before replacing it. A corrupt primary
        // never overwrites the recovery copy.
        if fm.fileExists(atPath: url.path),
           let current = try? Data(contentsOf: url, options: [.mappedIfSafe]),
           (try? decodeLedger(current)) != nil {
            try current.write(to: backup, options: .atomic)
        }
        try data.write(to: url, options: .atomic)
    }

    static func encodeCase(_ bundle: VGCaseBundle) throws -> Data {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        let payload = VGCaseBundle(
            format: bundle.format,
            schema: bundle.schema,
            product: bundle.product,
            productVersion: bundle.productVersion,
            exportedAt: bundle.exportedAt,
            ledger: bundle.ledger,
            artifacts: bundle.artifacts,
            evidence: bundle.evidence,
            runtime: bundle.runtime,
            memory: bundle.memory,
            reaEvidence: bundle.reaEvidence,
            integrity: nil
        )
        let payloadData = try encoder.encode(payload)
        let manifest = VGCaseIntegrityManifest(
            algorithm: "SHA-256",
            payloadSHA256: SHA256.hash(data: payloadData).map { String(format: "%02x", $0) }.joined(),
            artifactCount: payload.artifacts.count,
            ledgerEntryCount: payload.ledger.entries.count,
            runtimeEventCount: payload.runtime.count,
            memoryObservationCount: payload.memory.count
        )
        let complete = VGCaseBundle(
            format: payload.format,
            schema: payload.schema,
            product: payload.product,
            productVersion: payload.productVersion,
            exportedAt: payload.exportedAt,
            ledger: payload.ledger,
            artifacts: payload.artifacts,
            evidence: payload.evidence,
            runtime: payload.runtime,
            memory: payload.memory,
            reaEvidence: payload.reaEvidence,
            integrity: manifest
        )
        return try encoder.encode(complete)
    }

    static func decodeCase(_ data: Data) throws -> VGCaseBundle {
        guard data.count <= 32 * 1024 * 1024 else {
            throw NSError(domain: "HindSightCase", code: 1, userInfo: [NSLocalizedDescriptionKey: "Case bundle exceeds the 32 MiB safety limit."])
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let bundle = try decoder.decode(VGCaseBundle.self, from: data)
        guard bundle.format == "com.zensoftware.vectorglass.case", bundle.schema == 1,
              bundle.runtime.count <= 256, bundle.memory.count <= 64, bundle.artifacts.count <= 256,
              (bundle.reaEvidence ?? []).count <= 32 else {
            throw NSError(domain: "HindSightCase", code: 2, userInfo: [NSLocalizedDescriptionKey: "Unsupported or oversized HindSight case bundle."])
        }
        guard bundle.runtime.allSatisfy({ event in
            event.pc.utf8.count <= 32 && event.reason.utf8.count <= 4096 && event.event.utf8.count <= 128 &&
            event.registers.count <= 256 && event.registers.keys.allSatisfy({ $0.utf8.count <= 128 }) &&
            event.registers.values.allSatisfy({ $0.utf8.count <= 256 }) && event.breakpoints.count <= 256
        }), bundle.memory.allSatisfy({ $0.address.utf8.count <= 32 && $0.bytesHex.utf8.count <= 8192 }) else {
            throw NSError(domain: "HindSightCase", code: 3, userInfo: [NSLocalizedDescriptionKey: "Case runtime evidence exceeds a bounded field limit."])
        }
        guard (bundle.reaEvidence ?? []).allSatisfy({ evidence in
            evidence.operation.utf8.count <= 128 && evidence.targetPath.utf8.count <= 16_384 &&
            evidence.provider.utf8.count <= 64 && evidence.responseSHA256.count == 64 &&
            evidence.response.utf8.count <= 2 * 1024 * 1024 &&
            SHA256.hash(data: Data(evidence.response.utf8)).map { String(format: "%02x", $0) }.joined() == evidence.responseSHA256
        }) else {
            throw NSError(domain: "HindSightCase", code: 7, userInfo: [NSLocalizedDescriptionKey: "Case REA evidence failed bounded integrity validation."])
        }
        if let manifest = bundle.integrity {
            let payload = VGCaseBundle(
                format: bundle.format,
                schema: bundle.schema,
                product: bundle.product,
                productVersion: bundle.productVersion,
                exportedAt: bundle.exportedAt,
                ledger: bundle.ledger,
                artifacts: bundle.artifacts,
                evidence: bundle.evidence,
                runtime: bundle.runtime,
                memory: bundle.memory,
                reaEvidence: bundle.reaEvidence,
                integrity: nil
            )
            let encoder = JSONEncoder()
            encoder.dateEncodingStrategy = .iso8601
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
            let payloadData = try encoder.encode(payload)
            let digest = SHA256.hash(data: payloadData).map { String(format: "%02x", $0) }.joined()
            guard manifest.algorithm == "SHA-256",
                  manifest.payloadSHA256 == digest,
                  manifest.artifactCount == bundle.artifacts.count,
                  manifest.ledgerEntryCount == bundle.ledger.entries.count,
                  manifest.runtimeEventCount == bundle.runtime.count,
                  manifest.memoryObservationCount == bundle.memory.count else {
                throw NSError(domain: "HindSightCase", code: 6, userInfo: [NSLocalizedDescriptionKey: "Case integrity manifest does not match the exported evidence payload."])
            }
        }
        return bundle
    }

    static func pdf(_ bundle: VGCaseBundle) -> Data {
        func ascii(_ value: String) -> String {
            String(value.unicodeScalars.map { scalar in
                let v = scalar.value
                return (v >= 32 && v <= 126) ? Character(String(scalar)) : Character("?")
            })
        }
        func escape(_ value: String) -> String {
            ascii(value)
                .replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "(", with: "\\(")
                .replacingOccurrences(of: ")", with: "\\)")
        }
        func wrapped(_ line: String, width: Int = 88) -> [String] {
            if line.isEmpty { return [""] }
            var result: [String] = []
            var rest = line[...]
            while rest.count > width {
                let end = rest.index(rest.startIndex, offsetBy: width)
                let prefix = rest[..<end]
                if let split = prefix.lastIndex(of: " "), split > rest.startIndex {
                    result.append(String(rest[..<split]))
                    rest = rest[rest.index(after: split)...]
                } else {
                    result.append(String(prefix))
                    rest = rest[end...]
                }
            }
            result.append(String(rest))
            return result
        }

        let reportLines = markdown(bundle)
            .split(separator: "\n", omittingEmptySubsequences: false)
            .flatMap { wrapped(String($0)) }
        let linesPerPage = 50
        let pageCount = max(1, Int(ceil(Double(reportLines.count) / Double(linesPerPage))))
        var objects: [String] = []
        let pageObjectIDs = (0..<pageCount).map { 4 + ($0 * 2) }
        objects.append("<< /Type /Catalog /Pages 2 0 R >>")
        objects.append("<< /Type /Pages /Count \(pageCount) /Kids [\(pageObjectIDs.map { "\($0) 0 R" }.joined(separator: " "))] >>")
        objects.append("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
        for pageIndex in 0..<pageCount {
            let pageObject = 4 + pageIndex * 2
            let contentObject = pageObject + 1
            let start = pageIndex * linesPerPage
            let end = min(reportLines.count, start + linesPerPage)
            let pageLines = start < end ? Array(reportLines[start..<end]) : ["HindSight Investigation"]
            var stream = "BT\n/F1 9 Tf\n54 738 Td\n13 TL\n"
            for line in pageLines {
                stream += "(\(escape(line))) Tj\nT*\n"
            }
            stream += "ET\n"
            let streamLength = stream.utf8.count
            objects.append("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents \(contentObject) 0 R >>")
            objects.append("<< /Length \(streamLength) >>\nstream\n\(stream)endstream")
        }

        var data = Data("%PDF-1.4\n%HindSight\n".utf8)
        var offsets: [Int] = [0]
        for (index, object) in objects.enumerated() {
            offsets.append(data.count)
            data.append(Data("\(index + 1) 0 obj\n\(object)\nendobj\n".utf8))
        }
        let xrefOffset = data.count
        data.append(Data("xref\n0 \(objects.count + 1)\n".utf8))
        data.append(Data("0000000000 65535 f \n".utf8))
        for offset in offsets.dropFirst() {
            data.append(Data(String(format: "%010d 00000 n \n", offset).utf8))
        }
        data.append(Data("trailer\n<< /Size \(objects.count + 1) /Root 1 0 R >>\nstartxref\n\(xrefOffset)\n%%EOF\n".utf8))
        return data
    }

    static func markdown(_ bundle: VGCaseBundle) -> String {
        var lines: [String] = []
        lines.append("# HindSight Investigation — \(bundle.ledger.projectName)")
        lines.append("")
        lines.append("- Case ID: `\(bundle.ledger.caseID)`")
        lines.append("- Exported: \(ISO8601DateFormatter().string(from: bundle.exportedAt))")
        lines.append("- Product: \(bundle.product) \(bundle.productVersion)")
        lines.append("- Artifacts: \(bundle.artifacts.count)")
        lines.append("- Timeline entries: \(bundle.ledger.entries.count)")
        lines.append("- Hypotheses: \(bundle.ledger.hypotheses.count)")
        lines.append("- Bookmarks: \(bundle.ledger.bookmarks.count)")
        lines.append("")
        lines.append("## Artifacts")
        for artifact in bundle.artifacts {
            lines.append("- **\(artifact.name)** — \(artifact.architecture), \(artifact.size), SHA-256 `\(artifact.sourceSHA256 ?? "unavailable")`")
        }
        lines.append("")
        lines.append("## Hypotheses")
        if bundle.ledger.hypotheses.isEmpty { lines.append("No hypotheses recorded.") }
        for hypothesis in bundle.ledger.hypotheses {
            lines.append("### \(hypothesis.title)")
            lines.append("Status: **\(hypothesis.status.title)**")
            if !hypothesis.linkedAddresses.isEmpty { lines.append("Addresses: \(hypothesis.linkedAddresses.map { "`\($0)`" }.joined(separator: ", "))") }
            lines.append("")
            lines.append(hypothesis.detail)
            lines.append("")
        }
        lines.append("## Bookmarks")
        if bundle.ledger.bookmarks.isEmpty { lines.append("No bookmarks recorded.") }
        for bookmark in bundle.ledger.bookmarks {
            let location = [bookmark.workspace, bookmark.address].compactMap { $0 }.joined(separator: " · ")
            lines.append("- **\(bookmark.title)** — \(location)\(bookmark.note.isEmpty ? "" : " — \(bookmark.note)")")
        }
        lines.append("")
        lines.append("## Timeline")
        let formatter = ISO8601DateFormatter()
        for entry in bundle.ledger.entries {
            let location = entry.context.address.map { " @ `\($0)`" } ?? ""
            lines.append("- \(formatter.string(from: entry.timestamp)) — **\(entry.title)** [\(entry.provenance.title)]\(location): \(entry.detail)")
        }
        lines.append("")
        lines.append("## Evidence boundary")
        lines.append("Observed runtime facts, derived static analysis, evidence-backed correlations, and analyst assertions remain separate provenance classes. A hypothesis status is an analyst judgement, not machine proof.")
        return lines.joined(separator: "\n") + "\n"
    }
}
