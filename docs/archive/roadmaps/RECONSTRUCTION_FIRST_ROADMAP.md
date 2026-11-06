# Reconstruction-First Roadmap (Independent Draft)

Status: independent draft retained as an input to the [Chimera master list](CHIMERA_COMPLETION_ROADMAP.md). The master list governs sequencing, C17 output, validation claims, and completion gates.

## North star

Make source reconstruction the product's first completion target. Given a supported binary and its available metadata, HindSight should produce reviewable, recompilable C-family source whose observable behavior matches the machine code and whose structure approaches the likely original source wherever evidence permits.

“1:1” cannot mean byte-for-byte recovery of information erased by compilation: original comments, whitespace, macro boundaries, local names, typedef spelling, and many optimized-away constructs are not generally recoverable. Completion instead means:

1. semantic equivalence for the supported execution domain;
2. complete traceability from each reconstructed construct to machine evidence;
3. near-original types, names, expressions, and control flow when debug/runtime/linkage/cross-function evidence supports them;
4. explicit alternatives and uncertainty when the binary does not determine one source representation;
5. reproducible validation by recompilation, differential execution, and corpus scoring.

## Dependency spine

Every later phase depends on the earlier contracts. UI polish and breadth do not outrank semantic correctness.

```text
binary truth + normalized addresses
  -> complete decoded effects
  -> architecture-neutral typed IR
  -> memory + register SSA
  -> interprocedural summaries/call graph
  -> type/ownership/alias constraints
  -> source variables and expressions
  -> regions, loops, switches, exceptions
  -> source AST + evidence graph
  -> compilable emission
  -> differential validation and UI review
```

## Definition of done

The reconstruction milestone is complete only when all mandatory gates below pass on checked-in, legally redistributable `-O0`, `-O1`, `-O2`, `-Os`, stripped, and symbol-rich fixtures.

- Every emitted AST node has stable identity, source address range(s), confidence, and evidence/assumption edges.
- Unsupported instructions or effects cannot silently disappear; they block compilable output for the affected slice or become explicit intrinsics with a stated contract.
- Recompiled eligible functions pass ABI-aware differential tests against the original for generated and curated inputs.
- Memory, calls, volatile/atomic behavior, integer overflow policy, FP mode assumptions, and undefined-behavior assumptions are recorded.
- Round-trip CFG and def-use comparisons show no unexplained executable paths or side effects.
- Exact-source similarity is measured separately from semantic equivalence; prettier output cannot hide a behavioral regression.
- Every confidence claim is derived from evidence, not renderer heuristics.
- Resource budgets, cancellation, deterministic output, and truncation markers are exercised in CI.

## R0 — Establish measurable reconstruction truth

- [ ] Define a versioned reconstruction schema: function AST, expressions, variables, types, regions, effects, source spans, alternatives, confidence, and evidence references.
- [ ] Define an evidence graph schema with typed nodes and edges for bytes, decode, IR operations, definitions, uses, memory objects, CFG edges, calls, symbols, metadata, types, hypotheses, AST nodes, validation runs, and analyst overrides.
- [ ] Give every evidence node a stable content-derived identity and retain producer/version/budget information.
- [ ] Separate facts, derived facts, assumptions, analyst assertions, and rejected alternatives in the schema.
- [ ] Add queryable “why?”, “why not?”, “depends on”, “conflicts with”, and “invalidated by” paths.
- [ ] Establish corpus ground truth from source-to-binary builds with retained compiler map data used only by scoring, never by recovery.
- [ ] Add metrics for instruction/effect coverage, CFG agreement, def-use agreement, type accuracy, variable recovery, AST similarity, recompilation success, and differential behavior.
- [ ] Add per-function eligibility states: `equivalent`, `recompilable-with-intrinsics`, `review-only`, and `blocked`, each with machine-readable reasons.
- [ ] Gate regressions by optimization level, language, ABI feature, and instruction family rather than one aggregate score.

## R1 — Complete the semantic substrate

- [ ] Inventory every ARM64/ARM64e opcode observed in the corpus and system fixtures; classify it as exact semantics, conservative intrinsic, privileged/runtime-only, or unsupported.
- [ ] Finish shared IR coverage for scalar/vector integer and FP operations, conversions, flags, atomics, barriers, system effects, pointer authentication, and all supported memory addressing modes.
- [ ] Model NZCV, FPCR/FPSR, vector lanes, partial-register writes, SP, PC-relative values, and architectural aliases explicitly.
- [ ] Define exact IR integer widths, signedness-neutral bitvector behavior, poison/unknown values, overflow, shifts, endianness, alignment, volatility, atomic ordering, and exception behavior.
- [ ] Replace formatted-text reparsing with structured IR consumers everywhere in reconstruction.
- [ ] Add differential instruction tests against hardware or a qualified independent oracle for every exact semantic family.
- [ ] Add negative decoder tests proving reserved and neighboring encodings cannot be misclassified.
- [ ] Make semantic coverage reports and unsupported-effect blockers part of the reconstruction artifact.

