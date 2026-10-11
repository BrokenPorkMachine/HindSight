import Foundation

/// One-shot, bounded MCP probe for the optional local REA provider.
/// A future analysis session can reuse this framing while adding cancellation
/// and retained evidence handles; the coordinator owns the child process so the
/// SwiftUI process never blocks on provider startup or stdout parsing.
final class REAAgentHost {
    private let maxLineBytes = 4 * 1024 * 1024

    func probe(requestJSON: String) -> String {
        guard let data = requestJSON.data(using: .utf8), data.count <= 64 * 1024,
              let request = try? JSONDecoder().decode(REAProbeRequest.self, from: data),
              let validated = request.validated else {
            return failure("Invalid REA probe request.")
        }

        let process = Process()
        let input = Pipe()
        let output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        process.executableURL = URL(fileURLWithPath: validated.executable)
        process.arguments = validated.arguments
        do { try process.run() } catch { return failure("REA launch failed: \(error.localizedDescription)") }

        defer {
            input.fileHandleForWriting.closeFile()
            output.fileHandleForReading.readabilityHandler = nil
            if process.isRunning { process.terminate() }
        }

        let deadline = Date().addingTimeInterval(Double(validated.timeout_ms) / 1000)
        var nextID = 1
        do {
            let initializeID = nextID; nextID += 1
            try write(Self.message(id: initializeID, method: "initialize", params: [
                "protocolVersion": "2025-06-18",
                "capabilities": [String: String](),
                "clientInfo": ["name": "HindSight", "version": "0.5.7"]
            ]), to: input)
            guard let initialize = try readResponse(id: initializeID, from: output, until: deadline) else {
                return failure("REA initialize timed out or returned no response.")
            }
            guard initialize["error"] == nil else { return response(initialize, method: "initialize") }

            try write(Self.notification(method: "notifications/initialized"), to: input)
            let toolsID = nextID; nextID += 1
            try write(Self.message(id: toolsID, method: "tools/call", params: [
                "name": "binary_session", "arguments": [String: String]()
            ]), to: input)
            guard let result = try readResponse(id: toolsID, from: output, until: deadline) else {
                return failure("REA binary_session timed out or returned no response.")
            }
            return response(result, method: "binary_session")
        } catch {
            return failure("REA probe failed: \(error.localizedDescription)")
        }
    }

    private func write(_ object: [String: Any], to pipe: Pipe) throws {
        let data = try JSONSerialization.data(withJSONObject: object, options: [])
        pipe.fileHandleForWriting.write(data)
        pipe.fileHandleForWriting.write(Data([10]))
    }

    private func readResponse(id: Int, from pipe: Pipe, until deadline: Date) throws -> [String: Any]? {
        var buffer = Data()
        let lock = NSLock()
        let signal = DispatchSemaphore(value: 0)
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            lock.lock()
            if !chunk.isEmpty { buffer.append(chunk) }
            lock.unlock()
            signal.signal()
        }
        defer { pipe.fileHandleForReading.readabilityHandler = nil }
        while Date() < deadline {
            let remaining = max(0, deadline.timeIntervalSinceNow)
            guard signal.wait(timeout: .now() + remaining) == .success else { return nil }
            lock.lock()
            let snapshot = buffer
            buffer.removeAll(keepingCapacity: true)
            lock.unlock()
            guard snapshot.count <= maxLineBytes else { throw HostError(message: "REA response exceeded the bounded line size.") }
            buffer.append(snapshot)
            while let newline = buffer.firstIndex(of: 10) {
                let line = Data(buffer[..<newline]); buffer.removeSubrange(...newline)
                guard !line.isEmpty, let object = try JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
                if let responseID = object["id"] as? Int, responseID == id { return object }
            }
        }
        return nil
    }

    private static func message(id: Int, method: String, params: [String: Any]) -> [String: Any] {
        ["jsonrpc": "2.0", "id": id, "method": method, "params": params]
    }

    private static func notification(method: String) -> [String: Any] {
        ["jsonrpc": "2.0", "method": method]
    }

    private func response(_ object: [String: Any], method: String) -> String {
        let value: [String: Any] = ["ok": object["error"] == nil, "method": method, "response": object]
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: []),
              let text = String(data: data, encoding: .utf8) else { return failure("REA response could not be encoded.") }
        return text
    }

    private func failure(_ message: String) -> String {
        "{\"ok\":false,\"provider\":\"rea-agents\",\"message\":\"\(message.replacingOccurrences(of: "\\\"", with: "'"))\"}"
    }

    private struct HostError: LocalizedError { let message: String; var errorDescription: String? { message } }
}
