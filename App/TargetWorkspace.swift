import AppKit
import Foundation

extension WorkbenchModel {
    var unifiedTargets: [VGTargetIdentity] {
        var targets: [VGTargetIdentity] = []

        if let record = selected {
            var spaces: [VGAddressSpace] = [
                VGAddressSpace(
                    id: "static-file:\(record.id)",
                    name: "Captured file",
                    kind: "file-offset",
                    base: "0x0000000000000000",
                    size: UInt64(record.result.size).map(VGTargetUtilities.hex),
                    source: "captured source"
                )
            ]
            if let page = currentPage,
               let address = UInt64(page.section_address),
               let offset = UInt64(page.section_offset),
               let words = UInt64(page.total_words) {
                spaces.append(VGAddressSpace(
                    id: "static-text:\(record.id)",
                    name: "__TEXT.__text",
                    kind: "preferred-vm",
                    base: VGTargetUtilities.hex(address),
                    size: VGTargetUtilities.hex(words.saturatingMultiply(by: 4)),
                    source: "validated Mach-O section mapping @ file \(VGTargetUtilities.hex(offset))"
                ))
            }
            let source = record.result.source_sha256.map { "sha256:\($0)" }
            let targetFingerprint = source ?? VGTargetUtilities.fingerprint([record.path, record.name, record.result.size])
            targets.append(VGTargetIdentity(
                id: "static:\(record.id)",
                name: record.name,
                kind: .staticArtifact,
                architecture: record.result.architecture,
                backend: "HindSight Static Analyzer",
                state: "loaded",
                capabilities: [.readBytes, .decode, .analyze, .symbols, .mappings, .addressTranslation],
                addressSpaces: spaces,
                session: VGSessionHeader(
                    schemaVersion: 2,
                    productVersion: VGProduct.version,
                    targetID: "static:\(record.id)",
                    targetFingerprint: targetFingerprint,
                    engineFingerprint: VGTargetUtilities.buildFingerprint("reworker"),
                    modelFingerprint: VGTargetUtilities.fingerprint([record.result.file_kind, record.result.architecture]),
                    sourceFingerprint: source,
                    traceFingerprint: nil
                )
            ))
        }

        if debugAttached, let snapshot = debugSnapshot {
            let targetID = "native:\(snapshot.pid)"
            targets.append(VGTargetIdentity(
                id: targetID,
                name: "PID \(snapshot.pid)",
                kind: .nativeProcess,
                architecture: selected?.result.architecture ?? "arm64",
                backend: "Coordinator Mach Debugger",
                state: snapshot.stopped ? "stopped" : "running",
                capabilities: [.readBytes, .decode, .mappings, .registers, .executionControl, .breakpoints, .trace, .addressTranslation],
                addressSpaces: [VGAddressSpace(id: "native-va:\(snapshot.pid)", name: "Process virtual memory", kind: "runtime-vm", base: nil, size: nil, source: "Mach VM map")],
                session: VGSessionHeader(
                    schemaVersion: 2,
                    productVersion: VGProduct.version,
                    targetID: targetID,
                    targetFingerprint: VGTargetUtilities.fingerprint(["pid", String(snapshot.pid), debugLaunchPath]),
                    engineFingerprint: VGTargetUtilities.buildFingerprint("CoordinatorService"),
                    modelFingerprint: VGTargetUtilities.fingerprint([selected?.result.architecture ?? "arm64", "native-process"]),
                    sourceFingerprint: selected?.result.source_sha256.map { "sha256:\($0)" },
                    traceFingerprint: nil
                )
            ))
        }

        if vectorEmuRunning || !vectorEmuROM.isEmpty {
            let targetID = "vectoremu:\(vectorEmuSoC):\(vectorEmuPort)"
            let state = vectorEmuRunning ? (vectorEmuAttached ? "attached/paused-or-running" : "running") : "configured"
            targets.append(VGTargetIdentity(
                id: targetID,
                name: "VectorEmu \(vectorEmuSoC)",
                kind: .emulatedMachine,
                architecture: "aarch64",
                backend: "VectorEmu",
                state: state,
                capabilities: [.readBytes, .decode, .mappings, .registers, .executionControl, .breakpoints, .trace, .deterministicReset, .addressTranslation],
                addressSpaces: vectorEmuAddressSpaces(),
                session: VGSessionHeader(
                    schemaVersion: 2,
                    productVersion: VGProduct.version,
                    targetID: targetID,
                    targetFingerprint: VGTargetUtilities.fingerprint([vectorEmuROM, vectorEmuSoC, String(vectorEmuPort)]),
                    engineFingerprint: VGTargetUtilities.buildFingerprint("vg-emu-server"),
                    modelFingerprint: VGTargetUtilities.fingerprint(["soc", vectorEmuSoC]),
                    sourceFingerprint: nil,
                    traceFingerprint: vectorEmuTrace.isEmpty ? nil : VGTargetUtilities.fingerprint(["vector-trace-v1", targetID, vectorEmuTrace])
                )
            ))
        }

        if let state = virtualizationState {
            let targetID = "hypervisor:\(state.profileID ?? virtualizationProfileID)"
            let profile = virtualizationProfiles.first { $0.id == (state.profileID ?? virtualizationProfileID) }
            let spaces = profile?.regions.map {
                VGAddressSpace(id: "hv:\($0.id)", name: $0.name, kind: "guest-physical/\($0.kind)", base: $0.base, size: $0.size, source: "virtualization profile \(profile?.id ?? virtualizationProfileID)")
            } ?? [VGAddressSpace(id: "hv:guest", name: "Guest physical memory", kind: "guest-physical", base: nil, size: nil, source: "Hypervisor.framework backend")]
            targets.append(VGTargetIdentity(
                id: targetID,
                name: profile?.name ?? "Hypervisor Guest",
                kind: .hypervisorGuest,
                architecture: "arm64",
                backend: state.cpuBackend,
                state: state.running ? "running" : "stopped",
                capabilities: [.readBytes, .decode, .mappings, .registers, .executionControl, .breakpoints, .trace, .snapshots, .addressTranslation],
                addressSpaces: spaces,
                session: VGSessionHeader(
                    schemaVersion: 2,
                    productVersion: VGProduct.version,
                    targetID: targetID,
                    targetFingerprint: VGTargetUtilities.fingerprint([targetID, String(state.ramMiB), state.cpuBackend, state.deviceBackend, virtualizationImagePath]),
                    engineFingerprint: VGTargetUtilities.buildFingerprint("CoordinatorService"),
                    modelFingerprint: VGTargetUtilities.fingerprint([profile?.id ?? virtualizationProfileID, profile?.regions.map { "\($0.base):\($0.size):\($0.kind)" }.joined(separator: "|") ?? ""]),
                    sourceFingerprint: nil,
                    traceFingerprint: virtualizationRETitle == "VM Trace" && !virtualizationREText.isEmpty
                        ? VGTargetUtilities.fingerprint(["hypervisor-trace-v1", targetID, virtualizationREText]) : nil
                )
            ))
        }
        return targets
    }

