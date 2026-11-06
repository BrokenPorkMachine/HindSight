import Foundation

@main
struct BinaryWorkspaceContract {
    static func main() {
        let input = Data([0, 1, 2, 3])
        precondition((try? VGBinaryPatch.bytes("01 aF")) == [1, 175])
        precondition((try? VGBinaryPatch.bytes("0z")) == nil)
        let first = VGBinaryPatch.Edit(offset: 0, expected: [0], replacement: [7])
        let last = VGBinaryPatch.Edit(offset: 3, expected: [3], replacement: [6])
        precondition((try? VGBinaryPatch.apply(input, edits: [last, first])) == Data([7, 1, 2, 6]))
        precondition((try? VGBinaryPatch.apply(input, edits: [first, first])) == nil)
        let stale = VGBinaryPatch.Edit(offset: 3, expected: [9], replacement: [6])
        precondition((try? VGBinaryPatch.apply(input, edits: [first, stale])) == nil)
        precondition(input == Data([0, 1, 2, 3]))
        precondition((try? VGBinaryPatch.apply(input, offset: 1, expected: [1, 2], replacement: [9, 8])) == Data([0, 9, 8, 3]))
        precondition((try? VGBinaryPatch.apply(input, offset: 1, expected: [0], replacement: [9])) == nil)
        precondition((try? VGBinaryPatch.apply(input, offset: UInt64.max, expected: [1], replacement: [9])) == nil)
        precondition((try? VGBinaryPatch.apply(input, offset: 0, expected: [0], replacement: [1, 2])) == nil)
        var ecu = Data(repeating: 0, count: 0x20000)
        ecu[0] = 1; ecu[1] = 2
        func put32(_ value: UInt32, _ offset: Int) {
            ecu[offset] = UInt8(value & 0xff); ecu[offset + 1] = UInt8((value >> 8) & 0xff)
            ecu[offset + 2] = UInt8((value >> 16) & 0xff); ecu[offset + 3] = UInt8((value >> 24) & 0xff)
        }
        put32(0, 0x1F810); put32(1, 0x1F814)
        put32(0, 0x1F818); put32(UInt32.max, 0x1F81C)
        let ecuDescriptors = VGECUChecksums.descriptors(in: ecu)
        precondition(ecuDescriptors.count == 1 && ecuDescriptors[0].computed == 0x0201)
        precondition(ecuDescriptors[0].verdict == .invalid)
        let repairs = VGECUChecksums.repairEdits(in: ecu, descriptors: ecuDescriptors)
        precondition(repairs.count == 1)
        let repairedECU = try! VGBinaryPatch.apply(ecu, edits: repairs)
        precondition(VGECUChecksums.descriptors(in: repairedECU).allSatisfy { $0.verdict == .verified })
        let xdf = Data("""
        <?xml version="1.0"?><XDFFORMAT><XDFTABLE uniqueid="0x1"><title>KFZW</title>
        <XDFAXIS id="z"><EMBEDDEDDATA mmedaddress="0x100"/></XDFAXIS></XDFTABLE></XDFFORMAT>
        """.utf8)
        let xdfDefinitions = try! VGXDFDefinitions.parse(xdf)
        precondition(xdfDefinitions.count == 1 && xdfDefinitions[0].title == "KFZW" && xdfDefinitions[0].address == 0x100)
        precondition(VGXDFDefinitions.evidence(for: xdfDefinitions[0], firmware: Data(repeating: 0, count: 0x80)).contains("not-found"))
        let relocationXDF = Data("""
        <?xml version="1.0"?><XDFFORMAT><XDFTABLE uniqueid="map"><title>Map</title>
        <XDFAXIS id="x"><EMBEDDEDDATA mmedaddress="0x20"/></XDFAXIS>
        <XDFAXIS id="y"><EMBEDDEDDATA mmedaddress="0x60"/></XDFAXIS>
        <XDFAXIS id="z"><EMBEDDEDDATA mmedaddress="0xA0"/></XDFAXIS></XDFTABLE></XDFFORMAT>
        """.utf8)
        let relocationDefinitions = try! VGXDFDefinitions.parse(relocationXDF)
        var reference = Data(repeating: 0, count: 0x200), target = Data(repeating: 0, count: 0x300)
        for (ordinal, base) in [0x20, 0x60, 0xA0].enumerated() {
            for byte in 0..<24 { reference[base + byte] = UInt8(ordinal * 40 + byte + 1) }
            target.replaceSubrange((base + 0x40)..<(base + 0x40 + 24), with: reference[base..<(base + 24)])
        }
        let relocation = VGXDFDefinitions.relocate(relocationDefinitions, reference: reference, target: target)[0]
        precondition(relocation.state == .verified && relocation.resolvedAddress == 0xE0 && relocation.matchedAnchors == 3)
        let functionsText = """
        0x0000000100001000  0x0000000100001010        16 bytes  _entry
        0x0000000100001010  0x0000000100001020        16 bytes  _helper
        """
        let functions = VGGraphParser.functions(functionsText)
        precondition(functions.count == 2)
        precondition(functions[0].name == "_entry")
        precondition(functions[0].contains(0x100001008))

        let cachedNames = VGGraphParser.functions("""
        0x0000000100001000  0x0000000100001010  16 bytes  <discovered>
        0x0000000100001010  0x0000000100001020  16 bytes  -[Widget
        """, symbolResolver: """
        range start=0x0000000100001000 end=0x0000000100001010 size=16 symbol=_exported source=dyld-export
        range start=0x0000000100001010 end=0x0000000100001020 size=16 symbol=<unnamed-function> source=function-start
        """, objcClasses: """
          instance method 0x3000 name=run: types=v24@0:8@16 imp=0x0000000100001010 resolved=-[Widget run:] confidence=validated
        """)
        precondition(cachedNames.map(\.name) == ["_exported", "-[Widget run:]"])

        let semantic = """
        AArch64 register semantics v45
        Reconstructed pseudocode
        ========================
        // 0x0000000100001000 _entry; exact symbol
        /* return type unresolved */ function_100001000()
        {
            call_target(0x0000000100001010);
        }
        // 0x0000000100001010 _helper; exact symbol
        /* return type unresolved */ function_100001010()
        {
        }
        """
        precondition(VGBinaryWorkspaceParser.pseudocode(semantic, in: functions[0])?.contains("call_target") == true)
        precondition(VGBinaryWorkspaceParser.pseudocode(semantic, in: functions[0])?.contains("function_100001010") == false)
        precondition(VGBinaryWorkspaceParser.pseudocodeSourceAddress("  0x0000000100001004 call_target(0x0000000100001010);") == 0x100001004)
        precondition(VGBinaryWorkspaceParser.pseudocodeSourceAddress("  call_target(0x0000000100001010);") == nil)
        precondition(VGBinaryWorkspaceParser.pseudocodeSourceAddress("  function_100001000()") == nil)
        precondition(VGBinaryWorkspaceParser.signatureEvidence("ABI evidence\nSignature evidence: _CFRelease") == "Signature evidence: _CFRelease")

        let xrefs = """
        0x0000000100001004  --call  -->  0x0000000100001010
        0x0000000100001008  --branch-->  0x000000010000100c
        """
        let calls = """
        0x0000000100001000 _entry  --BL@0x0000000100001004-->  0x0000000100001010 _helper
        """
        let references = VGBinaryWorkspaceParser.references(xrefs: xrefs, callGraph: calls)
        precondition(references.count == 2)
        precondition(references.contains(where: { $0.isCall && $0.source == 0x100001004 && $0.target == 0x100001010 }))

        let ir = """
        0x0000000100001000  op       mov x0, x1
        0x0000000100001004  branch   bl 0x0000000100001010
        0x0000000100001008  cbranch  cbz x0, 0x000000010000100c
        0x000000010000100c  opaque   .inst 0xffffffff
        0x0000000100001010  term     ret
        """
        let lifted = VGBinaryWorkspaceParser.lifted(ir, in: functions[0])
        precondition(lifted.count == 4)
        precondition(lifted[1].text.hasPrefix("call "))
        precondition(lifted[2].text.hasPrefix("if (condition"))
        precondition(lifted[3].confidence == .opaque)

        let disassembly = """
          0x0000000100001000  d2800020  mov x0, #0x1
          0x0000000100001004  94000003  bl 0x0000000100001010 ; call -> _helper
        """
        let rows = VGDisassemblyParser.parse(disassembly)
        let data = VGBinaryWorkspaceParser.instructionData(rows: rows, address: 0x100001004)
        precondition(data?.word == 0x94000003)
        precondition(data?.bytes == [0x03, 0x00, 0x00, 0x94])

        let flow = """
        0x0000000100001000  x0 <- constant
        0x0000000100001004  decoded semantics not yet modeled
        """
        let exact = VGBinaryWorkspaceParser.dataflowLines(flow, address: 0x100001000, function: functions[0])
        precondition(exact.count == 1 && exact[0].contains("constant"))

        print("Binary Workspace contract passed.")
    }
}
