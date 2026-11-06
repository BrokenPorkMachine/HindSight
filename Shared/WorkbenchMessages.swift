import Foundation
import CryptoKit
/// Platform-specific indicators for workspaces.
enum VGWorkspacePlatform: String, CaseIterable {
    case apple = "Machine Lab"
    case ecu = "Ecu Platform"
}

enum VGArtifactProfile: String, Codable, CaseIterable {
    case apple
    case ecu
    case generic

    static func derive(from record: ProjectRecord?) -> VGArtifactProfile {
        guard let record else { return .generic }
        let options = record.result.import_options
        if options?.format == "ecu" || options?.architecture == "c167"
            || record.result.architecture.localizedCaseInsensitiveContains("C167")
            || record.result.file_kind.localizedCaseInsensitiveContains("ecu") { return .ecu }
        if record.result.file_kind.localizedCaseInsensitiveContains("mach-o") { return .apple }
        return .generic
    }
}

/// Persisted import interpretation, included in page and analysis cache identity.
struct BinaryImportOptions: Codable, Equatable {
    var format: String = "auto"
    var architecture: String = "auto"
    var load_address: String = "0"
    var file_offset: String = "0"
    static let automatic = BinaryImportOptions()
    static let analysisRevision = "prototype-abi-v38"
    private static func number(_ text: String) -> UInt64? {
        if text.hasPrefix("0x") { return UInt64(text.dropFirst(2), radix: 16) }
        return UInt64(text)
    }
    var validated: BinaryImportOptions? {
        guard ["auto", "macho", "elf", "pe", "sepos", "securerom", "iboot", "raw", "ecu"].contains(format),
              ["auto", "aarch64", "i386", "x86_64", "arm", "thumb", "c167"].contains(architecture),
              let address = Self.number(load_address), let offset = Self.number(file_offset),
              offset < 8_589_934_592 else { return nil }
        return BinaryImportOptions(format: format, architecture: architecture,
                                   load_address: String(address), file_offset: String(offset))
    }
    var arguments: [String] { [format, architecture, load_address, file_offset] }
    var cacheIdentity: String { "multiarch-v2:" + Self.analysisRevision + ":" + arguments.joined(separator: ":") }
}

struct HeaderResult: Codable {
    let `protocol`: Int
    let request_id: String
    let kind: String
    let file_kind: String
    let architecture: String
    let size: String
    let header_hex: String
    let captured: Bool
    let source_sha256: String?
    var object_name: String?
    let structure_status: String?
    let structure: String?
    let disassembly_status: String?
    let disassembly: String?
    var import_options: BinaryImportOptions? = nil
}
struct ProjectRecord: Codable, Identifiable {
    let id: String
    let path: String
    let name: String
    let result: HeaderResult
    var note: String
    var revision: Int
}

struct ProjectTypeLayoutField: Codable, Equatable {
    let name: String
    let declaration: String
    let offsetBits: Int
    let bitWidth: Int?

    private enum CodingKeys: String, CodingKey { case name, declaration, offsetBits, bitWidth }
}

struct ProjectTypeLayoutRecord: Codable, Equatable {
    let name: String
    let kind: String
    let size: Int
    let alignment: Int
    let fields: [ProjectTypeLayoutField]
    let fieldOffsetsComplete: Bool
}

struct ProjectTypeLayoutEvidence: Codable, Equatable {
    let schema: Int
    let producer: String
    let sdk: String
    let sdkPath: String
    let sdkBuild: String
    let target: String
    let compiler: String
    let declarationsSHA256: String
    let record: ProjectTypeLayoutRecord

    private enum CodingKeys: String, CodingKey {
        case schema, producer, sdk, sdkPath, sdkBuild, target, compiler, declarationsSHA256, record
    }