    var selectedUnifiedTarget: VGTargetIdentity? {
        unifiedTargets.first { $0.id == selectedTargetID } ?? unifiedTargets.first
    }

    func refreshUnifiedTargets() {
        let targets = unifiedTargets
        if !targets.contains(where: { $0.id == selectedTargetID }) {
            selectedTargetID = targets.first?.id ?? ""
        }
        if debugAttached { debugRefreshModules(); debugRefreshMemoryMap() }
        if vectorEmuAttached { vectorEmuDebug("regions") }
        if virtualizationProfiles.isEmpty { refreshVirtualizationProfiles() }
        targetStatus = targets.isEmpty ? "No active or configured targets." : "\(targets.count) target\(targets.count == 1 ? "" : "s") available. Capabilities are discovered per target."
    }

    func chooseUnifiedTarget(_ id: String) {
        selectedTargetID = id
        targetPivot = nil
        if let target = unifiedTargets.first(where: { $0.id == id }) {
            targetStatus = "Selected \(target.name) via \(target.backend)."
        }
    }

    func openSelectedTargetPrimaryView() {
        guard let target = selectedUnifiedTarget else { return }
        switch target.kind {
        case .staticArtifact: workspace = "Navigator"
        case .nativeProcess: workspace = "Debugger"
        case .emulatedMachine: workspace = "VectorEmu Lab"
        case .hypervisorGuest: workspace = "Hypervisor RE Suite"
        }
    }

