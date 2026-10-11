import Foundation
@MainActor
final class CoordinatorClient {
    private var connection: NSXPCConnection?
    private var generation = UUID()
    private var pending: [UUID: (Result<String, Error>) -> Void] = [:]
    private var deadlines: [UUID: Task<Void, Never>] = [:]
    var onDisconnect: (() -> Void)?
    private func ensureConnection() -> NSXPCConnection {
        if let connection { return connection }
        generation = UUID(); let token = generation
        let value = NSXPCConnection(serviceName: ServiceIdentity.coordinator)
        value.remoteObjectInterface = NSXPCInterface(with: CoordinatorProtocol.self)
        value.interruptionHandler = { [weak self] in Task { @MainActor in self?.failedGeneration(token, "Coordinator interrupted. Reconnect to restore the session.") } }
        value.invalidationHandler = { [weak self] in Task { @MainActor in self?.failedGeneration(token, "Coordinator disconnected. Reconnect to restore the session.") } }
        connection = value; value.resume(); return value
    }
    func request(timeout: UInt64 = 8,
                 maxReplyBytes: Int = WorkbenchProtocolLimits.defaultXPCReplyBytes,
                 invalidateConnectionOnTimeout: Bool = true,
                 operation: (CoordinatorProtocol, @escaping (String) -> Void) -> Void,
                 completion: @escaping (Result<String, Error>) -> Void) {
        let value = ensureConnection(); let token = generation; let id = UUID()
        pending[id] = completion
        deadlines[id] = Task { [weak self] in
            do { try await Task.sleep(nanoseconds: timeout * 1_000_000_000) } catch { return }
            guard let self else { return }
            if invalidateConnectionOnTimeout {
                self.failedGeneration(token, "Service deadline exceeded. The last write outcome may be unknown; reopen the project to refresh.")
            } else {
                self.failRequest(id: id, token: token, message: "Status request deadline exceeded.")
            }
        }
        let proxy = value.remoteObjectProxyWithErrorHandler { [weak self] error in
            Task { @MainActor in self?.failedGeneration(token, error.localizedDescription) }
        }
        guard let service = proxy as? CoordinatorProtocol else { failedGeneration(token, "Service interface unavailable."); return }
        operation(service) { [weak self] text in
            // XPC may return multi-megabyte analysis payloads. Count the immutable
            // reply off the main actor so a size check cannot stall SwiftUI.
            let replyBytes = text.lengthOfBytes(using: .utf8)
            Task { @MainActor in
                guard let self, self.generation == token else { return }
                self.deadlines.removeValue(forKey: id)?.cancel()
                let callback = self.pending.removeValue(forKey: id)
                guard maxReplyBytes > 0, maxReplyBytes <= WorkbenchProtocolLimits.analysisXPCReplyBytes,
                      replyBytes <= maxReplyBytes else {
                    callback?(.failure(NSError(domain: "HindSight", code: 2, userInfo: [NSLocalizedDescriptionKey: "Service reply exceeds the bounded \(maxReplyBytes)-byte limit."])))
                    return
                }
                callback?(.success(text))
            }
        }
    }
    func resetConnection(reason: String = "Coordinator reconnect requested.") {
        generation = UUID()
        let callbacks = Array(pending.values)
        pending.removeAll()
        deadlines.values.forEach { $0.cancel() }
        deadlines.removeAll()
        let old = connection
        connection = nil
        old?.invalidate()
        guard !callbacks.isEmpty else { return }
        let error = NSError(domain: "HindSight", code: 4, userInfo: [NSLocalizedDescriptionKey: reason])
        callbacks.forEach { $0(.failure(error)) }
    }

    private func failRequest(id: UUID, token: UUID, message: String) {
        guard generation == token else { return }
        deadlines.removeValue(forKey: id)?.cancel()
        guard let callback = pending.removeValue(forKey: id) else { return }
        callback(.failure(NSError(domain: "HindSight", code: 3, userInfo: [NSLocalizedDescriptionKey: message])))
    }
    private func failedGeneration(_ token: UUID, _ message: String) {
        guard generation == token else { return }
        generation = UUID()
        let callbacks = Array(pending.values); pending.removeAll()
        deadlines.values.forEach { $0.cancel() }; deadlines.removeAll()
        let old = connection; connection = nil; old?.invalidate()
        onDisconnect?()
        let error = NSError(domain: "HindSight", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
        callbacks.forEach { $0(.failure(error)) }
    }
}
