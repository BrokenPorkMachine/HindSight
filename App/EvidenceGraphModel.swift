import Foundation

struct VGEvidenceRuntimeFact: Identifiable, Hashable {
    let id: Int
    let timestamp: TimeInterval
    let runtimePC: UInt64
    let preferredPC: UInt64?
    let functionName: String?
    let imageName: String?
    let reason: String
    let confidence: String
}

struct VGEvidenceMemoryFact: Identifiable, Hashable {
    let id: Int
    let timestamp: TimeInterval
    let address: UInt64
    let preview: String
}

struct VGEvidenceAnalystAssertion: Identifiable, Hashable {
    let id: String
    let title: String
    let detail: String
    let status: String
    let address: UInt64?
}

struct VGEvidenceNode: Identifiable, Hashable {
    enum Kind: String, CaseIterable, Hashable {
        case investigation
        case function
        case address
        case staticEvidence
        case reaEvidence
        case correlationEvidence
        case runtimeObservation
        case memoryObservation
        case analystNote
        case analystHypothesis

        var title: String {
            switch self {
            case .investigation: return "Investigation"
            case .function: return "Function"
            case .address: return "Address"
            case .staticEvidence: return "Static evidence"
            case .reaEvidence: return "REA evidence"
            case .correlationEvidence: return "Correlation evidence"
            case .runtimeObservation: return "Runtime observation"
            case .memoryObservation: return "Memory observation"
            case .analystNote: return "Analyst note"
            case .analystHypothesis: return "Analyst hypothesis"
            }
        }
    }

    enum Provenance: String, CaseIterable, Hashable {
        case analysis
        case debugger
        case correlation
        case analyst

        var title: String {
            switch self {
            case .analysis: return "Analysis"
            case .debugger: return "Debugger"
            case .correlation: return "Correlation"
            case .analyst: return "Analyst"
            }
        }
    }

    enum Confidence: String, CaseIterable, Hashable {
        case observed
        case derived
        case correlated
        case asserted
        case unresolved
    }

    let id: String
    let kind: Kind
    let provenance: Provenance
    let confidence: Confidence
    let title: String
    let detail: String
    let address: UInt64?

    var addressText: String? { address.map { String(format: "0x%016llx", $0) } }
}

struct VGEvidenceEdge: Identifiable, Hashable {
    enum Relation: String, CaseIterable, Hashable {
        case contains
        case references
        case observedAt = "observed-at"
        case follows
        case nearbyMemory = "nearby-memory"
        case annotates
        case supports
    }

    let id: String
    let sourceID: String
    let targetID: String
    let relation: Relation
    let confidence: VGEvidenceNode.Confidence
    let explanation: String
}

struct VGEvidenceGraphPoint: Hashable {
    let x: Double
    let y: Double
}

struct VGEvidenceGraphLayout {
    let positions: [String: VGEvidenceGraphPoint]
    let width: Double
    let height: Double
    let visibleNodeIDs: Set<String>
}

struct VGEvidenceEdgeRoute: Hashable {
    let start: VGEvidenceGraphPoint
    let control1: VGEvidenceGraphPoint
    let control2: VGEvidenceGraphPoint
    let end: VGEvidenceGraphPoint
}

struct VGEvidenceGraph: Hashable {
    let nodes: [VGEvidenceNode]
    let edges: [VGEvidenceEdge]

    func node(_ id: String) -> VGEvidenceNode? { nodes.first { $0.id == id } }

    func connected(to id: String) -> [VGEvidenceEdge] {
        edges.filter { $0.sourceID == id || $0.targetID == id }
    }

