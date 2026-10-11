# Apple Silicon Reverse Engineering App Specification

Version 1.0 • 26 September 2026 • Status: proposed engineering baseline

## 1 Purpose and principal decisions

Build a native macOS workbench for disassembly, binary inspection, reverse engineering, controlled execution, pseudocode generation, and translation of supported machine code or structured pseudocode into C. Design the normal workflow around an Apple M2 Mac mini with 8 GB of unified memory; remain compatible with M1 and newer through capability detection.

Use Xcode as the integration, signing, profiling, testing, and packaging environment. Xcode is an IDE, not an implementation language. Use Swift and AppKit/SwiftUI for the application, Rust for analysis and resource management, and Python for optional scripts and integrations. Reuse mature C/C++ engines behind narrow adapters where rebuilding them would sacrifice correctness or years of development.

The core product must work offline without an LLM, Java runtime, virtual machine, or network account. An 8 GB Mac must remain usable during analysis. Large jobs must produce incremental results, pause cleanly, and resume from disk.

Hardware virtualization is an execution backend for compatible workloads. It is not an implementation of the complete Apple M-series SoC, an iPhone emulator, or an automatic way to run arbitrary ARM firmware. Readable pseudocode is a reconstruction; generated C is a separately validated translation. Neither is represented as recovery of the original source.

Throughout this specification, MUST denotes a release requirement, SHOULD a preferred implementation, and MAY an optional extension. Resource figures and performance figures are proposed budgets and acceptance targets, not measured product capabilities. Sources in section 22 support platform and dependency facts; the architecture and numerical budgets are engineering proposals.

## 2 Product scope and supported inputs

| Input or target | Initial release requirement | Later scope or limitation |
| --- | --- | --- |
| ARM64 Mach-O executables, dylibs, objects and bundles | Parse, inspect, disassemble, recover functions, cross-reference and selectively decompile | Execute complete apps through a real macOS environment, not a synthetic function emulator |
| Universal Mach-O | Enumerate slices and select ARM64 explicitly | Never treat slices as one address space |
| ARM64e Mach-O | Inspect subtype, disassemble supported instructions, report semantic coverage | PAC-sensitive execution requires a separately qualified backend and explicit policy |
| macOS app bundles | Locate executable, nested frameworks and extensions without launching them | Analyse each image independently, then build dependency links |
| Raw AArch64 code | Require base address, endianness, ISA profile and entry point | No guessed operating system or peripheral model |
| Assembly source | Import a declared AArch64 dialect, assemble through LLVM tooling, retain relocations | External symbols require definitions or explicit imports |
| Structured pseudocode | Parse the product's versioned typed subset | Free-form prose and arbitrary decompiler text need manual conversion or a reviewed AI proposal |
| x86-64 Mach-O | Optional post-MVP static backend | Software execution is separate; no ARM hardware virtualization of x86 instructions |
| ARM64 ELF | Later static loader and function harness | Linux guest execution uses a Linux environment |
| IPSW, firmware archives and dyld shared caches | Post-MVP adapters with bounded streaming import | No automatic boot promise, decryption, key derivation or full extraction into RAM |
| Encrypted or incomplete images | Identify unavailable bytes and continue inspecting available metadata | Block semantic claims for encrypted or absent code |

Full Swift source reconstruction, complete Objective-C runtime emulation, whole-system iOS emulation, Secure Enclave emulation, Apple GPU emulation, and kernel debugging are outside the initial release. Swift and Objective-C binaries remain valid static analysis inputs, with the language recovery limits described below.

## 3 User workflow

1. Open a binary, app bundle, assembly file or existing project. Import performs no execution.
2. See a quick summary: architectures, segments, signatures, imports, symbols, encryption flags, entry points, dependencies and warnings.
3. Select an analysis scope: one function, selected regions, executable sections, or background project analysis. Display resource estimates before expensive work.
4. Navigate synchronized hex, disassembly, control-flow graph, pseudocode, strings and references. Selecting a statement highlights its originating instruction ranges.
5. Rename functions, add types, specify calling conventions, annotate uncertain edges and save hypotheses independently of recovered facts.
6. Request C export for selected functions. Choose machine-state translation or higher-level reconstruction and inspect unresolved dependencies.
7. Optionally create an execution harness, define inputs and imported-call behavior, select a qualified backend and run under a bounded resource policy.
8. Compare registers, memory changes, return values and call traces between original code and generated C. Export results with their assumptions and validation status.

Project reopening must restore navigation, annotations and completed analysis without rescanning the entire input. A visible stop control must remain available during every operation.

