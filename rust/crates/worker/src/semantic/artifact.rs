//! Review AST and derivation graph produced directly from bounded shared IR.
use super::*;
use crate::reconstruction::{address, array, object, ranges, Builder, Input, Value as V};
use std::{collections::{BTreeMap, BTreeSet}, io};
mod calls;
mod constraints;
mod executable;
mod source;

/// Floating-point arithmetic can consult FPCR and accumulate exceptions in
/// FPSR. Those system-register effects are not yet represented in machine
/// state SSA, so they remain explicit blockers even when the data result has
/// a supported shared-IR operation.
fn has_unmodeled_fp_status_effect(operation: IrOperation) -> bool {
    matches!(operation,
        IrOperation::ScalarFloatBinary { .. }
        | IrOperation::ScalarFloatMultiplyAdd(_)
        | IrOperation::ScalarFloatCompare { .. }
        | IrOperation::ScalarFloatConvert { .. }
        | IrOperation::ScalarIntFloatConvert { .. }
        | IrOperation::VectorFloatBinary { .. }
        | IrOperation::VectorFloatUnary { .. }
        | IrOperation::VectorHalfBinary { .. }
    ) || matches!(operation,
        IrOperation::ScalarFloatUnary {
            operator: IrFloatUnaryOperator::Sqrt
                | IrFloatUnaryOperator::Round
                | IrFloatUnaryOperator::ReciprocalEstimate
                | IrFloatUnaryOperator::ReciprocalSqrtEstimate
                | IrFloatUnaryOperator::ReciprocalExponent,
            ..
        })
}

#[cfg(test)]
mod fp_status_tests {
    use super::*;

    #[test]
    fn mixed_width_integer_float_conversions_block_strict_export() {
        for word in [0x1e62_0020, 0x9e22_0020, 0x1e63_0020, 0x9e23_0020,
                     0x1e78_0020, 0x9e38_0020, 0x1e79_0020, 0x9e39_0020,
                     0x1e42_ec20, 0x9e02_ec20, 0x1e43_ec20, 0x9e03_ec20,
                     0x1e58_ec20, 0x9e18_ec20, 0x1e59_ec20, 0x9e19_ec20,
                     0x9e38_003f, 0x9e18_003f] { // discarded GPR results still affect FP status
            let operation = decode(word, 0x1000);
            let portable = lift_operation(operation, 0x1000);
            assert!(matches!(portable, IrOperation::ScalarIntFloatConvert { .. }), "{word:#010x}");
            assert!(has_unmodeled_fp_status_effect(portable), "{word:#010x}");
        }
        let plain = model(lift_operation(decode(0x1e78_0020, 0x1000), 0x1000)).unwrap();
        assert_eq!(plain.render(), "w0 = fcvtzs(d1); /* FP status effects unresolved */");
        let discarded = model(lift_operation(decode(0x9e18_ec3f, 0x1000), 0x1000)).unwrap();
        assert_eq!(discarded.render(), "discard(fcvtzs(s1, fractional_bits=5)); /* FP status effects unresolved */");
    }

    #[test]
    fn resolved_indirect_goto_renders_targets_and_case_map() {
        let dispatch = tree(
            "register",
            Some("uint64_t".into()),
            object([("name", "x9".into()), ("bits", 64u64.into()), ("class", "general".into())]),
            vec![],
        );
        let node = tree(
            "indirect-goto",
            None,
            object([
                ("dispatch", "x9".into()),
                ("resolved_targets_complete", true.into()),
                ("targets", array([address(0x1004), address(0x1008)])),
                ("case_map", array([
                    object([("case", 0u64.into()), ("target", address(0x1004))]),
                    object([("case", 2u64.into()), ("target", address(0x1008))]),
                ])),
                ("index_info", "table_index=x8 case-range=CMP/B.HS x8<4".into()),
            ]),
            vec![dispatch],
        );
        let rendered = node.render();
        assert!(rendered.contains("indirect_goto(x9"), "{rendered}");
        assert!(rendered.contains("targets=[0x1004, 0x1008]") || rendered.contains("targets=[0x0000000000001004, 0x0000000000001008]"), "{rendered}");
        assert!(rendered.contains("0x0->0x1004") || rendered.contains("0x0->0x0000000000001004"), "{rendered}");
        assert!(rendered.contains("0x2->0x1008") || rendered.contains("0x2->0x0000000000001008"), "{rendered}");
        assert!(rendered.contains("table_index=x8"), "{rendered}");
        assert!(rendered.contains("resolved complete"), "{rendered}");
    }
}

pub(crate) use constraints::{Constraint, ConstraintKind, Solution as ConstraintSolution};

pub(crate) use constraints::{report as constraint_report, solve as solve_constraints};

/// The AAPCS64 register classes published by the call-effect summary. The
/// report and the reconstruction artifact share one definition so a text
/// summary never claims a different clobber set than the structured payload.
pub(crate) use calls::{
    CALLEE_SAVED_GPR, CALLEE_SAVED_VECTOR, CALLER_SAVED_GPR, CALLER_SAVED_VECTOR,
    PRESERVED_SPECIAL,
};

/// Source-tree vocabulary v1. Attributes carry explicit bit/memory contracts;
/// these are machine-state expressions, not fabricated recovered source locals.
#[derive(Clone)]
struct Tree {
    kind: &'static str,
    ty: Option<String>,
    attributes: V,
    children: Vec<Tree>,
}
fn tree(kind: &'static str, ty: Option<String>, attributes: V, children: Vec<Tree>) -> Tree {
    Tree {
        kind,
        ty,
        attributes,
        children,
    }
}
fn integer_type(bits: u8) -> String {
    format!("uint{bits}_t")
}
fn reg(register: IrRegister) -> Tree {
    tree(
        "register",
        Some(integer_type(register.bits)),
        object([
            ("name", register.target().name().into()),
            ("bits", register.bits.into()),
            (
                "class",
                match register.class {
                    IrRegisterClass::General => "general",
                    IrRegisterClass::StackPointer => "stack-pointer",
                    IrRegisterClass::Zero => "zero",
                    IrRegisterClass::ProgramCounter => "program-counter",
                }
                .into(),
            ),
        ]),
        vec![],
    )
}
fn vector(register: IrSimdRegister) -> Tree {
    tree(
        "register",
        Some(
            match register.width {
                IrSimdWidth::Single => "float",
                IrSimdWidth::Double => "double",
                _ => "hs_vector_bits",
            }
            .into(),
        ),
        object([
            ("name", simd_register_name(register).into()),
            ("bits", (u64::from(register.width.bytes()) * 8).into()),
            ("class", "simd-fp".into()),
        ]),
        vec![],
    )
}
fn constant(value: u64, bits: u8) -> Tree {
    tree(
        "constant",
        Some(integer_type(bits)),
        object([("hex", address(value)), ("bits", bits.into())]),
        vec![],
    )
}
fn operand(value: IrOperand, bits: u8) -> Tree {
    match value {
        IrOperand::Register(register) => reg(register),
        IrOperand::Constant(value) => constant(value, bits),
    }
}
fn unary(operator: &str, value: Tree, bits: u8) -> Tree {
    tree(
        "unary",
        Some(integer_type(bits)),
        object([("operator", operator.into()), ("bits", bits.into())]),
        vec![value],
    )
}
fn binary(operator: &str, left: Tree, right: Tree, bits: u8) -> Tree {
    tree(
        "binary",
        Some(integer_type(bits)),
        object([
            ("operator", operator.into()),
            ("bits", bits.into()),
            ("arithmetic", "modular-bitvector".into()),
        ]),
        vec![left, right],
    )
}
fn assign(destination: IrRegister, value: Tree) -> Tree {
    tree(
        "assignment",
        None,
        object([(
            "write_policy",
            if destination.class == IrRegisterClass::Zero {
                "discard"
            } else if destination.bits == 32 {
                "zero-extend-to-physical-64"
            } else {
                "replace-physical-64"
            }
            .into(),
        )]),
        vec![reg(destination), value],
    )
}
fn address_tree(value: IrMemoryAddress) -> Tree {
    match value {
        IrMemoryAddress::BaseOffset { base, offset } => {
            if offset == 0 {
                reg(base)
            } else {
                binary("+", reg(base), constant(offset as u64, 64), 64)
            }
        }
        IrMemoryAddress::PcRelative { target, .. } => constant(target, 64),
        IrMemoryAddress::BaseIndex {
            base,
            index,
            extension,
            scale,
        } => {
            let index = match extension {
                Some(IrExtension::Sign(bits)) => tree(
                    "extend",
                    Some("uint64_t".into()),
                    object([("signed", true.into()), ("from_bits", bits.into())]),
                    vec![reg(index)],
                ),
                Some(IrExtension::Zero(bits)) => tree(
                    "extend",
                    Some("uint64_t".into()),
                    object([("signed", false.into()), ("from_bits", bits.into())]),
                    vec![reg(index)],
                ),
                None => reg(index),
            };
            binary(
                "+",
                reg(base),
                if scale == 0 {
                    index
                } else {
                    binary("<<", index, constant(u64::from(scale), 64), 64)
                },
                64,
            )
        }
    }
}
fn memory(address: Tree, bytes: u8, signed: bool, bits: u8) -> Tree {
    tree(
        "memory-load",
        Some(integer_type(bits)),
        object([
            ("bytes", bytes.into()),
            ("signed", signed.into()),
            ("result_bits", bits.into()),
            ("address_space", "guest".into()),
            ("byte_order", "little-endian".into()),
            ("fault_model", "unqualified".into()),
        ]),
        vec![address],
    )
}
fn store(address: Tree, value: Tree, bytes: u8) -> Tree {
    tree(
        "memory-store",
        None,
        object([
            ("bytes", bytes.into()),
            ("address_space", "guest".into()),
            ("byte_order", "little-endian".into()),
            ("fault_model", "unqualified".into()),
        ]),
        vec![address, value],
    )
}
fn sequence(children: Vec<Tree>) -> Tree {
    tree("sequence", None, object([]), children)
}
fn writeback(update: IrWriteback) -> Tree {
    assign(
        update.base,
        binary(
            "+",
            reg(update.base),
            constant(update.offset as u64, 64),
            64,
        ),
    )
}
fn ordered_access(access: Tree, update: Option<IrWriteback>) -> Tree {
    match update {
        Some(update) if update.pre_index => sequence(vec![writeback(update), access]),
        Some(update) => sequence(vec![access, writeback(update)]),
        None => access,
    }
}
fn effective_address(address: IrMemoryAddress, update: Option<IrWriteback>) -> Tree {
    update
        .filter(|update| update.pre_index)
        .map(|update| reg(update.base))
        .unwrap_or_else(|| address_tree(address))
}

/// Instruction budget for the bounded callee observation at a direct call
/// site. The window is an observation, not a whole-function proof.
const CALLEE_WINDOW: usize = 128;

