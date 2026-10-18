import Foundation
import Network
import Security

struct RemoteRequest {
    let method: String
    let path: String
    let body: Data
}

struct RemoteResponse {
    let code: Int
    let body: String
    static func json(_ body: String, code: Int = 200) -> Self { .init(code: code, body: body) }
    static func error(_ message: String, code: Int) -> Self {
        let data = try? JSONSerialization.data(withJSONObject: ["error": message])
        return .init(code: code, body: data.map { String(decoding: $0, as: UTF8.self) } ?? "{}")
    }
}

// The app owns this listener. Imported binaries are never executed by a request.
final class RemoteGateway {
    private let queue = DispatchQueue(label: "ca.zensoftware.hindsight.remote")
    private var listener: NWListener?
    private var connections: [UUID: NWConnection] = [:]
    private var secret = ""
    private var boundPort: UInt16 = 0
    private var boundAddress = "127.0.0.1"
    private var allowedOrigin = ""
    private var corsConnections: Set<UUID> = []
    private let handle: (RemoteRequest, @escaping (RemoteResponse) -> Void) -> Void
    private let status: (String) -> Void

    init(handle: @escaping (RemoteRequest, @escaping (RemoteResponse) -> Void) -> Void,
         status: @escaping (String) -> Void) {
        self.handle = handle
        self.status = status
    }

    func start(certificateLabel: String, bindAddress: String = "127.0.0.1", allowedOrigin: String = "", port: UInt16, token: String) throws {
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
        var certificate: SecCertificate?
        guard SecIdentityCopyCertificate(identity, &certificate) == errSecSuccess,
              let certificate else { throw GatewayError.invalidConfiguration }
        var commonName: CFString?
        guard SecCertificateCopyCommonName(certificate, &commonName) == errSecSuccess,
              commonName as String? == certificateLabel else { throw GatewayError.invalidConfiguration }
        try start(identity: identity, bindAddress: bindAddress, allowedOrigin: allowedOrigin, port: port, token: token)
    }