## 4 Language and component ownership

| Component | Implementation | Reason and boundary |
| --- | --- | --- |
| macOS application | Swift, SwiftUI shell, AppKit virtualized editors | Native accessibility, menus, windowing and efficient large text views |
| Apple framework adapters | Swift with minimal C shims where needed | Virtualization, Hypervisor, Metal, Core ML and platform lifecycle integration |
| Project and analysis coordinator | Rust | Typed data model, deterministic jobs, memory admission, cache control and concurrency |
| Loaders and binary inspection | Rust | Bounds-checked parsing and isolation of malformed input |
| Instruction decoding | LLVM MC through a small version-pinned C++ adapter | Mature AArch64 decoding; text disassembly alone is not instruction semantics [S5] |
| Lifting and core analysis | Rust semantic IR and analysis passes | Explicit machine semantics and provenance, independent of presentation |
| Established decompiler adapter | Ghidra-derived native SLEIGH/decompiler worker, feasibility-gated | Reuse mature lifting and structuring; this is a real integration project, not a turnkey API [S6, S7] |
| Function execution | Unicorn worker first, qualified Hypervisor backend later | Software control and hardware-assisted execution serve different observability needs [S1, S8] |
| Debugger integration | Isolated LLDB adapter | Process debugging and optional Python scripting through supported interfaces [S9] |
| Automation and plugins | Bundled, pinned CPython subprocess | Flexible scripts without placing Python on the UI or per-instruction hot path |
| Persistence | SQLite plus immutable blob files | Incremental results, transactions and bounded reads |
| Optional inference | Separate Swift/Core ML service; alternative model runtime only after profiling | Best-effort suggestions with a strict model memory budget |

Do not implement the UI in Python, an interpreter loop in SwiftUI, or a replacement decoder solely to satisfy a language preference. Do not load arbitrary Python extensions into the application process.

## 5 Process architecture and interfaces

The app owns presentation and user intent. A Rust coordinator owns project state and scheduling. Dedicated workers perform parsing, decoding, decompilation, execution, debugging, scripting and inference. The VM service owns guest lifecycle and must remain independent of analysis worker crashes.

Use XPC for signed Apple helper connections. Wrap Rust services in a minimal Swift helper where necessary rather than inventing an undocumented Rust-to-XPC ABI. Use a versioned framed protocol over private pipes or Unix sockets between internal workers. Reject unknown major protocol versions and oversized messages.

Control messages contain a protocol version, request ID, project ID, input hash, analysis revision, job kind, cancellation token, resource lease and payload reference. Event messages contain sequence number, progress, completed item IDs, diagnostics, coverage and terminal status. Addresses are structured as image ID plus unsigned offset; JSON addresses use hexadecimal strings to avoid numeric precision loss.

Bulk bytes use read-only file descriptors or bounded shared mappings. No multi-megabyte JSON arrays of instructions or register traces. Use paginated queries and backpressure. Initial limits: 256 KiB control frame, 1,000 result rows per page, and a 16 MiB bounded event queue.

Rust exposed through C ABI uses opaque handles, explicit ownership and buffer lengths. Swift must release Rust allocations through the owning API. Rust panics and C++ exceptions must never unwind across FFI. All unsafe blocks require documented invariants and targeted tests.

Jobs follow queued, admitted, running, checkpointing, completed, cancelled, failed or unsupported states. A cancelled or killed worker cannot commit a partial result as complete. The coordinator is the only database writer; workers submit validated transactions or content-addressed result blobs.

## 6 Import and static analysis

### 6.1 Safe bounded import

Open source files read-only. Record SHA-256, file length, metadata, selected slice and loader version. A fast preliminary view may precede hashing but must be marked provisional; detect file changes during import and restart or reject inconsistent reads. Never identify an input only by filename.

Check every offset, size, integer addition and section range. Validate Mach-O load commands, segment permissions, symbol and string tables, relocation records, exports, imports, function starts, compact unwind data and chained fixups. Retain file offsets, unslid virtual addresses and runtime addresses as distinct concepts.

For future archive import, enforce expanded-size, compression-ratio, entry-count and recursion limits; reject path traversal and escaping symlinks. Hash and decompress in bounded chunks, provide cancellation, and never eagerly expand an IPSW into memory. Large-file mapping is allowed only with bounded access and resident-page monitoring; a mapping alone does not make a workload memory-safe.

### 6.2 Analysis pipeline

