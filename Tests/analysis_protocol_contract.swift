import Foundation

@main
struct AnalysisProtocolContract {
    static func main() throws {
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
