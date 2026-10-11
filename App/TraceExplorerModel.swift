import Foundation

struct VGTraceEvent: Identifiable, Hashable {
    enum TransitionKind: String, Hashable {
        case origin = "origin"
        case sequential = "sequential"
        case callSite = "call-site"
        case branchSite = "branch-site"
        case returnSite = "return-site"
        case observedTransfer = "observed-transfer"
        case unresolved = "unresolved"
    }

    let id: Int
    let sequence: Int
    let timestamp: TimeInterval
    let runtimePC: UInt64
    let preferredPC: UInt64?
    let functionName: String?
    let modulePath: String?
    let confidence: String
    let event: String
    let reason: String
    let threadID: String?
    let changedRegisters: [VGRuntimeRegisterDelta]
    let breakpoints: [UInt64]
    let transition: TransitionKind
    let sourceMnemonic: String?
    let sourceOperands: String?
    let explanation: String

    var runtimeText: String { VGTargetUtilities.hex(runtimePC) }
    var preferredText: String { preferredPC.map(VGTargetUtilities.hex) ?? "unresolved" }
    var timeText: String {
        let date = Date(timeIntervalSince1970: timestamp)
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss.SSS"
        return formatter.string(from: date)
    }
}

struct VGTraceFunctionSummary: Identifiable, Hashable {
    var id: String { name + "@" + addressText }
    let name: String
    let address: UInt64
    let observations: Int
    var addressText: String { VGTargetUtilities.hex(address) }
}

struct VGTraceRegisterLane: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let changes: Int
    let latestValue: String
}

struct VGTraceSessionSummary: Hashable {
    let eventCount: Int
    let correlatedCount: Int
    let unresolvedCount: Int
    let uniqueStaticPCs: Int
    let functionCount: Int
    let registerChangeCount: Int
    let memoryObservationCount: Int
}

enum VGTraceExplorerEngine {
    static func events(
        samples: [VGRuntimeSample],
        binaryPath: String,
        modulesText: String,
        functions: [VGFunctionRecord],
        disassembly: [VGDisassemblyRow]
    ) -> [VGTraceEvent] {
        let ordered = samples.sorted {
            if $0.timestamp == $1.timestamp { return $0.id < $1.id }
            return $0.timestamp < $1.timestamp
        }
        let rowByAddress: [UInt64: VGDisassemblyRow] = Dictionary(uniqueKeysWithValues: disassembly.compactMap { row in
            guard row.kind == .instruction, let address = VGTargetUtilities.parseHex(row.address) else { return nil }
            return (address, row)
        })

        var output: [VGTraceEvent] = []
        output.reserveCapacity(ordered.count)
        var previousCorrelation: VGRuntimeCorrelation?

        for (index, sample) in ordered.enumerated() {
            let correlation = VGRuntimeCorrelationEngine.correlate(sample, binaryPath: binaryPath, modulesText: modulesText, functions: functions)
            let sourceRow = previousCorrelation?.preferredPC.flatMap { rowByAddress[$0] }
            let transition = classifyTransition(previous: previousCorrelation, current: correlation, source: sourceRow, isFirst: index == 0)
            let explanation = transitionExplanation(transition, source: sourceRow, previous: previousCorrelation, current: correlation)
            output.append(VGTraceEvent(
                id: sample.id,
                sequence: index + 1,
                timestamp: sample.timestamp,
                runtimePC: sample.pc,
                preferredPC: correlation.preferredPC,
                functionName: correlation.functionName,
                modulePath: correlation.modulePath,
                confidence: correlation.confidence,
                event: sample.event,
                reason: sample.reason,
                threadID: sample.threadID,
                changedRegisters: sample.changedRegisters,
                breakpoints: sample.breakpoints,
                transition: transition,
                sourceMnemonic: sourceRow?.mnemonic,
                sourceOperands: sourceRow?.operands,
                explanation: explanation
            ))
            previousCorrelation = correlation
        }
        return output
    }

