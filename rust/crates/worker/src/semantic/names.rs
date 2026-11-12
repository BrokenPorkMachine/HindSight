//! Parameter identities derived from shared effects, never rendered statements.
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug)]
pub struct ParameterNameSeed {
    pub register: u8,
    pub floating: bool,
    /// 32/64 for a supported source type, 0 for an untyped GPR candidate.
    pub bits: u8,
    pub name: String,
}

#[derive(Default, Debug)]
pub struct ParameterNameAnalysis {
    pub inputs: BTreeMap<u64, BTreeMap<String, String>>,
    pub outputs: BTreeMap<u64, BTreeMap<String, String>>,
    pub notes: BTreeMap<u64, String>,
    pub blocker: Option<&'static str>,
}

// Values name an entry parameter, not a register, display string or copy site.
// Equal copies on different paths therefore meet at the same identity.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NameState {
    registers: [Option<u8>; 64],
    frame: ObjectState,
    saved: BTreeMap<(i64, u8), u8>,
    exhausted: bool,
}

fn gp(register: IrRegister) -> Option<usize> {
    (register.class == IrRegisterClass::General && register.index < 31)
        .then_some(register.index as usize)
}

fn fp(index: u8) -> usize {
    32 + usize::from(index)
}

fn float_bits(precision: IrFloatPrecision) -> u8 {
    match precision {
        IrFloatPrecision::Half => 16,
        IrFloatPrecision::Single => 32,
        IrFloatPrecision::Double => 64,
    }
}

