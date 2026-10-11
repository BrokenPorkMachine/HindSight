# HindSight 0.5.5 Completion Audit

## Scope

This audit closes actionable TODOs, stubs, placeholders, stale release identities, and silent fake-success paths found in the 0.5.5 Unified Symbols source tree. The analysis ABI remains 71 fields and the engine identity remains `arm64-analysis-v15-schema71`; none of the fixes below change report compatibility.

## Correctness defects fixed

### HFS+ filesystem reader

The previous `HfsPlusReader.read_file()` path loaded filesystem metadata but ultimately returned `NotFound` instead of resolving files. It is now a bounded read-only implementation with:

- validation of the HFS+/HFSX signature, block geometry, and declared volume size;
- catalog B-tree leaf decoding;
- CNID-based path traversal from the root folder;
- data-fork and resource-fork extraction;
- inline extent handling and extent-overflow continuation;
- bounded reads with checked arithmetic and fail-closed malformed-tree handling.

The extent-overflow file itself is intentionally bootstrapped only from its inline extents. If that metadata cannot describe the overflow tree, HindSight returns an error rather than fabricating data. Case-insensitive HFS+ lookup currently uses Unicode lowercase matching rather than Apple's complete HFS+ Unicode collation table; exact/binary HFSX comparison is honored. That is an explicit fidelity boundary, not a placeholder.

### eMMC / SDHCI storage model

The data-command path previously allowed data-present commands to complete without a real transfer. It now:

- executes commands when the command register is written rather than read;
- backs CMD17 reads and CMD24 writes with the actual `SdCard` block store;
- raises the controller error path for unsupported data commands rather than reporting fake completion;
- resets controller state without erasing card contents;
- includes regression tests for read/write persistence, unsupported-command failure, and reset semantics.

### ARM64 logical immediates

The disassembler previously had a readability fallback capable of presenting a fabricated logical-immediate value when the encoding was invalid. Invalid encodings now render explicitly as `<invalid-logical-immediate ...>` and are never converted into an invented mask.

### DART timing model

Translation timing fields were present but incompletely consumed. IOTLB hits, successful cold walks, walker faults, and early validation/stream/root faults now all feed the timing counters. Bypass remains a bypass and does not masquerade as a translated walk.

### Release identity drift

Active release tests and VectorEmu server metadata contained stale 0.5.0/0.4.3 identities. They now consistently expect/report 0.5.5, including the embedded worker smoke test, macOS test scripts, beta notarization artifact name, `vg-emu-server`, its Cargo manifest, and lockfile entry.

### Capability wording

Several UI capability descriptions called bounded hardware models “stubs” or advertised a “secure-storage mock backend”. The implemented models are now named as models. Secure storage is explicitly unavailable when no evidence-backed implementation exists, avoiding a misleading capability claim.

## Marker and semantic audit

Production source was scanned for conventional and semantic incomplete-work indicators, including `TODO`, `FIXME`, `HACK`, `XXX`, `todo!()`, `unimplemented!()`, placeholder/dummy/mock implementation wording, empty stateful handlers, and fake-success paths. No actionable production placeholder remains from that scan.

Terms such as Mach-O *stubs* and test compile stubs are domain/test terminology and were not rewritten. Stateless reset methods such as ROM/fixed-value devices intentionally remain no-ops.

## Explicit supported boundaries that remain

These are not TODOs and are not silently simulated:

- The Hypervisor.framework execution backend is unavailable on Intel hosts; the Intel native shim returns unsupported while static ARM64 analysis and VectorEmu remain cross-host.
- Full private Apple Symbolication/CoreSymbolication ABI execution is not claimed. HindSight statically decodes framework/API evidence instead of loading undocumented private frameworks.
- Full dSYM ingestion, DWARF source-line/inlining resolution, and dyld-cache local-symbol/subcache reconstruction remain separate future feature work; current symbolication reports only evidence it can prove.
- HFS+ case-insensitive matching does not yet reproduce Apple's full Unicode collation algorithm, and a pathological extents-overflow file that itself requires overflow extents fails closed.
- Hardware-register behavior without source/trace evidence remains bounded/pass-through/unsupported rather than invented.

## Validation

Portable validation completed after the fixes:

- Python/contract suite: **132 passed, 6 skipped** (the skips are optional LLVM-oracle checks).
- Swift source syntax parse: passed for `App`, `PlatformServices`, and `Shared`.
- Hypervisor shim compile contract: passed for the Apple-Silicon implementation and Intel unsupported fallback.
- All shell scripts passed `bash -n` syntax validation.
- Python sources passed `compileall`.
- Completion-audit regression tests cover release identities, HFS+ implementation presence, eMMC backing/fail-closed behavior, logical-immediate handling, DART timing integration, and virtualization capability claims.

This environment does not provide Cargo/rustc or the macOS Xcode SDK, so a native Rust workspace build and final signed universal macOS application build must still be run on the release Mac. That is an environmental release gate, not a claimed passing build.
