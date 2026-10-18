# Chimera master list of completion

This is the implementation order for the remaining work in [ROADMAP.md](ROADMAP.md) and [DECOMPILER_GAP_ROADMAP.md](DECOMPILER_GAP_ROADMAP.md). Source recreation is the first product milestone. The original checklists retain their scope and completion status; this plan reorganizes their dependencies without dropping requirements.

Inputs: the two existing roadmaps, the independent [reconstruction-first draft](RECONSTRUCTION_FIRST_ROADMAP.md), the [Apple Silicon application specification](Apple_Silicon_Reverse_Engineering_App_Spec_v1.md), and the current implementation. No separate replacement roadmap from the other model was available when this synthesis was written. The user directed work to proceed with these inputs.

## Active priority: AArch64 pseudocode and source reconstruction

Re-plotted 2026-10-18. The next product outcome is dependable, named, typed AArch64 pseudocode for Apple binaries; the next is evidence-backed, recompilable C17/Objective-C reconstruction. Near-1:1 fidelity is a separate measured goal, not permission to invent erased source information. ARM64e uses this pipeline with authentication effects explicit until qualified.

Existing inputs are not restart tasks: normalized function boundaries, shared instruction/CFG records, bounded GPR SSA, prototype ABI recovery, isolated Darwin collection, Apple Clang prototype import, and bounded target property/ivar joins already exist. Their limited scope does not close general semantics, memory SSA, type inference or source reconstruction. Extend these implementations rather than build a second decompiler.

The table is the active delivery sequence; C00–C16 below remain the detailed scope/status registry. Each row delivers a working function slice. Do not wait for the whole ISA, every SDK declaration or a general solver before improving supported pseudocode. Evidence tracing, compilation and differential checks begin with the earliest eligible slice, not after the last row.

| Priority | Deliver next | Dependencies / scope | Exit evidence |
| --- | --- | --- | --- |
| 1 — now | Dependable named AArch64 pseudocode: preserve parameters through proven frame/copy/join operations; enforce exact instruction effects, widths and invalidation; expose unsupported effects | Existing function/IR pipeline; C00/C01/early C02–C06 | Scalar integer/FP, frame, call and member fixtures keep correct names and effects at `-O0/-O2`; unknown/reserved encodings cannot retain stale identities. |
| 2 | Canonical register/flags/vector and memory SSA, frame objects, aliases and lifetimes; retire text-reparsed identity/effect decisions | Priority 1; C02/C03 | Copies, partial writes, spills, overlapping slots, joins, loops and calls have correct structured def-use/memory versions and bounded analysis. |
| 3 | Complete the first slice's ABI, calls and native type joins: stack/aggregate arguments and results, C declarations, class/selector/method names, properties/ivars, struct fields and provider images | Priorities 1–2; C00N/C04–C06, iterated together | Compiler/libobjc/SDK oracle agreement; mismatched images/layouts/ABIs and ambiguous dispatch retain conflicts instead of fabricated names/types. |
| 4 | Typed source AST and source-level variables/expressions: parameters, locals, globals, fields, arrays, casts and side-effect-safe expression folding | Priorities 2–3; C05/C07 | Pseudocode replaces registers only where proven; every AST node links to bytes, IR, SSA and its name/type derivation. Real worker artifacts pass C00 validation. |
| 5 | Nested conditions, short circuit, loops, switches, early returns and tail calls | Canonical CFG plus priorities 2–4; C02/C08 | AST-to-CFG/effect checks preserve every supported reachable edge, exit and side effect; irreducible/unresolved flow stays explicit. |
| 6 | Export complete supported C17/Objective-C functions and dependencies; retain a strict machine-state C mode for the same semantics | Early strict emitter/interpreter from priority 1; recovered AST from priorities 3–5; C01/C09 | Recorded native toolchain compiles deterministic output with declarations, globals, imports, layouts, helpers and source maps; unresolved reachable effects block strict export. |
| 7 | Qualify the end-to-end AArch64 reconstruction milestone with behavior and near-original fidelity reported separately | Checks start at priority 1 and expand through priority 6; C10/first C12 gate | Original/IR/generated execution agrees on recorded returns, memory, traps and calls; source scoring covers names/types/AST independently. Full `-O0/-O1/-O2/-Os`, stripped/symbol-rich matrix. |
| 8 | Complete integrated review and corrections for that pipeline | Minimum source/evidence navigation accompanies priorities 1–7; full C11/C15 | Expression selection reaches all instruction origins; analyst edits invalidate only dependent work; cancellation/reanalysis preserves honest completeness. |
| 9 — after the first milestone | Extend AArch64 C++/Swift and remaining ARM64e semantics, then x86-64, ELF/PE/COFF, broad interoperability, team/on-prem and release completion | Qualified first C/Objective-C slice; remaining C12–C16 | Each added language/ISA/platform passes its own declared gates; all original requirements remain in scope. |

### Immediate implementation queue

These are the next bounded tasks within priority 1, not new completion claims or a duplicate checkbox ledger:

