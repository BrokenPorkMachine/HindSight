import Foundation

@main
struct RuntimeCorrelationContract {
    static func main() {
        let snap1 = DebugSnapshot(pid: 42, stopped: true, thread_count: 1, selected_thread_id: "0x77", pc: "0x0000000100011020", sp: "0x7000", fp: "0x7100", lr: "0x100011040", cpsr: 0, registers: ["x0":"0x1", "x1":"0x2"], breakpoints: ["0x100011020"])
        let first = VGRuntimeSample.capture(sequence: 1, snapshot: snap1, previous: nil, event: "stopped", reason: "single-step", timestamp: 1.0)!
        precondition(first.changedRegisters.count == 2)

        let snap2 = DebugSnapshot(pid: 42, stopped: true, thread_count: 1, selected_thread_id: "0x77", pc: "0x0000000100011024", sp: "0x7000", fp: "0x7100", lr: "0x100011040", cpsr: 0, registers: ["x0":"0x3", "x1":"0x2"], breakpoints: ["0x100011020"])
        let second = VGRuntimeSample.capture(sequence: 2, snapshot: snap2, previous: first, event: "stopped", reason: "single-step", timestamp: 2.0)!
        precondition(second.changedRegisters.map(\.name) == ["x0"])

        let modules = """
        MODULES (2)
        0x0000000100010000 slide=+65536 preferred=0x0000000100000000  /tmp/Sample
        0x0000000190000000 slide=+0 preferred=0x0000000190000000  /usr/lib/libSystem.B.dylib
        """
        let functions = [VGFunctionRecord(start: 0x100001000, end: 0x100001100, byteCount: 0x100, name: "sub_100001000")]
        let corr = VGRuntimeCorrelationEngine.correlate(second, binaryPath: "/tmp/Sample", modulesText: modules, functions: functions)
        precondition(corr.preferredPC == 0x100001024)
        precondition(corr.functionName == "sub_100001000")
        precondition(corr.confidence == "correlated")

        let heat = VGRuntimeCorrelationEngine.heat(samples: [first, second, second], binaryPath: "/tmp/Sample", modulesText: modules)
        precondition(heat[0x100001024] == 2)

        let ambiguous = modules + "\n0x0000000200000000 slide=+0 preferred=0x0000000200000000  /another/Sample\n"
        precondition(VGRuntimeCorrelationEngine.moduleForBinary("/not/exact/Sample", modulesText: ambiguous) == nil)

        let memory = DebugMemory(address: "0x1000", length: 4, hex: "0011aaff")
        let observation = VGRuntimeCorrelationEngine.memoryObservation(sequence: 1, memory: memory, timestamp: 3.0)!
        precondition(observation.bytes == [0x00,0x11,0xaa,0xff])
        precondition(observation.preview == "00 11 aa ff")

        print("Runtime correlation contract passed")
    }
}
