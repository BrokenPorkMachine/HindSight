//! Complete ARMv7-A (A32, ARM-state) execution core for the Apple A7 SEP.
//!
//! The Apple A7's Secure Enclave Processor ("SEP") is a 32-bit ARMv7-A core
//! paired with a private SRAM, a secure DMA window, and a bank of
//! cryptographic engines. Boot firmware (SEPROM) and the SEP operating
//! system (SEPOS) execute in A32 state, switching between the user/system,
//! FIQ, IRQ, supervisor, abort, and undefined modes via SWI and exception
//! entry/return.
//!
//! This module implements the full A32 instruction set in ARM state:
//!
//! * data processing with the complete barrel shifter (immediate ROR, and
//!   register or register-shifted-register LSL/LSR/ASR/ROR/RRX), including
//!   the ARMv7 immediate-`LSR #0`= 32 / immediate-`ASR #0`= 32 and
//!   `ROR #0`= `RRX` special cases, plus `MOVW`/`MOVT`;
//! * multiply and multiply-long (MUL, MLA, UMULL, UMLAL, SMULL, SMLAL),
//!   with flag semantics (`C`/`V` unaffected);
//! * PSR transfers: MRS (CPSR/SPSR) and MSR (register and immediate) with
//!   per-field masks;
//! * single-data transfers (LDR/STR word/byte, immediate or shifted-register
//!   offset, P/U/B/W/L addressing modes);
//! * halfword and signed data transfers (LDRH/STRH/LDRSB/LDRSH), immediate
//!   (8-bit) and register offset;
//! * LDRD/STRD double-word transfers;
//! * block data transfers (LDM/STM in all four IA/IB/DA/DB forms) with
//!   writeback, the user-register `^` qualifier, and SPSR restore on
//!   `LDM {...}, ^` with PC in the list;
//! * branch and exchange (B, BL, BX, BLX register and immediate);
//! * MRC/MCR to a minimal CP15 (SCTLR only; all other registers read back
//!   zero so the SEP's AUXCTL/feature probes do not fault);
//! * SWI/SVC and undefined-instruction vector entry (mode switch, SPSR save,
//!   banked LR), plus IRQ/FIQ delivery against the CPSR I/F masks and the
//!   0xFFFFFFFF high-vector base (SCTLR.V);
//! * WFI/WFE/YIELD/SEV HINT handling, and
//! * the exception-return path via `movs pc, lr` / `ldm ...,{..,pc}^`.
//!
//! Thumb-2 (the A32 `T` bit) is not implemented in this module, but a step with
//! the T bit set is **delegated to the narrow `aarch32::step`**, which has a full
//! Thumb path. The delegation is not a convenience: the A9 SEPROM starts in ARM
//! state and ends its reset path in Thumb (`bx 0x10004cdd`), so a core that
//! refused Thumb could not run that image at all.
//!
//! Banked state lives in `CpuState::a32` (`iphem_core::Aarch32State`),
//! indexed by mode: usr/sys=0, fiq=1, irq=2, svc=3, abt=4, und=5. The
//! FIQ mode owns a private r8-r12 bank; every mode owns a banked SP/LR and
//! the exception modes own an SPSR.
//!
//! Decode/execute order is: interrupts first (pending FIQ beats IRQ beats
//! normal execution), then primary class on bits[27:26], then the fixed
//! precedence inside the misc `00` class (MRS, MSR, BX/BLX-reg, multiply,
//! halfword/signed transfer, LDRD/STRD, CLZ, BKPT, data-processing) and the
//! data-processing-immediate space (MSR-immediate, MOVW, MOVT, arithmetic).
//! The masks/values were pinned against `clang -target armv7-apple-darwin`
//! encodings (see repository AGENTS.md for the convention).

use crate::ExecError;
use iphem_core::{CpuRunState, CpuState, Machine, TraceKind};

const T_BIT: u32 = 1 << 5;
const N_MASK: u32 = 1 << 31;
const Z_MASK: u32 = 1 << 30;
const C_MASK: u32 = 1 << 29;
const V_MASK: u32 = 1 << 28;

const MODE_USR: u32 = 0x10;
const MODE_FIQ: u32 = 0x11;
const MODE_IRQ: u32 = 0x12;
const MODE_SVC: u32 = 0x13;
const MODE_ABT: u32 = 0x17;
const MODE_UND: u32 = 0x1b;
const MODE_SYS: u32 = 0x1f;

/// Mode bits of a CPSR/SPSR value. Unknown encodings map to SVC so a
/// corrupted mode field cannot panic the core.
fn mode_index(mode: u32) -> usize {
    match mode {
        MODE_USR | MODE_SYS => 0,
        MODE_FIQ => 1,
        MODE_IRQ => 2,
        MODE_SVC => 3,
        MODE_ABT => 4,
        MODE_UND => 5,
        _ => 3,
    }
}

/// A working copy of the A32 register file. Kept as a plain struct so the
/// executor can borrow `Machine` for memory traffic freely; committed to
/// `CpuState` at every step boundary.
#[derive(Clone)]
struct A32 {
    r0_7: [u32; 8],
    r8_12: [u32; 5],
    r8_12_fiq: [u32; 5],
    sp: [u32; 6],
    lr: [u32; 6],
    pc: u32,
    cpsr: u32,
    spsr: [u32; 5],
}

fn carry_in(cpsr: u32) -> bool {
    cpsr & C_MASK != 0
}

fn condition(cpsr: u32, cond: u32) -> bool {
    let n = cpsr & N_MASK != 0;
    let z = cpsr & Z_MASK != 0;
    let c = cpsr & C_MASK != 0;
    let v = cpsr & V_MASK != 0;
    match cond {
        0 => z,
        1 => !z,
        2 => c,
        3 => !c,
        4 => n,
        5 => !n,
        6 => v,
        7 => !v,
        8 => c && !z,
        9 => !c || z,
        10 => n == v,
        11 => n != v,
        12 => !z && n == v,
        13 => z || n != v,
        14 => true,
        _ => false,
    }
}

impl A32 {
    fn from_state(s: &CpuState) -> Self {
        Self {
            r0_7: [
                s.x[0] as u32,
                s.x[1] as u32,
                s.x[2] as u32,
                s.x[3] as u32,
                s.x[4] as u32,
                s.x[5] as u32,
                s.x[6] as u32,
                s.x[7] as u32,
            ],
            r8_12: [
                s.x[8] as u32,
                s.x[9] as u32,
                s.x[10] as u32,
                s.x[11] as u32,
                s.x[12] as u32,
            ],
            r8_12_fiq: s.a32.r8_12_fiq,
            sp: s.a32.sp,
            lr: s.a32.lr,
            pc: s.pc as u32,
            cpsr: s.a32.cpsr,
            spsr: s.a32.spsr,
        }
    }

