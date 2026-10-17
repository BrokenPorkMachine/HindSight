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

- [x] **Normalize function boundaries.** One aligned, deduplicated boundary set from Mach-O function starts, executable nlist symbols, validated Objective-C IMPs, exports, validated `__unwind_info` index/page entries, and in-range direct `BL` targets drives CFG, function, pseudocode, unified-symbol, and static-symbolication ranges. Reports share boundary evidence and confidence; call-target-only entries remain candidates. The decompilation corpus verifies the function, resolver, and symbolication range sets match.
- [x] **Build a first-class per-function CFG.** Preserve block edges, predecessors, successors, reachability, dominators, loop backedges, and lifted branch conditions as structured data in the ARM64 semantics pass.
- [x] **Prune proven conditional CFG edges.** Block-local constants and known NZCV results remove impossible branch edges before reachability and dominator analysis; unknown or unsupported operations retain both paths.
- [x] **Add an initial lifted IR.** Address-bearing typed ARM64 instruction records and a function container now feed CFG construction, propagation, and rendering without reparsing formatted statements.
- [x] **Add a shared IR core.** ARM64 now maps common assignments, integer arithmetic/logical operations, simple memory accesses, calls, returns, and branches into architecture-neutral records. The CFG consumes shared control-flow operations; unsupported forms remain explicitly target-specific.
- [x] **Route the shared IR through analysis and rendering.** Common assignments, arithmetic/logic, supported memory operations, calls, returns, and branches now update value flow and render from the shared records.
- [x] **Preserve load/store writeback.** Pre-indexed and post-indexed immediate scalar and pair memory operations now record base-register updates in shared IR and apply them in architectural order. Pair forms with a writeback/base-register overlap remain target-specific because their architectural result is constrained or unpredictable.
- [ ] **Complete shared IR coverage.** ARM64 compare/test and conditional compare operations have explicit shared records; known NZCV values can pass through known flag-preserving moves, shifts, and memory operations to resolve a branch or select. Flag-setting arithmetic and unknown operations invalidate the state. Shifted arithmetic, shifts, multiply families, logical-register operations, move-wide keep, division, one-source operations, bitfield operations, extended add/subtract, sign/zero extension, pair load/store instructions, system markers, discarded zero-register results, atomic/exclusive single-register accesses, scalar single/double FP add, subtract, multiply, divide, register/zero comparisons, single-to-double/double-to-single precision conversion, signed/unsigned 32/64-bit integer/FP conversions, fixed-point conversions with encoded fractional scaling, and scalar `S`/`D` memory accesses using unsigned, unscaled, pre-indexed, post-indexed, and supported register-offset addressing now have shared records. Floating comparison flags are rendered but not propagated. Vector/SIMD lanes, unprivileged FP encodings, atomic pair forms, and remaining target-specific instructions still need explicit shared semantics or documented fallback rules. Fixed-point float-to-integer output records truncation and leaves out-of-range architecture behavior marked unresolved.
- [ ] **Replace machine-register temporaries with recovered variables.** The pseudocode names SP-relative slots only when the decoded function proves SP is unchanged and every instruction effect is modeled. Recovered C and Objective-C parameter names now flow from ABI registers through straight-line statements until a register is redefined; branch/loop joins keep explicit registers. Other functions retain register-based memory expressions. General register def-use chains, SSA versions, and merges at control-flow joins remain.
- [ ] **Structure common control flow.** Proven straight-line `if`/`else` diamonds now render as nested blocks only when both arms reach one explicit join and no branch enters an arm interior. Straight-line natural loops with one conditional backedge render as `do`/`while`; header-tested loops render as `while` only when their exit immediately follows one explicit backedge and the body is linear. Bounded jump tables render as candidate switches, while unstructured direct and indirect branches state why they remain as gotos. Nested regions and broader CFG structuring remain.
- [ ] **Improve call-site reconstruction.** Direct ARM64 BL pseudocode now maps known prototypes to AAPCS64 parameter registers, substitutes proven x0-x7 constants with instruction provenance, and uses entry-register evidence for candidate argument arity when a callee prototype is unavailable. Unknown prototype arity, indirect calls, stack arguments, and call side effects remain unresolved. Imported prototypes and Objective-C method encodings supply supported arity and names.
- [ ] **Keep source mapping in the UI.** Address-bearing pseudocode statements in the Binary Workspace navigate using only the renderer's leading instruction address; embedded call targets, constants, and memory addresses cannot hijack the source mapping. Assembly and pseudocode share the selected instruction, highlight it, and scroll it into view in each pane. Statements synthesized from multiple instructions, expression-level variable-use highlighting, and source ranges remain.

