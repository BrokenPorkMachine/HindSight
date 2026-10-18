# Chimera master list of completion

This is the implementation order for the remaining work in [ROADMAP.md](ROADMAP.md) and [DECOMPILER_GAP_ROADMAP.md](DECOMPILER_GAP_ROADMAP.md). Source recreation is the first product milestone. The original checklists retain their scope and completion status; this plan reorganizes their dependencies without dropping requirements.

Inputs: the two existing roadmaps, the independent [reconstruction-first draft](RECONSTRUCTION_FIRST_ROADMAP.md), the [Apple Silicon application specification](Apple_Silicon_Reverse_Engineering_App_Spec_v1.md), and the current implementation. No separate replacement roadmap from the other model was available when this synthesis was written. The user directed work to proceed with these inputs.

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

## Execution rules

1. Start with C00 and C01, then run C02–C12 as an expanding vertical slice. Shared evidence identities, budget accounting, persistence, cancellation, and source maps accompany every pass.
2. Preserve machine widths and effects before simplifying. Unknown reachable instructions/calls/memory effects block strict source export unless a defined intrinsic or explicit dependency models them.
3. The canonical pipeline consumes structured IR and a typed source AST. Rendered pseudocode is a presentation and optional parsed input language, not an analysis database.
4. Every task below is still open unless evidence explicitly establishes its complete acceptance gate. A schema draft or row-count test alone cannot close an integrated feature.
5. Continue original roadmap work after the reconstruction milestone; priority changes do not remove the rest of the objective.

## C00 — Artifact, provenance, eligibility, and corpus contracts

Dependencies: current captured-source identity, normalized function boundaries, worker IR/CFG.

- [ ] Define versioned reconstruction AST, effects, types, variables, hypotheses, source spans, dependencies, and validation artifacts; reconcile the draft JSON schema with C17 and the worker transport.
- [ ] Implement content-derived identities scoped to binary, function, pass version, and analysis settings; validate uniqueness, references, spans, graph integrity, limits, and artifact hashes.
- [ ] Separate observed facts, derivations, assumptions, analyst assertions, rejected alternatives, and conflicts; support bounded “why,” alternatives, dependency, contradiction, and invalidation queries.
- [ ] Introduce per-function review/export/validation eligibility with explicit blockers and completeness; incomplete analysis cannot acquire an equivalence label.
- [ ] Extend the redistributable corpus to `-O0/-O1/-O2/-Os`, stripped/symbol-rich builds, compiler variants, and per-feature ground truth. Keep scoring-only source/debug oracles out of the recovery path.

Acceptance: real worker artifacts pass structural and semantic validation; mutations of content, references, provenance, completeness, or eligibility are rejected. Reports distinguish decoded coverage, semantic coverage, fidelity, compilation, and tested behavior.

## C01 — Exact semantics, effects, and executable machine-state C

Dependencies: C00. Extend semantics as each corpus slice requires; full ISA qualification continues throughout C02–C12.

- [ ] Inventory exact, intrinsic, privileged/runtime, and unsupported instruction families; finish shared ARM64/ARM64e semantics including vector/half FP, flags, partial lanes, SP, PAC, atomics, barriers, traps, and exception effects.
- [ ] Specify typed bitvectors, modular arithmetic, comparisons, shifts, extensions, poison/unknowns, endianness, memory alignment, FP modes, and architectural aliases.
- [ ] Build an IR interpreter and strict C17 emitter with checked guest-memory helpers and explicit imports/intrinsics. Preserve ordering and side effects.
- [ ] Differential-test semantics against hardware or a qualified independent engine; exercise reserved and neighboring encodings and architectural edge cases.
- [ ] Replace text reparsing in reconstruction consumers with structured effects.

Acceptance: eligible machine-state C recompiles and agrees with original execution on recorded tests; unsupported reachable effects fail eligibility. FP/atomic/PAC claims require their dedicated state/model tests.

## C02 — Function discovery and complete control-flow facts

Dependencies: C00, C01 effects for the supported slice.

- [ ] Reconcile symbol, unwind, export, runtime, direct-call, prologue, thunk, tail-call, and cross-reference evidence for stripped/optimized code; retain competing boundaries and cold fragments.
- [ ] Canonicalize per-function CFG with reachability, dominators/post-dominators, loop forest, control dependence, exceptional exits, and unknown successors.
- [ ] Recover indirect destinations and switch data paths with bounds/range/index-normalization proof; retain explicit unresolved targets.

Acceptance: known-source fixture CFGs agree, including disconnected/unreachable code, tails, exceptional paths, and optimized switch lowering; no missing path is silently treated as a function return.

