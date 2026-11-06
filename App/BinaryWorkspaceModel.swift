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

struct VGECUChecksumDescriptor: Identifiable, Hashable {
    enum Verdict: String { case verified = "Verified", invalid = "Invalid" }
    let index: Int
    let start: UInt32
    let end: UInt32
    let stored: UInt32
    let complement: UInt32
    let computed: UInt32
    let verdict: Verdict
    let repairable: Bool
    var id: Int { index }
    var descriptorOffset: UInt64 { 0x1F810 + UInt64(index * 16) }
}

enum VGECUChecksums {
    static let tableOffset = 0x1F810
    static let maximumDescriptors = 31

    static func descriptors(in data: Data) -> [VGECUChecksumDescriptor] {
        guard data.count >= tableOffset + 16 else { return [] }
        func word(_ offset: Int) -> UInt32 {
            UInt32(data[offset]) | UInt32(data[offset + 1]) << 8 |
                UInt32(data[offset + 2]) << 16 | UInt32(data[offset + 3]) << 24
        }
        var result: [VGECUChecksumDescriptor] = []
        var summedBytes = 0
        for index in 0..<maximumDescriptors {
            let offset = tableOffset + index * 16
            guard offset + 16 <= data.count else { break }
            let start = word(offset), end = word(offset + 4)
            let stored = word(offset + 8), complement = word(offset + 12)
            if start == 0 && end == 0 && stored == 0 && complement == 0 { break }
            let span = UInt64(end) >= UInt64(start) ? UInt64(end) - UInt64(start) + 1 : 0
            guard span > 0, span.isMultiple(of: 2), UInt64(end) < UInt64(data.count),
                  span <= UInt64(4 * 1024 * 1024 - summedBytes) else {
                result.append(.init(index: index, start: start, end: end, stored: stored,
                                    complement: complement, computed: 0, verdict: .invalid, repairable: false))
                continue
            }
            var sum: UInt32 = 0
            var cursor = Int(start)
            while cursor < Int(end) {
                sum &+= UInt32(data[cursor]) | UInt32(data[cursor + 1]) << 8
                cursor += 2
            }
            summedBytes += Int(span)
            let valid = complement == ~stored && stored == sum
            result.append(.init(index: index, start: start, end: end, stored: stored,
                                complement: complement, computed: sum,
                                verdict: valid ? .verified : .invalid, repairable: true))
        }
        return result
    }

    static func repairEdits(in data: Data, descriptors: [VGECUChecksumDescriptor]) -> [VGBinaryPatch.Edit] {
        descriptors.filter { $0.verdict == .invalid && $0.repairable }.map { descriptor in
            let offset = Int(descriptor.descriptorOffset + 8)
            let expected = Array(data[offset..<(offset + 8)])
            let replacement = littleEndian(descriptor.computed) + littleEndian(~descriptor.computed)
            return VGBinaryPatch.Edit(offset: descriptor.descriptorOffset + 8, expected: expected, replacement: replacement)
        }
    }

    private static func littleEndian(_ value: UInt32) -> [UInt8] {
        [UInt8(value & 0xff), UInt8((value >> 8) & 0xff),
         UInt8((value >> 16) & 0xff), UInt8((value >> 24) & 0xff)]
    }
}

struct VGXDFDefinition: Identifiable, Hashable {
    struct Anchor: Hashable {
        let kind: String
        let address: UInt64
    }
    let id: String
    let title: String
    let address: UInt64
    let anchors: [Anchor]
}

struct VGXDFRelocation: Identifiable, Hashable {
    enum State: String { case verified, candidate, ambiguous, notFound = "not-found" }
    let definition: VGXDFDefinition
    let resolvedAddress: UInt64?
    let state: State
    let score: Double
    let matchedAnchors: Int
    let totalAnchors: Int
    let delta: Int64?
    var id: String { definition.id }
}

enum VGXDFDefinitions {
    static func parse(_ data: Data) throws -> [VGXDFDefinition] {
        let delegate = ParserDelegate()
        let parser = XMLParser(data: data)
        parser.delegate = delegate
        parser.shouldResolveExternalEntities = false
        guard parser.parse() else {
            throw parser.parserError ?? NSError(domain: "XDF", code: 1,
                userInfo: [NSLocalizedDescriptionKey: "The XDF document is not well-formed XML."])
        }
        return delegate.definitions.prefix(20_000).map { $0 }
    }

    static func evidence(for definition: VGXDFDefinition, firmware: Data) -> String {
        guard definition.address < UInt64(firmware.count) else { return "not-found (address outside image)" }
        let start = Int(definition.address)
        let end = min(firmware.count, start + 32)
        let sample = firmware[start..<end]
        guard sample.count >= 4 else { return "candidate (insufficient bytes for anchor)" }
        let distinct = Set(sample).count
        if distinct <= 1 { return "candidate@0x\(String(definition.address, radix: 16)) (uniform anchor; unverified)" }
        return "candidate@0x\(String(definition.address, radix: 16)) (bounded data present; anchor unverified)"
    }

