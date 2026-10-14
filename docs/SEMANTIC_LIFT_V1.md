# ARM64 instruction-derived semantics

The ARM64 worker now emits a separate `semantic_preview` analysis field. It is
computed from captured instruction words, not from rendered assembly text. Each
row begins with its unslid instruction address and includes the source word, so
the IR view can pivot to the exact disassembly evidence. Addressed rows also
enter the worker's typed evidence sideband and the synthesis fingerprint.

This slice models the move-wide immediate family (`MOVN`, `MOVZ`, `MOVK`),
flag-neutral `ADD`/`SUB` with immediate operands, unshifted register `ADD`/`SUB`,
flag-neutral logical operations (`AND`/`ORR`/`EOR` bitmask immediates and
unshifted register `AND`/`ORR`/`EOR` plus the negated `BIC`/`ORN`/`EON`),
PC-relative `ADR`/`ADRP`, direct `B`/`BL`, conditional and indirect control flow
(`B.cond`, `CBZ`/`CBNZ`, `TBZ`/`TBNZ`, `BR`/`BLR`), `RET`, and `NOP`. It emits an
explicit opaque barrier for every other instruction and for `LC_DATA_IN_CODE`
ranges. Output is bounded to eight functions and 128 words per function. The
structural backend emits an empty field. The cache engine identity advances to
`arm64-analysis-v18-schema72` so reports from older workers are not reused as if
they contained semantic output. The move-wide, logical, address, and control-flow
extensions stay inside that `v18`/72-field contract: they only refine
instructions earlier builds marked opaque, so a cached report from an older worker
is conservatively incomplete rather than contradictory. The `AArch64 register
semantics v6` banner identifies the statement set that produced a preview.

Statements name machine registers. `RET` labels x0 only as an ABI result-register
candidate because the return type is unknown. `BL` does not infer argument
count, call signature, result, or side effects. `MOVN` defines the bitwise
complement of its shifted immediate. `MOVK` merges sixteen bits into the prior
value of the destination register: a constant is claimed only when that prior
value is already known, otherwise only the bitfield insert is rendered and the
register becomes unknown (no constant is inferred). Logical operations render a
bitmask immediate or a register operand, with a leading `~` when the negated
`BIC`/`ORN`/`EON` form complements the right operand. The immediate is the
architectural `DecodeBitMasks` `wmask`; reserved bitmask encodings, flag-setting
`ANDS`/`BICS`, and shifted register operands stay opaque. `ADR` writes the sum of
the instruction address and a signed 21-bit byte offset; `ADRP` writes the sum of
the page-aligned instruction address and a signed 21-bit page offset shifted left
twelve bits. Both are address constants in unslid space and carry the
instruction as provenance. `B.cond` names the condition code directly without
tracking PSTATE flags; `CBZ`/`CBNZ` test a register against zero and `TBZ`/`TBNZ`
test a single bit, both of which name the register operand. `BR` and `BLR` name
the register that holds an unresolved indirect target. Every conditional and
indirect transfer clears tracked constants like any other control-flow barrier. A
typed operation
representation tracks exact integer constants through supported straight-line
instructions and records the contributing instruction addresses. Opaque
instructions, calls, branches, returns, data ranges, and function boundaries
clear that knowledge. Tracking stops if provenance would exceed eight
instruction addresses. The preview
does not yet propagate expressions across basic blocks, reconstruct source variables,
recover Objective-C sends or Swift calls, or model ARM64e pointer authentication.
It is the testable instruction-to-semantics foundation for those passes, not a
general source decompiler.

Verification uses exact encoding unit tests and the bundled ARM64 Mach-O fixture
through the worker wire protocol. The fixture asserts a known `x0 = x0 + x1`
statement, its instruction word and address, plus an opaque barrier. A synthetic
`MOVZ`+`MOVK` pair additionally asserts the merged constant, its
`proven_by` instruction addresses, and the exact `MOVK` word. A synthetic
`AND`-immediate plus `EOR`-register sequence asserts a bitmask operand, the folded
constant, its `proven_by` addresses, and the exact logical words. A synthetic
`ADRP`+`ADD` sequence asserts the materialized address, the folded pointer
constant, its `proven_by` addresses, and the exact `ADRP` word. A synthetic
`B.cond`/`CBZ`/`TBZ`/`BLR`/`BR` sequence asserts each rendered condition, the
named indirect targets, the exact branch word, and that the transfers clear a
preceding constant.