    /// Route related edges through distinct card ports and bezier channels.
    /// Stable sorting makes the layout repeatable across launches.
    func edgeRoutes(positions: [String: VGEvidenceGraphPoint], nodeWidth: Double = 218,
                    nodeHeight: Double = 72) -> [String: VGEvidenceEdgeRoute] {
        let outgoing = Dictionary(grouping: edges, by: \.sourceID)
        let incoming = Dictionary(grouping: edges, by: \.targetID)
        func offset(for edge: VGEvidenceEdge, peers: [VGEvidenceEdge]) -> Double {
            let ordered = peers.sorted { $0.id < $1.id }
            guard ordered.count > 1, let index = ordered.firstIndex(of: edge) else { return 0 }
            let spacing = min(22, 44 / Double(ordered.count - 1))
            return (Double(index) - Double(ordered.count - 1) / 2) * spacing
        }
        var result: [String: VGEvidenceEdgeRoute] = [:]
        for edge in edges {
            guard let source = positions[edge.sourceID], let target = positions[edge.targetID] else { continue }
            let sourceOffset = offset(for: edge, peers: outgoing[edge.sourceID] ?? [])
            let targetOffset = offset(for: edge, peers: incoming[edge.targetID] ?? [])
            let dx = target.x - source.x, dy = target.y - source.y
            let route: VGEvidenceEdgeRoute
            if abs(dx) >= nodeWidth * 0.75 {
                let direction = dx >= 0 ? 1.0 : -1.0
                let start = VGEvidenceGraphPoint(x: source.x + direction * nodeWidth / 2, y: source.y + sourceOffset)
                let end = VGEvidenceGraphPoint(x: target.x - direction * nodeWidth / 2, y: target.y + targetOffset)
                let bend = (sourceOffset - targetOffset) * 0.65
                let span = end.x - start.x
                route = VGEvidenceEdgeRoute(start: start,
                    control1: VGEvidenceGraphPoint(x: start.x + span * 0.38 + bend, y: start.y),
                    control2: VGEvidenceGraphPoint(x: end.x - span * 0.38 + bend, y: end.y), end: end)
            } else {
                let direction = dy >= 0 ? 1.0 : -1.0
                let start = VGEvidenceGraphPoint(x: source.x + sourceOffset, y: source.y + direction * nodeHeight / 2)
                let end = VGEvidenceGraphPoint(x: target.x + targetOffset, y: target.y - direction * nodeHeight / 2)
                let span = end.y - start.y
                let bend = (sourceOffset - targetOffset) * 0.65
                route = VGEvidenceEdgeRoute(start: start,
                    control1: VGEvidenceGraphPoint(x: start.x + bend, y: start.y + span * 0.38),
                    control2: VGEvidenceGraphPoint(x: end.x + bend, y: end.y - span * 0.38), end: end)
            }
            result[edge.id] = route
        }
        return result
    }

    func filtered(query: String, provenances: Set<VGEvidenceNode.Provenance>) -> VGEvidenceGraph {
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        var visible = Set(nodes.filter { node in
            if node.kind == .investigation { return true }
            guard provenances.contains(node.provenance) else { return false }
            guard !normalized.isEmpty else { return true }
            return node.title.lowercased().contains(normalized)
                || node.detail.lowercased().contains(normalized)
                || (node.addressText?.lowercased().contains(normalized) ?? false)
                || node.kind.rawValue.lowercased().contains(normalized)
                || node.confidence.rawValue.lowercased().contains(normalized)
        }.map(\.id))

        // Preserve direct context around matches without exploding back to the entire graph.
        if !normalized.isEmpty {
            let matched = visible
            for edge in edges where matched.contains(edge.sourceID) || matched.contains(edge.targetID) {
                if let source = node(edge.sourceID), source.kind == .investigation || provenances.contains(source.provenance) { visible.insert(edge.sourceID) }
                if let target = node(edge.targetID), target.kind == .investigation || provenances.contains(target.provenance) { visible.insert(edge.targetID) }
            }
        }
        return VGEvidenceGraph(
            nodes: nodes.filter { visible.contains($0.id) },
            edges: edges.filter { visible.contains($0.sourceID) && visible.contains($0.targetID) }
        )
    }

