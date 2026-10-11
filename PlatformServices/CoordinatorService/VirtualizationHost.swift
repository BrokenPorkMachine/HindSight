import Foundation

final class VirtualizationHost {
    private struct TraceEvent {
        let time = Date()
        let kind: String
        let detail: String
    }
    private struct Snapshot {
        let base: UInt64
        let data: Data
    }

    private var vm: OpaquePointer?
    private var profile: VirtualizationProfile?
    private var ramMiB = 0
    private var trace: [TraceEvent] = []
    private var snapshot: Snapshot?
    private var breakpoints: [UInt64: UInt32] = [:]

    static let profiles: [VirtualizationProfile] = [
        .init(id: "a12-research", name: "A12-class Research VM", cpuMode: "ARM64 via Hypervisor.framework", description: "ARM64 CPU virtualization with an emulated Apple-SoC device graph suitable for firmware and driver research.", maturity: "experimental", defaultRAMMiB: 2048, devices: ["Apple-style MMIO fabric", "AIC interrupt model", "DART/IOMMU model", "UART", "timer", "SPI/I2C models", "NVMe/ANS research model", "SEP mailbox model"], regions: [
            .init(name:"Guest RAM",kind:"memory",base:"0x00000000",size:"profile RAM",access:"RWX",notes:"HVF-backed research RAM"),
            .init(name:"AIC",kind:"MMIO model",base:"0x20000000",size:"0x00100000",access:"RW",notes:"Interrupt-controller research window"),
            .init(name:"DART",kind:"MMIO model",base:"0x21000000",size:"0x00100000",access:"RW",notes:"IOMMU translation model"),
            .init(name:"UART",kind:"MMIO model",base:"0x22000000",size:"0x00010000",access:"RW",notes:"Trace-friendly console model"),
            .init(name:"ANS/NVMe",kind:"MMIO model",base:"0x23000000",size:"0x00100000",access:"RW",notes:"Storage research model"),
            .init(name:"SEP mailbox",kind:"coprocessor",base:"0x24000000",size:"0x00100000",access:"RW",notes:"Mailbox/shared-memory model; no device secrets")
        ], limitations: ["Experimental profile: incomplete device models are intentionally non-production", "Not a cycle-accurate A12", "GPU/ANE/ISP are not hardware-virtualized", "Requires legally obtained guest firmware/images"]),
        .init(id: "t2-research", name: "T2 Research VM", cpuMode: "ARM64 via Hypervisor.framework", description: "T2-oriented coprocessor and peripheral research topology.", maturity: "experimental", defaultRAMMiB: 1024, devices: ["AIC", "DART", "SEP-style mailbox", "AES/storage bridge models", "USB/device-service models", "UART"], regions: [
            .init(name:"Guest RAM",kind:"memory",base:"0x00000000",size:"profile RAM",access:"RWX",notes:"HVF-backed research RAM"),
            .init(name:"AIC",kind:"MMIO model",base:"0x30000000",size:"0x00100000",access:"RW",notes:"Interrupt model"),
            .init(name:"DART",kind:"MMIO model",base:"0x31000000",size:"0x00100000",access:"RW",notes:"DMA/IOMMU model"),
            .init(name:"SEP mailbox",kind:"coprocessor",base:"0x32000000",size:"0x00100000",access:"RW",notes:"Research mailbox model"),
            .init(name:"Storage/AES bridge",kind:"MMIO model",base:"0x33000000",size:"0x00100000",access:"RW",notes:"No physical key material")
        ], limitations: ["Experimental profile: storage/AES and USB services are incomplete research models", "Not a replacement for physical T2 hardware", "Secure boot keys and device secrets are not emulated or supplied"]),
        .init(id: "sep-lab", name: "SEP Lab", cpuMode: "Emulated coprocessor", description: "SEP message/firmware experimentation surface. CPU execution is modeled separately from the main ARM64 HVF vCPU.", maturity: "experimental", defaultRAMMiB: 256, devices: ["SEP mailbox", "shared-memory windows", "interrupt doorbells"], regions: [
            .init(name:"Shared RAM",kind:"memory",base:"0x00000000",size:"profile RAM",access:"RW",notes:"Host-observable shared research memory"),
            .init(name:"SEP mailbox",kind:"coprocessor",base:"0x40000000",size:"0x00100000",access:"RW",notes:"Mailbox protocol experimentation"),
            .init(name:"Doorbells",kind:"interrupt",base:"0x40110000",size:"0x00010000",access:"RW",notes:"Modeled notification registers")
        ], limitations: ["Experimental profile: coprocessor execution is modeled, not hardware-equivalent", "Secure storage is intentionally unavailable rather than simulated without device evidence", "No UID/GID keys", "No Secure Enclave attestation", "No claim of hardware-equivalent security behavior"])
    ]