## R2 — Build whole-function state and memory SSA

- [ ] Promote bounded GPR SSA evidence into the canonical reconstruction IR with dominance-correct definitions, uses, phi nodes, and stable value IDs.
- [ ] Add FP/vector, flags, SP/frame, and relevant special-register SSA.
- [ ] Introduce memory SSA with explicit loads, stores, calls, atomics, volatile accesses, unknown clobbers, and region partitions.
- [ ] Recover stack frames across prologue/epilogue variants, dynamic alignment, red-zone-like compiler patterns, saved registers, and stack-pointer changes.
- [ ] Partition memory conservatively into stack objects, globals, constants, heap/unknown objects, fields, and escaped aliases.
- [ ] Add bounded alias analysis and make imprecision visible rather than merging unrelated objects silently.
- [ ] Recover variable live ranges and coalesce SSA values only when interference, alias, width, and type constraints permit it.
- [ ] Preserve copy provenance so compiler moves disappear from source without losing their evidence path.
- [ ] Verify reconstructed def-use and memory-effect graphs against execution traces for corpus functions.

## R3 — Interprocedural truth before pretty pseudocode

- [ ] Build a first-class call graph containing direct, tail, import, Objective-C, block, function-pointer, vtable, thunk, and unresolved indirect edges with confidence and alternatives.
- [ ] Compute bounded per-function summaries for parameters, returns, clobbers, memory regions, allocation/ownership, throws/nonreturns, and indirect targets.
- [ ] Iterate caller/callee constraints to a deterministic fixed point with explicit widening and budgets.
- [ ] Recover complete AAPCS64 argument/return locations including aggregates, homogeneous aggregates, variadics, stack values, and hidden result/context parameters.
- [ ] Add Swift ABI, broader Itanium C++, blocks/closures, Objective-C dispatch, and platform thunk models.
- [ ] Infer signatures from all callers and callees while retaining conflicts and alternative prototypes.
- [ ] Recognize library/runtime functions by validated symbol, import, signature, and behavior evidence; never by a weak name alone.
- [ ] Model call side effects precisely enough that unrelated values and memory survive calls when justified.

## R4 — Recover types, objects, and ownership

- [ ] Implement a project type library for SDK headers, DWARF, dSYM, user declarations, runtime encodings, structs, enums, unions, typedefs, templates, and Swift metadata.
- [ ] Use a constraint solver for bidirectional type propagation across SSA, phi nodes, calls, returns, pointer arithmetic, loads/stores, comparisons, and casts.
- [ ] Keep machine representation, inferred semantic type, and preferred source spelling distinct.
- [ ] Infer aggregate layouts from offset/width/use constraints and show competing layouts when evidence is insufficient.
- [ ] Recover Objective-C class/metaclass/category/protocol/property/ivar/method relationships and receiver-class alternatives.
- [ ] Recover Swift nominal/generic/existential/closure metadata and ownership conventions within an explicitly supported ABI set.
- [ ] Infer signedness, nullability, mutability, constness, ownership, lifetime, and alias qualifiers with separate confidence.
- [ ] Ensure incompatible evidence creates a visible conflict instead of an implicit cast or arbitrary winner.

## R5 — Reconstruct source variables and expressions

- [ ] Replace machine-register temporaries with SSA-backed parameters, locals, fields, globals, captured values, and return values.
- [ ] Name variables from debug/runtime/symbol/API evidence first; use deterministic role-based names only when source names are absent.
- [ ] Recover expression trees using dominance, use counts, side-effect ordering, sequence points, overflow rules, and readability/cost constraints.
- [ ] Recover lvalues, address-taking, dereferences, array indexing, field access, pointer arithmetic, casts, and compound assignments.
- [ ] Identify induction variables, bounds, accumulators, state variables, flags, and compiler-generated temporaries through evidence-backed patterns.
- [ ] Preserve multiple source expressions when machine behavior does not distinguish them; select a preferred rendering without deleting alternatives.
- [ ] Add expression-level machine-address ranges and variable-use highlighting.
- [ ] Make analyst renames/types/splits/merges versioned graph assertions with undo/redo, not destructive rewrites of machine evidence.

## R6 — Structure all supported control flow

