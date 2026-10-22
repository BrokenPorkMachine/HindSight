//! Exact effects for the two register files the GPR preview does not cover:
//! the NZCV flags and the vector file.
//!
//! The shared IR carries the GPR effects directly, but the flag-setting forms
//! and the vector widths are not part of it. Both are recovered here from the
//! decoded operation and the original encoding, never from rendered text.
//!
//! Two properties matter for a consumer:
//!
//! * a flag write is *partial*. `ANDS` writes `N` and `Z` and leaves `C` and
//!   `V` alone, so a consumer must not discard the carry it did not write.
//! * narrow scalar and Q=0 SIMD results zero-extend into the whole 128-bit
//!   register. Partial vector definitions are still needed for ABI call
//!   clobbers: `v8`-`v15` preserve their bottom 64 bits.

use super::*;

/// The position of the `N` flag bit inside the flag mask.
pub(super) const FLAG_BIT_N: u8 = 3;
/// The position of the `Z` flag bit inside the flag mask.
pub(super) const FLAG_BIT_Z: u8 = 2;
/// The position of the `C` flag bit inside the flag mask.
pub(super) const FLAG_BIT_C: u8 = 1;
/// The position of the `V` flag bit inside the flag mask.
pub(super) const FLAG_BIT_V: u8 = 0;
/// The `N` flag bit. The flag file uses the same bit layout as
/// `condition_holds` and the `hs_flags_arith` helper the machine-state
/// emitter generates.
pub(super) const FLAG_N: u8 = 1 << FLAG_BIT_N;
/// The `Z` flag bit.
pub(super) const FLAG_Z: u8 = 1 << FLAG_BIT_Z;
/// The `C` flag bit.
pub(super) const FLAG_C: u8 = 1 << FLAG_BIT_C;
/// The `V` flag bit.
pub(super) const FLAG_V: u8 = 1 << FLAG_BIT_V;
/// Every flag bit an instruction can write.
pub(super) const FLAG_ALL: u8 = FLAG_N | FLAG_Z | FLAG_C | FLAG_V;

/// The flags one instruction writes and reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FlagEffects {
    /// The flag bits the instruction writes. A bit that is absent is
    /// preserved, not invalidated.
    pub defines: u8,
    /// The flag bits the instruction reads.
    pub uses: u8,
}

impl FlagEffects {
    /// Whether the instruction touches the flag file at all.
    pub fn touches(self) -> bool {
        self.defines != 0 || self.uses != 0
    }
}

/// One byte range of a vector register that an instruction reads or writes.
///
/// A vector access range names the bytes whose value identity is read or
/// defined. Narrow scalar/Q=0 writes define all 16 bytes because the upper
/// bits are zero; ABI clobbers can still define only a subrange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VectorRange {
    /// The vector register.
    pub register: u8,
    /// The first byte the access covers inside the register.
    pub offset: u8,
    /// The number of bytes the access covers.
    pub bytes: u8,
}

impl VectorRange {
    /// A range covering the low `bytes` of a register.
    pub(super) fn low(register: u8, bytes: u8) -> Self {
        Self { register, offset: 0, bytes }
    }

    /// The range a scalar floating-point precision covers.
    pub(super) fn float(register: u8, precision: IrFloatPrecision) -> Self {
        Self::low(register, match precision {
            IrFloatPrecision::Half => 2,
            IrFloatPrecision::Single => 4,
            IrFloatPrecision::Double => 8,
        })
    }

    /// The published name of the range.
    pub(super) fn name(&self) -> String {
        format!("v{}[{}..{})", self.register, self.offset, self.offset + self.bytes)
    }
}

/// The vector registers one instruction writes and reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct VectorEffects {
    /// The byte ranges written. A narrow write preserves the rest of the
    /// register, so the range names the bytes the write really covers.
    pub writes: Vec<VectorRange>,
    /// The byte ranges read.
    pub reads: Vec<VectorRange>,
}

impl VectorEffects {
    /// Whether the instruction touches the vector file at all.
    pub fn touches(&self) -> bool {
        !self.writes.is_empty() || !self.reads.is_empty()
    }
}

