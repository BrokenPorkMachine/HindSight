import Foundation
import CryptoKit

@main
enum DarwinSDKDeclarationsContract {
    static func main() throws {
        func image(platform: UInt32 = 1, cpu: UInt32 = 0x0100000c, subtype: UInt32 = 0) -> Data {
            var bytes = Data(repeating: 0, count: 56)
            for (offset, value) in [(0, UInt32(0xfeedfacf)), (4, cpu), (8, subtype), (16, 1), (20, 24),
                                    (32, 0x32), (36, 24), (40, platform), (44, 14 << 16)] {
                for index in 0..<4 { bytes[offset + index] = UInt8(truncatingIfNeeded: value >> (index * 8)) }
            }
            return bytes
        }
        guard try DarwinSDKDeclarations.targetContext(machoCommands: image()).triple == "arm64-apple-macos14.0.0",
              try DarwinSDKDeclarations.targetContext(machoCommands: image(platform: 2, subtype: 2)).sdk == "iphoneos",
              try DarwinSDKDeclarations.targetContext(machoCommands: image(platform: 2, subtype: 2)).architecture == "arm64e",
              try DarwinSDKDeclarations.targetContext(machoCommands: image(platform: 7, cpu: 0x01000007)).triple == "x86_64-apple-ios14.0.0-simulator" else {
            throw DarwinSDKDeclarations.Invalid(message: "SDK target context changed platform/architecture.")
        }
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hindsight-sdk-capture-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        defer { try? FileManager.default.removeItem(at: directory) }
        let capture = directory.appendingPathComponent("capture"), link = directory.appendingPathComponent("link")
        let original = image()
        try original.write(to: capture)
        let digest = SHA256.hash(data: original).map { String(format: "%02x", $0) }.joined()
        guard try DarwinSDKDeclarations.verifiedTargetContext(path: capture.path, expectedSHA256: digest,
                expectedSize: String(original.count)).triple == "arm64-apple-macos14.0.0" else {
            throw DarwinSDKDeclarations.Invalid(message: "Verified capture lost its target context.")
        }
        func rejectCapture(_ path: String, _ hash: String, _ size: String) throws {
            do { _ = try DarwinSDKDeclarations.verifiedTargetContext(path: path, expectedSHA256: hash, expectedSize: size) }
            catch is DarwinSDKDeclarations.Invalid { return }
            throw DarwinSDKDeclarations.Invalid(message: "SDK target accepted an unverified captured source.")
        }
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: capture)
        try rejectCapture(link.path, digest, String(original.count))
        try rejectCapture(capture.path, String(repeating: "0", count: 64), String(original.count))
        try rejectCapture(capture.path, digest, String(original.count + 1))
        var changed = original; changed[44] = 15
        try changed.write(to: capture)
        try rejectCapture(capture.path, digest, String(original.count))
        func rejectImage(_ bytes: Data) throws {
            do { _ = try DarwinSDKDeclarations.targetContext(machoCommands: bytes) }
            catch { return }
            throw DarwinSDKDeclarations.Invalid(message: "Invalid SDK target acquired a guessed platform.")
        }
        try rejectImage(image(platform: 6))
        try rejectImage(image(subtype: 3))
        try rejectImage(image(platform: 2, cpu: 0x01000007))
        try rejectImage(Data(image().dropLast()))
        var conflicting = image()
        conflicting[16] = 2; conflicting[20] = 48
        conflicting.append(image(platform: 2).dropFirst(32))
        try rejectImage(conflicting)
        var badLength = image(); badLength[36] = 25
        try rejectImage(badLength)
        var badTools = image(); badTools[52] = 1
        try rejectImage(badTools)
        var noVersion = image(); noVersion[46] = 0
        try rejectImage(noVersion)
        var legacyIOS = image(); legacyIOS[32] = 0x25
        try rejectImage(legacyIOS)
        var legacyMac = Data(image().prefix(48))
        legacyMac[20] = 16; legacyMac[32] = 0x24; legacyMac[36] = 16
        legacyMac[40] = 0; legacyMac[42] = 14
        guard try DarwinSDKDeclarations.targetContext(machoCommands: legacyMac).triple == "arm64-apple-macos14.0.0" else {
            throw DarwinSDKDeclarations.Invalid(message: "Legacy macOS target lost explicit version evidence.")
        }
        func row(_ symbol: String, _ provider: String, addend: Int = 0) -> String {
            "symbol=\(symbol) import[0] dylib=\(provider) addend=\(addend) weak=no stub=0x1000\n"
        }
        let system = "/usr/lib/libSystem.B.dylib"
        var records = row("_malloc", system)
        records += row("_CFRelease", "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation")
        records += row("_private", "/System/Library/PrivateFrameworks/Hidden.framework/Hidden")
        records += row("_special", "<special>") + row("_offset", system, addend: 8)
        records += row("_collision", system) + row("_collision", "/tmp/custom.dylib")
        records += row("_malloc", system)
        let selected = try DarwinSDKDeclarations.importCandidates(linkageRecords: records)
        guard selected.map(\.name) == ["CFRelease", "malloc"], selected[1].provider == system else {
            throw DarwinSDKDeclarations.Invalid(message: "SDK selection lost provider/conflict evidence.")
        }
        func mustReject(_ records: String) throws {
            do { _ = try DarwinSDKDeclarations.importCandidates(linkageRecords: records) }
            catch is DarwinSDKDeclarations.Invalid { return }
            throw DarwinSDKDeclarations.Invalid(message: "SDK selection accepted an over-budget request.")
        }
        try mustReject(String(repeating: "x", count: 65_536))
        try mustReject((0..<129).map { row("_function\($0)", system) }.joined())
        try mustReject(row("_" + String(repeating: "f", count: 257), system))
        guard try DarwinSDKDeclarations.importCandidates(linkageRecords: "").isEmpty else {
            throw DarwinSDKDeclarations.Invalid(message: "Empty linkage acquired invented imports.")
        }
        func document(_ name: String = "sample", type: String = "unsigned long (unsigned long)", parameter: String = "size_t") throws -> Data {
            try JSONSerialization.data(withJSONObject: ["kind": "FunctionDecl", "name": name, "mangledName": "_" + name,
                "type": ["qualType": type], "inner": [["kind": "ParmVarDecl", "name": "value",
                    "type": ["qualType": parameter, "desugaredQualType": parameter == "size_t" ? "unsigned long" : parameter]]]])
        }
        let sample = try document()
        let declaration = try DarwinSDKDeclarations.declaration(filteredAST: sample + Data("\n".utf8) + sample, name: "sample")
        guard declaration?.declaration == "unsigned long sample(size_t value);",
              declaration?.abiDeclaration == "unsigned long sample(unsigned long value);",
              declaration?.workerEligible == true else {
            throw DarwinSDKDeclarations.Invalid(message: "Clang source/canonical declarations changed.")
        }
        let context = try DarwinSDKDeclarations.targetContext(machoCommands: image())
        let bound = DarwinSDKCollectedDeclaration(context: context, sdkPath: "/test/MacOSX.sdk",
            sdkBuild: "test-build", compiler: "Apple clang test", headerSHA256: String(repeating: "a", count: 64),
            function: declaration)
        let candidate = DarwinSDKImportCandidate(name: "sample", symbol: "_sample", provider: system)
        let tsv = try DarwinSDKDeclarations.workerPrototypes(candidates: [candidate], results: [bound],
            sourceSHA256: String(repeating: "b", count: 64))
        guard let tsv, let text = String(data: tsv, encoding: .utf8),
              text.hasPrefix("# source_sha256=\(String(repeating: "b", count: 64))\n"),
              text.contains("_sample\tunsigned long sample(unsigned long value);\n") else {
            throw DarwinSDKDeclarations.Invalid(message: "Native worker prototype catalog lost source/ABI identity.")
        }
        let missing = DarwinSDKCollectedDeclaration(context: context, sdkPath: bound.sdkPath,
            sdkBuild: bound.sdkBuild, compiler: bound.compiler, headerSHA256: bound.headerSHA256, function: nil)
        guard try DarwinSDKDeclarations.workerPrototypes(candidates: [candidate], results: [missing],
            sourceSHA256: String(repeating: "b", count: 64)) == nil else {
            throw DarwinSDKDeclarations.Invalid(message: "Missing SDK declaration became a worker prototype.")
        }
        do {
            _ = try DarwinSDKDeclarations.workerPrototypes(candidates: [candidate, candidate], results: [bound,
                DarwinSDKCollectedDeclaration(context: context, sdkPath: bound.sdkPath, sdkBuild: "changed",
                    compiler: bound.compiler, headerSHA256: bound.headerSHA256, function: declaration)],
                sourceSHA256: String(repeating: "b", count: 64))
            throw DarwinSDKDeclarations.Invalid(message: "Conflicting SDK build entered worker catalog.")
        } catch let error as DarwinSDKDeclarations.Invalid where error.message == "Conflicting SDK build entered worker catalog." {
            throw error
        } catch { }
        guard try DarwinSDKDeclarations.declaration(filteredAST: sample, name: "sam") == nil else {
            throw DarwinSDKDeclarations.Invalid(message: "Clang AST substring match became an exact declaration.")
        }
        let builtin = try document(type: "unsigned long (const void *)", parameter: "const void *")
        let restricted = try document(type: "unsigned long (const void *)", parameter: "const void *restrict")
        guard try DarwinSDKDeclarations.declaration(filteredAST: builtin + restricted, name: "sample")?.workerEligible == true else {
            throw DarwinSDKDeclarations.Invalid(message: "Top-level restrict created a false ABI conflict.")
        }
        for unsupported in [try document(parameter: "int (*)(int)"), try document(type: "int (*(unsigned long))(int)")] {
            guard try DarwinSDKDeclarations.declaration(filteredAST: unsupported, name: "sample")?.workerEligible == false else {
                throw DarwinSDKDeclarations.Invalid(message: "Structured declarator silently became a scalar ABI.")
            }
        }
        func rejectAST(_ data: Data) throws {
            do { _ = try DarwinSDKDeclarations.declaration(filteredAST: data, name: "sample") }
            catch { return }
            throw DarwinSDKDeclarations.Invalid(message: "Accepted malformed/conflicting Clang AST.")
        }
        try rejectAST(sample + (try document(type: "double (double)", parameter: "double")))
        try rejectAST(Data(sample.dropLast()))
        try rejectAST(Data("{\"inner\":]}".utf8))
        try rejectAST(Data(repeating: 32, count: 4 * 1024 * 1024 + 1))
        for functionType in ["int ()", "int (int)"] {
            let incomplete = try JSONSerialization.data(withJSONObject: ["kind": "FunctionDecl", "name": "sample", "type": ["qualType": functionType]])
            guard try DarwinSDKDeclarations.declaration(filteredAST: incomplete, name: "sample")?.workerEligible == false else {
                throw DarwinSDKDeclarations.Invalid(message: "Missing prototype/parameters fabricated a void signature.")
            }
        }
        try rejectAST(Data((0..<513).flatMap { _ in [UInt8](sample) }))
        try rejectAST(Data(String(repeating: "{", count: 257).utf8))
        if CommandLine.arguments.count == 3 {
            let actual = try DarwinSDKDeclarations.declaration(filteredAST: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])), name: CommandLine.arguments[2])
            guard actual?.workerEligible == true else { throw DarwinSDKDeclarations.Invalid(message: "Real SDK AST did not produce a supported declaration.") }
            print(actual!.abiDeclaration!)
        }
        if CommandLine.arguments.count == 4 && CommandLine.arguments[1] == "--macho" {
            let context = try DarwinSDKDeclarations.targetContext(machoCommands: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[2])))
            guard context.sdk == CommandLine.arguments[3] else { throw DarwinSDKDeclarations.Invalid(message: "Real Mach-O selected a different SDK.") }
            print(context.triple)
        }
        if CommandLine.arguments.count == 4 && CommandLine.arguments[1] == "--collect" {
            let context = try DarwinSDKDeclarations.targetContext(machoCommands: image())
            let result = try DarwinSDKDeclarations.collect(name: CommandLine.arguments[3], context: context,
                                                           headerPath: CommandLine.arguments[2])
            guard result.function?.workerEligible == true, result.function?.abiDeclaration != nil,
                  !result.sdkBuild.isEmpty, !result.headerSHA256.isEmpty else {
                throw DarwinSDKDeclarations.Invalid(message: "Apple Clang declaration was not bound to SDK identity.")
            }
            print(result.function!.abiDeclaration!)
        }
        print("Darwin SDK native import selection contract passed.")
    }
}
