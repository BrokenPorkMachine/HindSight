//! Narrow ARM/Thumb interpreter used by the T8020 SEPROM bootstrap path.
//!
//! The AP and normal SEP OS run AArch64. The T8020 SEPROM reset image is an
//! AArch32 vector table that enters Thumb-2 code before handing off to SEPOS.
//! Keep this execution path explicit and separate from the AArch64 decoder so
//! unsupported AArch32 behavior cannot silently alter ordinary CPU execution.

use crate::ExecError;
use iphem_core::{CpuRunState, Machine, TraceKind};

const T_BIT: u64 = 1 << 5;
const N_BIT: u64 = 1 << 31;
const Z_BIT: u64 = 1 << 30;
const C_BIT: u64 = 1 << 29;
const V_BIT: u64 = 1 << 28;

fn thumb(state: &iphem_core::CpuState) -> bool {
    state.pstate & T_BIT != 0
}

fn reg_read(state: &iphem_core::CpuState, n: u32, pc: u64, is_thumb: bool) -> u32 {
    match n {
        13 => state.sp as u32,
        15 => pc.wrapping_add(if is_thumb { 4 } else { 8 }) as u32,
        _ => state.x[n as usize] as u32,
    }
}

fn reg_write(state: &mut iphem_core::CpuState, n: u32, value: u32) {
    match n {
        13 => state.sp = value as u64,
        15 => state.pc = (value & !1) as u64,
        _ => state.x[n as usize] = value as u64,
    }
}

fn condition(pstate: u64, cond: u32) -> bool {
    let n = pstate & N_BIT != 0;
    let z = pstate & Z_BIT != 0;
    let c = pstate & C_BIT != 0;
    let v = pstate & V_BIT != 0;
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

fn set_nz(pstate: &mut u64, result: u32) {
    *pstate &= !(N_BIT | Z_BIT);
    if result & 0x8000_0000 != 0 {
        *pstate |= N_BIT;
    }
    if result == 0 {
        *pstate |= Z_BIT;
    }
}

fn set_sub_flags(pstate: &mut u64, left: u32, right: u32, result: u32) {
    set_nz(pstate, result);
    *pstate &= !(C_BIT | V_BIT);
    if left >= right {
        *pstate |= C_BIT;
    }
    if ((left ^ right) & (left ^ result) & 0x8000_0000) != 0 {
        *pstate |= V_BIT;
    }
}

fn set_add_flags(pstate: &mut u64, left: u32, right: u32, wide: u64) {
    let result = wide as u32;
    set_nz(pstate, result);
    *pstate &= !(C_BIT | V_BIT);
    if wide >> 32 != 0 {
        *pstate |= C_BIT;
    }
    if (!(left ^ right) & (left ^ result) & 0x8000_0000) != 0 {
        *pstate |= V_BIT;
    }
}

fn fetch16(m: &mut Machine, address: u64) -> Result<u16, ExecError> {
    Ok(m.mem_read(address, 2)? as u16)
}

fn load(m: &mut Machine, address: u64, size: usize) -> Result<u32, ExecError> {
    Ok(m.mem_read(address, size)? as u32)
}

fn store(m: &mut Machine, address: u64, size: usize, value: u32) -> Result<(), ExecError> {
    m.mem_write(address, size, value as u64)?;
    Ok(())
}

fn cp15_read(m: &Machine, opc1: u32, crn: u32, crm: u32, opc2: u32) -> u32 {
    let state = &m.cores.active().state;
    match (opc1, crn, crm, opc2) {
        (0, 1, 0, 0) => state.sctlr_el1 as u32,
        // Auxiliary control and implementation-defined SEP registers are not
        // needed to leave the observed reset setup; expose reset zero until a
        // evidence-backed register semantics are available.
        _ => 0,
    }
}

fn cp15_write(m: &mut Machine, opc1: u32, crn: u32, crm: u32, opc2: u32, value: u32) {
    if (opc1, crn, crm, opc2) == (0, 1, 0, 0) {
        m.cores.active_mut().state.sctlr_el1 = value as u64;
    }
}

fn thumb_expand_imm(first: u16, second: u16) -> u32 {
    let imm12 = (u32::from((first >> 10) & 1) << 11)
        | (u32::from((second >> 12) & 7) << 8)
        | u32::from(second & 0xff);
    let imm8 = imm12 & 0xff;
    if imm12 >> 10 == 0 {
        return match (imm12 >> 8) & 3 {
            0 => imm8,
            1 => (imm8 << 16) | imm8,
            2 => (imm8 << 24) | (imm8 << 8),
            _ => imm8.wrapping_mul(0x0101_0101),
        };
    }
    let unrotated = 0x80 | (imm12 & 0x7f);
    unrotated.rotate_right((imm12 >> 7) & 0x1f)
}

fn execute_cp_transfer(m: &mut Machine, op: u32) -> Result<(), ExecError> {
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
        let value = cp15_read(m, opc1, crn, crm, opc2);
        reg_write(&mut m.cores.active_mut().state, rt, value);
    } else {
        let value = reg_read(
            &m.cores.active().state,
            rt,
            m.cores.active().state.pc,
            thumb(&m.cores.active().state),
        );
        cp15_write(m, opc1, crn, crm, opc2, value);
    }
    Ok(())
}