/// The flag bits a condition predicate reads.
pub(super) fn predicate_flags(condition: IrFlagPredicate) -> u8 {
    match condition {
        IrFlagPredicate::Equal | IrFlagPredicate::NotEqual => FLAG_Z,
        IrFlagPredicate::CarrySet | IrFlagPredicate::CarryClear => FLAG_C,
        IrFlagPredicate::Negative | IrFlagPredicate::NonNegative => FLAG_N,
        IrFlagPredicate::Overflow | IrFlagPredicate::NoOverflow => FLAG_V,
        IrFlagPredicate::UnsignedHigher | IrFlagPredicate::UnsignedLowerOrSame => FLAG_C | FLAG_Z,
        IrFlagPredicate::SignedGreaterOrEqual
        | IrFlagPredicate::SignedLess
        | IrFlagPredicate::SignedGreater
        | IrFlagPredicate::SignedLessOrEqual => FLAG_N | FLAG_V | FLAG_Z,
        // An always-true or always-false predicate reads no flag.
        IrFlagPredicate::Always | IrFlagPredicate::Never => 0,
    }
}

/// The flag bits an encoded condition code reads.
///
/// The codes are the architectural condition codes in the order
/// `condition_name` names them, so the mapping matches `condition_holds`.
pub(super) fn condition_code_flags(code: u8) -> u8 {
    match code & 15 {
        0 | 1 => FLAG_Z,           // eq, ne
        2 | 3 => FLAG_C,           // hs, lo
        4 | 5 => FLAG_N,           // mi, pl
        6 | 7 => FLAG_V,           // vs, vc
        8 | 9 => FLAG_C | FLAG_Z,  // hi, ls
        10 | 11 => FLAG_N | FLAG_V, // ge, lt
        12 | 13 => FLAG_N | FLAG_V | FLAG_Z, // gt, le
        _ => 0,                    // al, nv
    }
}

/// The flag bits an instruction writes and reads.
///
/// The flag-setting writes come from the original encoding, because the
/// shared operation models a flag-setting form exactly like its flag-neutral
/// counterpart. The `S` bit of the add/subtract group and the `opc` field of
/// the logical group are the architectural source of the write, so they are
/// read from the encoding rather than re-parsed from rendered text.
pub(super) fn instruction_flag_effects(instruction: &LiftedInstruction) -> FlagEffects {
    let mut effects = FlagEffects::default();
    // Data-in-code and target-specific operations are not decoded into an
    // architectural effect set, so no narrower flag claim is sound. Preserve
    // the prior identities as inputs and invalidate every result.
    if instruction.data || matches!(instruction.portable, IrOperation::TargetSpecific) {
        effects.defines = FLAG_ALL;
        effects.uses = FLAG_ALL;
        return effects;
    }
    // `ADDS`/`SUBS` write every flag. The `S` bit is bit 29 of the
    // add/subtract group, and the decoder keeps the form when the
    // destination is a real register.
    match instruction.operation {
        Operation::AddImmediate { .. } | Operation::AddRegister { .. }
            if instruction.word & (1 << 29) != 0 =>
        {
            effects.defines |= FLAG_ALL;
        }
        // `ANDS`/`BICS` write `N` and `Z` only. `C` and `V` are preserved.
        // The logical group carries the operation in bits 30 and 29, and
        // `opc == 0b11` is the flag-setting form.
        Operation::LogicalImmediate { .. } | Operation::LogicalRegister { .. }
            if instruction.word & 0x6000_0000 == 0x6000_0000 =>
        {
            effects.defines |= FLAG_N | FLAG_Z;
        }
        _ => {}
    }
    match instruction.portable {
        // `CMP`/`CMN` write every flag. `TST` is `ANDS` with a zero
        // destination, so it writes `N` and `Z` only.
        IrOperation::Compare { kind, .. } => {
            effects.defines |= match kind {
                IrCompareKind::Test => FLAG_N | FLAG_Z,
                IrCompareKind::Compare | IrCompareKind::CompareNegative => FLAG_ALL,
            };
        }
        // A conditional compare writes every flag, and reads the flags its
        // condition names.
        IrOperation::ConditionalCompare { condition, .. } => {
            effects.defines |= FLAG_ALL;
            effects.uses |= condition_code_flags(condition);
        }
        // `MRS Xt, NZCV` consumes all four condition bits to produce Xt.
        IrOperation::ReadNZCV { .. } => {
            effects.uses |= FLAG_ALL;
        }
        // `MSR NZCV, Xt` replaces all four PSTATE condition bits. The source
        // register read is carried separately by the GPR effects.
        IrOperation::WriteNZCV { .. } => {
            effects.defines |= FLAG_ALL;
        }
        // A floating-point compare writes every flag.
        IrOperation::ScalarFloatCompare { .. } => {
            effects.defines |= FLAG_ALL;
        }
        // `ADC`/`SBC` consume C. `ADCS`/`SBCS` additionally define all NZCV
        // bits, and their carry input is an explicit flag-file use.
        IrOperation::AddCarry { set_flags, .. } => {
            effects.uses |= FLAG_C;
            if set_flags { effects.defines |= FLAG_ALL; }
        }
        // A conditional select reads the flags its predicate names.
        IrOperation::ConditionalSelect { condition, .. }
        | IrOperation::ScalarFloatSelect { condition, .. } => {
            effects.uses |= predicate_flags(condition);
        }
        // A conditional branch reads the flags its condition names.
        IrOperation::ConditionalBranch { condition: IrCondition::Flags(condition), .. } => {
            effects.uses |= predicate_flags(condition);
        }
        // A call does not preserve the flags. AAPCS64 makes the condition
        // flags caller-saved, so the values after the call are undefined
        // whatever the callee did.
        IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. } => {
            effects.defines |= FLAG_ALL;
        }
        // An unmodeled operation may write any flag. It is a barrier for the
        // whole flag file rather than a narrow claim.
        IrOperation::TargetSpecific => {
            effects.defines |= FLAG_ALL;
            effects.uses |= FLAG_ALL;
        }
        _ => {}
    }
    effects
}

