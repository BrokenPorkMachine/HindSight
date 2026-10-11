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