    func layout(limit: Int = 180) -> VGEvidenceGraphLayout {
        let visible = Array(nodes.prefix(max(1, limit)))
        guard !visible.isEmpty else {
            return VGEvidenceGraphLayout(positions: [:], width: 960, height: 620, visibleNodeIDs: [])
        }

        let laneOrder: [VGEvidenceNode.Kind] = [
            .investigation, .function, .address, .staticEvidence, .correlationEvidence,
            .reaEvidence, .runtimeObservation, .memoryObservation, .analystNote, .analystHypothesis
        ]
        // Keep evidence families in separate lanes across the full canvas.
        // Co-locating static, runtime, memory and analyst nodes at the center
        // made otherwise distinct paths collapse into an unreadable bundle.
        let laneSpacing = 250.0
        let laneMargin = 150.0
        let laneX = Dictionary(uniqueKeysWithValues: laneOrder.enumerated().map {
            ($0.element, laneMargin + Double($0.offset) * laneSpacing)
        })

        var grouped: [VGEvidenceNode.Kind: [VGEvidenceNode]] = [:]
        for node in visible { grouped[node.kind, default: []].append(node) }
        for kind in laneOrder {
            grouped[kind]?.sort {
                if $0.address != $1.address { return ($0.address ?? UInt64.max) < ($1.address ?? UInt64.max) }
                return $0.id < $1.id
            }
        }

        let top = 78.0
        let rowGap = 92.0
        var positions: [String: VGEvidenceGraphPoint] = [:]
        var maxRows = 1
        var rowsByLane: [Double: Int] = [:]
        for kind in laneOrder {
            let items = grouped[kind] ?? []
            let x = laneX[kind] ?? 730
            let startRow = rowsByLane[x, default: 0]
            for (index, node) in items.enumerated() {
                let y = top + Double(startRow + index) * rowGap
                positions[node.id] = VGEvidenceGraphPoint(x: x, y: y)
            }
            rowsByLane[x] = startRow + items.count
            maxRows = max(maxRows, startRow + items.count)
        }
        let regularBottom = top + Double(maxRows) * rowGap + 90
        let height = max(620, regularBottom)
        return VGEvidenceGraphLayout(
            positions: positions,
            width: laneMargin * 2 + Double(laneOrder.count - 1) * laneSpacing,
            height: height,
            visibleNodeIDs: Set(visible.map(\.id))
        )
    }

    var summary: (nodes: Int, edges: Int, observed: Int, correlated: Int, unresolved: Int) {
        (
            nodes.count,
            edges.count,
            nodes.filter { $0.confidence == .observed }.count,
            nodes.filter { $0.confidence == .correlated }.count,
            nodes.filter { $0.confidence == .unresolved }.count
        )
    }
}

