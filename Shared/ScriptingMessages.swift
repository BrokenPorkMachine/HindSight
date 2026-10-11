import Foundation

struct ScriptAction: Codable, Identifiable {
    let id: String
    let kind: String
    let workspace: String?
    let address: String?
    let cfg_focus: String?
    let symbol: String?
    let text: String?
    let key: String?
    let value_json: String?
    let category: String?
    init(kind: String, workspace: String? = nil, address: String? = nil, cfg_focus: String? = nil, symbol: String? = nil, text: String? = nil, key: String? = nil, value_json: String? = nil, category: String? = nil) {
        self.id = UUID().uuidString; self.kind = kind; self.workspace = workspace; self.address = address; self.cfg_focus = cfg_focus; self.symbol = symbol; self.text = text; self.key = key; self.value_json = value_json; self.category = category
    }
}

struct ScriptRunReply: Codable {
    let ok: Bool
    let stdout: String
    let stderr: String
    let result: String?
    let message: String
    let duration_ms: Int
    let actions: [ScriptAction]?
}

struct PluginCommand: Codable, Identifiable {
    let id: String
    let title: String
    let shortcut: String?
    let event: String?
}

struct PluginPanel: Codable, Identifiable {
    let id: String
    let title: String
    let event: String?
    let description: String?
    let dock: String?
    let min_width: Int?
}

struct PluginSetting: Codable, Identifiable {
    let id: String
    let title: String
    let type: String
    let default_value: String?
    let choices: [String]?
    let description: String?
}

struct PluginAnalysisPass: Codable, Identifiable {
    let id: String
    let title: String
    let event: String?
    let description: String?
}

struct PluginDebuggerFilter: Codable {
    let signals: [Int]?
    let reasons: [String]?
}


struct PluginDependency: Codable, Identifiable {
    var id: String { plugin_id }
    let plugin_id: String
    let minimum_version: String?
}

struct PluginAnnotation: Codable, Identifiable {
    let id: String
    let address: String?
    let category: String
    let text: String
    let created_at: String
}

struct PluginInfo: Codable, Identifiable {
    let id: String
    let name: String
    let version: String
    let api_version: Int
    let entry: String
    let capabilities: [String]
    let events: [String]
    let commands: [PluginCommand]
    let panels: [PluginPanel]
    let settings: [PluginSetting]
    let settings_values: [String: String]
    let analysis_passes: [PluginAnalysisPass]
    let debugger_filter: PluginDebuggerFilter?
    let package_format: String
    let dependencies: [PluginDependency]
    let dependency_status: String
    let signature_status: String
    let trusted: Bool
    let annotations: [PluginAnnotation]
    let custom_data_keys: [String]
    let path: String
    let source: String
    let enabled: Bool
}

struct PluginListReply: Codable {
    let ok: Bool
    let plugins: [PluginInfo]
    let message: String
}
