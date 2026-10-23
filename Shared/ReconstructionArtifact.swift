import Foundation
import CoreFoundation
import CryptoKit

struct ReconstructionRange: Sendable, Hashable {
    let start: UInt64
    let end: UInt64
}

/// Shared scope planning for reconstruction and review exports. Section
/// membership requires complete containment, never merely an overlapping start.
enum ReconstructionScopePlan {
    static func address(_ text: String) -> UInt64? {
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let hexadecimal = value.hasPrefix("0x")
        let digits = hexadecimal ? String(value.dropFirst(2)) : value
        guard !digits.isEmpty, digits.utf8.allSatisfy({ byte in
            (48...57).contains(byte) || (hexadecimal && (97...102).contains(byte))
        }) else { return nil }
        return UInt64(digits, radix: hexadecimal ? 16 : 10)
    }

    static func section(address: String, words: String, unitBytes: UInt64) -> ReconstructionRange? {
        guard unitBytes == 4, let start = UInt64(address), let count = UInt64(words),
              count > 0, count <= (UInt64.max - start) / 4 else { return nil }
        return ReconstructionRange(start: start, end: start + count * 4)
    }

    static func indices(scope: String, address: UInt64?, section: ReconstructionRange?,
                        functions: [ReconstructionRange], maximum: Int = 4096) throws -> [Int] {
        guard maximum > 0, functions.allSatisfy({ $0.start < $0.end }) else {
            throw ReconstructionArtifact.Invalid(message: "Invalid reconstruction function bounds or limit.")
        }
        let indices: [Int]
        switch scope {
        case "whole", "resume": indices = Array(functions.indices)
        case "section":
            guard let section, section.start < section.end else {
                throw ReconstructionArtifact.Invalid(message: "Load an instruction page to establish the current code section bounds.")
            }
            indices = functions.indices.filter { functions[$0].start >= section.start && functions[$0].end <= section.end }
        case "address":
            guard let address else { throw ReconstructionArtifact.Invalid(message: "Choose an address for reconstruction.") }
            indices = functions.indices.filter { functions[$0].start <= address && address < functions[$0].end }
            guard indices.count <= 1 else {
                throw ReconstructionArtifact.Invalid(message: "Competing function bounds contain this address; resolve them before export.")
            }
        default: throw ReconstructionArtifact.Invalid(message: "Unsupported reconstruction scope.")
        }
        guard !indices.isEmpty, indices.count <= maximum else {
            throw ReconstructionArtifact.Invalid(message: "Reconstruction scope requires 1–\(maximum) discovered functions; refine the scope rather than silently truncating it.")
        }
        return indices
    }
}

/// Identity fence for keeping a decoded review artifact attached only to the
/// project record and analysis revision that produced it.
struct ReconstructionReviewContext: Sendable, Equatable {
    let project: String
    let record: String
    let analysis: String
    let digest: String

    func matches(project: String?, record: String?, analysis: String?, digest: String?) -> Bool {
        self.project == project && self.record == record && self.analysis == analysis && self.digest == digest
    }
}

struct ReconstructionBatchOutcome: Sendable {
    let planned: Int
    let attempted: Int
    let recovered: Int
    let reviewOnly: Int
    let failed: Int
    let status: String

    init(states: [String], planned: Int, cancelled: Bool) throws {
        guard (1...4096).contains(planned), states.count <= planned,
              states.allSatisfy({ ["recovered", "review-only", "failed"].contains($0) }) else {
            throw ReconstructionArtifact.Invalid(message: "Invalid reconstruction batch outcome.")
        }
        self.planned = planned
        attempted = states.count
        recovered = states.filter { $0 == "recovered" }.count
        reviewOnly = states.filter { $0 == "review-only" }.count
        failed = states.filter { $0 == "failed" }.count
        status = cancelled ? "cancelled" : attempted < planned ? "incomplete" : failed > 0 ? "completed-with-failures" : "completed"
    }

    var summary: String {
        "\(status): \(attempted)/\(planned) attempted; \(recovered) recovered, \(reviewOnly) review-only, \(failed) failed."
    }
    var metadata: [String: Any] {
        ["batch_status": status, "recovered_functions": recovered,
         "review_only_functions": reviewOnly, "failed_functions": failed]
    }
}

struct ReconstructionSource: Sendable, Identifiable {
    let id: String
    let kind: String
    let text: String
    let status: String
    let contract: String
    var languages: [String] = []
}

/// File extensions select a specific validated artifact, never a fallback
/// declaration or a body in a different reconstruction representation.
enum ReconstructionExportFormat: String, CaseIterable, Sendable {
    case c, cpp, objc, objcxx, header, native, swift

    var allowsCustomExtension: Bool { self == .native }

    var sourceKind: String {
        switch self {
        case .header: return "recovered-header"
        case .native: return "machine-state-c"
        default: return "recovered-source"
        }
    }

