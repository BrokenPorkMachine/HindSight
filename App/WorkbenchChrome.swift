import SwiftUI

struct VGCanvasSurface<Content: View>: View {
    let viewport: VGCanvasViewport
    let content: Content

    init(viewport: VGCanvasViewport, @ViewBuilder content: () -> Content) {
        self.viewport = viewport
        self.content = content()
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            content
                .frame(width: viewport.contentSize.width, height: viewport.contentSize.height, alignment: .topLeading)
                .scaleEffect(viewport.zoom, anchor: .topLeading)
                .frame(width: viewport.contentSize.width * viewport.zoom, height: viewport.contentSize.height * viewport.zoom, alignment: .topLeading)
                .offset(viewport.origin)
        }
        .frame(width: viewport.viewportSize.width, height: viewport.viewportSize.height, alignment: .topLeading)
        .contentShape(Rectangle())
        .clipped()
    }
}

struct VGPanelGroupBoxStyle: GroupBoxStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            configuration.label
                .font(.system(.subheadline, design: .rounded, weight: .semibold))
                .foregroundStyle(.primary)
            configuration.content
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(.primary.opacity(0.08), lineWidth: 1)
        }
    }
}

struct VGWorkspaceRow: View {
    let descriptor: VGWorkspaceDescriptor
    let selected: Bool

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: descriptor.icon)
                .font(.system(size: 13, weight: .semibold))
                .symbolRenderingMode(.hierarchical)
                .frame(width: 18)
            Text(descriptor.title)
                .font(.system(size: 13, weight: selected ? .semibold : .regular))
                .lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 5)
        .padding(.horizontal, 7)
        .contentShape(Rectangle())
        .background {
            if selected {
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .fill(Color.accentColor.opacity(0.16))
            }
        }
    }
}

struct VGStatusPill: View {
    let label: String
    let systemImage: String
    let emphasized: Bool

    var body: some View {
        Label(label, systemImage: systemImage)
            .font(.system(size: 11, weight: .medium))
            .lineLimit(1)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(emphasized ? Color.accentColor.opacity(0.15) : Color.primary.opacity(0.055), in: Capsule())
    }
}

struct VGCommandPalette: View {
    @Binding var isPresented: Bool
    let currentWorkspace: String
    let onSelect: (String) -> Void
    @State private var query = ""
    @FocusState private var queryFocused: Bool

