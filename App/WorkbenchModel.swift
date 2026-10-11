import AppKit
import Combine
import Foundation
import UniformTypeIdentifiers

private struct VGAnalysisDecodeResult: Sendable {
    let reply: AnalysisReply?
    let errorMessage: String?

    static func decode(_ text: String) -> VGAnalysisDecodeResult {
        do {
            let data = Data(text.utf8)
            return VGAnalysisDecodeResult(reply: try JSONDecoder().decode(AnalysisReply.self, from: data), errorMessage: nil)
        } catch {
            return VGAnalysisDecodeResult(reply: nil, errorMessage: error.localizedDescription)
        }
    }
}

struct VGRecentProject: Codable, Hashable, Identifiable {
    let path: String
    let lastOpened: Date
    var id: String { path }
    var displayName: String { URL(fileURLWithPath: path, isDirectory: true).lastPathComponent }
}
@MainActor
final class WorkbenchModel: ObservableObject {
    @Published var project: ProjectSnapshot?
    @Published var selectedID: String? {
        didSet {
            if oldValue != selectedID {
                selectedEvidenceAnnotation = nil
                annotationDraft = ""
                annotationTagsText = ""
                annotationConfidence = .unclassified
                instructionPage = nil; pageStarts.removeAll(); traversingPreviousPage = false; analysisReport = nil; pageRecordID = nil; analysisRecordID = nil; evidenceIndexTask?.cancel(); analysisEvidenceIndex = nil; evidenceIndexRecordID = nil; analysisEvidenceIndexing = false; analysisEvidenceStatus = "Evidence index unavailable."; navigationGeneration = UUID(); resetWorkspaceHistory(); analysisHistoryCursor = -1; refreshAnalysisContinuityProjection()
                if !suppressLedgerRecording, let selected { recordLedger(kind: .recordSelected, provenance: .observed, detail: "Selected artifact \(selected.name).") }
                persistLaunchSessionIfPossible()
            }
        }
    }
    private var pageStarts: [UInt64] = []
    private var traversingPreviousPage = false
    @Published var instructionPage: InstructionPage?
    @Published var analysisReport: AnalysisReport?
    @Published private(set) var analysisEvidenceIndex: VGAnalysisEvidenceIndex?
    @Published private(set) var analysisEvidenceIndexing = false
    @Published private(set) var analysisEvidenceStatus = "Evidence index unavailable."
    @Published var workspace = "Overview" {
        didSet {
            guard !restoringNavigation, oldValue != workspace else { return }
            workspaceBack.append(navigationState(workspace: oldValue))
            if workspaceBack.count > 64 { workspaceBack.removeFirst(workspaceBack.count - 64) }
            workspaceForward.removeAll()
            if !suppressLedgerRecording {
                recordLedger(kind: .workspaceVisited, provenance: .asserted, detail: "Opened \(workspace).")
            }
            persistLaunchSessionIfPossible()
        }
    }
    @Published var layoutPreset = "Explore" {
        didSet {
            switch layoutPreset {
            case "Targets": workspace = "Targets"
            case "Code": workspace = "Binary Workspace"
            case "Binary Diff": workspace = "Binary Diff"
            case "Analysis Triage": workspace = "Analysis Triage"
            case "Control Flow": workspace = "CFG"
            case "Data": workspace = "Strings"
            case "Deep Dive": workspace = "Navigator"
            case "SSA Structure": workspace = "SSA Rename"
            case "Apple Runtime": workspace = "Runtime Graph"
            case "Semantic RE": workspace = "Navigator"
            case "Semantic Graph": workspace = "ObjC Classes"
            case "Runtime Linking": workspace = "Chained Fixups"
            case "Evidence Graph": workspace = "Evidence Graph"
            case "Investigation Ledger": workspace = "Investigation Ledger"
            case "Resolution Graph": workspace = "Chained Pointers"
            case "Pointer Auth": workspace = "ARM64e Pointers"
            case "Linker Evidence": workspace = "Multi-Start Chains"
            case "Import Resolution": workspace = "Dyld Imports"
            case "Linker Semantics": workspace = "Resolved Imports"
            case "Linkage Explorer": workspace = "Symbol Provenance"
            case "Selected Symbol": workspace = "Selected Symbol"
            case "Debug": workspace = "Debugger"
            case "Trace Explorer": workspace = "Trace Explorer"
            case "Runtime Correlation": workspace = "Runtime Correlation"
            case "Dynamic Analysis": workspace = "Trace Explorer"
            case "Debug Triage": workspace = "Stop Context"
            case "Scripting": workspace = "Python Console"
            case "Plugins": workspace = "Plugins"
            case "Automation SDK": workspace = "Automation SDK"
            case "REA Agent": workspace = "REA Agent"
            case "Extensions Platform": workspace = "Extensions Platform"
            case "Extension Manager": workspace = "Extension Manager"
            case "SoC Virtualization": workspace = "SoC Virtualization"
            case "Hypervisor RE": workspace = "Hypervisor RE Suite"
            case "Firmware & Runtime": workspace = "Firmware Runtime Lab"
            case "dyld Cache": workspace = "dyld Cache Browser"
            case "IPSW Lab": workspace = "IPSW Catalog"
            case "Machine Lab": workspace = "VectorEmu Lab"
            case "Software Emulation": workspace = "VectorEmu Lab"
            case "Type Graph": workspace = "Swift Fields"
            case "Notes": workspace = "Notes"
            default: workspace = "Overview"
            }
            persistLaunchSessionIfPossible()
        }
    }
    @Published var preferredLayoutPreset = "Explore" {
        didSet {
            guard VGWorkspaceCatalog.layouts.contains(preferredLayoutPreset) else { return }
            UserDefaults.standard.set(preferredLayoutPreset, forKey: "HindSight.preferred-layout.v1")
        }
    }
    @Published var selectedTargetID = ""
    @Published var targetPivotInput = ""
    @Published var targetPivot: VGAddressPivot?
    @Published var targetStatus = "Unified target registry ready."
    @Published var addressText = "" { didSet { if oldValue != addressText { persistLaunchSessionIfPossible() } } }
    @Published var analysisSearch = "" { didSet { if oldValue != analysisSearch { persistLaunchSessionIfPossible() } } }
    @Published var cfgFocus = "" { didSet { if oldValue != cfgFocus { persistLaunchSessionIfPossible() } } }
    @Published var inspectorPinned = false
    @Published var selectedEvidence = ""
    @Published var selectedEvidenceAnnotation: VGAnalysisAnnotation?
    @Published var annotationDraft = ""
    @Published var annotationTagsText = ""
    @Published var annotationConfidence: VGAnalysisConfidence = .unclassified
    @Published var selectedSymbol = "" { didSet { if oldValue != selectedSymbol { persistLaunchSessionIfPossible() } } }
    @Published var diffBaselineID = ""
    @Published var diffCandidateID = ""
    @Published var diffStatus = "Choose two captured artifacts, then analyze the pair."
    @Published var analysisReportsByRecordID: [String: AnalysisReport] = [:]
    private var analysisReportLRU: [String] = []
    private static let maxResidentAnalysisReports = 8
    @Published var analysisPins: [VGAnalysisPin] = []
    @Published var analysisRecentVisits: [VGAnalysisVisit] = []
    @Published var analysisCollections: [VGInvestigationCollection] = []
    @Published var analysisReviewBaselines: [String: VGInvestigationReviewBaseline] = [:]
    @Published var analysisReviewEntries: [VGInvestigationReviewEntry] = []
    @Published var analysisFollowUps: [VGInvestigationFollowUp] = []
    @Published var analysisSavedSearches: [VGAnalysisSavedSearch] = []
    @Published var analysisDelta: VGAnalysisDelta?
    @Published var analysisHistoryCursor = -1
    var analysisContinuityDocument = VGAnalysisContinuityDocument.empty
    @Published var debugPIDText = ""
    @Published var debugLaunchPath = ""
    @Published var debugAddress = ""
    @Published var debugMemoryLength = 64
    @Published var debugSnapshot: DebugSnapshot?
    @Published var debugMemory = ""
    @Published var debugEvent = "Debugger idle."
    @Published var debugAttached = false
    @Published var debugThreadID = ""
    @Published var debugThreads = ""
    @Published var debugMemoryMap = ""
    @Published var debugModules = ""
    @Published var debugBacktrace = ""
    @Published var debugDisassembly = ""
    @Published var debugFollowPC = true
    @Published var runtimeSamples: [VGRuntimeSample] = []
    @Published var runtimeMemoryObservations: [VGRuntimeMemoryObservation] = []
    @Published var investigationLedger: VGInvestigationLedger?
    @Published var ledgerStatus = "Ledger opens with the project."
    @Published var selectedLedgerEntryID = ""
    @Published var pythonSource = """
# HindSight Python API v2
from hindsight import api as vg

print("event:", vg.event)
print("workspace:", vg.selection.get("workspace"))
print("symbol:", vg.selection.get("symbol"))
# Example mutation (console has explicit automation capabilities):
# vg.navigate(workspace="Annotated Code", address=vg.selection.get("address"))
result = {"address": vg.selection.get("address"), "capabilities": sorted(vg.capabilities)}
"""
    @Published var pythonOutput = ""
    @Published var pythonError = ""
    @Published var pythonResult = ""
    @Published var pythonInterpreter = ""
    @Published var pythonTimeoutMS = 5000
    @Published var plugins: [PluginInfo] = []
    @Published var selectedPluginID = ""
    @Published var pluginStatus = "Plugins not scanned."
    @Published var pluginSettingDrafts: [String: String] = [:]
    @Published var activePluginPanelTitle = ""
    @Published var activePluginPanelOutput = ""
    @Published var activePluginPanelDock = "inspector"
    @Published var debugLastSignal: Int?
    @Published var debugLastReason = ""
    @Published var virtualizationProfiles: [VirtualizationProfile] = []
    @Published var virtualizationProfileID = "a12-research"
    @Published var virtualizationRAMMiB = 2048
    @Published var virtualizationState: VirtualizationState?
    @Published var virtualizationStatus = "Virtualization backend not queried."
    @Published var virtualizationRETitle = "Hypervisor RE Suite"
    @Published var virtualizationREText = "Start a VM, then use the virtualization-assisted reverse-engineering tools."
    @Published var virtualizationREAddress = "0x0"
    @Published var virtualizationRELength = 256
    @Published var virtualizationREPattern = "d4200000"
    @Published var virtualizationImagePath = ""
    @Published var dyldCachePath = ""
    @Published var dyldCache: DyldCacheSummary?
    @Published var dyldFilter = ""
    @Published var selectedDyldImageID = ""
    @Published var dyldReadAddress = ""
    @Published var dyldReadCount = 512
    @Published var dyldReadText = ""
    @Published var firmwareSoCFilter = "a12"
    @Published var firmwareDevices: [FirmwareDeviceRecord] = []
    @Published var firmwareDeviceID = ""
    @Published var firmwareReleases: [FirmwareReleaseRecord] = []
    @Published var firmwareBuildID = ""
    @Published var firmwareKeys: [FirmwareKeyRecord] = []
    @Published var firmwareStatus = "Firmware/runtime lab ready."
    @Published var firmwareText = ""
    @Published var ipswPath = ""
    @Published var mentorMode = true
    @Published var vectorEmuExecutable = ""
    @Published var vectorEmuROM = ""
    @Published var vectorEmuSoC = "t8020"
    @Published var vectorEmuPort = 37920
    @Published var vectorEmuRunning = false
    @Published var vectorEmuAttached = false
    @Published var vectorEmuAddress = "0x100000000"
    @Published var vectorEmuCount = 16
    @Published var vectorEmuSteps: UInt64 = 10000
    @Published var vectorEmuStatus = "VectorEmu lab ready."
    @Published var vectorEmuRegisters = ""
    @Published var vectorEmuDisassembly = ""
    @Published var vectorEmuTrace = ""
    @Published private(set) var reaStatus = "REA agents not probed."
    @Published var reaTargetPath = ""
    @Published var reaOperation = "binary_overview"
    @Published var reaProvider = "auto"
    @Published var vectorEmuRegions = ""
    @Published var vectorEmuRaw = ""
    @Published var iphemeralExecutable = ""
    @Published var iphemeralROM = ""
    @Published var iphemeralSoC = "t8020"
    @Published var iphemeralRunning = false
    @Published var iphemeralOutput = ""
    private var debugPollTimer: Timer?
    private var analysisMonitorTask: Task<Void, Never>?
    private var evidenceIndexTask: Task<Void, Never>?
    private var evidenceIndexRecordID: String?
    private var launchSessionSaveTask: Task<Void, Never>?
    private var runtimeSequence = 0
    private var runtimeMemorySequence = 0
    private var debugPollInFlight = false
    private var pageRecordID: String?
    private var analysisRecordID: String?
    private var navigationGeneration = UUID()
    private var restoringNavigation = false
    private var suppressLedgerRecording = false
    private struct NavigationState { let workspace: String; let addressText: String; let cfgFocus: String; let analysisSearch: String; let selectedSymbol: String }
    private var workspaceBack: [NavigationState] = []
    private var workspaceForward: [NavigationState] = []
    private func navigationState(workspace override: String? = nil) -> NavigationState { NavigationState(workspace: override ?? workspace, addressText: addressText, cfgFocus: cfgFocus, analysisSearch: analysisSearch, selectedSymbol: selectedSymbol) }
    private func restoreNavigation(_ state: NavigationState) { restoringNavigation = true; workspace = state.workspace; addressText = state.addressText; cfgFocus = state.cfgFocus; analysisSearch = state.analysisSearch; selectedSymbol = state.selectedSymbol; restoringNavigation = false }
    var canNavigateBack: Bool { !workspaceBack.isEmpty }
    var canNavigateForward: Bool { !workspaceForward.isEmpty }
    func navigateBack() {
        guard let target = workspaceBack.popLast() else { return }
        workspaceForward.append(navigationState())
        restoreNavigation(target)
    }
    func navigateForward() {
        guard let target = workspaceForward.popLast() else { return }
        workspaceBack.append(navigationState())
        restoreNavigation(target)
    }
    func resetWorkspaceHistory() {
        workspaceBack.removeAll(); workspaceForward.removeAll()
    }
    var currentPage: InstructionPage? { pageRecordID == selectedID ? instructionPage : nil }
    var currentAnalysis: AnalysisReport? { analysisRecordID == selectedID ? analysisReport : nil }
    var currentEvidenceIndex: VGAnalysisEvidenceIndex? { evidenceIndexRecordID == selectedID ? analysisEvidenceIndex : nil }

