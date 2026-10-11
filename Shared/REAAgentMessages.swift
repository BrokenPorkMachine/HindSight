import Foundation

struct REAProbeRequest: Codable {
    let executable: String
    let arguments: [String]
    let timeout_ms: Int

    var validated: REAProbeRequest? {
        let command = executable.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !command.isEmpty, command.utf8.count <= 4096,
              arguments.count <= 32, arguments.allSatisfy({ $0.utf8.count <= 4096 }),
              (500...60_000).contains(timeout_ms) else { return nil }
        return REAProbeRequest(executable: command, arguments: arguments, timeout_ms: timeout_ms)
    }
}

struct REAOperationRequest: Codable {
    let executable: String
    let arguments: [String]
    let target_path: String
    let provider_id: String
    let tool: String
    let timeout_ms: Int

    var validated: REAOperationRequest? {
        let target = target_path.trimmingCharacters(in: .whitespacesAndNewlines)
        guard target.hasPrefix("/"), target.utf8.count <= 16_384, !target.contains("\0"),
              ["auto", "hopper", "ghidra", "ida"].contains(provider_id),
              ["binary_overview", "list_procedures", "list_names"].contains(tool),
              let probe = try? JSONEncoder().encode(REAProbeRequest(executable: executable, arguments: arguments, timeout_ms: timeout_ms)),
              let validatedProbe = try? JSONDecoder().decode(REAProbeRequest.self, from: probe),
              validatedProbe.validated != nil else { return nil }
        return REAOperationRequest(executable: executable, arguments: arguments, target_path: target, provider_id: provider_id, tool: tool, timeout_ms: timeout_ms)
    }
}