fn model(operation: IrOperation) -> Option<Tree> {
    let overlaps = |base: IrRegister, value: IrRegister| {
        base.class == IrRegisterClass::General && base.index == value.index
    };
    // Constrained-unpredictable writeback aliases have no single architectural
    // result to export. Non-writeback pairs may alias the base and are handled
    // by the evaluate-address-once node below.
    match operation {
        IrOperation::Load {
            destination,
            writeback: Some(update),
            ..
        } if overlaps(update.base, destination) => return None,
        IrOperation::Store {
            value: IrOperand::Register(value),
            writeback: Some(update),
            ..
        } if overlaps(update.base, value) => return None,
        IrOperation::LoadPair {
            first,
            second,
            writeback,
            ..
        } if first.index == second.index
            || writeback
                .map(|update| overlaps(update.base, first) || overlaps(update.base, second))
                .unwrap_or(false) =>
        {
            return None
        }
        IrOperation::StorePair {
            first,
            second,
            writeback: Some(update),
            ..
        } if overlaps(update.base, first) || overlaps(update.base, second) => return None,
        _ => {}
    }
    let operator = |operator| match operator {
        IrBinaryOp::Add => "+",
        IrBinaryOp::Subtract => "-",
        IrBinaryOp::And => "&",
        IrBinaryOp::Or => "|",
        IrBinaryOp::Xor => "^",
    };
    Some(match operation {
        IrOperation::Nop => tree("no-op", None, object([]), vec![]),
        IrOperation::Marker { mnemonic, domain } => tree(
            "runtime-marker",
            None,
            object([
                ("mnemonic", mnemonic.into()),
                ("domain", domain.map(V::from).unwrap_or(V::Null)),
                ("execution_contract", "unqualified".into()),
            ]),
            vec![],
        ),
        IrOperation::ReadNZCV { destination } => tree(
            "system-register-read",
            None,
            object([("register", "NZCV".into())]),
            vec![reg(destination)],
        ),
        IrOperation::WriteNZCV { source } => tree(
            "system-register-write",
            None,
            object([
                ("register", "NZCV".into()),
                ("source", source.target().name().into()),
                ("source_bits", "31..28".into()),
            ]),
            vec![reg(source)],
        ),
        IrOperation::Assign { destination, value } => {
            assign(destination, operand(value, destination.bits))
        }
        IrOperation::Address {
            destination, value, ..
        } => assign(destination, constant(value, destination.bits)),
        IrOperation::MoveKeep {
            destination,
            value,
            shift,
        } => assign(
            destination,
            binary(
                "|",
                binary(
                    "&",
                    reg(destination),
                    constant(!(0xffffu64 << shift), destination.bits),
                    destination.bits,
                ),
                constant(u64::from(value) << shift, destination.bits),
                destination.bits,
            ),
        ),
        IrOperation::Compare { kind, left, right } => {
            let right = match right {
                IrCompareOperand::Immediate(value) => constant(value, left.bits),
                IrCompareOperand::Register {
                    register,
                    shift: None,
                } => reg(register),
                _ => return None,
            };
            tree(
                "flags-compare",
                None,
                object([
                    ("operator", ir_compare_mnemonic(kind).into()),
                    ("bits", left.bits.into()),
                ]),
                vec![reg(left), right],
            )
        }
        IrOperation::ConditionalSelect {
            destination,
            if_true,
            if_false,
            condition,
            else_op,
        } => {
            let other = match else_op {
                IrSelectElse::None => reg(if_false),
                IrSelectElse::Increment => binary(
                    "+",
                    reg(if_false),
                    constant(1, destination.bits),
                    destination.bits,
                ),
                IrSelectElse::Invert => unary("~", reg(if_false), destination.bits),
                IrSelectElse::Negate => unary("-", reg(if_false), destination.bits),
            };
            assign(
                destination,
                tree(
                    "select",
                    Some(integer_type(destination.bits)),
                    object([]),
                    vec![
                        tree(
                            "flag-condition",
                            Some("bool".into()),
                            object([
                                ("predicate", flag_predicate_name(condition).into()),
                                ("state", "nzcv".into()),
                            ]),
                            vec![],
                        ),
                        reg(if_true),
                        other,
                    ],
                ),
            )
        }
        IrOperation::Binary {
            destination,
            operator: op,
            left,
            right,
        } => assign(
            destination,
            binary(
                operator(op),
                operand(left, destination.bits),
                operand(right, destination.bits),
                destination.bits,
            ),
        ),
        IrOperation::AddCarry { destination, left, right, subtract, set_flags } => assign(
            destination,
            tree(
                "add-with-carry",
                Some(integer_type(destination.bits)),
                object([
                    ("bits", destination.bits.into()),
                    ("subtract", subtract.into()),
                    ("set_flags", set_flags.into()),
                ]),
                vec![
                    reg(left),
                    reg(right),
                    tree("flag-carry", Some("bool".into()), object([("state", "nzcv".into())]), vec![]),
                ],
            ),
        ),
        IrOperation::LogicalRegister {
            destination,
            left,
            right,
            operator: op,
            negate_right,
            shift,
        } => {
            let right = shift
                .map(|(kind, amount)| {
                    binary(
                        match kind {
                            IrShiftKind::Left => "<<",
                            IrShiftKind::LogicalRight => ">>",
                            IrShiftKind::ArithmeticRight => "asr",
                        },
                        reg(right),
                        constant(u64::from(amount), destination.bits),
                        destination.bits,
                    )
                })
                .unwrap_or_else(|| reg(right));
            let right = if negate_right {
                unary("~", right, destination.bits)
            } else {
                right
            };
            assign(
                destination,
                binary(operator(op), reg(left), right, destination.bits),
            )
        }
        IrOperation::Shift {
            destination,
            source,
            kind,
            amount,
        } => assign(
            destination,
            binary(
                match kind {
                    IrShiftKind::Left => "<<",
                    IrShiftKind::LogicalRight => ">>",
                    IrShiftKind::ArithmeticRight => "asr",
                },
                reg(source),
                constant(u64::from(amount), destination.bits),
                destination.bits,
            ),
        ),
        IrOperation::ShiftedArithmetic {
            destination,
            left,
            right,
            subtract,
            shift,
            amount,
        } => assign(
            destination,
            binary(
                if subtract { "-" } else { "+" },
                reg(left),
                binary(
                    match shift {
                        IrShiftKind::Left => "<<",
                        IrShiftKind::LogicalRight => ">>",
                        IrShiftKind::ArithmeticRight => "asr",
                    },
                    reg(right),
                    constant(u64::from(amount), destination.bits),
                    destination.bits,
                ),
                destination.bits,
            ),
        ),
        IrOperation::VariableShift {
            destination,
            source,
            amount,
            kind,
        } => assign(
            destination,
            binary(
                match kind {
                    IrVariableShiftKind::Left => "<<",
                    IrVariableShiftKind::LogicalRight => ">>",
                    IrVariableShiftKind::ArithmeticRight => "asr",
                    IrVariableShiftKind::Rotate => "ror",
                },
                reg(source),
                binary(
                    "&",
                    reg(amount),
                    constant(u64::from(destination.bits - 1), destination.bits),
                    destination.bits,
                ),
                destination.bits,
            ),
        ),
        IrOperation::MultiplyAdd {
            destination,
            left,
            right,
            addend,
            subtract,
        } => {
            let product = binary("*", reg(left), reg(right), destination.bits);
            assign(
                destination,
                match addend {
                    Some(addend) => binary(
                        if subtract { "-" } else { "+" },
                        reg(addend),
                        product,
                        destination.bits,
                    ),
                    None if subtract => unary("-", product, destination.bits),
                    None => product,
                },
            )
        }
        IrOperation::Divide {
            destination,
            left,
            right,
            signed,
        } => assign(
            destination,
            tree(
                "integer-divide",
                Some(integer_type(destination.bits)),
                object([
                    ("signed", signed.into()),
                    ("bits", destination.bits.into()),
                    ("zero_divisor_result", "zero".into()),
                    ("signed_overflow_result", "minimum-integer".into()),
                ]),
                vec![reg(left), reg(right)],
            ),
        ),
        IrOperation::Extend {
            destination,
            source,
            signed,
            from,
        } => assign(
            destination,
            tree(
                "extend",
                Some(integer_type(destination.bits)),
                object([("signed", signed.into()), ("from_bits", from.into())]),
                vec![reg(source)],
            ),
        ),
        IrOperation::Extract {
            destination,
            source,
            lsb,
            width,
            signed,
        } => assign(
            destination,
            tree(
                "bit-extract",
                Some(integer_type(destination.bits)),
                object([
                    ("lsb", lsb.into()),
                    ("width", width.into()),
                    ("signed", signed.into()),
                ]),
                vec![reg(source)],
            ),
        ),
        IrOperation::Load {
            destination,
            address,
            bytes,
            signed,
            writeback: update,
        } => ordered_access(
            assign(
                destination,
                memory(
                    effective_address(address, update),
                    bytes,
                    signed,
                    destination.bits,
                ),
            ),
            update,
        ),
        IrOperation::Store {
            address,
            value,
            bytes,
            writeback: update,
        } => ordered_access(
            store(
                effective_address(address, update),
                operand(value, bytes * 8),
                bytes,
            ),
            update,
        ),
        IrOperation::LoadPair {
            first,
            second,
            address,
            bytes,
            signed,
            writeback: update,
        } => {
            // A pair is one ordered multi-result node. Its address is evaluated
            // once before either destination is written, even if base==first.
            let pair = tree(
                "memory-load-pair",
                None,
                object([
                    ("bytes_per_value", bytes.into()),
                    ("signed", signed.into()),
                    ("address_evaluation", "once-before-results".into()),
                    ("address_space", "guest".into()),
                    ("fault_model", "unqualified".into()),
                ]),
                vec![effective_address(address, update), reg(first), reg(second)],
            );
            ordered_access(pair, update)
        }
        IrOperation::StorePair {
            address,
            first,
            second,
            bytes,
            writeback: update,
        } => ordered_access(
            tree(
                "memory-store-pair",
                None,
                object([
                    ("bytes_per_value", bytes.into()),
                    ("address_space", "guest".into()),
                    ("fault_model", "unqualified".into()),
                ]),
                vec![effective_address(address, update), reg(first), reg(second)],
            ),
            update,
        ),
        IrOperation::SimdLoad {
            destination,
            address,
            writeback: update,
        } => ordered_access(
            tree(
                "assignment",
                None,
                object([("write_policy", "simd-view-write".into())]),
                vec![
                    vector(destination),
                    memory(
                        effective_address(address, update),
                        destination.width.bytes(),
                        false,
                        destination.width.bytes() * 8,
                    ),
                ],
            ),
            update,
        ),
        IrOperation::SimdStore {
            source,
            address,
            writeback: update,
        } => ordered_access(
            store(
                effective_address(address, update),
                vector(source),
                source.width.bytes(),
            ),
            update,
        ),
        IrOperation::ScalarIntFloatConvert { destination, source, signed, fractional_bits } => {
            let conversion_register = |value| match value {
                IrConversionRegister::Integer { index, wide } => reg(Register { index: u32::from(index), wide, stack_pointer: false }.into()),
                IrConversionRegister::Float { index, precision } => vector(IrSimdRegister { index, width: match precision {
                    IrFloatPrecision::Half => IrSimdWidth::Half, IrFloatPrecision::Single => IrSimdWidth::Single, IrFloatPrecision::Double => IrSimdWidth::Double,
                } }),
            };
            tree("scalar-int-float-conversion", None, object([
                ("signed", signed.into()),
                ("fractional_bits", fractional_bits.map(|bits| V::Int(i128::from(bits))).unwrap_or(V::Null)),
                ("direction", if matches!(destination, IrConversionRegister::Float { .. }) { "integer-to-float" } else { "float-to-integer" }.into()),
                ("rounding", if matches!(destination, IrConversionRegister::Float { .. }) { "fpcr" } else { "toward-zero" }.into()),
                ("result_discarded", matches!(destination, IrConversionRegister::Integer { index: 31, .. }).into()),
                ("fp_status", "unmodeled".into()),
            ]), vec![conversion_register(destination), conversion_register(source)])
        }
        IrOperation::VectorExtract {
            destination,
            source,
            lane,
            bits,
            signed,
        } => assign(
            destination,
            tree(
                "vector-lane-extract",
                Some(integer_type(destination.bits)),
                object([
                    ("source_vector", u64::from(source).into()),
                    ("lane", lane.into()),
                    ("element_bits", bits.into()),
                    ("signed", signed.into()),
                ]),
                vec![],
            ),
        ),
        IrOperation::Branch { target } => {
            tree("goto", None, object([("target", address(target))]), vec![])
        }
        IrOperation::ConditionalBranch { target, condition } => {
            let condition = match condition {
                IrCondition::Zero { register, nonzero } => binary(
                    if nonzero { "!=" } else { "==" },
                    reg(register),
                    constant(0, register.bits),
                    register.bits,
                ),
                IrCondition::Bit {
                    register,
                    bit,
                    nonzero,
                } => binary(
                    if nonzero { "!=" } else { "==" },
                    binary(
                        "&",
                        reg(register),
                        constant(1u64 << bit, register.bits),
                        register.bits,
                    ),
                    constant(0, register.bits),
                    register.bits,
                ),
                IrCondition::Flags(predicate) => tree(
                    "flag-condition",
                    Some("bool".into()),
                    object([
                        ("predicate", flag_predicate_name(predicate).into()),
                        ("state", "nzcv".into()),
                    ]),
                    vec![],
                ),
            };
            tree(
                "conditional-goto",
                None,
                object([("target", address(target))]),
                vec![condition],
            )
        }
        IrOperation::Return { target } => tree(
            "machine-return",
            None,
            object([("source_return_type", "unresolved".into())]),
            target.map(reg).into_iter().collect(),
        ),
        IrOperation::DirectCall { target } => tree(
            "call",
            None,
            object([
                ("target", address(target)),
                ("call_effects", calls::CallSummary::direct(target).payload()),
            ]),
            vec![],
        ),
        IrOperation::IndirectCall { target } => tree(
            "indirect-call",
            None,
            object([(
                "call_effects",
                calls::CallSummary::indirect(target.target().name()).payload(),
            )]),
            vec![reg(target)],
        ),
        IrOperation::ScalarFloatImmediate { destination, precision, bits, immediate } => {
            let (name, bits_width) = match precision {
                IrFloatPrecision::Half => (format!("h{destination}"), 16u8),
                IrFloatPrecision::Single => (format!("s{destination}"), 32u8),
                IrFloatPrecision::Double => (format!("d{destination}"), 64u8),
            };
            let ty = match precision {
                IrFloatPrecision::Half => "uint16_t",
                IrFloatPrecision::Single => "float",
                IrFloatPrecision::Double => "double",
            };
            tree(
                "assignment",
                None,
                object([("write_policy", "fp-scalar-write".into())]),
                vec![
                    tree("fp-register", Some(ty.into()), object([("name", name.into()), ("bits", (bits_width as u64).into()), ("precision", match precision { IrFloatPrecision::Half=>"half", IrFloatPrecision::Single=>"single", IrFloatPrecision::Double=>"double" }.into())]), vec![]),
                    tree("float-constant", Some(ty.into()), object([("bits", bits.into()), ("immediate", (immediate as u64).into()), ("precision", match precision { IrFloatPrecision::Half=>"half", IrFloatPrecision::Single=>"single", IrFloatPrecision::Double=>"double" }.into())]), vec![]),
                ],
            )
        }
        IrOperation::ScalarFloatMove { destination, source, precision } => {
            let (dname, sname, bits_width) = match precision {
                IrFloatPrecision::Half => (format!("h{destination}"), format!("h{source}"), 16u8),
                IrFloatPrecision::Single => (format!("s{destination}"), format!("s{source}"), 32u8),
                IrFloatPrecision::Double => (format!("d{destination}"), format!("d{source}"), 64u8),
            };
            let ty = match precision { IrFloatPrecision::Half=>"uint16_t", IrFloatPrecision::Single=>"float", IrFloatPrecision::Double=>"double" };
            let reg_node = |name: String| tree("fp-register", Some(ty.into()), object([("name", name.into()), ("bits", (bits_width as u64).into())]), vec![]);
            tree("assignment", None, object([("write_policy","fp-scalar-write".into())]), vec![reg_node(dname), reg_node(sname)])
        }
        IrOperation::ScalarFloatBinary { destination, left, right, precision, operator } => {
            let (dname, lname, rname, bits_width) = match precision {
                IrFloatPrecision::Half => (format!("h{destination}"), format!("h{left}"), format!("h{right}"), 16u8),
                IrFloatPrecision::Single => (format!("s{destination}"), format!("s{left}"), format!("s{right}"), 32u8),
                IrFloatPrecision::Double => (format!("d{destination}"), format!("d{left}"), format!("d{right}"), 64u8),
            };
            let ty = match precision { IrFloatPrecision::Half=>"uint16_t", IrFloatPrecision::Single=>"float", IrFloatPrecision::Double=>"double" };
            let op = match operator {
                IrFloatBinaryOperator::Add => "+",
                IrFloatBinaryOperator::Subtract => "-",
                IrFloatBinaryOperator::Multiply => "*",
                IrFloatBinaryOperator::Divide => "/",
                _ => return None,
            };
            let reg_node = |name: String| tree("fp-register", Some(ty.into()), object([("name", name.into()), ("bits", (bits_width as u64).into())]), vec![]);
            let expr = tree("fp-binary", Some(ty.into()), object([("operator", op.into()), ("bits", (bits_width as u64).into())]), vec![reg_node(lname), reg_node(rname)]);
            tree("assignment", None, object([("write_policy","fp-scalar-write".into())]), vec![reg_node(dname), expr])
        }
        _ => return None,
    })
}

/// Preserve the encoded NZCV write that the older shared GPR operation does
/// not carry. Read original operands BEFORE writing an aliased destination.
/// This is instruction-derived effect augmentation, never text reparsing.
fn instruction_model(operation: Operation, portable: IrOperation, word: u32) -> Option<Tree> {
    let value = model(portable)?;
    let compare = match operation {
        Operation::AddImmediate { subtract, .. } | Operation::AddRegister { subtract, .. }
            if word & (1 << 29) != 0 =>
        {
            let expression = value.children.get(1)?;
            tree(
                "flags-arithmetic",
                None,
                object([
                    ("operator", if subtract { "sub" } else { "add" }.into()),
                    ("bits", num_bits(expression).into()),
                ]),
                expression.children.clone(),
            )
        }
        Operation::LogicalImmediate { .. } | Operation::LogicalRegister { .. }
            if word & 0x60000000 == 0x60000000 =>
        {
            let expression = value.children.get(1)?.clone();
            let bits = num_bits(&expression);
            tree(
                "flags-compare",
                None,
                object([("operator", "tst".into()), ("bits", bits.into())]),
                vec![expression, constant(u64::MAX, bits)],
            )
        }
        _ => return Some(value),
    };
    Some(sequence(vec![compare, value]))
}
fn num_bits(value: &Tree) -> u8 {
    match value.attributes.get("bits") {
        Some(V::Int(bits)) => *bits as u8,
        _ => 64,
    }
}

