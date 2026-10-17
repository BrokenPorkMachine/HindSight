import Foundation
import Darwin
import CryptoKit
import Network

final class FirmwareRuntimeHost {
    private final class SynchronousResult<Value>: @unchecked Sendable {
        private let lock = NSLock()
        private var value: Result<Value, Error>?
        func store(_ result: Result<Value, Error>) { lock.lock(); value = result; lock.unlock() }
        func load() -> Result<Value, Error>? { lock.lock(); defer { lock.unlock() }; return value }
    }
    private final class BoundedOutputBuffer: @unchecked Sendable {
        private let lock = NSLock()
        private var data = Data()
        private let capacity: Int
        init(capacity: Int) { self.capacity = capacity }
        func reset() { lock.lock(); data.removeAll(keepingCapacity: true); lock.unlock() }
        func append(_ chunk: Data) {
            lock.lock(); defer { lock.unlock() }
            data.append(chunk)
            if data.count > capacity { data.removeFirst(data.count - capacity) }
        }
        func string(suffix limit: Int) -> String {
            lock.lock(); defer { lock.unlock() }
            return String(decoding: data.suffix(limit), as: UTF8.self)
        }
    }
    private var vectorEmu: Process?
    private var vectorEmuGrouped = false
    private let vectorEmuOutput = BoundedOutputBuffer(capacity: 512 * 1024)
    private var vectorEmuPort = 37920
    private var iphemeral: Process?
    private var iphemeralGrouped = false
    private let iphemeralOutput = BoundedOutputBuffer(capacity: 512 * 1024)
    private var iphemeralPort = 37921
    private let maxReplyText = 512 * 1024

    deinit { stopVectorEmu(); stopIPhemeral() }

    private func establishProcessGroup(for process: Process) -> Bool {
        let pid = process.processIdentifier
        return pid > 0 && setpgid(pid, pid) == 0
    }

    private func terminateProcess(_ process: Process, grouped: Bool, wait: DispatchSemaphore? = nil) {
        guard process.isRunning else { return }
        let pid = process.processIdentifier
        if grouped { _ = kill(-pid, SIGTERM) } else { process.terminate() }
        if let wait { _ = wait.wait(timeout: .now() + 1) }
        if process.isRunning {
            if grouped { _ = kill(-pid, SIGKILL) } else { _ = kill(pid, SIGKILL) }
        }
    }

    func command(_ json: String) -> String {
        guard let data = json.data(using: .utf8), let r = try? JSONDecoder().decode(FirmwareRuntimeRequest.self, from: data) else {
            return encode(FirmwareRuntimeReply(ok:false, message:"Invalid firmware/runtime request."))
        }
        switch r.command {
        case "dyld_inspect": return inspectDyldCache(r.path)
        case "dyld_read": return readDyldCache(r.path, address:r.address, count:r.count)
        case "dyld_validate_image": return validateDyldImageSource(cachePath:r.path, imagePath:r.query, address:r.address)
        case "ipsw_devices": return fetchDevices(soc: r.soc)
        case "ipsw_releases": return fetchFirmwares(device: r.device)
        case "ipsw_download": return downloadIPSW(device:r.device, build:r.build, destination:r.destination)
        case "ipsw_import": return importIPSW(r.path)
        case "firmware_keys": return fetchFirmwareKeys(device:r.device, build:r.build)
        case "vectoremu_status": return vectorEmuStatus(executable:r.executable)
        case "vectoremu_start": return startVectorEmu(executable:r.executable, soc:r.soc, rom:r.rom, port:r.port)
        case "vectoremu_stop": stopVectorEmu(); return encode(.init(ok:true,message:"VectorEmu stopped.",running:false,emulatorOutput:boundedVectorEmuOutput()))
        case "vectoremu_debug": return vectorEmuDebug(r)
        case "iphemeral_status": return iphemeralStatus(executable:r.executable)
        case "iphemeral_start": return startIPhemeral(executable:r.executable, soc:r.soc, rom:r.rom, mode:r.mode, path:r.path, port:r.port, steps:r.steps)
        case "iphemeral_debug": return iphemeralDebug(r)
        case "iphemeral_stop": stopIPhemeral(); return encode(.init(ok:true,message:"iPhemeral software-emulation process stopped.",running:false,emulatorOutput:boundedOutput()))
        default: return encode(.init(ok:false, message:"Unsupported firmware/runtime command: \(r.command)"))
        }
    }

