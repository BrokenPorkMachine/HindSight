import Foundation

/// Owns one native PTY session for the local user. All mutable state is
/// serialized so XPC polling, input, resize, and termination cannot race.
final class TerminalHost {
    private let lock = NSLock()
    private var terminal: OpaquePointer?
    private var sessionID: String?
    private var exitMessage = ""
    private let maxInputBytes = 64 * 1024
    // Keep the encoded TerminalReply comfortably below the client's XPC reply
    // budget. Any bytes beyond this window remain in the PTY for the next poll.
    private let maxOutputBytes = 48 * 1024

    func start(workingDirectory: String) -> String {
        let directory = URL(fileURLWithPath: workingDirectory).standardizedFileURL.path
        var isDirectory: ObjCBool = false
        guard directory.hasPrefix("/"), FileManager.default.fileExists(atPath: directory, isDirectory: &isDirectory), isDirectory.boolValue,
              FileManager.default.isReadableFile(atPath: directory) else {
            return reply(false, "Terminal working directory is not a readable directory.")
        }
        lock.lock()
        stopLocked()
        var error = [CChar](repeating: 0, count: 512)
        let created = error.withUnsafeMutableBufferPointer { buffer in
            directory.withCString { path in
                "/bin/zsh".withCString { shell in
                    vg_terminal_pty_start(path, shell, 120, 32, buffer.baseAddress, buffer.count)
                }
            }
        }
        guard let created else {
            let message = String(decoding: error.map { UInt8(bitPattern: $0) }, as: UTF8.self).trimmingCharacters(in: .controlCharacters)
            lock.unlock()
            return reply(false, message.isEmpty ? "Unable to start terminal." : message)
        }
        terminal = created; sessionID = UUID().uuidString; exitMessage = ""
        lock.unlock()
        return reply(true, "Terminal started.")
    }

    func write(_ text: String) -> String {
        guard let data = text.data(using: .utf8), !data.isEmpty, data.count <= maxInputBytes else {
            return reply(false, "Terminal input is empty or exceeds 64 KiB.")
        }
        lock.lock()
        guard let terminal, vg_terminal_pty_is_running(terminal) != 0 else { lock.unlock(); return reply(false, "No terminal session is running.") }
        let result = data.withUnsafeBytes { raw in vg_terminal_pty_write(terminal, raw.baseAddress, data.count) }
        lock.unlock()
        return result == data.count ? reply(true, "Input sent.") : reply(false, "Terminal input failed.")
    }

    func read() -> String {
        lock.lock()
        guard let terminal else { lock.unlock(); return TerminalReply.encode(TerminalReply(ok: true, message: "No terminal session.", running: false)) }
        var output = Data(); var buffer = [UInt8](repeating: 0, count: 64 * 1024)
        while output.count < maxOutputBytes {
            let bufferLength = min(buffer.count, maxOutputBytes - output.count)
            let count = buffer.withUnsafeMutableBytes { raw in vg_terminal_pty_read(terminal, raw.baseAddress, bufferLength) }
            if count > 0 { output.append(buffer, count: Int(count)); continue }
            break
        }
        let running = vg_terminal_pty_is_running(terminal) != 0
        if !running && exitMessage.isEmpty { exitMessage = "Shell exited." }
        let reply = TerminalReply(ok: true, message: exitMessage, sessionID: sessionID, running: running,
                                  output: output.isEmpty ? nil : String(decoding: output, as: UTF8.self))
        lock.unlock()
        return TerminalReply.encode(reply)
    }

    func resize(columns: Int, rows: Int) -> String {
        let columns = min(max(columns, 20), 400); let rows = min(max(rows, 4), 200)
        lock.lock()
        guard let terminal else { lock.unlock(); return reply(false, "No terminal session is running.") }
        let ok = vg_terminal_pty_resize(terminal, UInt16(columns), UInt16(rows)) == 0
        lock.unlock()
        return reply(ok, ok ? "Terminal resized." : "Terminal resize failed.")
    }

    func interrupt() -> String {
        lock.lock()
        guard let terminal else { lock.unlock(); return reply(false, "No terminal session is running.") }
        let ok = vg_terminal_pty_interrupt(terminal) == 0
        lock.unlock()
        return reply(ok, ok ? "Interrupt sent." : "Unable to interrupt the terminal process.")
    }

    func eof() -> String {
        lock.lock()
        guard let terminal else { lock.unlock(); return reply(false, "No terminal session is running.") }
        let ok = vg_terminal_pty_eof(terminal) == 0
        lock.unlock()
        return reply(ok, ok ? "EOF sent." : "Unable to send EOF.")
    }

    func stop() { lock.lock(); stopLocked(); lock.unlock() }

    private func stopLocked() {
        if let terminal { vg_terminal_pty_stop(terminal) }
        terminal = nil; sessionID = nil; exitMessage = ""
    }

    private func reply(_ ok: Bool, _ message: String) -> String {
        lock.lock(); let id = sessionID; let running = terminal.map { vg_terminal_pty_is_running($0) != 0 } ?? false; lock.unlock()
        return TerminalReply.encode(TerminalReply(ok: ok, message: message, sessionID: id, running: running))
    }
}
