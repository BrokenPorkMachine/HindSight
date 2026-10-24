import Foundation
import CryptoKit
import Darwin

/// Lookup eligibility only. Captured provider names do not prove runtime image
/// ownership, implementation identity, or that a declaration exists in the SDK.
struct DarwinSDKImportCandidate: Codable, Equatable, Sendable {
    let name: String
    let symbol: String
    let provider: String
}

private func isEligiblePublicSDKProvider(_ provider: String) -> Bool {
    provider.range(of: #"^/usr/lib/(?:system/)?lib[A-Za-z0-9_.+-]+\.dylib$"#,
                   options: .regularExpression) != nil
        || provider.range(of: #"^/System/Library/Frameworks/([A-Za-z][A-Za-z0-9_]*)\.framework/(?:Versions/[A-Za-z0-9]+/)?\1$"#,
                          options: .regularExpression) != nil
}

struct DarwinSDKFunctionDeclaration: Codable, Equatable, Sendable {
    let name: String
    let symbol: String
    let declaration: String?
    let abiDeclaration: String?
    let workerEligible: Bool
}

struct DarwinSDKTargetContext: Codable, Equatable, Sendable {
    let architecture: String
    let sdk: String
    let triple: String

    fileprivate init(architecture: String, sdk: String, triple: String) {
        self.architecture = architecture
        self.sdk = sdk
        self.triple = triple
    }
}

struct DarwinSDKCollectedDeclaration: Codable, Equatable, Sendable {
    let context: DarwinSDKTargetContext
    let sdkPath: String
    let sdkBuild: String
    let compiler: String
    let headerSHA256: String
    let function: DarwinSDKFunctionDeclaration?
}

struct DarwinSDKCollectionEnvironment: Equatable, Sendable {
    let context: DarwinSDKTargetContext
    let sdkPath: String
    let sdkBuild: String
    let clang: String
    let compiler: String
    let headerSHA256: String
}

struct DarwinSDKDeclarationCatalog: Codable, Sendable {
    let schema: Int
    let sourceSHA256: String
    let candidates: [DarwinSDKImportCandidate]
    let declarations: [DarwinSDKCollectedDeclaration]

    init(sourceSHA256: String, candidates: [DarwinSDKImportCandidate], declarations: [DarwinSDKCollectedDeclaration]) throws {
        guard sourceSHA256.utf8.count == 64,
              sourceSHA256.allSatisfy({ "0123456789abcdef".contains($0) }),
              !candidates.isEmpty, candidates.count <= 128,
              candidates.count == declarations.count else {
            throw DarwinSDKDeclarations.Invalid(message: "Invalid source-bound SDK declaration catalog.")
        }
        schema = 1
        self.sourceSHA256 = sourceSHA256
        self.candidates = candidates
        self.declarations = declarations
        guard validated(for: sourceSHA256) else {
            throw DarwinSDKDeclarations.Invalid(message: "SDK declarations do not match their captured import candidates or collection identity.")
        }
    }

