// HindSight bounded symbol tooling. Run: swift scripts/vg-symbol-tools.swift demangle <symbol>
// or: swift scripts/vg-symbol-tools.swift pseudocode <analysis-report.json>
import Foundation

func demangle(_ symbol: String) -> String {
    guard symbol.utf8.count <= 1024, !symbol.contains("\0"),
          symbol.hasPrefix("$s") || symbol.hasPrefix("_$s") || symbol.hasPrefix("$S") || symbol.hasPrefix("_T") else { return symbol }
    // swift-demangle is invoked using an argv array: symbols never reach a shell.
    let task = Process()
    if FileManager.default.isExecutableFile(atPath: "/usr/bin/xcrun") {
        task.executableURL = URL(fileURLWithPath: "/usr/bin/xcrun")
        task.arguments = ["swift-demangle", "--compact", symbol]
    } else {
        task.executableURL = URL(fileURLWithPath: "/usr/local/swift/usr/bin/swift-demangle")
        task.arguments = ["--compact", symbol]
    }
    let output = Pipe()
    task.standardOutput = output
    task.standardError = FileHandle.nullDevice
    do {
        try task.run()
        // A bounded input yields modest output for well-formed symbols; read only after exit.
        task.waitUntilExit()
        guard task.terminationStatus == 0 else { return symbol }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        guard data.count <= 16384, let value = String(data: data, encoding: .utf8) else { return symbol }
        return value.trimmingCharacters(in: .whitespacesAndNewlines)
    } catch { return symbol }
}

func pseudocode(_ ir: String) -> String {
    // This is a conservative *structural* rendering, not semantic decompilation.
    // Unknown operations are visibly represented as opaque instead of guessed.
    var out = ["// HindSight structural pseudocode (no inferred types/semantics)"]
    let maxLines = 500, maxChars = 128_000
    var consumed = 0
    for line in ir.split(separator: "\n", omittingEmptySubsequences: false).prefix(maxLines) {
        let source = String(line.prefix(400))
        consumed += source.utf8.count
        if consumed > maxChars { out.append("// truncated: size budget"); break }
        let lower = source.lowercased()
        if lower.contains("conditional branch") || lower.contains("cond_branch") {
            out.append("if (/* condition unresolved */) { goto /* target from IR */; } // " + source)
        } else if lower.contains("branch") {
            out.append("goto /* target from IR */; // " + source)
        } else if lower.contains("terminal") || lower.contains("return") {
            out.append("return /* value unresolved */; // " + source)
        } else if !source.isEmpty {
            out.append("/* opaque instruction */ // " + source)
        }
    }
    if ir.split(separator:"\n").count > maxLines { out.append("// truncated: line budget") }
    return out.joined(separator:"\n")
}

let args = CommandLine.arguments
if args.count >= 3 && args[1] == "demangle" {
    print(demangle(args.dropFirst(2).joined(separator:" ")))
} else if args.count == 3 && args[1] == "pseudocode" {
    let url = URL(fileURLWithPath: args[2]); let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
    guard let bytes = attrs[.size] as? NSNumber, bytes.intValue <= 8_388_608 else { fatalError("report too large") }
    let json = try Data(contentsOf:url)
    guard let report = try JSONSerialization.jsonObject(with:json) as? [String:Any], let ir = report["ir"] as? String else { fatalError("missing ir field") }
    print(pseudocode(ir))
} else { fputs("Usage: swift scripts/vg-symbol-tools.swift demangle <symbol> | pseudocode <report.json>\n",stderr); exit(2) }