1. Keep the worker/decompilation/native-catalog/prototype-ABI gates green on the current tree. The parameter-name/frame-update regression now has a passing prologue test and unchanged 16-variant corpus gates; preserve negative clobber/join coverage as naming moves to structured IR.
2. Audit broad SIMD/system fallbacks and reserved/neighboring encodings. Replace unsound no-effect classifications with exact effects or conservative unknown barriers, including GPR, vector, flags and memory invalidation. Protect every reconstruction consumer, not only member substitution.
3. Parameter-name/copy/clobber decisions now use [bounded shared-effect identities](AARCH64_SOURCE_VARIABLES.md), including frame spills, paired writes and agreeing CFG paths. Next integrate these with canonical SSA/variable lifetimes, typed source AST and memory effects; rendered control-flow structuring and other text-derived reconstruction decisions still need migration.
4. Extend existing worker-produced function artifacts with richer native type/image origins and source/variable/effect evidence, preserving source/function/pass/settings identities, exact dependency spans, blockers and budget exhaustion. Join these records to coordinator/client review and export; offline schema tests alone do not finish that integration.
5. Expand the now-qualified C/Objective-C scalar walking skeleton through loops, external memory, native calls and source-level layouts, while exposing its recovered-source view in the app. Preserve compile/native differential checks throughout; do not hide unsupported effects behind stubs or postpone validation until general recovery is complete.

Darwin evidence is an early and continuous dependency, not a deferred catalog project. Use native dyld/Mach-O, libobjc, Foundation/CoreFoundation, Apple Clang SDK layouts, libSystem/dispatch and applicable Mach/XPC contracts. Keep target facts separate from helper-host observations. Qualify shared macOS/iOS dyld layouts and semantic contracts once at their demonstrated architecture/ABI/version scope; test only remaining platform-specific deltas separately. No blanket claim that macOS execution proves every iOS behavior.

Persistence, memory/work budgets, cancellation, cache invalidation and crash recovery from C15 accompany the affected passes. Additional formats, generalized team features and cosmetic work do not displace this queue unless they directly unblock it. Preserve already claimed work and coordinate overlapping changes; reprioritization does not authorize discarding another contributor's edits.

## What the comparison changed

| Question | Existing roadmaps | Independent draft | Master decision |
| --- | --- | --- | --- |
| First outcome | Broad target expansion alongside decompiler features | High fidelity source reconstruction | Source reconstruction first; breadth follows a validated working pipeline. |
| Foundation | CFG, shared IR, bounded GPR SSA, metadata, pseudocode already exist | Evidence schema and complete semantics before later phases | Reuse the existing foundation; add structured provenance and eligibility around real operations immediately. |
| Call/type ordering | Some structuring precedes complete ABI/type inference | Interprocedural analysis precedes type and expression recovery | Establish effects, ABI locations, memory SSA, call summaries, and basic types before high level expressions and structuring. Iterate these analyses together. |
| Scope of an iteration | Instruction families and individual features | Large sequential phases R0–R10 | Deliver repeated end-to-end vertical slices; never wait for the entire ISA or all languages before testing generated source. |
| Evidence graph | Investigation graph and text/navigation records | Full reconstruction derivation graph | Connect bytes → IR → SSA → types → AST → validation with stable identities from the first slice. Keep investigation and derivation graphs interoperable. |
| Validation | Useful structural corpus and source-map checks | Recompilation, differential tests, SMT, fidelity scoring | Add each gate as soon as its corresponding output exists. Differential agreement is bounded evidence; universal equivalence requires an explicit proof domain. |
| C dialect/output | Specification requires C17 and three output levels | Draft says C11 and two modes | Use C17: readable pseudocode, strict machine-state C, and recovered source. Each declares its completeness and execution model. |
| Completion | Some broad “foundation complete” labels cover still-open details | Many newly defined completion gates | Feature checkboxes require their full stated gates. Existing partial features are inputs, not proof that full reconstruction works. |

## The first outcome the user must be able to use

Open an ARM64/ARM64e Mach-O, select a function, inspect reconstructed source and all contributing machine evidence, export a complete supported function with its dependencies, compile it, and compare its execution with the original under a recorded ABI and memory model. Extend that working path until C and Objective-C corpus functions cover scalar integer/FP code, frames, globals, aggregates, calls, nested branches, loops, and switches. ARM64e authentication semantics remain explicit until qualified.

Original names, layout, idioms, and structure should be recovered whenever available evidence supports them. The fidelity score measures resemblance to known original source separately from behavioral correctness. Information erased by compilation remains an uncertainty or an analyst choice. Avoid a single unexplained confidence score.

Completion evidence must identify the source hash, architecture, ABI, function bounds, analysis revision, toolchain, execution model, assumptions, test seeds, supported paths, and unresolved effects. “Compiled,” “exercised,” “differential-tested,” and “formally checked under assumptions” are different claims.

## Darwin native runtime and shared macOS/iOS qualification

Apple reconstruction uses Darwin native facilities: SDK Mach-O definitions and available inspection helpers; dyld image/slide/linkage APIs; libSystem, libobjc, libdispatch, CoreFoundation and Foundation; and Mach/XPC for applicable process and service contracts. Reconstructed programs link real native libraries when those libraries supply the recovered behavior. The existing Mach debugger and NSXPC coordinator are integration points.