- [ ] Build a region tree from dominators, post-dominators, loop forests, control dependence, and exception edges.
- [ ] Recover nested conditionals, early exits, short-circuit Boolean expressions, ternaries, and guard clauses.
- [ ] Recover natural and multi-exit loops as `for`, `while`, or `do` forms using induction and exit evidence.
- [ ] Recover switches with case ranges, normalized indices, defaults, fallthrough, sparse tables, and compiler lowering variants.
- [ ] Model irreducible CFGs explicitly and use labeled control flow only where no semantics-preserving structured form is proven.
- [ ] Recover tail calls, non-returning calls, thunks, cold fragments, landing pads, and supported exception constructs.
- [ ] Verify the structured AST regenerates the original CFG modulo proven simplifications.

## R7 — Emit evidence-backed, recompilable source

- [ ] Make a typed source AST—not formatted pseudocode strings—the only input to source emission.
- [ ] Emit deterministic C11/Objective-C/C++/Swift-subset source according to the recovered language/ABI contract.
- [ ] Generate declarations, forward types, imports/includes, globals, constants, helper intrinsics, and calling-convention annotations needed to compile.
- [ ] Preserve volatile, atomic, packing, alignment, signed overflow, FP, and pointer-authentication behavior.
- [ ] Provide two modes: readable reconstruction and strict semantic reconstruction; both reference the same AST/evidence graph.
- [ ] Refuse “compilable” status when unresolved effects lack a sound intrinsic or contract.
- [ ] Emit a reconstruction manifest containing compiler assumptions, unresolved alternatives, blockers, evidence digest, and validation results.

## R8 — Prove equivalence and score fidelity

- [ ] Recompile reconstructed functions for the original target ABI and compare normalized CFG, calls, memory effects, and observable results.
- [ ] Add differential execution in bounded native and emulated harnesses with generated inputs, edge cases, and state snapshots.
- [ ] Add SMT-backed equivalence for bounded pure/basic-block regions and report solver assumptions/timeouts.
- [ ] Compare reconstructed AST with retained corpus source using normalized tree edit distance, type/name recovery, and control-structure metrics.
- [ ] Add metamorphic tests across compiler versions and optimization flags so recovery does not overfit one code generator.
- [ ] Require zero unexplained side-effect mismatches for `equivalent` status.
- [ ] Store validation results as evidence graph nodes linked to the exact binary, function, AST, toolchain, and test inputs.
- [ ] Publish a dashboard of honest coverage and fidelity, including failures and unsupported categories.

## R9 — Make the evidence graph the primary review interface

- [ ] Show synchronized source, AST, IR, SSA/memory SSA, CFG, assembly, bytes, types, calls, and runtime observations.
- [ ] Selecting any source construct highlights all contributing instructions and graph paths; selecting an instruction highlights every dependent source construct.
- [ ] Provide inline confidence, alternatives, conflicts, blockers, and assumption toggles.
- [ ] Let analysts ask why a name/type/expression/control structure was chosen and inspect the minimal supporting evidence path.
- [ ] Support per-function reanalysis after an override, with dependency invalidation and reproducible before/after diffs.
- [ ] Export/import the reconstruction AST, evidence graph, overrides, validation manifest, and content hashes.
- [ ] Add collaborative review state without allowing comments or votes to masquerade as machine facts.

## R10 — Breadth only after the vertical slice passes

- [ ] Qualify the full reconstruction pipeline first for ARM64/ARM64e Mach-O C and Objective-C.
- [ ] Add C++ only after exceptions, aggregates, vtables, thunks, and Itanium ABI gates pass.
- [ ] Add Swift only after its ABI, metadata, ownership, generics, and closure gates have dedicated corpora.
- [ ] Add x86-64 Mach-O through the same IR, evidence, AST, and validation contracts—never a parallel text-only decompiler.
- [ ] Extend ELF/PE/COFF reconstruction only after their relocation, linkage, debug, unwind, and ABI evidence is complete enough for the same gates.

## First vertical slice

Before broadening formats or UI, deliver one exceptional end-to-end slice:

- ARM64/ARM64e Mach-O;
- C and Objective-C functions with supported metadata;
- integer/scalar-FP code, stack/globals, direct calls, bounded indirect calls, structs, common loops/branches/switches;
- typed AST plus evidence graph;
- deterministic compilable C/Objective-C output;
- recompilation and differential-execution reports;
- synchronized source/IR/SSA/CFG/assembly review.

The slice is not complete because a demo looks convincing. It is complete when the corpus gates prove semantics, provenance, failure honesty, and reproducibility.

## Work deliberately deferred behind reconstruction

Until the first vertical slice passes, these are supporting work only when they unblock it: additional binary formats, generalized collaboration, on-prem execution, licensing, updater work, broad plugin ecosystems, and cosmetic pseudocode improvements without AST/equivalence gains.

Security, crash recovery, deterministic caching, cancellation, and resource limits are not deferred because they are prerequisites for trustworthy validation.
