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
            if self.saved.len() < 64 {
                self.saved.insert((offset, bytes), value);
            } else {
                self.saved.clear();
                self.exhausted = true;
            }
        }
    }

    fn transfer(&self, operation: IrOperation, seeds: &[ParameterNameSeed]) -> Self {
        let effects = operation.gpr_effects();
        let mut result = self.clone();
        result.frame = self.frame.transfer(operation, &BTreeSet::new());
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
            IrOperation::LogicalRegister {
                destination,
                left,
                right,
                operator: IrBinaryOp::Or,
                negate_right: false,
                shift: None,
            } => {
                let source = if left.class == IrRegisterClass::Zero {
                    Some(right)
                } else if right.class == IrRegisterClass::Zero || left == right {
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
            IrOperation::ScalarFloatMove {
                destination,
                source,
                precision,
            } => {
                result.registers[fp(destination)] =
                    self.read(fp(source), float_bits(precision), seeds);
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
    let fail = |reason| ParameterNameAnalysis {
        blocker: Some(reason),
        ..Default::default()
    };
    if start < base
        || end <= start
        || (start - base) % 4 != 0
        || (end - start) % 4 != 0
        || (end - start) / 4 > MAX_WORDS_PER_FUNCTION as u64
    {
        return fail("function bounds or instruction budget");
    }
    let Some(body) = usize::try_from(start - base)
        .ok()
        .zip(usize::try_from(end - base).ok())
        .and_then(|(lo, hi)| code.get(lo..hi))
    else {
        return fail("function bytes unavailable");
    };
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
    if seeds
        .iter()
        .any(|seed| !source_names.insert(seed.name.as_str()))
    {
        return fail("conflicting source parameter names");
    }
    if seeds
        .iter()
        .any(|seed| !seeded.insert((seed.floating, seed.register)))
    {
        return fail("conflicting parameter identity seeds");
    }
    let operations = body
        .chunks_exact(4)
        .enumerate()
        .map(|(index, bytes)| {
            let pc = start + index as u64 * 4;
            let position = file_offset.saturating_add(pc - base);
            if data_ranges
                .iter()
                .any(|&(lo, hi, _)| position < hi && position.saturating_add(4) > lo)
            {
                IrOperation::TargetSpecific
            } else {
                lift_operation(
                    decode(u32::from_le_bytes(bytes.try_into().unwrap()), pc),
                    pc,
                )
            }
        })
        .collect::<Vec<_>>();
    // Unknown successors may re-enter at any instruction with different state.
    if operations
        .iter()
        .any(|operation| matches!(operation, IrOperation::IndirectBranch { .. }))
    {
        return fail("unresolved indirect successors");
    }
    let mut successors = vec![Vec::new(); operations.len()];
    let mut predecessors = vec![Vec::new(); operations.len()];
    for (index, operation) in operations.iter().enumerate() {
        match *operation {
            IrOperation::Branch { target } | IrOperation::ConditionalBranch { target, .. } => {
                if target >= start && target < end && (target - start) % 4 == 0 {
                    successors[index].push(((target - start) / 4) as usize);
                }
                if matches!(operation, IrOperation::ConditionalBranch { .. })
                    && index + 1 < operations.len()
                {
                    successors[index].push(index + 1);
                }
            }
            IrOperation::Return { .. } => {}
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
    let mut steps = 0;
    while let Some(index) = queue.pop_front() {
        steps += 1;
        if steps > 32768 {
            return fail("parameter identity fixed-point budget");
        }
        let outgoing = incoming[index]
            .as_ref()
            .unwrap()
            .transfer(operations[index], seeds);
        if outgoing.exhausted {
            return fail("frame identity slot budget");
        }
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
    for (index, state) in incoming.iter().enumerate() {
        let Some(state) = state else { continue };
        let pc = start + index as u64 * 4;
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
        assert_eq!(
            analyze(&vec![0xd503201f; 129], &seeds).blocker,
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
        assert_eq!(
            analyze(&stores, &seeds).blocker,
            Some("frame identity slot budget")
        );
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
