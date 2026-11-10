//! Deterministic, bounded C167 execution core.
//!
//! This deliberately executes only instruction forms whose state transition is
//! modeled here. The decoder accepting an instruction does not imply the emulator
//! supports it. Unsupported instructions stop before changing architectural state.

use super::c167;
use std::collections::{BTreeMap, BTreeSet};

const MAX_IMAGE: usize = 2 * 1024 * 1024;
const MAX_STEPS: u64 = 1_000_000;
const MAX_TRACE: usize = 4096;
const MAX_RAM_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    Budget,
    Breakpoint(u32),
    Watchpoint { address: u32, access: WatchAccess },
    ArithmeticFault { pc: u32, reason: &'static str },
    OutOfBounds(u32),
    Unsupported { pc: u32, text: String },
    StackFault,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchAccess { Read, Write }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    pub pc: u32,
    pub bytes: Vec<u8>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub carry: bool,
    pub zero: bool,
    pub negative: bool,
    pub overflow: bool,
}

pub struct Machine {
    pub registers: [u16; 16],
    pub dpp: [u16; 4],
    pub cp: u16,
    pub csp: u8,
    pub mdh: u16,
    pub mdl: u16,
    pub pc: u32,
    pub sp: u32,
    pub steps: u64,
    pub flags: Flags,
    memory: Vec<u8>,
    ram: BTreeMap<u32, u8>,
    ram_seed_bytes: usize,
    ram_writes: BTreeSet<u32>,
    stack: Vec<u8>,
    trace: Vec<TraceEntry>,
    register_banks: [[u16; 16]; 64],
    override_page: Option<u16>,
    override_instructions: u8,
    watchpoint: Option<u32>,
    watch_hit: Option<(u32, WatchAccess)>,
}

impl Machine {
    pub fn new(image: &[u8], entry: u32, stack_top: u32) -> Result<Self, Stop> {
        if image.is_empty() || image.len() > MAX_IMAGE || entry as usize >= image.len() {
            return Err(Stop::OutOfBounds(entry));
        }
        if !(2..=0x1_0000).contains(&stack_top) { return Err(Stop::StackFault); }
        Ok(Self { registers: [0; 16], dpp: [0, 1, 2, 3], cp: 0xfc00,
                  csp: ((entry >> 16) & 0xff) as u8, mdh: 0, mdl: 0,
                  pc: entry, sp: stack_top, steps: 0, flags: Flags::default(),
                  memory: image.to_vec(), ram: BTreeMap::new(), ram_seed_bytes: 0,
                  ram_writes: BTreeSet::new(), stack: vec![0; 0x1_0000], trace: vec![],
                  register_banks: [[0; 16]; 64], override_page: None, override_instructions: 0,
                  watchpoint: None, watch_hit: None })
    }

    pub fn trace(&self) -> &[TraceEntry] { &self.trace }

    pub fn seed_ram(&mut self, spans: &[(u32, Vec<u8>)]) -> Result<(), Stop> {
        let mut staged = self.ram.clone();
        for (start, bytes) in spans {
            let end = start.checked_add(bytes.len() as u32).ok_or(Stop::OutOfBounds(*start))?;
            if end > 0x0100_0000 { return Err(Stop::OutOfBounds(end)); }
            for (offset, value) in bytes.iter().enumerate() {
                let address = start + offset as u32;
                if staged.insert(address, *value).is_some() { return Err(Stop::OutOfBounds(address)); }
            }
        }
        if staged.len() > MAX_RAM_BYTES { return Err(Stop::OutOfBounds(0x0100_0000)); }
        self.ram_seed_bytes = staged.len();
        self.ram = staged;
        Ok(())
    }

    pub fn ram_seed_bytes(&self) -> usize { self.ram_seed_bytes }
    pub fn ram_write_bytes(&self) -> usize { self.ram_writes.len() }
    pub fn ram_preview(&self) -> Vec<(u32, u8)> {
        self.ram.iter().take(256).map(|(address, value)| (*address, *value)).collect()
    }
    pub fn ram_truncated(&self) -> bool { self.ram.len() > 256 }

    pub fn run(&mut self, budget: u64) -> Stop {
        self.run_until(budget, None)
    }

    pub fn run_until(&mut self, budget: u64, breakpoint: Option<u32>) -> Stop {
        self.run_observed(budget, breakpoint, None)
    }

    pub fn run_observed(&mut self, budget: u64, breakpoint: Option<u32>, watchpoint: Option<u32>) -> Stop {
        self.watchpoint = watchpoint;
        let limit = budget.min(MAX_STEPS);
        for _ in 0..limit {
            if breakpoint == Some(self.pc) { return Stop::Breakpoint(self.pc); }
            self.watch_hit = None;
            if let Some(stop) = self.step() { return stop; }
            if let Some((address, access)) = self.watch_hit { return Stop::Watchpoint { address, access }; }
        }
        Stop::Budget
    }

