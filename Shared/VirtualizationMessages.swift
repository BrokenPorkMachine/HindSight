import Foundation

struct VirtualizationRegion: Codable, Identifiable, Hashable {
    var id: String { "\(name)-\(base)" }
    let name: String
    let kind: String
    let base: String
    let size: String
    let access: String
    let notes: String
}

struct VirtualizationProfile: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let cpuMode: String
    let description: String
    let maturity: String
    let defaultRAMMiB: Int
    let devices: [String]
    let regions: [VirtualizationRegion]
    let limitations: [String]
}

struct VirtualizationState: Codable {
    let ok: Bool
    let message: String
    let running: Bool
    let profileID: String?
    let ramMiB: Int
    let cpuBackend: String
    let deviceBackend: String
    let topology: [String]
    let limitations: [String]
}

struct VirtualizationRERequest: Codable {
    let command: String
    var address: String? = nil
    var length: Int? = nil
    var value: String? = nil
    var path: String? = nil
    var mode: String? = nil
}

struct VirtualizationREReply: Codable {
    let ok: Bool
    let message: String
    var title: String? = nil
    var text: String? = nil
    var address: String? = nil
    var pc: String? = nil
    var sp: String? = nil
    var exitReason: UInt32? = nil
    var count: Int? = nil
}
