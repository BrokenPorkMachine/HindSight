import Foundation

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
        var incoming: [String: Int] = Dictionary(uniqueKeysWithValues: visible.map { ($0.id, 0) })
        var successors: [String: [String]] = [:]

        for edge in edges where !edge.isCall {
            guard visibleIDs.contains(edge.sourceBlockID), let target = edge.targetBlockID, visibleIDs.contains(target) else { continue }
            successors[edge.sourceBlockID, default: []].append(target)
            incoming[target, default: 0] += 1
        }

        let orderedRoots = visible.filter { incoming[$0.id, default: 0] == 0 }
        var queue: [(String, Int)] = orderedRoots.map { ($0.id, 0) }
        if queue.isEmpty, let first = visible.first { queue = [(first.id, 0)] }

        var level: [String: Int] = [:]
        var cursor = 0
        while cursor < queue.count {
            let (id, depth) = queue[cursor]
            cursor += 1
            if let current = level[id], current <= depth { continue }
            level[id] = depth
            guard depth < 96 else { continue }
            for target in successors[id, default: []] where byID[target] != nil {
                if target == id { continue }
                let candidate = depth + 1
                if let existing = level[target] {
                    if candidate < existing { queue.append((target, candidate)) }
                } else {
                    queue.append((target, candidate))
                }
            }
        }

        var nextDisconnectedLevel = (level.values.max() ?? -1) + 1
        for block in visible where level[block.id] == nil {
            level[block.id] = nextDisconnectedLevel
            nextDisconnectedLevel += 1
        }

        var rowsByLevel: [Int: [VGCFGBlock]] = [:]
        for block in visible {
            rowsByLevel[level[block.id, default: 0], default: []].append(block)
        }
        for key in rowsByLevel.keys {
            rowsByLevel[key]?.sort { $0.start < $1.start }
        }

        let nodeWidth = 290.0
        let nodeHeight = 126.0
        let horizontalGap = 54.0
        // Keep blocks within a function readable while giving adjacent function
        // lanes a clearly visible separation. The canvas grows to make room.
        let functionGap = functionStarts.count > 1 ? 260.0 : 0.0
        let verticalGap = 76.0
        let margin = 70.0
        let maxRowCount = max(1, rowsByLevel.values.map(\.count).max() ?? 1)
        let functionFor: (UInt64) -> UInt64 = { address in
            functionStarts.filter { $0 <= address }.max() ?? 0
        }
        let rowWidth: ([VGCFGBlock]) -> Double = { row in
            guard !row.isEmpty else { return 0 }
            let functionBreaks = zip(row, row.dropFirst()).reduce(0) { total, pair in
                total + (functionFor(pair.0.start) != functionFor(pair.1.start) ? functionGap : 0)
            }
            return Double(row.count) * nodeWidth + Double(max(0, row.count - 1)) * horizontalGap + functionBreaks
        }
        let canvasWidth = max(760.0, margin * 2 + (rowsByLevel.values.map(rowWidth).max() ?? (Double(maxRowCount) * nodeWidth)))
        let targetWidth = max(canvasWidth, availableWidth ?? 0)
        let rowExpansion = max(0, targetWidth - canvasWidth) / Double(max(1, maxRowCount - 1))
        var positions: [String: VGGraphPoint] = [:]
        var currentY = margin + nodeHeight / 2
        var previousFunctions: Set<UInt64> = []
        let levelCount = rowsByLevel.count
        let intrinsicHeight = margin * 2 + Double(levelCount) * nodeHeight + Double(max(0, levelCount - 1)) * verticalGap
        let rowExpansionY = max(0, (availableHeight ?? intrinsicHeight) - intrinsicHeight) / Double(max(1, levelCount - 1))

        for graphLevel in rowsByLevel.keys.sorted() {
            let row = rowsByLevel[graphLevel] ?? []
            let rowFunctions = Set(row.map { functionFor($0.start) }.filter { $0 != 0 })
            if !previousFunctions.isEmpty && !rowFunctions.isEmpty && previousFunctions.isDisjoint(with: rowFunctions) {
                currentY += functionGap
            }
            let width = rowWidth(row) + Double(max(0, row.count - 1)) * rowExpansion
            let startX = (targetWidth - width) / 2 + nodeWidth / 2
            var x = startX
            for (index, block) in row.enumerated() {
                positions[block.id] = VGGraphPoint(
                    x: x,
                    y: currentY
                )
                if index + 1 < row.count {
                    let nextGap = horizontalGap + rowExpansion + (functionFor(block.start) != functionFor(row[index + 1].start) ? functionGap : 0)
                    x += nodeWidth + nextGap
                }
            }
            currentY += nodeHeight + verticalGap + rowExpansionY
            previousFunctions = rowFunctions
        }

        let canvasHeight = max(520.0, currentY - verticalGap + margin + nodeHeight / 2)
        return VGCFGLayout(positions: positions, width: targetWidth, height: max(canvasHeight, availableHeight ?? 0), visibleBlockIDs: visibleIDs)
    }
}

enum VGGraphParser {
    private static func hex(_ text: String) -> UInt64? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let raw = trimmed.hasPrefix("0x") ? String(trimmed.dropFirst(2)) : trimmed
        guard !raw.isEmpty, raw.count <= 16, raw.allSatisfy({ $0.isHexDigit }) else { return nil }
        return UInt64(raw, radix: 16)
    }

    static func functions(_ text: String) -> [VGFunctionRecord] {
        text.split(separator: "\n", omittingEmptySubsequences: true).compactMap { raw in
            let line = String(raw).trimmingCharacters(in: .whitespacesAndNewlines)
            let parts = line.split(whereSeparator: { $0.isWhitespace })
            if let startToken = parts.first, let start = hex(String(startToken)),
               let nameRange = line.range(of: #"\bname="#, options: .regularExpression) {
                let tail = line[nameRange.upperBound...]
                let name = tail.components(separatedBy: " source=").first?.trimmingCharacters(in: .whitespacesAndNewlines)
                    ?? "<candidate>"
                return VGFunctionRecord(start: start, end: start, byteCount: 0, name: name)
            }
            guard parts.count >= 4,
                  let start = hex(String(parts[0])),
                  let end = hex(String(parts[1])),
                  let bytes = UInt64(parts[2]) else { return nil }
            let name = parts.dropFirst(4).map(String.init).joined(separator: " ")
            return VGFunctionRecord(start: start, end: end, byteCount: bytes, name: name.isEmpty ? "<discovered>" : name)
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
