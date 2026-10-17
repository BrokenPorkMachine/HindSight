# Decompiler and Reverse Engineering Feature Roadmap

This checklist compares HindSight's current analysis path with capabilities documented by Hopper, Ghidra, IDA, and Binary Ninja. It focuses on features that can improve evidence-backed static analysis in HindSight's current Apple binary workflow. The commercial tools' output is not treated as ground truth: each recovered name, type, and structure should retain its evidence and uncertainty.

## Current baseline

- [x] ARM64 instruction semantics with bounded constant and provenance tracking.
- [x] Basic-block construction and bounded control-flow propagation.
- [x] Address-bearing ARM64 lifted instruction/function records consumed directly by CFG and value analysis.
- [x] C-like ARM64 output with dyld, Objective-C, C, CoreFoundation, and supported C++ symbol/signature recovery.
- [x] Pseudocode accessible from the Binary Workspace and application navigation.
- [x] Conservative conditional-return structuring for one proven branch pattern.
- [ ] General source-variable, stack-local, and type recovery.
- [ ] General C control-flow structuring, switch recovery, and architecture-independent decompilation.

## P0 — Make current pseudocode dependable and useful

- [ ] **Normalize function boundaries.** Combine Mach-O function starts, exports, Objective-C method IMPs, unwind/function-start metadata, symbol ranges, and validated control-flow discoveries. Track boundary source and confidence; prevent fallthrough into adjacent functions.
- [x] **Build a first-class per-function CFG.** Preserve block edges, predecessors, successors, reachability, dominators, loop backedges, and lifted branch conditions as structured data in the ARM64 semantics pass.
- [x] **Prune proven conditional CFG edges.** Block-local constants and known NZCV results remove impossible branch edges before reachability and dominator analysis; unknown or unsupported operations retain both paths.
- [x] **Add an initial lifted IR.** Address-bearing typed ARM64 instruction records and a function container now feed CFG construction, propagation, and rendering without reparsing formatted statements.
- [x] **Add a shared IR core.** ARM64 now maps common assignments, integer arithmetic/logical operations, simple memory accesses, calls, returns, and branches into architecture-neutral records. The CFG consumes shared control-flow operations; unsupported forms remain explicitly target-specific.
- [x] **Route the shared IR through analysis and rendering.** Common assignments, arithmetic/logic, supported memory operations, calls, returns, and branches now update value flow and render from the shared records.
- [x] **Preserve load/store writeback.** Pre-indexed and post-indexed immediate memory operations now record base-register updates in shared IR and apply them in architectural order.
- [ ] **Complete shared IR coverage.** ARM64 compare/test and conditional compare operations have explicit shared records; known NZCV values can pass through known flag-preserving moves, shifts, and memory operations to resolve a branch or select. Flag-setting arithmetic and unknown operations invalidate the state. Shifted arithmetic, shifts, multiply families, logical-register operations, move-wide keep, division, one-source operations, bitfield operations, extended add/subtract, and sign/zero extension also use shared operations.
- [ ] **Replace machine-register temporaries with recovered variables.** Use def-use chains and SSA versions, then merge versions at control-flow joins. Name stack slots and stable values as locals; retain register names only when recovery is ambiguous.
- [ ] **Structure common control flow.** Render proven `if`/`else`, early returns, loops, and switch/jump tables. Fall back to labeled blocks when the structure cannot be proven, and show why recovery stopped.
- [ ] **Improve call-site reconstruction.** Propagate ABI argument values to direct and indirect calls; use imported prototypes, recovered function signatures, Objective-C method encodings, and call-site evidence. Preserve unknown arguments and side effects explicitly.
- [ ] **Keep source mapping in the UI.** Make every pseudocode statement navigate to its instruction range; selecting an instruction highlights the corresponding expression and recovered variable uses.

## P1 — Recover types and Apple runtime semantics