    pub fn step(&mut self) -> Option<Stop> {
        let at = self.pc as usize;
        if at >= self.memory.len() { return Some(Stop::OutOfBounds(self.pc)); }
        let end = (at + 4).min(self.memory.len());
        let bytes = &self.memory[at..end];
        let Some((length, text)) = c167::instruction(bytes, u64::from(self.pc)) else {
            return Some(Stop::Unsupported { pc: self.pc, text: "undecoded".into() });
        };
        let raw = bytes[..length].to_vec();
        let override_was_active = self.override_instructions > 0;
        if self.trace.len() < MAX_TRACE {
            self.trace.push(TraceEntry { pc: self.pc, bytes: raw.clone(), text: text.clone() });
        }
        let next = self.pc.checked_add(length as u32).ok_or(Stop::OutOfBounds(self.pc));
        let next = match next { Ok(value) => value, Err(stop) => return Some(stop) };
        let op = raw[0];
        match op {
            0xE0 => self.registers[(raw[1] & 0x0f) as usize] = u16::from(raw[1] >> 4),
            0xE1 => self.set_byte_register(raw[1] & 0x0f, raw[1] >> 4),
            0xF0 => self.registers[(raw[1] >> 4) as usize] = self.registers[(raw[1] & 0x0f) as usize],
            0xF1 => {
                let value = self.byte_register(raw[1] & 0x0f);
                self.set_byte_register(raw[1] >> 4, value);
            }
            0xE6 => {
                let value = u16::from_le_bytes([raw[2], raw[3]]);
                if raw[1] >= 0xF0 { self.registers[(raw[1] & 0x0f) as usize] = value; }
                else if raw[1] < 4 { self.dpp[raw[1] as usize] = value & 0x03ff; }
                else if raw[1] == 6 { self.mdh = value; }
                else if raw[1] == 7 { self.mdl = value; }
                else if raw[1] == 8 { self.set_cp(value); }
                else if raw[1] == 9 && value >= 2 { self.sp = u32::from(value); }
                else { return Some(Stop::Unsupported { pc: self.pc, text }); }
            }
            0xF2 if raw[1] >= 0xF0 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                let Some(value) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.registers[(raw[1] & 0x0f) as usize] = value;
            }
            0xF3 if raw[1] >= 0xF0 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                let Some(value) = self.read_byte(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.set_byte_register(raw[1] & 0x0f, value);
            }
            0xF6 if raw[1] >= 0xF0 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                if self.write_word(address, self.registers[(raw[1] & 0x0f) as usize]).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
            }
            0xF7 if raw[1] >= 0xF0 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                if self.write_byte(address, self.byte_register(raw[1] & 0x0f)).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
            }
            op if op & 0x0f == 6 && op >> 4 <= 7 && raw[1] >= 0xF0 => {
                let source = u16::from_le_bytes([raw[2], raw[3]]);
                self.alu_word(op >> 4, (raw[1] & 0x0f) as usize, source);
            }
            op if op & 0x0f == 1 && op >> 4 <= 7 => {
                let destination = raw[1] >> 4;
                let source = self.byte_register(raw[1] & 0x0f);
                self.alu_byte(op >> 4, destination, source);
            }
            op if op & 0x0f == 2 && op >> 4 <= 7 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                let Some(source) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.alu_word(op >> 4, (raw[1] >> 4) as usize, source);
            }
            op if op & 0x0f == 4 && op >> 4 <= 7 && op >> 4 != 4 => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                let Some(left) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                let source = self.registers[(raw[1] >> 4) as usize];
                let (result, flags, _) = Self::alu_word_result(op >> 4, left, source, self.flags.carry);
                self.flags = flags;
                if self.write_word(address, result).is_none() { return Some(Stop::OutOfBounds(self.physical(address))); }
            }
            0xA8 => {
                let destination = (raw[1] >> 4) as usize;
                let address = self.registers[(raw[1] & 0x0f) as usize];
                let Some(value) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.registers[destination] = value;
            }
            0xB8 => {
                let address = self.registers[(raw[1] & 0x0f) as usize];
                if self.write_word(address, self.registers[(raw[1] >> 4) as usize]).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
            }
            0x98 => {
                let pointer = (raw[1] & 0x0f) as usize;
                let destination = (raw[1] >> 4) as usize;
                let address = self.registers[pointer];
                let Some(value) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.registers[destination] = value;
                self.registers[pointer] = address.wrapping_add(2);
            }
            0x88 => {
                let pointer = (raw[1] & 0x0f) as usize;
                let address = self.registers[pointer].wrapping_sub(2);
                if self.write_word(address, self.registers[(raw[1] >> 4) as usize]).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
                self.registers[pointer] = address;
            }
            0xA9 => {
                let destination = raw[1] >> 4;
                let address = self.registers[(raw[1] & 0x0f) as usize];
                let Some(value) = self.read_byte(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.set_byte_register(destination, value);
            }
            0xB9 => {
                let address = self.registers[(raw[1] & 0x0f) as usize];
                if self.write_byte(address, self.byte_register(raw[1] >> 4)).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
            }
            0x99 => {
                let pointer = (raw[1] & 0x0f) as usize;
                let address = self.registers[pointer];
                let Some(value) = self.read_byte(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                self.set_byte_register(raw[1] >> 4, value);
                self.registers[pointer] = address.wrapping_add(1);
            }
            0x89 => {
                let pointer = (raw[1] & 0x0f) as usize;
                let address = self.registers[pointer].wrapping_sub(1);
                if self.write_byte(address, self.byte_register(raw[1] >> 4)).is_none() {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                }
                self.registers[pointer] = address;
            }
            0xC4 | 0xD4 | 0xE4 | 0xF4 => {
                let register = raw[1] >> 4;
                let base = self.registers[(raw[1] & 0x0f) as usize];
                let address = base.wrapping_add(u16::from_le_bytes([raw[2], raw[3]]));
                match op {
                    0xC4 if self.write_word(address, self.registers[register as usize]).is_some() => {}
                    0xD4 => {
                        let Some(value) = self.read_word(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                        self.registers[register as usize] = value;
                    }
                    0xE4 if self.write_byte(address, self.byte_register(register)).is_some() => {}
                    0xF4 => {
                        let Some(value) = self.read_byte(address) else { return Some(Stop::OutOfBounds(self.physical(address))); };
                        self.set_byte_register(register, value);
                    }
                    _ => return Some(Stop::OutOfBounds(self.physical(address))),
                }
            }
            0x0C | 0x2C | 0x4C | 0x6C | 0xAC => {
                let destination = (raw[1] >> 4) as usize;
                let count = (self.registers[(raw[1] & 0x0f) as usize] & 0x0f) as u8;
                self.shift_word(op, destination, count);
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xBC => {
                self.shift_word(op, (raw[1] & 0x0f) as usize, raw[1] >> 4);
            }
            0x0B | 0x1B => {
                self.multiply(op == 0x0B, (raw[1] >> 4) as usize, (raw[1] & 0x0f) as usize);
            }
            0x4B | 0x5B | 0x6B | 0x7B if raw[1] >> 4 == raw[1] & 0x0f => {
                if let Err(stop) = self.divide(op, (raw[1] & 0x0f) as usize) { return Some(stop); }
            }
            0x80 | 0x90 | 0xA0 | 0xB0 => {
                self.compare_adjust(op, usize::from(raw[1] & 0x0f), u16::from(raw[1] >> 4));
            }
            0x82 | 0x92 | 0xA2 | 0xB2 if raw[1] >> 4 == 0x0f => {
                let address = u16::from_le_bytes([raw[2], raw[3]]);
                let Some(source) = self.read_word(address) else {
                    return Some(Stop::OutOfBounds(self.physical(address)));
                };
                self.compare_adjust(op, usize::from(raw[1] & 0x0f), source);
            }
            0x86 | 0x96 | 0xA6 | 0xB6 if raw[1] >> 4 == 0x0f => {
                let source = u16::from_le_bytes([raw[2], raw[3]]);
                self.compare_adjust(op, usize::from(raw[1] & 0x0f), source);
            }
            op if op & 0x0f == 0 && op >> 4 <= 7 => {
                let destination = (raw[1] >> 4) as usize;
                let source = self.registers[(raw[1] & 0x0f) as usize];
                self.alu_word(op >> 4, destination, source);
            }
            op if op & 0x0f == 8 && op >> 4 <= 7 => {
                let destination = (raw[1] >> 4) as usize;
                let source = match self.short_word_operand(raw[1] & 0x0f) {
                    Ok(value) => value,
                    Err(stop) => return Some(stop),
                };
                self.alu_word(op >> 4, destination, source);
            }
            op if op & 0x0f == 9 && op >> 4 <= 7 => {
                let destination = raw[1] >> 4;
                let source = match self.short_byte_operand(raw[1] & 0x0f) {
                    Ok(value) => value,
                    Err(stop) => return Some(stop),
                };
                self.alu_byte(op >> 4, destination, source);
            }
            0xD7 if raw[1] >> 6 == 1 && raw[1] & 0x0f == 0 => {
                self.override_page = Some((u16::from(raw[3] & 3) << 8) | u16::from(raw[2]));
                self.override_instructions = ((raw[1] >> 4) & 3) + 1;
            }
            0xDC if raw[1] >> 6 == 1 => {
                self.override_page = Some(self.registers[(raw[1] & 0x0f) as usize] & 0x03ff);
                self.override_instructions = ((raw[1] >> 4) & 3) + 1;
            }
            op if op & 0x0f == 0x0d => {
                if self.condition(op >> 4) {
                    let target = (i64::from(self.pc) + 2 + 2 * i64::from(raw[1] as i8)) as u32;
                    self.pc = (self.pc & 0x00ff_0000) | (target & 0xffff);
                } else {
                    self.pc = next;
                }
                self.finish_step(override_was_active);
                return None;
            }
            0xEA if raw[1] & 0x0f == 0 => {
                self.pc = if self.condition(raw[1] >> 4) {
                    (self.pc & 0x00ff_0000) | u32::from(u16::from_le_bytes([raw[2], raw[3]]))
                } else { next };
                self.finish_step(override_was_active);
                return None;
            }
            0xCA if raw[1] & 0x0f == 0 => {
                if self.condition(raw[1] >> 4) {
                    if self.push16(next as u16).is_err() { return Some(Stop::StackFault); }
                    self.pc = (self.pc & 0x00ff_0000) | u32::from(u16::from_le_bytes([raw[2], raw[3]]));
                } else { self.pc = next; }
                self.finish_step(override_was_active);
                return None;
            }
            0xBB => {
                if self.push16(next as u16).is_err() { return Some(Stop::StackFault); }
                let target = (i64::from(self.pc) + 2 + 2 * i64::from(raw[1] as i8)) as u32;
                self.pc = (self.pc & 0x00ff_0000) | (target & 0xffff);
                self.finish_step(override_was_active);
                return None;
            }
            0xCB if raw[1] == 0 => {
                let Some(return_pc) = self.pop16() else { return Some(Stop::StackFault); };
                self.pc = (self.pc & 0x00ff_0000) | u32::from(return_pc);
                self.finish_step(override_was_active);
                return None;
            }
            0xDA => {
                if self.push_far_return(self.csp, next as u16).is_err() {
                    return Some(Stop::StackFault);
                }
                self.csp = raw[1];
                self.pc = (u32::from(self.csp) << 16) | u32::from(u16::from_le_bytes([raw[2], raw[3]]));
                self.finish_step(override_was_active);
                return None;
            }
            0xDB if raw[1] == 0 => {
                let Some((return_csp, return_pc)) = self.pop_far_return() else {
                    return Some(Stop::StackFault);
                };
                self.csp = return_csp;
                self.pc = (u32::from(self.csp) << 16) | u32::from(return_pc);
                self.finish_step(override_was_active);
                return None;
            }
            0xEC if raw[1] >= 0xF0 => {
                if self.push16(self.registers[(raw[1] & 0x0f) as usize]).is_err() {
                    return Some(Stop::StackFault);
                }
            }
            0xFC if raw[1] >= 0xF0 => {
                let Some(value) = self.pop16() else { return Some(Stop::StackFault); };
                self.registers[(raw[1] & 0x0f) as usize] = value;
            }
            op if matches!(op & 0x0f, 0x0e | 0x0f) && raw[1] == 0x88 && op >> 4 <= 3 => {
                let set = op & 0x0f == 0x0f;
                match op >> 4 {
                    0 => self.flags.negative = set,
                    1 => self.flags.carry = set,
                    2 => self.flags.overflow = set,
                    _ => self.flags.zero = set,
                }
            }
            0xCC if raw[1] == 0x00 => {}
            0xEB if raw[1] >= 0xF0 => {
                let Some(return_pc) = self.pop16() else { return Some(Stop::StackFault); };
                let value = self.registers[(raw[1] & 0x0f) as usize];
                self.sp = u32::from(value.max(2));
                self.pc = (self.pc & 0x00ff_0000) | u32::from(return_pc);
                self.finish_step(override_was_active);
                return None;
            }
            0x81 => {
                let index = usize::from(raw[1] >> 4);
                let value = self.registers[index];
                let result = 0u16.wrapping_sub(value);
                self.registers[index] = result;
                self.flags = Flags { carry: value != 0, zero: result == 0, negative: result & 0x8000 != 0,
                    overflow: value == 0x8000 };
            }
            0x91 => {
                let index = usize::from(raw[1] >> 4);
                let result = !self.registers[index];
                self.registers[index] = result;
                self.unary_word_flags(result);
            }
            0xA1 => {
                let reg = raw[1] >> 4;
                let value = self.byte_register(reg);
                let result = 0u8.wrapping_sub(value);
                self.set_byte_register(reg, result);
                self.flags = Flags { carry: value != 0, zero: result == 0, negative: result & 0x80 != 0,
                    overflow: value == 0x80 };
            }
            0xB1 => {
                let reg = raw[1] >> 4;
                let result = !self.byte_register(reg);
                self.set_byte_register(reg, result);
                self.unary_byte_flags(result);
            }
            0xC0 => {
                let source = self.byte_register(raw[1] & 0x0f);
                self.registers[usize::from(raw[1] >> 4)] = u16::from(source);
            }
            0xD0 => {
                let source = self.byte_register(raw[1] & 0x0f);
                self.registers[usize::from(raw[1] >> 4)] = source as i8 as i16 as u16;
            }
            // SCXT requires sign-extended byte propagation semantics not yet admitted here.
            0x2B => {
                let source = self.registers[usize::from(raw[1] & 0x0f)];
                let value = if source == 0 { 0 } else { 15 - source.leading_zeros() as u16 };
                self.registers[usize::from(raw[1] >> 4)] = value;
                self.flags = Flags { carry: false, zero: source == 0, negative: false, overflow: false };
            }
            0x0A => {
                // BFLDL reg, #mask, #data
                let mask = u16::from(raw[2]);
                let data = u16::from(raw[3]) & 0x00ff;
                let current = if raw[1] >= 0xf0 { self.registers[usize::from(raw[1] & 0x0f)] }
                    else { match self.read_word(0xfe00 + 2 * u16::from(raw[1])) { Some(v) => v, None => return Some(Stop::OutOfBounds(0xfe00 + 2 * u32::from(raw[1]))) } };
                let result = (current & !mask) | (data & mask);
                if raw[1] >= 0xf0 { self.registers[usize::from(raw[1] & 0x0f)] = result; }
                else if self.write_word(0xfe00 + 2 * u16::from(raw[1]), result).is_none() { return Some(Stop::OutOfBounds(0xfe00 + 2 * u32::from(raw[1]))); }
            }
            0x1A => {
                let mask = u16::from(raw[3]) << 8;
                let data = u16::from(raw[2]) << 8;
                let current = if raw[1] >= 0xf0 { self.registers[usize::from(raw[1] & 0x0f)] }
                    else { match self.read_word(0xfe00 + 2 * u16::from(raw[1])) { Some(v) => v, None => return Some(Stop::OutOfBounds(0xfe00 + 2 * u32::from(raw[1]))) } };
                let result = (current & !mask) | (data & mask);
                if raw[1] >= 0xf0 { self.registers[usize::from(raw[1] & 0x0f)] = result; }
                else if self.write_word(0xfe00 + 2 * u16::from(raw[1]), result).is_none() { return Some(Stop::OutOfBounds(0xfe00 + 2 * u32::from(raw[1]))); }
            }
            op if op & 0x0f == 0x0e => {
                let bit = op >> 4;
                if self.set_bit_value(raw[1], bit, false).is_none() {
                    return Some(Stop::OutOfBounds(u32::from(self.bit_word_logical(raw[1]))));
                }
            }
            op if op & 0x0f == 0x0f => {
                let bit = op >> 4;
                if self.set_bit_value(raw[1], bit, true).is_none() {
                    return Some(Stop::OutOfBounds(u32::from(self.bit_word_logical(raw[1]))));
                }
            }
            0x8A | 0x9A | 0xAA | 0xBA if raw.len() == 4 && raw[3] & 0x0f == 0 => {
                let bit = raw[3] >> 4;
                let Some(value) = self.test_bit(raw[1], bit) else {
                    return Some(Stop::OutOfBounds(u32::from(self.bit_word_logical(raw[1]))));
                };
                let (jump, clear) = match op {
                    0x8A => (value, false),
                    0x9A => (!value, false),
                    0xAA => (value, true),
                    _ => (!value, true),
                };
                if clear && self.set_bit_value(raw[1], bit, false).is_none() {
                    return Some(Stop::OutOfBounds(u32::from(self.bit_word_logical(raw[1]))));
                }
                if jump {
                    let target = (i64::from(self.pc) + 4 + 2 * i64::from(raw[2] as i8)) as u32;
                    self.pc = (self.pc & 0x00ff_0000) | (target & 0xffff);
                } else {
                    self.pc = next;
                }
                self.finish_step(override_was_active);
                return None;
            }
            // Bounded modeled system-control sequences. Watchdog/service/init have no
            // peripheral model here and are treated as no-ops; halt/reset remain explicit.
            _ if matches!(text.as_str(), "diswdt" | "srvwdt" | "einit") => {}
            _ if matches!(text.as_str(), "idle" | "pwrdn" | "srst") => {
                return Some(Stop::Unsupported { pc: self.pc, text });
            }
            // RETS/RETI require segment/interrupt state not admitted by this slice.
            _ => return Some(Stop::Unsupported { pc: self.pc, text }),
        }
        self.pc = next;
        self.finish_step(override_was_active);
        None
    }

    fn finish_step(&mut self, override_was_active: bool) {
        if override_was_active {
            self.override_instructions -= 1;
            if self.override_instructions == 0 { self.override_page = None; }
        }
        self.steps += 1;
    }

    fn push16(&mut self, value: u16) -> Result<(), ()> {
        let next = self.sp.checked_sub(2).ok_or(())?;
        let at = next as usize;
        if at + 2 > self.stack.len() { return Err(()); }
        self.stack[at..at + 2].copy_from_slice(&value.to_le_bytes());
        self.sp = next;
        Ok(())
    }

    fn push_far_return(&mut self, csp: u8, ip: u16) -> Result<(), ()> {
        let next = self.sp.checked_sub(4).ok_or(())?;
        let at = next as usize;
        if at + 4 > self.stack.len() { return Err(()); }
        self.stack[at..at + 2].copy_from_slice(&ip.to_le_bytes());
        self.stack[at + 2..at + 4].copy_from_slice(&u16::from(csp).to_le_bytes());
        self.sp = next;
        Ok(())
    }

    fn set_cp(&mut self, value: u16) {
        let old_bank = usize::from(self.cp >> 10);
        self.register_banks[old_bank] = self.registers;
        self.cp = value & 0xfc00;
        self.registers = self.register_banks[usize::from(self.cp >> 10)];
    }

    fn physical(&self, logical: u16) -> u32 {
        if let Some(page) = self.override_page.filter(|_| self.override_instructions > 0) {
            return (u32::from(page & 0x03ff) << 14) | u32::from(logical & 0x3fff);
        }
        let selector = usize::from(logical >> 14);
        (u32::from(self.dpp[selector] & 0x03ff) << 14) | u32::from(logical & 0x3fff)
    }

    fn short_word_operand(&mut self, selector: u8) -> Result<u16, Stop> {
        if selector < 8 { return Ok(u16::from(selector)); }
        let pointer = usize::from(selector & 3);
        let address = self.registers[pointer];
        let value = self.read_word(address).ok_or_else(|| Stop::OutOfBounds(self.physical(address)))?;
        if selector >= 12 { self.registers[pointer] = address.wrapping_add(2); }
        Ok(value)
    }

    fn short_byte_operand(&mut self, selector: u8) -> Result<u8, Stop> {
        if selector < 8 { return Ok(selector); }
        let pointer = usize::from(selector & 3);
        let address = self.registers[pointer];
        let value = self.read_byte(address).ok_or_else(|| Stop::OutOfBounds(self.physical(address)))?;
        if selector >= 12 { self.registers[pointer] = address.wrapping_add(1); }
        Ok(value)
    }

    fn read_byte(&mut self, logical: u16) -> Option<u8> {
        let physical = self.physical(logical);
        if physical & 1 == 0 {
            if let Some(value) = self.read_core_sfr(physical) {
                self.record_access(physical, 1, WatchAccess::Read);
                return Some(value as u8);
            }
        }
        let value = self.ram.get(&physical).copied()
            .or_else(|| self.memory.get(physical as usize).copied())?;
        self.record_access(physical, 1, WatchAccess::Read);
        Some(value)
    }

    fn read_word(&mut self, logical: u16) -> Option<u16> {
        let physical = self.physical(logical);
        if let Some(value) = self.read_core_sfr(physical) {
            self.record_access(physical, 2, WatchAccess::Read);
            return Some(value);
        }
        let value = u16::from_le_bytes([
            self.ram.get(&physical).copied().or_else(|| self.memory.get(physical as usize).copied())?,
            self.ram.get(&(physical + 1)).copied().or_else(|| self.memory.get(physical as usize + 1).copied())?,
        ]);
        self.record_access(physical, 2, WatchAccess::Read);
        Some(value)
    }

    fn write_byte(&mut self, logical: u16, value: u8) -> Option<()> {
        let physical = self.physical(logical);
        if physical & 1 == 0 && self.write_core_sfr(physical, u16::from(value)) {
            self.record_access(physical, 1, WatchAccess::Write);
            return Some(());
        }
        if !self.ram.contains_key(&physical) && self.ram.len() >= MAX_RAM_BYTES { return None; }
        self.ram.insert(physical, value);
        self.ram_writes.insert(physical);
        self.record_access(physical, 1, WatchAccess::Write);
        Some(())
    }

    fn write_word(&mut self, logical: u16, value: u16) -> Option<()> {
        let physical = self.physical(logical);
        if self.write_core_sfr(physical, value) {
            self.record_access(physical, 2, WatchAccess::Write);
            return Some(());
        }
        if physical >= 0x00ff_ffff { return None; }
        let needed = usize::from(!self.ram.contains_key(&physical))
            + usize::from(!self.ram.contains_key(&(physical + 1)));
        if self.ram.len().checked_add(needed)? > MAX_RAM_BYTES { return None; }
        let bytes = value.to_le_bytes();
        self.ram.insert(physical, bytes[0]);
        self.ram.insert(physical + 1, bytes[1]);
        self.ram_writes.insert(physical);
        self.ram_writes.insert(physical + 1);
        self.record_access(physical, 2, WatchAccess::Write);
        Some(())
    }

    fn read_core_sfr(&self, physical: u32) -> Option<u16> {
        match physical {
            0xfe00..=0xfe06 if physical & 1 == 0 => Some(self.dpp[((physical - 0xfe00) / 2) as usize]),
            0xfe08 => Some(u16::from(self.csp)),
            0xfe0c => Some(self.mdh),
            0xfe0e => Some(self.mdl),
            0xfe10 => Some(self.cp),
            0xfe12 => u16::try_from(self.sp).ok(),
            0xff10 => Some(self.psw()),
            _ => None,
        }
    }

    fn write_core_sfr(&mut self, physical: u32, value: u16) -> bool {
        match physical {
            0xfe00..=0xfe06 if physical & 1 == 0 => self.dpp[((physical - 0xfe00) / 2) as usize] = value & 0x03ff,
            0xfe08 if value <= 0xff => self.csp = value as u8,
            0xfe0c => self.mdh = value,
            0xfe0e => self.mdl = value,
            0xfe10 => self.set_cp(value),
            0xfe12 if value >= 2 => self.sp = u32::from(value),
            0xff10 => self.set_psw(value),
            _ => return false,
        }
        true
    }

    fn psw(&self) -> u16 {
        u16::from(self.flags.negative)
            | (u16::from(self.flags.carry) << 1)
            | (u16::from(self.flags.overflow) << 2)
            | (u16::from(self.flags.zero) << 3)
    }

    fn set_psw(&mut self, value: u16) {
        self.flags.negative = value & 1 != 0;
        self.flags.carry = value & 2 != 0;
        self.flags.overflow = value & 4 != 0;
        self.flags.zero = value & 8 != 0;
    }

    fn record_access(&mut self, physical: u32, width: u32, access: WatchAccess) {
        if self.watchpoint.is_some_and(|watched| watched >= physical && watched < physical.saturating_add(width)) {
            self.watch_hit = self.watchpoint.map(|address| (address, access));
        }
    }

    fn byte_register(&self, register: u8) -> u8 {
        let word = self.registers[usize::from(register / 2)];
        if register & 1 == 0 { word as u8 } else { (word >> 8) as u8 }
    }

    fn set_byte_register(&mut self, register: u8, value: u8) {
        let word = &mut self.registers[usize::from(register / 2)];
        if register & 1 == 0 { *word = (*word & 0xff00) | u16::from(value); }
        else { *word = (*word & 0x00ff) | (u16::from(value) << 8); }
    }

    fn alu_word(&mut self, operation: u8, destination: usize, source: u16) {
        let left = self.registers[destination];
        let (result, flags, write) = Self::alu_word_result(operation, left, source, self.flags.carry);
        if write { self.registers[destination] = result; }
        self.flags = flags;
    }

    fn alu_word_result(operation: u8, left: u16, source: u16, carry: bool) -> (u16, Flags, bool) {
        let carry_in = u16::from(carry);
        let (result, carry, overflow, write) = match operation {
            0 => {
                let wide = u32::from(left) + u32::from(source);
                let result = wide as u16;
                (result, wide > 0xffff, ((left ^ result) & (source ^ result) & 0x8000) != 0, true)
            }
            1 => {
                let wide = u32::from(left) + u32::from(source) + u32::from(carry_in);
                let result = wide as u16;
                let adjusted = source.wrapping_add(carry_in);
                (result, wide > 0xffff, ((left ^ result) & (adjusted ^ result) & 0x8000) != 0, true)
            }
            2 | 4 => {
                let result = left.wrapping_sub(source);
                (result, left < source, ((left ^ source) & (left ^ result) & 0x8000) != 0, operation == 2)
            }
            3 => {
                let subtrahend = u32::from(source) + u32::from(carry_in);
                let result = left.wrapping_sub(subtrahend as u16);
                (result, u32::from(left) < subtrahend,
                 ((left ^ subtrahend as u16) & (left ^ result) & 0x8000) != 0, true)
            }
            5 => (left ^ source, false, false, true),
            6 => (left & source, false, false, true),
            7 => (left | source, false, false, true),
            _ => return (left, Flags::default(), false),
        };
        (result, Flags { carry, zero: result == 0, negative: result & 0x8000 != 0, overflow }, write)
    }

    fn alu_byte(&mut self, operation: u8, destination: u8, source: u8) {
        let left = self.byte_register(destination);
        let carry_in = u8::from(self.flags.carry);
        let (result, carry, overflow, write) = match operation {
            0 | 1 => {
                let addend = source.wrapping_add(if operation == 1 { carry_in } else { 0 });
                let wide = u16::from(left) + u16::from(source) + u16::from(if operation == 1 { carry_in } else { 0 });
                let result = wide as u8;
                (result, wide > 0xff, ((left ^ result) & (addend ^ result) & 0x80) != 0, true)
            }
            2 | 4 => {
                let result = left.wrapping_sub(source);
                (result, left < source, ((left ^ source) & (left ^ result) & 0x80) != 0, operation == 2)
            }
            3 => {
                let subtrahend = u16::from(source) + u16::from(carry_in);
                let result = left.wrapping_sub(subtrahend as u8);
                (result, u16::from(left) < subtrahend,
                 ((left ^ subtrahend as u8) & (left ^ result) & 0x80) != 0, true)
            }
            5 => (left ^ source, false, false, true),
            6 => (left & source, false, false, true),
            7 => (left | source, false, false, true),
            _ => return,
        };
        if write { self.set_byte_register(destination, result); }
        self.flags = Flags { carry, zero: result == 0, negative: result & 0x80 != 0, overflow };
    }

    fn compare_adjust(&mut self, operation: u8, register: usize, source: u16) {
        let original = self.registers[register];
        let (_, flags, _) = Self::alu_word_result(4, original, source, self.flags.carry);
        let amount = if operation & 0x10 == 0 { 1 } else { 2 };
        self.registers[register] = if operation & 0x20 == 0 {
            original.wrapping_add(amount)
        } else {
            original.wrapping_sub(amount)
        };
        self.flags = flags;
    }

    fn shift_word(&mut self, operation: u8, destination: usize, count: u8) {
        let mut result = self.registers[destination];
        let mut carry = false;
        let mut overflow = false;
        for _ in 0..count {
            if matches!(operation, 0x2C | 0x3C | 0x6C | 0x7C | 0xAC | 0xBC) {
                overflow |= carry;
                carry = result & 1 != 0;
                result = match operation {
                    0x2C | 0x3C => result.rotate_right(1),
                    0xAC | 0xBC => ((result as i16) >> 1) as u16,
                    _ => result >> 1,
                };
            } else {
                carry = result & 0x8000 != 0;
                result = if matches!(operation, 0x0C | 0x1C) {
                    result.rotate_left(1)
                } else { result << 1 };
            }
        }
        self.registers[destination] = result;
        self.flags = Flags { carry, zero: result == 0, negative: result & 0x8000 != 0, overflow };
    }

    fn multiply(&mut self, signed: bool, left: usize, right: usize) {
        let product = if signed {
            (i32::from(self.registers[left] as i16) * i32::from(self.registers[right] as i16)) as u32
        } else {
            u32::from(self.registers[left]) * u32::from(self.registers[right])
        };
        self.mdl = product as u16;
        self.mdh = (product >> 16) as u16;
        let overflow = if signed {
            let value = product as i32;
            value < i32::from(i16::MIN) || value > i32::from(i16::MAX)
        } else { product > u32::from(u16::MAX) };
        self.flags = Flags { carry: false, zero: product == 0, negative: product & 0x8000_0000 != 0, overflow };
    }

    fn divide(&mut self, operation: u8, divisor_register: usize) -> Result<(), Stop> {
        let divisor = self.registers[divisor_register];
        if divisor == 0 {
            return Err(Stop::ArithmeticFault { pc: self.pc, reason: "division by zero" });
        }
        let (quotient, remainder, overflow) = match operation {
            0x4B => {
                let dividend = i32::from(self.mdl as i16);
                let divisor = i32::from(divisor as i16);
                ((dividend / divisor) as i64, (dividend % divisor) as i64, false)
            }
            0x5B => (i64::from(self.mdl / divisor), i64::from(self.mdl % divisor), false),
            0x6B => {
                let dividend = i64::from(((u32::from(self.mdh) << 16) | u32::from(self.mdl)) as i32);
                let divisor = i64::from(divisor as i16);
                let quotient = dividend / divisor;
                (quotient, dividend % divisor, quotient < i64::from(i16::MIN) || quotient > i64::from(i16::MAX))
            }
            _ => {
                let dividend = (u32::from(self.mdh) << 16) | u32::from(self.mdl);
                let divisor = u32::from(divisor);
                let quotient = dividend / divisor;
                (i64::from(quotient), i64::from(dividend % divisor), quotient > u32::from(u16::MAX))
            }
        };
        if overflow {
            return Err(Stop::ArithmeticFault { pc: self.pc, reason: "division quotient overflow" });
        }
        self.mdl = quotient as u16;
        self.mdh = remainder as u16;
        self.flags = Flags { carry: false, zero: self.mdl == 0, negative: self.mdl & 0x8000 != 0, overflow: false };
        Ok(())
    }

    fn condition(&self, code: u8) -> bool {
        match code & 0x0f {
            0 => true,
            1 => false,
            2 => self.flags.zero,
            3 => !self.flags.zero,
            4 => self.flags.overflow,
            5 => !self.flags.overflow,
            6 => self.flags.negative,
            7 => !self.flags.negative,
            8 => self.flags.carry,
            9 => !self.flags.carry,
            10 => !self.flags.zero && self.flags.negative == self.flags.overflow,
            11 => self.flags.zero || self.flags.negative != self.flags.overflow,
            12 => self.flags.negative != self.flags.overflow,
            13 => self.flags.negative == self.flags.overflow,
            14 => !self.flags.carry && !self.flags.zero,
            _ => self.flags.carry || self.flags.zero,
        }
    }

    fn pop16(&mut self) -> Option<u16> {
        let at = self.sp as usize;
        if at + 2 > self.stack.len() { return None; }
        let value = u16::from_le_bytes([self.stack[at], self.stack[at + 1]]);
        self.sp = self.sp.checked_add(2)?;
        Some(value)
    }

    fn pop_far_return(&mut self) -> Option<(u8, u16)> {
        let at = self.sp as usize;
        if at.checked_add(4)? > self.stack.len() { return None; }
        let ip = u16::from_le_bytes([self.stack[at], self.stack[at + 1]]);
        let csp = u16::from_le_bytes([self.stack[at + 2], self.stack[at + 3]]);
        if csp > 0xff { return None; }
        self.sp = self.sp.checked_add(4)?;
        Some((csp as u8, ip))
    }

    fn bit_word_logical(&self, q: u8) -> u16 {
        if q >= 0xf0 {
            return self.registers[usize::from(q & 0x0f)];
        }
        if q >= 0x80 {
            0xff00 + 2 * u16::from(q - 0x80)
        } else {
            0xfd00 + 2 * u16::from(q)
        }
    }

    fn read_bit_word(&mut self, q: u8) -> Option<u16> {
        if q >= 0xf0 {
            Some(self.registers[usize::from(q & 0x0f)])
        } else {
            self.read_word(self.bit_word_logical(q))
        }
    }

    fn write_bit_word(&mut self, q: u8, value: u16) -> Option<()> {
        if q >= 0xf0 {
            self.registers[usize::from(q & 0x0f)] = value;
            Some(())
        } else {
            self.write_word(self.bit_word_logical(q), value)
        }
    }

    fn test_bit(&mut self, q: u8, bit: u8) -> Option<bool> {
        self.read_bit_word(q).map(|word| (word >> bit) & 1 != 0)
    }

    fn set_bit_value(&mut self, q: u8, bit: u8, value: bool) -> Option<()> {
        let word = self.read_bit_word(q)?;
        let mask = 1u16 << bit;
        let updated = if value { word | mask } else { word & !mask };
        self.write_bit_word(q, updated)
    }

    fn unary_word_flags(&mut self, result: u16) {
        self.flags = Flags { carry: false, zero: result == 0, negative: result & 0x8000 != 0, overflow: false };
    }

    fn unary_byte_flags(&mut self, result: u8) {
        self.flags = Flags { carry: false, zero: result == 0, negative: result & 0x80 != 0, overflow: false };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_register_arithmetic_is_deterministic() {
        let image = [0xE0, 0x51, 0x08, 0x13, 0xF0, 0x21, 0x44, 0x00];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(3), Stop::Budget);
        assert_eq!(machine.registers[1], 8);
        assert_eq!(machine.registers[2], 8);
        assert_eq!(machine.pc, 6);
        assert_eq!(machine.trace().len(), 3);
    }

    #[test]
    fn call_and_return_use_bounded_stack() {
        let mut image = vec![0; 0x20];
        image[0..4].copy_from_slice(&[0xCA, 0x00, 0x10, 0x00]);
        image[4..6].copy_from_slice(&[0xE0, 0x21]);
        image[0x10..0x12].copy_from_slice(&[0xE0, 0x72]);
        image[0x12..0x14].copy_from_slice(&[0xCB, 0x00]);
        let mut machine = Machine::new(&image, 0, 0x40).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.registers[1], 2);
        assert_eq!(machine.registers[2], 7);
        assert_eq!(machine.sp, 0x40);
        assert_eq!(machine.pc, 6);
    }

    #[test]
    fn far_call_and_segmented_return_preserve_csp_and_ip() {
        let mut image = vec![0; 0x20020];
        image[0x10000..0x10004].copy_from_slice(&[0xDA, 0x02, 0x10, 0x00]);
        image[0x10004..0x10006].copy_from_slice(&[0xE0, 0x71]);
        image[0x20010..0x20014].copy_from_slice(&[0xE0, 0x52, 0xDB, 0x00]);
        let mut machine = Machine::new(&image, 0x10000, 0x80).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!((machine.csp, machine.pc, machine.sp), (1, 0x10006, 0x80));
        assert_eq!((machine.registers[1], machine.registers[2]), (7, 5));
    }

    #[test]
    fn unsupported_instruction_stops_before_state_change() {
        let mut machine = Machine::new(&[0x44, 0x00], 0, 0x10).unwrap();
        assert_eq!(machine.step(), Some(Stop::Unsupported { pc: 0, text: "undecoded".into() }));
        assert_eq!(machine.pc, 0);
        assert_eq!(machine.steps, 0);
    }

    #[test]
    fn compare_drives_taken_and_not_taken_equal_branches() {
        let taken = [0xE0, 0x51, 0x48, 0x15, 0x2D, 0x02, 0xE0, 0x12, 0x44, 0x00, 0xE0, 0x92];
        let mut machine = Machine::new(&taken, 0, 0x100).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert!(machine.flags.zero);
        assert_eq!(machine.registers[2], 9);
        assert_eq!(machine.pc, 12);

        let not_taken = [0xE0, 0x41, 0x48, 0x15, 0x2D, 0x02, 0xE0, 0x12, 0x44, 0x00, 0xE0, 0x92];
        let mut machine = Machine::new(&not_taken, 0, 0x100).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert!(!machine.flags.zero);
        assert_eq!(machine.registers[2], 1);
        assert_eq!(machine.pc, 8);
    }

    #[test]
    fn arithmetic_flags_cover_unsigned_and_signed_relations() {
        let image = [0xE6, 0xF1, 0xFF, 0x7F, 0x08, 0x11, 0x28, 0x12];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(2), Stop::Budget);
        assert_eq!(machine.registers[1], 0x8000);
        assert!(machine.flags.negative && machine.flags.overflow && !machine.flags.carry);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0x7ffe);
        assert!(!machine.flags.negative && machine.flags.overflow && !machine.flags.carry);
    }

    #[test]
    fn dpp_translation_drives_direct_indirect_and_store_load_access() {
        let mut image = vec![0; 0x10008];
        image[0..22].copy_from_slice(&[
            0xE6, 0x01, 0x04, 0x00, // DPP1 = page 4
            0xF2, 0xF1, 0x00, 0x40, // r1 = [0x4000]
            0xE6, 0xF2, 0x02, 0x40, // r2 = 0x4002
            0xA8, 0x32,             // r3 = [r2]
            0xF6, 0xF1, 0x04, 0x40, // [0x4004] = r1
            0xF2, 0xF4, 0x04, 0x40, // r4 = [0x4004]
        ]);
        image[0x10000..0x10004].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(6), Stop::Budget);
        assert_eq!(machine.dpp[1], 4);
        assert_eq!(machine.registers[1], 0x1234);
        assert_eq!(machine.registers[3], 0x5678);
        assert_eq!(machine.registers[4], 0x1234);
    }

    #[test]
    fn seeded_ram_overlays_flash_and_writes_remain_separate() {
        let mut image = vec![0; 0x50];
        image[0..14].copy_from_slice(&[
            0xF2, 0xF1, 0x40, 0x00, // r1 = seeded RAM, not flash
            0x08, 0x11,             // add r1, #1
            0xF6, 0xF1, 0x42, 0x00, // RAM[0x42] = r1
            0xF2, 0xF2, 0x42, 0x00, // r2 = RAM[0x42]
        ]);
        image[0x40..0x44].copy_from_slice(&[0xaa, 0xaa, 0xbb, 0xbb]);
        let mut machine = Machine::new(&image, 0, 0x30).unwrap();
        machine.seed_ram(&[(0x40, vec![0x34, 0x12])]).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!((machine.registers[1], machine.registers[2]), (0x1235, 0x1235));
        assert_eq!(machine.ram_seed_bytes(), 2);
        assert_eq!(machine.ram_write_bytes(), 2);
        assert_eq!(&machine.memory[0x40..0x44], &[0xaa, 0xaa, 0xbb, 0xbb]);
    }

    #[test]
    fn seeded_ram_can_model_physical_space_outside_the_flash_capture() {
        let image = [0xF2, 0xF1, 0x00, 0x40];
        let mut machine = Machine::new(&image, 0, 0x20).unwrap();
        machine.dpp[1] = 0x0200;
        machine.seed_ram(&[(0x0080_0000, vec![0x78, 0x56])]).unwrap();
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0x5678);
    }

    #[test]
    fn short_alu_operands_use_indirect_and_postincrement_modes() {
        let mut image = vec![0; 0x80];
        image[0..12].copy_from_slice(&[
            0xE6, 0xF3, 0x40, 0x00, // r3 = 0x40
            0xE6, 0xF1, 0x01, 0x00, // r1 = 1
            0x08, 0x1F,             // add r1, [r3+]
            0x09, 0x2E,             // addb rl1, [r2+]
        ]);
        image[0x40..0x42].copy_from_slice(&2u16.to_le_bytes());
        image[0x00] = 0xE6; // code remains the byte source for r2=0
        let mut machine = Machine::new(&image, 0, 0x70).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.registers[1], 0x00e9);
        assert_eq!(machine.registers[3], 0x42);
        assert_eq!(machine.registers[2], 1);
    }

    #[test]
    fn extp_overrides_dpp_for_exactly_the_declared_instruction_count() {
        let mut image = vec![0; 0x10200];
        image[0..22].copy_from_slice(&[
            0xD7, 0x50, 0x04, 0x00, // EXTP page 4 for two instructions
            0xF2, 0xF1, 0x00, 0x01, // override: r1 = physical 0x10100
            0xF2, 0xF2, 0x02, 0x01, // override: r2 = physical 0x10102
            0xF2, 0xF3, 0x00, 0x01, // DPP0 again: r3 = physical 0x0100
            0xDC, 0x55,             // EXTP r5 for two instructions
            0xF2, 0xF4, 0x00, 0x01, // override: r4 = physical 0x10100
        ]);
        image[0x100..0x102].copy_from_slice(&0x1111u16.to_le_bytes());
        image[0x10100..0x10104].copy_from_slice(&[0x22, 0x22, 0x33, 0x33]);
        let mut machine = Machine::new(&image, 0, 0x80).unwrap();
        machine.registers[5] = 4;
        assert_eq!(machine.run(6), Stop::Budget);
        assert_eq!((machine.registers[1], machine.registers[2]), (0x2222, 0x3333));
        assert_eq!(machine.registers[3], 0x1111);
        assert_eq!(machine.registers[4], 0x2222);
        assert_eq!(machine.override_instructions, 1);
    }

    #[test]
    fn unmapped_dpp_access_fails_closed() {
        let image = [0xE6, 0x01, 0xFF, 0x03, 0xF2, 0xF1, 0x00, 0x40];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(2), Stop::OutOfBounds(0x00ff_c000));
        assert_eq!(machine.pc, 4);
        assert_eq!(machine.steps, 1);
    }

    #[test]
    fn breakpoint_stops_before_instruction_mutates_state() {
        let image = [0xE0, 0x51, 0xE0, 0x72, 0x44, 0x00];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run_until(10, Some(2)), Stop::Breakpoint(2));
        assert_eq!(machine.steps, 1);
        assert_eq!(machine.pc, 2);
        assert_eq!(machine.registers[1], 5);
        assert_eq!(machine.registers[2], 0);
        assert_eq!(machine.trace().len(), 1);
    }

    #[test]
    fn physical_memory_watchpoint_reports_dpp_translated_write() {
        let mut image = vec![0; 0x40];
        image[0..6].copy_from_slice(&[0xE0, 0x51, 0xF6, 0xF1, 0x20, 0x00]);
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run_observed(10, None, Some(0x21)),
                   Stop::Watchpoint { address: 0x21, access: WatchAccess::Write });
        assert_eq!(machine.steps, 2);
        assert_eq!(machine.pc, 6);
    }

    #[test]
    fn immediate_sfr_writes_track_cp_sp_and_md_accumulator() {
        let image = [
            0xE6, 0x08, 0x34, 0xF9, // CP masks to the architectural context boundary
            0xE6, 0x09, 0x80, 0x00, // SP
            0xE6, 0x06, 0x34, 0x12, // MDH
            0xE6, 0x07, 0x78, 0x56, // MDL
        ];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.cp, 0xf800);
        assert_eq!(machine.sp, 0x80);
        assert_eq!(machine.mdh, 0x1234);
        assert_eq!(machine.mdl, 0x5678);
    }

    #[test]
    fn long_memory_accesses_alias_core_sfrs_only_through_page_three() {
        let mut image = vec![0; 0x10000];
        image[0..20].copy_from_slice(&[
            0xE6, 0xFF, 0x09, 0x00, // r15 = 9
            0xF6, 0xFF, 0x00, 0xFE, // [DPP0] = r15 through DPP3/page 3
            0xF2, 0xF1, 0x00, 0xFE, // r1 = [DPP0]
            0xE6, 0x03, 0x04, 0x00, // DPP3 = page 4
            0xF2, 0xF2, 0x00, 0xFE, // now maps outside the image, not to DPP0
        ]);
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(3), Stop::Budget);
        assert_eq!(machine.dpp[0], 9);
        assert_eq!(machine.registers[1], 9);
        assert_eq!(machine.run(2), Stop::OutOfBounds(0x0001_3e00));
    }

    #[test]
    fn byte_alu_direct_word_alu_and_explicit_stack_forms_execute() {
        let mut image = vec![0; 0x100];
        image[0..20].copy_from_slice(&[
            0xE6, 0xF1, 0xFF, 0x00, // r1 = 0x00ff
            0xE6, 0xF2, 0x01, 0x00, // r2 = 1
            0x01, 0x24,             // addb rl1, rl2 -> 0
            0x11, 0x25,             // addcb rl1, rh2 -> 1 (carry in)
            0x22, 0x10, 0x40, 0x00, // sub r1, [0x40]
            0xEC, 0xF1,             // push r1
            0xFC, 0xF3,             // pop r3
        ]);
        image[0x40..0x42].copy_from_slice(&1u16.to_le_bytes());
        let mut machine = Machine::new(&image, 0, 0x80).unwrap();
        assert_eq!(machine.run(7), Stop::Budget);
        assert_eq!(machine.registers[1], 0);
        assert_eq!(machine.registers[3], 0);
        assert_eq!(machine.sp, 0x80);
        assert!(machine.flags.zero);
    }

    #[test]
    fn psw_flag_bits_are_shared_by_long_sfr_and_bit_accesses() {
        let image = [
            0xE6, 0xF1, 0x0f, 0x00, // r1 = N|C|V|Z
            0xF6, 0xF1, 0x10, 0xFF, // [PSW] = r1
            0x3E, 0x88,             // bclr PSW.3
            0xF2, 0xF2, 0x10, 0xFF, // r2 = [PSW]
        ];
        let mut machine = Machine::new(&image, 0, 0x80).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.registers[2], 7);
        assert!(machine.flags.negative && machine.flags.carry && machine.flags.overflow);
        assert!(!machine.flags.zero);
    }

    #[test]
    fn cp_switches_and_restores_bounded_register_banks() {
        let image = [
            0xE0, 0x51,             // bank 0xFC00: r1 = 5
            0xE6, 0x08, 0x00, 0xF8, // select bank 0xF800
            0xE0, 0x71,             // bank 0xF800: r1 = 7
            0xE6, 0x08, 0x00, 0xFC, // restore bank 0xFC00
        ];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.cp, 0xfc00);
        assert_eq!(machine.registers[1], 5);
        machine.set_cp(0xf800);
        assert_eq!(machine.registers[1], 7);
    }

    #[test]
    fn logical_word_forms_update_result_and_clear_carry_overflow() {
        let image = [
            0xE6, 0xF1, 0x0F, 0xF0, // r1 = 0xF00F
            0xE6, 0xF2, 0xFF, 0x00, // r2 = 0x00FF
            0x50, 0x12,             // xor r1, r2 -> 0xF0F0
            0x68, 0x10,             // and r1, #0 -> zero
            0x76, 0xF1, 0x00, 0x80, // or r1, #0x8000
        ];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        machine.flags.carry = true;
        machine.flags.overflow = true;
        assert_eq!(machine.run(5), Stop::Budget);
        assert_eq!(machine.registers[1], 0x8000);
        assert!(machine.flags.negative && !machine.flags.zero);
        assert!(!machine.flags.carry && !machine.flags.overflow);
    }

    #[test]
    fn shift_and_rotate_forms_follow_c167_carry_and_rounding_flags() {
        let image = [
            0xE6, 0xF1, 0x03, 0x80, // r1 = 0x8003
            0xE0, 0x22,             // r2 = 2
            0x4C, 0x12,             // shl r1, r2 -> 0x000c, C=0
            0x3C, 0x21,             // ror r1, #2 -> 0x0003, C=0, V=0
            0xE6, 0xF3, 0x03, 0x80, // r3 = 0x8003
            0xBC, 0x23,             // ashr r3, #2 -> 0xe000, C=1, V=1
        ];
        let mut machine = Machine::new(&image, 0, 0x100).unwrap();
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.registers[1], 3);
        assert!(!machine.flags.carry && !machine.flags.overflow);
        assert_eq!(machine.run(2), Stop::Budget);
        assert_eq!(machine.registers[3], 0xe000);
        assert!(machine.flags.carry && machine.flags.overflow && machine.flags.negative);
    }

    #[test]
    fn signed_and_unsigned_multiply_fill_md_and_set_word_overflow() {
        let mut signed = Machine::new(&[0x0B, 0x12], 0, 0x100).unwrap();
        signed.registers[1] = (-2i16) as u16;
        signed.registers[2] = 3;
        assert_eq!(signed.run(1), Stop::Budget);
        assert_eq!((signed.mdh, signed.mdl), (0xffff, 0xfffa));
        assert!(signed.flags.negative && !signed.flags.overflow && !signed.flags.carry);

        let mut unsigned = Machine::new(&[0x1B, 0x12], 0, 0x100).unwrap();
        unsigned.registers[1] = 0xffff;
        unsigned.registers[2] = 2;
        assert_eq!(unsigned.run(1), Stop::Budget);
        assert_eq!((unsigned.mdh, unsigned.mdl), (1, 0xfffe));
        assert!(unsigned.flags.overflow && !unsigned.flags.negative && !unsigned.flags.carry);
    }

    #[test]
    fn divide_forms_use_md_width_and_fail_closed_on_invalid_results() {
        let mut extended = Machine::new(&[0x7B, 0x11], 0, 0x100).unwrap();
        extended.mdh = 1;
        extended.mdl = 0;
        extended.registers[1] = 0x100;
        assert_eq!(extended.run(1), Stop::Budget);
        assert_eq!((extended.mdh, extended.mdl), (0, 0x100));

        let mut zero = Machine::new(&[0x5B, 0x11], 0, 0x100).unwrap();
        zero.mdh = 0x1234;
        zero.mdl = 0x5678;
        assert_eq!(zero.run(1), Stop::ArithmeticFault { pc: 0, reason: "division by zero" });
        assert_eq!((zero.mdh, zero.mdl, zero.steps, zero.pc), (0x1234, 0x5678, 0, 0));

        let mut overflow = Machine::new(&[0x7B, 0x11], 0, 0x100).unwrap();
        overflow.mdh = 1;
        overflow.mdl = 0;
        overflow.registers[1] = 1;
        assert_eq!(overflow.run(1), Stop::ArithmeticFault { pc: 0, reason: "division quotient overflow" });
        assert_eq!((overflow.mdh, overflow.mdl), (1, 0));
    }

    #[test]
    fn compare_adjust_forms_compare_original_then_increment_or_decrement() {
        let mut image = vec![0; 0x60];
        image[0..10].copy_from_slice(&[
            0x80, 0x51,             // cmpi1 r1, #5: equal, then r1 += 1
            0x96, 0xF2, 0x08, 0x00, // cmpi2 r2, #8: less, then r2 += 2
            0xA2, 0xF3, 0x40, 0x00, // cmpd1 r3, [0x40]: greater, then r3 -= 1
        ]);
        image[0x40..0x42].copy_from_slice(&2u16.to_le_bytes());
        let mut machine = Machine::new(&image, 0, 0x50).unwrap();
        machine.registers[1] = 5;
        machine.registers[2] = 7;
        machine.registers[3] = 3;

        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 6);
        assert!(machine.flags.zero && !machine.flags.carry);

        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[2], 9);
        assert!(machine.flags.carry && machine.flags.negative && !machine.flags.zero);

        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[3], 2);
        assert!(!machine.flags.carry && !machine.flags.zero && !machine.flags.negative);
    }

    #[test]
    fn compare_adjust_wraps_register_without_replacing_comparison_flags() {
        let image = [0xB6, 0xF1, 0x00, 0x00]; // cmpd2 r1, #0
        let mut machine = Machine::new(&image, 0, 0x20).unwrap();
        machine.registers[1] = 1;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0xffff);
        assert!(!machine.flags.zero && !machine.flags.carry && !machine.flags.negative);
    }

    #[test]
    fn nop_and_bounded_system_noops_advance_without_state_change() {
        // nop; diswdt; srvwdt; einit
        let image = [0xCC, 0x00, 0xA5, 0x5A, 0xA5, 0xA5, 0xA7, 0x58, 0xA7, 0xA7, 0xB5, 0x4A, 0xB5, 0xB5];
        let mut machine = Machine::new(&image, 0, 0x40).unwrap();
        let before = (machine.registers, machine.flags, machine.dpp, machine.cp, machine.sp, machine.mdh, machine.mdl);
        assert_eq!(machine.run(4), Stop::Budget);
        assert_eq!(machine.pc, 14);
        assert_eq!((machine.registers, machine.flags, machine.dpp, machine.cp, machine.sp, machine.mdh, machine.mdl), before);
    }

    #[test]
    fn halt_and_reset_system_controls_stop_as_explicit_unsupported() {
        for (bytes, text) in [
            (&[0x87u8, 0x78, 0x87, 0x87][..], "idle"),
            (&[0x97, 0x68, 0x97, 0x97][..], "pwrdn"),
            (&[0xB7, 0x48, 0xB7, 0xB7][..], "srst"),
        ] {
            let mut machine = Machine::new(bytes, 0, 0x20).unwrap();
            assert_eq!(machine.step(), Some(Stop::Unsupported { pc: 0, text: text.into() }));
            assert_eq!((machine.pc, machine.steps), (0, 0));
        }
    }

    #[test]
    fn bit_set_clear_and_bit_jumps_operate_on_gprs_and_psw() {
        // bset r1.0; bclr r1.0
        let image = [0x0F, 0xF1, 0x0E, 0xF1];
        let mut machine = Machine::new(&image, 0, 0x20).unwrap();
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0x0001);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0x0000);

        // jb PSW.3 (Z), rel +2 -> skip next 2-byte instruction and land on the following one
        let mut image = vec![0; 0x1010];
        image[0x1000..0x1004].copy_from_slice(&[0x8A, 0x88, 0x02, 0x30]);
        image[0x1004..0x1006].copy_from_slice(&[0x0F, 0xF1]); // skipped
        image[0x1008..0x100A].copy_from_slice(&[0x1F, 0xF1]); // executed
        let mut machine = Machine::new(&image, 0x1000, 0x40).unwrap();
        machine.flags.zero = true;
        assert_eq!(machine.run(2), Stop::Budget);
        assert_eq!(machine.registers[1], 0x0002);

        // jnb PSW.3 when clear falls through
        let mut image = vec![0; 0x1010];
        image[0x1000..0x1004].copy_from_slice(&[0x9A, 0x88, 0x02, 0x30]);
        image[0x1004..0x1006].copy_from_slice(&[0x0F, 0xF1]);
        image[0x1008..0x100A].copy_from_slice(&[0x1F, 0xF1]);
        let mut machine = Machine::new(&image, 0x1000, 0x40).unwrap();
        machine.flags.zero = true;
        assert_eq!(machine.run(2), Stop::Budget);
        assert_eq!(machine.registers[1], 0x0001);
    }

    #[test]
    fn jbc_clears_tested_bit_when_taken() {
        let mut image = vec![0; 0x20];
        // jbc r2.1, rel 0x01 => target 0 + 4 + 2 = 6
        image[0..4].copy_from_slice(&[0xAA, 0xF2, 0x01, 0x10]);
        image[4..6].copy_from_slice(&[0x0F, 0xF1]); // skipped if taken
        image[6..8].copy_from_slice(&[0x1F, 0xF1]);
        let mut machine = Machine::new(&image, 0, 0x20).unwrap();
        machine.registers[2] = 0x0002;
        assert_eq!(machine.run(2), Stop::Budget);
        assert_eq!(machine.registers[2], 0x0000);
        assert_eq!(machine.registers[1], 0x0002);
    }

    #[test]
    fn unary_extend_prior_retp_and_bfld_execute() {
        // neg r1
        let mut machine = Machine::new(&[0x81, 0x10], 0, 0x20).unwrap();
        machine.registers[1] = 1;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0xffff);
        assert!(machine.flags.carry && machine.flags.negative && !machine.flags.zero && !machine.flags.overflow);

        // cpl r2
        let mut machine = Machine::new(&[0x91, 0x20], 0, 0x20).unwrap();
        machine.registers[2] = 0x00ff;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[2], 0xff00);

        // negb rh0 (0xA1,0x10)
        let mut machine = Machine::new(&[0xA1, 0x10], 0, 0x20).unwrap();
        machine.set_byte_register(1, 0x01);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.byte_register(1), 0xff);

        // cplb rl1 (0xB1,0x20)
        let mut machine = Machine::new(&[0xB1, 0x20], 0, 0x20).unwrap();
        machine.set_byte_register(2, 0x0f);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.byte_register(2), 0xf0);

        // movbz r2, rh0 (0xC0,0x21)
        let mut machine = Machine::new(&[0xC0, 0x21], 0, 0x20).unwrap();
        machine.set_byte_register(1, 0x80);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[2], 0x0080);

        // movbs r2, rh0 (0xD0,0x21)
        let mut machine = Machine::new(&[0xD0, 0x21], 0, 0x20).unwrap();
        machine.set_byte_register(1, 0x80);
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[2], 0xff80);

        // prior r2,r1
        let mut machine = Machine::new(&[0x2B, 0x21], 0, 0x20).unwrap();
        machine.registers[1] = 0x0010;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[2], 4);

        // scxt remains unsupported until sign-extend semantics are admitted.
        let mut machine = Machine::new(&[0xC6, 0xF1, 0xFF, 0x00], 0, 0x20).unwrap();
        machine.registers[1] = 0x0001;
        assert_eq!(machine.step(), Some(Stop::Unsupported { pc: 0, text: "scxt r1, #0x00ff".into() }));
        assert_eq!((machine.pc, machine.registers[1]), (0, 0x0001));

        // bfldl r1,#0x0f,#0x05
        let mut machine = Machine::new(&[0x0A, 0xF1, 0x0F, 0x05], 0, 0x20).unwrap();
        machine.registers[1] = 0xfff0;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0xfff5);

        // bfldh r1,#0x05,#0x0f -> high-byte mask 0x0500, data 0x0f00
        let mut machine = Machine::new(&[0x1A, 0xF1, 0x0F, 0x05], 0, 0x20).unwrap();
        machine.registers[1] = 0xf0ff;
        assert_eq!(machine.run(1), Stop::Budget);
        assert_eq!(machine.registers[1], 0xf5ff);

        // retp r4
        let mut image = vec![0; 0x40];
        image[0..2].copy_from_slice(&[0xBB, 0x01]); // callr to 4
        image[4..8].copy_from_slice(&[0xE6, 0xF4, 0x30, 0x00]); // mov r4,#0x30
        image[8..10].copy_from_slice(&[0xEB, 0xF4]); // retp r4
        let mut machine = Machine::new(&image, 0, 0x20).unwrap();
        assert_eq!(machine.run(3), Stop::Budget);
        assert_eq!(machine.pc, 2);
        assert_eq!(machine.sp, 0x30);
    }
}
