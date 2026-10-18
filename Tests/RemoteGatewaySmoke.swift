import Foundation
import Security

// Standalone TLS/HTTP smoke fixture. Compile with App/RemoteGateway.swift.
enum WorkbenchProtocolLimits { static let analysisXPCReplyBytes = 12 * 1024 * 1024 }

@main struct RemoteGatewaySmoke {
static func main() throws {
guard CommandLine.arguments.count == 4,
      let port = UInt16(CommandLine.arguments[2]), port > 0,
      let input = try? Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])) else {
    fatalError("usage: smoke <PKCS12 file> <port> <password>")
}
var items: CFArray?
let status = SecPKCS12Import(input as CFData,
                           [kSecImportExportPassphrase as String: CommandLine.arguments[3]] as CFDictionary,
                           &items)
guard status == errSecSuccess,
      let entries = items as? [[String: Any]],
      let item = entries.first?[kSecImportItemIdentity as String],
      CFGetTypeID(item as CFTypeRef) == SecIdentityGetTypeID() else {
    fatalError("PKCS12 identity import failed: \(status)")
}
let identity = unsafeBitCast(item as CFTypeRef, to: SecIdentity.self)
let token = String(repeating: "a", count: 64)
let gateway = RemoteGateway(handle: { request, reply in
    if request.method == "GET", request.path == "/v1/capabilities" {
        reply(.json("{\"ok\":true}"))
    } else if request.method == "POST", request.path == "/v1/jobs/analysis", request.body == Data("{}".utf8) {
        reply(.json("{\"accepted\":true}", code: 202))
    } else {
        reply(.error("not found", code: 404))
    }
}, status: { print($0); fflush(stdout) })
try gateway.start(identity: identity, port: port, token: token)
RunLoop.current.run(until: Date().addingTimeInterval(30))
}
}