    deinit { stop() }

    func profilesJSON() -> String { encode(Self.profiles) }

    func start(profileID: String, ramMiB requested: Int) -> String {
        guard VGHostPlatform.supportsAppleSiliconHypervisor else {
            return state(ok: false, message: VGHostPlatform.unavailableReason(for: "Apple-Silicon Hypervisor RE"))
        }
        guard vm == nil else { return state(ok:false, message:"A virtual machine is already active.") }
        guard let p = Self.profiles.first(where: { $0.id == profileID }) else { return state(ok:false, message:"Unknown virtualization profile.") }
        let memory = min(max(requested > 0 ? requested : p.defaultRAMMiB, 16), 16 * 1024)
        var err = [CChar](repeating: 0, count: 512)
        let created = err.withUnsafeMutableBufferPointer { buffer in rw_hv_vm_create(UInt64(memory) * 1024 * 1024, buffer.baseAddress, buffer.count) }
        guard let created else { return state(ok:false, message:cString(err)) }
        vm = created; profile = p; ramMiB = memory; trace.removeAll(); snapshot = nil; breakpoints.removeAll()
        record("vm.start", "profile=\(p.id) ram=\(memory)MiB")
        return state(ok:true, message:"Virtualization backend started. RE tools can now inspect guest state without granting the UI direct hypervisor access.")
    }

    func stop() {
        if let vm { rw_hv_vm_destroy(vm) }
        vm = nil; profile = nil; ramMiB = 0; snapshot = nil; breakpoints.removeAll(); trace.removeAll()
    }

    func command(_ command: String) -> String {
        if command != "stop" && !VGHostPlatform.supportsAppleSiliconHypervisor {
            return state(ok: false, message: VGHostPlatform.unavailableReason(for: "Apple-Silicon Hypervisor RE"))
        }
        switch command {
        case "stop": stop(); return state(ok:true, message:"Virtual machine stopped.")
        case "reset":
            guard let vm else { return state(ok:false, message:"No active VM.") }
            var err = errorBuffer(); let rc = err.withUnsafeMutableBufferPointer { rw_hv_vm_reset(vm, $0.baseAddress, $0.count) }
            if rc == 0 { snapshot=nil; breakpoints.removeAll(); record("vm.reset","RAM cleared; PC=0") }
            return rc == 0 ? state(ok:true, message:"VM CPU/RAM reset.") : state(ok:false, message:cString(err))
        case "status": return state(ok:true, message: vm == nil ? "No active VM." : "VM backend active; Hypervisor RE tools available.")
        default: return state(ok:false, message:"Unsupported VM command.")
        }
    }

    func reCommand(_ json: String) -> String {
        guard VGHostPlatform.supportsAppleSiliconHypervisor else {
            return reply(false, VGHostPlatform.unavailableReason(for: "Apple-Silicon Hypervisor RE"))
        }
        guard let data=json.data(using:.utf8), let request=try? JSONDecoder().decode(VirtualizationRERequest.self,from:data) else { return reply(false,"Invalid virtualization RE request.") }
        switch request.command {
        case "memory": return memoryView(request)
        case "load_image": return loadImage(request)
        case "snapshot": return takeSnapshot(request)
        case "diff": return diffSnapshot(request)
        case "strings": return stringsSearch(request)
        case "pattern": return patternSearch(request)
        case "registers": return registerView()
        case "set_pc": return setPC(request)
        case "run_once": return runOnce()
        case "disassemble": return disassemble(request)
        case "breakpoint_set": return setBreakpoint(request)
        case "breakpoint_remove": return removeBreakpoint(request)
        case "breakpoints": return breakpointList()
        case "trace": return traceView()
        case "device_map": return deviceMap()
        case "dart": return dartInspector()
        case "interrupts": return interruptInspector()
        default: return reply(false,"Unsupported hypervisor RE command: \(request.command)")
        }
    }

