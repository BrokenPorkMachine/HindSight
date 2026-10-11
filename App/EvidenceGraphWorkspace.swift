import SwiftUI

struct VGEvidenceGraphWorkspace: View {
    let graph: VGEvidenceGraph
    let onSelectAddress: (String) -> Void
    let onOpenWorkspace: (String) -> Void

    @State private var query = ""
    @State private var selectedNodeID: String?
    @State private var zoom: CGFloat = 0.88
    @State private var zoomAnchor: CGFloat = 0.88
    @State private var includeAnalysis = true
    @State private var includeDebugger = true
    @State private var includeCorrelation = true
    @State private var includeAnalyst = true

    private var provenances: Set<VGEvidenceNode.Provenance> {
        var value = Set<VGEvidenceNode.Provenance>()
        if includeAnalysis { value.insert(.analysis) }
        if includeDebugger { value.insert(.debugger) }
        if includeCorrelation { value.insert(.correlation) }
        if includeAnalyst { value.insert(.analyst) }
        return value
    }

    private var filtered: VGEvidenceGraph { graph.filtered(query: query, provenances: provenances) }
    private var layout: VGEvidenceGraphLayout { filtered.layout() }
    private var selected: VGEvidenceNode? {
        if let selectedNodeID, let node = graph.node(selectedNodeID) { return node }
        return filtered.nodes.first(where: { $0.kind != .investigation }) ?? filtered.nodes.first
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            metrics
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Filters",
                trailingTitle: "Evidence",
                leadingIdealWidth: 250,
                trailingIdealWidth: 340,
                wideBreakpoint: 1220,
                compactBreakpoint: 790
            ) {
                filterPane
            } center: {
                graphPane
            } trailing: {
                inspector
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            if selectedNodeID == nil { selectedNodeID = filtered.nodes.first(where: { $0.kind != .investigation })?.id }
        }
    }

    private var toolbar: some View {
        VStack(spacing: 7) {
            HStack(spacing: 10) {
                Label("Evidence Graph", systemImage: "point.3.connected.trianglepath.dotted")
                    .font(.headline)
                TextField("Filter title, address, provenance, confidence…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .frame(minWidth: 180, idealWidth: 340, maxWidth: 520)
                Spacer(minLength: 0)
            }
            VGFlowLayout {
                Text("Graph scale").font(.caption2).foregroundStyle(.secondary)
                Button { zoom = max(0.5, zoom - 0.1); zoomAnchor = zoom } label: { Image(systemName: "minus.magnifyingglass") }.help("Zoom out")
                Text("\(Int(zoom * 100))%").font(.system(size: 10, design: .monospaced)).frame(width: 42)
                Button { zoom = min(1.5, zoom + 0.1); zoomAnchor = zoom } label: { Image(systemName: "plus.magnifyingglass") }.help("Zoom in")
                Button("Fit") { zoom = 0.78; zoomAnchor = zoom }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }

    private var metrics: some View {
        let summary = filtered.summary
        return VGHorizontalOverflowBar {
            HStack(spacing: 18) {
                metric("NODES", summary.nodes)
                metric("EDGES", summary.edges)
                metric("OBSERVED", summary.observed)
                metric("CORRELATED", summary.correlated)
                metric("UNRESOLVED", summary.unresolved)
                Text("Observed · Derived · Correlated · Asserted remain distinct")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
        }
        .background(.thinMaterial)
    }

    private func metric(_ label: String, _ value: Int) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label).font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            Text("\(value)").font(.system(size: 13, weight: .semibold, design: .monospaced))
        }
    }

    private var filterPane: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("PROVENANCE").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                Spacer()
            }
            .padding(10)
            Divider()
            VStack(alignment: .leading, spacing: 10) {
                provenanceToggle("Static analysis", icon: "doc.text.magnifyingglass", isOn: $includeAnalysis)
                provenanceToggle("Debugger", icon: "ladybug", isOn: $includeDebugger)
                provenanceToggle("Correlation", icon: "point.3.filled.connected.trianglepath.dotted", isOn: $includeCorrelation)
                provenanceToggle("Analyst", icon: "person.text.rectangle", isOn: $includeAnalyst)
            }
            .padding(12)
            Divider()
            Text("CONFIDENCE LANGUAGE")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
                .padding(.top, 12)
            VStack(alignment: .leading, spacing: 8) {
                legend(.observed, "Directly captured")
                legend(.derived, "Static derivation")
                legend(.correlated, "Evidence-backed mapping")
                legend(.asserted, "Analyst-authored")
                legend(.unresolved, "Not safely correlated")
            }
            .padding(12)
            Spacer()
            Text("Edges explain *why* two facts are connected. Temporal proximity and current selection are never promoted to causal proof.")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .padding(12)
        }
        .background(.thinMaterial)
    }

    private func provenanceToggle(_ title: String, icon: String, isOn: Binding<Bool>) -> some View {
        Toggle(isOn: isOn) {
            Label(title, systemImage: icon).font(.caption)
        }
        .toggleStyle(.switch)
        .controlSize(.small)
    }

    private func legend(_ confidence: VGEvidenceNode.Confidence, _ text: String) -> some View {
        HStack(spacing: 8) {
            Circle().fill(confidenceColor(confidence)).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 1) {
                Text(confidence.rawValue.uppercased()).font(.system(size: 8, weight: .bold))
                Text(text).font(.caption2).foregroundStyle(.secondary)
            }
        }
    }

    private var graphPane: some View {
        GeometryReader { proxy in
            ScrollView([.horizontal, .vertical]) {
                ZStack(alignment: .topLeading) {
                    Canvas { context, _ in
                        for edge in filtered.edges {
                            guard let a = layout.positions[edge.sourceID], let b = layout.positions[edge.targetID] else { continue }
                            var path = Path()
                            path.move(to: CGPoint(x: a.x, y: a.y))
                            let midX = (a.x + b.x) / 2
                            path.addCurve(
                                to: CGPoint(x: b.x, y: b.y),
                                control1: CGPoint(x: midX, y: a.y),
                                control2: CGPoint(x: midX, y: b.y)
                            )
                            context.stroke(path, with: .color(edgeColor(edge).opacity(0.55)), style: StrokeStyle(lineWidth: edge.relation == .observedAt ? 2.0 : 1.2, dash: dash(edge)))
                        }
                    }
                    .frame(width: CGFloat(layout.width), height: CGFloat(layout.height))

                    ForEach(filtered.nodes.prefix(180)) { node in
                        if let point = layout.positions[node.id] {
                            nodeCard(node)
                                .position(x: point.x, y: point.y)
                        }
                    }
                }
                .frame(width: CGFloat(layout.width), height: CGFloat(layout.height))
                .scaleEffect(zoom, anchor: .topLeading)
                .frame(width: CGFloat(layout.width) * zoom, height: CGFloat(layout.height) * zoom, alignment: .topLeading)
                .contentShape(Rectangle())
                .gesture(
                    MagnificationGesture()
                        .onChanged { value in zoom = min(1.5, max(0.5, zoomAnchor * value)) }
                        .onEnded { _ in zoomAnchor = zoom }
                )
            }
            .overlay(alignment: .bottomTrailing) { miniMap(maxWidth: min(220, proxy.size.width * 0.32)) }
            .background(Color(nsColor: .textBackgroundColor).opacity(0.55))
        }
    }

    private func nodeCard(_ node: VGEvidenceNode) -> some View {
        Button {
            selectedNodeID = node.id
            if let address = node.addressText { onSelectAddress(address) }
        } label: {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    Image(systemName: nodeIcon(node.kind))
                    Text(node.kind.title.uppercased())
                        .font(.system(size: 8, weight: .bold))
                    Spacer()
                    Circle().fill(confidenceColor(node.confidence)).frame(width: 7, height: 7)
                }
                Text(node.title)
                    .font(.system(size: 11, weight: .semibold, design: node.address == nil ? .default : .monospaced))
                    .lineLimit(1)
                if let address = node.addressText, node.title != address {
                    Text(address).font(.system(size: 8, design: .monospaced)).foregroundStyle(.secondary)
                }
                Text(node.detail)
                    .font(.system(size: 8))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            .padding(9)
            .frame(width: 218, height: 72, alignment: .topLeading)
            .background(node.id == selectedNodeID ? Color.accentColor.opacity(0.14) : Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 9))
            .overlay(RoundedRectangle(cornerRadius: 9).stroke(node.id == selectedNodeID ? Color.accentColor.opacity(0.8) : confidenceColor(node.confidence).opacity(0.28), lineWidth: node.id == selectedNodeID ? 1.5 : 1))
        }
        .buttonStyle(.plain)
        .contextMenu {
            if let address = node.addressText {
                Button("Open in Binary Workspace") { onSelectAddress(address); onOpenWorkspace("Binary Workspace") }
                Button("Open in Flow Graph") { onSelectAddress(address); onOpenWorkspace("CFG") }
            }
            if node.kind == .runtimeObservation { Button("Open Trace Explorer") { onOpenWorkspace("Trace Explorer") } }
            Button("Open Notes") { onOpenWorkspace("Notes") }
        }
    }

    private func miniMap(maxWidth: CGFloat) -> some View {
        let scale: CGFloat = maxWidth / max(1, CGFloat(layout.width))
        let height = CGFloat(layout.height) * scale
        return Canvas { context, _ in
            for edge in filtered.edges {
                guard let a = layout.positions[edge.sourceID], let b = layout.positions[edge.targetID] else { continue }
                var p = Path()
                p.move(to: CGPoint(x: CGFloat(a.x) * scale, y: CGFloat(a.y) * scale))
                p.addLine(to: CGPoint(x: CGFloat(b.x) * scale, y: CGFloat(b.y) * scale))
                context.stroke(p, with: .color(.secondary.opacity(0.22)), lineWidth: 0.5)
            }
            for node in filtered.nodes.prefix(180) {
                guard let p = layout.positions[node.id] else { continue }
                let rect = CGRect(x: CGFloat(p.x) * scale - 2, y: CGFloat(p.y) * scale - 2, width: 4, height: 4)
                context.fill(Path(ellipseIn: rect), with: .color(confidenceColor(node.confidence).opacity(0.8)))
            }
        }
        .frame(width: maxWidth, height: min(150, height))
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.secondary.opacity(0.18)))
        .padding(12)
        .allowsHitTesting(false)
    }

    private var inspector: some View {
        VStack(spacing: 0) {
            HStack {
                Text("EVIDENCE INSPECTOR").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                Spacer()
            }
            .padding(10)
            Divider()
            if let node = selected {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        VStack(alignment: .leading, spacing: 7) {
                            HStack {
                                Label(node.kind.title, systemImage: nodeIcon(node.kind)).font(.headline)
                                Spacer()
                                Text(node.confidence.rawValue.uppercased())
                                    .font(.system(size: 8, weight: .bold))
                                    .foregroundStyle(confidenceColor(node.confidence))
                            }
                            Text(node.title).font(.title3.weight(.semibold)).textSelection(.enabled)
                            if let address = node.addressText { Text(address).font(.system(.caption, design: .monospaced)).textSelection(.enabled) }
                            Text(node.detail).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                            HStack {
                                Text("SOURCE").font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
                                Text(node.provenance.title).font(.caption)
                            }
                        }
                        .padding(12)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))

                        if let address = node.addressText {
                            HStack {
                                Button("Binary") { onSelectAddress(address); onOpenWorkspace("Binary Workspace") }
                                Button("CFG") { onSelectAddress(address); onOpenWorkspace("CFG") }
                                Button("Correlation") { onSelectAddress(address); onOpenWorkspace("Runtime Correlation") }
                            }
                        }

                        let connected = graph.connected(to: node.id)
                        VStack(alignment: .leading, spacing: 8) {
                            Text("RELATIONSHIPS").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                            if connected.isEmpty {
                                Text("No visible relationships.").font(.caption).foregroundStyle(.secondary)
                            } else {
                                ForEach(connected.prefix(24)) { edge in
                                    let peerID = edge.sourceID == node.id ? edge.targetID : edge.sourceID
                                    VStack(alignment: .leading, spacing: 3) {
                                        HStack {
                                            Text(edge.relation.rawValue.uppercased()).font(.system(size: 8, weight: .bold)).foregroundStyle(edgeColor(edge))
                                            Spacer()
                                            Text(edge.confidence.rawValue).font(.system(size: 8)).foregroundStyle(.secondary)
                                        }
                                        Text(graph.node(peerID)?.title ?? peerID).font(.caption.weight(.medium))
                                        Text(edge.explanation).font(.caption2).foregroundStyle(.secondary)
                                    }
                                    .padding(8)
                                    .background(Color(nsColor: .controlBackgroundColor).opacity(0.65), in: RoundedRectangle(cornerRadius: 7))
                                }
                            }
                        }
                    }
                    .padding(12)
                }
            } else {
                ContentUnavailableView("Select evidence", systemImage: "cursorarrow.click.2")
            }
        }
        .background(.thinMaterial)
    }

    private func nodeIcon(_ kind: VGEvidenceNode.Kind) -> String {
        switch kind {
        case .investigation: return "scope"
        case .function: return "function"
        case .address: return "number"
        case .staticEvidence: return "checkmark.seal"
        case .correlationEvidence: return "point.3.filled.connected.trianglepath.dotted"
        case .runtimeObservation: return "ladybug"
        case .memoryObservation: return "memorychip"
        case .analystNote: return "note.text"
        case .analystHypothesis: return "lightbulb"
        }
    }

    private func confidenceColor(_ confidence: VGEvidenceNode.Confidence) -> Color {
        switch confidence {
        case .observed: return .green
        case .derived: return .blue
        case .correlated: return .purple
        case .asserted: return .orange
        case .unresolved: return .secondary
        }
    }

    private func edgeColor(_ edge: VGEvidenceEdge) -> Color {
        switch edge.relation {
        case .references: return .blue
        case .observedAt: return .purple
        case .nearbyMemory: return .green
        case .annotates: return .orange
        case .follows: return .teal
        case .supports: return .indigo
        case .contains: return .secondary
        }
    }

    private func dash(_ edge: VGEvidenceEdge) -> [CGFloat] {
        switch edge.confidence {
        case .observed, .correlated: return []
        case .derived: return [4, 3]
        case .asserted: return [2, 3]
        case .unresolved: return [1, 4]
        }
    }
}
