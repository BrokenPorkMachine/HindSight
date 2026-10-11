import Foundation

private func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

@main
struct REAAgentHostContract {
    static func main() {
        let fixture = CommandLine.arguments[1]
        let target = CommandLine.arguments[2]

        func request(arguments: [String]) -> String {
            let value = REAOperationRequest(executable: "/usr/bin/python3", arguments: arguments,
                                           target_path: target, provider_id: "auto", tool: "binary_overview", timeout_ms: 5_000)
            let data = try! JSONEncoder().encode(value)
            return String(decoding: data, as: UTF8.self)
        }

        let allowedHost = REAAgentHost()
        let allowed = allowedHost.analyze(requestJSON: request(arguments: [fixture, "allow"]), requestID: "allowed")
        let allowedObject = try! JSONSerialization.jsonObject(with: Data(allowed.utf8)) as! [String: Any]
        require(allowedObject["ok"] as? Bool == true, "fixture operation should succeed")
        let progress = allowedHost.progress(requestID: "allowed")
        require(progress.contains("fixture progress"), "fixture progress notification was not retained")

        let deniedHost = REAAgentHost()
        let denied = deniedHost.analyze(requestJSON: request(arguments: [fixture, "deny"]), requestID: "denied")
        let deniedObject = try! JSONSerialization.jsonObject(with: Data(denied.utf8)) as! [String: Any]
        require(deniedObject["ok"] as? Bool == false, "unavailable fixture operation should fail closed")
        require((deniedObject["message"] as? String)?.contains("unavailable") == true, "capability failure should explain availability")
        print("REA agent host fixture contract passed")
    }
}
