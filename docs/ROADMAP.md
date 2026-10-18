# HindSight Roadmap

The current dependency order is the [Chimera master list of completion](CHIMERA_COMPLETION_ROADMAP.md). Evidence-backed source recreation is the first completion milestone; all remaining requirements below remain in scope.

Immediate priority: AArch64 pseudocode and reconstruction for Apple binaries, using native Mach-O/dyld and Darwin runtime/SDK evidence for images, function/class/selector/method names, C declarations, properties, ivars, structs and types. Shared macOS/iOS semantics and decoder qualification are reused.

The [active delivery sequence and immediate queue](CHIMERA_COMPLETION_ROADMAP.md#active-priority-aarch64-pseudocode-and-source-reconstruction) govern execution: dependable pseudocode → canonical SSA/memory effects → ABI/native types → typed expressions/control flow → recompilable source → behavioral/fidelity qualification. Evidence and validation accompany every step; broader product work follows the first working reconstruction milestone unless it unblocks it.

## 0.1 — Genesis (foundation complete)

- [x] Independent HindSight product identity and macOS workbench.
- [x] Existing Mach-O/Apple-runtime semantic analysis retained.
- [x] Native debugger, scripting/extensions, and Hypervisor research surfaces retained.
- [x] VectorEmu extracted from reusable iPhemeral execution components.
- [x] Machine Lab workflow with registers, code, trace, regions, breakpoints, and lifecycle controls.
- [x] Evidence doctrine and Mentor Mode foundation.

**Exit gate:** clean Apple Silicon Xcode build; Cargo tests for both Rust workspaces; launch/attach/step/reset smoke test; memory-pressure test on an 8 GB M-series Mac; package/sign/notarize test.

## 0.2 — Unified Target Model (complete)

- [x] Common target/session/address-space identity across static, native, VectorEmu, and hypervisor backends.
- [x] Capability discovery rather than backend-name conditionals at the target layer.
- [x] One address pivot resolves file offset, preferred VM address, runtime slide, and declared emulated/guest mappings where a mapping is provable.
- [x] Exportable session header with target/engine/model/source fingerprints.
- [x] Engine/model identities use SHA-256; session exports hash the embedded worker, CoordinatorService, and VectorEmu executables. VectorEmu and Hypervisor trace views carry a SHA-256 trace identity. Native debugger trace identity and cryptographic source-content identity for unverified runtime images remain.

## 0.3 — Evidence Graph + Investigation Time (complete)

- [x] First-class provenance graph over static/runtime/analyst evidence.
- [x] Pin current context into bookmarks and first-class hypotheses.
- [x] Persistent investigation ledger with replayable bounded timeline.
- [x] Session snapshots that restore workbench context without claiming to rewind live targets.
- [x] Trace-to-static correlation and before/after register evidence.
- [x] Portable `.vgcase` plus Markdown/PDF/JSON investigation export.
- [x] Deterministic backend state snapshots and memory/map/control-flow diff for engines that can prove replay identity (VectorEmu snapshot/restore, Hypervisor guest state diff).

## 0.4 — Query & Recovery (complete — merged into 0.5 baseline)

- [x] Structured semantic query language across symbols, xrefs, types, runtime records, traces, and evidence.
- [x] Better function/type recovery and confidence reporting.
- [x] Searchable trace index with bounded storage.
- [x] Extension API v2 around target/evidence abstractions (Python `from hindsight import api as vg`).
- [x] Headless CLI for reproducible analysis pipelines (`vg analyze`, `vg export`, `vg diff`).
- [x] Decompiler and reverse-engineering capability track — [P0 foundation complete](DECOMPILER_GAP_ROADMAP.md): CFG, lifted IR, shared IR core, writeback preservation, function boundary normalization, shared IR coverage, variable recovery (parameter names through straight-line copies), control-flow structuring (if/else diamonds, natural loops, bounded jump tables), call-site reconstruction (direct BL with prototypes, indirect with resolved signatures, FP argument recovery), source mapping in UI (assembly/pseudocode sync, aligned mapping, corpus-measured).

## 0.5 — Evidence Core + ARM64 Semantics + Decompiler Foundation (stable at 0.5.12)

### 0.5.0 — Evidence Core (complete)
- [x] Typed evidence index off the SwiftUI render path (replaces repeated report reparsing).
- [x] Stable semantic evidence identities + address/symbol indices for Search, Triage, Related Evidence, collection health.
- [x] Backward-compatible cache envelope schema 3 (reads schema 2); project SQLite schema 1 unchanged.

### 0.5.1 — Typed Worker Evidence (complete)
- [x] Worker emits bounded typed navigation records + stage fingerprints.
- [x] Coordinator validates/caches in envelope schema 3; Swift prefers native records.
- [x] 66-field compatibility report preserved on wire and in cache.

### 0.5.2 — Dyld Soft Linking + Import Resolution (complete)
- [x] Lazy symbol pointer and non-lazy symbol pointer binding resolution.
- [x] Soft-linked framework presence checks with version constraints.

### 0.5.3 — Symbolication Refresh (complete)
- [x] Unified Symbol Resolver: exact nlist code symbols → exact indirect-symbol names → validated unnamed function starts.
- [x] Stub/pointer aliases retained; dyld import/export names without proven address remain name-only.
- [x] Resolver output feeds typed evidence index, address/symbol pivots, navigation, linkage fingerprints.

### 0.5.4 — Scalability Budgets + Hang Fixes (complete)
- [x] Bounded dominator, structural-phi, phi-operand, iterative value-flow with explicit global/per-function/pass/cardinality budgets.
- [x] Analysis progress as operational telemetry (stage + percentage), not ETA.
- [x] Off-main JSON decoding, XPC reply-size accounting, continuity fingerprinting.

### 0.5.5 — Unified Symbols Completion (complete)
- [x] 71-field compatibility report; `arm64-analysis-v15-schema71` engine identity.
- [x] Resolver precedence hardened; bounded provenance-preserving symbol view.

### 0.5.6 — Completion Audit + Crash Resolution (complete)
- [x] Production-source TODO/FIXME/unimplemented scan clean.
- [x] HFS+ file/fork traversal, eMMC persistent backing, ARM64 logical immediate validation, DART timing consumption.
- [x] Standalone crash-resolve script (text .crash + Mach-O UUID + static base).

### 0.5.7 — Scheduling + Demangling + Pseudocode Preview (complete)
- [x] Bounded priority-aware dispatch lanes (scripting/firmware) with Rust scheduling policy (memory-lease/generation gating).
- [x] Swift ABI demangler CLI.
- [x] Conservative pseudocode preview in IR workspace.

### 0.5.8 — Resource Governor + UI Polish (complete)
- [x] Resource governor with work bounds, progress exposure, cancellation without discarding completed analysis.
- [x] Settings and list-based workspaces migrated off SwiftUI `List`/`Form` (fixes NSOutlineView live-resize crash).
- [x] Evidence Graph + Flow Graph minimap alignment (click-to-jump, drag navigation, geometry preservation).

### 0.5.9 — Runtime Admission + Kernel Name Guesses (complete)
- [x] Runtime admission hardening; resolver precedence finalized.
- [x] Kernel symbol heuristics for Apple firmware images.

### 0.5.10 — Shared Admission (complete)
- [x] Shared admission logic across static/firmware/runtime paths.
- [x] Bounded metadata/linkage analysis for encrypted binaries (withholds code semantics, reports provenance).

### 0.5.11 — Analysis Admission + Lifting (complete)
- [x] Queued lifting with backpressure; staged worker progress records.
- [x] Improved CFG predecessor analysis (one-pass bounded indexes).

### 0.5.12 — Evidence Graph Navigation + Cache Control (current stable)
- [x] Evidence Graph / Flow Graph minimap sync (viewport rectangle, click-to-jump, drag, geometry preservation).
- [x] Function/symbol names consistent across evidence and runtime surfaces.
- [x] Analysis results clearable from toolbar with bounded project-local cache removal.
- [x] ARM64 semantic preview: full move-wide immediates (MOVN/MOVZ/MOVK), flag-neutral logical ops (AND/ORR/EOR immediates + shifted register forms), ADR/ADRP, conditional/indirect control flow (B.cond/CBZ/CBNZ/TBZ/TBNZ/BR/BLR with proven constant resolution), shifted arithmetic, multiply-add, sign/zero extends, bitfield extracts, general-register LDR/STR family with scaled/unsigned/register-offset addressing.
- [x] Scalar FP argument preservation through immediate materialization, GP/FP transfers, FP register copies.

**0.5 exit gate:** stable universal macOS (arm64 + x86_64) build; 8 GiB pressure test; real-target execution; signing/notarization matrix complete; source validator rejects production crash primitives.

## 0.6 — Broader Targets + Team Research (in progress)

### 0.6.1 — Binary Format Expansion (in progress)
- [x] Mach-O 32/64 (Intel + AArch64), ARM32/Thumb-2 for SEPOS composites.
- [x] Bounded ELF32/ELF64 executable/shared-image static ingestion (i386, x86-64, AArch64): validated load/section mappings, sectionless images, entry points, disassembly, CFG candidates, strings, verified paging and project capture. Big-endian images remain metadata-only.
- [x] ELF section-backed static/dynamic symbols, metadata-backed function names/starts, undefined imports, visible dynamic exports, binding/visibility provenance, and explicit TLS-offset handling. Symbols are bounded and validated against their declared sections.
- [x] Bounded ELF32/ELF64 relocatable-object ingestion with deterministic analysis-only section addresses and section-relative symbol rebasing.
- [x] ELF sectionless dynamic-link/relocation recovery (`PT_DYNAMIC`/`DT_*` relocation and symbol evidence for images without section headers): bounded `DT_RELA`/`DT_REL`/`DT_JMPREL` relocation tables, `DT_HASH`/`DT_GNU_HASH`-proven symbol counts, and `DT_SYMTAB` names/imports/exports/function starts; `DT_RELR` remains unapplied. Linux native process attach is split below because it requires a Linux release host.
- [ ] Linux native process attach (environmental gate; requires a Linux qualification host).
- [x] Bounded PE32/PE32+ image ingestion for Windows-oriented static projects (i386, x86-64, AArch64): preferred-base mappings, entry points, file-backed/zero-fill separation, disassembly, CFG candidates, strings and verified paging.
- [x] PE imports, exports and base relocations from data directories (DLL/symbol names, ordinals, export addresses and relocation sites). Standalone COFF objects, PDB and Authenticode remain separate.
- [x] Standalone COFF objects (COFF header, section table, symbols and relocations for `.obj` inputs).
- [x] PDB validation (MSF container, stream discovery and type/line/symbol evidence for Windows debug databases).
- [ ] Authenticode validation.
- [ ] Architecture abstraction beyond ARM64 where it adds real user demand (x86-64 Mach-O lifter via shared IR is P2 priority).
- [x] Hash-bound function-name exchange JSON and Ghidra/PyGhidra, IDA, and Binary Ninja adapters; worker report export, rebasing, conflict preview and verified structural-backend symbol catalogs. Adapter behavior is contract-tested; live third-party qualification remains open.
- [ ] Broader reversing artifact interoperability (types, comments, signatures and native tool project formats), GUI import and live adapter qualification.

### 0.6.2 — Decompiler Track P1/P2 (in progress — see [DECOMPILER_GAP_ROADMAP.md](DECOMPILER_GAP_ROADMAP.md))
- [x] [Mach-O prototype ABI foundation](MACHO_PROTOTYPE_ABI.md): Apple ARM64 GP/FP bank allocation, packed scalar stack locations, supported SDK aggregates and Objective-C encodings, typed return locations, callback/array pointers, explicit variadic/unknown-layout boundaries, and real compiled regression fixtures.
- [x] Prototype-backed GP argument casts with low-bit width/signedness, raw/provenance evidence, SDK/runtime ABI conflict handling, and scoped Objective-C protocol declarations with required/optional and class/instance membership; declarations do not invent implementations or dispatch targets.
- [x] Proven scalar outgoing stack arguments from bounded SP-relative stores at direct and resolved indirect calls, with typed casts and store provenance; ambiguous memory aliases and SP changes invalidate prior values.
- [x] Validated Objective-C adopted-protocol edges in recovered declarations, preserving `@protocol Child <Parent>` inheritance without inventing conformance, implementations, or dispatch targets.
- [x] Validated `class_ro_t` protocol lists as recovered `@interface Class <Protocol>` conformance declarations with explicit unresolved implementation coverage and dispatch targets.
- [x] Hash-bound project C prototype catalogs through `HINDSIGHT_PROTOTYPE_DB`, with bounded declaration validation and integration into function signatures, ABI locations, typed arguments, returns, and pseudocode calls.
- [x] Objective-C category protocol conformance when category-to-class pointers remain locally resolvable, with recovered `@interface Class (Category) <Protocol>` declarations and linked-image fallback to effective class conformance.
- [x] Bounded, cycle-checked Objective-C superclass protocol propagation with inherited conformance distinguished from direct declarations and implementation claims.
- [x] Protocol-qualified Objective-C object types from validated runtime encodings, preserving `id<Protocol>` and `Class<Protocol> *` through declarations and Apple ARM64 pointer ABI placement.
- [x] Bounded Objective-C protocol `extendedMethodTypes` recovery, joined in validated runtime method-list order so qualifiers lost from base encodings survive prototype and ABI reconstruction.
- [x] Bounded Itanium named-record pointer/reference parameter recovery for free functions, including nested names and CV/restrict qualifiers with Apple ARM64 pointer ABI placement.
- [ ] **P1 Types:** Function signatures (Swift, broader C/C++), stack locals/aggregate layouts, project type library (SDK headers, user C declarations), Objective-C dispatch/object model (class_ro_t, selector stubs, message sends), type propagation/casts, analyst overrides (persisted per-function revisions).
- [ ] **P2 Coverage:** Data references/strings (pointer/string xrefs, CFString/CFData, globals), library idiom recognition (bounds checks, tagged pointers, dispatch blocks), switch tables/indirect control flow (case-range proof, index normalization, vtable recovery), x86-64 Mach-O lifter through shared IR, stripped/optimized code handling (unwind metadata, thunk recognition, prologue evidence, call graph constraints), function/call graphs with confidence and navigation.

### 0.6.3 — Team Research Foundations (planned)
- [ ] Shareable investigation bundles with content hashing.
- [ ] Comment/review state, findings, evidence links, reproducible experiment recipes.
- [ ] Extension policy, signatures, allowlists, organization settings (capability-gated `.vgplugin` with signing/trust already in 0.5).
- [ ] Optional on-prem execution node for headless analysis pipelines.

## 1.0 — Commercial Release (target)

- [ ] Stable project schema and migrations (beyond v1).
- [ ] Stable SDK and target protocol (versioned, documented).
- [ ] Crash recovery and resumable long jobs (worker checkpointing, coordinator recovery).
- [ ] Performance budgets with regression tests (CI-enforced).
- [ ] Signed/notarized distribution and updater (sparkle or equivalent).
- [ ] Licensing tiers without crippling offline/local analysis.
- [ ] Documentation, guided labs, onboarding datasets (legally redistributable), enterprise deployment material.

## North-star engineering rule

Every release must either reduce the time from **question → evidence-backed answer**, increase reproducibility, or make advanced analysis understandable without making it less exact. Features that do none of those do not belong on the critical path.
