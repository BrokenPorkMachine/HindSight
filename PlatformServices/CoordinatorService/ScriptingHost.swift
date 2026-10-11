import Foundation
import Darwin
import CryptoKit

/// Bounded, out-of-process Python host and manifest-driven extension manager.
/// Python never runs inside the SwiftUI process. Mutation requests are capability-gated,
/// returned as actions, and either persisted by this host or applied by the app after exit.
final class ScriptingHost {
    static let apiVersion = 2
    static let supportedAPIVersions = Set([1, 2])
    static let maxSourceBytes = 256 * 1024
    static let maxContextBytes = 1024 * 1024
    static let maxOutputBytes = 1024 * 1024
    static let maxPlugins = 256
    static let maxActions = 128
    static let maxPersistentBytes = 64 * 1024
    static let maxPluginPackageBytes = 16 * 1024 * 1024
    static let maxPluginPackageEntries = 2048
    static let governedCapabilities: Set<String> = [
        "selection.read", "analysis.read", "debugger.read", "settings.read", "state.read",
        "annotation.read", "data.read", "navigation.write", "breakpoint.write", "debugger.control",
        "notes.write", "settings.write", "state.write", "annotation.write", "data.write",
        "analysis.custom"
    ]
    private let fm = FileManager.default
    // Fixed stripes bound lock-table memory even when an untrusted XPC client
    // sends arbitrarily many distinct plugin IDs.
    private let pluginLocks: [NSRecursiveLock] = (0..<64).map { _ in NSRecursiveLock() }

    private func pluginLock(for id: String) -> NSRecursiveLock {
        let bucket = UInt(bitPattern: id.hashValue) % UInt(pluginLocks.count)
        return pluginLocks[Int(bucket)]
    }

    private final class BoundedProcessOutput: @unchecked Sendable {
        private let lock = NSLock()
        private var stdout = Data()
        private var stderr = Data()
        func appendStdout(_ data: Data, limit: Int) {
            lock.lock(); defer { lock.unlock() }
            guard stdout.count < limit else { return }
            stdout.append(data.prefix(limit - stdout.count))
        }
        func appendStderr(_ data: Data, limit: Int) {
            lock.lock(); defer { lock.unlock() }
            guard stderr.count < limit else { return }
            stderr.append(data.prefix(limit - stderr.count))
        }
        func snapshot() -> (Data, Data) {
            lock.lock(); defer { lock.unlock() }
            return (stdout, stderr)
        }
    }

    private struct Manifest: Codable {
        let id: String
        let name: String
        let version: String
        let api_version: Int
        let entry: String
        let capabilities: [String]?
        let events: [String]?
        let commands: [PluginCommand]?
        let panels: [PluginPanel]?
        let settings: [PluginSetting]?
        let analysis_passes: [PluginAnalysisPass]?
        let debugger_filter: PluginDebuggerFilter?
        let dependencies: [PluginDependency]?
    }

    private struct SignatureFile: Codable {
        let algorithm: String
        let public_key_base64: String
        let signature_base64: String
        let signed_files: [String]
    }

