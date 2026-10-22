import Foundation
#if canImport(CoreGraphics)
import CoreGraphics
#endif

struct VGCanvasViewport {
    let contentSize: CGSize
    let viewportSize: CGSize
    let zoom: CGFloat
    var pan: CGSize = .zero

    var origin: CGSize {
        CGSize(width: (viewportSize.width - contentSize.width * zoom) / 2 + pan.width,
               height: max(0, (viewportSize.height - contentSize.height * zoom) / 2) + pan.height)
    }

    var visibleRect: CGRect {
        let scale = max(zoom, 0.01)
        return CGRect(
            x: max(0, -origin.width / scale), y: max(0, -origin.height / scale),
            width: min(contentSize.width, viewportSize.width / scale),
            height: min(contentSize.height, viewportSize.height / scale)
        )
    }

    func clampedPan(_ candidate: CGSize) -> CGSize {
        let overflowX = max(0, contentSize.width * zoom - viewportSize.width)
        let overflowY = max(0, contentSize.height * zoom - viewportSize.height)
        return CGSize(
            width: min(overflowX / 2, max(-overflowX / 2, candidate.width)),
            height: min(0, max(-overflowY, candidate.height))
        )
    }

    func pan(centering point: CGPoint) -> CGSize {
        clampedPan(CGSize(
            width: (contentSize.width / 2 - point.x) * zoom,
            height: viewportSize.height / 2 - point.y * zoom
        ))
    }

    func fittedZoom(widthOnly: Bool, maximum: CGFloat) -> CGFloat {
        let widthScale = viewportSize.width / max(contentSize.width, 1)
        let heightScale = viewportSize.height / max(contentSize.height, 1)
        return max(0.1, min(maximum, widthScale, widthOnly ? maximum : heightScale))
    }
}

struct VGMinimapProjection {
    let contentSize: CGSize
    let mapSize: CGSize
    var padding: CGFloat = 6

    var scale: CGFloat {
        min(max(1, mapSize.width - padding * 2) / max(contentSize.width, 1),
            max(1, mapSize.height - padding * 2) / max(contentSize.height, 1))
    }

    var origin: CGPoint {
        CGPoint(x: (mapSize.width - contentSize.width * scale) / 2,
                y: (mapSize.height - contentSize.height * scale) / 2)
    }

    func project(_ point: CGPoint) -> CGPoint {
        CGPoint(x: origin.x + point.x * scale, y: origin.y + point.y * scale)
    }

    func project(_ rect: CGRect) -> CGRect {
        CGRect(origin: project(rect.origin), size: CGSize(width: rect.width * scale, height: rect.height * scale))
    }

    func unproject(_ point: CGPoint) -> CGPoint {
        CGPoint(x: min(contentSize.width, max(0, (point.x - origin.x) / scale)),
                y: min(contentSize.height, max(0, (point.y - origin.y) / scale)))
    }
}

struct VGGraphPoint: Hashable {
    let x: Double
    let y: Double
}

struct VGFunctionRecord: Identifiable, Hashable {
    let start: UInt64
    let end: UInt64
    let byteCount: UInt64
    let name: String

    var id: UInt64 { start }
    var startText: String { String(format: "0x%016llx", start) }

    var extentLabel: String { end > start && byteCount > 0 ? "\(byteCount) B" : "Extent unknown" }

    func contains(_ address: UInt64) -> Bool {
        end == start ? address == start : address >= start && address < end
    }
}

struct VGCFGEdge: Identifiable, Hashable {
    let sourceAddress: UInt64
    let targetAddress: UInt64
    let kind: String
    let sourceBlockID: String
    let targetBlockID: String?

    var id: String {
        "\(sourceBlockID):\(sourceAddress):\(kind):\(targetAddress)"
    }

    var isCall: Bool { kind == "call" }
    var isFallthrough: Bool { kind == "fallthrough" }
    var isConditional: Bool { kind.contains("cond") || kind == "cbz" || kind == "cbnz" || kind == "tbz" || kind == "tbnz" }
}

struct VGCFGBlock: Identifiable, Hashable {
    let id: String
    let start: UInt64
    let end: UInt64
    let edges: [VGCFGEdge]

    var startText: String { String(format: "0x%016llx", start) }
    var endText: String { String(format: "0x%016llx", end) }
    var byteCount: UInt64 { end >= start ? end - start : 0 }

    func contains(_ address: UInt64) -> Bool {
        address >= start && address < end
    }
}

struct VGCFGLayout {
    let positions: [String: VGGraphPoint]
    let width: Double
    let height: Double
    let visibleBlockIDs: Set<String>
}

struct VGCFGGraph {
    let blocks: [VGCFGBlock]
    let edges: [VGCFGEdge]