The pipeline is load, discover, decode, lift, build CFG, analyse data flow, infer types, structure control flow, render and optionally export. Store each stage independently so users can inspect assembly even when decompilation fails.

Seed functions from symbols, entry points, unwind information, function-start tables and direct call targets. Use recursive traversal first. A separate linear sweep may find candidate code but must mark it as speculative. Preserve code/data ambiguity, overlapping hypotheses, tail calls, veneers, thunks, literal pools, jump tables and unresolved indirect branches.

Every decoded instruction records bytes, address, ISA feature requirements, operands, decoder version and status. Every lifted instruction additionally records semantic coverage. A successfully printed mnemonic does not imply executable or exportable semantics.

Display incomplete CFG edges explicitly. A large function exceeding a budget returns partial analysis with reasons, not a frozen application or invented structured code.

### 6.3 Apple language metadata

Recover Objective-C classes, protocols, categories, selectors and method tables where available. Treat dynamic message sends as candidate dispatch sets unless resolved by evidence. Recover Swift symbols, demangled names and version-qualified metadata when supported. Optimized Swift generics, ARC, closures and async functions may retain runtime calls and low-level representations.

Keep original mangled names and raw metadata beside inferred names. Never represent a guessed source type as a proven one. PAC, authenticated fixups and authenticated branches require explicit representation; dropping authentication bits silently is forbidden.

## 7 Intermediate representation and pseudocode

Use a versioned, typed, bit-vector IR. Include general-purpose registers, vector registers, NZCV flags, memory spaces, endianness, alignment, exception effects and ordered memory operations. Preserve instruction origins through every transformation.

IR operations include arithmetic, bitwise and shift operations, extensions, extraction and concatenation, loads/stores, conditional branches, calls, returns, intrinsics and explicit unsupported effects. Add SSA values and memory dependencies for analysis without assuming that unknown calls leave memory unchanged.

Implement constant propagation, dead-value elimination, dominators, loop recovery, stack-slot recovery, conservative alias analysis and calling-convention inference. Bound the number of iterations and propagate uncertainty. Floating-point operations preserve rounding and exceptional cases where supported; atomics and barriers must not be reduced to ordinary loads/stores.

Pseudocode offers two synchronized views: a faithful low-level form and a structured form. Preserve gotos when structuring would obscure or alter the CFG. Suggested variable names are editable; factual addresses remain recoverable. User type overrides create a new analysis revision and invalidate dependent results.

Each function reports decoded byte coverage, semantic coverage, unresolved edges, unknown calls, inferred types and validation evidence. Avoid a single unexplained confidence percentage.

### 7.1 Versioned pseudocode input language

Define a restricted grammar supporting fixed-width integers, explicitly sized pointers, typed parameters, locals, expressions, if/else, loops, switch, labels, goto, calls and returns. Define integer overflow, shifts and memory access in the language specification. Reject ambiguous types and syntax with line-level diagnostics.

Product-generated structured text must round-trip through this parser when it contains only supported constructs. Imported natural-language pseudocode is a draft for human review, not an executable specification. AI-assisted conversion must produce a typed AST and disclose assumptions before C export.

## 8 C generation and correctness contract

Provide three distinct outputs.

| Output | Purpose | Required label |
| --- | --- | --- |
| Readable pseudocode | Understand likely program behavior | Reconstructed and possibly incomplete |
| Machine-state C | Express supported instruction behavior with registers and checked guest-memory helpers | Compilable translation for a declared execution model |
| Reconstructed C | Produce human-editable functions with recovered parameters and types | Compilable only when dependencies and semantics are complete |

The canonical generator consumes verified IR or the typed pseudocode AST. Do not generate authoritative C by asking an LLM to rewrite assembly text.

Export C17 with fixed-width types, explicit ABI declarations, a support runtime, dependency manifest, address-to-source map and a validation report. Emit unsigned arithmetic for modular behavior, explicit signed comparisons, defined shift helpers and correct truncation/extension. Guest memory accesses must use checked helpers rather than converting guest addresses into arbitrary host pointers. Use byte-copy operations where alignment or aliasing would otherwise cause C undefined behavior.

Unknown instructions, system calls and external functions must remain explicit dependencies. A strict export fails when any reachable operation lacks semantics. A partial export may produce a harness that traps on unsupported operations, but must be labelled incomplete and never described as a functional replacement. Do not silently insert zero returns, empty stubs or fabricated library behavior.

Recovered C may depend on Objective-C, Swift or system runtime adapters. A self-contained equivalent C program is not promised for arbitrary macOS apps. Generated C cannot recover original comments, local names or erased abstractions reliably.

