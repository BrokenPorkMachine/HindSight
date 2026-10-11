# HindSight 0.5.3 Audit

## Fixed in this release
- Corrected current STATUS/VALIDATION documentation that still described old 0.5.0/66-field identities after the 0.5.2 report had already moved to 68 fields.
- Added cache identity bump for the two new symbolication fields so stale reports cannot masquerade as complete symbolication results.
- Kept private Apple Symbolication decoding static; no runtime private-framework dependency was introduced.

## Incomplete / review required
1. dSYM bundle ingestion and exact LC_UUID matching before DWARF line-table use.
2. DWARF DIE, line program, inline-call, and source-path decoding. Current release inventories debug sections only.
3. dyld shared-cache local-symbols/subcache symbol recovery and per-image symbolication from cache mappings.
4. Slide-aware symbolication across debugger/runtime modules; current static records are unslid.
5. Export-trie/function-start synthesis for stripped images should be unified into a ranked symbolication resolver rather than parallel evidence sources.
6. CoreSymbolication/Symbolication private ABI decoding is name/evidence based; structure/class layouts and version-specific private ABI contracts need fixture-backed research before being labeled decoded.
7. x86_64 binary semantic analysis remains less complete than ARM64; Intel hosts are supported, but the core analysis engine is still ARM64-focused.
8. Universal/fat Mach-O slice selection needs review for symbolication identity: UUID and symbols must be attached to the selected architecture slice, not the container generically.
9. Crash/IPS report parser and automatic frame symbolication are not yet implemented.
10. Swift async/back-deployment thunk demangling and inline attribution need dedicated symbolication tests.
11. Objective-C method symbol synthesis from class metadata is not yet fed into the symbolication resolver.
12. Symbol-server/dSYM discovery policy, provenance, cache eviction, and trust model are not implemented.
13. Malformed-DWARF fuzzing and large-dSYM memory budgets are not yet part of the portable gate.
14. Native macOS Xcode/Rust link, signing, Intel launch, Apple-Silicon launch, and representative real-cache validation remain release-host gates.
