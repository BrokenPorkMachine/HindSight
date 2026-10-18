import Foundation

@MainActor
final class RemoteAnalysisTransactions {
    private static let openAPI: String = { let raw = String(#"""
    {"openapi":"3.1.0","info":{"title":"HindSight Remote API","version":"1.0.0"},"servers":[{"url":"/v1"}],"security":[{"BearerAuth":[]}],"components":{"securitySchemes":{"BearerAuth":{"type":"http","scheme":"bearer"}},"schemas":{"Error":{"type":"object","required":["error"],"properties":{"error":{"type":"string"}}},"Capabilities":{"type":"object","required":["protocol","server","features","execution"],"properties":{"protocol":{"const":1},"server":{"const":"HindSight"},"features":{"type":"array","items":{"type":"string","enum":["project_read","analysis_jobs","job_poll","job_cancel"]}},"execution":{"const":false}}},"Project":{"type":"object","description":"Current open ProjectSnapshot. Records include stable record IDs, captured source SHA-256 identities, and analysis metadata.","additionalProperties":true},"AnalysisJobRequest":{"type":"object","required":["request_id","project_path","record_id","source_sha256"],"properties":{"request_id":{"type":"string","format":"uuid"},"project_path":{"type":"string","description":"Exact path returned by GET /project."},"record_id":{"type":"string","format":"uuid"},"source_sha256":{"type":"string","pattern":"^[a-fA-F0-9]{64}$"}}},"JobAccepted":{"type":"object","required":["job_id","state"],"properties":{"job_id":{"type":"string","format":"uuid"},"state":{"type":"string","enum":["accepted","existing"]}}},"JobState":{"type":"object","required":["job_id","state"],"properties":{"job_id":{"type":"string","format":"uuid"},"state":{"type":"string","enum":["running","completed","failed"]}}},"AnalysisResult":{"type":"object","description":"Source-bound AnalysisReply envelope; includes analysis.request_id and analysis.source_sha256.","additionalProperties":true}}},"paths":{"/capabilities":{"get":{"operationId":"getCapabilities","responses":{"200":{"description":"Protocol version and implemented feature set","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Capabilities"}}}}}}},"/project":{"get":{"operationId":"getCurrentProject","responses":{"200":{"description":"Current project snapshot","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Project"}}}},"409":{"description":"No ready project","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Error"}}}}}}},"/jobs/analysis":{"post":{"operationId":"submitAnalysis","description":"Submit or retry an analysis. Reusing a request ID with the same source returns the existing job for ten minutes; reuse with different input conflicts. At most two jobs run concurrently.","requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/AnalysisJobRequest"}}}},"responses":{"202":{"description":"Accepted or existing idempotent job","content":{"application/json":{"schema":{"$ref":"#/components/schemas/JobAccepted"}}}},"409":{"description":"Project, record, or captured source identity mismatch","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Error"}}}},"429":{"description":"Active or retained job limit reached","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Error"}}}}}}},"/jobs/{job_id}":{"parameters":[{"name":"job_id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}],"get":{"operationId":"getAnalysisJob","responses":{"200":{"description":"Current job state","content":{"application/json":{"schema":{"$ref":"#/components/schemas/JobState"}}}},"404":{"description":"Unknown or expired job"},"409":{"description":"Project or source changed"},"503":{"description":"Coordinator status unavailable"}}},"delete":{"operationId":"cancelAnalysisJob","responses":{"200":{"description":"Cancellation request result"}},"404":{"description":"Unknown or expired job"},"409":{"description":"Job already terminal"},"503":{"description":"Coordinator cancellation unavailable"}}}},"/jobs/{job_id}/result":{"parameters":[{"name":"job_id","in":"path","required":true,"schema":{"type":"string","format":"uuid"}}],"get":{"operationId":"getAnalysisResult","responses":{"200":{"description":"Source-bound analysis result","content":{"application/json":{"schema":{"$ref":"#/components/schemas/AnalysisResult"}}}},"404":{"description":"Unknown or expired job"},"409":{"description":"Not complete, failed, or project/source changed"}}}}}}
    """#)
        let nestedResponses = raw.replacingOccurrences(of: "\"Cancellation request result\"}},\"404\"",
                                                       with: "\"Cancellation request result\"},\"404\"")
        let phases = nestedResponses.replacingOccurrences(
            of: "\"running\",\"completed\",\"failed\"",
            with: "\"queued\",\"admitted\",\"checkingCache\",\"launching\",\"running\",\"receiving\",\"validating\",\"publishing\",\"cancelling\",\"completed\",\"failed\",\"cancelled\"")
        let progress = phases.replacingOccurrences(
            of: "\"state\":{\"type\":\"string\",\"enum\":[\"queued\",\"admitted\",\"checkingCache\",\"launching\",\"running\",\"receiving\",\"validating\",\"publishing\",\"cancelling\",\"completed\",\"failed\",\"cancelled\"]}",
            with: "\"state\":{\"type\":\"string\",\"enum\":[\"queued\",\"admitted\",\"checkingCache\",\"launching\",\"running\",\"receiving\",\"validating\",\"publishing\",\"cancelling\",\"completed\",\"failed\",\"cancelled\"]},\"operation\":{\"type\":\"string\"},\"detail\":{\"type\":\"string\"},\"progress_percent\":{\"type\":\"integer\",\"minimum\":0,\"maximum\":100},\"progress_stage\":{\"type\":\"string\"},\"elapsed_ms\":{\"type\":\"string\",\"pattern\":\"^[0-9]+$\"},\"updated_at_ms\":{\"type\":\"string\",\"pattern\":\"^[0-9]+$\"}")
        return progress.replacingOccurrences(
            of: "\"200\":{\"description\":\"Cancellation request result\"}",
            with: "\"200\":{\"description\":\"Cancellation request result\",\"content\":{\"application/json\":{\"schema\":{\"type\":\"object\",\"required\":[\"cancelled\"],\"properties\":{\"cancelled\":{\"type\":\"boolean\"}}}}}}")
    }()

    struct Backend {
        let analyze: (_ recordID: String, _ requestID: String, _ done: @escaping (Result<String, Error>) -> Void) -> Void
        let status: (_ requestID: String, _ done: @escaping (Result<String, Error>) -> Void) -> Void
        let cancel: (_ requestID: String, _ done: @escaping (Result<Bool, Error>) -> Void) -> Void
    }

    private struct Input: Decodable {
        let request_id: String
        let project_path: String
        let record_id: String
        let source_sha256: String
    }

    private struct Job {
        let id: String
        let projectPath: String
        let recordID: String
        let digest: String
        let created: Date
        var result: String?
        var failure: String?
    }

    private let backend: Backend
    private var jobs: [String: Job] = [:]
    private var generation: UInt64 = 0

    init(backend: Backend) { self.backend = backend }

    func stop() {
        generation &+= 1
        for job in jobs.values where job.result == nil && job.failure == nil {
            backend.cancel(job.id) { _ in }
        }
        jobs.removeAll()
    }

    func handle(_ request: RemoteRequest, project: ProjectSnapshot?, ready: Bool,
                reply: @escaping (RemoteResponse) -> Void) {
        jobs = jobs.filter { Date().timeIntervalSince($0.value.created) < 600 }
        if request.method == "GET", request.path == "/v1/capabilities" {
            reply(.json("{\"protocol\":1,\"server\":\"HindSight\",\"features\":[\"project_read\",\"analysis_jobs\",\"job_poll\",\"job_cancel\"],\"execution\":false}"))
            return
        }
        if request.method == "GET", request.path == "/v1/openapi.json" {
            reply(.json(Self.openAPI))
            return
        }
        if request.method == "GET", request.path == "/v1/project" {
            guard ready, let project, let data = try? JSONEncoder().encode(project) else {
                reply(.error("no open project", code: 409)); return
            }
            reply(.json(String(decoding: data, as: UTF8.self)))
            return
        }
        if request.method == "POST", request.path == "/v1/jobs/analysis" {
            guard let input = try? JSONDecoder().decode(Input.self, from: request.body),
                  let id = UUID(uuidString: input.request_id)?.uuidString,
                  UUID(uuidString: input.record_id) != nil,
                  input.source_sha256.count == 64,
                  input.source_sha256.allSatisfy({ $0.isHexDigit }),
                  let project, ready, project.path == input.project_path,
                  let record = project.records.first(where: { $0.id == input.record_id }),
                  record.result.captured, record.result.source_sha256 == input.source_sha256 else {
                reply(.error("project or captured source identity mismatch", code: 409)); return
            }
            if let previous = jobs[id] {
                guard previous.projectPath == project.path, previous.recordID == record.id,
                      previous.digest == input.source_sha256 else {
                    reply(.error("request ID reused for different source", code: 409)); return
                }
                reply(.json("{\"job_id\":\"\(id)\",\"state\":\"existing\"}", code: 202))
                return
            }
            guard jobs.values.filter({ $0.result == nil && $0.failure == nil }).count < 2 else {
                reply(.error("remote job limit reached", code: 429)); return
            }
            guard jobs.count < 256 else { reply(.error("remote job history limit reached", code: 429)); return }
            jobs[id] = Job(id: id, projectPath: project.path, recordID: record.id,
                           digest: input.source_sha256, created: Date())
            let acceptedGeneration = generation
            backend.analyze(record.id, id) { [weak self] result in
                // CoordinatorClient delivers callbacks on the main actor. Keeping
                // that handoff explicit lets test backends exercise the same path.
                Task { @MainActor in
                    guard let self, self.generation == acceptedGeneration,
                          var job = self.jobs[id] else { return }
                    switch result {
                    case .success(let text):
                        if let value = try? JSONDecoder().decode(AnalysisReply.self, from: Data(text.utf8)),
                           value.ok, let report = value.analysis,
                           report.matches(id: id, digest: job.digest) {
                            job.result = text
                        } else { job.failure = "analysis failed or source identity changed" }
                    case .failure(let error): job.failure = error.localizedDescription
                    }
                    self.jobs[id] = job
                    let completed = self.jobs.values.filter { $0.result != nil }
                        .sorted { $0.created < $1.created }
                    for old in completed.dropLast(4) {
                        self.jobs[old.id]?.result = nil
                        self.jobs[old.id]?.failure = "result expired; resubmit with a new request ID"
                    }
                }
            }
            reply(.json("{\"job_id\":\"\(id)\",\"state\":\"accepted\"}", code: 202))
            return
        }
        let parts = request.path.split(separator: "/")
        guard parts.count >= 3, parts.count <= 4, parts[0] == "v1", parts[1] == "jobs",
              let id = UUID(uuidString: String(parts[2]))?.uuidString,
              let job = jobs[id] else { reply(.error("job not found", code: 404)); return }
        guard ready, project?.path == job.projectPath,
              project?.records.first(where: { $0.id == job.recordID })?.result.source_sha256 == job.digest else {
            reply(.error("project or source changed", code: 409)); return
        }
        if request.method == "GET", parts.count == 4, parts[3] == "result" {
            if let result = job.result { reply(.json(result)) }
            else if let failure = job.failure { reply(.error(failure, code: 409)) }
            else { reply(.error("analysis not complete", code: 409)) }
            return
        }
        if request.method == "GET", parts.count == 3 {
            if job.result != nil { reply(.json("{\"job_id\":\"\(id)\",\"state\":\"completed\"}")); return }
            if job.failure != nil { reply(.json("{\"job_id\":\"\(id)\",\"state\":\"failed\"}")); return }
            backend.status(id) { result in
                switch result {
                case .success(let value):
                    guard let data = value.data(using: .utf8),
                          let statusReply = try? JSONDecoder().decode(InspectionStatusReply.self, from: data),
                          statusReply.ok, let status = statusReply.status,
                          status.request_id == id,
                          let body = try? JSONSerialization.data(withJSONObject: [
                            "job_id": id,
                            "state": status.phase.rawValue,
                            "operation": status.operation,
                            "detail": status.detail,
                            "progress_percent": status.progress_percent,
                            "progress_stage": status.progress_stage,
                            "elapsed_ms": String(status.elapsed_ms),
                            "updated_at_ms": String(status.updated_at_ms),
                            "diagnostic": status.diagnostic as Any? ?? NSNull()
                          ]) else {
                        reply(.error("invalid coordinator status", code: 503)); return
                    }
                    reply(.json(String(decoding: body, as: UTF8.self)))
                case .failure: reply(.error("status unavailable", code: 503))
                }
            }
            return
        }
        if request.method == "DELETE", parts.count == 3 {
            guard job.result == nil, job.failure == nil else { reply(.error("job already terminal", code: 409)); return }
            backend.cancel(id) { result in
                switch result {
                case .success(let cancelled): reply(.json("{\"cancelled\":\(cancelled)}"))
                case .failure: reply(.error("cancellation unavailable", code: 503))
                }
            }
            return
        }
        reply(.error("not found", code: 404))
    }
}
