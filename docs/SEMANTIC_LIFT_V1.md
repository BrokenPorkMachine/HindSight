# ARM64 instruction-derived semantics

The ARM64 worker now emits a separate `semantic_preview` analysis field. It is
computed from captured instruction words, not from rendered assembly text. Each
row begins with its unslid instruction address and includes the source word, so
the IR view can pivot to the exact disassembly evidence. Addressed rows also
enter the worker's typed evidence sideband and the synthesis fingerprint.

This slice models the move-wide immediate family (`MOVN`, `MOVZ`, `MOVK`),
flag-neutral `ADD`/`SUB` with immediate operands, unshifted register `ADD`/`SUB`,
flag-neutral logical operations (`AND`/`ORR`/`EOR` bitmask immediates and
unshifted register `AND`/`ORR`/`EOR` plus the negated `BIC`/`ORN`/`EON`), direct
`B`/`BL`, `RET`, and `NOP`. It emits an explicit opaque barrier for every other
instruction and for `LC_DATA_IN_CODE` ranges. Output is bounded to eight
functions and 128 words per function. The structural backend emits an empty
field. The cache engine identity advances to `arm64-analysis-v18-schema72` so
reports from older workers are not reused as if they contained semantic output.
The move-wide and logical extensions stay inside that `v18`/72-field contract:
they only refine instructions earlier builds marked opaque, so a cached report
from an older worker is conservatively incomplete rather than contradictory. The
`AArch64 register semantics v4` banner identifies the statement set that
produced a preview.

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
`ANDS`/`BICS`, and shifted register operands stay opaque. A typed operation
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
constant, its `proven_by` addresses, and the exact logical words.
