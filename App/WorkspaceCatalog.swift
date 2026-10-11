import SwiftUI

enum VGWorkspaceGroup: String, CaseIterable, Identifiable {
    case start = "Start"
    case staticAnalysis = "Static Analysis"
    case runtime = "Dynamic Analysis"
    case machine = "Machine Lab"
    case automation = "Automation"
    case evidence = "Evidence & Notes"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .start: return "square.grid.2x2"
        case .staticAnalysis: return "waveform.path.ecg.rectangle"
        case .runtime: return "bolt.horizontal.circle"
        case .machine: return "cpu"
        case .automation: return "terminal"
        case .evidence: return "checkmark.seal"
        }
    }
}

struct VGWorkspaceDescriptor: Identifiable, Hashable {
    let name: String
    let title: String
    let icon: String
    let group: VGWorkspaceGroup
    let subtitle: String
    let keywords: [String]
    let sidebarPriority: Int?

    var id: String { name }
    var appearsInSidebar: Bool { sidebarPriority != nil }

    /// Short, user-facing guidance shown before a workspace is opened. Keep
    /// capability boundaries explicit so advanced tools are discoverable on
    /// every host without implying that a disabled path is broken.
    var helpText: String {
        switch name {
        case "Debugger":
            return "Native ARM64 attach/launch; requires an arm64 process on Apple Silicon and may require macOS debugging permission."
        case "SoC Virtualization", "Hypervisor RE Suite", "Guest Memory", "Guest Disassembly", "Firmware Loader", "VM Snapshots", "Guest Search", "vCPU Monitor", "VM Breakpoints", "VM Trace", "GPA Map", "DART Inspector", "Interrupt Trace":
            return "Apple-Silicon Hypervisor.framework workflow; entitlement and runtime availability are checked when the service starts."
        case "VectorEmu Lab", "VectorEmu Registers", "VectorEmu Code", "VectorEmu Trace", "Full-System iPhemeral":
            return "Bounded software emulation; useful as the cross-host fallback for runtime-oriented inspection."
        default:
            return "Evidence stays bounded and provenance-labelled; use the Inspector and handoff tools to preserve analyst context."
        }
    }
}

