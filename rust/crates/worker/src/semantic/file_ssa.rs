//! Value identity for the two register files the general-purpose file does
//! not cover: the NZCV flag file and the vector file.
//!
//! The general-purpose file models one value per register. The flag file is
//! modelled one bit at a time, because `ANDS` writes `N` and `Z` and leaves
//! the carry and the overflow alone. The vector file is modelled one byte at
//! a time, because narrow scalar/Q=0 definitions zero upper bits while ABI
//! calls can clobber only the high half of callee-saved vector registers.
//!
//! Both files use the bounded construction the general-purpose file uses:
//! definitions and uses per instruction, liveness, phi placement at
//! dominance frontiers, and renaming over the dominator tree. A join whose
//! incoming values agree keeps that value instead of naming an opaque merge,
//! so an identity survives a control-flow join whenever the evidence supports
//! it, and a partial write never discards the identity of the part it
//! preserved.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::*;

/// The number of NZCV flag bits.
const FLAG_BITS: usize = 4;
/// The number of bytes in one vector register.
const VECTOR_BYTES: u8 = 16;
/// The number of vector registers.
const VECTOR_REGISTERS: usize = 32;
/// The number of slots the two files hold.
const SLOT_COUNT: usize = FLAG_BITS + VECTOR_REGISTERS * VECTOR_BYTES as usize;

/// The slot of one flag bit.
fn flag_slot(bit: u8) -> usize {
    bit as usize
}

/// The slot of one byte of a vector register.
fn vector_slot(register: u8, offset: u8) -> usize {
    FLAG_BITS + register as usize * VECTOR_BYTES as usize + offset as usize
}

/// One register-file location: a flag bit or a vector register.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FileLocation {
    /// One NZCV flag bit, addressed by its position in the flag mask.
    Flag(u8),
    /// One 128-bit vector register.
    Vector(u8),
}

/// The byte range of a location that carries one value identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileSegment {
    /// The first byte the identity covers inside the location.
    pub offset: u8,
    /// The number of bytes the identity covers.
    pub bytes: u8,
    /// Where the value came from.
    pub origin: SsaOrigin,
}

/// The value of a location: the byte ranges that each carry one identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FileValue {
    /// The ranges, in increasing offset order.
    pub segments: Vec<FileSegment>,
}

impl FileValue {
    /// A value whose whole location carries one identity.
    fn whole(bytes: u8, origin: SsaOrigin) -> Self {
        Self { segments: vec![FileSegment { offset: 0, bytes, origin }] }
    }

    /// The published name of the value.
    pub(super) fn name(&self, location: FileLocation) -> String {
        match location {
            FileLocation::Flag(bit) => {
                let origin = self.segments.first().map(|segment| segment.origin).unwrap_or(SsaOrigin::Entry);
                format!("{}@{}", files::flag_bit_name(bit), origin_name(origin))
            }
            FileLocation::Vector(register) => {
                format!("v{register}{}", segments_name(&self.segments))
            }
        }
    }
}

/// The published name of a run of byte ranges.
fn segments_name(segments: &[FileSegment]) -> String {
    segments.iter()
        .map(|segment| format!("[{}..{})@{}", segment.offset, segment.offset + segment.bytes, origin_name(segment.origin)))
        .collect::<Vec<_>>().join("+")
}

/// The published name of an origin.
pub(super) fn origin_name(origin: SsaOrigin) -> String {
    match origin {
        SsaOrigin::Entry => "entry".into(),
        SsaOrigin::Instruction(pc) => format!("{pc:#x}"),
        SsaOrigin::Phi(block) => format!("B{block}"),
        SsaOrigin::Barrier(pc) => format!("unknown({pc:#x})"),
    }
}