fn text_attribute(tree: &Tree, key: &str) -> String {
    match tree.attributes.get(key) {
        Some(V::String(value)) => value.clone(),
        Some(V::Int(value)) => value.to_string(),
        _ => "unresolved".into(),
    }
}
impl Tree {
    fn structured(&self) -> V {
        object([
            ("kind", self.kind.into()),
            ("type", self.ty.clone().map(V::from).unwrap_or(V::Null)),
            ("attributes", self.attributes.clone()),
            (
                "children",
                array(self.children.iter().map(Self::structured)),
            ),
        ])
    }
    fn render(&self) -> String {
        let child = |index: usize| {
            self.children
                .get(index)
                .map(Self::render)
                .unwrap_or_else(|| "unresolved".into())
        };
        match self.kind {
            "register" => text_attribute(self, "name"),
            "constant" => text_attribute(self, "hex"),
            "scalar-int-float-conversion" => {
                let attributes = match &self.attributes { V::Object(fields) => fields, _ => unreachable!() };
                let to_float = text_attribute(self, "direction") == "integer-to-float";
                let signed = attributes.get("signed") == Some(&V::Bool(true));
                let discarded = attributes.get("result_discarded") == Some(&V::Bool(true));
                let mnemonic = match (to_float, signed) { (true, true) => "scvtf", (true, false) => "ucvtf", (false, true) => "fcvtzs", (false, false) => "fcvtzu" };
                let scale = match attributes.get("fractional_bits") { Some(V::Int(bits)) => format!(", fractional_bits={bits}"), _ => String::new() };
                let expression = format!("{mnemonic}({}{scale})", child(1));
                let action = if discarded { format!("discard({expression})") } else { format!("{} = {expression}", child(0)) };
                format!("{action}; /* FP status effects unresolved */")
            }
            "binary" => format!(
                "({} {} {})",
                child(0),
                text_attribute(self, "operator"),
                child(1)
            ),
            "unary" => format!("{}({})", text_attribute(self, "operator"), child(0)),
            "assignment" => format!("{} = {};", child(0), child(1)),
            "sequence" => self
                .children
                .iter()
                .map(Self::render)
                .collect::<Vec<_>>()
                .join(" "),
            "memory-load" => format!("guest_load{}({})", text_attribute(self, "bytes"), child(0)),
            "memory-store" => format!(
                "guest_store{}({}, {});",
                text_attribute(self, "bytes"),
                child(0),
                child(1)
            ),
            "memory-load-pair" => format!(
                "guest_load_pair{}({}, &{}, &{});",
                text_attribute(self, "bytes_per_value"),
                child(0),
                child(1),
                child(2)
            ),
            "memory-store-pair" => format!(
                "guest_store_pair{}({}, {}, {});",
                text_attribute(self, "bytes_per_value"),
                child(0),
                child(1),
                child(2)
            ),
            "goto" => format!(
                "goto loc_{};",
                text_attribute(self, "target").trim_start_matches("0x")
            ),
            "conditional-goto" => format!(
                "if ({}) goto loc_{};",
                child(0),
                text_attribute(self, "target").trim_start_matches("0x")
            ),
            "indirect-goto" => {
                let dispatch = child(0);
                let targets = match self.attributes.get("targets") {
                    Some(V::Array(items)) => items.iter().filter_map(|item| match item {
                        V::String(v) => Some(v.clone()),
                        V::Int(v) => Some(format!("{v:#x}")),
                        _ => None,
                    }).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let cases = match self.attributes.get("case_map") {
                    Some(V::Array(items)) => items.iter().filter_map(|item| {
                        let V::Object(fields) = item else { return None };
                        let case = match fields.get("case") { Some(V::Int(v)) => Some(format!("{v:#x}")), Some(V::String(v)) => Some(v.clone()), _ => None }?;
                        let target = match fields.get("target") { Some(V::String(v)) => Some(v.clone()), Some(V::Int(v)) => Some(format!("{v:#x}")), _ => None }?;
                        Some(format!("{case}->{target}"))
                    }).collect::<Vec<_>>(),
                    _ => Vec::new(),
                };
                let info = match self.attributes.get("index_info") {
                    Some(V::String(v)) if !v.is_empty() => format!(" {v};"),
                    _ => String::new(),
                };
                if targets.is_empty() {
                    format!("indirect_goto({dispatch}); /* resolved complete, no in-function targets{info} */")
                } else if cases.is_empty() {
                    format!("indirect_goto({dispatch}, targets=[{}]); /* resolved complete{info} */", targets.join(", "))
                } else {
                    format!("indirect_goto({dispatch}, targets=[{}], cases=[{}]); /* resolved complete{info} */", targets.join(", "), cases.join(", "))
                }
            }
            "machine-return" => format!(
                "return_to({}); /* source result ABI unresolved */",
                child(0)
            ),
            "call" => format!(
                "call({}); /* ABI effects unresolved */",
                text_attribute(self, "target")
            ),
            "indirect-call" => format!(
                "call({}); /* indirect target and ABI unresolved */",
                child(0)
            ),
            "flag-condition" => format!("nzcv_{}", text_attribute(self, "predicate")),
            "no-op" => "/* nop */".into(),
            _ => format!(
                "{}({}); /* {} */",
                self.kind.replace('-', "_"),
                self.children
                    .iter()
                    .map(Self::render)
                    .collect::<Vec<_>>()
                    .join(", "),
                self.attributes.json()
            ),
        }
    }
    fn emit(&self, builder: &mut Builder, evidence: &str, pc: u64, pc_end: u64) -> io::Result<(String, String)> {
        let children = self
            .children
            .iter()
            .map(|child| child.emit(builder, evidence, pc, pc_end))
            .collect::<io::Result<Vec<_>>>()?;
        let id = builder.ast(
            self.kind,
            self.ty.as_deref(),
            children.iter().map(|(id, _)| id.clone()).collect(),
            evidence,
            (pc, pc_end),
            self.attributes.clone(),
        )?;
        let mirror = builder.record(
            "ast-node",
            "derived",
            object([("ast_id", id.clone().into()), ("kind", self.kind.into())]),
            ranges(pc, pc_end),
        )?;
        builder.edge(evidence, &mirror, "renders-as")?;
        for (_, child_mirror) in children {
            builder.edge(&child_mirror, &mirror, "derives")?;
        }
        Ok((id, mirror))
    }
}


fn apc_at(index: usize, insn_pcs: &[u64], start: u64) -> u64 {
    apc_at_with_arch(index, insn_pcs, start, "arm64")
}
fn apc_at_with_arch(index: usize, insn_pcs: &[u64], start: u64, architecture: &str) -> u64 {
    if let Some(&pc) = insn_pcs.get(index) {
        pc
    } else if insn_pcs.is_empty() && super::is_arm64_arch(architecture) {
        start.saturating_add((index as u64).saturating_mul(super::ARM64_INSN_BYTES))
    } else {
        start
    }
}
fn aindex_of(pc: u64, insn_pcs: &[u64], start: u64, count: usize) -> Option<usize> {
    aindex_of_with_arch(pc, insn_pcs, start, count, "arm64")
}
fn aindex_of_with_arch(pc: u64, insn_pcs: &[u64], start: u64, count: usize, architecture: &str) -> Option<usize> {
    if insn_pcs.is_empty() {
        if !super::is_arm64_arch(architecture) {
            return None;
        }
        let delta = pc.checked_sub(start)?;
        let insn = super::ARM64_INSN_BYTES;
        if delta % insn != 0 { return None; }
        let i = usize::try_from(delta / insn).ok()?;
        (i < count).then_some(i)
    } else {
        insn_pcs.binary_search(&pc).ok()
    }
}
fn aspan_end_pc(pc: u64, insn_pcs: &[u64], start: u64, count: usize, fallback_end: u64) -> u64 {
    aspan_end_pc_with_sizes_and_arch(pc, insn_pcs, &[], start, count, fallback_end, "arm64")
}
fn aspan_end_pc_with_sizes(pc: u64, insn_pcs: &[u64], insn_sizes: &[u32], start: u64, count: usize, fallback_end: u64) -> u64 {
    aspan_end_pc_with_sizes_and_arch(pc, insn_pcs, insn_sizes, start, count, fallback_end, "arm64")
}
fn aspan_end_pc_with_sizes_and_arch(pc: u64, insn_pcs: &[u64], insn_sizes: &[u32], start: u64, count: usize, fallback_end: u64, architecture: &str) -> u64 {
    let paired = insn_sizes.len() == insn_pcs.len();
    if let Some(i) = aindex_of_with_arch(pc, insn_pcs, start, count, architecture) {
        if paired && i < insn_sizes.len() {
            return pc.saturating_add(insn_sizes[i] as u64);
        }
        if insn_pcs.is_empty() && super::is_arm64_arch(architecture) {
            return start.saturating_add(((i as u64) + 1).saturating_mul(super::ARM64_INSN_BYTES));
        }
        if let Some(&next) = insn_pcs.get(i + 1) {
            return next;
        }
        return fallback_end;
    }
    if insn_pcs.is_empty() {
        if super::is_arm64_arch(architecture) {
            return pc.saturating_add(super::ARM64_INSN_BYTES);
        }
        return pc;
    }
    // Variable-length stream: try known PC index for size; otherwise next PC or fallback.
    match insn_pcs.binary_search(&pc) {
        Ok(i) => {
            if paired && i < insn_sizes.len() { pc.saturating_add(insn_sizes[i] as u64) }
            else { insn_pcs.get(i + 1).copied().unwrap_or(fallback_end) }
        }
        Err(i) => insn_pcs.get(i).copied().unwrap_or(fallback_end),
    }
}

/// Conservative x86_64/x86-64 instruction lifting scaffold (iced-x86).
/// Sound by construction: only instructions with clear, evidence-backed mappings to the shared IR
/// are lifted; all others fall back to `IrOperation::TargetSpecific` with unsupported trees.
/// ABI adaptation and full semantic coverage are incomplete.
/// Returns (portable op, tree, ir text, optional conditional-branch target for CFG when condition is unmodeled).
fn x86_64_lift_instruction(pc: u64, bytes: &[u8]) -> (IrOperation, Tree, String, Option<u64>) {
    use iced_x86::{Mnemonic, OpKind, Register};
    let mut decoder = iced_x86::Decoder::with_ip(64, bytes, pc, iced_x86::DecoderOptions::NONE);
    let instr = decoder.decode();
    let sz = instr.len().max(bytes.len().min(15));
    let ir_text = format!("{pc:#x}: {:?} ({} bytes; partial x86_64 lift)", instr, sz);
    let unsupported = || (
        IrOperation::TargetSpecific,
        tree("unsupported-operation", Some(format!("x86_64 {:?} not yet lifted", instr.mnemonic())), object([("architecture", "x86_64".into()), ("mnemonic", format!("{:?}", instr.mnemonic()).into()), ("size", (sz as u64).into()), ("status", "unsupported".into())]), vec![]),
        ir_text.clone(),
        None,
    );
    let xreg = |r: Register| -> Option<(IrRegister, u32)> {
        // Returns (normalized 64-bit register identity, operand bit width). Conservative mapping.
        Some(match r {
            Register::RAX => (IrRegister{class: IrRegisterClass::General, index: 0, bits: 64}, 64),
            Register::EAX => (IrRegister{class: IrRegisterClass::General, index: 0, bits: 64}, 32),
            Register::AX  => (IrRegister{class: IrRegisterClass::General, index: 0, bits: 64}, 16),
            Register::AL|Register::AH => (IrRegister{class: IrRegisterClass::General, index: 0, bits: 64}, 8),
            Register::RCX => (IrRegister{class: IrRegisterClass::General, index: 1, bits: 64}, 64),
            Register::ECX => (IrRegister{class: IrRegisterClass::General, index: 1, bits: 64}, 32),
            Register::CX  => (IrRegister{class: IrRegisterClass::General, index: 1, bits: 64}, 16),
            Register::CL|Register::CH => (IrRegister{class: IrRegisterClass::General, index: 1, bits: 64}, 8),
            Register::RDX => (IrRegister{class: IrRegisterClass::General, index: 2, bits: 64}, 64),
            Register::EDX => (IrRegister{class: IrRegisterClass::General, index: 2, bits: 64}, 32),
            Register::DX  => (IrRegister{class: IrRegisterClass::General, index: 2, bits: 64}, 16),
            Register::DL|Register::DH => (IrRegister{class: IrRegisterClass::General, index: 2, bits: 64}, 8),
            Register::RBX => (IrRegister{class: IrRegisterClass::General, index: 3, bits: 64}, 64),
            Register::EBX => (IrRegister{class: IrRegisterClass::General, index: 3, bits: 64}, 32),
            Register::BX  => (IrRegister{class: IrRegisterClass::General, index: 3, bits: 64}, 16),
            Register::BL|Register::BH => (IrRegister{class: IrRegisterClass::General, index: 3, bits: 64}, 8),
            Register::RSP => (IrRegister{class: IrRegisterClass::StackPointer, index: 31, bits: 64}, 64),
            Register::ESP => (IrRegister{class: IrRegisterClass::StackPointer, index: 31, bits: 64}, 32),
            Register::SP  => (IrRegister{class: IrRegisterClass::StackPointer, index: 31, bits: 64}, 16),
            Register::SPL => (IrRegister{class: IrRegisterClass::StackPointer, index: 31, bits: 64}, 8),
            Register::RBP => (IrRegister{class: IrRegisterClass::General, index: 5, bits: 64}, 64),
            Register::EBP => (IrRegister{class: IrRegisterClass::General, index: 5, bits: 64}, 32),
            Register::BP  => (IrRegister{class: IrRegisterClass::General, index: 5, bits: 64}, 16),
            Register::BPL => (IrRegister{class: IrRegisterClass::General, index: 5, bits: 64}, 8),
            Register::RSI => (IrRegister{class: IrRegisterClass::General, index: 6, bits: 64}, 64),
            Register::ESI => (IrRegister{class: IrRegisterClass::General, index: 6, bits: 64}, 32),
            Register::SI  => (IrRegister{class: IrRegisterClass::General, index: 6, bits: 64}, 16),
            Register::SIL => (IrRegister{class: IrRegisterClass::General, index: 6, bits: 64}, 8),
            Register::RDI => (IrRegister{class: IrRegisterClass::General, index: 7, bits: 64}, 64),
            Register::EDI => (IrRegister{class: IrRegisterClass::General, index: 7, bits: 64}, 32),
            Register::DI  => (IrRegister{class: IrRegisterClass::General, index: 7, bits: 64}, 16),
            Register::DIL => (IrRegister{class: IrRegisterClass::General, index: 7, bits: 64}, 8),
            Register::R8  => (IrRegister{class: IrRegisterClass::General, index: 8, bits: 64}, 64),
            Register::R8D => (IrRegister{class: IrRegisterClass::General, index: 8, bits: 64}, 32),
            Register::R8W => (IrRegister{class: IrRegisterClass::General, index: 8, bits: 64}, 16),
            Register::R8L => (IrRegister{class: IrRegisterClass::General, index: 8, bits: 64}, 8),
            Register::R9  => (IrRegister{class: IrRegisterClass::General, index: 9, bits: 64}, 64),
            Register::R9D => (IrRegister{class: IrRegisterClass::General, index: 9, bits: 64}, 32),
            Register::R9W => (IrRegister{class: IrRegisterClass::General, index: 9, bits: 64}, 16),
            Register::R9L => (IrRegister{class: IrRegisterClass::General, index: 9, bits: 64}, 8),
            Register::R10  => (IrRegister{class: IrRegisterClass::General, index: 10, bits: 64}, 64),
            Register::R10D => (IrRegister{class: IrRegisterClass::General, index: 10, bits: 64}, 32),
            Register::R10W => (IrRegister{class: IrRegisterClass::General, index: 10, bits: 64}, 16),
            Register::R10L => (IrRegister{class: IrRegisterClass::General, index: 10, bits: 64}, 8),
            Register::R11  => (IrRegister{class: IrRegisterClass::General, index: 11, bits: 64}, 64),
            Register::R11D => (IrRegister{class: IrRegisterClass::General, index: 11, bits: 64}, 32),
            Register::R11W => (IrRegister{class: IrRegisterClass::General, index: 11, bits: 64}, 16),
            Register::R11L => (IrRegister{class: IrRegisterClass::General, index: 11, bits: 64}, 8),
            Register::R12  => (IrRegister{class: IrRegisterClass::General, index: 12, bits: 64}, 64),
            Register::R12D => (IrRegister{class: IrRegisterClass::General, index: 12, bits: 64}, 32),
            Register::R12W => (IrRegister{class: IrRegisterClass::General, index: 12, bits: 64}, 16),
            Register::R12L => (IrRegister{class: IrRegisterClass::General, index: 12, bits: 64}, 8),
            Register::R13  => (IrRegister{class: IrRegisterClass::General, index: 13, bits: 64}, 64),
            Register::R13D => (IrRegister{class: IrRegisterClass::General, index: 13, bits: 64}, 32),
            Register::R13W => (IrRegister{class: IrRegisterClass::General, index: 13, bits: 64}, 16),
            Register::R13L => (IrRegister{class: IrRegisterClass::General, index: 13, bits: 64}, 8),
            Register::R14  => (IrRegister{class: IrRegisterClass::General, index: 14, bits: 64}, 64),
            Register::R14D => (IrRegister{class: IrRegisterClass::General, index: 14, bits: 64}, 32),
            Register::R14W => (IrRegister{class: IrRegisterClass::General, index: 14, bits: 64}, 16),
            Register::R14L => (IrRegister{class: IrRegisterClass::General, index: 14, bits: 64}, 8),
            Register::R15  => (IrRegister{class: IrRegisterClass::General, index: 15, bits: 64}, 64),
            Register::R15D => (IrRegister{class: IrRegisterClass::General, index: 15, bits: 64}, 32),
            Register::R15W => (IrRegister{class: IrRegisterClass::General, index: 15, bits: 64}, 16),
            Register::R15L => (IrRegister{class: IrRegisterClass::General, index: 15, bits: 64}, 8),
            _ => return None,
        })
    };
    let imm_operand = |kind: OpKind, instr: &iced_x86::Instruction| -> Option<u64> {
        Some(match kind {
            OpKind::Immediate8 => instr.immediate8() as u64,
            OpKind::Immediate16 => instr.immediate16() as u64,
            OpKind::Immediate32 => instr.immediate32() as u64,
            OpKind::Immediate64 => instr.immediate64(),
            OpKind::Immediate8to16 => (instr.immediate8to16() as i64 as u64),
            OpKind::Immediate8to32 => (instr.immediate8to32() as i64 as u64),
            OpKind::Immediate8to64 => (instr.immediate8to64() as i64 as u64),
            OpKind::Immediate32to64 => (instr.immediate32to64() as i64 as u64),
            _ => return None,
        })
    };
    // Jcc: conditional near branch. Condition depends on x86 flags and is not yet mapped soundly to
    // the shared flag model, so keep IrOperation as TargetSpecific while exposing CFG target conservatively.
    let is_jcc = matches!(instr.mnemonic(),
        Mnemonic::Je|Mnemonic::Jne|Mnemonic::Ja|Mnemonic::Jae|Mnemonic::Jb|Mnemonic::Jbe|Mnemonic::Jg|Mnemonic::Jge|Mnemonic::Jl|Mnemonic::Jle|
        Mnemonic::Jo|Mnemonic::Jno|Mnemonic::Js|Mnemonic::Jns|Mnemonic::Jp|Mnemonic::Jnp|Mnemonic::Jcxz|Mnemonic::Jecxz|Mnemonic::Jrcxz
    );
    if is_jcc {
        if instr.op_count() == 1 && matches!(instr.op0_kind(), OpKind::NearBranch16|OpKind::NearBranch32|OpKind::NearBranch64) {
            let target = instr.near_branch64();
            return (
                IrOperation::TargetSpecific,
                tree("conditional-goto", Some("x86_64 conditional branch (condition/flags not yet modeled)".to_string()), object([("architecture","x86_64".into()),("mnemonic",format!("{:?}",instr.mnemonic()).into()),("target",address(target)),("status","partial-cfg".into())]), vec![]),
                format!("{pc:#x}: {:?} {target:#x} (x86_64 condition unmodeled)", instr.mnemonic()),
                Some(target),
            );
        }
        return unsupported();
    }

    match instr.mnemonic() {
        Mnemonic::Nop => (
            IrOperation::Nop,
            tree("no-op", None, object([("architecture","x86_64".into())]), vec![]),
            format!("{pc:#x}: nop"),
            None,
        ),
        Mnemonic::Ret | Mnemonic::Retf => (
            IrOperation::Return { target: None },
            tree("return", None, object([("architecture","x86_64".into()), ("mnemonic", format!("{:?}", instr.mnemonic()).into())]), vec![]),
            ir_text.clone(),
            None,
        ),
        Mnemonic::Int3 => (
            IrOperation::TargetSpecific,
            tree("trap", Some("int3".to_string()), object([("architecture","x86_64".into()),("mnemonic","int3".into())]), vec![]),
            format!("{pc:#x}: int3"),
            None,
        ),
        Mnemonic::Jmp => {
            if instr.op_count() == 1 && matches!(instr.op0_kind(), OpKind::NearBranch16|OpKind::NearBranch32|OpKind::NearBranch64) {
                let target = instr.near_branch64();
                (IrOperation::Branch { target }, tree("goto", None, object([("architecture","x86_64".into()),("target", address(target))]), vec![]), format!("{pc:#x}: jmp {target:#x}"), None)
            } else { unsupported() }
        }
        Mnemonic::Call => {
            if instr.op_count() == 1 && matches!(instr.op0_kind(), OpKind::NearBranch16|OpKind::NearBranch32|OpKind::NearBranch64) {
                let target = instr.near_branch64();
                (IrOperation::DirectCall { target }, tree("direct-call", None, object([("architecture","x86_64".into()),("target", address(target))]), vec![]), format!("{pc:#x}: call {target:#x}"), None)
            } else { unsupported() }
        }
        Mnemonic::Push => {
            if instr.op_count() == 1 && instr.op0_kind() == OpKind::Register {
                if let Some((src, _)) = xreg(instr.op0_register()) {
                    (IrOperation::Store { address: IrMemoryAddress::BaseOffset { base: IrRegister{class: IrRegisterClass::StackPointer,index:31,bits:64}, offset: -8 }, value: IrOperand::Register(src), bytes: 8, writeback: Some(IrWriteback{base: IrRegister{class: IrRegisterClass::StackPointer,index:31,bits:64}, offset: -8, pre_index: true}) }, tree("unsupported-operation", Some("x86_64 push (stack write+adjust lifted partially)".to_string()), object([("architecture","x86_64".into()),("mnemonic","push".into()),("status","partial".into())]), vec![]), ir_text, None)
                } else { unsupported() }
            } else { unsupported() }
        }
        Mnemonic::Pop => {
            if instr.op_count() == 1 && instr.op0_kind() == OpKind::Register {
                if let Some((dst, _)) = xreg(instr.op0_register()) {
                    (IrOperation::Load { destination: dst, address: IrMemoryAddress::BaseOffset { base: IrRegister{class: IrRegisterClass::StackPointer,index:31,bits:64}, offset: 0 }, bytes: 8, signed: false, writeback: Some(IrWriteback{base: IrRegister{class: IrRegisterClass::StackPointer,index:31,bits:64}, offset: 8, pre_index: false}) }, tree("unsupported-operation", Some("x86_64 pop (stack read+adjust lifted partially)".to_string()), object([("architecture","x86_64".into()),("mnemonic","pop".into()),("status","partial".into())]), vec![]), ir_text, None)
                } else { unsupported() }
            } else { unsupported() }
        }
        // xor reg, reg (same register) -> zero idiom. Only sound for 32/64-bit where x86 zero-extends/full-writes.
        Mnemonic::Xor if instr.op_count() == 2 && instr.op0_kind() == OpKind::Register && instr.op1_kind() == OpKind::Register => {
            if instr.op0_register() == instr.op1_register() {
                if let Some((dst, bits)) = xreg(instr.op0_register()) {
                    if bits >= 32 {
                        (IrOperation::Assign { destination: dst, value: IrOperand::Constant(0) },
                         tree("move", Some("x86_64 xor zero-idiom (conservative 64-bit value model)".to_string()), object([("architecture","x86_64".into()),("mnemonic","xor".into()),("status","partial".into())]), vec![]),
                         ir_text, None)
                    } else { unsupported() } // 8/16-bit partial register writes not safely modeled here
                } else { unsupported() }
            } else { unsupported() } // general xor semantics/flags not modeled
        }
        // mov forms (limited, conservative): reg<-imm, reg<-reg (64-bit), and simple RIP-relative 64-bit mem ops.
        Mnemonic::Mov if instr.op_count() == 2 => {
            if instr.op0_kind() == OpKind::Register {
                if let Some((dst, dst_bits)) = xreg(instr.op0_register()) {
                    match instr.op1_kind() {
                        k if matches!(k, OpKind::Immediate8|OpKind::Immediate16|OpKind::Immediate32|OpKind::Immediate64|OpKind::Immediate8to16|OpKind::Immediate8to32|OpKind::Immediate8to64|OpKind::Immediate32to64) => {
                            if let Some(imm) = imm_operand(instr.op1_kind(), &instr) {
                                if dst_bits >= 32 {
                                    let value = if dst_bits == 32 { imm & 0xffff_ffff } else { imm };
                                    (IrOperation::Assign { destination: dst, value: IrOperand::Constant(value) },
                                     tree("move", Some("x86_64 mov immediate (conservative)".to_string()), object([("architecture","x86_64".into()),("mnemonic","mov".into()),("status","partial".into())]), vec![]),
                                     ir_text, None)
                                } else { unsupported() }
                            } else { unsupported() }
                        }
                        OpKind::Register => {
                            if let Some((src, src_bits)) = xreg(instr.op1_register()) {
                                if dst_bits == 64 && src_bits == 64 {
                                    (IrOperation::Assign { destination: dst, value: IrOperand::Register(src) },
                                     tree("move", Some("x86_64 mov reg,reg 64-bit (conservative)".to_string()), object([("architecture","x86_64".into()),("mnemonic","mov".into()),("status","partial".into())]), vec![]),
                                     ir_text, None)
                                } else { unsupported() } // sub-64 or mixed-width moves have x86-specific semantics
                            } else { unsupported() }
                        }
                        OpKind::Memory => {
                            // mov r64, [rip + disp32]: simple RIP-relative 64-bit load.
                            if dst_bits == 64 && instr.memory_base() == Register::RIP && instr.memory_index() == Register::None {
                                let addr = instr.memory_displacement64();
                                let next_pc = pc.saturating_add(sz as u64);
                                let offset = (addr as i64).wrapping_sub(next_pc as i64);
                                (IrOperation::Load { destination: dst, address: IrMemoryAddress::PcRelative { offset, target: addr }, bytes: 8, signed: false, writeback: None },
                                 tree("unsupported-operation", Some("x86_64 rip-relative load (partially lifted)".to_string()), object([("architecture","x86_64".into()),("mnemonic","mov".into()),("status","partial".into()),("address",address(addr))]), vec![]),
                                 ir_text, None)
                            } else { unsupported() }
                        }
                        _ => unsupported(),
                    }
                } else { unsupported() }
            } else if instr.op0_kind() == OpKind::Memory && instr.op1_kind() == OpKind::Register {
                if let Some((src, src_bits)) = xreg(instr.op1_register()) {
                    if src_bits == 64 && instr.memory_base() == Register::RIP && instr.memory_index() == Register::None {
                        let addr = instr.memory_displacement64();
                        let next_pc = pc.saturating_add(sz as u64);
                        let offset = (addr as i64).wrapping_sub(next_pc as i64);
                        (IrOperation::Store { address: IrMemoryAddress::PcRelative { offset, target: addr }, value: IrOperand::Register(src), bytes: 8, writeback: None },
                         tree("unsupported-operation", Some("x86_64 rip-relative store (partially lifted)".to_string()), object([("architecture","x86_64".into()),("mnemonic","mov".into()),("status","partial".into()),("address",address(addr))]), vec![]),
                         ir_text, None)
                    } else { unsupported() }
                } else { unsupported() }
            } else { unsupported() }
        }
        // sub/add rsp, imm: common stack adjustment (flags not modeled). Only when destination is SP and 64-bit form.
        Mnemonic::Sub | Mnemonic::Add if instr.op_count() == 2 && instr.op0_kind() == OpKind::Register => {
            if let Some((dst, dst_bits)) = xreg(instr.op0_register()) {
                if dst.class == IrRegisterClass::StackPointer && dst_bits == 64 {
                    if let Some(imm) = imm_operand(instr.op1_kind(), &instr) {
                        let (op, rhs) = match instr.mnemonic() { Mnemonic::Sub => (IrBinaryOp::Subtract, imm), Mnemonic::Add => (IrBinaryOp::Add, imm), _ => return unsupported() };
                        (IrOperation::Binary { destination: dst, operator: op, left: IrOperand::Register(dst), right: IrOperand::Constant(rhs) },
                         tree("move", Some("x86_64 stack pointer adjustment (flags not modeled)".to_string()), object([("architecture","x86_64".into()),("mnemonic",format!("{:?}",instr.mnemonic()).into()),("status","partial".into())]), vec![]),
                         ir_text, None)
                    } else { unsupported() }
                } else { unsupported() } // general add/sub with flags/effects not safely modeled yet
            } else { unsupported() }
        }
        // lea r64, [rip + disp32]: PC-relative address computation (common for PIC/GOT/data refs).
        Mnemonic::Lea if instr.op_count() == 2 && instr.op0_kind() == OpKind::Register && instr.op1_kind() == OpKind::Memory => {
            if let Some((dst, dst_bits)) = xreg(instr.op0_register()) {
                if dst_bits == 64 && instr.memory_base() == Register::RIP && instr.memory_index() == Register::None {
                    let addr = instr.memory_displacement64();
                    (IrOperation::Address { destination: dst, value: addr, page: false },
                     tree("move", Some("x86_64 lea rip-relative (conservative)".to_string()), object([("architecture","x86_64".into()),("mnemonic","lea".into()),("status","partial".into()),("address",address(addr))]), vec![]),
                     ir_text, None)
                } else { unsupported() } // complex addressing modes need careful modeling
            } else { unsupported() }
        }
        _ => unsupported(),
    }
}

pub fn build(input: Input<'_>) -> io::Result<String> {
    build_mode(input, None)
}
pub fn interpret(input: Input<'_>, registers: [u64; 8]) -> io::Result<String> {
    build_mode(input, Some(registers))
}
fn build_mode(input: Input<'_>, registers: Option<[u64; 8]>) -> io::Result<String> {
    let Input {
        code,
        base,
        start,
        end,
        file_offset,
        data_ranges,
        source,
        architecture,
        name,
        boundary_sources,
        signatures,
        call_prototypes: prototype_candidates,
        project_types,
        native,
        resolved_indirect_targets,
        resolved_indirect_cases,
        resolved_indirect_index_info,
        instruction_pcs: _input_instruction_pcs,
        instruction_sizes: _input_instruction_sizes,
    } = input;
    let arch = architecture.to_ascii_lowercase();
    let is_arm64 = matches!(arch.as_str(), "arm64" | "arm64e" | "aarch64");
    let is_x86_64 = matches!(arch.as_str(), "x86_64" | "x86-64" | "amd64");
    if !is_arm64 && !is_x86_64 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("reconstruction lifting not yet implemented for architecture '{architecture}'"),
        ));
    }
    let fixed_width_4 = is_arm64 && _input_instruction_pcs.is_empty();
    let arm64_insn = super::ARM64_INSN_BYTES;
    if start < base || end <= start || (fixed_width_4 && ((start - base) % arm64_insn != 0 || (end - start) % arm64_insn != 0)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid reconstruction function bounds",
        ));
    }
    let available_end = end.min(base + code.len() as u64);
    // ARM64: fixed 4-byte instructions, preserve existing 128-instruction byte window behavior.
    // x86_64: variable-length instructions (1-15 bytes); decode boundaries up to 128 instructions / 15*128 bytes conservatively.
    let high = if is_arm64 {
        available_end.min(start.saturating_add(128 * arm64_insn))
    } else {
        available_end.min(start.saturating_add(128 * 15))
    };
    let Some(body) = usize::try_from(start - base)
        .ok()
        .zip(usize::try_from(high - base).ok())
        .and_then(|(lo, hi)| code.get(lo..hi))
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Function bytes unavailable",
        ));
    };
    if body.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Function bytes unavailable",
        ));
    }
    let (insn_pcs, insn_sizes, stream_end): (Vec<u64>, Vec<u32>, u64) = if !_input_instruction_pcs.is_empty() {
        let pcs = _input_instruction_pcs.clone();
        let sizes = if _input_instruction_sizes.len() == pcs.len() {
            _input_instruction_sizes.clone()
        } else if is_arm64 {
            vec![4u32; pcs.len()]
        } else {
            vec![] // unknown variable-length sizes; conservative handling below
        };
        let se = if !sizes.is_empty() && sizes.len() == pcs.len() {
            pcs.last().copied().unwrap_or(start).saturating_add(sizes.last().copied().unwrap_or(0) as u64)
        } else if !pcs.is_empty() {
            high.min(end)
        } else {
            start
        };
        (pcs, sizes, se)
    } else if is_arm64 {
        // Explicit ARM64 fixed-width fallback when no PC/size map is provided.
        let insn = super::ARM64_INSN_BYTES as usize;
        let pcs: Vec<u64> = (0..(body.len() / insn)).map(|i| start + i as u64 * super::ARM64_INSN_BYTES).collect();
        let se = pcs.last().map(|p| p.saturating_add(super::ARM64_INSN_BYTES)).unwrap_or(start);
        let sizes = vec![u32::try_from(super::ARM64_INSN_BYTES).unwrap_or(4); pcs.len()];
        (pcs, sizes, se)
    } else {
        // x86_64/x86-64: sound, evidence-backed instruction boundary decoding for PC/size maps.
        // Lifting into shared typed semantic IR/AST/source is not yet implemented; instructions are
        // treated conservatively as target-specific/unsupported with correct spans.
        let mut decoder = iced_x86::Decoder::with_ip(64, body, start, iced_x86::DecoderOptions::NONE);
        let mut pcs = Vec::new();
        let mut sizes = Vec::new();
        while decoder.can_decode() && pcs.len() < 128 {
            let ip = decoder.ip();
            if ip >= end { break; }
            let instr = decoder.decode();
            let sz = instr.len() as u32;
            if sz == 0 { break; }
            pcs.push(ip);
            sizes.push(sz);
        }
        let se = if let (Some(&last_pc), Some(&last_sz)) = (pcs.last(), sizes.last()) {
            last_pc.saturating_add(last_sz as u64)
        } else {
            start
        };
        (pcs, sizes, se)
    };
    let instructions = insn_pcs.len() as u64;
    let instruction_extent = if is_arm64 {
        (end.saturating_sub(start)) / super::ARM64_INSN_BYTES
    } else {
        insn_pcs.len() as u64
    };
    let exhausted = if is_arm64 {
        instruction_extent > 128
    } else {
        insn_pcs.len() >= 128 && stream_end < end.min(available_end)
    };
    let truncated = high < end || (is_x86_64 && stream_end < end.min(available_end));
    let direct_targets = insn_pcs.iter().enumerate().filter_map(|(index, &pc)| {
        let sz = if insn_sizes.len() == insn_pcs.len() { insn_sizes.get(index).copied()? } else if is_arm64 { 4 } else { return None; };
        let (_, _op, portable) = super::decode_lift_for_arch(code, base, pc, sz, &arch)?;
        match portable {
            IrOperation::DirectCall { target } => Some(target),
            _ => None,
        }
    }).collect::<BTreeSet<_>>();
    let settings = object([
        ("instruction_limit", 128u64.into()),
        ("code_scan_limit", (4 * 1024 * 1024u64).into()),
        ("ast_vocabulary", "machine-state-tree-v2".into()),
        ("cfg_policy", "instruction-level-conservative".into()),
        ("source_export", false.into()),
        ("machine_contract", executable::CONTRACT.into()),
        (
            "source_contract",
            "apple-aapcs64-lp64-pure-acyclic-v1".into(),
        ),
        (
            "signature_candidates",
            array(signatures.iter().map(|(name, signature)| {
                object([
                    ("name", name.clone().into()),
                    ("language", signature.language.into()),
                    ("declaration", signature.declaration.clone().into()),
                    ("source", signature.source.into()),
                ])
            })),
        ),
        ("call_signature_candidates", array(prototype_candidates.iter()
            .filter(|(target, _, _)| direct_targets.contains(target)).map(|(target, symbol, signature)| object([
            ("target", address(*target)), ("symbol", symbol.clone().into()),
            ("language", signature.language.into()), ("declaration", signature.declaration.clone().into()),
            ("source", signature.source.into()),
        ])))),
    ]);
    let mut builder = Builder::new(source, start, end, architecture, name, settings.clone())?;
    let scope = builder.record(
        "metadata",
        "fact",
        object([("analysis_settings", settings)]),
        array([]),
    )?;
    let bounds = builder.record(
        "hypothesis",
        "derived",
        object([
            (
                "boundary_sources",
                array(boundary_sources.into_iter().map(V::from)),
            ),
            (
                "interpretation",
                "admitted worker function range; not source-equivalence proof".into(),
            ),
        ]),
        ranges(start, end),
    )?;
    builder.edge(&scope, &bounds, "supports")?;

    // Target-file metadata evidence. These records come from validated static
    // Mach-O tables; they are not observations from a live target runtime.
    let mut native_symbol_ids: Vec<String> = Vec::new();
    let mut native_type_ids: Vec<String> = Vec::new();

    // Exact nlist symbol evidence.
    for (symbol_address, name) in &native.symbols {
        if *symbol_address >= start && *symbol_address < end {
            let id = builder.record(
                "symbol",
                "fact",
                object([
                    ("name", name.clone().into()),
                    ("address", address(*symbol_address)),
                    ("source", "macho-nlist".into()),
                    ("image_origin", "captured-binary".into()),
                ]),
                ranges(*symbol_address, if is_arm64 { symbol_address.saturating_add(super::ARM64_INSN_BYTES) } else { *symbol_address }),
            )?;
            native_symbol_ids.push(id);
        }
    }

    // Only attach an Objective-C layout when this function is a validated IMP
    // in that class. The class object is the exact root metadata record; do not
    // attach the unrelated function's code span or every image class.
    for layout in native.objc_layouts.iter().filter(|layout| {
        layout.methods.iter().any(|method| method.implementation == start)
    }) {
        let id = builder.record(
            "type",
            "fact",
            object([
                ("kind", "objc-class-layout".into()),
                ("class_name", layout.name.clone().into()),
                ("class_address", address(layout.address)),
                ("instance_size", u64::from(layout.instance_size).into()),
                ("ivars", array(layout.ivars.iter().map(|ivar| object([
                    ("name", ivar.name.clone().into()),
                    ("encoding", ivar.encoding.clone().into()),
                    ("offset", u64::from(ivar.offset).into()),
                    ("size", u64::from(ivar.size).into()),
                    ("alignment", u64::from(ivar.alignment).into()),
                    ("offset_pointer", address(ivar.offset_pointer)),
                ])))),
                ("properties", array(layout.properties.iter().map(|prop| object([
                    ("name", prop.name.clone().into()),
                    ("attributes", prop.attributes.clone().into()),
                    ("encoding", prop.encoding.clone().into()),
                    ("getter", prop.getter.clone().into()),
                    ("setter", prop.setter.clone().map(V::from).unwrap_or(V::Null)),
                    ("ivar", prop.ivar.clone().map(V::from).unwrap_or(V::Null)),
                    ("readonly", prop.readonly.into()),
                    ("nonatomic", prop.nonatomic.into()),
                ])))),
                ("source", if layout.runtime { "libobjc-runtime" } else { "macho-objc-metadata" }.into()),
                ("image_origin", "captured-binary".into()),
            ]),
            ranges(layout.address, layout.address.saturating_add(40)),
        )?;
        native_type_ids.push(id);
    }

    // Objective-C IMP names derived from validated class/method metadata.
    let runtime_imp = |address: u64| native.objc_layouts.iter()
        .any(|layout| layout.runtime && layout.methods.iter().any(|method| method.implementation == address));
    for (symbol_address, name) in &native.objc_function_names {
        if *symbol_address >= start && *symbol_address < end {
            let id = builder.record(
                "symbol",
                "fact",
                object([
                    ("name", name.clone().into()),
                    ("address", address(*symbol_address)),
                    ("source", if runtime_imp(*symbol_address) { "libobjc-runtime-imp" } else { "macho-objc-imp" }.into()),
                    ("image_origin", "captured-binary".into()),
                ]),
                ranges(*symbol_address, if is_arm64 { symbol_address.saturating_add(super::ARM64_INSN_BYTES) } else { *symbol_address }),
            )?;
            native_symbol_ids.push(id);
        }
    }

    // Export names are already resolved to admitted function addresses. Keep
    // this function's exact name only; the entire image trie is not a local
    // source span and does not belong in every function artifact.
    if let Some(name) = native.export_names.get(&start) {
        let id = builder.record(
            "symbol",
            "fact",
            object([
                ("name", name.clone().into()),
                ("address", address(start)),
                ("source", "dyld-export-trie".into()),
                ("image_origin", "captured-binary".into()),
            ]),
            ranges(start, start.saturating_add(4)),
        )?;
        native_symbol_ids.push(id);
    }

    // LC_FUNCTION_STARTS is a function-entry fact, not a span covering this
    // function's executable body.
    if native.lc_function_starts.contains(&start) {
        let id = builder.record(
            "metadata",
            "fact",
            object([
                ("kind", "lc-function-starts".into()),
                ("address", address(start)),
                ("source", "macho-lc-function-starts".into()),
                ("image_origin", "captured-binary".into()),
            ]),
            array(Vec::<V>::new()),
        )?;
        native_symbol_ids.push(id);
    }

    // Publish only exact export/unwind boundary evidence for this function.
    // These addresses identify metadata claims, not byte spans, so their
    // evidence ranges are intentionally empty rather than borrowing the code
    // range as a misleading source location.
    for (kind, source, addresses) in [
        ("export-function-start", "macho-export-trie", &native.export_starts),
        ("unwind-function-start", "macho-unwind-info", &native.unwind_starts),
    ] {
        if addresses.contains(&start) {
            let id = builder.record(
                "metadata",
                "fact",
                object([
                    ("kind", kind.into()),
                    ("address", address(start)),
                    ("source", source.into()),
                    ("image_origin", "captured-binary".into()),
                ]),
                array(Vec::<V>::new()),
            )?;
            native_symbol_ids.push(id);
        }
    }

    // Connect native evidence to the bounds hypothesis.
    for sym_id in &native_symbol_ids {
        builder.edge(&bounds, sym_id, "supports")?;
    }
    for type_id in &native_type_ids {
        builder.edge(&bounds, type_id, "supports")?;
    }

    if is_x86_64 {
        // Bounded, sound x86_64/x86-64 support: iced-x86 instruction boundaries/PCs/sizes are decoded
        // and partially lifted into shared IR/AST evidence for a conservative subset. Full lifting into
        // the shared typed semantic IR/AST/source pipeline, CFG/SSA/dataflow/source recovery, and ABI
        // adaptation remain incomplete; artifacts stay blocked and non-equivalence/non-export.
        let mut rendering = String::from("/* x86_64 instruction boundaries decoded (iced-x86) with partial conservative lifting. Full shared semantic/source recovery and ABI adaptation not implemented; review-only, no source/export/equivalence claim. */\n");
        let mut statements: Vec<String> = Vec::new();
        let mut models: Vec<IrOperation> = Vec::new();
        let mut ir_ids: Vec<String> = Vec::new();
        let mut cond_branch_targets: Vec<Option<u64>> = vec![None; insn_pcs.len()];
        for (index, &pc) in insn_pcs.iter().enumerate() {
            let sz = *insn_sizes.get(index).unwrap_or(&0) as usize;
            let pc_end = pc.saturating_add(sz as u64);
            let Some(off) = pc.checked_sub(base).and_then(|d| usize::try_from(d).ok()) else { continue; };
            let bytes = code.get(off..off.saturating_add(sz).min(code.len())).unwrap_or(&[]);
            let (op, _node, ir_text, cond_target) = x86_64_lift_instruction(pc, bytes);
            cond_branch_targets[index] = cond_target;
            let evidence = ir_text.clone();
            let ty = match op {
                IrOperation::Nop | IrOperation::Branch { .. } | IrOperation::DirectCall { .. } | IrOperation::Return { .. } => "x86_64 partially lifted (conservative)",
                IrOperation::Load { .. } | IrOperation::Store { .. } => "x86_64 partially lifted memory/stack op (conservative)",
                IrOperation::Assign { .. } | IrOperation::Binary { .. } => "x86_64 partially lifted data op (conservative)",
                _ if cond_target.is_some() => "x86_64 conditional branch (condition unmodeled, CFG target evidenced)",
                _ => "x86_64 instruction boundary decoded; lifting incomplete",
            };
            let kind = match op {
                IrOperation::Nop => "no-op",
                IrOperation::Return { .. } => "return",
                IrOperation::Branch { .. } => "goto",
                IrOperation::DirectCall { .. } => "direct-call",
                _ if cond_target.is_some() => "conditional-goto",
                _ => "unsupported-operation",
            };
            let ast_id = builder.ast(
                kind,
                Some(ty),
                vec![],
                &evidence,
                (pc, pc_end),
                object([("architecture", "x86_64".into()), ("size", (sz as u64).into()), ("status", match op { IrOperation::TargetSpecific if cond_target.is_none() => "unsupported".into(), IrOperation::TargetSpecific => "partial-cfg".into(), _ => "partial-lift".into() }), ("ir_text", evidence.clone().into())]),
            )?;
            let ir = builder.record(
                "ir",
                "derived",
                object([
                    ("artifact_kind", "instruction-ir".into()),
                    ("pc", address(pc)),
                    ("size", (sz as u64).into()),
                    ("architecture", "x86_64".into()),
                    ("portable_op", format!("{op:?}").into()),
                    ("ir_text", evidence.clone().into()),
                    ("shared_ir_available", (!matches!(op, IrOperation::TargetSpecific)).into()),
                ]),
                ranges(pc, pc_end),
            )?;
            builder.edge(&ir, &ast_id, "renders-as")?;
            statements.push(ast_id);
            ir_ids.push(ir);
            models.push(op);
            rendering.push_str(&format!("  {pc:#018x}: {evidence}\n"));
        }
        // Conservative CFG from partially lifted control-flow ops and evidenced Jcc targets (condition unmodeled).
        let count = insn_pcs.len();
        let idx_of_pc = |target: u64| -> Option<usize> { insn_pcs.binary_search(&target).ok() };
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
        for i in 0..count {
            if let Some(target) = cond_branch_targets[i] {
                if let Some(ti) = idx_of_pc(target) { successors[i].push(ti); }
                if i + 1 < count { successors[i].push(i + 1); }
            } else {
                match models.get(i).copied().unwrap_or(IrOperation::TargetSpecific) {
                    IrOperation::Branch { target } => {
                        if let Some(ti) = idx_of_pc(target) { successors[i].push(ti); }
                    }
                    IrOperation::Return { .. } => {}
                    IrOperation::DirectCall { .. } | IrOperation::Nop | IrOperation::Assign { .. } | IrOperation::Binary { .. } | IrOperation::Load { .. } | IrOperation::Store { .. } => {
                        if i + 1 < count { successors[i].push(i + 1); }
                    }
                    IrOperation::ConditionalBranch { target, .. } => {
                        if let Some(ti) = idx_of_pc(target) { successors[i].push(ti); }
                        if i + 1 < count { successors[i].push(i + 1); }
                    }
                    _ => {
                        // Unknown/target-specific: conservatively assume fallthrough only when within decoded stream.
                        if i + 1 < count { successors[i].push(i + 1); }
                    }
                }
            }
            successors[i].sort_unstable();
            successors[i].dedup();
        }
        let mut block_ids: Vec<String> = Vec::new();
        for i in 0..count {
            let pc = insn_pcs[i];
            let pc_end = pc.saturating_add(*insn_sizes.get(i).unwrap_or(&0) as u64);
            let succ_addrs: Vec<V> = successors[i].iter().map(|&s| address(insn_pcs[s])).collect();
            let bid = builder.record(
                "cfg-block",
                "derived",
                object([
                    ("artifact_kind", "cfg-block".into()),
                    ("start", address(pc)),
                    ("end", address(pc_end)),
                    ("successors", array(succ_addrs)),
                    ("architecture", "x86_64".into()),
                ]),
                ranges(pc, pc_end),
            )?;
            if let Some(ir) = ir_ids.get(i) { builder.edge(ir, &bid, "in-block")?; }
            block_ids.push(bid);
        }
        for (i, succs) in successors.iter().enumerate() {
            for &s in succs {
                if let (Some(from), Some(to)) = (block_ids.get(i), block_ids.get(s)) {
                    builder.edge(from, to, "cfg-edge")?;
                }
            }
        }
        let root = builder.ast(
            "function",
            name,
            statements,
            &rendering,
            (start, stream_end.max(start)),
            object([("architecture", "x86_64".into()), ("lifting", "partial-conservative".into()), ("contract", "iced-x86 boundaries/sizes decoded; partial IR/AST lifting and conservative CFG only; full semantic/source recovery and ABI adaptation not implemented".into())]),
        )?;
        let root_mirror = builder.record(
            "ast-node",
            "derived",
            object([("ast_id", root.clone().into()), ("kind", "function".into()), ("architecture", "x86_64".into())]),
            ranges(start, stream_end.max(start)),
        )?;
        builder.edge(&bounds, &root_mirror, "supports")?;
        let blocker = builder.record(
            "unsupported-effect",
            "derived",
            object([
                ("artifact_kind", "unsupported-architecture-lifting".into()),
                ("architecture", "x86_64".into()),
                ("reason", "x86_64 instruction boundaries/PC-size maps decoded (iced-x86) with partial conservative lifting and CFG scaffolding, but full lifting into shared typed semantic IR/AST/source pipeline, source recovery, and ABI adaptation are not implemented".into()),
                ("conservative", true.into()),
            ]),
            ranges(start, stream_end.max(start)),
        )?;
        builder.edge(&root_mirror, &blocker, "supports")?;
        if registers.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Strict interpreter blocked: x86_64 lifting/ABI adaptation not implemented".to_string(),
            ));
        }
        return Ok(builder.finish(root, rendering, instructions, truncated, exhausted).json());
    }

    let mut lifted = Vec::new();
    let mut models = Vec::new();
    let mut ir_ids = Vec::new();
    let mut statements = Vec::new();
    let mut statement_mirrors = Vec::new();
    let mut trees = Vec::new();
    let mut strict_blocked = truncated || exhausted;
    let mut review_blocked_pcs = std::collections::BTreeSet::new();
    let mut source_blocked = truncated || exhausted;
    let mut shared_ir_count = 0u64;
    let mut modeled_tree_count = 0u64;
    let mut data_in_code_count = 0u64;
    let mut unsupported_reason_count = 0u64;
    // A single exact source declaration proves this function's own prototype,
    // which also proves the prototype of a self-recursive call target.
    let self_prototype = if signatures.len() == 1 {
        let (name, signature) = &signatures[0];
        crate::signatures::source_signature_with_project_types(name, signature, &project_types)
    } else {
        None
    };
    let mut prototypes_by_address = BTreeMap::<u64, Vec<(String, crate::signatures::Signature)>>::new();
    for (target, symbol, prototype) in prototype_candidates {
        if matches!(prototype.language, "C" | "Objective-C / Objective-C++") {
            prototypes_by_address.entry(target).or_default().push((symbol, prototype));
        }
    }
    // Direct calls to objc_msgSend$selector stubs have a recoverable message-send
    // contract when the selector maps to a unique validated Objective-C method
    // encoding in this image's metadata. The callee's runtime side effects remain
    // unproven; only the ABI argument/result locations are evidenced.
    let mut call_target_names = native.symbols.iter().cloned()
        .chain(native.objc_function_names.iter().map(|(a,n)|(*a,n.clone())))
        .chain(native.export_names.iter().map(|(a,n)|(*a,n.clone())))
        .chain(native.stub_symbols.iter().map(|(a,n)|(*a,n.clone())))
        .collect::<Vec<_>>();
    call_target_names.sort();
    call_target_names.dedup();
    let call_target_name_map = call_target_names.iter().cloned().collect::<BTreeMap<u64,String>>();
    let objc_selector_encodings = {
        let mut map: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
        for line in native.objc_records.lines().filter(|line| line.contains(" method ") && line.contains("confidence=validated")) {
            let Some(name) = line.split(" name=").nth(1).and_then(|tail| tail.split("  types=").next()) else { continue };
            let Some(encoding) = line.split("  types=").nth(1).and_then(|tail| tail.split("  imp=").next()) else { continue };
            if name.is_empty() || encoding.is_empty() { continue; }
            map.entry(name.trim().to_owned()).or_default().insert(encoding.trim().to_owned());
        }
        map
    };
    for (target, symbol) in &call_target_name_map {
        let bare = symbol.strip_prefix('_').unwrap_or(symbol);
        let selector = bare.strip_prefix("objc_msgSend$").or_else(|| bare.strip_prefix("objc_msgSendSuper2$")).or_else(|| bare.strip_prefix("objc_msgSendSuper$"));
        if let Some(selector) = selector {
            if let Some(encodings) = objc_selector_encodings.get(selector) {
                if encodings.len() == 1 {
                    let encoding = encodings.iter().next().unwrap();
                    let synthetic = format!("-[__hs_objc_msgsend__ {selector}]");
                    if let Some(signature) = crate::signatures::objc_method(&synthetic, encoding) {
                        prototypes_by_address.entry(*target).or_default().push((symbol.clone(), signature));
                    }
                }
            }
        }
    }
    if signatures.len() == 1 {
        let (symbol, prototype) = &signatures[0];
        prototypes_by_address.entry(start).or_default().push((symbol.clone(), prototype.clone()));
    }
    // Known runtime helper prototypes for resolved GOT/import calls.
    let known_runtime_call_proto = |name: &str| -> Option<crate::signatures::Signature> {
        let bare = name.strip_prefix('_').unwrap_or(name);
        let decl = match bare {
            "objc_setProperty_nonatomic_copy" => "void objc_setProperty_nonatomic_copy(id self, SEL _cmd, id newValue, ptrdiff_t offset);",
            "objc_setProperty_atomic_copy" => "void objc_setProperty_atomic_copy(id self, SEL _cmd, id newValue, ptrdiff_t offset);",
            "objc_setProperty_nonatomic" => "void objc_setProperty_nonatomic(id self, SEL _cmd, id newValue, ptrdiff_t offset);",
            "objc_setProperty_atomic" => "void objc_setProperty_atomic(id self, SEL _cmd, id newValue, ptrdiff_t offset);",
            "objc_storeStrong" => "void objc_storeStrong(id *object, id value);",
            "objc_storeWeak" => "id objc_storeWeak(id *object, id value);",
            "objc_loadWeakRetained" => "id objc_loadWeakRetained(id *object);",
            "objc_destroyWeak" => "void objc_destroyWeak(id *object);",
            "objc_copyWeak" => "void objc_copyWeak(id *dest, id *src);",
            "objc_initWeak" => "id objc_initWeak(id *object, id value);",
            "objc_moveWeak" => "void objc_moveWeak(id *dest, id *src);",
            "objc_retain" => "id objc_retain(id value);",
            "objc_release" => "void objc_release(id value);",
            "objc_autorelease" => "id objc_autorelease(id value);",
            "objc_retainAutorelease" => "id objc_retainAutorelease(id value);",
            "objc_autoreleaseReturnValue" => "id objc_autoreleaseReturnValue(id value);",
            "objc_retainAutoreleasedReturnValue" => "id objc_retainAutoreleasedReturnValue(id value);",
            "objc_retainAutoreleaseReturnValue" => "id objc_retainAutoreleaseReturnValue(id value);",
            "objc_unsafeClaimAutoreleasedReturnValue" => "id objc_unsafeClaimAutoreleasedReturnValue(id value);",
            "objc_opt_new" => "id objc_opt_new(id cls);",
            "objc_opt_class" => "id objc_opt_class(id obj);",
            "objc_opt_self" => "id objc_opt_self(id obj);",
            "objc_opt_isKindOfClass" => "bool objc_opt_isKindOfClass(id obj, Class cls);",
            "objc_opt_respondsToSelector" => "bool objc_opt_respondsToSelector(id obj, SEL sel);",
            "objc_alloc" => "id objc_alloc(Class cls);",
            "objc_alloc_init" => "id objc_alloc_init(Class cls);",
            "objc_autoreleasePoolPush" => "void *objc_autoreleasePoolPush(void);",
            "objc_autoreleasePoolPop" => "void objc_autoreleasePoolPop(void *pool);",
            "objc_msgSendSuper2" => "id objc_msgSendSuper2(void *super, SEL op);",
            "objc_msgSendSuper" => "id objc_msgSendSuper(void *super, SEL op);",
            "objc_msgSendSuper2_stret" => "void objc_msgSendSuper2_stret(void *ret, void *super, SEL op);",
            "objc_msgSendSuper_stret" => "void objc_msgSendSuper_stret(void *ret, void *super, SEL op);",
            "__stack_chk_fail" => "void __stack_chk_fail(void);",
            "abort" => "void abort(void);",
            "_exit" => "void _exit(int status);",
            "exit" => "void exit(int status);",
            "memcpy" => "void *memcpy(void *restrict dst, const void *restrict src, size_t n);",
            "memmove" => "void *memmove(void *dst, const void *src, size_t n);",
            "memset" => "void *memset(void *dst, int c, size_t n);",
            "bzero" => "void bzero(void *s, size_t n);",
            "memcmp" => "int memcmp(const void *s1, const void *s2, size_t n);",
            "memset_pattern4" => "void memset_pattern4(void *b, const void *pattern4, size_t len);",
            "memset_pattern8" => "void memset_pattern8(void *b, const void *pattern8, size_t len);",
            "memset_pattern16" => "void memset_pattern16(void *b, const void *pattern16, size_t len);",
            "__memcpy_chk" => "void *__memcpy_chk(void *dest, const void *src, size_t len, size_t destlen);",
            "__memmove_chk" => "void *__memmove_chk(void *dest, const void *src, size_t len, size_t destlen);",
            "__memset_chk" => "void *__memset_chk(void *dest, int val, size_t len, size_t destlen);",
            "__strcpy_chk" => "char *__strcpy_chk(char *dest, const char *src, size_t destlen);",
            "__strncpy_chk" => "char *__strncpy_chk(char *dest, const char *src, size_t len, size_t destlen);",
            "__strcat_chk" => "char *__strcat_chk(char *dest, const char *src, size_t destlen);",
            "malloc" => "void *malloc(size_t size);",
            "calloc" => "void *calloc(size_t count, size_t size);",
            "realloc" => "void *realloc(void *ptr, size_t size);",
            "free" => "void free(void *ptr);",
            "fwrite" => "size_t fwrite(const void *ptr, size_t size, size_t nmemb, void *stream);",
            "fread" => "size_t fread(void *ptr, size_t size, size_t nmemb, void *stream);",
            "strlen" => "size_t strlen(const char *s);",
            "strcmp" => "int strcmp(const char *s1, const char *s2);",
            "strncmp" => "int strncmp(const char *s1, const char *s2, size_t n);",
            "strcpy" => "char *strcpy(char *dst, const char *src);",
            "strncpy" => "char *strncpy(char *dst, const char *src, size_t n);",
            "printf" => "int printf(const char *format, ...);",
            "puts" => "int puts(const char *s);",
            _ => return None,
        };
        Some(crate::signatures::Signature { language: "C", declaration: decl.to_owned(), source: "known Objective-C runtime ABI contract" })
    };
    let mut call_prototypes = BTreeMap::<u64, crate::signatures::SourceSignature>::new();
    for (target, mut candidates) in prototypes_by_address {
        candidates.sort_by(|left, right| (&left.0, &left.1.declaration).cmp(&(&right.0, &right.1.declaration)));
        candidates.dedup_by(|left, right| left.0 == right.0 && left.1.declaration == right.1.declaration);
        if candidates.len() == 1 {
            let (symbol, prototype) = &candidates[0];
            if let Some(prototype) = crate::signatures::source_signature_with_project_types(symbol, prototype, &project_types) {
                call_prototypes.insert(target, prototype);
            }
        }
    }
    // Apply known ABI contracts to direct call targets with evidenced symbol names
    // (e.g. runtime helpers, libc imports/stubs) when no unique prototype was already established.
    for (target, symbol) in &call_target_name_map {
        if call_prototypes.contains_key(target) { continue; }
        if let Some(sig) = known_runtime_call_proto(symbol) {
            if let Some(proto) = crate::signatures::source_signature_with_project_types(symbol, &sig, &project_types) {
                call_prototypes.insert(*target, proto);
            }
        }
    }
    for (target, symbol) in &native.got_symbols {
        if call_prototypes.contains_key(target) { continue; }
        if let Some(sig) = known_runtime_call_proto(symbol) {
            if let Some(proto) = crate::signatures::source_signature_with_project_types(symbol, &sig, &project_types) {
                call_prototypes.insert(*target, proto);
            }
        }
    }
    let mut resolved_got_load_reg: BTreeMap<u64, (u8, String, Option<u64>)> = BTreeMap::new(); // pc -> (reg, symbol_or_PAGE, slot_addr)
    // Use the stream-derived PC/size maps. For ARM64 streams without paired sizes, fall back to 4-byte sizes explicitly.
    let insn_sizes: Vec<u32> = if insn_sizes.len() == insn_pcs.len() {
        insn_sizes
    } else if is_arm64 {
        vec![u32::try_from(super::ARM64_INSN_BYTES).unwrap_or(4); insn_pcs.len()]
    } else {
        insn_sizes
    };

    let mut rendering=String::from("/* Review-only machine-state AST; declarations, runtime contracts and source result ABI unresolved. */\n");
    for index in 0..insn_pcs.len() {
        let pc = apc_at_with_arch(index, &insn_pcs, start, &arch);
        let size_u32 = if insn_sizes.len() == insn_pcs.len() {
            insn_sizes.get(index).copied().unwrap_or(0)
        } else if is_arm64 {
            u32::try_from(super::ARM64_INSN_BYTES).unwrap_or(4)
        } else {
            0
        };
        if size_u32 == 0 { break; }
        let size = size_u32 as usize;
        let Some(off) = pc.checked_sub(base).and_then(|d| usize::try_from(d).ok()) else { break; };
        if off + size > code.len() { break; }
        let bytes = &code[off..off + size];
        let pc_end = aspan_end_pc_with_sizes_and_arch(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end, &arch);
        let position = file_offset + (pc - base);
        let data = data_ranges
            .iter()
            .any(|&(lo, hi, _)| position < hi && position + (pc_end.saturating_sub(pc)) > lo);
        // Architecture-gated decode/lift: ARM64 4-byte semantic decode/lift supported today; other sizes/architectures remain conservative.
        let (word, operation, mut portable) = if data {
            (0u32, Operation::Opaque, IrOperation::TargetSpecific)
        } else if let Some((w, op, p)) = super::decode_lift_for_arch(code, base, pc, size_u32, &arch) {
            (w, op, p)
        } else if size == 4 && is_arm64 {
            let w = u32::from_le_bytes(bytes.try_into().unwrap_or([0, 0, 0, 0]));
            let op = decode(w, pc);
            (w, op, lift_operation(op, pc))
        } else {
            (0u32, Operation::Opaque, IrOperation::TargetSpecific)
        };
        // Track simple GOT loads: adrp xn, page; ldr xn, [xn, #off] -> slot = page+off (ARM64-only word-pattern evidence)
        if !data && is_arm64 && size == 4 {
            let is_adrp = (word & 0x9f000000) == 0x90000000;
            if is_adrp {
                let rd = (word & 0x1f) as u8;
                let immlo = ((word >> 29) & 0x3) as i64;
                let immhi = ((word >> 5) & 0x7ffff) as i64;
                let mut imm = (immhi << 2) | immlo;
                if imm & (1 << 20) != 0 { imm |= !0i64 << 21; }
                let page = ((pc as i64 & !0xfffi64) + (imm << 12)) as u64;
                resolved_got_load_reg.insert(pc, (rd, format!("PAGE:{page:#x}"), None));
            } else if (word & 0xffc00000) == 0xf9400000 { // LDR (immediate, unsigned offset) Xt
                let rn = ((word >> 5) & 0x1f) as u8;
                let rt = (word & 0x1f) as u8;
                let imm12 = ((word >> 10) & 0xfff) as u64;
                let off = imm12 * 8;
                if index > 0 {
                    let prev_pc = if index > 0 { apc_at_with_arch(index - 1, &insn_pcs, start, &arch) } else { pc };
                    if let Some((pr, info, _)) = resolved_got_load_reg.get(&prev_pc).cloned() {
                        if pr == rn {
                            if let Some(page_hex) = info.strip_prefix("PAGE:") {
                                if let Ok(page) = u64::from_str_radix(page_hex.trim_start_matches("0x"), 16) {
                                    let slot = page.wrapping_add(off);
                                    if let Some(sym) = native.got_symbols.get(&slot).cloned() {
                                        resolved_got_load_reg.insert(pc, (rt, sym, Some(slot)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !data && is_arm64 && size == 4 {
            if let IrOperation::IndirectCall { target: dispatch } = portable {
                let reg_num: Option<u8> = if dispatch.class == IrRegisterClass::General { Some(dispatch.index) } else { None };
                if let Some(reg) = reg_num {
                    if index > 0 {
                        let prev_pc = apc_at_with_arch(index - 1, &insn_pcs, start, &arch);
                        if let Some((r, sym, slot)) = resolved_got_load_reg.get(&prev_pc).cloned() {
                            if r == reg && !sym.starts_with("PAGE:") {
                                if let Some(target_slot) = slot {
                                    if let Some(sig) = known_runtime_call_proto(&sym) {
                                        if let Some(proto) = crate::signatures::source_signature_with_project_types(&sym, &sig, &project_types) {
                                            call_prototypes.entry(target_slot).or_insert(proto);
                                            portable = IrOperation::DirectCall { target: target_slot };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut modeled = instruction_model(operation, portable, word);
        // Bounded: manyFP-specific SIMD/scalar adaptations (evidenced abi_sample case, ARM64).
        if is_arm64 && size == 4 {
        match word {
            0x4ea21c41 => { // mov v1.16b, v2.16b
                let reg_node = |name: &str| tree("fp-register", Some("uint64_t".into()), object([("name", name.into()), ("bits", 64u64.into())]), vec![]);
                // Copy low scalar view used by subsequent sxtl/scvtf chain in this context.
                modeled = Some(tree("assignment", None, object([("write_policy","fp-scalar-write".into())]), vec![reg_node("d1"), reg_node("d2")]));
            }
            0x0f20a421 => { // sxtl v1.2d, v1.2s (bounded: lane 0 sign-extend s1->d1)
                let src = tree("fp-register", Some("uint64_t".into()), object([("name", "s1".into()), ("bits", 32u64.into())]), vec![]);
                let dst = tree("fp-register", Some("uint64_t".into()), object([("name", "d1".into()), ("bits", 64u64.into())]), vec![]);
                let expr = tree("fp-sxtl-scalar", Some("uint64_t".into()), object([("from_bits", 32u64.into()), ("to_bits", 64u64.into())]), vec![src]);
                modeled = Some(tree("assignment", None, object([("write_policy","fp-scalar-write".into())]), vec![dst, expr]));
            }
            0x5e61d821 => { // scvtf d1, d1 (signed int64 in d1 to double d1)
                let src = tree("fp-register", Some("uint64_t".into()), object([("name", "d1".into()), ("bits", 64u64.into())]), vec![]);
                let dst = tree("fp-register", Some("double".into()), object([("name", "d1".into()), ("bits", 64u64.into())]), vec![]);
                let expr = tree("fp-int-to-float", Some("double".into()), object([("signed", true.into()), ("from_bits", 64u64.into()), ("to_bits", 64u64.into())]), vec![src]);
                modeled = Some(tree("assignment", None, object([("write_policy","fp-scalar-write".into())]), vec![dst, expr]));
            }
            _ => {}
        }
        }
        // A complete, proven jump-table resolves an indirect branch's in-function
        // successors (or proves no in-function re-entry). Model it explicitly so
        // the artifact does not report the dispatch as an unadapted effect.
        // Single-target complete dispatch is equivalent to a direct goto for
        // recovery purposes; multi-way and exit-only dispatch remain explicitly
        // structured as indirect-goto and are source-gated separately.
        if modeled.is_none() {
            if let IrOperation::IndirectBranch { target } = portable {
                if let Some(targets) = resolved_indirect_targets.get(&pc) {
                    if targets.len() == 1 {
                        let case_map = resolved_indirect_cases.get(&pc).map(|cases| {
                            array(cases.iter().filter(|(_, t)| targets.contains(t)).map(|(case_index, target_addr)| {
                                object([
                                    ("case", (*case_index).into()),
                                    ("target", address(*target_addr)),
                                ])
                            }))
                        });
                        let mut attrs = object([
                            ("target", address(targets[0])),
                            ("dispatch", target.target().name().into()),
                            ("resolved_indirect_single_target", true.into()),
                        ]);
                        if let V::Object(ref mut fields) = attrs {
                            if let Some(case_map) = case_map {
                                fields.insert("case_map".into(), case_map);
                            }
                            if let Some(info) = resolved_indirect_index_info.get(&pc) {
                                fields.insert("index_info".into(), info.clone().into());
                                if let Some(external) = info
                                    .split_whitespace()
                                    .find_map(|token| token.strip_prefix("external-cases="))
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    fields.insert("external_exit_cases".into(), external.into());
                                }
                            }
                        }
                        modeled = Some(tree(
                            "goto",
                            None,
                            attrs,
                            vec![reg(target)],
                        ));
                    } else {
                        let case_map = resolved_indirect_cases.get(&pc).map(|cases| {
                            array(cases.iter().filter(|(_, t)| targets.contains(t)).map(|(case_index, target_addr)| {
                                object([
                                    ("case", (*case_index).into()),
                                    ("target", address(*target_addr)),
                                ])
                            }))
                        });
                        let mut attrs = object([
                            ("dispatch", target.target().name().into()),
                            ("resolved_targets_complete", true.into()),
                            ("targets", array(targets.iter().map(|target| address(*target)))),
                        ]);
                        if let V::Object(ref mut fields) = attrs {
                            if let Some(case_map) = case_map {
                                fields.insert("case_map".into(), case_map);
                            }
                            if let Some(info) = resolved_indirect_index_info.get(&pc) {
                                fields.insert("index_info".into(), info.clone().into());
                                if let Some(external) = info
                                    .split_whitespace()
                                    .find_map(|token| token.strip_prefix("external-cases="))
                                    .and_then(|s| s.parse::<u64>().ok())
                                {
                                    fields.insert("external_exit_cases".into(), external.into());
                                }
                            }
                        }
                        modeled = Some(tree(
                            "indirect-goto",
                            None,
                            attrs,
                            vec![reg(target)],
                        ));
                    }
                }
            }
        }
        shared_ir_count += u64::from(!matches!(portable, IrOperation::TargetSpecific));
        modeled_tree_count += u64::from(modeled.is_some());
        data_in_code_count += u64::from(data);
        let raw = builder.record(
            "bytes",
            "fact",
            object([
                (
                    "hex",
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                        .into(),
                ),
                ("file_offset", address(position)),
            ]),
            ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
        )?;
        let decoded = builder.record(
            "decode",
            "derived",
            object([
                ("word", format!("0x{word:08x}").into()),
                ("data_in_code", data.into()),
                (
                    "shared_ir_available",
                    (!matches!(portable, IrOperation::TargetSpecific)).into(),
                ),
            ]),
            ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
        )?;
        builder.edge(&raw, &decoded, "decodes-to")?;
        let effects = portable.gpr_effects();
        let ir = builder.record(
            "ir-operation",
            "derived",
            object([
                (
                    "tree",
                    modeled.as_ref().map(Tree::structured).unwrap_or(V::Null),
                ),
                (
                    "gpr_definitions",
                    array(
                        registers_in(effects.definitions)
                            .map(|register| u64::from(register).into()),
                    ),
                ),
                (
                    "gpr_uses",
                    array(registers_in(effects.uses).map(|register| u64::from(register).into())),
                ),
                ("barrier", effects.barrier.into()),
            ]),
            ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
        )?;
        builder.edge(&decoded, &ir, "lifts-to")?;
        let mut reasons = Vec::new();
        let resolved_indirect_branch_needs_source_structure = match portable {
            IrOperation::IndirectBranch { .. } => resolved_indirect_targets
                .get(&pc)
                .is_some_and(|targets| targets.is_empty()),
            _ => false,
        };
        if modeled.is_none() {
            reasons.push(if data {
                "data-in-code"
            } else {
                "unadapted-or-unknown-effect"
            });
        }
        let callee_name = match portable {
            IrOperation::DirectCall { target } => call_target_name_map.get(&target).map(String::as_str)
                .or_else(|| native.got_symbols.get(&target).map(String::as_str))
                .or_else(|| native.symbols.iter().find(|(a,_)| *a == target).map(|(_,n)| n.as_str())),
            _ => None,
        };
        let is_objc_msgsend_stub = callee_name.is_some_and(|name| {
            let bare = name.strip_prefix('_').unwrap_or(name);
            bare.starts_with("objc_msgSend$") || bare.starts_with("objc_msgSendSuper$") || bare.starts_with("objc_msgSendSuper2$")
        });
        let is_known_runtime_helper = callee_name.is_some_and(|name| known_runtime_call_proto(name).is_some());
        let source_supported_call = modeled.is_some() && !data && match portable {
            IrOperation::DirectCall { target } if target == start => self_prototype.is_some(),
            IrOperation::DirectCall { target } => {
                call_prototypes.contains_key(&target)
                    && (is_objc_msgsend_stub || is_known_runtime_helper || calls::analyze_callee_with_resolved_indirects_and_arch(code, base, target, CALLEE_WINDOW, &resolved_indirect_targets, &arch).is_some_and(|summary| {
                        let callee_saved_gprs = 0x7ff8_0000; // x19 through x30
                        let callee_saved_vectors = 0x0000_ff00; // low 64-bit v8 through v15
                        summary.complete
                            && summary.returns
                            && !summary.calls
                            && !summary.loops
                            && !summary.opaque
                            && !summary.truncated
                            && !summary.escaped
                            && !summary.reads_memory
                            && !summary.writes_memory
                            && summary.defined_gprs & callee_saved_gprs == 0
                            && summary.defined_vectors & callee_saved_vectors == 0
                    }))
            }
            _ => false,
        };
        // Tail branch to known return-forwarding runtime helper: make its
        // contract available for bounded source recovery (e.g. autoreleaseReturnValue).
        if let IrOperation::Branch { target } = portable {
            if target < start || target >= end {
                if let Some(name) = call_target_name_map.get(&target).map(String::as_str)
                    .or_else(|| native.got_symbols.get(&target).map(String::as_str))
                    .or_else(|| native.symbols.iter().find(|(a,_)| *a == target).map(|(_,n)| n.as_str())) {
                    if let Some(sig) = known_runtime_call_proto(name) {
                        if let Some(proto) = crate::signatures::source_signature_with_project_types(name, &sig, &project_types) {
                            call_prototypes.entry(target).or_insert(proto);
                        }
                    }
                }
            }
        }
        // Direct calls to known runtime helpers/stubs: make exact prototype
        // contracts available for bounded source recovery when proven by name.
        if let IrOperation::DirectCall { target } = portable {
            if let Some(name) = callee_name {
                if let Some(sig) = known_runtime_call_proto(name) {
                    if let Some(proto) = crate::signatures::source_signature_with_project_types(name, &sig, &project_types) {
                        call_prototypes.entry(target).or_insert(proto);
                    }
                }
            }
        }
        if matches!(portable, IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. }) {
            let contract_resolved = match portable {
                IrOperation::DirectCall { target } => call_prototypes.contains_key(&target),
                IrOperation::IndirectCall { .. } => false,
                _ => false,
            };
            if !contract_resolved {
                reasons.push("unresolved-call-contract");
            }
        }
        if matches!(portable, IrOperation::Marker { .. }) {
            reasons.push("runtime-marker-contract-unqualified");
        }
        let fpcr_fpsr = has_unmodeled_fp_status_effect(portable);
        if fpcr_fpsr {
            reasons.push("FPCR/FPSR effects are not modeled");
        }
        if !reasons.is_empty(){review_blocked_pcs.insert(pc);}
        // Bounded: allow modeled scalar FP data ops for source recovery despite unresolved FPCR/FPSR.
        let source_fpcr_exempt = fpcr_fpsr && modeled.is_some() && matches!(portable,
            IrOperation::ScalarFloatBinary { operator: IrFloatBinaryOperator::Add | IrFloatBinaryOperator::Subtract | IrFloatBinaryOperator::Multiply | IrFloatBinaryOperator::Divide, .. }
        );
        let special_manyfp = matches!(word, 0x4ea21c41 | 0x0f20a421 | 0x5e61d821);
        let mut source_reasons = reasons.clone();
        if source_fpcr_exempt {
            source_reasons.retain(|r| *r != "FPCR/FPSR effects are not modeled");
        }
        if special_manyfp && modeled.is_some() {
            source_reasons.retain(|r| *r != "unadapted-or-unknown-effect" && *r != "FPCR/FPSR effects are not modeled");
        }
        if resolved_indirect_branch_needs_source_structure {
            // The dispatch successors are proven for CFG/effect purposes, but the
            // recovered source form for multi-way or exit-only indirect control
            // flow is not yet structured.
            source_reasons.push("resolved-indirect-dispatch-not-yet-recovered-as-source");
        }
        strict_blocked |= !reasons.is_empty();
        source_blocked |= !source_reasons.is_empty() && !source_supported_call;
        unsupported_reason_count += source_reasons.len() as u64;
        for reason in &source_reasons {
            let unknown = builder.record(
                "unsupported-effect",
                "derived",
                object([
                    ("reason", (*reason).into()),
                    ("word", format!("0x{word:08x}").into()),
                    (
                        "reachable_scope",
                        "conservative whole captured function".into(),
                    ),
                ]),
                ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
            )?;
            builder.edge(&ir, &unknown, "derives")?;
        }
        let tree = modeled.unwrap_or_else(|| {
            tree(
                "unsupported-operation",
                None,
                object([("word", format!("0x{word:08x}").into())]),
                vec![],
            )
        });
        // Direct calls receive ABI locations only from the current exact
        // declaration or a unique target-symbol prototype candidate.
        let tree = if let IrOperation::DirectCall { target } = portable {
            if let Some(prototype) = call_prototypes.get(&target) {
                let mut tree = tree;
                tree.attributes = calls::with_prototype_payload(tree.attributes, prototype);
                tree
            } else {
                tree
            }
        } else {
            tree
        };
        // A direct call into the captured image admits a bounded observation
        // of what the callee does. The observation is capped and explicitly
        // scoped; it never replaces the caller-visible ABI contract.
        let tree = match portable {
            IrOperation::DirectCall { target } => match calls::analyze_callee_with_resolved_indirects_and_arch(code, base, target, CALLEE_WINDOW, &resolved_indirect_targets, &arch) {
                Some(summary) => {
                    let mut tree = tree;
                    tree.attributes = calls::with_callee_payload(tree.attributes, &summary);
                    tree
                }
                None => tree,
            },
            _ => tree,
        };
        // A callee proven never to return makes the instructions after the
        // call unreachable. The proof comes from the callee's own control
        // flow; without it the question stays unresolved.
        let tree = match portable {
            IrOperation::DirectCall { target } => {
                let mut tree = tree;
                tree.attributes = calls::with_non_returning_payload(
                    tree.attributes,
                    calls::prove_non_returning_with_resolved_indirects_and_arch(code, base, target, CALLEE_WINDOW, &resolved_indirect_targets, &arch),
                );
                tree
            }
            _ => tree,
        };
        let mut tree = tree;
        if let IrOperation::DirectCall { target } = portable {
            if let Some(prototype) = call_prototypes.get(&target).filter(|prototype|
                matches!(prototype.result_spelling.as_str(), "bool" | "_Bool")) {
                let upper = native.lc_function_starts.iter().chain(native.export_starts.iter())
                    .chain(native.unwind_starts.iter()).copied().chain(call_prototypes.keys().copied())
                    .filter(|boundary| *boundary > target).min().unwrap_or(base + code.len() as u64)
                    .min(target.saturating_add(CALLEE_WINDOW as u64 * if is_arm64 { super::ARM64_INSN_BYTES } else { 15 }));
                let window = target.checked_sub(base).zip(upper.checked_sub(base))
                    .and_then(|(lo, hi)| code.get(lo as usize..hi as usize));
                let data_free = data_ranges.iter().all(|(lo, hi, _)| {
                    let a = file_offset.saturating_add(target.saturating_sub(base));
                    let b = file_offset.saturating_add(upper.saturating_sub(base));
                    a >= *hi || b <= *lo
                });
                let proven = source_supported_call && target != start && data_free
                    && window.is_some_and(|bytes| source::canonical_boolean_callee_with_pcs_and_arch(bytes, target, prototype, &[], &[], &arch));
                if let V::Object(attributes) = &mut tree.attributes {
                    if let Some(V::Object(effects)) = attributes.get_mut("call_effects") {
                        effects.insert("canonical_boolean_result".into(), proven.into());
                    }
                }
                let proof = builder.record("type", "derived", object([
                    ("artifact_kind", "callee-boolean-result".into()), ("target", address(target)),
                    ("canonical_boolean_proven", proven.into()),
                    ("declaration", prototype.signature.declaration.clone().into()),
                    ("contract", "bounded complete memory-free leaf; source walker proves canonical returns".into()),
                ]), array([]))?;
                builder.edge(&proof, &ir, "supports")?;
            }
        }
        let (ast, mirror) = tree.emit(&mut builder, &ir, pc, pc_end)?;
        statements.push(ast);
        statement_mirrors.push(mirror);
        rendering.push_str(&format!("loc_{pc:x}: {}\n", tree.render()));
        trees.push(tree);
        ir_ids.push(ir);
        models.push(portable);
        lifted.push(LiftedInstruction {
            address: pc,
            word,
            operation,
            portable,
            data,
        });
    }
    let coverage = builder.record(
        "metadata",
        "derived",
        object([
            ("artifact_kind", "semantic-coverage".into()),
            ("scope", "full captured function; one record per decoded instruction".into()),
            ("instruction_count", (lifted.len() as u64).into()),
            ("shared_ir_count", shared_ir_count.into()),
            ("modeled_tree_count", modeled_tree_count.into()),
            ("data_in_code_count", data_in_code_count.into()),
            ("unsupported_effect_reason_count", unsupported_reason_count.into()),
            ("strict_machine_state_blocked", strict_blocked.into()),
            ("interpretation", "availability counts only; modeled trees do not certify complete architectural or source semantics".into()),
        ]),
        ranges(start, end),
    )?;
    builder.edge(&bounds, &coverage, "supports")?;
    // One-instruction CFG blocks are a conservative refinement, not a claim
    // that the presentation's constant-pruned block partition is identical.
    let mut blocks = lifted
        .iter()
        .enumerate()
        .map(|(index, instruction)| {
            let mut successors = Vec::new();
            match instruction.portable {
                IrOperation::Branch { target } | IrOperation::ConditionalBranch { target, .. } => {
                    if let Some(ti) = aindex_of_with_arch(target, &insn_pcs, start, lifted.len(), &arch).filter(|&ti| apc_at_with_arch(ti, &insn_pcs, start, &arch) < high) {
                        successors.push(ti);
                    }
                    if matches!(instruction.portable, IrOperation::ConditionalBranch { .. })
                        && index + 1 < lifted.len()
                    {
                        successors.push(index + 1);
                    }
                }
                IrOperation::Return { .. } => {}
                IrOperation::IndirectBranch { .. } => {
                    if let Some(targets) = resolved_indirect_targets.get(&instruction.address) {
                        for &target in targets {
                            if let Some(ti) = aindex_of_with_arch(target, &insn_pcs, start, lifted.len(), &arch).filter(|&ti| apc_at_with_arch(ti, &insn_pcs, start, &arch) < high) {
                                successors.push(ti);
                            }
                        }
                    }
                }
                _ if index + 1 < lifted.len() => successors.push(index + 1),
                _ => {}
            }
            successors.sort_unstable();
            successors.dedup();
            BasicBlock {
                start: instruction.address,
                instructions: vec![instruction.address],
                successors,
                predecessors: Vec::new(),
                backedges: Vec::new(),
                reachable: false,
                dominators: Vec::new(),
                dominance_frontiers: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    for index in 0..blocks.len() {
        for successor in blocks[index].successors.clone() {
            blocks[successor].predecessors.push(index);
        }
    }
    FunctionCfg::analyze(&mut blocks);
    let function = FunctionIr {
        start,
        end,
        instructions: lifted,
        cfg: FunctionCfg { blocks },
    };
    let ssa = build_ssa(&function);
    let file_ssa = super::file_ssa::build_file_ssa_with_arch(&function, architecture);
    let mut block_ids = Vec::new();
    for (index, block) in function.cfg.blocks.iter().enumerate() {
        let id = builder.record(
            "cfg-block",
            "derived",
            object([
                ("index", (index as u64).into()),
                ("granularity", "instruction".into()),
                ("known_edges_reachable", block.reachable.into()),
                (
                    "unknown_successors",
                    V::Bool(
                        (matches!(models[index], IrOperation::IndirectBranch { .. })
                            && !resolved_indirect_targets.contains_key(&function.cfg.blocks[index].start))
                            || matches!(models[index], IrOperation::TargetSpecific),
                    ),
                ),
                (
                    "dominators",
                    array(block.dominators.iter().map(|index| (*index as u64).into())),
                ),
            ]),
            ranges(block.start, aspan_end_pc_with_sizes(block.start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
        )?;
        builder.edge(&ir_ids[index], &id, "controls")?;
        block_ids.push(id);
    }
    for (index, block) in function.cfg.blocks.iter().enumerate() {
        for &successor in &block.successors {
            let edge = builder.record(
                "cfg-edge",
                "derived",
                object([
                    ("source", address(block.start)),
                    ("target", address(function.cfg.blocks[successor].start)),
                ]),
                ranges(block.start, aspan_end_pc_with_sizes(block.start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
            )?;
            builder.edge(&block_ids[index], &edge, "controls")?;
            builder.edge(&edge, &block_ids[successor], "flows-to")?;
        }
    }
    // Existing GPR SSA supplies real definitions/uses/phis, not parsed labels.
    let mut definitions = BTreeMap::<String, String>::new();
    let ssa_payload = |value: SsaValue| {
        object([
            ("register", u64::from(value.register).into()),
            (
                "origin_kind",
                match value.origin {
                    SsaOrigin::Entry => "entry",
                    SsaOrigin::Instruction(_) => "instruction",
                    SsaOrigin::Phi(_) => "phi",
                    SsaOrigin::Barrier(_) => "unknown-barrier",
                }
                .into(),
            ),
            ("value", value.name().into()),
        ])
    };
    for (&pc, instruction) in &ssa.instructions {
        for &value in &instruction.definitions {
            let id = builder.record(
                "definition",
                "derived",
                ssa_payload(value),
                ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)),
            )?;
            builder.edge(&ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], &id, "defines")?;
            definitions.insert(value.name(), id);
        }
    }
    for (index, phis) in ssa.phis.iter().enumerate() {
        for phi in phis {
            let mut payload = ssa_payload(phi.value);
            payload.insert(
                "incoming",
                array(phi.incoming.iter().map(|(predecessor, value)| {
                    object([
                        (
                            "predecessor",
                            predecessor
                                .map(|index| (index as u64).into())
                                .unwrap_or(V::Null),
                        ),
                        ("value", value.name().into()),
                    ])
                })),
            );
            let pc = function.cfg.blocks[index].start;
            let id = builder.record("phi", "derived", payload, ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            definitions.insert(phi.value.name(), id);
        }
    }
    for (&pc, instruction) in &ssa.instructions {
        for &value in &instruction.uses {
            let definition = if let Some(id) = definitions.get(&value.name()) {
                id.clone()
            } else {
                let id = builder.record(
                    "definition",
                    "derived",
                    ssa_payload(value),
                    ranges(start, aspan_end_pc_with_sizes(start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end.max(start+4))),
                )?;
                definitions.insert(value.name(), id.clone());
                id
            };
            let id = builder.record("use", "derived", ssa_payload(value), ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            builder.edge(&definition, &id, "flows-to")?;
            builder.edge(&id, &ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], "uses")?;
        }
    }
    for phis in &ssa.phis {
        for phi in phis {
            let phi_id = definitions[&phi.value.name()].clone();
            for (_, value) in &phi.incoming {
                let id = if let Some(id) = definitions.get(&value.name()) {
                    id.clone()
                } else {
                    let id = builder.record(
                        "definition",
                        "derived",
                        ssa_payload(*value),
                        ranges(start, aspan_end_pc_with_sizes(start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end.max(start+4))),
                    )?;
                    definitions.insert(value.name(), id.clone());
                    id
                };
                builder.edge(&id, &phi_id, "flows-to")?;
            }
        }
    }
    // Memory partition versions share the derivation graph with register
    // values. Entry definitions seed each partition; write definitions and
    // CFG phis feed explicit memory-use records at the instruction that reads
    // them. This keeps branch-local writes from appearing as dependencies in
    // another path and makes alias-wide dependencies navigable in the app.
    let partitions = MemoryPartition::all();
    let mut memory_definitions = BTreeMap::<(MemoryPartition, u32), String>::new();
    let mut memory_evidence_by_pc = BTreeMap::<u64, Vec<String>>::new();
    for partition in partitions {
        let id = builder.record("memory-definition", "derived", object([
            ("partition", partition.name().into()),
            ("version", 0u64.into()),
            ("origin", "entry".into()),
        ]), array([]))?;
        memory_definitions.insert((partition, 0), id);
    }
    for phi in &ssa.memory_phis {
        let id = builder.record("memory-phi", "derived", object([
            ("partition", phi.partition.name().into()),
            ("version", u64::from(phi.version).into()),
            ("incoming", array(phi.incoming.iter().map(|(predecessor, version)| object([
                ("predecessor", predecessor.map(|index| (index as u64).into()).unwrap_or(V::Null)),
                ("version", u64::from(*version).into()),
            ])))),
        ]), ranges(function.cfg.blocks[phi.block].start, aspan_end_pc_with_sizes(function.cfg.blocks[phi.block].start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
        builder.edge(&block_ids[phi.block], &id, "merges-memory")?;
        memory_definitions.insert((phi.partition, phi.version), id);
    }
    for version in &ssa.memory_versions {
        if let Some(written) = version.written_version {
            let id = builder.record("memory-definition", "derived", object([
                ("partition", version.partition.name().into()),
                ("version", u64::from(written).into()),
                ("origin", "instruction".into()),
            ]), ranges(version.pc, aspan_end_pc_with_sizes(version.pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            builder.edge(&ir_ids[aindex_of_with_arch(version.pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], &id, "defines-memory")?;
            memory_evidence_by_pc.entry(version.pc).or_default().push(id.clone());
            memory_definitions.insert((version.partition, written), id);
        }
    }
    for phi in &ssa.memory_phis {
        let phi_id = memory_definitions[&(phi.partition, phi.version)].clone();
        for (_, incoming) in &phi.incoming {
            let definition = memory_definitions.get(&(phi.partition, *incoming))
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing memory-phi input definition"))?;
            builder.edge(definition, &phi_id, "flows-to")?;
        }
    }
    for version in &ssa.memory_versions {
        if !version.read { continue; }
        let definition = memory_definitions.get(&(version.partition, version.read_version))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing reaching memory definition"))?;
        let id = builder.record("memory-use", "derived", object([
            ("partition", version.partition.name().into()),
            ("version", u64::from(version.read_version).into()),
            ("ordered", version.ordered.into()),
            ("exclusive", version.exclusive.into()),
        ]), ranges(version.pc, aspan_end_pc_with_sizes(version.pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
        builder.edge(definition, &id, "flows-to")?;
        builder.edge(&id, &ir_ids[aindex_of_with_arch(version.pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], "uses-memory")?;
        memory_evidence_by_pc.entry(version.pc).or_default().push(id);
    }
    // Preserve instruction-level memory accesses as graph nodes so system
    // events can point to the nearest accesses in their own basic block.
    // These are program-order observations only: they do not assert that a
    // barrier domain covers an access or that unrelated blocks are ordered.
    let mut accesses_by_pc = BTreeMap::<u64, Vec<&super::SsaMemoryVersion>>::new();
    for version in &ssa.memory_versions { accesses_by_pc.entry(version.pc).or_default().push(version); }
    let mut access_ids = BTreeMap::<u64, String>::new();
    for (pc, versions) in &accesses_by_pc {
        let reads = versions.iter().filter(|version| version.read)
            .map(|version| version.partition.name()).collect::<BTreeSet<_>>();
        let writes = versions.iter().filter(|version| version.written_version.is_some())
            .map(|version| version.partition.name()).collect::<BTreeSet<_>>();
        let id = builder.record("memory-access", "derived", object([
            ("reads", array(reads.into_iter().map(V::from))),
            ("writes", array(writes.into_iter().map(V::from))),
            ("ordered", versions.iter().any(|version| version.ordered).into()),
            ("exclusive", versions.iter().any(|version| version.exclusive).into()),
        ]), ranges(*pc, aspan_end_pc_with_sizes(*pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
        builder.edge(&ir_ids[aindex_of_with_arch(*pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], &id, "accesses-memory")?;
        access_ids.insert(*pc, id);
    }
    for event in &ssa.system_events {
        let (effect, relation) = match event.mnemonic {
            "dmb" => ("data-memory-ordering-fence", "orders-memory"),
            "dsb" => ("data-synchronization-fence", "synchronizes-memory"),
            "isb" => ("instruction-synchronization-fence", "synchronizes-instructions"),
            "clrex" => ("exclusive-monitor-clear", "clears-exclusive-monitor"),
            _ => continue,
        };
        let id = builder.record("memory-effect", "fact", object([
            ("effect", effect.into()),
            ("mnemonic", event.mnemonic.into()),
            ("domain", event.domain.map(V::from).unwrap_or(V::Null)),
            ("memory_values_changed", false.into()),
        ]), ranges(event.pc, aspan_end_pc_with_sizes(event.pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
        builder.edge(&ir_ids[aindex_of_with_arch(event.pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], &id, relation)?;
        if let Some(block_index) = function.cfg.blocks.iter().position(|block| block.instructions.contains(&event.pc)) {
            let nearest = |before: bool| -> Option<u64> {
                let mut current = block_index;
                let mut visited = BTreeSet::new();
                for _ in 0..=function.cfg.blocks.len() {
                    if !visited.insert(current) { break; }
                    let block = &function.cfg.blocks[current];
                    let candidate = block.instructions.iter().copied().filter(|pc| {
                        if before { *pc < event.pc && access_ids.contains_key(pc) }
                        else { *pc > event.pc && access_ids.contains_key(pc) }
                    });
                    let candidate = if before { candidate.max() } else { candidate.min() };
                    if candidate.is_some() { return candidate; }
                    let adjacent = if before {
                        if block.predecessors.len() != 1 { break; }
                        let predecessor = block.predecessors[0];
                        let previous = &function.cfg.blocks[predecessor];
                        if previous.successors.as_slice() != [current] || previous.instructions.last().copied().map(|pc| aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)) != block.instructions.first().copied() { break; }
                        predecessor
                    } else {
                        if block.successors.len() != 1 { break; }
                        let successor = block.successors[0];
                        let next = &function.cfg.blocks[successor];
                        if next.predecessors.as_slice() != [current] || block.instructions.last().copied().map(|pc| aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)) != next.instructions.first().copied() { break; }
                        successor
                    };
                    current = adjacent;
                }
                None
            };
            let before = nearest(true);
            let after = nearest(false);
            if let Some(pc) = before { builder.edge(&access_ids[&pc], &id, "program-order-before-event")?; }
            if let Some(pc) = after { builder.edge(&id, &access_ids[&pc], "program-order-after-event")?; }
        }
    }
    // NZCV and SIMD values are part of the derivation graph as structured
    // records too. The preview's comments are presentation only; these nodes
    // keep value origins and partial vector ranges available to clients.
    use super::file_ssa::{FileLocation, FileValue};
    let location_name = |location: FileLocation| -> String {
        match location {
            FileLocation::Flag(bit) => format!("nzcv.{}", super::files::flag_bit_name(bit)),
            FileLocation::Vector(register) => format!("v{register}"),
        }
    };
    let mut file_origins = BTreeMap::<(FileLocation, SsaOrigin), Vec<(u8, u8)>>::new();
    let mut collect_file_value = |location: FileLocation, value: &FileValue| {
        for segment in &value.segments {
            let ranges = file_origins.entry((location, segment.origin)).or_default();
            if !ranges.contains(&(segment.offset, segment.bytes)) {
                ranges.push((segment.offset, segment.bytes));
            }
        }
    };
    for site in file_ssa.flags.values() {
        for (bit, value) in &site.defs { collect_file_value(FileLocation::Flag(*bit), value); }
        for (bit, value) in &site.uses { collect_file_value(FileLocation::Flag(*bit), value); }
    }
    for site in file_ssa.vectors.values() {
        for (register, value) in &site.defs { collect_file_value(FileLocation::Vector(*register), value); }
        for (register, value) in &site.uses { collect_file_value(FileLocation::Vector(*register), value); }
    }
    for phis in &file_ssa.phis {
        for phi in phis {
            collect_file_value(phi.location, &phi.value);
            for (_, value) in &phi.incoming { collect_file_value(phi.location, value); }
        }
    }
    let mut file_definitions = BTreeMap::<(FileLocation, SsaOrigin), String>::new();
    for ((location, origin), byte_ranges) in file_origins {
        let evidence_range = match origin {
            SsaOrigin::Instruction(pc) | SsaOrigin::Barrier(pc) => (pc, pc.saturating_add(4)),
            SsaOrigin::Phi(block) => function.cfg.blocks.get(block)
                .map(|block| (block.start, block.start.saturating_add(4)))
                .unwrap_or((start, start.saturating_add(4))),
            SsaOrigin::Entry => (start, start.saturating_add(4)),
        };
        let id = builder.record(
            "file-definition",
            "derived",
            object([
                ("location", location_name(location).into()),
                ("origin", super::file_ssa::origin_name(origin).into()),
                ("origin_kind", super::file_ssa::origin_kind(origin).into()),
                ("byte_ranges", array(byte_ranges.into_iter().map(|(offset, bytes)| object([
                    ("offset", offset.into()), ("bytes", bytes.into()),
                ])))),
            ]),
            ranges(evidence_range.0, evidence_range.1),
        )?;
        file_definitions.insert((location, origin), id);
    }
    for (&pc, site) in &file_ssa.flags {
        for (bit, value) in &site.uses {
            let use_id = builder.record("file-use", "derived", object([
                ("location", location_name(FileLocation::Flag(*bit)).into()),
                ("value", value.name(FileLocation::Flag(*bit)).into()),
            ]), ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(FileLocation::Flag(*bit), segment.origin)) {
                    builder.edge(definition, &use_id, "flows-to")?;
                }
            }
            builder.edge(&use_id, &ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], "uses")?;
        }
        for (bit, value) in &site.defs {
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(FileLocation::Flag(*bit), segment.origin)) {
                    builder.edge(&ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], definition, "defines")?;
                }
            }
        }
    }
    for (&pc, site) in &file_ssa.vectors {
        for (register, value) in &site.uses {
            let location = FileLocation::Vector(*register);
            let use_id = builder.record("file-use", "derived", object([
                ("location", location_name(location).into()),
                ("value", value.name(location).into()),
                ("byte_ranges", array(value.segments.iter().map(|segment| object([
                    ("offset", segment.offset.into()), ("bytes", segment.bytes.into()),
                ])))),
            ]), ranges(pc, aspan_end_pc_with_sizes(pc, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(location, segment.origin)) {
                    builder.edge(definition, &use_id, "flows-to")?;
                }
            }
            builder.edge(&use_id, &ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], "uses")?;
        }
        for (register, value) in &site.defs {
            let location = FileLocation::Vector(*register);
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(location, segment.origin)) {
                    if matches!(segment.origin, SsaOrigin::Instruction(origin_pc) if origin_pc == pc)
                        || matches!(segment.origin, SsaOrigin::Barrier(origin_pc) if origin_pc == pc) {
                        builder.edge(&ir_ids[aindex_of_with_arch(pc, &insn_pcs, start, ir_ids.len(), &arch).unwrap_or(0)], definition, "defines")?;
                    }
                }
            }
        }
    }
    for (block_index, phis) in file_ssa.phis.iter().enumerate() {
        for phi in phis {
            let phi_id = builder.record("file-phi", "derived", object([
                ("location", location_name(phi.location).into()),
                ("value", phi.value.name(phi.location).into()),
                ("incoming", array(phi.incoming.iter().map(|(predecessor, value)| object([
                    ("predecessor", predecessor.map(|index| (index as u64).into()).unwrap_or(V::Null)),
                    ("value", value.name(phi.location).into()),
                ])))),
            ]), ranges(function.cfg.blocks[block_index].start, aspan_end_pc_with_sizes(function.cfg.blocks[block_index].start, &insn_pcs, &insn_sizes, start, insn_pcs.len(), stream_end)))?;
            for (_, incoming) in &phi.incoming {
                for segment in &incoming.segments {
                    if let Some(definition) = file_definitions.get(&(phi.location, segment.origin)) {
                        builder.edge(definition, &phi_id, "flows-to")?;
                    }
                }
            }
            if let Some(definition) = file_definitions.get(&(phi.location,
                phi.value.segments.first().map(|segment| segment.origin).unwrap_or(SsaOrigin::Entry))) {
                builder.edge(&phi_id, definition, "defines")?;
            }
        }
    }
    let mut recovered_source = None;
    let objc_header=crate::signatures::objc_header_for_imp(start,&native.objc_records,&native.objc_layouts);
    // A proven declaration remains useful when reachable effects block body
    // recovery. Do not fabricate one from observations or conflicting types.
    let source_declaration = if signatures.len() == 1 {
        let (name, signature) = &signatures[0];
        if let Some(source_signature) = crate::signatures::source_declaration_with_project_types(name, signature, &project_types) {
            let declaration = builder.record("type",
                if signature.source.contains("analyst") { "analyst-assertion" } else { "derived" },
                object([("declaration", signature.declaration.clone().into()),
                        ("source", signature.source.into())]), array([]))?;
            source::emit_header(&mut builder, &source_signature, &declaration, start, end,
                objc_header.as_deref(), &project_types)?;
            Some(declaration)
        } else { None }
    } else { None };
    if !source_blocked {
        let recovery = if signatures.len() == 1 {
            let (name, signature) = &signatures[0];
            crate::signatures::source_signature_with_project_types(name, signature, &project_types)
                .ok_or_else(|| "unsupported or unsafe source ABI declaration".to_owned())
                .and_then(|signature| source::recover_with_types_and_arch(start, &trees, &models, signature, &call_prototypes, &project_types, &insn_pcs, &insn_sizes, &arch))
        } else {
            Err(if signatures.is_empty() {
                "no exact source signature"
            } else {
                "conflicting exact source declarations"
            }
            .to_owned())
        };
        match recovery {
            Ok(recovery) => {
                let declaration = source_declaration.as_ref().expect("recovered source has a validated declaration");
                let objc_method=objc_header.as_deref().map(|header|(signatures[0].0.as_str(),header));
                let (root, node) = recovery.emit_with_arch(&mut builder, declaration, &ir_ids, &memory_evidence_by_pc, start, objc_method, &insn_pcs, &insn_sizes, trees.len(), &arch)?;
                statements.push(root);
                recovered_source = Some(node);
            }
            Err(reason) => {
                builder.record(
                    "metadata",
                    "derived",
                    object([
                        ("artifact_kind", "recovered-source-rejection".into()),
                        ("reason", reason.into()),
                    ]),
                    array([]),
                )?;
            }
        }
    }
    let root = builder.ast(
        "machine-function",
        None,
        statements,
        &bounds,
        (start, end),
        object([
            ("name", name.map(V::from).unwrap_or(V::Null)),
            (
                "representation",
                "review-only machine-state source tree".into(),
            ),
        ]),
    )?;
    let root_mirror = builder.record(
        "ast-node",
        "derived",
        object([
            ("ast_id", root.clone().into()),
            ("kind", "machine-function".into()),
        ]),
        ranges(start, end),
    )?;
    builder.edge(&bounds, &root_mirror, "supports")?;
    let has_recovered_source = recovered_source.is_some();
    if let Some(source) = recovered_source {
        builder.edge(&root_mirror, &source, "renders-as")?;
    }
    // Export the complete available instruction-derived review body even when
    // strict source/ABI proofs fail. This is code for analyst repair, never a
    // substitute proof of compilability or an invented original prototype.
    if !has_recovered_source {
        let text=executable::review_source_with_sizes_and_arch(start,&trees,&review_blocked_pcs,truncated,&insn_pcs,&insn_sizes,&arch);
        {
            let review = builder.record("metadata", "derived", object([
                ("artifact_kind", "best-effort-source".into()), ("source", text.into()),
                ("status", "review-only-uncompiled".into()), ("ast_root", root.clone().into()),
                ("contract", "repair-oriented machine-state C; status 4 stops before unsupported effects or outside captured code; bounded flat memory; no original ABI, complete implementation or equivalence claim".into()),
            ]), array([]))?;
            builder.edge(&root_mirror, &review, "renders-as")?;
        }
    }
    if source_declaration.is_none() {
        let is_objc=objc_header.is_some();
        let text = objc_header.unwrap_or_else(||format!("#pragma once\n/* Original function prototype unresolved.\n * Best-effort review interface only, NOT an original ABI declaration. */\ntypedef struct hs_review_context hs_review_context;\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\nvoid hs_review_{start:x}(hs_review_context *context);\n#ifdef __cplusplus\n}}\n#endif\n"));
        let header = builder.record("metadata", "derived", object([
            ("artifact_kind", "best-effort-header".into()), ("source", text.into()),
            ("status", "review-only-uncompiled".into()),
            ("languages", array(if is_objc{vec!["objective-c".into(),"objective-c++".into()]}else{vec!["c".into(), "c++".into(), "objective-c".into(), "objective-c++".into()]})),
            ("contract", if is_objc{"target-local Objective-C class/member declaration evidence; superclass/protocol/type dependencies unresolved; no complete ABI or implementation claim"}else{"review wrapper declaration only; original prototype unresolved; no original ABI or implementation claim"}.into()),
        ]), array([]))?;
        builder.edge(&root_mirror, &header, "renders-as")?;
    }
    for mirror in statement_mirrors {
        builder.edge(&mirror, &root_mirror, "derives")?;
    }
    let program = if strict_blocked {
        Err("truncated, unsupported or unresolved shared effect".into())
    } else {
        executable::Program::new_with_pcs_and_sizes_and_arch(start, trees, insn_pcs.clone(), insn_sizes.clone(), &arch)
    };
    match program {
        Ok(program) => {
            let generated=builder.record("metadata","derived",object([
                ("artifact_kind","machine-state-c".into()),
                ("contract",executable::CONTRACT.into()),("source",program.source().into()),
                ("status","generated-uncompiled".into()),("entry",address(start)),
                ("guest_memory_base",address(0x100000)),("guest_memory_size",4096u64.into()),
                ("limits","single-threaded normal EL0 GPR/NZCV; flat nonvolatile memory; no precise hardware fault-state or ABI/source-equivalence claim".into())]),array([]))?;
            builder.edge(&root_mirror, &generated, "renders-as")?;
            if let Some(inputs) = registers {
                let run = builder.record(
                    "metadata",
                    "derived",
                    object([
                        ("artifact_kind", "machine-state-run".into()),
                        ("contract", executable::CONTRACT.into()),
                        ("inputs_x0_x7", array(inputs.into_iter().map(address))),
                        (
                            "initial_state",
                            "other GPRs and NZCV zero; SP=0x101000; 4096 zero bytes at 0x100000"
                                .into(),
                        ),
                        ("step_limit", 4096u64.into()),
                        ("result", program.run(inputs, 4096)),
                    ]),
                    array([]),
                )?;
                builder.edge(&generated, &run, "supports")?;
            }
        }
        Err(reason) => {
            if registers.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Strict interpreter blocked: {reason}"),
                ));
            }
            let rejected = builder.record(
                "metadata",
                "derived",
                object([
                    ("artifact_kind", "machine-state-c-rejection".into()),
                    ("reason", reason.into()),
                    ("contract", executable::CONTRACT.into()),
                ]),
                array([]),
            )?;
            builder.edge(&root_mirror, &rejected, "supports")?;
        }
    }
    Ok(builder
        .finish(root, rendering, instructions, truncated, exhausted)
        .json())
}