macOS execution qualifies the shared foundation of iOS semantics when the relevant contract and implementation are shared. Reuse instruction/IR, C/Objective-C, Foundation/CoreFoundation, dispatch, allocation, and messaging qualification at the level established by evidence. Record function/API, architecture, ABI/data layout, implementation/version identity or source-backed shared contract, preconditions, environment, and validation scope. Version/ABI/platform differences become explicit unqualified deltas; simulator/device/guest checks address those deltas.

In particular, use one shared Mach-O/dyld decoder and test corpus for both platforms. Common load commands, binding/export opcodes, chained-fixup pointer formats, and cache structures are selected by their encoded layout/version and feature fields. The originating OS is metadata rather than a reason to duplicate parsing. Successful macOS fixtures qualify the same dyld decoding paths for iOS; platform-specific records and unsupported format versions retain their own coverage status.

| Facility | Reconstruction use | Qualification evidence |
| --- | --- | --- |
| SDK `mach-o/loader.h`, `getsect.h`, `dyld.h`, `dyld_images.h`, fixup definitions | Image/slice/platform identity, sections, loaded images, slides, imports/fixups | SDK/build identity, UUID, load commands and mapped range agreement |
| dyld / `dladdr` / validated symbol lookup | Runtime resolution and binding evidence | Actual provider image/symbol, image identity, architecture, symbol contract |
| libobjc, Foundation, CoreFoundation | Objects, encodings, dispatch, ARC/runtime behavior and real framework calls | Shared contract/implementation evidence, metadata, layout, object and return effects |
| libSystem and libdispatch | Memory, strings, synchronization, blocks and execution primitives | ABI, ordering/lifetime contract, native observations and recorded environment |
| Mach and XPC/NSXPC | Task/VM/port/message and service-boundary behavior | Rights, message format, connection/endpoint contract, actual supported environment |

Native helpers inspect mapped runtime images; bounded static parsers inspect captured files. Each runtime observation stays attached to its platform and implementation, with an explicit shared-qualification edge where applicable.

## Execution rules

1. Follow the active delivery order above, starting with current AArch64 pseudocode correctness. Extend thin C00 contracts, C00N native evidence and canonical function parsing wherever that slice requires them; do not make catalog or ISA completeness a prerequisite for useful output. Shared evidence identities, budget accounting, persistence, cancellation, and source maps accompany every pass.
2. Preserve machine widths and effects before simplifying. Unknown reachable instructions/calls/memory effects block strict source export unless a defined intrinsic or explicit dependency models them.
3. The canonical pipeline consumes structured IR and a typed source AST. Rendered pseudocode is a presentation and optional parsed input language, not an analysis database.
4. Every task below is still open unless evidence explicitly establishes its complete acceptance gate. A schema draft or row-count test alone cannot close an integrated feature.
5. Continue original roadmap work after the reconstruction milestone; priority changes do not remove the rest of the objective.

## C00 — Artifact, provenance, eligibility, and corpus contracts

Dependencies: current captured-source identity, normalized function boundaries, worker IR/CFG.

- [~] Define versioned reconstruction AST, effects, types, variables, hypotheses, source spans, dependencies, and validation artifacts; reconcile the draft JSON schema with C17 and the worker transport. *(in progress — claimed)*
- [~] Implement content-derived identities scoped to binary, function, pass version, and analysis settings; validate uniqueness, references, spans, graph integrity, limits, and artifact hashes. *(in progress — claimed)*
- [x] Separate observed facts, derivations, assumptions, analyst assertions, rejected alternatives, and conflicts; support bounded “why,” alternatives, dependency, contradiction, and invalidation queries.
- [x] Introduce per-function review/export/validation eligibility with explicit blockers and completeness; incomplete analysis cannot acquire an equivalence label.
- [~] Extend the redistributable corpus to `-O0/-O1/-O2/-Os`, stripped/symbol-rich builds, compiler variants, and per-feature ground truth. Keep scoring-only source/debug oracles out of the recovery path. *(in progress — claimed)*

Acceptance: real worker artifacts pass structural and semantic validation; mutations of content, references, provenance, completeness, or eligibility are rejected. Reports distinguish decoded coverage, semantic coverage, fidelity, compilation, and tested behavior.

Current progress: a C17 artifact schema, offline content/reference/AST/eligibility validator, and bounded evidence-query implementation exist with executable positive/adversarial tests. The worker now produces bounded, review-only AArch64 machine-state AST/evidence artifacts directly from shared IR, normalized function boundaries, conservative instruction CFG and structured register SSA. Cross-language content-ID, mutation, truncation and captured-source checks qualify this first transport slice; recovered-source compilation, memory/type semantics, native catalog joins and general semantic validation remain open. See [worker artifact scope](WORKER_RECONSTRUCTION_ARTIFACTS.md); this does not close C00.

## C00N — Darwin native naming, declarations, images, and type evidence

Dependencies: captured image identity and the thin C00 evidence contract. This work begins before general SSA/type inference and supplies its initial names and types.

- [x] Establish [isolated Darwin runtime collection and Apple Clang C prototype import](DARWIN_NATIVE_EVIDENCE.md): real helper-process image/selector/property/ivar/layout records, compiler-derived declarations, source/platform-bound worker prototype input, and `-O0/-O2` pseudocode integration. Rich native-record joins, full type-library ingestion and target-runtime collection remain in the open tasks below.

