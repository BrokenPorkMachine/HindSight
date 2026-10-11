import Foundation
@main struct PageContractSmoke {
    static func main() throws {
        let id = UUID().uuidString
        let digest = String(repeating: "a", count: 64)
        let valid: [String: Any] = ["protocol": 1, "kind": "page_result", "request_id": id,
            "source_sha256": digest, "start_word": "256", "total_words": "600",
            "section_address": "4294967296", "section_offset": "4096", "word_count": 256, "text": "instructions"]
        func decode(_ object: [String: Any]) throws -> InstructionPage {
            try JSONDecoder().decode(InstructionPage.self, from: JSONSerialization.data(withJSONObject: object))
        }
        let page = try decode(valid)
        precondition(page.matches(id: id, digest: digest, start: 256))
        precondition(!page.matches(id: UUID().uuidString, digest: digest, start: 256))
        precondition(!page.matches(id: id, digest: String(repeating: "b", count: 64), start: 256))
        precondition(!page.matches(id: id, digest: digest, start: 0))
        for (key, value) in [("total_words", "256"), ("section_address", "18446744073709551612"), ("section_offset", "1"), ("text", String(repeating: "a", count: 65536))] {
            var changed = valid; changed[key] = value
            let decoded = try decode(changed)
            precondition(!decoded.matches(id: id, digest: digest, start: 256))
        }
        var last = valid; last["start_word"] = "512"; last["word_count"] = 88
        let final = try decode(last)
        precondition(final.matches(id: id, digest: digest, start: 512))
        var variable = valid; variable["unit_bytes"] = 1; variable["word_count"] = 255; variable["section_offset"] = "129"
        let variablePage = try decode(variable)
        precondition(variablePage.matches(id: id, digest: digest, start: 256))
        precondition(variablePage.forRequest(id).unit_bytes == 1)
        for unit in [-1, 0, 3, 8] {
            variable["unit_bytes"] = unit
            let changed = try decode(variable)
            precondition(!changed.matches(id: id, digest: digest, start: 256))
        }
        var invalidCount = valid; invalidCount["word_count"] = 255
        let changedCount = try decode(invalidCount)
        precondition(!changedCount.matches(id: id, digest: digest, start: 256))
        let input = BinaryImportOptions(format: "sepos", architecture: "thumb", load_address: "0x1000", file_offset: "0x94000")
        precondition(input.validated?.arguments == ["sepos", "thumb", "4096", "606208"])
        precondition(input.validated?.cacheIdentity != BinaryImportOptions.automatic.cacheIdentity)
        precondition(BinaryImportOptions(format: "bad").validated == nil)
        print("Swift page identity, extent, frame-content and final-page checks passed")
    }
}
