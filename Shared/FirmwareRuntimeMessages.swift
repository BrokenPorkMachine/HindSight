import Foundation

struct DyldCacheImageRecord: Codable, Identifiable, Hashable {
    var id: String { "\(loadAddress)|\(path)" }
    let path: String
    let loadAddress: String
    let textSize: UInt32
    let cacheFile: String
}


struct DyldCacheMappingRecord: Codable, Identifiable, Hashable {
    var id: String { "\(address)|\(fileOffset)" }
    let address: String
    let size: UInt64
    let fileOffset: UInt64
    let maxProtection: UInt32
    let initialProtection: UInt32
}

struct DyldCacheSummary: Codable {
    let ok: Bool
    let message: String
    let path: String
    let magic: String
    let mappingCount: Int
    let imageCount: Int
    let mappings: [DyldCacheMappingRecord]
    let images: [DyldCacheImageRecord]
}

struct FirmwareDeviceRecord: Codable, Identifiable, Hashable {
    var id: String { identifier }
    let name: String
    let identifier: String
    let boardconfig: String?
    let platform: String?
    let apple_cpu: String?
}

struct FirmwareReleaseRecord: Codable, Identifiable, Hashable {
    var id: String { "\(identifier)-\(buildid)" }
    let identifier: String
    let version: String
    let buildid: String
    let sha256sum: String?
    let filesize: UInt64?
    let url: String?
    let releasedate: String?
    let signed: Bool?
}

struct FirmwareKeyRecord: Codable, Identifiable, Hashable {
    var id: String { "\(filename)-\(iv)" }
    let filename: String
    let iv: String
    let key: String
    let sourcePage: String
}

struct FirmwareRuntimeRequest: Codable {
    let command: String
    var path: String? = nil
    var device: String? = nil
    var build: String? = nil
    var soc: String? = nil
    var destination: String? = nil
    var executable: String? = nil
    var rom: String? = nil
    var query: String? = nil
    var port: Int? = nil
    var debugCommand: String? = nil
    var address: String? = nil
    var count: Int? = nil
    var steps: UInt64? = nil
    var source: String? = nil
    var mode: String? = nil
}

struct FirmwareRuntimeReply: Codable {
    let ok: Bool
    let message: String
    var text: String? = nil
    var dyld: DyldCacheSummary? = nil
    var devices: [FirmwareDeviceRecord]? = nil
    var firmwares: [FirmwareReleaseRecord]? = nil
    var keys: [FirmwareKeyRecord]? = nil
    var outputPath: String? = nil
    var running: Bool? = nil
    var emulatorOutput: String? = nil
    var emulatorJSON: String? = nil
}
