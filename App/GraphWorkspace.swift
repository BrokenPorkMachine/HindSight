import SwiftUI

private enum VGSemanticPaneMode: String, CaseIterable, Identifiable {
    case code = "Code"
    case ir = "IR"
    case flow = "Data Flow"

    var id: String { rawValue }
}

struct VGGraphWorkspace: View {
    let cfgText: String
    let functionsText: String
    let disassemblyText: String
    let irText: String
    let dataflowText: String
    let selectedAddress: String
    let debugPC: String?
    let onSelectAddress: (String) -> Void

    @State private var zoom: CGFloat = 0.85
    @State private var zoomAnchor: CGFloat = 0.85
    @State private var selectedBlockID = ""
    @State private var functionQuery = ""
    @State private var showFunctions = true
    @State private var showSemantics = true
    @State private var showCalls = true
    @State private var semanticMode: VGSemanticPaneMode = .code
    @State private var graph: VGCFGGraph
    @State private var functions: [VGFunctionRecord]
    @State private var layout: VGCFGLayout

    private let nodeWidth: CGFloat = 290
    private let nodeHeight: CGFloat = 126

    init(
        cfgText: String,
        functionsText: String,
        disassemblyText: String,
        irText: String,
        dataflowText: String,
        selectedAddress: String,
        debugPC: String?,
        onSelectAddress: @escaping (String) -> Void
    ) {
        self.cfgText = cfgText
        self.functionsText = functionsText
        self.disassemblyText = disassemblyText
        self.irText = irText
        self.dataflowText = dataflowText
        self.selectedAddress = selectedAddress
        self.debugPC = debugPC
        self.onSelectAddress = onSelectAddress

        let parsedFunctions = VGGraphParser.functions(functionsText)
        let parsedGraph = VGGraphParser.cfg(cfgText, functionStarts: Set(parsedFunctions.map(\.start)))
        _graph = State(initialValue: parsedGraph)
        _functions = State(initialValue: parsedFunctions)
        _layout = State(initialValue: parsedGraph.layout(limit: 240))
    }

    private var selectedAddressValue: UInt64? { VGGraphParser.address(from: selectedAddress) }
    private var debugPCValue: UInt64? { debugPC.flatMap { VGGraphParser.address(from: $0) } }

    private var selectedBlock: VGCFGBlock? {
        if !selectedBlockID.isEmpty, let exact = graph.blocks.first(where: { $0.id == selectedBlockID }) { return exact }
        if let address = selectedAddressValue { return graph.block(containing: address) }
        return graph.blocks.first
    }