enum VGWorkspaceCatalog {
    static let descriptors: [VGWorkspaceDescriptor] = {
        var result: [VGWorkspaceDescriptor] = []
        func add(_ group: VGWorkspaceGroup, _ names: [String], sidebar: [String], subtitle: String, icon: String, overrides: [String: (String, String)] = [:]) {
            for (index, name) in names.enumerated() {
                let override = overrides[name]
                result.append(VGWorkspaceDescriptor(
                    name: name,
                    title: override?.0 ?? name,
                    icon: override?.1 ?? icon,
                    group: group,
                    subtitle: subtitle,
                    keywords: [group.rawValue, subtitle] + name.split(separator: " ").map(String.init),
                    sidebarPriority: sidebar.firstIndex(of: name).map { $0 + (index * 0) }
                ))
            }
        }

        add(.start,
            ["Overview", "Targets", "Selected Symbol"],
            sidebar: ["Overview", "Targets", "Selected Symbol"],
            subtitle: "Orient, select a target, and follow the current investigation.",
            icon: "scope",
            overrides: [
                "Overview": ("Investigation Home", "square.grid.2x2"),
                "Targets": ("Targets", "scope"),
                "Selected Symbol": ("Selected Symbol", "scope")
            ])

        add(.staticAnalysis,
            ["Binary Workspace", "Binary Diff", "Analysis Triage", "Analysis Insights", "Analysis Quality", "Navigator", "Disassembly", "Annotated Code", "Functions", "Function Intelligence", "Function Neighborhoods", "Tail-Call Candidates", "CFG", "Dominators", "Dominance Frontiers", "Phi Nodes", "IR", "SSA", "SSA Rename", "Strings", "Xrefs", "Data References", "Relocations", "Jump Tables", "Call Graph", "Data Flow", "Metadata", "Runtime Records", "Runtime Graph", "ObjC Classes", "Swift Types", "Swift Contexts", "Selector Xrefs", "Call Values", "Chained Fixups", "Chained Starts", "ObjC Protocols", "Swift Fields", "Swift Field Records", "Inter-block Values", "Predecessor Values", "Chained Pointers", "Multi-Start Chains", "ARM64e Pointers", "Swift Ownership", "Propagated Values", "Indirect Calls", "Dyld Imports", "Dyld Linking", "Soft Linking", "Symbolication", "Symbol Resolver", "Apple Symbolication", "Dylib Ordinals", "Resolved Imports", "ARM64e Bind Symbols", "Authenticated Targets", "Indirect Symbols", "Pointer Slots", "Exports", "Symbol Provenance", "Linkage Records", "Symbol Callers", "GOT/Auth-GOT", "Import/Export Collisions"],
            sidebar: ["Binary Workspace", "Binary Diff", "Analysis Triage", "Analysis Insights", "Analysis Quality", "Navigator", "Disassembly", "Functions", "Function Neighborhoods", "CFG", "Strings", "Xrefs", "Runtime Graph", "Symbol Resolver", "Symbol Provenance"],
            subtitle: "Understand code, control flow, data, symbols, and Apple runtime evidence.",
            icon: "doc.text.magnifyingglass",
            overrides: [
                "Binary Workspace": ("Binary Workspace", "rectangle.3.group"),
                "Binary Diff": ("Binary Diff", "arrow.left.arrow.right.square"),
                "Analysis Triage": ("Analysis Triage", "scope"),
                "Analysis Insights": ("Analysis Insights", "sparkles.rectangle.stack"),
                "Analysis Quality": ("Analysis Quality", "checkmark.seal.text.page"),
                "Navigator": ("Search & Navigate", "magnifyingglass"),
                "Disassembly": ("Disassembly", "text.alignleft"),
                "Annotated Code": ("Annotated Code", "text.badge.checkmark"),
                "Functions": ("Functions", "function"),
                "Function Intelligence": ("Function Intelligence", "chart.bar.xaxis"),
                "Function Neighborhoods": ("Function Neighborhoods", "point.3.connected.trianglepath.dotted"),
                "Tail-Call Candidates": ("Tail-Call Candidates", "arrowshape.turn.up.right"),
                "CFG": ("Flow Graph", "point.3.connected.trianglepath.dotted"),
                "Strings": ("Strings", "text.quote"),
                "Xrefs": ("Cross References", "arrow.triangle.branch"),
                "Data References": ("Data References", "arrow.turn.down.right"),
                "Runtime Graph": ("Runtime Graph", "circle.hexagongrid"),
                "Symbol Resolver": ("Symbol Resolver", "scope"),
                "Symbol Provenance": ("Symbol Provenance", "checkmark.seal")
            ])

        add(.runtime,
            ["Trace Explorer", "Runtime Correlation", "Debugger", "Threads", "Memory Map", "Modules", "Backtrace", "Live Disassembly", "Stop Context"],
            sidebar: ["Trace Explorer", "Runtime Correlation", "Debugger", "Live Disassembly", "Threads", "Memory Map", "Modules", "Stop Context"],
            subtitle: "Inspect a live process, execution state, memory, modules, and stops.",
            icon: "ladybug",
            overrides: [
                "Trace Explorer": ("Trace Explorer", "point.topleft.down.to.point.bottomright.curvepath"),
                "Runtime Correlation": ("Runtime Correlation", "point.3.filled.connected.trianglepath.dotted"),
                "Debugger": ("Debugger", "ladybug"),
                "Live Disassembly": ("Live Disassembly", "bolt.horizontal"),
                "Threads": ("Threads", "person.3"),
                "Memory Map": ("Memory Map", "square.stack.3d.up"),
                "Modules": ("Loaded Images", "shippingbox"),
                "Stop Context": ("Stop Context", "pause.circle")
            ])

        add(.machine,
            ["SoC Virtualization", "Firmware Runtime Lab", "dyld Cache Browser", "IPSW Catalog", "Firmware Keys", "VectorEmu Lab", "VectorEmu Registers", "VectorEmu Code", "VectorEmu Trace", "Full-System iPhemeral", "Hypervisor RE Suite", "Guest Memory", "Guest Disassembly", "Firmware Loader", "VM Snapshots", "Guest Search", "vCPU Monitor", "VM Breakpoints", "VM Trace", "GPA Map", "DART Inspector", "Interrupt Trace"],
            sidebar: ["VectorEmu Lab", "Hypervisor RE Suite", "Firmware Runtime Lab", "SoC Virtualization", "dyld Cache Browser", "IPSW Catalog"],
            subtitle: "Move from files to machine state, emulation, virtualization, and firmware.",
            icon: "cpu",
            overrides: [
                "VectorEmu Lab": ("VectorEmu", "cpu"),
                "Hypervisor RE Suite": ("Hypervisor · Experimental", "memorychip"),
                "Firmware Runtime Lab": ("Firmware Lab", "externaldrive.badge.gearshape"),
                "SoC Virtualization": ("SoC Virtualization · Experimental", "square.stack.3d.up.badge.a"),
                "dyld Cache Browser": ("dyld Cache", "shippingbox.and.arrow.backward"),
                "IPSW Catalog": ("IPSW Catalog", "archivebox"),
                "Full-System iPhemeral": ("Full-System iPhemeral · Experimental", "memorychip")
            ])

        add(.automation,
            ["Python Console", "Plugins", "Automation SDK", "REA Agent", "Extensions Platform", "Extension Manager"],
            sidebar: ["Python Console", "REA Agent", "Extensions Platform", "Extension Manager"],
            subtitle: "Automate repeatable work and extend the workbench safely.",
            icon: "terminal",
            overrides: [
                "Python Console": ("Python Console", "terminal"),
                "REA Agent": ("REA Agent", "sparkles.rectangle.stack"),
                "Extensions Platform": ("Extensions", "puzzlepiece.extension"),
                "Extension Manager": ("Extension Manager", "checkmark.shield")
            ])

        add(.evidence,
            ["Investigation Ledger", "Evidence Graph", "Notes"],
            sidebar: ["Investigation Ledger", "Evidence Graph", "Notes"],
            subtitle: "Connect machine evidence, correlations, and analyst context without collapsing provenance.",
            icon: "note.text",
            overrides: [
                "Investigation Ledger": ("Investigation Ledger", "book.pages"),
                "Evidence Graph": ("Evidence Graph", "point.3.connected.trianglepath.dotted"),
                "Notes": ("Notes", "note.text")
            ])

        return result
    }()