/// The vector ranges a call may clobber.
///
/// AAPCS64 makes `v0`-`v7` and `v16`-`v31` caller-saved, so the callee may
/// change them completely. `v8`-`v15` are callee-saved, but only their bottom
/// 64 bits are preserved, so the callee may change the top 64 bits.
pub(super) fn call_vector_clobbers() -> Vec<VectorRange> {
    (0..32u8).map(|register| {
        if (8..=15).contains(&register) {
            VectorRange { register, offset: 8, bytes: 8 }
        } else {
            VectorRange::low(register, 16)
        }
    }).collect()
}

/// The vector registers an instruction writes and reads.
pub(super) fn instruction_vector_effects(instruction: &LiftedInstruction) -> VectorEffects {
    let mut effects = VectorEffects::default();
    // Data-in-code and target-specific operations may read or write any
    // vector register. Treat them as whole-file barriers; otherwise a later
    // use could incorrectly retain a pre-instruction identity.
    if instruction.data || matches!(instruction.portable, IrOperation::TargetSpecific) {
        for register in 0..32 {
            effects.reads.push(VectorRange::low(register, 16));
            effects.writes.push(VectorRange::low(register, 16));
        }
        return effects;
    }
    let write = |effects: &mut VectorEffects, range: VectorRange| {
        if !effects.writes.contains(&range) { effects.writes.push(range); }
    };
    let read = |effects: &mut VectorEffects, range: VectorRange| {
        if !effects.reads.contains(&range) { effects.reads.push(range); }
    };
    match instruction.portable {
        IrOperation::SimdLoad { destination, .. } => {
            write(&mut effects, VectorRange::low(destination.index, 16));
        }
        IrOperation::SimdPairLoad { first, second, .. } => {
            write(&mut effects, VectorRange::low(first.index, 16));
            write(&mut effects, VectorRange::low(second.index, 16));
        }
        // A store reads the value it stores.
        IrOperation::SimdStore { source, .. } => {
            read(&mut effects, VectorRange::low(source.index, source.width.bytes()));
        }
        IrOperation::SimdPairStore { first, second, .. } => {
            read(&mut effects, VectorRange::low(first.index, first.width.bytes()));
            read(&mut effects, VectorRange::low(second.index, second.width.bytes()));
        }
        IrOperation::VectorBitwise { destination, left, right, operator } => {
            write(&mut effects, VectorRange::low(destination.index, 16));
            read(&mut effects, VectorRange::low(left.index, left.width.bytes()));
            read(&mut effects, VectorRange::low(right.index, right.width.bytes()));
            // The bit-insert forms also read the destination: `BSL`, `BIT` and
            // `BIF` select bits from the previous destination value, so the
            // destination is an input as well as an output.
            if matches!(operator, IrVectorBitwiseOperator::MaskSelect
                | IrVectorBitwiseOperator::BitInsert
                | IrVectorBitwiseOperator::BitInsertIfFalse) {
                read(&mut effects, VectorRange::low(destination.index, destination.width.bytes()));
            }
        }
        IrOperation::VectorFloatBinary { destination, left, right, .. }
        | IrOperation::VectorHalfBinary { destination, left, right, .. }
        => {
            write(&mut effects, VectorRange::low(destination.index, 16));
            read(&mut effects, VectorRange::low(left.index, left.width.bytes()));
            read(&mut effects, VectorRange::low(right.index, right.width.bytes()));
        }
        IrOperation::VectorArithmetic { destination, left, right, operator, .. } => {
            write(&mut effects, VectorRange::low(destination.index, 16));
            read(&mut effects, VectorRange::low(left.index, left.width.bytes()));
            read(&mut effects, VectorRange::low(right.index, right.width.bytes()));
            if matches!(operator, IrVectorArithmeticOperator::MultiplyAccumulate | IrVectorArithmeticOperator::MultiplySubtract) {
                read(&mut effects, VectorRange::low(destination.index, destination.width.bytes()));
            }
        }
        IrOperation::VectorFloatUnary { destination, source, .. } => {
            write(&mut effects, VectorRange::low(destination.index, 16));
            read(&mut effects, VectorRange::low(source.index, source.width.bytes()));
        }
        IrOperation::VectorImmediate { destination, .. } => {
            // `MOVI` writes the whole register.
            write(&mut effects, VectorRange::low(destination, 16));
        }
        IrOperation::VectorExtract { source, lane, bits, .. } => {
            // Lane extraction depends only on the selected lane. Keep its
            // byte-level provenance exact instead of making unrelated lanes
            // appear to contribute to the GPR result. If an invalid internal
            // form reaches this boundary, conservatively depend on the whole
            // register instead of publishing an out-of-bounds range.
            let bytes = bits / 8;
            let offset = lane.saturating_mul(bytes);
            if matches!(bits, 8 | 16 | 32 | 64) && offset <= 16 - bytes {
                read(&mut effects, VectorRange { register: source, offset, bytes });
            } else {
                read(&mut effects, VectorRange::low(source, 16));
            }
        }
        // A scalar floating-point instruction writes the whole destination
        // register and leaves the bits above its lane zero. The measurement
        // that settles this is recorded in
        // docs/AARCH64_SCALAR_FP_EFFECTS.md: the write is not a partial write
        // that preserves the rest of the register, and it does not take the
        // upper bits from another operand or from the old destination.
        IrOperation::ScalarFloatImmediate { destination, .. } => {
            write(&mut effects, VectorRange::low(destination, 16));
        }
        IrOperation::ScalarFloatMove { destination, source, precision } => {
            // FMOV (register) copies one lane into a zero-filled register.
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(source, precision));
        }
        IrOperation::ScalarFloatBinary { destination, left, right, precision, .. } => {
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(left, precision));
            read(&mut effects, VectorRange::float(right, precision));
        }
        IrOperation::ScalarFloatUnary { destination, source, precision, .. } => {
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(source, precision));
        }
        IrOperation::ScalarFloatMultiplyAdd(IrFloatMultiplyAdd { destination, left, right, addend, precision, .. }) => {
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(left, precision));
            read(&mut effects, VectorRange::float(right, precision));
            read(&mut effects, VectorRange::float(addend, precision));
        }
        IrOperation::ScalarFloatConvert { destination, source, source_precision, .. } => {
            // A conversion writes the whole destination and reads only the
            // source lane. It does not read the old destination.
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(source, source_precision));
        }
        // A floating-point comparison reads both operands and writes no
        // vector register.
        IrOperation::ScalarFloatCompare { left, right, precision } => {
            read(&mut effects, VectorRange::float(left, precision));
            if let Some(right) = right {
                read(&mut effects, VectorRange::float(right, precision));
            }
        }
        // A floating-point conditional select reads both candidates and
        // writes the whole destination register.
        IrOperation::ScalarFloatSelect { destination, if_true, if_false, precision, .. } => {
            write(&mut effects, VectorRange::low(destination, 16));
            read(&mut effects, VectorRange::float(if_true, precision));
            read(&mut effects, VectorRange::float(if_false, precision));
        }
        // A conversion between an integer and a floating-point register reads
        // the floating-point side and writes the whole destination register.
        IrOperation::ScalarIntFloatConvert { destination, source, .. } => {
            if let IrConversionRegister::Float { index, precision } = source {
                read(&mut effects, VectorRange::float(index, precision));
            }
            if let IrConversionRegister::Float { index, .. } = destination {
                write(&mut effects, VectorRange::low(index, 16));
            }
        }
        // A call does not preserve the caller-saved vector registers.
        IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. } => {
            effects.writes.extend(call_vector_clobbers());
        }
        _ => {}
    }
    effects.writes = normalize_vector_ranges(effects.writes);
    effects.reads = normalize_vector_ranges(effects.reads);
    effects
}

