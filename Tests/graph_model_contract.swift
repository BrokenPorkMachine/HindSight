import Foundation

@main
struct GraphModelContract {
    static func main() {
        let cfg = """
        block B0000 0x0000000100001000..0x0000000100001008
            b.cond      0x0000000100001004 -> 0x0000000100001010
            fallthrough 0x0000000100001004 -> 0x0000000100001008
        block B0001 0x0000000100001008..0x0000000100001010
            call        0x000000010000100c -> 0x0000000100001020
            b           0x000000010000100c -> 0x0000000100001018
        block B0002 0x0000000100001010..0x0000000100001018
            b           0x0000000100001014 -> 0x0000000100001018
        block B0003 0x0000000100001018..0x0000000100001020
        block B0004 0x0000000100001020..0x0000000100001028
        """
        let graph = VGGraphParser.cfg(cfg)
        precondition(graph.blocks.count == 5)
        precondition(graph.edges.count == 5)
        precondition(graph.blocks[0].id == "B0000")
        precondition(graph.blocks[0].byteCount == 8)
        precondition(graph.block(containing: 0x100001004)?.id == "B0000")
        precondition(graph.block(startingAt: 0x100001018)?.id == "B0003")
        precondition(graph.edges.contains { $0.kind == "fallthrough" && $0.targetBlockID == "B0001" })
        precondition(graph.edges.contains { $0.isCall && $0.targetBlockID == "B0004" })

        let layout = graph.layout(limit: 240)
        precondition(layout.positions.count == 5)
        precondition(layout.width >= 760)
        precondition(layout.height >= 520)
        precondition(layout.visibleBlockIDs == Set(["B0000", "B0001", "B0002", "B0003", "B0004"]))
        if let root = layout.positions["B0000"], let next = layout.positions["B0001"] {
            precondition(next.y > root.y)
        } else {
            preconditionFailure("missing deterministic positions")
        }

        let funcs = VGGraphParser.functions("""
        0x0000000100001000  0x0000000100001020        32 bytes  _main
        0x0000000100001020  0x0000000100001030        16 bytes  _helper
        """)
        precondition(funcs.count == 2)
        precondition(funcs[0].name == "_main")
        precondition(funcs[0].contains(0x100001018))
        precondition(!funcs[0].contains(0x100001020))
        precondition(VGGraphParser.address(from: "pc=0x0000000100001018 stopped") == 0x100001018)
        precondition(VGGraphParser.address(from: "no address") == nil)

        let candidates = VGGraphParser.functions("0x1000 name=_firmware_entry source=nlist section=1\n0x1020 name=sub_1020 source=prologue candidate")
        precondition(candidates.count == 2 && candidates[0].name == "_firmware_entry")
        precondition(candidates[0].end == candidates[0].start && candidates[0].contains(0x1000))
        precondition(!candidates[0].contains(0x1001) && candidates[0].extentLabel == "Extent unknown")
        print("graph model contract passed")
    }
}