    var fileExtension: String {
        switch self {
        case .header: return "h"
        case .native: return "n"
        case .objc: return "m"
        case .objcxx: return "mm"
        default: return rawValue
        }
    }

    func source(in sources: [ReconstructionSource]) -> ReconstructionSource? {
        if self == .objc, let objc = sources.first(where: { $0.kind == "recovered-objective-c" }) { return objc }
        if let exact = sources.first(where: { $0.kind == sourceKind }) { return exact }
        if [.c, .objc].contains(self), let machine = sources.first(where: { $0.kind == "machine-state-c" }) { return machine }
        return sources.first {
            $0.kind == (self == .header ? "best-effort-header" : "best-effort-source")
        }
    }

    func render(_ source: ReconstructionSource) -> String? {
        guard (self == .objc && source.kind == "recovered-objective-c") || source.kind == sourceKind || source.kind == (self == .header ? "best-effort-header" : "best-effort-source") ||
                ([.c, .objc].contains(self) && source.kind == "machine-state-c") else { return nil }
        if [.swift, .cpp, .objcxx].contains(self) {
            return "// Recovered C/Objective-C evidence; translation to the selected language is unresolved.\n"
                + source.text.components(separatedBy: .newlines).map { "// " + $0 }.joined(separator: "\n")
        }
        return source.text
    }
}

struct ReconstructionExportFile: Codable, Sendable {
    let path: String
    let bytes: Int
    let sha256: String
}
struct ReconstructionExpression: Sendable, Identifiable {
    let id: String
    let kind: String
    let type: String
    let description: String
    let ranges: [ReconstructionRange]
    let evidence: [String]
}
struct ReconstructionEvidence: Sendable, Identifiable {
    let id: String
    let kind: String
    let provenance: String
    let description: String
    let ranges: [ReconstructionRange]
}
struct ReconstructionSemanticCoverage: Sendable {
    let instructions: Int
    let sharedIR: Int
    let modeledTrees: Int
    let dataInCode: Int
    let unsupportedReasons: Int
    let strictMachineStateBlocked: Bool
}
struct ReconstructionReply: Codable, Sendable {
    let requestID: String
    let message: String
    let artifactJSON: String?
    static func encode(id: String, artifact: String? = nil, message: String) -> String {
        let value = Self(requestID: id, message: String(message.prefix(4096)), artifactJSON: artifact)
        if let data = try? JSONEncoder().encode(value), data.count <= 12 * 1024 * 1024 { return String(decoding: data, as: UTF8.self) }
        return "{\"requestID\":\"\(id)\",\"message\":\"Reconstruction reply exceeds its transport budget.\"}"
    }
}

/// Native consumer of the worker's versioned artifact. It verifies content
/// identities and graph integrity; it does not certify arbitrary C semantics,
/// execute generated source, or promote compilation to equivalence.
struct ReconstructionArtifact: Sendable {
    static let passVersion = "arm64-reconstruction-62"
    static let maxBytes = 4 * 1024 * 1024
    let json: String
    let digest: String
    let start: UInt64
    let end: UInt64
    let pass: String
    let settings: String
    let eligibility: String
    let blockers: [String]
    let review: String
    let sources: [ReconstructionSource]
    let expressions: [ReconstructionExpression]
    let evidence: [String: ReconstructionEvidence]
    let semanticCoverage: ReconstructionSemanticCoverage
    private let incoming: [String: [String]]