    // MARK: dyld shared cache
    private func inspectDyldCache(_ path: String?) -> String {
        guard let path, path.hasPrefix("/"), path.utf8.count < 16384 else { return encode(.init(ok:false,message:"Choose a dyld shared cache file.")) }
        do {
            let url=URL(fileURLWithPath:path); let data=try Data(contentsOf:url, options:[.mappedIfSafe])
            guard data.count >= 152 else { throw HostError("File is too small to be a dyld shared cache.") }
            let magic=ascii(data,0,16).trimmingCharacters(in:.controlCharacters.union(.whitespaces))
            guard magic.hasPrefix("dyld_v") else { throw HostError("Not a dyld shared cache (magic: \(magic)).") }
            let mappingOffset=Int(u32(data,16)); let mappingCount=Int(u32(data,20))
            guard mappingCount >= 0 && mappingCount <= 4096 && mappingOffset >= 0 && mappingOffset + mappingCount*32 <= data.count else { throw HostError("Invalid cache mapping table.") }
            let imagesTextOffset=Int(u64(data,136)); let imagesTextCount=Int(u64(data,144))
            guard imagesTextCount >= 0 && imagesTextCount <= 200_000, imagesTextOffset >= 0, imagesTextOffset + imagesTextCount*32 <= data.count else { throw HostError("The cache image-text table is absent or outside the file. Select the primary cache file of the set.") }
            var mappings:[DyldCacheMappingRecord]=[]; mappings.reserveCapacity(mappingCount)
            for i in 0..<mappingCount {
                let o = mappingOffset + i * 32
                let address = u64(data, o)
                let span = u64(data, o + 8)
                let fileOffset = u64(data, o + 16)
                let maxProtection = u32(data, o + 24)
                let initialProtection = u32(data, o + 28)
                guard fileOffset <= UInt64(data.count), span <= UInt64(data.count) - fileOffset else { continue }
                mappings.append(.init(address:String(format:"0x%016llx",address),size:span,fileOffset:fileOffset,maxProtection:maxProtection,initialProtection:initialProtection))
            }
            var images:[DyldCacheImageRecord]=[]; images.reserveCapacity(min(imagesTextCount,50_000))
            for i in 0..<imagesTextCount {
                let o=imagesTextOffset+i*32; let load=u64(data,o+16); let size=u32(data,o+24); let po=Int(u32(data,o+28))
                guard po >= 0 && po < data.count else { continue }
                let pathName=cString(data, po, 4096)
                if !pathName.isEmpty { images.append(.init(path:pathName,loadAddress:String(format:"0x%016llx",load),textSize:size,cacheFile:url.lastPathComponent)) }
                if images.count >= 100_000 { break }
            }
            let summary=DyldCacheSummary(ok:true,message:"Loaded \(images.count) cached images. Framework/dylib selection is available without extracting the whole cache.",path:path,magic:magic,mappingCount:mappings.count,imageCount:images.count,mappings:mappings,images:images)
            return encode(.init(ok:true,message:summary.message,dyld:summary))
        } catch { return encode(.init(ok:false,message:error.localizedDescription)) }
    }

    private func validateDyldImageSource(cachePath: String?, imagePath: String?, address: String?) -> String {
        guard let cachePath, cachePath.hasPrefix("/"), let imagePath,
              imagePath.hasPrefix("/System/Library/") || imagePath.hasPrefix("/usr/lib/"),
              let address, let loadAddress = UInt64(address.hasPrefix("0x") ? String(address.dropFirst(2)) : address, radix: 16) else {
            return encode(.init(ok:false, message:"The selected cache image does not have a supported local system image path."))
        }
        do {
            let cacheURL = URL(fileURLWithPath: cachePath).standardizedFileURL
            let imageURL = URL(fileURLWithPath: imagePath).standardizedFileURL.resolvingSymlinksInPath()
            guard imageURL.path.hasPrefix("/System/Library/") || imageURL.path.hasPrefix("/usr/lib/") else {
                throw HostError("The cache image path resolves outside supported system image directories.")
            }
            let cache = try Data(contentsOf: cacheURL, options:[.mappedIfSafe])
            guard cache.count >= 152 else { throw HostError("The selected file is too small to be a dyld shared cache.") }
            guard ascii(cache, 0, 16).hasPrefix("dyld_v") else { throw HostError("The selected file is not a dyld shared cache.") }
            let imageTableOffset = Int(u64(cache, 136)), imageCount = Int(u64(cache, 144))
            guard imageCount >= 0, imageCount <= 200_000, imageTableOffset >= 0,
                  imageTableOffset <= cache.count, imageCount <= (cache.count - imageTableOffset) / 32 else {
                throw HostError("The cache image table failed bounds validation.")
            }
            var imageMatched = false
            for index in 0..<imageCount {
                let row = imageTableOffset + index * 32
                let rowAddress = u64(cache, row + 16)
                let pathOffset = Int(u32(cache, row + 28))
                guard pathOffset >= 0, pathOffset < cache.count else { continue }
                if rowAddress == loadAddress && cString(cache, pathOffset, 4096) == imagePath {
                    imageMatched = true
                    break
                }
            }
            guard imageMatched else { throw HostError("The selected path and load address do not identify the same image in this cache.") }

            let mappingOffset = Int(u32(cache, 16)), mappingCount = Int(u32(cache, 20))
            guard mappingCount >= 0, mappingCount <= 4096, mappingOffset >= 0,
                  mappingOffset <= cache.count, mappingCount <= (cache.count - mappingOffset) / 32 else {
                throw HostError("The cache mapping table failed bounds validation.")
            }
            var headerOffset: Int?
            var available = 0
            for index in 0..<mappingCount {
                let row = mappingOffset + index * 32
                let vm = u64(cache, row), span = u64(cache, row + 8), fileOffset = u64(cache, row + 16)
                guard loadAddress >= vm, loadAddress - vm < span else { continue }
                let delta = loadAddress - vm
                guard fileOffset <= UInt64(cache.count), delta <= UInt64(cache.count) - fileOffset else { continue }
                headerOffset = Int(fileOffset + delta)
                available = Int(min(span - delta, min(UInt64(cache.count) - fileOffset - delta, 65_536)))
                break
            }
            guard let headerOffset, available >= 32 else { throw HostError("The image header is not backed by the selected primary cache file; it may be in a subcache.") }
            let cacheHeader = cache.subdata(in: headerOffset..<(headerOffset + available))
            let local = try Data(contentsOf: imageURL, options:[.mappedIfSafe])
            guard let cacheUUID = machoUUID(cacheHeader), let localUUID = machoUUID(local.prefix(65_536)), cacheUUID == localUUID else {
                throw HostError("The installed image UUID does not match the selected cache image. Import the matching system image before analysis.")
            }
            return encode(.init(ok:true, message:"Matched the selected dyld cache image to its installed Mach-O UUID.", outputPath:imageURL.path))
        } catch { return encode(.init(ok:false, message:error.localizedDescription)) }
    }

