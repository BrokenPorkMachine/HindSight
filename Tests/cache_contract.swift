import Foundation
@main struct CacheContractSmoke {
    static func main() throws {
        let cache = VerifiedPageCache()
        let digest = String(repeating: "a", count: 64)
        let firstID = UUID().uuidString; let nextID = UUID().uuidString
        let page = InstructionPage(protocol: 1, kind: "page_result", request_id: firstID,
                                   source_sha256: digest, start_word: "0", total_words: "10",
                                   section_address: "4096", section_offset: "128", word_count: 10, text: "test")
        let key = try VerifiedPageCache.key(path: "/object", digest: digest, size: "168", start: 0, stamp: "stamp1")
        precondition(cache.get(key) == nil)
        cache.put(page, key: key)
        guard let cached = cache.get(key) else { fatalError("Cache miss after put") }
        precondition(cached.forRequest(nextID).matches(id: nextID, digest: digest, start: 0))
        precondition(!cached.matches(id: nextID, digest: digest, start: 0))
        let changed = try VerifiedPageCache.key(path: "/object", digest: digest, size: "168", start: 0, stamp: "stamp2")
        precondition(cache.get(changed) == nil)
        cache.clear(); precondition(cache.get(key) == nil)
        print("Swift/C cache adapter, request rebinding and metadata-key isolation passed")
    }
}