## P1 — Recover types and Apple runtime semantics

- [ ] **Recover function signatures.** Exact ABI prototypes cover selected Foundation/CoreFoundation, C runtime, libdispatch, and Objective-C runtime APIs; validated Objective-C method encodings, supported Itanium mangling, and ARM64 entry-register candidates add further evidence. Pseudocode maps Objective-C self/_cmd and typed selector parameters to ABI registers, maps C/C++ prototype parameters across the integer and FP register banks, and labels unsupported incoming values as candidates. Direct calls show formal-to-register bindings and retain provenance for proven constant substitutions. Swift function signatures, debug-info ingestion, complete per-parameter confidence, and broader caller/callee constraint solving remain.
- [ ] **Infer stack locals and aggregate layouts.** The report groups decoded SP-relative accesses by signed offset, observed width, read/write role, and example sites. Pseudocode uses the same slot labels only when SP stability is proven; otherwise it keeps machine expressions and explains that SP stability was not proven. General pointer-relative grouping, variable lifetimes, aggregate field inference, and analyst edits remain.
- [ ] **Add a project type library.** Import and apply SDK headers, user C declarations, structs, enums, typedefs, and function prototypes. Initially support Foundation, CoreFoundation, libSystem, and Objective-C runtime APIs.
- [ ] **Model Objective-C dispatch and objects.** Validated class and method metadata now handles 72-byte 64-bit `class_ro_t` records and relative method lists; selector-specific `__objc_stubs` resolve their validated `__objc_selrefs` selector and render message sends. Receiver and argument types remain unresolved, as do other stub shapes, receiver class resolution, property patterns, and complete class/metaclass/category/protocol/IMP relationships.
- [ ] **Add type propagation and casts.** Propagate types bidirectionally through assignments, calls, returns, pointer arithmetic, and memory access. Mark conflicts and avoid silently coercing incompatible evidence.
- [ ] **Add analyst overrides.** Project-local function overrides now persist an analyst name, signature hypothesis, confidence, and note, display them as annotations without altering decoded evidence, and retain up to ten per-function revisions for undo. Variable/type edits, switch marking, split/merge operations, and cross-function undo/redo remain.

## P2 — Broaden analysis coverage

- [ ] **Recover data references and strings.** Improve pointer/string cross-references, constant-address resolution, Objective-C constant strings, CFString/CFData patterns, and global variable naming.
- [ ] **Recognize common library idioms.** Exact prototypes now identify memory allocation/copy/clear/compare, string operations, Objective-C retain/release, and libdispatch calls in pseudocode. Data-flow idiom recognition for bounds checks, tagged pointers, dispatch blocks, and Foundation/CoreFoundation patterns remains.
- [ ] **Recover switch tables and indirect control flow.** The worker now recovers up to 32 executable-aligned targets for indexed 64-bit pointer tables and table-relative signed-offset tables when ADR/ADRP plus ADD provenance and the BR data path match. The pseudocode renders those entries as a candidate switch on the table index, with address-bearing case targets and an unresolved default. Case-range proof, index normalization, other encodings, and Objective-C/C++ vtable recovery remain.
- [ ] **Add another architecture through the IR.** Prioritize x86-64 Mach-O. Implement its lifter, ABI model, and calling-convention tests against the same function IR; do not claim decompilation coverage for architectures that only receive structural parsing.
- [ ] **Handle stripped and optimized code better.** Use unwind metadata, thunk/tail-call recognition, prologue evidence, cross-references, and call graph constraints to discover functions and distinguish cold fragments.
- [ ] **Add function and call graphs.** Show callers/callees, Objective-C dispatch possibilities, unresolved indirect calls, and confidence. Support graph navigation and analyst annotations.

## P3 — Make results reviewable and scalable

- [ ] **Add per-function analysis controls.** The Binary Workspace now filters CFG blocks to the selected function and lets users navigate from block/edge records to instruction addresses alongside assembly, lifted statements, IR, and pseudocode. Per-function reanalysis, analysis bounds, and simplification controls remain.
- [ ] **Expose uncertainty and alternatives.** Let analysts inspect competing type, target, function-boundary, and control-flow hypotheses with the evidence that supports each.
- [ ] **Add decompilation quality corpus.** Public-domain C, Objective-C, C++, and Objective-C++ fixtures compile at `-O0`/`-O2` with stripped and unstripped variants. The macOS runner scores function, CFG, IR, pseudocode, validated Objective-C method, selector message-send, and address-bearing parameter-name-use evidence without exact source-text matching. Expression, call-target, type, source-mapping quality metrics and CI integration remain.
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