/// The kind of origin a value carries.
///
/// The name of an origin already distinguishes an entry value, an
/// instruction, a merge and an unknown barrier. The kind states the same
/// fact for a consumer that stores the two separately.
pub(super) fn origin_kind(origin: SsaOrigin) -> &'static str {
    match origin {
        SsaOrigin::Entry => "entry",
        SsaOrigin::Instruction(_) => "instruction",
        SsaOrigin::Phi(_) => "phi",
        SsaOrigin::Barrier(_) => "unknown-barrier",
    }
}

/// One merged value at a control-flow join.
pub(super) struct FilePhi {
    /// The location the merge is for.
    pub location: FileLocation,
    /// The merged value.
    pub value: FileValue,
    /// The values the predecessors supply.
    pub incoming: Vec<(Option<usize>, FileValue)>,
}

/// The flag file values at one instruction.
pub(super) struct FlagSite {
    /// The flag bits read, with the value each read observed.
    pub uses: Vec<(u8, FileValue)>,
    /// The flag bits written, with the value each write produced.
    pub defs: Vec<(u8, FileValue)>,
}

/// The vector file values at one instruction.
pub(super) struct VectorSite {
    /// The registers read, with the value each read observed.
    pub uses: Vec<(u8, FileValue)>,
    /// The registers written, with the value each write produced.
    pub defs: Vec<(u8, FileValue)>,
}

/// The value identity of the flag file and the vector file.
pub(super) struct FileSsa {
    /// The merges at each block, one per flag bit or vector register.
    pub phis: Vec<Vec<FilePhi>>,
    /// The flag file values at each instruction.
    pub flags: BTreeMap<u64, FlagSite>,
    /// The vector file values at each instruction.
    pub vectors: BTreeMap<u64, VectorSite>,
}

/// A set of register-file slots.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SlotMask {
    words: Vec<u64>,
}

impl SlotMask {
    fn new() -> Self {
        Self { words: vec![0; SLOT_COUNT.div_ceil(64)] }
    }

    fn set(&mut self, slot: usize) {
        self.words[slot / 64] |= 1u64 << (slot % 64);
    }

    fn contains(&self, slot: usize) -> bool {
        self.words[slot / 64] & (1u64 << (slot % 64)) != 0
    }

    fn union(&mut self, other: &SlotMask) {
        for (word, other_word) in self.words.iter_mut().zip(other.words.iter()) {
            *word |= *other_word;
        }
    }

    fn difference(&mut self, other: &SlotMask) {
        for (word, other_word) in self.words.iter_mut().zip(other.words.iter()) {
            *word &= !*other_word;
        }
    }
}

/// One slot merge at a control-flow join.
struct SlotPhi {
    /// The slot the merge is for.
    slot: usize,
    /// The merged value. It starts as the merge itself and is refined to the
    /// common value when every predecessor agrees.
    value: SsaOrigin,
    /// The values the predecessors supply.
    incoming: Vec<(Option<usize>, SsaOrigin)>,
}

/// The slots one instruction reads and writes in the two files.
struct InstructionEffects {
    /// The flag bits read.
    flag_uses: Vec<u8>,
    /// The flag bits written.
    flag_defines: Vec<u8>,
    /// The vector ranges read.
    vector_uses: Vec<files::VectorRange>,
    /// The vector ranges written.
    vector_defines: Vec<files::VectorRange>,
    /// Every value the instruction writes is unknown rather than computed.
    barrier: bool,
}

impl InstructionEffects {
    /// The slots the instruction reads.
    fn use_slots(&self) -> Vec<usize> {
        let mut slots = Vec::new();
        for bit in &self.flag_uses { slots.push(flag_slot(*bit)); }
        for range in &self.vector_uses {
            for offset in range.offset..range.offset + range.bytes {
                slots.push(vector_slot(range.register, offset));
            }
        }
        slots
    }

    /// The slots the instruction writes.
    fn define_slots(&self) -> Vec<usize> {
        let mut slots = Vec::new();
        for bit in &self.flag_defines { slots.push(flag_slot(*bit)); }
        for range in &self.vector_defines {
            for offset in range.offset..range.offset + range.bytes {
                slots.push(vector_slot(range.register, offset));
            }
        }
        slots
    }
}

