import Foundation
import Darwin

/// Queue-confined supervisor for one bounded capture worker. No arbitrary executable API.
final class WorkerSupervisor {
    private final class Job {
        let id: String
        let process: Process
        let lease: UInt64
        let globalLockFD: Int32
        private var resourceAdmission: UUID?
        let completion: (Result<Data, Error>) -> Void
        let operation: String
        let startedAt: Date
        var phase: InspectionPhase = .admitted
        var detail = "Worker admission granted."
        var updatedAt = Date()
        var progressPercent = 0
        var progressStage = "admission"
        var diagnostic: String?
        var stopReason: String?
        init(id: String, operation: String, process: Process, lease: UInt64, globalLockFD: Int32, resourceAdmission: UUID?, startedAt: Date, completion: @escaping (Result<Data, Error>) -> Void) {
            self.id = id; self.operation = operation; self.process = process; self.lease = lease; self.globalLockFD = globalLockFD; self.resourceAdmission = resourceAdmission; self.startedAt = startedAt; self.completion = completion
        }
        func update(_ phase: InspectionPhase, _ detail: String, progressPercent: Int? = nil, progressStage: String? = nil, diagnostic: String? = nil) {
            self.phase = phase; self.detail = detail; self.updatedAt = Date()
            if let progressPercent { self.progressPercent = min(100, max(0, progressPercent)) }
            if let progressStage, !progressStage.isEmpty { self.progressStage = String(progressStage.prefix(48)) }
            if let diagnostic, !diagnostic.isEmpty { self.diagnostic = diagnostic }
        }
        func snapshot(now: Date = Date()) -> InspectionStatus {
            let startMS = UInt64(max(0, startedAt.timeIntervalSince1970 * 1000.0))
            let updateMS = UInt64(max(0, updatedAt.timeIntervalSince1970 * 1000.0))
            let elapsedMS = UInt64(max(0, now.timeIntervalSince(startedAt) * 1000.0))
            return InspectionStatus(request_id: id, operation: operation, phase: phase, detail: detail, started_at_ms: startMS, updated_at_ms: updateMS, elapsed_ms: elapsedMS, progress_percent: progressPercent, progress_stage: progressStage, diagnostic: diagnostic)
        }
        func releaseResources() {
            if let resourceAdmission { SharedResourceLedger.shared.release(resourceAdmission); self.resourceAdmission = nil }
        }
        deinit {
            releaseResources()
            if globalLockFD >= 0 { flock(globalLockFD, LOCK_UN); Darwin.close(globalLockFD) }
        }
    }
    private enum TaskPriority: Int { case background, utility, userInitiated, interactive }
    private struct QueuedTask {
        let id: String
        let operation: String
        let priority: TaskPriority
        let sequence: UInt64
        let submittedAt: UInt64
        let submittedDate: Date
        let notBefore: UInt64
        let attempts: Int
        let start: (Date, Int) -> Void
        let cancel: () -> Void
    }
    private final class DiagnosticCapture: @unchecked Sendable {
        private let limit: Int
        private let onProgress: (Int, String, String) -> Void
        private let lock = NSLock()
        private var storage = Data()
        private var evidenceRecords: [WorkerEvidenceRecord] = []
        private var evidenceTruncated = false
        private var evidenceFingerprints: [String: String] = [:]
        init(limit: Int, onProgress: @escaping (Int, String, String) -> Void) {
            self.limit = limit
            self.onProgress = onProgress
        }
        func drain(_ handle: FileHandle) {
            var pending = Data()
            while true {
                do {
                    guard let chunk = try handle.read(upToCount: 4096), !chunk.isEmpty else {
                        if !pending.isEmpty { consume(pending) }
                        return
                    }
                    pending.append(chunk)
                    while let newline = pending.firstIndex(of: 0x0a) {
                        let line = Data(pending[..<newline])
                        pending.removeSubrange(...newline)
                        consume(line)
                    }
                    if pending.count > 24 * 1024 {
                        appendDiagnostic(pending.prefix(8192))
                        pending.removeAll(keepingCapacity: true)
                    }
                } catch {
                    if !pending.isEmpty { consume(pending) }
                    return
                }
            }
        }
        private func consume(_ bytes: Data) {
            var line = String(decoding: bytes, as: UTF8.self)
            if line.hasSuffix("\r") { line.removeLast() }
            if line.hasPrefix("VG_EVIDENCE\t") {
                let payload = String(line.dropFirst("VG_EVIDENCE\t".count))
                if let data = payload.data(using: .utf8), data.count <= 20 * 1024,
                   let record = try? JSONDecoder().decode(WorkerEvidenceRecord.self, from: data) {
                    lock.lock()
                    if evidenceRecords.count < 50_000 { evidenceRecords.append(record) } else { evidenceTruncated = true }
                    lock.unlock()
                    return
                }
            }
            if line.hasPrefix("VG_EVIDENCE_MANIFEST\t") {
                let payload = String(line.dropFirst("VG_EVIDENCE_MANIFEST\t".count))
                if let data = payload.data(using: .utf8), data.count <= 32 * 1024,
                   let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let fingerprints = object["stage_fingerprints"] as? [String: String] {
                    lock.lock()
                    evidenceFingerprints = Dictionary(uniqueKeysWithValues: fingerprints.prefix(64).map { ($0.key, $0.value) })
                    evidenceTruncated = evidenceTruncated || ((object["truncated"] as? Bool) ?? false)
                    lock.unlock()
                    return
                }
            }
            if line.hasPrefix("VG_PROGRESS\t") {
                let fields = line.split(separator: "\t", maxSplits: 3, omittingEmptySubsequences: false)
                if fields.count == 4, let percent = Int(fields[1]), (0...100).contains(percent) {
                    let stage = String(fields[2].prefix(48))
                    let detail = String(fields[3].prefix(256))
                    if !stage.isEmpty && !detail.isEmpty { onProgress(percent, stage, detail); return }
                }
            }
            appendDiagnostic(bytes)
        }
        private func appendDiagnostic<T: DataProtocol>(_ bytes: T) {
            lock.lock(); defer { lock.unlock() }
            guard storage.count < limit else { return }
            storage.append(contentsOf: bytes.prefix(limit - storage.count))
            if storage.count < limit { storage.append(0x0a) }
        }
        var text: String {
            lock.lock(); defer { lock.unlock() }
            return String(decoding: storage, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }
        var evidenceBundle: WorkerEvidenceBundle? {
            lock.lock(); defer { lock.unlock() }
            guard !evidenceRecords.isEmpty else { return nil }
            return WorkerEvidenceBundle(schema: 1, producer: "worker-native-evidence-v1",
                                        records: evidenceRecords, truncated: evidenceTruncated,
                                        stage_fingerprints: evidenceFingerprints).validated()
        }
    }
    private final class EvidenceBundleCapture: @unchecked Sendable { var bundle: WorkerEvidenceBundle? }
    private final class ResultCapture: @unchecked Sendable {
        private let lock = NSLock()
        private var storage: Result<Data, Error>?
        func store(_ result: Result<Data, Error>) {
            lock.lock(); storage = result; lock.unlock()
        }
        var value: Result<Data, Error>? {
            lock.lock(); defer { lock.unlock() }
            return storage
        }
    }
    private let queue: DispatchQueue
    private var job: Job?
    private var queuedTasks: [QueuedTask] = []
    private var nextTaskSequence: UInt64 = 0
    private let maxQueuedTasks = 16
    private var retryPumpScheduled = false
    private var recentStatuses: [String: InspectionStatus] = [:]
    private var recentStatusOrder: [String] = []
    private let governor: OpaquePointer?
    var isBusy: Bool { job != nil || !queuedTasks.isEmpty }
    var isAvailable: Bool { governor != nil }
    init(queue: DispatchQueue) {
        self.queue = queue
        // Only this queue uses the pointer; the ABI forbids concurrent use.
        // A missing governor is a recoverable service-degraded state in beta builds,
        // never a reason to crash the XPC service at startup.
        self.governor = reworkbench_governor_new(128, 1)
    }
    deinit { if let governor { reworkbench_governor_free(governor) } }

