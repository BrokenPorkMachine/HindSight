import Foundation

@MainActor private final class FakeCoordinator {
    var submits: [String] = []
    var pending: [String: (Result<String, Error>) -> Void] = [:]
    var cancellations: [String] = []
    var statusFails = false

    var backend: RemoteAnalysisTransactions.Backend {
        .init(analyze: { record, id, done in
            self.submits.append(record)
            self.pending[id] = done
        }, status: { id, done in
            if self.statusFails {
                done(.failure(NSError(domain: "fixture", code: 1)))
            } else {
                done(.success("{\"ok\":true,\"status\":{\"request_id\":\"\(id)\",\"phase\":\"running\"}}"))
            }
        }, cancel: { id, done in
            self.cancellations.append(id)
            done(.success(true))
        })
    }
}

@main struct RemoteTransactionsTest {
    static let digest = String(repeating: "a", count: 64)
    static let record = "11111111-1111-4111-8111-111111111111"
    static let first = "22222222-2222-4222-8222-222222222222"
    static let second = "33333333-3333-4333-8333-333333333333"
    static let third = "44444444-4444-4444-8444-444444444444"

    static func json(_ value: Any) throws -> Data {
        try JSONSerialization.data(withJSONObject: value)
    }

    static func project(path: String = "/fixture/project", digest: String = digest) throws -> ProjectSnapshot {
        let result: [String: Any] = ["protocol": 1, "request_id": first, "kind": "header_result",
                                      "file_kind": "macho", "architecture": "aarch64", "size": "4",
                                      "header_hex": "", "captured": true, "source_sha256": digest]
        let object: [String: Any] = ["path": path, "records": [["id": record, "path": "/fixture/binary",
                                                                  "name": "fixture", "result": result,
                                                                  "note": "", "revision": 0]]]
        return try JSONDecoder().decode(ProjectSnapshot.self, from: json(object))
    }

    static func body(id: String, digest: String = digest) throws -> Data {
        try json(["request_id": id, "project_path": "/fixture/project", "record_id": record,
                  "source_sha256": digest])
    }

    static func analysis(id: String, digest: String = digest) throws -> String {
        var report: [String: Any] = [:]
        for field in AnalysisReport.jsonFields { report[field] = "" }
        report["protocol"] = 1
        report["kind"] = "analysis_result"
        report["request_id"] = id
        report["source_sha256"] = digest
        for field in ["function_count", "symbol_count", "basic_block_count", "analyzed_instructions"] {
            report[field] = 0
        }
        report["truncated"] = false
        let envelope: [String: Any] = ["ok": true, "message": "done", "analysis": report]
        return String(decoding: try json(envelope), as: UTF8.self)
    }

    @MainActor static func response(_ jobs: RemoteAnalysisTransactions, _ method: String,
                                    _ path: String, body: Data = Data(),
                                    project: ProjectSnapshot?, ready: Bool = true) async -> RemoteResponse {
        await withCheckedContinuation { continuation in
            jobs.handle(.init(method: method, path: path, body: body), project: project, ready: ready) {
                continuation.resume(returning: $0)
            }
        }
    }

    @MainActor static func waitForState(_ jobs: RemoteAnalysisTransactions, id: String,
                                        expected: String, project: ProjectSnapshot) async throws {
        for _ in 0..<100 {
            let current = await response(jobs, "GET", "/v1/jobs/\(id)", project: project)
            if current.body.contains("\"state\":\"\(expected)\"") { return }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        preconditionFailure("job did not reach \(expected)")
    }

    @MainActor static func main() async throws {
        let fake = FakeCoordinator()
        let jobs = RemoteAnalysisTransactions(backend: fake.backend)
        let source = try project()
        func check(_ condition: Bool, _ message: String) {
            precondition(condition, message)
        }

        let capabilities = await response(jobs, "GET", "/v1/capabilities", project: nil)
        check(capabilities.code == 200 && capabilities.body.contains("analysis_jobs"), "capability contract")
        check(await response(jobs, "GET", "/v1/project", project: nil).code == 409, "no project")
        check(await response(jobs, "GET", "/v1/project", project: source).code == 200, "project read")
        check(await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: first),
                             project: source, ready: false).code == 409, "session readiness")
        check(await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: first, digest: String(repeating: "b", count: 64)),
                             project: source).code == 409, "stale source submission")
        check(fake.submits.isEmpty, "rejected submissions must not start work")

        let accepted = await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: first), project: source)
        check(accepted.code == 202 && accepted.body.contains("accepted"), "job admission")
        let retry = await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: first), project: source)
        check(retry.code == 202 && retry.body.contains("existing") && fake.submits.count == 1,
              "retry must not repeat analysis")
        check(await response(jobs, "GET", "/v1/jobs/\(first)/result", project: source).code == 409,
              "result cannot publish before completion")
        check(await response(jobs, "GET", "/v1/jobs/\(first)", project: source).body.contains("running"),
              "status polling")
        fake.statusFails = true
        check(await response(jobs, "GET", "/v1/jobs/\(first)", project: source).code == 503,
              "status failure")
        fake.statusFails = false
        check(await response(jobs, "GET", "/v1/jobs/\(first)", project: try project(path: "/fixture/other")).code == 409,
              "project switch must invalidate job access")

        let acceptedSecond = await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: second), project: source)
        check(acceptedSecond.code == 202, "second admission")
        check(await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: third),
                             project: source).code == 429, "active job limit")
        fake.pending.removeValue(forKey: first)?(.success(try analysis(id: first, digest: String(repeating: "b", count: 64))))
        try await waitForState(jobs, id: first, expected: "failed", project: source)
        check(await response(jobs, "GET", "/v1/jobs/\(first)/result", project: source).code == 409,
              "forged source result must not publish")

        fake.pending.removeValue(forKey: second)?(.success(try analysis(id: second)))
        try await waitForState(jobs, id: second, expected: "completed", project: source)
        let published = await response(jobs, "GET", "/v1/jobs/\(second)/result", project: source)
        check(published.code == 200 && published.body.contains(second), "verified result publication")
        check(await response(jobs, "GET", "/v1/project", project: source, ready: false).code == 409,
              "disconnected project must not be exposed")
        check(await response(jobs, "GET", "/v1/jobs/\(second)/result", project: source, ready: false).code == 409,
              "disconnected result must not be exposed")
        check(await response(jobs, "DELETE", "/v1/jobs/\(second)", project: source).code == 409,
              "terminal job cannot be cancelled")

        check(await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: third),
                             project: source).code == 202, "admit after terminal work")
        check(await response(jobs, "DELETE", "/v1/jobs/\(third)", project: source).body.contains("true"),
              "cancel active job")
        check(fake.cancellations == [third], "cancel routed to backend")
        let oldCompletion = fake.pending[third]
        jobs.stop()
        check(fake.cancellations == [third, third], "stop revokes active work")
        check(await response(jobs, "GET", "/v1/jobs/\(third)", project: source).code == 404,
              "stop removes job access")
        check(await response(jobs, "POST", "/v1/jobs/analysis", body: try body(id: third),
                             project: source).code == 202, "restart admits a fresh transaction")
        oldCompletion?(.success(try analysis(id: third)))
        try await Task.sleep(nanoseconds: 20_000_000)
        check(await response(jobs, "GET", "/v1/jobs/\(third)/result", project: source).code == 409,
              "late completion from revoked session must not publish")
        fake.pending.removeValue(forKey: third)?(.success(try analysis(id: third)))
        try await waitForState(jobs, id: third, expected: "completed", project: source)
        print("Remote analysis transactional tests passed.")
    }
}
