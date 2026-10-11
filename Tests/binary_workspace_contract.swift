import Foundation

@main
struct BinaryWorkspaceContract {
    static func main() {
        let functionsText = """
        0x0000000100001000  0x0000000100001010        16 bytes  _entry
        0x0000000100001010  0x0000000100001020        16 bytes  _helper
        """
        let functions = VGGraphParser.functions(functionsText)
        precondition(functions.count == 2)
        precondition(functions[0].name == "_entry")
        precondition(functions[0].contains(0x100001008))

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