/// The flag and vector effects of one instruction.
fn instruction_effects(instruction: &LiftedInstruction) -> InstructionEffects {
    let flags = files::instruction_flag_effects(instruction);
    let vectors = files::instruction_vector_effects(instruction);
    let mut flag_uses = Vec::new();
    let mut flag_defines = Vec::new();
    // The published flag order is the architectural one: `N`, `Z`, `C`, `V`.
    for bit in [files::FLAG_BIT_N, files::FLAG_BIT_Z, files::FLAG_BIT_C, files::FLAG_BIT_V] {
        if flags.uses & (1 << bit) != 0 { flag_uses.push(bit); }
        if flags.defines & (1 << bit) != 0 { flag_defines.push(bit); }
    }
    let barrier = instruction.data
        || matches!(instruction.portable, IrOperation::TargetSpecific
            | IrOperation::DirectCall { .. } | IrOperation::IndirectCall { .. });
    InstructionEffects {
        flag_uses,
        flag_defines,
        vector_uses: vectors.reads.clone(),
        vector_defines: vectors.writes.clone(),
        barrier,
    }
}

/// The value identity of the flag file and the vector file.
pub(super) fn build_file_ssa(function: &FunctionIr) -> FileSsa {
    let blocks = &function.cfg.blocks;
    let mut result = FileSsa {
        phis: (0..blocks.len()).map(|_| Vec::new()).collect(),
        flags: BTreeMap::new(),
        vectors: BTreeMap::new(),
    };
    if blocks.is_empty() { return result; }

    let effects: Vec<InstructionEffects> = function.instructions.iter().map(instruction_effects).collect();
    let mut block_defines: Vec<SlotMask> = blocks.iter().map(|_| SlotMask::new()).collect();
    let mut block_uses: Vec<SlotMask> = blocks.iter().map(|_| SlotMask::new()).collect();
    for (index, block) in blocks.iter().enumerate().filter(|(_, block)| block.reachable) {
        for &pc in &block.instructions {
            let site = &effects[instruction_index(function, pc)];
            for slot in site.use_slots() { block_uses[index].set(slot); }
            for slot in site.define_slots() { block_defines[index].set(slot); }
        }
    }

    // Liveness: a slot is live when a later instruction may still read it.
    let mut live_in: Vec<SlotMask> = blocks.iter().map(|_| SlotMask::new()).collect();
    loop {
        let mut changed = false;
        for index in (0..blocks.len()).rev().filter(|&index| blocks[index].reachable) {
            let mut live_out = SlotMask::new();
            for &successor in &blocks[index].successors { live_out.union(&live_in[successor]); }
            let mut next = live_out.clone();
            next.difference(&block_defines[index]);
            next.union(&block_uses[index]);
            if next != live_in[index] {
                live_in[index] = next;
                changed = true;
            }
        }
        if !changed { break; }
    }

    // Phi placement: a merge is needed where a slot reaches a join from more
    // than one path.
    let mut slot_phis: Vec<Vec<SlotPhi>> = blocks.iter().map(|_| Vec::new()).collect();
    for slot in 0..SLOT_COUNT {
        let mut work = VecDeque::from([0usize]);
        let mut visited = BTreeSet::from([0usize]);
        for (index, block) in blocks.iter().enumerate() {
            if block.reachable && block_defines[index].contains(slot) && visited.insert(index) {
                work.push_back(index);
            }
        }
        let mut placed = BTreeSet::new();
        while let Some(index) = work.pop_front() {
            for &frontier in &blocks[index].dominance_frontiers {
                if !live_in[frontier].contains(slot) || !placed.insert(frontier) { continue; }
                slot_phis[frontier].push(SlotPhi {
                    slot,
                    value: SsaOrigin::Phi(frontier),
                    incoming: Vec::new(),
                });
                if visited.insert(frontier) { work.push_back(frontier); }
            }
        }
    }

    // Renaming over the dominator tree. The deepest strict dominator is the
    // parent, irrespective of address order.
    let mut children = vec![Vec::new(); blocks.len()];
    for (index, block) in blocks.iter().enumerate().skip(1).filter(|(_, block)| block.reachable) {
        if let Some(&parent) = block.dominators.iter().filter(|&&dominator| dominator != index)
            .max_by_key(|&&dominator| blocks[dominator].dominators.len()) {
            children[parent].push(index);
        }
    }
    let mut current = vec![SsaOrigin::Entry; SLOT_COUNT];
    // The values each instruction observed and produced, kept per slot so a
    // merge can be refined to the value every path agrees on.
    let mut observed: BTreeMap<u64, Vec<(usize, SsaOrigin)>> = BTreeMap::new();
    let mut produced: BTreeMap<u64, Vec<(usize, SsaOrigin)>> = BTreeMap::new();
    // The whole register state after an instruction that writes a vector
    // register, so a narrow write can show the part it preserved.
    let mut register_state: BTreeMap<u64, Vec<(u8, Vec<SsaOrigin>)>> = BTreeMap::new();
    enum Visit { Enter(usize), Exit(Vec<SsaOrigin>) }
    let mut pending = vec![Visit::Enter(0)];
    while let Some(visit) = pending.pop() {
        let index = match visit {
            Visit::Exit(saved) => { current = saved; continue; }
            Visit::Enter(index) => index,
        };
        pending.push(Visit::Exit(current.clone()));
        for phi in &mut slot_phis[index] {
            if index == 0 {
                // The synthetic entry value covers the path that reaches the
                // entry block without a predecessor.
                phi.incoming.push((None, SsaOrigin::Entry));
            }
            current[phi.slot] = phi.value;
        }
        for &pc in &blocks[index].instructions {
            let site = &effects[instruction_index(function, pc)];
            let uses: Vec<(usize, SsaOrigin)> = site.use_slots()
                .into_iter().map(|slot| (slot, current[slot])).collect();
            observed.insert(pc, uses);
            let defs: Vec<(usize, SsaOrigin)> = site.define_slots().into_iter().map(|slot| {
                let origin = if site.barrier { SsaOrigin::Barrier(pc) } else { SsaOrigin::Instruction(pc) };
                current[slot] = origin;
                (slot, origin)
            }).collect();
            produced.insert(pc, defs);
            let mut written: Vec<(u8, Vec<SsaOrigin>)> = Vec::new();
            for range in &site.vector_defines {
                if written.iter().any(|(register, _)| *register == range.register) { continue; }
                let state = (0..VECTOR_BYTES).map(|offset| current[vector_slot(range.register, offset)]).collect();
                written.push((range.register, state));
            }
            register_state.insert(pc, written);
        }
        for &successor in &blocks[index].successors {
            for phi in &mut slot_phis[successor] {
                phi.incoming.push((Some(index), current[phi.slot]));
            }
        }
        pending.extend(children[index].iter().rev().map(|&child| Visit::Enter(child)));
    }

    // A merge whose predecessors agree is not a merge: the value survives the
    // join unchanged. The recorded values are rewritten so that no use keeps
    // naming the merge.
    let mut trivial: BTreeMap<(usize, usize), SsaOrigin> = BTreeMap::new();
    for (block_index, phis) in slot_phis.iter().enumerate() {
        for phi in phis {
            if phi.incoming.is_empty() { continue; }
            let first = phi.incoming[0].1;
            if phi.incoming.iter().all(|(_, origin)| *origin == first) {
                trivial.insert((phi.slot, block_index), first);
            }
        }
    }
    let resolve = |slot: usize, origin: SsaOrigin| -> SsaOrigin {
        let mut current = origin;
        // The chain is acyclic by construction; the bound keeps a malformed
        // chain finite.
        for _ in 0..64 {
            let SsaOrigin::Phi(block) = current else { break };
            match trivial.get(&(slot, block)) {
                Some(next) if *next != current => current = *next,
                _ => break,
            }
        }
        current
    };
    for phis in &mut slot_phis {
        for phi in phis {
            phi.value = resolve(phi.slot, phi.value);
            for (_, origin) in &mut phi.incoming {
                *origin = resolve(phi.slot, *origin);
            }
        }
    }
    for values in observed.values_mut().chain(produced.values_mut()) {
        for (slot, origin) in values.iter_mut() {
            *origin = resolve(*slot, *origin);
        }
    }

    // Publish the flag file values.
    for (index, instruction) in function.instructions.iter().enumerate() {
        let pc = instruction.address;
        let site = &effects[index];
        let flag_uses: Vec<(u8, FileValue)> = site.flag_uses.iter().map(|bit| {
            let origin = observed.get(&pc).and_then(|values| values.iter()
                .find(|(slot, _)| *slot == flag_slot(*bit)).map(|(_, origin)| *origin)).unwrap_or(SsaOrigin::Entry);
            (*bit, FileValue::whole(1, origin))
        }).collect();
        let flag_defs: Vec<(u8, FileValue)> = site.flag_defines.iter().map(|bit| {
            let origin = produced.get(&pc).and_then(|values| values.iter()
                .find(|(slot, _)| *slot == flag_slot(*bit)).map(|(_, origin)| *origin)).unwrap_or(SsaOrigin::Entry);
            (*bit, FileValue::whole(1, origin))
        }).collect();
        if !flag_uses.is_empty() || !flag_defs.is_empty() {
            result.flags.insert(pc, FlagSite { uses: flag_uses, defs: flag_defs });
        }
    }

    // Publish the vector file values.
    for (index, instruction) in function.instructions.iter().enumerate() {
        let pc = instruction.address;
        let site = &effects[index];
        let mut vector_uses = Vec::new();
        for range in &site.vector_uses {
            let origins: Vec<SsaOrigin> = (0..range.bytes).map(|offset| {
                observed.get(&pc).and_then(|values| values.iter()
                    .find(|(slot, _)| *slot == vector_slot(range.register, range.offset + offset)).map(|(_, origin)| *origin))
                    .unwrap_or(SsaOrigin::Entry)
            }).collect();
            vector_uses.push((range.register, group_segments(range.offset, &origins)));
        }
        let mut vector_defs = Vec::new();
        if let Some(state) = register_state.get(&pc) {
            for (register, origins) in state {
                vector_defs.push((*register, group_segments(0, origins)));
            }
        }
        if !vector_uses.is_empty() || !vector_defs.is_empty() {
            result.vectors.insert(pc, VectorSite { uses: vector_uses, defs: vector_defs });
        }
    }

    // Publish the merges, one per flag bit and vector register.
    for (block_index, phis) in slot_phis.iter().enumerate() {
        let mut merges: Vec<FilePhi> = Vec::new();
        for phi in phis {
            match slot_location(phi.slot) {
                FileLocation::Flag(bit) => {
                    let value = FileValue::whole(1, phi.value);
                    let incoming = phi.incoming.iter()
                        .map(|(predecessor, origin)| (*predecessor, FileValue::whole(1, *origin)))
                        .collect();
                    merges.push(FilePhi { location: FileLocation::Flag(bit), value, incoming });
                }
                FileLocation::Vector(register) => {
                    // A register merge covers the bytes that reach the join
                    // from more than one path. The bytes only one path writes
                    // keep their own identity.
                    let mut bytes = Vec::new();
                    for offset in 0..VECTOR_BYTES {
                        let slot = vector_slot(register, offset);
                        if phis.iter().any(|other| other.slot == slot) { bytes.push(offset); }
                    }
                    if bytes.is_empty() { continue; }
                    let value = group_runs(&bytes.iter().map(|offset| {
                        let origin = phis.iter().find(|other| other.slot == vector_slot(register, *offset))
                            .map(|other| other.value).unwrap_or(phi.value);
                        (*offset, origin)
                    }).collect::<Vec<_>>());
                    let predecessors: Vec<Option<usize>> = phi.incoming.iter().map(|(predecessor, _)| *predecessor).collect();
                    let incoming = predecessors.iter().map(|predecessor| {
                        let origins = bytes.iter().map(|offset| {
                            let other = phis.iter().find(|candidate| candidate.slot == vector_slot(register, *offset));
                            other.and_then(|other| other.incoming.iter()
                                .find(|(source, _)| source == predecessor).map(|(_, origin)| *origin))
                                .unwrap_or(SsaOrigin::Entry)
                        }).collect::<Vec<_>>();
                        (*predecessor, group_runs(&bytes.iter().zip(origins)
                            .map(|(offset, origin)| (*offset, origin)).collect::<Vec<_>>()))
                    }).collect();
                    // Each byte slot contributes a phi internally, but the
                    // register-level view above has already grouped every
                    // merged byte range. Publish one identical register phi,
                    // rather than duplicating the same aggregate once per
                    // participating byte.
                    if !merges.iter().any(|existing: &FilePhi| {
                        existing.location == FileLocation::Vector(register)
                            && existing.value == value
                            && existing.incoming == incoming
                    }) {
                        merges.push(FilePhi { location: FileLocation::Vector(register), value, incoming });
                    }
                }
            }
        }
        result.phis[block_index] = merges;
    }

    result
}