- [ ] **Recover function signatures.** Combine exports, nlist symbols, debug info where available, Objective-C encodings, Swift metadata where supported, C/C++ mangling, ABI register use, return behavior, and call-site constraints. Track per-parameter evidence and confidence.
- [ ] **Infer stack locals and aggregate layouts.** Group stack and pointer-relative accesses by base, offset, access width, and use. Suggest structure fields only when access evidence supports a stable layout; allow analyst edits.
- [ ] **Add a project type library.** Import and apply SDK headers, user C declarations, structs, enums, typedefs, and function prototypes. Initially support Foundation, CoreFoundation, libSystem, and Objective-C runtime APIs.
- [ ] **Model Objective-C dispatch and objects.** Resolve `objc_msgSend` call shapes against selector/class/method metadata; render message sends and property-like patterns only when receiver and selector evidence support them. Track class, metaclass, category, protocol, and IMP relationships.
- [ ] **Add type propagation and casts.** Propagate types bidirectionally through assignments, calls, returns, pointer arithmetic, and memory access. Mark conflicts and avoid silently coercing incompatible evidence.
- [ ] **Add analyst overrides.** Rename functions/variables, set types and signatures, mark a switch, split or merge variables, and add comments. Persist edits as project annotations with undo and provenance, not as mutations to the original binary.

## P2 — Broaden analysis coverage

- [ ] **Recover data references and strings.** Improve pointer/string cross-references, constant-address resolution, Objective-C constant strings, CFString/CFData patterns, and global variable naming.
- [ ] **Recognize common library idioms.** Detect memory copy/clear, retain/release, bounds checks, tagged-pointer checks, dispatch blocks, and common Foundation/CoreFoundation patterns using data flow and call prototypes.
- [ ] **Recover switch tables and indirect control flow.** Identify bounded jump tables and Objective-C/C++ vtables; record table entries and confidence, and keep unresolved targets visible.
- [ ] **Add another architecture through the IR.** Prioritize x86-64 Mach-O. Implement its lifter, ABI model, and calling-convention tests against the same function IR; do not claim decompilation coverage for architectures that only receive structural parsing.
- [ ] **Handle stripped and optimized code better.** Use unwind metadata, thunk/tail-call recognition, prologue evidence, cross-references, and call graph constraints to discover functions and distinguish cold fragments.
- [ ] **Add function and call graphs.** Show callers/callees, Objective-C dispatch possibilities, unresolved indirect calls, and confidence. Support graph navigation and analyst annotations.

## P3 — Make results reviewable and scalable

- [ ] **Add per-function analysis controls.** Reanalyze, bound analysis, select simplification level, show raw lifted IR, show CFG, and compare assembly, IR, and pseudocode.
- [ ] **Expose uncertainty and alternatives.** Let analysts inspect competing type, target, function-boundary, and control-flow hypotheses with the evidence that supports each.
- [ ] **Add decompilation quality corpus.** Maintain redistributable small C, Objective-C, C++, and Objective-C++ fixtures with optimized and stripped builds. Measure recovered CFG, expressions, calls, types, and source mapping without requiring exact source-text matches.
- [ ] **Add resource and cancellation controls.** Bound work per function and project, expose progress, and allow cancellation without discarding completed analysis.
- [ ] **Export standard artifacts.** Add function signatures, types, call graphs, and annotations to JSON and interoperable formats where practical.

## Suggested implementation order

1. Expand shared IR coverage for the remaining instruction families and add another lifter.
2. SSA and variable/stack-local recovery.
3. Structured `if`/`else`, loops, and switches.
4. ABI-aware calls and signature/type propagation.
5. Foundation/CoreFoundation and Objective-C object/dispatch models.
6. Analyst type/variable edits and persistent annotations.
7. x86-64 lifter and broader function discovery.
8. UI navigation, quality corpus, and artifact export improvements alongside each phase.

## Reference capabilities

- [Hopper tutorial](https://www.hopperapp.com/tutorial.html) documents CFG navigation, annotations, references, and C-like pseudocode.
- [Ghidra P-code reference](https://ghidra.re/ghidra_docs/languages/html/pcoderef.html) describes a processor-independent instruction semantics layer used to build data-flow graphs.
- [Ghidra Decompiler API](https://ghidra.re/ghidra_docs/api/ghidra/app/decompiler/DecompInterface.html) includes graph structuring and decompilation controls.
- [IDA Microcode API](https://ida-domain.docs.hex-rays.com/ref/microcode/) exposes a block graph, call analysis, local variables, and global/local optimization passes.
- [Binary Ninja IL overview](https://docs.binary.ninja/dev/bnil-overview.html) documents its progression from lifted semantics through SSA, variable/type-aware MLIL, and structured HLIL.
- [Binary Ninja HLIL](https://docs.binary.ninja/dev/bnil-hlil.html) describes its AST representation for nested control flow and pseudocode rendering.