    private var results: [VGWorkspaceDescriptor] {
        Array(VGWorkspaceCatalog.search(query).prefix(60))
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Jump to any HindSight tool…", text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 18, weight: .medium))
                    .focused($queryFocused)
                Text("esc")
                    .font(.system(size: 10, weight: .medium, design: .rounded))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 3)
                    .background(.quaternary, in: RoundedRectangle(cornerRadius: 5))
            }
            .padding(16)

            Divider()

            ScrollView {
                if results.isEmpty {
                    ContentUnavailableView(
                        "No matching tool",
                        systemImage: "magnifyingglass",
                        description: Text("Try a workspace name, analysis concept, or task such as code, trace, firmware, or evidence.")
                    )
                    .frame(maxWidth: .infinity, minHeight: 360)
                } else {
                    LazyVStack(spacing: 3) {
                        ForEach(results) { descriptor in
                            Button {
                                onSelect(descriptor.name)
                                isPresented = false
                            } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: descriptor.icon)
                                        .font(.system(size: 15, weight: .semibold))
                                        .symbolRenderingMode(.hierarchical)
                                        .frame(width: 24)
                                    VStack(alignment: .leading, spacing: 2) {
                                        HStack(spacing: 7) {
                                            Text(descriptor.title).fontWeight(.medium)
                                            if descriptor.name == currentWorkspace {
                                                Text("CURRENT")
                                                    .font(.system(size: 9, weight: .bold))
                                                    .foregroundStyle(.secondary)
                                            }
                                        }
                                        Text("\(descriptor.group.rawValue)  •  \(descriptor.subtitle)")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                        Text(descriptor.helpText)
                                            .font(.caption2)
                                            .foregroundStyle(.tertiary)
                                            .lineLimit(1)
                                    }
                                    Spacer()
                                    Image(systemName: "return")
                                        .font(.caption)
                                        .foregroundStyle(.tertiary)
                                }
                                .padding(.horizontal, 14)
                                .padding(.vertical, 9)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(8)
                }
            }

            Divider()
            HStack {
                Text("Searches all \(VGWorkspaceCatalog.descriptors.count) workspaces — advanced tools stay available without crowding the sidebar.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("⌘K")
                    .font(.system(.caption2, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
            .padding(12)
        }
        .frame(width: 720, height: 560)
        .background(.ultraThickMaterial)
        .onAppear { queryFocused = true }
        .onExitCommand { isPresented = false }
    }
}

struct VGWorkspaceTitle: View {
    let descriptor: VGWorkspaceDescriptor
    let recordName: String?
    let architecture: String?

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(Color.accentColor.opacity(0.13))
                Image(systemName: descriptor.icon)
                    .font(.system(size: 18, weight: .semibold))
                    .symbolRenderingMode(.hierarchical)
            }
            .frame(width: 38, height: 38)

            VStack(alignment: .leading, spacing: 2) {
                Text(descriptor.title)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                HStack(spacing: 6) {
                    Text(descriptor.subtitle)
                    if let recordName {
                        Text("•")
                        Text(recordName)
                    }
                    if let architecture, !architecture.isEmpty {
                        Text("•")
                        Text(architecture)
                    }
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                Text(descriptor.helpText)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }
        }
    }
}


// MARK: - Responsive flow / data primitives (0.4.2)

/// Wraps compact controls onto additional rows instead of allowing a single
/// toolbar to determine the minimum width of an entire workspace.
struct VGFlowLayout: Layout {
    var horizontalSpacing: CGFloat = 8
    var verticalSpacing: CGFloat = 8

    struct Cache { var sizes: [CGSize] = [] }

    func makeCache(subviews: Subviews) -> Cache {
        Cache(sizes: subviews.map { $0.sizeThatFits(.unspecified) })
    }

    func updateCache(_ cache: inout Cache, subviews: Subviews) {
        cache.sizes = subviews.map { $0.sizeThatFits(.unspecified) }
    }

    private func measuredRows(maxWidth: CGFloat, sizes: [CGSize]) -> (CGSize, [(CGPoint, CGSize)]) {
        let widthLimit = max(1, maxWidth)
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var usedWidth: CGFloat = 0
        var placements: [(CGPoint, CGSize)] = []

        for size in sizes {
            if x > 0, x + size.width > widthLimit {
                usedWidth = max(usedWidth, x - horizontalSpacing)
                x = 0
                y += rowHeight + verticalSpacing
                rowHeight = 0
            }
            placements.append((CGPoint(x: x, y: y), size))
            x += size.width + horizontalSpacing
            rowHeight = max(rowHeight, size.height)
        }
        if !sizes.isEmpty { usedWidth = max(usedWidth, x - horizontalSpacing) }
        return (CGSize(width: min(widthLimit, usedWidth), height: y + rowHeight), placements)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout Cache) -> CGSize {
        let intrinsicWidth = cache.sizes.reduce(CGFloat.zero) { $0 + $1.width }
            + horizontalSpacing * CGFloat(max(0, cache.sizes.count - 1))
        let available = proposal.width ?? max(1, intrinsicWidth)
        let result = measuredRows(maxWidth: available, sizes: cache.sizes).0
        return CGSize(width: proposal.width ?? result.width, height: result.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout Cache) {
        let (_, placements) = measuredRows(maxWidth: bounds.width, sizes: cache.sizes)
        for (index, placement) in placements.enumerated() {
            subviews[index].place(
                at: CGPoint(x: bounds.minX + placement.0.x, y: bounds.minY + placement.0.y),
                anchor: .topLeading,
                proposal: ProposedViewSize(width: placement.1.width, height: placement.1.height)
            )
        }
    }
}

/// A compact key/value presentation that remains legible when cards collapse to
/// a single column. Values are intentionally allowed to wrap instead of
/// reserving fixed table widths.
struct VGKeyValueField: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label.uppercased())
                .font(.system(size: 9, weight: .bold, design: .rounded))
                .foregroundStyle(.tertiary)
            Text(value)
                .font(monospaced ? .system(.caption, design: .monospaced) : .caption)
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct VGTagPill: View {
    let text: String
    let systemImage: String?

    init(text: String, systemImage: String? = nil) {
        self.text = text
        self.systemImage = systemImage
    }

    var body: some View {
        Group {
            if let systemImage {
                Label(text, systemImage: systemImage)
            } else {
                Text(text)
            }
        }
        .font(.system(size: 10, weight: .medium, design: .monospaced))
        .padding(.horizontal, 7)
        .padding(.vertical, 4)
        .background(Color.primary.opacity(0.055), in: Capsule())
    }
}

