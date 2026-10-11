import Foundation

struct VGRuntimeRegisterDelta: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let before: String?
    let after: String
}

struct VGRuntimeSample: Identifiable, Hashable {
    let id: Int
    let timestamp: TimeInterval
    let event: String
    let reason: String
    let threadID: String?
    let stopped: Bool
    let pc: UInt64
    let sp: UInt64?
    let fp: UInt64?
    let lr: UInt64?
    let registers: [String: String]
    let changedRegisters: [VGRuntimeRegisterDelta]
    let breakpoints: [UInt64]

    var pcText: String { VGTargetUtilities.hex(pc) }

    static func capture(
        sequence: Int,
        snapshot: DebugSnapshot,
        previous: VGRuntimeSample?,
        event: String,
        reason: String,
        timestamp: TimeInterval = Date().timeIntervalSince1970
    ) -> VGRuntimeSample? {
        guard let pc = VGTargetUtilities.parseHex(snapshot.pc) else { return nil }
        let deltas = snapshot.registers.keys.sorted().compactMap { name -> VGRuntimeRegisterDelta? in
            guard let after = snapshot.registers[name] else { return nil }
            let before = previous?.registers[name]
            guard before != after else { return nil }
            return VGRuntimeRegisterDelta(name: name, before: before, after: after)
        }
        return VGRuntimeSample(
            id: sequence,
            timestamp: timestamp,
            event: event,
            reason: reason,
            threadID: snapshot.selected_thread_id,
            stopped: snapshot.stopped,
            pc: pc,
            sp: VGTargetUtilities.parseHex(snapshot.sp),
            fp: VGTargetUtilities.parseHex(snapshot.fp),
            lr: VGTargetUtilities.parseHex(snapshot.lr),
            registers: snapshot.registers,
            changedRegisters: deltas,
            breakpoints: snapshot.breakpoints.compactMap(VGTargetUtilities.parseHex)
        )
    }
}

struct VGRuntimeMemoryObservation: Identifiable, Hashable {
    let id: Int
    let timestamp: TimeInterval
    let address: UInt64
    let bytes: [UInt8]

    var addressText: String { VGTargetUtilities.hex(address) }
    var preview: String {
        bytes.prefix(24).map { String(format: "%02x", $0) }.joined(separator: " ") + (bytes.count > 24 ? " …" : "")
    }
}

struct VGRuntimeModule: Identifiable, Hashable {
    var id: String { path + "@" + VGTargetUtilities.hex(loadAddress) }
    let loadAddress: UInt64
    let slide: Int64
    let preferredAddress: UInt64
    let path: String

    var basename: String { URL(fileURLWithPath: path).lastPathComponent }
}

struct VGRuntimeCorrelation: Identifiable, Hashable {
    var id: Int { sampleID }
    let sampleID: Int
    let runtimePC: UInt64
    let preferredPC: UInt64?
    let modulePath: String?
    let slide: Int64?
    let functionName: String?
    let confidence: String
    let explanation: String

    var runtimeText: String { VGTargetUtilities.hex(runtimePC) }
    var preferredText: String { preferredPC.map(VGTargetUtilities.hex) ?? "unresolved" }
}

enum VGRuntimeCorrelationEngine {
    static func modules(_ text: String) -> [VGRuntimeModule] {
        VGTargetUtilities.nativeModuleRows(text).map {
            VGRuntimeModule(loadAddress: $0.load, slide: $0.slide, preferredAddress: $0.preferred, path: $0.path)
        }
    }

    static func moduleForBinary(_ binaryPath: String, modulesText: String) -> VGRuntimeModule? {
        let modules = modules(modulesText)
        guard !modules.isEmpty else { return nil }
        let canonical = URL(fileURLWithPath: binaryPath).standardizedFileURL.path
        let exact = modules.filter { URL(fileURLWithPath: $0.path).standardizedFileURL.path == canonical }
        if exact.count == 1 { return exact[0] }

        let basename = URL(fileURLWithPath: binaryPath).lastPathComponent
        let named = modules.filter { $0.basename == basename }
        return named.count == 1 ? named[0] : nil
    }

    static func preferredAddress(runtime: UInt64, slide: Int64) -> UInt64? {
        if slide >= 0 {
            let magnitude = UInt64(slide)
            return runtime >= magnitude ? runtime - magnitude : nil
        }
        let magnitude = slide == Int64.min ? UInt64(Int64.max) + 1 : UInt64(-slide)
        let result = runtime.addingReportingOverflow(magnitude)
        return result.overflow ? nil : result.partialValue
    }

    static func correlate(
        _ sample: VGRuntimeSample,
        binaryPath: String,
        modulesText: String,
        functions: [VGFunctionRecord]
    ) -> VGRuntimeCorrelation {
        guard let module = moduleForBinary(binaryPath, modulesText: modulesText) else {
            return VGRuntimeCorrelation(
                sampleID: sample.id,
                runtimePC: sample.pc,
                preferredPC: nil,
                modulePath: nil,
                slide: nil,
                functionName: nil,
                confidence: "unresolved",
                explanation: "No unique loaded-module match for the analyzed binary; no static address was invented."
            )
        }
        guard let preferred = preferredAddress(runtime: sample.pc, slide: module.slide) else {
            return VGRuntimeCorrelation(
                sampleID: sample.id,
                runtimePC: sample.pc,
                preferredPC: nil,
                modulePath: module.path,
                slide: module.slide,
                functionName: nil,
                confidence: "unresolved",
                explanation: "The observed dyld slide could not be applied without integer overflow."
            )
        }
        let function = functions.first(where: { $0.contains(preferred) })
        let explanation: String
        if let function {
            explanation = "Runtime PC translated with the uniquely matched loaded image and its observed dyld slide, then matched to validated static function range \(function.name)."
        } else {
            explanation = "Runtime PC translated with the uniquely matched loaded image and its observed dyld slide, but no validated static function range contains the resulting address; address correlation only."
        }
        return VGRuntimeCorrelation(
            sampleID: sample.id,
            runtimePC: sample.pc,
            preferredPC: preferred,
            modulePath: module.path,
            slide: module.slide,
            functionName: function?.name,
            confidence: "correlated",
            explanation: explanation
        )
    }

    static func heat(
        samples: [VGRuntimeSample],
        binaryPath: String,
        modulesText: String
    ) -> [UInt64: Int] {
        guard let module = moduleForBinary(binaryPath, modulesText: modulesText) else { return [:] }
        var counts: [UInt64: Int] = [:]
        for sample in samples {
            guard let preferred = preferredAddress(runtime: sample.pc, slide: module.slide) else { continue }
            counts[preferred, default: 0] += 1
        }
        return counts
    }

    static func memoryObservation(sequence: Int, memory: DebugMemory, timestamp: TimeInterval = Date().timeIntervalSince1970) -> VGRuntimeMemoryObservation? {
        guard let address = VGTargetUtilities.parseHex(memory.address) else { return nil }
        let raw = memory.hex.filter { !$0.isWhitespace }
        guard raw.count % 2 == 0, raw.count <= 8192 else { return nil }
        var bytes: [UInt8] = []
        bytes.reserveCapacity(raw.count / 2)
        var index = raw.startIndex
        while index < raw.endIndex {
            let next = raw.index(index, offsetBy: 2)
            guard let byte = UInt8(raw[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        guard bytes.count == memory.length else { return nil }
        return VGRuntimeMemoryObservation(id: sequence, timestamp: timestamp, address: address, bytes: bytes)
    }
}