    func cacheAnalysisReport(_ report: AnalysisReport, recordID: String) {
        analysisReportsByRecordID[recordID] = report
        analysisReportLRU.removeAll { $0 == recordID }
        analysisReportLRU.append(recordID)
        while analysisReportLRU.count > Self.maxResidentAnalysisReports {
            let evicted = analysisReportLRU.removeFirst()
            if evicted == selectedID {
                analysisReportLRU.append(evicted)
                if analysisReportLRU.count <= Self.maxResidentAnalysisReports { break }
                continue
            }
            analysisReportsByRecordID.removeValue(forKey: evicted)
        }
    }

    func rebuildEvidenceIndex(recordID: String, report: AnalysisReport, workerEvidence: WorkerEvidenceBundle? = nil) {
        evidenceIndexTask?.cancel()
        let sources = VGAnalysisNavigation.sources(report)
        let bundle = workerEvidence?.validated()
        let generation = navigationGeneration
        analysisEvidenceIndexing = true
        analysisEvidenceStatus = bundle == nil ? "Indexing compatibility evidence…" : "Loading worker-native typed evidence…"
        analysisEvidenceIndex = nil
        evidenceIndexRecordID = nil
        evidenceIndexTask = Task { [weak self] in
            let index = await Task.detached(priority: .utility) {
                if let bundle, let native = VGAnalysisEvidenceIndex.build(worker: bundle, compatibilitySources: sources) { return native }
                return VGAnalysisEvidenceIndex.build(from: sources)
            }.value
            guard !Task.isCancelled, let self, self.navigationGeneration == generation, self.selectedID == recordID else { return }
            self.analysisEvidenceIndex = index
            self.evidenceIndexRecordID = recordID
            self.analysisEvidenceIndexing = false
            let origin = bundle == nil ? "compatibility adapter" : "worker-native evidence"
            self.analysisEvidenceStatus = index.truncated
                ? "Evidence index ready via \(origin) (bounded at \(VGAnalysisEvidenceIndex.maxRecords) records)."
                : "Evidence index ready via \(origin) · \(index.records.count) typed records."
        }
    }

    @Published var status = "Create or open a project."
    @Published var capabilities = "Not queried"
    @Published var busy = false
    @Published var connected = false
    @Published var connectingToCoordinator = false
    @Published var restoringProjectSession = false
    @Published var projectSessionReady = false
    @Published var coordinatorVersion = ""
    @Published var coordinatorABI = 0
    @Published var activeRequest: String?
    @Published var analysisJobStatus: InspectionStatus?
    @Published var drafts: [String: String] = [:]
    @Published private(set) var savedLaunchSessionAvailable = false
    @Published private(set) var savedLaunchProjectName = ""
    @Published private(set) var recentProjects: [VGRecentProject] = []

    private struct LaunchSession: Codable {
        let schema: Int
        let projectPath: String
        let selectedRecordID: String?
        let workspace: String
        let layoutPreset: String
        let addressText: String?
        let cfgFocus: String?
        let analysisSearch: String?
        let selectedSymbol: String?
        let updatedAt: Date
    }
    private static let launchSessionDefaultsKey = "HindSight.launch-session.v1"
    private static let recentProjectsDefaultsKey = "HindSight.recent-projects.v1"
    private static let maxRecentProjects = 8
    private static let maxLaunchSessionBytes = 32 * 1024
    private var savedLaunchSession: LaunchSession?
    private var applyingLaunchSession = false
    private let client = CoordinatorClient()
    init() {
        if let saved = UserDefaults.standard.string(forKey: "HindSight.preferred-layout.v1"), VGWorkspaceCatalog.layouts.contains(saved) {
            preferredLayoutPreset = saved
            layoutPreset = saved
        }
        savedLaunchSession = Self.readSavedLaunchSession()
        savedLaunchSessionAvailable = savedLaunchSession != nil
        savedLaunchProjectName = savedLaunchSession.map { URL(fileURLWithPath: $0.projectPath, isDirectory: true).lastPathComponent } ?? ""
        recentProjects = Self.readRecentProjects()
        client.onDisconnect = { [weak self] in
            guard let self else { return }
            self.connected = false
            self.connectingToCoordinator = false
            self.restoringProjectSession = false
            self.projectSessionReady = false
            self.coordinatorVersion = ""
            self.coordinatorABI = 0
            self.busy = false
            self.activeRequest = nil
            self.analysisMonitorTask?.cancel()
            self.analysisMonitorTask = nil
            self.stopDebugPolling()
            self.debugAttached = false
            self.status = self.project == nil
                ? "Coordinator disconnected. Reconnect to continue."
                : "Coordinator disconnected. Reconnect to restore the open project session."
        }
    }
    private static func readSavedLaunchSession() -> LaunchSession? {
        let defaults = UserDefaults.standard
        guard let data = defaults.data(forKey: launchSessionDefaultsKey) else { return nil }
        guard !data.isEmpty, data.count <= maxLaunchSessionBytes,
              let session = try? JSONDecoder().decode(LaunchSession.self, from: data),
              [1, 2].contains(session.schema),
              !session.projectPath.isEmpty, session.projectPath.utf8.count <= 4096, session.projectPath.hasPrefix("/"),
              URL(fileURLWithPath: session.projectPath, isDirectory: true).standardizedFileURL.path == session.projectPath,
              session.workspace.utf8.count <= 128, VGWorkspaceCatalog.allNames.contains(session.workspace),
              session.layoutPreset.utf8.count <= 128, VGWorkspaceCatalog.layouts.contains(session.layoutPreset),
              (session.selectedRecordID?.utf8.count ?? 0) <= 256,
              (session.addressText?.utf8.count ?? 0) <= 128, (session.cfgFocus?.utf8.count ?? 0) <= 128,
              (session.analysisSearch?.utf8.count ?? 0) <= 1024, (session.selectedSymbol?.utf8.count ?? 0) <= 1024 else {
            defaults.removeObject(forKey: launchSessionDefaultsKey)
            return nil
        }
        return session
    }

    private static func readRecentProjects() -> [VGRecentProject] {
        guard let data = UserDefaults.standard.data(forKey: recentProjectsDefaultsKey),
              data.count <= maxLaunchSessionBytes,
              let values = try? JSONDecoder().decode([VGRecentProject].self, from: data) else { return [] }
        return Array(values.filter { value in
            value.path.hasPrefix("/") && value.path.utf8.count <= 4096 &&
            URL(fileURLWithPath: value.path, isDirectory: true).standardizedFileURL.path == value.path
        }.prefix(maxRecentProjects))
    }

    private func rememberRecentProject(_ path: String) {
        let normalized = URL(fileURLWithPath: path, isDirectory: true).standardizedFileURL.path
        guard normalized.hasPrefix("/"), normalized.utf8.count <= 4096 else { return }
        recentProjects.removeAll { $0.path == normalized }
        recentProjects.insert(VGRecentProject(path: normalized, lastOpened: Date()), at: 0)
        recentProjects = Array(recentProjects.prefix(Self.maxRecentProjects))
        guard let data = try? JSONEncoder().encode(recentProjects), data.count <= Self.maxLaunchSessionBytes else { return }
        UserDefaults.standard.set(data, forKey: Self.recentProjectsDefaultsKey)
    }