/// Width of a physical GPR read, not of a formatted register token. The
/// conservative default is 64 when a future effect lacks a width adapter.
fn read_bits(operation: IrOperation, register: u8) -> u8 {
    let operand = |value| match value {
        IrOperand::Register(register) => Some(register),
        _ => None,
    };
    let address = |value| match value {
        IrMemoryAddress::BaseOffset { base, .. } => vec![base],
        IrMemoryAddress::BaseIndex { base, index, .. } => vec![base, index],
        IrMemoryAddress::PcRelative { .. } => Vec::new(),
    };
    let values = match operation {
        IrOperation::Assign { value, .. } => operand(value).into_iter().collect(),
        IrOperation::Binary { left, right, .. } => {
            operand(left).into_iter().chain(operand(right)).collect()
        }
        IrOperation::LogicalRegister { left, right, .. }
        | IrOperation::ShiftedArithmetic { left, right, .. }
        | IrOperation::ExtractConcat { left, right, .. }
        | IrOperation::AddCarry { left, right, .. }
        | IrOperation::AddExtended { left, right, .. }
        | IrOperation::Divide { left, right, .. } => vec![left, right],
        IrOperation::ConditionalSelect {
            if_true, if_false, ..
        } => vec![if_true, if_false],
        IrOperation::MoveKeep { destination, .. } => vec![destination],
        IrOperation::Shift { source, .. }
        | IrOperation::OneSource { source, .. }
        | IrOperation::Extend { source, .. }
        | IrOperation::Extract { source, .. } => vec![source],
        IrOperation::VariableShift { source, amount, .. } => vec![source, amount],
        IrOperation::Insert {
            destination,
            source,
            ..
        } => vec![destination, source],
        IrOperation::MultiplyAdd {
            left,
            right,
            addend,
            ..
        }
        | IrOperation::WideningMultiply {
            left,
            right,
            addend,
            ..
        } => vec![Some(left), Some(right), addend]
            .into_iter()
            .flatten()
            .collect(),
        IrOperation::Compare { left, right, .. }
        | IrOperation::ConditionalCompare { left, right, .. } => {
            let right = match right {
                IrCompareOperand::Register { register, .. }
                | IrCompareOperand::Extended { register, .. } => Some(register),
                _ => None,
            };
            [Some(left), right].into_iter().flatten().collect()
        }
        IrOperation::Load { address: value, .. }
        | IrOperation::LoadPair { address: value, .. }
        | IrOperation::SimdLoad { address: value, .. }
        | IrOperation::SimdPairLoad { address: value, .. }
        | IrOperation::SimdStore { address: value, .. }
        | IrOperation::SimdPairStore { address: value, .. }
        | IrOperation::AtomicLoad { address: value, .. } => address(value),
        IrOperation::Store {
            address: value,
            value: source,
            ..
        } => address(value).into_iter().chain(operand(source)).collect(),
        IrOperation::StorePair {
            address: value,
            first,
            second,
            ..
        } => address(value).into_iter().chain([first, second]).collect(),
        IrOperation::AtomicStore {
            address: value,
            value: source,
            ..
        } => address(value).into_iter().chain([source]).collect(),
        IrOperation::ScalarIntFloatConvert {
            source: IrConversionRegister::Integer { index, wide },
            ..
        } => vec![IrRegister {
            class: IrRegisterClass::General,
            index,
            bits: if wide { 64 } else { 32 },
        }],
        IrOperation::ConditionalBranch {
            condition: IrCondition::Zero { register, .. } | IrCondition::Bit { register, .. },
            ..
        } => vec![register],
        IrOperation::IndirectCall { target }
        | IrOperation::IndirectBranch { target }
        | IrOperation::Return {
            target: Some(target),
        } => vec![target],
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter(|value| gp(*value) == Some(register as usize))
        .map(|value| value.bits)
        .max()
        .unwrap_or(64)
}

fn resolve_candidate_widths(
    operations: &[IrOperation],
    successors: &[Vec<usize>],
    seeds: &[ParameterNameSeed],
) -> Vec<ParameterNameSeed> {
    // May-entry liveness: union every path on which a register still holds its
    // entry bits. A read after a dominating definition cannot seed a parameter.
    let mut incoming = vec![None::<u32>; operations.len()];
    incoming[0] = Some(u32::MAX);
    let mut queue = VecDeque::from([0usize]);
    let mut widths = [0u8; 32];
    while let Some(index) = queue.pop_front() {
        let entry = incoming[index].unwrap();
        let effects = operations[index].gpr_effects();
        for register in 0..31u8 {
            if entry & effects.uses & (1 << register) != 0 {
                widths[register as usize] =
                    widths[register as usize].max(read_bits(operations[index], register));
            }
        }
        let outgoing = entry & !effects.definitions;
        for &successor in &successors[index] {
            let merged = incoming[successor].unwrap_or(0) | outgoing;
            if incoming[successor] != Some(merged) {
                incoming[successor] = Some(merged);
                if !queue.contains(&successor) {
                    queue.push_back(successor);
                }
            }
        }
    }
    seeds
        .iter()
        .cloned()
        .map(|mut seed| {
            if seed.bits == 0 {
                seed.bits = if widths[seed.register as usize] == 32 {
                    32
                } else {
                    64
                };
            }
            seed
        })
        .collect()
}

impl NameState {
    fn entry(seeds: &[ParameterNameSeed]) -> Self {
        let mut result = Self {
            registers: [None; 64],
            frame: ObjectState::entry(),
            saved: BTreeMap::new(),
            exhausted: false,
        };
        for (index, seed) in seeds.iter().enumerate() {
            result.registers[usize::from(seed.register) + if seed.floating { 32 } else { 0 }] =
                Some(index as u8);
        }
        result
    }

    fn merge(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for (value, incoming) in result.registers.iter_mut().zip(other.registers) {
            if *value != incoming {
                *value = None;
            }
        }
        result.frame = self.frame.merge(&other.frame);
        result
            .saved
            .retain(|key, value| other.saved.get(key) == Some(value));
        result.exhausted |= other.exhausted;
        result
    }

    fn read(&self, register: usize, bits: u8, seeds: &[ParameterNameSeed]) -> Option<u8> {
        self.registers[register].filter(|&seed| seeds[seed as usize].bits <= bits)
    }

    fn load(
        &self,
        address: ObjectValue,
        bytes: u8,
        bits: u8,
        floating: bool,
        seeds: &[ParameterNameSeed],
    ) -> Option<u8> {
        let ObjectValue::Stack(offset) = address else {
            return None;
        };
        self.saved.get(&(offset, bytes)).copied().filter(|&seed| {
            let seed = &seeds[seed as usize];
            seed.bits <= bytes.saturating_mul(8) && seed.bits <= bits && seed.floating == floating
        })
    }

    fn store(
        &mut self,
        address: ObjectValue,
        bytes: u8,
        value: Option<u8>,
        seeds: &[ParameterNameSeed],
    ) {
        let ObjectValue::Stack(offset) = address else {
            self.saved.clear();
            return;
        };
        let Some(end) = offset.checked_add(i64::from(bytes)) else {
            self.saved.clear();
            return;
        };
        self.saved
            .retain(|&(at, size), _| at + i64::from(size) <= offset || at >= end);
        if let Some(value) =
            value.filter(|&value| seeds[value as usize].bits <= bytes.saturating_mul(8))
        {
            if self.saved.len() < 64 || self.saved.contains_key(&(offset, bytes)) {
                self.saved.insert((offset, bytes), value);
            } else {
                // Bounded: stop tracking additional frame identities rather than
                // discarding existing proven parameter spills.
                self.exhausted = true;
            }
        }
    }

    fn transfer(&self, operation: IrOperation, seeds: &[ParameterNameSeed]) -> Self {
        let effects = operation.gpr_effects();
        let mut result = self.clone();
        result.frame = self.frame.transfer(operation, &BTreeSet::new());
        if matches!(operation, IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. }) {
            // AAPCS64 preserves x19-x29 and the low 64 bits of v8-v15 across
            // calls. Caller-saved values, x18 (platform-specific/reserved),
            // and LR are invalidated. Stack evidence is cleared because a
            // call may reach caller-owned storage through an escaped pointer.
            for index in (0..=18).chain(std::iter::once(30)) {
                result.registers[index] = None;
            }
            for index in 0..32 {
                if !(8..=15).contains(&index) {
                    result.registers[fp(index as u8)] = None;
                }
            }
            result.saved.clear();
            return result;
        }
        if effects.barrier {
            result.registers.fill(None);
            result.saved.clear();
            return result;
        }
        for index in 0..32 {
            if effects.definitions & (1 << index) != 0 {
                result.registers[index] = None;
            }
        }
        // Explicit V-register writes, including *both* destinations of pairs.
        // FP identity is width-specific; an S view of a double is not the same
        // source value, and a D view of a float includes unrelated upper bits.
        let vector_destinations = match operation {
            IrOperation::ScalarFloatImmediate { destination, .. }
            | IrOperation::ScalarFloatMove { destination, .. }
            | IrOperation::ScalarFloatBinary { destination, .. }
            | IrOperation::ScalarFloatUnary { destination, .. }
            | IrOperation::ScalarFloatSelect { destination, .. }
            | IrOperation::ScalarFloatConvert { destination, .. }
            | IrOperation::VectorImmediate { destination, .. } => vec![destination],
            IrOperation::ScalarFloatMultiplyAdd(value) => vec![value.destination],
            IrOperation::SimdLoad { destination, .. }
            | IrOperation::VectorBitwise { destination, .. } => vec![destination.index],
            IrOperation::SimdPairLoad { first, second, .. } => vec![first.index, second.index],
            IrOperation::ScalarIntFloatConvert {
                destination: IrConversionRegister::Float { index, .. },
                ..
            } => vec![index],
            _ => Vec::new(),
        };
        for destination in vector_destinations {
            result.registers[fp(destination)] = None;
        }

        // Addresses below use the incoming base plus the IR's encoded access
        // offset. The frame transfer separately applies pre/post writeback.
        match operation {
            IrOperation::Assign {
                destination,
                value: IrOperand::Register(source),
            } => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source))
                {
                    result.registers[destination_index] =
                        self.read(source_index, source.bits.min(destination.bits), seeds);
                }
            }
            IrOperation::ConditionalSelect {
                destination,
                if_true,
                if_false,
                else_op: IrSelectElse::None,
                ..
            } => {
                if let (Some(destination_index), Some(true_index), Some(false_index)) =
                    (gp(destination), gp(if_true), gp(if_false))
                {
                    let required_bits = if_true.bits.min(if_false.bits).min(destination.bits);
                    let true_value = self.read(true_index, required_bits, seeds);
                    let false_value = self.read(false_index, required_bits, seeds);
                    if true_value.is_some() && true_value == false_value {
                        result.registers[destination_index] = true_value;
                    }
                }
            }
            IrOperation::LogicalRegister {
                destination,
                left,
                right,
                operator: IrBinaryOp::And,
                negate_right: true,
                ..
            } if right.class == IrRegisterClass::Zero => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(left)) {
                    result.registers[destination_index] =
                        self.read(source_index, left.bits.min(destination.bits), seeds);
                }
            }
            IrOperation::LogicalRegister {
                destination,
                left,
                right,
                operator: IrBinaryOp::Xor,
                negate_right: false,
                ..
            } if left.class == IrRegisterClass::Zero || right.class == IrRegisterClass::Zero => {
                let source = if left.class == IrRegisterClass::Zero { right } else { left };
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source)) {
                    result.registers[destination_index] =
                        self.read(source_index, source.bits.min(destination.bits), seeds);
                }
            }
            IrOperation::LogicalRegister {
                destination,
                left,
                right,
                operator: IrBinaryOp::Or,
                negate_right: false,
                shift,
            } => {
                let source = if left.class == IrRegisterClass::Zero {
                    shift.is_none().then_some(right)
                } else if right.class == IrRegisterClass::Zero
                    || (shift.is_none() && left == right)
                {
                    Some(left)
                } else {
                    None
                };
                if let Some((destination_index, source)) = gp(destination).zip(source) {
                    result.registers[destination_index] = gp(source).and_then(|index| {
                        self.read(index, source.bits.min(destination.bits), seeds)
                    });
                }
            }
            IrOperation::Binary {
                destination,
                operator,
                left: IrOperand::Register(left),
                right: IrOperand::Register(right),
            } => {
                let source = if right.class == IrRegisterClass::Zero
                    && matches!(operator, IrBinaryOp::Add | IrBinaryOp::Subtract)
                {
                    Some(left)
                } else if operator == IrBinaryOp::Add && left.class == IrRegisterClass::Zero {
                    Some(right)
                } else {
                    None
                };
                if let Some(source) = source {
                    if let (Some(destination_index), Some(source_index)) =
                        (gp(destination), gp(source))
                    {
                        result.registers[destination_index] = self.read(
                            source_index,
                            source.bits.min(destination.bits),
                            seeds,
                        );
                    }
                }
            }
            IrOperation::Binary {
                destination,
                left: IrOperand::Register(source),
                right: IrOperand::Constant(0),
                operator: IrBinaryOp::Add | IrBinaryOp::Subtract,
            } => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source))
                {
                    result.registers[destination_index] =
                        self.read(source_index, source.bits.min(destination.bits), seeds);
                }
            }
            IrOperation::ShiftedArithmetic {
                destination,
                left,
                right,
                subtract,
                ..
            } => {
                let source = if right.class == IrRegisterClass::Zero {
                    Some(left)
                } else if !subtract && left.class == IrRegisterClass::Zero {
                    Some(right)
                } else {
                    None
                };
                if let Some(source) = source {
                    if let (Some(destination_index), Some(source_index)) =
                        (gp(destination), gp(source))
                    {
                        result.registers[destination_index] = self.read(
                            source_index,
                            source.bits.min(destination.bits),
                            seeds,
                        );
                    }
                }
            }
            IrOperation::Shift {
                destination,
                source,
                amount: 0,
                ..
            } => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source)) {
                    result.registers[destination_index] =
                        self.read(source_index, source.bits.min(destination.bits), seeds);
                }
            }
            IrOperation::Extend {
                destination,
                source,
                from,
                ..
            } => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source)) {
                    result.registers[destination_index] =
                        self.read(source_index, from.min(source.bits).min(destination.bits), seeds);
                }
            }
            IrOperation::Extract {
                destination,
                source,
                lsb: 0,
                width,
                signed: false,
                ..
            } => {
                if let (Some(destination_index), Some(source_index)) = (gp(destination), gp(source)) {
                    result.registers[destination_index] =
                        self.read(source_index, width.min(source.bits).min(destination.bits), seeds);
                }
            }
            IrOperation::ScalarFloatMove {
                destination,
                source,
                precision,
            } => {
                result.registers[fp(destination)] =
                    self.read(fp(source), float_bits(precision), seeds);
            }
            IrOperation::ScalarFloatSelect {
                destination,
                if_true,
                if_false,
                precision,
                ..
            } => {
                let bits = float_bits(precision);
                let true_value = self.read(fp(if_true), bits, seeds);
                let false_value = self.read(fp(if_false), bits, seeds);
                if true_value.is_some() && true_value == false_value {
                    result.registers[fp(destination)] = true_value;
                }
            }
            IrOperation::VectorBitwise {
                destination,
                left,
                operator: IrVectorBitwiseOperator::Move,
                ..
            } => {
                result.registers[fp(destination.index)] = self.read(
                    fp(left.index),
                    destination.width.bytes().saturating_mul(8),
                    seeds,
                );
            }
            IrOperation::Load {
                destination,
                address,
                bytes,
                ..
            } => {
                if let Some(index) = gp(destination) {
                    result.registers[index] = self.load(
                        self.frame.address(address),
                        bytes,
                        destination.bits,
                        false,
                        seeds,
                    );
                }
            }
            IrOperation::LoadPair {
                first,
                second,
                address,
                bytes,
                ..
            } => {
                let address = self.frame.address(address);
                for (register, address) in [
                    (first, address),
                    (second, ObjectState::offset(address, i64::from(bytes))),
                ] {
                    if let Some(index) = gp(register) {
                        result.registers[index] =
                            self.load(address, bytes, register.bits, false, seeds);
                    }
                }
            }
            IrOperation::Store {
                address,
                value,
                bytes,
                ..
            } => {
                let value = match value {
                    IrOperand::Register(register) => {
                        gp(register).and_then(|index| self.read(index, register.bits, seeds))
                    }
                    _ => None,
                };
                result.store(self.frame.address(address), bytes, value, seeds);
            }
            IrOperation::StorePair {
                address,
                first,
                second,
                bytes,
                ..
            } => {
                let address = self.frame.address(address);
                for (register, address) in [
                    (first, address),
                    (second, ObjectState::offset(address, i64::from(bytes))),
                ] {
                    let value =
                        gp(register).and_then(|index| self.read(index, register.bits, seeds));
                    result.store(address, bytes, value, seeds);
                }
            }
            IrOperation::SimdLoad {
                destination,
                address,
                ..
            } => {
                result.registers[fp(destination.index)] = self.load(
                    self.frame.address(address),
                    destination.width.bytes(),
                    destination.width.bytes().saturating_mul(8),
                    true,
                    seeds,
                );
            }
            IrOperation::SimdPairLoad {
                first,
                second,
                address,
                bytes,
                ..
            } => {
                let address = self.frame.address(address);
                for (register, address) in [
                    (first, address),
                    (second, ObjectState::offset(address, i64::from(bytes))),
                ] {
                    result.registers[fp(register.index)] = self.load(
                        address,
                        bytes,
                        register.width.bytes().saturating_mul(8),
                        true,
                        seeds,
                    );
                }
            }
            IrOperation::SimdStore {
                source, address, ..
            } => {
                result.store(
                    self.frame.address(address),
                    source.width.bytes(),
                    self.read(
                        fp(source.index),
                        source.width.bytes().saturating_mul(8),
                        seeds,
                    ),
                    seeds,
                );
            }
            IrOperation::SimdPairStore {
                first,
                second,
                address,
                bytes,
                ..
            } => {
                let address = self.frame.address(address);
                for (register, address) in [
                    (first, address),
                    (second, ObjectState::offset(address, i64::from(bytes))),
                ] {
                    result.store(
                        address,
                        bytes,
                        self.read(
                            fp(register.index),
                            register.width.bytes().saturating_mul(8),
                            seeds,
                        ),
                        seeds,
                    );
                }
            }
            // Atomic accesses cannot restore a non-atomic saved identity.
            IrOperation::AtomicStore { .. } => result.saved.clear(),
            _ => {}
        }
        result
    }

    fn display(&self, seeds: &[ParameterNameSeed]) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        for (register, value) in self.registers.iter().enumerate() {
            let Some(value) = value else { continue };
            let seed = &seeds[*value as usize];
            let index = register % 32;
            let prefix = match (register >= 32, seed.bits) {
                (false, 32) => 'w',
                (false, _) => 'x',
                (true, 32) => 's',
                (true, _) => 'd',
            };
            let mut name = if index == usize::from(seed.register) {
                seed.name.clone()
            } else {
                format!(
                    "{}_copy_{}{index}",
                    seed.name,
                    if register >= 32 { prefix } else { 'x' }
                )
            };
            if index != usize::from(seed.register) {
                // A generated alias must not shadow a real parameter name.
                while seeds.iter().any(|other| other.name == name) {
                    name.push_str(&format!("_p{value}"));
                }
            }
            result.insert(format!("{prefix}{index}"), name);
        }
        result
    }
}

