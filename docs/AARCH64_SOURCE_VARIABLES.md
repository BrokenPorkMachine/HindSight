# AArch64 source-parameter identities

The pseudocode renderer consumes a bounded, shared-IR parameter-identity
analysis in `rust/crates/worker/src/semantic/names.rs`. Assignment strings,
comment capitalization and rendered branch targets no longer decide whether
a source parameter survives. Text remains a presentation layer; general typed
AST reconstruction and canonical memory/FP/flags SSA remain open.

## Implemented scope

- Supported ABI declarations seed parameter identity and source width.
  Untyped GP candidates select their register view from entry-reaching IR
  reads; a later use after replacement cannot retroactively type the input.
- Copy chains retain entry identity rather than growing display strings.
  Distinct copies of the same entry value can agree at a CFG join; conflicts,
  killed paths and entry/backedge disagreements retain machine registers and
  produce bounded join-alternative notes.
- `EOR` with the architectural zero register is an exact copy for any decoded
  LSL/LSR/ASR shift of that zero operand, with no negation. A real XOR does not
  retain either operand's identity; width checks still prevent a W-register
  truncation from masquerading as the original 64-bit source value.
- `BIC Rd,Rn,XZR` is also an exact copy because its complement operand is all
  ones, even when the zero operand has a decoded shift. BIC with any general
  register remains a value transform for parameter naming.
- Register `ADD` with XZR on either side and `SUB` with XZR on the right are
  exact copies, including shifted-zero operands. `0 - Rn` remains a value
  transform and does not inherit the source identity.
- `ORR Rn,XZR` retains `Rn` even when the zero operand is shifted. Shifts on
  the nonzero operand remain value transforms and drop the identity.
- Decoded bitfield-shift aliases with a zero distance retain the source
  identity; nonzero shifts do not. General extracts and extensions remain
  value transforms even when only some bits happen to match.
- Decoded `CSEL` and `FCSEL` preserve an identity only when both alternatives
  resolve to the same incoming parameter at a compatible width. Distinct GP or
  FP inputs remain unaliased; `CSINC`/`CSINV`/`CSNEG` do not inherit identity
  because they transform the selected value. Native fixtures exercise the
  agreeing GP/FP cases, with negative unit tests for conflicting inputs and
  transforming GP selects.
- W/X and S/D views are not interchangeable source values. Narrowing a known
  64-bit parameter loses its name; a write to Wd zero-extends into Xd and
  invalidates the old 64-bit identity. Scalar conversions and SIMD lane
  extracts are not source-variable copies. Both destinations of paired writes
  matter.
- Modeled SP/frame adjustments and exact non-overlapping spills/restores can
  retain parameter identities, including GP/FP pairs and pre/post writeback.
  Saved values require compatible bank and sufficient access width. Partial
  overlapping writes, possible aliases, calls and unknown effects invalidate
  affected stack-save state. Across AAPCS64 calls, only callee-saved GP x19-x29
  and the low 64 bits of v8-v15 retain parameter identities; caller-saved
  registers, x18 and LR are cleared. This ABI rule does not imply a proven
  callee memory-effect summary or general alias/memory SSA.
- Per-instruction input and output names are separate. An overwritten register
  does not keep its former parameter name on the assignment's left side.
  Presentation substitution advances names after each emitted assignment,
  preserves comments/quoted text/callable names, and handles writeback order.
- Scalar pre-indexed accesses use the updated base without applying the offset
  twice. An `LDP` whose first destination overwrites its addressing base
  captures both accesses' original effective address in an instruction-scoped
  temporary before either load.
- Memory partition provenance now survives proven 64-bit constant add/subtract
  from a global or frame pointer. A 32-bit pointer view, dynamic index,
  pointer-plus-pointer expression, or unsupported arithmetic clears the
  provenance rather than narrowing an access to a possibly wrong partition.

Analysis requires complete aligned function bytes, at most 128 instructions,
32 seeds and 64 saved identity slots. The identity fixed point has a 32,768-step
budget; frame offsets are bounded by the existing ±1 MiB frame model. Unknown
indirect successors, unavailable/truncated bytes and exhausted analysis budgets
withhold substitutions with an explicit reason. Join annotations have a 4 KiB
per-function budget and explicit truncation. Data-in-code acts as an unknown
effect, not an instruction carrying an identity.

## Qualification

```sh
cargo test --manifest-path rust/Cargo.toml -p reworkbench-worker
python3 scripts/test-arm64-nameflow.py rust/target/debug/reworker
python3 scripts/run-decompilation-corpus.py --worker rust/target/debug/reworker
```

Unit tests exercise raw instruction words and structured effects, including
widths, copies, joins, loops, unreachable predecessors, spills, alias/overlap
invalidation, paired writes, writeback, data-in-code and budgets. The previous
text-only naming fixtures now exercise the production bytecode analysis.

Seventeen CC0 assembly functions are built by Apple Clang. Hash-bound
declarations from their header supply legitimate name/type evidence. The
integration gate checks actual recovered pseudocode and, on Apple Silicon,
4,420 original-native result checks over copy/frame/pair/FP/pre-index,
overlapping-base, zero-register and callee-saved-call fixtures. Intel macOS
cross-assembles and checks pseudocode without claiming execution.
These are naming/effect checks, not differential execution of reconstructed
source: typed AST export and original/IR/generated equivalence are still open.
