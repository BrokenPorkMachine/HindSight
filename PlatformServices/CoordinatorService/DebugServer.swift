import Foundation

/// Queue-confined native debugger facade. The C backend owns the Mach/ptrace session.
final class DebugServer {
    private var hostUnavailableMessage: String? {
        VGHostPlatform.supportsArm64NativeDebugger ? nil : VGHostPlatform.unavailableReason(for: "Native ARM64 debugging")
    }
    private func errorBuffer() -> [CChar] { Array(repeating: 0, count: 1024) }
    private func message(_ buffer: [CChar], fallback: String) -> String {
        buffer.withUnsafeBufferPointer { ptr in
            guard let base = ptr.baseAddress, base.pointee != 0 else { return fallback }
            return String(cString: base)
        }
    }
    private func withErrorBuffer(_ buffer: inout [CChar], _ body: (UnsafeMutablePointer<CChar>, Int) -> Int32) -> Int32 {
        buffer.withUnsafeMutableBufferPointer { storage -> Int32 in
            guard let base = storage.baseAddress else { return -1 }
            return body(base, storage.count)
        }
    }
    private func jsonResult(_ body: (_ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>, _ err: UnsafeMutablePointer<CChar>, _ cap: Int) -> Int32) -> Result<String, Error> {
        var out: UnsafeMutablePointer<CChar>?
        var err = errorBuffer()
        let rc = err.withUnsafeMutableBufferPointer { ep -> Int32 in
            guard let base = ep.baseAddress else { return -1 }
            return body(&out, base, ep.count)
        }
        guard rc == 0, let out else { return .failure(WorkbenchError(message(err, fallback: "Debugger operation failed."))) }
        defer { rw_debug_free_string(out) }
        return .success(String(cString: out))
    }
    func attach(pid: Int32) -> String {
        if let hostUnavailableMessage { return DebugCommandReply.failure(hostUnavailableMessage) }
        var err = errorBuffer()
        let rc = err.withUnsafeMutableBufferPointer { buffer -> Int32 in
            guard let base = buffer.baseAddress else { return -1 }
            return rw_debug_attach(pid, base, buffer.count)
        }
        return rc == 0 ? DebugCommandReply.success("Attached to PID \(pid).", pid: pid) : DebugCommandReply.failure(message(err, fallback: "Attach failed."))
    }
    func launch(path: String) -> String {
        if let hostUnavailableMessage { return DebugCommandReply.failure(hostUnavailableMessage) }
        guard path.hasPrefix("/"), path.utf8.count <= 16384, !path.contains("\0") else { return DebugCommandReply.failure("Invalid launch path.") }
        var pid: Int32 = 0; var err = errorBuffer()
        let rc = path.withCString { cpath in
            err.withUnsafeMutableBufferPointer { buffer -> Int32 in
                guard let base = buffer.baseAddress else { return -1 }
                return rw_debug_launch(cpath, &pid, base, buffer.count)
            }
        }
        return rc == 0 ? DebugCommandReply.success("Launched and attached PID \(pid).", pid: pid) : DebugCommandReply.failure(message(err, fallback: "Launch failed."))
    }
    func command(_ name: String, address: UInt64? = nil) -> String {
        if let hostUnavailableMessage { return DebugCommandReply.failure(hostUnavailableMessage) }
        var err = errorBuffer(); let rc: Int32
        switch name {
        case "detach": rc = withErrorBuffer(&err) { rw_debug_detach($0, $1) }
        case "continue": rc = withErrorBuffer(&err) { rw_debug_continue($0, $1) }
        case "interrupt": rc = withErrorBuffer(&err) { rw_debug_interrupt($0, $1) }
        case "step": rc = withErrorBuffer(&err) { rw_debug_step($0, $1) }
        case "breakpoint.set":
            guard let address else { return DebugCommandReply.failure("Breakpoint address required.") }
            rc = withErrorBuffer(&err) { rw_debug_set_breakpoint(address, $0, $1) }
        case "breakpoint.remove":
            guard let address else { return DebugCommandReply.failure("Breakpoint address required.") }
            rc = withErrorBuffer(&err) { rw_debug_remove_breakpoint(address, $0, $1) }
        default: return DebugCommandReply.failure("Unknown debugger command.")
        }
        return rc == 0 ? DebugCommandReply.success("Debugger command completed.") : DebugCommandReply.failure(message(err, fallback: "Debugger command failed."))
    }
    func snapshot() -> Result<String, Error> { if let hostUnavailableMessage { return .failure(WorkbenchError(hostUnavailableMessage)) }; return jsonResult { rw_debug_snapshot($0, $1, $2) } }
    func event() -> Result<String, Error> { if let hostUnavailableMessage { return .failure(WorkbenchError(hostUnavailableMessage)) }; return jsonResult { rw_debug_poll_event($0, $1, $2) } }
    func memory(address: UInt64, length: Int) -> Result<String, Error> {
        if let hostUnavailableMessage { return .failure(WorkbenchError(hostUnavailableMessage)) }
        guard (1...4096).contains(length) else { return .failure(WorkbenchError("Memory reads are bounded to 1...4096 bytes.")) }
        return jsonResult { rw_debug_read_memory(address, length, $0, $1, $2) }
    }
    func query(_ name: String) -> Result<String, Error> {
        if let hostUnavailableMessage { return .failure(WorkbenchError(hostUnavailableMessage)) }
        switch name {
        case "threads": return jsonResult { rw_debug_threads($0, $1, $2) }
        case "memory-map": return jsonResult { rw_debug_memory_map($0, $1, $2) }
        case "modules": return jsonResult { rw_debug_modules($0, $1, $2) }
        case "backtrace": return jsonResult { rw_debug_backtrace($0, $1, $2) }
        case "disassembly": return jsonResult { rw_debug_disassembly($0, $1, $2) }
        default: return .failure(WorkbenchError("Unknown debugger query."))
        }
    }
    func selectThread(_ threadID: UInt64) -> String {
        if let hostUnavailableMessage { return DebugCommandReply.failure(hostUnavailableMessage) }
        var err = errorBuffer()
        let rc = withErrorBuffer(&err) { rw_debug_select_thread(threadID, $0, $1) }
        return rc == 0 ? DebugCommandReply.success("Selected thread 0x" + String(threadID, radix: 16) + ".") : DebugCommandReply.failure(message(err, fallback: "Thread selection failed."))
    }
    func shutdown() { if VGHostPlatform.supportsArm64NativeDebugger { _ = command("detach") } }
}
