# HindSight Development Plan

## Product direction

HindSight should help a reverse engineer move from an immutable artifact to a traceable, analyst-owned conclusion. The product must make the next investigation step obvious while keeping verified observations, inferred structure, runtime facts, and analyst judgement distinct.

## Current baseline

The app already has a broad macOS workbench covering project/artifact management, bounded multi-architecture analysis, evidence triage, binary diffing, debugger/runtime surfaces, firmware and emulation tools, scripting/plugins, investigation continuity, collections, review, follow-ups, exports, Quick Start, and evidence annotations.

## Priority roadmap

### Phase 1 — Investigation usability

- [x] Replace visible legacy branding with HindSight.
- [x] Add Quick Start and the Import → Analyze → Triage → Document → Export path.
- [x] Add evidence annotations with notes, tags, confidence, persistence, handoff export, and Annotated Triage mode.
- [x] Add saved searches and reusable triage filters.
- [x] Make annotations available from every evidence card without requiring the inspector.
- [x] Add an analyst-facing “annotated handoff” summary to the main Overview.

Acceptance: a new user can import an artifact, run analysis, find a result, annotate it, retrieve it later, and export a handoff without searching through advanced workspaces.

### Phase 2 — Analysis quality and depth

- [x] Add bounded compact-unwind metadata ingestion; DWARF/dSYM sections remain explicitly inventory-only.
- [x] Add bounded ARM64 stack-slot variable-location candidates from decoded stack-relative loads/stores; source names, lifetimes, layouts, and types remain unresolved.
- [x] Label validated Swift type-descriptor names with relative-pointer provenance and confidence.
- [x] Surface bounded Swift field-variable counts with explicit unresolved layout/declaration boundaries.
- [x] Add bounded AArch64 call-argument/return-register provenance; source-level types and callee prototypes remain explicit unknowns.
- [x] Extend the bounded ARM64 `semantic_preview` to the complete move-wide immediate family (MOVN/MOVZ/MOVK); `MOVK` claims a constant only from a known prior value, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to flag-neutral logical operations (AND/ORR/EOR bitmask immediates and unshifted register AND/ORR/EOR/BIC/ORN/EON); reserved bitmask encodings, ANDS/BICS, and shifted operands stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to PC-relative ADR/ADRP address materialization with unslid provenance; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to conditional and indirect control flow (B.cond, CBZ/CBNZ, TBZ/TBNZ, BR/BLR) with named condition/register tests and unresolved indirect targets; PSTATE flags and indirect-target values remain unrecovered, and each transfer clears tracked constants.
- [x] Resolve BR/BLR indirect targets in the bounded ARM64 `semantic_preview` when the target register carries a proven constant, printing the address and its provenance; untracked registers stay named as unresolved, and value-flow recovery for unknown registers remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to immediate shifts (LSL/LSR/ASR, the canonical UBFM/SBFM aliases) with sign-propagating arithmetic-shift constant folding; bitfield extracts stay opaque, and shifted register operands remain open.
- [x] Extend the bounded ARM64 `semantic_preview` to multiply-add (MADD/MSUB with the MUL/MNEG zero-addend aliases) with 32/64-bit constant folding; the wider SMADDL/UMADDL/... variants stay opaque, and shifted register operands remain open.
- [x] Extend the bounded ARM64 `semantic_preview` to integer division (SDIV/UDIV) with zero-divisor-yields-zero and saturating INT_MIN/-1 semantics, rendering `dst = left sdiv right` / `udiv right` and folding proven constants with provenance; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the widening and high-half multiplies (SMADDL/UMADDL/SMSUBL/UMSUBL with their SMULL/UMULL zero-addend aliases, and SMULH/UMULH), keeping the narrow 32-bit source view explicit for the widening forms and folding proven 128-bit products with provenance; unallocated op31 encodings stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the sign/zero extend aliases (UXTB/UXTH/SXTB/SXTH/SXTW) with width-correct constant folding; general bitfield inserts/extracts stay opaque for now, and shifted register operands remain open.
- [x] Extend the bounded ARM64 `semantic_preview` to LSL/LSR/ASR shifted register operands on the modeled ADD/SUB and logical operations, applying the shift before the enclosing operation and folding it into proven constants; ROR and flag-setting variants stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the general bitfield extracts (UBFX/SBFX) and inserts (BFI/BFXIL/UBFIZ/SBFIZ), rendering the non-wrapping field `[lsb, lsb+width)` with zero/sign folding; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to 32/64-bit general-register loads and stores (LDR/STR with a scaled unsigned immediate offset), resolving base+offset addresses with provenance and treating a load as an unknown barrier for its destination; byte/halfword, sign-extending, prefetch, scaled-FP, unscaled, and indexed forms stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the byte/halfword and sign-extending general-register loads and stores (STRB/STRH/LDRB/LDRH/LDRSB/LDRSH/LDRSW), naming the exact access width and signedness in the dereference when it differs from the data-register width and keeping the whole-register forms untyped; prefetch, the unallocated opc 3 32/64-bit forms, scaled-FP, unscaled, and indexed forms stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to register-offset single-register loads and stores ([base, index, extend/shift]) for the same family, rendering the index expression with UXTW/LSL/SXTW/SXTX and optional scaling, and resolving addresses with provenance when base and index are proven constants; pre/post-index, pairs, and literal loads stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to unscaled (`LDUR`/`STUR`, signed immediate), pre-index (`[Rn, #imm]!`), and post-index (`[Rn], #imm`) single-register loads and stores, with negative offsets, writeback rendering, and provenance propagation into the base register; register-offset-with-writeback, pairs, and literal loads stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to PC-relative literal loads (`LDR`/`LDRSW` literal), rendering the address as `pc + offset` or `pc - abs(offset)`, with resolved target address and provenance; `PRFM` literal stays opaque, and cross-basic-block propagation remains open.
- [x] Correct the ARM64 `semantic_preview` literal-load class: the earlier mask (bits 29-24 == `011010`) also matched the conditional-select and data-processing (1 source)/(2 source) groups, so real words such as `csel` and `lsl w0, w0, w0` were mis-decoded as literal loads; the class now requires bits 29-24 == `011000` with `opc` at bits 31-30 and imm19 at bits 23-5, the non-existent `LDRSB`/`LDRSH` literal forms are dropped, the encodings are re-verified with `clang -arch arm64`, and the cache engine identity advances to `arm64-analysis-v19-schema72` because this correction removes previously emitted statements; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to conditional select (CSEL/CSINC/CSINV/CSNEG with the CSET/CSETM/CINC/CINV/CNEG aliases), rendering `dst = cond ? rn : rm;` with the increment/complement/negate false-operand forms, naming the condition code directly, and selecting (and constant-folding) the true operand only for the always-true `al` condition because PSTATE flags are not tracked; other conditions are unknown barriers, the flag-setting form and reserved encoding bit stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the register-defined variable shifts (LSLV/LSRV/ASRV/RORV) from the data-processing (2 source) group, rendering `dst = src << amount;` / `>> amount` / `>>> amount` and `dst = ror(src, amount);`, reading the amount from a whole register reduced modulo the register width (so a shift by the full width is a no-op) and folding proven constants with provenance; the division forms share the group and are modeled above, unallocated opcodes of the group stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the data-processing (1 source) family (RBIT/REV16/REV/REV32/CLZ/CLS), rendering `dst = rbit(src);` (and the `rev16`/`rev`/`rev32`/`clz`/`cls` forms) computed at the register width, folding bit/byte reversals and leading-bit counts with provenance, keeping the opcode-2 32-bit `REV`/64-bit `REV32` split and the 64-bit-only opcode-3 `REV`, and leaving the unallocated opcodes of the group opaque; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the concatenated-field extract (EXTR) and its same-register ROR immediate alias, rendering `dst = extr(left, right, lsb);` (and `dst = ror(src, lsb);` when the sources match), folding the field with provenance, enforcing the encoding's `N == sf` and `o0 == 0` requirements, bounding the 32-bit `imms` to 0..31, and leaving unallocated encodings opaque; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the flag-neutral add/subtract with carry (ADC/SBC), rendering `dst = left + right + carry;` and `dst = left + ~right + carry;` with the untracked `PSTATE.C` input left as an explicit unknown barrier (so no constant is ever folded), while the flag-setting ADCS/SBCS forms and the conditional compares stay opaque; cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the add/subtract (extended register) form (ADD/SUB with UXT*/SXT* and the `imm3` shift, flag-neutral only), rendering the extension applied before the shift (`dst = left + (zext(w2, 8) << 2);` and signed counterparts, with UXTX/SXTX using the operand unchanged), folding the extension and shift with provenance, naming the stack pointer for `Rd`/`Rn` when their field is 31 while `Rm` names `ZR`, and leaving the flag-setting ADDS/SUBS forms and unallocated `imm3` values opaque; cross-basic-block propagation remains open.
- [x] Audit the bounded ARM64 `semantic_preview` against the encodings adjacent to the recently modeled families (no new instruction and no statement change, so the banner is unchanged): the extended-register `imm3` values 5..7, the register-offset option values 000/001/100/101, the conditional-select bit-29 word, and the SBFM/BFM/UBFM `N != sf` words all stay opaque, and a cross-mask test pins one representative word per family so a mask added later cannot silently swallow a neighbouring family.
- [x] Extend the bounded ARM64 `semantic_preview` to the exclusive and acquire/release single-register loads and stores (LDXR/STXR/LDAXR/STLXR and the ordered LDAR/STLR), addressed by the base register alone, rendering loads as `dst = ldxr(base);`/`ldaxr`/`ldar`, an exclusive store as `status = stxr(base, src);`/`stlxr` (the success status is an unknown barrier), and the ordered STLR as `stlr(base, src);`, with the byte/halfword suffix named; the register pair forms (LDXP/STXP/LDAXP/STLXP) stay opaque, and cross-basic-block propagation remains open.
- [x] Extend the bounded ARM64 `semantic_preview` to the NOP-family system instructions as no-register-effect markers: the barriers DMB/DSB/ISB/CLREX (rendering their domain, e.g. `/* dmb ish */`), the modeled HINTs YIELD/WFE/WFI/SEV/SEVL/ESB/CSDB, and the exception-generating SVC/BRK/HLT (clearing tracked values because the trap may not return); an unallocated DMB/DSB option, an unmodeled HINT number (BTI, HINT #8) and the DCPS group stay opaque, and cross-basic-block propagation remains open.
- [x] Validate bounded executable-target pointer runs for jump-table triage; dispatch use and index bounds remain explicit unknowns.
- [x] Add metadata-guided automatic ARM/Thumb control-flow transitions; register-indirect mode changes remain explicit unresolved evidence.
- [x] Expand bounded UTF-16 Unicode string decoding with provenance-preserving output.
- [x] Add bounded IMG4/IM4P container inspection with compression-marker inventory; plaintext analysis remains explicit.
- [x] Improve bounded structural pseudocode with operand-preserving call, value-flow, memory-write, compare, and arithmetic sketches; full type/side-effect lifting remains open.
- [x] Add bounded non-ARM64 opcode-semantic labels to annotated disassembly; full lifting/decompilation remains open.
- [x] Distinguish bounded direct versus indirect calls and unconditional versus conditional/indirect control flow in non-ARM64 labels; operand recovery and full lifting remain open.

Acceptance: every new inference is provenance-labelled, bounded, and covered by synthetic and real-input regression tests.

### Phase 3 — Runtime investigation

- [x] Add explicit stop-event and stop-reason context to static-to-runtime pivots; richer breakpoint-condition context remains open.
- [x] Distinguish module/slide address correlation from validated static-function correlation in runtime explanations.
- [x] Add bounded `.vgcase` runtime trace capture/replay and restore the saved timeline into Trace Explorer without debugger attachment.
- [x] Add clearer host capability, permission, and entitlement diagnostics, including explicit architecture gates and honest deferred runtime-probe status.
- [x] Expand Intel and Apple Silicon qualification coverage for host gates and non-ARM64 semantic classification; full hardware release qualification remains open.

Acceptance: an analyst can connect a static finding to a reproducible runtime observation and include both in a handoff.

### Phase 4 — Product readiness

- [x] Surface the bundled synthetic ARM64 Mach-O tutorial fixture in Quick Start with a reproducible analyst walkthrough.
- [x] Add workspace-level help and unsupported-capability explanations.
- [x] Add bounded recent-project management with stale-path cleanup and persisted startup/layout preferences.
- [x] Add explicit compact, medium, and wide adaptive-layout modes with source-level regression coverage for the shared pane primitive.
- [ ] Complete signing, notarization, release-host, memory-pressure, and crash-diagnostics gates. Source-side identity, crash parser, startup-pressure, universal-slice, gate-coverage, strict Clippy, ordinary Rust tests, and the elevated universal Debug/Release candidate gate pass on this arm64 host; `scripts/qualify-release.sh` now automates the preflight, candidate/stable gate, report capture, and manual-matrix checklist. Developer ID/notary credentials and the manual real-target, memory-pressure, and crash-diagnostics qualification matrix remain required.

Acceptance: a clean install is understandable, testable, supportable, and distributable on supported macOS hosts.

### Phase 5 — Enterprise-grade trust and operations

- [x] Add a visible Support & Readiness center with build identity, Coordinator health, host capability status, and diagnostics actions.
- [x] Add structured operation/error taxonomy with recovery guidance and support-safe export.
- [x] Complete review decisions and conflict-safe handoff workflows; final decisions require rationale, exact baseline conflicts are explained/remediable, and case integrity manifests, bounded verification, and visible trust status are implemented.
- [x] Complete local policy and extension-governance surfaces; explicit capability allowlisting, managed policy modes, blocked policy status, and fail-closed execution/enablement are implemented.
- [x] Document managed-defaults namespace and fail-closed expectations, signed-release-manifest requirements, bounded retention, and the opt-in/manual crash-diagnostics boundary.

Acceptance: an enterprise operator can deploy, diagnose, govern, and hand off a local investigation without guessing which build, capability, policy, or evidence boundary is active.

### Phase 6 — Optional REA agent bridge

- [x] Define a local-first REA/MCP integration boundary and evidence-provenance requirements.
- [x] Add a coordinator-owned, bounded REA probe with explicit Settings opt-in.
- [x] Add explicit target-scoped REA overview, procedure, and name operations.
- [x] Add coordinator-owned cancellation for active REA operations.
- [x] Expose retained REA records as provenance nodes in the Evidence Graph.
- [x] Restore retained REA records during `.vgcase` import with hash validation.
- [x] Add bounded streamed progress history and retained evidence handles.
- [x] Gate REA operations on explicit provider capability reports when supplied.
- [x] Import REA evidence into HindSight cases and the evidence graph with compatibility checks.
- [x] Add deterministic MCP fixtures for handshake, capability gating, and progress retention.
- [x] Add deterministic release-preflight qualification coverage for the bridge; real-provider qualification remains a manual release-host gate.

Acceptance: an analyst can explicitly select a local target and REA operation, inspect bounded provider output, and retain the resulting evidence in a HindSight case without losing provenance or silently expanding scope.

## Work loop

For every loop:

1. Read this plan and inspect the current repository state.
2. Select the highest-priority incomplete item that can be completed safely in the current workload.
3. State the intended outcome and implementation boundary.
4. Implement the smallest complete slice.
5. Run focused tests, parser/type checks, and `git diff --check`.
6. Update this plan when an item is completed or its scope changes.
7. Commit only the completed slice and push `main`.
8. Confirm the remote and working tree state, then begin the next loop.

## Today's workload

The Phase 1 usability slices, bundled synthetic tutorial fixture and walkthrough, bounded ARM64 stack-slot variable-location candidates with operand-derived value-shape hints, bounded IMG4/IM4P container inventory, compact-unwind record ingestion, executable-target pointer-run validation, metadata-guided mixed ARM/Thumb decoding, bounded AArch64 call-register provenance, move-wide-immediate ARM64 semantic-preview constant tracking, flag-neutral ARM64 logical-operation semantic-preview tracking, PC-relative ADR/ADRP ARM64 semantic-preview address tracking, conditional/indirect ARM64 semantic-preview control-flow tracking with indirect-target resolution from proven constants, immediate-shift, multiply-add, sign/zero-extend, shifted-register-operand, bitfield-extract, conditional-select, and load/store ARM64 semantic-preview constant folding, Swift type-descriptor provenance labels, Swift field-variable boundaries, bounded non-ARM64 semantic labels with direct/indirect and conditional-control distinctions, bounded structural pseudocode sketches, explicit runtime correlation strength, explicit runtime stop-event/reason context, `.vgcase` runtime trace replay, host capability/permission/entitlement diagnostics, workspace-level help/unsupported-capability explanations, bounded recent-project management, persisted startup/layout preferences, architecture-focused qualification tests, startup-pressure diagnostics, shared compact/medium/wide layout coverage, cross-file Swift access-control validation, current bridge smoke expectations, optional release-argument handling, elevated universal Debug/Release candidate qualification, and non-secret release preflight reporting are complete. The remaining workload is the credentialed notarization path and the manual real-target, memory-pressure, and crash-diagnostics matrix.
