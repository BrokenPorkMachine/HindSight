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
    fn emit(&self, builder: &mut Builder, evidence: &str, pc: u64) -> io::Result<(String, String)> {
        let children = self
            .children
            .iter()
            .map(|child| child.emit(builder, evidence, pc))
            .collect::<io::Result<Vec<_>>>()?;
        let id = builder.ast(
            self.kind,
            self.ty.as_deref(),
            children.iter().map(|(id, _)| id.clone()).collect(),
            evidence,
            (pc, pc + 4),
            self.attributes.clone(),
        )?;
        let mirror = builder.record(
            "ast-node",
            "derived",
            object([("ast_id", id.clone().into()), ("kind", self.kind.into())]),
            ranges(pc, pc + 4),
        )?;
        builder.edge(evidence, &mirror, "renders-as")?;
        for (_, child_mirror) in children {
            builder.edge(&child_mirror, &mirror, "derives")?;
        }
        Ok((id, mirror))
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
        native,
    } = input;
    if start < base || end <= start || (start - base) % 4 != 0 || (end - start) % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid reconstruction function bounds",
        ));
    }
    let available_end = end.min(base + code.len() as u64);
    let high = available_end.min(start.saturating_add(128 * 4));
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
    let instructions = (end - start) / 4;
    let exhausted = instructions > 128;
    let truncated = high < end;
    let direct_targets = body.chunks_exact(4).enumerate().filter_map(|(index, bytes)| {
        let pc = start + index as u64 * 4;
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        match lift_operation(decode(word, pc), pc) {
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
                ranges(*symbol_address, *symbol_address + 4),
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
                ("source", "macho-objc-metadata".into()),
                ("image_origin", "captured-binary".into()),
            ]),
            ranges(layout.address, layout.address.saturating_add(40)),
        )?;
        native_type_ids.push(id);
    }

    // Objective-C IMP names derived from validated class/method metadata.
    for (symbol_address, name) in &native.objc_function_names {
        if *symbol_address >= start && *symbol_address < end {
            let id = builder.record(
                "symbol",
                "fact",
                object([
                    ("name", name.clone().into()),
                    ("address", address(*symbol_address)),
                    ("source", "macho-objc-imp".into()),
                    ("image_origin", "captured-binary".into()),
                ]),
                ranges(*symbol_address, *symbol_address + 4),
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

    let mut lifted = Vec::new();
    let mut models = Vec::new();
    let mut ir_ids = Vec::new();
    let mut statements = Vec::new();
    let mut statement_mirrors = Vec::new();
    let mut trees = Vec::new();
    let mut strict_blocked = truncated || exhausted;
    let mut source_blocked = truncated || exhausted;
    let mut shared_ir_count = 0u64;
    let mut modeled_tree_count = 0u64;
    let mut data_in_code_count = 0u64;
    let mut unsupported_reason_count = 0u64;
    // A single exact source declaration proves this function's own prototype,
    // which also proves the prototype of a self-recursive call target.
    let self_prototype = if signatures.len() == 1 {
        let (name, signature) = &signatures[0];
        crate::signatures::source_signature(name, signature)
    } else {
        None
    };
    let mut prototypes_by_address = BTreeMap::<u64, Vec<(String, crate::signatures::Signature)>>::new();
    for (target, symbol, prototype) in prototype_candidates {
        if prototype.language == "C" {
            prototypes_by_address.entry(target).or_default().push((symbol, prototype));
        }
    }
    if signatures.len() == 1 {
        let (symbol, prototype) = &signatures[0];
        prototypes_by_address.entry(start).or_default().push((symbol.clone(), prototype.clone()));
    }
    let mut call_prototypes = BTreeMap::<u64, crate::signatures::SourceSignature>::new();
    for (target, mut candidates) in prototypes_by_address {
        candidates.sort_by(|left, right| (&left.0, &left.1.declaration).cmp(&(&right.0, &right.1.declaration)));
        candidates.dedup_by(|left, right| left.0 == right.0 && left.1.declaration == right.1.declaration);
        if candidates.len() == 1 {
            let (symbol, prototype) = &candidates[0];
            if let Some(prototype) = crate::signatures::source_signature(symbol, prototype) {
                call_prototypes.insert(target, prototype);
            }
        }
    }
    let mut rendering=String::from("/* Review-only machine-state AST; declarations, runtime contracts and source result ABI unresolved. */\n");
    for (index, bytes) in body.chunks_exact(4).enumerate() {
        let pc = start + index as u64 * 4;
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        let position = file_offset + (pc - base);
        let data = data_ranges
            .iter()
            .any(|&(lo, hi, _)| position < hi && position + 4 > lo);
        let operation = decode(word, pc);
        let portable = if data {
            IrOperation::TargetSpecific
        } else {
            lift_operation(operation, pc)
        };
        let modeled = instruction_model(operation, portable, word);
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
            ranges(pc, pc + 4),
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
            ranges(pc, pc + 4),
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
            ranges(pc, pc + 4),
        )?;
        builder.edge(&decoded, &ir, "lifts-to")?;
        let mut reasons = Vec::new();
        if modeled.is_none() {
            reasons.push(if data {
                "data-in-code"
            } else {
                "unadapted-or-unknown-effect"
            });
        }
        let source_supported_call = modeled.is_some() && !data && match portable {
            IrOperation::DirectCall { target } if target == start => self_prototype.is_some(),
            IrOperation::DirectCall { target } => {
                call_prototypes.contains_key(&target)
                    && calls::analyze_callee(code, base, target, CALLEE_WINDOW).is_some_and(|summary| {
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
                    })
            }
            _ => false,
        };
        if matches!(
            portable,
            IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. }
        ) {
            reasons.push("unresolved-call-contract");
        }
        if matches!(portable, IrOperation::Marker { .. }) {
            reasons.push("runtime-marker-contract-unqualified");
        }
        if has_unmodeled_fp_status_effect(portable) {
            reasons.push("FPCR/FPSR effects are not modeled");
        }
        strict_blocked |= !reasons.is_empty();
        source_blocked |= !reasons.is_empty() && !source_supported_call;
        unsupported_reason_count += reasons.len() as u64;
        for reason in reasons {
            let unknown = builder.record(
                "unsupported-effect",
                "derived",
                object([
                    ("reason", reason.into()),
                    ("word", format!("0x{word:08x}").into()),
                    (
                        "reachable_scope",
                        "conservative whole captured function".into(),
                    ),
                ]),
                ranges(pc, pc + 4),
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
            IrOperation::DirectCall { target } => match calls::analyze_callee(code, base, target, CALLEE_WINDOW) {
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
                    calls::prove_non_returning(code, base, target, CALLEE_WINDOW),
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
                    .min(target.saturating_add(CALLEE_WINDOW as u64 * 4));
                let window = target.checked_sub(base).zip(upper.checked_sub(base))
                    .and_then(|(lo, hi)| code.get(lo as usize..hi as usize));
                let data_free = data_ranges.iter().all(|(lo, hi, _)| {
                    let a = file_offset.saturating_add(target.saturating_sub(base));
                    let b = file_offset.saturating_add(upper.saturating_sub(base));
                    a >= *hi || b <= *lo
                });
                let proven = source_supported_call && target != start && data_free
                    && window.is_some_and(|bytes| source::canonical_boolean_callee(bytes, target, prototype));
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
        let (ast, mirror) = tree.emit(&mut builder, &ir, pc)?;
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
                    if target >= start && target < high && (target - start) % 4 == 0 {
                        successors.push(((target - start) / 4) as usize);
                    }
                    if matches!(instruction.portable, IrOperation::ConditionalBranch { .. })
                        && index + 1 < lifted.len()
                    {
                        successors.push(index + 1);
                    }
                }
                IrOperation::Return { .. } | IrOperation::IndirectBranch { .. } => {}
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
    let file_ssa = super::file_ssa::build_file_ssa(&function);
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
                    matches!(
                        models[index],
                        IrOperation::IndirectBranch { .. } | IrOperation::TargetSpecific
                    )
                    .into(),
                ),
                (
                    "dominators",
                    array(block.dominators.iter().map(|index| (*index as u64).into())),
                ),
            ]),
            ranges(block.start, block.start + 4),
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
                ranges(block.start, block.start + 4),
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
                ranges(pc, pc + 4),
            )?;
            builder.edge(&ir_ids[((pc - start) / 4) as usize], &id, "defines")?;
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
            let id = builder.record("phi", "derived", payload, ranges(pc, pc + 4))?;
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
                    ranges(start, start + 4),
                )?;
                definitions.insert(value.name(), id.clone());
                id
            };
            let id = builder.record("use", "derived", ssa_payload(value), ranges(pc, pc + 4))?;
            builder.edge(&definition, &id, "flows-to")?;
            builder.edge(&id, &ir_ids[((pc - start) / 4) as usize], "uses")?;
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
                        ranges(start, start + 4),
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
        ]), ranges(function.cfg.blocks[phi.block].start, function.cfg.blocks[phi.block].start + 4))?;
        builder.edge(&block_ids[phi.block], &id, "merges-memory")?;
        memory_definitions.insert((phi.partition, phi.version), id);
    }
    for version in &ssa.memory_versions {
        if let Some(written) = version.written_version {
            let id = builder.record("memory-definition", "derived", object([
                ("partition", version.partition.name().into()),
                ("version", u64::from(written).into()),
                ("origin", "instruction".into()),
            ]), ranges(version.pc, version.pc + 4))?;
            builder.edge(&ir_ids[((version.pc - start) / 4) as usize], &id, "defines-memory")?;
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
        ]), ranges(version.pc, version.pc + 4))?;
        builder.edge(definition, &id, "flows-to")?;
        builder.edge(&id, &ir_ids[((version.pc - start) / 4) as usize], "uses-memory")?;
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
        ]), ranges(*pc, *pc + 4))?;
        builder.edge(&ir_ids[((pc - start) / 4) as usize], &id, "accesses-memory")?;
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
        ]), ranges(event.pc, event.pc + 4))?;
        builder.edge(&ir_ids[((event.pc - start) / 4) as usize], &id, relation)?;
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
                        if previous.successors.as_slice() != [current] || previous.instructions.last().copied().map(|pc| pc + 4) != block.instructions.first().copied() { break; }
                        predecessor
                    } else {
                        if block.successors.len() != 1 { break; }
                        let successor = block.successors[0];
                        let next = &function.cfg.blocks[successor];
                        if next.predecessors.as_slice() != [current] || block.instructions.last().copied().map(|pc| pc + 4) != next.instructions.first().copied() { break; }
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
            ]), ranges(pc, pc + 4))?;
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(FileLocation::Flag(*bit), segment.origin)) {
                    builder.edge(definition, &use_id, "flows-to")?;
                }
            }
            builder.edge(&use_id, &ir_ids[((pc - start) / 4) as usize], "uses")?;
        }
        for (bit, value) in &site.defs {
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(FileLocation::Flag(*bit), segment.origin)) {
                    builder.edge(&ir_ids[((pc - start) / 4) as usize], definition, "defines")?;
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
            ]), ranges(pc, pc + 4))?;
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(location, segment.origin)) {
                    builder.edge(definition, &use_id, "flows-to")?;
                }
            }
            builder.edge(&use_id, &ir_ids[((pc - start) / 4) as usize], "uses")?;
        }
        for (register, value) in &site.defs {
            let location = FileLocation::Vector(*register);
            for segment in &value.segments {
                if let Some(definition) = file_definitions.get(&(location, segment.origin)) {
                    if matches!(segment.origin, SsaOrigin::Instruction(origin_pc) if origin_pc == pc)
                        || matches!(segment.origin, SsaOrigin::Barrier(origin_pc) if origin_pc == pc) {
                        builder.edge(&ir_ids[((pc - start) / 4) as usize], definition, "defines")?;
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
            ]), ranges(function.cfg.blocks[block_index].start, function.cfg.blocks[block_index].start + 4))?;
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
        if let Some(source_signature) = crate::signatures::source_declaration(name, signature) {
            let declaration = builder.record("type",
                if signature.source.contains("analyst") { "analyst-assertion" } else { "derived" },
                object([("declaration", signature.declaration.clone().into()),
                        ("source", signature.source.into())]), array([]))?;
            source::emit_header(&mut builder, &source_signature, &declaration, start, end, objc_header.as_deref())?;
            Some(declaration)
        } else { None }
    } else { None };
    if !source_blocked {
        let recovery = if signatures.len() == 1 {
            let (name, signature) = &signatures[0];
            crate::signatures::source_signature(name, signature)
                .ok_or_else(|| "unsupported or unsafe source ABI declaration".to_owned())
                .and_then(|signature| source::recover(start, &trees, &models, signature, &call_prototypes))
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
                let (root, node) = recovery.emit(&mut builder, declaration, &ir_ids, &memory_evidence_by_pc, start, objc_method)?;
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
        let mut available = String::new();
        for line in rendering.lines() {
            if available.len() + line.len() + 5 > 1_000_000 {
                available.push_str("    /* TRUNCATED: review source byte budget exhausted. */\n    hs_unresolved_reconstruction_budget();\n");
                break;
            }
            available.push_str("    ");
            available.push_str(line);
            available.push('\n');
        }
        let text = format!("/* BEST-EFFORT C-LIKE RECONSTRUCTION: not compilable or ABI-qualified.\n * Symbolic registers/intrinsics and unresolved effects require analyst repair.\n * This review wrapper is NOT the original function prototype.\n * Captured instruction tree truncated: {truncated}.\n */\n#include <stdint.h>\ntypedef struct hs_review_context hs_review_context;\nvoid hs_review_{start:x}(hs_review_context *context) {{\n    (void)context;\n{available}}}\n");
        {
            let review = builder.record("metadata", "derived", object([
                ("artifact_kind", "best-effort-source".into()), ("source", text.into()),
                ("status", "review-only-uncompiled".into()), ("ast_root", root.clone().into()),
                ("contract", "instruction-derived C-like code; unresolved symbolic registers/intrinsics; no original ABI, compilation or equivalence claim".into()),
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
        executable::Program::new(start, trees)
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
