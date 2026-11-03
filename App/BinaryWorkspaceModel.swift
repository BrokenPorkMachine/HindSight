import Foundation

enum VGBinaryPatch {
    struct Edit: Identifiable {
        let id = UUID()
        let offset: UInt64
        let expected: [UInt8]
        let replacement: [UInt8]
    }
    static func apply(_ data: Data, edits: [Edit]) throws -> Data {
        let sorted = edits.sorted { $0.offset < $1.offset }
        var end: UInt64 = 0
        for edit in sorted {
            guard edit.offset >= end else { throw PatchError.overlap }
            _ = try apply(data, offset: edit.offset, expected: edit.expected, replacement: edit.replacement)
            end = edit.offset + UInt64(edit.expected.count)
        }
        var output = data
        for edit in sorted {
            output.replaceSubrange(Int(edit.offset)..<Int(edit.offset + UInt64(edit.expected.count)), with: edit.replacement)
        }
        return output
    }
    enum PatchError: Error, LocalizedError {
        case invalidHex, lengthMismatch, outOfBounds, originalMismatch, overlap
        var errorDescription: String? {
            switch self {
            case .overlap: return "Staged patches overlap. Remove the conflicting edit first."
            case .invalidHex: return "Enter complete hexadecimal byte pairs."
            case .lengthMismatch: return "Original and replacement bytes must have the same nonzero length."
            case .outOfBounds: return "The patch extends beyond the file."
            case .originalMismatch: return "The original bytes do not match the file at this offset."
            }
        }
    }
    static func bytes(_ text: String) throws -> [UInt8] {
        let compact = text.filter { !$0.isWhitespace }
        guard !compact.isEmpty, compact.count.isMultiple(of: 2), compact.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }) else { throw PatchError.invalidHex }
        var result: [UInt8] = []
        var index = compact.startIndex
        while index < compact.endIndex {
            let end = compact.index(index, offsetBy: 2)
            guard let byte = UInt8(compact[index..<end], radix: 16) else { throw PatchError.invalidHex }
            result.append(byte)
            index = end
        }
        return result
    }
    static func apply(_ data: Data, offset: UInt64, expected: [UInt8], replacement: [UInt8]) throws -> Data {
        guard !expected.isEmpty, expected.count == replacement.count else { throw PatchError.lengthMismatch }
        guard offset <= UInt64(data.count), UInt64(expected.count) <= UInt64(data.count) - offset else { throw PatchError.outOfBounds }
        let start = Int(offset)
        guard Array(data[start..<(start + expected.count)]) == expected else { throw PatchError.originalMismatch }
        var output = data
        output.replaceSubrange(start..<(start + expected.count), with: replacement)
        return output
    }
}

struct VGBinaryReference: Identifiable, Hashable {
    let source: UInt64
    let target: UInt64
    let kind: String
    let detail: String

    var id: String { "\(source):\(kind):\(target)" }
    var sourceText: String { String(format: "0x%016llx", source) }
    var targetText: String { String(format: "0x%016llx", target) }
    var isCall: Bool { kind.lowercased().contains("call") || kind.lowercased() == "bl" }
}

struct VGLiftedLine: Identifiable, Hashable {
    enum Confidence: String, Hashable {
        case decoded = "decoded"
        case modeled = "modeled"
        case opaque = "opaque"
    }

    let address: UInt64
    let kind: String
    let source: String
    let text: String
    let confidence: Confidence

    var id: String { "\(address):\(kind):\(source)" }
    var addressText: String { String(format: "0x%016llx", address) }
}

struct VGBinaryCFGLine: Identifiable, Hashable {
    let address: UInt64?
    let text: String
    var id: String { "\(address.map { String($0) } ?? "-"):\(text)" }
}

struct VGInstructionData: Hashable {
    let address: UInt64
    let word: UInt32
    let bytes: [UInt8]
    let mnemonic: String
    let operands: String
    let annotation: String
    let fileOffset: String