    func pivotUnifiedAddress() {
        guard let target = selectedUnifiedTarget else {
            targetStatus = "Select a target first."
            targetPivot = nil
            return
        }
        guard let address = VGTargetUtilities.parseHex(targetPivotInput) else {
            targetStatus = "Enter a hexadecimal address such as 0x100000000."
            targetPivot = nil
            return
        }

        var translations: [VGAddressTranslation] = []
        switch target.kind {
        case .staticArtifact:
            translations = staticTranslations(address)
        case .nativeProcess:
            translations = nativeTranslations(address)
        case .emulatedMachine:
            translations = mappedIdentityTranslations(address, spaces: target.addressSpaces, label: "VectorEmu bus")
        case .hypervisorGuest:
            translations = mappedIdentityTranslations(address, spaces: target.addressSpaces, label: "guest physical")
        }

        if translations.isEmpty {
            targetPivot = VGAddressPivot(targetID: target.id, input: VGTargetUtilities.hex(address), translations: [], confidence: "unresolved", explanation: "HindSight could not prove a mapping for this address from the evidence currently loaded. It deliberately does not guess.")
            targetStatus = "No provable mapping for \(VGTargetUtilities.hex(address))."
        } else {
            let confidence = target.kind == .nativeProcess ? "correlated" : "proven"
            let explanation = target.kind == .nativeProcess
                ? "The static/runtime join uses a matching loaded-image basename plus its measured dyld slide and the validated static section mapping. The running image is correlated by loaded-image basename, measured dyld slide, and validated static mapping; because runtime memory is not content-hashed, this is explicitly correlation rather than cryptographic identity."
                : "Every row below is derived from a loaded section or declared machine region. No heuristic address guesses are included."
            targetPivot = VGAddressPivot(targetID: target.id, input: VGTargetUtilities.hex(address), translations: translations, confidence: confidence, explanation: explanation)
            targetStatus = "Resolved \(translations.count) evidence-backed address representation\(translations.count == 1 ? "" : "s")."
        }
    }

    func copyTargetSessionJSON() {
        guard let data = targetSessionData(), let text = String(data: data, encoding: .utf8) else { targetStatus = "Nothing to copy."; return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        targetStatus = "Copied target session JSON."
    }

    func exportTargetSessionJSON() {
        guard let data = targetSessionData(), let target = selectedUnifiedTarget else { targetStatus = "Select a target first."; return }
        let panel = NSSavePanel()
        panel.title = "Export HindSight Target Session"
        panel.nameFieldStringValue = "\(target.name.replacingOccurrences(of: " ", with: "-"))-session.json"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try data.write(to: url, options: .atomic)
            targetStatus = "Exported reproducible session header to \(url.lastPathComponent)."
        } catch {
            targetStatus = "Session export failed: \(error.localizedDescription)"
        }
    }

    private func targetSessionData() -> Data? {
        guard let target = selectedUnifiedTarget else { return nil }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return try? encoder.encode(VGTargetSessionExport(target: target, pivot: targetPivot))
    }

    private func staticTranslations(_ address: UInt64) -> [VGAddressTranslation] {
        guard let page = currentPage,
              let vm = UInt64(page.section_address),
              let file = UInt64(page.section_offset),
              let words = UInt64(page.total_words) else { return [] }
        let bytes = words.saturatingMultiply(by: 4)
        var rows: [VGAddressTranslation] = []
        if address >= vm, address - vm < bytes, let mapped = VGTargetUtilities.add(file, address - vm) {
            rows.append(.init(space: "preferred-vm", address: VGTargetUtilities.hex(address), detail: "__TEXT.__text + \(VGTargetUtilities.hex(address - vm))", evidence: "validated Mach-O section mapping"))
            rows.append(.init(space: "file-offset", address: VGTargetUtilities.hex(mapped), detail: "captured file offset", evidence: "section file offset + VM delta"))
        }
        if address >= file, address - file < bytes, let mapped = VGTargetUtilities.add(vm, address - file) {
            rows.append(.init(space: "file-offset", address: VGTargetUtilities.hex(address), detail: "captured file offset", evidence: "validated Mach-O section mapping"))
            rows.append(.init(space: "preferred-vm", address: VGTargetUtilities.hex(mapped), detail: "__TEXT.__text + \(VGTargetUtilities.hex(address - file))", evidence: "section VM address + file delta"))
        }
        return VGTargetUtilities.unique(rows)
    }