    static func relocate(_ definitions: [VGXDFDefinition], reference: Data, target: Data) -> [VGXDFRelocation] {
        definitions.prefix(4_000).map { definition in
            let usable = definition.anchors.prefix(4).compactMap { anchor -> (VGXDFDefinition.Anchor, Data)? in
                guard anchor.address < UInt64(reference.count) else { return nil }
                let start = Int(anchor.address), length = min(24, reference.count - start)
                guard length >= 12 else { return nil }
                let signature = reference.subdata(in: start..<(start + length))
                guard Set(signature).count >= 4 else { return nil }
                return (anchor, signature)
            }
            guard !usable.isEmpty else {
                return .init(definition: definition, resolvedAddress: nil, state: .notFound,
                             score: 0, matchedAnchors: 0, totalAnchors: 0, delta: nil)
            }
            var support: [Int64: Set<String>] = [:]
            for (anchor, signature) in usable {
                var searchStart = target.startIndex
                var matches = 0
                while searchStart < target.endIndex, matches < 8,
                      let range = target.range(of: signature, options: [], in: searchStart..<target.endIndex) {
                    let delta = Int64(range.lowerBound) - Int64(anchor.address)
                    support[delta, default: []].insert("\(anchor.kind):\(anchor.address)")
                    matches += 1
                    searchStart = range.lowerBound + 1
                }
            }
            var ranked: [(delta: Int64, count: Int)] = support.map { entry in
                (delta: entry.key, count: entry.value.count)
            }
            ranked.sort { lhs, rhs in
                if lhs.count != rhs.count { return lhs.count > rhs.count }
                return lhs.delta.magnitude < rhs.delta.magnitude
            }
            guard let best = ranked.first else {
                return .init(definition: definition, resolvedAddress: nil, state: .notFound,
                             score: 0, matchedAnchors: 0, totalAnchors: usable.count, delta: nil)
            }
            let tied = ranked.dropFirst().first?.count == best.count
            guard definition.address <= UInt64(Int64.max) else {
                return .init(definition: definition, resolvedAddress: nil, state: .notFound,
                             score: 0, matchedAnchors: best.count, totalAnchors: usable.count, delta: nil)
            }
            let addition = Int64(definition.address).addingReportingOverflow(best.delta)
            guard !addition.overflow else {
                return .init(definition: definition, resolvedAddress: nil, state: .notFound,
                             score: 0, matchedAnchors: best.count, totalAnchors: usable.count, delta: nil)
            }
            let resolvedSigned = addition.partialValue
            let resolved = resolvedSigned >= 0 && UInt64(resolvedSigned) < UInt64(target.count) ? UInt64(resolvedSigned) : nil
            let score = Double(best.count) / Double(usable.count)
            let state: VGXDFRelocation.State = tied ? .ambiguous : (best.count >= 2 && score >= 0.66 ? .verified : .candidate)
            return .init(definition: definition, resolvedAddress: resolved, state: resolved == nil ? .notFound : state,
                         score: score, matchedAnchors: best.count, totalAnchors: usable.count, delta: best.delta)
        }
    }

    private final class ParserDelegate: NSObject, XMLParserDelegate {
        var definitions: [VGXDFDefinition] = []
        private var tableID = ""
        private var title = ""
        private var address: UInt64?
        private var anchors: [VGXDFDefinition.Anchor] = []
        private var axisKind = "table"
        private var titleText = ""
        private var inTitle = false

        func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?,
                    qualifiedName qName: String?, attributes attributeDict: [String: String] = [:]) {
            let name = elementName.uppercased()
            if name == "XDFTABLE" {
                tableID = attributeDict["uniqueid"] ?? attributeDict["id"] ?? UUID().uuidString
                title = "Untitled table"; address = nil; anchors = []; axisKind = "table"
            } else if !tableID.isEmpty && name == "TITLE" {
                inTitle = true; titleText = ""
            } else if !tableID.isEmpty && name == "XDFAXIS" {
                axisKind = (attributeDict["id"] ?? "axis").lowercased()
            } else if !tableID.isEmpty && name == "EMBEDDEDDATA" {
                let raw = attributeDict["mmedaddress"] ?? attributeDict["address"]
                if let parsed = raw.flatMap(Self.number) {
                    anchors.append(.init(kind: axisKind, address: parsed))
                    if axisKind == "z" || address == nil { address = parsed }
                }
            }
        }

        func parser(_ parser: XMLParser, foundCharacters string: String) {
            if inTitle { titleText += string }
        }

        func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
            let name = elementName.uppercased()
            if name == "TITLE", inTitle {
                let trimmed = titleText.trimmingCharacters(in: .whitespacesAndNewlines)
                if !trimmed.isEmpty { title = trimmed }
                inTitle = false
            } else if name == "XDFTABLE", !tableID.isEmpty {
                if let address { definitions.append(.init(id: tableID, title: title, address: address, anchors: anchors)) }
                tableID = ""; address = nil; anchors = []
            } else if name == "XDFAXIS" {
                axisKind = "table"
            }
        }

        private static func number(_ value: String) -> UInt64? {
            let text = value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            return text.hasPrefix("0x") ? UInt64(text.dropFirst(2), radix: 16) : UInt64(text)
        }
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