- [~] Build one version-aware Mach-O/dyld path for macOS/iOS: UUIDs, architecture/subtype, platform/build version, segments/sections, symbols/exports, dylib ordinals, reexports, stubs/GOT/authenticated pointers, chained fixups and shared-cache image identities. *(in progress — claimed)*
- [~] Collect native loaded-image ownership and slide/symbol evidence through dyld, `dladdr` and applicable Mach APIs; join file/preferred/runtime addresses only through a validated mapping. Native introspection runs in its own helper or qualified target runtime. *(in progress — claimed)*
- [~] Recover function names from exact target symbols, validated exports/import providers, runtime IMPs, debug records, and SDK declarations. Preserve exact names and raw mangling alongside inferred aliases and provenance; a nearby symbol address alone does not establish an exact function identity. *(in progress — claimed)*
- [~] Collect Objective-C class/metaclass names, superclasses, class/instance methods, selectors, complete method encodings, return/argument types and IMP ownership using target metadata and applicable libobjc APIs (`class_getName`, `class_getSuperclass`, `class_copyMethodList`, `method_getName`, `sel_getName`, `method_getTypeEncoding`, `method_getImplementation`). *(in progress — claimed)*
- [~] Recover properties and their attributes, declared types, getter/setter selectors, backing ivars and ownership qualifiers; recover ivar names, encodings, offsets, widths, alignment and inheritance boundaries using metadata plus `class_copyPropertyList`, `property_getAttributes`, `class_copyIvarList`, `ivar_getTypeEncoding`, and `ivar_getOffset` where runtime access exists. *(in progress — claimed)*
- [~] Import C-style function declarations and SDK definitions through the Apple Clang AST: typedefs, enums, pointers/function pointers, callbacks/blocks, structs/unions, field offsets, packing, alignment, bitfields, variadic prototypes and ABI attributes. Associate declarations with the actual providing image and retain aliases/source spelling. *(in progress — claimed)*
- [~] Use SDK/Clang record layouts, `sizeof`/alignment/offset probes, Objective-C encodings and Foundation size/alignment helpers to establish structure layouts and types. Runtime encodings seed layout evidence; missing field names or erased typedefs remain explicitly unresolved until headers/debug/type constraints supply them. *(in progress — claimed)*
- [~] Feed this catalog into function-boundary parsing, prototype/ABI recovery, selector/message-send reconstruction, class/property/ivar expressions and struct field access. Reuse libSystem, libobjc, Foundation/CoreFoundation, dispatch, Mach and XPC contracts and implementations where applicable. *(in progress — claimed)*
- [~] Record shared macOS/iOS qualification per layout/decoder/runtime contract, including version/architecture/ABI applicability. Run identical shared-format fixtures through the same dyld decoder; only the platform-specific deltas require separate qualification. *(in progress — claimed)*

Acceptance: native fixtures exercise images, C functions, classes/metaclasses, instance/class selectors, properties/ivars, C structs and ABI layouts. Their names/types/layouts agree with SDK/Clang/runtime ground truth. Catalog joins reject image/UUID/ABI mismatches; shared-format macOS/iOS fixtures yield the same semantic decoding. Every catalog-derived pseudocode construct links back to the record that supplied it.

## C01 — Exact semantics, effects, and executable machine-state C

Dependencies: C00. Extend semantics as each corpus slice requires; full ISA qualification continues throughout C02–C12.

Verified progress: integer SIMD-to-GPR lane extracts now use explicit shared effects across all 82 legal forms. Apple-assembled fixtures and 42,640 Apple-Silicon comparisons qualify normal EL0 non-streaming lane results. The unsound broad SIMD preservation fallback is removed, and vector-bitwise decoding checks fixed bit 24. General SIMD/system effects, memory/flags SSA, strict emission and whole-function equivalence remain open.