Validation levels are: generated, compiled, exercised, differential-tested, and formally checked under stated assumptions. Compilation proves syntax and type acceptance only. Differential tests prove agreement on the tested cases only. Formal checking, if added, applies to an explicitly bounded model rather than an entire unknown binary.

Compile in an isolated worker. Run both versions with equivalent inputs, ABI state, mapped memory and import models. Compare return values, relevant registers, memory writes, traps and externally visible call events. Include overflow boundaries, zero, maximum values, invalid pointers and randomized inputs. Record compiler flags, engine versions and the seed.

## 9 Execution backend selection

| Backend | Suitable work | Main limitation | Initial priority |
| --- | --- | --- | --- |
| Rust IR interpreter | Semantic debugging, small tests and export validation | Slow and limited to implemented semantics | Required |
| Unicorn software CPU worker | Selected AArch64 functions and instruction traces | CPU emulation does not supply macOS frameworks or the full Apple SoC | Required after static MVP |
| LLDB on a permitted local process | Debug user-built or otherwise debuggable macOS apps | Host side effects and OS attachment restrictions | Optional explicit action |
| Hypervisor framework worker | Compatible ARM64 harnesses where guest setup is justified | Requires a monitor, mappings and trap handling; observability costs performance | Feasibility-gated |
| Virtualization framework guest | Run macOS or ARM Linux environments with their own runtime | Guest RAM and storage overhead; limited transparent host introspection | Optional on 8 GB |

Default to static analysis. For a selected function needing instruction-level visibility, prefer software execution. For a qualified hardware harness, offer Hypervisor execution with its reduced tracing guarantees. For a complete macOS app requiring system services, use a compatible macOS guest or a permitted local debug session.

Backend switching is explicit at a checkpoint with serialized state and a compatibility check. Do not silently switch execution models mid-function. An unsupported operation stops execution with a structured reason.

### 9.1 Software execution contract

A harness defines ISA profile, ABI, entry and exit addresses, registers, mapped regions, permissions, stack, imported functions, deterministic clock/RNG inputs and limits. Default to one guest thread. Initial ceilings: 10 million instructions, 5 seconds wall time, 256 MiB guest memory and 64 MiB buffered trace; users may request larger jobs through admission control.

Unmapped access, undefined instructions and unmodelled calls stop with register and memory diagnostics. Imported calls may use explicit deterministic stubs. Do not forward guest syscalls to the host. A trace distinguishes executed, modelled and unsupported behavior.

Record/replay initially covers the deterministic software harness only. It does not promise deterministic full macOS replay. Self-modifying code invalidates affected decode and translation cache entries. PAC authentication, exclusive accesses, floating-point state and advanced extensions require backend-specific tests before being advertised.

### 9.2 Hardware virtualization contract

Apple provides low-level virtual CPU and memory facilities through Hypervisor and higher-level VM management through Virtualization [S1–S3]. Use public interfaces and declared entitlements.

For Hypervisor, implement a minimal guest monitor with exception vectors, guest page tables, virtual-to-physical mappings, stack layout, return trampoline and an exception/exit dispatcher. Define what happens to SVC, system-register access, faults, interrupts and imported calls. A macOS Mach-O does not become runnable merely by mapping its text segment into a virtual CPU.

Preflight ISA features, memory granularity, privilege level and ABI. Use approved baseline AArch64 fixtures first. Host CPU features and guest-exposed features are not interchangeable. Do not claim general ARM64e support from the host model name. Never assume access to device secrets or host PAC keys.

Use batched run intervals. Dense instruction tracing may remove the speed advantage; the UI should show when software execution is the better choice. Hardware mode is not cycle-accurate Apple CPU emulation.

For Virtualization, validate the selected guest configuration and OS restore-image requirements at runtime. Keep guest identity, disk state and configuration together. Use an authenticated guest agent for debugging and trace collection; the host VM API does not automatically expose guest application state. Network, shared folders, clipboard and device forwarding are off by default and individually enabled.

Full macOS guests are a constrained optional workflow on 8 GB. If the guest minimum plus host reserve and measured overhead cannot fit, refuse admission and explain the shortfall. Never reduce guest RAM below its supported minimum. A stopped VM can release resources; pausing alone is not assumed to release guest RAM. Saved-state support must be probed; otherwise use a clean shutdown and cold restart.

Nested virtualization, iOS virtualization, bare-metal Apple firmware boot, Secure Enclave services and transparent GPU passthrough are not baseline features. Guest graphics acceleration, where supplied by Apple, is separate from unrestricted access to the physical GPU.

