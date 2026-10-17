import Foundation

@main
struct TargetModelContract {
    static func main() {
        precondition(VGTargetUtilities.parseHex("0x1000") == 0x1000)
        precondition(VGTargetUtilities.parseHex("ff") == 0xff)
        precondition(VGTargetUtilities.parseHex("not-hex") == nil)
        precondition(VGTargetUtilities.hex(0x1234) == "0x0000000000001234")
        precondition(VGTargetUtilities.add(UInt64.max, 1) == nil)
        precondition(VGTargetUtilities.applying(slide: 0x1000, to: 0x2000) == 0x3000)
        precondition(VGTargetUtilities.applying(slide: -0x1000, to: 0x2000) == 0x1000)
        precondition(VGTargetUtilities.applying(slide: -0x3000, to: 0x2000) == nil)
        let digest = VGTargetUtilities.fingerprint(["a", "b"])
        precondition(digest.hasPrefix("sha256:") && digest.count == 71)
        precondition(digest != VGTargetUtilities.fingerprint(["ab", "c"]))

        let modules = VGTargetUtilities.nativeModuleRows("0x0000000100004000 slide=+16384 preferred=0x0000000100000000  /tmp/Demo\n")
        precondition(modules.count == 1)
        precondition(modules[0].load == 0x100004000)
        precondition(modules[0].slide == 16384)
        precondition(modules[0].preferred == 0x100000000)
        precondition(modules[0].path == "/tmp/Demo")

        let row = VGAddressTranslation(space: "preferred-vm", address: "0x1000", detail: "demo", evidence: "test")
        precondition(VGTargetUtilities.unique([row, row]).count == 1)
        let session = VGSessionHeader(schemaVersion: 2, productVersion: "test", targetID: "target",
                                      targetFingerprint: digest, engineFingerprint: "sha256:build",
                                      modelFingerprint: digest, sourceFingerprint: nil, traceFingerprint: digest)
        let identity = VGTargetIdentity(id: "target", name: "test", kind: .staticArtifact,
                                        architecture: "arm64", backend: "test", state: "loaded",
                                        capabilities: [], addressSpaces: [], session: session)
        let export = try! JSONSerialization.jsonObject(with: JSONEncoder().encode(VGTargetSessionExport(target: identity, pivot: nil))) as! [String: Any]
        precondition(export["schemaVersion"] as? Int == 2)
        precondition(((export["target"] as? [String: Any])?["session"] as? [String: Any])?["traceFingerprint"] as? String == digest)
        print("Target model contract passed.")
    }
}
