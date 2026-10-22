import Foundation

@main
enum ReconstructionArtifactContract {
    static func mustReject(_ body: () throws -> Void, _ label: String) throws {
        do { try body() }
        catch { return }
        throw NSError(domain: "ReconstructionArtifactContract", code: 1,
                      userInfo: [NSLocalizedDescriptionKey: "Accepted invalid \(label)"])
    }

    static func main() throws {
        guard CommandLine.arguments.count == 5 else { throw NSError(domain: "ReconstructionArtifactContract", code: 2) }
        let json = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
        let digest = CommandLine.arguments[2]
        guard let start = UInt64(CommandLine.arguments[3], radix: 16),
              let end = UInt64(CommandLine.arguments[4], radix: 16) else { throw NSError(domain: "ReconstructionArtifactContract", code: 3) }
        let reviewContext = ReconstructionReviewContext(project: "/project", record: "record-1",
                                                        analysis: "analysis-1", digest: digest)
        guard reviewContext.matches(project: "/project", record: "record-1", analysis: "analysis-1", digest: digest),
              !reviewContext.matches(project: "/other", record: "record-1", analysis: "analysis-1", digest: digest),
              !reviewContext.matches(project: "/project", record: "record-2", analysis: "analysis-1", digest: digest),
              !reviewContext.matches(project: "/project", record: "record-1", analysis: "analysis-2", digest: digest),
              !reviewContext.matches(project: "/project", record: "record-1", analysis: "analysis-1", digest: String(repeating: "0", count: 64)) else {
            throw NSError(domain: "ReconstructionArtifactContract", code: 6,
                          userInfo: [NSLocalizedDescriptionKey: "Stale reconstruction review context was accepted"])
        }
        let accepted = try ReconstructionArtifact.decode(json, digest: digest, start: start, end: end)
        guard accepted.start == start, accepted.end == end, !accepted.evidence.isEmpty,
              !accepted.expressions.isEmpty, accepted.semanticCoverage.instructions > 0,
              accepted.semanticCoverage.modeledTrees <= accepted.semanticCoverage.instructions else {
            throw NSError(domain: "ReconstructionArtifactContract", code: 4)
        }
        let exportURL = URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent("reconstruction-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: exportURL) }
        try accepted.writeReviewExport(to: exportURL)
        guard try String(contentsOf: exportURL, encoding: .utf8) == json else {
            throw NSError(domain: "ReconstructionArtifactContract", code: 7,
                          userInfo: [NSLocalizedDescriptionKey: "Review export differs from validated artifact"])
        }
        do {
            try accepted.writeReviewExport(sourceID: "sha256:" + String(repeating: "0", count: 64), to: exportURL)
            throw NSError(domain: "ReconstructionArtifactContract", code: 8,
                          userInfo: [NSLocalizedDescriptionKey: "Export accepted a source outside the artifact"])
        } catch is ReconstructionArtifact.Invalid {}

        try mustReject({ _ = try ReconstructionArtifact.decode(json, digest: String(repeating: "0", count: 64), start: start, end: end) }, "stale source digest")
        try mustReject({ _ = try ReconstructionArtifact.decode(json, digest: digest, start: start, end: end + 4) }, "stale function bounds")

        guard var document = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              var edges = document["edges"] as? [[String: Any]] else { throw NSError(domain: "ReconstructionArtifactContract", code: 5) }
        edges.append(["id": "sha256:" + String(repeating: "0", count: 64), "source": "missing", "target": "missing"])
        document["edges"] = edges
        let malformed = String(decoding: try JSONSerialization.data(withJSONObject: document), as: UTF8.self)
        try mustReject({ _ = try ReconstructionArtifact.decode(malformed, digest: digest, start: start, end: end) }, "malformed graph")
        print("Swift reconstruction artifact identity, bounds, and graph contract passed")
    }
}