/// Complete bounded function analysis. Unknown effects kill state; unresolved
/// indirect successors and budget exhaustion withhold the whole substitution.
/// `resolved_indirect_targets` maps indirect-branch instruction PCs to proven
/// in-function successor PCs (e.g. bounded jump-table cases); when provided for
/// an indirect branch, its known targets are used instead of bailing the whole
/// analysis.
#[allow(clippy::too_many_arguments)]
pub fn parameter_value_names(
    code: &[u8],
    base: u64,
    start: u64,
    end: u64,
    file_offset: u64,
    data_ranges: &[(u64, u64, u16)],
    seeds: &[ParameterNameSeed],
) -> ParameterNameAnalysis {
    parameter_value_names_with_resolved_indirects(code, base, start, end, file_offset, data_ranges, seeds, &BTreeMap::new())
}

/// Like [`parameter_value_names`] but with proven indirect-branch targets.
#[allow(clippy::too_many_arguments)]
pub fn parameter_value_names_with_resolved_indirects(
    code: &[u8],
    base: u64,
    start: u64,
    end: u64,
    file_offset: u64,
    data_ranges: &[(u64, u64, u16)],
    seeds: &[ParameterNameSeed],
    resolved_indirect_targets: &BTreeMap<u64, Vec<u64>>,
) -> ParameterNameAnalysis {
    parameter_value_names_with_resolved_indirects_and_pcs(code, base, start, end, file_offset, data_ranges, seeds, resolved_indirect_targets, &[], &[])
}