    private func memoryView(_ r: VirtualizationRERequest) -> String {
        guard let base=parseAddress(r.address), let bytes=read(base, length: minLength(base, r.length ?? 256, 4096)) else { return reply(false,"Guest memory read failed or is out of range.") }
        record("memory.read", "gpa=\(hex(base)) size=\(bytes.count)")
        return reply(true,"Read \(bytes.count) guest bytes.",title:"Guest Memory",text:hexDump(bytes,base:base),address:hex(base),count:bytes.count)
    }
    private func loadImage(_ r: VirtualizationRERequest) -> String {
        guard let vm, let path=r.path, !path.isEmpty, let base=parseAddress(r.address) else { return reply(false,"Active VM, image path, and GPA are required.") }
        do {
            let attrs=try FileManager.default.attributesOfItem(atPath:path); let size=(attrs[.size] as? NSNumber)?.uint64Value ?? 0
            guard size > 0 && size <= 256*1024*1024 else { return reply(false,"Guest images are bounded to 256 MiB per load.") }
            let data=try Data(contentsOf:URL(fileURLWithPath:path),options:[.mappedIfSafe]); let ramBytes=UInt64(ramMiB)*1024*1024; guard base <= ramBytes, UInt64(data.count) <= ramBytes-base else { return reply(false,"Image does not fit guest RAM at requested GPA.") }
            var err=errorBuffer(); let rc=data.withUnsafeBytes { raw in err.withUnsafeMutableBufferPointer { rw_hv_vm_write_memory(vm,base,raw.baseAddress,data.count,$0.baseAddress,$0.count) } }
            guard rc == 0 else { return reply(false,cString(err)) }
            record("image.load", "\(URL(fileURLWithPath:path).lastPathComponent) -> \(hex(base)) size=\(data.count)")
            return reply(true,"Loaded \(data.count) bytes into guest RAM.",title:"Firmware / ROM Loader",text:"\(path)\nGPA \(hex(base))\nsize \(data.count) bytes",address:hex(base),count:data.count)
        } catch { return reply(false,error.localizedDescription) }
    }
    private func takeSnapshot(_ r: VirtualizationRERequest) -> String {
        guard let base=parseAddress(r.address) else { return reply(false,"Snapshot GPA is invalid.") }; let length=min(max(r.length ?? 65536,1),1024*1024)
        guard let data=read(base, length: length) else { return reply(false,"Snapshot range is outside guest RAM.") }
        snapshot=Snapshot(base:base,data:data); record("snapshot.take","gpa=\(hex(base)) size=\(data.count)")
        return reply(true,"Captured bounded guest-memory snapshot.",title:"VM Snapshot",text:"base \(hex(base))\nsize \(data.count) bytes\nfingerprint \(snapshotFingerprint(data))",address:hex(base),count:data.count)
    }
    private func diffSnapshot(_ r: VirtualizationRERequest) -> String {
        guard let snap=snapshot, let now=read(snap.base, length: snap.data.count) else { return reply(false,"Take a valid snapshot before diffing.") }
        var lines:[String]=[]; var changed=0
        for i in 0..<now.count where now[i] != snap.data[i] { changed += 1; if lines.count < 512 { lines.append("\(hex(snap.base+UInt64(i))): \(String(format:"%02x",snap.data[i])) -> \(String(format:"%02x",now[i]))") } }
        if changed > lines.count { lines.append("… \(changed-lines.count) additional byte changes omitted") }
        record("snapshot.diff","base=\(hex(snap.base)) changed=\(changed)")
        return reply(true,"Compared current RAM with snapshot.",title:"Memory Diff",text:"changed bytes: \(changed)\n"+lines.joined(separator:"\n"),address:hex(snap.base),count:changed)
    }
    private func stringsSearch(_ r: VirtualizationRERequest) -> String {
        guard let base=parseAddress(r.address) else { return reply(false,"Search GPA is invalid.") }; let length=min(max(r.length ?? 1024*1024,1),16*1024*1024)
        guard let data=read(base, length: length) else { return reply(false,"Search range is outside guest RAM.") }
        var result:[String]=[]; var start:Int?=nil
        func emit(_ end:Int) { if let s=start, end-s >= 4, result.count < 2048 { let bytes=data[s..<end]; result.append("\(hex(base+UInt64(s)))  \(String(decoding:bytes,as:UTF8.self))") }; start=nil }
        for i in 0..<data.count { let b=data[i]; if b>=0x20 && b<=0x7e { if start==nil { start=i } } else { emit(i) } }; emit(data.count)
        record("search.strings","gpa=\(hex(base)) size=\(length) hits=\(result.count)")
        return reply(true,"Recovered bounded printable guest strings.",title:"Guest String Search",text:result.isEmpty ? "No ASCII strings of length ≥4 found." : result.joined(separator:"\n"),address:hex(base),count:result.count)
    }
    private func patternSearch(_ r: VirtualizationRERequest) -> String {
        guard let base=parseAddress(r.address), let pattern=parseHexBytes(r.value ?? ""), !pattern.isEmpty else { return reply(false,"Provide a valid GPA and hex byte pattern (for example d4200000).") }
        let length=min(max(r.length ?? 1024*1024,pattern.count),16*1024*1024); guard let data=read(base, length: length) else { return reply(false,"Search range is outside guest RAM.") }
        var hits:[String]=[]; if pattern.count <= data.count { for i in 0...(data.count-pattern.count) where data[i..<(i+pattern.count)].elementsEqual(pattern) { if hits.count < 4096 { hits.append(hex(base+UInt64(i))) } } }
        record("search.pattern","pattern=\(r.value ?? "") hits=\(hits.count)")
        return reply(true,"Guest pattern search complete.",title:"Guest Pattern Search",text:hits.isEmpty ? "No matches." : hits.joined(separator:"\n"),address:hex(base),count:hits.count)
    }
    private func registerView() -> String {
        guard let vm else { return reply(false,"No active VM.") }; var pc:UInt64=0,sp:UInt64=0,err=errorBuffer(); let rc=err.withUnsafeMutableBufferPointer { rw_hv_vm_get_pc_sp(vm,&pc,&sp,$0.baseAddress,$0.count) }
        guard rc==0 else { return reply(false,cString(err)) }; record("vcpu.registers","pc=\(hex(pc)) sp=\(hex(sp))")
        return reply(true,"Read vCPU state.",title:"vCPU Registers",text:"PC  \(hex(pc))\nSP  \(hex(sp))",address:hex(pc),pc:hex(pc),sp:hex(sp))
    }
    private func setPC(_ r: VirtualizationRERequest) -> String {
        guard let vm,let pc=parseAddress(r.address) else { return reply(false,"Active VM and aligned PC are required.") }; var err=errorBuffer(); let rc=err.withUnsafeMutableBufferPointer { rw_hv_vm_set_pc(vm,pc,$0.baseAddress,$0.count) }; guard rc==0 else{return reply(false,cString(err))};record("vcpu.set_pc",hex(pc));return registerView()
    }
    private func runOnce() -> String {
        guard let vm else{return reply(false,"No active VM.")};var reason:UInt32=0,pc:UInt64=0,err=errorBuffer();let rc=err.withUnsafeMutableBufferPointer{rw_hv_vm_run_once(vm,&reason,&pc,$0.baseAddress,$0.count)};guard rc==0 else{return reply(false,cString(err))};record("vcpu.exit","reason=\(reason) pc=\(hex(pc))");return reply(true,"vCPU ran until the next Hypervisor.framework exit.",title:"vCPU Exit",text:"exit reason \(reason)\nPC \(hex(pc))",address:hex(pc),pc:hex(pc),exitReason:reason)
    }
    private func disassemble(_ r: VirtualizationRERequest) -> String {
        guard let base = parseAddress(r.address) else {
            return reply(false, "Disassembly GPA is invalid.")
        }
        let requested = min(max(r.length ?? 256, 4), 4096)
        let length = requested - (requested % 4)
        guard let data = read(base, length: length) else {
            return reply(false, "Disassembly range is outside guest RAM.")
        }
        var lines: [String] = []
        for i in stride(from: 0, to: data.count, by: 4) {
            let b0 = UInt32(data[i])
            let b1 = UInt32(data[i + 1]) << 8
            let b2 = UInt32(data[i + 2]) << 16
            let b3 = UInt32(data[i + 3]) << 24
            let word = b0 | b1 | b2 | b3
            var out = [CChar](repeating: 0, count: 256)
            _ = re_arm64_decode(word, base + UInt64(i), &out, out.count)
            lines.append("\(hex(base + UInt64(i)))  \(String(format: "%08x", word))  \(String(cString: out))")
        }
        record("code.disassemble", "gpa=\(hex(base)) bytes=\(data.count)")
        return reply(true, "Decoded guest ARM64 bytes using HindSight's native decoder.", title: "Guest Disassembly", text: lines.joined(separator: "\n"), address: hex(base), count: lines.count)
    }
    private func setBreakpoint(_ r: VirtualizationRERequest) -> String {
        guard let vm,let addr=parseAddress(r.address),addr%4==0,!breakpoints.keys.contains(addr) else{return reply(false,"Breakpoint address must be a new 4-byte-aligned guest GPA.")};guard breakpoints.count<256,let old=read(addr, length: 4) else{return reply(false,"Breakpoint limit reached or address unreadable.")};let word=UInt32(old[0]) | (UInt32(old[1]) << 8) | (UInt32(old[2]) << 16) | (UInt32(old[3]) << 24);var brk:UInt32=0xd4200000;var err=errorBuffer();let rc=withUnsafeBytes(of:&brk){ bytes in err.withUnsafeMutableBufferPointer{rw_hv_vm_write_memory(vm,addr,bytes.baseAddress,4,$0.baseAddress,$0.count)}};guard rc==0 else{return reply(false,cString(err))};breakpoints[addr]=word;record("breakpoint.set",hex(addr));return breakpointList()
    }
    private func removeBreakpoint(_ r: VirtualizationRERequest) -> String {
        guard let vm,let addr=parseAddress(r.address),var word=breakpoints.removeValue(forKey:addr) else{return reply(false,"No HindSight guest breakpoint exists at that GPA.")};var err=errorBuffer();let rc=withUnsafeBytes(of:&word){ bytes in err.withUnsafeMutableBufferPointer{rw_hv_vm_write_memory(vm,addr,bytes.baseAddress,4,$0.baseAddress,$0.count)}};guard rc==0 else{breakpoints[addr]=word;return reply(false,cString(err))};record("breakpoint.remove",hex(addr));return breakpointList()
    }
    private func breakpointList() -> String {
        let rows = breakpoints.keys.sorted().compactMap { address -> String? in
            guard let original = breakpoints[address] else { return nil }
            return "\(hex(address)) original=\(String(format: "%08x", original))"
        }
        return reply(true, "Guest software breakpoints.", title: "VM Breakpoints", text: rows.isEmpty ? "No guest breakpoints." : rows.joined(separator: "\n"), count: rows.count)
    }
    private func traceView() -> String { let f=ISO8601DateFormatter();let rows=trace.suffix(1024).map{"\(f.string(from:$0.time))  \($0.kind)  \($0.detail)"};return reply(true,"Hypervisor RE trace timeline.",title:"VM Trace",text:rows.isEmpty ? "No VM trace events yet." : rows.joined(separator:"\n"),count:rows.count) }
    private func deviceMap() -> String { guard let p=profile else{return reply(false,"No VM profile is active.")};let rows=p.regions.map{"\($0.base)  \($0.size)  \($0.access.padding(toLength:3,withPad:" ",startingAt:0))  \($0.kind)  \($0.name)\n    \($0.notes)"};return reply(true,"Profile GPA/MMIO topology.",title:"GPA / Device Map",text:rows.joined(separator:"\n"),count:rows.count) }
    private func dartInspector() -> String { guard let p=profile else{return reply(false,"No VM profile is active.")};let regions=p.regions.filter{$0.name.localizedCaseInsensitiveContains("DART")};let text=regions.isEmpty ? "This profile has no DART model." : regions.map{"\($0.name) \($0.base) size=\($0.size)\n\($0.notes)\n\nTranslation tables are model state until a guest/device implementation programs them; HindSight will not invent translations."}.joined(separator:"\n");return reply(true,"DART/IOMMU model inspector.",title:"DART Inspector",text:text,count:regions.count) }
    private func interruptInspector() -> String { guard let p=profile else{return reply(false,"No VM profile is active.")};let regions=p.regions.filter{$0.name.localizedCaseInsensitiveContains("AIC") || $0.kind=="interrupt"};let text=regions.isEmpty ? "This profile has no interrupt-controller region." : regions.map{"\($0.name) \($0.base) size=\($0.size)\n\($0.notes)"}.joined(separator:"\n\n");return reply(true,"Interrupt-model topology. Dynamic IRQ injection tracing will populate here as device models become executable.",title:"Interrupt / AIC Inspector",text:text,count:regions.count) }

