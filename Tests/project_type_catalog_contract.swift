import Foundation
import CryptoKit

@main
struct ProjectTypeCatalogContract {
    static func main() {
        let digest = String(repeating: "a", count: 64)
        let target = "arm64-apple-macos14.0"
        let unique = ProjectTypeCandidate(name: "Word", declaration: "typedef unsigned long Word;",
            target: target, source: "analyst", sourceSHA256: digest)
        let conflict = ProjectTypeCandidate(name: "Pair", declaration: "typedef unsigned long Pair;",
            target: target, source: "analyst", sourceSHA256: digest)
        let otherConflict = ProjectTypeCandidate(name: "Pair", declaration: "typedef unsigned int Pair;",
            target: target, source: "analyst", sourceSHA256: digest)
        let stale = ProjectTypeCandidate(name: "Stale", declaration: "typedef int Stale;",
            target: target, source: "analyst", sourceSHA256: String(repeating: "b", count: 64))
        let otherTarget = ProjectTypeCandidate(name: "Other", declaration: "typedef int Other;",
            target: "arm64-apple-ios17.0", source: "analyst", sourceSHA256: digest)
        let catalog = ProjectTypeCatalog(candidates: [unique, conflict, otherConflict, stale, otherTarget])
        let resolved = catalog.resolvedDeclarations(sourceSHA256: digest, target: target)
        precondition(resolved.map(\.name) == ["Word"])
        precondition(catalog.resolvedDeclarations(sourceSHA256: String(repeating: "c", count: 64), target: target).isEmpty)
        precondition(catalog.resolvedDeclarations(sourceSHA256: digest, target: "arm64-apple-ios17.0").map(\.name) == ["Other"])
        precondition(catalog.candidates(sourceSHA256: digest, architecture: "aarch64").map(\.name).sorted()
            == ["Other", "Pair", "Pair", "Word"])
        precondition(catalog.candidates(sourceSHA256: String(repeating: "b", count: 64), architecture: "arm64").map(\.name)
            == ["Stale"])
        precondition(catalog.candidates(sourceSHA256: digest, architecture: "x86_64").isEmpty)

        let declaration = "struct Point { int x; int y; };"
        let declarationDigest = SHA256.hash(data: Data(declaration.utf8)).map { String(format: "%02x", $0) }.joined()
        let layout = ProjectTypeLayoutEvidence(schema: 1, producer: "hindsight-apple-clang-record-layout-v1",
            sdk: "macosx", sdkPath: "/Applications/Xcode.app/SDKs/MacOSX.sdk", sdkBuild: "26A425",
            target: target, compiler: "Apple clang version 21.0.0", declarationsSHA256: declarationDigest,
            record: ProjectTypeLayoutRecord(name: "Point", kind: "struct", size: 8, alignment: 4,
                fields: [ProjectTypeLayoutField(name: "x", declaration: "int x", offsetBits: 0, bitWidth: nil),
                         ProjectTypeLayoutField(name: "y", declaration: "int y", offsetBits: 32, bitWidth: nil)],
                fieldOffsetsComplete: true))
        let withLayout = ProjectTypeCandidate(name: "Point", declaration: declaration, target: target,
            source: "analyst", sourceSHA256: digest, layoutEvidence: layout)
        precondition(withLayout.validated != nil)
        let encodedCatalog = try! JSONEncoder().encode(ProjectTypeCatalog(candidates: [withLayout]))
        let decodedCatalog = try! JSONDecoder().decode(ProjectTypeCatalog.self, from: encodedCatalog)
        precondition(decodedCatalog.candidates.first?.layoutEvidence == layout)
        precondition(decodedCatalog.resolvedDeclarations(sourceSHA256: digest, target: target).first?.layoutEvidence == layout)
        precondition(withLayout.id != ProjectTypeCandidate(name: "Point", declaration: declaration,
            target: target, source: "analyst", sourceSHA256: digest).id)
        let badLayout = ProjectTypeLayoutEvidence(schema: 1, producer: "hindsight-apple-clang-record-layout-v1",
            sdk: "macosx", sdkPath: "/Applications/Xcode.app/SDKs/MacOSX.sdk", sdkBuild: "26A425",
            target: target, compiler: "Apple clang version 21.0.0", declarationsSHA256: String(repeating: "0", count: 64),
            record: layout.record)
        precondition(ProjectTypeCandidate(name: "Point", declaration: declaration, target: target,
            source: "analyst", sourceSHA256: digest, layoutEvidence: badLayout).validated == nil)
        let changedRecord = ProjectTypeLayoutRecord(name: "Point", kind: "struct", size: 12, alignment: 4,
            fields: layout.record.fields, fieldOffsetsComplete: true)
        let changedLayout = ProjectTypeLayoutEvidence(schema: 1, producer: layout.producer, sdk: layout.sdk,
            sdkPath: layout.sdkPath, sdkBuild: layout.sdkBuild, target: target, compiler: layout.compiler,
            declarationsSHA256: declarationDigest, record: changedRecord)
        let conflictingLayout = ProjectTypeCandidate(name: "Point", declaration: declaration, target: target,
            source: "analyst", sourceSHA256: digest, layoutEvidence: changedLayout)
        let layoutConflict = ProjectTypeCatalog(candidates: [withLayout, conflictingLayout])
        precondition(layoutConflict.conflicts.contains("Point"))
        precondition(layoutConflict.resolvedDeclarations(sourceSHA256: digest, target: target).isEmpty)
    }
}