    private func machoUUID(_ data: Data) -> Data? {
        guard data.count >= 32 else { return nil }
        let magic = u32(data, 0)
        guard magic == 0xfeedfacf || magic == 0xfeedface else { return nil }
        let commandCount = Int(u32(data, 16))
        guard commandCount <= 8192 else { return nil }
        let headerSize = magic == 0xfeedfacf ? 32 : 28
        var offset = headerSize
        for _ in 0..<commandCount {
            guard offset <= data.count, data.count - offset >= 8 else { return nil }
            let command = u32(data, offset), size = Int(u32(data, offset + 4))
            guard size >= 8, size <= data.count - offset else { return nil }
            if command == 0x1b {
                guard size >= 24 else { return nil }
                return data.subdata(in:(offset + 8)..<(offset + 24))
            }
            offset += size
        }
        return nil
    }

    private func readDyldCache(_ path: String?, address: String?, count: Int?) -> String {
        guard let path, path.hasPrefix("/"), let address else { return encode(.init(ok:false,message:"Cache path and virtual address are required.")) }
        let trimmed = address.lowercased().hasPrefix("0x") ? String(address.dropFirst(2)) : address
        guard let virtual = UInt64(trimmed, radix:16) else { return encode(.init(ok:false,message:"Invalid cache virtual address.")) }
        let requested = min(max(count ?? 256, 1), 64 * 1024)
        do {
            let data = try Data(contentsOf:URL(fileURLWithPath:path), options:[.mappedIfSafe])
            guard data.count >= 32 else { throw HostError("Cache file is too small.") }
            let mappingOffset=Int(u32(data,16)); let mappingCount=Int(u32(data,20))
            guard mappingCount >= 0 && mappingCount <= 4096 && mappingOffset >= 0 && mappingOffset + mappingCount*32 <= data.count else { throw HostError("Invalid cache mapping table.") }
            var translated: UInt64?
            var available: UInt64 = 0
            var mappingDescription = ""
            for i in 0..<mappingCount {
                let o=mappingOffset+i*32; let base=u64(data,o); let span=u64(data,o+8); let fileOffset=u64(data,o+16)
                guard virtual >= base, virtual - base < span else { continue }
                let delta=virtual-base; translated=fileOffset+delta; available=span-delta
                mappingDescription="mapping[\(i)] vm=\(String(format:"0x%016llx",base))..\(String(format:"0x%016llx",base+span)) file=\(String(format:"0x%llx",fileOffset))"
                break
            }
            guard let fileOffset=translated, fileOffset < UInt64(data.count) else { throw HostError("Address is not backed by a primary-cache file mapping. It may reside in another subcache.") }
            let n=Int(min(UInt64(requested), min(available,UInt64(data.count)-fileOffset)))
            let bytes=data[Int(fileOffset)..<Int(fileOffset)+n]
            var out="dyld cache read\n\(mappingDescription)\naddress=\(String(format:"0x%016llx",virtual)) file_offset=\(String(format:"0x%llx",fileOffset)) bytes=\(n)\n\n"
            for (row,start) in stride(from:0,to:n,by:16).enumerated() {
                let end=min(start+16,n); let chunk=bytes[bytes.index(bytes.startIndex,offsetBy:start)..<bytes.index(bytes.startIndex,offsetBy:end)]
                let hex=chunk.map{String(format:"%02x",$0)}.joined(separator:" ").padding(toLength:47,withPad:" ",startingAt:0)
                let ascii=chunk.map{ $0 >= 0x20 && $0 < 0x7f ? Character(UnicodeScalar($0)) : "." }.map(String.init).joined()
                out += String(format:"%016llx  %@  |%@|\n", virtual + UInt64(row*16), hex, ascii)
            }
            return encode(.init(ok:true,message:"Read \(n) bytes from the dyld shared cache mapping.",text:out))
        } catch { return encode(.init(ok:false,message:error.localizedDescription)) }
    }

