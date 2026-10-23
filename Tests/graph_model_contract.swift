import Foundation

@main
struct GraphModelContract {
    static func main() {
        let viewport = VGCanvasViewport(contentSize: CGSize(width: 1000, height: 1200), viewportSize: CGSize(width: 400, height: 600), zoom: 1)
        precondition(viewport.origin.width == -300)
        precondition(viewport.visibleRect == CGRect(x: 300, y: 0, width: 400, height: 600))
        precondition(viewport.clampedPan(CGSize(width: 999, height: -999)) == CGSize(width: 300, height: -600))
        precondition(viewport.pan(centering: CGPoint(x: 0, y: 0)) == CGSize(width: 300, height: 0))
        precondition(viewport.pan(centering: CGPoint(x: 1000, y: 1200)) == CGSize(width: -300, height: -600))
        precondition(viewport.fittedZoom(widthOnly: false, maximum: 1) == 0.4)
        let smallViewport = VGCanvasViewport(contentSize: CGSize(width: 200, height: 100), viewportSize: CGSize(width: 400, height: 600), zoom: 1)
        let scaledMap = VGMinimapProjection(contentSize: CGSize(width: 10000, height: 20000), mapSize: CGSize(width: 190, height: 120))
        let node = scaledMap.project(CGRect(x: 100, y: 200, width: 290, height: 126))
        precondition(abs(node.width / node.height - 290.0 / 126.0) < 0.000001)
        let scaledViewport = VGCanvasViewport(contentSize: CGSize(width: 10000, height: 20000), viewportSize: CGSize(width: 800, height: 600), zoom: 0.8)
        precondition(scaledViewport.visibleRect.width == 1000 && scaledViewport.visibleRect.height == 750)
        let mapBounds = scaledMap.project(CGRect(origin: .zero, size: scaledViewport.contentSize))
        precondition(mapBounds.minX >= 0 && mapBounds.maxX <= 190 && mapBounds.minY >= 0 && mapBounds.maxY <= 120)
        precondition(smallViewport.origin.width == 100)
        precondition(smallViewport.clampedPan(CGSize(width: 50, height: -50)) == .zero)
        for contentSize in [CGSize(width: 1000, height: 4000), CGSize(width: 4000, height: 1000)] {
            let minimap = VGMinimapProjection(contentSize: contentSize, mapSize: CGSize(width: 200, height: 120))
            let graphCenter = CGPoint(x: contentSize.width / 2, y: contentSize.height / 2)
            let mapCenter = minimap.project(graphCenter)
            precondition(abs(mapCenter.x - 100) < 0.000001 && abs(mapCenter.y - 60) < 0.000001,
                         "tall and wide minimaps must center the graph in both axes")
            let restored = minimap.unproject(mapCenter)
            precondition(abs(restored.x - graphCenter.x) < 0.000001 && abs(restored.y - graphCenter.y) < 0.000001,
                         "minimap navigation must account for the centered origin")
            precondition(minimap.unproject(.zero) == .zero)
            let projectedBounds = minimap.project(CGRect(origin: .zero, size: contentSize))
            precondition(abs(projectedBounds.midX - 100) < 0.000001 && abs(projectedBounds.midY - 60) < 0.000001)
        }
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
        let narrow = graph.layout(availableWidth: 480, availableHeight: 700)
        let leftBranch = narrow.positions["B0001"]!
        let rightBranch = narrow.positions["B0002"]!
        precondition(leftBranch.y == rightBranch.y, "narrow viewports must preserve branch siblings on one layer")
        precondition(rightBranch.x - leftBranch.x >= 290, "branch nodes must remain separate")
        precondition(narrow.positions["B0000"]!.x == narrow.width / 2, "entry must be centered above its branches")
        let wide = graph.layout(availableWidth: 1800, availableHeight: 1200)
        precondition(wide.positions["B0002"]!.x - wide.positions["B0001"]!.x >= 1100,
                     "wide flow graphs should spread branch paths across the map")
        precondition(wide.positions["B0004"]!.y - wide.positions["B0000"]!.y >= 900,
                     "wide flow graphs should use available vertical map space")

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

        let paddedCandidates = VGGraphParser.functions("  0x1030    name=_padded_entry source=nlist")
        precondition(paddedCandidates.count == 1 && paddedCandidates[0].name == "_padded_entry")
        let entryBlocks = (0..<24).map { index in
            VGCFGBlock(id: "entry\(index)", start: UInt64(index * 16), end: UInt64(index * 16 + 8), edges: [])
        }
        let entries = VGCFGGraph(blocks: entryBlocks, edges: [])
        let wrapped = entries.layout(availableWidth: 1000, availableHeight: 700)
        precondition(wrapped.width <= 1000, "entry blocks must not form an unbounded horizontal strip")
        precondition(Set(wrapped.positions.values.map(\.y)).count == 12)
        precondition(wrapped.positions.values.allSatisfy { $0.x >= 145 && $0.x + 145 <= wrapped.width })
        precondition(wrapped.positions["entry2"]!.y > wrapped.positions["entry0"]!.y)
        print("graph model contract passed")
    }
}