    private func persistLaunchSessionIfPossible() {
        guard !applyingLaunchSession, project != nil else { return }
        launchSessionSaveTask?.cancel()
        launchSessionSaveTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 350_000_000)
            guard !Task.isCancelled, let self else { return }
            self.writeLaunchSessionNow()
        }
    }

    private func writeLaunchSessionNow() {
        guard !applyingLaunchSession, let project else { return }
        let session = LaunchSession(
            schema: 2,
            projectPath: URL(fileURLWithPath: project.path, isDirectory: true).standardizedFileURL.path,
            selectedRecordID: selectedID,
            workspace: VGWorkspaceCatalog.allNames.contains(workspace) ? workspace : "Overview",
            layoutPreset: VGWorkspaceCatalog.layouts.contains(layoutPreset) ? layoutPreset : "Explore",
            addressText: String(addressText.prefix(128)),
            cfgFocus: String(cfgFocus.prefix(128)),
            analysisSearch: String(analysisSearch.prefix(1024)),
            selectedSymbol: String(selectedSymbol.prefix(1024)),
            updatedAt: Date()
        )
        guard let data = try? JSONEncoder().encode(session), data.count <= Self.maxLaunchSessionBytes else { return }
        UserDefaults.standard.set(data, forKey: Self.launchSessionDefaultsKey)
        savedLaunchSession = session
        savedLaunchSessionAvailable = true
        savedLaunchProjectName = URL(fileURLWithPath: project.path, isDirectory: true).lastPathComponent
    }

    private func applySavedLaunchView(_ session: LaunchSession) {
        applyingLaunchSession = true
        suppressLedgerRecording = true
        restoringNavigation = true
        layoutPreset = VGWorkspaceCatalog.layouts.contains(session.layoutPreset) ? session.layoutPreset : "Explore"
        workspace = VGWorkspaceCatalog.allNames.contains(session.workspace) ? session.workspace : "Overview"
        addressText = session.addressText ?? ""
        cfgFocus = session.cfgFocus ?? ""
        analysisSearch = session.analysisSearch ?? ""
        selectedSymbol = session.selectedSymbol ?? ""
        if let recordID = session.selectedRecordID, project?.records.contains(where: { $0.id == recordID }) == true {
            selectedID = recordID
        }
        restoringNavigation = false
        suppressLedgerRecording = false
        applyingLaunchSession = false
        resetWorkspaceHistory()
        persistLaunchSessionIfPossible()
    }

    private func restoreSavedLaunchSession(_ session: LaunchSession) {
        guard !restoringProjectSession else { return }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: session.projectPath, isDirectory: &isDirectory), isDirectory.boolValue else {
            UserDefaults.standard.removeObject(forKey: Self.launchSessionDefaultsKey)
            savedLaunchSession = nil
            savedLaunchSessionAvailable = false
            savedLaunchProjectName = ""
            projectSessionReady = true
            refreshPlugins()
            status = "Coordinator ready. The saved project no longer exists; choose another project."
            return
        }
        restoringProjectSession = true
        projectSessionReady = false
        status = "Reopening last project…"
        client.request(operation: { $0.openProject(session.projectPath, create: false, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            self.restoringProjectSession = false
            if self.apply(result) {
                self.applySavedLaunchView(session)
                self.projectSessionReady = true
                self.status = "Restored \(self.savedLaunchProjectName.isEmpty ? "the last project" : self.savedLaunchProjectName) from the previous session."
            } else {
                self.projectSessionReady = true
                self.refreshPlugins()
                let detail = self.status
                self.status = "Could not reopen the saved project: \(detail)"
            }
        }
    }

    func reopenSavedLaunchSession() {
        guard !busy, project == nil else { return }
        guard connected, !connectingToCoordinator, !restoringProjectSession else {
            status = "Connect to the Coordinator before reopening the last project."
            return
        }
        guard let session = savedLaunchSession ?? Self.readSavedLaunchSession() else {
            savedLaunchSessionAvailable = false
            savedLaunchProjectName = ""
            status = "No saved launch session is available."
            return
        }
        savedLaunchSession = session
        restoreSavedLaunchSession(session)
    }

    func forgetSavedLaunchSession() {
        guard project == nil else { return }
        UserDefaults.standard.removeObject(forKey: Self.launchSessionDefaultsKey)
        savedLaunchSession = nil
        savedLaunchSessionAvailable = false
        savedLaunchProjectName = ""
        status = "Saved launch session cleared."
    }

    var selected: ProjectRecord? { project?.records.first { $0.id == selectedID } }
    var projectServiceReady: Bool { connected && projectSessionReady }
    var canImportBinary: Bool { projectServiceReady && project != nil && !busy }
    var coordinatorStateLabel: String {
        if restoringProjectSession { return "Restoring project…" }
        if connectingToCoordinator { return "Connecting…" }
        if connected && project != nil && !projectSessionReady { return "Project session unavailable" }
        return connected ? "Coordinator online" : "Coordinator offline"
    }
    var noteText: String {
        get { guard let selected else { return "" }; return drafts[selected.id] ?? selected.note }
        set { guard let selected else { return }; drafts[selected.id] = newValue }
    }
    var noteDirty: Bool { guard let selected else { return false }; return noteText != selected.note }
    private struct CoordinatorHandshake: Decodable {
        let version: String
        let release_channel: String
        let abi: Int
        let rust_bridge: Bool
        let analysis: Bool
        let execution: Bool
    }

    private func validateCoordinatorHandshake(_ value: String) throws -> CoordinatorHandshake {
        let data = Data(value.utf8)
        let handshake = try JSONDecoder().decode(CoordinatorHandshake.self, from: data)
        guard handshake.version == VGProduct.version else {
            throw NSError(domain: "HindSight", code: 20, userInfo: [NSLocalizedDescriptionKey: "App/Coordinator version mismatch (app \(VGProduct.version), service \(handshake.version)). Rebuild or reinstall both components together."])
        }
        guard handshake.release_channel == VGProduct.releaseChannel, handshake.abi == 1 else {
            throw NSError(domain: "HindSight", code: 21, userInfo: [NSLocalizedDescriptionKey: "Coordinator protocol identity is incompatible with this app build."])
        }
        guard handshake.rust_bridge, handshake.analysis, handshake.execution else {
            throw NSError(domain: "HindSight", code: 22, userInfo: [NSLocalizedDescriptionKey: "Coordinator capability handshake is incomplete; analysis execution is unavailable."])
        }
        return handshake
    }

    // Keep launch interactive: defer XPC activation until the first frame has settled.
    // Thermal pressure delays nonessential startup; explicit reconnect remains immediate.
    @Published private(set) var startupPhase = "UI ready"
    @Published private(set) var startupPressure = "normal"
    private var startupPressureSource: DispatchSourceMemoryPressure?
    private var startupPressureUntil: Date = .distantPast
    private var startupTask: Task<Void, Never>?

    private func beginPressureObservation() {
        guard startupPressureSource == nil else { return }
        let source = DispatchSource.makeMemoryPressureSource(eventMask: [.warning, .critical], queue: .main)
        source.setEventHandler { [weak self, weak source] in
            guard let self else { return }
            let critical = source?.data.contains(.critical) == true
            self.startupPressure = critical ? "critical" : "warning"
            self.startupPressureUntil = Date().addingTimeInterval(critical ? 20 : 8)
            self.startupPhase = "Startup paused for memory pressure"
        }
        startupPressureSource = source
        source.resume()
    }

    private var shouldDeferBackgroundStartup: Bool {
        ProcessInfo.processInfo.thermalState == .serious ||
        ProcessInfo.processInfo.thermalState == .critical ||
        Date() < startupPressureUntil
    }
    func scheduleColdStart() {
        guard startupTask == nil, !connected, !connectingToCoordinator else { return }
        beginPressureObservation()
        startupPhase = "UI ready; coordinator pending"
        startupTask = Task { [weak self] in
            defer { self?.startupTask = nil }
            try? await Task.sleep(for: .milliseconds(900))
            guard !Task.isCancelled, let self else { return }
            // Keep the UI available under sustained pressure; do not force XPC startup.
            while self.shouldDeferBackgroundStartup {
                self.startupPhase = "Waiting for memory/thermal headroom"
                try? await Task.sleep(for: .seconds(2))
                guard !Task.isCancelled else { return }
            }
            guard !Task.isCancelled else { return }
            self.startupPressure = "normal"
            self.startupPhase = "Connecting coordinator"
            self.connect()
        }
    }

    func connect(force: Bool = false) {
        guard !connectingToCoordinator, !restoringProjectSession else { return }
        if connected && projectSessionReady && !force { return }
        startupTask?.cancel()
        startupTask = nil
        startupPhase = "Connecting coordinator"
        connectingToCoordinator = true
        projectSessionReady = false
        if project == nil { status = "Connecting to Coordinator…" }
        client.request(operation: { $0.capabilities(withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let value = try result.get()
                let handshake = try self.validateCoordinatorHandshake(value)
                self.connected = true
                self.coordinatorVersion = handshake.version
                self.coordinatorABI = handshake.abi
                self.capabilities = value
                self.connectingToCoordinator = false
                if let path = self.project?.path {
                    self.restoreProjectSession(path: path)
                } else if let session = self.savedLaunchSession {
                    self.restoreSavedLaunchSession(session)
                } else {
                    self.projectSessionReady = true
                    self.startupPhase = "Core ready; optional services on demand"
                    self.status = "Coordinator ready. Create or open a project."
                    // Plugin discovery is demand-driven from its workspace.
                }
            } catch {
                self.connectingToCoordinator = false
                self.restoringProjectSession = false
                self.connected = false
                self.projectSessionReady = false
                self.coordinatorVersion = ""
                self.coordinatorABI = 0
                self.startupPhase = "Coordinator unavailable"
                self.status = "Coordinator unavailable: \(error.localizedDescription)"
            }
        }
    }

    func probeREAAgents() {
        guard !busy, connected else {
            reaStatus = connected ? "Wait for the current operation to finish." : "Connect the Coordinator before probing REA."
            return
        }
        let request = REAProbeRequest(executable: "/usr/bin/env",
                                      arguments: ["npx", "-y", "rea-agents@6.4.0", "mcp"],
                                      timeout_ms: 10_000)
        guard let data = try? JSONEncoder().encode(request), let payload = String(data: data, encoding: .utf8) else {
            reaStatus = "Could not encode the REA probe request."
            return
        }
        reaStatus = "Starting local REA agent…"
        client.request(timeout: 15, maxReplyBytes: 4 * 1024 * 1024, invalidateConnectionOnTimeout: false,
                       operation: { $0.reaProbe(payload, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let response):
                if let data = response.data(using: .utf8),
                   let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                   let message = object["message"] as? String {
                    self.reaStatus = message
                } else {
                    self.reaStatus = response
                }
            case .failure(let error):
                self.reaStatus = "REA probe failed: \(error.localizedDescription)"
            }
        }
    }

    func runREAOperation() {
        guard !busy, connected else {
            reaStatus = connected ? "Wait for the current operation to finish." : "Connect the Coordinator before running REA."
            return
        }
        let request = REAOperationRequest(executable: "/usr/bin/env",
                                          arguments: ["npx", "-y", "rea-agents@6.4.0", "mcp"],
                                          target_path: reaTargetPath,
                                          provider_id: reaProvider,
                                          tool: reaOperation,
                                          timeout_ms: 60_000)
        guard let data = try? JSONEncoder().encode(request), let payload = String(data: data, encoding: .utf8) else {
            reaStatus = "Could not encode the REA operation request."
            return
        }
        reaStatus = "Running REA (reaOperation)…"
        client.request(timeout: 65, maxReplyBytes: 4 * 1024 * 1024, invalidateConnectionOnTimeout: false,
                       operation: { $0.reaAnalyze(payload, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let response): self.reaStatus = response
            case .failure(let error): self.reaStatus = "REA operation failed: \(error.localizedDescription)"
            }
        }
    }

    func reconnect() {
        guard !busy else { status = "Finish or cancel the active operation before reconnecting the Coordinator."; return }
        client.resetConnection()
        connected = false
        connectingToCoordinator = false
        restoringProjectSession = false
        projectSessionReady = false
        connect(force: true)
    }

    func retryProjectRestore() {
        guard !busy else { status = "Finish or cancel the active operation before restoring the project session."; return }
        guard connected, let path = project?.path else { reconnect(); return }
        guard !restoringProjectSession else { return }
        restoreProjectSession(path: path)
    }

    private func restoreProjectSession(path: String) {
        restoringProjectSession = true
        projectSessionReady = false
        status = "Coordinator online. Restoring project session…"
        client.request(operation: { $0.openProject(path, create: false, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            self.restoringProjectSession = false
            if self.apply(result) {
                self.projectSessionReady = true
                self.status = "Coordinator recovered and project session restored."
            } else {
                self.projectSessionReady = false
                let detail = self.status
                self.status = "Coordinator online, but project restore failed: \(detail)"
            }
        }
    }

    private func discardDraftsIfNeeded() -> Bool {
        guard drafts.contains(where: { id, text in project?.records.first(where: {$0.id == id})?.note != text }) else { return true }
        let alert = NSAlert(); alert.messageText = "Discard unsaved notes?"
        alert.informativeText = "Save the notes before switching projects if you want to keep them."
        alert.addButton(withTitle: "Cancel"); alert.addButton(withTitle: "Discard")
        return alert.runModal() == .alertSecondButtonReturn
    }
    func newProject() {
        guard !busy, discardDraftsIfNeeded() else { return }
        guard connected, !connectingToCoordinator, !restoringProjectSession else { status = "Reconnect the Coordinator before creating a project."; return }
        let panel = NSSavePanel(); panel.title = "Create project folder"; panel.nameFieldStringValue = "Analysis.vectorglass"
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        open(path: url.path, create: true)
    }
    func openProject() {
        guard !busy, discardDraftsIfNeeded() else { return }
        guard connected, !connectingToCoordinator, !restoringProjectSession else { status = "Reconnect the Coordinator before opening a project."; return }
        let panel = NSOpenPanel(); panel.title = "Open project folder"; panel.canChooseDirectories = true; panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        open(path: url.path, create: false)
    }
    func refreshProject() { if let path = project?.path, !busy { open(path: path, create: false, clearDrafts: false) } }
    private func open(path: String, create: Bool, clearDrafts: Bool = true) {
        busy = true; status = "Opening project…"
        client.request(operation: { $0.openProject(path, create: create, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false
            if self.apply(result), clearDrafts { self.drafts.removeAll() }
            if self.project != nil, let path = self.project?.path { self.rememberRecentProject(path) }
        }
    }

    func openRecentProject(_ recent: VGRecentProject) {
        guard !busy, discardDraftsIfNeeded() else { return }
        guard connected, !connectingToCoordinator, !restoringProjectSession else { status = "Reconnect the Coordinator before opening a recent project."; return }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: recent.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            recentProjects.removeAll { $0.path == recent.path }
            if let data = try? JSONEncoder().encode(recentProjects) { UserDefaults.standard.set(data, forKey: Self.recentProjectsDefaultsKey) }
            status = "Recent project is no longer available: \(recent.displayName)."
            return
        }
        open(path: recent.path, create: false)
    }
    func selectBinary() {
        guard !busy else { status = "Finish or cancel the active operation before importing another artifact."; return }
        guard projectSessionReady else { status = "Restore the project session before importing a binary."; return }
        guard project != nil else { status = "Create or open a project before importing a binary."; return }
        let panel = NSOpenPanel(); panel.title = "Import binary"; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let alert = NSAlert()
        alert.messageText = "Import \(url.lastPathComponent)"
        alert.informativeText = "Mach-O uses validated header addresses. Raw firmware needs an explicit architecture and load address. For composite SEPI, choose an image offset; Auto selects the first validated embedded Mach-O."
        alert.addButton(withTitle: "Import"); alert.addButton(withTitle: "Cancel")
        let format = NSPopUpButton(frame: .zero, pullsDown: false)
        let formats = ["auto", "macho", "sepos", "securerom", "iboot", "raw"]
        format.addItems(withTitles: ["Auto / composite", "Mach-O", "SEPOS / SEPI", "SecureROM (raw)", "iBoot (raw)", "Raw binary"])
        let architecture = NSPopUpButton(frame: .zero, pullsDown: false)
        let architectures = ["auto", "aarch64", "i386", "x86_64", "arm", "thumb"]
        architecture.addItems(withTitles: ["From header (ARM32 defaults to Thumb-2)", "AArch64", "Intel 32-bit", "Intel 64-bit", "ARM32", "Thumb-2"])
        let address = NSTextField(string: "0x0")
        let offset = NSTextField(string: "0x0")
        let stack = NSStackView(views: [NSTextField(labelWithString: "Image format"), format,
            NSTextField(labelWithString: "Architecture / instruction mode"), architecture,
            NSTextField(labelWithString: "Raw load address (hex or decimal; 0 = file-relative)"), address,
            NSTextField(labelWithString: "Image offset in file (hex or decimal)"), offset])
        stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = 8
        stack.frame = NSRect(x: 0, y: 0, width: 460, height: 235)
        for view in [format, architecture, address, offset] { view.widthAnchor.constraint(equalToConstant: 460).isActive = true }
        alert.accessoryView = stack
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let input = BinaryImportOptions(format: formats[format.indexOfSelectedItem], architecture: architectures[architecture.indexOfSelectedItem], load_address: address.stringValue.lowercased(), file_offset: offset.stringValue.lowercased())
        guard let options = input.validated, let encoded = try? JSONEncoder().encode(options),
              let optionsJSON = String(data: encoded, encoding: .utf8) else { status = "Invalid import mapping. Enter valid hexadecimal or decimal addresses."; return }
        if ["securerom", "iboot", "raw"].contains(options.format), options.architecture == "auto" {
            status = "Choose the raw firmware architecture before importing."; return
        }
        let id = UUID().uuidString; activeRequest = id; busy = true; analysisJobStatus = nil; status = "Capturing and inspecting source…"
        client.request(timeout: 150, operation: { $0.importBinary(url.path, optionsJSON: optionsJSON, requestID: id, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false; self.activeRequest = nil
            if self.apply(result) { self.selectedID = self.project?.records.first?.id }
        }
    }
    func loadPage(start: UInt64 = 0, forceVerify: Bool = false) {
        guard !busy, projectServiceReady, let record = selected, record.result.captured,
              let digest = record.result.source_sha256, let projectPath = project?.path else { return }
        let id = UUID().uuidString; let generation = navigationGeneration
        activeRequest = id; busy = true; analysisJobStatus = nil; status = "Loading instruction page…"
        client.request(timeout: 150, operation: { $0.instructionPage(record.id, startWord: String(start), forceVerify: forceVerify, requestID: id, withReply: $1) }) { [weak self] result in
            guard let self, self.activeRequest == id else { return }
            self.busy = false; self.activeRequest = nil
            defer { self.traversingPreviousPage = false }
            guard self.navigationGeneration == generation, self.selectedID == record.id, self.project?.path == projectPath else {
                self.status = "Discarded a page for an earlier selection."; return
            }
            do {
                let reply = try JSONDecoder().decode(PageReply.self, from: Data(try result.get().utf8))
                guard reply.ok, let page = reply.page else { self.status = reply.message; return }
                guard page.matches(id: id, digest: digest, start: start) else { self.status = "Page identity or bounds mismatch."; return }
                if !self.traversingPreviousPage, let old = self.currentPage, let oldStart = UInt64(old.start_word), oldStart != start {
                    self.pageStarts.append(oldStart); if self.pageStarts.count > 256 { self.pageStarts.removeFirst() }
                }
                self.traversingPreviousPage = false
                self.instructionPage = page; self.pageRecordID = record.id
                self.status = reply.message.isEmpty ? "Instruction page loaded." : reply.message
            } catch { self.status = error.localizedDescription }
        }
    }
    private func startAnalysisMonitor(requestID: String) {
        analysisMonitorTask?.cancel()
        analysisJobStatus = nil
        analysisMonitorTask = Task { [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                let result: Result<String, Error> = await withCheckedContinuation { continuation in
                    self.client.request(timeout: 3, invalidateConnectionOnTimeout: false, operation: { service, reply in
                        service.inspectionStatus(requestID, withReply: reply)
                    }) { continuation.resume(returning: $0) }
                }
                guard !Task.isCancelled, self.activeRequest == requestID else { return }
                if case .success(let text) = result,
                   let data = text.data(using: .utf8),
                   let reply = try? JSONDecoder().decode(InspectionStatusReply.self, from: data),
                   reply.ok, let status = reply.status {
                    self.analysisJobStatus = status
                    if status.terminal { return }
                }
                do { try await Task.sleep(nanoseconds: 500_000_000) }
                catch { return }
            }
        }
    }

    private func stopAnalysisMonitor(clear: Bool = false) {
        analysisMonitorTask?.cancel()
        analysisMonitorTask = nil
        if clear { analysisJobStatus = nil }
    }

    var analysisProgressLabel: String? {
        guard let value = analysisJobStatus else { return nil }
        let seconds = value.elapsedSeconds
        let elapsed = seconds < 60 ? String(format: "%.1fs", seconds) : String(format: "%dm %02ds", Int(seconds) / 60, Int(seconds) % 60)
        let stage = value.progress_stage.replacingOccurrences(of: "-", with: " ").capitalized
        if value.operation == "analysis" {
            return "\(stage.isEmpty ? value.phase.displayName : stage) · \(value.progress_percent)% · \(elapsed)"
        }
        return "\(value.phase.displayName) · \(elapsed)"
    }

    var analysisProgressValue: Double? {
        guard let value = analysisJobStatus, value.operation == "analysis" else { return nil }
        return Double(min(100, max(0, value.progress_percent)))
    }

    func analyzeSelected() {
        guard !busy, projectServiceReady, let record = selected, record.result.captured,
              let digest = record.result.source_sha256, let projectPath = project?.path else { return }
        let id = UUID().uuidString; let generation = navigationGeneration
        activeRequest = id; busy = true; status = "Preparing bounded analysis…"
        startAnalysisMonitor(requestID: id)
        client.request(timeout: 260, maxReplyBytes: WorkbenchProtocolLimits.analysisXPCReplyBytes,
                       operation: { $0.analyzeRecord(record.id, requestID: id, withReply: $1) }) { [weak self] result in
            guard let self, self.activeRequest == id else { return }
            self.stopAnalysisMonitor()
            guard self.navigationGeneration == generation, self.selectedID == record.id, self.project?.path == projectPath else {
                self.busy = false; self.activeRequest = nil
                self.status = "Discarded analysis for an earlier selection."; return
            }
            let text: String
            do { text = try result.get() }
            catch {
                self.busy = false; self.activeRequest = nil
                self.status = error.localizedDescription
                return
            }
            self.status = "Decoding bounded analysis result…"
            Task { [weak self] in
                let decoded = await Task.detached(priority: .userInitiated) { VGAnalysisDecodeResult.decode(text) }.value
                guard let self, self.activeRequest == id else { return }
                self.busy = false; self.activeRequest = nil
                guard self.navigationGeneration == generation, self.selectedID == record.id, self.project?.path == projectPath else {
                    self.status = "Discarded analysis for an earlier selection."; return
                }
                guard let reply = decoded.reply else {
                    self.status = decoded.errorMessage ?? "Analysis result decoding failed."
                    return
                }
                guard reply.ok, let report = reply.analysis, report.matches(id: id, digest: digest) else {
                    self.status = reply.message; return
                }
                self.analysisReport = report; self.analysisRecordID = record.id
                self.cacheAnalysisReport(report, recordID: record.id)
                self.rebuildEvidenceIndex(recordID: record.id, report: report, workerEvidence: reply.evidence)
                self.recordAnalysisSnapshot(recordID: record.id, sourceSHA256: digest, report: report)
                self.status = reply.message.isEmpty ? "Analysis completed." : reply.message
                self.recordLedger(kind: .analysisCompleted, provenance: .derived, detail: "Analyzed \(report.analyzed_instructions) instructions, \(report.function_count) functions, and \(report.basic_block_count) basic blocks.")
                self.triggerPluginEvent("on_analysis_complete")
            }
        }
    }
    func prepareBinaryDiff() {
        guard !busy, projectServiceReady, let project else { return }
        guard !diffBaselineID.isEmpty, !diffCandidateID.isEmpty, diffBaselineID != diffCandidateID else {
            diffStatus = "Choose two different project artifacts."
            return
        }
        let ids = [diffBaselineID, diffCandidateID]
        let records = ids.compactMap { id in project.records.first(where: { $0.id == id }) }
        guard records.count == 2, records.allSatisfy({ $0.result.captured && $0.result.source_sha256 != nil }) else {
            diffStatus = "Binary Diff requires two captured artifacts with source identity."
            return
        }
        guard records[0].result.architecture == records[1].result.architecture else {
            diffStatus = "Architecture mismatch: \(records[0].result.architecture) vs \(records[1].result.architecture)."
            return
        }
        let missing = records.filter { analysisReportsByRecordID[$0.id] == nil }
        if missing.isEmpty {
            diffStatus = "Diff evidence ready."
            recordLedger(kind: .workspaceVisited, provenance: .derived, detail: "Prepared Binary Diff for \(records[0].name) → \(records[1].name).")
            return
        }
        busy = true
        diffStatus = "Analyzing \(missing.count) missing side(s)…"
        let projectPath = project.path

        func analyzeNext(_ index: Int) {
            guard index < missing.count else {
                self.busy = false
                self.activeRequest = nil
                self.diffStatus = "Diff evidence ready."
                self.recordLedger(kind: .analysisCompleted, provenance: .derived, detail: "Prepared Binary Diff for \(records[0].name) → \(records[1].name).")
                return
            }
            let record = missing[index]
            guard let digest = record.result.source_sha256 else {
                self.busy = false
                self.diffStatus = "Missing source identity for \(record.name)."
                return
            }
            let requestID = UUID().uuidString
            self.activeRequest = requestID
            self.diffStatus = "Analyzing \(record.name)…"
            self.startAnalysisMonitor(requestID: requestID)
            self.client.request(timeout: 260, maxReplyBytes: WorkbenchProtocolLimits.analysisXPCReplyBytes,
                                operation: { $0.analyzeRecord(record.id, requestID: requestID, withReply: $1) }) { [weak self] result in
                guard let self, self.activeRequest == requestID else { return }
                self.stopAnalysisMonitor()
                let text: String
                do { text = try result.get() }
                catch {
                    self.busy = false
                    self.activeRequest = nil
                    self.diffStatus = error.localizedDescription
                    return
                }
                self.diffStatus = "Decoding bounded analysis result…"
                Task { [weak self] in
                    let decoded = await Task.detached(priority: .userInitiated) { VGAnalysisDecodeResult.decode(text) }.value
                    guard let self, self.activeRequest == requestID else { return }
                    guard let reply = decoded.reply else {
                        self.busy = false
                        self.activeRequest = nil
                        self.diffStatus = decoded.errorMessage ?? "Diff analysis result decoding failed."
                        return
                    }
                    guard self.project?.path == projectPath, reply.ok, let report = reply.analysis, report.matches(id: requestID, digest: digest) else {
                        self.busy = false
                        self.activeRequest = nil
                        self.diffStatus = reply.message.isEmpty ? "Diff analysis identity check failed." : reply.message
                        return
                    }
                    self.cacheAnalysisReport(report, recordID: record.id)
                    self.recordAnalysisSnapshot(recordID: record.id, sourceSHA256: digest, report: report)
                    if self.selectedID == record.id {
                        self.analysisReport = report
                        self.analysisRecordID = record.id
                        self.rebuildEvidenceIndex(recordID: record.id, report: report, workerEvidence: reply.evidence)
                    }
                    analyzeNext(index + 1)
                }
            }
        }
        analyzeNext(0)
    }

    func analysisForDiff(recordID: String) -> AnalysisReport? {
        analysisReportsByRecordID[recordID]
    }

    func filteredAnalysisLines() -> String {
        guard currentAnalysis != nil else { return "Run Analyze binary first." }
        guard let index = currentEvidenceIndex else { return analysisEvidenceIndexing ? "Indexing typed evidence…" : "Evidence index unavailable." }
        let query = analysisSearch.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return "Type a symbol, address, string, or analysis term to search across the current result." }
        let hits = index.search(query, limit: 500)
        guard !hits.isEmpty else { return "No analysis hits for ‘\(analysisSearch)’." }
        let body = hits.map { "[\($0.source)] \($0.text)" }.joined(separator: "\n")
        return hits.count >= 500 ? body + "\n… result cap reached" : body
    }

    func contextInspector() -> String {
        let addr = addressText.isEmpty ? "none" : "0x" + addressText.replacingOccurrences(of: "0x", with: "")
        let cfg = cfgFocus.isEmpty ? "none" : cfgFocus
        let query = analysisSearch.isEmpty ? "none" : analysisSearch
        return "Workspace: \(workspace)\nAddress: \(addr)\nCFG focus: \(cfg)\nSearch: \(query)\nSelected symbol: \(selectedSymbol.isEmpty ? "none" : selectedSymbol)\nDebugger: \(debugAttached ? "attached PID " + debugPIDText : "idle")\nDebug PC: \(debugSnapshot?.pc ?? "none")\nVM: \(virtualizationState?.running == true ? (virtualizationState?.profileID ?? "active") : "idle")\nGuest GPA: \(virtualizationREAddress)\nHistory: back=\(workspaceBack.count) forward=\(workspaceForward.count)\nInspector: \(inspectorPinned ? "pinned" : "following selection")\nSelection: \(selectedEvidence.isEmpty ? "none" : selectedEvidence)"
    }

    func searchAddress() -> UInt64? {
        let text = analysisSearch.lowercased()
        guard let range = text.range(of: "0x[0-9a-f]+", options: .regularExpression) else { return nil }
        var token = String(text[range]); token.removeFirst(2)
        return UInt64(token, radix: 16)
    }
    func openSearchInDisassembly() {
        guard let address = searchAddress() else { status = "Search text does not contain a hexadecimal virtual address."; return }
        addressText = String(format: "%llx", address); workspace = "Disassembly"
        if currentPage != nil { jumpToAddress() } else { loadPage() }
    }
    func focusSearchInCFG() {
        guard let address = searchAddress() else { status = "Search text does not contain a hexadecimal virtual address."; return }
        cfgFocus = String(format: "0x%016llx", address); workspace = "CFG"
    }

    func verifyPage() {
        loadPage(start: currentPage.flatMap { UInt64($0.start_word) } ?? 0, forceVerify: true)
    }
    func previousPage() {
        guard let page = currentPage, let start = UInt64(page.start_word) else { return }
        if page.unit_bytes != nil, page.unitBytes < 4 {
            let previous = pageStarts.popLast() ?? 0
            traversingPreviousPage = true
            loadPage(start: previous)
        } else { loadPage(start: start >= 256 ? start - 256 : 0) }
    }
    func nextPage() {
        guard let page = currentPage, let start = UInt64(page.start_word), let total = UInt64(page.total_words),
              start < total, UInt64(page.word_count) < total - start else { return }
        loadPage(start: start + UInt64(page.word_count))
    }
    func jumpToAddress() {
        guard let page = currentPage, let base = UInt64(page.section_address), let total = UInt64(page.total_words) else { return }
        var value = addressText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if value.hasPrefix("0x") { value.removeFirst(2) }
        let unit = page.unitBytes
        guard unit > 0, let address = UInt64(value, radix: 16), address >= base, (address - base) % unit == 0,
              (address - base) / unit < total else { status = "Enter an aligned hexadecimal VA inside the selected code section."; return }
        loadPage(start: (address - base) / unit)
    }
    func cancel() {
        guard let id = activeRequest else { return }; status = "Cancellation requested…"
        client.request(operation: { service, reply in
            service.cancelInspection(id) { accepted in reply(accepted ? "accepted" : "already finished") }
        }) { [weak self] result in
            if case .failure(let error) = result { self?.status = error.localizedDescription }
        }
    }
    func chooseDebugExecutable() {
        let panel = NSOpenPanel(); panel.title = "Choose executable to debug"; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        debugLaunchPath = url.path
    }
    func debugLaunch() {
        guard !debugLaunchPath.isEmpty else { status = "Choose an executable first."; return }
        client.request(timeout: 20, operation: { $0.debugLaunch(self.debugLaunchPath, withReply: $1) }) { [weak self] result in self?.applyDebugCommand(result) }
    }
    func debugAttachPID() {
        guard let pid = Int32(debugPIDText), pid > 1 else { status = "Enter a valid PID greater than 1."; return }
        client.request(timeout: 20, operation: { $0.debugAttach(pid, withReply: $1) }) { [weak self] result in self?.applyDebugCommand(result) }
    }
    func debugContinue() { debugCommand("continue", shouldRefresh: false) }
    func debugInterrupt() { debugCommand("interrupt") }
    func debugStep() { debugCommand("step", shouldRefresh: false) }
    func debugDetach() { debugCommand("detach", shouldRefresh: false) }
    func debugSetBreakpoint() { debugCommand("breakpoint.set", address: debugAddress.isEmpty ? addressText : debugAddress) }
    func debugRemoveBreakpoint() { debugCommand("breakpoint.remove", address: debugAddress.isEmpty ? addressText : debugAddress) }
    private func debugCommand(_ command: String, address: String = "", shouldRefresh: Bool = true) {
        client.request(timeout: 10, operation: { $0.debugCommand(command, address: address, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let reply = try JSONDecoder().decode(DebugCommandReply.self, from: Data(try result.get().utf8))
                self.status = reply.message
                if reply.ok {
                    if command == "detach" { self.debugAttached = false; self.debugSnapshot = nil; self.stopDebugPolling() }
                    else {
                        self.debugAttached = true
                        if command == "breakpoint.set" || command == "breakpoint.remove" {
                            self.recordLedger(kind: .breakpointChanged, provenance: .observed, detail: "\(command == "breakpoint.set" ? "Set" : "Removed") software breakpoint at \(address).")
                        }
                        if shouldRefresh { self.debugRefreshSnapshot() }; self.startDebugPolling()
                    }
                }
            } catch { self.status = error.localizedDescription }
        }
    }
    private func applyDebugCommand(_ result: Result<String, Error>) {
        do {
            let reply = try JSONDecoder().decode(DebugCommandReply.self, from: Data(try result.get().utf8))
            status = reply.message
            guard reply.ok else { return }
            debugAttached = true
            if let pid = reply.pid { debugPIDText = String(pid) }
            resetRuntimeEvidence()
            debugRefreshModules()
            debugRefreshSnapshot(); startDebugPolling()
        } catch { status = error.localizedDescription }
    }
    func debugRefreshSnapshot() {
        guard debugAttached else { return }
        client.request(timeout: 5, operation: { $0.debugSnapshot(withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let text = try result.get()
                if let command = try? JSONDecoder().decode(DebugCommandReply.self, from: Data(text.utf8)), !command.ok { self.status = command.message; return }
                let snap = try JSONDecoder().decode(DebugSnapshot.self, from: Data(text.utf8))
                self.debugSnapshot = snap; self.debugPIDText = String(snap.pid); self.debugAddress = snap.pc; self.debugThreadID = snap.selected_thread_id ?? self.debugThreadID
                self.recordRuntimeSample(snap)
                if self.debugFollowPC, self.selected != nil { self.addressText = snap.pc; self.cfgFocus = snap.pc }
                self.debugRefreshDynamicViews()
            } catch { self.status = error.localizedDescription }
        }
    }
    func debugReadMemory() {
        let source = debugAddress.isEmpty ? addressText : debugAddress
        guard !source.isEmpty else { status = "Enter a memory address."; return }
        client.request(timeout: 5, operation: { $0.debugReadMemory(source, length: self.debugMemoryLength, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let text = try result.get()
                if let command = try? JSONDecoder().decode(DebugCommandReply.self, from: Data(text.utf8)), !command.ok { self.status = command.message; return }
                let memory = try JSONDecoder().decode(DebugMemory.self, from: Data(text.utf8))
                self.recordRuntimeMemory(memory)
                self.debugMemory = "\(memory.address) [\(memory.length) bytes]\n" + stride(from: 0, to: memory.hex.count, by: 32).map { i in
                    let start = memory.hex.index(memory.hex.startIndex, offsetBy: i); let end = memory.hex.index(start, offsetBy: min(32, memory.hex.count - i))
                    return String(memory.hex[start..<end])
                }.joined(separator: "\n")
            } catch { self.status = error.localizedDescription }
        }
    }

    func debugMoveMemory(_ direction: Int) {
        let source = debugAddress.isEmpty ? addressText : debugAddress
        var raw = source.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
        if raw.hasPrefix("0x") { raw.removeFirst(2) }
        guard let value = UInt64(raw, radix: 16) else { status = "Enter a valid hexadecimal memory address."; return }
        let delta = UInt64(max(1, debugMemoryLength))
        let next: UInt64
        if direction < 0 { next = value >= delta ? value - delta : 0 } else { next = value.addingReportingOverflow(delta).overflow ? value : value + delta }
        debugAddress = "0x" + String(next, radix: 16)
        debugReadMemory()
    }
    func debugOpenPCInAnnotatedCode() { guard let pc = debugSnapshot?.pc, selected != nil else { return }; addressText = pc; analysisSearch = pc; workspace = "Annotated Code" }
    func debugOpenPCInCFG() { guard let pc = debugSnapshot?.pc, selected != nil else { return }; addressText = pc; cfgFocus = pc; workspace = "CFG" }
    func debugOpenPCInRuntimeCorrelation() { guard let pc = debugSnapshot?.pc, selected != nil else { return }; addressText = pc; analysisSearch = pc; cfgFocus = pc; workspace = "Runtime Correlation" }
    func debugOpenPCInTraceExplorer() { guard let pc = debugSnapshot?.pc, selected != nil else { return }; addressText = pc; analysisSearch = pc; cfgFocus = pc; workspace = "Trace Explorer" }

    func debugQuery(_ query: String, assign: @escaping (WorkbenchModel, String) -> Void) {
        guard debugAttached else { return }
        client.request(timeout: 5, operation: { $0.debugQuery(query, withReply: $1) }) { [weak self] result in
            guard let self else { return }; do {
                let text = try result.get()
                if let command = try? JSONDecoder().decode(DebugCommandReply.self, from: Data(text.utf8)), !command.ok { self.status = command.message; return }
                assign(self, text)
            } catch { self.status = error.localizedDescription }
        }
    }
    func debugRefreshDynamicViews() {
        debugQuery("threads") { $0.debugThreads = $1 }
        debugQuery("backtrace") { $0.debugBacktrace = $1 }
        debugQuery("disassembly") { $0.debugDisassembly = $1 }
    }
    func debugRefreshMemoryMap() { debugQuery("memory-map") { $0.debugMemoryMap = $1 } }
    func debugRefreshModules() { debugQuery("modules") { $0.debugModules = $1 } }
    func resetRuntimeEvidence() {
        runtimeSamples.removeAll()
        runtimeMemoryObservations.removeAll()
        runtimeSequence = 0
        runtimeMemorySequence = 0
    }
    private func recordRuntimeSample(_ snapshot: DebugSnapshot) {
        if let last = runtimeSamples.last,
           last.pcText.lowercased() == snapshot.pc.lowercased(),
           last.registers == snapshot.registers,
           last.stopped == snapshot.stopped,
           last.breakpoints == snapshot.breakpoints.compactMap(VGTargetUtilities.parseHex) { return }
        runtimeSequence += 1
        let event = snapshot.stopped ? "stopped" : "snapshot"
        let reason = debugLastReason.isEmpty ? debugEvent : debugLastReason
        guard let sample = VGRuntimeSample.capture(sequence: runtimeSequence, snapshot: snapshot, previous: runtimeSamples.last, event: event, reason: reason) else { return }
        runtimeSamples.append(sample)
        if runtimeSamples.count > 256 { runtimeSamples.removeFirst(runtimeSamples.count - 256) }
        recordLedger(kind: .debuggerStop, provenance: .observed, detail: "\(event) at \(sample.pcText)\(reason.isEmpty ? "" : " — " + reason).")
    }
    private func recordRuntimeMemory(_ memory: DebugMemory) {
        runtimeMemorySequence += 1
        guard let observation = VGRuntimeCorrelationEngine.memoryObservation(sequence: runtimeMemorySequence, memory: memory) else { return }
        runtimeMemoryObservations.append(observation)
        if runtimeMemoryObservations.count > 64 { runtimeMemoryObservations.removeFirst(runtimeMemoryObservations.count - 64) }
        recordLedger(kind: .memoryObserved, provenance: .observed, detail: "Read \(observation.bytes.count) bytes at \(observation.addressText).")
    }
    func debugSelectThread() {
        guard !debugThreadID.isEmpty else { status = "Enter a thread ID."; return }
        client.request(timeout: 5, operation: { $0.debugSelectThread(self.debugThreadID, withReply: $1) }) { [weak self] result in
            guard let self else { return }; do { let reply = try JSONDecoder().decode(DebugCommandReply.self, from: Data(try result.get().utf8)); self.status = reply.message; if reply.ok { self.debugRefreshSnapshot() } } catch { self.status = error.localizedDescription }
        }
    }

    private func startDebugPolling() {
        guard debugPollTimer == nil else { return }
        debugPollTimer = Timer.scheduledTimer(withTimeInterval: 0.35, repeats: true) { [weak self] _ in Task { @MainActor in self?.pollDebugEvent() } }
    }
    private func stopDebugPolling() { debugPollTimer?.invalidate(); debugPollTimer = nil; debugPollInFlight = false }
    private func pollDebugEvent() {
        guard debugAttached, !debugPollInFlight else { return }; debugPollInFlight = true
        client.request(timeout: 3, operation: { $0.debugPollEvent(withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.debugPollInFlight = false
            do {
                let text = try result.get()
                if let command = try? JSONDecoder().decode(DebugCommandReply.self, from: Data(text.utf8)), !command.ok { self.debugEvent = command.message; return }
                let event = try JSONDecoder().decode(DebugEvent.self, from: Data(text.utf8))
                guard event.event != "none" else { return }
                self.debugEvent = [event.event, event.reason, event.signal.map { "signal=\($0)" }, event.code.map { "code=\($0)" }, event.pc.map { "pc=\($0)" }].compactMap { $0 }.joined(separator: " ")
                self.debugLastSignal = event.signal; self.debugLastReason = event.reason ?? event.event
                if self.debugFollowPC, let pc = event.pc, self.selected != nil { self.addressText = pc; self.cfgFocus = pc }
                self.triggerPluginEvent("on_stop")
                if event.event == "exited" || event.event == "signaled" { self.debugAttached = false; self.debugSnapshot = nil; self.stopDebugPolling() }
                else { self.debugRefreshSnapshot() }
            } catch { self.debugEvent = error.localizedDescription }
        }
    }
    func refreshVirtualizationProfiles() {
        client.request(timeout: 5, operation: { $0.virtualizationProfiles(withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let profiles = try JSONDecoder().decode([VirtualizationProfile].self, from: Data(try result.get().utf8))
                self.virtualizationProfiles = profiles
                if !profiles.contains(where: { $0.id == self.virtualizationProfileID }), let first = profiles.first { self.virtualizationProfileID = first.id; self.virtualizationRAMMiB = first.defaultRAMMiB }
                self.virtualizationStatus = "Loaded \(profiles.count) virtualization profiles."
            } catch { self.virtualizationStatus = error.localizedDescription }
        }
    }
    func startVirtualMachine() {
        busy = true; virtualizationStatus = "Starting Hypervisor.framework backend…"
        client.request(timeout: 12, operation: { $0.virtualizationStart(self.virtualizationProfileID, ramMiB: self.virtualizationRAMMiB, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false
            do { let state = try JSONDecoder().decode(VirtualizationState.self, from: Data(try result.get().utf8)); self.virtualizationState = state; self.virtualizationStatus = state.message } catch { self.virtualizationStatus = error.localizedDescription }
        }
    }
    func virtualizationCommand(_ command: String) {
        client.request(timeout: 8, operation: { $0.virtualizationCommand(command, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do { let state = try JSONDecoder().decode(VirtualizationState.self, from: Data(try result.get().utf8)); self.virtualizationState = state; self.virtualizationStatus = state.message } catch { self.virtualizationStatus = error.localizedDescription }
        }
    }
    func virtualizationRE(_ command: String, value: String? = nil, path: String? = nil, length: Int? = nil) {
        let request = VirtualizationRERequest(command: command, address: virtualizationREAddress, length: length ?? virtualizationRELength, value: value, path: path, mode: nil)
        guard let data = try? JSONEncoder().encode(request) else { virtualizationStatus = "Unable to encode virtualization RE request."; return }
        let json = String(decoding: data, as: UTF8.self)
        client.request(timeout: command == "run_once" ? 15 : 8, operation: { $0.virtualizationRE(json, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let reply = try JSONDecoder().decode(VirtualizationREReply.self, from: Data(try result.get().utf8))
                self.virtualizationStatus = reply.message
                self.virtualizationRETitle = reply.title ?? "Hypervisor RE Suite"
                self.virtualizationREText = reply.text ?? reply.message
                if let address = reply.address { self.virtualizationREAddress = address }
                if let pc = reply.pc { self.virtualizationREAddress = pc }
            } catch { self.virtualizationStatus = error.localizedDescription }
        }
    }
    func chooseVirtualizationImage() {
        let panel = NSOpenPanel(); panel.title = "Choose firmware / ROM / guest image"; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        virtualizationImagePath = url.path
    }
    func loadVirtualizationImage() {
        guard !virtualizationImagePath.isEmpty else { virtualizationStatus = "Choose an image first."; return }
        virtualizationRE("load_image", path: virtualizationImagePath)
    }
    func firmwareRuntime(_ request: FirmwareRuntimeRequest, timeout: UInt64 = 30, completion: ((FirmwareRuntimeReply)->Void)? = nil) {
        guard let d = try? JSONEncoder().encode(request) else { firmwareStatus = "Unable to encode firmware request."; return }
        let json = String(decoding:d, as:UTF8.self)
        firmwareStatus = "Working…"
        client.request(timeout: timeout, operation: { $0.firmwareRuntime(json, withReply:$1) }) { [weak self] result in
            guard let self else { return }
            do {
                let r = try JSONDecoder().decode(FirmwareRuntimeReply.self, from:Data(try result.get().utf8))
                self.firmwareStatus = r.message
                if let t=r.text { self.firmwareText=t }
                if let d=r.dyld { self.dyldCache=d; self.selectedDyldImageID=d.images.first?.id ?? ""; self.dyldReadAddress=d.images.first?.loadAddress ?? "" }
                if request.command == "dyld_read", let t=r.text { self.dyldReadText=t }
                if let x=r.devices { self.firmwareDevices=x; if !x.contains(where:{$0.identifier==self.firmwareDeviceID}) { self.firmwareDeviceID=x.first?.identifier ?? "" } }
                if let x=r.firmwares { self.firmwareReleases=x; if !x.contains(where:{$0.buildid==self.firmwareBuildID}) { self.firmwareBuildID=x.first?.buildid ?? "" } }
                if let x=r.keys { self.firmwareKeys=x }
                if let p=r.outputPath { self.ipswPath=p }
                if request.command.hasPrefix("vectoremu_") {
                    self.vectorEmuStatus = r.message
                    if let b=r.running { self.vectorEmuRunning=b }
                    if let o=r.emulatorOutput, !o.isEmpty { self.vectorEmuRaw=o }
                } else if request.command.hasPrefix("iphemeral_") {
                    if let b=r.running { self.iphemeralRunning=b }
                    if let o=r.emulatorOutput { self.iphemeralOutput=o }
                }
                completion?(r)
            } catch { self.firmwareStatus=error.localizedDescription }
        }
    }
    func chooseDyldCache() { let p=NSOpenPanel(); p.title="Choose dyld shared cache"; p.canChooseDirectories=false; p.allowsMultipleSelection=false; guard p.runModal() == .OK,let u=p.url else{return}; dyldCachePath=u.path; dyldReadText=""; firmwareRuntime(.init(command:"dyld_inspect",path:u.path)) }
    func useSelectedDyldImageAddress() { if let image=dyldCache?.images.first(where:{$0.id==selectedDyldImageID}) { dyldReadAddress=image.loadAddress } }
    func readDyldCacheBytes() { guard !dyldCachePath.isEmpty,!dyldReadAddress.isEmpty else { firmwareStatus="Open a cache and select an address first."; return }; firmwareRuntime(.init(command:"dyld_read",path:dyldCachePath,address:dyldReadAddress,count:dyldReadCount)) }
    func readSelectedDyldImageHeader() { useSelectedDyldImageAddress(); dyldReadCount=max(512,min(dyldReadCount,4096)); readDyldCacheBytes() }
    func refreshFirmwareDevices() { firmwareRuntime(.init(command:"ipsw_devices",soc:firmwareSoCFilter)) }
    func refreshFirmwareReleases() { guard !firmwareDeviceID.isEmpty else{return}; firmwareRuntime(.init(command:"ipsw_releases",device:firmwareDeviceID)) }
    func chooseIPSW() { let p=NSOpenPanel(); p.title="Import IPSW"; p.allowedContentTypes=[]; p.canChooseDirectories=false; guard p.runModal() == .OK,let u=p.url else{return}; ipswPath=u.path; firmwareRuntime(.init(command:"ipsw_import",path:u.path),timeout:60) }
    func downloadSelectedIPSW() { guard !firmwareDeviceID.isEmpty,!firmwareBuildID.isEmpty else{return}; let p=NSOpenPanel(); p.title="Choose IPSW download folder"; p.canChooseFiles=false; p.canChooseDirectories=true; guard p.runModal() == .OK,let u=p.url else{return}; firmwareRuntime(.init(command:"ipsw_download",device:firmwareDeviceID,build:firmwareBuildID,destination:u.path),timeout:1900) }
    func fetchSelectedFirmwareKeys() { guard !firmwareDeviceID.isEmpty,!firmwareBuildID.isEmpty else{return}; firmwareRuntime(.init(command:"firmware_keys",device:firmwareDeviceID,build:firmwareBuildID),timeout:30) }
    func chooseVectorEmuExecutable() { let p=NSOpenPanel(); p.title="Choose vg-emu-server"; p.canChooseDirectories=false; guard p.runModal() == .OK,let u=p.url else{return}; vectorEmuExecutable=u.path }
    func chooseVectorEmuROM() { let p=NSOpenPanel(); p.title="Choose research ROM / firmware image"; p.canChooseDirectories=false; guard p.runModal() == .OK,let u=p.url else{return}; vectorEmuROM=u.path }
    func vectorEmuStatusCheck() { firmwareRuntime(.init(command:"vectoremu_status", executable:vectorEmuExecutable)) }
    func startVectorEmu() {
        vectorEmuAttached=false
        firmwareRuntime(.init(command:"vectoremu_start", soc:vectorEmuSoC, executable:vectorEmuExecutable, rom:vectorEmuROM, port:vectorEmuPort), timeout:15) { [weak self] reply in
            guard let self, reply.ok else { return }
            DispatchQueue.main.asyncAfter(deadline: DispatchTime.now() + DispatchTimeInterval.milliseconds(250)) { self.vectorEmuDebug("attach") }
        }
    }
    func stopVectorEmu() { firmwareRuntime(.init(command:"vectoremu_stop")); vectorEmuAttached=false }
    func vectorEmuDebug(_ command:String) {
        let req = FirmwareRuntimeRequest(command:"vectoremu_debug", port:vectorEmuPort, debugCommand:command, address:vectorEmuAddress, count:vectorEmuCount, steps:vectorEmuSteps)
        firmwareRuntime(req, timeout:command == "run" ? 8 : 4) { [weak self] reply in
            guard let self else { return }
            let text = reply.text ?? reply.emulatorJSON ?? reply.message
            self.vectorEmuRaw = text
            switch command {
            case "attach": self.vectorEmuAttached = reply.ok
            case "detach", "shutdown": self.vectorEmuAttached = false
            case "registers", "cpu_state": self.vectorEmuRegisters = text
            case "disassemble": self.vectorEmuDisassembly = text
            case "trace": self.vectorEmuTrace = text
            case "regions": self.vectorEmuRegions = text
            default: break
            }
        }
    }
    func vectorEmuRefreshContext() { vectorEmuDebug("registers"); vectorEmuDebug("disassemble") }
    func chooseIPhemeralExecutable() { let p=NSOpenPanel(); p.title="Choose iphem-cli"; p.canChooseDirectories=false; guard p.runModal() == .OK,let u=p.url else{return}; iphemeralExecutable=u.path }
    func chooseIPhemeralROM() { let p=NSOpenPanel(); p.title="Choose ROM / firmware image"; p.canChooseDirectories=false; guard p.runModal() == .OK,let u=p.url else{return}; iphemeralROM=u.path }
    func iphemeralStatusCheck() { firmwareRuntime(.init(command:"iphemeral_status",executable:iphemeralExecutable)) }
    func startIPhemeral() { firmwareRuntime(.init(command:"iphemeral_start",soc:iphemeralSoC,executable:iphemeralExecutable,rom:iphemeralROM),timeout:15) }
    func stopIPhemeral() { firmwareRuntime(.init(command:"iphemeral_stop")) }
    var filteredDyldImages:[DyldCacheImageRecord] { let q=dyldFilter.lowercased().trimmingCharacters(in:.whitespacesAndNewlines); guard let imgs=dyldCache?.images else{return[]}; return q.isEmpty ? Array(imgs.prefix(5000)) : Array(imgs.filter{$0.path.lowercased().contains(q)}.prefix(5000)) }
    var selectedFirmwareDevice: FirmwareDeviceRecord? { firmwareDevices.first{$0.identifier==firmwareDeviceID} }
    func useSelectedDeviceForEmulator() { if let d=selectedFirmwareDevice,let p=d.platform { iphemeralSoC=p }; workspace="Full-System iPhemeral" }
    func useSelectedDeviceForVectorEmu() { if let d=selectedFirmwareDevice,let p=d.platform { vectorEmuSoC=p }; workspace="VectorEmu Lab" }

    func choosePythonInterpreter() {
        let panel = NSOpenPanel(); panel.title = "Choose Python 3 interpreter"; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }; pythonInterpreter = url.path
    }
    func scriptingContextJSON() -> String {
        var selection: [String: Any] = ["workspace": workspace, "layout": layoutPreset, "address": addressText, "cfg_focus": cfgFocus, "symbol": selectedSymbol, "evidence": selectedEvidence]
        if let r = selected { selection["record_id"] = r.id; selection["record_name"] = r.name; selection["architecture"] = r.result.architecture }
        if let t = selectedUnifiedTarget {
            selection["target_id"] = t.id
            selection["target_kind"] = t.kind.rawValue
            selection["target_backend"] = t.backend
            selection["target_capabilities"] = t.capabilities.map(\.rawValue).sorted()
        }
        var analysis: [String: Any] = [:]
        if let a = currentAnalysis {
            func clip(_ x: String) -> String { String(x.prefix(65536)) }
            analysis = ["functions": clip(a.functions), "annotated_disassembly": clip(a.annotated_disassembly), "xrefs": clip(a.xrefs), "call_graph": clip(a.call_graph), "strings": clip(a.strings), "relocations": clip(a.relocations), "runtime_graph": clip(a.runtime_graph), "symbol_provenance": clip(a.symbol_provenance), "function_intelligence": clip(a.function_intelligence), "data_references": clip(a.data_references), "tail_call_candidates": clip(a.tail_call_candidates), "function_neighborhoods": clip(a.function_neighborhoods), "analysis_quality": clip(a.analysis_quality), "analysis_insights": clip(a.analysis_insights)]
        }
        var debugger: [String: Any] = ["attached": debugAttached, "event": debugEvent]
        if let d = debugSnapshot { debugger["pid"] = d.pid; debugger["pc"] = d.pc; debugger["sp"] = d.sp; debugger["fp"] = d.fp; debugger["lr"] = d.lr; debugger["stopped"] = d.stopped; debugger["selected_thread_id"] = d.selected_thread_id ?? ""; debugger["registers"] = d.registers }
        let root: [String: Any] = ["api_version": 2, "selection": selection, "analysis": analysis, "debugger": debugger]
        guard let data = try? JSONSerialization.data(withJSONObject: root, options: [.sortedKeys]), data.count <= 1024 * 1024 else { return "{\"api_version\":2}" }
        return String(decoding: data, as: UTF8.self)
    }
    func runPython() {
        guard pythonSource.utf8.count <= 256 * 1024 else { status = "Python source exceeds 256 KiB."; return }
        busy = true; status = "Running Python out-of-process…"; pythonOutput = ""; pythonError = ""; pythonResult = ""
        let context = scriptingContextJSON()
        client.request(timeout: UInt64(max(2, min(35, pythonTimeoutMS / 1000 + 3))), operation: { $0.scriptingRun(self.pythonSource, contextJSON: context, timeoutMS: self.pythonTimeoutMS, interpreter: self.pythonInterpreter, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false
            do { let reply = try JSONDecoder().decode(ScriptRunReply.self, from: Data(try result.get().utf8)); self.pythonOutput = reply.stdout; self.pythonError = reply.stderr; self.pythonResult = reply.result ?? ""; self.applyScriptActions(reply.actions ?? []); self.status = "\(reply.message) \(reply.duration_ms) ms • \(reply.actions?.count ?? 0) action(s)" } catch { self.status = error.localizedDescription }
        }
    }
    func refreshPlugins() {
        client.request(timeout: 5, operation: { $0.pluginList(withReply: $1) }) { [weak self] result in
            guard let self else { return }; do { let reply = try JSONDecoder().decode(PluginListReply.self, from: Data(try result.get().utf8)); self.plugins = reply.plugins; if !reply.plugins.contains(where: {$0.id == self.selectedPluginID}) { self.selectedPluginID = reply.plugins.first?.id ?? "" }; self.pluginStatus = reply.message } catch { self.pluginStatus = error.localizedDescription }
        }
    }
    func runSelectedPlugin() {
        guard !selectedPluginID.isEmpty else { status = "Select a plugin first."; return }
        busy = true; status = "Running plugin \(selectedPluginID)…"; pythonOutput = ""; pythonError = ""; pythonResult = ""
        let context = scriptingContextJSON()
        client.request(timeout: UInt64(max(2, min(35, pythonTimeoutMS / 1000 + 3))), operation: { $0.pluginRun(self.selectedPluginID, contextJSON: context, timeoutMS: self.pythonTimeoutMS, interpreter: self.pythonInterpreter, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false
            do { let reply = try JSONDecoder().decode(ScriptRunReply.self, from: Data(try result.get().utf8)); self.pythonOutput = reply.stdout; self.pythonError = reply.stderr; self.pythonResult = reply.result ?? ""; self.applyScriptActions(reply.actions ?? []); self.status = "\(reply.message) \(reply.duration_ms) ms • \(reply.actions?.count ?? 0) action(s)" } catch { self.status = error.localizedDescription }
        }
    }
    func setPluginSetting(_ setting: PluginSetting, valueJSON: String) {
        guard !selectedPluginID.isEmpty else { return }
        client.request(timeout: 5, operation: { $0.pluginSetSetting(self.selectedPluginID, key: setting.id, valueJSON: valueJSON, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do { let data=Data(try result.get().utf8); let obj=try JSONSerialization.jsonObject(with:data) as? [String:Any]; self.status=(obj?["message"] as? String) ?? "Setting updated."; self.refreshPlugins() } catch { self.status=error.localizedDescription }
        }
    }
    func setSelectedPluginEnabled(_ enabled: Bool) {
        guard !selectedPluginID.isEmpty else { return }
        client.request(timeout: 5, operation: { $0.pluginSetEnabled(self.selectedPluginID, enabled: enabled, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let data = Data(try result.get().utf8)
                let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
                self.status = (object?["message"] as? String) ?? "Plugin state updated."
                if (object?["ok"] as? Bool) == true {
                    let decision = enabled ? "enabled" : "disabled"
                    self.recordLedger(kind: .extensionStateChanged, provenance: .asserted, detail: "Extension " + self.selectedPluginID + " " + decision + " by the analyst.")
                }
                self.refreshPlugins()
            } catch { self.status = error.localizedDescription }
        }
    }
    func setSelectedPluginTrusted(_ trusted: Bool) {
        guard !selectedPluginID.isEmpty else { return }
        client.request(timeout: 5, operation: { $0.pluginSetTrusted(self.selectedPluginID, trusted: trusted, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let data = Data(try result.get().utf8)
                let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
                self.status = (object?["message"] as? String) ?? "Extension trust updated."
                if (object?["ok"] as? Bool) == true {
                    let decision = trusted ? "granted" : "revoked"
                    self.recordLedger(kind: .extensionTrustChanged, provenance: .asserted, detail: "Extension " + self.selectedPluginID + " trust " + decision + " by the analyst.")
                }
                self.refreshPlugins()
            } catch { self.status = error.localizedDescription }
        }
    }

    func installPlugin() {
        let panel = NSOpenPanel(); panel.title = "Install HindSight Extension Folder / .vgplugin Bundle"; panel.canChooseDirectories = true; panel.canChooseFiles = true; panel.treatsFilePackagesAsDirectories = true; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        client.request(timeout: 10, operation: { $0.pluginInstall(url.path, withReply: $1) }) { [weak self] result in
            guard let self else { return }; do { let data=Data(try result.get().utf8); let obj=try JSONSerialization.jsonObject(with:data) as? [String:Any]; self.status=(obj?["message"] as? String) ?? "Plugin install finished."; self.refreshPlugins() } catch { self.status=error.localizedDescription }
        }
    }

    func runPluginEvent(_ pluginID: String, event: String) {
        let context = scriptingContextJSON()
        client.request(timeout: UInt64(max(2, min(35, pythonTimeoutMS / 1000 + 3))), operation: { $0.pluginRunEvent(pluginID, event: event, contextJSON: context, timeoutMS: self.pythonTimeoutMS, interpreter: self.pythonInterpreter, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do { let reply = try JSONDecoder().decode(ScriptRunReply.self, from: Data(try result.get().utf8)); if reply.ok { self.pythonOutput = reply.stdout; self.pythonError = reply.stderr; self.pythonResult = reply.result ?? ""; self.applyScriptActions(reply.actions ?? []) } } catch { self.pluginStatus = error.localizedDescription }
        }
    }
    func triggerPluginEvent(_ event: String) {
        for plugin in plugins where plugin.enabled && plugin.events.contains(event) {
            if event == "on_stop", let filter = plugin.debugger_filter {
                if let signals = filter.signals, !signals.isEmpty { guard let signal=debugLastSignal,signals.contains(signal) else { continue } }
                if let reasons = filter.reasons, !reasons.isEmpty, !reasons.contains(where: { debugLastReason.localizedCaseInsensitiveContains($0) }) { continue }
            }
            runPluginEvent(plugin.id, event: event)
        }
    }
    func runPluginCommand(_ command: PluginCommand) { guard !selectedPluginID.isEmpty else { return }; runPluginEvent(selectedPluginID, event: command.event ?? "command.\(command.id)") }
    func runPluginAnalysisPass(_ pass: PluginAnalysisPass) { guard !selectedPluginID.isEmpty else { return }; runPluginEvent(selectedPluginID, event: pass.event ?? "analysis.pass.\(pass.id)") }
    func runPluginPanel(_ panel: PluginPanel) {
        guard !selectedPluginID.isEmpty else { return }
        activePluginPanelTitle = panel.title; activePluginPanelDock = panel.dock ?? "inspector"
        let context = scriptingContextJSON(); let event = panel.event ?? "panel.\(panel.id)"
        client.request(timeout: UInt64(max(2, min(35, pythonTimeoutMS / 1000 + 3))), operation: { $0.pluginRunEvent(self.selectedPluginID, event: event, contextJSON: context, timeoutMS: self.pythonTimeoutMS, interpreter: self.pythonInterpreter, withReply: $1) }) { [weak self] result in
            guard let self else { return }
            do {
                let reply = try JSONDecoder().decode(ScriptRunReply.self, from: Data(try result.get().utf8))
                self.pythonOutput = reply.stdout; self.pythonError = reply.stderr; self.pythonResult = reply.result ?? ""; self.applyScriptActions(reply.actions ?? [])
                self.activePluginPanelOutput = !reply.stdout.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? reply.stdout : (reply.result ?? reply.stderr)
                self.status = reply.message
            } catch { self.pluginStatus = error.localizedDescription }
        }
    }
    func closePluginPanel() { activePluginPanelTitle = ""; activePluginPanelOutput = "" }
    func applyScriptActions(_ actions: [ScriptAction]) {
        for action in actions.prefix(128) {
            switch action.kind {
            case "navigation":
                if let value = action.workspace, !value.isEmpty { workspace = value }
                if let value = action.address, !value.isEmpty { addressText = value }
                if let value = action.cfg_focus { cfgFocus = value }
                if let value = action.symbol { selectedSymbol = value }
            case "breakpoint.set":
                if let value = action.address { debugAddress = value; debugCommand("breakpoint.set", address: value) }
            case "breakpoint.remove":
                if let value = action.address { debugAddress = value; debugCommand("breakpoint.remove", address: value) }
            case "debugger.continue": debugContinue()
            case "debugger.interrupt": debugInterrupt()
            case "debugger.step": debugStep()
            case "notes.append":
                if let text = action.text, selected != nil { noteText = String((noteText + (noteText.isEmpty ? "" : "\n") + text).prefix(4096)); saveNote() }
            default: break
            }
        }
    }

    func saveNote() {
        guard !busy, projectServiceReady, let selected else { return }
        let text = noteText
        guard text.utf8.count <= 4096 else { status = "Note exceeds 4096 UTF-8 bytes."; return }
        busy = true
        client.request(operation: { $0.saveNote(selected.id, expectedRevision: selected.revision, text: text, withReply: $1) }) { [weak self] result in
            guard let self else { return }; self.busy = false
            if self.apply(result) {
                self.drafts.removeValue(forKey: selected.id)
                self.recordLedger(kind: .noteSaved, provenance: .asserted, detail: "Saved analyst note for \(selected.name).")
            }
        }
    }
    @discardableResult
    private func apply(_ result: Result<String, Error>) -> Bool {
        do {
            let payload = try result.get()
            connected = true
            connectingToCoordinator = false
            let reply = try JSONDecoder().decode(ServiceReply.self, from: Data(payload.utf8))
            guard reply.ok, let snapshot = reply.project else { status = reply.message; return false }
            let projectChanged = project?.path != snapshot.path
            if projectChanged {
                instructionPage = nil; analysisReport = nil; pageRecordID = nil; analysisRecordID = nil; navigationGeneration = UUID()
                analysisReportsByRecordID.removeAll()
                diffBaselineID = snapshot.records.first?.id ?? ""
                diffCandidateID = snapshot.records.dropFirst().first?.id ?? ""
                diffStatus = snapshot.records.count >= 2 ? "Analyze both sides to build an evidence-backed diff." : "Import a second artifact to compare builds."
            }
            project = snapshot; connected = true; projectSessionReady = true; restoringProjectSession = false; startupPhase = "Project ready; optional services on demand"; if projectChanged { loadAnalysisContinuity(projectPath: snapshot.path) }
            let validRecordIDs = Set(snapshot.records.map(\.id))
            analysisReportsByRecordID = analysisReportsByRecordID.filter { validRecordIDs.contains($0.key) }
            if diffBaselineID.isEmpty { diffBaselineID = snapshot.records.first?.id ?? "" }
            if diffCandidateID.isEmpty { diffCandidateID = snapshot.records.first(where: { $0.id != diffBaselineID })?.id ?? "" }
            if projectChanged { loadInvestigationLedger(for: snapshot) }
            if !snapshot.records.contains(where: {$0.id == selectedID}) { selectedID = snapshot.records.first?.id }
            persistLaunchSessionIfPossible()
            status = reply.message.isEmpty ? "Project open." : reply.message
            return true
        } catch { status = error.localizedDescription; return false }
    }
    private func ledgerContext() -> VGLedgerContext {
        VGLedgerContext(
            recordID: selected?.id,
            recordName: selected?.name,
            sourceSHA256: selected?.result.source_sha256,
            workspace: workspace,
            address: addressText.isEmpty ? nil : addressText,
            symbol: selectedSymbol.isEmpty ? nil : selectedSymbol,
            targetID: selectedTargetID.isEmpty ? nil : selectedTargetID
        )
    }

    private func loadInvestigationLedger(for snapshot: ProjectSnapshot) {
        let name = URL(fileURLWithPath: snapshot.path, isDirectory: true).lastPathComponent
        do {
            let loaded = try VGLedgerStore.loadRecovering(projectPath: snapshot.path, projectName: name)
            investigationLedger = loaded.ledger
            if loaded.recoveredFromBackup {
                ledgerStatus = "Recovered the case ledger from its last known-good backup. The primary ledger will be healed on the next save."
                persistLedger()
            } else {
                ledgerStatus = "Persistent case ledger loaded."
            }
            recordLedger(kind: .projectOpened, provenance: .observed, detail: "Opened project \(name).")
        } catch {
            investigationLedger = VGInvestigationLedger(projectName: name)
            ledgerStatus = "Started a new ledger because the existing ledger could not be loaded: \(error.localizedDescription)"
            persistLedger()
        }
    }

    private func persistLedger() {
        guard let ledger = investigationLedger, let path = project?.path else { return }
        do {
            try VGLedgerStore.save(ledger, projectPath: path)
            ledgerStatus = "Ledger saved · \(ledger.entries.count) timeline events."
        } catch {
            ledgerStatus = "Ledger save failed: \(error.localizedDescription)"
        }
    }

    func recordLedger(kind: VGLedgerEntryKind, provenance: VGLedgerProvenance, detail: String, title: String? = nil) {
        guard !suppressLedgerRecording, var ledger = investigationLedger else { return }
        let entry = VGLedgerEntry(kind: kind, provenance: provenance, title: title, detail: String(detail.prefix(4096)), context: ledgerContext())
        ledger.append(entry)
        investigationLedger = ledger
        selectedLedgerEntryID = entry.id
        persistLedger()
    }

    func createHypothesis(title: String, detail: String) {
        let clean = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty, var ledger = investigationLedger else { return }
        let address = addressText.isEmpty ? nil : addressText
        let hypothesis = ledger.addHypothesis(title: clean, detail: detail, address: address, linkedEntryID: selectedLedgerEntryID.isEmpty ? nil : selectedLedgerEntryID)
        investigationLedger = ledger
        persistLedger()
        recordLedger(kind: .hypothesisCreated, provenance: .asserted, detail: "Proposed hypothesis: \(hypothesis.title)")
    }

    func setHypothesisStatus(_ id: String, _ status: VGHypothesisStatus) {
        guard var ledger = investigationLedger, let updated = ledger.setHypothesisStatus(id: id, status: status) else { return }
        investigationLedger = ledger
        persistLedger()
        recordLedger(kind: .hypothesisUpdated, provenance: .asserted, detail: "Hypothesis ‘\(updated.title)’ marked \(status.title).")
    }

    func addLedgerBookmark(title: String, note: String) {
        guard var ledger = investigationLedger else { return }
        let bookmark = VGCaseBookmark(
            title: String(title.prefix(160)),
            note: String(note.prefix(2048)),
            workspace: workspace,
            address: addressText.isEmpty ? nil : addressText,
            symbol: selectedSymbol.isEmpty ? nil : selectedSymbol,
            recordID: selectedID
        )
        ledger.addBookmark(bookmark)
        investigationLedger = ledger
        persistLedger()
        recordLedger(kind: .bookmarkCreated, provenance: .asserted, detail: "Bookmarked \(bookmark.workspace)\(bookmark.address.map { " at " + $0 } ?? ""): \(bookmark.title)")
    }

    func captureInvestigationSnapshot(title: String) {
        guard var ledger = investigationLedger else { return }
        let snapshot = VGInvestigationSnapshot(
            title: String(title.prefix(160)),
            workspace: workspace,
            layoutPreset: layoutPreset,
            address: addressText,
            cfgFocus: cfgFocus,
            analysisSearch: analysisSearch,
            selectedSymbol: selectedSymbol,
            selectedRecordID: selectedID,
            selectedTargetID: selectedTargetID,
            evidence: String(selectedEvidence.prefix(4096))
        )
        ledger.addSnapshot(snapshot)
        investigationLedger = ledger
        persistLedger()
        recordLedger(kind: .snapshotCreated, provenance: .asserted, detail: "Captured snapshot ‘\(snapshot.title)’ in \(snapshot.workspace).")
    }

    func restoreInvestigationSnapshot(_ snapshot: VGInvestigationSnapshot) {
        suppressLedgerRecording = true
        if let recordID = snapshot.selectedRecordID, project?.records.contains(where: { $0.id == recordID }) == true { selectedID = recordID }
        layoutPreset = snapshot.layoutPreset
        workspace = snapshot.workspace
        addressText = snapshot.address
        cfgFocus = snapshot.cfgFocus
        analysisSearch = snapshot.analysisSearch
        selectedSymbol = snapshot.selectedSymbol
        selectedTargetID = snapshot.selectedTargetID
        selectedEvidence = snapshot.evidence
        suppressLedgerRecording = false
        recordLedger(kind: .snapshotRestored, provenance: .asserted, detail: "Restored snapshot ‘\(snapshot.title)’. Live target state was not rewound.")
    }

    func openLedgerContext(_ context: VGLedgerContext) {
        suppressLedgerRecording = true
        if let recordID = context.recordID, project?.records.contains(where: { $0.id == recordID }) == true { selectedID = recordID }
        workspace = context.workspace
        if let address = context.address { addressText = address; analysisSearch = address; cfgFocus = address }
        if let symbol = context.symbol { selectedSymbol = symbol }
        if let targetID = context.targetID { selectedTargetID = targetID }
        suppressLedgerRecording = false
        recordLedger(kind: .workspaceVisited, provenance: .asserted, detail: "Reopened context from the investigation ledger.")
    }

    private func caseBundle() -> VGCaseBundle? {
        guard let ledger = investigationLedger, let project else { return nil }
        let artifacts = project.records.map { record in
            VGCaseArtifactIdentity(id: record.id, name: record.name, sourceSHA256: record.result.source_sha256, architecture: record.result.architecture, size: record.result.size)
        }
        let evidence = currentAnalysis.map {
            VGCaseEvidenceSnapshot(
                summary: $0.summary,
                functions: $0.functions,
                xrefs: $0.xrefs,
                callGraph: $0.call_graph,
                cfg: $0.cfg,
                ir: $0.ir,
                symbolProvenance: $0.symbol_provenance,
                runtimeGraph: $0.runtime_graph,
                annotatedDisassembly: $0.annotated_disassembly
            )
        }
        let runtime = runtimeSamples.map {
            VGCaseRuntimeEvent(
                sequence: $0.id,
                timestamp: $0.timestamp,
                event: $0.event,
                reason: $0.reason,
                threadID: $0.threadID,
                stopped: $0.stopped,
                pc: $0.pcText,
                sp: $0.sp.map(VGTargetUtilities.hex),
                fp: $0.fp.map(VGTargetUtilities.hex),
                lr: $0.lr.map(VGTargetUtilities.hex),
                registers: $0.registers,
                breakpoints: $0.breakpoints.map(VGTargetUtilities.hex)
            )
        }
        let memory = runtimeMemoryObservations.map {
            VGCaseMemoryObservation(sequence: $0.id, timestamp: $0.timestamp, address: $0.addressText, bytesHex: $0.bytes.map { String(format: "%02x", $0) }.joined())
        }
        return VGCaseBundle(
            format: "com.zensoftware.vectorglass.case",
            schema: 1,
            product: VGProduct.displayName,
            productVersion: VGProduct.version,
            exportedAt: Date(),
            ledger: ledger,
            artifacts: artifacts,
            evidence: evidence,
            runtime: runtime,
            memory: memory,
            integrity: nil
        )
    }

    private func exportData(_ data: Data, suggestedName: String, title: String) {
        let panel = NSSavePanel()
        panel.title = title
        panel.nameFieldStringValue = suggestedName
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try data.write(to: url, options: .atomic)
            recordLedger(kind: .caseExported, provenance: .asserted, detail: "Exported \(url.lastPathComponent).")
            status = "Exported \(url.lastPathComponent)."
        } catch { status = "Export failed: \(error.localizedDescription)" }
    }

    func exportInvestigationCase() {
        guard let bundle = caseBundle() else { return }
        do {
            let data = try VGLedgerStore.encodeCase(bundle)
            guard data.count <= 32 * 1024 * 1024 else { status = "Case bundle exceeds the 32 MiB export limit."; return }
            let base = bundle.ledger.projectName.replacingOccurrences(of: "/", with: "-")
            exportData(data, suggestedName: "\(base).vgcase", title: "Export HindSight Case")
        } catch { status = "Case encoding failed: \(error.localizedDescription)" }
    }

    func importInvestigationCase() {
        let panel = NSOpenPanel()
        panel.title = "Import HindSight Case"
        if let caseType = UTType(filenameExtension: "vgcase") {
            panel.allowedContentTypes = [caseType]
        }
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            let data = try Data(contentsOf: url, options: [.mappedIfSafe])
            let bundle = try VGLedgerStore.decodeCase(data)
            let restored = bundle.runtime.compactMap { event in
                VGRuntimeSample.restore(event, previous: runtimeSamples.last)
            }
            guard restored.count == bundle.runtime.count else {
                throw NSError(domain: "HindSightCase", code: 4, userInfo: [NSLocalizedDescriptionKey: "The case contains runtime events with invalid addresses."])
            }
            func decodeHex(_ value: String) -> [UInt8]? {
                let chars = Array(value)
                guard chars.count <= 8192, chars.count.isMultiple(of: 2) else { return nil }
                return stride(from: 0, to: chars.count, by: 2).compactMap { index in
                    UInt8(String(chars[index...index + 1]), radix: 16)
                }.count == chars.count / 2 ? stride(from: 0, to: chars.count, by: 2).compactMap { index in UInt8(String(chars[index...index + 1]), radix: 16) } : nil
            }
            let memory = bundle.memory.compactMap { observation -> VGRuntimeMemoryObservation? in
                guard let address = VGTargetUtilities.parseHex(observation.address), let bytes = decodeHex(observation.bytesHex) else { return nil }
                return VGRuntimeMemoryObservation(id: observation.sequence, timestamp: observation.timestamp, address: address, bytes: bytes)
            }
            guard memory.count == bundle.memory.count else {
                throw NSError(domain: "HindSightCase", code: 5, userInfo: [NSLocalizedDescriptionKey: "The case contains invalid memory-observation bytes."])
            }
            runtimeSamples = restored
            runtimeMemoryObservations = memory
            runtimeSequence = restored.map(\.id).max() ?? 0
            runtimeMemorySequence = memory.map(\.id).max() ?? 0
            if let artifact = bundle.artifacts.first, project?.records.contains(where: { $0.id == artifact.id }) == true { selectedID = artifact.id }
            workspace = "Trace Explorer"
            ledgerStatus = "Imported \(restored.count) runtime observations from \(url.lastPathComponent); debugger remains detached."
            status = ledgerStatus
        } catch { status = "Case import failed: \(error.localizedDescription)" }
    }

    func exportInvestigationMarkdown() {
        guard let bundle = caseBundle() else { return }
        let data = Data(VGLedgerStore.markdown(bundle).utf8)
        exportData(data, suggestedName: "\(bundle.ledger.projectName)-investigation.md", title: "Export Investigation Markdown")
    }

    func exportInvestigationPDF() {
        guard let bundle = caseBundle() else { return }
        let data = VGLedgerStore.pdf(bundle)
        exportData(data, suggestedName: "\(bundle.ledger.projectName)-investigation.pdf", title: "Export Investigation PDF")
    }

    func exportInvestigationJSON() {
        guard let ledger = investigationLedger else { return }
        do {
            let encoder = JSONEncoder(); encoder.dateEncodingStrategy = .iso8601; encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
            let data = try encoder.encode(ledger)
            exportData(data, suggestedName: "\(ledger.projectName)-ledger.json", title: "Export Investigation Ledger")
        } catch { status = "Ledger encoding failed: \(error.localizedDescription)" }
    }

    func diagnosticsText() -> String {
        let process = ProcessInfo.processInfo
        let projectName = project.map { URL(fileURLWithPath: $0.path, isDirectory: true).lastPathComponent } ?? "none"
        let artifactCount = project?.records.count ?? 0
        let selectedName = selected?.name ?? "none"
        let analysisPhase = analysisJobStatus?.phase.rawValue ?? "none"
        let analysisStage = analysisJobStatus?.progress_stage ?? "none"
        let analysisProgress = analysisJobStatus?.progress_percent ?? 0
        return [
            "\(VGProduct.displayName) diagnostics",
            "product_version=\(VGProduct.version)",
            "release_channel=\(VGProduct.releaseChannel)",
            "host_architecture=\(VGHostPlatform.architecture.rawValue)",
            "host_platform=\(VGHostPlatform.displayName)",
            "apple_silicon_hypervisor_available=\(VGHostPlatform.supportsAppleSiliconHypervisor)",
            "apple_silicon_hypervisor_status=\(VGHostPlatform.hypervisorStatus)",
            "arm64_native_debugger_available=\(VGHostPlatform.supportsArm64NativeDebugger)",
            "arm64_native_debugger_status=\(VGHostPlatform.nativeDebuggerStatus)",
            "vectoremu_host_available=\(VGHostPlatform.supportsVectorEmu)",
            "vectoremu_host_status=\(VGHostPlatform.vectorEmuStatus)",
            "os=\(process.operatingSystemVersionString)",
            "physical_memory_bytes=\(process.physicalMemory)",
            "processor_count=\(process.processorCount)",
            "coordinator_connected=\(connected)",
            "coordinator_version=\(coordinatorVersion.isEmpty ? "unknown" : coordinatorVersion)",
            "coordinator_abi=\(coordinatorABI)",
            "project_session_ready=\(projectSessionReady)",
            "startup_phase=\(startupPhase)",
            "startup_pressure=\(startupPressure)",
            "active_request=\(activeRequest ?? "none")",
            "saved_launch_session=\(savedLaunchSessionAvailable)",
            "saved_launch_project=\(savedLaunchProjectName.isEmpty ? "none" : savedLaunchProjectName)",
            "project=\(projectName)",
            "artifact_count=\(artifactCount)",
            "selected_artifact=\(selectedName)",
            "workspace=\(workspace)",
            "analysis_pins=\(analysisPins.count)",
            "analysis_recent=\(analysisRecentVisits.count)",
            "analysis_collections=\(analysisCollections.count)",
            "analysis_review_baselines=\(analysisReviewBaselines.count)",
            "analysis_review_entries=\(analysisReviewEntries.count)",
            "analysis_follow_ups=\(analysisFollowUps.count)",
            "analysis_open_follow_ups=\(analysisFollowUps.filter { $0.state == .open }.count)",
            "analysis_delta=\(analysisDelta?.summary ?? "none")",
            "busy=\(busy)",
            "analysis_phase=\(analysisPhase)",
            "analysis_stage=\(analysisStage)",
            "analysis_progress_percent=\(analysisProgress)",
            "debug_attached=\(debugAttached)",
            "vectoremu_running=\(vectorEmuRunning)",
            "status=\(status.replacingOccurrences(of: "\n", with: " "))"
        ].joined(separator: "\n")
    }

    func copyDiagnostics() {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(diagnosticsText(), forType: .string)
        status = "Diagnostics copied to the clipboard."
    }

    func exportDiagnostics() {
        let panel = NSSavePanel()
        panel.title = "Export HindSight Diagnostics"
        panel.nameFieldStringValue = "HindSight-diagnostics.txt"
        panel.allowedContentTypes = [.plainText]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try Data(diagnosticsText().utf8).write(to: url, options: .atomic)
            status = "Diagnostics exported to (url.lastPathComponent)."
        } catch {
            status = "Diagnostics export failed: (error.localizedDescription)"
        }
    }

    private func firstHexAddress(in text: String) -> UInt64? {
        guard let range = text.range(
            of: #"(?i)\b0x[0-9a-f]{1,16}\b"#,
            options: .regularExpression
        ) else { return nil }
        return UInt64(text[range].dropFirst(2), radix: 16)
    }

    func selectEvidence(_ line: String) {
        selectedEvidence = line
        loadSelectedEvidenceAnnotation()
        if let address = firstHexAddress(in: line) { addressText = String(format: "0x%llx", address); cfgFocus = String(format: "0x%llx", address) }
        if let range = line.range(of: "symbol=") { let tail = line[range.upperBound...]; selectedSymbol = String(tail.prefix { !$0.isWhitespace }) }
        else if line.hasPrefix("symbol ") { selectedSymbol = String(line.dropFirst(7).prefix { !$0.isWhitespace }) }
        if !selectedSymbol.isEmpty { triggerPluginEvent("on_symbol_selected") }
    }

    private func loadSelectedEvidenceAnnotation() {
        guard let recordID = selectedID, !selectedEvidence.isEmpty else {
            selectedEvidenceAnnotation = nil
            annotationDraft = ""
            annotationTagsText = ""
            annotationConfidence = .unclassified
            return
        }
        let id = "\(recordID)|\(vgContinuityFingerprint(selectedEvidence))"
        let annotation = analysisContinuityDocument.annotations[id]
        selectedEvidenceAnnotation = annotation
        annotationDraft = annotation?.note ?? ""
        annotationTagsText = annotation?.tags.joined(separator: ", ") ?? ""
        annotationConfidence = annotation?.confidence ?? .unclassified
    }

    func saveSelectedEvidenceAnnotation() {
        guard let recordID = selectedID, let projectPath = project?.path, !selectedEvidence.isEmpty else { return }
        let tags = annotationTagsText.split(separator: ",").map(String.init)
        let annotation = VGAnalysisAnnotation(recordID: recordID, evidence: selectedEvidence, note: annotationDraft, tags: tags, confidence: annotationConfidence)
        let isEmpty = annotation.note.isEmpty && annotation.tags.isEmpty && annotation.confidence == .unclassified
        if isEmpty {
            analysisContinuityDocument.annotations.removeValue(forKey: annotation.id)
            selectedEvidenceAnnotation = nil
            status = "Removed evidence annotation."
        } else {
            analysisContinuityDocument.annotations[annotation.id] = annotation
            selectedEvidenceAnnotation = annotation
            annotationDraft = annotation.note
            annotationTagsText = annotation.tags.joined(separator: ", ")
            status = "Saved evidence annotation."
        }
        persistAnalysisContinuity(projectPath: projectPath)
    }
    func symbolEvidence(_ text: String) -> String {
        let q = selectedSymbol.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !q.isEmpty else { return "Select a symbol-bearing evidence row to populate this pane." }
        let lines = text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init).filter { $0.localizedCaseInsensitiveContains(q) }
        return lines.isEmpty ? "No evidence for \(q) in this channel." : lines.prefix(256).joined(separator: "\n")
    }

}