Executable progress: the [worker machine-state slice](WORKER_RECONSTRUCTION_ARTIFACTS.md#executable-machine-state-slice) now interprets shared-tree integer/branch/checked-memory semantics and emits complete C17 for supported captured functions. Native C/Objective-C/assembly `-O0/-O2` fixtures qualify bounded returns and explicit buffers; interpreter/generated runs additionally agree on the full modeled register/memory state. Graph-linked compilation/differential observations remain separate from recovered-source fidelity and formal equivalence. General instruction/call/FP/exception coverage and canonical flags/memory SSA remain open; this does not close C01 or C10.

- [~] Inventory exact, intrinsic, privileged/runtime, and unsupported instruction families; finish shared ARM64/ARM64e semantics including vector/half FP, flags, partial lanes, SP, PAC, atomics, barriers, traps, and exception effects. *(in progress — claimed)*
- [~] Specify typed bitvectors, modular arithmetic, comparisons, shifts, extensions, poison/unknowns, endianness, memory alignment, FP modes, and architectural aliases. *(in progress — claimed)*
- [~] Build an IR interpreter and strict C17 emitter with checked guest-memory helpers and explicit imports/intrinsics. Preserve ordering and side effects. *(in progress — claimed)*
- [~] Connect the Darwin native support runtime and record reusable macOS/iOS semantic qualifications with explicit ABI/environment deltas. *(in progress — claimed)*
- [~] Differential-test semantics against hardware or a qualified independent engine; exercise reserved and neighboring encodings and architectural edge cases. *(in progress — claimed)*
- [~] Replace text reparsing in reconstruction consumers with structured effects. *(in progress — claimed)*
- [~] Render named/typed low-level pseudocode directly from the structured effects and C00N evidence before general source recovery is complete; keep expression source ranges and explicit unresolved effects throughout. *(in progress — claimed)*

Acceptance: eligible machine-state C recompiles and agrees with original execution on recorded tests; unsupported reachable effects fail eligibility. FP/atomic/PAC claims require their dedicated state/model tests.

## C02 — Function discovery and complete control-flow facts

Dependencies: C00 and C00N seed identity/boundaries; C01 effects complete the CFG for the supported slice. Initial function parsing proceeds using the existing decoder/IR while semantics expand.

- [~] Reconcile symbol, unwind, export, runtime, direct-call, prologue, thunk, tail-call, and cross-reference evidence for stripped/optimized code; retain competing boundaries and cold fragments. *(in progress — claimed)*
- [~] Canonicalize per-function CFG with reachability, dominators/post-dominators, loop forest, control dependence, exceptional exits, and unknown successors. *(in progress — claimed)*
- [~] Recover indirect destinations and switch data paths with bounds/range/index-normalization proof; retain explicit unresolved targets. *(in progress — claimed)*

Acceptance: known-source fixture CFGs agree, including disconnected/unreachable code, tails, exceptional paths, and optimized switch lowering; no missing path is silently treated as a function return.

## C03 — Register, flags, vector, and memory SSA

Dependencies: C01, C02.

- [~] Promote GPR SSA evidence to canonical structured definitions, uses, phis, and value IDs with W/X, SP, vector lanes, FP, and flags. *(in progress — claimed)*
- [~] Add memory SSA, call clobbers, ordered/volatile/atomic effects, and conservative alias partitions for frame objects, globals, constants, heap, and escaped/unknown memory. *(in progress — claimed)*
- [~] Prove frame adjustments and saved-register behavior; handle dynamic alignment, overlapping slots, spills, and lifetimes. *(in progress — claimed)*
- [~] Preserve instruction origins through SSA transforms, copies, merges, and dead-value elimination; bound fixed points and publish exhaustion reasons. *(in progress — claimed)*

Acceptance: dominance, phi inputs, interference, def-use, memory versions, read-before-write, loops, aliasing, and unknown-effect fixtures agree with the reference model and traces.

## C04 — Calls, ABI, summaries, and the call graph

Dependencies: C00N names/prototypes/layouts, C02 and C03; basic library types seed the solver before general C05 propagation.

- [~] Complete Apple AAPCS64 parameter/return recovery: GP/FP banks, stack values, homogeneous/arbitrary aggregates, varargs, hidden result/context values, returns, tails, and nonreturning calls. *(in progress — claimed)*
- [~] Build typed direct/import/tail/thunk/indirect/dispatch call edges, confidence, candidate sets, and caller/callee navigation. *(in progress — claimed)*
- [~] Compute effects, clobbers, memory, ownership/allocation, throws/nonreturn, arguments/returns, and target summaries; iterate caller/callee constraints with bounded convergence. *(in progress — claimed)*
- [~] Preserve prototype conflicts and per-parameter evidence; treat names, signatures, targets, and implementations as separate claims. *(in progress — claimed)*

Acceptance: compiled ABI fixtures and execution agree on argument/return locations and call effects; caller constraints improve recovery without inventing indirect targets or suppressing unknown memory writes.

## C05 — Type library, layout, and constraint propagation

Dependencies: C00/C00N seed the library and layouts; iterate with C03/C04 for inferred types.

- [~] Import project/SDK C declarations, DWARF/dSYM, structs/unions/enums/typedefs/prototypes, and validated runtime types; persist source and version. *(in progress — claimed)*
- [~] Preserve native C declarations, Objective-C property/ivar attributes and actual compiler field layouts while propagating them into parameters, locals, fields, method declarations and expressions. *(in progress — claimed)*
- [~] Solve types bidirectionally across SSA, calls, returns, casts, loads/stores, and pointer arithmetic, separating machine representation, semantic type, and source spelling. *(in progress — claimed)*
- [~] Infer stack/global/pointer-relative aggregate layouts and fields from width/offset/use/lifetime/alias evidence; retain incompatible layouts and alternatives. *(in progress — claimed)*
- [~] Recover signedness, nullability, mutability, constness, ownership, and lifetime only to the degree supported by evidence. *(in progress — claimed)*

Acceptance: held-out fixture types/layouts match the source oracle where determined; incompatible constraints remain inspectable and never disappear through silent casts.

## C06 — Apple object models, strings, and library idioms

Dependencies: C00N establishes class/selector/property/ivar facts early; C04/C05 add receiver, dispatch and data-flow inference.

Verified progress: target-local structured property/ivar layouts now feed declarations, object accessor types and bounded shared-IR-proven `self->ivar` accesses. Native-oracle fixtures qualify LP64 offsets and class/metaclass method lists; ambiguous receivers/layouts remain explicit. This does not complete general dispatch/receiver/type inference below.

- [~] Complete Objective-C class/metaclass/superclass/category/protocol/ivar/property/IMP relationships, selector stubs, receiver classes, and dynamic dispatch candidate sets. *(in progress — claimed)*
- [~] Render validated property/ivar access, class/instance method declarations and C-style prototypes with their owning image and type/layout evidence; retain property-accessor calls when an accessor rewrite has not been justified. *(in progress — claimed)*
- [~] Recover strings, pointer/string xrefs, CFString/CFData, globals, block captures, tagged pointers, bounds checks, ownership/runtime calls, and Foundation/CoreFoundation idioms from data flow. *(in progress — claimed)*
- [~] Connect runtime observations and their validated image mappings to static hypotheses without promoting temporal proximity to causation. *(in progress — claimed)*

Acceptance: metadata, message sends, receiver/argument types, data references, and idiom rewrites have graph evidence and preserve machine effects; dynamic alternatives remain visible.

## C07 — Source variables, expressions, and editable hypotheses

Dependencies: C03–C06.

Verified progress: parameter identity flow no longer reparses machine statements. Entry-based identities, width-specific views, bounded frame saves/restores, paired writes and CFG must-joins feed separate input/output naming maps. Six Apple-assembled fixture functions and 1,560 original-native result checks supplement the existing corpus. This is not complete SSA-backed source-variable recovery, typed expression AST or reconstructed-source behavioral validation.

Source progress: [named scalar recovery](WORKER_RECONSTRUCTION_ARTIFACTS.md#named-scalar-source-recovery) now joins exact C/Objective-C declarations to structured ABI inputs, produces an instruction/declaration-linked expression DAG, folds proven private-frame copies, handles acyclic return paths, and emits complete scalar C/C-style IMP adapters. The qualification matrix covers four optimization levels and symbol-rich/local-symbol-stripped fixtures, with native result comparisons and generated-source sanitization. Loops, external memory/calls, general SSA-backed lifetimes/layouts, modern chained Objective-C metadata joins, UI integration and broader source fidelity remain open; C07–C10 are not complete.

- [~] Recover parameters, locals, aggregates, fields, globals, captures, lifetimes, and coalesced SSA variables; name from factual evidence or deterministic role descriptions. *(in progress — claimed)*
- [~] Form expression trees with use counts, dominance, effect ordering, widths, casts, lvalues, array/field access, and defined overflow/shift semantics. *(in progress — claimed)*
- [~] Recognize induction variables, accumulators, state variables, and compiler temporaries with retained supporting facts. *(in progress — claimed)*
- [~] Persist analyst variable/type edits, split/merge, switch marking, and cross-function undo/redo; invalidate and recompute dependent hypotheses by analysis revision. *(in progress — claimed)*

Acceptance: regenerated expressions retain def-use and memory/call ordering; branch joins, alias conflicts, partial writes, and register reuse cannot leak an old source identity.

## C08 — Nested source control flow

Dependencies: C02–C05, C07.

- [~] Build typed regions for nested conditions, short circuit/ternaries, guards/early exits, loops, multi-exit regions, switches/defaults/fallthrough, tails, and supported exception constructs. *(in progress — claimed)*
- [~] Use induction/bounds evidence for `for` and normalized switch indices; represent irreducible regions explicitly. *(in progress — claimed)*
- [~] Regenerate and compare CFG and effects from the source AST after each structural transform. *(in progress — claimed)*

Acceptance: nested and optimized fixtures retain all reachable exits and side effects; no extra edge or removed case is justified solely by a pretty rendering.

## C09 — Reconstructed source export and pseudocode round trip

Dependencies: C04–C08 and the machine-state emitter from C01.

- [~] Emit deterministic compilable recovered C17/Objective-C from the typed AST, with declarations, dependencies, globals, runtime adapters, helpers, ABI annotations, and source maps. *(in progress — claimed)*
- [~] Define/parse the restricted pseudocode grammar with typed diagnostics and explicit arithmetic/memory semantics; generated supported text round-trips to the same AST. *(in progress — claimed)*
- [~] Offer readable pseudocode, machine-state C, and recovered source with independent completeness/validation labels and exact unresolved dependency manifests. *(in progress — claimed)*
- [~] Preserve packing, alignment, volatility, atomic ordering, signed behavior, FP assumptions, and authentication effects; reject export where a reachable dependency is unresolved. *(in progress — claimed)*

Acceptance: exported projects compile under recorded flags and their address/AST/evidence maps validate; no silent stub, zero return, or invented framework behavior makes a function appear complete.

## C10 — Behavioral validation and reconstruction fidelity

Dependencies: C00/C01; expand continuously with C02–C09.

- [~] Execute original and generated code with matched ISA, ABI, inputs, mapped memory, deterministic imports, and bounds; compare returns/registers, memory writes, traps, and call events. *(in progress — claimed)*
- [~] Exercise integer/FP boundary cases, invalid memory, random seeds, aliasing, and supported concurrency/atomic models. *(in progress — claimed)*
- [~] Add bounded SMT equivalence for supported pure/region models, including assumptions, timeouts, and counterexamples. *(in progress — claimed)*
- [~] Score recovered AST structure, variables/types/names, CFG/def-use/effects, recompilation, and tested behavior separately by language/optimization/feature/compiler. *(in progress — claimed)*
- [~] Store input/toolchain/backend/test identities and results as graph evidence; enforce per-category regression thresholds in CI. *(in progress — claimed)*
- [~] Reuse macOS qualification for shared AArch64, Mach-O/dyld, native type/layout and Darwin runtime semantics on iOS; report the tested common foundation and the remaining platform/version/ABI deltas separately. *(in progress — claimed)*

Acceptance: zero unexplained behavioral mismatches in the qualification matrix. A differential pass reports its tested domain; a formal result states its proof model and assumptions. Compilation alone never grants equivalence.

## C11 — Evidence-centered source review

Dependencies: C00; extend alongside C03–C10.

- [~] Synchronize source/AST, types, SSA/memory SSA, CFG, calls, assembly, bytes, and runtime observations; one selection follows all contributing instruction spans. *(in progress — claimed)*
- [~] Expose why/alternatives/conflicts/assumptions/blockers and variable uses at expression granularity. *(in progress — claimed)*
- [~] Add per-function reanalysis, analysis bounds, simplification controls, cancellation/progress, and retained completed work. *(in progress — claimed)*
- [~] Export/import typed signatures, types, call graphs, annotations, AST/evidence graphs, manifests, and hashes; qualify GUI import and native adapters. *(in progress — claimed)*

Acceptance: multi-instruction expressions navigate to every origin; constants/call targets never steal the source selection. Edits trigger the correct dependent work and cancelled results retain accurate completeness.

## C12 — Reconstruction qualification and language expansion

Dependencies: C00–C11 for the first supported slice.

- [~] Qualify C and Objective-C on ARM64/ARM64e Mach-O under the full corpus, execution, fidelity, provenance, resource, and recovery gates. *(in progress — claimed)*
- [~] Extend C++: Itanium ABI, receiver/aggregate recovery, vtables, templates/debug types, exceptions, thunks, and optimized fixtures. *(in progress — claimed)*
- [~] Extend Swift: versioned ABI, signatures, metadata, generics/existentials, ownership, closures, async and runtime adapters with dedicated fixtures. *(in progress — claimed)*
- [~] Add the x86-64 Mach-O lifter, ABI, shared analyses and calling-convention/differential tests through the same reconstruction pipeline. *(in progress — claimed)*

Acceptance: each language/ISA declares and passes its own supported matrix. Structural ingestion of a new ISA does not qualify reconstructed source.

## C13 — Format and platform completion

Dependencies: C00/C01 contracts; format work serves reconstruction after the first qualification milestone.

- [x] ELF sectionless dynamic linking and relocations (`PT_DYNAMIC`/`DT_*` evidence for images without section headers): bounded `DT_RELA`/`DT_REL`/`DT_JMPREL` relocation tables, `DT_HASH`/`DT_GNU_HASH`-proven symbol counts, and `DT_SYMTAB` names/imports/exports/function starts; `DT_RELR` remains unapplied.
- [ ] Qualify Linux native attach with its process/ABI/image identity model (environmental gate; requires a Linux qualification host).
- [x] PE imports, exports and base relocations from data directories (DLL/symbol names, ordinals, export addresses and relocation sites). Standalone COFF objects, PDB and Authenticode remain separate.
- [x] Standalone COFF objects (COFF header, section table, symbols and relocations for `.obj` inputs).
- [x] PDB validation (MSF container, stream discovery and type/line/symbol evidence for Windows debug databases).
- [~] Authenticode validation (PE security directory, PKCS#7/CMS signed data and certificate chain verification). *(in progress — claimed; do not start independently)*
- [ ] Complete broader native project/type/comment/signature interoperability, GUI import, and live Ghidra/IDA/Binary Ninja adapter qualification.

Acceptance: malformed/overflow/ambiguous fixtures fail with bounded diagnostics; live and static mappings retain exact identity; round trips preserve supported artifacts and conflicts.

## C14 — Investigation and team completion

Dependencies: C00 identities, C11 artifacts, C15 persistence/protocols.

- [ ] Shareable investigation bundles with content hashing and replayable experiment recipes.
- [ ] Findings, review/comment state, evidence links, and reproducible experiment records.
- [ ] Extension signatures, capability policy, allowlists, trust and organization settings.
- [ ] Optional on-premises execution node with versioned authenticated headless jobs and returned evidence identities.

Acceptance: bundle tampering and dependency changes are detected; team assertions retain authorship; extension and remote execution permissions enforce their declared capabilities.

## C15 — Persistence, resource, and recovery prerequisites

Remote integration companion: [app-hosted server contract and client-readiness/push gate](HINDSIGHT_REMOTE.md). The HindSight application owns the heavy work and streams results to authenticated clients. Client repository/branch placement is deferred until server completion; no remote-server completion is claimed here.

Dependencies: existing project/cache/job model. Implement affected portions with each reconstruction pass.

- [ ] Version project/analysis schemas and tested migrations beyond v1; stable documented SDK/target protocol and explicit compatibility negotiation.
- [ ] Add worker checkpoints, coordinator recovery, resumable long jobs, atomic publication, and dependency-based invalidation.
- [ ] Enforce CI regression budgets for memory, latency, work/cardinality, queue/backpressure, cancellation, deterministic output, and the 8 GiB profile.

Acceptance: interruption/corruption/migration tests preserve completed evidence and reject incomplete publication; checkpoint replay agrees with a clean run; measured supported workloads meet their published budgets.

## C16 — Distribution and product completion

Dependencies: reconstruction qualification, C13–C15 and all applicable release gates.

- [ ] Universal signed/notarized distribution, qualified updater/rollback, entitlements, and update authenticity.
- [ ] Licensing tiers that preserve offline/local analysis and documented license-state recovery.
- [ ] Guided labs, redistributable datasets, onboarding, complete documentation, and enterprise deployment material.
- [ ] Re-run the original 0.1, 0.5, 1.0, native execution, signing/notarization, universal-build, and memory-pressure gates against current artifacts.

Acceptance: clean-install/update/offline/recovery tests and reproducible lab walkthroughs pass on the supported host matrix. External signing credentials and deployment decisions are requested only when their concrete release step is ready.

## Traceability: every remaining original checklist item

An original checkbox closes only when all mapped work and its stated gate are proven. `DG` refers to `DECOMPILER_GAP_ROADMAP.md`; `RM` to `ROADMAP.md`. Baseline and aggregate rows deliberately overlap detailed rows.

| Original remaining requirement | Master dependencies |
| --- | --- |
| DG general source-variable, stack-local, type recovery (in progress) | C03–C07, C09–C12 |
| DG general C control flow, switches, architecture-independent decompilation | C01–C03, C08–C12 |
| DG complete shared IR coverage | C01, C10, C12 |
| DG replace machine-register temporaries | C03–C07, C09–C11 |
| DG structure common control flow | C02, C08–C10 |
| DG improve call-site reconstruction | C03–C06, C09–C10 |
| DG keep source mapping in the UI | C00, C07–C11 |
| DG recover function signatures | C04–C06, C10, C12 |
| DG infer stack locals and aggregate layouts | C03, C05, C07, C10 |
| DG add project type library | C05, C11–C12, C15 |
| DG model Objective-C dispatch and objects | C04–C06, C10–C12 |
| DG add type propagation and casts | C03–C05, C07, C10 |
| DG add analyst overrides | C00, C07, C11, C15 |
| DG recover data references and strings | C03, C05–C07, C10 |
| DG recognize library idioms | C04–C07, C10 |
| DG recover switches and indirect control flow | C02–C04, C06, C08, C10 |
| DG add another architecture through IR | C01, C10, C12 |
| DG handle stripped and optimized code | C00–C10, C12 |
| DG add function/call graphs | C02, C04, C06, C11 |
| DG add per-function controls | C00, C11, C15 |
| DG expose uncertainty and alternatives | C00, C04–C08, C11 |
| DG resource/cancellation controls | C00, C11, C15 |
| DG export standard artifacts | C09, C11, C13 |
| RM ELF sectionless linking/relocations and Linux attach | C13 |
| RM COFF, PE linkage/relocations, PDB, Authenticode | C13 |
| RM architecture abstraction beyond ARM64 | C01, C12 |
| RM reversing artifact interop, GUI, live adapters | C11, C13 |
| RM P1 Types aggregate | C03–C07, C10–C12, C15 |
| RM P2 Coverage aggregate | C01–C08, C10–C12 |
| RM shareable investigation bundles | C00, C11, C14–C15 |
| RM comments/review/findings/evidence/recipes | C11, C14–C15 |
| RM extension policy/signatures/allowlists/org settings | C14–C15 |
| RM on-prem execution node | C14–C15 |
| RM stable project schema/migrations | C15 |
| RM stable SDK/target protocol | C15 |
| RM crash recovery/resumable long jobs | C15 |
| RM performance regression budgets | C10–C11, C15 |
| RM signed/notarized distribution/updater | C16 |
| RM offline-compatible licensing tiers | C16 |
| RM documentation/labs/datasets/enterprise material | C00, C10, C14, C16 |

## Next executable work

1. Extend real per-function IR/CFG/SSA/AST artifacts through canonical flags/memory effects and exact native name/type evidence (C00–C07); preserve the passing source-bound artifact gate.
2. Extend the existing Darwin helper and Apple Clang declaration importer with validated catalog-to-target joins for provider images, C declarations, classes/methods/properties/ivars and arbitrary struct layouts (C00N).
3. Expand the passing machine-state C17/interpreter/native walking skeleton through native imports, broader semantics and recovered source; retain graph-linked compilation/behavior evidence with explicit limits (C01/C09/C10).
4. Extend that same function path through memory/register SSA, ABI/type solving, recovered variables/expressions, nested control flow, compilable recovered source, and synchronized evidence review (C03–C11).
5. Expand the AArch64 macOS/iOS shared corpus, enforce per-feature fidelity/behavior gates, and only then promote broader architecture/format/product work (C12–C16).