    private let runner = #"""
import json, sys, types, traceback, uuid
REQ=json.load(sys.stdin)
CTX=REQ.get("context") or {}
CAP=set(REQ.get("capabilities") or [])
EVENT=REQ.get("event") or "manual"
ACTIONS=[]
def action(kind, **kw):
    if len(ACTIONS) >= 128: raise RuntimeError("automation action limit reached")
    item={"id":str(uuid.uuid4()),"kind":kind}
    item.update({k:v for k,v in kw.items() if v is not None})
    ACTIONS.append(item)
class API:
    api_version=2
    context=CTX
    event=EVENT
    capabilities=frozenset(CAP)
    def require(self, capability):
        if capability not in CAP: raise PermissionError("plugin capability not granted: "+str(capability))
    @property
    def selection(self): self.require("selection.read"); return CTX.get("selection", {})
    @property
    def analysis(self): self.require("analysis.read"); return CTX.get("analysis", {})
    @property
    def debugger(self): self.require("debugger.read"); return CTX.get("debugger", {})
    @property
    def settings(self): self.require("settings.read"); return (CTX.get("plugin") or {}).get("settings", {})
    @property
    def state(self): self.require("state.read"); return (CTX.get("plugin") or {}).get("state", {})
    @property
    def annotations(self): self.require("annotation.read"); return (CTX.get("plugin") or {}).get("annotations", [])
    @property
    def custom_data(self): self.require("data.read"); return (CTX.get("plugin") or {}).get("custom_data", {})
    def navigate(self, workspace=None, address=None, cfg_focus=None, symbol=None):
        self.require("navigation.write"); action("navigation",workspace=workspace,address=address,cfg_focus=cfg_focus,symbol=symbol)
    def breakpoint_set(self, address): self.require("breakpoint.write"); action("breakpoint.set",address=str(address))
    def breakpoint_remove(self, address): self.require("breakpoint.write"); action("breakpoint.remove",address=str(address))
    def debugger_continue(self): self.require("debugger.control"); action("debugger.continue")
    def debugger_interrupt(self): self.require("debugger.control"); action("debugger.interrupt")
    def debugger_step(self): self.require("debugger.control"); action("debugger.step")
    def note_append(self, text): self.require("notes.write"); action("notes.append",text=str(text)[:4096])
    def setting_set(self, key, value):
        self.require("settings.write"); action("plugin.setting.set",key=str(key)[:128],value_json=json.dumps(value,ensure_ascii=False,separators=(",",":")))
    def state_set(self, key, value):
        self.require("state.write"); action("plugin.state.set",key=str(key)[:128],value_json=json.dumps(value,ensure_ascii=False,separators=(",",":")))
    def annotate(self, text, address=None, category="note"):
        self.require("annotation.write"); action("annotation.add",text=str(text)[:4096],address=None if address is None else str(address)[:64],category=str(category)[:64])
    def custom_data_set(self, key, value):
        self.require("data.write"); action("plugin.data.set",key=str(key)[:128],value_json=json.dumps(value,ensure_ascii=False,separators=(",",":")))
    def emit(self, kind, payload=None): print(json.dumps({"event":str(kind),"payload":payload}, sort_keys=True))
rw=API()
mod=types.ModuleType("hindsight"); mod.api=rw; mod.context=CTX; mod.API_VERSION=2
sys.modules["hindsight"]=mod
sys.modules["vectorglass"]=mod
# Compatibility aliases for existing extensions.
sys.modules["reworkbench"]=mod
result=None
try:
    glb={"__name__":"__hindsight_script__","__file__":REQ.get("filename","<script>"),"rw":rw}
    exec(compile(REQ.get("source", ""), glb["__file__"], "exec"), glb, glb)
    result=glb.get("result")
    try: json.dumps(result)
    except Exception: result=repr(result)
    print("__REWORKBENCH_RESULT__"+json.dumps(result, ensure_ascii=False))
    print("__REWORKBENCH_ACTIONS__"+json.dumps(ACTIONS, ensure_ascii=False))
except BaseException:
    traceback.print_exc(); sys.exit(70)
"""#

    func run(source: String, contextJSON: String, capabilities: [String], timeoutMS: Int, filename: String = "<console>", interpreter: String = "", event: String = "manual") -> String {
        let start = DispatchTime.now()
        guard source.utf8.count <= Self.maxSourceBytes else { return encodeRun(ok:false,stderr:"",message:"Python source exceeds 256 KiB.",duration:0) }
        guard contextJSON.utf8.count <= Self.maxContextBytes else { return encodeRun(ok:false,stderr:"",message:"Script context exceeds 1 MiB.",duration:0) }
        guard let contextData=contextJSON.data(using:.utf8), let context=try? JSONSerialization.jsonObject(with:contextData) else { return encodeRun(ok:false,stderr:"",message:"Invalid script context JSON.",duration:0) }
        let request:[String:Any]=["source":source,"context":context,"capabilities":capabilities,"filename":filename,"event":String(event.prefix(128))]
        guard let input=try? JSONSerialization.data(withJSONObject:request) else { return encodeRun(ok:false,stderr:"",message:"Could not encode script request.",duration:0) }
        let executable:URL; let arguments:[String]
        if !interpreter.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty { executable=URL(fileURLWithPath:interpreter); arguments=["-I","-c",runner] }
        else { executable=URL(fileURLWithPath:"/usr/bin/env"); arguments=["python3","-I","-c",runner] }
        let p=Process(); p.executableURL=executable; p.arguments=arguments
        let stdin=Pipe(), stdout=Pipe(), stderr=Pipe(); p.standardInput=stdin; p.standardOutput=stdout; p.standardError=stderr
        let output = BoundedProcessOutput()
        stdout.fileHandleForReading.readabilityHandler={ h in output.appendStdout(h.availableData, limit: Self.maxOutputBytes) }
        stderr.fileHandleForReading.readabilityHandler={ h in output.appendStderr(h.availableData, limit: Self.maxOutputBytes) }
        let sem=DispatchSemaphore(value:0); p.terminationHandler={_ in sem.signal()}
        do { try p.run() } catch { return encodeRun(ok:false,stderr:"",message:"Python 3 could not be launched: \(error.localizedDescription).",duration:0) }
        stdin.fileHandleForWriting.write(input); try? stdin.fileHandleForWriting.close()
        let timeout = max(250, min(timeoutMS, 30_000))
        let deadline = DispatchTime.now() + DispatchTimeInterval.milliseconds(timeout)
        let timedOut = sem.wait(timeout: deadline) == .timedOut
        if timedOut {
            p.terminate()
            let terminationDeadline = DispatchTime.now() + DispatchTimeInterval.seconds(1)
            _ = sem.wait(timeout: terminationDeadline)
            if p.isRunning {
                _ = kill(p.processIdentifier, SIGKILL)
                _ = sem.wait(timeout: DispatchTime.now() + DispatchTimeInterval.seconds(1))
            }
        }
        stdout.fileHandleForReading.readabilityHandler=nil; stderr.fileHandleForReading.readabilityHandler=nil
        output.appendStdout(stdout.fileHandleForReading.readDataToEndOfFile(), limit: Self.maxOutputBytes)
        output.appendStderr(stderr.fileHandleForReading.readDataToEndOfFile(), limit: Self.maxOutputBytes)
        let (out, err) = output.snapshot()
        var outText=String(decoding:out,as:UTF8.self); let errText=String(decoding:err,as:UTF8.self); var result:String?; var actions:[ScriptAction]=[]
        if let r=outText.range(of:"__REWORKBENCH_ACTIONS__",options:.backwards) { let tail=String(outText[r.upperBound...]).split(separator:"\n",maxSplits:1).first.map(String.init) ?? "[]"; if let d=tail.data(using:.utf8), let decoded=try? JSONDecoder().decode([ScriptAction].self,from:d) { actions=Array(decoded.prefix(Self.maxActions)) }; outText.removeSubrange(r.lowerBound..<outText.endIndex) }
        if let r=outText.range(of:"__REWORKBENCH_RESULT__",options:.backwards) { let tail=String(outText[r.upperBound...]).split(separator:"\n",maxSplits:1).first.map(String.init) ?? "null"; if let d=tail.data(using:.utf8), let obj=try? JSONSerialization.jsonObject(with:d), let rendered=try? JSONSerialization.data(withJSONObject:obj,options:[.prettyPrinted,.sortedKeys]) { result=String(decoding:rendered,as:UTF8.self) }; outText.removeSubrange(r.lowerBound..<outText.endIndex) }
        let ms=Int((DispatchTime.now().uptimeNanoseconds-start.uptimeNanoseconds)/1_000_000)
        if p.isRunning {
            return encodeRun(ok:false,stdout:outText,stderr:errText,result:result,message:"Python execution did not terminate cleanly after timeout.",duration:ms,actions:actions)
        }
        let status = p.terminationStatus
        return encodeRun(ok:!timedOut && status==0,stdout:outText,stderr:errText,result:result,message:timedOut ? "Python execution timed out after \(timeout) ms." : (status==0 ? "Python completed." : "Python exited with status \(status)."),duration:ms,actions:actions)
    }

    func listPlugins(projectPath:String?)->String {
        var byID:[String:PluginInfo]=[:]
        for (source,root) in pluginRoots(projectPath:projectPath) {
            guard let dirs=try? fm.contentsOfDirectory(at:root,includingPropertiesForKeys:[.isDirectoryKey],options:[.skipsHiddenFiles]) else { continue }
            for dir in dirs.prefix(Self.maxPlugins) { if let info=loadPlugin(dir:dir,source:source), byID[info.id] == nil { byID[info.id]=info } }
        }
        let values = byID.values.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        let reply=PluginListReply(ok:true,plugins:values,message:"\(values.count) extension(s) discovered • API v2 Extensions Platform.")
        return String(decoding:(try? JSONEncoder().encode(reply)) ?? Data("{\"ok\":false,\"plugins\":[],\"message\":\"encoding failed\"}".utf8),as:UTF8.self)
    }

    func runPlugin(id:String,projectPath:String?,contextJSON:String,timeoutMS:Int,interpreter:String,event:String="manual")->String {
        let executionLock = pluginLock(for: id)
        executionLock.lock(); defer { executionLock.unlock() }
        guard let (info,dir)=findPlugin(id:id,projectPath:projectPath), info.enabled else { return encodeRun(ok:false,stderr:"",message:"Plugin not found or disabled.",duration:0) }
        guard info.policy_status == "allowed" else { return encodeRun(ok:false,stderr:"",message:"Extension blocked by local policy: \(info.policy_status).",duration:0) }
        guard info.trusted else { return encodeRun(ok:false,stderr:"",message:"Extension is not trusted. Review it in Extension Manager before execution.",duration:0) }
        guard dependenciesSatisfied(info.dependencies, projectPath: projectPath) else { return encodeRun(ok:false,stderr:"",message:"Extension dependencies are not satisfied.",duration:0) }
        let allowed = info.events.contains(event) || info.commands.contains(where:{$0.event==event}) || info.panels.contains(where:{$0.event==event}) || info.analysis_passes.contains(where:{$0.event==event})
        if event != "manual" && !allowed { return encodeRun(ok:false,stderr:"",message:"Plugin does not subscribe to event \(event).",duration:0) }
        if event.hasPrefix("analysis.pass.") && !info.capabilities.contains("analysis.custom") { return encodeRun(ok:false,stderr:"",message:"Custom analysis pass requires analysis.custom capability.",duration:0) }
        let entry=dir.appendingPathComponent(info.entry).standardizedFileURL
        guard entry.path.hasPrefix(dir.standardizedFileURL.path+"/"),let data=try? Data(contentsOf:entry),data.count<=Self.maxSourceBytes,let code=String(data:data,encoding:.utf8) else { return encodeRun(ok:false,stderr:"",message:"Plugin entry is invalid or exceeds 256 KiB.",duration:0) }
        let augmented=augmentContext(contextJSON, info:info)
        let raw=run(source:code,contextJSON:augmented,capabilities:info.capabilities,timeoutMS:timeoutMS,filename:entry.path,interpreter:interpreter,event:event)
        return persistInternalActions(raw, info:info)
    }

    func setPluginSetting(id:String,key:String,valueJSON:String,projectPath:String?)->String {
        let executionLock = pluginLock(for: id)
        executionLock.lock(); defer { executionLock.unlock() }
        guard let (info,_)=findPlugin(id:id,projectPath:projectPath),info.settings.contains(where:{$0.id==key}) else { return "{\"ok\":false,\"message\":\"Plugin/setting not found.\"}" }
        guard valueJSON.utf8.count<=4096,let value=parseJSON(valueJSON) else { return "{\"ok\":false,\"message\":\"Setting value must be valid JSON and <= 4096 bytes.\"}" }
        var values=loadSettings(pluginID:id,definitions:info.settings); values[key]=value
        guard saveObject(values,to:pluginDataRoot(id).appendingPathComponent("settings.json")) else { return "{\"ok\":false,\"message\":\"Could not persist setting.\"}" }
        return "{\"ok\":true,\"message\":\"Setting saved.\"}"
    }

    func setPluginEnabled(id:String,enabled:Bool,projectPath:String?)->String {
        let executionLock = pluginLock(for: id)
        executionLock.lock(); defer { executionLock.unlock() }
        guard let (info,dir)=findPlugin(id:id,projectPath:projectPath) else { return "{\"ok\":false,\"message\":\"Plugin not found.\"}" }
        guard info.policy_status == "allowed" || !enabled else { return "{\"ok\":false,\"message\":\"Extension is blocked by local policy and cannot be enabled.\"}" }
        let marker=dir.appendingPathComponent(".disabled")
        do { if enabled { if fm.fileExists(atPath:marker.path){try fm.removeItem(at:marker)} } else { fm.createFile(atPath:marker.path,contents:Data()) }; return "{\"ok\":true,\"message\":\"Plugin \(jsonEscape(info.name)) \(enabled ? "enabled" : "disabled").\"}" } catch { return "{\"ok\":false,\"message\":\"\(jsonEscape(error.localizedDescription))\"}" }
    }

    func setPluginTrusted(id:String,trusted:Bool,projectPath:String?)->String {
        let executionLock = pluginLock(for: id)
        executionLock.lock(); defer { executionLock.unlock() }
        guard let (info,_)=findPlugin(id:id,projectPath:projectPath) else { return "{\"ok\":false,\"message\":\"Plugin not found.\"}" }
        if info.signature_status == "invalid" { return "{\"ok\":false,\"message\":\"Invalidly signed extensions cannot be trusted.\"}" }
        let marker=pluginDataRoot(id).appendingPathComponent("trusted.json")
        do {
            try fm.createDirectory(at: marker.deletingLastPathComponent(), withIntermediateDirectories: true)
            if trusted { try Data("true".utf8).write(to:marker,options:.atomic) } else if fm.fileExists(atPath:marker.path) { try fm.removeItem(at:marker) }
            return "{\"ok\":true,\"message\":\"Extension trust updated.\"}"
        } catch { return "{\"ok\":false,\"message\":\"\(jsonEscape(error.localizedDescription))\"}" }
    }

    /// Installs either a normal plugin folder or a HindSight .vgplugin bundle. Legacy .rewplugin bundles remain readable.
    func installPlugin(fromPath path:String)->String {
        let src=URL(fileURLWithPath:path).standardizedFileURL
        guard safePluginTree(src), let info=loadPlugin(dir:src,source:"install") else { return "{\"ok\":false,\"message\":\"Selected folder/bundle is invalid, unsafe, or exceeds package bounds.\"}" }
        let executionLock = pluginLock(for: info.id)
        executionLock.lock(); defer { executionLock.unlock() }
        let root=userPluginRoot()
        do {
            try fm.createDirectory(at:root,withIntermediateDirectories:true)
            let sourceExtension=src.pathExtension.lowercased(); let bundle=sourceExtension=="vgplugin" || sourceExtension=="rewplugin"
            let dst=root.appendingPathComponent(info.id + (bundle ? ".vgplugin" : ""),isDirectory:true)
            if src.path==dst.path{return "{\"ok\":true,\"message\":\"Extension is already installed.\"}"}
            if fm.fileExists(atPath:dst.path){try fm.removeItem(at:dst)}
            // Remove a legacy sibling form so one ID never has two user copies.
            let alternates=[
                root.appendingPathComponent(info.id,isDirectory:true),
                root.appendingPathComponent(info.id + ".vgplugin",isDirectory:true),
                root.appendingPathComponent(info.id + ".rewplugin",isDirectory:true)
            ].filter { $0.path != dst.path }
            for alternate in alternates where fm.fileExists(atPath:alternate.path) { try fm.removeItem(at:alternate) }
            try fm.copyItem(at:src,to:dst)
            return "{\"ok\":true,\"message\":\"Installed \(jsonEscape(info.name)) as \(bundle ? ".vgplugin bundle" : "plugin folder").\"}"
        } catch { return "{\"ok\":false,\"message\":\"\(jsonEscape(error.localizedDescription))\"}" }
    }

    private func findPlugin(id:String,projectPath:String?)->(PluginInfo,URL)? {
        guard validID(id) else { return nil }
        for (source,root) in pluginRoots(projectPath:projectPath) {
            guard let dirs=try? fm.contentsOfDirectory(at:root,includingPropertiesForKeys:[.isDirectoryKey],options:[.skipsHiddenFiles]) else { continue }
            for dir in dirs.prefix(Self.maxPlugins) { if let info=loadPlugin(dir:dir,source:source),info.id==id { return (info,dir) } }
        }
        return nil
    }

    private func loadPlugin(dir:URL,source:String)->PluginInfo? {
        guard let dirValues=try? dir.resourceValues(forKeys:[.isDirectoryKey,.isSymbolicLinkKey]),dirValues.isDirectory==true,dirValues.isSymbolicLink != true else{return nil}
        let manifestURL=dir.appendingPathComponent("plugin.json")
        guard regularNonSymlinkFile(manifestURL,maxBytes:64*1024),let data=try? Data(contentsOf:manifestURL),let m=try? JSONDecoder().decode(Manifest.self,from:data) else{return nil}
        guard Self.supportedAPIVersions.contains(m.api_version),validID(m.id),!m.entry.contains(".."),!m.entry.hasPrefix("/"),m.entry.hasSuffix(".py") else{return nil}
        let entry=dir.appendingPathComponent(m.entry).standardizedFileURL
        guard entry.path.hasPrefix(dir.standardizedFileURL.path+"/"),regularNonSymlinkFile(entry,maxBytes:Self.maxSourceBytes) else{return nil}
        let disabled=fm.fileExists(atPath:dir.appendingPathComponent(".disabled").path)
        let settings=Array((m.settings ?? []).prefix(64)).filter{validID($0.id)}
        let values=renderSettings(loadSettings(pluginID:m.id,definitions:settings))
        let deps=Array((m.dependencies ?? []).prefix(32)).filter{validID($0.plugin_id)}
        let signatureStatus=verifySignature(dir:dir)
        let trusted = signatureStatus == "verified" || fm.fileExists(atPath:pluginDataRoot(m.id).appendingPathComponent("trusted.json").path)
        let capabilities=Array((m.capabilities ?? []).prefix(48))
        let policyStatus = capabilities.first(where: { !Self.governedCapabilities.contains($0) }).map { "unknown capability \($0)" } ?? "allowed"
        let annotations=loadAnnotations(pluginID:m.id)
        let customData=loadObject(pluginDataRoot(m.id).appendingPathComponent("custom-data.json"))
        return PluginInfo(id:m.id,name:String(m.name.prefix(120)),version:String(m.version.prefix(64)),api_version:m.api_version,entry:m.entry,capabilities:capabilities,events:Array((m.events ?? []).prefix(24)),commands:Array((m.commands ?? []).prefix(48)),panels:Array((m.panels ?? []).prefix(24)),settings:settings,settings_values:values,analysis_passes:Array((m.analysis_passes ?? []).prefix(24)),debugger_filter:m.debugger_filter,package_format:{
            let ext=dir.pathExtension.lowercased()
            return ext=="vgplugin" ? "vgplugin" : (ext=="rewplugin" ? "legacy-rewplugin" : "folder")
        }(),dependencies:deps,dependency_status:"unchecked",signature_status:signatureStatus,policy_status:policyStatus,trusted:trusted,annotations:annotations,custom_data_keys:customData.keys.sorted(),path:dir.path,source:source,enabled:!disabled)
    }

    private func augmentContext(_ contextJSON:String,info:PluginInfo)->String {
        guard let data=contextJSON.data(using:.utf8),var root=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] else{return contextJSON}
        let annotationPayload: [[String: Any]] = loadAnnotations(pluginID: info.id).map { annotation in
            [
                "id": annotation.id,
                "address": annotation.address ?? NSNull(),
                "category": annotation.category,
                "text": annotation.text,
                "created_at": annotation.created_at
            ]
        }
        root["plugin"] = [
            "id": info.id,
            "name": info.name,
            "version": info.version,
            "settings": loadSettings(pluginID: info.id, definitions: info.settings),
            "state": loadObject(pluginDataRoot(info.id).appendingPathComponent("state.json")),
            "annotations": annotationPayload,
            "custom_data": loadObject(pluginDataRoot(info.id).appendingPathComponent("custom-data.json"))
        ]
        guard let out=try? JSONSerialization.data(withJSONObject:root,options:[.sortedKeys]),out.count<=Self.maxContextBytes else{return contextJSON}
        return String(decoding:out,as:UTF8.self)
    }

    private func persistInternalActions(_ raw:String,info:PluginInfo)->String {
        guard let data=raw.data(using:.utf8),let reply=try? JSONDecoder().decode(ScriptRunReply.self,from:data) else{return raw}
        var settings=loadSettings(pluginID:info.id,definitions:info.settings)
        var state=loadObject(pluginDataRoot(info.id).appendingPathComponent("state.json"))
        var customData=loadObject(pluginDataRoot(info.id).appendingPathComponent("custom-data.json"))
        var annotations=loadAnnotations(pluginID:info.id)
        var outward:[ScriptAction]=[]; var settingsDirty=false; var stateDirty=false; var dataDirty=false; var annotationsDirty=false
        let settingIDs=Set(info.settings.map{$0.id})
        for action in reply.actions ?? [] {
            if action.kind=="plugin.setting.set",info.capabilities.contains("settings.write"),let key=action.key,settingIDs.contains(key),let value=parseJSON(action.value_json) { settings[key]=value; settingsDirty=true; continue }
            if action.kind=="plugin.state.set",info.capabilities.contains("state.write"),let key=action.key,validID(key),let value=parseJSON(action.value_json) { state[key]=value; stateDirty=true; continue }
            if action.kind=="plugin.data.set",info.capabilities.contains("data.write"),let key=action.key,validID(key),let value=parseJSON(action.value_json) { customData[key]=value; dataDirty=true; continue }
            if action.kind=="annotation.add",info.capabilities.contains("annotation.write"),let text=action.text,!text.isEmpty { annotations.append(PluginAnnotation(id:UUID().uuidString,address:action.address.map{String($0.prefix(64))},category:String((action.category ?? "note").prefix(64)),text:String(text.prefix(4096)),created_at:ISO8601DateFormatter().string(from:Date()))); if annotations.count>512 { annotations.removeFirst(annotations.count-512) }; annotationsDirty=true; continue }
            outward.append(action)
        }
        if settingsDirty { _=saveObject(settings,to:pluginDataRoot(info.id).appendingPathComponent("settings.json")) }
        if stateDirty { _=saveObject(state,to:pluginDataRoot(info.id).appendingPathComponent("state.json")) }
        if dataDirty { _=saveObject(customData,to:pluginDataRoot(info.id).appendingPathComponent("custom-data.json")) }
        if annotationsDirty { _=saveAnnotations(annotations,pluginID:info.id) }
        return encodeRun(ok:reply.ok,stdout:reply.stdout,stderr:reply.stderr,result:reply.result,message:reply.message,duration:reply.duration_ms,actions:outward)
    }

    private func loadSettings(pluginID:String,definitions:[PluginSetting])->[String:Any] {
        var values:[String:Any]=[:]
        for d in definitions { if let raw=d.default_value { if let value=parseJSON(raw) { values[d.id]=value } else { values[d.id]=raw } } }
        for (k,v) in loadObject(pluginDataRoot(pluginID).appendingPathComponent("settings.json")) { if definitions.contains(where:{$0.id==k}) { values[k]=v } }
        return values
    }

    private func renderSettings(_ values:[String:Any])->[String:String] {
        var out:[String:String]=[:]
        for (k,v) in values { if let data=try? JSONSerialization.data(withJSONObject:v,options:[.fragmentsAllowed,.sortedKeys]) { out[k]=String(decoding:data,as:UTF8.self) } else { out[k]=String(describing:v) } }
        return out
    }

    private func parseJSON(_ text:String?)->Any? { guard let text,let data=text.data(using:.utf8) else{return nil}; return try? JSONSerialization.jsonObject(with:data,options:[.fragmentsAllowed]) }
    private func loadObject(_ url:URL)->[String:Any] { guard let data=try? Data(contentsOf:url),data.count<=Self.maxPersistentBytes,let obj=(try? JSONSerialization.jsonObject(with:data)) as? [String:Any] else{return [:]}; return obj }
    @discardableResult private func saveObject(_ obj:[String:Any],to url:URL)->Bool { guard let data=try? JSONSerialization.data(withJSONObject:obj,options:[.prettyPrinted,.sortedKeys]),data.count<=Self.maxPersistentBytes else{return false}; do{try fm.createDirectory(at:url.deletingLastPathComponent(),withIntermediateDirectories:true); try data.write(to:url,options:.atomic); return true}catch{return false} }
    private func loadAnnotations(pluginID:String)->[PluginAnnotation] {
        let url=pluginDataRoot(pluginID).appendingPathComponent("annotations.json")
        guard let d=try? Data(contentsOf:url),d.count<=Self.maxPersistentBytes,let values=try? JSONDecoder().decode([PluginAnnotation].self,from:d) else{return []}
        return Array(values.suffix(512))
    }
    private func saveAnnotations(_ values:[PluginAnnotation],pluginID:String)->Bool {
        guard let d=try? JSONEncoder().encode(Array(values.suffix(512))),d.count<=Self.maxPersistentBytes else{return false}
        let url=pluginDataRoot(pluginID).appendingPathComponent("annotations.json"); do{try fm.createDirectory(at:url.deletingLastPathComponent(),withIntermediateDirectories:true);try d.write(to:url,options:.atomic);return true}catch{return false}
    }
    private func verifySignature(dir:URL)->String {
        let url=dir.appendingPathComponent("signature.json")
        guard fm.fileExists(atPath:url.path) else{return "unsigned"}
        guard regularNonSymlinkFile(url,maxBytes:64*1024),let d=try? Data(contentsOf:url),let sig=try? JSONDecoder().decode(SignatureFile.self,from:d),sig.algorithm.lowercased()=="ed25519",let pk=Data(base64Encoded:sig.public_key_base64),let signature=Data(base64Encoded:sig.signature_base64),sig.signed_files.count<=64 else{return "invalid"}
        var message=Data()
        for rel in sig.signed_files.sorted() {
            guard !rel.contains(".."),!rel.hasPrefix("/"),rel != "signature.json" else{return "invalid"}
            let file=dir.appendingPathComponent(rel).standardizedFileURL; guard file.path.hasPrefix(dir.standardizedFileURL.path+"/"),regularNonSymlinkFile(file,maxBytes:Self.maxSourceBytes*4),let bytes=try? Data(contentsOf:file) else{return "invalid"}
            message.append(Data(rel.utf8)); message.append(0); var n=UInt64(bytes.count).bigEndian; withUnsafeBytes(of:&n){message.append(contentsOf:$0)}; message.append(bytes)
        }
        do { let key=try Curve25519.Signing.PublicKey(rawRepresentation:pk); return key.isValidSignature(signature,for:message) ? "verified" : "invalid" } catch { return "invalid" }
    }
    private func dependenciesSatisfied(_ deps:[PluginDependency],projectPath:String?)->Bool {
        guard !deps.isEmpty else{return true}
        var versions:[String:String]=[:]
        for (source,root) in pluginRoots(projectPath:projectPath) { _=source; guard let dirs=try? fm.contentsOfDirectory(at:root,includingPropertiesForKeys:nil,options:[.skipsHiddenFiles]) else{continue}; for dir in dirs.prefix(Self.maxPlugins) { if let i=loadPlugin(dir:dir,source:"dependency") { versions[i.id]=i.version } } }
        return deps.allSatisfy { dep in guard let v=versions[dep.plugin_id] else{return false}; guard let min=dep.minimum_version,!min.isEmpty else{return true}; return compareVersions(v,min) >= 0 }
    }
    private func compareVersions(_ a:String,_ b:String)->Int {
        let aa=a.split(separator:".").map{Int($0.prefix{ $0.isNumber }) ?? 0}; let bb=b.split(separator:".").map{Int($0.prefix{ $0.isNumber }) ?? 0}; for i in 0..<max(aa.count,bb.count){let x=i<aa.count ? aa[i]:0;let y=i<bb.count ? bb[i]:0;if x != y{return x<y ? -1:1}};return 0
    }
    private func regularNonSymlinkFile(_ url: URL, maxBytes: Int) -> Bool {
        guard let values=try? url.resourceValues(forKeys:[.isRegularFileKey,.isSymbolicLinkKey,.fileSizeKey]),
              values.isRegularFile==true,values.isSymbolicLink != true,(values.fileSize ?? maxBytes + 1)<=maxBytes else{return false}
        return true
    }
    private func safePluginTree(_ root: URL) -> Bool {
        guard let rootValues=try? root.resourceValues(forKeys:[.isDirectoryKey,.isSymbolicLinkKey]),rootValues.isDirectory==true,rootValues.isSymbolicLink != true else{return false}
        guard let enumerator=fm.enumerator(at:root,includingPropertiesForKeys:[.isDirectoryKey,.isRegularFileKey,.isSymbolicLinkKey,.fileSizeKey],options:[],errorHandler:{_,_ in false}) else{return false}
        var entries=0; var bytes=0
        for case let url as URL in enumerator {
            entries += 1; if entries > Self.maxPluginPackageEntries { return false }
            guard let values=try? url.resourceValues(forKeys:[.isDirectoryKey,.isRegularFileKey,.isSymbolicLinkKey,.fileSizeKey]),values.isSymbolicLink != true else{return false}
            if values.isDirectory==true { continue }
            guard values.isRegularFile==true else{return false}
            bytes += values.fileSize ?? 0; if bytes > Self.maxPluginPackageBytes { return false }
        }
        return true
    }

    private func pluginDataRoot(_ id: String) -> URL {
        applicationSupportRoot().appendingPathComponent("VectorGlass/PluginData", isDirectory: true).appendingPathComponent(id, isDirectory: true)
    }
    private func validID(_ s:String)->Bool { !s.isEmpty && s.utf8.count<=128 && s.range(of:"^[A-Za-z0-9._-]+$",options:.regularExpression) != nil }
    private func pluginRoots(projectPath:String?)->[(String,URL)] { var r:[(String,URL)]=[]; if let p=projectPath,!p.isEmpty { r.append(("project",URL(fileURLWithPath:p).appendingPathComponent("Plugins",isDirectory:true))) }; r.append(("user",userPluginRoot())); r.append(("legacy-user",legacyUserPluginRoot())); return r }
    private func applicationSupportRoot() -> URL {
        if let root = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first { return root }
        return fm.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support", isDirectory: true)
    }
    private func userPluginRoot() -> URL {
        applicationSupportRoot().appendingPathComponent("HindSight/Plugins", isDirectory: true)
    }
    private func legacyUserPluginRoot() -> URL {
        applicationSupportRoot().appendingPathComponent("VectorGlass/Plugins", isDirectory: true)
    }
    private func encodeRun(ok:Bool,stdout:String="",stderr:String,result:String?=nil,message:String,duration:Int,actions:[ScriptAction]=[])->String { let reply=ScriptRunReply(ok:ok,stdout:stdout,stderr:stderr,result:result,message:message,duration_ms:duration,actions:actions); return String(decoding:(try? JSONEncoder().encode(reply)) ?? Data("{\"ok\":false,\"stdout\":\"\",\"stderr\":\"encoding failed\",\"message\":\"encoding failed\",\"duration_ms\":0}".utf8),as:UTF8.self) }
    private func jsonEscape(_ s:String)->String { let d=(try? JSONSerialization.data(withJSONObject:[s])) ?? Data("[\"error\"]".utf8); let x=String(decoding:d,as:UTF8.self); return String(x.dropFirst().dropLast()) }
}