    struct Invalid: LocalizedError { let message: String; var errorDescription: String? { message } }
    private static func require(_ condition: Bool, _ message: String) throws { if !condition { throw Invalid(message: message) } }
    private static func hash(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
    private static func frame(_ bytes: Data, into output: inout Data) {
        var size = UInt64(bytes.count).bigEndian
        withUnsafeBytes(of: &size) { output.append(contentsOf: $0) }; output.append(bytes)
    }
    /// Typed, length-framed canonical encoding shared with Rust and Python.
    private static func canonical(_ value: Any, into output: inout Data, depth: Int, visits: inout Int) throws {
        visits += 1
        try require(depth <= 64 && visits <= 2_000_000 && output.count <= 32 * 1024 * 1024, "Reconstruction value budget exceeded.")
        if value is NSNull { output.append(110) }
        else if let number = value as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() { output.append(number.boolValue ? 116 : 102) }
            else {
                let type = String(cString: number.objCType)
                let text: String
                if type == "d" || type == "f" {
                    let real = number.doubleValue
                    try require(real.isFinite && real.rounded(.towardZero) == real && real >= Double(Int64.min) && real < 9_223_372_036_854_775_808, "Unsupported reconstruction numeric representation.")
                    text = String(Int64(real))
                } else { text = number.stringValue }
                try require(!text.isEmpty && text.utf8.allSatisfy { (48...57).contains($0) || $0 == 45 }, "Invalid reconstruction integer.")
                output.append(105); frame(Data(text.utf8), into: &output)
            }
        } else if let text = value as? String { output.append(115); frame(Data(text.utf8), into: &output) }
        else if let values = value as? [Any] {
            output.append(97); var count = UInt64(values.count).bigEndian; withUnsafeBytes(of: &count) { output.append(contentsOf: $0) }
            for child in values { try canonical(child, into: &output, depth: depth + 1, visits: &visits) }
        } else if let fields = value as? [String: Any] {
            output.append(111); var count = UInt64(fields.count).bigEndian; withUnsafeBytes(of: &count) { output.append(contentsOf: $0) }
            for (key, child) in fields.sorted(by: { $0.key.utf8.lexicographicallyPrecedes($1.key.utf8) }) {
                try canonical(key, into: &output, depth: depth + 1, visits: &visits)
                try canonical(child, into: &output, depth: depth + 1, visits: &visits)
            }
        } else { throw Invalid(message: "Unsupported reconstruction value.") }
    }
    private static func content(_ value: Any) throws -> String {
        var bytes = Data(), visits = 0; try canonical(value, into: &bytes, depth: 0, visits: &visits); return hash(bytes)
    }
    private static func hex(_ text: String) -> UInt64? {
        guard text.hasPrefix("0x"), (3...18).contains(text.utf8.count), text.dropFirst(2).utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else { return nil }
        return UInt64(text.dropFirst(2), radix: 16)
    }
    private static func identifier(_ text: String) -> Bool { text.count == 71 && text.hasPrefix("sha256:") && text.dropFirst(7).utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } }

    static func decode(_ json: String, digest: String, start: UInt64, end: UInt64) throws -> Self {
        try require(json.utf8.count <= maxBytes && start < end, "Reconstruction bounds or byte budget exceeded.")
        guard let document = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              let schema = document["schema"] as? Int, schema == 1,
              document["producer"] as? String == "hindsight-reconstruction-v1",
              document["source_sha256"] as? String == digest,
              let scope = document["scope"] as? [String: String], let pass = scope["pass_version"], let settings = scope["settings_digest"], identifier(settings),
              let function = document["function"] as? [String: Any], let functionID = function["id"] as? String,
              (function["start"] as? String).flatMap(hex) == start, (function["end"] as? String).flatMap(hex) == end,
              ["arm64", "arm64e"].contains(function["architecture"] as? String ?? ""), function["abi"] as? String == "apple-aapcs64-lp64",
              let nodes = document["nodes"] as? [[String: Any]], let edges = document["edges"] as? [[String: Any]],
              let ast = document["ast"] as? [String: Any], let root = ast["root"] as? String, let astNodes = ast["nodes"] as? [[String: Any]],
              let review = ast["rendering"] as? String, review.utf8.count <= 1_048_576,
              let eligibility = document["eligibility"] as? [String: Any], let state = eligibility["state"] as? String,
              let reasons = eligibility["reasons"] as? [[String: Any]], let truncated = document["truncated"] as? Bool,
              let budgets = document["budgets"] as? [String: Any], let limits = budgets["limits"] as? [String: Int], let consumed = budgets["consumed"] as? [String: Int], let exhausted = budgets["exhausted"] as? [String],
              let validation = document["validation"] as? [[String: Any]], let alternatives = document["alternatives"] as? [[String: Any]] else { throw Invalid(message: "Reconstruction identity, schema or bounds mismatch.") }
        try require(Set(document.keys) == ["schema","producer","source_sha256","analysis_engine","scope","function","eligibility","nodes","edges","ast","alternatives","budgets","validation","truncated"], "Unexpected reconstruction fields.")
        try require(document["analysis_engine"] as? String == "arm64-analysis-v103-schema129" && pass == passVersion, "Unsupported reconstruction engine/pass; regenerate with the bundled worker.")
        try require(nodes.count <= 100_000 && edges.count <= 400_000 && astNodes.count <= 100_000 && reasons.count <= 128 && validation.count <= 128 && alternatives.count <= 1024, "Reconstruction record budget exceeded.")
        let scopeKey: [Any] = [pass, settings]
        var identities = Set<String>()
        func check(_ domain: String, _ record: [String: Any], function: String?) throws -> String {
            guard let id = record["id"] as? String, identifier(id) else { throw Invalid(message: "Invalid reconstruction content identity.") }
            var body = record; body.removeValue(forKey: "id")
            let expected = "sha256:" + (try content(["hindsight-content-v2", domain, digest, function as Any? ?? NSNull(), scopeKey, body] as [Any]))
            try require(id == expected && identities.insert(id).inserted, "Reconstruction content changed or duplicated.")
            return id
        }
        _ = try check("function", function, function: nil)
        func spans(_ record: [String: Any], local: Bool) throws -> [ReconstructionRange] {
            guard let values = record["ranges"] as? [[String: String]], values.count <= 256 else { throw Invalid(message: "Invalid reconstruction source spans.") }
            var result: [ReconstructionRange] = []
            for span in values {
                guard Set(span.keys) == ["start","end"], let a = span["start"].flatMap(hex), let b = span["end"].flatMap(hex), a < b else { throw Invalid(message: "Invalid reconstruction source span.") }
                try require((!local || (a >= start && b <= end)) && !result.contains(where: { a < $0.end && $0.start < b }), "Reconstruction spans overlap or escape their function.")
                result.append(ReconstructionRange(start: a, end: b))
            }; return result
        }
        func description(_ value: Any) -> String {
            guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), let text = String(data: data, encoding: .utf8) else { return "" }
            return String(text.prefix(4096))
        }
        let localKinds: Set<String> = ["decode","ir-operation","definition","use","phi","memory-definition","memory-use","memory-phi","memory-access","file-definition","file-use","file-phi","cfg-block","cfg-edge","ast-node"]
        var evidence: [String: ReconstructionEvidence] = [:], sources: [ReconstructionSource] = [], blockers: [String] = [], unsupported = false
        var semanticCoverage: ReconstructionSemanticCoverage?
        var decodedCount = 0, sharedIRCount = 0, modeledTreeCount = 0, dataInCodeCount = 0, unsupportedReasonCount = 0
        for node in nodes {
            let id = try check("evidence", node, function: functionID)
            guard let kind = node["kind"] as? String, let provenance = node["epistemic"] as? String, ["fact","derived","assumption","analyst-assertion","rejected"].contains(provenance), let payload = node["payload"] as? [String: Any] else { throw Invalid(message: "Invalid reconstruction evidence.") }
            let ranges = try spans(node, local: localKinds.contains(kind))
            unsupported = unsupported || kind == "unsupported-effect"
            if kind == "decode" {
                decodedCount += 1
                if payload["shared_ir_available"] as? Bool == true { sharedIRCount += 1 }
                if payload["data_in_code"] as? Bool == true { dataInCodeCount += 1 }
            } else if kind == "ir-operation" {
                if !(payload["tree"] is NSNull) { modeledTreeCount += 1 }
            } else if kind == "unsupported-effect" {
                unsupportedReasonCount += 1
            }
            evidence[id] = ReconstructionEvidence(id: id, kind: kind, provenance: provenance, description: description(payload), ranges: ranges)
            if payload["artifact_kind"] as? String == "semantic-coverage" {
                guard semanticCoverage == nil,
                      let instructions = payload["instruction_count"] as? Int,
                      let sharedIR = payload["shared_ir_count"] as? Int,
                      let modeledTrees = payload["modeled_tree_count"] as? Int,
                      let dataInCode = payload["data_in_code_count"] as? Int,
                      let unsupportedReasons = payload["unsupported_effect_reason_count"] as? Int,
                      let strictBlocked = payload["strict_machine_state_blocked"] as? Bool,
                      payload["scope"] as? String == "full captured function; one record per decoded instruction",
                      (payload["interpretation"] as? String)?.contains("do not certify complete") == true else { throw Invalid(message: "Invalid semantic coverage summary.") }
                semanticCoverage = ReconstructionSemanticCoverage(instructions: instructions, sharedIR: sharedIR, modeledTrees: modeledTrees, dataInCode: dataInCode, unsupportedReasons: unsupportedReasons, strictMachineStateBlocked: strictBlocked)
            }
            if let settingsValue = payload["analysis_settings"] as? [String: Any] { try require("sha256:" + (try content(["hindsight-settings-v1", settingsValue])) == settings, "Retained reconstruction settings do not match their digest.") }
            if let kind = payload["artifact_kind"] as? String, ["recovered-source","recovered-objective-c","machine-state-c","recovered-header","best-effort-source","best-effort-header"].contains(kind), let text = payload["source"] as? String {
                let bestEffort = kind.hasPrefix("best-effort-")
                let declarationOnly = kind == "recovered-header" || kind == "best-effort-header"
                try require(text.utf8.count <= 1_048_576 && payload["status"] as? String == (bestEffort ? "review-only-uncompiled" : declarationOnly ? "declaration-only-uncompiled" : "generated-uncompiled"), "Invalid generated-source status or budget.")
                if declarationOnly { try require(payload["ast_root"] == nil, "Declaration-only header claims a body AST.") }
                let languages = payload["languages"] as? [String] ?? []
                if declarationOnly {
                    try require(!languages.isEmpty && Set(languages).count == languages.count &&
                        languages.allSatisfy { ["c", "c++", "objective-c", "objective-c++"].contains($0) },
                        "Invalid or missing header language contract.")
                }
                sources.append(ReconstructionSource(id: id, kind: kind, text: text, status: bestEffort ? "Best effort; unresolved parts; not compilation/ABI qualified" : declarationOnly ? "Declaration only; implementation recovery not implied" : "Generated; not compiled in this app", contract: payload["contract"] as? String ?? "Unspecified", languages: languages))
            }
            if let reason = payload["reason"] as? String { blockers.append(reason) }
        }
        guard let semanticCoverage else { throw Invalid(message: "Semantic coverage summary is missing.") }
        let expectedStrictBlocked = unsupportedReasonCount > 0 || truncated || !exhausted.isEmpty
        try require(semanticCoverage.instructions == decodedCount && semanticCoverage.sharedIR == sharedIRCount && semanticCoverage.modeledTrees == modeledTreeCount && semanticCoverage.dataInCode == dataInCodeCount && semanticCoverage.unsupportedReasons == unsupportedReasonCount && semanticCoverage.strictMachineStateBlocked == expectedStrictBlocked, "Semantic coverage summary disagrees with instruction evidence.")
        var incoming: [String: [String]] = [:]
        for edge in edges {
            _ = try check("edge", edge, function: functionID)
            guard let source = edge["source"] as? String, let target = edge["target"] as? String, evidence[source] != nil, evidence[target] != nil else { throw Invalid(message: "Dangling reconstruction graph edge.") }
            incoming[target, default: []].append(source)
        }
        var expressions: [ReconstructionExpression] = [], children: [String: [String]] = [:]
        for node in astNodes {
            let id = try check("ast", node, function: functionID)
            guard let kind = node["kind"] as? String, let refs = node["evidence"] as? [String], !refs.isEmpty, refs.count <= 4096, refs.allSatisfy({ evidence[$0] != nil }), let childIDs = node["children"] as? [String], childIDs.count <= 4096 else { throw Invalid(message: "Invalid reconstruction AST dependencies.") }
            let ranges = try spans(node, local: true); try require(!ranges.isEmpty, "AST source spans are missing.")
            children[id] = childIDs
            expressions.append(ReconstructionExpression(id: id, kind: kind, type: node["type"] as? String ?? "unresolved", description: description(node["attributes"] ?? [:]), ranges: ranges, evidence: refs))
        }
        try require(children[root] != nil && children.values.joined().allSatisfy { children[$0] != nil }, "Missing reconstruction AST root/child.")
        var seen = Set<String>(), active = Set<String>(), pending: [(String, Bool)] = [(root, false)]
        while let (id, leaving) = pending.popLast() {
            if leaving { active.remove(id); seen.insert(id) }
            else if active.contains(id) { throw Invalid(message: "Cyclic reconstruction AST.") }
            else if !seen.contains(id) {
                guard let childIDs = children[id] else { throw Invalid(message: "Missing reconstruction AST node.") }
                active.insert(id); pending.append((id, true)); for child in childIDs.reversed() { pending.append((child, false)) }
            }
        }
        try require(seen.count == children.count, "Orphaned reconstruction AST.")
        for alternative in alternatives {
            _ = try check("alternative", alternative, function: functionID)
            guard let subject = alternative["subject"] as? String, evidence[subject] != nil, let candidates = alternative["candidates"] as? [String], Set(candidates).count == candidates.count, candidates.allSatisfy({ evidence[$0] != nil }) else { throw Invalid(message: "Invalid reconstruction alternatives.") }
            if let selected = alternative["selected"] as? String { try require(candidates.contains(selected), "Selected reconstruction alternative is missing.") }
        }
        for reason in reasons {
            guard let blocking = reason["blocking"] as? Bool, let refs = reason["evidence"] as? [String], refs.allSatisfy({ evidence[$0] != nil }) else { throw Invalid(message: "Invalid reconstruction blocker.") }
            if blocking { blockers.append(reason["message"] as? String ?? "Unspecified blocker") }
        }
        try require(Set(limits.keys) == Set(consumed.keys) && Set(exhausted).isSubset(of: Set(limits.keys)) && limits.values.allSatisfy { $0 > 0 } && consumed.values.allSatisfy { $0 >= 0 }, "Invalid reconstruction budgets.")
        for (key, limit) in limits {
            guard let used = consumed[key] else { throw Invalid(message: "Missing reconstruction budget.") }
            try require(used <= limit || exhausted.contains(key), "Unreported reconstruction exhaustion.")
        }
        // Current bundled producer emits review/blocked artifacts. Do not accept
        // an invented recompilable/equivalent label from an untrusted payload.
        try require(["review-only","blocked"].contains(state) && ast["compilable"] as? Bool == false, "Unsupported reconstruction eligibility claim.")
        try require(state != "review-only" || (!truncated && exhausted.isEmpty && !unsupported && reasons.allSatisfy { $0["blocking"] as? Bool == false }), "Incomplete reconstruction claims review eligibility.")
        try require(state != "blocked" || reasons.contains { $0["blocking"] as? Bool == true }, "Blocked reconstruction lacks a reason.")
        var candidate = document; candidate.removeValue(forKey: "validation"); candidate.removeValue(forKey: "eligibility")
        let candidateDigest = try content(["hindsight-candidate-v1", candidate])
        for run in validation {
            _ = try check("validation", run, function: functionID)
            try require(run["artifact_sha256"] as? String == candidateDigest && (run["status"] as? String != "passed" || (run["mismatches"] as? [Any])?.isEmpty == true), "Validation belongs to a different reconstruction or contradicts its status.")
        }
        return Self(json: json, digest: digest, start: start, end: end, pass: pass, settings: settings, eligibility: state, blockers: Array(Set(blockers)).sorted(), review: review, sources: sources, expressions: expressions, evidence: evidence, semanticCoverage: semanticCoverage, incoming: incoming)
    }

    func why(_ ids: [String], limit: Int = 512) -> [ReconstructionEvidence] {
        var pending = ids, seen = Set<String>(), result: [ReconstructionEvidence] = []
        while let id = pending.popLast(), seen.count < min(2048, max(1, limit)) {
            guard seen.insert(id).inserted, let node = evidence[id] else { continue }
            result.append(node); pending.append(contentsOf: incoming[id] ?? [])
        }
        return result.sorted { $0.id < $1.id }
    }

    /// Write either the complete review artifact or one of its generated
    /// sources. Source selection is identity-bound to this validated artifact.
    func writeReviewExport(sourceID: String? = nil, to url: URL) throws {
        let text: String
        if let sourceID {
            guard let source = sources.first(where: { $0.id == sourceID }) else {
                throw Invalid(message: "Selected generated source does not belong to this reconstruction artifact.")
            }
            text = source.text
        } else { text = json }
        try text.write(to: url, atomically: true, encoding: .utf8)
    }

    /// Export exactly this validated function. Preflight the entire output set
    /// before writing so name collisions and byte limits preserve prior work.
    func writeBatchExport(to directory: URL, maximumBytes: Int) throws -> [ReconstructionExportFile] {
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        try Self.require(values.isDirectory == true && values.isSymbolicLink != true,
                         "Batch output must be a real directory.")
        let prefix = String(format: "function-%016llx", start)
        var outputs = [(prefix + ".reconstruction.json", Data(json.utf8))]
        var kinds = Set<String>()
        for source in sources {
            try Self.require(kinds.insert(source.kind).inserted, "Duplicate generated output kind.")
            let suffix: String
            switch source.kind {
            case "recovered-source", "best-effort-source": suffix = ".c"
            case "recovered-objective-c": suffix = ".m"
            case "recovered-header", "best-effort-header": suffix = ".h"
            case "machine-state-c": suffix = ".machine.c"
            default: throw Invalid(message: "Unsupported generated output kind.")
            }
            outputs.append((prefix + suffix, Data(source.text.utf8)))
        }
        let bytes = outputs.reduce(0) { $0 + $1.1.count }
        try Self.require(Set(outputs.map(\.0)).count == outputs.count, "Competing source representations have the same output path.")
        try Self.require(maximumBytes >= 0 && bytes <= maximumBytes, "Batch output byte budget exhausted.")
        for (filename, _) in outputs {
            let target = directory.appendingPathComponent(filename)
            let link = try? target.resourceValues(forKeys: [.isSymbolicLinkKey])
            try Self.require(!FileManager.default.fileExists(atPath: target.path) && link?.isSymbolicLink != true,
                             "Batch output already exists; prior evidence was preserved.")
        }
        var files: [ReconstructionExportFile] = []
        for (filename, data) in outputs {
            try data.write(to: directory.appendingPathComponent(filename), options: .withoutOverwriting)
            files.append(ReconstructionExportFile(path: filename, bytes: data.count, sha256: Self.hash(data)))
        }
        return files
    }

    private struct VerifiedBatchEntry {
        let start: UInt64
        let end: UInt64
        let state: String
        let artifact: ReconstructionArtifact?
    }

    static func verifiedBatchCheckpoint(at directory: URL, digest: String, worker: String,
                                         revision: String) throws -> Data {
        try verifiedBatch(at: directory, digest: digest, worker: worker, revision: revision).data
    }

    private static func readBatchBytes(at url: URL, maximum: Int) throws -> Data {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let bytes = try handle.read(upToCount: maximum + 1) ?? Data()
        try require(bytes.count <= maximum, "Batch input grew beyond its byte budget.")
        return bytes
    }

    private static func verifiedBatch(at directory: URL, digest: String, worker: String,
                                      revision: String) throws -> (data: Data, entries: [VerifiedBatchEntry]) {
        let values = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
        try require(values.isDirectory == true && values.isSymbolicLink != true, "Unsafe batch directory.")
        let path = directory.appendingPathComponent("checkpoint.json")
        let metadata = try path.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        try require(metadata.isRegularFile == true && metadata.isSymbolicLink != true
                    && (metadata.fileSize ?? Int.max) <= 16 * 1024 * 1024, "Unsafe checkpoint.")
        let data = try readBatchBytes(at: path, maximum: 16 * 1024 * 1024)
        guard let document = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              document["schema"] as? Int == 1, document["source_sha256"] as? String == digest,
              document["pass"] as? String == passVersion,
              worker != "unavailable", document["worker"] as? String == worker,
              document["revision"] as? String == revision,
              let entries = document["functions"] as? [[String: Any]], entries.count <= 4096 else {
            throw Invalid(message: "Checkpoint source, worker or reconstruction version changed.")
        }
        var seen = Set<String>(), total = 0
        if let all = document["requested_dump_all"] {
            guard let all = all as? Bool, !all || (document["requested_format"] as? String ?? "").isEmpty else {
                throw Invalid(message: "Invalid or conflicting checkpoint dump-all intent.")
            }
        }
        if let requested = document["requested_format"] {
            guard let requested = requested as? String,
                  requested.isEmpty || ReconstructionExportFormat(rawValue: requested) != nil else {
                throw Invalid(message: "Unsupported checkpoint dump format.")
            }
        }
        var verified: [VerifiedBatchEntry] = []
        for entry in entries {
            try require(["recovered", "review-only", "failed"].contains(entry["state"] as? String ?? ""),
                        "Invalid checkpoint state.")
            guard let startText = entry["start"] as? String, let start = hex(startText),
                  let endText = entry["end"] as? String, let end = hex(endText), start < end else {
                throw Invalid(message: "Invalid checkpoint function bounds.")
            }
            let files = entry["files"] as? [[String: Any]] ?? []
            try require(files.count <= 5, "Checkpoint file count exceeded.")
            let prefix = String(format: "function-%016llx", start)
            var retained: [String: Data] = [:]
            for file in files {
                guard let name = file["path"] as? String,
                      [".reconstruction.json", ".c", ".h", ".m", ".machine.c"].contains(where: { name == prefix + $0 }),
                      let size = file["bytes"] as? Int, size >= 0, size <= maxBytes,
                      let digest = file["sha256"] as? String, seen.insert(name).inserted else {
                    throw Invalid(message: "Invalid checkpoint output identity.")
                }
                total += size
                try require(total <= 256 * 1024 * 1024, "Checkpoint byte budget exceeded.")
                let url = directory.appendingPathComponent(name)
                let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
                try require(values.isRegularFile == true && values.isSymbolicLink != true && values.fileSize == size,
                            "Retained batch file changed.")
                let bytes = try readBatchBytes(at: url, maximum: size)
                try require(bytes.count == size && hash(bytes) == digest, "Retained batch hash mismatch.")
                retained[name] = bytes
            }
            var decoded: ReconstructionArtifact?
            if entry["state"] as? String != "failed" {
                guard let bytes = retained[prefix + ".reconstruction.json"] else {
                    throw Invalid(message: "Retained artifact is missing.")
                }
                let artifact = try decode(String(decoding: bytes, as: UTF8.self), digest: digest, start: start, end: end)
                try require(retained.count == artifact.sources.count + 1, "Retained sources are missing.")
                for source in artifact.sources {
                    let suffix = ["recovered-header", "best-effort-header"].contains(source.kind) ? ".h" : source.kind == "recovered-objective-c" ? ".m" : source.kind == "machine-state-c" ? ".machine.c" : ".c"
                    try require(retained[prefix + suffix] == Data(source.text.utf8), "Retained source differs from its artifact.")
                }
                try require((entry["state"] as? String == "recovered") == artifact.sources.contains(where: { $0.kind == "recovered-source" }),
                            "Checkpoint recovery state disagrees with its artifact.")
                decoded = artifact
            }
            verified.append(VerifiedBatchEntry(start: start, end: end, state: entry["state"] as? String ?? "failed", artifact: decoded))
        }
        try require(document["output_bytes"] as? Int == total, "Checkpoint byte accounting mismatch.")
        let outcomeKeys = ["batch_status", "recovered_functions", "review_only_functions", "failed_functions"]
        if outcomeKeys.contains(where: { document[$0] != nil }) {
            guard let planned = document["planned"] as? [[String: Any]], let cancelled = document["cancelled"] as? Bool else {
                throw Invalid(message: "Checkpoint outcome lacks its planned scope or cancellation state.")
            }
            let outcome = try ReconstructionBatchOutcome(states: verified.map(\.state), planned: planned.count, cancelled: cancelled)
            try require(document["batch_status"] as? String == outcome.status &&
                        document["recovered_functions"] as? Int == outcome.recovered &&
                        document["review_only_functions"] as? Int == outcome.reviewOnly &&
                        document["failed_functions"] as? Int == outcome.failed,
                        "Checkpoint outcome counters disagree with retained attempts.")
        }
        return (data, verified)
    }

    /// Produce an exact representation dump from verified in-memory artifacts,
    /// never reopening mutable source paths after their validation.
    static func writeBatchAllFormatExport(from directory: URL, to output: URL,
                                         digest: String, worker: String, revision: String,
                                         maximumBytes: Int = 256 * 1024 * 1024) throws -> Data {
        let checkpoint = try verifiedBatchCheckpoint(at: directory, digest: digest, worker: worker, revision: revision)
        try require(!FileManager.default.fileExists(atPath: output.path)
                    && (try? output.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) != true,
                    "Choose a new dump-all directory; existing output is preserved.")
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: false)
        var formats: [[String: Any]] = []
        var total = 0
        for format in [ReconstructionExportFormat.c, .header, .objc, .native] {
            let data = try writeBatchFormatExport(from: directory,
                to: output.appendingPathComponent(format.fileExtension, isDirectory: true),
                format: format, digest: digest, worker: worker, revision: revision,
                maximumBytes: maximumBytes - total)
            guard let manifest = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  manifest["source_checkpoint_sha256"] as? String == hash(checkpoint),
                  let bytes = manifest["output_bytes"] as? Int else {
                throw Invalid(message: "Batch changed during dump-all; partial output retained without a completion manifest.")
            }
            total += bytes + data.count
            try require(total <= maximumBytes, "Dump-all byte budget exhausted; partial output retained.")
            formats.append(["folder": format.fileExtension, "format": format.rawValue,
                            "available": manifest["available"] ?? 0, "unavailable": manifest["unavailable"] ?? 0,
                            "manifest_sha256": hash(data)])
        }
        let manifest = try JSONSerialization.data(withJSONObject: [
            "schema": 1, "kind": "reconstruction-all-formats-export", "source_sha256": digest,
            "source_checkpoint_sha256": hash(checkpoint), "output_bytes": total,
            "validation": "export only; compilation and behavior unqualified", "formats": formats
        ], options: [.prettyPrinted, .sortedKeys])
        try require(total + manifest.count <= maximumBytes, "Dump-all manifest exceeds byte budget; partial output retained.")
        try manifest.write(to: output.appendingPathComponent("export-manifest.json"), options: .withoutOverwriting)
        return manifest
    }

    static func writeBatchFormatExport(from directory: URL, to output: URL,
                                      format: ReconstructionExportFormat, digest: String, worker: String,
                                      revision: String, maximumBytes: Int = 256 * 1024 * 1024) throws -> Data {
        let batch = try verifiedBatch(at: directory, digest: digest, worker: worker, revision: revision)
        try require(!batch.entries.isEmpty, "No completed function attempts are available to dump.")
        var outputs: [(String, Data)] = []
        var entries: [[String: Any]] = []
        var total = 0, available = 0
        for entry in batch.entries {
            let prefix = String(format: "function-%016llx", entry.start)
            var files: [[String: Any]] = []
            let source = entry.artifact.flatMap { format.source(in: $0.sources) }
            var planned: [(String, Data)] = []
            if let artifact = entry.artifact {
                planned.append((prefix + ".reconstruction.json", Data(artifact.json.utf8)))
            }
            if let source, let rendered = format.render(source) {
                planned.append((prefix + "." + format.fileExtension, Data(rendered.utf8)))
                available += 1
            }
            for (name, bytes) in planned {
                total += bytes.count
                try require(maximumBytes >= 0 && total <= maximumBytes, "Dump byte budget exhausted before publication.")
                outputs.append((name, bytes))
                files.append(["path": name, "bytes": bytes.count, "sha256": hash(bytes)])
            }
            entries.append(["start": String(format: "0x%016llx", entry.start),
                            "end": String(format: "0x%016llx", entry.end), "state": entry.state,
                            "available": source != nil, "files": files,
                            "representation": source?.kind ?? "unavailable",
                            "languages": source?.languages ?? [],
                            "blockers": entry.artifact?.blockers ?? ["Function reconstruction failed."]])
        }
        let manifest = try JSONSerialization.data(withJSONObject: [
            "schema": 1, "kind": "reconstruction-format-export", "format": format.rawValue,
            "artifact_kind": format.sourceKind, "source_sha256": digest, "worker": worker,
            "pass": passVersion, "revision": revision, "source_checkpoint_sha256": hash(batch.data),
            "available": available, "unavailable": entries.count - available, "output_bytes": total,
            "failed_functions": batch.entries.filter { $0.state == "failed" }.count,
            "best_effort": entries.filter { ($0["representation"] as? String)?.hasPrefix("best-effort-") == true }.count,
            "coverage": "validated retained batch only; undiscovered functions remain unknown",
            "translation": [.cpp, .objcxx, .swift].contains(format) ? "unresolved; commented evidence only" : "not requested",
            "validation": "export only; compilation and behavior unqualified", "functions": entries
        ], options: [.prettyPrinted, .sortedKeys])
        try require(!FileManager.default.fileExists(atPath: output.path)
                    && (try? output.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) != true,
                    "Choose a new dump directory; existing output is preserved.")
        try FileManager.default.createDirectory(at: output, withIntermediateDirectories: false)
        for (name, bytes) in outputs { try bytes.write(to: output.appendingPathComponent(name), options: .withoutOverwriting) }
        try manifest.write(to: output.appendingPathComponent("export-manifest.json"), options: .withoutOverwriting)
        return manifest
    }
}
