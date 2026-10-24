import Foundation
import CryptoKit
import SQLite3
import Darwin
struct WorkbenchError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
    init(_ message: String) { self.message = message }
}
/// Queue-confined SQLite storage adapter. SQLite remains the authoritative project store; Rust is used for bounded analysis/bridge work rather than duplicating transactional storage.
final class ProjectStore {
    private struct LegacyAnalysisCacheEnvelope: Codable {
        let schema: Int
        let productVersion: String
        let sourceSHA256: String
        let report: AnalysisReport
    }
    private struct AnalysisCacheEnvelopeV2: Codable {
        let schema: Int
        let engineVersion: String
        let producerVersion: String
        let sourceSHA256: String
        let report: AnalysisReport
    }
    private struct AnalysisCacheEnvelope: Codable {
        let schema: Int
        let engineVersion: String
        let producerVersion: String
        let sourceSHA256: String
        let report: AnalysisReport
        let evidence: WorkerEvidenceBundle?
    }

    private static let analysisCacheSchema = 3
    private static let maxAnalysisCacheBytes = WorkbenchProtocolLimits.analysisXPCReplyBytes
    private var db: OpaquePointer?
    private var lockFD: Int32 = -1
    let path: String
    private let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
    init(path: String, create: Bool) throws {
        self.path = URL(fileURLWithPath: path).standardizedFileURL.path
        let root = URL(fileURLWithPath: self.path, isDirectory: true)
        let fm = FileManager.default
        if create {
            guard !fm.fileExists(atPath: self.path) else { throw WorkbenchError("Choose a new project directory.") }
            try fm.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        } else {
            var isDirectory: ObjCBool = false
            guard fm.fileExists(atPath: self.path, isDirectory: &isDirectory), isDirectory.boolValue,
                  fm.fileExists(atPath: root.appendingPathComponent("project.sqlite").path) else {
                throw WorkbenchError("This directory is not a HindSight project.")
            }
        }
        let rootValues = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard rootValues.isDirectory == true, rootValues.isSymbolicLink != true else {
            throw WorkbenchError("Project path must be a real directory, not a symbolic link.")
        }
        let databaseURL = root.appendingPathComponent("project.sqlite")
        if !create {
            let databaseValues = try databaseURL.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard databaseValues.isRegularFile == true, databaseValues.isSymbolicLink != true else {
                throw WorkbenchError("Project database must be a regular file, not a symbolic link.")
            }
        }
        do {
            lockFD = Darwin.open(root.appendingPathComponent("writer.lock").path, O_CREAT | O_RDWR | O_NOFOLLOW, S_IRUSR | S_IWUSR)
            guard lockFD >= 0, flock(lockFD, LOCK_EX | LOCK_NB) == 0 else { throw WorkbenchError("Project writer lock unavailable. Close the other writer first.") }
            let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX | (create ? SQLITE_OPEN_CREATE : 0)
            guard sqlite3_open_v2(databaseURL.path, &db, flags, nil) == SQLITE_OK else { throw WorkbenchError("Cannot open project database.") }
            sqlite3_busy_timeout(db, 1000)
            if !create {
                let version = try scalar("PRAGMA user_version")
                let metadataVersion = try scalar("SELECT schema_version FROM project_meta WHERE singleton=1")
                if version == 1 && metadataVersion == 1 {
                    try execute("""
                    BEGIN IMMEDIATE;
                    CREATE TABLE project_type_candidates (
                     id TEXT PRIMARY KEY CHECK(length(id)=64),
                     name TEXT NOT NULL,
                     declaration TEXT NOT NULL,
                     target TEXT NOT NULL,
                     source TEXT NOT NULL,
                     source_sha256 TEXT NOT NULL CHECK(length(source_sha256)=64)
                    );
                    CREATE INDEX project_type_candidates_by_name ON project_type_candidates(name);
                    ALTER TABLE project_meta RENAME TO project_meta_v1;
                    CREATE TABLE project_meta (
                     singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                     schema_version INTEGER NOT NULL CHECK(schema_version=2)
                    );
                    INSERT INTO project_meta VALUES (1,2);
                    DROP TABLE project_meta_v1;
                    PRAGMA user_version=2;
                    COMMIT;
                    """)
                } else if version != 2 || metadataVersion != 2 {
                    throw WorkbenchError("Unsupported project schema. No migration performed.")
                }
            }
            try execute("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-8192;")
            if create {
                guard let schema = Bundle.main.url(forResource: "schema", withExtension: "sql") else { throw WorkbenchError("Missing database schema resource.") }
                try execute(String(contentsOf: schema, encoding: .utf8))
            }
            guard try scalarText("PRAGMA quick_check") == "ok" else {
                throw WorkbenchError("Project database integrity check failed. Open a backup instead of continuing to write this project.")
            }
            try cleanupAbandonedStaging(root: root)
        } catch { close(); throw error }
    }
    deinit { close() }
    private func close() {
        if let db { sqlite3_close_v2(db); self.db = nil }
        if lockFD >= 0 { flock(lockFD, LOCK_UN); Darwin.close(lockFD); lockFD = -1 }
    }
    private func execute(_ sql: String) throws {
        guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else { throw WorkbenchError(String(cString: sqlite3_errmsg(db))) }
    }
    private func prepare(_ sql: String) throws -> OpaquePointer {
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
            throw WorkbenchError(String(cString: sqlite3_errmsg(db)))
        }
        return statement
    }
    private func scalar(_ sql: String) throws -> Int {
        let statement = try prepare(sql); defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW else { throw WorkbenchError("Project metadata is missing.") }
        return Int(sqlite3_column_int64(statement, 0))
    }
    private func scalarText(_ sql: String) throws -> String {
        let statement = try prepare(sql); defer { sqlite3_finalize(statement) }
        guard sqlite3_step(statement) == SQLITE_ROW, let raw = sqlite3_column_text(statement, 0) else {
            throw WorkbenchError("Project database integrity result is unavailable.")
        }
        return String(cString: raw)
    }
    private func cleanupAbandonedStaging(root: URL) throws {
        let staging = root.appendingPathComponent("staging", isDirectory: true)
        let fm = FileManager.default
        if !fm.fileExists(atPath: staging.path) {
            if let values = try? staging.resourceValues(forKeys: [.isSymbolicLinkKey]), values.isSymbolicLink == true {
                throw WorkbenchError("Project staging path must not be a symbolic link.")
            }
            return
        }
        _ = try validatedDirectory(staging, createIfMissing: false, label: "Project staging")
        let entries = try fm.contentsOfDirectory(at: staging, includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey], options: [.skipsHiddenFiles])
        guard entries.count <= 4096 else { throw WorkbenchError("Project staging directory contains too many entries to recover safely.") }
        for url in entries {
            guard url.pathExtension == "partial", UUID(uuidString: url.deletingPathExtension().lastPathComponent) != nil else { continue }
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true else { continue }
            try fm.removeItem(at: url)
        }
    }
    private func bind(_ value: String, _ index: Int32, _ statement: OpaquePointer) throws {
        guard value.utf8.count <= 256 * 1024 else { throw WorkbenchError("Database field exceeds limit.") }
        let code = value.withCString { sqlite3_bind_text(statement, index, $0, Int32(value.utf8.count), transient) }
        guard code == SQLITE_OK else { throw WorkbenchError("Cannot bind database value.") }
    }
    private func text(_ statement: OpaquePointer, _ index: Int32) throws -> String {
        let count = Int(sqlite3_column_bytes(statement, index))
        guard count <= 256 * 1024 else { throw WorkbenchError("Database field exceeds limit.") }
        guard count > 0 else { return "" }
        guard let bytes = sqlite3_column_text(statement, index),
              let value = String(bytes: UnsafeBufferPointer(start: bytes, count: count), encoding: .utf8) else {
            throw WorkbenchError("Database contains invalid UTF-8.")
        }
        return value
    }
    func snapshot() throws -> ProjectSnapshot {
        let statement = try prepare("SELECT id,source_path,name,header_json,note,revision FROM records ORDER BY rowid DESC LIMIT 101")
        defer { sqlite3_finalize(statement) }
        var records: [ProjectRecord] = []
        while true {
            let step = sqlite3_step(statement)
            if step == SQLITE_DONE { break }
            guard step == SQLITE_ROW else { throw WorkbenchError("Cannot read project records.") }
            guard records.count < 100 else { throw WorkbenchError("Project exceeds the 100-record foundation limit.") }
            let result = try JSONDecoder().decode(HeaderResult.self, from: Data(try text(statement, 3).utf8))
            records.append(ProjectRecord(id: try text(statement, 0), path: try text(statement, 1), name: try text(statement, 2), result: result, note: try text(statement, 4), revision: Int(sqlite3_column_int64(statement, 5))))
        }
        let snapshot = ProjectSnapshot(path: path, records: records)
        guard try JSONEncoder().encode(snapshot).count <= 1_500_000 else {
            throw WorkbenchError("Project reply budget exceeded. Reduce record or note size.")
        }
        return snapshot
    }
    func projectTypeCatalog() throws -> ProjectTypeCatalog {
        let statement = try prepare("SELECT id,name,declaration,target,source,source_sha256 FROM project_type_candidates ORDER BY name,id LIMIT 10001")
        defer { sqlite3_finalize(statement) }
        var candidates: [ProjectTypeCandidate] = []
        var estimatedBytes = 0
        while true {
            let step = sqlite3_step(statement)
            if step == SQLITE_DONE { break }
            guard step == SQLITE_ROW, candidates.count < 10_000 else {
                throw WorkbenchError("Project type catalog exceeds its 10,000-candidate limit or could not be read.")
            }
            let storedID = try text(statement, 0)
            let candidate = ProjectTypeCandidate(name: try text(statement, 1), declaration: try text(statement, 2),
                target: try text(statement, 3), source: try text(statement, 4), sourceSHA256: try text(statement, 5))
            let expectedID = SHA256.hash(data: Data(candidate.id.utf8)).map { String(format: "%02x", $0) }.joined()
            guard candidate.validated != nil, storedID == expectedID else {
                throw WorkbenchError("Project type catalog contains invalid or altered evidence.")
            }
            estimatedBytes += candidate.name.utf8.count + candidate.declaration.utf8.count
                + candidate.target.utf8.count + candidate.source.utf8.count + candidate.sourceSHA256.utf8.count + 256
            guard estimatedBytes <= 4 * 1024 * 1024 else { throw WorkbenchError("Project type catalog exceeds its 8 MiB reply budget.") }
            candidates.append(candidate)
        }
        let catalog = ProjectTypeCatalog(candidates: candidates)
        guard catalog.validated() != nil,
              try JSONEncoder().encode(catalog).count <= 8 * 1024 * 1024 else {
            throw WorkbenchError("Project type catalog exceeds its encoding budget.")
        }
        return catalog
    }
    func addProjectTypeCandidate(_ candidate: ProjectTypeCandidate) throws -> ProjectTypeCatalog {
        guard candidate.validated != nil else { throw WorkbenchError("Invalid project type evidence candidate.") }
        let records = try prepare("SELECT header_json FROM records")
        defer { sqlite3_finalize(records) }
        var sourceIsCaptured = false
        var sourceArchitecture: String?
        while true {
            let step = sqlite3_step(records)
            if step == SQLITE_DONE { break }
            guard step == SQLITE_ROW else { throw WorkbenchError("Cannot verify project type source identity.") }
            let header = try JSONDecoder().decode(HeaderResult.self, from: Data(try text(records, 0).utf8))
            if header.captured, header.source_sha256 == candidate.sourceSHA256 {
                sourceIsCaptured = true
                sourceArchitecture = header.architecture
                break
            }
        }
        guard sourceIsCaptured else { throw WorkbenchError("Type evidence must reference a captured source in this project.") }
        let candidateArchitecture = candidate.target.split(separator: "-", maxSplits: 1).first.map(String.init) ?? ""
        let architectureMatches: Bool
        switch sourceArchitecture?.lowercased() {
        case "arm64", "arm64e", "aarch64": architectureMatches = ["arm64", "arm64e", "aarch64"].contains(candidateArchitecture)
        case "x86-64", "x86_64", "intel x86_64": architectureMatches = candidateArchitecture == "x86_64"
        case "i386", "intel i386": architectureMatches = candidateArchitecture == "i386"
        case "arm32": architectureMatches = ["arm", "thumb"].contains(candidateArchitecture)
        default: architectureMatches = false
        }
        guard architectureMatches else {
            throw WorkbenchError("Type evidence target architecture does not match the captured source.")
        }
        let id = SHA256.hash(data: Data(candidate.id.utf8)).map { String(format: "%02x", $0) }.joined()
        let exists = try prepare("SELECT 1 FROM project_type_candidates WHERE id=?")
        defer { sqlite3_finalize(exists) }
        guard sqlite3_bind_text(exists, 1, id, -1, transient) == SQLITE_OK else {
            throw WorkbenchError("Cannot bind project type evidence identity.")
        }
        let existence = sqlite3_step(exists)
        if existence == SQLITE_DONE {
            guard try scalar("SELECT count(*) FROM project_type_candidates") < 10_000 else {
                throw WorkbenchError("Project type catalog reached its 10,000-candidate limit.")
            }
            let insert = try prepare("INSERT INTO project_type_candidates(id,name,declaration,target,source,source_sha256) VALUES(?,?,?,?,?,?)")
            defer { sqlite3_finalize(insert) }
            guard sqlite3_bind_text(insert, 1, id, -1, transient) == SQLITE_OK else { throw WorkbenchError("Cannot bind project type evidence identity.") }
            try bind(candidate.name, 2, insert)
            try bind(candidate.declaration, 3, insert)
            try bind(candidate.target, 4, insert)
            try bind(candidate.source, 5, insert)
            try bind(candidate.sourceSHA256, 6, insert)
            guard sqlite3_step(insert) == SQLITE_DONE else { throw WorkbenchError(String(cString: sqlite3_errmsg(db))) }
        } else if existence != SQLITE_ROW {
            throw WorkbenchError(String(cString: sqlite3_errmsg(db)))
        }
        return try projectTypeCatalog()
    }
    func removeProjectTypeCandidate(_ candidate: ProjectTypeCandidate) throws -> ProjectTypeCatalog {
        guard candidate.validated != nil else { throw WorkbenchError("Invalid project type evidence identity.") }
        let id = SHA256.hash(data: Data(candidate.id.utf8)).map { String(format: "%02x", $0) }.joined()
        let statement = try prepare("DELETE FROM project_type_candidates WHERE id=?")
        defer { sqlite3_finalize(statement) }
        guard sqlite3_bind_text(statement, 1, id, -1, transient) == SQLITE_OK else {
            throw WorkbenchError("Cannot bind project type evidence identity.")
        }
        guard sqlite3_step(statement) == SQLITE_DONE else { throw WorkbenchError(String(cString: sqlite3_errmsg(db))) }
        guard sqlite3_changes(db) == 1 else { throw WorkbenchError("Project type evidence changed; refresh the catalog and retry.") }
        return try projectTypeCatalog()
    }
    private func transaction(_ operation: () throws -> ProjectSnapshot) throws -> ProjectSnapshot {
        try execute("BEGIN IMMEDIATE")
        do { let result = try operation(); try execute("COMMIT"); return result }
        catch { try? execute("ROLLBACK"); throw error }
    }
    private func validatedDirectory(_ url: URL, createIfMissing: Bool, label: String) throws -> URL {
        let fm = FileManager.default
        if let values = try? url.resourceValues(forKeys: [.isSymbolicLinkKey]), values.isSymbolicLink == true {
            throw WorkbenchError("\(label) must be a real directory, not a symbolic link.")
        }
        var isDirectory: ObjCBool = false
        if !fm.fileExists(atPath: url.path, isDirectory: &isDirectory) {
            guard createIfMissing else { throw WorkbenchError("\(label) directory is unavailable.") }
            do {
                try fm.createDirectory(at: url, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
            } catch {
                throw WorkbenchError("Cannot create \(label.lowercased()) directory.")
            }
        }
        let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        guard values.isDirectory == true, values.isSymbolicLink != true else {
            throw WorkbenchError("\(label) must be a real directory, not a symbolic link.")
        }
        return url
    }
    private func objectDirectory(_ name: String) throws -> URL {
        let url = URL(fileURLWithPath: path).appendingPathComponent(name, isDirectory: true)
        return try validatedDirectory(url, createIfMissing: true, label: "Project \(name)")
    }
    func capturedSource(id: String) throws -> (path: String, digest: String, size: String, options: BinaryImportOptions?) {
        let statement = try prepare("SELECT header_json FROM records WHERE id=?")
        defer { sqlite3_finalize(statement) }
        try bind(id, 1, statement)
        guard sqlite3_step(statement) == SQLITE_ROW else { throw WorkbenchError("Captured record is unavailable.") }
        let result = try JSONDecoder().decode(HeaderResult.self, from: Data(try text(statement, 0).utf8))
        guard result.captured,
              let digest = result.source_sha256, digest.count == 64,
              digest.allSatisfy({ "0123456789abcdef".contains($0) }),
              let name = result.object_name, let size = UInt64(result.size), size <= 8_589_934_592 else {
            throw WorkbenchError("Record has no captured object with a valid identity.")
        }
        let components = name.split(separator: "/", omittingEmptySubsequences: false)
        guard components.count == 2, components[0] == "objects",
              components[1].hasPrefix(digest + "-"), components[1].hasSuffix(".bin") else {
            throw WorkbenchError("Invalid captured object reference.")
        }
        let suffix = String(components[1].dropFirst(65).dropLast(4))
        guard UUID(uuidString: suffix) != nil else { throw WorkbenchError("Invalid captured object name.") }
        let directory = URL(fileURLWithPath: path).appendingPathComponent("objects", isDirectory: true)
        _ = try validatedDirectory(directory, createIfMissing: false, label: "Captured object storage")
        let options = result.import_options ?? .automatic
        guard options.validated == options else { throw WorkbenchError("Invalid saved import mapping.") }
        return (directory.appendingPathComponent(String(components[1])).path, digest, String(size), options)
    }
    private func analysisCacheURL(digest: String) throws -> URL {
        guard digest.count == 64, digest.allSatisfy({ "0123456789abcdef".contains($0) }) else {
            throw WorkbenchError("Invalid analysis cache identity.")
        }
        return try objectDirectory("analysis-cache").appendingPathComponent(digest + ".json")
    }

    func cachedAnalysis(digest: String, requestID: String, engineVersion: String, compatibleLegacyProductVersions: Set<String>) throws -> (report: AnalysisReport, evidence: WorkerEvidenceBundle?, legacy: Bool)? {
        guard UUID(uuidString: requestID) != nil, engineVersion.utf8.count <= 64,
              compatibleLegacyProductVersions.count <= 8, compatibleLegacyProductVersions.allSatisfy({ $0.utf8.count <= 64 }) else {
            throw WorkbenchError("Invalid analysis cache request.")
        }
        let url = try analysisCacheURL(digest: digest)
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else { return nil }
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true,
              let size = values.fileSize, size > 0, size <= Self.maxAnalysisCacheBytes else {
            try? fm.removeItem(at: url)
            return nil
        }
        let data = try Data(contentsOf: url, options: [.mappedIfSafe])
        guard data.count <= Self.maxAnalysisCacheBytes,
              let rawEnvelope = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let schema = rawEnvelope["schema"] as? Int,
              let rawReport = rawEnvelope["report"] as? [String: Any],
              Set(rawReport.keys) == AnalysisReport.jsonFields else {
            return nil
        }

        let report: AnalysisReport
        let evidence: WorkerEvidenceBundle?
        let legacy: Bool
        switch schema {
        case 1:
            guard Set(rawEnvelope.keys) == Set(["schema", "productVersion", "sourceSHA256", "report"]),
                  let envelope = try? JSONDecoder().decode(LegacyAnalysisCacheEnvelope.self, from: data),
                  compatibleLegacyProductVersions.contains(envelope.productVersion),
                  envelope.sourceSHA256 == digest else { return nil }
            report = envelope.report
            evidence = nil
            legacy = true
        case 2:
            guard Set(rawEnvelope.keys) == Set(["schema", "engineVersion", "producerVersion", "sourceSHA256", "report"]),
                  let envelope = try? JSONDecoder().decode(AnalysisCacheEnvelopeV2.self, from: data),
                  envelope.engineVersion == engineVersion, envelope.engineVersion.utf8.count <= 64,
                  envelope.producerVersion.utf8.count <= 64, envelope.sourceSHA256 == digest else { return nil }
            report = envelope.report
            evidence = nil
            legacy = false
        case Self.analysisCacheSchema:
            let baseKeys = Set(["schema", "engineVersion", "producerVersion", "sourceSHA256", "report"])
            let envelopeKeys = Set(rawEnvelope.keys)
            guard (envelopeKeys == baseKeys || envelopeKeys == baseKeys.union(["evidence"])),
                  let envelope = try? JSONDecoder().decode(AnalysisCacheEnvelope.self, from: data),
                  envelope.engineVersion == engineVersion, envelope.engineVersion.utf8.count <= 64,
                  envelope.producerVersion.utf8.count <= 64, envelope.sourceSHA256 == digest else { return nil }
            report = envelope.report
            evidence = envelope.evidence?.validated()
            legacy = false
        default:
            return nil
        }

        guard report.matches(id: report.request_id, digest: digest),
              let rebound = report.forRequest(requestID),
              rebound.matches(id: requestID, digest: digest) else { return nil }
        return (rebound, evidence, legacy)
    }

    func storeAnalysisCache(_ report: AnalysisReport, evidence: WorkerEvidenceBundle? = nil, digest: String, engineVersion: String, producerVersion: String) throws {
        guard engineVersion.utf8.count <= 64, producerVersion.utf8.count <= 64,
              report.matches(id: report.request_id, digest: digest) else {
            throw WorkbenchError("Analysis result is not eligible for caching.")
        }
        let url = try analysisCacheURL(digest: digest)
        let envelope = AnalysisCacheEnvelope(
            schema: Self.analysisCacheSchema,
            engineVersion: engineVersion,
            producerVersion: producerVersion,
            sourceSHA256: digest,
            report: report,
            evidence: evidence?.validated()
        )
        let data = try JSONEncoder().encode(envelope)
        guard data.count > 0, data.count <= Self.maxAnalysisCacheBytes else {
            throw WorkbenchError("Analysis result exceeds the project cache limit.")
        }
        let fm = FileManager.default
        if fm.fileExists(atPath: url.path) || (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true {
            let existing = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard existing.isRegularFile == true, existing.isSymbolicLink != true else {
                throw WorkbenchError("Analysis cache destination must be a regular file, not a symbolic link or special file.")
            }
        }
        try data.write(to: url, options: [.atomic])
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true,
              values.fileSize == data.count else {
            try? FileManager.default.removeItem(at: url)
            throw WorkbenchError("Analysis cache publication could not be verified.")
        }
    }

    /// Retain the complete public SDK lookup, including missing and unsupported
    /// declarations, under the project cache with content-addressed identity.
    func storeSDKDeclarationCatalog(_ data: Data, sourceSHA256: String) throws -> (fileName: String, catalogSHA256: String) {
        guard data.count <= 4 * 1024 * 1024,
              sourceSHA256.utf8.count == 64,
              sourceSHA256.allSatisfy({ "0123456789abcdef".contains($0) }),
              let catalog = try? JSONDecoder().decode(DarwinSDKDeclarationCatalog.self, from: data),
              catalog.schema == 1, catalog.sourceSHA256 == sourceSHA256,
              (try? catalog.encoded()) == data else {
            throw WorkbenchError("SDK declaration catalog failed source, schema or canonical encoding checks.")
        }
        let catalogSHA256 = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        let fileName = "sdk-declarations-\(sourceSHA256)-\(catalogSHA256).json"
        let directory = try objectDirectory("analysis-cache")
        let url = directory.appendingPathComponent(fileName)
        let fm = FileManager.default
        if fm.fileExists(atPath: url.path) || (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true {
            let existing = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
            guard existing.isRegularFile == true, existing.isSymbolicLink != true,
                  existing.fileSize == data.count else {
                throw WorkbenchError("SDK declaration catalog destination is unsafe or has an unexpected size.")
            }
            let retained = try Data(contentsOf: url)
            guard retained == data,
                  SHA256.hash(data: retained).map({ String(format: "%02x", $0) }).joined() == catalogSHA256 else {
                throw WorkbenchError("Content-addressed SDK catalog path contains changed evidence; the existing file was preserved.")
            }
            return (fileName, catalogSHA256)
        }
        try data.write(to: url, options: [.atomic])
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true, values.fileSize == data.count,
              let stored = try? Data(contentsOf: url), SHA256.hash(data: stored).map({ String(format: "%02x", $0) }).joined() == catalogSHA256 else {
            try? fm.removeItem(at: url)
            throw WorkbenchError("SDK declaration catalog publication could not be verified.")
        }
        return (fileName, catalogSHA256)
    }

    func clearAnalysisCache() throws -> Int {
        let cacheDirectory = URL(fileURLWithPath: path).appendingPathComponent("analysis-cache", isDirectory: true)
        let fm = FileManager.default
        guard fm.fileExists(atPath: cacheDirectory.path) else { return 0 }
        _ = try validatedDirectory(cacheDirectory, createIfMissing: false, label: "Project analysis cache")
        let entries = try fm.contentsOfDirectory(at: cacheDirectory,
                                                  includingPropertiesForKeys: [.isRegularFileKey, .isSymbolicLinkKey],
                                                  options: [.skipsHiddenFiles])
        guard entries.count <= 4096 else { throw WorkbenchError("Project analysis cache contains too many entries to clear safely.") }
        var removed = 0
        for url in entries where url.pathExtension == "json" {
            let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true else {
                throw WorkbenchError("Project analysis cache contains an unsafe entry.")
            }
            try fm.removeItem(at: url)
            removed += 1
        }
        return removed
    }

    func prepareCapture() throws -> URL {
        try objectDirectory("staging").appendingPathComponent(UUID().uuidString + ".partial")
    }
    func addCaptured(path source: String, result: HeaderResult, staging: URL) throws -> ProjectSnapshot {
        guard result.captured, let digest = result.source_sha256, digest.count == 64,
              digest.allSatisfy({ "0123456789abcdef".contains($0) }) else {
            throw WorkbenchError("Invalid source identity.")
        }
        let stagingValues = try staging.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard stagingValues.isRegularFile == true, stagingValues.isSymbolicLink != true,
              let stagedSize = stagingValues.fileSize, UInt64(stagedSize) == UInt64(result.size) else {
            throw WorkbenchError("Staged source does not match worker response or is not a regular non-symlink file.")
        }
        let directory = try objectDirectory("objects")
        let name = digest + "-" + UUID().uuidString + ".bin"
        let destination = directory.appendingPathComponent(name)
        try FileManager.default.moveItem(at: staging, to: destination)
        let fd = Darwin.open(directory.path, O_RDONLY | O_NOFOLLOW)
        guard fd >= 0 else {
            try? FileManager.default.removeItem(at: destination)
            throw WorkbenchError("Cannot open object directory for synchronization.")
        }
        defer { Darwin.close(fd) }
        guard fsync(fd) == 0 else {
            try? FileManager.default.removeItem(at: destination)
            throw WorkbenchError("Cannot synchronize object directory.")
        }
        var saved = result
        saved.object_name = "objects/" + name
        do {
            return try add(path: source, result: saved)
        } catch {
            // Keep capture publication transactional from the user's point of view: if the
            // SQLite record cannot commit, remove the newly published object and sync the
            // directory again so a failed inspection does not accumulate orphan binaries.
            try? FileManager.default.removeItem(at: destination)
            _ = fsync(fd)
            throw error
        }
    }
    func add(path: String, result: HeaderResult) throws -> ProjectSnapshot {
        return try transaction {
            guard try scalar("SELECT count(*) FROM records") < 100 else {
                throw WorkbenchError("Foundation limit: 100 inspection records per project.")
            }
            let statement = try prepare("INSERT INTO records(id,source_path,name,header_json) VALUES(?,?,?,?)")
            defer { sqlite3_finalize(statement) }
            let json = String(decoding: try JSONEncoder().encode(result), as: UTF8.self)
            try bind(UUID().uuidString, 1, statement)
            try bind(path, 2, statement)
            try bind(URL(fileURLWithPath: path).lastPathComponent, 3, statement)
            try bind(json, 4, statement)
            guard sqlite3_step(statement) == SQLITE_DONE else { throw WorkbenchError("Could not commit inspection record.") }
            return try snapshot()
        }
    }
    func save(id: String, expectedRevision: Int, note: String) throws -> ProjectSnapshot {
        return try transaction {
            guard note.utf8.count <= 4096, expectedRevision >= 0, expectedRevision < Int.max else {
                throw WorkbenchError("Note limit is 4096 UTF-8 bytes; revision must be valid.")
            }
            let statement = try prepare("UPDATE records SET note=?, revision=revision+1 WHERE id=? AND revision=?")
            defer { sqlite3_finalize(statement) }
            try bind(note, 1, statement)
            try bind(id, 2, statement)
            guard sqlite3_bind_int64(statement, 3, Int64(expectedRevision)) == SQLITE_OK else { throw WorkbenchError("Invalid revision.") }
            guard sqlite3_step(statement) == SQLITE_DONE else { throw WorkbenchError("Note update failed.") }
            guard sqlite3_changes(db) == 1 else { throw WorkbenchError("Note conflict or missing record. Refresh the project.") }
            return try snapshot()
        }
    }
}
