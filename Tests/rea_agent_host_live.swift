import Foundation

private func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

@main
struct REAAgentHostLiveQualification {
    static func main() {
        guard CommandLine.arguments.count == 3 else { fail("usage: rea-agent-host-live <rea-executable> <target>") }
        let executable = CommandLine.arguments[1]
        let target = CommandLine.arguments[2]
        let host = REAAgentHost()
        let encoder = JSONEncoder()

        let probe = REAProbeRequest(executable: executable, arguments: ["mcp"], timeout_ms: 20_000)
        let probeJSON = String(decoding: try! encoder.encode(probe), as: UTF8.self)
        let probeResult = host.probe(requestJSON: probeJSON)
        guard let probeObject = try? JSONSerialization.jsonObject(with: Data(probeResult.utf8)) as? [String: Any],
              probeObject["ok"] as? Bool == true else { fail("live REA probe failed: \(probeResult)") }

        let operation = REAOperationRequest(executable: executable, arguments: ["mcp"], target_path: target,
                                            provider_id: "hopper", tool: "binary_overview", timeout_ms: 60_000)
        let operationJSON = String(decoding: try! encoder.encode(operation), as: UTF8.self)
        let result = host.analyze(requestJSON: operationJSON, requestID: "live-qualification")
        guard let object = try? JSONSerialization.jsonObject(with: Data(result.utf8)) as? [String: Any],
              object["ok"] as? Bool == true else { fail("live REA operation failed: \(result)") }
        let progress = host.progress(requestID: "live-qualification")
        print("REA live host qualification passed • response \(result.utf8.count) bytes • progress \(progress.utf8.count) bytes")
    }
}
