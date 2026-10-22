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
                done(.success("{\"ok\":true,\"message\":\"\",\"status\":{\"request_id\":\"\(id)\",\"operation\":\"analysis\",\"phase\":\"running\",\"detail\":\"fixture progress\",\"started_at_ms\":1,\"updated_at_ms\":2,\"elapsed_ms\":1,\"progress_percent\":33,\"progress_stage\":\"fixture\",\"diagnostic\":null}}"))
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
                                    project: ProjectSnapshot?, ready: Bool = true,
                                    user: RemoteUser? = nil) async -> RemoteResponse {
        await withCheckedContinuation { continuation in
            jobs.handle(.init(method: method, path: path, body: body, user: user), project: project, ready: ready) {
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
        // The protocol-mechanics instance has no registry on purpose, so the
        // authorization path is exercised separately below.
        let jobs = RemoteAnalysisTransactions(backend: fake.backend, userRegistry: nil)
        let source = try project()
        func check(_ condition: Bool, _ message: String) {
            precondition(condition, message)
        }

        let capabilities = await response(jobs, "GET", "/v1/capabilities", project: nil)
        check(capabilities.code == 200 && capabilities.body.contains("analysis_jobs"), "capability contract")

        // App-hosted transactions require an authenticated user with an exact
        // project grant. The fixture-only transaction instance above omits a
        // registry intentionally, so it can test protocol mechanics in
        // isolation; this instance verifies the production authorization path.
        let registry = RemoteUserRegistry()
        let securedUser = registry.user(forSubject: "fixture-user")
        // A separate coordinator keeps the authorization checks independent of
        // the protocol-mechanics instance's submission counters.
        let securedFake = FakeCoordinator()
        let secured = RemoteAnalysisTransactions(backend: securedFake.backend, userRegistry: registry)
        let deniedProject = await response(secured, "GET", "/v1/project", project: source, user: securedUser)
        check(deniedProject.code == 403, "project access requires a grant")
        let deniedSubmit = await response(secured, "POST", "/v1/jobs/analysis", body: try body(id: second), project: source, user: securedUser)
        check(deniedSubmit.code == 403, "analysis submission requires a grant")
        _ = registry.grant(userID: securedUser.id, projectPath: source.path, recordID: nil,
                           capabilities: RemoteCapability.all, grantedBy: "fixture-admin")
        let allowedProject = await response(secured, "GET", "/v1/project", project: source, user: securedUser)
        check(allowedProject.code == 200, "project grant permits project snapshot")
        check(allowedProject.scope?.projectPath == source.path, "project read reports its scope")
        let otherUser = registry.user(forSubject: "fixture-other-user")
        let otherDenied = await response(secured, "GET", "/v1/project", project: source, user: otherUser)
        check(otherDenied.code == 403, "grants do not leak between users")

        // A record-scoped grant covers that record and never the whole
        // project, because a project-wide request would expose the records
        // the grant does not name.
        let recordUser = registry.user(forSubject: "fixture-record-user")
        _ = registry.grant(userID: recordUser.id, projectPath: source.path, recordID: record,
                           capabilities: RemoteCapability.all, grantedBy: "fixture-admin")
        let recordProjectDenied = await response(secured, "GET", "/v1/project", project: source, user: recordUser)
        check(recordProjectDenied.code == 403, "a record-scoped grant does not grant a project-wide read")
        let recordSubmit = await response(secured, "POST", "/v1/jobs/analysis", body: try body(id: third),
                                          project: source, user: recordUser)
        check(recordSubmit.code == 202, "a record-scoped grant covers its own record")
        check(recordSubmit.scope?.recordID == record && recordSubmit.scope?.requestID == third,
              "a submission reports the record and request it acted on")

        // A job belongs to the user that submitted it. Another user with a
        // grant for the same project cannot poll it, read its result, cancel
        // it, or even learn that it exists.
        _ = registry.grant(userID: otherUser.id, projectPath: source.path, recordID: nil,
                           capabilities: RemoteCapability.all, grantedBy: "fixture-admin")
        let foreignPoll = await response(secured, "GET", "/v1/jobs/\(third)", project: source, user: otherUser)
        check(foreignPoll.code == 404, "another user cannot poll a foreign job")
        let foreignResult = await response(secured, "GET", "/v1/jobs/\(third)/result", project: source, user: otherUser)
        check(foreignResult.code == 404, "another user cannot read a foreign result")
        let foreignCancel = await response(secured, "DELETE", "/v1/jobs/\(third)", project: source, user: otherUser)
        check(foreignCancel.code == 404, "another user cannot cancel a foreign job")
        let ownerPoll = await response(secured, "GET", "/v1/jobs/\(third)", project: source, user: recordUser)
        check(ownerPoll.code == 200, "the owner polls its own job")
        // A request ID belongs to its first user, so a retry from another
        // user is not answered with that job.
        let foreignRetry = await response(secured, "POST", "/v1/jobs/analysis", body: try body(id: third),
                                          project: source, user: otherUser)
        check(foreignRetry.code == 404, "a request ID does not cross users")
        let ownerCancel = await response(secured, "DELETE", "/v1/jobs/\(third)", project: source, user: recordUser)
        check(ownerCancel.body.contains("true"), "the owner cancels its own job")
        check(securedFake.cancellations == [third], "cancel routed to backend")
        check(fake.submits.isEmpty, "the isolated instance still starts no work")

        let openAPI = await response(jobs, "GET", "/v1/openapi.json", project: nil)
        check(openAPI.code == 200 && openAPI.body.contains("\"openapi\":\"3.1.0\""), "OpenAPI endpoint")
        var parsedOpenAPI: [String: Any]?
        do { parsedOpenAPI = try JSONSerialization.jsonObject(with: Data(openAPI.body.utf8)) as? [String: Any] }
        catch {
            let index = (error as NSError).userInfo["NSJSONSerializationErrorIndex"] as? Int ?? 0
            let start = openAPI.body.index(openAPI.body.startIndex, offsetBy: min(index, openAPI.body.count))
            let before = openAPI.body.index(start, offsetBy: -min(40, openAPI.body.distance(from: openAPI.body.startIndex, to: start)))
            let after = openAPI.body.index(start, offsetBy: min(100, openAPI.body.distance(from: start, to: openAPI.body.endIndex)))
            FileHandle.standardError.write(Data("OpenAPI JSON parse error: \(error); context=\(openAPI.body[before..<after])\n".utf8))
            preconditionFailure("OpenAPI endpoint returns valid JSON")
        }
        for path in ["/capabilities", "/project", "/jobs/analysis", "/jobs/{job_id}", "/jobs/{job_id}/result"] {
            check(openAPI.body.contains("\"\(path)\":"), "OpenAPI route \(path)")
        }
        let paths = parsedOpenAPI?["paths"] as? [String: Any]
        let analysisPath = paths?["/jobs/analysis"] as? [String: Any]
        let analysisPost = analysisPath?["post"] as? [String: Any]
        let resultPath = paths?["/jobs/{job_id}/result"] as? [String: Any]
        let resultGet = resultPath?["get"] as? [String: Any]
        check(analysisPost?["operationId"] as? String == "submitAnalysis" &&
              resultGet?["operationId"] as? String == "getAnalysisResult", "OpenAPI operation identifiers")
        // Every route that can deny a caller documents the denial, so a
        // client is never surprised by a response the contract omits.
        let projectGet = (paths?["/project"] as? [String: Any])?["get"] as? [String: Any]
        let jobGet = (paths?["/jobs/{job_id}"] as? [String: Any])?["get"] as? [String: Any]
        let jobDelete = (paths?["/jobs/{job_id}"] as? [String: Any])?["delete"] as? [String: Any]
        for (name, operation) in [("project", projectGet), ("analysis", analysisPost),
                                  ("job", jobGet), ("cancel", jobDelete), ("result", resultGet)] {
            let responses = operation?["responses"] as? [String: Any]
            check(responses?["403"] != nil, "OpenAPI documents the 403 denial for \(name)")
        }
        let components = parsedOpenAPI?["components"] as? [String: Any]
        let schemas = components?["schemas"] as? [String: Any]
        let jobState = schemas?["JobState"] as? [String: Any]
        let jobProperties = jobState?["properties"] as? [String: Any]
        let state = jobProperties?["state"] as? [String: Any]
        let phases = state?["enum"] as? [String]
        check(phases?.contains("checkingCache") == true && jobProperties?["progress_percent"] != nil,
              "OpenAPI describes job phases and progress fields")
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
        let running = await response(jobs, "GET", "/v1/jobs/\(first)", project: source)
        let runningJSON = try JSONSerialization.jsonObject(with: Data(running.body.utf8)) as? [String: Any]
        check(running.code == 200 && runningJSON?["state"] as? String == "running" &&
              runningJSON?["job_id"] as? String == first && runningJSON?["progress_percent"] as? Int == 33,
              "status polling contract")
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
