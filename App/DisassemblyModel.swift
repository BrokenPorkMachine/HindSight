import Foundation

struct VGDisassemblyRow: Identifiable, Hashable {
    enum Kind: Hashable {
        case instruction
        case symbol
        case metadata
    }

    let id: Int
    let kind: Kind
    let address: String
    let fileOffset: String
    let word: String
    let mnemonic: String
    let operands: String
    let annotation: String
    let raw: String

    var normalizedAddress: String {
        address.lowercased().replacingOccurrences(of: "0x", with: "")
    }

    var isControlFlow: Bool {
        let op = mnemonic.lowercased()
        return op == "b" || op == "bl" || op == "br" || op == "blr" || op == "ret"
            || op.hasPrefix("b.") || op.hasPrefix("cb") || op.hasPrefix("tb")
            || op.hasPrefix("j") || op.hasPrefix("call") || op.hasPrefix("ret") || op == "bx" || op == "blx" || op.hasPrefix("loop")
    }

    var isCall: Bool {
        let op = mnemonic.lowercased()
        return op == "bl" || op == "blr" || op == "blx" || op.hasPrefix("call")
    }
}

enum VGDisassemblyParser {
    private static func isHex(_ token: String, width: ClosedRange<Int>? = nil) -> Bool {
        var value = token.lowercased()
        if value.hasPrefix("0x") { value.removeFirst(2) }
        value = value.trimmingCharacters(in: CharacterSet(charactersIn: ":"))
        guard !value.isEmpty, value.allSatisfy({ $0.isHexDigit }) else { return false }
        guard let width else { return true }
        return width.contains(value.count)
    }

    private static func splitInstruction(_ source: String) -> (String, String, String) {
        let pieces = source.components(separatedBy: ";")
        let code = pieces.first?.trimmingCharacters(in: .whitespaces) ?? ""
        let annotation = pieces.dropFirst().joined(separator: ";").trimmingCharacters(in: .whitespaces)
        let fields = code.split(maxSplits: 1, whereSeparator: { $0.isWhitespace })
        let mnemonic = fields.first.map(String.init) ?? ""
        let operands = fields.count > 1 ? String(fields[1]) : ""
        return (mnemonic, operands, annotation)
    }

    static func parse(_ text: String) -> [VGDisassemblyRow] {
        var rows: [VGDisassemblyRow] = []
        for (index, rawSubstring) in text.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
            let raw = String(rawSubstring)
            let trimmed = raw.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }

            let fields = trimmed.split(whereSeparator: { $0.isWhitespace }).map(String.init)
            if fields.count >= 2, isHex(fields[0]), fields[1].hasPrefix("<"), trimmed.hasSuffix(":") {
                rows.append(.init(id: index, kind: .symbol, address: fields[0].trimmingCharacters(in: CharacterSet(charactersIn: ":")), fileOffset: "", word: "", mnemonic: fields.dropFirst().joined(separator: " "), operands: "", annotation: "", raw: raw))
                continue
            }

            if fields.count >= 4, isHex(fields[0], width: 16...18), isHex(fields[1], width: 16...18), isHex(fields[2], width: 2...30), fields[2].count % 2 == 0 {
                let instruction = fields.dropFirst(3).joined(separator: " ")
                let decoded = splitInstruction(instruction)
                rows.append(.init(id: index, kind: .instruction, address: fields[0], fileOffset: fields[1], word: fields[2], mnemonic: decoded.0, operands: decoded.1, annotation: decoded.2, raw: raw))
                continue
            }

            if fields.count >= 3, isHex(fields[0]), isHex(fields[1], width: 8...8) {
                let instruction = fields.dropFirst(2).joined(separator: " ")
                let decoded = splitInstruction(instruction)
                rows.append(.init(id: index, kind: .instruction, address: fields[0].trimmingCharacters(in: CharacterSet(charactersIn: ":")), fileOffset: "", word: fields[1], mnemonic: decoded.0, operands: decoded.1, annotation: decoded.2, raw: raw))
                continue
            }

            rows.append(.init(id: index, kind: .metadata, address: "", fileOffset: "", word: "", mnemonic: "", operands: "", annotation: "", raw: raw))
        }
        return rows
    }
}

