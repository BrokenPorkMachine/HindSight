import Foundation

@main
struct AnalysisStatusContract {
    static func require(_ condition: @autoclosure () -> Bool, _ message: String) {
        if !condition() {
            FileHandle.standardError.write(Data("analysis status contract failed: \(message)\n".utf8))
            exit(1)
        }
    }

    static func main() throws {
        let phases = InspectionPhase.allCases
        require(phases == [.queued, .admitted, .checkingCache, .launching, .running, .receiving, .validating, .publishing, .cancelling, .completed, .failed, .cancelled], "phase order drift")
        require(!InspectionPhase.queued.terminal, "queued must not be terminal")
        require(InspectionPhase.queued.displayName == "Queued", "queued phase label drift")
        require(!InspectionPhase.running.terminal, "running must not be terminal")
        require(InspectionPhase.completed.terminal, "completed must be terminal")
        require(InspectionPhase.failed.terminal, "failed must be terminal")
        require(InspectionPhase.cancelled.terminal, "cancelled must be terminal")

        let status = InspectionStatus(
            request_id: "11111111-1111-1111-1111-111111111111",
            operation: "analysis",
            phase: .running,
            detail: "Worker is analyzing the captured artifact.",
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            elapsed_ms: 1_500,
            progress_percent: 44,
            progress_stage: "decode",
            diagnostic: nil
        )
        let payload = InspectionStatusReply.encode(status: status)
        let decoded = try JSONDecoder().decode(InspectionStatusReply.self, from: Data(payload.utf8))
        require(decoded.ok, "encoded status should be successful")
        require(decoded.status == status, "status JSON round-trip failed")
        require(abs((decoded.status?.elapsedSeconds ?? 0) - 1.5) < 0.001, "elapsed conversion failed")
        require(decoded.status?.phase.displayName == "Analyzing", "phase label drift")
        require(decoded.status?.progress_percent == 44, "progress percentage drift")
        require(decoded.status?.progress_stage == "decode", "progress stage drift")

        let errorPayload = InspectionStatusReply.encode(error: "No active or recent inspection matches this request.")
        let errorReply = try JSONDecoder().decode(InspectionStatusReply.self, from: Data(errorPayload.utf8))
        require(!errorReply.ok && errorReply.status == nil, "error reply must not contain status")

        print("Analysis status contract passed: \(phases.count) phases.")
    }
}
