import Foundation
import Network
import Security

// The app owns this listener. Imported binaries are never executed by a request.
final class RemoteGateway {
    private let queue = DispatchQueue(label: "ca.zensoftware.hindsight.remote")
    private var listener: NWListener?
    private var connections: [UUID: NWConnection] = [:]
    private var secret = ""
    private let handle: (String, @escaping (String) -> Void) -> Void
    private let status: (String) -> Void

    init(handle: @escaping (String, @escaping (String) -> Void) -> Void,
         status: @escaping (String) -> Void) {
        self.handle = handle
        self.status = status
    }

    func start(certificateLabel: String, port: UInt16, token: String) throws {
        guard !certificateLabel.isEmpty, port > 0, token.utf8.count >= 32 else {
            throw GatewayError.invalidConfiguration
        }
        let query: [String: Any] = [kSecClass as String: kSecClassIdentity,
                                    kSecMatchLimit as String: kSecMatchLimitOne,
                                    kSecReturnRef as String: true,
                                    kSecMatchSubjectContains as String: certificateLabel]
        var item: CFTypeRef?
        let result = SecItemCopyMatching(query as CFDictionary, &item)
        guard result == errSecSuccess, let item, CFGetTypeID(item) == SecIdentityGetTypeID() else {
            throw GatewayError.identityUnavailable(result)
        }
        let identity = unsafeBitCast(item, to: SecIdentity.self)
        guard let secIdentity = sec_identity_create(identity) else { throw GatewayError.invalidConfiguration }
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)
        sec_protocol_options_set_local_identity(tls.securityProtocolOptions, secIdentity)
        let parameters = NWParameters(tls: tls, tcp: NWProtocolTCP.Options())
        guard let endpointPort = NWEndpoint.Port(rawValue: port) else { throw GatewayError.invalidConfiguration }
        parameters.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: endpointPort)
        let newListener = try NWListener(using: parameters)
        queue.sync {
            listener?.cancel()
            connections.values.forEach { $0.cancel() }
            connections.removeAll()
            secret = token
            listener = newListener
            newListener.stateUpdateHandler = { [weak self] state in
                guard let self else { return }
                switch state {
                case .ready: self.status("HTTPS listening on 127.0.0.1:\(port)")
                case .failed(let error): self.status("Gateway failed: \(error)"); self.stop()
                default: break
                }
            }
            newListener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
            newListener.start(queue: queue)
        }
    }

    func stop() {
        queue.async {
            self.listener?.cancel()
            self.listener = nil
            self.connections.values.forEach { $0.cancel() }
            self.connections.removeAll()
            self.secret = ""
            self.status("Off")
        }
    }

    private func accept(_ connection: NWConnection) {
        guard connections.count < 8 else { connection.cancel(); return }
        let id = UUID()
        connections[id] = connection
        queue.asyncAfter(deadline: .now() + 15) { [weak self, weak connection] in
            guard let self, let connection, self.connections[id] != nil else { return }
            connection.cancel()
            self.connections.removeValue(forKey: id)
        }
        connection.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready: self.read(connection, id: id, data: Data())
            case .failed, .cancelled: self.connections.removeValue(forKey: id)
            default: break
            }
        }
        connection.start(queue: queue)
    }

    private func read(_ connection: NWConnection, id: UUID, data: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self, connection] chunk, _, complete, error in
            guard let self, self.connections[id] != nil else { return }
            var bytes = data
            if let chunk { bytes.append(chunk) }
            if bytes.count > 16_384 { self.send(connection, id: id, code: 413, body: "{\"error\":\"request too large\"}"); return }
            guard let end = bytes.range(of: Data("\r\n\r\n".utf8)) else {
                if complete || error != nil { connection.cancel(); self.connections.removeValue(forKey: id) }
                else { self.read(connection, id: id, data: bytes) }
                return
            }
            // This first read-only protocol has no request body or transfer coding.
            guard end.upperBound == bytes.endIndex,
                  let header = String(data: bytes, encoding: .utf8) else {
                self.send(connection, id: id, code: 400, body: "{\"error\":\"invalid request\"}"); return
            }
            let lines = header.components(separatedBy: "\r\n")
            guard let request = lines.first, request.hasSuffix(" HTTP/1.1"),
                  !lines.dropFirst().contains(where: { $0.lowercased().hasPrefix("content-length:") || $0.lowercased().hasPrefix("transfer-encoding:") }),
                  lines.dropFirst().allSatisfy({ $0.isEmpty || $0.contains(":") }) else {
                self.send(connection, id: id, code: 400, body: "{\"error\":\"invalid request\"}"); return
            }
            let auth = lines.dropFirst().filter { $0.lowercased().hasPrefix("authorization:") }
            guard auth.count == 1 else { self.send(connection, id: id, code: 401, body: "{\"error\":\"unauthorized\"}"); return }
            let supplied = auth[0].dropFirst("Authorization:".count).trimmingCharacters(in: .whitespaces)
            guard supplied.hasPrefix("Bearer "), constantTimeEqual(String(supplied.dropFirst(7)), secret) else {
                self.send(connection, id: id, code: 401, body: "{\"error\":\"unauthorized\"}"); return
            }
            guard request == "GET /v1/capabilities HTTP/1.1" || request == "GET /v1/project HTTP/1.1" else {
                self.send(connection, id: id, code: 404, body: "{\"error\":\"not found\"}"); return
            }
            handle(request) { [weak self, weak connection] body in
                guard let self, let connection else { return }
                self.queue.async { self.send(connection, id: id, code: 200, body: body) }
            }
        }
    }

    private func send(_ connection: NWConnection, id: UUID, code: Int, body: String) {
        let payload = Data(body.utf8)
        let reason = code == 200 ? "OK" : code == 401 ? "Unauthorized" : code == 404 ? "Not Found" : "Bad Request"
        var response = Data("HTTP/1.1 \(code) \(reason)\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: \(payload.count)\r\nConnection: close\r\n\r\n".utf8)
        response.append(payload)
        connection.send(content: response, completion: .contentProcessed { [weak self] _ in
            connection.cancel()
            self?.queue.async { self?.connections.removeValue(forKey: id) }
        })
    }

    private func constantTimeEqual(_ a: String, _ b: String) -> Bool {
        let left = Array(a.utf8), right = Array(b.utf8)
        guard left.count == right.count else { return false }
        var difference: UInt8 = 0
        for index in left.indices { difference |= left[index] ^ right[index] }
        return difference == 0
    }
}

private enum GatewayError: LocalizedError {
    case invalidConfiguration
    case identityUnavailable(OSStatus)
    var errorDescription: String? {
        switch self {
        case .invalidConfiguration: return "Provide a Keychain TLS identity label, port, and strong token."
        case .identityUnavailable(let status): return "TLS identity unavailable in Keychain (\(status))."
        }
    }
}
