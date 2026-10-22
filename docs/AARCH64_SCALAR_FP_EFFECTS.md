# AArch64 scalar floating-point file effects

A scalar floating-point instruction writes a SIMD&FP destination register. The
question this note answers is what happens to the bits of that destination
above the scalar lane: are they preserved, do they come from another
operand, or are they written as a known value?

The answer decides whether a reconstruction can claim a *known* upper half or
must treat it as unknown, and it decides which operands a reconstruction may
claim a dependency on.

## The competing models

| Model | Claim | Consequence for a reconstruction |
| --- | --- | --- |
| Preserve | Only the scalar lane is written; the rest of the register survives. | The upper half keeps the identity it had, and the instruction depends on the old destination. |
| Merge | The whole register is written, and the upper half takes bits from the first source operand, or from the old destination for a conversion. | The upper half is a *different* value that depends on another operand. |
| Zero | The whole register is written and the upper half is zero. | The upper half is a known constant, and the instruction depends on no other operand. |

The distinction is not cosmetic: a merge claim is a false statement about the
machine, and it invents a dependency edge that no differential test against
native execution can ever justify.

## The measurement

`scripts/probe-scalar-fp-effects.sh` runs `scripts/probe-scalar-fp-effects.c`
on the host. Every case loads a destination whose 128 bits are a known
pattern, loads a first source whose upper half carries a *different* known
pattern, executes one instruction, and stores the destination back. The
printed classification says where the upper half came from:

* `preserved` — the destination's own upper bits survived;
* `zeroed` — the write cleared the upper bits;
* `merged-from-source` — the upper bits came from the first source operand;
* `other` — the upper bits are a computed vector result.

The sources carry zero in their low half, so every operation is well defined
and any merge is visible.

Result on an Apple Silicon (ARMv8.5-A) host:

```
fadd d0, d1, d2                zeroed
fsub d0, d1, d2                zeroed
fmul d0, d1, d2                zeroed
fdiv d0, d1, d2                zeroed
fmadd d0, d1, d2, d3           zeroed
fmsub d0, d1, d2, d3           zeroed
fnmadd d0, d1, d2, d3          zeroed
fabs d0, d1                    zeroed
fneg d0, d1                    zeroed
fsqrt d0, d1                   zeroed
frinta d0, d1                  zeroed
fcsel d0, d1, d2, eq           zeroed
fmov d0, d1                    zeroed
fcvt d0, s1                    zeroed
fcvt s0, d1                    zeroed
scvtf d0, x3                   zeroed
ucvtf d0, x3                   zeroed
fmov d0, x3                    zeroed
fadd s0, s1, s2                zeroed
fcvt h0, s1                    zeroed
fadd h0, h1, h2                zeroed
fmov h0, h1                    zeroed
fmov d0, #1.0                  zeroed
fmov s0, #1.0                  zeroed
fmov h0, #1.0                  zeroed
fadd v0.2s, v1.2s, v2.2s       zeroed      (64-bit vector form)
fadd v0.2d, v1.2d, v2.2d       computed    (128-bit vector form)
fadd v0.4s, v1.4s, v2.4s       computed    (128-bit vector form)
```

## The supported model

**Zero.** A scalar floating-point instruction writes the whole 128-bit
destination register, and the bits above the scalar lane are zero. It does not
read the old destination and it does not read another operand for those bits.

Consequences for the reconstruction:

* The write covers the whole register, so a consumer may treat the
  destination's upper half as a known zero rather than as a preserved or
  merged value.
* The instruction has no dependency on the previous destination value and no
  dependency on the first source's upper half. A reconstruction that keeps
  those dependencies is claiming a value the machine never produced.
* A `D` form zeroes bits 64..128; an `S` form zeroes bits 32..128; an `H`
  form zeroes bits 16..128. The lane width of the *destination* decides the
  zeroing boundary, not the width of the source.
* A 64-bit vector form such as `fadd v0.2s` behaves the same way: it writes
  the low 64 bits and zeroes the upper 64. A 128-bit vector form computes
  every lane and keeps its result in all 128 bits.

## Scope and limits

* The measurement covers the destination's upper bits for the listed forms.
  It does not qualify FPCR rounding modes, flush-to-zero, the alternate
  half-precision mode, exception behaviour, or the NaN propagation rules.
* The measurement is a property of the machine that produced it. A different
  implementation may differ, so the claim is scoped to the host that ran the
  probe, and a native differential test remains the only way to qualify a
  wider set of forms.
* The probe needs an AArch64 host; the script reports a skip elsewhere.
