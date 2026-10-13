import SwiftUI

struct VGInvestigationLedgerWorkspace: View {
    enum Mode: String, CaseIterable, Identifiable {
        case timeline = "Timeline"
        case hypotheses = "Hypotheses"
        case bookmarks = "Bookmarks"
        case snapshots = "Snapshots"
        var id: String { rawValue }
    }

    let ledger: VGInvestigationLedger
    let status: String
    let selectedEntryID: String
    let onSelectEntry: (String) -> Void
    let onOpenContext: (VGLedgerContext) -> Void
    let onCreateHypothesis: (String, String) -> Void
    let onSetHypothesisStatus: (String, VGHypothesisStatus) -> Void
    let onAddBookmark: (String, String) -> Void
    let onCaptureSnapshot: (String) -> Void
    let onRestoreSnapshot: (VGInvestigationSnapshot) -> Void
    let onExportCase: () -> Void
    let onImportCase: () -> Void
    let onExportMarkdown: () -> Void
    let onExportPDF: () -> Void
    let onExportJSON: () -> Void

    @State private var mode: Mode = .timeline
    @State private var query = ""
    @State private var provenance: VGLedgerProvenance?
    @State private var newHypothesisTitle = ""
    @State private var newHypothesisDetail = ""
    @State private var bookmarkTitle = ""
    @State private var bookmarkNote = ""
    @State private var snapshotTitle = ""

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            metrics
            Divider()
            VGAdaptiveThreePane(
                leadingTitle: "Case",
                trailingTitle: "Entry",
                leadingIdealWidth: 245,
                trailingIdealWidth: 330,
                wideBreakpoint: 1210,
                compactBreakpoint: 780
            ) {
                navigationPane
            } center: {
                mainPane
            } trailing: {
                inspector
            }
        }
        .background(Color(nsColor: .controlBackgroundColor).opacity(0.22))
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Investigation Ledger").font(.system(size: 17, weight: .semibold, design: .rounded))
                    Text("Persistent case state, analyst hypotheses, bookmarks, snapshots, and replayable provenance.")
                        .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer(minLength: 0)
            }
            VGFlowLayout {
                Button { replayStep(-1) } label: { Image(systemName: "chevron.left") }
                    .help("Previous ledger event").disabled(ledger.entries.isEmpty)
                Button { replayStep(1) } label: { Image(systemName: "chevron.right") }
                    .help("Next ledger event").disabled(ledger.entries.isEmpty)
                Button("Open Context") { if let entry = selectedEntry { onOpenContext(entry.context) } }
                    .disabled(selectedEntry == nil)
                Button("Snapshot") { onCaptureSnapshot(snapshotTitle.isEmpty ? "Session snapshot" : snapshotTitle); snapshotTitle = "" }
                Menu("Export") {
                    Button("HindSight Case (.vgcase)", action: onExportCase)
                    Button("Markdown Report", action: onExportMarkdown)
                    Button("PDF Report", action: onExportPDF)
                    Button("Ledger JSON", action: onExportJSON)
                }
                .buttonStyle(.borderedProminent)
                Button("Import Case…", action: onImportCase)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    private var metrics: some View {
        VGHorizontalOverflowBar {
            HStack(spacing: 24) {
                metric("TIMELINE", ledger.entries.count)
                metric("HYPOTHESES", ledger.hypotheses.count)
                metric("BOOKMARKS", ledger.bookmarks.count)
                metric("SNAPSHOTS", ledger.snapshots.count)
                Text(status).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
        }
        .background(.thinMaterial)
    }

    private func metric(_ name: String, _ value: Int) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(name).font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            Text("\(value)").font(.system(size: 13, weight: .semibold, design: .monospaced))
        }
    }

    private var navigationPane: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("CASE")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(.secondary)
                .padding(12)
            ForEach(Mode.allCases) { item in
                Button {
                    mode = item
                } label: {
                    HStack {
                        Image(systemName: icon(item)).frame(width: 18)
                        Text(item.rawValue)
                        Spacer()
                        Text("\(count(item))").font(.caption2).foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(mode == item ? Color.accentColor.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 7))
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 6)
            }
            Divider().padding(.vertical, 8)
            Text("PROVENANCE")
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 12)
            Button("All") { provenance = nil }.buttonStyle(.plain).padding(.horizontal, 12).padding(.vertical, 5)
            ForEach(VGLedgerProvenance.allCases, id: \.self) { p in
                Button { provenance = p } label: {
                    HStack { Circle().fill(color(p)).frame(width: 7, height: 7); Text(p.title); Spacer() }
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 12)
                .padding(.vertical, 5)
            }
            Spacer()
            VStack(alignment: .leading, spacing: 4) {
                Text("CASE ID").font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
                Text(ledger.caseID).font(.system(size: 9, design: .monospaced)).textSelection(.enabled).lineLimit(2)
            }
            .padding(12)
        }
        .background(.thinMaterial)
    }

    @ViewBuilder private var mainPane: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("Search case evidence…", text: $query)
                    .textFieldStyle(.roundedBorder)
                if mode == .snapshots {
                    TextField("Snapshot title", text: $snapshotTitle)
                        .textFieldStyle(.roundedBorder)
                        .frame(maxWidth: 220)
                }
            }
            .padding(12)
            Divider()
            switch mode {
            case .timeline: timeline
            case .hypotheses: hypotheses
            case .bookmarks: bookmarks
            case .snapshots: snapshots
            }
        }
    }

    private var filteredEntries: [VGLedgerEntry] {
        Array(ledger.entries.filter { entry in
            let provenanceMatch = provenance == nil || entry.provenance == provenance
            let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            let queryMatch = q.isEmpty || entry.title.lowercased().contains(q) || entry.detail.lowercased().contains(q) || entry.context.workspace.lowercased().contains(q) || (entry.context.address?.lowercased().contains(q) ?? false)
            return provenanceMatch && queryMatch
        }.reversed())
    }

    private var timeline: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 2) {
                ForEach(filteredEntries) { entry in
                    Button {
                        onSelectEntry(entry.id)
                    } label: {
                        HStack(alignment: .top, spacing: 10) {
                            VStack(spacing: 3) {
                                Circle().fill(color(entry.provenance)).frame(width: 9, height: 9)
                                Rectangle().fill(Color.secondary.opacity(0.15)).frame(width: 1, height: 37)
                            }
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(entry.title).font(.system(size: 12, weight: .semibold))
                                    Text(entry.provenance.title.uppercased()).font(.system(size: 8, weight: .bold)).foregroundStyle(color(entry.provenance))
                                    Spacer()
                                    Text(entry.timestamp, style: .time).font(.caption2).foregroundStyle(.secondary)
                                }
                                Text(entry.detail).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                                HStack(spacing: 8) {
                                    Label(entry.context.workspace, systemImage: "rectangle.3.group").font(.caption2)
                                    if let address = entry.context.address { Text(address).font(.system(size: 9, design: .monospaced)) }
                                    if let symbol = entry.context.symbol, !symbol.isEmpty { Text(symbol).font(.caption2) }
                                }.foregroundStyle(.secondary)
                            }
                        }
                        .padding(.vertical, 3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 8)
                        .background(entry.id == selectedEntryID ? Color.accentColor.opacity(0.10) : Color.clear, in: RoundedRectangle(cornerRadius: 7))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(8)
        }
    }

    private var hypotheses: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 6) {
                    ForEach(ledger.hypotheses.filter { queryMatch($0.title + " " + $0.detail) }.reversed()) { hypothesis in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Text(hypothesis.title).font(.headline)
                                Spacer()
                                Menu(hypothesis.status.title) {
                                    ForEach(VGHypothesisStatus.allCases, id: \.self) { status in
                                        Button(status.title) { onSetHypothesisStatus(hypothesis.id, status) }
                                    }
                                }
                                .menuStyle(.borderlessButton)
                            }
                            Text(hypothesis.detail).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                            if !hypothesis.linkedAddresses.isEmpty {
                                Text(hypothesis.linkedAddresses.joined(separator: " · ")).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary)
                            }
                        }
                        .padding(.vertical, 5)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(12)
            }
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                Text("NEW HYPOTHESIS").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                TextField("What do you think is happening?", text: $newHypothesisTitle).textFieldStyle(.roundedBorder)
                TextEditor(text: $newHypothesisDetail).font(.caption).frame(height: 72).overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.secondary.opacity(0.2)))
                HStack { Spacer(); Button("Record hypothesis") { onCreateHypothesis(newHypothesisTitle, newHypothesisDetail); newHypothesisTitle = ""; newHypothesisDetail = "" }.disabled(newHypothesisTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
            }.padding(12)
        }
    }

    private var bookmarks: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 6) {
                    ForEach(ledger.bookmarks.filter { queryMatch($0.title + " " + $0.note + " " + ($0.address ?? "")) }.reversed()) { bookmark in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(bookmark.title).font(.headline)
                            HStack { Text(bookmark.workspace); if let address = bookmark.address { Text(address).font(.system(size: 9, design: .monospaced)) } }.font(.caption).foregroundStyle(.secondary)
                            if !bookmark.note.isEmpty { Text(bookmark.note).font(.caption).textSelection(.enabled) }
                        }
                        .padding(.vertical, 4)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(12)
            }
            Divider()
            VStack(alignment: .leading, spacing: 8) {
                Text("BOOKMARK CURRENT CONTEXT").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary)
                HStack {
                    TextField("Bookmark title", text: $bookmarkTitle).textFieldStyle(.roundedBorder)
                    Button("Bookmark") { onAddBookmark(bookmarkTitle.isEmpty ? "Investigation bookmark" : bookmarkTitle, bookmarkNote); bookmarkTitle = ""; bookmarkNote = "" }
                }
                TextField("Optional note", text: $bookmarkNote).textFieldStyle(.roundedBorder)
            }.padding(12)
        }
    }

    private var snapshots: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 6) {
                ForEach(ledger.snapshots.filter { queryMatch($0.title + " " + $0.workspace + " " + $0.address) }.reversed()) { snapshot in
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(snapshot.title).font(.headline)
                            Text("\(snapshot.workspace) · \(snapshot.layoutPreset)").font(.caption).foregroundStyle(.secondary)
                            if !snapshot.address.isEmpty { Text(snapshot.address).font(.system(size: 9, design: .monospaced)).foregroundStyle(.secondary) }
                        }
                        Spacer()
                        Button("Restore") { onRestoreSnapshot(snapshot) }
                    }
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .padding(12)
        }
    }

    private var selectedEntry: VGLedgerEntry? { ledger.entries.first(where: { $0.id == selectedEntryID }) }

    private var inspector: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack { Text("LEDGER INSPECTOR").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary); Spacer() }.padding(12)
            Divider()
            if let entry = selectedEntry {
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(entry.title).font(.headline)
                            Text(entry.timestamp.formatted(date: .abbreviated, time: .standard)).font(.caption).foregroundStyle(.secondary)
                            provenanceBadge(entry.provenance)
                        }
                        Text(entry.detail).font(.caption).textSelection(.enabled)
                        inspectorField("Workspace", entry.context.workspace)
                        inspectorField("Address", entry.context.address ?? "none", mono: true)
                        inspectorField("Symbol", entry.context.symbol ?? "none")
                        inspectorField("Artifact", entry.context.recordName ?? "none")
                        inspectorField("SHA-256", entry.context.sourceSHA256 ?? "unavailable", mono: true)
                        if entry.context.address != nil || entry.context.recordID != nil {
                            Button("Open recorded context") { onOpenContext(entry.context) }.buttonStyle(.borderedProminent)
                        }
                        Divider()
                        Text("Ledger entries record what HindSight or the analyst observed at a point in time. Restoring context does not rewind a live target.")
                            .font(.caption2).foregroundStyle(.secondary)
                    }.padding(14)
                }
            } else {
                ContentUnavailableView("Select a ledger entry", systemImage: "book.pages", description: Text("Inspect provenance and reopen the recorded analysis context."))
                    .frame(maxHeight: .infinity)
            }
        }
        .background(.thinMaterial)
    }

    private func inspectorField(_ title: String, _ value: String, mono: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title.uppercased()).font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            Text(value).font(mono ? .system(size: 10, design: .monospaced) : .caption).textSelection(.enabled)
        }
    }

    private func provenanceBadge(_ value: VGLedgerProvenance) -> some View {
        Text(value.title.uppercased()).font(.system(size: 8, weight: .bold)).foregroundStyle(color(value)).padding(.horizontal, 6).padding(.vertical, 3).background(color(value).opacity(0.12), in: Capsule())
    }

    private func queryMatch(_ text: String) -> Bool {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return q.isEmpty || text.lowercased().contains(q)
    }

    private func replayStep(_ delta: Int) {
        let ordered = ledger.entries.sorted { $0.timestamp < $1.timestamp }
        guard !ordered.isEmpty else { return }
        let current = ordered.firstIndex(where: { $0.id == selectedEntryID }) ?? (delta > 0 ? -1 : ordered.count)
        let next = min(ordered.count - 1, max(0, current + delta))
        onSelectEntry(ordered[next].id)
    }

    private func count(_ mode: Mode) -> Int {
        switch mode { case .timeline: return ledger.entries.count; case .hypotheses: return ledger.hypotheses.count; case .bookmarks: return ledger.bookmarks.count; case .snapshots: return ledger.snapshots.count }
    }

    private func icon(_ mode: Mode) -> String {
        switch mode { case .timeline: return "clock.arrow.circlepath"; case .hypotheses: return "lightbulb"; case .bookmarks: return "bookmark"; case .snapshots: return "camera" }
    }

    private func color(_ provenance: VGLedgerProvenance) -> Color {
        switch provenance { case .observed: return .green; case .derived: return .blue; case .correlated: return .purple; case .asserted: return .orange }
    }
}