    private static func classifyTransition(
        previous: VGRuntimeCorrelation?,
        current: VGRuntimeCorrelation,
        source: VGDisassemblyRow?,
        isFirst: Bool
    ) -> VGTraceEvent.TransitionKind {
        if isFirst { return .origin }
        guard let previous, let from = previous.preferredPC, let to = current.preferredPC else { return .unresolved }
        if let source {
            let mnemonic = source.mnemonic.lowercased()
            if source.isCall { return .callSite }
            if mnemonic == "ret" { return .returnSite }
            if source.isControlFlow { return .branchSite }
        }
        let next = from.addingReportingOverflow(4)
        if !next.overflow, next.partialValue == to { return .sequential }
        return .observedTransfer
    }

    private static func transitionExplanation(
        _ transition: VGTraceEvent.TransitionKind,
        source: VGDisassemblyRow?,
        previous: VGRuntimeCorrelation?,
        current: VGRuntimeCorrelation
    ) -> String {
        switch transition {
        case .origin:
            return "First captured debugger observation in this bounded session history."
        case .sequential:
            return "Consecutive observations correlate to adjacent static instruction addresses. This does not prove no instructions executed between debugger observations."
        case .callSite:
            return "The previous correlated static instruction is a call site (\(source?.mnemonic ?? "call")). The debugger only observed the endpoints; HindSight does not claim a complete instruction trace."
        case .branchSite:
            return "The previous correlated static instruction is control flow (\(source?.mnemonic ?? "branch")). The transition is classified from static evidence plus observed endpoints."
        case .returnSite:
            return "The previous correlated static instruction is a return site. The destination was observed, not reconstructed from an assumed call stack."
        case .observedTransfer:
            let from = previous?.preferredText ?? "unresolved"
            return "Debugger observations moved from \(from) to \(current.preferredText), but the previous static instruction does not establish the intervening path."
        case .unresolved:
            return "At least one endpoint could not be uniquely correlated to the analyzed image, so no static transition was inferred."
        }
    }

    static func functionSummary(_ events: [VGTraceEvent]) -> [VGTraceFunctionSummary] {
        var counts: [String: (UInt64, Int)] = [:]
        for event in events {
            guard let address = event.preferredPC, let name = event.functionName else { continue }
            let current = counts[name] ?? (address, 0)
            counts[name] = (min(current.0, address), current.1 + 1)
        }
        return counts.map { key, value in
            VGTraceFunctionSummary(name: key, address: value.0, observations: value.1)
        }.sorted {
            if $0.observations == $1.observations { return $0.address < $1.address }
            return $0.observations > $1.observations
        }
    }

    static func registerLanes(_ events: [VGTraceEvent]) -> [VGTraceRegisterLane] {
        var changes: [String: (Int, String)] = [:]
        for event in events {
            for delta in event.changedRegisters {
                let current = changes[delta.name] ?? (0, delta.after)
                changes[delta.name] = (current.0 + 1, delta.after)
            }
        }
        return changes.map { VGTraceRegisterLane(name: $0.key, changes: $0.value.0, latestValue: $0.value.1) }
            .sorted {
                if $0.changes == $1.changes { return $0.name < $1.name }
                return $0.changes > $1.changes
            }
    }

    static func nearbyMemory(_ observations: [VGRuntimeMemoryObservation], event: VGTraceEvent?, window: TimeInterval = 1.5) -> [VGRuntimeMemoryObservation] {
        guard let event else { return Array(observations.suffix(8)) }
        return observations
            .filter { abs($0.timestamp - event.timestamp) <= window }
            .sorted { abs($0.timestamp - event.timestamp) < abs($1.timestamp - event.timestamp) }
            .prefix(8)
            .map { $0 }
    }

    static func summary(events: [VGTraceEvent], memoryObservations: [VGRuntimeMemoryObservation]) -> VGTraceSessionSummary {
        let correlated = events.filter { $0.preferredPC != nil }
        let staticPCs = Set(correlated.compactMap(\.preferredPC))
        let functions = Set(correlated.compactMap(\.functionName))
        return VGTraceSessionSummary(
            eventCount: events.count,
            correlatedCount: correlated.count,
            unresolvedCount: events.count - correlated.count,
            uniqueStaticPCs: staticPCs.count,
            functionCount: functions.count,
            registerChangeCount: events.reduce(0) { $0 + $1.changedRegisters.count },
            memoryObservationCount: memoryObservations.count
        )
    }
}