/// Sort and coalesce overlapping or adjacent ranges within a register.
/// Coalesce overlapping or adjacent byte ranges before they become SSA
/// records, keeping each register-file use/definition unique and bounded.
fn normalize_vector_ranges(mut ranges: Vec<VectorRange>) -> Vec<VectorRange> {
    ranges.sort_by_key(|range| (range.register, range.offset));
    let mut normalized: Vec<VectorRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = normalized.last_mut() {
            let previous_end = previous.offset + previous.bytes;
            let range_end = range.offset + range.bytes;
            if previous.register == range.register && range.offset <= previous_end {
                previous.bytes = previous_end.max(range_end) - previous.offset;
                continue;
            }
        }
        normalized.push(range);
    }
    normalized
}

/// The flag bits a definition covers, in architectural order.
pub(super) fn flag_names(bits: u8) -> Vec<&'static str> {
    let mut names = Vec::new();
    if bits & FLAG_N != 0 { names.push("n"); }
    if bits & FLAG_Z != 0 { names.push("z"); }
    if bits & FLAG_C != 0 { names.push("c"); }
    if bits & FLAG_V != 0 { names.push("v"); }
    names
}

/// The name of one flag bit, addressed by its position in the flag mask.
pub(super) fn flag_bit_name(bit: u8) -> &'static str {
    match bit {
        FLAG_BIT_N => "n",
        FLAG_BIT_Z => "z",
        FLAG_BIT_C => "c",
        FLAG_BIT_V => "v",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(word: u32) -> LiftedInstruction {
        let operation = decode(word, 0x1000);
        let portable = lift_operation(operation, 0x1000);
        LiftedInstruction { address: 0x1000, word, operation, portable, data: false }
    }

    #[test]
    fn flag_setting_arithmetic_writes_every_flag() {
        // adds x0, x1, #1 = 0xf1000440
        let effects = instruction_flag_effects(&instruction(0xb100_0420));
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
        // subs x0, x1, #1 = 0xf1000400
        let effects = instruction_flag_effects(&instruction(0xf100_0420));
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
    }

    #[test]
    fn flag_setting_logical_writes_only_negative_and_zero() {
        // ands x0, x1, #0xff = 0x9240_0c00
        let effects = instruction_flag_effects(&instruction(0xf240_1c20));
        assert_eq!(effects.defines, FLAG_N | FLAG_Z, "{effects:?}");
        // The carry and overflow are preserved, so they are not invalidated.
        assert_eq!(effects.defines & FLAG_C, 0, "{effects:?}");
        assert_eq!(effects.defines & FLAG_V, 0, "{effects:?}");
    }

    #[test]
    fn a_compare_writes_every_flag_and_a_test_writes_only_two() {
        // cmp x0, #1 = 0xf100001f
        let effects = instruction_flag_effects(&instruction(0xf100_041f));
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
        // tst x0, #1 = 0xb400001f
        let effects = instruction_flag_effects(&instruction(0xf240_001f));
        assert_eq!(effects.defines, FLAG_N | FLAG_Z, "{effects:?}");
    }

    #[test]
    fn a_carry_consumes_the_carry_flag_without_writing_it() {
        // adc x0, x1, x2 = 0x9a020020
        let effects = instruction_flag_effects(&instruction(0x9a02_0020));
        assert_eq!(effects.uses, FLAG_C, "{effects:?}");
        assert_eq!(effects.defines, 0, "{effects:?}");
    }

    #[test]
    fn flag_setting_carry_consumes_c_and_defines_all_nzcv_bits() {
        // adcs x0, x1, x2 = 0xba020020; sbcs x0, x1, x2 = 0xfa020020
        for word in [0xba02_0020, 0xfa02_0020] {
            let effects = instruction_flag_effects(&instruction(word));
            assert_eq!(effects.uses, FLAG_C, "{effects:?}");
            assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
        }
    }

    #[test]
    fn a_conditional_select_reads_the_flags_its_predicate_names() {
        // csel x0, x1, x2, eq = 0x9a821020
        let effects = instruction_flag_effects(&instruction(0x9a82_0020));
        assert_eq!(effects.uses, FLAG_Z, "{effects:?}");
        assert_eq!(effects.defines, 0, "{effects:?}");
        // csel x0, x1, x2, hi = 0x9a8c_1020 reads the carry and the zero flag.
        let effects = instruction_flag_effects(&instruction(0x9a82_8020));
        assert_eq!(effects.uses, FLAG_C | FLAG_Z, "{effects:?}");
    }

    #[test]
    fn a_conditional_compare_writes_every_flag_and_reads_its_condition() {
        // ccmp x4, x5, #0x0, eq = 0xfa450080
        let effects = instruction_flag_effects(&instruction(0xfa45_0080));
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
        assert_eq!(effects.uses, FLAG_Z, "{effects:?}");
    }

    #[test]
    fn an_opaque_instruction_claims_every_flag() {
        let mut instruction = instruction(0xffff_ffff);
        instruction.portable = IrOperation::TargetSpecific;
        let effects = instruction_flag_effects(&instruction);
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
    }

    #[test]
    fn msr_nzcv_defines_all_flags_and_reads_its_gpr_source() {
        // clang-verified: msr NZCV, x0
        let instruction = instruction(0xd51b_4200);
        assert!(matches!(instruction.portable, IrOperation::WriteNZCV { .. }));
        assert_eq!(instruction_flag_effects(&instruction), FlagEffects { defines: FLAG_ALL, uses: 0 });
        assert_eq!(instruction.portable.gpr_effects().uses, 1);
    }

    #[test]
    fn mrs_nzcv_reads_all_flags_and_defines_only_its_gpr_destination() {
        // clang-verified: mrs x0, NZCV
        let instruction = instruction(0xd53b_4200);
        assert!(matches!(instruction.portable, IrOperation::ReadNZCV { .. }));
        assert_eq!(instruction_flag_effects(&instruction), FlagEffects { defines: 0, uses: FLAG_ALL });
        let effects = instruction.portable.gpr_effects();
        assert_eq!(effects.uses, 0);
        assert_eq!(effects.definitions, 1);
        assert!(!effects.barrier);
    }

    #[test]
    fn a_reserved_vector_fp_encoding_invalidates_all_register_file_identities() {
        // FRECPE with Q=0 and 64-bit lanes would form the reserved .1d view.
        let instruction = instruction(0x0ee1_d820);
        assert!(matches!(instruction.operation, Operation::Opaque));
        let gpr = instruction.portable.gpr_effects();
        assert!(gpr.barrier);
        assert_eq!(gpr.definitions, u32::MAX);
        let flags = instruction_flag_effects(&instruction);
        assert_eq!(flags.defines, FLAG_ALL);
        assert_eq!(flags.uses, FLAG_ALL);
        let vectors = instruction_vector_effects(&instruction);
        assert_eq!(vectors.writes, (0..32).map(|register| VectorRange::low(register, 16)).collect::<Vec<_>>());
        assert_eq!(vectors.reads, vectors.writes);
    }

    #[test]
    fn a_call_does_not_preserve_the_flags() {
        // bl #0 = 0x94000000
        let effects = instruction_flag_effects(&instruction(0x9400_0000));
        assert_eq!(effects.defines, FLAG_ALL, "{effects:?}");
        assert_eq!(effects.uses, 0, "{effects:?}");
    }

    #[test]
    fn a_vector_binary_operation_writes_its_destination_and_reads_its_sources() {
        // orr v0.16b, v1.16b, v2.16b = 0x4e221c20
        let effects = instruction_vector_effects(&instruction(0x4ea2_1c20));
        assert_eq!(effects.writes, vec![VectorRange::low(0, 16)], "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange::low(1, 16), VectorRange::low(2, 16)], "{effects:?}");
    }

    #[test]
    fn vector_multiply_accumulate_reads_the_prior_destination_version() {
        // `mla v0.8b, v1.8b, v2.8b` and its `mls` neighbor both consume V0.
        for word in [0x0e22_9420, 0x2e22_9420] {
            let effects = instruction_vector_effects(&instruction(word));
            assert!(effects.writes.contains(&VectorRange::low(0, 16)), "{effects:?}");
            assert!(effects.reads.contains(&VectorRange::low(0, 8)), "accumulator is an input: {effects:?}");
            assert!(effects.reads.contains(&VectorRange::low(1, 8)), "{effects:?}");
            assert!(effects.reads.contains(&VectorRange::low(2, 8)), "{effects:?}");
        }
    }

    #[test]
    fn vector_fp_effects_preserve_gpr_and_nzcv_and_keep_q0_zero_extension() {
        // `fadd v0.2s, v0.2s, v1.2s` (Q=0) and its Q=1 `.4s` counterpart.
        // A read-modify-write destination remains an input, and Q selects
        // the exact source-byte range while the architectural write is 128b.
        for (word, source_bytes) in [(0x0e21_d400, 8), (0x4e21_d400, 16), (0x4e61_d400, 16)] {
            let binary = instruction(word);
            assert!(matches!(binary.portable, IrOperation::VectorFloatBinary { .. }));
            assert_eq!(instruction_vector_effects(&binary), VectorEffects {
                writes: vec![VectorRange::low(0, 16)],
                reads: vec![VectorRange::low(0, source_bytes), VectorRange::low(1, source_bytes)],
            });
            assert_eq!(binary.portable.gpr_effects().uses, 0);
            assert_eq!(instruction_flag_effects(&binary), FlagEffects::default());
        }

        // fadd v0.4h, v1.4h, v2.4h (Q=0, 16-bit lanes).
        let half = instruction(0x0e42_1420);
        assert!(matches!(half.portable, IrOperation::VectorHalfBinary { element_bits: 16, .. }));
        assert_eq!(instruction_vector_effects(&half), VectorEffects {
            writes: vec![VectorRange::low(0, 16)],
            reads: vec![VectorRange::low(1, 8), VectorRange::low(2, 8)],
        });
        let gpr = half.portable.gpr_effects();
        assert_eq!((gpr.uses, gpr.definitions, gpr.barrier), (0, 0, false));
        assert_eq!(instruction_flag_effects(&half), FlagEffects::default());

        // The Q=1 `.8h` form reads all 128 source bits while preserving the
        // same full-register destination write.
        let half_quad = instruction(0x4e42_1420);
        assert!(matches!(half_quad.portable, IrOperation::VectorHalfBinary { element_bits: 16, .. }));
        assert_eq!(instruction_vector_effects(&half_quad), VectorEffects {
            writes: vec![VectorRange::low(0, 16)],
            reads: vec![VectorRange::low(1, 16), VectorRange::low(2, 16)],
        });

        // frecpe v3.2d, v4.2d (Q=1, 64-bit lanes).
        let double = instruction(0x4ee1_d883);
        assert!(matches!(double.portable, IrOperation::VectorFloatUnary { element_bits: 64, .. }));
        assert_eq!(instruction_vector_effects(&double), VectorEffects {
            writes: vec![VectorRange::low(3, 16)],
            reads: vec![VectorRange::low(4, 16)],
        });
        let gpr = double.portable.gpr_effects();
        assert_eq!((gpr.uses, gpr.definitions, gpr.barrier), (0, 0, false));
        assert_eq!(instruction_flag_effects(&double), FlagEffects::default());

        // frecpe v5.2s, v6.2s (Q=0, 32-bit lanes).
        let single = instruction(0x0ea1_d8c5);
        assert!(matches!(single.portable, IrOperation::VectorFloatUnary { element_bits: 32, .. }));
        assert_eq!(instruction_vector_effects(&single), VectorEffects {
            writes: vec![VectorRange::low(5, 16)],
            reads: vec![VectorRange::low(6, 8)],
        });
        let gpr = single.portable.gpr_effects();
        assert_eq!((gpr.uses, gpr.definitions, gpr.barrier), (0, 0, false));
        assert_eq!(instruction_flag_effects(&single), FlagEffects::default());
    }

    #[test]
    fn a_64_bit_vector_write_zeros_the_rest_of_the_register() {
        // Q=0 vector forms write 64 result bits and zero bits 64..128.
        // The source dependencies remain limited to the low 64 bits.
        let mut instruction = instruction(0x4ea2_1c20);
        instruction.portable = IrOperation::VectorBitwise {
            destination: IrSimdRegister { index: 3, width: IrSimdWidth::Double },
            left: IrSimdRegister { index: 4, width: IrSimdWidth::Double },
            right: IrSimdRegister { index: 5, width: IrSimdWidth::Double },
            operator: IrVectorBitwiseOperator::Or,
        };
        let effects = instruction_vector_effects(&instruction);
        assert_eq!(effects.writes, vec![VectorRange::low(3, 16)], "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange::low(4, 8), VectorRange::low(5, 8)], "{effects:?}");
    }

    #[test]
    fn narrow_simd_loads_define_zero_extended_full_registers() {
        let address = IrMemoryAddress::BaseOffset {
            base: IrRegister { class: IrRegisterClass::General, index: 1, bits: 64 },
            offset: 0,
        };
        let mut single = instruction(0xfd00_0000);
        single.portable = IrOperation::SimdLoad {
            destination: IrSimdRegister { index: 3, width: IrSimdWidth::Double },
            address,
            writeback: None,
        };
        assert_eq!(instruction_vector_effects(&single).writes, vec![VectorRange::low(3, 16)]);

        let mut pair = instruction(0xad00_0000);
        pair.portable = IrOperation::SimdPairLoad {
            first: IrSimdRegister { index: 4, width: IrSimdWidth::Single },
            second: IrSimdRegister { index: 5, width: IrSimdWidth::Single },
            address,
            bytes: 8,
            writeback: None,
        };
        assert_eq!(instruction_vector_effects(&pair).writes,
                   vec![VectorRange::low(4, 16), VectorRange::low(5, 16)]);
    }

    #[test]
    fn the_bit_insert_forms_also_read_their_destination() {
        // bit v0.16b, v1.16b, v2.16b = 0x6ea21c20
        let effects = instruction_vector_effects(&instruction(0x6ea2_1c20));
        assert_eq!(effects.writes, vec![VectorRange::low(0, 16)], "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange::low(0, 16), VectorRange::low(1, 16), VectorRange::low(2, 16)], "{effects:?}");
    }

    #[test]
    fn scalar_float_arithmetic_writes_the_whole_destination_register() {
        // fadd d0, d1, d2 = 0x1e622820
        // The measurement in docs/AARCH64_SCALAR_FP_EFFECTS.md shows that a
        // scalar floating-point write covers the whole 128-bit destination and
        // leaves the bits above the lane zero. The write is therefore not a
        // partial write that preserves the rest of the register, and no
        // operand other than the two source lanes contributes.
        let effects = instruction_vector_effects(&instruction(0x1e62_2820));
        assert_eq!(effects.writes, vec![VectorRange::low(0, 16)], "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange::float(1, IrFloatPrecision::Double),
                                       VectorRange::float(2, IrFloatPrecision::Double)], "{effects:?}");
    }

    #[test]
    fn a_lane_extract_reads_only_the_selected_lane() {
        // umov x0, v1.d[0] = 0x4e083c20
        let effects = instruction_vector_effects(&instruction(0x4e08_3c20));
        assert_eq!(effects.writes, Vec::new(), "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange { register: 1, offset: 0, bytes: 8 }], "{effects:?}");
        // umov x0, v1.d[1] = 0x4e183c20
        // The second 64-bit lane is the upper half of the register, so the
        // read covers bytes 8..16 and no other lane contributes.
        let effects = instruction_vector_effects(&instruction(0x4e18_3c20));
        assert_eq!(effects.writes, Vec::new(), "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange { register: 1, offset: 8, bytes: 8 }], "{effects:?}");
    }

    #[test]
    fn a_store_reads_the_vector_value_it_stores() {
        // str d0, [x0] = 0xfd000000
        let effects = instruction_vector_effects(&instruction(0xfd00_0000));
        assert_eq!(effects.writes, Vec::new(), "{effects:?}");
        assert_eq!(effects.reads, vec![VectorRange::low(0, 8)], "{effects:?}");
    }

    #[test]
    fn a_call_clobbers_the_caller_saved_vector_registers() {
        // bl #0 = 0x94000000
        let effects = instruction_vector_effects(&instruction(0x9400_0000));
        // `v0`-`v7` and `v16`-`v31` are caller-saved and change completely.
        assert_eq!(effects.writes[0], VectorRange::low(0, 16), "{effects:?}");
        assert_eq!(effects.writes[7], VectorRange::low(7, 16), "{effects:?}");
        assert_eq!(effects.writes[8], VectorRange { register: 8, offset: 8, bytes: 8 }, "{effects:?}");
        // `v8`-`v15` keep their bottom 64 bits, so only the top half changes.
        assert_eq!(effects.writes[15], VectorRange { register: 15, offset: 8, bytes: 8 }, "{effects:?}");
        assert_eq!(effects.writes[16], VectorRange::low(16, 16), "{effects:?}");
        assert_eq!(effects.writes.len(), 32, "{effects:?}");
    }

    #[test]
    fn the_preview_publishes_the_flag_and_vector_effects() {
        // adds x0, x1, #1 ; ands x0, x1, #0xff ; csel x0, x1, x2, hi ;
        // orr v0.16b, v1.16b, v2.16b ; fadd d0, d1, d2 ; ret
        let words = [
            0xb100_0420u32, 0xf240_1c20, 0x9a82_8020, 0x4ea2_1c20, 0x1e62_2820, 0xd65f_03c0,
        ];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        // `ADDS` writes every flag.
        assert!(report.contains("// ssa-flags 0x1000 defines=[n,z,c,v] uses=[]"), "{report}");
        // `ANDS` writes only `N` and `Z`, so the carry and overflow survive.
        assert!(report.contains("// ssa-flags 0x1004 defines=[n,z] uses=[]"), "{report}");
        // `CSEL` reads the carry and zero flags its predicate names. The
        // published order is the architectural one, so it is `z` then `c`.
        assert!(report.contains("// ssa-flags 0x1008 defines=[] uses=[z,c]"), "{report}");
        // A vector write names the byte range it covers, so a consumer can
        // tell a whole-register write from a narrow one.
        assert!(report.contains("// ssa-vector 0x100c writes=[v0[0..16)] reads=[v1[0..16),v2[0..16)]"), "{report}");
        // `FADD D0, D1, D2` writes the whole destination register and leaves
        // the bits above the lane zero, so the write is not a narrow one.
        assert!(report.contains("// ssa-vector 0x1010 writes=[v0[0..16)] reads=[v1[0..8),v2[0..8)]"), "{report}");
    }
}
