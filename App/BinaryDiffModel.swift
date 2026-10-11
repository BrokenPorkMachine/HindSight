import Foundation

enum VGBinaryDiffStatus: String, Codable, Hashable, CaseIterable {
    case unchanged
    case changed
    case added
    case removed
}

struct VGFunctionDiff: Identifiable, Hashable {
    let baseline: VGFunctionRecord?
    let candidate: VGFunctionRecord?
    let status: VGBinaryDiffStatus
    let matchReason: String
    let codeChanged: Bool
    let cfgChanged: Bool
    let referencesChanged: Bool
    let semanticsChanged: Bool
    let baselineInstructionCount: Int
    let candidateInstructionCount: Int
    let baselineBlockCount: Int
    let candidateBlockCount: Int
    let baselineReferenceCount: Int
    let candidateReferenceCount: Int
    let baselineLiftedCount: Int
    let candidateLiftedCount: Int

    var id: String {
        let lhs = baseline.map { String($0.start) } ?? "none"
        let rhs = candidate.map { String($0.start) } ?? "none"
        return "\(lhs):\(rhs):\(status.rawValue)"
    }

    var displayName: String {
        let left = baseline?.name ?? ""
        let right = candidate?.name ?? ""
        if !right.isEmpty && right != "<discovered>" { return right }
        if !left.isEmpty && left != "<discovered>" { return left }
        if let candidate { return candidate.startText }
        if let baseline { return baseline.startText }
        return "<unknown>"
    }

    var baselineAddress: String { baseline?.startText ?? "—" }
    var candidateAddress: String { candidate?.startText ?? "—" }

    var changeSummary: String {
        switch status {
        case .added: return "Added function"
        case .removed: return "Removed function"
        case .unchanged: return "No modeled change"
        case .changed:
            var parts: [String] = []
            if codeChanged { parts.append("code") }
            if cfgChanged { parts.append("CFG") }
            if referencesChanged { parts.append("xrefs") }
            if semanticsChanged { parts.append("lifted semantics") }
            return parts.isEmpty ? "Metadata changed" : parts.joined(separator: " • ")
        }
    }
}

struct VGBinaryDiffReport {
    let baselineName: String
    let candidateName: String
    let functions: [VGFunctionDiff]

    var unchangedCount: Int { functions.filter { $0.status == .unchanged }.count }
    var changedCount: Int { functions.filter { $0.status == .changed }.count }
    var addedCount: Int { functions.filter { $0.status == .added }.count }
    var removedCount: Int { functions.filter { $0.status == .removed }.count }
    var semanticChangeCount: Int { functions.filter(\.semanticsChanged).count }
    var cfgChangeCount: Int { functions.filter(\.cfgChanged).count }
}

private struct VGFunctionEvidence {
    let function: VGFunctionRecord
    let instructionSignatures: [String]
    let blockSignatures: [String]
    let referenceSignatures: [String]
    let liftedSignatures: [String]
    let codeFingerprint: UInt64

    var strongName: String? {
        let name = function.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != "<discovered>", !name.lowercased().hasPrefix("sub_") else { return nil }
        return name
    }
}

