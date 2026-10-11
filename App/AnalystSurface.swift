import Foundation
import SwiftUI
#if os(macOS)
import AppKit
#endif

enum VGInspectorMode: String, CaseIterable, Identifiable {
    case context = "Context"
    case evidence = "Evidence"
    case target = "Target"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .context: return "scope"
        case .evidence: return "checkmark.seal"
        case .target: return "cpu"
        }
    }
}

struct VGWorkspaceTabStrip: View {
    let workspaces: [String]
    let activeWorkspace: String
    let onSelect: (String) -> Void
    let onClose: (String) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                ForEach(workspaces, id: \.self) { name in
                    let descriptor = VGWorkspaceCatalog.descriptor(for: name)
                    HStack(spacing: 7) {
                        Button {
                            onSelect(name)
                        } label: {
                            HStack(spacing: 7) {
                                Image(systemName: descriptor.icon)
                                    .font(.system(size: 11, weight: .semibold))
                                    .symbolRenderingMode(.hierarchical)
                                Text(descriptor.title)
                                    .font(.system(size: 12, weight: name == activeWorkspace ? .semibold : .medium))
                                    .lineLimit(1)
                            }
                        }
                        .buttonStyle(.plain)

                        if workspaces.count > 1 {
                            Button {
                                onClose(name)
                            } label: {
                                Image(systemName: "xmark")
                                    .font(.system(size: 8, weight: .bold))
                                    .foregroundStyle(.secondary)
                            }
                            .buttonStyle(.plain)
                            .help("Close \(descriptor.title)")
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background {
                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                            .fill(name == activeWorkspace ? Color.primary.opacity(0.095) : Color.clear)
                    }
                    .overlay {
                        if name == activeWorkspace {
                            RoundedRectangle(cornerRadius: 7, style: .continuous)
                                .strokeBorder(Color.primary.opacity(0.08), lineWidth: 1)
                        }
                    }
                    .contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }
}

struct VGMetricTile: View {
    let title: String
    let value: String
    let detail: String
    let icon: String

    var body: some View {
        HStack(spacing: 11) {
            ZStack {
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .fill(Color.accentColor.opacity(0.11))
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .semibold))
                    .symbolRenderingMode(.hierarchical)
            }
            .frame(width: 34, height: 34)

            VStack(alignment: .leading, spacing: 1) {
                Text(value)
                    .font(.system(size: 17, weight: .semibold, design: .rounded))
                Text(title)
                    .font(.caption)
                    .fontWeight(.medium)
                Text(detail)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 11, style: .continuous)
                .strokeBorder(Color.primary.opacity(0.07), lineWidth: 1)
        }
    }
}

struct VGRichDisassemblyView: View {
    let text: String
    let selectedAddress: String
    let emptyMessage: String
    let showFileOffset: Bool
    let onSelectAddress: (String) -> Void

    private var rows: [VGDisassemblyRow] { VGDisassemblyParser.parse(text) }
    private var normalizedSelection: String {
        selectedAddress.lowercased().replacingOccurrences(of: "0x", with: "").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        if rows.isEmpty {
            ContentUnavailableView("No instructions", systemImage: "text.alignleft", description: Text(emptyMessage))
        } else {
            ScrollView([.horizontal, .vertical]) {
                LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                    Section {
                        ForEach(rows) { row in
                            rowView(row)
                        }
                    } header: {
                        HStack(spacing: 0) {
                            Text("ADDRESS").frame(width: 168, alignment: .leading)
                            if showFileOffset { Text("FILE OFFSET").frame(width: 150, alignment: .leading) }
                            Text("WORD").frame(width: 92, alignment: .leading)
                            Text("INSTRUCTION").frame(minWidth: 520, maxWidth: .infinity, alignment: .leading)
                        }
                        .font(.system(size: 9, weight: .bold, design: .rounded))
                        .foregroundStyle(.secondary)
                        .tracking(0.7)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 7)
                        .background(.bar)
                    }
                }
            }
            .background(Color.primary.opacity(0.018), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.07), lineWidth: 1)
            }
        }
    }

    @ViewBuilder private func rowView(_ row: VGDisassemblyRow) -> some View {
        switch row.kind {
        case .symbol:
            HStack(spacing: 8) {
                Image(systemName: "bookmark.fill")
                    .font(.caption2)
                    .foregroundStyle(Color.accentColor)
                Text(row.address)
                    .font(.system(size: 11, weight: .medium, design: .monospaced))
                Text(row.mnemonic)
                    .font(.system(size: 12, weight: .semibold, design: .monospaced))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .frame(minWidth: 780, maxWidth: .infinity, alignment: .leading)
            .background(Color.accentColor.opacity(0.055))

        case .metadata:
            Text(row.raw)
                .font(.system(size: 10, design: .monospaced))
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .frame(minWidth: 780, maxWidth: .infinity, alignment: .leading)

        case .instruction:
            Button {
                onSelectAddress(row.address)
            } label: {
                HStack(spacing: 0) {
                    HStack(spacing: 6) {
                        Image(systemName: row.isCall ? "arrow.turn.up.right" : row.isControlFlow ? "arrow.triangle.branch" : "circle.fill")
                            .font(.system(size: row.isControlFlow ? 9 : 4, weight: .semibold))
                            .foregroundStyle(row.isControlFlow ? AnyShapeStyle(Color.accentColor) : AnyShapeStyle(Color.gray.opacity(0.5)))
                            .frame(width: 12)
                        Text(row.address)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                    .frame(width: 168, alignment: .leading)

                    if showFileOffset {
                        Text(row.fileOffset)
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundStyle(.tertiary)
                            .frame(width: 150, alignment: .leading)
                    }

                    Text(row.word)
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundStyle(.tertiary)
                        .frame(width: 92, alignment: .leading)

                    HStack(spacing: 8) {
                        Text(row.mnemonic)
                            .font(.system(size: 11, weight: .semibold, design: .monospaced))
                            .frame(width: 64, alignment: .leading)
                        Text(row.operands)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(.primary)
                        if !row.annotation.isEmpty {
                            Text("; \(row.annotation)")
                                .font(.system(size: 10, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 30)
                    }
                    .frame(minWidth: 520, maxWidth: .infinity, alignment: .leading)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .contentShape(Rectangle())
                .background {
                    if !normalizedSelection.isEmpty, row.normalizedAddress == normalizedSelection {
                        RoundedRectangle(cornerRadius: 5, style: .continuous)
                            .fill(Color.accentColor.opacity(0.13))
                    }
                }
            }
            .buttonStyle(.plain)
            .contextMenu {
                Button("Use Address") { onSelectAddress(row.address) }
                Button("Copy Instruction") {
                    #if os(macOS)
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(row.raw, forType: .string)
                    #endif
                }
            }
        }
    }
}