/// Like [`parameter_value_names_with_resolved_indirects`] but with optional instruction PC/size maps.
/// When maps are provided, analysis uses PC-map indexing/spans; decode remains ARM64 4-byte for now
/// and non-4-byte instruction sizes are treated conservatively as target-specific.
#[allow(clippy::too_many_arguments)]
pub fn parameter_value_names_with_resolved_indirects_and_pcs(
    code: &[u8],
    base: u64,
    start: u64,
    end: u64,
    file_offset: u64,
    data_ranges: &[(u64, u64, u16)],
    seeds: &[ParameterNameSeed],
    resolved_indirect_targets: &BTreeMap<u64, Vec<u64>>,
    insn_pcs: &[u64],
    insn_sizes: &[u32],
) -> ParameterNameAnalysis {
    const MAX_NAME_ANALYSIS_WORDS: usize = 512;
    let fail = |reason| ParameterNameAnalysis {
        blocker: Some(reason),
        ..Default::default()
    };
    let fixed_width = insn_pcs.is_empty();
    if start < base || end <= start {
        return fail("function bounds or instruction budget");
    }
    if fixed_width && (start - base) % 4 != 0 {
        return fail("function bounds or instruction budget");
    }
    let Some(body_end) = end.checked_sub(base).and_then(|d| usize::try_from(d).ok()).map(|e| e.min(code.len())) else {
        return fail("function bytes unavailable");
    };
    let Some(body_start) = start.checked_sub(base).and_then(|d| usize::try_from(d).ok()) else {
        return fail("function bytes unavailable");
    };
    if body_start >= body_end || body_end > code.len() {
        return fail("function bytes unavailable");
    }
    let (pcs, sizes): (Vec<u64>, Vec<u32>) = if fixed_width {
        let count = (body_end - body_start) / 4;
        if count == 0 || count > MAX_NAME_ANALYSIS_WORDS {
            return fail("function bounds or instruction budget");
        }
        let pcs = (0..count).map(|i| start + i as u64 * 4).collect();
        let sizes = vec![4; count];
        (pcs, sizes)
    } else {
        let mut pcs = Vec::new();
        let mut sizes = Vec::new();
        for (index, &pc) in insn_pcs.iter().enumerate() {
            if pc < start || pc >= end { continue; }
            let sz = if insn_sizes.len() == insn_pcs.len() { *insn_sizes.get(index).unwrap_or(&4) } else { 4 };
            pcs.push(pc);
            sizes.push(sz);
        }
        (pcs, sizes)
    };
    if pcs.is_empty() || pcs.len() > MAX_NAME_ANALYSIS_WORDS {
        return fail("function bounds or instruction budget");
    }
    if seeds.len() > 32
        || seeds.iter().any(|seed| {
            seed.register >= if seed.floating { 32 } else { 31 }
                || !(matches!(seed.bits, 32 | 64) || (seed.bits == 0 && !seed.floating))
                || seed.name.len() > 128
                || seed.name.is_empty()
                || !seed.name.bytes().enumerate().all(|(index, byte)| {
                    byte == b'_'
                        || byte.is_ascii_alphabetic()
                        || (index > 0 && byte.is_ascii_digit())
                })
        })
    {
        return fail("invalid parameter identity seeds");
    }
    let mut seeded = BTreeSet::new();
    let mut source_names = BTreeSet::new();
    if seeds.iter().any(|seed| !source_names.insert(seed.name.as_str())) {
        return fail("conflicting source parameter names");
    }
    if seeds.iter().any(|seed| !seeded.insert((seed.floating, seed.register))) {
        return fail("conflicting parameter identity seeds");
    }
    let idx_of = |target: u64| -> Option<usize> {
        if fixed_width {
            let d = target.checked_sub(start)?;
            if d % 4 != 0 { return None; }
            let i = (d / 4) as usize;
            (i < pcs.len()).then_some(i)
        } else {
            pcs.binary_search(&target).ok()
        }
    };
    let operations = pcs
        .iter()
        .enumerate()
        .map(|(index, &pc)| {
            let position = file_offset.saturating_add(pc.saturating_sub(base));
            let sz = *sizes.get(index).unwrap_or(&4) as u64;
            let pc_end = pc.saturating_add(sz);
            let end_pos = file_offset.saturating_add(pc_end.saturating_sub(base));
            let Some(off) = pc.checked_sub(base).and_then(|d| usize::try_from(d).ok()) else { return IrOperation::TargetSpecific; };
            // ARM64 4-byte decode only for now; variable-length decode remains conservative.
            if *sizes.get(index).unwrap_or(&4) != 4 || off.saturating_add(sz as usize) > code.len() {
                return IrOperation::TargetSpecific;
            }
            let word = u32::from_le_bytes(code[off..off + 4].try_into().unwrap());
            if data_ranges.iter().any(|&(lo, hi, _)| position < hi && end_pos > lo) {
                IrOperation::TargetSpecific
            } else {
                lift_operation(decode(word, pc), pc)
            }
        })
        .collect::<Vec<_>>();
    // Unknown successors may re-enter at any instruction with different state.
    // A present entry means targets are proven complete (possibly all external
    // exits, i.e. no in-function successors).
    if operations.iter().enumerate().any(|(index, operation)| {
        let pc = pcs[index];
        matches!(operation, IrOperation::IndirectBranch { .. })
            && !resolved_indirect_targets.contains_key(&pc)
    }) {
        return fail("unresolved indirect successors");
    }
    let mut successors = vec![Vec::new(); operations.len()];
    let mut predecessors = vec![Vec::new(); operations.len()];
    for (index, operation) in operations.iter().enumerate() {
        match *operation {
            IrOperation::Branch { target } | IrOperation::ConditionalBranch { target, .. } => {
                if let Some(ti) = idx_of(target).filter(|&ti| pcs[ti] < end) {
                    successors[index].push(ti);
                }
                if matches!(operation, IrOperation::ConditionalBranch { .. })
                    && index + 1 < operations.len()
                {
                    successors[index].push(index + 1);
                }
            }
            IrOperation::Return { .. } => {}
            IrOperation::IndirectBranch { .. } => {
                let pc = pcs[index];
                if let Some(targets) = resolved_indirect_targets.get(&pc) {
                    for &target in targets {
                        if let Some(ti) = idx_of(target).filter(|&ti| pcs[ti] < end) {
                            successors[index].push(ti);
                        }
                    }
                }
            }
            _ if index + 1 < operations.len() => successors[index].push(index + 1),
            _ => {}
        }
        successors[index].sort_unstable();
        successors[index].dedup();
        for &successor in &successors[index] {
            predecessors[successor].push(index);
        }
    }
    let resolved_seeds = resolve_candidate_widths(&operations, &successors, seeds);
    let seeds = resolved_seeds.as_slice();
    let mut incoming = vec![None::<NameState>; operations.len()];
    incoming[0] = Some(NameState::entry(seeds));
    let mut queue = VecDeque::from([0usize]);
    let mut steps = 0usize;
    let step_budget = (operations.len() * 256).clamp(32768, 131072);
    while let Some(index) = queue.pop_front() {
        steps += 1;
        if steps > step_budget {
            return fail("parameter identity fixed-point budget");
        }
        let outgoing = incoming[index]
            .as_ref()
            .unwrap()
            .transfer(operations[index], seeds);
        for &successor in &successors[index] {
            let next = incoming[successor]
                .as_ref()
                .map(|prior| prior.merge(&outgoing))
                .unwrap_or_else(|| outgoing.clone());
            if incoming[successor].as_ref() != Some(&next) {
                incoming[successor] = Some(next);
                if !queue.contains(&successor) {
                    queue.push_back(successor);
                }
            }
        }
    }
    let mut result = ParameterNameAnalysis::default();
    let mut note_bytes = 0usize;
    if incoming.iter().flatten().any(|state| state.exhausted) {
        result.notes.insert(
            start,
            "frame identity slot budget reached; additional spills may be untracked"
                .into(),
        );
        note_bytes = result.notes[&start].len();
    }
    for (index, state) in incoming.iter().enumerate() {
        let Some(state) = state else { continue };
        let pc = pcs[index];
        result.inputs.insert(pc, state.display(seeds));
        result
            .outputs
            .insert(pc, state.transfer(operations[index], seeds).display(seeds));
        let mut alternatives: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let reachable = predecessors[index]
            .iter()
            .copied()
            .filter(|&pred| incoming[pred].is_some())
            .collect::<Vec<_>>();
        if reachable.len() + usize::from(index == 0) < 2 {
            continue;
        }
        let mut paths = reachable
            .iter()
            .map(|&pred| {
                incoming[pred]
                    .as_ref()
                    .unwrap()
                    .transfer(operations[pred], seeds)
                    .display(seeds)
            })
            .collect::<Vec<_>>();
        if index == 0 {
            paths.push(NameState::entry(seeds).display(seeds));
        }
        for names in &paths {
            for (register, name) in names {
                alternatives
                    .entry(register.clone())
                    .or_default()
                    .insert(name.clone());
            }
        }
        let mut descriptions = Vec::new();
        for (register, mut names) in alternatives {
            if paths.iter().any(|path| !path.contains_key(&register)) {
                names.insert("unknown/redefined".into());
            }
            if names.len() > 1 {
                let description = format!(
                    "{register} reaches with conflicting names ({})",
                    names.into_iter().collect::<Vec<_>>().join(" | ")
                );
                if descriptions.iter().map(String::len).sum::<usize>() + description.len() > 3072 {
                    descriptions
                        .push("remaining join alternatives truncated by annotation budget".into());
                    break;
                }
                descriptions.push(description);
            }
        }
        if !descriptions.is_empty() && note_bytes < 4096 {
            let note = format!(
                "merge at {pc:#x}: {}; machine registers retained after the join",
                descriptions.join("; ")
            );
            if note_bytes + note.len() <= 4096 {
                note_bytes += note.len();
                result.notes.insert(pc, note);
            } else {
                result.notes.insert(
                    pc,
                    "remaining join alternatives truncated by per-function annotation budget"
                        .into(),
                );
                note_bytes = 4096;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(register: u8, bits: u8, floating: bool) -> ParameterNameSeed {
        ParameterNameSeed {
            register,
            bits,
            floating,
            name: format!("value{register}"),
        }
    }
    fn reg(index: u8, bits: u8) -> IrRegister {
        IrRegister {
            class: IrRegisterClass::General,
            index,
            bits,
        }
    }
    fn stack(offset: i64) -> IrMemoryAddress {
        IrMemoryAddress::BaseOffset {
            base: IrRegister {
                class: IrRegisterClass::StackPointer,
                index: 31,
                bits: 64,
            },
            offset,
        }
    }
    fn copy(destination: u8, source: u8, bits: u8) -> IrOperation {
        IrOperation::Assign {
            destination: reg(destination, bits),
            value: IrOperand::Register(reg(source, bits)),
        }
    }
    fn analyze(words: &[u32], seeds: &[ParameterNameSeed]) -> ParameterNameAnalysis {
        let code = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        parameter_value_names(
            &code,
            0x1000,
            0x1000,
            0x1000 + code.len() as u64,
            0,
            &[],
            seeds,
        )
    }

    #[test]
    fn copy_identity_is_entry_based_not_a_growing_display_string() {
        let seeds = [seed(0, 64, false)];
        let state = NameState::entry(&seeds)
            .transfer(copy(1, 0, 64), &seeds)
            .transfer(copy(2, 1, 64), &seeds);
        assert_eq!(state.registers[1], state.registers[2]);
        assert_eq!(state.display(&seeds)["x2"], "value0_copy_x2");
        let restored = state.transfer(copy(0, 2, 64), &seeds);
        assert_eq!(restored.display(&seeds)["x0"], "value0");
    }

    #[test]
    fn calls_retain_only_aapcs64_callee_saved_parameter_identities() {
        let seeds = [
            seed(0, 64, false),
            seed(19, 64, false),
            seed(29, 64, false),
            seed(0, 64, true),
            seed(8, 64, true),
            seed(16, 64, true),
        ];
        let state = NameState::entry(&seeds);
        for operation in [
            IrOperation::DirectCall { target: 0x2000 },
            IrOperation::IndirectCall { target: reg(0, 64) },
        ] {
            let after = state.transfer(operation, &seeds);
            let names = after.display(&seeds);
            assert!(!names.contains_key("x0"));
            assert!(!names.contains_key("x18"));
            assert!(names.contains_key("x19"));
            assert!(names.contains_key("x29"));
            assert!(!names.contains_key("x30"));
            assert!(!names.contains_key("d0"));
            assert!(names.contains_key("d8"));
            assert!(!names.contains_key("d16"));
            assert!(after.saved.is_empty());
        }
    }

    #[test]
    fn width_and_bank_changes_do_not_rename_a_different_value() {
        let seeds = [
            seed(0, 64, false),
            seed(1, 32, false),
            seed(0, 64, true),
            seed(1, 32, true),
        ];
        let state = NameState::entry(&seeds);
        assert!(!state.display(&seeds).contains_key("w0"));
        assert!(!state.display(&seeds).contains_key("x1"));
        assert!(!state.display(&seeds).contains_key("s0"));
        assert!(!state.display(&seeds).contains_key("d1"));
        assert!(state.transfer(copy(2, 0, 32), &seeds).registers[2].is_none());
        let copied = state.transfer(copy(2, 1, 64), &seeds);
        assert_eq!(copied.display(&seeds)["w2"], "value1_copy_x2");
        assert!(!copied.display(&seeds).contains_key("x2"));
        // Zero-extension of a 32-bit parameter identity preserves it.
        let extended = state.transfer(
            IrOperation::Extend {
                destination: reg(2, 64),
                source: reg(1, 32),
                signed: false,
                from: 32,
            },
            &seeds,
        );
        assert_eq!(extended.display(&seeds)["w2"], "value1_copy_x2");
        let narrow = state.transfer(
            IrOperation::ScalarFloatMove {
                destination: 2,
                source: 0,
                precision: IrFloatPrecision::Single,
            },
            &seeds,
        );
        assert!(narrow.registers[fp(2)].is_none());
        let exact = state.transfer(
            IrOperation::ScalarFloatMove {
                destination: 2,
                source: 1,
                precision: IrFloatPrecision::Single,
            },
            &seeds,
        );
        assert_eq!(exact.display(&seeds)["s2"], "value1_copy_s2");
        let cross = state.transfer(
            IrOperation::ScalarIntFloatConvert {
                destination: IrConversionRegister::Float {
                    index: 0,
                    precision: IrFloatPrecision::Double,
                },
                source: IrConversionRegister::Integer {
                    index: 1,
                    wide: false,
                },
                signed: true,
                fractional_bits: None,
            },
            &seeds,
        );
        assert!(cross.registers[fp(0)].is_none());
        assert_eq!(cross.registers[1], state.registers[1]);
    }

    #[test]
    fn intact_spills_restore_names_but_overlapping_alias_and_call_writes_kill_them() {
        let seeds = [seed(0, 64, false)];
        let store = IrOperation::Store {
            address: stack(-16),
            value: IrOperand::Register(reg(0, 64)),
            bytes: 8,
            writeback: None,
        };
        let load = IrOperation::Load {
            destination: reg(2, 64),
            address: stack(-16),
            bytes: 8,
            signed: false,
            writeback: None,
        };
        let stored = NameState::entry(&seeds).transfer(store, &seeds);
        assert_eq!(
            stored.transfer(load, &seeds).display(&seeds)["x2"],
            "value0_copy_x2"
        );
        let overlap = IrOperation::Store {
            address: stack(-12),
            value: IrOperand::Constant(0),
            bytes: 4,
            writeback: None,
        };
        assert!(stored
            .transfer(overlap, &seeds)
            .transfer(load, &seeds)
            .registers[2]
            .is_none());
        let disjoint = IrOperation::Store {
            address: stack(-8),
            value: IrOperand::Constant(0),
            bytes: 4,
            writeback: None,
        };
        assert!(stored
            .transfer(disjoint, &seeds)
            .transfer(load, &seeds)
            .registers[2]
            .is_some());
        let alias = IrOperation::Store {
            address: IrMemoryAddress::BaseOffset {
                base: reg(3, 64),
                offset: 0,
            },
            value: IrOperand::Constant(0),
            bytes: 4,
            writeback: None,
        };
        assert!(stored
            .transfer(alias, &seeds)
            .transfer(load, &seeds)
            .registers[2]
            .is_none());
        for effect in [
            IrOperation::DirectCall { target: 0x2000 },
            IrOperation::TargetSpecific,
        ] {
            assert!(stored
                .transfer(effect, &seeds)
                .transfer(load, &seeds)
                .registers[2]
                .is_none());
        }
        // Reinterpreting GP stack bits as a float does not preserve the source variable.
        let float_load = IrOperation::SimdLoad {
            destination: IrSimdRegister {
                index: 2,
                width: IrSimdWidth::Double,
            },
            address: stack(-16),
            writeback: None,
        };
        assert!(stored.transfer(float_load, &seeds).registers[fp(2)].is_none());
    }

    #[test]
    fn paired_accesses_and_sp_writeback_handle_both_values() {
        let seeds = [
            seed(0, 64, false),
            seed(1, 64, false),
            seed(0, 64, true),
            seed(1, 64, true),
        ];
        let sp = IrRegister {
            class: IrRegisterClass::StackPointer,
            index: 31,
            bits: 64,
        };
        let store = IrOperation::StorePair {
            address: stack(-16),
            first: reg(0, 64),
            second: reg(1, 64),
            bytes: 8,
            writeback: Some(IrWriteback {
                base: sp,
                offset: -16,
                pre_index: true,
            }),
        };
        let load = IrOperation::LoadPair {
            address: stack(0),
            first: reg(2, 64),
            second: reg(3, 64),
            bytes: 8,
            signed: false,
            writeback: Some(IrWriteback {
                base: sp,
                offset: 16,
                pre_index: false,
            }),
        };
        let restored = NameState::entry(&seeds)
            .transfer(store, &seeds)
            .transfer(load, &seeds);
        assert_eq!(restored.display(&seeds)["x2"], "value0_copy_x2");
        assert_eq!(restored.display(&seeds)["x3"], "value1_copy_x3");
        assert_eq!(restored.frame.registers[31], ObjectValue::Stack(0));
        let pair = IrOperation::SimdPairLoad {
            first: IrSimdRegister {
                index: 0,
                width: IrSimdWidth::Double,
            },
            second: IrSimdRegister {
                index: 1,
                width: IrSimdWidth::Double,
            },
            address: stack(0),
            bytes: 8,
            writeback: None,
        };
        let killed = NameState::entry(&seeds).transfer(pair, &seeds);
        assert!(killed.registers[fp(0)].is_none() && killed.registers[fp(1)].is_none());
        assert!(killed.registers[0].is_some());
    }

    #[test]
    fn bytecode_cfg_joins_ignore_unreachable_paths_and_loop_backedges_remove_stale_names() {
        let seeds = [seed(0, 64, false)];
        // Unreachable x0 overwrite cannot poison a forward-branch target.
        let unreachable = analyze(&[0x14000002, 0xd2800000, 0xaa0003e2, 0xd65f03c0], &seeds);
        assert!(!unreachable.inputs.contains_key(&0x1004));
        assert_eq!(unreachable.outputs[&0x1008]["x2"], "value0_copy_x2");
        // A name present only on the first iteration must disappear at the header.
        let looped = analyze(&[0xaa0003e2, 0xd2800000, 0xb5ffffc1, 0xd65f03c0], &seeds);
        assert!(!looped.inputs[&0x1000].contains_key("x0"));
        assert!(!looped.outputs[&0x1000].contains_key("x2"));
        assert!(looped.notes[&0x1000].contains("unknown/redefined"));
        let preserved = analyze(&[0xaa0003e2, 0xb5ffffe1, 0xd65f03c0], &seeds);
        assert_eq!(preserved.inputs[&0x1004]["x2"], "value0_copy_x2");
    }

    #[test]
    fn data_opaque_indirect_and_function_budgets_are_explicit() {
        let seeds = [seed(0, 64, false)];
        assert!(analyze(&[0xffffffff, 0xd65f03c0], &seeds).inputs[&0x1004].is_empty());
        assert_eq!(
            analyze(&[0xd61f0200], &seeds).blocker,
            Some("unresolved indirect successors")
        );
        // 129 instructions is within the parameter-name analysis budget (512),
        // so a simple NOP+... sequence is analyzable rather than blocked.
        assert!(analyze(&vec![0xd503201f; 129], &seeds).blocker.is_none());
        assert_eq!(
            analyze(&vec![0xd503201f; 513], &seeds).blocker,
            Some("function bounds or instruction budget")
        );
        let code = [0xd503201fu32, 0xd65f03c0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let data = parameter_value_names(
            &code,
            0x1000,
            0x1000,
            0x1008,
            0x200,
            &[(0x200, 0x204, 1)],
            &seeds,
        );
        assert!(data.inputs[&0x1004].is_empty());
        let mut bad = seed(0, 64, false);
        bad.name = "not an identifier".into();
        assert!(analyze(&[0xd65f03c0], &[bad]).blocker.is_some());
        assert!(
            analyze(&[0xd65f03c0], &[seed(0, 64, false), seed(0, 32, false)])
                .blocker
                .is_some()
        );
        let stores = (0..65u32)
            .map(|index| 0xf90003e0 | (index << 10))
            .chain([0xd65f03c0])
            .collect::<Vec<_>>();
        // Bounded: exceeding frame identity slots no longer discards existing
        // proven parameter spills; analysis remains available.
        assert!(analyze(&stores, &seeds).blocker.is_none());
    }

    #[test]
    fn resolved_indirect_branch_targets_remove_successor_blocker() {
        let seeds = [seed(0, 64, false)];
        // mov x2, x0; br x1; ret
        let words = [0xaa0003e2u32, 0xd61f0020, 0xd65f03c0];
        let unresolved = analyze(&words, &seeds);
        assert_eq!(unresolved.blocker, Some("unresolved indirect successors"));
        let code = words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>();
        let mut resolved = BTreeMap::new();
        resolved.insert(0x1004u64, vec![0x1008u64]);
        let analysis = parameter_value_names_with_resolved_indirects(
            &code,
            0x1000,
            0x1000,
            0x1000 + code.len() as u64,
            0,
            &[],
            &seeds,
            &resolved,
        );
        assert!(analysis.blocker.is_none(), "{:?}", analysis.blocker);
        assert_eq!(analysis.outputs[&0x1000]["x2"], "value0_copy_x2");
        assert_eq!(analysis.inputs[&0x1008]["x2"], "value0_copy_x2");

        // Proven complete indirect branch with no in-function successors (all
        // external exits) does not block analysis and adds no re-entry edges.
        let mut exit_resolved = BTreeMap::new();
        exit_resolved.insert(0x1004u64, Vec::new());
        let exit_analysis = parameter_value_names_with_resolved_indirects(
            &code,
            0x1000,
            0x1000,
            0x1000 + code.len() as u64,
            0,
            &[],
            &seeds,
            &exit_resolved,
        );
        assert!(exit_analysis.blocker.is_none(), "{:?}", exit_analysis.blocker);
        assert_eq!(exit_analysis.outputs[&0x1000]["x2"], "value0_copy_x2");
    }

    #[test]
    fn generated_aliases_do_not_shadow_real_parameter_names() {
        let first = seed(0, 64, false);
        let mut other = seed(1, 64, false);
        other.name = "value0_copy_x2".into();
        let seeds = [first, other];
        let state = NameState::entry(&seeds).transfer(copy(2, 0, 64), &seeds);
        assert_eq!(state.display(&seeds)["x2"], "value0_copy_x2_p0");
        assert_eq!(state.display(&seeds)["x1"], "value0_copy_x2");
    }

    #[test]
    fn untyped_candidate_widths_come_only_from_entry_reaching_uses() {
        let seeds = [seed(0, 0, false)];
        let narrow = analyze(&[0x0b010002, 0xd65f03c0], &seeds); // add w2,w0,w1
        assert_eq!(narrow.inputs[&0x1000]["w0"], "value0");
        assert!(!narrow.inputs[&0x1000].contains_key("x0"));
        let mixed = analyze(&[0xaa0003e2, 0x0b010003, 0xd65f03c0], &seeds);
        assert_eq!(mixed.inputs[&0x1000]["x0"], "value0");
        assert!(!mixed.inputs[&0x1004].contains_key("w0"));
        // A W read after replacement does not retroactively type the input.
        let overwritten = analyze(&[0xd2800000, 0x0b010002, 0xd65f03c0], &seeds);
        assert_eq!(overwritten.inputs[&0x1000]["x0"], "value0");
        assert!(overwritten.inputs[&0x1004].is_empty());
    }
}
