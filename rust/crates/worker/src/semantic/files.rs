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
//! * a vector write is *partial*. A `D` form writes the low 64 bits of a
//!   128-bit register and preserves the upper half, so a consumer must not
//!   treat a narrow write as replacing the whole register.

use super::*;

/// The `N` flag bit. The flag file uses the same bit layout as
/// `condition_holds` and the `hs_flags_arith` helper the machine-state
/// emitter generates.
pub(super) const FLAG_N: u8 = 0b1000;
/// The `Z` flag bit.
pub(super) const FLAG_Z: u8 = 0b0100;
/// The `C` flag bit.
pub(super) const FLAG_C: u8 = 0b0010;
/// The `V` flag bit.
pub(super) const FLAG_V: u8 = 0b0001;
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

/// The vector registers one instruction writes and reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct VectorEffects {
    /// The registers written, with the width each write covers. A `Quad`
    /// write replaces the whole 128-bit register; a narrower write replaces
    /// only the named width and preserves the rest of the register.
    pub writes: Vec<(u8, IrSimdWidth)>,
    /// The registers read.
    pub reads: Vec<u8>,
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
    // A data-in-code range is not decoded, so no narrower claim is sound:
    // the instruction may write any flag.
    if instruction.data {
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
        // A floating-point compare writes every flag.
        IrOperation::ScalarFloatCompare { .. } => {
            effects.defines |= FLAG_ALL;
        }
        // `ADC`/`SBC` consume the carry and write no flag. The flag-setting
        // carry forms stay opaque, so no untracked carry is ever claimed.
        IrOperation::AddCarry { .. } => {
            effects.uses |= FLAG_C;
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

/// The vector registers an instruction writes and reads.
pub(super) fn instruction_vector_effects(instruction: &LiftedInstruction) -> VectorEffects {
    let mut effects = VectorEffects::default();
    // A data-in-code range is not decoded, so no narrower claim is sound.
    if instruction.data {
        for index in 0..32 {
            effects.writes.push((index, IrSimdWidth::Quad));
        }
        return effects;
    }
    let write = |effects: &mut VectorEffects, register: IrSimdRegister| {
        if !effects.writes.iter().any(|(index, _)| *index == register.index) {
            effects.writes.push((register.index, register.width));
        }
    };
    let read = |effects: &mut VectorEffects, register: IrSimdRegister| {
        if !effects.reads.contains(&register.index) {
            effects.reads.push(register.index);
        }
    };
    match instruction.portable {
        IrOperation::SimdLoad { destination, .. } => write(&mut effects, destination),
        IrOperation::SimdPairLoad { first, second, .. } => {
            write(&mut effects, first);
            write(&mut effects, second);
        }
        IrOperation::VectorBitwise { destination, left, right, .. }
        | IrOperation::VectorFloatBinary { destination, left, right, .. }
        | IrOperation::VectorHalfBinary { destination, left, right, .. }
        | IrOperation::VectorArithmetic { destination, left, right, .. } => {
            write(&mut effects, destination);
            read(&mut effects, left);
            read(&mut effects, right);
        }
        IrOperation::VectorFloatUnary { destination, source, .. } => {
            write(&mut effects, destination);
            read(&mut effects, source);
        }
        IrOperation::VectorImmediate { destination, .. } => {
            // `MOVI` writes the whole register.
            write(&mut effects, IrSimdRegister { index: destination, width: IrSimdWidth::Quad });
        }
        IrOperation::VectorExtract { source, .. } => {
            // A lane extract reads the vector register and writes a
            // general-purpose register.
            read(&mut effects, IrSimdRegister { index: source, width: IrSimdWidth::Quad });
        }
        // The scalar floating-point forms write the low part of the vector
        // register and preserve the rest of it.
        IrOperation::ScalarFloatImmediate { destination, precision, .. } => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(precision) });
        }
        IrOperation::ScalarFloatMove { destination, source, precision } => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: source, width: float_width(precision) });
        }
        IrOperation::ScalarFloatBinary { destination, left, right, precision, .. } => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: left, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: right, width: float_width(precision) });
        }
        IrOperation::ScalarFloatUnary { destination, source, precision, .. } => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: source, width: float_width(precision) });
        }
        IrOperation::ScalarFloatMultiplyAdd(IrFloatMultiplyAdd { destination, left, right, addend, precision, .. }) => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: left, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: right, width: float_width(precision) });
            read(&mut effects, IrSimdRegister { index: addend, width: float_width(precision) });
        }
        IrOperation::ScalarFloatConvert { destination, destination_precision, source, source_precision } => {
            write(&mut effects, IrSimdRegister { index: destination, width: float_width(destination_precision) });
            read(&mut effects, IrSimdRegister { index: source, width: float_width(source_precision) });
        }
        _ => {}
    }
    effects.writes.sort_by_key(|(index, _)| *index);
    effects.reads.sort_unstable();
    effects
}