enum VGEvidenceGraphBuilder {
    private static func parseAddress(_ text: String) -> UInt64? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let raw = trimmed.lowercased().hasPrefix("0x") ? String(trimmed.dropFirst(2)) : trimmed
        guard !raw.isEmpty, raw.count <= 16, raw.allSatisfy({ $0.isHexDigit }) else { return nil }
        return UInt64(raw, radix: 16)
    }

    static func allAddresses(in text: String) -> [UInt64] {
        guard let regex = try? NSRegularExpression(pattern: #"(?i)0x[0-9a-f]{1,16}"#) else { return [] }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.matches(in: text, range: range).compactMap { match in
            guard let swiftRange = Range(match.range, in: text) else { return nil }
            return parseAddress(String(text[swiftRange]))
        }
    }

    private static func clip(_ text: String, max: Int = 140) -> String {
        let oneLine = text.replacingOccurrences(of: "\t", with: " ")
            .replacingOccurrences(of: "  ", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard oneLine.count > max else { return oneLine }
        return String(oneLine.prefix(max - 1)) + "…"
    }

    private static func function(containing address: UInt64, functions: [VGFunctionRecord]) -> VGFunctionRecord? {
        functions.first { $0.contains(address) }
    }

    private static func functionNodeID(_ function: VGFunctionRecord) -> String { "fn:\(function.start)" }
    private static func addressNodeID(_ address: UInt64) -> String { "addr:\(address)" }

    static func build(
        functionsText: String,
        xrefsText: String,
        symbolProvenanceText: String,
        runtime: [VGEvidenceRuntimeFact],
        memory: [VGEvidenceMemoryFact],
        note: String,
        assertions: [VGEvidenceAnalystAssertion] = [],
        selectedAddress: UInt64?,
        reaEvidence: [VGREAAgentEvidence] = [],
        imageName: String? = nil
    ) -> VGEvidenceGraph {
        let rootID = "investigation"
        let functions = Array(VGGraphParser.functions(functionsText).prefix(96))
        var nodes: [VGEvidenceNode] = [
            VGEvidenceNode(
                id: rootID,
                kind: .investigation,
                provenance: .analysis,
                confidence: .derived,
                title: "Current investigation",
                detail: "Evidence graph root. Edges preserve source and confidence rather than collapsing observed, derived, correlated, and analyst assertions.",
                address: nil
            )
        ]
        var edges: [VGEvidenceEdge] = []
        var nodeIDs = Set([rootID])
        var edgeIDs = Set<String>()

        func appendNode(_ node: VGEvidenceNode) {
            if nodeIDs.insert(node.id).inserted { nodes.append(node) }
        }
        func appendEdge(_ source: String, _ target: String, _ relation: VGEvidenceEdge.Relation, _ confidence: VGEvidenceNode.Confidence, _ explanation: String) {
            guard source != target else { return }
            let id = "\(source)>\(relation.rawValue)>\(target)"
            if edgeIDs.insert(id).inserted {
                edges.append(VGEvidenceEdge(id: id, sourceID: source, targetID: target, relation: relation, confidence: confidence, explanation: explanation))
            }
        }
        func ensureAddressNode(_ address: UInt64, detail: String = "Referenced address") -> String {
            if let function = function(containing: address, functions: functions) { return functionNodeID(function) }
            let id = addressNodeID(address)
            appendNode(VGEvidenceNode(
                id: id,
                kind: .address,
                provenance: .analysis,
                confidence: .derived,
                title: String(format: "0x%016llx", address),
                detail: detail,
                address: address
            ))
            appendEdge(rootID, id, .contains, .derived, "Address surfaced by bounded static evidence but is not inside a discovered function range.")
            return id
        }

        for function in functions {
            let id = functionNodeID(function)
            let imageDetail = imageName.map { "\($0) · " } ?? ""
            appendNode(VGEvidenceNode(
                id: id,
                kind: .function,
                provenance: .analysis,
                confidence: .derived,
                title: function.name,
                detail: "\(imageDetail)\(function.startText) · \(function.extentLabel)",
                address: function.start
            ))
            appendEdge(rootID, id, .contains, .derived, "Function range is derived from the bounded static-analysis result.")
        }

        for line in xrefsText.split(separator: "\n", omittingEmptySubsequences: true).map(String.init).prefix(160) {
            let addresses = allAddresses(in: line)
            guard addresses.count >= 2 else { continue }
            let source = ensureAddressNode(addresses[0], detail: clip(line))
            let target = ensureAddressNode(addresses[1], detail: clip(line))
            appendEdge(source, target, .references, .derived, "Static reference evidence: \(clip(line))")
        }

        for (index, line) in symbolProvenanceText.split(separator: "\n", omittingEmptySubsequences: true).map(String.init).prefix(64).enumerated() {
            guard let address = allAddresses(in: line).first else { continue }
            let id = "static:\(index):\(address)"
            appendNode(VGEvidenceNode(
                id: id,
                kind: .staticEvidence,
                provenance: .analysis,
                confidence: .derived,
                title: "Symbol provenance",
                detail: clip(line),
                address: address
            ))
            let target = ensureAddressNode(address, detail: clip(line))
            appendEdge(id, target, .supports, .derived, "Static provenance record supports the correlated address or function node.")
        }

        for evidence in reaEvidence.suffix(32) {
            let id = "rea:\(evidence.id)"
            appendNode(VGEvidenceNode(
                id: id,
                kind: .reaEvidence,
                provenance: .analysis,
                confidence: .observed,
                title: "REA \(evidence.operation)",
                detail: "\(URL(fileURLWithPath: evidence.targetPath).lastPathComponent) · \(evidence.provider) · SHA-256 \(evidence.responseSHA256)\(evidence.evidenceID.map { " · Evidence \($0)" } ?? "")",
                address: nil
            ))
            appendEdge(rootID, id, .contains, .observed, "REA response retained with explicit target, provider, and SHA-256 provenance.")
        }

        var runtimeNodeIDs: [(VGEvidenceRuntimeFact, String)] = []
        for fact in runtime.sorted(by: { $0.timestamp < $1.timestamp }).suffix(64) {
            let id = "runtime:\(fact.id)"
            let runtimeDetail = [
                fact.imageName,
                String(format: "runtime 0x%016llx", fact.runtimePC),
                fact.reason.isEmpty ? nil : fact.reason
            ]
            .compactMap { $0 }
            .joined(separator: " · ")
            appendNode(VGEvidenceNode(
                id: id,
                kind: .runtimeObservation,
                provenance: .debugger,
                confidence: .observed,
                title: fact.functionName ?? "Runtime observation",
                detail: runtimeDetail,
                address: nil
            ))
            appendEdge(rootID, id, .contains, .observed, "Debugger observation is retained as runtime evidence independently of static correlation.")

            let correlationID = "correlation:\(fact.id)"
            if let preferred = fact.preferredPC {
                let correlationDetail = [fact.imageName, fact.confidence.isEmpty ? nil : fact.confidence]
                    .compactMap { $0 }
                    .joined(separator: " · ")
                appendNode(VGEvidenceNode(
                    id: correlationID,
                    kind: .correlationEvidence,
                    provenance: .correlation,
                    confidence: .correlated,
                    title: fact.functionName ?? "Runtime ↔ static mapping",
                    detail: correlationDetail.isEmpty ? "Evidence-backed runtime/static mapping" : correlationDetail,
                    address: preferred
                ))
                appendEdge(id, correlationID, .supports, .correlated, "This correlation record maps an observed runtime endpoint to static evidence; it is separate from the observation itself.")
                let target = ensureAddressNode(preferred, detail: "Runtime-correlated static address")
                appendEdge(correlationID, target, .observedAt, .correlated, "Runtime/static correlation resolves to this preferred address through the recorded mapping evidence.")
            } else {
                appendNode(VGEvidenceNode(
                    id: correlationID,
                    kind: .correlationEvidence,
                    provenance: .correlation,
                    confidence: .unresolved,
                    title: "Static correlation unresolved",
                    detail: fact.confidence.isEmpty ? "No unique static mapping established" : fact.confidence,
                    address: nil
                ))
                appendEdge(id, correlationID, .supports, .unresolved, "The runtime observation is real, but HindSight could not establish a unique static mapping for it.")
            }
            runtimeNodeIDs.append((fact, id))
        }
        for pair in zip(runtimeNodeIDs, runtimeNodeIDs.dropFirst()) {
            appendEdge(pair.0.1, pair.1.1, .follows, .observed, "Chronological ordering of captured debugger observations. This edge is not a complete instruction trace.")
        }

        for fact in memory.sorted(by: { $0.timestamp < $1.timestamp }).suffix(32) {
            let id = "memory:\(fact.id)"
            appendNode(VGEvidenceNode(
                id: id,
                kind: .memoryObservation,
                provenance: .debugger,
                confidence: .observed,
                title: String(format: "memory 0x%016llx", fact.address),
                detail: clip(fact.preview, max: 96),
                address: fact.address
            ))
            if let nearest = runtimeNodeIDs.min(by: { abs($0.0.timestamp - fact.timestamp) < abs($1.0.timestamp - fact.timestamp) }), abs(nearest.0.timestamp - fact.timestamp) <= 1.5 {
                appendEdge(nearest.1, id, .nearbyMemory, .observed, "Explicit memory read captured within 1.5 seconds of this debugger observation; temporal proximity does not imply causation.")
            } else {
                appendEdge(rootID, id, .contains, .observed, "Explicit debugger memory read with no nearby captured runtime stop.")
            }
        }

        if let selectedAddress {
            let selectedID = ensureAddressNode(selectedAddress, detail: "Current analyst selection")
            appendEdge(rootID, selectedID, .supports, .derived, "Current investigation selection anchors the graph view; selection is not promoted to evidence confidence.")
        }

        let trimmedNote = note.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedNote.isEmpty {
            let id = "analyst-note"
            appendNode(VGEvidenceNode(
                id: id,
                kind: .analystNote,
                provenance: .analyst,
                confidence: .asserted,
                title: "Analyst note",
                detail: clip(trimmedNote, max: 240),
                address: selectedAddress
            ))
            if let selectedAddress {
                let target = ensureAddressNode(selectedAddress, detail: "Address selected when analyst note is shown")
                appendEdge(id, target, .annotates, .asserted, "Analyst-authored note associated with the current selected address. This is an assertion, not machine-derived evidence.")
            } else {
                appendEdge(id, rootID, .annotates, .asserted, "Analyst-authored investigation note with no selected address anchor.")
            }
        }

        for assertion in assertions.prefix(64) {
            let id = "hypothesis:\(assertion.id)"
            appendNode(VGEvidenceNode(
                id: id,
                kind: .analystHypothesis,
                provenance: .analyst,
                confidence: .asserted,
                title: assertion.title,
                detail: "[\(assertion.status)] \(clip(assertion.detail, max: 220))",
                address: assertion.address
            ))
            if let address = assertion.address {
                let target = ensureAddressNode(address, detail: "Address linked by analyst hypothesis")
                appendEdge(id, target, .annotates, .asserted, "Analyst-authored hypothesis linked to this address. Hypothesis status is an analyst judgement, not machine proof.")
            } else {
                appendEdge(id, rootID, .annotates, .asserted, "Analyst-authored hypothesis with no address anchor. Hypothesis status is not promoted to observed evidence.")
            }
        }

        return VGEvidenceGraph(nodes: nodes, edges: edges)
    }
}