    func start(identity: SecIdentity, bindAddress: String = "127.0.0.1", allowedOrigin: String = "", port: UInt16, token: String) throws {
        guard port > 0, token.utf8.count >= 32 else { throw GatewayError.invalidConfiguration }
        let octets = bindAddress.split(separator: ".", omittingEmptySubsequences: false)
        guard octets.count == 4,
              octets.allSatisfy({ !$0.isEmpty && $0.allSatisfy(\.isNumber) && UInt8($0) != nil }),
              let first = UInt8(octets[0]), let second = UInt8(octets[1]),
              first == 127 && second == 0 && octets[2] == "0" && octets[3] == "1" ||
              first == 10 || first == 172 && (16...31).contains(second) || first == 192 && second == 168,
              let ip = IPv4Address(bindAddress) else { throw GatewayError.invalidConfiguration }
        let isLoopback = bindAddress == "127.0.0.1"
        guard allowedOrigin.isEmpty && isLoopback || Self.validOrigin(allowedOrigin) else {
            throw GatewayError.invalidConfiguration
        }
        guard let secIdentity = sec_identity_create(identity) else { throw GatewayError.invalidConfiguration }
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)
        sec_protocol_options_set_local_identity(tls.securityProtocolOptions, secIdentity)
        let parameters = NWParameters(tls: tls, tcp: NWProtocolTCP.Options())
        guard let endpointPort = NWEndpoint.Port(rawValue: port) else { throw GatewayError.invalidConfiguration }
        parameters.requiredLocalEndpoint = .hostPort(host: .ipv4(ip), port: endpointPort)
        let newListener = try NWListener(using: parameters)
        queue.sync {
            listener?.cancel()
            connections.values.forEach { $0.cancel() }
            connections.removeAll()
            secret = token
            boundPort = port
            boundAddress = bindAddress
            self.allowedOrigin = allowedOrigin
            corsConnections.removeAll()
            listener = newListener
            newListener.stateUpdateHandler = { [weak self] state in
                guard let self, self.listener === newListener else { return }
                switch state {
                case .ready: self.status("HTTPS listening on \(bindAddress):\(port)")
                case .failed(let error): self.stopOnQueue(status: "Gateway failed: \(error)")
                default: break
                }
            }
            newListener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
            newListener.start(queue: queue)
        }
    }

    func stop() {
        queue.sync { stopOnQueue(status: "Off") }
    }

    private func stopOnQueue(status value: String) {
        listener?.cancel()
        listener = nil
        connections.values.forEach { $0.cancel() }
        connections.removeAll()
        secret = ""
        boundPort = 0
        allowedOrigin = ""
        corsConnections.removeAll()
        status(value)
    }

    private func accept(_ connection: NWConnection) {
        guard connections.count < 8 else { connection.cancel(); return }
        let id = UUID()
        connections[id] = connection
        queue.asyncAfter(deadline: .now() + 20) { [weak self, weak connection] in
            guard let self, let connection, self.connections[id] != nil else { return }
            connection.cancel()
            self.connections.removeValue(forKey: id)
            self.corsConnections.remove(id)
        }
        connection.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready: self.read(connection, id: id, data: Data())
            case .failed, .cancelled:
                self.connections.removeValue(forKey: id)
                self.corsConnections.remove(id)
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
            if bytes.count > 20_480 { self.send(connection, id: id, response: .error("request too large", code: 413)); return }
            guard let end = bytes.range(of: Data("\r\n\r\n".utf8)) else {
                if complete || error != nil { connection.cancel(); self.connections.removeValue(forKey: id); self.corsConnections.remove(id) }
                else { self.read(connection, id: id, data: bytes) }
                return
            }
            guard end.lowerBound <= 16_384,
                  let header = String(data: bytes[..<end.upperBound], encoding: .utf8) else {
                self.send(connection, id: id, response: .error("invalid request", code: 400)); return
            }
            let lines = header.components(separatedBy: "\r\n")
            guard let requestLine = lines.first,
                  let parsed = Self.parseLine(requestLine),
                  lines.dropFirst().allSatisfy({ $0.isEmpty || ($0.contains(":") && !$0.contains("\0")) }),
                  !lines.dropFirst().contains(where: { $0.lowercased().hasPrefix("transfer-encoding:") }) else {
                self.send(connection, id: id, response: .error("invalid request", code: 400)); return
            }
            let lengths = lines.dropFirst().filter { $0.lowercased().hasPrefix("content-length:") }
            guard lengths.count <= 1,
                  let expected = lengths.first.map({ Int($0.dropFirst("Content-Length:".count).trimmingCharacters(in: .whitespaces)) }) ?? 0,
                  expected >= 0, expected <= 4096,
                  (parsed.method == "POST" || expected == 0) else {
                self.send(connection, id: id, response: .error("invalid body length", code: 400)); return
            }
            let body = Data(bytes[end.upperBound...])
            if body.count < expected {
                if complete || error != nil { connection.cancel(); self.connections.removeValue(forKey: id); self.corsConnections.remove(id) }
                else { self.read(connection, id: id, data: bytes) }
                return
            }
            guard body.count == expected else {
                self.send(connection, id: id, response: .error("invalid body", code: 400)); return
            }
            let auth = lines.dropFirst().filter { $0.lowercased().hasPrefix("authorization:") }
            let hosts = lines.dropFirst().filter { $0.lowercased().hasPrefix("host:") }
            let origins = lines.dropFirst().filter { $0.lowercased().hasPrefix("origin:") }
            let validHosts = boundAddress == "127.0.0.1" ? ["127.0.0.1:\(boundPort)", "localhost:\(boundPort)"] : ["\(boundAddress):\(boundPort)"]
            guard hosts.count == 1,
                  validHosts.contains(
                    hosts[0].dropFirst("Host:".count).trimmingCharacters(in: .whitespaces).lowercased()),
                  origins.count <= 1,
                  origins.isEmpty || origins[0].dropFirst("Origin:".count).trimmingCharacters(in: .whitespaces) == allowedOrigin else {
                self.send(connection, id: id, response: .error("invalid host or origin", code: 400)); return
            }
            if !origins.isEmpty { corsConnections.insert(id) }
            if parsed.method == "OPTIONS" {
                let requestedMethods = lines.dropFirst().filter { $0.lowercased().hasPrefix("access-control-request-method:") }
                let requestedHeaders = lines.dropFirst().filter { $0.lowercased().hasPrefix("access-control-request-headers:") }
                let method = requestedMethods.first?.dropFirst("Access-Control-Request-Method:".count).trimmingCharacters(in: .whitespaces)
                let headerNames = requestedHeaders.first?.dropFirst("Access-Control-Request-Headers:".count).split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces).lowercased() } ?? []
                guard origins.count == 1, requestedMethods.count == 1, requestedHeaders.count <= 1,
                      ["GET", "POST", "DELETE"].contains(method ?? ""),
                      headerNames.allSatisfy({ ["authorization", "content-type"].contains($0) }),
                      auth.isEmpty else {
                    self.send(connection, id: id, response: .error("invalid preflight", code: 400)); return
                }
                self.send(connection, id: id, response: .json("", code: 204)); return
            }
            guard auth.count == 1 else { self.send(connection, id: id, response: .error("unauthorized", code: 401)); return }
            let supplied = auth[0].dropFirst("Authorization:".count).trimmingCharacters(in: .whitespaces)
            guard supplied.hasPrefix("Bearer "), constantTimeEqual(String(supplied.dropFirst(7)), secret) else {
                self.send(connection, id: id, response: .error("unauthorized", code: 401)); return
            }
            handle(.init(method: parsed.method, path: parsed.path, body: body)) { [weak self, weak connection] response in
                guard let self, let connection else { return }
                self.queue.async { if self.connections[id] != nil { self.send(connection, id: id, response: response) } }
            }
        }
    }

    private static func parseLine(_ line: String) -> (method: String, path: String)? {
        let parts = line.split(separator: " ", omittingEmptySubsequences: false)
        guard parts.count == 3, parts[2] == "HTTP/1.1", ["GET", "POST", "DELETE", "OPTIONS"].contains(parts[0]),
              parts[1].hasPrefix("/v1/"), parts[1].utf8.count <= 256,
              !parts[1].contains("?"), !parts[1].contains("%"), !parts[1].contains("..") else { return nil }
        return (String(parts[0]), String(parts[1]))
    }

    private func send(_ connection: NWConnection, id: UUID, response value: RemoteResponse) {
        let payload = Data(value.body.utf8)
        guard payload.count <= WorkbenchProtocolLimits.analysisXPCReplyBytes + 4096 else { connection.cancel(); return }
        let reason = [200: "OK", 202: "Accepted", 204: "No Content", 400: "Bad Request", 401: "Unauthorized", 404: "Not Found", 409: "Conflict", 413: "Payload Too Large", 429: "Too Many Requests", 500: "Internal Server Error" , 503: "Service Unavailable"][value.code] ?? "Error"
        let cors = corsConnections.contains(id) ? "Access-Control-Allow-Origin: \(allowedOrigin)\r\nVary: Origin\r\nAccess-Control-Allow-Credentials: true\r\n" : ""
        let preflight = value.code == 204 ? "Access-Control-Allow-Methods: GET, POST, DELETE\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Max-Age: 600\r\n" : ""
        var response = Data("HTTP/1.1 \(value.code) \(reason)\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\(cors)\(preflight)Content-Length: \(payload.count)\r\nConnection: close\r\n\r\n".utf8)
        response.append(payload)
        connection.send(content: response, completion: .contentProcessed { [weak self] _ in
            connection.cancel()
            self?.queue.async { self?.connections.removeValue(forKey: id); self?.corsConnections.remove(id) }
        })
    }

    private func constantTimeEqual(_ a: String, _ b: String) -> Bool {
        let left = Array(a.utf8), right = Array(b.utf8)
        guard left.count == right.count else { return false }
        var difference: UInt8 = 0
        for index in left.indices { difference |= left[index] ^ right[index] }
        return difference == 0
    }

    private static func validOrigin(_ value: String) -> Bool {
        guard let url = URLComponents(string: value), url.scheme == "https", let host = url.host,
              !host.isEmpty, url.user == nil, url.password == nil, url.path.isEmpty,
              url.query == nil, url.fragment == nil, !value.contains("\r"), !value.contains("\n") else { return false }
        return value == "https://\(host)" + (url.port.map { ":\($0)" } ?? "")
    }
}

private enum GatewayError: LocalizedError {
    case invalidConfiguration
    case identityUnavailable(OSStatus)
    var errorDescription: String? {
        switch self {
        case .invalidConfiguration: return "Provide a Keychain TLS identity, private IPv4 bind address, exact HTTPS origin, port, and strong token."
        case .identityUnavailable(let status): return "TLS identity unavailable in Keychain (\(status))."
        }
    }
}