    private var filteredFunctions: [VGFunctionRecord] {
        let query = functionQuery.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return Array(functions.prefix(2000)) }
        return functions.filter {
            $0.name.lowercased().contains(query) || $0.startText.lowercased().contains(query)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            graphToolbar
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Functions",
                trailingTitle: "Semantics",
                leadingVisible: showFunctions,
                trailingVisible: showSemantics,
                leadingIdealWidth: 235,
                trailingIdealWidth: 390
            ) {
                functionNavigator
            } center: {
                graphCanvas
            } trailing: {
                semanticPane
            }
        }
        .background(Color.primary.opacity(0.012))
        .onAppear { synchronizeSelection() }
        .onChange(of: cfgText) { _, newValue in
            let parsedFunctions = VGGraphParser.functions(functionsText)
            functions = parsedFunctions
            graph = VGGraphParser.cfg(newValue, functionStarts: Set(parsedFunctions.map(\.start)))
            layout = graph.layout(limit: 240)
            synchronizeSelection()
        }
        .onChange(of: functionsText) { _, newValue in
            let parsedFunctions = VGGraphParser.functions(newValue)
            functions = parsedFunctions
            graph = VGGraphParser.cfg(cfgText, functionStarts: Set(parsedFunctions.map(\.start)))
            layout = graph.layout(limit: 240)
            synchronizeSelection()
        }
        .onChange(of: selectedAddress) { _, _ in synchronizeSelection() }
    }

    private var graphToolbar: some View {
        VStack(spacing: 6) {
            HStack(spacing: 8) {
                Label("Flow Graph", systemImage: "point.3.connected.trianglepath.dotted")
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                Spacer(minLength: 8)
                Text(graph.blocks.count > layout.visibleBlockIDs.count ? "Showing \(layout.visibleBlockIDs.count) / \(graph.blocks.count) blocks" : "\(graph.blocks.count) blocks • \(graph.edges.count) edges")
                    .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
            }
            VGFlowLayout {
                Button { showFunctions.toggle() } label: { Label("Functions", systemImage: "sidebar.left") }
                    .buttonStyle(.borderless)
                Button { showSemantics.toggle() } label: { Label("Semantics", systemImage: "sidebar.right") }
                    .buttonStyle(.borderless)
                Toggle("Calls", isOn: $showCalls).toggleStyle(.button).controlSize(.small)
                Button { setZoom(zoom - 0.1) } label: { Image(systemName: "minus.magnifyingglass") }.buttonStyle(.borderless)
                Text("\(Int(zoom * 100))%").font(.system(size: 10, weight: .medium, design: .monospaced)).frame(width: 42)
                Button { setZoom(zoom + 0.1) } label: { Image(systemName: "plus.magnifyingglass") }.buttonStyle(.borderless)
                Button("Fit") { setZoom(0.72) }.controlSize(.small)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private var functionNavigator: some View {
        VStack(spacing: 0) {
            HStack {
                Text("FUNCTIONS")
                    .font(.system(size: 9, weight: .bold, design: .rounded))
                    .tracking(0.7)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("\(functions.count)")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 10)
            .padding(.top, 10)
            .padding(.bottom, 7)

            TextField("Filter functions", text: $functionQuery)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 8)
                .padding(.bottom, 8)

            Divider()

            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(filteredFunctions) { function in
                        Button {
                            selectAddress(function.start)
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(function.name)
                                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                                    .lineLimit(1)
                                HStack(spacing: 5) {
                                    Text(function.startText)
                                    Text("•")
                                    Text("\(function.extentLabel)")
                                }
                                .font(.system(size: 9, design: .monospaced))
                                .foregroundStyle(.secondary)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 6)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                            .background {
                                if let block = selectedBlock, function.contains(block.start) {
                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                        .fill(Color.accentColor.opacity(0.10))
                                }
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(5)
            }
        }
        .frame(minWidth: 190, idealWidth: 235, maxWidth: 300)
        .background(.thinMaterial)
    }

    private var graphCanvas: some View {
        GeometryReader { proxy in
            ZStack(alignment: .bottomTrailing) {
                ScrollView([.horizontal, .vertical]) {
                    ZStack(alignment: .topLeading) {
                        edgeCanvas
                        ForEach(graph.blocks.filter { layout.visibleBlockIDs.contains($0.id) }) { block in
                            if let point = layout.positions[block.id] {
                                graphNode(block)
                                    .position(x: CGFloat(point.x), y: CGFloat(point.y))
                            }
                        }
                    }
                    .frame(width: CGFloat(layout.width), height: CGFloat(layout.height), alignment: .topLeading)
                    .scaleEffect(zoom, anchor: .topLeading)
                    .frame(width: CGFloat(layout.width) * zoom, height: CGFloat(layout.height) * zoom, alignment: .topLeading)
                    .gesture(
                        MagnificationGesture()
                            .onChanged { value in
                                zoom = clampZoom(zoomAnchor * value)
                            }
                            .onEnded { _ in zoomAnchor = zoom }
                    )
                }
                .background {
                    Canvas { context, size in
                        let step: CGFloat = 22
                        var path = Path()
                        var x: CGFloat = 0
                        while x <= size.width { path.move(to: CGPoint(x: x, y: 0)); path.addLine(to: CGPoint(x: x, y: size.height)); x += step }
                        var y: CGFloat = 0
                        while y <= size.height { path.move(to: CGPoint(x: 0, y: y)); path.addLine(to: CGPoint(x: size.width, y: y)); y += step }
                        context.stroke(path, with: .color(Color.primary.opacity(0.025)), lineWidth: 0.5)
                    }
                }

                minimap
                    .padding(12)
                    .shadow(radius: 8, y: 3)
            }
            .overlay(alignment: .topLeading) {
                if graph.blocks.isEmpty {
                    ContentUnavailableView("No flow graph", systemImage: "point.3.connected.trianglepath.dotted", description: Text("Run Analyze binary to build the bounded CFG."))
                        .frame(width: proxy.size.width, height: proxy.size.height)
                }
            }
        }
        .frame(minWidth: 480, minHeight: 520)
    }

    private var edgeCanvas: some View {
        Canvas { context, _ in
            for edge in graph.edges {
                if edge.isCall && !showCalls { continue }
                guard layout.visibleBlockIDs.contains(edge.sourceBlockID),
                      let targetID = edge.targetBlockID,
                      layout.visibleBlockIDs.contains(targetID),
                      let source = layout.positions[edge.sourceBlockID],
                      let target = layout.positions[targetID] else { continue }

                let start = CGPoint(x: CGFloat(source.x), y: CGFloat(source.y) + nodeHeight / 2)
                let end = CGPoint(x: CGFloat(target.x), y: CGFloat(target.y) - nodeHeight / 2)
                let delta = max(42.0, abs(end.y - start.y) * 0.42)
                var path = Path()
                path.move(to: start)
                path.addCurve(
                    to: end,
                    control1: CGPoint(x: start.x, y: start.y + delta),
                    control2: CGPoint(x: end.x, y: end.y - delta)
                )

                let color: Color = edge.isCall ? .orange : edge.isFallthrough ? .secondary : .accentColor
                let dash: [CGFloat] = edge.isCall ? [6, 5] : []
                context.stroke(path, with: .color(color.opacity(edge.isFallthrough ? 0.35 : 0.64)), style: StrokeStyle(lineWidth: edge.isCall ? 1.2 : 1.5, lineCap: .round, lineJoin: .round, dash: dash))

                var arrow = Path()
                arrow.move(to: CGPoint(x: end.x, y: end.y))
                arrow.addLine(to: CGPoint(x: end.x - 4.5, y: end.y - 8))
                arrow.addLine(to: CGPoint(x: end.x + 4.5, y: end.y - 8))
                arrow.closeSubpath()
                context.fill(arrow, with: .color(color.opacity(0.72)))
            }
        }
        .frame(width: CGFloat(layout.width), height: CGFloat(layout.height))
        .allowsHitTesting(false)
    }

    private func graphNode(_ block: VGCFGBlock) -> some View {
        let selected = selectedBlock?.id == block.id
        let executing = debugPCValue.map(block.contains) ?? false
        let function = functions.first(where: { $0.contains(block.start) })

        return Button {
            selectedBlockID = block.id
            selectAddress(block.start)
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 7) {
                    Image(systemName: executing ? "play.fill" : selected ? "scope" : "cube")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(executing ? Color.red : selected ? Color.accentColor : Color.secondary)
                    Text(block.id)
                        .font(.system(size: 11, weight: .bold, design: .monospaced))
                    Spacer()
                    Text("\(block.byteCount) B")
                        .font(.system(size: 9, design: .monospaced))
                        .foregroundStyle(.secondary)
                }

                Text(block.startText)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                    .foregroundStyle(.primary)

                if let function {
                    Text(function.name)
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                } else {
                    Text("unattributed block")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }

                HStack(spacing: 5) {
                    ForEach(Array(block.edges.prefix(4))) { edge in
                        Text(edgeBadge(edge))
                            .font(.system(size: 8, weight: .semibold, design: .rounded))
                            .padding(.horizontal, 5)
                            .padding(.vertical, 2)
                            .background(edge.isCall ? Color.orange.opacity(0.13) : Color.primary.opacity(0.055), in: Capsule())
                    }
                    Spacer(minLength: 0)
                }
            }
            .padding(11)
            .frame(width: nodeWidth, height: nodeHeight, alignment: .topLeading)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 11, style: .continuous)
                    .strokeBorder(executing ? Color.red.opacity(0.85) : selected ? Color.accentColor.opacity(0.85) : Color.primary.opacity(0.11), lineWidth: executing || selected ? 2 : 1)
            }
            .contentShape(RoundedRectangle(cornerRadius: 11, style: .continuous))
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button("Select Block") { selectedBlockID = block.id; selectAddress(block.start) }
            Button("Use Start Address") { selectAddress(block.start) }
        }
    }

    private var semanticPane: some View {
        VStack(spacing: 0) {
            HStack {
                Text("BLOCK CONTEXT")
                    .font(.system(size: 9, weight: .bold, design: .rounded))
                    .tracking(0.7)
                    .foregroundStyle(.secondary)
                Spacer()
                if let block = selectedBlock {
                    Text(block.id)
                        .font(.system(size: 10, weight: .semibold, design: .monospaced))
                }
            }
            .padding(.horizontal, 10)
            .padding(.top, 10)
            .padding(.bottom, 7)

            Picker("Semantic view", selection: $semanticMode) {
                ForEach(VGSemanticPaneMode.allCases) { mode in Text(mode.rawValue).tag(mode) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 8)
            .padding(.bottom, 8)

            Divider()

            if let block = selectedBlock {
                ScrollView([.horizontal, .vertical]) {
                    switch semanticMode {
                    case .code:
                        codeForBlock(block)
                    case .ir:
                        semanticLines(irText, block: block, empty: "No IR records intersect this block.")
                    case .flow:
                        semanticLines(dataflowText, block: block, empty: "No modeled register-flow records intersect this block.")
                    }
                }
            } else {
                ContentUnavailableView("Select a block", systemImage: "cursorarrow.click", description: Text("Choose a graph node to synchronize code and semantic evidence."))
            }
        }
        .frame(minWidth: 330, idealWidth: 410, maxWidth: 540)
        .background(.thinMaterial)
    }

    @ViewBuilder private func codeForBlock(_ block: VGCFGBlock) -> some View {
        let rows = VGDisassemblyParser.parse(disassemblyText).filter { row in
            guard row.kind != .metadata, let address = VGGraphParser.address(from: row.address) else { return false }
            return block.contains(address)
        }

        if rows.isEmpty {
            Text("No annotated disassembly intersects \(block.id).")
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(12)
        } else {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(rows.prefix(96)) { row in
                    Button {
                        if let address = VGGraphParser.address(from: row.address) { selectAddress(address) }
                    } label: {
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text(row.address)
                                .foregroundStyle(.secondary)
                                .frame(width: 145, alignment: .leading)
                            if row.kind == .symbol {
                                Text(row.mnemonic).fontWeight(.semibold)
                            } else {
                                Text(row.mnemonic)
                                    .fontWeight(.semibold)
                                    .frame(width: 54, alignment: .leading)
                                Text(row.operands)
                                if !row.annotation.isEmpty { Text("; \(row.annotation)").foregroundStyle(.secondary) }
                            }
                            Spacer(minLength: 8)
                        }
                        .font(.system(size: 10, design: .monospaced))
                        .padding(.horizontal, 9)
                        .padding(.vertical, 4)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 5)
        }
    }

    @ViewBuilder private func semanticLines(_ text: String, block: VGCFGBlock, empty: String) -> some View {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init).filter { line in
            guard let address = VGGraphParser.address(from: line) else { return false }
            return block.contains(address)
        }
        if lines.isEmpty {
            Text(empty).font(.caption).foregroundStyle(.secondary).padding(12)
        } else {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(lines.prefix(128).enumerated()), id: \.offset) { _, line in
                    Button {
                        if let address = VGGraphParser.address(from: line) { selectAddress(address) }
                    } label: {
                        Text(line)
                            .font(.system(size: 10, design: .monospaced))
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 9)
                            .padding(.vertical, 4)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 5)
        }
    }

    private var minimap: some View {
        let scale = minimapScale
        return ZStack {
            RoundedRectangle(cornerRadius: 9, style: .continuous)
                .fill(.ultraThickMaterial)
            Canvas { context, _ in
                for edge in graph.edges where !edge.isCall || showCalls {
                    guard let targetID = edge.targetBlockID,
                          let source = layout.positions[edge.sourceBlockID],
                          let target = layout.positions[targetID],
                          layout.visibleBlockIDs.contains(edge.sourceBlockID), layout.visibleBlockIDs.contains(targetID) else { continue }
                    var path = Path()
                    path.move(to: CGPoint(x: CGFloat(source.x) * scale, y: CGFloat(source.y) * scale))
                    path.addLine(to: CGPoint(x: CGFloat(target.x) * scale, y: CGFloat(target.y) * scale))
                    context.stroke(path, with: .color(Color.secondary.opacity(0.20)), lineWidth: 0.5)
                }
                for block in graph.blocks where layout.visibleBlockIDs.contains(block.id) {
                    guard let point = layout.positions[block.id] else { continue }
                    let selected = selectedBlock?.id == block.id
                    let executing = debugPCValue.map(block.contains) ?? false
                    let rect = CGRect(x: CGFloat(point.x) * scale - 2.4, y: CGFloat(point.y) * scale - 1.7, width: 4.8, height: 3.4)
                    context.fill(Path(ellipseIn: rect), with: .color(executing ? Color.red : selected ? Color.accentColor : Color.primary.opacity(0.38)))
                }
            }
            .padding(6)
        }
        .frame(width: 190, height: 120)
        .overlay { RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(Color.primary.opacity(0.10), lineWidth: 1) }
        .allowsHitTesting(false)
    }

    private var minimapScale: CGFloat {
        let widthScale = 176 / max(CGFloat(layout.width), 1)
        let heightScale = 106 / max(CGFloat(layout.height), 1)
        return min(widthScale, heightScale)
    }

    private func edgeBadge(_ edge: VGCFGEdge) -> String {
        if edge.isFallthrough { return "next" }
        if edge.isCall { return "call" }
        if edge.isConditional { return "cond" }
        return edge.kind
    }

    private func synchronizeSelection() {
        guard let address = selectedAddressValue, let block = graph.block(containing: address) else { return }
        selectedBlockID = block.id
    }

    private func selectAddress(_ address: UInt64) {
        if let block = graph.block(containing: address) { selectedBlockID = block.id }
        onSelectAddress(String(format: "0x%016llx", address))
    }

    private func clampZoom(_ value: CGFloat) -> CGFloat {
        min(max(value, 0.35), 1.8)
    }

    private func setZoom(_ value: CGFloat) {
        zoom = clampZoom(value)
        zoomAnchor = zoom
    }
}