## C03 — Register, flags, vector, and memory SSA

Dependencies: C01, C02.

- [ ] Promote GPR SSA evidence to canonical structured definitions, uses, phis, and value IDs with W/X, SP, vector lanes, FP, and flags.
- [ ] Add memory SSA, call clobbers, ordered/volatile/atomic effects, and conservative alias partitions for frame objects, globals, constants, heap, and escaped/unknown memory.
- [ ] Prove frame adjustments and saved-register behavior; handle dynamic alignment, overlapping slots, spills, and lifetimes.
- [ ] Preserve instruction origins through SSA transforms, copies, merges, and dead-value elimination; bound fixed points and publish exhaustion reasons.

Acceptance: dominance, phi inputs, interference, def-use, memory versions, read-before-write, loops, aliasing, and unknown-effect fixtures agree with the reference model and traces.

## C04 — Calls, ABI, summaries, and the call graph

Dependencies: C02, C03; basic library types from C05 seed the solver.

- [ ] Complete Apple AAPCS64 parameter/return recovery: GP/FP banks, stack values, homogeneous/arbitrary aggregates, varargs, hidden result/context values, returns, tails, and nonreturning calls.
- [ ] Build typed direct/import/tail/thunk/indirect/dispatch call edges, confidence, candidate sets, and caller/callee navigation.
- [ ] Compute effects, clobbers, memory, ownership/allocation, throws/nonreturn, arguments/returns, and target summaries; iterate caller/callee constraints with bounded convergence.
- [ ] Preserve prototype conflicts and per-parameter evidence; treat names, signatures, targets, and implementations as separate claims.

Acceptance: compiled ABI fixtures and execution agree on argument/return locations and call effects; caller constraints improve recovery without inventing indirect targets or suppressing unknown memory writes.

## C05 — Type library, layout, and constraint propagation

Dependencies: C00; iterate with C03/C04.

- [ ] Import project/SDK C declarations, DWARF/dSYM, structs/unions/enums/typedefs/prototypes, and validated runtime types; persist source and version.
- [ ] Solve types bidirectionally across SSA, calls, returns, casts, loads/stores, and pointer arithmetic, separating machine representation, semantic type, and source spelling.
- [ ] Infer stack/global/pointer-relative aggregate layouts and fields from width/offset/use/lifetime/alias evidence; retain incompatible layouts and alternatives.
- [ ] Recover signedness, nullability, mutability, constness, ownership, and lifetime only to the degree supported by evidence.

Acceptance: held-out fixture types/layouts match the source oracle where determined; incompatible constraints remain inspectable and never disappear through silent casts.

## C06 — Apple object models, strings, and library idioms

Dependencies: C04, C05.

- [ ] Complete Objective-C class/metaclass/superclass/category/protocol/ivar/property/IMP relationships, selector stubs, receiver classes, and dynamic dispatch candidate sets.
- [ ] Recover strings, pointer/string xrefs, CFString/CFData, globals, block captures, tagged pointers, bounds checks, ownership/runtime calls, and Foundation/CoreFoundation idioms from data flow.
- [ ] Connect runtime observations and their validated image mappings to static hypotheses without promoting temporal proximity to causation.

Acceptance: metadata, message sends, receiver/argument types, data references, and idiom rewrites have graph evidence and preserve machine effects; dynamic alternatives remain visible.

## C07 — Source variables, expressions, and editable hypotheses

Dependencies: C03–C06.

- [ ] Recover parameters, locals, aggregates, fields, globals, captures, lifetimes, and coalesced SSA variables; name from factual evidence or deterministic role descriptions.
- [ ] Form expression trees with use counts, dominance, effect ordering, widths, casts, lvalues, array/field access, and defined overflow/shift semantics.
- [ ] Recognize induction variables, accumulators, state variables, and compiler temporaries with retained supporting facts.
- [ ] Persist analyst variable/type edits, split/merge, switch marking, and cross-function undo/redo; invalidate and recompute dependent hypotheses by analysis revision.

Acceptance: regenerated expressions retain def-use and memory/call ordering; branch joins, alias conflicts, partial writes, and register reuse cannot leak an old source identity.

## C08 — Nested source control flow

Dependencies: C02–C05, C07.

- [ ] Build typed regions for nested conditions, short circuit/ternaries, guards/early exits, loops, multi-exit regions, switches/defaults/fallthrough, tails, and supported exception constructs.
- [ ] Use induction/bounds evidence for `for` and normalized switch indices; represent irreducible regions explicitly.
- [ ] Regenerate and compare CFG and effects from the source AST after each structural transform.

