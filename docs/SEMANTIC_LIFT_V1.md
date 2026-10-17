# ARM64 instruction-derived semantics

The ARM64 worker now emits a separate `semantic_preview` analysis field. It is
computed from captured instruction words, not from rendered assembly text. Each
row begins with its unslid instruction address and includes the source word, so
the IR view can pivot to the exact disassembly evidence. Addressed rows also
enter the worker's typed evidence sideband and the synthesis fingerprint.

This slice models the move-wide immediate family (`MOVN`, `MOVZ`, `MOVK`),
flag-neutral `ADD`/`SUB` with immediate or register operands (including an
`LSL`/`LSR`/`ASR`-shifted register operand),
flag-neutral logical operations (`AND`/`ORR`/`EOR` bitmask immediates and
register `AND`/`ORR`/`EOR` plus the negated `BIC`/`ORN`/`EON`, also with an
`LSL`/`LSR`/`ASR`-shifted register operand),
immediate shifts (`LSL`/`LSR`/`ASR`), register-defined variable shifts
(`LSLV`/`LSRV`/`ASRV`/`RORV`), multiply-add (`MADD`/`MSUB`, with the
`MUL`/`MNEG` zero-addend aliases), integer division (`SDIV`/`UDIV`), widening
and high-half multiplies (`SMADDL`/`UMADDL`/`SMSUBL`/`UMSUBL` with their
`SMULL`/`UMULL` zero-addend aliases, and `SMULH`/`UMULH`), sign/zero extends
(`UXTB`/`UXTH`/`SXTB`/
`SXTH`/`SXTW`),
one-source bit/byte reversals and leading-bit counts
(`RBIT`/`REV16`/`REV`/`REV32`/`CLZ`/`CLS`),
concatenated field extract (`EXTR` with its same-register `ROR` immediate alias),
bitfield extracts (`UBFX`/`SBFX`), conditional select
(`CSEL`/`CSINC`/`CSINV`/`CSNEG` and their `CSET`/`CSETM`/`CINC`/`CINV`/`CNEG`
aliases), general-register loads and stores (`STR`/`LDR`
and their byte/halfword and sign-extending forms with a scaled unsigned
immediate offset or with register-offset `[base, index, extend/shift]`),
PC-relative `ADR`/`ADRP`, direct `B`/`BL`, conditional and indirect control flow
(`B.cond`, `CBZ`/`CBNZ`, `TBZ`/`TBNZ`, `BR`/`BLR`), `RET`, and `NOP`. It emits an
explicit opaque barrier for every other instruction and for `LC_DATA_IN_CODE`
ranges. Output is bounded to eight functions and 128 words per function. The
structural backend emits an empty field. The cache engine identity advances to
`arm64-analysis-v19-schema72` so reports from older workers are not reused as if
they contained semantic output. The move-wide, logical, shift, one-source, extract, multiply, extend,
bitfield, conditional-select, memory, address, and control-flow extensions stay
inside that `v19`/72-field contract:
they only refine instructions earlier builds marked opaque, so a cached report
from an older worker is conservatively incomplete rather than contradictory. The
literal-load correction below is the one exception: it removes a previously
emitted statement class, so the identity advances to reject the older,
contradictory reports. The
`AArch64 register semantics v24` banner identifies the statement set that
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
`ANDS`/`BICS`, and the `ROR` shift stay opaque. A register operand of a modeled
`ADD`/`SUB` or logical operation may carry an `LSL`/`LSR`/`ASR` immediate shift,
rendered parenthesized (for example `w0 = w1 + (w2 << 3);` or
`w0 = w1 & ~(w2 >> 4);`), with the shift applied before the enclosing operation.
Shifted-register `ADD`/`SUB` never use `ROR`; the logical forms allow it and it
stays opaque. The immediate shift
aliases of the bitfield group are modeled only in their canonical single-field
forms: `LSL` and `LSR` (from `UBFM`) and `ASR` (from `SBFM`), rendered with `<<`,
`>>`, and `>>>` (Verilog-style sign-propagating right shift). The general
bitfield extracts `UBFX` and `SBFX` render `dst = (src >> lsb) & mask;` and
`dst = sext(src >> lsb, width);` for the non-wrapping field `[lsb, lsb + width)`.
Bitfield inserts are now modeled: `BFI` renders `dst = (dst & ~(mask << lsb)) | ((src & mask) << lsb)`;
`BFXIL` renders `dst = (dst & ~mask) | ((src >> lsb) & mask)`;
`UBFIZ` renders `dst = src & mask` (zero-extended); `SBFIZ` renders `dst = sext(src & mask, width)`.
The wider `op31` multiplications stay opaque. The sign/zero extend aliases `UXTB`/`UXTH` (zero-extending) and
`SXTB`/`SXTH`/`SXTW` (sign-extending) are modeled as `dst = zext(src, N)` and
`dst = sext(src, N)`; they read the narrow source view, so a 64-bit destination
still names a `w` source. `MADD`
and `MSUB` with `op31==0` render `dst = left * right + add` and
`dst = add - left * right`; when the accumulator is the zero register the addend
is omitted as `dst = left * right` (`MUL`) or `dst = 0 - left * right` (`MNEG`),
and the wider `op31` variants (`SMADDL`/`UMADDL`/...) stay opaque. Integer
division `SDIV`/`UDIV` renders `dst = left sdiv right` / `udiv right`. ARM64
division never traps: a zero divisor yields zero, and the signed
`INT_MIN / -1` overflow saturates to `INT_MIN`, so a divided constant is always
defined and folds with its operand provenance. Register-defined shifts
(`LSLV`/`LSRV`/`ASRV`/`RORV`) from the same two-source group render
`dst = src << amount;`, `dst = src >> amount;`, `dst = src >>> amount;`, and
`dst = ror(src, amount);`, with the shift amount read from a whole register and
reduced modulo the register width (so a shift by the full width is a no-op);
results fold with their operand provenance, and the remaining opcodes of the
group stay opaque. The data-processing (1 source) family renders
`dst = rbit(src);`, `dst = rev16(src);`, `dst = rev(src);`,
`dst = rev32(src);`, `dst = clz(src);`, and `dst = cls(src);`, each computed at
the register width and folded with provenance (bit/byte reversal, leading-zero
count, and leading-sign count with the sign bit itself excluded). Within that
group the opcode-2 word is `REV32` for a 64-bit register but 32-bit `REV` for a
32-bit one, opcode-3 `REV` is 64-bit only, and the remaining opcodes (including
the register pointer-authentication subset) stay opaque. The concatenated-field
extract `EXTR` renders `dst = extr(left, right, lsb);`, taking the register-width
field at `lsb` from the pair `left:right`, and collapses to
`dst = ror(src, lsb);` when both sources are the same register (the `ROR`
immediate alias). It folds with provenance, and for a 32-bit register `lsb` is
bounded to 0..31 (a set bit 15 stays opaque); the encoding's `N == sf` and
`o0 == 0` requirements are enforced. The same `op31` group
also models
the widening and high-half multiplies. `SMULL`/`UMULL` render
`dst = sext(left, 32) * sext(right, 32);` and `dst = zext(left, 32) * zext(right, 32);`,
keeping the narrow 32-bit source view explicit while producing a 64-bit
destination; `SMADDL`/`UMADDL` and `SMSUBL`/`UMSUBL` render the same product with
a `+ accumulator` or `accumulator -` form. `SMULH`/`UMULH` read two 64-bit
sources and write the upper half of their product, rendered as
`dst = smulh(left, right);` / `dst = umulh(left, right);`. The widening and
high-half encodings are only allocated with `sf == 1`; `op31` 011/111 stay
opaque, and a zero-register destination discards the result. Conditional select
(`CSEL`/`CSINC`/`CSINV`/`CSNEG`) renders the true operand selected by the
condition against the false operand under its per-instruction transformation:
`dst = cond ? rn : rm;`, `dst = cond ? rn : (rm + 1);` for `CSINC`,
`dst = cond ? rn : (~rm);` for `CSINV`, and `dst = cond ? rn : (-rm);` for
`CSNEG`, with the `CSET`/`CSETM`/`CINC`/`CINV`/`CNEG` aliases sharing those
encodings (so `cset x10, eq` renders `x10 = ne ? xzr : (xzr + 1);`). The
condition code is named directly and PSTATE flags are not tracked, so a defined
value is claimed only for the always-true (`al`) condition, where the true
operand is selected unconditionally and its provenance is preserved; every other
condition (including `nv`) writes an unknown barrier rather than guessing the
flags. The flag-setting form (`S == 1`) and the reserved bit 11 stay opaque. The
complete general-register load/store family with a scaled unsigned immediate
offset is modeled: `STR`/`LDR` (size 2/3) plus the byte/halfword and
sign-extending forms
(`STRB`/`STRH`/`LDRB`/`LDRH`/`LDRSB`/`LDRSH`/`LDRSW`). The dereference is
untyped when the access width equals the data-register width, as in `x0 = *(x1);`
and `*(x1) = x0;`; otherwise the exact access width and signedness are named, as
in `w6 = *(u8 *)(x7);` and `x20 = *(s32 *)(x21);`. The
base is a 64-bit address register (`sp` when `Rn == 31`); a load defines its
destination as unknown while preserving the base, and a store leaves tracked
registers unchanged. When the base carries an exact constant, the resolved
address and its provenance are printed. The register-offset family
(`[base, index, extend/shift]` with the `UXTW`/`LSL`/`SXTW`/`SXTX` options and
the optional left shift by the access size) builds its index from the source
register (zero-extension for `UXTW`, sign-extension for `SXTW`, full width for
`LSL`/`SXTX`) and combines the proven base and proven index into one resolved
address. The unscaled and writeback forms are also modeled: `LDUR`/`STUR` with a
signed immediate (`base + off` or `base - abs(off)`, as in `*(x1 - 0x8)`),
post-index `[Rn], #imm` (access uses `base`, then `Rn = Rn + imm`, rendered as
`x0 = *(x1); x1 = x1 + 0x8;`), and pre-index `[Rn, #imm]!` (access uses
`Rn + imm` and `Rn` is updated first, rendered as `x0 = *(x1 + 0x8); x1 = x1 + 0x8;`).
The writeback propagates proven constants into the base register with its provenance
when available. PC-relative literal `LDR` (`opc` 0 for 32-bit and 1 for 64-bit)
and `LDRSW` (`opc` 2) are modeled with the address rendered as `pc + offset` (or
`pc - abs(offset)` for negative offsets) and the resolved target address with
provenance; the literal `PRFM` prefetch (`opc` 3) stays opaque. There is no
`LDRSB`/`LDRSH` literal form. The class requires bit 25 `0` (`011000`), so the
data-processing (1 source)/(2 source)/(3 source) and conditional-select groups
(`01101x`), which do not have a literal form, stay out of it. Prefetch
(`size 3`, opc 2), the
unallocated `opc 3` 32/64-bit forms, the unprivileged `LDTR`/`STTR` (mode 10),
and SIMD/scalar-FP stay opaque. `ADR`
writes the sum of
the instruction address and a signed 21-bit byte offset; `ADRP` writes the sum of
the page-aligned instruction address and a signed 21-bit page offset shifted left
twelve bits. Both are address constants in unslid space and carry the
instruction as provenance. `B.cond` names the condition code directly without
tracking PSTATE flags; `CBZ`/`CBNZ` test a register against zero and `TBZ`/`TBNZ`
test a single bit, both of which name the register operand. `BR` and `BLR` name
the register that holds an indirect target: when that register carries a proven
constant, the target address is resolved and its provenance is printed, otherwise
the register is named as an unresolved target. Every conditional and
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
preceding constant. A synthetic `MOVZ`+`BR` and `MOVZ`+`BLR` pair asserts that a
proven-constant indirect target is resolved to its address with provenance. A
synthetic `LSL`/`LSR`/`ASR` sequence asserts each shift operator, the exact shift
word, and sign-propagating arithmetic-shift constant folding. A synthetic
`MUL`/`MADD`/`MSUB` sequence asserts each rendered form, the exact multiply word,
and folded products and differences with their `proven_by` addresses. A synthetic
`SDIV`/`UDIV` sequence asserts each rendered form, the exact division words, folded
quotients with `proven_by` addresses, and that a zero divisor folds to zero. A
synthetic `LSLV`/`LSRV`/`ASRV`/`RORV` sequence asserts each rendered form, the
exact shift words verified against `clang -arch arm64`, folded results including
a shift amount that wraps modulo the register width and a sign-propagating
`ASRV`, and that an unallocated opcode of the group stays opaque. A synthetic
`RBIT`/`REV16`/`REV`/`REV32`/`CLZ`/`CLS` sequence asserts each rendered form and
its exact word, the 32-bit `REV`/`REV32` split, folded bit/byte reversals and
leading-bit counts (including a narrow count), and that an unallocated opcode of
the group stays opaque. A synthetic `EXTR`/`ROR`-immediate sequence asserts each
rendered form and its exact word, the same-register rotate collapse, folded
extracts (including an `lsb == 0` fold whose provenance omits the unused source)
and rotates, and that an `N != sf` word and a 32-bit word with `imms == 32` stay
opaque. A
synthetic `SMULL`/`UMULL`/`SMADDL`/`UMADDL`/`SMSUBL`/`UMSUBL`/`SMULH`/`UMULH`
sequence asserts each rendered form, the exact multiply words verified against
`clang -arch arm64`, folded products and quotients with `proven_by` addresses, and
that unallocated `op31` encodings and a zero-register destination stay opaque. A synthetic
conditional-select sequence asserts the `CSEL`/`CSINC`/`CSINV`/`CSNEG` renderings,
their `CSET`/`CSETM`/`CINC`/`CINV`/`CNEG` aliases, the exact words verified
against `clang -arch arm64`/`otool`, that the always-true condition selects the
true operand and folds its constant with `proven_by` addresses, that any other
condition is a barrier, and that a zero-register destination and the reserved
encoding bit stay opaque. A synthetic
`SXTB`/`UXTB`/`SXTW` sequence asserts the sign- and zero-extension operators, the
exact extend words, and width-correct folded values (including a 32-bit source
sign-extended into 64 bits). A synthetic shifted-register sequence asserts each
rendered `ADD`/`SUB`/logical shifted form, the exact words, the `ROR`-still-opaque
boundary, and folded shifted operands with their `proven_by` addresses. A
synthetic `UBFX`/`SBFX` sequence asserts the zero- and sign-extract operators
(including an elided `#0` shift), the exact extract words, folded extracts with
their `proven_by` addresses. A synthetic bitfield insert sequence asserts the
`BFI`, `BFXIL`, `UBFIZ`, and `SBFIZ` rendering, their exact words, and folded
inserts with `proven_by` addresses when the source is a known constant. A
synthetic `STR`/`LDR` sequence asserts the rendered store and
load forms, the scaled immediate offset and `sp` base, the resolved addresses
with their `proven_by` addresses, that a load invalidates its destination
register, and that unscaled/indexed forms remain opaque. A synthetic
`STRB`/`LDRB`/`LDRSW` sequence asserts the byte/halfword and sign-extending
width-qualified dereferences, the signed-versus-zero extension, and that
prefetch and the unallocated `opc 3` forms remain opaque. A synthetic
register-offset and writeback sequence asserts the combined base+index address
and provenance, a negative signed immediate, post-index writeback rendering and
provenance, and that `LDTR`/`STTR` and prefetch remain opaque. A synthetic
literal load sequence asserts the PC-relative address rendering, negative offsets,
the zero-extending and sign-extending literal forms (`LDR`/`LDRSW`), their
resolved target addresses with provenance, that the data-processing and
conditional-select words are no longer claimed as literal loads, and that `PRFM`
literal stays opaque.
