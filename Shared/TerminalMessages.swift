import Foundation

struct TerminalReply: Codable {
    let ok: Bool
    let message: String
    var sessionID: String? = nil
    var running: Bool = false
    var output: String? = nil

    static func encode(_ value: Self) -> String {
        guard let data = try? JSONEncoder().encode(value) else {
            return "{\"ok\":false,\"message\":\"Terminal reply encoding failed\"}"
        }
        return String(decoding: data, as: UTF8.self)
    }
}

enum TerminalDisplay {
    final class Stream {
        private enum State { case normal, escape, csi, osc, oscEscape }
        private var state = State.normal

        func append(_ text: String) -> String {
            var result = String()
            for scalar in text.unicodeScalars {
                let value = scalar.value
                switch state {
                case .normal:
                    if value == 0x1B { state = .escape }
                    else if value == 0x09 || value == 0x0A || value == 0x0D || value >= 0x20 { result.unicodeScalars.append(scalar) }
                case .escape:
                    if value == 0x5B { state = .csi }
                    else if value == 0x5D { state = .osc }
                    else { state = .normal }
                case .csi:
                    if (0x40...0x7E).contains(value) { state = .normal }
                case .osc:
                    if value == 0x07 { state = .normal }
                    else if value == 0x1B { state = .oscEscape }
                case .oscEscape:
                    state = value == 0x5C ? .normal : .osc
                }
            }
            return result
        }

        func reset() { state = .normal }
    }

    /// Keeps terminal output readable in a plain SwiftUI text surface while
    /// retaining the raw stream in the model for future renderer upgrades.
    static func sanitized(_ text: String) -> String {
        Stream().append(text)
    }
}