    func validated(for expectedSourceSHA256: String) -> Bool {
        guard schema == 1, sourceSHA256 == expectedSourceSHA256,
              sourceSHA256.utf8.count == 64,
              sourceSHA256.allSatisfy({ "0123456789abcdef".contains($0) }),
              !candidates.isEmpty, candidates.count <= 128,
              candidates.count == declarations.count else { return false }
        func bounded(_ value: String, maximum: Int) -> Bool {
            value.utf8.count <= maximum && !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        }
        func validFunctionDeclaration(_ value: String?, name: String) -> Bool {
            guard let value else { return true }
            guard !value.isEmpty, value.utf8.count <= 1024, value.hasSuffix(";"),
                  value.rangeOfCharacter(from: CharacterSet(charactersIn: "\n\r\t\\{}[]#=\"")) == nil,
                  value.filter({ $0 == "(" }).count == 1, value.filter({ $0 == ")" }).count == 1,
                  let opening = value.firstIndex(of: "("), let closing = value.lastIndex(of: ")"),
                  value.index(after: closing) == value.index(before: value.endIndex) else { return false }
            let declaredName = value[..<opening].split(whereSeparator: \.isWhitespace).last
            let arguments = value[value.index(after: opening)..<closing]
            return declaredName.map(String.init) == name
                && !arguments.contains("(") && !arguments.contains(")")
                && value.dropLast().filter({ $0 == ";" }).isEmpty
        }
        guard let first = declarations.first,
              ["arm64", "arm64e", "x86_64"].contains(first.context.architecture),
              !first.context.sdk.isEmpty, bounded(first.context.sdk, maximum: 32),
              !first.context.triple.isEmpty, bounded(first.context.triple, maximum: 128),
              first.sdkPath.hasPrefix("/"), bounded(first.sdkPath, maximum: 4096),
              first.sdkBuild.range(of: #"^[A-Za-z0-9_.-]{1,128}$"#, options: .regularExpression) != nil,
              !first.compiler.isEmpty, bounded(first.compiler, maximum: 512),
              first.headerSHA256.range(of: #"^[0-9a-f]{64}$"#, options: .regularExpression) != nil else { return false }
        let triplePrefix = "\(first.context.architecture)-apple-"
        guard first.context.triple.hasPrefix(triplePrefix) else { return false }
        let platform = String(first.context.triple.dropFirst(triplePrefix.count))
        guard platform.range(of: #"^(?:macos|ios)[0-9]+\.[0-9]+\.[0-9]+(?:-simulator)?$"#,
                             options: .regularExpression) != nil else { return false }
        if platform.hasPrefix("ios"), !platform.hasSuffix("-simulator"),
           first.context.architecture == "x86_64" { return false }
        switch first.context.sdk {
        case "macosx":
            guard platform.hasPrefix("macos"), !platform.hasSuffix("-simulator"),
                  URL(fileURLWithPath: first.sdkPath).lastPathComponent == "MacOSX.sdk" else { return false }
        case "iphoneos":
            guard platform.hasPrefix("ios"), !platform.hasSuffix("-simulator"),
                  URL(fileURLWithPath: first.sdkPath).lastPathComponent == "iPhoneOS.sdk" else { return false }
        case "iphonesimulator":
            guard platform.hasPrefix("ios"), platform.hasSuffix("-simulator"),
                  URL(fileURLWithPath: first.sdkPath).lastPathComponent == "iPhoneSimulator.sdk" else { return false }
        default: return false
        }
        var importedSymbols = Set<String>()
        for (candidate, declaration) in zip(candidates, declarations) {
            guard !candidate.name.isEmpty, bounded(candidate.name, maximum: 256),
                  candidate.name.range(of: #"^[A-Za-z][A-Za-z0-9_]*$"#, options: .regularExpression) != nil,
                  !candidate.symbol.isEmpty, bounded(candidate.symbol, maximum: 1024),
                  candidate.symbol == "_" + candidate.name, importedSymbols.insert(candidate.symbol).inserted,
                  !candidate.provider.isEmpty, bounded(candidate.provider, maximum: 4096),
                  isEligiblePublicSDKProvider(candidate.provider),
                  declaration.context == first.context,
                  declaration.sdkPath == first.sdkPath, declaration.sdkBuild == first.sdkBuild,
                  declaration.compiler == first.compiler, declaration.headerSHA256 == first.headerSHA256 else { return false }
            if let function = declaration.function {
                guard function.name == candidate.name, function.symbol == candidate.symbol,
                      bounded(function.name, maximum: 256), bounded(function.symbol, maximum: 1024),
                      validFunctionDeclaration(function.declaration, name: function.name),
                      validFunctionDeclaration(function.abiDeclaration, name: function.name),
                      !function.workerEligible || function.abiDeclaration.map({ !$0.isEmpty }) == true else { return false }
            }
        }
        return true
    }

    func encoded() throws -> Data {
        guard validated(for: sourceSHA256) else {
            throw DarwinSDKDeclarations.Invalid(message: "SDK declaration catalog failed its internal provenance checks.")
        }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(self)
        guard data.count <= 4 * 1024 * 1024 else {
            throw DarwinSDKDeclarations.Invalid(message: "SDK declaration catalog exceeds its byte budget.")
        }
        return data
    }
}

final class DarwinSDKLookupCancellation: @unchecked Sendable {
    private let lock = NSLock()
    private var stopped = false
    var isCancelled: Bool {
        lock.lock(); defer { lock.unlock() }
        return stopped
    }
    func cancel() {
        lock.lock(); stopped = true; lock.unlock()
    }
    func check() throws {
        if isCancelled { throw DarwinSDKDeclarations.Invalid(message: "SDK declaration lookup cancelled.") }
    }
}

enum DarwinSDKDeclarations {
    struct Invalid: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }
    private struct Claim: Hashable {
        let provider: String
        let addend: String
        let publicSystem: Bool
    }

    /// Run only from a background execution lane. File-backed output avoids a
    /// pipe deadlock if Clang prints more than the accepted AST budget.
    private static func tool(_ executable: String, _ arguments: [String], timeout: TimeInterval,
                             maxOutput: Int, cancellation: DarwinSDKLookupCancellation?) throws -> Data {
        try cancellation?.check()
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hindsight-sdk-tool-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let stdout = directory.appendingPathComponent("stdout"), stderr = directory.appendingPathComponent("stderr")
        guard FileManager.default.createFile(atPath: stdout.path, contents: nil, attributes: [.posixPermissions: 0o600]),
              FileManager.default.createFile(atPath: stderr.path, contents: nil, attributes: [.posixPermissions: 0o600]) else {
            throw Invalid(message: "Cannot create bounded SDK compiler output files.")
        }
        let output = try FileHandle(forWritingTo: stdout), diagnostics = try FileHandle(forWritingTo: stderr)
        defer { try? output.close(); try? diagnostics.close() }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = output
        process.standardError = diagnostics
        process.environment = ["LANG": "C", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"]
        let finished = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in finished.signal() }
        do { try process.run() } catch { throw Invalid(message: "Cannot launch Apple SDK compiler tool: \(error.localizedDescription)") }
        let deadline = DispatchTime.now().uptimeNanoseconds + UInt64(timeout * 1_000_000_000)
        var stoppedFor: String?
        while finished.wait(timeout: .now() + .milliseconds(50)) == .timedOut {
            if cancellation?.isCancelled == true {
                stoppedFor = "SDK declaration lookup cancelled."
            } else if DispatchTime.now().uptimeNanoseconds >= deadline {
                stoppedFor = "Apple SDK compiler tool exceeded its deadline."
            } else {
                let outputBytes = (try? FileManager.default.attributesOfItem(atPath: stdout.path)[.size] as? NSNumber)?.int64Value ?? 0
                let diagnosticBytes = (try? FileManager.default.attributesOfItem(atPath: stderr.path)[.size] as? NSNumber)?.int64Value ?? 0
                if outputBytes > maxOutput || diagnosticBytes > 32 * 1024 {
                    stoppedFor = "Apple SDK compiler output exceeds byte budget."
                }
            }
            if stoppedFor != nil {
                if process.isRunning { process.terminate() }
                if finished.wait(timeout: .now() + 2) == .timedOut {
                    Darwin.kill(process.processIdentifier, SIGKILL)
                    _ = finished.wait(timeout: .now() + 2)
                }
                break
            }
        }
        if let stoppedFor { throw Invalid(message: stoppedFor) }
        try cancellation?.check()
        let outputSize = try FileManager.default.attributesOfItem(atPath: stdout.path)[.size] as? NSNumber
        let diagnosticSize = try FileManager.default.attributesOfItem(atPath: stderr.path)[.size] as? NSNumber
        guard let outputSize, let diagnosticSize, outputSize.int64Value <= maxOutput,
              diagnosticSize.int64Value <= 32 * 1024 else {
            throw Invalid(message: "Apple SDK compiler output exceeds byte budget.")
        }
        let warning = (try? Data(contentsOf: stderr)).flatMap { String(data: $0, encoding: .utf8) } ?? ""
        guard process.terminationReason == .exit, process.terminationStatus == 0 else {
            throw Invalid(message: "Apple SDK compiler rejected declaration lookup: \(String(warning.prefix(4096))).")
        }
        return try Data(contentsOf: stdout)
    }

    /// Collect one exact public declaration. Callers retain a missing result as
    /// evidence; only `workerEligible` declarations may enter a worker catalog.
    static func collect(name: String, context: DarwinSDKTargetContext, headerPath: String,
                        cancellation: DarwinSDKLookupCancellation? = nil) throws -> DarwinSDKCollectedDeclaration {
        let environment = try collectionEnvironment(context: context, headerPath: headerPath, cancellation: cancellation)
        return try collect(name: name, environment: environment, headerPath: headerPath, cancellation: cancellation)
    }

    /// Resolve shared SDK/compiler/header identity once, then reuse it while
    /// collecting each symbol in one bounded request.
    static func collectionEnvironment(context: DarwinSDKTargetContext, headerPath: String,
                                      cancellation: DarwinSDKLookupCancellation? = nil) throws -> DarwinSDKCollectionEnvironment {
        try cancellation?.check()
        guard ["macosx", "iphoneos", "iphonesimulator"].contains(context.sdk),
              context.triple.utf8.count <= 80,
              context.triple.range(of: #"^(arm64|arm64e|x86_64)-apple-(macos|ios)[0-9]+\.[0-9]+\.[0-9]+(-simulator)?$"#, options: .regularExpression) != nil else {
            throw Invalid(message: "Invalid SDK declaration target.")
        }
        let headerURL = URL(fileURLWithPath: headerPath).standardizedFileURL
        let headerValues = try headerURL.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard headerValues.isRegularFile == true, headerValues.isSymbolicLink != true,
              let headerSize = headerValues.fileSize, headerSize <= 256 * 1024 else {
            throw Invalid(message: "SDK declaration header is not a bounded regular file.")
        }
        let header = try Data(contentsOf: headerURL)
        guard header.count == headerSize else { throw Invalid(message: "SDK declaration header changed during lookup.") }
        let headerSHA256 = SHA256.hash(data: header).map { String(format: "%02x", $0) }.joined()
        let xcrun = "/usr/bin/xcrun"
        func line(_ arguments: [String]) throws -> String {
            let data = try tool(xcrun, arguments, timeout: 10, maxOutput: 4096, cancellation: cancellation)
            guard let value = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !value.isEmpty, !value.contains("\n"), !value.contains("\r") else {
                throw Invalid(message: "Apple SDK tool returned an invalid identity.")
            }
            return value
        }
        let sdkPath = try line(["--sdk", context.sdk, "--show-sdk-path"])
        let sdkBuild = try line(["--sdk", context.sdk, "--show-sdk-build-version"])
        let clang = try line(["--sdk", context.sdk, "--find", "clang"])
        guard sdkPath.hasPrefix("/"), clang.hasPrefix("/"), sdkBuild.utf8.count <= 128 else {
            throw Invalid(message: "Apple SDK compiler identity is invalid.")
        }
        let version = try tool(clang, ["--version"], timeout: 10, maxOutput: 4096, cancellation: cancellation)
        guard let compiler = String(data: version, encoding: .utf8)?.split(separator: "\n").first.map(String.init),
              !compiler.isEmpty else { throw Invalid(message: "Apple Clang version is unavailable.") }
        return DarwinSDKCollectionEnvironment(context: context, sdkPath: sdkPath, sdkBuild: sdkBuild,
                                              clang: clang, compiler: compiler, headerSHA256: headerSHA256)
    }

    static func collect(name: String, environment: DarwinSDKCollectionEnvironment, headerPath: String,
                        cancellation: DarwinSDKLookupCancellation? = nil) throws -> DarwinSDKCollectedDeclaration {
        try cancellation?.check()
        guard cIdentifier(name), ["macosx", "iphoneos", "iphonesimulator"].contains(environment.context.sdk),
              environment.context.triple.utf8.count <= 80,
              environment.context.triple.range(of: #"^(arm64|arm64e|x86_64)-apple-(macos|ios)[0-9]+\.[0-9]+\.[0-9]+(-simulator)?$"#, options: .regularExpression) != nil else {
            throw Invalid(message: "Invalid SDK declaration target or C name.")
        }
        let headerURL = URL(fileURLWithPath: headerPath).standardizedFileURL
        let headerValues = try headerURL.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard headerValues.isRegularFile == true, headerValues.isSymbolicLink != true,
              let headerSize = headerValues.fileSize, headerSize <= 256 * 1024 else {
            throw Invalid(message: "SDK declaration header is not a bounded regular file.")
        }
        let header = try Data(contentsOf: headerURL)
        guard header.count == headerSize,
              SHA256.hash(data: header).map({ String(format: "%02x", $0) }).joined() == environment.headerSHA256 else {
            throw Invalid(message: "SDK declaration header changed after environment resolution.")
        }
        let context = environment.context
        let sdkPath = environment.sdkPath
        let sdkBuild = environment.sdkBuild
        let compiler = environment.compiler
        let headerSHA256 = environment.headerSHA256
        let clang = environment.clang
        let ast = try tool(clang, ["-target", context.triple, "-isysroot", sdkPath,
                                   "-x", "objective-c", "-std=c17", "-fblocks", "-fno-modules", "-fsyntax-only",
                                   "-Xclang", "-ast-dump=json", "-Xclang", "-ast-dump-filter", "-Xclang", name,
                                   headerURL.path], timeout: 60, maxOutput: 4 * 1024 * 1024,
                           cancellation: cancellation)
        try cancellation?.check()
        guard try Data(contentsOf: headerURL) == header else {
            throw Invalid(message: "SDK declaration header changed during compiler lookup.")
        }
        return DarwinSDKCollectedDeclaration(context: context, sdkPath: sdkPath, sdkBuild: sdkBuild,
            compiler: compiler, headerSHA256: headerSHA256, function: try declaration(filteredAST: ast, name: name))
    }

    /// The worker accepts only scalar ABI declarations. Missing and unsupported
    /// matches remain in `results` for the caller's review/status record.
    static func workerPrototypes(candidates: [DarwinSDKImportCandidate], results: [DarwinSDKCollectedDeclaration],
                                 sourceSHA256: String) throws -> Data? {
        guard !candidates.isEmpty, candidates.count == results.count, candidates.count <= 128,
              sourceSHA256.utf8.count == 64,
              sourceSHA256.allSatisfy({ "0123456789abcdef".contains($0) }),
              let first = results.first else { throw Invalid(message: "Invalid SDK prototype collection.") }
        var lines = ["# source_sha256=\(sourceSHA256)", "# producer=hindsight-apple-clang-declarations-v1",
                     "# target=\(first.context.triple)", "# sdk=\(first.sdkPath)",
                     "# sdk_build=\(first.sdkBuild)", "# compiler=\(first.compiler)",
                     "# header_sha256=\(first.headerSHA256)"]
        for (candidate, result) in zip(candidates, results) {
            guard result.context == first.context, result.sdkPath == first.sdkPath,
                  result.sdkBuild == first.sdkBuild, result.compiler == first.compiler,
                  result.headerSHA256 == first.headerSHA256 else {
                throw Invalid(message: "SDK identity changed while collecting import declarations.")
            }
            guard let function = result.function else { continue }
            guard function.name == candidate.name, function.symbol == candidate.symbol else {
                throw Invalid(message: "SDK declaration does not match the captured import.")
            }
            guard function.workerEligible, let declaration = function.abiDeclaration else { continue }
            lines.append("\(candidate.symbol)\t\(declaration)")
        }
        guard lines.count > 7 else { return nil }
        let data = Data((lines.joined(separator: "\n") + "\n").utf8)
        guard data.count <= 4 * 1024 * 1024 else { throw Invalid(message: "SDK prototype catalog exceeds worker byte budget.") }
        return data
    }

    /// Read the captured file itself, rather than trusting separately supplied
    /// header bytes. The whole-file digest binds the target to the project
    /// capture; the header buffer remains bounded independently of file size.
    static func verifiedTargetContext(path: String, expectedSHA256: String, expectedSize: String,
                                      cancellation: DarwinSDKLookupCancellation? = nil) throws -> DarwinSDKTargetContext {
        try cancellation?.check()
        guard expectedSHA256.utf8.count == 64,
              expectedSHA256.allSatisfy({ "0123456789abcdef".contains($0) }),
              let size = UInt64(expectedSize) else {
            throw Invalid(message: "Invalid captured source identity for SDK lookup.")
        }
        let descriptor = Darwin.open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard descriptor >= 0 else { throw Invalid(message: "Cannot open captured source for SDK lookup.") }
        defer { Darwin.close(descriptor) }
        var before = stat(), after = stat(), atPath = stat()
        guard fstat(descriptor, &before) == 0, (before.st_mode & S_IFMT) == S_IFREG,
              before.st_size >= 0, UInt64(before.st_size) == size else {
            throw Invalid(message: "Captured SDK source is not the recorded regular file.")
        }
        var hasher = SHA256(), header = Data(), total: UInt64 = 0
        let headerLimit = 16 * 1024 * 1024 + 32
        var buffer = [UInt8](repeating: 0, count: 65_536)
        while true {
            try cancellation?.check()
            let count = buffer.withUnsafeMutableBytes { Darwin.read(descriptor, $0.baseAddress, $0.count) }
            if count < 0 && errno == EINTR { continue }
            guard count >= 0 else { throw Invalid(message: "Cannot read captured SDK source.") }
            if count == 0 { break }
            let chunk = Data(buffer.prefix(count))
            hasher.update(data: chunk)
            total += UInt64(count)
            guard total <= size else { throw Invalid(message: "Captured SDK source grew during lookup.") }
            if header.count < headerLimit { header.append(chunk.prefix(headerLimit - header.count)) }
        }
        func sameFile(_ lhs: stat, _ rhs: stat) -> Bool {
            lhs.st_dev == rhs.st_dev && lhs.st_ino == rhs.st_ino && lhs.st_size == rhs.st_size &&
            lhs.st_mtimespec.tv_sec == rhs.st_mtimespec.tv_sec && lhs.st_mtimespec.tv_nsec == rhs.st_mtimespec.tv_nsec &&
            lhs.st_ctimespec.tv_sec == rhs.st_ctimespec.tv_sec && lhs.st_ctimespec.tv_nsec == rhs.st_ctimespec.tv_nsec
        }
        guard fstat(descriptor, &after) == 0, lstat(path, &atPath) == 0,
              (atPath.st_mode & S_IFMT) == S_IFREG,
              sameFile(before, after), sameFile(before, atPath), total == size,
              hasher.finalize().map({ String(format: "%02x", $0) }).joined() == expectedSHA256 else {
            throw Invalid(message: "Captured SDK source changed or differs from its recorded digest.")
        }
        try cancellation?.check()
        return try targetContext(machoCommands: header)
    }

    /// Inspect only captured thin Mach-O header/load-command bytes. Source
    /// hashing and file-stability checks belong to the collecting caller.
    static func targetContext(machoCommands data: Data) throws -> DarwinSDKTargetContext {
        guard data.count >= 32, data.count <= 16 * 1024 * 1024 + 32 else {
            throw Invalid(message: "SDK target header exceeds bounds.")
        }
        let bytes = [UInt8](data)
        func u32(_ offset: Int) -> UInt32 {
            (0..<4).reduce(UInt32(0)) { $0 | UInt32(bytes[offset + $1]) << ($1 * 8) }
        }
        guard u32(0) == 0xfeedfacf else { throw Invalid(message: "SDK lookup requires a thin little-endian 64-bit Mach-O.") }
        let architecture: String
        switch u32(4) {
        case 0x0100000c:
            let subtype = u32(8) & 0x00ffffff
            guard subtype <= 2 else { throw Invalid(message: "Unsupported AArch64 SDK CPU subtype.") }
            architecture = subtype == 2 ? "arm64e" : "arm64"
        case 0x01000007: architecture = "x86_64"
        default: throw Invalid(message: "Unsupported SDK target architecture.")
        }
        let count = Int(u32(16)), size = Int(u32(20))
        guard count <= 8192, size <= 16 * 1024 * 1024, size <= data.count - 32 else {
            throw Invalid(message: "SDK target load-command budget or framing mismatch.")
        }
        var cursor = 32, targets = Set<[UInt32]>()
        for _ in 0..<count {
            guard cursor + 8 <= 32 + size else { throw Invalid(message: "Truncated SDK target load command.") }
            let command = u32(cursor), length = Int(u32(cursor + 4))
            guard length >= 8, length % 8 == 0, length <= 32 + size - cursor else {
                throw Invalid(message: "Invalid SDK target load-command length.")
            }
            if command == 0x32 {
                guard length >= 24, UInt64(u32(cursor + 20)) * 8 + 24 <= UInt64(length) else {
                    throw Invalid(message: "Invalid Mach-O build-version record.")
                }
                targets.insert([u32(cursor + 8), u32(cursor + 12)])
            } else if command == 0x24 {
                guard length == 16 else { throw Invalid(message: "Invalid legacy macOS minimum-version record.") }
                targets.insert([1, u32(cursor + 8)])
            } else if command == 0x25 {
                // Legacy iPhoneOS commands do not explicitly distinguish the
                // simulator. Require modern platform evidence instead.
                throw Invalid(message: "Legacy iOS SDK platform is not explicit; automatic lookup withheld.")
            }
            cursor += length
        }
        guard cursor == 32 + size, targets.count == 1, let target = targets.first, target[1] != 0 else {
            throw Invalid(message: "Missing or conflicting SDK target platform/version evidence.")
        }
        let sdk: String, platform: String
        switch target[0] {
        case 1: sdk = "macosx"; platform = "macos"
        case 2:
            guard architecture != "x86_64" else { throw Invalid(message: "Device iOS platform conflicts with x86_64 architecture.") }
            sdk = "iphoneos"; platform = "ios"
        case 7: sdk = "iphonesimulator"; platform = "ios"
        default: throw Invalid(message: "Unsupported automatic SDK platform.")
        }
        let version = "\(target[1] >> 16).\((target[1] >> 8) & 255).\(target[1] & 255)"
        return DarwinSDKTargetContext(architecture: architecture, sdk: sdk,
            triple: "\(architecture)-apple-\(platform)\(version)" + (target[0] == 7 ? "-simulator" : ""))
    }

    private static func cIdentifier(_ value: String) -> Bool {
        value.utf8.count <= 256 && value.range(of: #"^[A-Za-z_][A-Za-z0-9_]*$"#, options: .regularExpression) != nil
    }

    /// Clang's filtered AST is a stream of JSON objects, not necessarily a
    /// single JSON document. Bound framing before Foundation allocates trees.
    private static func astDocuments(_ data: Data) throws -> [[String: Any]] {
        guard data.count <= 4 * 1024 * 1024 else { throw Invalid(message: "Filtered Clang AST exceeds byte budget.") }
        let bytes = [UInt8](data)
        var cursor = 0, documents: [[String: Any]] = []
        while cursor < bytes.count {
            while cursor < bytes.count && [9, 10, 13, 32].contains(bytes[cursor]) { cursor += 1 }
            if cursor == bytes.count { break }
            guard bytes[cursor] == 123, documents.count < 512 else { throw Invalid(message: "Invalid or excessive Clang AST documents.") }
            let start = cursor
            var stack: [UInt8] = [], quoted = false, escaped = false
            repeat {
                let byte = bytes[cursor]
                if quoted {
                    if escaped { escaped = false }
                    else if byte == 92 { escaped = true }
                    else if byte == 34 { quoted = false }
                } else if byte == 34 { quoted = true }
                else if byte == 123 || byte == 91 {
                    stack.append(byte)
                    guard stack.count <= 256 else { throw Invalid(message: "Clang AST nesting exceeds budget.") }
                } else if byte == 125 || byte == 93 {
                    guard stack.popLast() == (byte == 125 ? 123 : 91) else { throw Invalid(message: "Mismatched Clang AST framing.") }
                }
                cursor += 1
            } while cursor < bytes.count && (!stack.isEmpty || quoted)
            guard stack.isEmpty, !quoted,
                  let document = try JSONSerialization.jsonObject(with: Data(bytes[start..<cursor])) as? [String: Any] else {
                throw Invalid(message: "Incomplete Clang AST document.")
            }
            documents.append(document)
        }
        return documents
    }

    /// Preserve source spellings while comparing canonical parameter types.
    /// Top-level restrict is absent from a C function type; pointed-to const
    /// and all other shape remain part of the conflict check.
    static func declaration(filteredAST: Data, name: String) throws -> DarwinSDKFunctionDeclaration? {
        guard cIdentifier(name) else { throw Invalid(message: "Invalid SDK C function name.") }
        let matches = try astDocuments(filteredAST).filter { $0["kind"] as? String == "FunctionDecl" && $0["name"] as? String == name }
        guard let node = matches.last else { return nil }
        func parameters(_ node: [String: Any]) throws -> [[String: Any]] {
            if node["inner"] == nil { return [] }
            guard let children = node["inner"] as? [[String: Any]] else { throw Invalid(message: "Invalid SDK parameter records.") }
            return children.filter { $0["kind"] as? String == "ParmVarDecl" }
        }
        func spelling(_ node: [String: Any], canonical: Bool) -> String? {
            guard let types = node["type"] as? [String: Any] else { return nil }
            return (canonical ? types["desugaredQualType"] as? String : nil) ?? types["qualType"] as? String
        }
        var signatures = Set<[String]>()
        for match in matches {
            guard let functionType = spelling(match, canonical: false) else { throw Invalid(message: "SDK declaration has no function type.") }
            var key = [functionType]
            for parameter in try parameters(match) {
                guard let type = spelling(parameter, canonical: true) else { throw Invalid(message: "SDK parameter has no type.") }
                key.append(type.replacingOccurrences(of: #"\s*(?:\brestrict|\b__restrict|\b__restrict__)\s*$"#, with: "", options: .regularExpression).trimmingCharacters(in: .whitespaces))
            }
            signatures.insert(key)
        }
        guard signatures.count == 1 else { throw Invalid(message: "Conflicting SDK declarations for \(name).") }
        func render(canonical: Bool) throws -> String? {
            guard let functionType = spelling(node, canonical: false), !functionType.contains("__attribute__"),
                  functionType.filter({ $0 == "(" }).count == 1,
                  functionType.filter({ $0 == ")" }).count == 1, functionType.hasSuffix(")"),
                  let opening = functionType.firstIndex(of: "(") else { return nil }
            let result = functionType[..<opening].trimmingCharacters(in: .whitespaces)
            guard !result.isEmpty, result.rangeOfCharacter(from: CharacterSet(charactersIn: ";{}[]\n\r\t")) == nil else { return nil }
            let parameterRecords = try parameters(node)
            let argumentText = String(functionType[functionType.index(after: opening)..<functionType.index(before: functionType.endIndex)]).trimmingCharacters(in: .whitespaces)
            guard !argumentText.isEmpty else { return nil } // C no-prototype type is not (void).
            let argumentTypes = argumentText.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
            let variadic = node["variadic"] as? Bool == true
            let fixedCount = argumentText == "void" ? 0 : argumentTypes.count - (variadic ? 1 : 0)
            guard fixedCount == parameterRecords.count, variadic == (argumentTypes.last == "...") else { return nil }
            var arguments: [String] = []
            for (index, parameter) in parameterRecords.enumerated() {
                guard let type = spelling(parameter, canonical: canonical),
                      type.rangeOfCharacter(from: CharacterSet(charactersIn: "()[]{}\n\r\t;")) == nil else { return nil }
                let parameterName = parameter["name"] as? String ?? "arg\(index)"
                guard cIdentifier(parameterName) else { return nil }
                arguments.append("\(type) \(parameterName)")
            }
            if variadic { arguments.append("...") }
            let declaration = "\(result) \(name)(\(arguments.isEmpty ? "void" : arguments.joined(separator: ", ")));"
            guard declaration.utf8.count <= 1024,
                  declaration.rangeOfCharacter(from: CharacterSet(charactersIn: "\n\r\t\\{}#=\"")) == nil else { return nil }
            return declaration
        }
        let symbol = node["mangledName"] as? String ?? "_" + name
        let source = try render(canonical: false), abi = try render(canonical: true)
        return DarwinSDKFunctionDeclaration(name: name, symbol: symbol, declaration: source, abiDeclaration: abi,
            workerEligible: abi != nil && node["storageClass"] as? String != "static" && symbol == "_" + name)
    }

    /// Match the bounded worker linkage section, preserving contradictory
    /// providers instead of silently selecting the first matching library.
    static func importCandidates(linkageRecords: String) throws -> [DarwinSDKImportCandidate] {
        guard linkageRecords.utf8.count <= 65_535 else {
            throw Invalid(message: "SDK linkage section exceeds byte budget.")
        }
        let row = try NSRegularExpression(pattern: #"^symbol=(_[A-Za-z][A-Za-z0-9_]*) import\[\d+\] dylib=(\S+) addend=(-?\d+) weak=(yes|no)(?:\s|$)"#)
        var claims: [String: Set<Claim>] = [:]
        for line in linkageRecords.split(separator: "\n") {
            let value = String(line)
            guard let match = row.firstMatch(in: value, range: NSRange(value.startIndex..., in: value)),
                  let symbolRange = Range(match.range(at: 1), in: value),
                  let providerRange = Range(match.range(at: 2), in: value),
                  let addendRange = Range(match.range(at: 3), in: value) else { continue }
            let symbol = String(value[symbolRange]), provider = String(value[providerRange])
            let claim = Claim(provider: provider, addend: String(value[addendRange]),
                              publicSystem: isEligiblePublicSDKProvider(provider))
            claims[symbol, default: []].insert(claim)
        }
        var result: [DarwinSDKImportCandidate] = []
        for symbol in claims.keys.sorted() {
            guard let identities = claims[symbol], identities.count == 1,
                  let claim = identities.first, claim.publicSystem,
                  Int64(claim.addend) == 0 else { continue }
            guard symbol.utf8.count <= 257 else {
                throw Invalid(message: "SDK C function name exceeds byte budget.")
            }
            result.append(DarwinSDKImportCandidate(name: String(symbol.dropFirst()), symbol: symbol, provider: claim.provider))
        }
        guard result.count <= 128 else {
            throw Invalid(message: "SDK lookup exceeds 128-symbol budget.")
        }
        return result
    }
}