Acceptance: nested and optimized fixtures retain all reachable exits and side effects; no extra edge or removed case is justified solely by a pretty rendering.

## C09 — Reconstructed source export and pseudocode round trip

Dependencies: C04–C08 and the machine-state emitter from C01.

- [ ] Emit deterministic compilable recovered C17/Objective-C from the typed AST, with declarations, dependencies, globals, runtime adapters, helpers, ABI annotations, and source maps.
- [ ] Define/parse the restricted pseudocode grammar with typed diagnostics and explicit arithmetic/memory semantics; generated supported text round-trips to the same AST.
- [ ] Offer readable pseudocode, machine-state C, and recovered source with independent completeness/validation labels and exact unresolved dependency manifests.
- [ ] Preserve packing, alignment, volatility, atomic ordering, signed behavior, FP assumptions, and authentication effects; reject export where a reachable dependency is unresolved.

Acceptance: exported projects compile under recorded flags and their address/AST/evidence maps validate; no silent stub, zero return, or invented framework behavior makes a function appear complete.

## C10 — Behavioral validation and reconstruction fidelity

Dependencies: C00/C01; expand continuously with C02–C09.

- [ ] Execute original and generated code with matched ISA, ABI, inputs, mapped memory, deterministic imports, and bounds; compare returns/registers, memory writes, traps, and call events.
- [ ] Exercise integer/FP boundary cases, invalid memory, random seeds, aliasing, and supported concurrency/atomic models.
- [ ] Add bounded SMT equivalence for supported pure/region models, including assumptions, timeouts, and counterexamples.
- [ ] Score recovered AST structure, variables/types/names, CFG/def-use/effects, recompilation, and tested behavior separately by language/optimization/feature/compiler.
- [ ] Store input/toolchain/backend/test identities and results as graph evidence; enforce per-category regression thresholds in CI.

Acceptance: zero unexplained behavioral mismatches in the qualification matrix. A differential pass reports its tested domain; a formal result states its proof model and assumptions. Compilation alone never grants equivalence.

## C11 — Evidence-centered source review

Dependencies: C00; extend alongside C03–C10.

- [ ] Synchronize source/AST, types, SSA/memory SSA, CFG, calls, assembly, bytes, and runtime observations; one selection follows all contributing instruction spans.
- [ ] Expose why/alternatives/conflicts/assumptions/blockers and variable uses at expression granularity.
- [ ] Add per-function reanalysis, analysis bounds, simplification controls, cancellation/progress, and retained completed work.
- [ ] Export/import typed signatures, types, call graphs, annotations, AST/evidence graphs, manifests, and hashes; qualify GUI import and native adapters.

Acceptance: multi-instruction expressions navigate to every origin; constants/call targets never steal the source selection. Edits trigger the correct dependent work and cancelled results retain accurate completeness.

## C12 — Reconstruction qualification and language expansion

Dependencies: C00–C11 for the first supported slice.

- [ ] Qualify C and Objective-C on ARM64/ARM64e Mach-O under the full corpus, execution, fidelity, provenance, resource, and recovery gates.
- [ ] Extend C++: Itanium ABI, receiver/aggregate recovery, vtables, templates/debug types, exceptions, thunks, and optimized fixtures.
- [ ] Extend Swift: versioned ABI, signatures, metadata, generics/existentials, ownership, closures, async and runtime adapters with dedicated fixtures.
- [ ] Add the x86-64 Mach-O lifter, ABI, shared analyses and calling-convention/differential tests through the same reconstruction pipeline.

Acceptance: each language/ISA declares and passes its own supported matrix. Structural ingestion of a new ISA does not qualify reconstructed source.

## C13 — Format and platform completion

Dependencies: C00/C01 contracts; format work serves reconstruction after the first qualification milestone.

- [ ] ELF sectionless dynamic linking and relocations; qualify Linux native attach with its process/ABI/image identity model.
- [ ] Standalone COFF, PE imports/exports/base relocations, PDB and Authenticode validation.
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

1. Validate the draft artifact schema with real positive and adversarial artifact instances, content identities, references, AST/evidence integrity, and eligibility gates (C00).
2. Bind real lifted instructions and their coverage/effects to evidence nodes; publish per-function reconstruction blockers without reparsing pseudocode (C00/C01).
3. Implement the first machine-state C17 and IR-interpreter fixture, compile it, run differential tests, and retain graph-linked results (C01/C10).
4. Carry that fixture through SSA, calls/types, recovered AST, structured source, and synchronized review; add complexity only after the complete path runs (C02–C11).