/// The vector-register width a scalar floating-point precision covers.
fn float_width(precision: IrFloatPrecision) -> IrSimdWidth {
    match precision {
        IrFloatPrecision::Half => IrSimdWidth::Half,
        IrFloatPrecision::Single => IrSimdWidth::Single,
        IrFloatPrecision::Double => IrSimdWidth::Double,
    }
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

/// The vector-register width as it appears in a published row.
pub(super) fn vector_width_name(width: IrSimdWidth) -> &'static str {
    match width {
        IrSimdWidth::Byte => "b",
        IrSimdWidth::Half => "h",
        IrSimdWidth::Single => "s",
        IrSimdWidth::Double => "d",
        IrSimdWidth::Quad => "q",
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
    fn a_vector_binary_operation_writes_its_destination_and_reads_its_sources() {
        // orr v0.16b, v1.16b, v2.16b = 0x4e221c20
        let effects = instruction_vector_effects(&instruction(0x4ea2_1c20));
        assert_eq!(effects.writes, vec![(0, IrSimdWidth::Quad)], "{effects:?}");
        assert_eq!(effects.reads, vec![1, 2], "{effects:?}");
    }

    #[test]
    fn a_narrow_vector_write_preserves_the_rest_of_the_register() {
        // The `D` form writes only the low 64 bits of the register, so the
        // published width must say so rather than claim the whole register.
        let mut instruction = instruction(0x4ea2_1c20);
        instruction.portable = IrOperation::VectorBitwise {
            destination: IrSimdRegister { index: 3, width: IrSimdWidth::Double },
            left: IrSimdRegister { index: 4, width: IrSimdWidth::Double },
            right: IrSimdRegister { index: 5, width: IrSimdWidth::Double },
            operator: IrVectorBitwiseOperator::Or,
        };
        let effects = instruction_vector_effects(&instruction);
        assert_eq!(effects.writes, vec![(3, IrSimdWidth::Double)], "{effects:?}");
        assert_eq!(effects.reads, vec![4, 5], "{effects:?}");
    }

    #[test]
    fn a_scalar_float_operation_writes_only_the_narrow_part_of_the_register() {
        // fadd d0, d1, d2 = 0x1e622820
        let effects = instruction_vector_effects(&instruction(0x1e62_2820));
        assert_eq!(effects.writes, vec![(0, IrSimdWidth::Double)], "{effects:?}");
        assert_eq!(effects.reads, vec![1, 2], "{effects:?}");
    }

    #[test]
    fn a_lane_extract_reads_the_vector_register() {
        // umov x0, v1.d[1] = 0x4e183c20
        let effects = instruction_vector_effects(&instruction(0x4e18_3c20));
        assert_eq!(effects.writes, Vec::new(), "{effects:?}");
        assert_eq!(effects.reads, vec![1], "{effects:?}");
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
        // A vector write names the width it covers, so a consumer can tell a
        // whole-register write from a narrow one.
        assert!(report.contains("// ssa-vector 0x100c writes=[v0:q] reads=[v1,v2]"), "{report}");
        assert!(report.contains("// ssa-vector 0x1010 writes=[v0:d] reads=[v1,v2]"), "{report}");
    }
}
