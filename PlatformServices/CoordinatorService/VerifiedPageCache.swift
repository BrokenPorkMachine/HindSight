import Foundation
/// Queue confined. Reuse is based on metadata continuity, not a fresh content hash.
final class VerifiedPageCache {
    private let handle: OpaquePointer?
    init() { handle = reworkbench_cache_new(1024 * 1024, 16) }
    deinit { if let handle { reworkbench_cache_free(handle) } }
    func clear() { if let handle { reworkbench_cache_clear(handle) } }
    static func stamp(_ path: String) throws -> String {
        var output = [CChar](repeating: 0, count: 512)
        let result = path.withCString { pointer in
            output.withUnsafeMutableBufferPointer { reworkbench_file_stamp(pointer, $0.baseAddress, $0.count) }
        }
        guard result == 0 else { throw NSError(domain: "VectorGlass", code: 3, userInfo: [NSLocalizedDescriptionKey: "Captured file metadata is unavailable or the object is not a regular file."]) }
        return String(cString: output)
    }
    static func key(path: String, digest: String, size: String, start: UInt64, stamp: String, configuration: String = "") throws -> Data {
        try JSONEncoder().encode(["page-cache-v2-a64-integer", path, digest, size, String(start), stamp] + (configuration.isEmpty ? [] : [configuration]))
    }
    func get(_ key: Data) -> InstructionPage? {
        guard let handle else { return nil }
        let data: Data? = key.withUnsafeBytes { bytes in
            var count = 0
            guard let pointer = reworkbench_cache_get(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, &count), count <= 131072 else { return nil }
            return Data(bytes: pointer, count: count)
        }
        guard let data else { return nil }
        return try? JSONDecoder().decode(InstructionPage.self, from: data)
    }
    func put(_ page: InstructionPage, key: Data) {
        guard let handle, let data = try? JSONEncoder().encode(page) else { return }
        _ = key.withUnsafeBytes { keyBytes in
            data.withUnsafeBytes { valueBytes in
                reworkbench_cache_put(handle, keyBytes.bindMemory(to: UInt8.self).baseAddress, keyBytes.count,
                                      valueBytes.bindMemory(to: UInt8.self).baseAddress, valueBytes.count)
            }
        }
    }
}