    // MARK: firmware catalog / IPSW
    private func fetchDevices(soc: String?) -> String {
        do {
            guard let url = URL(string: "https://ipsw.info/api/v1/devices?format=full") else { throw HostError("Firmware catalog URL is invalid.") }
            let data=try fetch(url, max:8*1024*1024)
            let objects=try JSONSerialization.jsonObject(with:data) as? [[String:Any]] ?? []
            let needle=(soc ?? "").lowercased().trimmingCharacters(in:.whitespacesAndNewlines)
            let devices:[FirmwareDeviceRecord]=objects.compactMap { o in
                guard let identifier=o["identifier"] as? String, let name=o["name"] as? String else { return nil }
                let cpu=o["apple_cpu"] as? String; let platform=o["platform"] as? String; let board=o["boardconfig"] as? String
                if !needle.isEmpty {
                    let hay=[cpu,platform,board,name,identifier].compactMap{$0}.joined(separator:" ").lowercased()
                    guard hay.contains(needle) else { return nil }
                }
                return FirmwareDeviceRecord(name:name,identifier:identifier,boardconfig:board,platform:platform,apple_cpu:cpu)
            }.sorted { ($0.apple_cpu ?? "",$0.name) < ($1.apple_cpu ?? "",$1.name) }
            return encode(.init(ok:true,message:"Loaded \(devices.count) Apple firmware targets\(needle.isEmpty ? "" : " matching \(needle)").",devices:devices))
        } catch { return encode(.init(ok:false,message:"Firmware catalog: \(error.localizedDescription)")) }
    }
    private func fetchFirmwares(device: String?) -> String {
        guard let device, validToken(device) else { return encode(.init(ok:false,message:"Select a valid device identifier.")) }
        do {
            guard let url = URL(string: "https://ipsw.info/api/v1/firmware/\(device)") else { throw HostError("Firmware release URL is invalid.") }
            let data=try fetch(url,max:16*1024*1024)
            let raw=try JSONSerialization.jsonObject(with:data) as? [[String:Any]] ?? []
            let values=raw.compactMap { o -> FirmwareReleaseRecord? in
                guard let identifier=o["identifier"] as? String, let version=o["version"] as? String, let build=o["buildid"] as? String else { return nil }
                let size=(o["filesize"] as? NSNumber)?.uint64Value
                return .init(identifier:identifier,version:version,buildid:build,sha256sum:o["sha256sum"] as? String,filesize:size,url:o["url"] as? String,releasedate:o["releasedate"] as? String,signed:o["signed"] as? Bool)
            }
            return encode(.init(ok:true,message:"Loaded \(values.count) IPSW releases for \(device). Download URLs are resolved by the catalog to Apple's CDN.",firmwares:values))
        } catch { return encode(.init(ok:false,message:"Firmware releases: \(error.localizedDescription)")) }
    }
    private func downloadIPSW(device:String?, build:String?, destination:String?) -> String {
        guard let device, validToken(device), let build, validToken(build), let destination, destination.hasPrefix("/") else { return encode(.init(ok:false,message:"Device, build, and destination folder are required.")) }
        do {
            guard let metaURL = URL(string: "https://ipsw.info/api/v1/firmware/\(device)/\(build)") else { throw HostError("Firmware metadata URL is invalid.") }
            let metaData=try fetch(metaURL,max:1024*1024); let o=try JSONSerialization.jsonObject(with:metaData) as? [String:Any] ?? [:]
            guard let link=o["url"] as? String, let remote=URL(string:link), ["https"].contains(remote.scheme?.lowercased() ?? "") else { throw HostError("Catalog did not return a secure IPSW URL.") }
            let version=o["version"] as? String ?? build
            let expectedHash=(o["sha256sum"] as? String)?.lowercased()
            let filename="\(device)_\(version)_\(build)_Restore.ipsw".replacingOccurrences(of:"/",with:"_")
            let out=URL(fileURLWithPath:destination,isDirectory:true).appendingPathComponent(filename)
            let tmp=try download(remote, timeout:1800)
            if let expectedHash, !expectedHash.isEmpty {
                let actual = try sha256File(tmp)
                guard actual == expectedHash else { try? FileManager.default.removeItem(at: tmp); throw HostError("IPSW SHA-256 mismatch. Expected \(expectedHash), got \(actual).") }
            }
            let fm=FileManager.default; if fm.fileExists(atPath:out.path) { try fm.removeItem(at:out) }; try fm.moveItem(at:tmp,to:out)
            return encode(.init(ok:true,message:"Downloaded IPSW from Apple's CDN and verified the catalog SHA-256 when available.",outputPath:out.path))
        } catch { return encode(.init(ok:false,message:"IPSW download: \(error.localizedDescription)")) }
    }
    private func importIPSW(_ path:String?) -> String {
        guard let path, path.hasPrefix("/"), FileManager.default.fileExists(atPath:path) else { return encode(.init(ok:false,message:"Choose an IPSW file.")) }
        do {
            let list=try runTool("/usr/bin/unzip",["-Z1",path],timeout:30,max:4*1024*1024)
            let lines=list.split(separator:"\n").map(String.init)
            let manifest=lines.first(where:{$0.hasSuffix("BuildManifest.plist")}) ?? "BuildManifest.plist"
            var summary="IPSW: \(URL(fileURLWithPath:path).lastPathComponent)\nEntries: \(lines.count)\nBuild manifest: \(manifest)\n\nLikely reverse-engineering payloads:\n"
            let interesting=lines.filter { s in let l=s.lowercased(); return l.contains("kernelcache") || l.contains("dyld") || l.hasSuffix(".im4p") || l.contains("sep") || l.contains("iboot") || l.contains("ibss") || l.contains("ibec") }.prefix(400)
            summary += interesting.joined(separator:"\n")
            return encode(.init(ok:true,message:"Indexed IPSW archive. Use firmware-key lookup for public wiki keys and extract/mount legally obtained payloads for analysis.",text:String(summary.prefix(maxReplyText)),outputPath:path))
        } catch { return encode(.init(ok:false,message:"IPSW import: \(error.localizedDescription)")) }
    }

    // MARK: public firmware keys
    private func fetchFirmwareKeys(device:String?, build:String?) -> String {
        guard let device, validToken(device), let build, validToken(build) else { return encode(.init(ok:false,message:"Device and build are required for key lookup.")) }
        do {
            guard var comps = URLComponents(string: "https://theapplewiki.com/w/api.php") else { throw HostError("Firmware key service URL is invalid.") }
            comps.queryItems=[.init(name:"action",value:"query"),.init(name:"list",value:"search"),.init(name:"srsearch",value:"\(build) \(device) prefix:Keys:"),.init(name:"srlimit",value:"10"),.init(name:"format",value:"json")]
            guard let searchURL = comps.url else { throw HostError("Firmware key search URL could not be constructed.") }
            let search=try fetch(searchURL,max:2*1024*1024)
            let obj=try JSONSerialization.jsonObject(with:search) as? [String:Any]
            let results=((obj?["query"] as? [String:Any])?["search"] as? [[String:Any]]) ?? []
            guard let title=results.compactMap({$0["title"] as? String}).first(where:{$0.contains(build) && $0.contains(device)}) ?? results.first?["title"] as? String else { throw HostError("No public The Apple Wiki key page was found for \(device) / \(build).") }
            guard var page = URLComponents(string: "https://theapplewiki.com/w/api.php") else { throw HostError("Firmware key service URL is invalid.") }
            page.queryItems=[.init(name:"action",value:"parse"),.init(name:"page",value:title),.init(name:"prop",value:"wikitext"),.init(name:"format",value:"json")]
            guard let pageURL = page.url else { throw HostError("Firmware key page URL could not be constructed.") }
            let pdata=try fetch(pageURL,max:4*1024*1024); let pobj=try JSONSerialization.jsonObject(with:pdata) as? [String:Any]
            guard let parse=pobj?["parse"] as? [String:Any], let wwrap=parse["wikitext"] as? [String:Any], let wiki=wwrap["*"] as? String else { throw HostError("The Apple Wiki returned no parsable key text.") }
            let keys=parseKeyRecords(wiki,source:title)
            return encode(.init(ok:true,message:"Fetched \(keys.count) publicly published firmware key record(s) from The Apple Wiki. Device-unique UID/GID secrets are not generated or inferred.",text:"Source: https://theapplewiki.com/wiki/\(title.addingPercentEncoding(withAllowedCharacters:.urlPathAllowed) ?? title)",keys:keys))
        } catch { return encode(.init(ok:false,message:"Firmware key lookup: \(error.localizedDescription)")) }
    }
    private func parseKeyRecords(_ wiki:String, source:String) -> [FirmwareKeyRecord] {
        let lines=wiki.components(separatedBy:.newlines); var filename=""; var iv=""; var key=""; var out:[FirmwareKeyRecord]=[]
        func clean(_ s:String)->String { s.replacingOccurrences(of:"'''",with:"").replacingOccurrences(of:"`",with:"").trimmingCharacters(in:.whitespacesAndNewlines) }
        for line in lines {
            let low=line.lowercased()
            if low.contains("filename") { if let i=line.firstIndex(of:"=") { filename=clean(String(line[line.index(after:i)...])) } else if let i=line.firstIndex(of:":") { filename=clean(String(line[line.index(after:i)...])) } }
            if let m=firstHex(line,min:32,max:32), low.contains("iv") { iv=m }
            if let m=firstHex(line,min:32,max:128), low.contains("key") && !low.contains("keypage") { key=m }
            if !filename.isEmpty && !iv.isEmpty && !key.isEmpty { out.append(.init(filename:filename,iv:iv,key:key,sourcePage:source)); filename=""; iv=""; key="" }
            if out.count >= 512 { break }
        }
        return out
    }