    var addressText: String { String(format: "0x%016llx", address) }
    var wordText: String { String(format: "0x%08x", word) }
    var bytesText: String { bytes.map { String(format: "%02x", $0) }.joined(separator: " ") }
    var unsignedText: String { String(word) }
    var signedText: String { String(Int32(bitPattern: word)) }
}

enum VGBinaryWorkspaceParser {
    static func reconstructedPseudocode(_ semantic: String?) -> String? {
        let marker = "Reconstructed pseudocode\n========================\n"
        guard let semantic, let range = semantic.range(of: marker) else { return nil }
        let result = String(semantic[range.upperBound...])
        return result.isEmpty ? nil : result
    }

    static func signatureEvidence(_ callValues: String) -> String? {
        guard let range = callValues.range(of: "Signature evidence:") else { return nil }
        return String(callValues[range.lowerBound...])
    }

    static func pseudocode(_ semantic: String?, in function: VGFunctionRecord?) -> String? {
        guard let function, let text = reconstructedPseudocode(semantic) else { return nil }
        var result: [String] = []
        var matching = false
        for raw in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            if line.hasPrefix("// 0x") {
                let address = line.dropFirst(3).split(separator: " ").first.flatMap { self.address(String($0)) }
                if matching { break }
                matching = address == function.start
            }
            if matching { result.append(line) }
        }
        return result.isEmpty ? nil : result.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func address(_ text: String) -> UInt64? {
        VGGraphParser.address(from: text)
    }

    /// Returns the instruction address only when the renderer placed it at the
    /// start of an address-bearing statement. Other addresses on the line may
    /// be call targets, constants, or memory locations and are not source maps.
    static func pseudocodeSourceAddress(_ line: String) -> UInt64? {
        let trimmed = line.drop(while: { $0.isWhitespace })
        guard trimmed.hasPrefix("0x") else { return nil }
        let token = trimmed.prefix(while: { !$0.isWhitespace })
        return address(String(token))
    }

    static func references(xrefs: String, callGraph: String) -> [VGBinaryReference] {
        var result: [VGBinaryReference] = []
        var seen = Set<String>()

        for line in xrefs.split(separator: "\n", omittingEmptySubsequences: true).map(String.init) {
            let lower = line.lowercased()
            guard let arrow = lower.range(of: "--"), let toArrow = lower.range(of: "-->"),
                  let source = address(String(lower[..<arrow.lowerBound])),
                  let target = address(String(lower[toArrow.upperBound...])) else { continue }
            let kindStart = arrow.upperBound
            let kindEnd = toArrow.lowerBound
            let kind = String(lower[kindStart..<kindEnd]).trimmingCharacters(in: CharacterSet(charactersIn: "- "))
            let ref = VGBinaryReference(source: source, target: target, kind: kind.isEmpty ? "xref" : kind, detail: line)
            if seen.insert(ref.id).inserted { result.append(ref) }
        }

        for line in callGraph.split(separator: "\n", omittingEmptySubsequences: true).map(String.init) {
            let addresses = allAddresses(in: line)
            guard addresses.count >= 3 else { continue }
            let ref = VGBinaryReference(source: addresses[1], target: addresses[2], kind: "call", detail: line)
            if seen.insert(ref.id).inserted { result.append(ref) }
        }

        return result.sorted {
            if $0.source == $1.source { return $0.target < $1.target }
            return $0.source < $1.source
        }
    }

