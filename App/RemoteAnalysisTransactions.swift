import Foundation

@MainActor
final class RemoteAnalysisTransactions {
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
                case .success(let value): reply(.json(value))
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
