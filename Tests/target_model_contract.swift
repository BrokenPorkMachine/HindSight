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

        let modules = VGTargetUtilities.nativeModuleRows("0x0000000100004000 slide=+16384 preferred=0x0000000100000000  /tmp/Demo\n")
        precondition(modules.count == 1)
        precondition(modules[0].load == 0x100004000)
        precondition(modules[0].slide == 16384)
        precondition(modules[0].preferred == 0x100000000)
        precondition(modules[0].path == "/tmp/Demo")

        let row = VGAddressTranslation(space: "preferred-vm", address: "0x1000", detail: "demo", evidence: "test")
        precondition(VGTargetUtilities.unique([row, row]).count == 1)
        print("Target model contract passed.")
    }
}
