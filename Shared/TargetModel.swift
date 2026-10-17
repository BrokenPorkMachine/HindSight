import Foundation
import CryptoKit

enum VGProduct {
    static let name = "HindSight"
    static let displayName = "HindSight Research Tool"
    static let bundleName = "HindSight.app"
    static let version = "0.5.11"
    static let releaseChannel = "stable"
    static let isPrerelease = false
}

enum VGTargetKind: String, Codable, CaseIterable, Hashable {
    case staticArtifact = "static"
    case nativeProcess = "native"
    case emulatedMachine = "emulated"
    case hypervisorGuest = "virtualized"
}

enum VGTargetCapability: String, Codable, CaseIterable, Hashable {
    case readBytes = "read.bytes"
    case decode = "decode"
    case analyze = "analyze"
    case symbols = "symbols"
    case mappings = "mappings"
    case registers = "registers"
    case executionControl = "execution.control"
    case breakpoints = "breakpoints"
    case trace = "trace"
    case deterministicReset = "deterministic.reset"
    case snapshots = "snapshots"
    case addressTranslation = "address.translate"
}

struct VGAddressSpace: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let kind: String
    let base: String?
    let size: String?
    let source: String
}

struct VGSessionHeader: Codable, Hashable {
    let schemaVersion: Int
    let productVersion: String
    let targetID: String
    let targetFingerprint: String
    let engineFingerprint: String
    let modelFingerprint: String
    let sourceFingerprint: String?
    let traceFingerprint: String?
}

struct VGTargetIdentity: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let kind: VGTargetKind
    let architecture: String
    let backend: String
    let state: String
    let capabilities: [VGTargetCapability]
    let addressSpaces: [VGAddressSpace]
    let session: VGSessionHeader
}

struct VGAddressTranslation: Codable, Identifiable, Hashable {
    var id: String { "\(space)|\(address)|\(detail)" }
    let space: String
    let address: String
    let detail: String
    let evidence: String
}

struct VGAddressPivot: Codable, Hashable {
    let targetID: String
    let input: String
    let translations: [VGAddressTranslation]
    let confidence: String
    let explanation: String
}

struct VGTargetSessionExport: Encodable {
    let format = "ca.zensoftware.VectorGlass.target-session"
    let schemaVersion = 2
    let target: VGTargetIdentity
    let pivot: VGAddressPivot?
}

enum VGTargetUtilities {
    private static let embeddedBuildFingerprints: [String: String] = {
        let executableDirectory = Bundle.main.bundleURL
            .appendingPathComponent("Contents/XPCServices/CoordinatorService.xpc/Contents/MacOS", isDirectory: true)
        return Dictionary(uniqueKeysWithValues: ["reworker", "CoordinatorService", "vg-emu-server"].map { name in
            let url = executableDirectory.appendingPathComponent(name, isDirectory: false)
            return (name, fileFingerprint(at: url) ?? "unavailable")
        })
    }()

    static func parseHex(_ text: String) -> UInt64? {
        var value = text.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if value.hasPrefix("0x") { value.removeFirst(2) }
        guard !value.isEmpty else { return nil }
        return UInt64(value, radix: 16)
    }

    static func hex(_ value: UInt64) -> String {
        String(format: "0x%016llx", value)
    }

    static func fingerprint(_ fields: [String]) -> String {
        var hasher = SHA256()
        hasher.update(data: Data("HindSight.target-fingerprint.v1\0".utf8))
        for field in fields {
            let data = Data(field.utf8)
            var length = UInt64(data.count).bigEndian
            hasher.update(data: withUnsafeBytes(of: &length) { Data($0) })
            hasher.update(data: data)
        }
        return "sha256:" + hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    static func buildFingerprint(_ executable: String) -> String {
        embeddedBuildFingerprints[executable] ?? "unavailable"
    }

    private static func fileFingerprint(at url: URL) -> String? {
        guard FileManager.default.isExecutableFile(atPath: url.path),
              let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        var hasher = SHA256()
        do {
            while let data = try handle.read(upToCount: 1_048_576), !data.isEmpty {
                hasher.update(data: data)
            }
            return "sha256:" + hasher.finalize().map { String(format: "%02x", $0) }.joined()
        } catch { return nil }
    }


    static func add(_ lhs: UInt64, _ rhs: UInt64) -> UInt64? {
        let result = lhs.addingReportingOverflow(rhs)
        return result.overflow ? nil : result.partialValue
    }

    static func applying(slide: Int64, to value: UInt64) -> UInt64? {
        if slide >= 0 {
            let (result, overflow) = value.addingReportingOverflow(UInt64(slide))
            return overflow ? nil : result
        }
        let magnitude = slide == Int64.min ? UInt64(Int64.max) + 1 : UInt64(-slide)
        guard value >= magnitude else { return nil }
        return value - magnitude
    }

    static func unique(_ rows: [VGAddressTranslation]) -> [VGAddressTranslation] {
        var seen = Set<String>()
        return rows.filter { seen.insert($0.id).inserted }
    }

    static func nativeModuleRows(_ text: String) -> [(load: UInt64, slide: Int64, preferred: UInt64, path: String)] {
        text.split(separator: "\n").compactMap { raw in
            let line = String(raw)
            guard line.hasPrefix("0x"),
                  let loadToken = line.split(separator: " ").first,
                  let load = parseHex(String(loadToken)),
                  let slideRange = line.range(of: "slide="),
                  let preferredRange = line.range(of: "preferred=0x") else { return nil }
            let afterSlide = line[slideRange.upperBound...]
            guard let slideToken = afterSlide.split(separator: " ").first,
                  let slide = Int64(slideToken) else { return nil }
            let preferredStart = preferredRange.upperBound
            let preferredTail = line[preferredStart...]
            guard let preferredToken = preferredTail.split(separator: " ").first,
                  let preferred = UInt64(preferredToken, radix: 16) else { return nil }
            let pathStart = preferredTail.dropFirst(preferredToken.count).drop(while: { $0.isWhitespace })
            return (load, slide, preferred, String(pathStart))
        }
    }
}
