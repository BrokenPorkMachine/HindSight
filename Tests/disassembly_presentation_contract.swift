import Foundation

@main
struct DisassemblyPresentationContract {
    static func main() {
        let page = """
VA (unslid)        File offset       Word      Instruction
0000000100001000  0000000000004000  d503201f  nop
0000000100001004  0000000000004004  94000002  bl 0x10000100c

2 of 2 words shown; 2 decoded, 0 unsupported. Start word: 0.
"""
        let pageRows = VGDisassemblyParser.parse(page)
        precondition(pageRows.filter { $0.kind == .instruction }.count == 2)
        let first = pageRows.first { $0.kind == .instruction }!
        precondition(first.address == "0000000100001000")
        precondition(first.fileOffset == "0000000000004000")
        precondition(first.word == "d503201f")
        precondition(first.mnemonic == "nop")

        let annotated = """
0x0000000100001000 <_main>:
  0x0000000100001000  d503201f  nop
  0x0000000100001004  94000002  bl 0x000000010000100c  ; call -> _helper
"""
        let annotatedRows = VGDisassemblyParser.parse(annotated)
        precondition(annotatedRows.contains { $0.kind == .symbol })
        let call = annotatedRows.first { $0.mnemonic == "bl" }!
        precondition(call.isCall)
        precondition(call.isControlFlow)
        precondition(call.annotation == "call -> _helper")
        precondition(call.normalizedAddress == "0000000100001004")
        print("Disassembly presentation contract passed.")
    }
}