    func block(containing address: UInt64) -> VGCFGBlock? {
        blocks.first { $0.contains(address) }
    }

    func block(startingAt address: UInt64) -> VGCFGBlock? {
        blocks.first { $0.start == address }
    }

    func layout(limit: Int = 240, functionStarts: Set<UInt64> = [], availableWidth: Double? = nil, availableHeight: Double? = nil) -> VGCFGLayout {
        let visible = Array(blocks.prefix(max(1, limit)))
        guard !visible.isEmpty else {
            return VGCFGLayout(positions: [:], width: 640, height: 420, visibleBlockIDs: [])
        }

        let visibleIDs = Set(visible.map(\.id))
        let byID = Dictionary(uniqueKeysWithValues: visible.map { ($0.id, $0) })
        var successors: [String: [String]] = [:]
        var neighbors: [String: [String]] = [:]

        for edge in edges {
            guard visibleIDs.contains(edge.sourceBlockID), let target = edge.targetBlockID, visibleIDs.contains(target) else { continue }
            successors[edge.sourceBlockID, default: []].append(target)
            neighbors[edge.sourceBlockID, default: []].append(target)
            neighbors[target, default: []].append(edge.sourceBlockID)
        }
        let nodeWidth = 290.0
        let nodeHeight = 126.0
        let horizontalGap = 54.0
        let verticalGap = 76.0
        let margin = 70.0
        struct ComponentLayout {
            let positions: [String: VGGraphPoint]
            let width: Double
            let height: Double
        }

        // Keep connected branches in their own layers. Only independent graphs
        // may wrap; wrapping a branch layer destroys its visible fork/join shape.
        var remaining = visibleIDs
        var components: [ComponentLayout] = []
        for seed in visible where remaining.contains(seed.id) {
            var members: [String] = [seed.id]
            remaining.remove(seed.id)
            var cursor = 0
            while cursor < members.count {
                let id = members[cursor]
                cursor += 1
                for neighbor in neighbors[id, default: []] where remaining.remove(neighbor) != nil {
                    members.append(neighbor)
                }
            }
            members.sort {
                let left = byID[$0]?.start ?? 0
                let right = byID[$1]?.start ?? 0
                return left == right ? $0 < $1 : left < right
            }
            var incoming = Dictionary(uniqueKeysWithValues: members.map { ($0, 0) })
            for id in members {
                for target in successors[id, default: []] { incoming[target, default: 0] += 1 }
            }
            let roots = members.filter { id in
                incoming[id] == 0 || byID[id].map { functionStarts.contains($0.start) } == true
            }
            var queue = (roots.isEmpty ? [members[0]] : roots).map { ($0, 0) }
            var levels: [String: Int] = [:]
            cursor = 0
            func visitQueue() {
                while cursor < queue.count {
                    let (id, depth) = queue[cursor]
                    cursor += 1
                    guard levels[id] == nil else { continue }
                    levels[id] = depth
                    for target in successors[id, default: []] where levels[target] == nil {
                        queue.append((target, depth + 1))
                    }
                }
            }
            visitQueue()
            for id in members where levels[id] == nil {
                queue.append((id, 0))
                visitQueue()
            }
            var rows: [Int: [String]] = [:]
            for id in members { rows[levels[id] ?? 0, default: []].append(id) }
            let columns = rows.values.map(\.count).max() ?? 1
            let width = Double(columns) * nodeWidth + Double(columns - 1) * horizontalGap
            let height = Double(rows.count) * nodeHeight + Double(rows.count - 1) * verticalGap
            var points: [String: VGGraphPoint] = [:]
            for (depth, row) in rows {
                let rowWidth = Double(row.count) * nodeWidth + Double(row.count - 1) * horizontalGap
                for (column, id) in row.enumerated() {
                    points[id] = VGGraphPoint(
                        x: (width - rowWidth) / 2 + nodeWidth / 2 + Double(column) * (nodeWidth + horizontalGap),
                        y: nodeHeight / 2 + Double(depth) * (nodeHeight + verticalGap)
                    )
                }
            }
            components.append(ComponentLayout(positions: points, width: width, height: height))
        }

        let componentGap = 64.0
        let minimumContentWidth = components.count > 1 ? nodeWidth * 2 + componentGap : nodeWidth
        let targetWidth = max(760, availableWidth ?? 1100, margin * 2 + minimumContentWidth, margin * 2 + (components.map(\.width).max() ?? 0))
        var shelves: [[ComponentLayout]] = []
        var shelf: [ComponentLayout] = []
        var shelfWidth = 0.0
        for component in components {
            let addedWidth = component.width + (shelf.isEmpty ? 0 : componentGap)
            if !shelf.isEmpty && shelfWidth + addedWidth > targetWidth - margin * 2 {
                shelves.append(shelf)
                shelf = []
                shelfWidth = 0
            }
            shelfWidth += component.width + (shelf.isEmpty ? 0 : componentGap)
            shelf.append(component)
        }
        if !shelf.isEmpty { shelves.append(shelf) }
        let contentHeight = shelves.reduce(0.0) { $0 + ($1.map(\.height).max() ?? 0) } + Double(max(0, shelves.count - 1)) * 96
        let targetHeight = max(520, availableHeight ?? 0, contentHeight + margin * 2)
        var y = max(margin, (targetHeight - contentHeight) / 2)
        var positions: [String: VGGraphPoint] = [:]
        for shelf in shelves {
            let width = shelf.reduce(0.0) { $0 + $1.width } + Double(shelf.count - 1) * componentGap
            var x = (targetWidth - width) / 2
            for component in shelf {
                let isSingleComponent = components.count == 1
                let localPoints = Array(component.positions.values)
                let minX = localPoints.map(\.x).min() ?? 0, maxX = localPoints.map(\.x).max() ?? 0
                let minY = localPoints.map(\.y).min() ?? 0, maxY = localPoints.map(\.y).max() ?? 0
                let xScale = isSingleComponent && maxX > minX
                    ? max(1, (targetWidth - 2 * margin - nodeWidth) / (maxX - minX)) : 1
                let yScale = isSingleComponent && maxY > minY
                    ? max(1, (targetHeight - 2 * margin - nodeHeight) / (maxY - minY)) : 1
                let targetMinY = margin + nodeHeight / 2
                for (id, point) in component.positions {
                    let expandedX = isSingleComponent
                        ? targetWidth / 2 + (point.x - (minX + maxX) / 2) * xScale
                        : x + point.x
                    let expandedY = isSingleComponent ? targetMinY + (point.y - minY) * yScale : y + point.y
                    positions[id] = VGGraphPoint(x: expandedX, y: expandedY)
                }
                x += component.width + componentGap
            }
            y += (shelf.map(\.height).max() ?? 0) + 96
        }
        return VGCFGLayout(positions: positions, width: targetWidth, height: targetHeight, visibleBlockIDs: visibleIDs)
    }
}