enum VGBinaryDiffEngine {
    static func compare(
        baselineName: String,
        baseline: AnalysisReport,
        candidateName: String,
        candidate: AnalysisReport
    ) -> VGBinaryDiffReport {
        let lhs = evidence(from: baseline)
        let rhs = evidence(from: candidate)
        var unmatchedLeft = Set(lhs.indices)
        var unmatchedRight = Set(rhs.indices)
        var pairs: [(Int, Int, String)] = []

        // Stage 1: unique, non-generic symbol names are strong identity evidence.
        let leftNames = uniqueNameIndex(lhs)
        let rightNames = uniqueNameIndex(rhs)
        for name in Set(leftNames.keys).intersection(rightNames.keys).sorted() {
            guard let li = leftNames[name], let ri = rightNames[name] else { continue }
            pairs.append((li, ri, "unique symbol"))
            unmatchedLeft.remove(li)
            unmatchedRight.remove(ri)
        }

        // Stage 2: exact normalized instruction fingerprints survive address/layout churn.
        var rightByFingerprint: [UInt64: [Int]] = [:]
        for ri in unmatchedRight where !rhs[ri].instructionSignatures.isEmpty {
            rightByFingerprint[rhs[ri].codeFingerprint, default: []].append(ri)
        }
        for li in unmatchedLeft.sorted() where !lhs[li].instructionSignatures.isEmpty {
            let fingerprint = lhs[li].codeFingerprint
            guard let candidates = rightByFingerprint[fingerprint], candidates.count == 1, let ri = candidates.first else { continue }
            let competingLeft = unmatchedLeft.filter { lhs[$0].codeFingerprint == fingerprint }
            guard competingLeft.count == 1 else { continue }
            pairs.append((li, ri, "exact normalized code fingerprint"))
            unmatchedLeft.remove(li)
            unmatchedRight.remove(ri)
            rightByFingerprint[fingerprint] = []
        }

        var results = pairs.map { li, ri, reason in diff(lhs[li], rhs[ri], reason: reason) }
        results.append(contentsOf: unmatchedLeft.sorted().map { removed(lhs[$0]) })
        results.append(contentsOf: unmatchedRight.sorted().map { added(rhs[$0]) })
        results.sort { a, b in
            let la = a.candidate?.start ?? a.baseline?.start ?? 0
            let lb = b.candidate?.start ?? b.baseline?.start ?? 0
            if la == lb { return a.displayName < b.displayName }
            return la < lb
        }
        return VGBinaryDiffReport(baselineName: baselineName, candidateName: candidateName, functions: results)
    }

    private static func evidence(from report: AnalysisReport) -> [VGFunctionEvidence] {
        let functions = VGGraphParser.functions(report.functions)
        let rows = VGDisassemblyParser.parse(report.annotated_disassembly)
        let graph = VGGraphParser.cfg(report.cfg)
        let references = VGBinaryWorkspaceParser.references(xrefs: report.xrefs, callGraph: report.call_graph)
        return functions.map { function in
            let instructions = rows.compactMap { row -> String? in
                guard row.kind == .instruction,
                      let address = parseAddressToken(row.address),
                      function.contains(address) else { return nil }
                return normalizeInstruction(row)
            }
            let blocks = graph.blocks.filter { function.contains($0.start) }.map { block in
                let internalEdges = block.edges.filter { function.contains($0.targetAddress) && !$0.isCall }
                let kinds = internalEdges.map(\.kind).sorted().joined(separator: ",")
                return "block:\(block.byteCount):edges:\(internalEdges.count):\(kinds)"
            }.sorted()
            let refs = references.filter { function.contains($0.source) }.map { ref in
                let sourceOffset = ref.source >= function.start ? ref.source - function.start : 0
                let targetIdentity: String
                if function.contains(ref.target) {
                    targetIdentity = "internal+\(ref.target - function.start)"
                } else if let targetFunction = functions.first(where: { $0.start == ref.target }), targetFunction.name != "<discovered>" {
                    targetIdentity = "function:\(targetFunction.name)"
                } else {
                    // Address-only external targets are intentionally not treated as stable identity.
                    targetIdentity = "external-unresolved"
                }
                return "\(sourceOffset):\(ref.kind.lowercased()):\(targetIdentity)"
            }.sorted()
            let lifted = VGBinaryWorkspaceParser.lifted(report.ir, in: function).map { line in
                "\(line.kind):\(normalizeText(line.text)):\(line.confidence.rawValue)"
            }
            return VGFunctionEvidence(
                function: function,
                instructionSignatures: instructions,
                blockSignatures: blocks,
                referenceSignatures: refs,
                liftedSignatures: lifted,
                codeFingerprint: fingerprint(instructions)
            )
        }
    }

    private static func uniqueNameIndex(_ evidence: [VGFunctionEvidence]) -> [String: Int] {
        var buckets: [String: [Int]] = [:]
        for (index, item) in evidence.enumerated() {
            if let name = item.strongName { buckets[name, default: []].append(index) }
        }
        return buckets.compactMapValues { $0.count == 1 ? $0[0] : nil }
    }

