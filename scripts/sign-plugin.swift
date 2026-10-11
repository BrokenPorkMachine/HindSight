#!/usr/bin/env swift
import Foundation
import CryptoKit

func die(_ s: String) -> Never { fputs("sign-plugin: \(s)\n", stderr); exit(2) }
let args = CommandLine.arguments
if args.count != 3 { die("usage: swift scripts/sign-plugin.swift <Extension.vgplugin> <private-key-file>") }
let bundle = URL(fileURLWithPath: args[1]).standardizedFileURL
let keyURL = URL(fileURLWithPath: args[2]).standardizedFileURL
let manifestURL = bundle.appendingPathComponent("plugin.json")
guard let manifestData = try? Data(contentsOf: manifestURL),
      let manifest = try? JSONSerialization.jsonObject(with: manifestData) as? [String: Any],
      let entry = manifest["entry"] as? String else { die("invalid plugin.json") }
var signed = ["plugin.json", entry]
if let extra = manifest["signed_files"] as? [String] { signed.append(contentsOf: extra) }
signed = Array(Set(signed)).sorted()
var message = Data()
for rel in signed {
    if rel.contains("..") || rel.hasPrefix("/") || rel == "signature.json" { die("unsafe signed_files entry: \(rel)") }
    let u = bundle.appendingPathComponent(rel).standardizedFileURL
    guard u.path.hasPrefix(bundle.path + "/"), let bytes = try? Data(contentsOf: u) else { die("missing signed file: \(rel)") }
    message.append(Data(rel.utf8)); message.append(0)
    var n = UInt64(bytes.count).bigEndian
    withUnsafeBytes(of: &n) { message.append(contentsOf: $0) }
    message.append(bytes)
}
let key: Curve25519.Signing.PrivateKey
if let raw = try? Data(contentsOf: keyURL), let decoded = Data(base64Encoded: raw), let k = try? Curve25519.Signing.PrivateKey(rawRepresentation: decoded) { key = k }
else { key = Curve25519.Signing.PrivateKey(); try Data(key.rawRepresentation.base64EncodedString().utf8).write(to: keyURL, options: .atomic); chmod(keyURL.path, S_IRUSR | S_IWUSR) }
let sig = try key.signature(for: message)
let obj: [String: Any] = ["algorithm":"ed25519", "public_key_base64":key.publicKey.rawRepresentation.base64EncodedString(), "signature_base64":sig.base64EncodedString(), "signed_files":signed]
let out = try JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted,.sortedKeys])
try out.write(to: bundle.appendingPathComponent("signature.json"), options: .atomic)
print("Signed \(bundle.lastPathComponent) with \(signed.count) file(s).")
