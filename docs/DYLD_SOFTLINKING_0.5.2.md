# HindSight 0.5.2 — dyld and Soft-Linking Decode

## Scope

This release adds two static-linkage evidence channels and bounded shared-cache reads. `Dyld Linking` decodes legacy `LC_DYLD_INFO` / `LC_DYLD_INFO_ONLY` rebase, bind, weak-bind, and lazy-bind opcode streams. `Soft Linking` surfaces weak dylib load commands, weak import flags from both chained and opcode-based binds, explicit references to `SoftLinking.framework`, and common dynamic lookup helpers (`dlopen`, `dlsym`, `NSClassFromString`, `NSProtocolFromString`, `NSSelectorFromString`).

The decoder records segment-relative offsets, computed unslid virtual addresses, dylib ordinals, symbol names, addends, and weak-import classification. Malformed ULEB/SLEB streams and out-of-range payloads terminate bounded decoding rather than being followed.

## Shared cache reads

The dyld Cache Browser now retains validated mapping records and can translate a selected virtual address into the primary cache file mapping. Reads are capped at 64 KiB and emit a hexdump with both VM and file offsets. If an address belongs to another subcache, the read is rejected explicitly rather than assuming a file layout.

## Compatibility

The analysis report grows from 66 to 68 fields (`dyld_linking`, `softlinking`) and therefore uses engine identity `arm64-analysis-v12-schema68`. Protocol ABI remains 1. Older 66-field analysis caches are intentionally invalidated by the engine identity change.
