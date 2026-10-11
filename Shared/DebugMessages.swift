import Foundation

struct DebugCommandReply: Codable {
    let ok: Bool
    let message: String
    let pid: Int32?
    static func success(_ message: String = "", pid: Int32? = nil) -> String {
        encode(DebugCommandReply(ok: true, message: message, pid: pid))
    }
    static func failure(_ message: String) -> String {
        encode(DebugCommandReply(ok: false, message: message, pid: nil))
    }
    private static func encode(_ value: DebugCommandReply) -> String {
        guard let data = try? JSONEncoder().encode(value) else { return "{\"ok\":false,\"message\":\"Debug reply encoding failed\"}" }
        return String(decoding: data, as: UTF8.self)
    }
}

struct DebugSnapshot: Codable {
    let pid: Int32
    let stopped: Bool
    let thread_count: UInt32
    let selected_thread_id: String?
    let pc: String
    let sp: String
    let fp: String
    let lr: String
    let cpsr: UInt32
    let registers: [String: String]
    let breakpoints: [String]
}

struct DebugMemory: Codable {
    let address: String
    let length: Int
    let hex: String
}

struct DebugEvent: Codable {
    let event: String
    let signal: Int?
    let code: Int?
    let reason: String?
    let pc: String?
}