    private func nativeTranslations(_ address: UInt64) -> [VGAddressTranslation] {
        guard let record = selected,
              let page = currentPage,
              let vm = UInt64(page.section_address),
              let file = UInt64(page.section_offset),
              let words = UInt64(page.total_words) else { return [] }
        let rows = VGTargetUtilities.nativeModuleRows(debugModules)
        let module = rows.first { row in
            let moduleName = URL(fileURLWithPath: row.path).lastPathComponent
            return moduleName == record.name || row.path.hasSuffix("/\(record.name)")
        }
        guard let module else { return [] }
        let bytes = words.saturatingMultiply(by: 4)
        guard let runtimeBase = VGTargetUtilities.applying(slide: module.slide, to: vm) else { return [] }
        var result: [VGAddressTranslation] = []
        if address >= runtimeBase, address - runtimeBase < bytes {
            let delta = address - runtimeBase
            result.append(.init(space: "runtime-vm", address: VGTargetUtilities.hex(address), detail: "\(record.name) + \(VGTargetUtilities.hex(delta))", evidence: "matching dyld module basename + measured slide + validated static section"))
            if let preferred = VGTargetUtilities.add(vm, delta) {
                result.append(.init(space: "preferred-vm", address: VGTargetUtilities.hex(preferred), detail: "unslid Mach-O address", evidence: "correlated runtime address − measured dyld slide"))
            }
            if let fileOffset = VGTargetUtilities.add(file, delta) {
                result.append(.init(space: "file-offset", address: VGTargetUtilities.hex(fileOffset), detail: "captured file offset", evidence: "validated __text file mapping"))
            }
        } else if address >= vm, address - vm < bytes {
            let delta = address - vm
            result.append(.init(space: "preferred-vm", address: VGTargetUtilities.hex(address), detail: "unslid Mach-O address", evidence: "validated __text mapping"))
            if let runtime = VGTargetUtilities.applying(slide: module.slide, to: address) {
                result.append(.init(space: "runtime-vm", address: VGTargetUtilities.hex(runtime), detail: "\(record.name) + \(VGTargetUtilities.hex(delta))", evidence: "correlated preferred VM + measured dyld slide"))
            }
            if let fileOffset = VGTargetUtilities.add(file, delta) {
                result.append(.init(space: "file-offset", address: VGTargetUtilities.hex(fileOffset), detail: "captured file offset", evidence: "validated __text file mapping"))
            }
        }
        return VGTargetUtilities.unique(result)
    }

    private func mappedIdentityTranslations(_ address: UInt64, spaces: [VGAddressSpace], label: String) -> [VGAddressTranslation] {
        for space in spaces {
            guard let baseText = space.base,
                  let sizeText = space.size,
                  let base = VGTargetUtilities.parseHex(baseText),
                  let size = VGTargetUtilities.parseHex(sizeText),
                  address >= base,
                  address - base < size else { continue }
            return [VGAddressTranslation(space: space.kind, address: VGTargetUtilities.hex(address), detail: "\(space.name) + \(VGTargetUtilities.hex(address - base))", evidence: "declared \(label) region from \(space.source)")]
        }
        return []
    }

    private func vectorEmuAddressSpaces() -> [VGAddressSpace] {
        guard let data = vectorEmuRegions.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let result = root["result"] as? [String: Any],
              let regions = result["regions"] as? [[String: Any]] else {
            return [VGAddressSpace(id: "vectoremu:bus", name: "Machine bus", kind: "emulated-physical", base: nil, size: nil, source: "VectorEmu /regions (refresh required)")]
        }
        return regions.enumerated().compactMap { index, region in
            guard let base = region["base"] as? String, let size = region["size"] as? String else { return nil }
            let device = region["device"] as? String ?? "region \(index)"
            return VGAddressSpace(id: "vectoremu:\(index):\(base)", name: device, kind: "emulated-physical", base: base, size: size, source: "VectorEmu /regions")
        }
    }
}

private extension UInt64 {
    func saturatingMultiply(by rhs: UInt64) -> UInt64 {
        let result = multipliedReportingOverflow(by: rhs)
        return result.overflow ? UInt64.max : result.partialValue
    }
}
