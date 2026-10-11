import Foundation

@main
struct TraceExplorerContract {
    static func main() {
        let functionsText = """
        0x0000000100001000  0x0000000100001010        16 bytes  main
        0x0000000100002000  0x0000000100002010        16 bytes  helper
        """
        let disassemblyText = """
        0000000100001000 0000000000000000 94000400 bl 0x0000000100002000
        0000000100001004 0000000000000004 d503201f nop
        0000000100001008 0000000000000008 d65f03c0 ret
        0000000100002000 0000000000001000 d503201f nop
        0000000100002004 0000000000001004 d65f03c0 ret
        """
        let modulesText = "MODULES (1)\n0x0000000100100000 slide=+1048576 preferred=0x0000000100000000  /tmp/demo"
        let functions = VGGraphParser.functions(functionsText)
        let rows = VGDisassemblyParser.parse(disassemblyText)

        func snapshot(pc: String, x0: String) -> DebugSnapshot {
            DebugSnapshot(pid: 42, stopped: true, thread_count: 1, selected_thread_id: "1", pc: pc, sp: "0x7000", fp: "0x7100", lr: "0x100101004", cpsr: 0, registers: ["x0": x0], breakpoints: [])
        }

        let s1 = VGRuntimeSample.capture(sequence: 1, snapshot: snapshot(pc: "0x100101000", x0: "0x1"), previous: nil, event: "stopped", reason: "breakpoint", timestamp: 1.0)!
        let s2 = VGRuntimeSample.capture(sequence: 2, snapshot: snapshot(pc: "0x100102000", x0: "0x2"), previous: s1, event: "stopped", reason: "step", timestamp: 2.0)!
        let s3 = VGRuntimeSample.capture(sequence: 3, snapshot: snapshot(pc: "0x100102004", x0: "0x2"), previous: s2, event: "stopped", reason: "step", timestamp: 3.0)!

        let events = VGTraceExplorerEngine.events(samples: [s1, s2, s3], binaryPath: "/tmp/demo", modulesText: modulesText, functions: functions, disassembly: rows)
        precondition(events.count == 3)
        precondition(events[0].transition == .origin)
        precondition(events[1].transition == .callSite)
        precondition(events[2].transition == .sequential)
        precondition(events[1].functionName == "helper")
        precondition(events[1].changedRegisters.count == 1)

        let summary = VGTraceExplorerEngine.summary(events: events, memoryObservations: [])
        precondition(summary.eventCount == 3)
        precondition(summary.correlatedCount == 3)
        precondition(summary.uniqueStaticPCs == 3)
        precondition(summary.functionCount == 2)

        let functionsSeen = VGTraceExplorerEngine.functionSummary(events)
        precondition(functionsSeen.first?.name == "helper")
        precondition(functionsSeen.first?.observations == 2)

        let lanes = VGTraceExplorerEngine.registerLanes(events)
        precondition(lanes.first?.name == "x0")
        precondition(lanes.first?.changes == 2)

        let ambiguousModules = modulesText + "\n0x0000000200100000 slide=+1048576 preferred=0x0000000200000000  /other/demo"
        let unresolved = VGTraceExplorerEngine.events(samples: [s1], binaryPath: "/missing/demo", modulesText: ambiguousModules, functions: functions, disassembly: rows)
        precondition(unresolved.first?.preferredPC == nil)
        precondition(unresolved.first?.transition == .origin)

        print("trace explorer contract passed")
    }
}