// MARK: - Adaptive workbench composition (0.4.0-beta.11)

enum VGAdaptivePane: String, CaseIterable, Identifiable {
    case navigator = "Navigator"
    case inspector = "Inspector"
    var id: String { rawValue }
}

enum VGAdaptiveLayoutMode: String, CaseIterable {
    case wide
    case medium
    case compact

    static func resolve(width: CGFloat, wideBreakpoint: CGFloat, compactBreakpoint: CGFloat) -> Self {
        if width >= wideBreakpoint { return .wide }
        if width >= compactBreakpoint { return .medium }
        return .compact
    }
}

/// A responsive three-pane workbench primitive.
///
/// Wide windows keep the classic navigator / canvas / inspector arrangement.
/// Medium windows protect the canvas and collapse auxiliary panes into a single
/// switchable dock. Compact windows move that dock below the canvas instead of
/// squeezing three minimum-width columns until controls disappear.
struct VGAdaptiveThreePane<Leading: View, Center: View, Trailing: View>: View {
    let leadingTitle: String
    let trailingTitle: String
    let leadingVisible: Bool
    let trailingVisible: Bool
    let leadingIdealWidth: CGFloat
    let trailingIdealWidth: CGFloat
    let wideBreakpoint: CGFloat
    let compactBreakpoint: CGFloat
    let leading: Leading
    let center: Center
    let trailing: Trailing

    @State private var selectedAuxiliary: VGAdaptivePane = .navigator

    init(
        leadingTitle: String = "Navigator",
        trailingTitle: String = "Inspector",
        leadingVisible: Bool = true,
        trailingVisible: Bool = true,
        leadingIdealWidth: CGFloat = 260,
        trailingIdealWidth: CGFloat = 340,
        wideBreakpoint: CGFloat = 1180,
        compactBreakpoint: CGFloat = 780,
        @ViewBuilder leading: () -> Leading,
        @ViewBuilder center: () -> Center,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.leadingTitle = leadingTitle
        self.trailingTitle = trailingTitle
        self.leadingVisible = leadingVisible
        self.trailingVisible = trailingVisible
        self.leadingIdealWidth = leadingIdealWidth
        self.trailingIdealWidth = trailingIdealWidth
        self.wideBreakpoint = wideBreakpoint
        self.compactBreakpoint = compactBreakpoint
        self.leading = leading()
        self.center = center()
        self.trailing = trailing()
    }

    private var availableAuxiliaryPanes: [VGAdaptivePane] {
        var panes: [VGAdaptivePane] = []
        if leadingVisible { panes.append(.navigator) }
        if trailingVisible { panes.append(.inspector) }
        return panes
    }

    private var effectiveAuxiliaryPane: VGAdaptivePane? {
        let available = availableAuxiliaryPanes
        if available.contains(selectedAuxiliary) { return selectedAuxiliary }
        return available.first
    }