    static func allAddresses(in text: String) -> [UInt64] {
        let expression = try? NSRegularExpression(pattern: #"(?i)0x[0-9a-f]{1,16}"#)
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return (expression?.matches(in: text, range: range) ?? []).compactMap { match in
            guard let swiftRange = Range(match.range, in: text) else { return nil }
            return address(String(text[swiftRange]))
        }
    }

    static func irLines(_ text: String, in function: VGFunctionRecord?) -> [(UInt64, String, String)] {
        text.split(separator: "\n", omittingEmptySubsequences: true).compactMap { raw in
            let line = String(raw)
            let fields = line.split(maxSplits: 2, whereSeparator: { $0.isWhitespace })
            guard fields.count >= 3, let pc = address(String(fields[0])) else { return nil }
            if let function, !function.contains(pc) { return nil }
            return (pc, String(fields[1]).trimmingCharacters(in: .whitespaces), String(fields[2]).trimmingCharacters(in: .whitespaces))
        }
    }

    static func cfgLines(_ text: String, in function: VGFunctionRecord?) -> [VGBinaryCFGLine] {
        var result: [VGBinaryCFGLine] = []
        var currentBlockMatches = false
        for raw in text.split(separator: "\n", omittingEmptySubsequences: true) {
            let line = String(raw)
            if line.hasPrefix("block B") {
                let blockStart = allAddresses(in: line).first
                currentBlockMatches = blockStart.map { function?.contains($0) ?? true } ?? false
                if currentBlockMatches { result.append(VGBinaryCFGLine(address: blockStart, text: line)) }
            } else if currentBlockMatches {
                let address = allAddresses(in: line).first
                result.append(VGBinaryCFGLine(address: address, text: line))
            }
        }
        return result
    }

    static func lifted(_ text: String, in function: VGFunctionRecord?) -> [VGLiftedLine] {
        irLines(text, in: function).map { pc, kind, asm in
            let normalizedKind = kind.lowercased()
            let lowerAsm = asm.lowercased()
            let statement: String
            let confidence: VGLiftedLine.Confidence

            switch normalizedKind {
            case "term":
                statement = "return  /* \(asm) */"
                confidence = .modeled
            case "branch":
                if lowerAsm.hasPrefix("bl ") {
                    statement = "call \(asm.dropFirst(3))"
                } else if lowerAsm.hasPrefix("b ") {
                    statement = "goto \(asm.dropFirst(2))"
                } else {
                    statement = "branch  /* \(asm) */"
                }
                confidence = .modeled
            case "cbranch":
                statement = "if (condition from \(asm.split(separator: " ").first ?? "branch")) goto target  /* \(asm) */"
                confidence = .modeled
            case "opaque":
                statement = "/* opaque instruction barrier: \(asm) */"
                confidence = .opaque
            default:
                statement = "execute  /* \(asm) */"
                confidence = .decoded
            }
            return VGLiftedLine(address: pc, kind: normalizedKind, source: asm, text: statement, confidence: confidence)
        }
    }

    static func instructionData(rows: [VGDisassemblyRow], address: UInt64?) -> VGInstructionData? {
        guard let address else { return nil }
        guard let row = rows.first(where: { item in
            guard item.kind == .instruction, let itemAddress = self.address(item.address) else { return false }
            return itemAddress == address
        }) else { return nil }
        let wordText = row.word.lowercased().replacingOccurrences(of: "0x", with: "")
        guard wordText.count <= 8, let word = UInt32(wordText, radix: 16) else { return nil }
        return VGInstructionData(
            address: address,
            word: word,
            bytes: [
                UInt8(truncatingIfNeeded: word),
                UInt8(truncatingIfNeeded: word >> 8),
                UInt8(truncatingIfNeeded: word >> 16),
                UInt8(truncatingIfNeeded: word >> 24)
            ],
            mnemonic: row.mnemonic,
            operands: row.operands,
            annotation: row.annotation,
            fileOffset: row.fileOffset
        )
    }

    static func dataflowLines(_ text: String, address: UInt64?, function: VGFunctionRecord?) -> [String] {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init)
        if let address {
            let exact = lines.filter { self.address($0) == address }
            if !exact.isEmpty { return exact }
        }
        if let function {
            return lines.filter { line in
                guard let pc = self.address(line) else { return false }
                return function.contains(pc)
            }
        }
        return []
    }
}
