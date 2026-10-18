import Foundation
/// Persisted import interpretation, included in page and analysis cache identity.
struct BinaryImportOptions: Codable, Equatable {
    var format: String = "auto"
    var architecture: String = "auto"
    var load_address: String = "0"
    var file_offset: String = "0"
    static let automatic = BinaryImportOptions()
    static let analysisRevision = "prototype-abi-v2"
    private static func number(_ text: String) -> UInt64? {
        if text.hasPrefix("0x") { return UInt64(text.dropFirst(2), radix: 16) }
        return UInt64(text)
    }
    var validated: BinaryImportOptions? {
        guard ["auto", "macho", "elf", "pe", "sepos", "securerom", "iboot", "raw"].contains(format),
              ["auto", "aarch64", "i386", "x86_64", "arm", "thumb"].contains(architecture),
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
struct ProjectSnapshot: Codable { let path: String; let records: [ProjectRecord] }
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
        "strings", "xrefs", "call_graph", "dataflow", "ssa", "dominators", "phi_nodes",
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

    func matches(id: String, digest: String) -> Bool {
        `protocol` == 1 && kind == "analysis_result" && request_id == id && source_sha256 == digest &&
        function_count >= 0 && symbol_count >= 0 && basic_block_count >= 0 && analyzed_instructions >= 0 &&
        functions.utf8.count < 65536 && cfg.utf8.count < 65536 && ir.utf8.count < 65536 && (semantic_preview?.utf8.count ?? 0) < 65536 &&
        strings.utf8.count < 65536 && xrefs.utf8.count < 65536 && call_graph.utf8.count < 65536 &&
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

struct VGOperationError: Codable, Hashable, Identifiable {
    let id: UUID
    let timestamp: Date
    let operation: String
    let code: String
    let message: String
    let recovery: String
    let retryable: Bool
}
