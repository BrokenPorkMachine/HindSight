import Foundation
import Darwin

private final class REAOutputReader {
    private let handle: FileHandle
    private let lock = NSLock()
    private let signal = DispatchSemaphore(value: 0)
    private var buffer = Data()
    private var reachedEOF = false

    init(handle: FileHandle) {
        self.handle = handle
        handle.readabilityHandler = { [weak self] handle in
            guard let self else { return }
            let chunk = handle.availableData
            self.lock.lock()
            if !chunk.isEmpty { self.buffer.append(chunk) }
            else { self.reachedEOF = true }
            self.lock.unlock()
            self.signal.signal()
        }
    }
    deinit { handle.readabilityHandler = nil }

    func nextLine(until deadline: Date, maxBytes: Int) -> Data? {
        while Date() < deadline {
            lock.lock()
            if let newline = buffer.firstIndex(of: 10) {
                let line = Data(buffer[..<newline])
                buffer.removeSubrange(...newline)
                lock.unlock()
                return line
            }
            let size = buffer.count
            let eof = reachedEOF
            lock.unlock()
            if eof { return nil }
            if size > maxBytes { return nil }
            let remaining = max(0, deadline.timeIntervalSinceNow)
            guard signal.wait(timeout: .now() + remaining) == .success else { return nil }
        }
        return nil
    }
}

/// Bounded, one-shot MCP client for the optional local REA provider.
final class REAAgentHost {
    private let maxLineBytes = 4 * 1024 * 1024
    private let processLock = NSLock()
    private struct ManagedProcess {
        let process: Process
        let grouped: Bool
    }
    private var processes: [String: ManagedProcess] = [:]
    private var progressHistory: [String: [String]] = [:]

    func probe(requestJSON: String) -> String {
        guard let data = requestJSON.data(using: .utf8), data.count <= 64 * 1024,
              let request = try? JSONDecoder().decode(REAProbeRequest.self, from: data),
              let validated = request.validated else { return failure("Invalid REA probe request.") }
        return run(executable: validated.executable, arguments: validated.arguments,
                   targetPath: nil, providerID: "auto", tool: nil, timeoutMS: validated.timeout_ms)
    }

    func analyze(requestJSON: String, requestID: String) -> String {
        guard let data = requestJSON.data(using: .utf8), data.count <= 64 * 1024,
              let request = try? JSONDecoder().decode(REAOperationRequest.self, from: data),
              let validated = request.validated else { return failure("Invalid REA operation request.") }
        guard FileManager.default.fileExists(atPath: validated.target_path) else {
            return failure("REA target does not exist: \(validated.target_path)")
        }
        return run(executable: validated.executable, arguments: validated.arguments,
                   targetPath: validated.target_path, providerID: validated.provider_id,
                   tool: validated.tool, timeoutMS: validated.timeout_ms, requestID: requestID)
    }

    func progress(requestID: String) -> String {
        processLock.lock()
        let events = progressHistory[requestID] ?? []
        processLock.unlock()
        guard let data = try? JSONSerialization.data(withJSONObject: events, options: []),
              let text = String(data: data, encoding: .utf8) else { return "[]" }
        return text
    }

    func cancel(requestID: String) -> Bool {
        processLock.lock()
        let managed = processes[requestID]
        processLock.unlock()
        guard let managed, managed.process.isRunning else { return false }
        terminate(managed, wait: nil)
        return true
    }

    private func establishProcessGroup(for process: Process) -> Bool {
        let pid = process.processIdentifier
        return pid > 0 && setpgid(pid, pid) == 0
    }

    private func terminate(_ managed: ManagedProcess, wait: DispatchSemaphore?) {
        guard managed.process.isRunning else { return }
        let pid = managed.process.processIdentifier
        if managed.grouped { _ = kill(-pid, SIGTERM) } else { managed.process.terminate() }
        if let wait { _ = wait.wait(timeout: .now() + 1) }
        if managed.process.isRunning {
            if managed.grouped { _ = kill(-pid, SIGKILL) } else { _ = kill(pid, SIGKILL) }
        }
    }

