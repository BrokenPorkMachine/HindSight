import Foundation

@main
struct EvidenceGraphContract {
    static func main() {
        let functions = """
        0x0000000000001000 0x0000000000001020 32 seed func_main
        0x0000000000002000 0x0000000000002020 32 seed func_worker
        """
        let xrefs = """
        0x0000000000001004 -- call --> 0x0000000000002000
        0x0000000000001010 -- data --> 0x0000000000003000
        """
        let provenance = """
        0x0000000000001000 symbol=func_main source=nlist
        0x0000000000002000 symbol=func_worker source=LC_FUNCTION_STARTS
        """
        let runtime = [
            VGEvidenceRuntimeFact(id: 1, timestamp: 10.0, runtimePC: 0x5000, preferredPC: 0x1004, functionName: "func_main", imageName: "Demo", reason: "breakpoint", confidence: "unique-module+slide"),
            VGEvidenceRuntimeFact(id: 2, timestamp: 11.0, runtimePC: 0x6000, preferredPC: 0x2000, functionName: "func_worker", imageName: "Demo", reason: "step", confidence: "unique-module+slide"),
            VGEvidenceRuntimeFact(id: 3, timestamp: 12.0, runtimePC: 0x7000, preferredPC: nil, functionName: nil, imageName: nil, reason: "ambiguous image", confidence: "unresolved")
        ]
        let memory = [
            VGEvidenceMemoryFact(id: 1, timestamp: 11.2, address: 0x9000, preview: "41 42 43 44")
        ]
        let graph = VGEvidenceGraphBuilder.build(
            functionsText: functions,
            xrefsText: xrefs,
            symbolProvenanceText: provenance,
            runtime: runtime,
            memory: memory,
            note: "Suspect call edge at 0x1004",
            assertions: [VGEvidenceAnalystAssertion(id: "h1", title: "Auth path", detail: "Call may gate access", status: "Supported", address: 0x1004)],
            selectedAddress: 0x1004
        )

        precondition(graph.nodes.contains(where: { $0.kind == .function && $0.title == "func_main" }))
        precondition(graph.nodes.contains(where: { $0.kind == .function && $0.title == "func_worker" }))
        precondition(graph.edges.contains(where: { $0.relation == .references }))
        precondition(graph.nodes.filter { $0.kind == .runtimeObservation && $0.confidence == .observed }.count == 3)
        precondition(graph.nodes.filter { $0.kind == .correlationEvidence && $0.confidence == .correlated }.count == 2)
        precondition(graph.edges.contains(where: { $0.relation == .observedAt && $0.confidence == .correlated }))
        precondition(graph.nodes.contains(where: { $0.kind == .correlationEvidence && $0.confidence == .unresolved }))
        precondition(graph.edges.contains(where: { $0.relation == .nearbyMemory && $0.confidence == .observed }))
        precondition(graph.nodes.contains(where: { $0.kind == .analystNote && $0.confidence == .asserted }))
        precondition(graph.nodes.contains(where: { $0.kind == .analystHypothesis && $0.title == "Auth path" && $0.confidence == .asserted }))
        precondition(graph.edges.contains(where: { $0.relation == .annotates && $0.confidence == .asserted }))
        precondition(graph.edges.filter { $0.relation == .follows }.count == 2)
        let filtered = graph.filtered(query: "func_worker", provenances: Set(VGEvidenceNode.Provenance.allCases))
        precondition(filtered.nodes.contains(where: { $0.title == "func_worker" }))
        precondition(filtered.nodes.count < graph.nodes.count)
        let layout = graph.layout()
        precondition(layout.positions.count == graph.nodes.count)
        precondition(layout.width >= 1200 && layout.height >= 620)
        let points = Array(layout.positions.values)
        for index in points.indices {
            for other in points.indices where other > index {
                precondition(abs(points[index].x - points[other].x) >= 218 || abs(points[index].y - points[other].y) >= 72,
                             "evidence cards sharing a lane must not overlap")
            }
        }
        print("evidence graph contract passed: \(graph.nodes.count) nodes, \(graph.edges.count) edges")
    }
}