## 10 Acceleration strategy

Acceleration is selected by measured end-to-end benefit, not by the availability of an accelerator.

| Facility | Intended use | Guardrail |
| --- | --- | --- |
| ARM64 CPU and NEON | Byte scans, hashes where supported by vetted libraries, batched decoding and trace processing | Runtime feature detection; no unsupported ISA assumptions |
| Bounded CPU parallelism | Independent functions and image regions | Two heavy workers by default; never one worker per core without memory admission |
| Accelerate | Dense numerical work introduced by similarity or embedding features | Benchmark against Rust scalar/SIMD first |
| Metal | Large graph rendering and optional batch comparison kernels | Small workloads stay on CPU; GPU allocations count against unified memory [S10] |
| Core ML | Optional naming, summarization or function-classification models | Device placement depends on model support and runtime scheduling [S11] |
| Hypervisor | Qualified same-architecture guest execution | No automatic benefit for static analysis or detailed single-step traces |
| Virtualization | Full supported guest environments | Admit guest RAM and overhead before launch |

The Neural Engine is not a general-purpose disassembly, compiler or emulation accelerator. Core ML may use it for compatible inference; it cannot be promised for arbitrary models. Do not ship an LLM as a requirement for the main workflow.

Optional model mode starts disabled. Admit one small model only after measuring its complete memory footprint, including weights, activations and context cache. No promise that a parameter count or quantization level alone will fit. Model mode is mutually exclusive with full VM mode on the 8 GB profile. Suggestions retain model identity and remain separate from analysis facts. Remote inference is a separately enabled integration with visible data selection.

## 11 Resource budgets for an 8 GB Mac

All values below use MiB. Count resident application/service memory and committed workload allocations conservatively; report shared mappings separately to avoid double counting. The OS owns scheduling and memory reclamation, so these are controlled-workload budgets, not guarantees against system pressure.

| Normal static mode component | Budget |
| --- | ---: |
| App, visible views and coordinator | 320 |
| Database pages and indexes | 192 |
| Active image and decode windows | 256 |
| Analysis and decompiler workers combined | 768 |
| Cached IR and pseudocode | 384 |
| Optional active Python worker | 128 |
| Metal and UI working buffers | 128 |
| Transient reserve | 384 |
| Total admission envelope | 2,560 |

Target idle footprint below 350 MiB and typical interactive analysis below 1.5 GiB. The 2.5 GiB envelope is the normal admission ceiling, not a target to fill. Heavy worker native allocations count even when a runtime heap limit exists.

Function emulation uses at most a 3 GiB combined application envelope by evicting static caches and suspending background decompilation. On 8 GB, full VM mode reduces the host workbench to a proposed 768 MiB envelope and allocates the guest only after checking guest minimums, VM overhead, other application pressure and a planning reserve of at least 2 GiB for the host system. This may make some macOS guest versions unsuitable on the reference machine. ARM Linux guests may start from a proposed 1–2 GiB configuration only when their requirements permit it.

The governor tracks worker footprints, allocation reservations, Metal allocations, guest commitments, swap growth, thermal state and system memory-pressure notifications. Allocation estimates must be corrected from observed high-water marks. Enforce bounded containers and preallocation checks; use a separate watchdog for engines without reliable internal limits. Watchdog termination is recovery, not a hard kernel memory quota.

On warning pressure: stop admitting background work, evict reproducible caches, reduce workers to one and unload optional inference. On critical pressure: checkpoint and terminate or stop heavyweight services, retaining UI and saved annotations. Do not automatically restart until pressure has recovered and the user-visible job state explains what happened.

Limit default analysis cache to 5 GiB and trace storage to 1 GiB per session. User annotations and original inputs are never cache eviction candidates. Refuse new large disk writes when free space falls below the larger of 2 GiB or twice the projected write. VM creation separately checks its required disk capacity. Queue and trace overflow must report dropped data or pause collection; never silently claim a complete trace.

## 12 Interface requirements

Use a project navigator on the left, synchronized work views in the centre and a collapsible inspector on the right. A lower panel contains diagnostics, jobs and execution state. Users can hide panels for smaller displays.

Provide native menus, keyboard navigation, command search, find references, jump to address, back/forward history, bookmarking and editable names/types. Use selectable, searchable text and accessible labels. Support dark/light appearances, scalable fonts, VoiceOver and sufficient contrast. Status must not rely on colour alone.

