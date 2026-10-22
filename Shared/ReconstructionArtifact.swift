import Foundation
import CoreFoundation
import CryptoKit

struct ReconstructionRange: Sendable, Hashable {
    let start: UInt64
    let end: UInt64
}
struct ReconstructionSource: Sendable, Identifiable {
    let id: String
    let kind: String
    let text: String
    let status: String
    let contract: String
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
        try require(document["analysis_engine"] as? String == "arm64-analysis-v93-schema122" && pass == "arm64-reconstruction-12", "Unsupported reconstruction engine/pass; regenerate with the bundled worker.")
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
        let localKinds: Set<String> = ["decode","ir-operation","definition","use","phi","memory-definition","memory-use","memory-phi","file-definition","file-use","file-phi","cfg-block","cfg-edge","ast-node"]
        var evidence: [String: ReconstructionEvidence] = [:], sources: [ReconstructionSource] = [], blockers: [String] = [], unsupported = false
        for node in nodes {
            let id = try check("evidence", node, function: functionID)
            guard let kind = node["kind"] as? String, let provenance = node["epistemic"] as? String, ["fact","derived","assumption","analyst-assertion","rejected"].contains(provenance), let payload = node["payload"] as? [String: Any] else { throw Invalid(message: "Invalid reconstruction evidence.") }
            let ranges = try spans(node, local: localKinds.contains(kind))
            unsupported = unsupported || kind == "unsupported-effect"
            evidence[id] = ReconstructionEvidence(id: id, kind: kind, provenance: provenance, description: description(payload), ranges: ranges)
            if let settingsValue = payload["analysis_settings"] as? [String: Any] { try require("sha256:" + (try content(["hindsight-settings-v1", settingsValue])) == settings, "Retained reconstruction settings do not match their digest.") }
            if let kind = payload["artifact_kind"] as? String, ["recovered-source","machine-state-c"].contains(kind), let text = payload["source"] as? String {
                try require(text.utf8.count <= 1_048_576 && payload["status"] as? String == "generated-uncompiled", "Invalid generated-source status or budget.")
                sources.append(ReconstructionSource(id: id, kind: kind, text: text, status: "Generated; not compiled in this app", contract: payload["contract"] as? String ?? "Unspecified"))
            }
            if let reason = payload["reason"] as? String { blockers.append(reason) }
        }
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
        return Self(json: json, digest: digest, start: start, end: end, pass: pass, settings: settings, eligibility: state, blockers: Array(Set(blockers)).sorted(), review: review, sources: sources, expressions: expressions, evidence: evidence, incoming: incoming)
    }

    func why(_ ids: [String], limit: Int = 512) -> [ReconstructionEvidence] {
        var pending = ids, seen = Set<String>(), result: [ReconstructionEvidence] = []
        while let id = pending.popLast(), seen.count < min(2048, max(1, limit)) {
            guard seen.insert(id).inserted, let node = evidence[id] else { continue }
            result.append(node); pending.append(contentsOf: incoming[id] ?? [])
        }
        return result.sorted { $0.id < $1.id }
    }
}
