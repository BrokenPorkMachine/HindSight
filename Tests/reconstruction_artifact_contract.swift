import Foundation
import CryptoKit

@main
enum ReconstructionArtifactContract {
    static func mustReject(_ body: () throws -> Void, _ label: String) throws {
        do { try body() }
        catch { return }
        throw NSError(domain: "ReconstructionArtifactContract", code: 1,
                      userInfo: [NSLocalizedDescriptionKey: "Accepted invalid \(label)"])
    }

    static func main() throws {
        let mixed = try ReconstructionBatchOutcome(states: ["recovered", "review-only", "failed"], planned: 3, cancelled: false)
        guard mixed.status == "completed-with-failures", mixed.recovered == 1, mixed.reviewOnly == 1,
              mixed.failed == 1, mixed.attempted == 3 else { throw ReconstructionArtifact.Invalid(message: "Incorrect mixed batch outcome") }
        guard try ReconstructionBatchOutcome(states: [], planned: 3, cancelled: false).status == "incomplete",
              try ReconstructionBatchOutcome(states: ["review-only"], planned: 1, cancelled: false).status == "completed",
              try ReconstructionBatchOutcome(states: ["recovered"], planned: 1, cancelled: true).status == "cancelled" else {
            throw ReconstructionArtifact.Invalid(message: "Incorrect partial/cancelled batch outcome")
        }
        try mustReject({ _ = try ReconstructionBatchOutcome(states: ["unknown"], planned: 1, cancelled: false) }, "unknown outcome state")
        try mustReject({ _ = try ReconstructionBatchOutcome(states: ["failed"], planned: 0, cancelled: false) }, "outcome exceeds plan")
        let headerSource = ReconstructionSource(id: "header", kind: "recovered-header", text: "header_sentinel", status: "declaration only", contract: "test")
        let bodySource = ReconstructionSource(id: "body", kind: "recovered-source", text: "body_sentinel", status: "generated", contract: "test")
        let machineSource = ReconstructionSource(id: "machine", kind: "machine-state-c", text: "machine_sentinel", status: "generated", contract: "test")
        let formatSources = [headerSource, bodySource, machineSource]
        let objcSource = ReconstructionSource(id: "objc", kind: "recovered-objective-c", text: "@implementation Recovered", status: "generated", contract: "synthetic adapter")
        guard ReconstructionExportFormat.objc.source(in: formatSources + [objcSource])?.id == objcSource.id,
              ReconstructionExportFormat.objc.render(objcSource) == objcSource.text,
              ReconstructionExportFormat.c.render(objcSource) == nil else {
            throw ReconstructionArtifact.Invalid(message: "Objective-C code selection lost its representation")
        }
        for format in ReconstructionExportFormat.allCases {
            let expected = format == .header ? headerSource : format == .native ? machineSource : bodySource
            let fallback = [ReconstructionExportFormat.c, .objc].contains(format) ? machineSource : nil
            guard format.source(in: formatSources)?.id == expected.id,
                  format.source(in: Array(formatSources.reversed()))?.id == expected.id,
                  format.render(expected)?.contains(expected.text) == true,
                  format.source(in: formatSources.filter { $0.kind != expected.kind })?.id == fallback?.id,
                  format.render(format == .header ? bodySource : headerSource) == nil else {
                throw ReconstructionArtifact.Invalid(message: "Reconstruction export substituted another artifact kind.")
            }
        }
        let reviewBody = ReconstructionSource(id: "review-code", kind: "best-effort-source", text: "loc_1000: x0 = x1;", status: "review", contract: "ABI unresolved")
        let reviewHeader = ReconstructionSource(id: "review-header", kind: "best-effort-header", text: "void hs_review_1000(void);", status: "review", contract: "not original ABI")
        guard ReconstructionExportFormat.c.source(in: [reviewBody])?.id == reviewBody.id,
              ReconstructionExportFormat.c.source(in: [reviewBody, machineSource])?.id == machineSource.id,
              ReconstructionExportFormat.header.source(in: [reviewHeader])?.id == reviewHeader.id,
              ReconstructionExportFormat.c.render(reviewBody) == reviewBody.text else {
            throw ReconstructionArtifact.Invalid(message: "Best-effort code selection lost code or qualification precedence")
        }
        guard ReconstructionExportFormat.native.fileExtension == "n",
              ReconstructionExportFormat.native.allowsCustomExtension,
              !ReconstructionExportFormat.c.allowsCustomExtension,
              ReconstructionExportFormat.header.fileExtension == "h",
              ReconstructionExportFormat.objc.fileExtension == "m",
              ReconstructionExportFormat.objcxx.fileExtension == "mm",
              ReconstructionExportFormat(rawValue: "unknown") == nil,
              ReconstructionExportFormat.cpp.render(bodySource)?.contains("// body_sentinel") == true,
              ReconstructionExportFormat.swift.render(bodySource)?.contains("translation to the selected language is unresolved") == true else {
            throw ReconstructionArtifact.Invalid(message: "Incorrect reconstruction format or translation claim.")
        }
        guard ReconstructionScopePlan.address(" 0xABcd ") == 43981,
              ReconstructionScopePlan.address("18446744073709551615") == UInt64.max,
              ReconstructionScopePlan.address("0") == 0,
              ["", "0x", "-1", "+1", "1.5", "0x10000000000000000", "18446744073709551616", "1 2", "FF"].allSatisfy({ ReconstructionScopePlan.address($0) == nil }) else {
            throw ReconstructionArtifact.Invalid(message: "Invalid reconstruction address parsing.")
        }
        let scopeFunctions = [ReconstructionRange(start: 8, end: 16),
                              ReconstructionRange(start: 16, end: 24),
                              ReconstructionRange(start: 24, end: 36),
                              ReconstructionRange(start: 40, end: 44)]
        let section = ReconstructionScopePlan.section(address: "16", words: "4", unitBytes: 4)
        guard section == ReconstructionRange(start: 16, end: 32),
              try ReconstructionScopePlan.indices(scope: "section", address: nil, section: section, functions: scopeFunctions) == [1],
              try ReconstructionScopePlan.indices(scope: "address", address: 16, section: nil, functions: scopeFunctions) == [1],
              try ReconstructionScopePlan.indices(scope: "whole", address: nil, section: nil, functions: scopeFunctions) == [0,1,2,3],
              ReconstructionScopePlan.section(address: "16", words: "0", unitBytes: 4) == nil,
              ReconstructionScopePlan.section(address: "16", words: "4", unitBytes: 2) == nil,
              ReconstructionScopePlan.section(address: String(UInt64.max - 3), words: "1", unitBytes: 4) == nil,
              ReconstructionScopePlan.section(address: "invalid", words: "4", unitBytes: 4) == nil else {
            throw ReconstructionArtifact.Invalid(message: "Reconstruction scope selection disagrees with exact bounds.")
        }
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "section", address: nil, section: nil, functions: scopeFunctions) }, "section without bounds")
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "address", address: 44, section: nil, functions: scopeFunctions) }, "exclusive function end")
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "address", address: nil, section: nil, functions: scopeFunctions) }, "missing address")
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "address", address: 20, section: nil,
            functions: scopeFunctions + [ReconstructionRange(start: 18, end: 28)]) }, "ambiguous address")
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "whole", address: nil, section: nil, functions: scopeFunctions, maximum: 3) }, "silent scope truncation")
        try mustReject({ _ = try ReconstructionScopePlan.indices(scope: "unknown", address: nil, section: nil, functions: scopeFunctions) }, "unknown scope")
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

        let batch = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("reconstruction-batch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: batch, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: batch) }
        try mustReject({ _ = try accepted.writeBatchExport(to: batch, maximumBytes: 0) }, "batch byte budget")
        guard try FileManager.default.contentsOfDirectory(atPath: batch.path).isEmpty else {
            throw ReconstructionArtifact.Invalid(message: "Budget rejection wrote partial output")
        }
        let files = try accepted.writeBatchExport(to: batch, maximumBytes: 16 * 1024 * 1024)
        guard files.count == accepted.sources.count + 1 else {
            throw ReconstructionArtifact.Invalid(message: "Batch omitted retained sources")
        }
        for file in files {
            let data = try Data(contentsOf: batch.appendingPathComponent(file.path))
            let hash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard data.count == file.bytes && hash == file.sha256 else {
                throw ReconstructionArtifact.Invalid(message: "Batch file identity differs from its bytes")
            }
        }
        try mustReject({ _ = try accepted.writeBatchExport(to: batch, maximumBytes: 16 * 1024 * 1024) },
                       "batch overwrite")
        let alias = batch.appendingPathExtension("link")
        try FileManager.default.createSymbolicLink(at: alias, withDestinationURL: batch)
        defer { try? FileManager.default.removeItem(at: alias) }
        try mustReject({ _ = try accepted.writeBatchExport(to: alias, maximumBytes: 16 * 1024 * 1024) },
                       "symlink batch directory")
        let fileObjects = try JSONSerialization.jsonObject(with: JSONEncoder().encode(files))
        let checkpoint: [String: Any] = [
            "schema": 1, "source_sha256": digest, "worker": "test-worker",
            "revision": "test-revision", "pass": ReconstructionArtifact.passVersion,
            "output_bytes": files.reduce(0) { $0 + $1.bytes },
            "functions": [["start": String(format: "0x%016llx", start),
                           "end": String(format: "0x%016llx", end),
                           "state": accepted.sources.contains { $0.kind == "recovered-source" } ? "recovered" : "review-only",
                           "files": fileObjects]]
        ]
        let checkpointData = try JSONSerialization.data(withJSONObject: checkpoint)
        try checkpointData.write(to: batch.appendingPathComponent("checkpoint.json"))
        let verified = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
            worker: "test-worker", revision: "test-revision")
        guard verified == checkpointData else { throw ReconstructionArtifact.Invalid(message: "Checkpoint changed") }
        var countedCheckpoint = checkpoint
        countedCheckpoint["planned"] = [["start": String(format: "0x%016llx", start), "end": String(format: "0x%016llx", end)]]
        countedCheckpoint["cancelled"] = false
        let state = accepted.sources.contains { $0.kind == "recovered-source" } ? "recovered" : "review-only"
        let countedOutcome = try ReconstructionBatchOutcome(states: [state], planned: 1, cancelled: false)
        countedCheckpoint.merge(countedOutcome.metadata) { _, value in value }
        try JSONSerialization.data(withJSONObject: countedCheckpoint).write(to: batch.appendingPathComponent("checkpoint.json"))
        _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest, worker: "test-worker", revision: "test-revision")
        for key in ["batch_status", "recovered_functions", "review_only_functions", "failed_functions"] {
            var forged = countedCheckpoint
            forged[key] = key == "batch_status" ? "completed-with-failures" : 123
            try JSONSerialization.data(withJSONObject: forged).write(to: batch.appendingPathComponent("checkpoint.json"))
            try mustReject({ _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
                worker: "test-worker", revision: "test-revision") }, "forged outcome \(key)")
        }
        for invalidFormat: Any in ["unsupported-format", 42] {
            var invalidCheckpoint = checkpoint
            invalidCheckpoint["requested_format"] = invalidFormat
            try JSONSerialization.data(withJSONObject: invalidCheckpoint).write(to: batch.appendingPathComponent("checkpoint.json"))
            try mustReject({ _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
                worker: "test-worker", revision: "test-revision") }, "invalid saved dump intent")
        }
        try checkpointData.write(to: batch.appendingPathComponent("checkpoint.json"))
        for format in ReconstructionExportFormat.allCases {
            let dump = batch.appendingPathComponent("dump-" + format.rawValue, isDirectory: true)
            let manifestData = try ReconstructionArtifact.writeBatchFormatExport(from: batch, to: dump,
                format: format, digest: digest, worker: "test-worker", revision: "test-revision")
            let manifest = try JSONSerialization.jsonObject(with: manifestData) as! [String: Any]
            let expected = format.source(in: accepted.sources)
            guard manifest["failed_functions"] as? Int == 0 else { throw ReconstructionArtifact.Invalid(message: "Dump invented failed functions") }
            guard manifest["available"] as? Int == (expected == nil ? 0 : 1),
                  manifest["unavailable"] as? Int == (expected == nil ? 1 : 0),
                  let dumped = manifest["functions"] as? [[String: Any]],
                  let retainedFiles = dumped.first?["files"] as? [[String: Any]] else {
                throw ReconstructionArtifact.Invalid(message: "Dump format availability is incorrect")
            }
            guard dumped.first?["languages"] as? [String] == (expected?.languages ?? []) else {
                throw ReconstructionArtifact.Invalid(message: "Dump lost the source language contract")
            }
            for file in retainedFiles {
                let name = file["path"] as! String
                let data = try Data(contentsOf: dump.appendingPathComponent(name))
                let hash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
                guard data.count == file["bytes"] as? Int, hash == file["sha256"] as? String else {
                    throw ReconstructionArtifact.Invalid(message: "Dump output hash or size changed")
                }
                if name.hasSuffix(".reconstruction.json") {
                    guard String(decoding: data, as: UTF8.self) == accepted.json else {
                        throw ReconstructionArtifact.Invalid(message: "Dump omitted exact reconstruction evidence")
                    }
                } else {
                    guard let expected, String(decoding: data, as: UTF8.self) == format.render(expected),
                          name.hasSuffix("." + format.fileExtension) else {
                        throw ReconstructionArtifact.Invalid(message: "Dump substituted an unavailable source kind")
                    }
                }
            }
            try mustReject({ _ = try ReconstructionArtifact.writeBatchFormatExport(from: batch, to: dump,
                format: format, digest: digest, worker: "test-worker", revision: "test-revision") }, "overwrite format dump")
        }
        let allDump = batch.appendingPathComponent("all-formats", isDirectory: true)
        let allData = try ReconstructionArtifact.writeBatchAllFormatExport(from: batch, to: allDump,
            digest: digest, worker: "test-worker", revision: "test-revision")
        let allManifest = try JSONSerialization.jsonObject(with: allData) as! [String: Any]
        let formats = allManifest["formats"] as! [[String: Any]]
        guard formats.compactMap({ $0["folder"] as? String }) == ["c", "h", "m", "n"] else {
            throw ReconstructionArtifact.Invalid(message: "Dump-all folders changed")
        }
        for item in formats {
            let folder = item["folder"] as! String
            let actual = try Data(contentsOf: allDump.appendingPathComponent(folder + "/export-manifest.json"))
            let expectedFolder = ["c": "c", "h": "header", "m": "objc", "n": "native"][folder]!
            let expected = try Data(contentsOf: batch.appendingPathComponent("dump-" + expectedFolder + "/export-manifest.json"))
            guard actual == expected,
                  SHA256.hash(data: actual).map({ String(format: "%02x", $0) }).joined() == item["manifest_sha256"] as? String else {
                throw ReconstructionArtifact.Invalid(message: "Dump-all differs from individual output")
            }
        }
        try mustReject({ _ = try ReconstructionArtifact.writeBatchAllFormatExport(from: batch, to: allDump,
            digest: digest, worker: "test-worker", revision: "test-revision") }, "overwrite dump-all")
        let limitedAll = batch.appendingPathComponent("all-limited", isDirectory: true)
        try mustReject({ _ = try ReconstructionArtifact.writeBatchAllFormatExport(from: batch, to: limitedAll,
            digest: digest, worker: "test-worker", revision: "test-revision", maximumBytes: 1) }, "dump-all aggregate byte budget")
        guard !FileManager.default.fileExists(atPath: limitedAll.appendingPathComponent("export-manifest.json").path) else {
            throw ReconstructionArtifact.Invalid(message: "Incomplete dump-all claimed completion")
        }
        var allCheckpoint = checkpoint
        allCheckpoint["requested_format"] = ""
        allCheckpoint["requested_dump_all"] = true
        try JSONSerialization.data(withJSONObject: allCheckpoint).write(to: batch.appendingPathComponent("checkpoint.json"))
        let restoredAll = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
            worker: "test-worker", revision: "test-revision")
        guard (try JSONSerialization.jsonObject(with: restoredAll) as? [String: Any])?["requested_dump_all"] as? Bool == true else {
            throw ReconstructionArtifact.Invalid(message: "Checkpoint lost dump-all intent")
        }
        for value: Any in ["yes", true] {
            var forged = checkpoint
            forged["requested_format"] = "c"
            forged["requested_dump_all"] = value
            try JSONSerialization.data(withJSONObject: forged).write(to: batch.appendingPathComponent("checkpoint.json"))
            try mustReject({ _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
                worker: "test-worker", revision: "test-revision") }, "conflicting dump-all intent")
        }
        try checkpointData.write(to: batch.appendingPathComponent("checkpoint.json"))
        let rejectedDump = batch.appendingPathComponent("dump-over-budget", isDirectory: true)
        try mustReject({ _ = try ReconstructionArtifact.writeBatchFormatExport(from: batch, to: rejectedDump,
            format: .c, digest: digest, worker: "test-worker", revision: "test-revision", maximumBytes: 1) }, "dump budget")
        guard !FileManager.default.fileExists(atPath: rejectedDump.path) else {
            throw ReconstructionArtifact.Invalid(message: "Budget refusal published a partial dump")
        }
        try mustReject({ _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
            worker: "other-worker", revision: "test-revision") }, "changed batch worker")
        let tampered = batch.appendingPathComponent(files[0].path)
        var changed = try Data(contentsOf: tampered)
        changed.append(contentsOf: Data("changed".utf8))
        try changed.write(to: tampered)
        try mustReject({ _ = try ReconstructionArtifact.verifiedBatchCheckpoint(at: batch, digest: digest,
            worker: "test-worker", revision: "test-revision") }, "changed retained artifact")

        try mustReject({ _ = try ReconstructionArtifact.decode(json, digest: String(repeating: "0", count: 64), start: start, end: end) }, "stale source digest")
        try mustReject({ _ = try ReconstructionArtifact.decode(json, digest: digest, start: start, end: end + 4) }, "stale function bounds")
        var stale = try JSONSerialization.jsonObject(with: Data(json.utf8)) as! [String: Any]
        var staleScope = stale["scope"] as! [String: Any]
        staleScope["pass_version"] = "arm64-reconstruction-54"
        stale["scope"] = staleScope
        let staleJSON = String(decoding: try JSONSerialization.data(withJSONObject: stale), as: UTF8.self)
        try mustReject({ _ = try ReconstructionArtifact.decode(staleJSON, digest: digest, start: start, end: end) },
                       "earlier reconstruction pass")

        guard var document = try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              var edges = document["edges"] as? [[String: Any]] else { throw NSError(domain: "ReconstructionArtifactContract", code: 5) }
        edges.append(["id": "sha256:" + String(repeating: "0", count: 64), "source": "missing", "target": "missing"])
        document["edges"] = edges
        let malformed = String(decoding: try JSONSerialization.data(withJSONObject: document), as: UTF8.self)
        try mustReject({ _ = try ReconstructionArtifact.decode(malformed, digest: digest, start: start, end: end) }, "malformed graph")
        print("Swift reconstruction artifact identity, bounds, and graph contract passed")
    }
}