    static let layouts = ["Explore", "Targets", "Code", "Binary Diff", "Analysis Triage", "Control Flow", "Data", "Deep Dive", "SSA Structure", "Apple Runtime", "Semantic RE", "Semantic Graph", "Runtime Linking", "Type Graph", "Evidence Graph", "Investigation Ledger", "Resolution Graph", "Pointer Auth", "Linker Evidence", "Import Resolution", "Linker Semantics", "Linkage Explorer", "Selected Symbol", "Debug", "Trace Explorer", "Runtime Correlation", "Dynamic Analysis", "Debug Triage", "Scripting", "Plugins", "Automation SDK", "REA Agent", "Extensions Platform", "Extension Manager", "SoC Virtualization", "Hypervisor RE", "Firmware & Runtime", "dyld Cache", "IPSW Lab", "Machine Lab", "Software Emulation", "Notes"]

    static var allNames: [String] { descriptors.map(\.name) }

    static func descriptor(for name: String) -> VGWorkspaceDescriptor {
        descriptors.first(where: { $0.name == name }) ?? VGWorkspaceDescriptor(
            name: name,
            title: name,
            icon: "square.grid.2x2",
            group: .staticAnalysis,
            subtitle: "Reverse-engineering workspace.",
            keywords: [name],
            sidebarPriority: nil
        )
    }

    static func sidebar(in group: VGWorkspaceGroup) -> [VGWorkspaceDescriptor] {
        descriptors
            .filter { $0.group == group && $0.appearsInSidebar }
            .sorted { ($0.sidebarPriority ?? 999) < ($1.sidebarPriority ?? 999) }
    }

    static func search(_ query: String) -> [VGWorkspaceDescriptor] {
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !normalized.isEmpty else {
            return descriptors.sorted { lhs, rhs in
                if lhs.group == rhs.group { return lhs.title < rhs.title }
                let lhsRank = VGWorkspaceGroup.allCases.firstIndex(of: lhs.group) ?? Int.max
                let rhsRank = VGWorkspaceGroup.allCases.firstIndex(of: rhs.group) ?? Int.max
                return lhsRank < rhsRank
            }
        }
        return descriptors.filter { descriptor in
            descriptor.name.lowercased().contains(normalized)
                || descriptor.title.lowercased().contains(normalized)
                || descriptor.subtitle.lowercased().contains(normalized)
                || descriptor.keywords.contains(where: { $0.lowercased().contains(normalized) })
        }.sorted { lhs, rhs in
            let lPrefix = lhs.title.lowercased().hasPrefix(normalized)
            let rPrefix = rhs.title.lowercased().hasPrefix(normalized)
            if lPrefix != rPrefix { return lPrefix }
            return lhs.title < rhs.title
        }
    }
}