fn execute_arm(m: &mut Machine, pc: u64, op: u32) -> Result<(), ExecError> {
    let cond = op >> 28;
    if cond != 0xf && !condition(m.cores.active_mut().state.pstate, cond) {
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // BLX (immediate) is the SEPROM reset vector's ARM → Thumb transition.
    if op & 0xfe00_0000 == 0xfa00_0000 {
        let offset = (((op & 0x00ff_ffff) << 2) | (((op >> 24) & 1) << 1)) as i32;
        let target = (pc as u32)
            .wrapping_add(8)
            .wrapping_add((offset << 6 >> 6) as u32);
        m.cores.active_mut().state.x[14] = (pc + 4) | 1;
        m.cores.active_mut().state.pstate |= T_BIT;
        m.cores.active_mut().state.pc = (target & !1) as u64;
        return Ok(());
    }

    // WFI/WFE/YIELD/SEV hints. A32 WFI enters the reset ROM's deliberate
    // idle loop when the firmware handoff signature check fails.
    if op & 0x0fff_fff0 == 0x0320_f000 {
        let hint = op & 0xf;
        if hint == 3 {
            m.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // B / BL immediate.
    if op & 0x0e00_0000 == 0x0a00_0000 {
        let offset = ((op << 8) as i32 >> 6) as u32;
        if op & (1 << 24) != 0 {
            m.cores.active_mut().state.x[14] = pc + 4;
        }
        m.cores.active_mut().state.pc = (pc as u32).wrapping_add(8).wrapping_add(offset) as u64;
        return Ok(());
    }

    // MRC / MCR (including the SEPROM's SCTLR setup).
    if op & 0x0f00_0010 == 0x0e00_0010 {
        execute_cp_transfer(m, op)?;
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // BX / BLX register.
    if op & 0x0fff_fff0 == 0x012f_ff10 || op & 0x0fff_fff0 == 0x012f_ff30 {
        let rm = op & 0xf;
        let target = reg_read(&m.cores.active().state, rm, pc, false);
        if op & 0x20 != 0 {
            m.cores.active_mut().state.x[14] = pc + 4;
        }
        if target & 1 != 0 {
            m.cores.active_mut().state.pstate |= T_BIT;
        } else {
            m.cores.active_mut().state.pstate &= !T_BIT;
        }
        m.cores.active_mut().state.pc = (target & !1) as u64;
        return Ok(());
    }

    // Data-processing immediate subset used by the vector table.
    if op & 0x0e00_0000 == 0x0200_0000 {
        let opcode = (op >> 21) & 0xf;
        let set_flags = op & (1 << 20) != 0;
        let rn = (op >> 16) & 0xf;
        let rd = (op >> 12) & 0xf;
        let rotate = ((op >> 8) & 0xf) * 2;
        let imm = (op & 0xff).rotate_right(rotate);
        let left = reg_read(&m.cores.active().state, rn, pc, false);
        let result = match opcode {
            2 | 10 => left.wrapping_sub(imm), // SUB / CMP
            4 => left.wrapping_add(imm),
            12 => left & imm,
            13 => imm, // MOV
            14 => left & !imm,
            _ => return Err(ExecError::Undefined),
        };
        if opcode != 10 {
            reg_write(&mut m.cores.active_mut().state, rd, result);
        }
        if set_flags || opcode == 10 {
            match opcode {
                2 | 10 => set_sub_flags(&mut m.cores.active_mut().state.pstate, left, imm, result),
                4 => set_add_flags(
                    &mut m.cores.active_mut().state.pstate,
                    left,
                    imm,
                    left as u64 + imm as u64,
                ),
                _ => set_nz(&mut m.cores.active_mut().state.pstate, result),
            }
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    Err(ExecError::Undefined)
}

fn execute_thumb16(m: &mut Machine, pc: u64, op: u16) -> Result<(), ExecError> {
    // Work on a snapshot so memory accesses can borrow the Machine while this
    // instruction's register effects remain isolated until it completes.
    let mut state = m.cores.active().state.clone();
    let rd = ((op >> 8) & 7) as u32;
    match op & 0xf800 {
        0x2000 => {
            let value = (op & 0xff) as u32;
            state.x[rd as usize] = value as u64;
            set_nz(&mut state.pstate, value);
        }
        0x2800 => {
            let left = reg_read(&state, rd, pc, true);
            set_sub_flags(
                &mut state.pstate,
                left,
                (op & 0xff) as u32,
                left.wrapping_sub((op & 0xff) as u32),
            );
        }
        0x3000 | 0x3800 => {
            let left = reg_read(&state, rd, pc, true);
            let right = (op & 0xff) as u32;
            if op & 0xf800 == 0x3000 {
                let wide = left as u64 + right as u64;
                state.x[rd as usize] = wide as u32 as u64;
                set_add_flags(&mut state.pstate, left, right, wide);
            } else {
                let result = left.wrapping_sub(right);
                state.x[rd as usize] = result as u64;
                set_sub_flags(&mut state.pstate, left, right, result);
            }
        }
        0xe000 => {
            let imm = (((op & 0x7ff) as i32) << 21 >> 20) as u32;
            state.pc = (pc as u32).wrapping_add(4).wrapping_add(imm) as u64;
            m.cores.active_mut().state = state;
            return Ok(());
        }
        0xf000 | 0xf800 => return Err(ExecError::Undefined), // Thumb-2 BL handled by the wide decoder.
        _ => {
            if op & 0xf000 == 0xd000 {
                let cond = ((op >> 8) & 0xf) as u32;
                if cond == 0xf {
                    return Err(ExecError::Undefined); // SVC entry is added with the SEP vector path.
                }
                if condition(state.pstate, cond) {
                    let imm = (((op & 0xff) as i32) << 24 >> 23) as u32;
                    state.pc = (pc as u32).wrapping_add(4).wrapping_add(imm) as u64;
                    m.cores.active_mut().state = state;
                    return Ok(());
                }
            } else if op & 0xf800 == 0x1800 {
                let immediate = op & 0x0400 != 0;
                let subtract = op & 0x0200 != 0;
                let operand = if immediate {
                    u32::from((op >> 6) & 7)
                } else {
                    reg_read(&state, u32::from((op >> 6) & 7), pc, true)
                };
                let rn = u32::from((op >> 3) & 7);
                let rd = u32::from(op & 7);
                let left = reg_read(&state, rn, pc, true);
                if subtract {
                    let result = left.wrapping_sub(operand);
                    state.x[rd as usize] = result as u64;
                    set_sub_flags(&mut state.pstate, left, operand, result);
                } else {
                    let wide = u64::from(left) + u64::from(operand);
                    state.x[rd as usize] = wide as u32 as u64;
                    set_add_flags(&mut state.pstate, left, operand, wide);
                }
            } else if op & 0xff00 == 0x4700 {
                let rm = ((op >> 3) & 0xf) as u32;
                let target = reg_read(&state, rm, pc, true);
                if op & 0x0080 != 0 {
                    state.x[14] = pc + 2;
                }
                if target & 1 != 0 {
                    state.pstate |= T_BIT;
                } else {
                    state.pstate &= !T_BIT;
                }
                state.pc = (target & !1) as u64;
                m.cores.active_mut().state = state;
                return Ok(());
            } else if op & 0xff00 == 0x4600 {
                let rd = (((op >> 4) & 8) | (op & 7)) as u32;
                let rm = ((op >> 3) & 0xf) as u32;
                let value = reg_read(&state, rm, pc, true);
                reg_write(&mut state, rd, value);
                if rd == 15 {
                    m.cores.active_mut().state = state;
                    return Ok(());
                }
            } else if op & 0xfc00 == 0x4400 {
                let kind = (op >> 8) & 3;
                let rd = (((op >> 4) & 8) | (op & 7)) as u32;
                let rm = ((op >> 3) & 0xf) as u32;
                let left = reg_read(&state, rd, pc, true);
                let right = reg_read(&state, rm, pc, true);
                match kind {
                    0 => {
                        reg_write(&mut state, rd, left.wrapping_add(right));
                        if rd == 15 {
                            m.cores.active_mut().state = state;
                            return Ok(());
                        }
                    }
                    1 => set_sub_flags(&mut state.pstate, left, right, left.wrapping_sub(right)),
                    2 => {
                        reg_write(&mut state, rd, right);
                        if rd == 15 {
                            m.cores.active_mut().state = state;
                            return Ok(());
                        }
                    }
                    _ => return Err(ExecError::Undefined),
                }
            } else if op & 0xff00 == 0xbf00 {
                // NOP, WFE, WFI, and YIELD hints used during early startup.
            } else if op & 0xf800 == 0x4800 {
                let address = ((pc + 4) & !3).wrapping_add(((op & 0xff) as u64) << 2);
                let value = load(m, address, 4)?;
                state.x[rd as usize] = value as u64;
            } else if op & 0xfc00 == 0x4000 {
                let opcode = (op >> 6) & 0xf;
                let rm = u32::from((op >> 3) & 7);
                let rn = u32::from(op & 7);
                let left = reg_read(&state, rn, pc, true);
                let right = reg_read(&state, rm, pc, true);
                if opcode == 10 {
                    set_sub_flags(&mut state.pstate, left, right, left.wrapping_sub(right));
                } else {
                    return Err(ExecError::Undefined);
                }
            } else if op & 0xf000 == 0x5000 {
                let kind = (op >> 9) & 7;
                let rm = ((op >> 6) & 7) as u32;
                let rn = ((op >> 3) & 7) as u32;
                let rt = (op & 7) as u32;
                let address =
                    reg_read(&state, rn, pc, true) as u64 + reg_read(&state, rm, pc, true) as u64;
                match kind {
                    0 => store(m, address, 4, reg_read(&state, rt, pc, true))?,
                    2 => store(m, address, 1, reg_read(&state, rt, pc, true))?,
                    4 => state.x[rt as usize] = load(m, address, 4)? as u64,
                    6 => state.x[rt as usize] = load(m, address, 1)? as u64,
                    _ => return Err(ExecError::Undefined),
                }
            } else if op & 0xf800 == 0x6000 || op & 0xf800 == 0x6800 {
                let rn = ((op >> 3) & 7) as u32;
                let rt = (op & 7) as u32;
                let address =
                    reg_read(&state, rn, pc, true) as u64 + (((op >> 6) & 0x1f) as u64 * 4);
                if op & 0x0800 == 0 {
                    store(m, address, 4, reg_read(&state, rt, pc, true))?;
                } else {
                    state.x[rt as usize] = load(m, address, 4)? as u64;
                }
            } else if op & 0xfe00 == 0xb400 {
                // PUSH {registers,lr}.
                let list = (op & 0xff) as u32;
                let with_lr = op & 0x0100 != 0;
                let count = list.count_ones() + u32::from(with_lr);
                let mut address = state.sp.wrapping_sub(u64::from(count) * 4);
                for n in 0..8 {
                    if list & (1 << n) != 0 {
                        store(m, address, 4, state.x[n] as u32)?;
                        address += 4;
                    }
                }
                if with_lr {
                    store(m, address, 4, state.x[14] as u32)?;
                }
                state.sp = state.sp.wrapping_sub(u64::from(count) * 4);
            } else if op & 0xfe00 == 0xbc00 {
                // POP {registers,pc}.
                let list = (op & 0xff) as u32;
                let with_pc = op & 0x0100 != 0;
                let mut address = state.sp;
                for n in 0..8 {
                    if list & (1 << n) != 0 {
                        state.x[n] = load(m, address, 4)? as u64;
                        address += 4;
                    }
                }
                if with_pc {
                    let target = load(m, address, 4)?;
                    address += 4;
                    if target & 1 != 0 {
                        state.pstate |= T_BIT;
                    } else {
                        state.pstate &= !T_BIT;
                    }
                    state.pc = (target & !1) as u64;
                    state.sp = address;
                    return Ok(());
                }
                state.sp = address;
            } else {
                return Err(ExecError::Undefined);
            }
        }
    }
    state.pc = pc + 2;
    m.cores.active_mut().state = state;
    Ok(())
}

fn execute_thumb32(m: &mut Machine, pc: u64, first: u16, second: u16) -> Result<(), ExecError> {
    // Architectural barrier and hint encodings (DSB/DMB/ISB/CLREX).
    if first == 0xf3bf && matches!(second & 0xfff0, 0x8f40 | 0x8f50 | 0x8f60 | 0x8f70) {
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // BL/BLX immediate. The SEPROM uses BLX to enter the ARM-state WFI
    // fallback, so decode it before the overlapping F1/F3 immediate classes.
    if first & 0xf800 == 0xf000 && second & 0xd000 == 0xc000 {
        let s = u32::from((first >> 10) & 1);
        let j1 = u32::from((second >> 13) & 1);
        let j2 = u32::from((second >> 11) & 1);
        let i1 = !(j1 ^ s) & 1;
        let i2 = !(j2 ^ s) & 1;
        let is_blx = second & 0x1000 == 0;
        let imm = (s << 24)
            | (i1 << 23)
            | (i2 << 22)
            | (u32::from(first & 0x03ff) << 12)
            | if is_blx {
                u32::from((second >> 1) & 0x03ff) << 2
            } else {
                u32::from(second & 0x07ff) << 1
            };
        let offset = ((imm << 7) as i32 >> 7) as u32;
        m.cores.active_mut().state.x[14] = (pc + 4) | 1;
        let base = if is_blx { (pc + 4) & !3 } else { pc + 4 };
        m.cores.active_mut().state.pc = u64::from((base as u32).wrapping_add(offset) & !1);
        if is_blx {
            m.cores.active_mut().state.pc &= !3;
            m.cores.active_mut().state.pstate &= !T_BIT;
        }
        return Ok(());
    }

    // Thumb-2 ADD/SUB (modified immediate) used for pointer setup.
    if first & 0xfb00 == 0xf100 || first & 0xfb00 == 0xf300 {
        let subtract = first & 0x00a0 == 0x00a0;
        let rn = u32::from(first & 0xf);
        let rd = u32::from((second >> 8) & 0xf);
        let left = reg_read(&m.cores.active().state, rn, pc, true);
        let imm = thumb_expand_imm(first, second);
        let result = if subtract {
            left.wrapping_sub(imm)
        } else {
            left.wrapping_add(imm)
        };
        reg_write(&mut m.cores.active_mut().state, rd, result);
        if first & 0x10 != 0 {
            if subtract {
                set_sub_flags(&mut m.cores.active_mut().state.pstate, left, imm, result);
            } else {
                set_add_flags(
                    &mut m.cores.active_mut().state.pstate,
                    left,
                    imm,
                    u64::from(left) + u64::from(imm),
                );
            }
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // MRRC/MCRR coprocessor pair transfers. The T8020 SEPROM reset sequence
    // reads the CP15 CR14 reset-signature pair via `mrrc p15, #0, ...`, and
    // could in principle rewrite it via `mcrr`. Field layout pinned by clang:
    //   `mrrc p15, #0, r8, r9, c14` = 0x8f0eec59 (bytes 59 ec 0e 8f)
    //   `mcrr p15, #0, r8, r9, c14` = 0x8f0eec49
    //   `mrrc p15, #5, r2, r3, c14` = 0x2f5eec53 (opc1 = bits[23:20] = 5)
    // with first16 lower nibble = the second syntax register (Rt2),
    // second16 bits[15:12] = the first syntax register (Rt), second16
    // bits[11:8] = coprocessor (1111 = p15), bits[7:4] = opc1, bits[3:0] =
    // CRm, and first16 bit4 = 1 (MRRC) / 0 (MCRR).
    if first & 0xffe0 == 0xec40 && second & 0x0f00 == 0x0f00 {
        let rt2 = u32::from(first & 0xf);
        let rt = u32::from((second >> 12) & 0xf);
        let opc1 = u32::from((second >> 4) & 0xf);
        let crm = u32::from(second & 0xf);
        let is_read = first & 0x0010 != 0;
        if opc1 == 0 && crm == 14 {
            if is_read {
                let value = m.cores.active_mut().state.a32.cr14;
                reg_write(&mut m.cores.active_mut().state, rt, value as u32);
                reg_write(&mut m.cores.active_mut().state, rt2, (value >> 32) as u32);
            } else {
                let low = reg_read(&m.cores.active().state, rt, pc, true);
                let high = reg_read(&m.cores.active().state, rt2, pc, true);
                m.cores.active_mut().state.a32.cr14 = (u64::from(high) << 32) | u64::from(low);
            }
        } else if is_read {
            // Unmodeled CP15 pair registers (e.g. the p14 counter) read zero
            // until a real model is added from evidence.
            reg_write(&mut m.cores.active_mut().state, rt, 0);
            reg_write(&mut m.cores.active_mut().state, rt2, 0);
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // MRC / MCR in Thumb-2 use the A32 coprocessor transfer encoding.
    if first & 0xff00 == 0xee00 && second & 0x0f00 == 0x0f00 {
        let op = (u32::from(first) << 16) | u32::from(second);
        execute_cp_transfer(m, op)?;
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // Thumb-2 data-processing immediate. SEPROM's early control setup uses
    // ORR.W to set the SCTLR auxiliary-control enable bit.
    if first & 0xfa00 == 0xf000 {
        let opcode = u32::from((first >> 5) & 0xf);
        let set_flags = first & 0x10 != 0;
        let rn = u32::from(first & 0xf);
        let rd = u32::from((second >> 8) & 0xf);
        let left = if rn == 15 && opcode == 3 {
            0 // ORN with PC as Rn is the architectural MVN alias.
        } else {
            reg_read(&m.cores.active().state, rn, pc, true)
        };
        let imm = thumb_expand_imm(first, second);
        let result = match opcode {
            0 => left & imm,
            1 => left & !imm,
            2 => left | imm,
            3 => left | !imm,
            4 => left ^ imm,
            _ => return Err(ExecError::Undefined),
        };
        reg_write(&mut m.cores.active_mut().state, rd, result);
        if set_flags {
            set_nz(&mut m.cores.active_mut().state.pstate, result);
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // Thumb-2 shifted-register logical operations, needed by the early
    // feature/auxiliary-control setup sequence.
    if first & 0xfe00 == 0xea00 {
        let opcode = u32::from((first >> 5) & 0xf);
        let rn = u32::from(first & 0xf);
        let rd = u32::from((second >> 8) & 0xf);
        let rm = u32::from(second & 0xf);
        let left = if rn == 15 && opcode == 3 {
            0 // ORN with PC as Rn is the architectural MVN alias.
        } else {
            reg_read(&m.cores.active().state, rn, pc, true)
        };
        let right = reg_read(&m.cores.active().state, rm, pc, true);
        let result = match opcode {
            0 => left & right,
            1 => left & !right,
            2 => left | right,
            3 => left | !right,
            4 => left ^ right,
            7 => !right,
            _ => return Err(ExecError::Undefined),
        };
        reg_write(&mut m.cores.active_mut().state, rd, result);
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // MOVW / MOVT (Thumb-2).
    if first & 0xfbf0 == 0xf240 || first & 0xfbf0 == 0xf2c0 {
        let imm16 = (u32::from(first & 0xf) << 12)
            | (u32::from((first >> 10) & 1) << 11)
            | (u32::from((second >> 12) & 7) << 8)
            | u32::from(second & 0xff);
        let rd = ((second >> 8) & 0xf) as u32;
        let old = reg_read(&m.cores.active().state, rd, pc, true);
        let value = if first & 0x0080 == 0 {
            // MOVW: Rd = ZeroExtend(imm16) — the upper half is cleared, not kept.
            imm16
        } else {
            (old & 0x0000_ffff) | (imm16 << 16)
        };
        reg_write(&mut m.cores.active_mut().state, rd, value);
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    // STRD/LDRD immediate with pre/post-indexing. The T8020 SEPROM reset
    // sequence uses post-index STRD to clear SECURE RAM windows.
    if first & 0xfe40 == 0xe840 {
        let p = first & 0x0100 != 0;
        let u = first & 0x0080 != 0;
        let w = first & 0x0020 != 0;
        let load_pair = first & 0x0010 != 0;
        let rn = u32::from(first & 0xf);
        let rt = u32::from((second >> 12) & 0xf);
        let rt2 = u32::from((second >> 8) & 0xf);
        let imm = u32::from(second & 0xff) * 4;
        let base = reg_read(&m.cores.active().state, rn, pc, true);
        let indexed = if u {
            base.wrapping_add(imm)
        } else {
            base.wrapping_sub(imm)
        };
        let address = if p { indexed } else { base };
        if load_pair {
            let first_value = load(m, u64::from(address), 4)?;
            let second_value = load(m, u64::from(address.wrapping_add(4)), 4)?;
            reg_write(&mut m.cores.active_mut().state, rt, first_value);
            reg_write(&mut m.cores.active_mut().state, rt2, second_value);
        } else {
            store(
                m,
                u64::from(address),
                4,
                reg_read(&m.cores.active().state, rt, pc, true),
            )?;
            store(
                m,
                u64::from(address.wrapping_add(4)),
                4,
                reg_read(&m.cores.active().state, rt2, pc, true),
            )?;
        }
        if w || !p {
            reg_write(&mut m.cores.active_mut().state, rn, indexed);
        }
        m.cores.active_mut().state.pc = pc + 4;
        return Ok(());
    }

    Err(ExecError::Undefined)
}

/// Execute one ARM or Thumb instruction in the SEP-only AArch32 mode.
pub fn step(m: &mut Machine) -> Result<(), ExecError> {
    if matches!(
        m.cores.active_mut().run_state,
        CpuRunState::WaitingForInterrupt
    ) {
        return Ok(());
    }
    let pc = m.cores.active_mut().state.pc;
    let is_thumb = thumb(&m.cores.active().state);
    m.current_instruction_pc = Some(pc);
    let result = if is_thumb {
        let first = fetch16(m, pc)?;
        let len = if matches!(first >> 11, 0b11101..=0b11111) {
            let second = fetch16(m, pc + 2)?;
            execute_thumb32(m, pc, first, second)
        } else {
            execute_thumb16(m, pc, first)
        };
        len.map(|()| u32::from(first))
    } else {
        let op = m.mem_read(pc, 4)? as u32;
        execute_arm(m, pc, op).map(|()| op)
    };
    m.current_instruction_pc = None;
    let op = result?;
    m.cores.active_mut().instructions = m.cores.active_mut().instructions.wrapping_add(1);
    if m.trace.is_enabled() {
        m.trace.record(
            pc,
            op as u64,
            if is_thumb { 2 } else { 4 },
            TraceKind::Instruction,
            pc,
            m.cores.active_mut().state.el as u8,
            m.cores.active_mut().state.pstate,
            None,
        );
    }
    Ok(())
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
                size: 0x1000,
                device: Box::new(SparseRam::new(0, 0x1000)),
            })
            .unwrap();
        machine
    }

    #[test]
    fn arm_branch_and_thumb_flag_updates_execute() {
        let mut machine = machine();
        machine.mem_write(0, 4, 0xea00_0000).unwrap(); // B +0
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 8);

        machine.cores.active_mut().state.pc = 0x100;
        machine.cores.active_mut().state.pstate |= T_BIT;
        machine.mem_write(0x100, 2, 0x2001).unwrap(); // MOVS r0, #1
        machine.mem_write(0x102, 2, 0x3801).unwrap(); // SUBS r0, #1
        step(&mut machine).unwrap();
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.x[0], 0);
        assert_ne!(machine.cores.active_mut().state.pstate & Z_BIT, 0);
        assert_eq!(machine.cores.active_mut().state.pc, 0x104);
    }

    #[test]
    fn thumb_barrier_and_strd_post_index_advance() {
        let mut machine = machine();
        machine.cores.active_mut().state.pstate |= T_BIT;
        machine.cores.active_mut().state.x[0] = 0x400;
        machine.cores.active_mut().state.x[2] = 0x1234_5678;
        machine.cores.active_mut().state.x[3] = 0x9abc_def0;
        machine.mem_write(0, 4, 0x2302_e8e0).unwrap(); // STRD r2, r3, [r0], #8
        machine.mem_write(4, 4, 0x8f4f_f3bf).unwrap(); // DSB
        step(&mut machine).unwrap();
        assert_eq!(machine.mem_read(0x400, 4).unwrap(), 0x1234_5678);
        assert_eq!(machine.mem_read(0x404, 4).unwrap(), 0x9abc_def0);
        assert_eq!(machine.cores.active_mut().state.x[0], 0x408);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 8);
    }

    #[test]
    fn thumb_blx_reaches_arm_wfi_fallback_instead_of_falling_through() {
        let mut machine = machine();
        machine.cores.active_mut().state.pc = 0x388;
        machine.cores.active_mut().state.pstate |= T_BIT;
        machine.mem_write(0x388, 4, 0xee70_f7ff).unwrap(); // BLX 0x6c
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x6c);
        assert_eq!(machine.cores.active_mut().state.x[14], 0x38d);
        assert_eq!(machine.cores.active_mut().state.pstate & T_BIT, 0);

        machine.mem_write(0x6c, 4, 0xe320_f003).unwrap(); // WFI
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x70);
        assert!(matches!(
            machine.cores.active_mut().run_state,
            CpuRunState::WaitingForInterrupt
        ));
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x70);
    }

    #[test]
    fn movw_clears_upper_halfword_and_movt_preserves_lower() {
        let mut machine = machine();
        machine.cores.active_mut().state.pstate |= T_BIT;
        machine.cores.active_mut().state.pc = 0x100;
        // Dirty r0 high half first, then MOVW r0, #0x6c1c: the write must clear it.
        machine.cores.active_mut().state.x[0] = 0x1000_0001;
        machine.mem_write(0x100, 4, 0x401c_f646).unwrap(); // MOVW r0, #0x6c1c
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.x[0], 0x6c1c);

        // MOVT r0, #0xbd9e must preserve the low halfword.
        machine.cores.active_mut().state.pc = 0x100;
        machine.mem_write(0x100, 4, 0x509e_f6cb).unwrap(); // MOVT r0, #0xbd9e
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.x[0], 0xbd9e_6c1c);
    }

    #[test]
    fn cp15_cr14_mrrc_mcrr_round_trip_and_seprom_encoding() {
        let mut machine = machine();
        machine.cores.active_mut().state.pstate |= T_BIT;

        // MRRC read: clang-exact `mrrc p15, #0, r8, r9, c14` = 0x8f0eec59.
        machine.cores.active_mut().state.a32.cr14 = 0x1122_3344_5566_7788;
        machine.cores.active_mut().state.pc = 0x100;
        machine.mem_write(0x100, 4, 0x8f0e_ec59).unwrap();
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.x[8], 0x5566_7788);
        assert_eq!(machine.cores.active_mut().state.x[9], 0x1122_3344);

        // MCRR write: clang-exact `mcrr p15, #0, r8, r9, c14` = 0x8f0eec49.
        machine.cores.active_mut().state.x[8] = 0x5566_7788;
        machine.cores.active_mut().state.x[9] = 0x1122_3344;
        machine.cores.active_mut().state.pc = 0x104;
        machine.mem_write(0x104, 4, 0x8f0e_ec49).unwrap();
        step(&mut machine).unwrap();
        assert_eq!(
            machine.cores.active_mut().state.a32.cr14,
            0x1122_3344_5566_7788
        );

        // opc1 is decoded from bits[23:20]: `mrrc p15, #5, r2, r3, c14` is
        // not CR14, so the unmodeled pair reads zero (clang: 0x2f5eec53).
        machine.cores.active_mut().state.pc = 0x108;
        machine.mem_write(0x108, 4, 0x2f5e_ec53).unwrap();
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.x[2], 0);
        assert_eq!(machine.cores.active_mut().state.x[3], 0);
    }

    #[test]
    fn t8020_seprom_irq_and_fiq_vectors_enter_wfi_fallback() {
        let mut machine = machine();
        machine.mem_write(0x18, 4, 0xea00_0010).unwrap(); // IRQ: B 0x60
        machine.mem_write(0x1c, 4, 0xea00_0010).unwrap(); // FIQ: B 0x64
        machine.mem_write(0x60, 4, 0xea00_0001).unwrap(); // B 0x6c
        machine.mem_write(0x64, 4, 0xe3a0_d01c).unwrap(); // MOV sp, #0x1c
        machine.mem_write(0x68, 4, 0xeaff_ffff).unwrap(); // B 0x6c
        machine.mem_write(0x6c, 4, 0xe320_f003).unwrap(); // WFI

        machine.cores.active_mut().state.pc = 0x18;
        machine.cores.active_mut().state.pstate &= !T_BIT;
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x60);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x6c);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x70);
        assert!(matches!(
            machine.cores.active_mut().run_state,
            CpuRunState::WaitingForInterrupt
        ));

        machine.cores.active_mut().run_state = CpuRunState::Running;
        machine.cores.active_mut().state.pc = 0x1c;
        machine.cores.active_mut().state.sp = 0x19c2_0000;
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x64);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.sp, 0x1c);
        assert_eq!(machine.cores.active_mut().state.pc, 0x68);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x6c);
        step(&mut machine).unwrap();
        assert_eq!(machine.cores.active_mut().state.pc, 0x70);
        assert!(matches!(
            machine.cores.active_mut().run_state,
            CpuRunState::WaitingForInterrupt
        ));
    }
}