enum VGGraphParser {
    private static func hex(_ text: String) -> UInt64? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let raw = trimmed.hasPrefix("0x") ? String(trimmed.dropFirst(2)) : trimmed
        guard !raw.isEmpty, raw.count <= 16, raw.allSatisfy({ $0.isHexDigit }) else { return nil }
        return UInt64(raw, radix: 16)
    }

    static func functions(_ text: String, symbolResolver: String = "", objcClasses: String = "") -> [VGFunctionRecord] {
        var resolved: [UInt64: (priority: Int, name: String)] = [:]
        func offer(_ address: UInt64, _ name: String, priority: Int) {
            guard !name.isEmpty, !name.hasPrefix("<"), name.utf8.count <= 1024,
                  priority > (resolved[address]?.priority ?? 0) else { return }
            resolved[address] = (priority, name)
        }
        for raw in symbolResolver.split(separator: "\n") {
            let line = String(raw)
            guard line.hasPrefix("range start="),
                  let startField = line.dropFirst("range start=".count).split(separator: " ").first,
                  let address = hex(String(startField)),
                  let symbolRange = line.range(of: " symbol="),
                  let sourceRange = line.range(of: " source=", range: symbolRange.upperBound..<line.endIndex) else { continue }
            let name = String(line[symbolRange.upperBound..<sourceRange.lowerBound])
            let source = String(line[sourceRange.upperBound...].split(separator: " ").first ?? "")
            let priority = source == "nlist" ? 4 : source == "dyld-export" ? 2 : source == "indirect-symbol" ? 1 : 0
            offer(address, name, priority: priority)
        }
        for raw in objcClasses.split(separator: "\n") {
            let line = String(raw)
            guard line.contains(" method "), line.contains("confidence=validated"),
                  let imp = line.range(of: " imp="),
                  let address = hex(String(line[imp.upperBound...].split(separator: " ").first ?? "")),
                  let nameStart = line.range(of: " resolved="),
                  let confidence = line.range(of: " confidence=", range: nameStart.upperBound..<line.endIndex) else { continue }
            let name = String(line[nameStart.upperBound..<confidence.lowerBound]).trimmingCharacters(in: .whitespaces)
            guard (name.hasPrefix("-[") || name.hasPrefix("+[")), name.hasSuffix("]") else { continue }
            offer(address, name, priority: 3)
        }
        return text.split(separator: "\n", omittingEmptySubsequences: true).compactMap { raw in
            let line = String(raw).trimmingCharacters(in: .whitespacesAndNewlines)
            let parts = line.split(whereSeparator: { $0.isWhitespace })
            if let startToken = parts.first, let start = hex(String(startToken)),
               let nameRange = line.range(of: #"\bname="#, options: .regularExpression) {
                let tail = line[nameRange.upperBound...]
                let name = tail.components(separatedBy: " source=").first?.trimmingCharacters(in: .whitespacesAndNewlines)
                    ?? "<candidate>"
                return VGFunctionRecord(start: start, end: start, byteCount: 0, name: resolved[start]?.name ?? name)
            }
            guard parts.count >= 4,
                  let start = hex(String(parts[0])),
                  let end = hex(String(parts[1])),
                  let bytes = UInt64(parts[2]) else { return nil }
            let name = parts.dropFirst(4).map(String.init).joined(separator: " ")
            return VGFunctionRecord(start: start, end: end, byteCount: bytes, name: resolved[start]?.name ?? (name.isEmpty ? "<discovered>" : name))
        }
    }

    static func cfg(_ text: String, functionStarts: Set<UInt64> = []) -> VGCFGGraph {
        struct PendingBlock {
            let id: String
            let start: UInt64
            let end: UInt64
            var rawEdges: [(String, UInt64, UInt64)]
        }

        var pending: [PendingBlock] = []
        var current: PendingBlock?

        func finish(_ block: PendingBlock?, into items: inout [PendingBlock]) {
            if let block { items.append(block) }
        }

        for rawLine in text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init) {
            let line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("block ") {
                finish(current, into: &pending)
                let parts = line.split(whereSeparator: { $0.isWhitespace })
                guard parts.count >= 3 else { current = nil; continue }
                let id = String(parts[1])
                let range = String(parts[2]).components(separatedBy: "..")
                guard range.count == 2, let start = hex(range[0]), let end = hex(range[1]), end >= start else {
                    current = nil
                    continue
                }
                current = PendingBlock(id: id, start: start, end: end, rawEdges: [])
                continue
            }

            guard var block = current else { continue }
            let parts = line.split(whereSeparator: { $0.isWhitespace })
            guard parts.count >= 4,
                  let arrow = parts.firstIndex(of: "->"), arrow >= 2, arrow + 1 < parts.count,
                  let from = hex(String(parts[arrow - 1])),
                  let to = hex(String(parts[arrow + 1])) else { continue }
            let kind = parts[..<(arrow - 1)].map(String.init).joined(separator: " ")
            block.rawEdges.append((kind, from, to))
            current = block
        }
        finish(current, into: &pending)

        let startToID = Dictionary(uniqueKeysWithValues: pending.map { ($0.start, $0.id) })

        func blockID(containing address: UInt64) -> String? {
            if let exact = startToID[address] { return exact }
            return pending.first { $0.start <= address && address < $0.end }?.id
        }

        var blocks: [VGCFGBlock] = []
        var allEdges: [VGCFGEdge] = []
        for item in pending {
            let blockEdges = item.rawEdges.map { raw in
                VGCFGEdge(
                    sourceAddress: raw.1,
                    targetAddress: raw.2,
                    kind: raw.0,
                    sourceBlockID: item.id,
                    targetBlockID: blockID(containing: raw.2)
                )
            }
            blocks.append(VGCFGBlock(id: item.id, start: item.start, end: item.end, edges: blockEdges))
            allEdges.append(contentsOf: blockEdges)
        }

        // The Rust analyzer intentionally records only decoded control-flow
        // edges. A normal sequential transfer between two adjacent leaders is
        // still real CFG structure, so restore it here unless the next leader
        // starts a known function or the block already has a terminating edge.
        for index in blocks.indices.dropLast() where !functionStarts.isEmpty {
            let current = blocks[index]
            let next = blocks[index + 1]
            guard current.end == next.start,
                  !functionStarts.contains(next.start),
                  !current.edges.contains(where: { !$0.isCall }) else { continue }

            let edge = VGCFGEdge(
                sourceAddress: current.end > current.start ? current.end - 4 : current.start,
                targetAddress: next.start,
                kind: "fallthrough",
                sourceBlockID: current.id,
                targetBlockID: next.id
            )
            blocks[index] = VGCFGBlock(id: current.id, start: current.start, end: current.end, edges: current.edges + [edge])
            allEdges.append(edge)
        }
        return VGCFGGraph(blocks: blocks, edges: allEdges)
    }

    static func address(from text: String) -> UInt64? {
        let lower = text.lowercased()
        guard let range = lower.range(of: "0x[0-9a-f]{1,16}", options: .regularExpression) else { return nil }
        return hex(String(lower[range]))
    }
}