    func start(path: String, staging: String, id: String, completion: @escaping (Result<HeaderResult, Error>) -> Void) {
        run(path: path, arguments: ["--capture-inspect", path, staging, id], id: id, operation: "capture") { result in
            completion(result.flatMap { data in Result { try Self.captureResult(data, id: id) } })
        }
    }
    func page(path: String, digest: String, size: String, start: UInt64, id: String,
              completion: @escaping (Result<InstructionPage, Error>) -> Void) {
        run(path: path, arguments: ["--page", path, digest, size, String(start), id], id: id, operation: "page") { result in
            completion(result.flatMap { data in Result {
                guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                      Set(object.keys) == Set(["protocol", "kind", "request_id", "source_sha256", "start_word", "total_words", "section_address", "section_offset", "word_count", "text"]) else {
                    throw WorkbenchError("Unexpected page fields.")
                }
                let page = try JSONDecoder().decode(InstructionPage.self, from: data)
                guard page.matches(id: id, digest: digest, start: start) else { throw WorkbenchError("Page identity or bounds mismatch.") }
                return page
            } })
        }
    }
    func analyze(path: String, digest: String, size: String, id: String,
                 completion: @escaping (Result<(AnalysisReport, WorkerEvidenceBundle?), Error>) -> Void) {
        let evidenceBox = EvidenceBundleCapture()
        run(path: path, arguments: ["--analyze", path, digest, size, id], id: id, operation: "analysis",
            maxFrameBytes: WorkbenchProtocolLimits.analysisWorkerFrameBytes, deadlineSeconds: 240,
            admissionBytes: 512 * 1_048_576,
            evidenceSink: { evidenceBox.bundle = $0 }) { [weak self] result in
            do {
                let data = try result.get()
                self?.updateRecent(id: id, phase: .validating, detail: "Validating analysis identity, bounds, and schema.", progressPercent: 98, progressStage: "validate")
                guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                      Set(object.keys) == AnalysisReport.jsonFields else {
                    throw WorkbenchError("Unexpected analysis fields.")
                }
                let report = try JSONDecoder().decode(AnalysisReport.self, from: data)
                guard report.matches(id: id, digest: digest) else { throw WorkbenchError("Analysis identity or bounds mismatch.") }
                self?.updateRecent(id: id, phase: .publishing, detail: "Analysis validated; publishing the bounded result to the workbench.", progressPercent: 99, progressStage: "publish")
                completion(.success((report, evidenceBox.bundle)))
            } catch {
                self?.updateRecent(id: id, phase: .failed, detail: error.localizedDescription)
                completion(.failure(error))
            }
        }
    }
    private static func captureResult(_ data: Data, id: String) throws -> HeaderResult {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw WorkbenchError("Invalid capture JSON.") }
                let expected: Set<String> = ["protocol", "request_id", "kind", "file_kind", "architecture", "size", "header_hex", "captured", "source_sha256", "structure_status", "structure", "disassembly_status", "disassembly"]
                guard Set(object.keys) == expected else { throw WorkbenchError("Unexpected worker fields.") }
                let decoded = try JSONDecoder().decode(HeaderResult.self, from: data)
                guard decoded.`protocol` == 1, decoded.request_id == id, decoded.kind == "header_result",
                      decoded.captured, UInt64(decoded.size) != nil, decoded.header_hex.utf8.count <= 95,
                      let digest = decoded.source_sha256, digest.count == 64,
                      digest.allSatisfy({ "0123456789abcdef".contains($0) }),
                      ["checked", "unsupported", "rejected"].contains(decoded.structure_status ?? ""),
                      (decoded.structure?.utf8.count ?? Int.max) < 65536,
                      ["preview", "metadata-only", "unavailable"].contains(decoded.disassembly_status ?? ""),
                      (decoded.disassembly?.utf8.count ?? Int.max) < 65536 else {
                    throw WorkbenchError("Worker response identity or coverage mismatch.")
                }
        return decoded
    }
    private static func acquireGlobalWorkerLock() throws -> Int32 {
        let lockURL = FileManager.default.temporaryDirectory.appendingPathComponent("ca.zensoftware.VectorGlass.worker.lock", isDirectory: false)
        let fd = Darwin.open(lockURL.path, O_CREAT | O_RDWR | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard fd >= 0 else { throw WorkbenchError("Cannot open the per-user worker admission lock.") }
        var info = stat()
        guard fstat(fd, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG, info.st_uid == getuid() else {
            Darwin.close(fd)
            throw WorkbenchError("Worker admission lock failed its ownership/type check.")
        }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else {
            Darwin.close(fd)
            throw WorkbenchError("Another HindSight instance is already running a heavy analysis worker. Wait for it to finish or cancel it first.")
        }
        return fd
    }

    private func run(path: String, arguments: [String], id: String, operation: String,
                     maxFrameBytes: Int = WorkbenchProtocolLimits.controlWorkerFrameBytes,
                     deadlineSeconds: TimeInterval = 120,
                     admissionBytes: UInt64 = 0,
                     bypassQueue: Bool = false,
                     admissionAttempt: Int = 0,
                     submittedDate: Date? = nil,
                     evidenceSink: ((WorkerEvidenceBundle?) -> Void)? = nil,
                     completion: @escaping (Result<Data, Error>) -> Void) {
        guard UUID(uuidString: id) != nil, path.utf8.count <= 16384, path.hasPrefix("/"), !path.contains("\0"),
              maxFrameBytes > 0, maxFrameBytes <= WorkbenchProtocolLimits.analysisWorkerFrameBytes else {
            completion(.failure(WorkbenchError("Invalid inspection request."))); return
        }
        guard let governor else {
            completion(.failure(WorkbenchError("Rust admission governor is unavailable. Restart HindSight; if the problem persists, run scripts/test-macos.sh and rebuild the CoordinatorService.")))
            return
        }
        guard job?.id != id, !queuedTasks.contains(where: { $0.id == id }) else {
            completion(.failure(WorkbenchError("Duplicate worker request ID.")))
            return
        }
        if job != nil || (!queuedTasks.isEmpty && !bypassQueue) {
            let priority: TaskPriority = operation == "page" ? .interactive : .userInitiated
            let enqueued = enqueue(id: id, operation: operation, priority: priority,
                                   attempts: admissionAttempt,
                                   start: { [weak self] submittedDate, attempts in
                self?.run(path: path, arguments: arguments, id: id, operation: operation,
                          maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                          admissionBytes: admissionBytes, bypassQueue: true, admissionAttempt: attempts,
                          submittedDate: submittedDate,
                          evidenceSink: evidenceSink, completion: completion)
            }, cancel: { completion(.failure(WorkbenchError("Queued worker request cancelled."))) })
            if !enqueued { completion(.failure(WorkbenchError("Worker task queue is full (\(self.queuedTasks.count)/\(self.maxQueuedTasks)). Retry after active work completes."))) }
            return
        }
        guard let executable = Bundle.main.url(forAuxiliaryExecutable: "reworker"),
              FileManager.default.isExecutableFile(atPath: executable.path) else {
            completion(.failure(WorkbenchError("Bundled Rust worker is missing. Rebuild the coordinator target."))); return
        }
        // Fail closed on new heavyweight work when the coordinator is already
        // memory constrained. This is admission control, not active preemption.
        let resource = RuntimeResourceAdmission.snapshot()
        guard resource.allowsHeavyWorker else {
            retryAdmission(path: path, arguments: arguments, id: id, operation: operation,
                           maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                           admissionBytes: admissionBytes, attempt: admissionAttempt,
                           submittedDate: submittedDate, evidenceSink: evidenceSink, completion: completion,
                           error: WorkbenchError("Analysis deferred: memory or thermal pressure is elevated (\(resource.description)). Close other heavy workloads or retry later."))
            return
        }
        let resourceAdmission: UUID?
        if admissionBytes > 0 {
            guard let token = SharedResourceLedger.shared.reserve(admissionBytes) else {
                retryAdmission(path: path, arguments: arguments, id: id, operation: operation,
                               maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                               admissionBytes: admissionBytes, attempt: admissionAttempt,
                               submittedDate: submittedDate, evidenceSink: evidenceSink, completion: completion,
                               error: WorkbenchError("Worker resource budget is busy; retry after active work completes."))
                return
            }
            resourceAdmission = token
        } else { resourceAdmission = nil }
        let globalLockFD: Int32
        do { globalLockFD = try Self.acquireGlobalWorkerLock() }
        catch {
            if let resourceAdmission { SharedResourceLedger.shared.release(resourceAdmission) }
            if error.localizedDescription.contains("Another HindSight instance") {
                retryAdmission(path: path, arguments: arguments, id: id, operation: operation,
                               maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                               admissionBytes: admissionBytes, attempt: admissionAttempt,
                               submittedDate: submittedDate, evidenceSink: evidenceSink, completion: completion,
                               error: error)
            } else { completion(.failure(error)) }
            return
        }
        let lease = reworkbench_governor_admit(governor, 128)
        guard lease != 0 else {
            flock(globalLockFD, LOCK_UN); Darwin.close(globalLockFD)
            if let resourceAdmission { SharedResourceLedger.shared.release(resourceAdmission) }
            retryAdmission(path: path, arguments: arguments, id: id, operation: operation,
                           maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                           admissionBytes: admissionBytes, attempt: admissionAttempt,
                           submittedDate: submittedDate, evidenceSink: evidenceSink, completion: completion,
                           error: WorkbenchError("Worker reservation denied."))
            return
        }

        let process = Process()
        let output = Pipe()
        let diagnostics = Pipe()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = output
        process.standardError = diagnostics
        process.environment = ["LANG": "en_US.UTF-8"]

        let active = Job(id: id, operation: operation, process: process, lease: lease, globalLockFD: globalLockFD, resourceAdmission: resourceAdmission, startedAt: submittedDate ?? Date(), completion: completion)
        let lifecycle = DispatchGroup()
        let outputState = ResultCapture()
        let diagnosticState = DiagnosticCapture(limit: 32 * 1024) { [weak self, weak active] percent, stage, detail in
            self?.queue.async { [weak self, weak active] in
                guard let self, let active, self.job === active else { return }
                active.update(.running, detail, progressPercent: percent, progressStage: stage)
            }
        }

        // Process exit is one asynchronous lifecycle event. Never synchronously wait for it on
        // a higher-QoS thread: that caused the beta.4 priority inversion and SIGKILL path.
        lifecycle.enter()
        process.terminationHandler = { _ in lifecycle.leave() }
        job = active
        active.update(.launching, "Launching bounded Rust worker.", progressPercent: operation == "analysis" ? 1 : 0, progressStage: operation == "analysis" ? "launch" : operation)
        do { try process.run(); active.update(.running, operation == "analysis" ? "Worker is verifying the captured artifact." : "Worker is processing the request.", progressPercent: operation == "analysis" ? 2 : nil, progressStage: operation == "analysis" ? "verify" : nil) } catch {
            process.terminationHandler = nil
            lifecycle.leave()
            active.update(.failed, "Worker launch failed: \(error.localizedDescription)")
            remember(active.snapshot())
            job = nil
            reworkbench_governor_revoke(governor, lease)
            active.releaseResources()
            completion(.failure(error))
            return
        }

        // Parent must close its copies or EOF is never observable.
        try? output.fileHandleForWriting.close()
        try? diagnostics.fileHandleForWriting.close()

        lifecycle.enter()
        DispatchQueue.global(qos: .utility).async {
            defer { lifecycle.leave(); try? output.fileHandleForReading.close() }
            do {
                let prefix = try Self.exact(output.fileHandleForReading, 4)
                let length = prefix.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
                guard Int(length) <= maxFrameBytes else {
                    throw WorkbenchError("Worker frame exceeds the bounded \(maxFrameBytes)-byte limit.")
                }
                let data = try Self.exact(output.fileHandleForReading, Int(length))
                self.queue.async { [self, active] in
                    guard job === active else { return }
                    active.update(.receiving, "Received a bounded worker result; checking framing and JSON.", progressPercent: operation == "analysis" ? 96 : nil, progressStage: operation == "analysis" ? "receive" : nil)
                }
                guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    throw WorkbenchError("Invalid worker JSON.")
                }
                if object["kind"] as? String == "error" {
                    throw WorkbenchError(object["message"] as? String ?? "Worker failed.")
                }
                let extra = try output.fileHandleForReading.read(upToCount: 1) ?? Data()
                guard extra.isEmpty else { throw WorkbenchError("Worker emitted trailing output.") }
                outputState.store(.success(data))
            } catch {
                outputState.store(.failure(error))
            }
        }

        lifecycle.enter()
        DispatchQueue.global(qos: .utility).async {
            defer { lifecycle.leave(); try? diagnostics.fileHandleForReading.close() }
            diagnosticState.drain(diagnostics.fileHandleForReading)
        }

        queue.asyncAfter(deadline: .now() + deadlineSeconds) { [weak self, weak active] in
            guard let self, let active, self.job === active else { return }
            _ = self.stop(id: id, reason: "\(operation.capitalized) exceeded its bounded \(Int(deadlineSeconds))-second deadline.")
        }

        // Finalization occurs only after process exit and both pipes have reached EOF. DispatchGroup
        // notification is asynchronous; there is no waitUntilExit()/wait() priority inversion.
        lifecycle.notify(queue: queue) { [self, active] in
            guard job === active else { return }
            process.terminationHandler = nil
            job = nil
            let publishAllowed = reworkbench_governor_may_publish(governor, active.lease)
            reworkbench_governor_revoke(governor, active.lease)
            var result = outputState.value ?? .failure(WorkbenchError("Worker exited without producing a framed result."))
            let diagnosticText = diagnosticState.text
            evidenceSink?(diagnosticState.evidenceBundle)
            if case .failure(let error) = result, !diagnosticText.isEmpty {
                result = .failure(WorkbenchError("\(error.localizedDescription)\nWorker diagnostic: \(diagnosticText)"))
            }
            if let reason = active.stopReason {
                active.update(.cancelled, reason, diagnostic: diagnosticText)
                remember(active.snapshot())
                active.completion(.failure(WorkbenchError(reason)))
            } else if !publishAllowed {
                active.update(.cancelled, "Worker lease was revoked before publication.", diagnostic: diagnosticText)
                remember(active.snapshot())
                active.completion(.failure(WorkbenchError("Worker lease revoked.")))
            } else if process.terminationStatus != 0 {
                let suffix = diagnosticText.isEmpty ? "" : "\nWorker diagnostic: \(diagnosticText)"
                let termination: String
                if process.terminationReason == .uncaughtSignal {
                    termination = "Analysis worker terminated by signal \(process.terminationStatus). The app remains isolated from the worker failure; SIGKILL/SIGABRT can indicate memory pressure, a native decoder fault, or an external stop."
                } else {
                    termination = "Worker exited with status \(process.terminationStatus)."
                }
                active.update(.failed, termination, diagnostic: diagnosticText)
                remember(active.snapshot())
                active.completion(.failure(WorkbenchError("\(termination)\(suffix)")))
            } else {
                switch result {
                case .success:
                    if operation == "analysis" {
                        active.update(.validating, "Worker result received; validating analysis schema and identity.", progressPercent: 98, progressStage: "validate", diagnostic: diagnosticText)
                    } else {
                        active.update(.completed, "Worker request completed.", diagnostic: diagnosticText)
                    }
                case .failure(let error):
                    active.update(.failed, error.localizedDescription, diagnostic: diagnosticText)
                }
                remember(active.snapshot())
                active.completion(result)
            }
            active.releaseResources()
            startNextQueued()
        }
    }
    func status(id: String) -> InspectionStatus? {
        if let active = job, active.id == id { return active.snapshot() }
        guard let current = recentStatuses[id] else { return nil }
        guard current.phase == .queued else { return current }
        let now = Date()
        let nowMS = UInt64(max(0, now.timeIntervalSince1970 * 1000.0))
        let elapsed = nowMS >= current.started_at_ms ? nowMS - current.started_at_ms : current.elapsed_ms
        return InspectionStatus(request_id: current.request_id, operation: current.operation, phase: current.phase,
                                detail: current.detail, started_at_ms: current.started_at_ms,
                                updated_at_ms: nowMS, elapsed_ms: elapsed,
                                progress_percent: current.progress_percent, progress_stage: current.progress_stage,
                                diagnostic: current.diagnostic)
    }

    private func enqueue(id: String, operation: String, priority: TaskPriority,
                         attempts: Int = 0, delayNanos: UInt64 = 0,
                         start: @escaping (Date, Int) -> Void, cancel: @escaping () -> Void) -> Bool {
        guard queuedTasks.count < maxQueuedTasks else { return false }
        nextTaskSequence &+= 1
        let now = DispatchTime.now().uptimeNanoseconds
        let submittedDate = Date()
        queuedTasks.append(QueuedTask(id: id, operation: operation, priority: priority,
                                      sequence: nextTaskSequence,
                                      submittedAt: now, submittedDate: submittedDate,
                                      notBefore: now &+ delayNanos, attempts: attempts,
                                      start: start, cancel: cancel))
        recordStatus(id: id, operation: operation, phase: .queued,
                     detail: "Waiting for the worker slot (queue position \(queuedTasks.count)).",
                     progressPercent: 0, progressStage: "queue")
        updateQueuedPositions()
        return true
    }

    private func updateQueuedPositions() {
        let now = DispatchTime.now().uptimeNanoseconds
        let order = queuedTasks.indices.sorted { lhs, rhs in
            let a = queuedTasks[lhs], b = queuedTasks[rhs]
            let aReady = a.notBefore <= now, bReady = b.notBefore <= now
            if aReady != bReady { return aReady }
            let aPriority = a.priority.rawValue + Int(min(3, (now - a.submittedAt) / 2_000_000_000))
            let bPriority = b.priority.rawValue + Int(min(3, (now - b.submittedAt) / 2_000_000_000))
            return aPriority == bPriority ? a.sequence < b.sequence : aPriority > bPriority
        }
        for (offset, index) in order.enumerated() {
            let task = queuedTasks[index]
            updateRecent(id: task.id, phase: .queued,
                         detail: "Waiting for worker slot (queue position \(offset + 1) of \(order.count)).",
                         progressStage: "queue")
        }
    }

    private func startNextQueued() {
        while job == nil && !queuedTasks.isEmpty {
            let now = DispatchTime.now().uptimeNanoseconds
            let eligible = queuedTasks.indices.filter { queuedTasks[$0].notBefore <= now }
            guard let index = eligible.max(by: { lhs, rhs in
                let a = queuedTasks[lhs], b = queuedTasks[rhs]
                let aAge = Int(min(3, (now - a.submittedAt) / 2_000_000_000))
                let bAge = Int(min(3, (now - b.submittedAt) / 2_000_000_000))
                let aPriority = a.priority.rawValue + aAge
                let bPriority = b.priority.rawValue + bAge
                return aPriority == bPriority ? a.sequence > b.sequence : aPriority < bPriority
            }) else {
                scheduleRetryPump(after: queuedTasks.map { $0.notBefore }.min().map { $0 > now ? $0 - now : 0 } ?? 0)
                return
            }
            let task = queuedTasks.remove(at: index)
            updateRecent(id: task.id, phase: .admitted, detail: "Worker slot available; dispatching queued task.", progressStage: "dispatch")
            updateQueuedPositions()
            task.start(task.submittedDate, task.attempts)
        }
    }

    private func scheduleRetryPump(after delayNanos: UInt64) {
        guard !retryPumpScheduled else { return }
        retryPumpScheduled = true
        let boundedDelay = max(1_000_000, delayNanos)
        queue.asyncAfter(deadline: .now() + .nanoseconds(Int(min(UInt64(Int.max), boundedDelay)))) { [weak self] in
            guard let self else { return }
            self.retryPumpScheduled = false
            self.startNextQueued()
        }
    }

    private func retryAdmission(path: String, arguments: [String], id: String, operation: String,
                                maxFrameBytes: Int, deadlineSeconds: TimeInterval, admissionBytes: UInt64,
                                attempt: Int, submittedDate: Date?, evidenceSink: ((WorkerEvidenceBundle?) -> Void)?,
                                completion: @escaping (Result<Data, Error>) -> Void, error: Error) {
        guard attempt < 20 else { completion(.failure(error)); return }
        let priority: TaskPriority = operation == "page" ? .interactive : .userInitiated
        let nextAttempt = attempt + 1
        let accepted = enqueue(id: id, operation: operation, priority: priority,
                               attempts: nextAttempt, delayNanos: 250_000_000,
                               start: { [weak self] date, attempt in
            self?.run(path: path, arguments: arguments, id: id, operation: operation,
                      maxFrameBytes: maxFrameBytes, deadlineSeconds: deadlineSeconds,
                      admissionBytes: admissionBytes, bypassQueue: true,
                      admissionAttempt: attempt, submittedDate: submittedDate ?? date,
                      evidenceSink: evidenceSink, completion: completion)
        }, cancel: { completion(.failure(WorkbenchError("Queued worker request cancelled."))) })
        guard accepted else { completion(.failure(WorkbenchError("Worker task queue is full while waiting for resources."))); return }
        // A dispatched queue item will let its outer pump advance to other eligible work.
        if job == nil { startNextQueued() }
    }

    private func cancelQueuedTasks(reason: String) {
        let cancelled = queuedTasks
        queuedTasks.removeAll()
        for task in cancelled {
            updateRecent(id: task.id, phase: .cancelled, detail: reason, progressStage: "cancelled")
            task.cancel()
        }
    }

    private func updateRecent(id: String, phase: InspectionPhase, detail: String, progressPercent: Int? = nil, progressStage: String? = nil, diagnostic: String? = nil) {
        guard let current = recentStatuses[id] else { return }
        // Cancellation is a distinct terminal outcome. A later wrapper-level failure caused by
        // the cancelled process must not relabel an acknowledged cancellation as a failure.
        if current.phase == .cancelled && phase == .failed { return }
        let now = Date()
        let nowMS = UInt64(max(0, now.timeIntervalSince1970 * 1000.0))
        let elapsed = nowMS >= current.started_at_ms ? nowMS - current.started_at_ms : current.elapsed_ms
        remember(InspectionStatus(request_id: id, operation: current.operation, phase: phase, detail: detail,
                                  started_at_ms: current.started_at_ms, updated_at_ms: nowMS, elapsed_ms: elapsed,
                                  progress_percent: min(100, max(0, progressPercent ?? current.progress_percent)),
                                  progress_stage: String((progressStage ?? current.progress_stage).prefix(48)),
                                  diagnostic: diagnostic ?? current.diagnostic))
    }

    func recordStatus(id: String, operation: String, phase: InspectionPhase, detail: String, progressPercent: Int, progressStage: String) {
        guard UUID(uuidString: id) != nil else { return }
        let now = Date()
        let nowMS = UInt64(max(0, now.timeIntervalSince1970 * 1000.0))
        let current = recentStatuses[id]
        remember(InspectionStatus(request_id: id, operation: operation, phase: phase, detail: detail,
                                  started_at_ms: current?.started_at_ms ?? nowMS, updated_at_ms: nowMS,
                                  elapsed_ms: current.map { nowMS >= $0.started_at_ms ? nowMS - $0.started_at_ms : $0.elapsed_ms } ?? 0,
                                  progress_percent: min(100, max(0, progressPercent)), progress_stage: String(progressStage.prefix(48)),
                                  diagnostic: current?.diagnostic))
    }

    func finishStatus(id: String, success: Bool, detail: String) {
        updateRecent(id: id, phase: success ? .completed : .failed, detail: detail, progressPercent: success ? 100 : nil, progressStage: success ? "complete" : nil)
    }

    private func remember(_ status: InspectionStatus) {
        recentStatuses[status.request_id] = status
        recentStatusOrder.removeAll { $0 == status.request_id }
        recentStatusOrder.append(status.request_id)
        while recentStatusOrder.count > 32 {
            let oldest = recentStatusOrder.removeFirst()
            recentStatuses.removeValue(forKey: oldest)
        }
    }

    @discardableResult
    func stop(id: String, reason: String = "Inspection cancelled.") -> Bool {
        if let index = queuedTasks.firstIndex(where: { $0.id == id }) {
            let task = queuedTasks.remove(at: index)
            updateRecent(id: id, phase: .cancelled, detail: reason, progressStage: "cancelled")
            updateQueuedPositions()
            task.cancel()
            startNextQueued()
            return true
        }
        guard let active = job, active.id == id else { return false }
        if active.stopReason != nil { return true }
        active.stopReason = reason
        active.update(.cancelling, reason)
        if let governor { reworkbench_governor_revoke(governor, active.lease) }
        if active.process.isRunning { active.process.terminate() }
        queue.asyncAfter(deadline: .now() + 0.5) { [weak self, weak active] in
            guard let self, let active, self.job === active, active.process.isRunning else { return }
            Darwin.kill(active.process.processIdentifier, SIGKILL)
        }
        return true
    }
    func stopAll() {
        cancelQueuedTasks(reason: "Client disconnected before the queued task started.")
        if let job { _ = stop(id: job.id, reason: "Client disconnected.") }
    }
    private static func exact(_ handle: FileHandle, _ count: Int) throws -> Data {
        var result = Data()
        while result.count < count {
            let chunk = try handle.read(upToCount: count - result.count) ?? Data()
            guard !chunk.isEmpty else { throw WorkbenchError("Truncated worker frame.") }
            result.append(chunk)
        }
        return result
    }
}


/// Best-effort per-process admission guard. No shared cross-process lease accounting.
/// Shared coordinator-process reservation ledger. Conservative estimates are not RSS measurements.
final class SharedResourceLedger {
    static let shared = SharedResourceLedger()
    private let lock = NSLock()
    private var reservations: [UUID: UInt64] = [:]
    private init() {}
    func reserve(_ bytes: UInt64) -> UUID? {
        lock.lock(); defer { lock.unlock() }
        let physical = ProcessInfo.processInfo.physicalMemory
        let ceiling = min(physical / 4, 1_073_741_824) // <=1 GiB on 8 GiB machines
        let committed = reservations.values.reduce(UInt64(0)) { $0 &+ $1 }
        guard bytes <= ceiling, committed <= ceiling - bytes,
              ProcessInfo.processInfo.thermalState != .serious,
              ProcessInfo.processInfo.thermalState != .critical else { return nil }
        let token = UUID(); reservations[token] = bytes; return token
    }
    func release(_ token: UUID) { lock.lock(); reservations.removeValue(forKey: token); lock.unlock() }
}

enum RuntimeResourceAdmission {
    struct Snapshot {
        let residentBytes: UInt64
        let physicalBytes: UInt64
        let thermal: ProcessInfo.ThermalState
        var allowsHeavyWorker: Bool {
            let thermalOK = thermal != .serious && thermal != .critical
            let residentOK = residentBytes == 0 || physicalBytes == 0 ||
                residentBytes < physicalBytes / 3
            return thermalOK && residentOK
        }
        var description: String {
            "resident=\\(residentBytes / 1_048_576)MiB physical=\\(physicalBytes / 1_048_576)MiB thermal=\\(thermal.rawValue)"
        }
    }

    static func snapshot() -> Snapshot {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
        let status: kern_return_t = withUnsafeMutablePointer(to: &info) { pointer in
            pointer.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count)
            }
        }
        return Snapshot(
            residentBytes: status == KERN_SUCCESS ? UInt64(info.resident_size) : 0,
            physicalBytes: ProcessInfo.processInfo.physicalMemory,
            thermal: ProcessInfo.processInfo.thermalState
        )
    }
}