/// The index of the instruction at an address.
fn instruction_index(function: &FunctionIr, pc: u64) -> usize {
    ((pc - function.start) / 4) as usize
}

/// The location a slot belongs to.
fn slot_location(slot: usize) -> FileLocation {
    if slot < FLAG_BITS {
        FileLocation::Flag(slot as u8)
    } else {
        FileLocation::Vector(((slot - FLAG_BITS) / VECTOR_BYTES as usize) as u8)
    }
}

/// Group consecutive slots that share one origin into byte ranges.
fn group_segments(offset: u8, origins: &[SsaOrigin]) -> FileValue {
    group_runs(&origins.iter().enumerate()
        .map(|(index, origin)| (offset + index as u8, *origin)).collect::<Vec<_>>())
}

/// Group consecutive byte offsets that share one origin into byte ranges.
fn group_runs(runs: &[(u8, SsaOrigin)]) -> FileValue {
    let mut segments: Vec<FileSegment> = Vec::new();
    for (offset, origin) in runs {
        match segments.last_mut() {
            Some(segment) if segment.origin == *origin && segment.offset + segment.bytes == *offset => {
                segment.bytes += 1;
            }
            _ => segments.push(FileSegment { offset: *offset, bytes: 1, origin: *origin }),
        }
    }
    FileValue { segments }
}

/// The published names of the flag file values at one instruction.
pub(super) fn flag_value_names(site: &FlagSite) -> (Vec<String>, Vec<String>) {
    let uses = site.uses.iter().map(|(bit, value)| value.name(FileLocation::Flag(*bit))).collect();
    let defs = site.defs.iter().map(|(bit, value)| value.name(FileLocation::Flag(*bit))).collect();
    (uses, defs)
}

/// The published names of the vector file values.
///
/// Consecutive registers that carry the same byte ranges from the same
/// origins are published as one range, so a call that clobbers the
/// caller-saved registers does not fill the preview with 32 entries.
pub(super) fn vector_value_names(entries: &[(u8, FileValue)]) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let (register, value) = &entries[index];
        let mut last = *register;
        while index + 1 < entries.len() {
            let (next_register, next_value) = &entries[index + 1];
            if *next_register != last + 1 || next_value.segments != value.segments { break; }
            last = *next_register;
            index += 1;
        }
        let segments = segments_name(&value.segments);
        if last == *register {
            names.push(format!("v{register}{segments}"));
        } else {
            names.push(format!("v{register}-v{last}{segments}"));
        }
        index += 1;
    }
    names
}