    fn commit(&self, s: &mut CpuState) {
        s.x[0] = u64::from(self.r0_7[0]);
        s.x[1] = u64::from(self.r0_7[1]);
        s.x[2] = u64::from(self.r0_7[2]);
        s.x[3] = u64::from(self.r0_7[3]);
        s.x[4] = u64::from(self.r0_7[4]);
        s.x[5] = u64::from(self.r0_7[5]);
        s.x[6] = u64::from(self.r0_7[6]);
        s.x[7] = u64::from(self.r0_7[7]);
        s.x[8] = u64::from(self.r8_12[0]);
        s.x[9] = u64::from(self.r8_12[1]);
        s.x[10] = u64::from(self.r8_12[2]);
        s.x[11] = u64::from(self.r8_12[3]);
        s.x[12] = u64::from(self.r8_12[4]);
        s.sp = u64::from(self.sp[self.mi()]);
        s.pc = u64::from(self.pc);
        s.pstate = u64::from(self.cpsr);
        s.a32.cpsr = self.cpsr;
        s.a32.spsr = self.spsr;
        s.a32.r8_12_fiq = self.r8_12_fiq;
        s.a32.sp = self.sp;
        s.a32.lr = self.lr;
    }

    fn mode(&self) -> u32 {
        self.cpsr & 0x1f
    }

    fn mi(&self) -> usize {
        mode_index(self.mode())
    }

    fn get(&self, reg: u32) -> u32 {
        match reg {
            0..=7 => self.r0_7[reg as usize],
            8..=12 => {
                if self.mode() == MODE_FIQ {
                    self.r8_12_fiq[reg as usize - 8]
                } else {
                    self.r8_12[reg as usize - 8]
                }
            }
            13 => self.sp[self.mi()],
            14 => self.lr[self.mi()],
            _ => self.pc.wrapping_add(8),
        }
    }

    /// Read one of a register bank belonging to `mode` (used by the `^`
    /// user-register block transfers).
    fn get_banked(&self, mode: u32, reg: u32) -> u32 {
        let mi = mode_index(mode);
        match reg {
            0..=7 => self.r0_7[reg as usize],
            8..=12 => {
                if mode == MODE_FIQ {
                    self.r8_12_fiq[reg as usize - 8]
                } else {
                    self.r8_12[reg as usize - 8]
                }
            }
            13 => self.sp[mi],
            14 => self.lr[mi],
            _ => self.pc.wrapping_add(8),
        }
    }

    fn set(&mut self, reg: u32, value: u32) {
        match reg {
            0..=7 => self.r0_7[reg as usize] = value,
            8..=12 => {
                if self.mode() == MODE_FIQ {
                    self.r8_12_fiq[reg as usize - 8] = value;
                } else {
                    self.r8_12[reg as usize - 8] = value;
                }
            }
            13 => self.sp[self.mi()] = value,
            14 => self.lr[self.mi()] = value,
            _ => self.branch(value),
        }
    }

    fn set_banked(&mut self, mode: u32, reg: u32, value: u32) {
        let mi = mode_index(mode);
        match reg {
            0..=7 => self.r0_7[reg as usize] = value,
            8..=12 => {
                if mode == MODE_FIQ {
                    self.r8_12_fiq[reg as usize - 8] = value;
                } else {
                    self.r8_12[reg as usize - 8] = value;
                }
            }
            13 => self.sp[mi] = value,
            14 => self.lr[mi] = value,
            _ => self.branch(value),
        }
    }

    /// Branch/exchange: the low bit selects the Thumb bit. Thumb targets are
    /// recorded on the CPSR T bit (already set here) but not executed by this
    /// module; the `step` guard returns `Undefined` in that case.
    fn branch(&mut self, value: u32) {
        if value & 1 != 0 {
            self.cpsr |= T_BIT;
        } else {
            self.cpsr &= !T_BIT;
        }
        self.pc = value & !1;
    }

    fn restore_spsr(&mut self) {
        let mi = self.mi();
        if mi != 0 {
            self.cpsr = self.spsr[mi - 1];
        }
    }

    fn set_flags_nz(&mut self, result: u32) {
        self.cpsr &= !(N_MASK | Z_MASK);
        if result & N_MASK != 0 {
            self.cpsr |= N_MASK;
        }
        if result == 0 {
            self.cpsr |= Z_MASK;
        }
    }

    fn set_sub_flags(&mut self, left: u32, right: u32, result: u32) {
        self.set_flags_nz(result);
        self.cpsr &= !(C_MASK | V_MASK);
        if left >= right {
            self.cpsr |= C_MASK;
        }
        if (left ^ right) & (left ^ result) & N_MASK != 0 {
            self.cpsr |= V_MASK;
        }
    }

    fn set_add_flags(&mut self, left: u32, right: u32, wide: u64) {
        let result = wide as u32;
        self.set_flags_nz(result);
        self.cpsr &= !(C_MASK | V_MASK);
        if wide >> 32 != 0 {
            self.cpsr |= C_MASK;
        }
        if (left ^ result) & (right ^ result) & N_MASK != 0 {
            self.cpsr |= V_MASK;
        }
    }
}

/// ARMv7 barrel shifter. `amount_imm` distinguishes the ARMv7 encodings
/// where `LSR #0`/`ASR #0` decode as a shift-by-32 and `ROR #0` decodes as
/// RRX (register-form `amount == 0` is always "no shift").
fn apply_shift(value: u32, typ: u32, amount: u32, carry: bool, amount_imm: bool) -> (u32, bool) {
    match typ {
        0 => {
            // LSL
            if amount == 0 {
                (value, carry)
            } else if amount >= 32 {
                (0, amount == 32 && value & 1 != 0)
            } else {
                let carry_out = value & (1 << (32 - amount)) != 0;
                (value << amount, carry_out)
            }
        }
        1 => {
            // LSR
            if amount == 0 {
                if amount_imm {
                    (0, value & N_MASK != 0)
                } else {
                    (value, carry)
                }
            } else if amount >= 32 {
                (0, amount == 32 && value & N_MASK != 0)
            } else {
                let carry_out = value & (1 << (amount - 1)) != 0;
                (value >> amount, carry_out)
            }
        }
        2 => {
            // ASR
            if amount == 0 {
                if amount_imm {
                    let fill = (value & N_MASK != 0) as u32;
                    (if fill != 0 { u32::MAX } else { 0 }, value & N_MASK != 0)
                } else {
                    (value, carry)
                }
            } else if amount >= 32 {
                let sign = value & N_MASK != 0;
                (if sign { u32::MAX } else { 0 }, sign)
            } else {
                let carry_out = value & (1 << (amount - 1)) != 0;
                let shifted = ((value as i32) >> amount) as u32;
                (shifted, carry_out)
            }
        }
        _ => {
            // ROR / RRX
            if amount == 0 {
                if amount_imm {
                    let rotated = u32::from(carry) << 31 | (value >> 1);
                    (rotated, value & 1 != 0)
                } else {
                    (value, carry)
                }
            } else {
                let n = amount & 31;
                if n == 0 {
                    (value, value & N_MASK != 0)
                } else {
                    let carry_out = value & (1 << (n - 1)) != 0;
                    (value.rotate_right(n), carry_out)
                }
            }
        }
    }
}

