import Foundation
import CryptoKit

@main
struct InvestigationLedgerContract {
    static func main() throws {
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        var ledger = VGInvestigationLedger(projectName: "Case Alpha", caseID: "case-1", now: now)
        let context = VGLedgerContext(recordID: "r1", recordName: "target", sourceSHA256: "abc", workspace: "Binary Workspace", address: "0x1000", symbol: "entry", targetID: nil)
        ledger.append(VGLedgerEntry(timestamp: now, kind: .analysisCompleted, provenance: .derived, detail: "3 functions", context: context))
        let h = ledger.addHypothesis(title: "Auth branch controls access", detail: "Need runtime confirmation", address: "0x1000", linkedEntryID: ledger.entries.last?.id, now: now)
        precondition(h.status == .proposed)
        let updated = ledger.setHypothesisStatus(id: h.id, status: .supported, now: now.addingTimeInterval(1))
        precondition(updated?.status == .supported)
        ledger.addBookmark(VGCaseBookmark(timestamp: now, title: "Interesting branch", note: "Follow x19", workspace: "Binary Workspace", address: "0x1000", symbol: "entry", recordID: "r1"))
        ledger.addSnapshot(VGInvestigationSnapshot(timestamp: now, title: "Before stepping", workspace: "Debugger", layoutPreset: "Dynamic Analysis", address: "0x1000", cfgFocus: "0x1000", analysisSearch: "entry", selectedSymbol: "entry", selectedRecordID: "r1", selectedTargetID: "", evidence: "branch"))
        precondition(ledger.entries.count == 1)
        precondition(ledger.hypotheses.count == 1)
        precondition(ledger.bookmarks.count == 1)
        precondition(ledger.snapshots.count == 1)

        let reaResponse = "{\"ok\":true,\"method\":\"binary_overview\"}"
        let reaDigest = SHA256.hash(data: Data(reaResponse.utf8)).map { String(format: "%02x", $0) }.joined()
        let bundle = VGCaseBundle(
            format: "com.zensoftware.vectorglass.case",
            schema: 1,
            product: "HindSight",
            productVersion: "0.4.3",
            exportedAt: now,
            ledger: ledger,
            artifacts: [VGCaseArtifactIdentity(id: "r1", name: "target", sourceSHA256: "abc", architecture: "arm64", size: "4096")],
            evidence: nil,
            runtime: [],
            memory: [],
            reaEvidence: [VGREAAgentEvidence(id: "rea-1", capturedAt: now, operation: "binary_overview", targetPath: "/tmp/target", provider: "hopper", responseSHA256: reaDigest, response: reaResponse, evidenceID: "ev_fixture", targetSHA256: String(repeating: "a", count: 64), analysisProfileDigest: String(repeating: "b", count: 64))]
        )
        let data = try VGLedgerStore.encodeCase(bundle)
        let boundedCase = try VGLedgerStore.decodeCase(data)
        precondition(boundedCase.format == "com.zensoftware.vectorglass.case")
        let decoder = JSONDecoder(); decoder.dateDecodingStrategy = .iso8601
        let decoded = try decoder.decode(VGCaseBundle.self, from: data)
        precondition(decoded.ledger.caseID == "case-1")
        precondition(decoded.ledger.hypotheses[0].status == .supported)
        precondition(decoded.reaEvidence?.first?.responseSHA256 == reaDigest)
        precondition(decoded.reaEvidence?.first?.evidenceID == "ev_fixture")
        let markdown = VGLedgerStore.markdown(bundle)
        precondition(markdown.contains("Auth branch controls access"))
        precondition(markdown.contains("Evidence boundary"))
        let pdf = VGLedgerStore.pdf(bundle)
        precondition(String(decoding: pdf.prefix(8), as: UTF8.self).hasPrefix("%PDF-1.4"))
        precondition(String(decoding: pdf.suffix(16), as: UTF8.self).contains("%%EOF"))

        // Beta persistence contract: keep a last-known-good backup and recover from a
        // damaged primary instead of silently discarding the case history.
        let tempRoot = FileManager.default.temporaryDirectory.appendingPathComponent("vectorglass-ledger-contract-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempRoot, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempRoot) }
        try VGLedgerStore.save(ledger, projectPath: tempRoot.path)
        var second = ledger
        second.append(VGLedgerEntry(timestamp: now.addingTimeInterval(2), kind: .noteSaved, provenance: .asserted, detail: "second save", context: context))
        try VGLedgerStore.save(second, projectPath: tempRoot.path)
        try Data("{not-json".utf8).write(to: VGLedgerStore.ledgerURL(projectPath: tempRoot.path), options: .atomic)
        let recovered = try VGLedgerStore.loadRecovering(projectPath: tempRoot.path, projectName: "Case Alpha")
        precondition(recovered.recoveredFromBackup)
        precondition(recovered.ledger.entries.count == ledger.entries.count)

        // Untrusted/shared projects must not redirect case writes through a symlink.
        let symlinkRoot = FileManager.default.temporaryDirectory.appendingPathComponent("vectorglass-ledger-symlink-\(UUID().uuidString)", isDirectory: true)
        let outside = FileManager.default.temporaryDirectory.appendingPathComponent("vectorglass-ledger-outside-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: symlinkRoot, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: symlinkRoot); try? FileManager.default.removeItem(at: outside) }
        try FileManager.default.createSymbolicLink(at: symlinkRoot.appendingPathComponent(".vectorglass"), withDestinationURL: outside)
        do {
            try VGLedgerStore.save(ledger, projectPath: symlinkRoot.path)
            preconditionFailure("ledger save unexpectedly followed a symbolic-link storage directory")
        } catch { }

        print("Investigation ledger contract passed: \(decoded.ledger.entries.count) timeline / \(decoded.ledger.hypotheses.count) hypothesis / \(decoded.ledger.snapshots.count) snapshot / \(pdf.count) PDF bytes")
    }
}