    private func read(_ base:UInt64,length:Int)->Data?{guard let vm,length>=0,UInt64(length)<=UInt64(ramMiB)*1024*1024,base<=UInt64(ramMiB)*1024*1024-UInt64(length) else{return nil};var data=Data(count:length);var err=errorBuffer();let rc=data.withUnsafeMutableBytes{ raw in err.withUnsafeMutableBufferPointer{rw_hv_vm_read_memory(vm,base,raw.baseAddress,length,$0.baseAddress,$0.count)}};return rc==0 ? data:nil}
    private func minLength(_ base:UInt64,_ requested:Int,_ cap:Int)->Int{let maxRAM=UInt64(ramMiB)*1024*1024;guard base<maxRAM else{return 0};return min(max(requested,1),cap,Int(maxRAM-base))}
    private func record(_ kind:String,_ detail:String){trace.append(.init(kind:kind,detail:detail));if trace.count>4096{trace.removeFirst(trace.count-4096)}}
    private func parseAddress(_ text:String?)->UInt64?{guard var s=text?.trimmingCharacters(in:.whitespacesAndNewlines),!s.isEmpty else{return nil};if s.lowercased().hasPrefix("0x"){s=String(s.dropFirst(2))};return UInt64(s,radix:16)}
    private func parseHexBytes(_ text:String)->[UInt8]?{let s=text.replacingOccurrences(of:"0x",with:"").replacingOccurrences(of:" ",with:"").replacingOccurrences(of:"_",with:"");guard !s.isEmpty,s.count%2==0 else{return nil};var out:[UInt8]=[];var i=s.startIndex;while i<s.endIndex{let j=s.index(i,offsetBy:2);guard let b=UInt8(s[i..<j],radix:16) else{return nil};out.append(b);i=j};return out}
    private func hex(_ v:UInt64)->String{String(format:"0x%016llx",v)}
    private func hexDump(_ data:Data,base:UInt64)->String{var rows:[String]=[];for off in stride(from:0,to:data.count,by:16){let end=min(off+16,data.count);let chunk=data[off..<end];let hexes=chunk.map{String(format:"%02x",$0)}.joined(separator:" ").padding(toLength:47,withPad:" ",startingAt:0);let ascii=chunk.map{($0>=0x20 && $0<=0x7e) ? Character(UnicodeScalar($0)) : "."}.map(String.init).joined();rows.append("\(hex(base+UInt64(off)))  \(hexes)  |\(ascii)|")};return rows.joined(separator:"\n")}
    private func snapshotFingerprint(_ data:Data)->String{var h:UInt64=1469598103934665603;for b in data{h ^= UInt64(b);h &*= 1099511628211};return String(format:"fnv1a64:%016llx",h)}
    private func errorBuffer()->[CChar]{[CChar](repeating:0,count:512)}
    private func cString(_ buffer: [CChar]) -> String {
        buffer.withUnsafeBufferPointer { storage in
            guard let base = storage.baseAddress else { return "" }
            return String(cString: base)
        }
    }
    private func encode<T:Encodable>(_ value:T)->String{let d=try? JSONEncoder().encode(value);return String(decoding:d ?? Data("{}".utf8),as:UTF8.self)}
    private func reply(_ ok:Bool,_ message:String,title:String?=nil,text:String?=nil,address:String?=nil,pc:String?=nil,sp:String?=nil,exitReason:UInt32?=nil,count:Int?=nil)->String{encode(VirtualizationREReply(ok:ok,message:message,title:title,text:text,address:address,pc:pc,sp:sp,exitReason:exitReason,count:count))}
    private func state(ok: Bool, message: String) -> String { let p=profile;return encode(VirtualizationState(ok:ok,message:message,running:vm != nil,profileID:p?.id,ramMiB:ramMiB,cpuBackend:p?.cpuMode ?? "Hypervisor.framework / emulation",deviceBackend:"HindSight VMKit device graph",topology:p?.devices ?? [],limitations:p?.limitations ?? ["No VM active."])) }
}