/// Sign-extend helper for branch offsets.
fn sign_extend(v: u32, bits: u32) -> i32 {
    let s = 32 - bits;
    ((v << s) as i32) >> s
}

fn fetch(m: &mut Machine, address: u64) -> Result<u32, ExecError> {
    m.mem_read(address, 4)
        .map(|v| v as u32)
        .map_err(|_| ExecError::Fetch)
}

fn load(m: &mut Machine, address: u64, size: usize) -> Result<u32, ExecError> {
    m.mem_read(address, size)
        .map(|v| v as u32)
        .map_err(|_| ExecError::Data)
}

fn store(m: &mut Machine, address: u64, size: usize, value: u32) -> Result<(), ExecError> {
    m.mem_write(address, size, u64::from(value))
        .map_err(|_| ExecError::Data)
}

/// Vector base selector: low 32-bit vectors below 0x10 unless SCTLR.V selects
/// the high vectors, mirroring the A32 control-register behaviour.
fn vector_base(m: &Machine) -> u32 {
    let v = m.cores.active().state.sctlr_el1 & (1 << 13) != 0;
    if v {
        0xffff_0000
    } else {
        0
    }
}

fn enter_exception(
    c: &mut A32,
    m: &Machine,
    offset: u32,
    new_mode: u32,
    mask_irq: bool,
    mask_fiq: bool,
    lr_value: u32,
) {
    let idx = mode_index(new_mode);
    c.spsr[idx - 1] = c.cpsr;
    c.lr[idx] = lr_value;
    c.pc = (vector_base(m) + offset) & !1;
    c.cpsr = (c.cpsr & !0x1f) | new_mode;
    c.cpsr &= !T_BIT;
    if mask_irq {
        c.cpsr |= 1 << 7;
    }
    if mask_fiq {
        c.cpsr |= 1 << 6;
    }
}

/// Deliver a pending FIQ or IRQ if it is not masked by the current CPSR.
/// Returns true when an exception entry was taken.
fn take_pending_interrupt(c: &mut A32, m: &mut Machine) -> bool {
    if c.cpsr & (1 << 6) == 0 && m.cores.active().pending_fiq.is_some() {
        enter_exception(
            c,
            m,
            0x1c, // FIQ vector
            MODE_FIQ,
            true,
            true,
            c.pc.wrapping_add(4),
        );
        return true;
    }
    if c.cpsr & (1 << 7) == 0 && m.cores.active().pending_irq.is_some() {
        enter_exception(
            c,
            m,
            0x18, // IRQ vector
            MODE_IRQ,
            true,
            false,
            c.pc.wrapping_add(4),
        );
        return true;
    }
    false
}

fn execute(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let cond = op >> 28;
    if cond != 0xf && !condition(c.cpsr, cond) {
        c.pc = c.pc.wrapping_add(4);
        return Ok(());
    }

    match (op >> 26) & 3 {
        0 => execute_misc(c, m, op),
        1 => execute_single(c, m, op),
        2 => {
            if op & (1 << 25) != 0 {
                execute_branch(c, m, op)
            } else {
                execute_block(c, m, op)
            }
        }
        _ => {
            if cond == 0xf && op & (1 << 24) != 0 {
                execute_blx_immediate(c, op)
            } else {
                execute_supervisor(c, m)
            }
        }
    }
}