Disassembly and hex viewers render visible rows plus a small buffer. Pseudocode editing uses a virtualized text implementation. Large graphs use level of detail, collapsed regions and selection-based expansion; do not create one SwiftUI view per instruction or graph edge. Graph layout runs outside the main thread and obeys a node budget.

The status strip shows analysis scope, worker count, resource pressure, selected backend, ISA coverage and incomplete-result warnings. It must distinguish not yet analysed, unsupported, failed and cancelled.

## 13 Project format and reproducibility

Use a versioned project directory with manifest.json, analysis.sqlite, blobs, annotations, scripts and optional traces. Store large sources by immutable copy or external reference with verified hash. Moving a referenced file prompts relocation; a changed hash creates a new image revision rather than reusing stale results.

Database entities include image, address space, section, function, instruction range, CFG edge, IR artifact, symbol, type, annotation, job, execution session and validation result. Every derived artifact records input hash, engine/version, configuration, parent revision and coverage. Key caches by those fields, not by address alone.

Use transactions, checkpoints and atomic artifact publication. Keep user annotations separate from generated analysis. Schema upgrades require a backup and reversible migration path; a newer unsupported schema opens read-only or refuses cleanly. Project archives validate paths before extraction.

Exports include selected assembly, pseudocode, C project, JSON analysis and a human-readable evidence report. A C project includes support code, build instructions, dependency list, unresolved-operation list and validation fixtures. Export timestamps must not change semantic artifact hashes.

## 14 Python extension model

Python scripts access a versioned SDK for queries, annotations, job submission and reports. Scripts receive batches or iterators, never an entire project materialized as Python dictionaries by default. Limit one Python worker to the admitted budget; scientific packages are optional and lazy-loaded.

Example SDK operations are project.images(), functions.query(), disassemble(range), references.to(address), annotations.propose(), decompile(function_id) and exports.request(). Mutating operations form a reviewable transaction and support undo. Long requests return job handles.

Treat process isolation and security isolation as distinct. A subprocess alone does not prevent filesystem or network access. Restricted plugins must run in a tested OS-enforced sandbox with brokered project access. If that restriction cannot be enforced, label the environment trusted-script mode and require an explicit launch action. Never auto-run code embedded in a project.

Bundle a compatible CPython rather than relying on a system Python. Pin runtime and extension versions, sign shipped binaries and keep native extensions out of the UI process. Provide a separate LLDB-compatible environment where its bindings require a particular runtime.

## 15 Security and macOS integration

Distribute a signed, notarized native ARM64 application outside the Mac App Store initially. Use hardened runtime. Split entitlements by helper: static analysis needs neither debugger nor hypervisor privileges; execution helpers receive only capabilities required by their backend.

JIT-capable engines must follow Apple's executable-memory rules and pass a signed release-build test on physical Apple Silicon [S12, S13]. A worker boundary alone does not solve JIT restrictions. If the chosen software engine cannot meet those requirements, ship the interpreter fallback and keep that engine disabled until qualified.

LLDB attachment obeys target permissions and macOS protections. The debugger entitlement is not universal permission to debug protected processes [S14]. Diagnose failures; do not make disabling system protections a product setup requirement.

Do not launch imported binaries, restore guest networking, run scripts, upload code or overwrite source files as side effects of opening a project. Host debugging is a distinct action because the target can affect the host. Parse and render untrusted names as data. Use capability-restricted workers where supported, least-privilege file descriptors and no root daemon or kernel extension for the initial product.

Audit shipped dependencies and their licences. LLVM/Ghidra integration and Unicorn distribution require per-component review; do not assume that a process boundary removes redistribution obligations. Pin release tags or commits and retain build manifests, notices and an SBOM. Keep updates signed and replace helpers atomically.

## 16 Build and repository organization

Use one Xcode workspace with targets for the app, platform helpers, bridge framework and macOS tests. A Cargo workspace contains crates for model, loaders, decode adapter, IR, analysis, C generation, project storage, scheduler and command-line tools. Keep Python SDK and tests in a separate versioned package. Native third-party engines build through reproducible scripts and CMake as appropriate.

Suggested paths are App, Helpers, Bridge, rust/crates, python/sdk, native/adapters, schemas, fixtures, tests, benchmarks and docs. Xcode invokes deterministic dependency build scripts with declared outputs; it must not download unpinned dependencies during an ordinary build. Lock Rust, Python and native engine versions. Build a command-line analysis frontend against the same Rust services for testing and automation.