    private func run(executable: String, arguments: [String], targetPath: String?, providerID: String,
                     tool: String?, timeoutMS: Int, requestID: String? = nil) -> String {
        let process = Process()
        let input = Pipe()
        let output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        do { try process.run() } catch { return failure("REA launch failed: \(error.localizedDescription)") }
        let managed = ManagedProcess(process: process, grouped: establishProcessGroup(for: process))
        if let requestID {
            processLock.lock()
            processes[requestID] = managed
            progressHistory[requestID] = ["Launching local REA MCP server."]
            if progressHistory.count > 16, let oldest = progressHistory.keys.first, oldest != requestID {
                progressHistory.removeValue(forKey: oldest)
            }
            processLock.unlock()
        }
        let reader = REAOutputReader(handle: output.fileHandleForReading)
        defer {
            if let requestID { processLock.lock(); processes.removeValue(forKey: requestID); processLock.unlock() }
            input.fileHandleForWriting.closeFile()
            terminate(managed, wait: nil)
        }

        let deadline = Date().addingTimeInterval(Double(timeoutMS) / 1000)
        var nextID = 1
        do {
            let initializeID = nextID; nextID += 1
            try write(Self.message(id: initializeID, method: "initialize", params: [
                "protocolVersion": "2025-06-18", "capabilities": [String: Any](),
                "clientInfo": ["name": "HindSight", "version": "0.5.10"]
            ]), to: input)
            guard let initialize = try readResponse(id: initializeID, from: reader, until: deadline, requestID: requestID) else {
                return failure("REA initialize timed out or returned no response.")
            }
            guard initialize["error"] == nil else { return response(initialize, method: "initialize") }
            try write(Self.notification(method: "notifications/initialized"), to: input)

            let sessionID = nextID; nextID += 1
            try write(Self.message(id: sessionID, method: "tools/call", params: [
                "name": "binary_session", "arguments": [String: Any]()
            ]), to: input)
            guard let session = try readResponse(id: sessionID, from: reader, until: deadline, requestID: requestID) else {
                return failure("REA binary_session timed out or returned no response.")
            }
            guard session["error"] == nil else { return response(session, method: "binary_session") }
            guard let targetPath, let tool else { return response(session, method: "binary_session") }
            if let availability = Self.toolAvailability(in: session, tool: tool), !availability.available {
                return failure("REA operation \(tool) is unavailable: \(availability.reason)")
            }

            let openID = nextID; nextID += 1
            try write(Self.message(id: openID, method: "tools/call", params: [
                "name": "open_binary", "arguments": ["path": targetPath, "provider_id": providerID]
            ]), to: input)
            guard let opened = try readResponse(id: openID, from: reader, until: deadline, requestID: requestID) else {
                return failure("REA open_binary timed out or returned no response.")
            }
            guard opened["error"] == nil else { return response(opened, method: "open_binary") }

            let operationID = nextID; nextID += 1
            try write(Self.message(id: operationID, method: "tools/call", params: [
                "name": tool, "arguments": [String: Any](),
                "_meta": ["progressToken": "hindsight-\(operationID)"]
            ]), to: input)
            guard let result = try readResponse(id: operationID, from: reader, until: deadline, requestID: requestID) else {
                return failure("REA \(tool) timed out or returned no response.")
            }
            return response(result, method: tool)
        } catch { return failure("REA operation failed: \(error.localizedDescription)") }
    }

    private func write(_ object: [String: Any], to pipe: Pipe) throws {
        let data = try JSONSerialization.data(withJSONObject: object, options: [])
        pipe.fileHandleForWriting.write(data)
        pipe.fileHandleForWriting.write(Data([10]))
    }

    private func readResponse(id: Int, from reader: REAOutputReader, until deadline: Date, requestID: String?) throws -> [String: Any]? {
        while let line = reader.nextLine(until: deadline, maxBytes: maxLineBytes) {
            guard !line.isEmpty, let object = try JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
            if let responseID = object["id"] as? Int, responseID == id { return object }
            if let requestID, let method = object["method"] as? String, method == "notifications/progress" {
                let summary: String
                if let params = object["params"] as? [String: Any],
                   let message = params["message"] as? String, !message.isEmpty {
                    summary = message
                } else if let params = object["params"] as? [String: Any],
                          let progress = params["progress"] {
                    let total = params["total"].map { "/\($0)" } ?? ""
                    summary = "Progress \(progress)\(total)"
                } else {
                    summary = "REA provider progress notification received."
                }
                processLock.lock()
                var events = progressHistory[requestID] ?? []
                let bounded = String(summary.prefix(512))
                if events.last != bounded { events.append(bounded) }
                if events.count > 64 { events.removeFirst(events.count - 64) }
                progressHistory[requestID] = events
                processLock.unlock()
            }
        }
        return nil
    }

    private static func message(id: Int, method: String, params: [String: Any]) -> [String: Any] {
        ["jsonrpc": "2.0", "id": id, "method": method, "params": params]
    }
    private static func notification(method: String) -> [String: Any] { ["jsonrpc": "2.0", "method": method] }

    private static func toolAvailability(in object: Any, tool: String) -> (available: Bool, reason: String)? {
        if let dictionary = object as? [String: Any] {
            if let catalog = dictionary["tool_availability"] as? [String: Any],
               let entry = catalog[tool] as? [String: Any],
               let available = entry["available"] as? Bool {
                let reason = (entry["reason"] as? String) ?? "The provider did not report a remediation."
                return (available, String(reason.prefix(512)))
            }
            for value in dictionary.values {
                if let result = toolAvailability(in: value, tool: tool) { return result }
                if let text = value as? String,
                   let data = text.data(using: .utf8),
                   let decoded = try? JSONSerialization.jsonObject(with: data),
                   let result = toolAvailability(in: decoded, tool: tool) { return result }
            }
        } else if let array = object as? [Any] {
            for value in array {
                if let result = toolAvailability(in: value, tool: tool) { return result }
            }
        }
        return nil
    }

    private func response(_ object: [String: Any], method: String) -> String {
        let value: [String: Any] = ["ok": object["error"] == nil, "method": method, "response": object]
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: []),
              let text = String(data: data, encoding: .utf8) else { return failure("REA response could not be encoded.") }
        return text
    }

    private func failure(_ message: String) -> String {
        let value: [String: Any] = ["ok": false, "provider": "rea-agents", "message": message]
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: []),
              let text = String(data: data, encoding: .utf8) else { return "{\"ok\":false}" }
        return text
    }
}