    var body: some View {
        GeometryReader { proxy in
            Group {
                switch VGAdaptiveLayoutMode.resolve(width: proxy.size.width, wideBreakpoint: wideBreakpoint, compactBreakpoint: compactBreakpoint) {
                case .wide:
                    wideLayout
                case .medium:
                    mediumLayout
                case .compact:
                    compactLayout
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: .topLeading)
        }
    }

    @ViewBuilder private var wideLayout: some View {
        HSplitView {
            if leadingVisible {
                leading
                    .frame(minWidth: 190, idealWidth: leadingIdealWidth, maxWidth: max(leadingIdealWidth + 90, 300))
            }
            center
                .frame(minWidth: 360)
            if trailingVisible {
                trailing
                    .frame(minWidth: 250, idealWidth: trailingIdealWidth, maxWidth: max(trailingIdealWidth + 100, 390))
            }
        }
    }

    @ViewBuilder private var mediumLayout: some View {
        if availableAuxiliaryPanes.isEmpty {
            center
        } else {
            HSplitView {
                center
                    .frame(minWidth: 420)
                auxiliaryDock
                    .frame(minWidth: 260, idealWidth: 320, maxWidth: 390)
            }
        }
    }

    @ViewBuilder private var compactLayout: some View {
        if availableAuxiliaryPanes.isEmpty {
            center
        } else {
            VSplitView {
                center
                    .frame(minHeight: 320)
                auxiliaryDock
                    .frame(minHeight: 190, idealHeight: 260, maxHeight: 390)
            }
        }
    }

    private var auxiliaryDock: some View {
        VStack(spacing: 0) {
            if availableAuxiliaryPanes.count > 1 {
                Picker("Auxiliary Pane", selection: Binding(
                    get: { effectiveAuxiliaryPane ?? .navigator },
                    set: { selectedAuxiliary = $0 }
                )) {
                    if leadingVisible { Text(leadingTitle).tag(VGAdaptivePane.navigator) }
                    if trailingVisible { Text(trailingTitle).tag(VGAdaptivePane.inspector) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(8)
                Divider()
            } else if let pane = effectiveAuxiliaryPane {
                HStack {
                    Text(pane == .navigator ? leadingTitle : trailingTitle)
                        .font(.system(size: 10, weight: .bold, design: .rounded))
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                Divider()
            }

            Group {
                switch effectiveAuxiliaryPane {
                case .navigator: leading
                case .inspector: trailing
                case .none: EmptyView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .background(.bar)
    }
}

/// Keeps dense status/metric strips reachable on smaller windows without
/// forcing the surrounding workspace to adopt their combined intrinsic width.
struct VGHorizontalOverflowBar<Content: View>: View {
    let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            content
                .fixedSize(horizontal: true, vertical: false)
        }
    }
}

/// Two-pane companion for report/tool surfaces that previously assumed a wide
/// horizontal canvas. Below the breakpoint it becomes a vertical split so both
/// panes stay reachable without shrinking their controls below useful sizes.
struct VGAdaptiveTwoPane<Primary: View, Secondary: View>: View {
    let breakpoint: CGFloat
    let primary: Primary
    let secondary: Secondary

    init(
        breakpoint: CGFloat = 860,
        @ViewBuilder primary: () -> Primary,
        @ViewBuilder secondary: () -> Secondary
    ) {
        self.breakpoint = breakpoint
        self.primary = primary()
        self.secondary = secondary()
    }

    var body: some View {
        GeometryReader { proxy in
            Group {
                if proxy.size.width >= breakpoint {
                    HSplitView {
                        primary.frame(minWidth: 280)
                        secondary.frame(minWidth: 280)
                    }
                } else {
                    VSplitView {
                        primary.frame(minHeight: 160)
                        secondary.frame(minHeight: 160)
                    }
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height, alignment: .topLeading)
        }
    }
}
