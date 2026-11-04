import Foundation

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
    }
}
