import Foundation
import CryptoKit

/// Bounded, aging-priority worker pool for plugin and firmware operations.
/// The coordinator queue only admits work; execution and resource waits happen off it.
final class BoundedServiceLane {
    enum Priority: Int { case background = 0, utility = 1, initiated = 2, interactive = 3 }
    private struct Job {
        let id: UUID
        let generation: UInt64
        let priority: Priority
        let sequence: UInt64
        let submittedAt: UInt64
        let notBefore: UInt64
        let reply: (String) -> Void
        let work: () -> String
        let attempt: Int
    }
    private let lock = NSLock()
    private let queue: DispatchQueue
    private let maxConcurrent: Int
    private let maxPending: Int
    private let reservationBytes: UInt64
    private var pending: [Job] = []
    private var running: Set<UUID> = []
    private var generation: UInt64 = 0
    private var sequence: UInt64 = 0

    init(name: String, qos: DispatchQoS, maxConcurrent: Int, maxPending: Int = 64, reservationBytes: UInt64 = 32 * 1_048_576) {
        precondition(maxConcurrent > 0 && maxPending > 0)
        queue = DispatchQueue(label: name, qos: qos, attributes: .concurrent)
        self.maxConcurrent = maxConcurrent
        self.maxPending = maxPending
        self.reservationBytes = reservationBytes
    }
    @discardableResult
    func submit(priority: Priority = .utility,
                reply: @escaping (String) -> Void,
                work: @escaping () -> String) -> Bool {
        lock.lock()
        guard pending.count < maxPending else {
            lock.unlock()
            reply(ServiceReply.failure("Background dispatch lane saturated; retry after active work completes."))
            return false
        }
        sequence &+= 1
        let now = DispatchTime.now().uptimeNanoseconds
        pending.append(Job(id: UUID(), generation: generation, priority: priority,
                           sequence: sequence, submittedAt: now, notBefore: now,
                           reply: reply, work: work, attempt: 0))
        let next = takeReadyLocked(now: now)
        lock.unlock()
        start(next)
        return true
    }
    /// Invalidates all pending jobs. In-flight operations finish but their replies
    /// are discarded and replaced with a cancellation result.
    func cancelQueued() {
        lock.lock()
        generation &+= 1
        let rejected = pending
        pending.removeAll()
        lock.unlock()
        for job in rejected { job.reply(ServiceReply.failure("Queued operation cancelled.")) }
    }
    private func takeReadyLocked(now: UInt64) -> [Job] {
        var result: [Job] = []
        while running.count < maxConcurrent {
            let eligible = pending.indices.filter { pending[$0].notBefore <= now }
            guard let index = eligible.max(by: { lhs, rhs in
                let a = pending[lhs], b = pending[rhs]
                // Each two seconds of waiting advances a job one class, so old
                // background work eventually runs without starving new interaction.
                let aAge = Int(min(3, (now - a.submittedAt) / 2_000_000_000))
                let bAge = Int(min(3, (now - b.submittedAt) / 2_000_000_000))
                let aPriority = a.priority.rawValue + aAge
                let bPriority = b.priority.rawValue + bAge
                return aPriority == bPriority ? a.sequence > b.sequence : aPriority < bPriority
            }) else { break }
            let job = pending.remove(at: index)
            running.insert(job.id)
            result.append(job)
        }
        return result
    }
    private func start(_ jobs: [Job]) { for job in jobs { execute(job) } }
    private func pump() {
        lock.lock()
        let next = takeReadyLocked(now: DispatchTime.now().uptimeNanoseconds)
        lock.unlock()
        start(next)
    }
    private func execute(_ job: Job) {
        queue.async { [self] in
            guard let token = SharedResourceLedger.shared.reserve(reservationBytes) else {
                // Requeue with a bounded delay; resource pressure does not occupy a worker slot.
                lock.lock()
                running.remove(job.id)
                let valid = generation == job.generation
                if valid && job.attempt < 20 {
                    let retryAt = DispatchTime.now().uptimeNanoseconds &+ 250_000_000
                    pending.append(Job(id: job.id, generation: job.generation,
                                       priority: job.priority, sequence: job.sequence,
                                       submittedAt: job.submittedAt, notBefore: retryAt,
                                       reply: job.reply, work: job.work,
                                       attempt: job.attempt + 1))
                }
                let next = takeReadyLocked(now: DispatchTime.now().uptimeNanoseconds)
                lock.unlock()
                if !valid || job.attempt >= 20 {
                    job.reply(ServiceReply.failure("Resource admission timed out or cancelled."))
                }
                start(next)
                if valid && job.attempt < 20 { queue.asyncAfter(deadline: .now() + .milliseconds(250)) { [weak self] in self?.pump() } }
                return
            }
            let response: String = { defer { SharedResourceLedger.shared.release(token) }; return job.work() }()
            lock.lock()
            let valid = generation == job.generation
            running.remove(job.id)
            let next = takeReadyLocked(now: DispatchTime.now().uptimeNanoseconds)
            lock.unlock()
            job.reply(valid ? response : ServiceReply.failure("Operation superseded by newer session."))
            start(next)
        }
    }
}
final class CoordinatorService: NSObject, CoordinatorProtocol {
    private let queue = DispatchQueue(label: "ca.zensoftware.VectorGlass.coordinator")
    private var store: ProjectStore?
    private let pageCache = VerifiedPageCache()
    private lazy var supervisor = WorkerSupervisor(queue: queue)
    private let debugServer = DebugServer()
    private let scriptingHost = ScriptingHost()
    private let virtualizationHost = VirtualizationHost()
    private let firmwareRuntimeHost = FirmwareRuntimeHost()
    private let reaAgentHost = REAAgentHost()
    private let terminalHost = TerminalHost()
    private let firmwareLane = BoundedServiceLane(name: "ca.zensoftware.VectorGlass.firmware", qos: .utility, maxConcurrent: 1, maxPending: 16, reservationBytes: 192 * 1_048_576)
    private let scriptingLane = BoundedServiceLane(name: "ca.zensoftware.VectorGlass.scripting", qos: .utility, maxConcurrent: 4, maxPending: 64, reservationBytes: 64 * 1_048_576)
    private let reaLane = BoundedServiceLane(name: "ca.zensoftware.VectorGlass.rea", qos: .utility, maxConcurrent: 1, maxPending: 4, reservationBytes: 128 * 1_048_576)
    private let analysisCacheEngineVersion = "arm64-analysis-v74-schema118"
    private let analysisCacheProducerVersion = "0.5.11"
    private let compatibleLegacyAnalysisCacheVersions: Set<String> = ["0.4.1"]
    func capabilities(withReply reply: @escaping (String) -> Void) {
        guard let value = reworkbench_capabilities_json() else { reply("Bridge unavailable"); return }
        reply(String(cString: value))
    }
    func openProject(_ path: String, create: Bool, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            do {
                guard !supervisor.isBusy else { throw WorkbenchError("Cancel the active inspection before switching projects.") }
                guard path.hasPrefix("/"), path.utf8.count <= 16384, !path.contains("\0") else { throw WorkbenchError("Invalid project path.") }
                pageCache.clear()
                let canonical = URL(fileURLWithPath: path).standardizedFileURL.path
                if !create, let store, store.path == canonical { reply(ServiceReply.success(try store.snapshot())); return }
                let next = try ProjectStore(path: path, create: create)
                let snapshot = try next.snapshot()
                store = next
                reply(ServiceReply.success(snapshot))
            } catch { reply(ServiceReply.failure(error.localizedDescription)) }
        }
    }
    func inspectHeader(_ path: String, requestID: String, withReply reply: @escaping (String) -> Void) {
        captureBinary(path, options: nil, requestID: requestID, reply: reply)
    }
    func importBinary(_ path: String, optionsJSON: String, requestID: String, withReply reply: @escaping (String) -> Void) {
        guard optionsJSON.utf8.count < 1024,
              let decoded = try? JSONDecoder().decode(BinaryImportOptions.self, from: Data(optionsJSON.utf8)),
              let options = decoded.validated else { reply(ServiceReply.failure("Invalid binary import options.")); return }
        captureBinary(path, options: options, requestID: requestID, reply: reply)
    }
    private func captureBinary(_ path: String, options: BinaryImportOptions?, requestID: String, reply: @escaping (String) -> Void) {
        queue.async { [self] in
            guard let current = store else { reply(ServiceReply.failure("Create or open a project first.")); return }
            let staging: URL
            do { staging = try current.prepareCapture() }
            catch { reply(ServiceReply.failure(error.localizedDescription)); return }
            supervisor.start(path: path, staging: staging.path, id: requestID, options: options) { [self] result in
                defer { try? FileManager.default.removeItem(at: staging) }
                do {
                    guard store === current else { throw WorkbenchError("Project changed while worker ran.") }
                    let header = try result.get()
                    reply(ServiceReply.success(try current.addCaptured(path: path, result: header, staging: staging), "Source captured and structural inspection saved."))
                } catch { reply(ServiceReply.failure(error.localizedDescription)) }
            }
        }
    }
    func instructionPage(_ recordID: String, startWord: String, forceVerify: Bool, requestID: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            do {
                guard let current = store, let start = UInt64(startWord), UUID(uuidString: recordID) != nil, UUID(uuidString: requestID) != nil else {
                    throw WorkbenchError("Invalid page request or no open project.")
                }
                let source = try current.capturedSource(id: recordID)
                let before = try VerifiedPageCache.stamp(source.path)
                let key = try VerifiedPageCache.key(path: source.path, digest: source.digest, size: source.size, start: start, stamp: before, configuration: source.options?.cacheIdentity ?? "")
                if forceVerify { pageCache.clear() }
                if let cached = pageCache.get(key) {
                    let page = cached.forRequest(requestID)
                    guard try VerifiedPageCache.stamp(source.path) == before else {
                        pageCache.clear(); throw WorkbenchError("Captured object changed during cache lookup. Retry to verify it.")
                    }
                    if page.matches(id: requestID, digest: source.digest, start: start) {
                        reply(PageReply.encode(page: page, message: "Cached page reused; file metadata unchanged. Choose Verify again for a fresh hash.")); return
                    }
                    pageCache.clear()
                }
                supervisor.page(path: source.path, digest: source.digest, size: source.size, start: start, id: requestID, options: source.options) { [self] result in
                    do {
                        guard store === current else { throw WorkbenchError("Project changed during page request.") }
                        let page = try result.get()
                        guard try VerifiedPageCache.stamp(source.path) == before else {
                            pageCache.clear(); throw WorkbenchError("Captured object metadata changed during request. Retry to verify it.")
                        }
                        pageCache.put(page, key: key)
                        reply(PageReply.encode(page: page, message: "Page loaded; full source SHA-256 verified."))
                    } catch { reply(PageReply.encode(error: error.localizedDescription)) }
                }
            } catch { reply(PageReply.encode(error: error.localizedDescription)) }
        }
    }
    func analyzeRecord(_ recordID: String, requestID: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            do {
                guard let current = store, UUID(uuidString: recordID) != nil, UUID(uuidString: requestID) != nil else {
                    throw WorkbenchError("Invalid analysis request or no open project.")
                }
                let source = try current.capturedSource(id: recordID)
                let engineIdentity = source.options.map { "multiarch-v1:" + SHA256.hash(data: Data($0.cacheIdentity.utf8)).map { String(format: "%02x", $0) }.joined().prefix(48) } ?? (analysisCacheEngineVersion + "-patterns1")
                let before = try VerifiedPageCache.stamp(source.path)
                supervisor.recordStatus(id: requestID, operation: "analysis", phase: .checkingCache,
                                        detail: "Checking project-local analysis cache against source and engine identity.",
                                        progressPercent: 0, progressStage: "cache")
                var cacheWarning: String?
                let cached: (report: AnalysisReport, evidence: WorkerEvidenceBundle?, legacy: Bool)?
                do {
                    cached = try current.cachedAnalysis(
                        digest: source.digest, requestID: requestID, engineVersion: engineIdentity,
                        compatibleLegacyProductVersions: []
                    )
                } catch {
                    cached = nil
                    cacheWarning = "Project analysis cache could not be read; running a fresh worker analysis."
                }
                if let cached {
                    guard try VerifiedPageCache.stamp(source.path) == before else {
                        throw WorkbenchError("Captured object changed during analysis-cache lookup. Retry.")
                    }
                    var message = cached.legacy
                        ? "Compatible 0.4.1 analysis cache reused and migrated to the schema-2 engine-identity envelope."
                        : "Cached analysis reused; source identity, file metadata, and analysis-engine identity match."
                    if cached.legacy {
                        do {
                            try current.storeAnalysisCache(cached.report, evidence: cached.evidence, digest: source.digest,
                                                           engineVersion: engineIdentity,
                                                           producerVersion: analysisCacheProducerVersion)
                        } catch {
                            message += " Cache migration was not persisted."
                        }
                    }
                    supervisor.finishStatus(id: requestID, success: true, detail: message)
                    reply(AnalysisReply.encode(analysis: cached.report, evidence: cached.evidence, message: message))
                    return
                }
                supervisor.recordStatus(id: requestID, operation: "analysis", phase: .admitted,
                                        detail: cacheWarning ?? "No compatible cached result; launching bounded worker analysis.",
                                        progressPercent: 1, progressStage: "cache-miss")
                supervisor.analyze(path: source.path, digest: source.digest, size: source.size, id: requestID, options: source.options) { [self] result in
                    do {
                        guard store === current else { throw WorkbenchError("Project changed during analysis.") }
                        let workerResult = try result.get()
                        let report = workerResult.0
                        let evidence = workerResult.1
                        guard try VerifiedPageCache.stamp(source.path) == before else {
                            throw WorkbenchError("Captured object metadata changed during analysis. Retry.")
                        }
                        var message = report.truncated ? "Analysis completed within bounded scan limits." : "Analysis completed."
                        do {
                            try current.storeAnalysisCache(report, evidence: evidence, digest: source.digest,
                                                           engineVersion: engineIdentity,
                                                           producerVersion: analysisCacheProducerVersion)
                        } catch {
                            message += " Project analysis cache was not updated."
                        }
                        supervisor.finishStatus(id: requestID, success: true, detail: message)
                        reply(AnalysisReply.encode(analysis: report, evidence: evidence, message: message))
                    } catch {
                        supervisor.finishStatus(id: requestID, success: false, detail: error.localizedDescription)
                        reply(AnalysisReply.encode(error: error.localizedDescription))
                    }
                }
            } catch { reply(AnalysisReply.encode(error: error.localizedDescription)) }
        }
    }
    func cancelInspection(_ requestID: String, withReply reply: @escaping (Bool) -> Void) {
        queue.async { [self] in reply(supervisor.stop(id: requestID)) }
    }
    func inspectionStatus(_ requestID: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            guard UUID(uuidString: requestID) != nil else {
                reply(InspectionStatusReply.encode(error: "Invalid inspection request ID.")); return
            }
            guard let status = supervisor.status(id: requestID) else {
                reply(InspectionStatusReply.encode(error: "No active or recent inspection matches this request.")); return
            }
            reply(InspectionStatusReply.encode(status: status))
        }
    }
    func clearAnalysisCache(withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            do {
                guard let current = store else { throw WorkbenchError("Create or open a project first.") }
                guard !supervisor.isBusy else { throw WorkbenchError("Cancel the active inspection before clearing the analysis cache.") }
                let removed = try current.clearAnalysisCache()
                reply(ServiceReply.success("Cleared \(removed) cached analysis result\(removed == 1 ? "" : "s")."))
            } catch { reply(ServiceReply.failure(error.localizedDescription)) }
        }
    }
    func debugAttach(_ pid: Int32, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(debugServer.attach(pid: pid)) }
    }
    func debugLaunch(_ path: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(debugServer.launch(path: path)) }
    }
    func debugCommand(_ command: String, address: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            let value = address.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let raw = value.hasPrefix("0x") ? String(value.dropFirst(2)) : value
            let parsed = raw.isEmpty ? nil : UInt64(raw, radix: 16)
            if !raw.isEmpty && parsed == nil { reply(DebugCommandReply.failure("Invalid hexadecimal address.")); return }
            reply(debugServer.command(command, address: parsed))
        }
    }
    func debugSnapshot(withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            switch debugServer.snapshot() { case .success(let value): reply(value); case .failure(let error): reply(DebugCommandReply.failure(error.localizedDescription)) }
        }
    }
    func debugReadMemory(_ address: String, length: Int, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            var value = address.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(); if value.hasPrefix("0x") { value.removeFirst(2) }
            guard let parsed = UInt64(value, radix: 16) else { reply(DebugCommandReply.failure("Invalid hexadecimal address.")); return }
            switch debugServer.memory(address: parsed, length: length) { case .success(let value): reply(value); case .failure(let error): reply(DebugCommandReply.failure(error.localizedDescription)) }
        }
    }
    func debugPollEvent(withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            switch debugServer.event() { case .success(let value): reply(value); case .failure(let error): reply(DebugCommandReply.failure(error.localizedDescription)) }
        }
    }
    func debugQuery(_ query: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            switch debugServer.query(query) { case .success(let value): reply(value); case .failure(let error): reply(DebugCommandReply.failure(error.localizedDescription)) }
        }
    }
    func debugSelectThread(_ threadID: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            var raw = threadID.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(); if raw.hasPrefix("0x") { raw.removeFirst(2) }
            guard let value = UInt64(raw, radix: 16) else { reply(DebugCommandReply.failure("Invalid thread ID.")); return }
            reply(debugServer.selectThread(value))
        }
    }
    func scriptingRun(_ source: String, contextJSON: String, timeoutMS: Int, interpreter: String, withReply reply: @escaping (String) -> Void) {
        scriptingLane.submit(priority: .initiated, reply: reply) { [scriptingHost] in scriptingHost.run(source: source, contextJSON: contextJSON, capabilities: ["selection.read", "analysis.read", "debugger.read", "navigation.write", "breakpoint.write", "debugger.control", "notes.write", "annotation.read", "annotation.write", "data.read", "data.write"], timeoutMS: timeoutMS, interpreter: interpreter) }
    }
    func pluginList(withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(scriptingHost.listPlugins(projectPath: store?.path)) }
    }
    func pluginRun(_ pluginID: String, contextJSON: String, timeoutMS: Int, interpreter: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            let projectPath = store?.path
            scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.runPlugin(id: pluginID, projectPath: projectPath, contextJSON: contextJSON, timeoutMS: timeoutMS, interpreter: interpreter) }
        }
    }
    func pluginRunEvent(_ pluginID: String, event: String, contextJSON: String, timeoutMS: Int, interpreter: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            let projectPath = store?.path
            scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.runPlugin(id: pluginID, projectPath: projectPath, contextJSON: contextJSON, timeoutMS: timeoutMS, interpreter: interpreter, event: event) }
        }
    }
    func pluginInstall(_ path: String, withReply reply: @escaping (String) -> Void) {
        scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.installPlugin(fromPath: path) }
    }
    func pluginSetEnabled(_ pluginID: String, enabled: Bool, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in let projectPath=store?.path; scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.setPluginEnabled(id: pluginID, enabled: enabled, projectPath: projectPath) } }
    }
    func pluginSetTrusted(_ pluginID: String, trusted: Bool, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in let projectPath=store?.path; scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.setPluginTrusted(id: pluginID, trusted: trusted, projectPath: projectPath) } }
    }
    func pluginSetSetting(_ pluginID: String, key: String, valueJSON: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in let projectPath=store?.path; scriptingLane.submit(reply: reply) { [scriptingHost] in scriptingHost.setPluginSetting(id: pluginID, key:key, valueJSON:valueJSON, projectPath:projectPath) } }
    }
    func virtualizationProfiles(withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(virtualizationHost.profilesJSON()) }
    }
    func virtualizationStart(_ profileID: String, ramMiB: Int, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(virtualizationHost.start(profileID: profileID, ramMiB: ramMiB)) }
    }
    func virtualizationCommand(_ command: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(virtualizationHost.command(command)) }
    }
    func virtualizationRE(_ requestJSON: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in reply(virtualizationHost.reCommand(requestJSON)) }
    }
    func firmwareRuntime(_ requestJSON: String, withReply reply: @escaping (String) -> Void) {
        firmwareLane.submit(priority: .initiated, reply: reply) { [firmwareRuntimeHost] in firmwareRuntimeHost.command(requestJSON) }
    }
    func reaProbe(_ requestJSON: String, withReply reply: @escaping (String) -> Void) {
        reaLane.submit(priority: .initiated, reply: reply) { [reaAgentHost] in reaAgentHost.probe(requestJSON: requestJSON) }
    }
    func reaAnalyze(_ requestJSON: String, requestID: String, withReply reply: @escaping (String) -> Void) {
        reaLane.submit(priority: .initiated, reply: reply) { [reaAgentHost] in reaAgentHost.analyze(requestJSON: requestJSON, requestID: requestID) }
    }
    func reaCancel(_ requestID: String, withReply reply: @escaping (Bool) -> Void) {
        reply(reaAgentHost.cancel(requestID: requestID))
    }
    func reaProgress(_ requestID: String, withReply reply: @escaping (String) -> Void) {
        reply(reaAgentHost.progress(requestID: requestID))
    }
    func saveNote(_ recordID: String, expectedRevision: Int, text: String, withReply reply: @escaping (String) -> Void) {
        queue.async { [self] in
            do {
                guard let store else { throw WorkbenchError("No project is open.") }
                reply(ServiceReply.success(try store.save(id: recordID, expectedRevision: expectedRevision, note: text), "Note saved."))
            } catch { reply(ServiceReply.failure(error.localizedDescription)) }
        }
    }
    func disconnect() { queue.async { [self] in supervisor.stopAll(); firmwareLane.cancelQueued(); scriptingLane.cancelQueued(); reaLane.cancelQueued(); debugServer.shutdown(); terminalHost.stop(); pageCache.clear(); store = nil } }

    func terminalStart(_ workingDirectory: String, withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.start(workingDirectory: workingDirectory))
    }
    func terminalWrite(_ input: String, withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.write(input))
    }
    func terminalRead(withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.read())
    }
    func terminalResize(_ columns: Int, rows: Int, withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.resize(columns: columns, rows: rows))
    }
    func terminalInterrupt(withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.interrupt())
    }
    func terminalEOF(withReply reply: @escaping (String) -> Void) {
        reply(terminalHost.eof())
    }
    func terminalStop(withReply reply: @escaping (String) -> Void) {
        terminalHost.stop(); reply(TerminalReply.encode(TerminalReply(ok: true, message: "Terminal stopped.")))
    }
}
final class ServiceDelegate: NSObject, NSXPCListenerDelegate {
    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        let service = CoordinatorService()
        connection.exportedInterface = NSXPCInterface(with: CoordinatorProtocol.self)
        connection.exportedObject = service
        connection.invalidationHandler = { service.disconnect() }
        connection.interruptionHandler = { service.disconnect() }
        connection.resume()
        return true
    }
}
let delegate = ServiceDelegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()