    private static func diff(_ lhs: VGFunctionEvidence, _ rhs: VGFunctionEvidence, reason: String) -> VGFunctionDiff {
        let codeChanged = lhs.instructionSignatures != rhs.instructionSignatures
        let cfgChanged = lhs.blockSignatures != rhs.blockSignatures
        let refsChanged = lhs.referenceSignatures != rhs.referenceSignatures
        let semanticChanged = lhs.liftedSignatures != rhs.liftedSignatures
        let status: VGBinaryDiffStatus = (codeChanged || cfgChanged || refsChanged || semanticChanged) ? .changed : .unchanged
        return VGFunctionDiff(
            baseline: lhs.function,
            candidate: rhs.function,
            status: status,
            matchReason: reason,
            codeChanged: codeChanged,
            cfgChanged: cfgChanged,
            referencesChanged: refsChanged,
            semanticsChanged: semanticChanged,
            baselineInstructionCount: lhs.instructionSignatures.count,
            candidateInstructionCount: rhs.instructionSignatures.count,
            baselineBlockCount: lhs.blockSignatures.count,
            candidateBlockCount: rhs.blockSignatures.count,
            baselineReferenceCount: lhs.referenceSignatures.count,
            candidateReferenceCount: rhs.referenceSignatures.count,
            baselineLiftedCount: lhs.liftedSignatures.count,
            candidateLiftedCount: rhs.liftedSignatures.count
        )
    }

    private static func added(_ rhs: VGFunctionEvidence) -> VGFunctionDiff {
        VGFunctionDiff(
            baseline: nil, candidate: rhs.function, status: .added, matchReason: "unmatched candidate function",
            codeChanged: true, cfgChanged: true, referencesChanged: true, semanticsChanged: true,
            baselineInstructionCount: 0, candidateInstructionCount: rhs.instructionSignatures.count,
            baselineBlockCount: 0, candidateBlockCount: rhs.blockSignatures.count,
            baselineReferenceCount: 0, candidateReferenceCount: rhs.referenceSignatures.count,
            baselineLiftedCount: 0, candidateLiftedCount: rhs.liftedSignatures.count
        )
    }

    private static func removed(_ lhs: VGFunctionEvidence) -> VGFunctionDiff {
        VGFunctionDiff(
            baseline: lhs.function, candidate: nil, status: .removed, matchReason: "unmatched baseline function",
            codeChanged: true, cfgChanged: true, referencesChanged: true, semanticsChanged: true,
            baselineInstructionCount: lhs.instructionSignatures.count, candidateInstructionCount: 0,
            baselineBlockCount: lhs.blockSignatures.count, candidateBlockCount: 0,
            baselineReferenceCount: lhs.referenceSignatures.count, candidateReferenceCount: 0,
            baselineLiftedCount: lhs.liftedSignatures.count, candidateLiftedCount: 0
        )
    }

    private static func parseAddressToken(_ text: String) -> UInt64? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let raw = trimmed.lowercased().hasPrefix("0x") ? String(trimmed.dropFirst(2)) : trimmed
        guard !raw.isEmpty, raw.count <= 16, raw.allSatisfy({ $0.isHexDigit }) else { return nil }
        return UInt64(raw, radix: 16)
    }

    private static func normalizeInstruction(_ row: VGDisassemblyRow) -> String {
        let mnemonic = row.mnemonic.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        let operands = normalizeText(row.operands.lowercased())
        return operands.isEmpty ? mnemonic : "\(mnemonic) \(operands)"
    }

    private static func normalizeText(_ input: String) -> String {
        // Normalize absolute addresses/immediates so image slides and layout churn do not
        // automatically become semantic differences. Register names and structure remain.
        let pattern = #"(?i)0x[0-9a-f]{4,16}"#
        let regex = try? NSRegularExpression(pattern: pattern)
        let range = NSRange(input.startIndex..<input.endIndex, in: input)
        let replaced = regex?.stringByReplacingMatches(in: input, range: range, withTemplate: "<addr>") ?? input
        return replaced.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    }

    private static func fingerprint(_ lines: [String]) -> UInt64 {
        // Stable FNV-1a; this is an identity aid, not a cryptographic claim.
        var hash: UInt64 = 0xcbf29ce484222325
        for byte in lines.joined(separator: "\n").utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x100000001b3
        }
        return hash
    }
}
