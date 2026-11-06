import Foundation

@main
enum FunctionNameExchangeContract {
    static func document(_ rows: [[String: String]], hash: String? = nil) -> Data {
        let value: [String: Any] = [
            "schema": "hindsight.functions.v1", "address_space": "image-relative",
            "source_sha256": hash ?? String(repeating: "a", count: 64),
            "source_image_base": "0x1000", "producer": "Ghidra", "functions": rows
        ]
        return try! JSONSerialization.data(withJSONObject: value)
    }

    static func main() throws {
        let row = ["offset": "0x20", "name": "recovered_name", "provenance": "Ghidra function name; USER_DEFINED"]
        let digest = String(repeating: "a", count: 64)
        let decoded = try JSONDecoder().decode(FunctionNameExchange.self, from: document([row]))
        guard let resolved = decoded.resolved(sourceSHA256: digest, knownStarts: [0x1020]) else {
            throw NSError(domain: "FunctionNameExchange", code: 1)
        }
        precondition(resolved.matches.count == 1 && resolved.matches[0].0 == 0x1020)
        precondition(resolved.matches[0].1 == "recovered_name" && resolved.unmatched == 0)
        let mismatch = try JSONDecoder().decode(FunctionNameExchange.self, from: document([row]))
        precondition(mismatch.resolved(sourceSHA256: String(repeating: "b", count: 64), knownStarts: [0x1020]) == nil)
        let duplicate = try JSONDecoder().decode(FunctionNameExchange.self, from: document([row, row]))
        precondition(duplicate.resolved(sourceSHA256: digest, knownStarts: [0x1020]) == nil)
        let malformed = ["offset": "../0x20", "name": "bad", "provenance": "test"]
        let invalid = try JSONDecoder().decode(FunctionNameExchange.self, from: document([malformed]))
        precondition(invalid.resolved(sourceSHA256: digest, knownStarts: [0x1020]) == nil)
        let unmatched = try JSONDecoder().decode(FunctionNameExchange.self, from: document([row]))
        precondition(unmatched.resolved(sourceSHA256: digest, knownStarts: [0x1030])?.unmatched == 1)
    }
}
