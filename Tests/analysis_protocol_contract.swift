import Foundation

@main
struct AnalysisProtocolContract {
    static func main() throws {
        if CommandLine.arguments.count == 3 {
            let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
            let report = try JSONDecoder().decode(AnalysisReport.self, from: data)
            precondition(data.count <= WorkbenchProtocolLimits.analysisWorkerFrameBytes)
            precondition(report.validationFailure(id: report.request_id, digest: CommandLine.arguments[2]) == nil,
                         "Real worker report failed the native identity/section contract")
            print("Real worker analysis decoded and passed native identity/section bounds (\(data.count) bytes).")
            return
        }
        precondition(WorkbenchProtocolLimits.controlWorkerFrameBytes == 256 * 1024)
        precondition(WorkbenchProtocolLimits.analysisWorkerFrameBytes == 8 * 1024 * 1024)
        precondition(WorkbenchProtocolLimits.defaultXPCReplyBytes == 2 * 1024 * 1024)
        precondition(WorkbenchProtocolLimits.analysisXPCReplyBytes == 12 * 1024 * 1024)
        precondition(AnalysisReport.jsonFields.count == 75)

        let requestID = "00000000-0000-0000-0000-000000000000"
        let digest = String(repeating: "a", count: 64)
        let numeric: Set<String> = ["protocol", "function_count", "symbol_count", "basic_block_count", "analyzed_instructions"]
        var object: [String: Any] = [:]
        let evidence = String(repeating: "evidence line\\n", count: 800)
        for key in AnalysisReport.jsonFields {
            if numeric.contains(key) { object[key] = key == "protocol" ? 1 : 7 }
            else if key == "truncated" { object[key] = false }
            else { object[key] = evidence }
        }
        object["kind"] = "analysis_result"
        object["request_id"] = requestID
        object["source_sha256"] = digest
        object["summary"] = "bounded modern analysis report"

        let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        precondition(data.count > WorkbenchProtocolLimits.controlWorkerFrameBytes,
                     "fixture must prove analysis can exceed the control frame")
        precondition(data.count < WorkbenchProtocolLimits.analysisWorkerFrameBytes)
        precondition(Set(object.keys) == AnalysisReport.jsonFields)
        let report = try JSONDecoder().decode(AnalysisReport.self, from: data)
        precondition(report.matches(id: requestID, digest: digest))
        precondition(report.validationFailure(id: "11111111-1111-1111-1111-111111111111", digest: digest) == "request ID mismatch")
        precondition(report.validationFailure(id: requestID, digest: String(repeating: "b", count: 64)) == "captured-source digest mismatch")
        var oversized = object
        oversized["functions"] = String(repeating: "x", count: 65536)
        let oversizedReport = try JSONDecoder().decode(AnalysisReport.self,
            from: JSONSerialization.data(withJSONObject: oversized, options: [.sortedKeys]))
        precondition(oversizedReport.validationFailure(id: requestID, digest: digest) ==
                    "analysis section 'functions' exceeds its 65535-byte limit (65536 bytes)")
        let reboundID = "11111111-1111-1111-1111-111111111111"
        let rebound = report.forRequest(reboundID)
        precondition(rebound?.matches(id: reboundID, digest: digest) == true)
        precondition(rebound?.source_sha256 == report.source_sha256)

        var unexpected = object
        unexpected["legacy_or_unexpected"] = true
        precondition(Set(unexpected.keys) != AnalysisReport.jsonFields)

        let reply = AnalysisReply.encode(analysis: report, message: "Analysis completed.")
        precondition(reply.utf8.count <= WorkbenchProtocolLimits.analysisXPCReplyBytes)
        let decoded = try JSONDecoder().decode(AnalysisReply.self, from: Data(reply.utf8))
        precondition(decoded.ok && decoded.analysis?.request_id == requestID)
        print("analysis protocol contract: \(AnalysisReport.jsonFields.count) fields, \(data.count) worker bytes")
    }
}