    // MARK: VectorEmu built-in deterministic execution lab
    private func vectorEmuStatus(executable: String?) -> String {
        guard let path = resolveVectorEmu(executable) else {
            return encode(.init(ok:false, message:"VectorEmu is not built yet. Build the HindSight app target once or choose vg-emu-server manually.", running:vectorEmu?.isRunning ?? false, emulatorOutput:boundedVectorEmuOutput()))
        }
        do {
            let version = try runTool(path, ["--version"], timeout:5, max:64*1024)
            return encode(.init(ok:true, message:"VectorEmu ready: \(path)", text:version, running:vectorEmu?.isRunning ?? false, emulatorOutput:boundedVectorEmuOutput()))
        } catch {
            return encode(.init(ok:false, message:"VectorEmu probe failed: \(error.localizedDescription)", running:vectorEmu?.isRunning ?? false, emulatorOutput:boundedVectorEmuOutput()))
        }
    }

    private func startVectorEmu(executable:String?, soc:String?, rom:String?, port:Int?) -> String {
        guard vectorEmu?.isRunning != true else {
            return encode(.init(ok:false, message:"VectorEmu is already running.", running:true, emulatorOutput:boundedVectorEmuOutput()))
        }
        guard let exe = resolveVectorEmu(executable),
              let soc, validToken(soc),
              let rom, rom.hasPrefix("/"), FileManager.default.fileExists(atPath: rom) else {
            return encode(.init(ok:false, message:"Choose a local research ROM/image and a supported SoC.", running:false))
        }
        let selectedPort = port ?? 37920
        guard (1024...65535).contains(selectedPort) else {
            return encode(.init(ok:false, message:"VectorEmu port must be between 1024 and 65535.", running:false))
        }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: exe)
        p.arguments = ["--soc", soc, "--rom", rom, "--port", String(selectedPort)]
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        vectorEmuOutput.reset()
        vectorEmuPort = selectedPort
        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            guard let self else { return }
            let data = handle.availableData
            if !data.isEmpty { self.appendVectorEmuOutput(data) }
        }
        p.terminationHandler = { _ in
            pipe.fileHandleForReading.readabilityHandler = nil
        }
        do {
            try p.run()
            vectorEmuGrouped = establishProcessGroup(for: p)
            vectorEmu = p
            return encode(.init(ok:true, message:"VectorEmu started on 127.0.0.1:\(selectedPort). Attach when the listener is ready.", running:true, emulatorOutput:boundedVectorEmuOutput()))
        } catch {
            return encode(.init(ok:false, message:"VectorEmu launch failed: \(error.localizedDescription)", running:false, emulatorOutput:boundedVectorEmuOutput()))
        }
    }

    private func stopVectorEmu() {
        if let process = vectorEmu, process.isRunning {
            // Prefer protocol shutdown so the engine exits between instruction batches.
            if let payload = try? JSONSerialization.data(withJSONObject:["id":Int(Date().timeIntervalSince1970 * 1000),"command":"shutdown"], options:[]),
               let _ = try? vectorEmuRoundTrip(payload, port:vectorEmuPort, timeout:1.0) { }
            process.interrupt()
            terminateProcess(process, grouped: vectorEmuGrouped)
        }
        vectorEmu = nil
        vectorEmuGrouped = false
    }

    private func vectorEmuDebug(_ request: FirmwareRuntimeRequest) -> String {
        let command = request.debugCommand ?? "status"
        let allowed: Set<String> = ["hello","status","attach","detach","pause","run","step","reset","shutdown","registers","cpu_state","regions","read_memory","disassemble","breakpoint_add","breakpoint_remove","breakpoints","trace","header","convert"]
        guard allowed.contains(command) else {
            return encode(.init(ok:false, message:"VectorEmu command is not exposed by this UI: \(command)", running:vectorEmu?.isRunning ?? false))
        }
        var object: [String:Any] = ["id":Int(Date().timeIntervalSince1970 * 1000), "command":command]
        if let address = request.address, !address.isEmpty { object["address"] = address }
        if let count = request.count {
            if command == "read_memory" { object["length"] = max(1, min(count, 65536)) }
            else { object["count"] = max(1, min(count, 10000)) }
        }
        if let steps = request.steps { object["steps"] = min(steps, 1_000_000_000) }
        if let source = request.source, source.utf8.count <= 16 * 1024 { object["source"] = source }
        do {
            let payload = try JSONSerialization.data(withJSONObject:object, options:[])
            let raw = try vectorEmuRoundTrip(payload, port:request.port ?? vectorEmuPort, timeout: command == "run" ? 4.0 : 2.0)
            let pretty = prettyJSON(raw)
            return encode(.init(ok:true, message:"VectorEmu \(command) completed.", text:pretty, running:vectorEmu?.isRunning ?? false, emulatorOutput:boundedVectorEmuOutput(), emulatorJSON:String(decoding:raw,as:UTF8.self)))
        } catch {
            return encode(.init(ok:false, message:"VectorEmu \(command): \(error.localizedDescription)", running:vectorEmu?.isRunning ?? false, emulatorOutput:boundedVectorEmuOutput()))
        }
    }

    private func resolveVectorEmu(_ requested:String?) -> String? {
        let fm = FileManager.default
        var candidates:[String] = []
        if let requested, !requested.isEmpty { candidates.append(requested) }
        if let env = ProcessInfo.processInfo.environment["VECTORGLASS_EMU_SERVER"] { candidates.append(env) }
        if let here = Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("vg-emu-server").path { candidates.append(here) }
        candidates += ["/opt/homebrew/bin/vg-emu-server", "/usr/local/bin/vg-emu-server", NSHomeDirectory()+"/bin/vg-emu-server"]
        return candidates.first { fm.isExecutableFile(atPath:$0) }
    }

    private func vectorEmuRoundTrip(_ payload:Data, port:Int, timeout:TimeInterval) throws -> Data {
        guard (1...65535).contains(port), let nwPort = NWEndpoint.Port(rawValue:UInt16(port)) else { throw HostError("Invalid loopback port.") }
        let connection = NWConnection(host:NWEndpoint.Host("127.0.0.1"), port:nwPort, using:.tcp)
        let queue = DispatchQueue(label:"ca.zensoftware.VectorGlass.vectoremu")
        let sem = DispatchSemaphore(value:0)
        var output = Data()
        var terminalError: Error?
        var finished = false
        func finish(_ error:Error? = nil) {
            guard !finished else { return }
            finished = true
            terminalError = error
            sem.signal()
        }
        func receiveNext() {
            connection.receive(minimumIncompleteLength:1, maximumLength:64*1024) { data, _, complete, error in
                if let data { output.append(data) }
                if output.count > 1_048_576 { finish(HostError("VectorEmu reply exceeded 1 MiB.")); return }
                if output.contains(0x0a) { finish(); return }
                if let error { finish(error); return }
                if complete { finish(); return }
                receiveNext()
            }
        }
        connection.stateUpdateHandler = { state in
            switch state {
            case .ready:
                var framed = payload
                framed.append(0x0a)
                connection.send(content:framed, completion:.contentProcessed { error in
                    if let error { finish(error) } else { receiveNext() }
                })
            case .failed(let error): finish(error)
            case .cancelled: if !finished { finish(HostError("VectorEmu connection cancelled.")) }
            default: break
            }
        }
        connection.start(queue:queue)
        guard sem.wait(timeout:.now()+timeout) == .success else {
            connection.cancel()
            throw HostError("VectorEmu did not reply before the timeout.")
        }
        connection.cancel()
        if let terminalError { throw terminalError }
        guard let newline = output.firstIndex(of:0x0a) else { throw HostError("VectorEmu returned an incomplete frame.") }
        return Data(output[..<newline])
    }

    private func prettyJSON(_ data:Data) -> String {
        guard let object = try? JSONSerialization.jsonObject(with:data),
              let pretty = try? JSONSerialization.data(withJSONObject:object, options:[.prettyPrinted,.sortedKeys]) else { return String(decoding:data,as:UTF8.self) }
        return String(decoding:pretty,as:UTF8.self)
    }

    private func appendVectorEmuOutput(_ data: Data) { vectorEmuOutput.append(data) }
    private func boundedVectorEmuOutput() -> String { vectorEmuOutput.string(suffix: 256 * 1024) }

    // MARK: iPhemeral adapter
    private func iphemeralStatus(executable:String?) -> String {
        let path=resolveIPhemeral(executable)
        guard let path else { return encode(.init(ok:false,message:"iPhemeral CLI was not found. Choose iphem-cli in the Full-System iPhemeral workspace.",running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput())) }
        do {
            let help=try runTool(path,["--help"],timeout:5,max:128*1024)
            let version=help.split(separator:"\n").first(where:{$0.localizedCaseInsensitiveContains("IPHEMERAL") || $0.localizedCaseInsensitiveContains("iphem-cli")}) ?? Substring(URL(fileURLWithPath:path).lastPathComponent)
            return encode(.init(ok:true,message:"iPhemeral 1.9-compatible CLI ready: \(path)",text:String(version)+"\n\n"+help,running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput()))
        } catch { return encode(.init(ok:false,message:"iPhemeral probe failed: \(error.localizedDescription)",running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput())) }
    }
    private func startIPhemeral(executable:String?, soc:String?, rom:String?, mode:String?, path:String?, port:Int?, steps:UInt64?) -> String {
        guard iphemeral?.isRunning != true else { return encode(.init(ok:false,message:"iPhemeral is already running.",running:true,emulatorOutput:boundedOutput())) }
        guard let exe=resolveIPhemeral(executable), let soc, validToken(soc) else { return encode(.init(ok:false,message:"Choose iphem-cli and a supported SoC.",running:false)) }
        let selectedMode = mode ?? "raw"
        let selectedPort = port ?? 37921
        guard (1024...65535).contains(selectedPort) else { return encode(.init(ok:false,message:"iPhemeral debugger port must be between 1024 and 65535.",running:false)) }
        if soc.lowercased() != "t8020", selectedMode != "raw" {
            let hasReadableROM = rom.map { $0.hasPrefix("/") && FileManager.default.isReadableFile(atPath: $0) } ?? false
            if !hasReadableROM {
                return encode(.init(ok:false,message:"iPhemeral requires a matching SecureROM/iBoot image for SoC \(soc) in this boot mode.",running:false))
            }
        }
        var arguments=["--soc",soc]
        switch selectedMode {
        case "raw":
            guard let rom, rom.hasPrefix("/"), FileManager.default.isReadableFile(atPath:rom) else { return encode(.init(ok:false,message:"Raw mode requires a readable SecureROM/iBoot image.",running:false)) }
            arguments += ["--rom",rom]
        case "ipsw", "system_boot", "dfu_restore", "springboard", "ipsw_install":
            guard let path, path.hasPrefix("/"), FileManager.default.fileExists(atPath:path) else { return encode(.init(ok:false,message:"This iPhemeral mode requires a local IPSW or firmware path.",running:false)) }
            let flag: String
            switch selectedMode {
            case "ipsw": flag = "--ipsw"
            case "system_boot": flag = "--system-boot"
            case "dfu_restore": flag = "--dfu-restore"
            case "springboard": flag = "--springboard"
            case "ipsw_install": flag = "--ipsw-install"
            default: return encode(.init(ok:false,message:"Unsupported iPhemeral boot mode: \(selectedMode).",running:false))
            }
            arguments += [flag,path]
            if let rom, rom.hasPrefix("/"), FileManager.default.isReadableFile(atPath:rom) { arguments += ["--rom",rom] }
        default: return encode(.init(ok:false,message:"Unsupported iPhemeral boot mode: \(selectedMode).",running:false))
        }
        arguments += ["--debug-server",String(selectedPort),"--log-uart"]
        if let steps, steps > 0 { arguments += ["--steps",String(min(steps,1_000_000_000))] }
        let p=Process(); p.executableURL=URL(fileURLWithPath:exe); p.arguments=arguments
        let pipe=Pipe(); p.standardOutput=pipe; p.standardError=pipe; iphemeralOutput.reset()
        iphemeralPort=selectedPort
        pipe.fileHandleForReading.readabilityHandler={ [weak self] h in guard let self else { return }; let d=h.availableData; if d.isEmpty { return }; self.appendEmulatorOutput(d) }
        p.terminationHandler={ _ in pipe.fileHandleForReading.readabilityHandler=nil }
        do { try p.run(); iphemeralGrouped = establishProcessGroup(for: p); iphemeral=p; return encode(.init(ok:true,message:"Started iPhemeral \(selectedMode) backend for \(soc); live RE server on 127.0.0.1:\(selectedPort).",running:true,emulatorOutput:boundedOutput())) }
        catch { return encode(.init(ok:false,message:"iPhemeral launch failed: \(error.localizedDescription)",running:false,emulatorOutput:boundedOutput())) }
    }
    private func iphemeralDebug(_ request:FirmwareRuntimeRequest) -> String {
        let command=request.debugCommand ?? "status"
        let allowed:Set<String>=["hello","status","attach","detach","pause","run","step","reset","shutdown","registers","cpu_state","regions","read_memory","disassemble","breakpoint_add","breakpoint_remove","breakpoints","trace","watch_write","header","convert"]
        guard allowed.contains(command) else { return encode(.init(ok:false,message:"iPhemeral RE command is not exposed by this UI: \(command)",running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput())) }
        guard iphemeral?.isRunning == true else { return encode(.init(ok:false,message:"Start iPhemeral before using its live RE server.",running:false,emulatorOutput:boundedOutput())) }
        var object:[String:Any] = ["id":Int(Date().timeIntervalSince1970*1000),"command":command]
        if let address=request.address,!address.isEmpty { object["address"]=address }
        if let count=request.count { object[command == "read_memory" ? "length":"count"]=max(1,min(count,65536)) }
        if let steps=request.steps { object["steps"]=min(steps,1_000_000_000) }
        do { let payload=try JSONSerialization.data(withJSONObject:object); let raw=try vectorEmuRoundTrip(payload,port:request.port ?? iphemeralPort,timeout:command == "run" ? 8 : 3); return encode(.init(ok:true,message:"iPhemeral RE \(command) completed.",text:prettyJSON(raw),running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput(),emulatorJSON:String(decoding:raw,as:UTF8.self))) }
        catch { return encode(.init(ok:false,message:"iPhemeral RE \(command): \(error.localizedDescription)",running:iphemeral?.isRunning ?? false,emulatorOutput:boundedOutput())) }
    }
    private func stopIPhemeral() { if let p=iphemeral, p.isRunning { p.interrupt(); terminateProcess(p, grouped: iphemeralGrouped) }; iphemeral=nil; iphemeralGrouped=false }
    private func resolveIPhemeral(_ requested:String?) -> String? {
        let fm=FileManager.default; var candidates:[String]=[]; if let r=requested,!r.isEmpty { candidates.append(r) }; if let e=ProcessInfo.processInfo.environment["IPHEM_CLI"] { candidates.append(e) }
        candidates += ["/opt/homebrew/bin/iphem-cli","/usr/local/bin/iphem-cli",NSHomeDirectory()+"/bin/iphem-cli",NSHomeDirectory()+"/Applications/iphem-cli"]
        return candidates.first { fm.isExecutableFile(atPath:$0) }
    }

    // MARK: helpers
    private func fetch(_ url: URL, max: Int) throws -> Data {
        let sem = DispatchSemaphore(value: 0)
        let result = SynchronousResult<Data>()
        let task = URLSession.shared.dataTask(with: url) { data, response, error in
            defer { sem.signal() }
            if let error {
                result.store(.failure(error))
                return
            }
            guard let http = response as? HTTPURLResponse,
                  http.url?.scheme?.lowercased() == "https",
                  (200..<300).contains(http.statusCode),
                  let data,
                  data.count <= max else {
                result.store(.failure(HostError("HTTPS request failed or response exceeded bounds.")))
                return
            }
            result.store(.success(data))
        }
        task.resume()
        guard sem.wait(timeout: .now() + 30) == .success else {
            task.cancel()
            throw HostError("Network request timed out.")
        }
        guard let completed = result.load() else { throw HostError("Network request completed without a result.") }
        return try completed.get()
    }

    private func download(_ url: URL, timeout: TimeInterval) throws -> URL {
        let sem = DispatchSemaphore(value: 0)
        let result = SynchronousResult<URL>()
        let task = URLSession.shared.downloadTask(with: url) { temp, response, error in
            defer { sem.signal() }
            if let error {
                result.store(.failure(error))
                return
            }
            guard let http = response as? HTTPURLResponse,
                  http.url?.scheme?.lowercased() == "https",
                  (200..<300).contains(http.statusCode),
                  let temp else {
                result.store(.failure(HostError("HTTPS download failed.")))
                return
            }
            do {
                let dst = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".ipsw")
                try FileManager.default.copyItem(at: temp, to: dst)
                result.store(.success(dst))
            } catch {
                result.store(.failure(error))
            }
        }
        task.resume()
        guard sem.wait(timeout: .now() + timeout) == .success else {
            task.cancel()
            throw HostError("Download timed out.")
        }
        guard let completed = result.load() else { throw HostError("Download completed without a result.") }
        return try completed.get()
    }

    private func sha256File(_ url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while true {
            let data = try handle.read(upToCount: 4 * 1024 * 1024) ?? Data()
            if data.isEmpty { break }
            hasher.update(data: data)
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    private func runTool(_ executable: String, _ args: [String], timeout: TimeInterval, max: Int) throws -> String {
        guard max > 0 else { throw HostError("Tool output bound must be positive.") }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = args
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        final class Capture: @unchecked Sendable {
            private let lock = NSLock()
            private var data = Data()
            private var exceeded = false
            func append(_ chunk: Data, max: Int) {
                lock.lock(); defer { lock.unlock() }
                guard !exceeded else { return }
                if chunk.count > max - data.count {
                    exceeded = true
                    let remaining = max - data.count
                    if remaining > 0 { data.append(chunk.prefix(remaining)) }
                    return
                }
                data.append(chunk)
            }
            func snapshot() -> (Data, Bool) {
                lock.lock(); defer { lock.unlock() }
                return (data, exceeded)
            }
        }

        let capture = Capture()
        let finished = DispatchSemaphore(value: 0)
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if !chunk.isEmpty { capture.append(chunk, max: max) }
        }
        process.terminationHandler = { _ in finished.signal() }

        do { try process.run() }
        catch {
            pipe.fileHandleForReading.readabilityHandler = nil
            throw error
        }
        let grouped = establishProcessGroup(for: process)

        if finished.wait(timeout: .now() + timeout) != .success {
            terminateProcess(process, grouped: grouped, wait: finished)
            pipe.fileHandleForReading.readabilityHandler = nil
            throw HostError("Tool timed out.")
        }

        pipe.fileHandleForReading.readabilityHandler = nil
        let tail = pipe.fileHandleForReading.readDataToEndOfFile()
        if !tail.isEmpty { capture.append(tail, max: max) }
        let (data, exceeded) = capture.snapshot()
        guard !exceeded else { throw HostError("Tool output exceeded bound.") }
        guard process.terminationStatus == 0 else {
            let message = String(decoding: data.prefix(2048), as: UTF8.self)
            throw HostError(message.isEmpty ? "Tool exited with status \(process.terminationStatus)." : message)
        }
        return String(decoding: data, as: UTF8.self)
    }
    private func u32(_ d:Data,_ o:Int)->UInt32 { d[o..<o+4].withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)}.littleEndian }
    private func u64(_ d:Data,_ o:Int)->UInt64 { d[o..<o+8].withUnsafeBytes{$0.loadUnaligned(as:UInt64.self)}.littleEndian }
    private func ascii(_ d:Data,_ o:Int,_ n:Int)->String { String(bytes:d[o..<min(d.count,o+n)],encoding:.ascii) ?? "" }
    private func cString(_ d:Data,_ o:Int,_ max:Int)->String { let e=min(d.count,o+max); let s=d[o..<e]; let z=s.firstIndex(of:0) ?? s.endIndex; return String(bytes:s[..<z],encoding:.utf8) ?? "" }
    private func validToken(_ s:String)->Bool { !s.isEmpty && s.utf8.count<128 && s.range(of:"^[A-Za-z0-9,._+-]+$",options:.regularExpression) != nil }
    private func firstHex(_ s:String,min:Int,max:Int)->String? { let r="(?i)(?<![0-9a-f])([0-9a-f]{\(min),\(max)})(?![0-9a-f])"; guard let re=try? NSRegularExpression(pattern:r),let m=re.firstMatch(in:s,range:NSRange(s.startIndex...,in:s)),let rr=Range(m.range(at:1),in:s) else{return nil};return String(s[rr]) }
    private func appendEmulatorOutput(_ data: Data) { iphemeralOutput.append(data) }
    private func boundedOutput() -> String { iphemeralOutput.string(suffix: 256 * 1024) }
    private func encode(_ reply: FirmwareRuntimeReply)->String { guard let d=try? JSONEncoder().encode(reply) else{return "{\"ok\":false,\"message\":\"Encoding failure\"}"};return String(decoding:d,as:UTF8.self) }
    private struct HostError:LocalizedError { let text:String; init(_ t:String){text=t}; var errorDescription:String?{text} }
}