    func validated(for candidate: ProjectTypeCandidate) -> Bool {
        let digest = SHA256.hash(data: Data(candidate.declaration.utf8)).map { String(format: "%02x", $0) }.joined()
        let simulator = target.hasSuffix("-simulator")
        let ios = target.contains("-apple-ios")
        let sdkMatches = ios ? (simulator ? sdk == "iphonesimulator" : sdk == "iphoneos") : sdk == "macosx"
        let sdkLeaf = URL(fileURLWithPath: sdkPath).lastPathComponent
        let sdkDirectoryMatches: Bool
        switch sdk {
        case "macosx": sdkDirectoryMatches = sdkLeaf.hasPrefix("MacOSX") && sdkLeaf.hasSuffix(".sdk")
        case "iphoneos": sdkDirectoryMatches = sdkLeaf.hasPrefix("iPhoneOS") && sdkLeaf.hasSuffix(".sdk")
        default: sdkDirectoryMatches = sdkLeaf.hasPrefix("iPhoneSimulator") && sdkLeaf.hasSuffix(".sdk")
        }
        let tripleValid = target.range(of: #"^(arm64|arm64e|x86_64)-apple-(macos|ios)[0-9]+\.[0-9]+(?:\.[0-9]+)?(?:-simulator)?$"#,
                                       options: .regularExpression) != nil
        let safeMetadata: (String, Int) -> Bool = { value, limit in
            !value.isEmpty && value.utf8.count <= limit
                && !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        }
        let powerOfTwo = record.alignment > 0 && (record.alignment & (record.alignment - 1)) == 0
        let (recordBits, overflow) = record.size.multipliedReportingOverflow(by: 8)
        guard schema == 1, producer == "hindsight-apple-clang-record-layout-v1",
              ["macosx", "iphoneos", "iphonesimulator"].contains(sdk),
              sdkPath.hasPrefix("/"), safeMetadata(sdkPath, 4096), sdkDirectoryMatches,
              sdkBuild.range(of: #"^[A-Za-z0-9_.-]{1,128}$"#, options: .regularExpression) != nil,
              target == candidate.target, tripleValid, sdkMatches,
              compiler.hasPrefix("Apple clang version"), safeMetadata(compiler, 512),
              declarationsSHA256 == digest, record.name == candidate.name,
              ["struct", "union"].contains(record.kind), record.size > 0, record.size <= 1_048_576,
              powerOfTwo, !overflow, record.fieldOffsetsComplete, record.fields.count <= 1024,
              Set(record.fields.map(\.name)).count == record.fields.count else { return false }
        for field in record.fields {
            guard field.name.range(of: #"^[A-Za-z_][A-Za-z0-9_]{0,127}$"#, options: .regularExpression) != nil,
                  safeMetadata(field.declaration, 512), field.offsetBits >= 0, field.offsetBits < recordBits else { return false }
            if let width = field.bitWidth, width <= 0 || width > recordBits - field.offsetBits { return false }
        }
        return true
    }
}

struct ProjectTypeCandidate: Codable, Equatable, Identifiable {
    let name: String
    let declaration: String
    let target: String
    let source: String
    let sourceSHA256: String
    var layoutEvidence: ProjectTypeLayoutEvidence? = nil
    var id: String {
        let base = [name, declaration, target, source, sourceSHA256].map { "\($0.utf8.count):\($0)" }.joined(separator: "|")
        guard let layoutEvidence else { return base }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(layoutEvidence) else { return base }
        let digest = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
        return base + "|layout:\(digest)"
    }

    var validated: ProjectTypeCandidate? {
        let identifier = name.range(of: #"^[A-Za-z_][A-Za-z0-9_]{0,255}$"#, options: .regularExpression) != nil
        let targetArchitecture = target.split(separator: "-", maxSplits: 1).first.map(String.init) ?? ""
        let targetValid = target.range(of: #"^[A-Za-z0-9_.-]{1,128}$"#, options: .regularExpression) != nil
            && ["arm64", "arm64e", "aarch64", "x86_64", "i386", "arm", "thumb"].contains(targetArchitecture)
        let sourceValid = source == "analyst"
        let digestValid = sourceSHA256.range(of: #"^[0-9a-f]{64}$"#, options: .regularExpression) != nil
        let declarationValid = !declaration.isEmpty && declaration.utf8.count <= 16 * 1024
            && declaration.unicodeScalars.allSatisfy { $0.value < 128 }
            && !declaration.unicodeScalars.contains(where: {
                CharacterSet.controlCharacters.contains($0) && $0.value != 0x0a && $0.value != 0x09
            }) && Self.isTypeDeclaration(declaration)
        let layoutValid = layoutEvidence.map { evidence in
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys]
            return evidence.validated(for: self) && ((try? encoder.encode(evidence).count).map { $0 <= 256 * 1024 } ?? false)
        } ?? true
        return identifier && targetValid && sourceValid && digestValid && declarationValid && layoutValid ? self : nil
    }

    private static func isTypeDeclaration(_ declaration: String) -> Bool {
        let value = declaration.trimmingCharacters(in: .whitespacesAndNewlines)
        guard ["typedef ", "struct ", "union ", "enum "].contains(where: value.hasPrefix),
              value.hasSuffix(";"), !value.contains("//"), !value.contains("/*"), !value.contains("*/"),
              !value.contains(where: { ["#", "\\", "\"", "'"].contains(String($0)) }) else { return false }
        var delimiters: [Character] = []
        for (index, character) in value.enumerated() {
            switch character {
            case "{", "(", "[": delimiters.append(character)
            case "}", ")", "]":
                let expected: Character = character == "}" ? "{" : character == ")" ? "(" : "["
                guard delimiters.popLast() == expected else { return false }
            case ";" where delimiters.isEmpty:
                if index != value.count - 1 { return false }
            default: break
            }
        }
        return delimiters.isEmpty
    }
}

struct ProjectTypeCatalog: Codable {
    static let currentSchema = 1
    let schema: Int
    let candidates: [ProjectTypeCandidate]

    init(candidates: [ProjectTypeCandidate], schema: Int = Self.currentSchema) {
        self.schema = schema
        self.candidates = candidates
    }

    var conflicts: Set<String> {
        conflicts(for: nil)
    }

    func conflicts(for target: String?) -> Set<String> {
        let matching = target.map { value in candidates.filter { $0.target == value } } ?? candidates
        let declarations = Dictionary(grouping: matching, by: { "\($0.name)|\($0.target)" })
        return Set(declarations.values.compactMap { rows -> String? in
            let distinctLayouts = Set(rows.compactMap { Self.layoutFactIdentity($0.layoutEvidence) })
            guard Set(rows.map { $0.declaration }).count > 1 || distinctLayouts.count > 1 else { return nil }
            return rows.first?.name
        })
    }

    func declaration(for name: String, target: String) -> String? {
        let matching = candidates.filter { $0.name == name && $0.target == target }
        let declarations = Set(matching.map { $0.declaration })
        guard declarations.count == 1 else { return nil }
        return declarations.first
    }

    /// Type declarations eligible to influence one reconstruction request.
    /// Exact capture and target matching prevents stale or cross-target analyst
    /// assertions from becoming source dependencies. Conflicting names are
    /// withheld as a group, even if one candidate happens to be encountered
    /// first in storage order.
    func resolvedDeclarations(sourceSHA256: String, target: String) -> [ProjectTypeCandidate] {
        guard validated() != nil,
              sourceSHA256.range(of: #"^[0-9a-f]{64}$"#, options: .regularExpression) != nil,
              target.range(of: #"^[A-Za-z0-9_.-]{1,128}$"#, options: .regularExpression) != nil else { return [] }
        let eligible = candidates.filter {
            $0.sourceSHA256 == sourceSHA256 && $0.target == target && $0.validated != nil
        }
        let grouped = Dictionary(grouping: eligible, by: \.name)
        return grouped.keys.sorted().compactMap { name in
            guard let rows = grouped[name], Set(rows.map(\.declaration)).count == 1,
                  Set(rows.compactMap({ Self.layoutFactIdentity($0.layoutEvidence) })).count <= 1 else { return nil }
            return rows.sorted { left, right in
                if (left.layoutEvidence != nil) != (right.layoutEvidence != nil) { return left.layoutEvidence != nil }
                return left.id < right.id
            }.first
        }
    }

    private static func layoutFactIdentity(_ evidence: ProjectTypeLayoutEvidence?) -> String? {
        guard let evidence else { return nil }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(evidence.record) else { return "invalid-layout" }
        return String(decoding: data, as: UTF8.self)
    }

    /// Candidates portable to a selected captured image. The source digest is
    /// exact; architecture aliases are normalized while the candidate's full
    /// target triple remains intact for later ABI matching.
    func candidates(sourceSHA256: String, architecture: String) -> [ProjectTypeCandidate] {
        guard validated() != nil else { return [] }
        func normalizedArchitecture(_ value: String) -> String {
            let token = value.split(separator: "-", maxSplits: 1).first.map(String.init)?.lowercased() ?? ""
            switch token {
            case "aarch64", "arm64e": return "arm64"
            case "i686": return "i386"
            default: return token
            }
        }
        let wantedArchitecture = normalizedArchitecture(architecture)
        guard !wantedArchitecture.isEmpty else { return [] }
        return candidates.filter {
            $0.sourceSHA256 == sourceSHA256 && normalizedArchitecture($0.target) == wantedArchitecture
        }
    }

    func validated() -> ProjectTypeCatalog? {
        guard schema == Self.currentSchema, candidates.count <= 10_000,
              candidates.allSatisfy({ $0.validated != nil }),
              Set(candidates.map { $0.id }).count == candidates.count else { return nil }
        let estimatedBytes = candidates.reduce(0) {
            $0 + $1.name.utf8.count + $1.declaration.utf8.count + $1.target.utf8.count
                + $1.source.utf8.count + $1.sourceSHA256.utf8.count
                + ((try? JSONEncoder().encode($1.layoutEvidence).count) ?? 0) + 256
        }
        guard estimatedBytes <= 4 * 1024 * 1024 else { return nil }
        return self
    }
}

struct ProjectTypeCatalogReply: Codable {
    let ok: Bool
    let message: String
    let catalog: ProjectTypeCatalog?
    static func success(_ catalog: ProjectTypeCatalog) -> Self { Self(ok: true, message: "", catalog: catalog) }
    static func failure(_ message: String) -> Self { Self(ok: false, message: message, catalog: nil) }
    var encoded: String {
        guard let data = try? JSONEncoder().encode(self), let text = String(data: data, encoding: .utf8) else {
            return #"{"ok":false,"message":"Type catalog reply encoding failed","catalog":null}"#
        }
        return text
    }
}

struct FunctionNameExchange: Decodable {
    struct Function: Decodable {
        let offset: String
        let name: String
        let provenance: String
    }
    let schema: String
    let address_space: String
    let source_sha256: String
    let source_image_base: String
    let producer: String
    let functions: [Function]

    func resolved(sourceSHA256: String, knownStarts: Set<UInt64>) -> (matches: [(UInt64, String, String)], unmatched: Int)? {
        guard schema == "hindsight.functions.v1", address_space == "image-relative",
              source_sha256.lowercased() == sourceSHA256.lowercased(),
              source_sha256.count == 64, source_sha256.allSatisfy({ $0.isHexDigit }),
              let base = Self.address(source_image_base),
              !producer.isEmpty, producer.utf8.count <= 256,
              !producer.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
              functions.count <= 20_000 else { return nil }
        var seen = Set<UInt64>()
        var matches: [(UInt64, String, String)] = []
        var unmatched = 0
        for function in functions {
            guard let offset = Self.address(function.offset), offset <= UInt64.max - base,
                  function.name.utf8.count <= 128, !function.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  function.provenance.utf8.count <= 512,
                  !function.name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
                  !function.provenance.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return nil }
            let address = base + offset
            guard seen.insert(address).inserted else { return nil }
            if knownStarts.contains(address) { matches.append((address, function.name, function.provenance)) }
            else { unmatched += 1 }
        }
        return (matches, unmatched)
    }

    private static func address(_ text: String) -> UInt64? {
        guard text.hasPrefix("0x"), (3...18).contains(text.count),
              text.dropFirst(2).allSatisfy({ $0.isHexDigit }) else { return nil }
        return UInt64(text.dropFirst(2), radix: 16)
    }
}

/// Versioned project boundary shared by the app, Coordinator and remote API.
///
/// Older coordinators emitted only `path` and `records`; decoding those
/// payloads as schema 1 keeps the wire boundary backward compatible while all
/// newly emitted snapshots identify the v2 contract explicitly.
struct ProjectSnapshot: Codable {
    static let currentSchema = 2
    let schema: Int
    let path: String
    let records: [ProjectRecord]

    init(path: String, records: [ProjectRecord], schema: Int = ProjectSnapshot.currentSchema) {
        self.schema = schema
        self.path = path
        self.records = records
    }

    private enum CodingKeys: String, CodingKey { case schema, path, records }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        schema = try container.decodeIfPresent(Int.self, forKey: .schema) ?? 1
        path = try container.decode(String.self, forKey: .path)
        records = try container.decode([ProjectRecord].self, forKey: .records)
        guard schema == 1 || schema == Self.currentSchema else {
            throw DecodingError.dataCorruptedError(forKey: .schema, in: container,
                                                   debugDescription: "Unsupported project snapshot schema \(schema)")
        }
    }
}
struct ServiceReply: Codable {
    let ok: Bool
    let message: String
    let project: ProjectSnapshot?
    static func success(_ message: String) -> String {
        encode(Self(ok: true, message: message, project: nil))
    }
    static func success(_ project: ProjectSnapshot, _ message: String = "") -> String {
        encode(Self(ok: true, message: message, project: project))
    }
    static func failure(_ message: String) -> String { encode(Self(ok: false, message: message, project: nil)) }
    private static func encode(_ value: Self) -> String {
        guard let data = try? JSONEncoder().encode(value), let text = String(data: data, encoding: .utf8) else {
            return "{\"ok\":false,\"message\":\"Reply encoding failed\"}"
        }
        return text
    }
}

struct InstructionPage: Codable {
    let `protocol`: Int
    let kind: String
    let request_id: String
    let source_sha256: String
    let start_word: String
    let total_words: String
    let section_address: String
    let section_offset: String
    let word_count: Int
    let text: String
    var unit_bytes: Int? = nil
    var unitBytes: UInt64 { UInt64(exactly: unit_bytes ?? 4) ?? 0 }
    func forRequest(_ id: String) -> InstructionPage {
        InstructionPage(protocol: `protocol`, kind: kind, request_id: id, source_sha256: source_sha256,
                        start_word: start_word, total_words: total_words, section_address: section_address,
                        section_offset: section_offset, word_count: word_count, text: text, unit_bytes: unit_bytes)
    }
    func matches(id: String, digest: String, start: UInt64) -> Bool {
        let unit = unitBytes
        guard [1, 2, 4].contains(unit), `protocol` == 1, kind == "page_result", request_id == id,
              source_sha256 == digest, UInt64(start_word) == start,
              let total = UInt64(total_words), total <= 8_589_934_592, start < total,
              let address = UInt64(section_address), address % unit == 0,
              let offset = UInt64(section_offset), offset % unit == 0,
              total <= (UInt64.max - address) / unit, total <= (UInt64.max - offset) / unit,
              word_count > 0, word_count <= Int(min(256, total - start)),
              (unit_bytes != nil || word_count == Int(min(256, total - start))), text.utf8.count < 65536 else { return false }
        return true
    }
}
struct PageReply: Codable {
    let ok: Bool
    let message: String
    let page: InstructionPage?
    static func encode(page: InstructionPage? = nil, error: String = "", message: String = "") -> String {
        let value = PageReply(ok: page != nil, message: page == nil ? error : message, page: page)
        guard let data = try? JSONEncoder().encode(value) else { return "{\"ok\":false,\"message\":\"Page encoding failed\"}" }
        return String(decoding: data, as: UTF8.self)
    }
}


enum WorkbenchProtocolLimits {
    static let controlWorkerFrameBytes = 256 * 1024
    static let analysisWorkerFrameBytes = 8 * 1024 * 1024
    static let defaultXPCReplyBytes = 2 * 1024 * 1024
    static let analysisXPCReplyBytes = 12 * 1024 * 1024
}

enum InspectionPhase: String, Codable, CaseIterable {
    case queued
    case admitted
    case checkingCache
    case launching
    case running
    case receiving
    case validating
    case publishing
    case cancelling
    case completed
    case failed
    case cancelled

    var terminal: Bool {
        switch self {
        case .completed, .failed, .cancelled: return true
        default: return false
        }
    }

    var displayName: String {
        switch self {
        case .queued: return "Queued"
        case .admitted: return "Admitted"
        case .checkingCache: return "Checking cache"
        case .launching: return "Launching worker"
        case .running: return "Analyzing"
        case .receiving: return "Receiving result"
        case .validating: return "Validating result"
        case .publishing: return "Publishing result"
        case .cancelling: return "Cancelling"
        case .completed: return "Completed"
        case .failed: return "Failed"
        case .cancelled: return "Cancelled"
        }
    }
}

struct InspectionStatus: Codable, Equatable {
    let request_id: String
    let operation: String
    let phase: InspectionPhase
    let detail: String
    let started_at_ms: UInt64
    let updated_at_ms: UInt64
    let elapsed_ms: UInt64
    let progress_percent: Int
    let progress_stage: String
    let diagnostic: String?

    var terminal: Bool { phase.terminal }
    var elapsedSeconds: Double { Double(elapsed_ms) / 1000.0 }
}

struct InspectionStatusReply: Codable {
    let ok: Bool
    let message: String
    let status: InspectionStatus?

    static func encode(status: InspectionStatus? = nil, error: String = "") -> String {
        let value = InspectionStatusReply(ok: status != nil, message: status == nil ? error : "", status: status)
        guard let data = try? JSONEncoder().encode(value) else {
            return "{\"ok\":false,\"message\":\"Inspection status encoding failed\"}"
        }
        return String(decoding: data, as: UTF8.self)
    }
}

struct AnalysisReport: Codable, Sendable {
    static let jsonFields: Set<String> = [
        "protocol", "kind", "request_id", "source_sha256", "function_count", "symbol_count",
        "basic_block_count", "analyzed_instructions", "truncated", "functions", "cfg", "ir",
        "strings", "xrefs", "call_graph", "typed_call_graph", "call_effect_summaries",
        "prototype_conflicts", "dataflow", "ssa", "dominators", "phi_nodes",
        "dominance_frontiers", "relocations", "jump_tables", "metadata", "runtime_records",
        "annotated_disassembly", "ssa_renamed", "runtime_graph", "swift_types", "selector_xrefs",
        "phi_operands", "objc_classes", "swift_contexts", "call_values", "chained_fixups",
        "chained_starts", "objc_protocols", "swift_fields", "swift_field_records",
        "interblock_values", "predecessor_values", "chained_pointers", "chained_multistarts",
        "arm64e_pointers", "swift_field_ownership", "propagated_values", "indirect_calls",
        "dyld_imports", "dyld_linking", "softlinking", "symbolication", "symbol_resolver", "apple_symbolication", "arm64e_bind_symbols", "dylib_ordinals", "resolved_imports",
        "authenticated_bind_targets", "indirect_symbols", "pointer_slots", "export_trie",
        "symbol_provenance", "linkage_records", "symbol_callers", "got_auth_slots",
        "import_export_collisions", "function_intelligence", "data_references", "tail_call_candidates", "function_neighborhoods", "analysis_quality", "analysis_insights", "semantic_preview", "summary"
    ]
    let `protocol`: Int
    let kind: String
    let request_id: String
    let source_sha256: String
    let function_count: Int
    let symbol_count: Int
    let basic_block_count: Int
    let analyzed_instructions: Int
    let truncated: Bool
    let functions: String
    let cfg: String
    let ir: String
    var semantic_preview: String? = nil
    let strings: String
    let xrefs: String
    let call_graph: String
    let typed_call_graph: String
    let call_effect_summaries: String
    let prototype_conflicts: String
    let dataflow: String
    let ssa: String
    let dominators: String
    let phi_nodes: String
    let dominance_frontiers: String
    let relocations: String
    let jump_tables: String
    let metadata: String
    let runtime_records: String
    let annotated_disassembly: String
    let ssa_renamed: String
    let runtime_graph: String
    let swift_types: String
    let selector_xrefs: String
    let phi_operands: String
    let objc_classes: String
    let swift_contexts: String
    let call_values: String
    let chained_fixups: String
    let chained_starts: String
    let objc_protocols: String
    let swift_fields: String
    let swift_field_records: String
    let interblock_values: String
    let predecessor_values: String
    let chained_pointers: String
    let chained_multistarts: String
    let arm64e_pointers: String
    let swift_field_ownership: String
    let propagated_values: String
    let indirect_calls: String
    let dyld_imports: String
    let dyld_linking: String
    let softlinking: String
    let symbolication: String
    let symbol_resolver: String
    let apple_symbolication: String
    let arm64e_bind_symbols: String
    let dylib_ordinals: String
    let resolved_imports: String
    let authenticated_bind_targets: String
    let indirect_symbols: String
    let pointer_slots: String
    let export_trie: String
    let symbol_provenance: String
    let linkage_records: String
    let symbol_callers: String
    let got_auth_slots: String
    let import_export_collisions: String
    let function_intelligence: String
    let data_references: String
    let tail_call_candidates: String
    let function_neighborhoods: String
    let analysis_quality: String
    let analysis_insights: String
    let summary: String
    func forRequest(_ id: String) -> AnalysisReport? {
        guard UUID(uuidString: id) != nil,
              let encoded = try? JSONEncoder().encode(self),
              var object = (try? JSONSerialization.jsonObject(with: encoded)) as? [String: Any] else {
            return nil
        }
        object["request_id"] = id
        guard let rebound = try? JSONSerialization.data(withJSONObject: object),
              rebound.count <= WorkbenchProtocolLimits.analysisWorkerFrameBytes else {
            return nil
        }
        return try? JSONDecoder().decode(AnalysisReport.self, from: rebound)
    }

    /// Explain the failed identity/budget class at the coordinator boundary;
    /// ordinary callers can keep using the Boolean `matches` predicate.
    func validationFailure(id: String, digest: String) -> String? {
        if `protocol` != 1 { return "unsupported protocol version" }
        if kind != "analysis_result" { return "unexpected result kind" }
        if request_id != id { return "request ID mismatch" }
        if source_sha256 != digest { return "captured-source digest mismatch" }
        if function_count < 0 || symbol_count < 0 || basic_block_count < 0 || analyzed_instructions < 0 {
            return "negative analysis count"
        }
        guard !matches(id: id, digest: digest) else { return nil }
        if let encoded = try? JSONEncoder().encode(self),
           let object = (try? JSONSerialization.jsonObject(with: encoded)) as? [String: Any] {
            for name in Self.jsonFields.sorted() {
                guard let section = object[name] as? String else { continue }
                let limit = name == "summary" ? 32_768 : 65_535
                let size = section.utf8.count
                if size > limit {
                    return "analysis section '\(name)' exceeds its \(limit)-byte limit (\(size) bytes)"
                }
            }
        }
        return "analysis report exceeds a section size bound"
    }

    func matches(id: String, digest: String) -> Bool {
        `protocol` == 1 && kind == "analysis_result" && request_id == id && source_sha256 == digest &&
        function_count >= 0 && symbol_count >= 0 && basic_block_count >= 0 && analyzed_instructions >= 0 &&
        functions.utf8.count < 65536 && cfg.utf8.count < 65536 && ir.utf8.count < 65536 && (semantic_preview?.utf8.count ?? 0) < 65536 &&
        strings.utf8.count < 65536 && xrefs.utf8.count < 65536 && call_graph.utf8.count < 65536 && typed_call_graph.utf8.count < 65536 && call_effect_summaries.utf8.count < 65536 && prototype_conflicts.utf8.count < 65536 &&
        dataflow.utf8.count < 65536 && ssa.utf8.count < 65536 && dominators.utf8.count < 65536 &&
        phi_nodes.utf8.count < 65536 && dominance_frontiers.utf8.count < 65536 && relocations.utf8.count < 65536 && jump_tables.utf8.count < 65536 &&
        metadata.utf8.count < 65536 && runtime_records.utf8.count < 65536 && annotated_disassembly.utf8.count < 65536 && ssa_renamed.utf8.count < 65536 && runtime_graph.utf8.count < 65536 && swift_types.utf8.count < 65536 && selector_xrefs.utf8.count < 65536 && phi_operands.utf8.count < 65536 && objc_classes.utf8.count < 65536 && swift_contexts.utf8.count < 65536 && call_values.utf8.count < 65536 && chained_fixups.utf8.count < 65536 && chained_starts.utf8.count < 65536 && objc_protocols.utf8.count < 65536 && swift_fields.utf8.count < 65536 && swift_field_records.utf8.count < 65536 && interblock_values.utf8.count < 65536 && predecessor_values.utf8.count < 65536 && chained_pointers.utf8.count < 65536 && chained_multistarts.utf8.count < 65536 && arm64e_pointers.utf8.count < 65536 && swift_field_ownership.utf8.count < 65536 && propagated_values.utf8.count < 65536 && indirect_calls.utf8.count < 65536 && dyld_imports.utf8.count < 65536 && dyld_linking.utf8.count < 65536 && softlinking.utf8.count < 65536 && symbolication.utf8.count < 65536 && symbol_resolver.utf8.count < 65536 && apple_symbolication.utf8.count < 65536 && arm64e_bind_symbols.utf8.count < 65536 && dylib_ordinals.utf8.count < 65536 && resolved_imports.utf8.count < 65536 && authenticated_bind_targets.utf8.count < 65536 && indirect_symbols.utf8.count < 65536 && pointer_slots.utf8.count < 65536 && export_trie.utf8.count < 65536 && symbol_provenance.utf8.count < 65536 && linkage_records.utf8.count < 65536 && symbol_callers.utf8.count < 65536 && got_auth_slots.utf8.count < 65536 && import_export_collisions.utf8.count < 65536 && function_intelligence.utf8.count < 65536 && data_references.utf8.count < 65536 && tail_call_candidates.utf8.count < 65536 && function_neighborhoods.utf8.count < 65536 && analysis_quality.utf8.count < 65536 && analysis_insights.utf8.count < 65536 && summary.utf8.count < 32768
    }
}

struct WorkerEvidenceRecord: Codable, Hashable, Sendable {
    let source: String
    let workspace: String
    let kind: String
    let text: String
    let address: String?
    let symbol: String?
    let ordinal: Int
    let focus_priority: Int
}

struct WorkerEvidenceBundle: Codable, Sendable {
    let schema: Int
    let producer: String
    let records: [WorkerEvidenceRecord]
    let truncated: Bool
    let stage_fingerprints: [String: String]

    func validated() -> WorkerEvidenceBundle? {
        guard schema == 1, producer == "worker-native-evidence-v1",
              records.count <= 50_000, stage_fingerprints.count <= 64 else { return nil }
        for record in records {
            guard record.source.utf8.count <= 128, record.workspace.utf8.count <= 128,
                  record.kind.utf8.count <= 64, record.text.utf8.count <= 16 * 1024,
                  record.ordinal >= 0, record.focus_priority >= 0 && record.focus_priority <= 200,
                  (record.address?.utf8.count ?? 0) <= 18, (record.symbol?.utf8.count ?? 0) <= 1024 else { return nil }
        }
        guard stage_fingerprints.allSatisfy({ $0.key.utf8.count <= 64 && $0.value.utf8.count <= 64 }) else { return nil }
        return self
    }
}

struct AnalysisReply: Codable, Sendable {
    let ok: Bool
    let message: String
    let analysis: AnalysisReport?
    let evidence: WorkerEvidenceBundle?
    static func encode(analysis: AnalysisReport? = nil, evidence: WorkerEvidenceBundle? = nil, error: String = "", message: String = "") -> String {
        let safeEvidence = evidence?.validated()
        let value = AnalysisReply(ok: analysis != nil, message: analysis == nil ? error : message, analysis: analysis, evidence: safeEvidence)
        guard let data = try? JSONEncoder().encode(value) else { return "{\"ok\":false,\"message\":\"Analysis encoding failed\"}" }
        guard data.count <= WorkbenchProtocolLimits.analysisXPCReplyBytes else {
            // Typed evidence is an acceleration sideband. Preserve the validated compatibility report
            // if including the sideband would exceed the bounded XPC reply budget.
            let fallback = AnalysisReply(ok: analysis != nil, message: analysis == nil ? error : message + " Typed evidence omitted by XPC size budget.", analysis: analysis, evidence: nil)
            guard let fallbackData = try? JSONEncoder().encode(fallback), fallbackData.count <= WorkbenchProtocolLimits.analysisXPCReplyBytes else {
                return "{\"ok\":false,\"message\":\"Analysis reply exceeds the bounded XPC transport limit.\"}"
            }
            return String(decoding: fallbackData, as: UTF8.self)
        }
        return String(decoding: data, as: UTF8.self)
    }
}

struct C167EmulationFlags: Codable, Sendable, Hashable {
    let carry: Bool
    let zero: Bool
    let negative: Bool
    let overflow: Bool
}

struct C167EmulationTrace: Codable, Sendable, Hashable, Identifiable {
    let pc: UInt64
    let bytes: String
    let text: String
    var id: String { "\(pc):\(bytes)" }
}

struct C167EmulationResult: Codable, Sendable {
    let `protocol`: Int
    let kind: String
    let request_id: String
    let pc: UInt64
    let sp: UInt64
    let cp: UInt16
    let csp: UInt8
    let mdh: UInt16
    let mdl: UInt16
    let steps: UInt64
    let stop: String
    let flags: C167EmulationFlags
    let registers: [String: UInt16]
    let dpp: [String: UInt16]
    let ram_seed_bytes: Int
    let ram_write_bytes: Int
    let ram_truncated: Bool
    let ram: [String: UInt8]
    let trace: [C167EmulationTrace]

    func validated(requestID: String) -> C167EmulationResult? {
        guard `protocol` == 1, kind == "c167_emulation_result", request_id == requestID,
              pc <= 0x00ff_ffff, sp <= 0x1_0000, cp & 0x03ff == 0, steps <= 1_000_000,
              registers.count == 16, dpp.count == 4, ram_seed_bytes <= 4096,
              ram_write_bytes <= 2 * 1024 * 1024, ram.count <= 256, trace.count <= 4096,
              registers.keys.allSatisfy({ key in
                  key.first == "r" && Int(key.dropFirst()).map { (0..<16).contains($0) } == true
              }), dpp.allSatisfy({ key, value in
                  key.hasPrefix("dpp") && Int(key.dropFirst(3)).map { (0..<4).contains($0) } == true && value <= 0x03ff
              }), ram.keys.allSatisfy({ key in key.count == 6 && UInt32(key, radix: 16) != nil }),
              trace.allSatisfy({ $0.pc <= 0x00ff_ffff && $0.bytes.utf8.count <= 8 && $0.text.utf8.count <= 256 }) else { return nil }
        return self
    }
}

struct C167EmulationReply: Codable, Sendable {
    let ok: Bool
    let message: String
    let result: C167EmulationResult?
    static func encode(result: C167EmulationResult? = nil, error: String = "") -> String {
        let value = C167EmulationReply(ok: result != nil, message: result == nil ? error : "Bounded C167 execution completed.", result: result)
        guard let data = try? JSONEncoder().encode(value), data.count <= WorkbenchProtocolLimits.defaultXPCReplyBytes else {
            return "{\"ok\":false,\"message\":\"C167 emulator reply encoding failed.\",\"result\":null}"
        }
        return String(decoding: data, as: UTF8.self)
    }
}

struct VGOperationError: Codable, Hashable, Identifiable {
    let id: UUID
    let timestamp: Date
    let operation: String
    let code: String
    let message: String
    let recovery: String
    let retryable: Bool
}
