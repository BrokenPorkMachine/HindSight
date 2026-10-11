import Foundation

enum VGHostArchitecture: String, Codable, CaseIterable {
    case arm64
    case x86_64
    case unknown
}

enum VGHostPlatform {
    static var architecture: VGHostArchitecture {
        #if arch(arm64)
        return .arm64
        #elseif arch(x86_64)
        return .x86_64
        #else
        return .unknown
        #endif
    }

    /// True only when this process is executing as an arm64 process. This is
    /// intentionally process-architecture based: an x86_64 slice running under
    /// Rosetta must not attempt Apple-Silicon-only Hypervisor/debugger paths.
    static var isAppleSiliconProcess: Bool { architecture == .arm64 }
    static var isIntelProcess: Bool { architecture == .x86_64 }

    /// HindSight's current Hypervisor backend uses the Apple-Silicon AArch64
    /// Hypervisor.framework API and guest register model.
    static var supportsAppleSiliconHypervisor: Bool { isAppleSiliconProcess }

    /// The native debugger currently decodes ARM_THREAD_STATE64 and installs
    /// AArch64 BRK instructions. Static ARM64 analysis remains cross-host.
    static var supportsArm64NativeDebugger: Bool { isAppleSiliconProcess }

    /// VectorEmu is a software emulator and is intentionally cross-host.
    static var supportsVectorEmu: Bool { architecture != .unknown }

    /// Keep architecture support separate from runtime authorization. The
    /// latter is only knowable when the host service attempts the operation.
    static var nativeDebuggerStatus: String {
        supportsArm64NativeDebugger
            ? "supported by this process; macOS debugging permission is not probed until attach"
            : "unsupported: requires an arm64 process on Apple Silicon"
    }

    static var hypervisorStatus: String {
        supportsAppleSiliconHypervisor
            ? "supported by this process; Hypervisor.framework entitlement/runtime availability is not probed until start"
            : "unsupported: requires an arm64 process on Apple Silicon"
    }

    static var vectorEmuStatus: String {
        supportsVectorEmu ? "available: software emulation" : "unavailable: host architecture is unknown"
    }

    static var displayName: String {
        switch architecture {
        case .arm64: return "Apple Silicon (arm64)"
        case .x86_64: return "Intel Mac (x86_64)"
        case .unknown: return "Unknown macOS architecture"
        }
    }

    static func unavailableReason(for feature: String) -> String {
        "\(feature) is unavailable in the \(architecture.rawValue) HindSight process because this host-dependent path requires an arm64 process on Apple Silicon. Static ARM64 analysis, projects, evidence, scripting, plugins, firmware tooling, and VectorEmu remain available."
    }
}
