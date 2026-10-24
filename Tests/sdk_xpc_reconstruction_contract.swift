import Foundation
import CryptoKit

@main
enum SDKXPCContract {
    static func main() throws {
        guard CommandLine.arguments.count == 3 else { fatalError("binary and project paths required") }
        let connection = NSXPCConnection(serviceName: ServiceIdentity.coordinator)
        connection.remoteObjectInterface = NSXPCInterface(with: CoordinatorProtocol.self)
        connection.resume()
        defer { connection.invalidate() }
        func call(_ operation: (CoordinatorProtocol, @escaping (String) -> Void) -> Void) throws -> [String: Any] {
            let finished = DispatchSemaphore(value: 0)
            var reply: String?
            var failure: Error?
            guard let service = connection.remoteObjectProxyWithErrorHandler({ error in
                failure = error; finished.signal()
            }) as? CoordinatorProtocol else { throw NSError(domain: "SDKXPC", code: 1) }
            operation(service) { text in reply = text; finished.signal() }
            guard finished.wait(timeout: .now() + 240) == .success else { throw NSError(domain: "SDKXPC", code: 2) }
            if let failure { throw failure }
            guard let reply, let data = reply.data(using: .utf8),
                  let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                throw NSError(domain: "SDKXPC", code: 3)
            }
            return object
        }
        let opened = try call { $0.openProject(CommandLine.arguments[2], create: true, withReply: $1) }
        guard opened["ok"] as? Bool == true else { throw NSError(domain: "SDKXPC", code: 4, userInfo: [NSLocalizedDescriptionKey: String(describing: opened)]) }
        let captured = try call { $0.inspectHeader(CommandLine.arguments[1], requestID: UUID().uuidString, withReply: $1) }
        guard captured["ok"] as? Bool == true,
              let project = captured["project"] as? [String: Any],
              let records = project["records"] as? [[String: Any]],
              let recordID = records.first?["id"] as? String else {
            throw NSError(domain: "SDKXPC", code: 5, userInfo: [NSLocalizedDescriptionKey: String(describing: captured)])
        }
        let analysis = try call { $0.analyzeRecord(recordID, requestID: UUID().uuidString, withReply: $1) }
        guard analysis["ok"] as? Bool == true,
              let report = analysis["analysis"] as? [String: Any],
              let functions = report["functions"] as? String,
              let linkage = report["linkage_records"] as? String,
              linkage.contains("symbol=_fwrite import[0] dylib=/usr/lib/libSystem.B.dylib addend=0"),
              let line = functions.split(separator: "\n").first(where: { $0.contains("_HSWritePrefix") }) else {
            throw NSError(domain: "SDKXPC", code: 6, userInfo: [NSLocalizedDescriptionKey: String(describing: analysis)])
        }
        let fields = line.split(whereSeparator: \.isWhitespace)
        guard fields.count >= 2,
              let start = UInt64(fields[0].dropFirst(2), radix: 16),
              let end = UInt64(fields[1].dropFirst(2), radix: 16) else { throw NSError(domain: "SDKXPC", code: 7) }
        let cancelledID = UUID().uuidString
        let cancelledCompletion = DispatchSemaphore(value: 0)
        var cancelledReply: String?
        guard let cancelService = connection.remoteObjectProxyWithErrorHandler({ _ in cancelledCompletion.signal() }) as? CoordinatorProtocol else {
            throw NSError(domain: "SDKXPC", code: 9)
        }
        cancelService.reconstructFunction(recordID, startAddress: String(start), endAddress: String(end),
                                          requestID: cancelledID) { text in
            cancelledReply = text
            cancelledCompletion.signal()
        }
        var sawLookup = false
        for _ in 0..<100 {
            let progress = try call { $0.inspectionStatus(cancelledID, withReply: $1) }
            if let status = progress["status"] as? [String: Any],
               status["phase"] as? String == "running", status["progress_stage"] as? String == "sdk-lookup" {
                sawLookup = true
                break
            }
            Thread.sleep(forTimeInterval: 0.01)
        }
        guard sawLookup else { throw NSError(domain: "SDKXPC", code: 10, userInfo: [NSLocalizedDescriptionKey: "SDK lookup phase was not observable."]) }
        let accepted = DispatchSemaphore(value: 0)
        var cancellationAccepted = false
        cancelService.cancelInspection(cancelledID) { value in cancellationAccepted = value; accepted.signal() }
        guard accepted.wait(timeout: .now() + 10) == .success, cancellationAccepted,
              cancelledCompletion.wait(timeout: .now() + 30) == .success,
              let cancelledReply, let cancelledData = cancelledReply.data(using: .utf8),
              let cancelled = try JSONSerialization.jsonObject(with: cancelledData) as? [String: Any],
              cancelled["artifactJSON"] == nil,
              (cancelled["message"] as? String)?.contains("cancelled before worker launch") == true else {
            throw NSError(domain: "SDKXPC", code: 11, userInfo: [NSLocalizedDescriptionKey: "SDK cancellation did not stop worker publication."])
        }
        let cancellationStatus = try call { $0.inspectionStatus(cancelledID, withReply: $1) }
        guard (cancellationStatus["status"] as? [String: Any])?["phase"] as? String == "cancelled" else {
            throw NSError(domain: "SDKXPC", code: 12, userInfo: [NSLocalizedDescriptionKey: "SDK lookup did not retain cancelled status."])
        }
        let recovered = try call {
            $0.reconstructFunction(recordID, startAddress: String(start), endAddress: String(end),
                                   requestID: UUID().uuidString, withReply: $1)
        }
        let source = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        let digest = SHA256.hash(data: source).map { String(format: "%02x", $0) }.joined()
        guard let artifact = recovered["artifactJSON"] as? String,
              let data = artifact.data(using: .utf8),
              let body = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              body["source_sha256"] as? String == digest,
              (recovered["message"] as? String)?.contains("1 supported declarations were supplied") == true,
              let nodes = body["nodes"] as? [[String: Any]],
              nodes.contains(where: { node in
                  guard let payload = node["payload"] as? [String: Any],
                        let settings = payload["analysis_settings"] as? [String: Any],
                        let calls = settings["call_signature_candidates"] as? [[String: Any]] else { return false }
                  return calls.contains { $0["symbol"] as? String == "_fwrite" &&
                      ($0["declaration"] as? String)?.contains("fwrite(") == true }
              }) else {
            throw NSError(domain: "SDKXPC", code: 8, userInfo: [NSLocalizedDescriptionKey: String(describing: recovered)])
        }
        print("App/XPC SDK lookup cancellation and declaration-to-artifact reconstruction passed.")
    }
}