fn execute_misc(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    // WFI/WFE/YIELD/SEV hints. WFI parks the core; the others retire as
    // no-ops. Checked before the plausible-but-overlapping MSR-immediate
    // space (clang: WFI=0xE320F003, WFE=0xE320F002, YIELD=0xE320F001).
    if op & 0x0fff_fff0 == 0x0320_f000 {
        let hint = op & 0xf;
        if hint == 3 {
            m.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;
        }
        c.pc = c.pc.wrapping_add(4);
        return Ok(());
    }
    if op & (1 << 25) != 0 {
        // Data-processing immediate space (also MSR immediate, MOVW/MOVT).
        if op & 0x0fb0_f000 == 0x0320_f000 {
            execute_msr_immediate(c, op);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0ff0_0000 == 0x0300_0000 {
            let rn = (op >> 16) & 0xf;
            let rd = (op >> 12) & 0xf;
            let imm16 = (rn << 12) | (op & 0xfff);
            c.set(rd, imm16);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0ff0_0000 == 0x0340_0000 {
            let rn = (op >> 16) & 0xf;
            let rd = (op >> 12) & 0xf;
            let imm16 = (rn << 12) | (op & 0xfff);
            let old = c.get(rd);
            c.set(rd, (old & 0x0000_ffff) | (imm16 << 16));
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        execute_dp(c, m, op, true)
    } else {
        if op & 0x0fbf_0fff == 0x010f_0000 {
            let rd = (op >> 12) & 0xf;
            let value = if op & (1 << 22) != 0 {
                let mi = c.mi();
                if mi == 0 {
                    0
                } else {
                    c.spsr[mi - 1]
                }
            } else {
                c.cpsr
            };
            c.set(rd, value);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0ff0_fff0 == 0x0120_f000 {
            execute_msr_register(c, m, op, false);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0ff0_fff0 == 0x0160_f000 {
            execute_msr_register(c, m, op, true);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0fff_fff0 == 0x012f_ff10 {
            let rm = op & 0xf;
            let target = c.get(rm);
            c.branch(target);
            return Ok(());
        }
        if op & 0x0fff_fff0 == 0x012f_ff30 {
            let rm = op & 0xf;
            let target = c.get(rm);
            let mi = c.mi();
            c.lr[mi] = c.pc.wrapping_add(4);
            c.branch(target);
            return Ok(());
        }
        if op & 0x0000_00f0 == 0x0000_0090 {
            execute_multiply(c, m, op);
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        // Extra load/store (b7=1 && b4=1): halfword/signed-byte transfers have
        // bits[7:4] = 1SH1 with the L bit at bit20; the LDRD/STRD double-word
        // family uses the same 1101/1111 shapes but forces L=0 and encodes
        // load/store in the H position (bit5 = 0 LDRD / 1 STRD). Pinned by
        // clang: LDRH=0xE1D540B6, STRH=0xE1C540B6, LDRSB=0xE1D540D6,
        // LDRSH=0xE1D540F6, LDRD=0xE1C860D0, STRD=0xE1C860F0.
        if op & 0x0000_0090 == 0x0000_0090 {
            match op & 0x0000_00f0 {
                0xb0 => {
                    execute_halfword(c, m, op)?;
                    c.pc = c.pc.wrapping_add(4);
                    return Ok(());
                }
                0xd0 | 0xf0 if op & (1 << 20) != 0 => {
                    execute_halfword(c, m, op)?;
                    c.pc = c.pc.wrapping_add(4);
                    return Ok(());
                }
                0xd0 | 0xf0 => {
                    execute_ldrd(c, m, op)?;
                    return Ok(());
                }
                _ => {}
            }
        }
        if op & 0x0fff_0ff0 == 0x016f_0f10 {
            let rd = (op >> 12) & 0xf;
            let rm = op & 0xf;
            let value = c.get(rm);
            c.set(rd, value.leading_zeros());
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0ff0_00f0 == 0x0120_0070 {
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        if op & 0x0f00_0010 == 0x0e00_0010 {
            execute_cp_transfer(c, m, op)?;
            c.pc = c.pc.wrapping_add(4);
            return Ok(());
        }
        execute_dp(c, m, op, false)
    }
}

fn msr_field_bits(mask: u32) -> u32 {
    let mut field = 0;
    if mask & 1 != 0 {
        field |= 0x0000_00ff; // c: control bits [7:0]
    }
    if mask & 2 != 0 {
        field |= 0x0000_ff00; // x: extension bits [15:8]
    }
    if mask & 4 != 0 {
        field |= 0x00ff_0000; // s: status bits [23:16]
    }
    if mask & 8 != 0 {
        field |= 0xff00_0000; // f: flags bits [31:24]
    }
    field
}

fn execute_msr_register(c: &mut A32, m: &mut Machine, op: u32, spsr: bool) {
    let mask = (op >> 16) & 0xf;
    let rm = op & 0xf;
    let value = c.get(rm);
    let _ = m;
    let target = if spsr {
        let mi = c.mi();
        if mi == 0 {
            return;
        }
        &mut c.spsr[mi - 1]
    } else {
        &mut c.cpsr
    };
    let field_bits = msr_field_bits(mask);
    *target = (*target & !field_bits) | (value & field_bits);
}

fn execute_msr_immediate(c: &mut A32, op: u32) {
    let mask = (op >> 16) & 0xf;
    let rot = ((op >> 8) & 0xf) * 2;
    let imm = (op & 0xff).rotate_right(rot);
    let field_bits = msr_field_bits(mask);
    c.cpsr = (c.cpsr & !field_bits) | (imm & field_bits);
}

fn execute_supervisor(c: &mut A32, m: &mut Machine) -> Result<(), ExecError> {
    let _ = m;
    enter_exception(c, m, 0x08, MODE_SVC, false, false, c.pc.wrapping_add(4));
    Ok(())
}

fn execute_blx_immediate(c: &mut A32, op: u32) -> Result<(), ExecError> {
    let h = (op >> 24) & 1;
    let imm24 = op & 0x00ff_ffff;
    let offset = sign_extend((imm24 << 2) | (h << 1), 26);
    let mi = c.mi();
    c.lr[mi] = c.pc.wrapping_add(4);
    let base = (c.pc.wrapping_add(8)) & !3;
    let target = (base as i64 + i64::from(offset)) as u32;
    c.pc = target & !1;
    c.cpsr |= T_BIT;
    Ok(())
}

fn execute_branch(c: &mut A32, _m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let link = op & (1 << 24) != 0;
    let offset = sign_extend(op & 0x00ff_ffff, 24) << 2;
    if link {
        let mi = c.mi();
        c.lr[mi] = c.pc.wrapping_add(4);
    }
    c.pc = (c.pc.wrapping_add(8)).wrapping_add(offset as u32);
    Ok(())
}

fn execute_multiply(c: &mut A32, _m: &mut Machine, op: u32) {
    let set_flags = op & (1 << 20) != 0;
    let long = op & (1 << 23) != 0;
    if !long {
        let rd = (op >> 16) & 0xf;
        let rn = (op >> 12) & 0xf;
        let rs = (op >> 8) & 0xf;
        let rm = op & 0xf;
        let accumulate = op & (1 << 21) != 0;
        let mval = u64::from(c.get(rm)) * u64::from(c.get(rs))
            + if accumulate { u64::from(c.get(rn)) } else { 0 };
        let result = mval as u32;
        c.set(rd, result);
        if set_flags {
            c.set_flags_nz(result);
        }
    } else {
        let rdh = (op >> 16) & 0xf;
        let rdl = (op >> 12) & 0xf;
        let rs = (op >> 8) & 0xf;
        let rm = op & 0xf;
        let signed = op & (1 << 22) != 0;
        let accumulate = op & (1 << 21) != 0;
        let mval = if signed {
            i64::from(c.get(rm) as i32) * i64::from(c.get(rs) as i32)
        } else {
            i64::from(c.get(rm)) * i64::from(c.get(rs))
        };
        let acc = if accumulate {
            u64::from(c.get(rdl)) | (u64::from(c.get(rdh)) << 32)
        } else {
            0
        };
        let wide = (mval as u64).wrapping_add(acc);
        let lo = wide as u32;
        let hi = (wide >> 32) as u32;
        c.set(rdl, lo);
        c.set(rdh, hi);
        if set_flags {
            let result64 = wide as i64;
            c.cpsr &= !(N_MASK | Z_MASK);
            if result64 == 0 {
                c.cpsr |= Z_MASK;
            }
            if result64 < 0 {
                c.cpsr |= N_MASK;
            }
        }
    }
}

/// Decode the A32 "data processing" operand2 and set the operation running.
fn execute_dp(c: &mut A32, m: &mut Machine, op: u32, immediate: bool) -> Result<(), ExecError> {
    let opcode = (op >> 21) & 0xf;
    let set_flags = op & (1 << 20) != 0;
    let rn = (op >> 16) & 0xf;
    let rd = (op >> 12) & 0xf;

    let (op2, sh_carry) = if immediate {
        let rot = ((op >> 8) & 0xf) * 2;
        let imm = (op & 0xff).rotate_right(rot);
        let carry = if rot == 0 {
            carry_in(c.cpsr)
        } else {
            imm & N_MASK != 0
        };
        (imm, carry)
    } else {
        let register_shift = op & (1 << 4) != 0;
        let typ = (op >> 5) & 3;
        let rm = op & 0xf;
        let value = c.get(rm);
        if register_shift {
            let rs = (op >> 8) & 0xf;
            let amount = c.get(rs) & 0xff;
            apply_shift(value, typ, amount, carry_in(c.cpsr), false)
        } else {
            let amount = (op >> 7) & 0x1f;
            apply_shift(value, typ, amount, carry_in(c.cpsr), true)
        }
    };

    let result = execute_dp_core(c, opcode, rn, op2, sh_carry, set_flags, rd);
    let tests_only = matches!(opcode, 8..=11);
    let moved_pc = !tests_only && rd == 15;
    if !tests_only {
        if set_flags && rd == 15 {
            c.pc = result & !1;
            c.restore_spsr();
        } else if rd == 15 {
            c.branch(result);
        } else {
            c.set(rd, result);
        }
    }
    let _ = m;
    if !moved_pc {
        c.pc = c.pc.wrapping_add(4);
    }
    Ok(())
}

fn execute_dp_core(
    c: &mut A32,
    opcode: u32,
    rn: u32,
    op2: u32,
    sh_carry: bool,
    set_flags: bool,
    _rd: u32,
) -> u32 {
    let carry = if set_flags {
        sh_carry
    } else {
        carry_in(c.cpsr)
    };
    let left = c.get(rn);
    match opcode {
        0 | 1 | 12 | 13 | 14 | 15 => {
            let r = match opcode {
                0 => left & op2,
                1 => left ^ op2,
                12 => left | op2,
                13 => op2,
                14 => left & !op2,
                _ => !op2,
            };
            if set_flags {
                c.set_flags_nz(r);
                c.cpsr = (c.cpsr & !C_MASK) | (if carry { C_MASK } else { 0 });
            }
            r
        }
        8 | 9 => {
            let r = if opcode == 8 { left & op2 } else { left ^ op2 };
            if set_flags {
                c.set_flags_nz(r);
                c.cpsr = (c.cpsr & !C_MASK) | (if carry { C_MASK } else { 0 });
            }
            r
        }
        2 | 3 | 6 | 7 | 10 => {
            // SUB / RSB / SBC / RSC / CMP are all implemented as subtraction
            // of `a - b` with the ARMv7 borrow-from-carries semantics for the
            // SBC/RSC pair (subtract `1 - C`).
            let a = if matches!(opcode, 2 | 6 | 10) {
                left
            } else {
                op2
            };
            let b = if matches!(opcode, 2 | 6 | 10) {
                op2
            } else {
                left
            };
            let borrow = if matches!(opcode, 6 | 7) && !carry {
                1
            } else {
                0
            };
            let r = a.wrapping_sub(b).wrapping_sub(borrow);
            if set_flags || opcode == 10 {
                c.set_sub_flags(a, b.wrapping_add(borrow), r);
            }
            r
        }
        _ => {
            // ADD / ADC / CMN
            let rhs = if opcode == 5 {
                u64::from(op2) + u64::from(carry)
            } else {
                u64::from(op2)
            };
            let wide = u64::from(left) + rhs;
            let r = wide as u32;
            if set_flags || opcode == 11 {
                c.set_add_flags(left, rhs as u32, wide);
            }
            r
        }
    }
}

fn execute_cp_transfer(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let is_read = op & (1 << 20) != 0;
    let opc1 = (op >> 21) & 7;
    let crn = (op >> 16) & 0xf;
    let rt = (op >> 12) & 0xf;
    let cp = (op >> 8) & 0xf;
    let opc2 = (op >> 5) & 7;
    let crm = op & 0xf;
    if cp != 15 {
        return Err(ExecError::Undefined);
    }
    if is_read {
        let value = if (opc1, crn, crm, opc2) == (0, 1, 0, 0) {
            m.cores.active().state.sctlr_el1 as u32
        } else {
            0
        };
        c.set(rt, value);
    } else {
        let value = c.get(rt);
        if (opc1, crn, crm, opc2) == (0, 1, 0, 0) {
            m.cores.active_mut().state.sctlr_el1 = u64::from(value);
        }
    }
    Ok(())
}

fn execute_single(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let i = op & (1 << 25) != 0;
    let p = op & (1 << 24) != 0;
    let u = op & (1 << 23) != 0;
    let b = op & (1 << 22) != 0;
    let w = op & (1 << 21) != 0;
    let l = op & (1 << 20) != 0;
    let rn = (op >> 16) & 0xf;
    let rd = (op >> 12) & 0xf;
    let base = c.get(rn);

    let offset = if i {
        let typ = (op >> 5) & 3;
        let register_shift = op & (1 << 4) != 0;
        let value = c.get(op & 0xf);
        if register_shift {
            let rs = (op >> 8) & 0xf;
            let amount = c.get(rs) & 0xff;
            apply_shift(value, typ, amount, carry_in(c.cpsr), false).0
        } else {
            let amount = (op >> 7) & 0x1f;
            apply_shift(value, typ, amount, carry_in(c.cpsr), true).0
        }
    } else {
        op & 0xfff
    };

    let indexed = if u {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    };
    let effective = if p { indexed } else { base };
    let size = if b { 1 } else { 4 };
    let mut loaded_pc = false;
    if l {
        if rd == 15 {
            let target = load(m, u64::from(effective), size)?;
            c.pc = target & !1;
            loaded_pc = true;
        } else {
            let value = load(m, u64::from(effective), size)?;
            c.set(rd, value);
        }
    } else {
        let value = if rd == 15 {
            c.pc.wrapping_add(8)
        } else {
            c.get(rd)
        };
        store(m, u64::from(effective), size, value)?;
    }
    if w || !p {
        c.set(rn, indexed);
    }
    if !loaded_pc {
        c.pc = c.pc.wrapping_add(4);
    }
    Ok(())
}

fn execute_halfword(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let p = op & (1 << 24) != 0;
    let u = op & (1 << 23) != 0;
    let w = op & (1 << 21) != 0;
    let l = op & (1 << 20) != 0;
    let rn = (op >> 16) & 0xf;
    let rd = (op >> 12) & 0xf;
    let s = op & (1 << 6) != 0;
    let h = op & (1 << 5) != 0;
    let base = c.get(rn);

    let offset = if op & (1 << 22) != 0 {
        ((op >> 8) & 0xf) << 4 | (op & 0xf)
    } else {
        c.get(op & 0xf)
    };
    let indexed = if u {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    };
    let effective = if p { indexed } else { base };

    let size = if h { 2 } else { 1 };
    let mut loaded_pc = false;
    if l {
        let raw = load(m, u64::from(effective), size)?;
        let shift = (4 - size) * 8;
        let value = if s {
            ((raw << shift) as i32 >> shift) as u32
        } else {
            raw
        };
        if rd == 15 {
            c.pc = value & !1;
            loaded_pc = true;
        } else {
            c.set(rd, value);
        }
    } else {
        let value = if rd == 15 {
            c.pc.wrapping_add(8)
        } else {
            c.get(rd)
        };
        store(m, u64::from(effective), size, value)?;
    }
    if w || !p {
        c.set(rn, indexed);
    }
    if !loaded_pc {
        c.pc = c.pc.wrapping_add(4);
    }
    Ok(())
}

fn execute_ldrd(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let p = op & (1 << 24) != 0;
    let u = op & (1 << 23) != 0;
    let w = op & (1 << 21) != 0;
    // Double-word family: the load/store selector is the H bit (bit5): bit20
    // stays 0 for both LDRD (0xD0) and STRD (0xF0), leaving the load/store
    // distinction to bit5 exactly as clang encodes them.
    let l = op & (1 << 5) == 0;
    let rn = (op >> 16) & 0xf;
    let rt = (op >> 12) & 0xf;
    let rt2 = rt.wrapping_add(1) & 0xf;
    let base = u64::from(c.get(rn));
    let offset = if op & (1 << 22) != 0 {
        u64::from((((op >> 8) & 0xf) << 4) | (op & 0xf))
    } else {
        u64::from(c.get(op & 0xf))
    };
    let indexed = if u {
        base.wrapping_add(offset)
    } else {
        base.wrapping_sub(offset)
    };
    let a = if p { indexed } else { base };
    if l {
        let v1 = load(m, a, 4)?;
        let v2 = load(m, a.wrapping_add(4), 4)?;
        c.set(rt, v1);
        c.set(rt2, v2);
    } else {
        store(m, a, 4, c.get(rt))?;
        store(m, a.wrapping_add(4), 4, c.get(rt2))?;
    }
    if w || !p {
        c.set(rn, indexed as u32);
    }
    c.pc = c.pc.wrapping_add(4);
    Ok(())
}

fn execute_block(c: &mut A32, m: &mut Machine, op: u32) -> Result<(), ExecError> {
    let p = op & (1 << 24) != 0;
    let u = op & (1 << 23) != 0;
    let s = op & (1 << 22) != 0;
    let w = op & (1 << 21) != 0;
    let l = op & (1 << 20) != 0;
    let rn = (op >> 16) & 0xf;
    let rc_mode = if s { MODE_USR } else { c.mode() };
    let list = op & 0xffff;
    let count = list.count_ones();
    let loaded_pc = l && list & (1 << 15) != 0;
    let base = if s {
        c.get_banked(rc_mode, rn)
    } else {
        c.get(rn)
    };

    let match_index = |reg: u32| -> u32 {
        let mut pos = 0;
        for r in 0..=reg {
            if list & (1 << r) != 0 {
                pos += 1;
            }
        }
        pos - 1
    };

    for reg in 0..16u32 {
        if list & (1 << reg) == 0 {
            continue;
        }
        let pos = match_index(reg);
        let address = match (p, u) {
            (false, true) => base.wrapping_add(4 * pos),
            (true, true) => base.wrapping_add(4).wrapping_add(4 * pos),
            (false, false) => base.wrapping_sub(4 * pos),
            (true, false) => base.wrapping_sub(4 * count).wrapping_add(4 * pos),
        };
        if l {
            let value = load(m, u64::from(address), 4)?;
            if reg == 15 {
                c.pc = value & !1;
                if s {
                    c.restore_spsr();
                }
            } else if s {
                c.set_banked(MODE_USR, reg, value);
            } else {
                c.set(reg, value);
            }
        } else {
            let value = if s {
                c.get_banked(MODE_USR, reg)
            } else {
                c.get(reg)
            };
            store(m, u64::from(address), 4, value)?;
        }
    }
    if w {
        let final_base = if u {
            base.wrapping_add(4 * count)
        } else {
            base.wrapping_sub(4 * count)
        };
        c.set(rn, final_base);
    }
    if !loaded_pc {
        c.pc = c.pc.wrapping_add(4);
    }
    Ok(())
}

/// Execute one A32 (ARM-state) instruction.
pub fn step(m: &mut Machine) -> Result<(), ExecError> {
    if matches!(
        m.cores.active_mut().run_state,
        CpuRunState::WaitingForInterrupt
    ) {
        if m.cores.active().pending_irq.is_none() && m.cores.active().pending_fiq.is_none() {
            return Ok(());
        }
        m.cores.active_mut().run_state = CpuRunState::Running;
    }
    let mut c = A32::from_state(&m.cores.active().state);
    if take_pending_interrupt(&mut c, m) {
        m.cores.active_mut().pending_irq = None;
        m.cores.active_mut().pending_fiq = None;
        c.commit(&mut m.cores.active_mut().state);
        m.cores.active_mut().instructions = m.cores.active_mut().instructions.wrapping_add(1);
        return Ok(());
    }
    if m.cores.active().state.a32.cpsr & T_BIT != 0 {
        // Thumb-2 is the narrow module's path, and the *handoff* is this core's:
        // the A9 SEPROM's own reset path (`AppleSEPROM-230.0.0.4.1`, file offset
        // 0x42ac..0x42b4) ends `movw ip,#0x4cdd; movt ip,#0x1000; bx ip`, so a
        // SEPROM that starts in ARM state ends in Thumb state. Returning
        // `Undefined` here made that image unrunnable on the full core and left
        // the split unexplained -- the image looked like it was an ARM-only ROM.
        //
        // Both modules read and write the same `CpuState`, and this is checked
        // before `A32::from_state` so no register state has been staged: the
        // narrow module works on the state directly, and the run-state and
        // pending-interrupt handling above is shared, so delegating cannot skip
        // an interrupt that is already pending.
        return super::aarch32::step(m);
    }

    let pc = c.pc;
    let op = fetch(m, u64::from(pc))?;
    m.current_instruction_pc = Some(u64::from(pc));
    let result = execute(&mut c, m, op);
    let op_word = if result.is_ok() {
        op
    } else {
        m.mem_read(u64::from(pc), 4).unwrap_or(0) as u32
    };
    m.current_instruction_pc = None;
    c.commit(&mut m.cores.active_mut().state);
    result?;
    m.cores.active_mut().instructions = m.cores.active_mut().instructions.wrapping_add(1);
    if m.trace.is_enabled() {
        m.trace.record(
            u64::from(pc),
            u64::from(op_word),
            4,
            TraceKind::Instruction,
            u64::from(pc),
            0,
            u64::from(c.cpsr),
            None,
        );
    }
    Ok(())
}

/// Reset the machine into A32 state with the given reset CPSR and registers.
/// Convenience for harnesses/probes booting the SEP core.
pub fn enter(m: &mut Machine, pc: u32, cpsr: u32) {
    m.cores.active_mut().state.pc = u64::from(pc);
    m.cores.active_mut().state.pstate = u64::from(cpsr);
    m.cores.active_mut().state.a32.cpsr = cpsr;
    m.cores.active_mut().state.a32 = iphem_core::Aarch32State::default();
    m.cores.active_mut().state.a32.cpsr = cpsr;
    m.cores.active_mut().run_state = CpuRunState::Running;
}

/// A32 interrupt delivery for the SEP harness: parks the core while masked,
/// otherwise takes the pending FIQ/IRQ vector on the next `step`.
pub fn signal_irq(m: &mut Machine) {
    if m.cores.active_mut().state.a32.cpsr & (1 << 7) != 0 {
        m.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iphem_core::{BusRegion, MachineConfig, SparseRam};

    fn machine() -> Machine {
        let mut machine = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        machine
            .bus
            .map(BusRegion {
                base: 0,
                size: 0x10_0000,
                device: Box::new(SparseRam::new(0, 0x10_0000)),
            })
            .unwrap();
        machine
    }

    fn boot(m: &mut Machine, cpsr: u32) {
        enter(m, 0x8000, cpsr);
    }

    fn run(m: &mut Machine, words: &[u32]) {
        for (i, w) in words.iter().enumerate() {
            m.mem_write(0x8000 + (i as u64) * 4, 4, u64::from(*w))
                .unwrap();
        }
    }

    const SVC_MODE: u32 = 0xd3; // SVC, I, F
    const USR_MODE: u32 = 0xd0; // USR, I, F

    #[test]
    fn mov_ror_and_rrx_follow_armv7_barrel_shifter_rules() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        // clang: mov r0,#0x08 = 0xe3a00008; mov r1,r0,ror #4 = 0xe1a01260;
        // mov r2,r0,rrx = 0xe1a02060.
        run(&mut m, &[0xe3a0_0008, 0xe1a0_1260, 0xe1a0_2060]);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[0], 0x08);
        step(&mut m).unwrap();
        // ror #4 of 0x00000008 -> 0x80000000 (wraps the low bits up).
        assert_eq!(m.cores.active_mut().state.x[1], 0x8000_0000);
        // C clear: rrx shifts r0 in 0 -> 0x08 >> 1 = 0x04.
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[2], 0x04);
        // C set: rrx of 0x80000000 rotates in the carry -> 0xC0000000.
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.a32.cpsr |= C_MASK;
        m.cores.active_mut().state.x[0] = 0x8000_0000;
        run(&mut m, &[0xe1a0_2060]);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[2], 0xc000_0000);
    }

    #[test]
    fn data_processing_sets_and_clears_nzcv_correctly() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        // movs r2,#5 = 0xe3b02005; adds r3,r2,#1 = 0xe2923001;
        // subs r4,r2,r2 = 0xe0524003.
        run(&mut m, &[0xe3b0_2005, 0xe292_3001, 0xe052_4003]);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[2], 5);
        assert_eq!(m.cores.active_mut().state.a32.cpsr & (N_MASK | Z_MASK), 0);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[3], 6);
        assert_eq!(m.cores.active_mut().state.a32.cpsr & (N_MASK | Z_MASK), 0);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[4], 0);
        assert_ne!(m.cores.active_mut().state.a32.cpsr & Z_MASK, 0);
    }

    #[test]
    fn multiply_and_multiply_long_execute() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.x[1] = 7;
        m.cores.active_mut().state.x[2] = 6;
        m.cores.active_mut().state.x[3] = 9;
        m.cores.active_mut().state.x[4] = 0x1_0000;
        m.cores.active_mut().state.x[5] = 0x1_0000;
        // mul r0,r1,r2 = 0xe0000291 -> 42; smlal r7,r8,r3,r3 = 0xe0e87393
        // -> signed 9*9 + {r8,r7}; umull r7,r8,r4,r5 = 0xe0887594 -> 2^32.
        run(
            &mut m,
            &[0xe000_0291, 0xe0e8_7393, 0xe088_7594, 0xe12f_ff1e],
        );
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[0], 42);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[7], 81);
        assert_eq!(m.cores.active_mut().state.x[8], 0);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[7], 0);
        assert_eq!(m.cores.active_mut().state.x[8], 1);
    }

    #[test]
    fn block_transfer_database_stack_pointer_writeback_matches_arm() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.a32.sp[3] = 0x9000;
        m.cores.active_mut().state.x[0] = 0x1111_1111;
        m.cores.active_mut().state.x[1] = 0x2222_2222;
        m.cores.active_mut().state.x[4] = 0x4444_4444;
        m.cores.active_mut().state.a32.lr[3] = 0x5555_5555;
        // stmdb sp!,{r0,r1} = 0xe92d0003; ldmia sp!,{r2,r3} = 0xe8bd000c;
        // stmfd sp!,{r4,lr} = 0xe92d4011; ldmfd sp!,{r4,r6,lr} = 0xe8bd4050.
        run(
            &mut m,
            &[
                0xe92d_0003,
                0xe8bd_000c,
                0xe92d_4011,
                0xe8bd_4050,
                0xe12f_ff1e,
            ],
        );
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.a32.sp[3], 0x8ff8);
        assert_eq!(m.mem_read(0x8ff8, 4).unwrap(), 0x1111_1111);
        assert_eq!(m.mem_read(0x8ffc, 4).unwrap(), 0x2222_2222);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[2], 0x1111_1111);
        assert_eq!(m.cores.active_mut().state.x[3], 0x2222_2222);
        assert_eq!(m.cores.active_mut().state.a32.sp[3], 0x9000);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.a32.sp[3], 0x8ff8);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[4], 0x4444_4444);
        // ldmfd sp!,{r4,r6,lr} pops 3 registers: sp 0x8ff8 -> 0x9004.
        assert_eq!(m.cores.active_mut().state.a32.sp[3], 0x9004);
    }

    #[test]
    fn ldr_ldrd_strd_and_signed_halfword_transfers_execute() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.x[8] = 0x9000;
        m.cores.active_mut().state.x[5] = 0x9020;
        m.cores.active_mut().state.x[6] = 0xdead_beef;
        m.cores.active_mut().state.x[7] = 0xcafe_f000;
        // ldrd r6,r7,[r8] = 0xe1c860d0; strd r6,r7,[r8,#16]! = 0xe1e861f0;
        // ldrsh r4,[r5,#6] = 0xe1d540f6; strh r4,[r5,#6] = 0xe1c540b6.
        m.mem_write(0x9000, 4, 0xcafe_f000).unwrap();
        m.mem_write(0x9004, 4, 0xdead_beef).unwrap();
        m.mem_write(0x9026, 2, 0xffff).unwrap();
        run(
            &mut m,
            &[0xe1c8_60d0, 0xe1e8_61f0, 0xe1d5_40f6, 0xe1c5_40b6],
        );
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[6], 0xcafe_f000);
        assert_eq!(m.cores.active_mut().state.x[7], 0xdead_beef);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[8], 0x9010);
        assert_eq!(m.mem_read(0x9010, 4).unwrap(), 0xcafe_f000);
        assert_eq!(m.mem_read(0x9014, 4).unwrap(), 0xdead_beef);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[4], 0xffff_ffff);
        step(&mut m).unwrap();
        assert_eq!(m.mem_read(0x9026, 2).unwrap(), 0xffff);
    }

    #[test]
    fn msr_register_spsr_and_clz_discriminate_by_psr_and_report_zeros() {
        let mut m = machine();
        boot(&mut m, USR_MODE);
        // svc #1 vectors to 0x08 (branches to 0x8020) in SVC mode, then:
        // clz r0,r1 = 0xe16f0f11; msr cpsr_x,r1 = 0xe122f001;
        // msr spsr_f,r1 = 0xe168f001; msr spsr_x,r2 = 0xe162f002;
        // mrs r3,cpsr = 0xe10f3003; mrs r4,spsr = 0xe14f4004.
        run(&mut m, &[0xef00_0001]);
        m.mem_write(0x8020, 4, 0xe16f_0f11).unwrap();
        m.mem_write(0x8024, 4, 0xe122_f001).unwrap();
        m.mem_write(0x8028, 4, 0xe168_f001).unwrap();
        m.mem_write(0x802c, 4, 0xe162_f002).unwrap();
        m.mem_write(0x8030, 4, 0xe10f_3000).unwrap();
        m.mem_write(0x8034, 4, 0xe14f_4000).unwrap();
        m.mem_write(0x08, 4, 0xea00_2004).unwrap();
        m.cores.active_mut().state.x[1] = 0x8000_0001;
        m.cores.active_mut().state.x[2] = 0x0000_0100;
        // svc #1
        step(&mut m).unwrap();
        // b 0x8020
        step(&mut m).unwrap();
        // clz r0,r1 at 0x8020
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[0], 0);
        // msr cpsr_x,r1
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.a32.cpsr & 0x0000_ff00, 0);
        // msr spsr_f,r1
        step(&mut m).unwrap();
        assert_eq!(
            m.cores.active_mut().state.a32.spsr[2] & 0x8000_0000,
            0x8000_0000
        );
        // msr spsr_x,r2
        step(&mut m).unwrap();
        assert_eq!(
            m.cores.active_mut().state.a32.spsr[2] & 0x0000_ff00,
            0x0000_0100
        );
        // mrs r3,cpsr
        step(&mut m).unwrap();
        assert_eq!(
            m.cores.active().state.x[3],
            u64::from(m.cores.active().state.a32.cpsr)
        );
        // mrs r4,spsr
        step(&mut m).unwrap();
        assert_eq!(
            m.cores.active().state.x[4],
            u64::from(m.cores.active().state.a32.spsr[2])
        );
    }

    #[test]
    fn svc_enters_banked_mode_and_movs_pc_lr_returns() {
        let mut m = machine();
        boot(&mut m, USR_MODE);
        // svc #1 = 0xef000001, handler movs pc,lr = 0xe1b0f00e.
        run(&mut m, &[0xef00_0001]);
        m.mem_write(0x8020, 4, 0xe1b0_f00e).unwrap();
        // Vector at 0x08: b 0x8020. offset = (0x8020 - 0x10) / 4 = 0x2004.
        m.mem_write(0x08, 4, 0xea00_2004).unwrap();
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.pc, 0x08);
        assert_eq!(m.cores.active_mut().state.a32.cpsr & 0x1f, MODE_SVC);
        assert_eq!(m.cores.active_mut().state.a32.lr[3], 0x8004);
        assert_eq!(m.cores.active_mut().state.a32.spsr[2], USR_MODE);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.pc, 0x8020);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.a32.cpsr, USR_MODE);
        assert_eq!(m.cores.active_mut().state.pc, 0x8004);
    }

    #[test]
    fn fiq_delivery_is_masked_by_cpsr_f_and_taken_when_unmasked() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().pending_fiq = Some(0);
        run(&mut m, &[0xe3a0_2005]);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.pc, 0x8004);
        m.cores.active_mut().state.a32.cpsr &= !(1 << 6);
        m.cores.active_mut().pending_fiq = Some(0);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.pc, 0x1c);
        assert_eq!(m.cores.active_mut().state.a32.cpsr & 0x1f, MODE_FIQ);
        assert_ne!(m.cores.active_mut().state.a32.cpsr & (1 << 6), 0);
        assert_eq!(m.cores.active_mut().state.a32.lr[1], 0x8008);
    }

    #[test]
    fn conditional_execution_skips_false_branches() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.x[1] = 0xdead_beef;
        // combineq r0,r1 = 0x01a00001; movne r0,r1 = 0x11a00001.
        run(&mut m, &[0x01a0_0001, 0x11a0_0001]);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.pc, 0x8004);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[0], 0xdead_beef);
        assert_eq!(m.cores.active_mut().state.pc, 0x8008);
    }

    #[test]
    fn movw_movt_mrs_and_msr_round_trip() {
        let mut m = machine();
        boot(&mut m, SVC_MODE);
        m.cores.active_mut().state.x[0] = 0x8000_0000;
        // movw r5,#0xabcd = 0xe30a5bcd; movt r6,#0x1357 = 0xe3416357;
        // msr cpsr_f,r0 = 0xe128f000; mrs r1,cpsr = 0xe10f1000;
        // msr cpsr_c,#0x13 = 0xe321f013.
        run(
            &mut m,
            &[
                0xe30a_5bcd,
                0xe341_6357,
                0xe128_f000,
                0xe10f_1000,
                0xe321_f013,
            ],
        );
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[5], 0xabcd);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.x[6], 0x1357_0000);
        step(&mut m).unwrap();
        assert_ne!(m.cores.active_mut().state.a32.cpsr & N_MASK, 0);
        step(&mut m).unwrap();
        assert_ne!(m.cores.active_mut().state.x[1] as u32 & N_MASK, 0);
        step(&mut m).unwrap();
        assert_eq!(m.cores.active_mut().state.a32.cpsr & 0x1f, MODE_SVC);
        assert_eq!(m.cores.active_mut().state.a32.cpsr & 0xff, 0x13);
    }
}