Proposed deployment baseline: macOS 14 on ARM64, subject to dependency qualification. Build with a supported Xcode release and record the exact Xcode/macOS SDK/Rust/Python/engine matrix for each release. Guard newer APIs with availability checks and runtime capability queries. CI must not mistake an ARM Linux test for macOS framework validation.

## 17 Acceptance tests and performance targets

Use a physical M2 Mac mini with 8 GB RAM as the primary acceptance machine. Record OS/build versions, background workloads, thermal state, swap baseline and input hashes. Benchmark a signed release build separately from running under Xcode, which consumes part of the same memory budget. Publish median and p95 values over repeated cold and warm runs.

| Gate | Proposed acceptance criterion |
| --- | --- |
| Responsiveness | Common navigation p95 below 100 ms on cached results; no analysis work on the main thread |
| Small import | First useful metadata within 1 second for a local 25 MiB fixture; full hash completion reported separately |
| Large import | 1 GiB fixture provides progress and partial metadata without whole-file allocation or exceeding the static envelope |
| Cancellation | UI acknowledges within 100 ms; cooperative worker stops within 1 second; watchdog resolves an unresponsive worker within 3 seconds |
| Small function decompilation | Warm p95 below 1 second for supported fixtures up to 200 instructions; record complexity and backend |
| Memory | Static reference corpus stays within 2.5 GiB application envelope; critical-pressure response begins within 1 second |
| Recovery | Killing a worker preserves committed analysis and all saved annotations |
| C generation | Every strict supported export compiles and passes its declared differential suite; unsupported semantics reject strict export |
| Offline operation | Import, disassembly, annotation, supported decompilation and export work with networking disabled |
| Privacy | Project bytes leave the machine only through a separately enabled user action |

Maintain fixtures with known source compiled at multiple optimization levels, with/without symbols, for C, C++, Objective-C and Swift. Include signed/unsigned boundary cases, switches, indirect calls, vector operations, relocations, PAC instructions and malformed files.

Use parser fuzzing, decode comparison against LLVM tools, IR unit tests, generated-code compiler diagnostics and differential execution against native fixture programs where safe. Backend comparison cannot establish truth when engines share the same bug; retain independent source-level expected results. Test atomics, exceptions and unsupported instructions explicitly. Function recovery metrics must be measured only on fixtures with reliable ground truth.

Run integration tests for release signing, JIT allocation, debugger permission failures, VM admission refusal, archive limits, disk-full errors, pressure recovery and schema migration. Never advertise a backend or instruction family solely because its library builds successfully.

## 18 Delivery phases and exit criteria

| Phase | Deliverable | Exit criterion |
| --- | --- | --- |
| 0 Feasibility | Native shell, Rust bridge, LLVM decode, decompiler spike, software execution signing test and hardware harness spike | Measured 8 GB footprints; documented engine selection and ISA coverage; unsupported promises removed |
| 1 Static MVP | Mach-O/raw loaders, disassembly, symbols, strings, references, annotations and persistent projects | Responsive large-file handling, recovery tests and bounded memory pass |
| 2 Decompilation | Provenance-aware IR, CFG, pseudocode, type overrides and a supported function subset | Golden fixtures and explicit incomplete-analysis behavior pass |
| 3 C export | Typed pseudocode input, machine-state C, restricted reconstructed C and validation reports | Strict export gate passes across the supported corpus |
| 4 Dynamic analysis | Bounded interpreter/Unicorn harness, register/memory UI, trace comparison and LLDB integration | Signed backend tests, deterministic software replay and cancellation pass |
| 5 Hardware execution | Qualified Hypervisor harness and optional Virtualization guest management | Capability matrix, memory admission and guest-agent tests pass on reference hardware |
| 6 Expansion | Additional loaders/ISAs, advanced Swift metadata, selected Metal acceleration and optional inference | Each addition has measured benefit and does not regress the 8 GB workflow |

Do not assign a credible completion date before phase 0. A useful static workbench is much smaller than a mature decompiler, debugger and emulator combined. Scope releases by tested capabilities rather than presenting all phases as a finished first release.

## 19 Initial engineering backlog

1. Create the Xcode/Cargo bridge, protocol schema and read-only project model.
2. Implement the resource lease system, cancellation and crash-safe persistence before adding expensive engines.
3. Load ARM64 Mach-O with validated address mappings and paginated metadata.
4. Integrate LLVM decoding and synchronized hex/disassembly navigation.
5. Build function discovery, cross-references and a bounded CFG viewer.
6. Qualify a native Ghidra-derived worker against representative stripped binaries; measure actual total footprint. If it cannot fit, retain static functionality and either limit the supported Rust decompiler subset or offer an explicitly heavier external Ghidra workflow. Do not quietly replace the required low-memory mode with a JVM dependency.
7. Define the IR semantics and instruction coverage manifest; implement integer/control-flow/memory operations first.
8. Add structured pseudocode and typed overrides with provenance.
9. Add strict machine-state C export and differential fixtures before higher-level C reconstruction.
10. Qualify software execution and hardened-runtime packaging, then build hardware acceleration around demonstrated needs.

## 20 Principal risks and mitigations

| Risk | Required mitigation |
| --- | --- |
| Native decompiler integration is larger than expected | Phase 0 spike; bounded supported subset; no dependency on an unproven standalone API |
| ARM64e or advanced ISA behavior is missing | Separate decode/lift/execute/export coverage and reject unsupported paths |
| C output compiles but changes behavior | Defined bit-vector semantics, dependency contracts and differential testing |
| 8 GB host starts swapping heavily | Admission control, small caches, one heavy mode at a time and pressure-driven stop |
| Full VM leaves insufficient RAM | Runtime guest-minimum validation; disable launch when headroom is inadequate |
| Graphs or traces exhaust memory | Pagination, LOD, disk streaming and explicit collection limits |
| Third-party engine cannot satisfy code signing | Qualify release builds early; interpreter fallback |
| Plugins escape intended restrictions | Tested OS sandbox or clearly identified trusted-script execution |
| Static analysis overstates recovered intent | Preserve bytes, source mapping, hypotheses and incomplete edges |
| Acceleration makes small tasks slower | Benchmarked thresholds and CPU fallback |

## 21 Definition of the first complete product

The first complete release lets a user open an ARM64 macOS binary, navigate assembly and references, annotate and save it, inspect pseudocode for a documented supported subset, export strict compilable C for eligible functions, execute those functions in a bounded harness and inspect a truthful validation report. It stays responsive within the 8 GB resource profile.

Hardware virtualization is enabled only for qualified workloads. AI assistance remains optional. A release that only displays mock pseudocode or emits unvalidated C is a prototype, not completion of this specification.

## 22 Primary sources

These references were consulted on 26 September 2026. Pin and revalidate exact dependency versions during phase 0. Links establish available facilities and documented constraints; they do not validate the proposed app or its performance targets.

- [S1 Apple Hypervisor framework](https://developer.apple.com/documentation/hypervisor) — low-level hardware-facilitated VM and virtual CPU interfaces.
- [S2 Apple Virtualization framework](https://developer.apple.com/documentation/virtualization) — high-level guest configuration and lifecycle facilities.
- [S3 Apple WWDC Create macOS or Linux virtual machines](https://developer.apple.com/videos/play/wwdc2022/10002/) — distinction between low-level Hypervisor and full guest management.
- [S4 Apple Running macOS in a virtual machine on Apple silicon](https://developer.apple.com/documentation/virtualization/running-macos-in-a-virtual-machine-on-apple-silicon) — macOS guest setup reference.
- [S5 LLVM MCDisassembler](https://llvm.org/doxygen/classllvm_1_1MCDisassembler.html) and [llvm-mc](https://llvm.org/docs/CommandGuide/llvm-mc.html) — instruction decoding and assembly facilities.
- [S6 Ghidra project](https://github.com/NationalSecurityAgency/ghidra) — reverse-engineering framework and upstream integration/build requirements.
- [S7 Ghidra SLEIGH specification](https://ghidra.re/ghidra_docs/languages/html/sleigh.html) — processor specification and lifting model.
- [S8 Unicorn project](https://github.com/unicorn-engine/unicorn) — software CPU emulation; verify exact release coverage and licence before distribution.
- [S9 LLDB script-driven debugging](https://lldb.llvm.org/use/tutorials/script-driven-debugging.html) — debugger automation and Python interfaces.
- [S10 Apple Metal](https://developer.apple.com/metal/) — GPU graphics and compute interface.
- [S11 Apple Core ML](https://developer.apple.com/documentation/coreml) — supported model execution across CPU, GPU and Neural Engine.
- [S12 Apple Hardened Runtime](https://developer.apple.com/documentation/security/hardened-runtime) — executable code and debugging restrictions.
- [S13 Apple Porting just-in-time compilers to Apple silicon](https://developer.apple.com/documentation/apple-silicon/porting-just-in-time-compilers-to-apple-silicon) — JIT porting requirements.
- [S14 Apple Debugging tool entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.cs.debugger) — debugger authorization capability.
