//! AArch64 architectural interpreter.
//!
//! Decode order is fixed and architectural: per-class masks overlap between
//! classes so ordering defines precedence. Masks/values derive from the
//! clang-assembled battery (`enc4_words.json`) and are verified by
//! `crates/iphem-arm/tests/basic.rs`.
use iphem_core::{CpuRunState, ExceptionLevel, Machine, TraceKind};

mod aarch32;
mod aarch32_core;
pub mod symbolic;

/// Fixed SVE vector length this core models: 128 bits = 16 bytes. LD1/ST1
/// element-scaled immediates (`#imm, MUL VL`) multiply this constant.
const SVE_VL_BYTES: i64 = 16;

/// Execute one AArch32 ARM/Thumb instruction for SEPROM-only boot tracing.
pub use aarch32::step as step_aarch32;

/// Enter the ARMv7 core with a reset SVC CPSR and programme counter.
pub use aarch32_core::enter as enter_aarch32_core;
/// Park the ARMv7 core while a pending IRQ is masked by `CPSR.I`.
pub use aarch32_core::signal_irq as aarch32_core_signal_irq;
/// Execute one A32 (ARM-state) instruction of the complete ARMv7 core.
pub use aarch32_core::step as step_aarch32_core;

#[derive(Debug, Clone, Copy)]
pub struct Instruction(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecError {
    Fetch,
    Data,
    Undefined,
    Synchronous,
}

impl From<iphem_core::BusError> for ExecError {
    fn from(_: iphem_core::BusError) -> Self {
        ExecError::Data
    }
}

fn full(sf: u32) -> u64 {
    if sf == 1 {
        u64::MAX
    } else {
        u32::MAX as u64
    }
}

/// Phase 190 SVE predicate-logical byte operation. `func` is decoded from
/// the instruction's class bit (bit23), `o2` (bit9) and `o3` (bit4):
/// class 0 (&) = AND/BIC/EOR, class 1 (|) = ORR/ORN/NOR/NAND.
fn sve_pred_bool(func: u32, a: u8, b: u8) -> u8 {
    match func {
        0 => a & b,
        1 => a & !b,
        2 => a ^ b,
        4 => a | b,
        5 => a | !b,
        6 => !(a | b),
        7 => !(a & b),
        _ => unreachable!("reserved predicate-logical encoding {func}"),
    }
}

fn sign_extend(v: u64, bits: u32) -> u64 {
    let s = 64 - bits;
    ((v << s) as i64 >> s) as u64
}

fn reg(s: &iphem_core::CpuState, n: u32) -> u64 {
    if n == 31 {
        0
    } else {
        s.x[n as usize]
    }
}

fn reg_sp(s: &iphem_core::CpuState, n: u32) -> u64 {
    if n == 31 {
        s.sp
    } else {
        s.x[n as usize]
    }
}

fn wr(s: &mut iphem_core::CpuState, n: u32, v: u64) {
    if n != 31 {
        s.x[n as usize] = v
    }
}

fn wr_sp(s: &mut iphem_core::CpuState, n: u32, v: u64) {
    if n == 31 {
        s.sp = v
    } else {
        s.x[n as usize] = v
    }
}

fn trace(m: &mut Machine, pc: u64, op: u32) {
    m.cores.active_mut().instructions += 1;
    if m.trace.is_enabled() {
        m.trace.record(
            pc,
            op as u64,
            4,
            TraceKind::Instruction,
            pc,
            m.cores.active_mut().state.el as u8,
            m.cores.active_mut().state.pstate,
            None,
        );
    }
}

fn saved_pstate(s: &iphem_core::CpuState) -> u64 {
    iphem_core::saved_pstate(s)
}

fn bank_active_sp(s: &mut iphem_core::CpuState) {
    let bank = if s.spsel { s.el as usize } else { 0 };
    s.sp_el[bank] = s.sp;
}

fn select_sp(s: &mut iphem_core::CpuState, el: ExceptionLevel, spsel: bool) {
    s.el = el;
    s.currentel = el as u8;
    s.spsel = el != ExceptionLevel::El0 && spsel;
    let bank = if s.spsel { el as usize } else { 0 };
    if s.sp_el[bank] == 0 {
        s.sp_el[bank] = 0x19c20000;
    }
    s.sp = s.sp_el[bank];
    s.pstate = (s.pstate & !0xf) | ((el as u64) << 2) | u64::from(s.spsel);
}

fn exception_to(m: &mut Machine, target_el: ExceptionLevel, esr: u64, far: u64) -> ExecError {
    let return_address = m.cores.active_mut().state.pc;
    exception_to_at(m, target_el, esr, far, return_address)
}

/// Map a register's minimum-accessible exception level to the trap target.
/// An access from a lower EL is taken as an UNDEFINED-instruction exception
/// to the EL the register is defined for.
fn sysreg_trap_el(min: u8) -> ExceptionLevel {
    match min {
        1 => ExceptionLevel::El1,
        2 => ExceptionLevel::El2,
        _ => ExceptionLevel::El3,
    }
}

fn exception_to_at(
    m: &mut Machine,
    target_el: ExceptionLevel,
    esr: u64,
    far: u64,
    return_address: u64,
) -> ExecError {
    iphem_core::take_sync(
        m.cores.active_mut(),
        &mut m.trace,
        target_el,
        esr,
        far,
        return_address,
    );
    ExecError::Synchronous
}

fn atomic_alignment_exception(m: &mut Machine, address: u64, write: bool, pc: u64) -> ExecError {
    let source = m.cores.active_mut().state.el;
    let target = if source == ExceptionLevel::El0 {
        ExceptionLevel::El1
    } else {
        source
    };
    let ec = if source == target { 0x25 } else { 0x24 };
    let esr = (ec << 26) | (1 << 25) | (u64::from(write) << 6) | 0x21;
    exception_to_at(m, target, esr, address, pc)
}

fn check_sp_alignment(m: &mut Machine, rn: u32, pc: u64) -> Result<(), ExecError> {
    if rn != 31 {
        return Ok(());
    }
    let source = m.cores.active_mut().state.el;
    let enforced = match source {
        ExceptionLevel::El0 => m.cores.active_mut().state.sctlr_el1 & (1 << 4) != 0, // SA0
        ExceptionLevel::El1 => m.cores.active_mut().state.sctlr_el1 & (1 << 3) != 0, // SA
        ExceptionLevel::El2 => m.cores.active_mut().state.sctlr_el2 & (1 << 3) != 0,
        ExceptionLevel::El3 => m.cores.active_mut().state.sctlr_el3 & (1 << 3) != 0,
    };
    if !enforced || m.cores.active_mut().state.sp & 0xf == 0 {
        return Ok(());
    }
    let target = if source == ExceptionLevel::El0 {
        ExceptionLevel::El1
    } else {
        source
    };
    Err(exception_to_at(m, target, 0x26u64 << 26, 0, pc))
}

fn exception(m: &mut Machine, esr: u64, far: u64) -> ExecError {
    let target = match m.cores.active_mut().state.el {
        ExceptionLevel::El0 => ExceptionLevel::El1,
        el => el,
    };
    exception_to(m, target, esr, far)
}

fn branch_to(m: &mut Machine, fault_pc: u64, target: u64) -> Result<(), ExecError> {
    if target & 3 != 0 {
        // PC alignment faults report the branch instruction as the preferred
        // return address and the invalid target in FAR_EL1.
        m.cores.active_mut().state.pc = fault_pc;
        return Err(exception(m, 0x22u64 << 26, target));
    }
    m.cores.active_mut().state.pc = target;
    Ok(())
}

fn set_flags(s: &mut iphem_core::CpuState, nzcv: u32) {
    s.pstate = (s.pstate & !0xf0000000) | ((nzcv as u64) << 28);
    s.nzcv = nzcv;
}

fn cond_check(pstate: u64, cond: u32) -> bool {
    let nzcv = (pstate >> 28) & 0xf;
    let (n, z, c, v) = (nzcv & 8 != 0, nzcv & 4 != 0, nzcv & 2 != 0, nzcv & 1 != 0);
    match cond & 0xf {
        0x0 => z,
        0x1 => !z,
        0x2 => c,
        0x3 => !c,
        0x4 => n,
        0x5 => !n,
        0x6 => v,
        0x7 => !v,
        0x8 => c && !z,
        0x9 => !c || z,
        0xa => n == v,
        0xb => n != v,
        0xc => !z && n == v,
        0xd => z || n != v,
        0xe | 0xf => true,
        _ => false,
    }
}

/// Add with carry over `width` bits (32 or 64). Returns (result, nzcv).
fn add_with_carry(a: u64, b: u64, cin: u64, width: u32) -> (u64, u32) {
    let mask = if width == 32 {
        u32::MAX as u64
    } else {
        u64::MAX
    };
    let am = a & mask;
    let bm = b & mask;
    let sum = am as u128 + bm as u128 + (cin & 1) as u128;
    let result = (sum & mask as u128) as u64;
    let n = ((result >> (width - 1)) & 1) as u32;
    let z = (result == 0) as u32;
    let c = (sum > mask as u128) as u32;
    let sign = 1u64 << (width - 1);
    let v = ((!(am ^ bm) & (am ^ result) & sign) != 0) as u32;
    (result, (n << 3) | (z << 2) | (c << 1) | v)
}

fn ror(v: u64, amt: u32, width: u32) -> u64 {
    // DecodeBitMasks also rotates 2/4/8/16-bit elements before replication.
    // A native u64 rotate loses the wrapped bits when the element is masked.
    let mask = full_elem(width);
    let value = v & mask;
    let shift = amt % width;
    if shift == 0 {
        value
    } else {
        ((value >> shift) | (value << (width - shift))) & mask
    }
}

/// Structural shift for register-source shifted operands (LSL/LSR/ASR/ROR).
fn apply_shift(b: u64, ty: u32, amt: u32, width: u32) -> u64 {
    let full = if width == 32 {
        u32::MAX as u64
    } else {
        u64::MAX
    };
    let sh = amt % width;
    let b = b & full;
    match ty {
        0 => b.wrapping_shl(sh) & full,
        1 => b.wrapping_shr(sh) & full,
        2 => {
            let signed = if width == 32 {
                b as u32 as i32 as i64
            } else {
                b as i64
            };
            (signed >> sh) as u64 & full
        }
        3 => ror(b, sh, width),
        _ => b & full,
    }
}

fn full_elem(esize: u32) -> u64 {
    if esize == 64 {
        u64::MAX
    } else {
        (1u64 << esize) - 1
    }
}

/// AArch64 BitMaskImmediate.
fn decode_bitmask(n: u32, immr: u32, imms: u32, is64: bool) -> Option<u64> {
    decode_bit_masks(n, immr, imms, is64, true).map(|(wmask, _)| wmask)
}

fn decode_bit_masks(
    n: u32,
    immr: u32,
    imms: u32,
    is64: bool,
    immediate: bool,
) -> Option<(u64, u64)> {
    if !is64 && n != 0 {
        return None;
    }
    let selector = (n << 6) | ((!imms) & 0x3f);
    if selector == 0 {
        return None;
    }
    let len = 31 - selector.leading_zeros();
    if len < 1 || (!is64 && len > 5) {
        return None;
    }
    let levels = (1u32 << len) - 1;
    let s = imms & levels;
    let r = immr & levels;
    if immediate && s == levels {
        return None;
    }
    let diff = s.wrapping_sub(r) & levels;
    let esize = 1u32 << len;
    let welem = ror(full_elem(s + 1), r, esize) & full_elem(esize);
    let telem = full_elem(diff + 1);
    let total = if is64 { 64 } else { 32 };
    let mut wmask = 0u64;
    let mut tmask = 0u64;
    for shift in (0..total).step_by(esize as usize) {
        wmask |= welem << shift;
        tmask |= telem << shift;
    }
    let regmask = full(is64 as u32);
    Some((wmask & regmask, tmask & regmask))
}

// ---------------------------------------------------------------------------
// Decoded tree.
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Op {
    Eret,
    Drps,
    Brk(u16),
    Hlt(u16),
    Svc(u16),
    Hvc(u16),
    Smc(u16),
    Sys(u32),
    Branch {
        link: bool,
        imm: i64,
    },
    Bcond {
        cond: u32,
        imm: i64,
    },
    Br(u32),
    Blr(u32),
    Ret(u32),
    /// Pointer-authenticated return (RETAA/RETAB). Authenticates the link
    /// register with the A/B instruction key and a zero modifier, then returns.
    RetPac {
        key: u8,
    },
    /// Pointer-authenticated ERET (ERETAA/ERETAB). Authenticates ELR_ELx with
    /// the A/B instruction key and a zero modifier, then ERETs.
    EretPac {
        key: u8,
    },
    /// Hint-space pointer authentication (PACIA1716...AUTIBSP). Targets X16/X17
    /// or X30/SP depending on the SP selector.
    PacHint {
        key: u8,
        sign: bool,
        sp: bool,
    },
    /// Authenticated branch (BRAA/BRAB/BLRAA/BLRAB).
    PacBranch {
        key: u8,
        link: bool,
        modifier: u32,
        target: u32,
    },
    Nop,
    Cbz {
        is_cbnz: bool,
        sf: u32,
        rt: u32,
        imm: i64,
    },
    Tbz {
        is_tbnz: bool,
        bit: u32,
        rt: u32,
        imm: i64,
    },
    Adr {
        rd: u32,
        imm: i64,
    },
    Adrp {
        rd: u32,
        imm: i64,
    },
    AddSubImm {
        sf: u32,
        sub: bool,
        set: bool,
        shl: bool,
        imm: u64,
        rn: u32,
        rd: u32,
    },
    MovImm {
        sf: u32,
        kind: u32,
        hw: u32,
        imm: u64,
        rd: u32,
    },
    LogImm {
        sf: u32,
        opc: u32,
        n: u32,
        immr: u32,
        imms: u32,
        rn: u32,
        rd: u32,
    },
    Extract {
        sf: u32,
        lsb: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    Bitfield {
        sf: u32,
        opc: u32,
        immr: u32,
        imms: u32,
        rn: u32,
        rd: u32,
    },
    AddSubShift {
        sf: u32,
        sub: bool,
        set: bool,
        shift: u32,
        amount: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    AddSubExt {
        sf: u32,
        sub: bool,
        set: bool,
        option: u32,
        amount: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    LogShift {
        sf: u32,
        opc: u32,
        n: u32,
        shift: u32,
        amount: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    Dp1 {
        sf: u32,
        sub: u32,
        rn: u32,
        rd: u32,
    },
    /// Pointer Authentication data-processing form. The instruction signs or
    /// authenticates Xd using Xn as modifier. Until architectural PAC keys are
    /// modeled, execution preserves Xd while traces retain the real mnemonic.
    PacData {
        sub: u32,
        rn: u32,
        rd: u32,
    },
    /// Generic authentication code: Xn data, Xm/SP modifier, APGA key.
    Pacga {
        rn: u32,
        rm: u32,
        rd: u32,
    },
    /// Remove a pointer authentication code using the translation regime.
    Xpac {
        data: bool,
        rd: u32,
    },
    Dp2 {
        sf: u32,
        sub: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    Dp3 {
        sf: u32,
        sub: u32,
        rm: u32,
        ra: u32,
        rn: u32,
        rd: u32,
    },
    Carry {
        sf: u32,
        sub: bool,
        set: bool,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    CCmp {
        sf: u32,
        sub: bool,
        is_imm: bool,
        cond: u32,
        nzcv: u32,
        rm: u32,
        rn: u32,
    },
    CondSel {
        sf: u32,
        sel_op: u32,
        rm: u32,
        cond: u32,
        rn: u32,
        rd: u32,
    },
    LdrLit {
        vector: bool,
        size: u32,
        sw: bool,
        prefetch: bool,
        rt: u32,
        imm: i64,
    },
    Excl {
        sf: u32,
        load: bool,
        size: u32,
        rs: u32,
        rn: u32,
        rt: u32,
    },
    Ordered {
        load: bool,
        size: u32,
        rn: u32,
        rt: u32,
    },
    Lse {
        sf: u32,
        sub: u32,
        size: u32,
        acquire: bool,
        release: bool,
        rm: u32,
        rn: u32,
        rt: u32,
    },
    /// LSE compare-and-swap pair (CASP{A,L}{B,H}), with adjacent even
    /// compare/result and replacement register pairs.
    LsePair {
        size: u32,
        acquire: bool,
        release: bool,
        rs: u32,
        rn: u32,
        rt: u32,
    },
    LdrAuth {
        key: u8,
        writeback: bool,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    LdrStrU {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        off: u64,
    },
    LdrStrPre {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    LdrStrPost {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    LdrStrReg {
        size: u32,
        opc: u32,
        option: u32,
        amount: u32,
        rn: u32,
        rm: u32,
        rt: u32,
    },
    LdStPair {
        size: u32,
        load: bool,
        signed: bool,
        pre: bool,
        post: bool,
        imm: i64,
        rn: u32,
        rt: u32,
        rt2: u32,
    },
    /// SIMD&FP loaded/stored to V registers (Q transfers split into two
    /// 8-byte accesses; opc bit23 selects the 128-bit variant).
    SimdLdrStrU {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        off: u64,
    },
    SimdLdrStrPre {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    SimdLdrStrPost {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    SimdLdurStur {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    SimdLdrStrReg {
        size: u32,
        opc: u32,
        option: u32,
        amount: u32,
        rn: u32,
        rm: u32,
        rt: u32,
    },
    /// SIMD&FP load/store pair (S/D/Q element sizes from opc, imm7 scaled).
    SimdLdStPair {
        size: u32,
        load: bool,
        pre: bool,
        post: bool,
        imm: i64,
        rn: u32,
        rt: u32,
        rt2: u32,
    },
    /// One-register AdvSIMD structure transfer (LD1/ST1), including optional
    /// post-index by an immediate vector size or general-purpose register.
    SimdStructSingle {
        q: bool,
        load: bool,
        post: bool,
        size: u32,
        rn: u32,
        rt: u32,
        rm: u32,
    },
    SimdStructPair {
        q: bool,
        load: bool,
        post: bool,
        size: u32,
        rn: u32,
        rt: u32,
        rm: u32,
    },
    /// Two/three/four-register AdvSIMD structure transfer (LD2-4/ST2-4), whole
    /// vectors or the top half when Q is clear.
    SimdStructMulti {
        n: u32,
        q: bool,
        load: bool,
        post: bool,
        size: u32,
        rn: u32,
        rt: u32,
        rm: u32,
    },
    /// Load single element and replicate across every lane of one or more
    /// destination vector registers (LD1R..LD4R). One `1 << size`-byte element
    /// is read and reproduced in each lane. The register count is encoded in
    /// bits[15:12] (`1100`=1/2, `1110`=3/4) with bit21 selecting the list
    /// length (verified against clang: ld1r 0x0d40c908, ld2r 0x0d60c908,
    /// ld3r 0x0d40e908, ld4r 0x0d60e908). Load-only: the STnR forms are
    /// unallocated.
    SimdStructReplicate {
        n: u32,
        q: bool,
        post: bool,
        size: u32,
        rn: u32,
        rt: u32,
        rm: u32,
    },
    FpThreeSrc {
        sub: bool,
        op31: u32,
        rm: u32,
        ra: u32,
        rn: u32,
        rd: u32,
    },
    FpTwoSrc {
        opc: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    FpCmpReg {
        signal_all_nans: bool,
        rn: u32,
        rm: u32,
    },
    FpCmpZero {
        signal_all_nans: bool,
        rn: u32,
    },
    FpCCmp {
        sub: bool,
        cond: u32,
        nzcv: u32,
        rm: u32,
        rn: u32,
    },
    FpSel {
        rm: u32,
        cond: u32,
        rn: u32,
        rd: u32,
    },
    FpOneSrc {
        opcode: u32,
        rn: u32,
        rd: u32,
    },
    FpImm {
        imm8: u32,
        typ: u32,
        rd: u32,
    },
    FpGprConv {
        sf: u32,
        typ: u32,
        opc: u32,
        rn: u32,
        rd: u32,
    },
    FpFixed {
        sf: u32,
        typ: u32,
        u: bool,
        to_fp: bool,
        imm6: u32,
        rn: u32,
        rd: u32,
    },
    // AdvSIMD scalar float<->int conversion with a vector-register destination
    // (FCVTZS/FCVTZU/SCVTF/UCVTF <Vd>.<T>, <Vn>.<T>). Byte3 = 0x5E (signed)
    // or 0x7E (unsigned); bit21/bit16 set, bits18:17 clear; byte1 bits15:10 =
    // 0b101110 for FP->int with bit23=1, 0b110110 for int->FP with bit23=0.
    // Precision (clang-verified): FP32=(b22,b20,b19)=000, FP64=100, FP16=111.
    FpRegConv {
        unsigned: bool,
        to_fp: bool,
        prec: u32,
        rn: u32,
        rd: u32,
    },
    NeonUminv {
        q: bool,
        size: u32,
        rn: u32,
        rd: u32,
    },
    NeonUmaxv {
        q: bool,
        size: u32,
        rn: u32,
        rd: u32,
    },
    NeonAddLv {
        q: bool,
        size: u32,
        signed: bool,
        rn: u32,
        rd: u32,
    },
    NeonRev64 {
        q: bool,
        size: u32,
        rn: u32,
        rd: u32,
    },
    NeonShiftImmediate {
        q: bool,
        size: u32,
        right: bool,
        amount: u32,
        rn: u32,
        rd: u32,
    },
    NeonShiftLong {
        q: bool,
        unsigned: bool,
        src_size: u32,
        amount: u32,
        rn: u32,
        rd: u32,
    },
    // AdvSIMD shift left/right by register: USHL/SSHL (unsigned/signed
    // shift). Each lane of Vn is shifted by the signed count held in the
    // matching lane of Vm. Element widths 32/64/16 map to (b23,b22) =
    // (1,0)/(1,1)/(0,1) with bit21 set; bit28 distinguishes the scalar
    // D-register form (single 64-bit lane); Q (bit30) widens vectors.
    NeonShiftReg {
        q: bool,
        unsigned: bool,
        size: u32,
        scalar: bool,
        rn: u32,
        rm: u32,
        rd: u32,
    },
    // AdvSIMD element extract to a general-purpose register (UMOV). The
    // 32-bit form writes a zero-extended W register; the 64-bit form an X
    // register. bit18:17 = 01 selects 32-bit, 10 selects 64-bit lanes; the
    // lane index lives in bits20:19 (32-bit, 4 lanes) or bit19 (64-bit, 2).
    NeonUmov {
        size: u32,
        index: u32,
        rn: u32,
        rd: u32,
    },
    /// Vector table lookup TBL/TBX. `n` table registers starting at `vn` are
    /// concatenated (16 bytes each when Q, else 8) and each byte lane of `vm`
    /// indexes into the concatenated byte stream; out-of-range lanes read 0
    /// (TBL) or keep their original value (TBX). TBX is len&1 != 0.
    NeonTblTbx {
        q: bool,
        tbx: bool,
        n: u32,
        vn: u32,
        vm: u32,
        vd: u32,
    },
    NeonExt {
        q: bool,
        offset: u32,
        rn: u32,
        rm: u32,
        rd: u32,
    },
    NeonMovi64 {
        q: bool,
        imm: u8,
        rd: u32,
    },
    NeonMovi {
        q: bool,
        bits: u32,
        value: u64,
        rd: u32,
    },
    NeonCopy {
        q: bool,
        op: u32,
        size: u32,
        index: u32,
        elem: bool,
        rn: u32,
        rd: u32,
    },
    NeonDupGeneral {
        q: bool,
        size: u32,
        rn: u32,
        rd: u32,
    },
    NeonThreeSame {
        q: bool,
        op: NeonThreeSameOp,
        size: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    NeonBitwise {
        q: bool,
        op: NeonBitwiseOp,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    NeonOneSrc {
        q: bool,
        op: NeonOneSrcOp,
        size: u32,
        rn: u32,
        rd: u32,
    },
    NeonFpOneSrc {
        q: bool,
        fneg: bool,
        sqrt: bool,
        size: u32,
        rn: u32,
        rd: u32,
    },
    /// ARMv8 Advanced SIMD AES round and mix-column operations.
    Aes {
        op: u32,
        rn: u32,
        rd: u32,
    },
    /// ARMv8 SHA-256 hash-round and message-schedule operations.
    Sha256 {
        op: u32,
        rn: u32,
        rm: u32,
        rd: u32,
    },
    // ── Phase 190 SVE (Armv8.2 SVE, VL=128) ────────────────────────────────
    // Scalar predicates WHILELO/WHILELS/WHILELT/WHILELE. Encodings verified
    // against llvm-mc: `whilelo p0.d, x0, x1` = 0x25e11c00.
    SveWhile {
        size: u32,
        sf: bool,
        unsigned: bool,
        le: bool,
        rn: u32,
        rm: u32,
        pd: u32,
    },
    // CNTB/CNTH/CNTW/CNTD. `cntb x3` = 0x0420e3e3; `<Xd>, {pattern}{, mul n}`.
    SveCnt {
        size: u32,
        pattern: u32,
        imm: u32,
        rd: u32,
    },
    // INCP/DECP, scalar and vector. `incp x0, p0.d` = 0x25ec8800,
    // `incp z4.d, p4.d` = 0x25ec8084, `decp x1, p1.s` = 0x25ad8821.
    SveIncp {
        dec: bool,
        scalar: bool,
        size: u32,
        pg: u32,
        rd: u32,
    },
    // Predicate logicals AND/ANDS/ORR/ORRS/EOR/EORS/BIC/BICS/ORN/ORN/NOR/NORS/
    // NAND (byte-granular only, as the architecture dictates for predicate
    // logic). `and p5.b, p6/z, p7.b, p8.b` = 0x250858e5.
    SvePredLogical {
        func: u32,
        set: bool,
        pd: u32,
        pg: u32,
        pn: u32,
        pm: u32,
    },
    // Contiguous first-faulting load LD1B/H/W/D. `ld1d {z0.d}, p0/z, [x2]`
    // = 0xa5e0a040. Zt is the low 128 bits of the Z register.
    SveLd1 {
        size: u32,
        imm: i64,
        pg: u32,
        rn: u32,
        zt: u32,
    },
    // Contiguous store ST1B/H/W/D. `st1d {z0.d}, p0, [x2]` = 0xe5e0e040.
    SveSt1 {
        size: u32,
        imm: i64,
        pg: u32,
        rn: u32,
        zt: u32,
    },
    Ldur {
        size: u32,
        opc: u32,
        rn: u32,
        rt: u32,
        imm: i64,
    },
    Crc32 {
        sz: u32,
        c: bool,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    Fjcvtzs {
        rn: u32,
        rd: u32,
    },
    /// LDAPR/LDAPRB/LDAPRH/LDAPUR/LDAPURB/LDAPURH/LDAPURSB/LDAPURSH/LDAPURSW
    /// (ARMv8.3 RCpc load acquires). Modelled as load-acquire (acquire semantics are no-ops here).
    Ldapr {
        size: u32, // 0=byte, 1=half, 2=word, 3=dword
        sign_extend: bool,
        rn: u32,
        rt: u32,
        /// Signed byte offset (0 for non-unscaled).
        imm: i64,
    },
    /// SDOT/UDOT (ARMv8.2 dot product, NEON).
    Dot {
        /// false=SDOT, true=UDOT.
        unsigned: bool,
        /// false=8B/2S arrangement, true=16B/4S.
        q: bool,
        /// Element-by-element (Some(l)) or vector (None).
        elem: Option<u32>,
        rm: u32,
        rn: u32,
        rd: u32,
    },
    /// FCADD / FCMLA (ARMv8.3 complex arithmetic, NEON).
    FcComplexArith {
        /// false=FCADD, true=FCMLA.
        is_cmla: bool,
        /// false=2S/4S (f32), true=2D/4D — but for now only 2S/4S used.
        sz: bool,
        /// false=64-bit (2S), true=128-bit (4S).
        q: bool,
        /// Rotation: 0=0°, 1=90°, 2=180°, 3=270°.
        rot: u32,
        rm: u32,
        rn: u32,
        rd: u32,
    },
}

// ========================================================================
// NEON operation enums.
// ========================================================================

/// NEON three-same (integer/FP) operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeonThreeSameOp {
    Cmhi,
    Cmeq,
    Add,
    Sub,
    Mla,
    Mls,
    Mul,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FMax,
    FMin,
    FMaxNmp,
    FMinNmp,
    FMla,
    FMls,
}

impl NeonThreeSameOp {
    pub fn fp_decode(u: bool, size: u32, opc: u32) -> Option<Self> {
        // AdvSIMD three-same floating point (verified against clang). Lane
        // width = size bit0 (bit22), handled by the executor as `size & 1`.
        // For the U=0 family, size bit1 (bit23) selects the paired flavor
        // (ADD/SUB, MAX/MIN, MAXNM/MINNM, MLA/MLS).
        let vb = (size >> 1) & 1;
        Some(match (u, opc, vb) {
            (false, 0b11010, 0) => Self::FAdd,
            (false, 0b11010, 1) => Self::FSub,
            (true, 0b11011, _) => Self::FMul,
            (true, 0b11111, _) => Self::FDiv,
            (false, 0b11110, 0) => Self::FMax,
            (false, 0b11110, 1) => Self::FMin,
            (false, 0b11000, 0) => Self::FMaxNmp,
            (false, 0b11000, 1) => Self::FMinNmp,
            (false, 0b11001, 0) => Self::FMla,
            (false, 0b11001, 1) => Self::FMls,
            _ => return None,
        })
    }
}

/// NEON bitwise operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeonBitwiseOp {
    And,
    Bic,
    Eor,
    Orr,
    Orn,
    Bsl,
    Bit,
    Bif,
}

/// NEON integer one-source operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeonOneSrcOp {
    Neg,
    Abs,
    Not,
    /// Bytes are set to the number of set bits (CNT), byte lanes are reversed
    /// within each element (REV16) or each 32-bit half (REV32), or the bit
    /// order of each byte is reversed (RBIT). Grouped because they share the
    /// one-source prefix and are selected by the six-bit op6 field. REV64 has
    /// its own `NeonRev64` op.
    ByteCount,
    Rev16,
    Rev32,
    Rbit,
}

// ========================================================================
// Scalar floating-point data-processing. All masks/values verified against
// clang on arm64 (see test vectors). bit22 is the S/D type selector and is
// masked out so both precisions collapse onto one variant; the executor
// uses bit22 to pick single vs double.
// -----------------------------------------------------------------------
// Three-source FMADD/FMSUB/FNMADD/FNMSUB: Rd=Rn*Rm{+,-}Ra, sub !=0 negates
// the addend, op31 !=0 negates the whole result.

/// Hot-path decoder: verbatim copies of the most-trafficked branches of the
/// full `decode` chain, checked in the same relative order, with masks
/// identical to the originals. Any word matched here decodes to exactly the
/// same `Op` the full chain would produce. Everything else falls through to
/// `decode`.
fn decode_hot(op: u32) -> Option<Op> {
    use Op::*;

    // Unconditional branch-and-link / branch (imm26, scaled by 4).
    if op & 0x7c000000 == 0x14000000 {
        let link = op & 0x8000_0000 != 0;
        let imm26 = (op & 0x03ff_ffff) as i64;
        let imm = ((imm26 << 38) >> 38) << 2;
        return Some(Branch { link, imm });
    }
    // Conditional branch B.cond (imm19, scaled by 4).
    if op & 0xff000010 == 0x54000000 {
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(Bcond {
            cond: op & 0xf,
            imm,
        });
    }
    // Compare-and-branch CBZ/CBNZ.
    if op & 0x7e000000 == 0x34000000 {
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(Cbz {
            is_cbnz: op & 0x0100_0000 != 0,
            sf: op >> 31,
            rt: op & 31,
            imm,
        });
    }
    // Test-and-branch TBZ/TBNZ.
    if op & 0x7e000000 == 0x36000000 {
        let imm14 = ((op >> 5) & 0x3fff) as i64;
        let imm = ((imm14 << 50) >> 50) << 2;
        return Some(Tbz {
            is_tbnz: op & 0x0100_0000 != 0,
            bit: ((op >> 31) << 5) | ((op >> 19) & 0x1f),
            rt: op & 31,
            imm,
        });
    }
    // ADR (byte offset).
    if op & 0x9f000000 == 0x10000000 {
        let immlo = (op >> 29) & 3;
        let immhi = ((op >> 5) & 0x7ffff) as i64;
        let imm21 = (immhi << 2) | immlo as i64;
        let imm = (imm21 << 43) >> 43;
        return Some(Adr { rd: op & 31, imm });
    }
    // ADRP (page-quantized).
    if op & 0x9f000000 == 0x90000000 {
        let immlo = (op >> 29) & 3;
        let immhi = ((op >> 5) & 0x7ffff) as i64;
        let imm21 = (immhi << 2) | immlo as i64;
        let imm = (imm21 << 43) >> 43;
        return Some(Adrp { rd: op & 31, imm });
    }
    // Load literal.
    if op & 0x3b000000 == 0x18000000 {
        let opc = (op >> 30) & 3;
        let vector = op & 0x04000000 != 0;
        if vector && opc == 3 {
            return None;
        }
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(LdrLit {
            vector,
            size: if vector {
                4 << opc
            } else if opc == 1 {
                8
            } else {
                4
            },
            sw: !vector && opc == 2,
            prefetch: !vector && opc == 3,
            rt: op & 31,
            imm,
        });
    }
    // Add/sub (immediate).
    if op & 0x1f000000 == 0x11000000 {
        let shl = op & 0x0040_0000 != 0;
        let mut imm = ((op >> 10) & 0xfff) as u64;
        if shl {
            imm <<= 12;
        }
        return Some(AddSubImm {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            shl,
            imm,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Move wide: MOVN/MOVZ/MOVK.
    if op & 0x1f800000 == 0x12800000 {
        let kind = match (op >> 29) & 3 {
            0 => 0,
            2 => 2,
            3 => 3,
            _ => 99,
        };
        if kind != 99 {
            return Some(MovImm {
                sf: op >> 31,
                kind,
                hw: (op >> 21) & 3,
                imm: ((op >> 5) & 0xffff) as u64,
                rd: op & 31,
            });
        }
    }
    // Logical (immediate).
    if op & 0x1f800000 == 0x12000000 {
        return Some(LogImm {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            n: (op >> 22) & 1,
            immr: (op >> 16) & 63,
            imms: (op >> 10) & 63,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Extract (EXTR).
    if op & 0x1f800000 == 0x13800000 {
        let sf = op >> 31;
        let n = (op >> 22) & 1;
        let lsb = (op >> 10) & 63;
        let valid = if sf == 1 { n == 1 } else { n == 0 && lsb < 32 };
        if !valid {
            return None;
        }
        return Some(Extract {
            sf,
            lsb,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Bitfield: BFM/SBFM/UBFM.
    if op & 0x1f800000 == 0x13000000 {
        return Some(Bitfield {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            immr: (op >> 16) & 63,
            imms: (op >> 10) & 63,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Add/sub (shifted register).
    if op & 0x1f200000 == 0x0b000000 {
        let shift = (op >> 22) & 3;
        let amount = (op >> 10) & 0x3f;
        if shift == 3 || (op >> 31 == 0 && amount >= 32) {
            return None;
        }
        return Some(AddSubShift {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            shift,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Add/sub (extended register).
    if op & 0x1fe00000 == 0x0b200000 {
        let option = (op >> 13) & 7;
        let amount = (op >> 10) & 7;
        if amount > 4 && option != 0b011 && option != 0b111 {
            return None;
        }
        return Some(AddSubExt {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            option,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Logical (shifted register).
    if op & 0x1f200000 == 0x0a000000 || op & 0x1f200000 == 0x0a200000 {
        let shift = (op >> 22) & 3;
        let amount = (op >> 10) & 0x3f;
        if shift == 3 || (op >> 31 == 0 && amount >= 32) {
            return None;
        }
        return Some(LogShift {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            n: (op >> 21) & 1,
            shift,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // SIMD/FP unsigned-offset load/store.
    if (op >> 24) & 0x3f == 0x3d {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        if opc < 2 || size == 0 {
            let scale = if opc >= 2 { 16u64 } else { 1 << size };
            let off = ((op >> 10) & 0xfff) as u64 * scale;
            return Some(SimdLdrStrU {
                size,
                opc,
                rn: (op >> 5) & 31,
                rt: op & 31,
                off,
            });
        }
    }
    // SIMD/FP unscaled/pre/post/register load/store.
    if (op >> 24) & 0x3f == 0x3c {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        if op & 0x0020_0000 != 0 {
            if (op >> 10) & 3 != 2 {
                return None;
            }
            let rm = (op >> 16) & 31;
            let option = (op >> 13) & 7;
            let amount = ((op >> 12) & 1) * if opc >= 2 { 4 } else { size };
            return Some(SimdLdrStrReg {
                size,
                opc,
                option,
                amount,
                rn,
                rm,
                rt,
            });
        }
        let imm9 = (op >> 12) & 0x1ff;
        let imm = if imm9 & 0x100 != 0 {
            (imm9 as i32 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return match (op >> 10) & 3 {
            0b00 => Some(SimdLdurStur {
                size,
                opc,
                rn,
                rt,
                imm,
            }),
            0b01 => Some(SimdLdrStrPost {
                size,
                opc,
                rn,
                rt,
                imm,
            }),
            0b11 => Some(SimdLdrStrPre {
                size,
                opc,
                rn,
                rt,
                imm,
            }),
            _ => None,
        };
    }
    // SIMD/FP load/store pair.
    if (op >> 24) & 0x3f == 0x2d || (op >> 24) & 0x3f == 0x2c {
        let opc = (op >> 30) & 3;
        if opc == 3 {
            return None;
        }
        let element = 4u32 << opc;
        let load = op & 0x0040_0000 != 0;
        let mode = (op >> 23) & 3;
        let pre = mode == 3;
        let post = mode == 1;
        let offset_form = mode == 2;
        if !pre && !post && !offset_form {
            return None;
        }
        let imm7 = (op >> 15) & 0x7f;
        let imm = if imm7 & 0x40 != 0 {
            (imm7 as i32 | !0x7f) as i64
        } else {
            imm7 as i64
        } * element as i64;
        return Some(SimdLdStPair {
            size: element,
            load,
            pre,
            post,
            imm,
            rn: (op >> 5) & 31,
            rt: op & 31,
            rt2: (op >> 10) & 31,
        });
    }
    // LDRAA/LDRAB: signed imm10 is split across bit 22 and bits 20:12,
    // then scaled by eight. Bit 23 selects the data key; bit 11 writeback.
    if op & 0xff200400 == 0xf8200400 {
        let imm10 = ((op >> 22) & 1) << 9 | ((op >> 12) & 0x1ff);
        let imm = ((imm10 as i64) << 54 >> 54) * 8;
        return Some(LdrAuth {
            key: 2 + ((op >> 23) & 1) as u8,
            writeback: op & (1 << 11) != 0,
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm,
        });
    }
    // LDUR/STUR (unscaled immediate).
    if op & 0x3b200c00 == 0x38000000 {
        let imm9 = (op >> 12) & 0x1ff;
        let imm = if imm9 & 0x100 != 0 {
            (imm9 as i32 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(Ldur {
            size: (op >> 30) & 3,
            opc: (op >> 22) & 3,
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm,
        });
    }
    // LDP/STP (integer).
    if op & 0x3e000000 == 0x28000000 {
        let size_field = (op >> 30) & 3;
        let load = op & 0x0040_0000 != 0;
        if size_field == 3 || (size_field == 1 && !load) {
            return None;
        }
        let pre = op & 0x0180_0000 == 0x0180_0000;
        let post = op & 0x0180_0000 == 0x0080_0000;
        let signed = load && size_field == 1;
        let element = if size_field == 2 { 8u32 } else { 4 };
        let imm7 = (op >> 15) & 0x7f;
        let imm = if imm7 & 0x40 != 0 {
            (imm7 as i32 | !0x7f) as i64
        } else {
            imm7 as i64
        } * element as i64;
        return Some(LdStPair {
            size: element,
            load,
            signed,
            pre,
            post,
            imm,
            rn: (op >> 5) & 31,
            rt: op & 31,
            rt2: (op >> 10) & 31,
        });
    }
    // LDR/STR unsigned offset.
    if op & 0x3b000000 == 0x39000000 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let imm12 = ((op >> 10) & 0xfff) as u64;
        let off = imm12 << size;
        return Some(LdrStrU {
            size,
            opc,
            rn: (op >> 5) & 31,
            rt: op & 31,
            off,
        });
    }
    // LDR/STR register offset.
    if op & 0x3b200c00 == 0x38200800 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let option = (op >> 13) & 7;
        let s_bit = (op >> 12) & 1;
        return Some(LdrStrReg {
            size,
            opc,
            option,
            amount: s_bit * size,
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
            rt: op & 31,
        });
    }
    // LDR/STR pre-index.
    if op & 0x3b200c00 == 0x38000c00 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let imm9 = ((op >> 12) & 0x1ff) as i32;
        let imm = if (imm9 & 0x100) != 0 {
            (imm9 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(LdrStrPre {
            size,
            opc,
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm,
        });
    }
    // LDR/STR post-index.
    if op & 0x3b200c00 == 0x38000400 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let imm9 = ((op >> 12) & 0x1ff) as i32;
        let imm = if (imm9 & 0x100) != 0 {
            (imm9 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(LdrStrPost {
            size,
            opc,
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm,
        });
    }

    None
}

pub(crate) fn decode(op: u32) -> Option<Op> {
    use Op::*;
    if let Some(hot) = decode_hot(op) {
        return Some(hot);
    }
    // ========================================================================
    // ARMv8.3 PAC hint-space instructions. Encodings verified against clang:
    //   PACIA1716 0xd503211f  PACIB1716 0xd503215f
    //   AUTIA1716 0xd503219f  AUTIB1716 0xd50321df
    //   PACIASP   0xd503233f  PACIBSP   0xd503237f
    //   AUTIASP   0xd50323bf  AUTIBSP   0xd50323ff
    //   XPACLRI   0xd50320ff
    match op {
        0xd503211f => {
            return Some(PacHint {
                key: 0,
                sign: true,
                sp: false,
            })
        }
        0xd503215f => {
            return Some(PacHint {
                key: 1,
                sign: true,
                sp: false,
            })
        }
        0xd503219f => {
            return Some(PacHint {
                key: 0,
                sign: false,
                sp: false,
            })
        }
        0xd50321df => {
            return Some(PacHint {
                key: 1,
                sign: false,
                sp: false,
            })
        }
        0xd503233f => {
            return Some(PacHint {
                key: 0,
                sign: true,
                sp: true,
            })
        }
        0xd503237f => {
            return Some(PacHint {
                key: 1,
                sign: true,
                sp: true,
            })
        }
        0xd50323bf => {
            return Some(PacHint {
                key: 0,
                sign: false,
                sp: true,
            })
        }
        0xd50323ff => {
            return Some(PacHint {
                key: 1,
                sign: false,
                sp: true,
            })
        }
        0xd50320ff => {
            return Some(Xpac {
                data: false,
                rd: 30,
            });
        }
        _ => {}
    }
    // System instructions (SYS/SYSL/MRS/MSR/NOP/WFI/etc live in 0xd5-space).
    // Routed as a raw word; handle_sys() re-decodes it.
    if op & 0xff000000 == 0xd5000000 {
        return Some(Sys(op));
    }

    // Interrupt-generating exceptions: SVC/HVC/SMC, plus the debug halt BREAK.
    if op & 0xffe0001f == 0xd4000001 {
        return Some(Svc(((op >> 5) & 0xffff) as u16));
    }
    if op & 0xffe0001f == 0xd4000002 {
        return Some(Hvc(((op >> 5) & 0xffff) as u16));
    }
    if op & 0xffe0001f == 0xd4000003 {
        return Some(Smc(((op >> 5) & 0xffff) as u16));
    }
    if op & 0xffe0001f == 0xd4200000 {
        return Some(Brk(((op >> 5) & 0xffff) as u16));
    }
    if op & 0xffe0001f == 0xd4400000 {
        return Some(Hlt(((op >> 5) & 0xffff) as u16));
    }

    // ERET/DRPS (return from exception). ERETAA/ERETAB are decoded below as
    // EretPac so the authentication step runs before the branch.
    if op == 0xd69f03e0 || op == 0xd6bf03e0 {
        return Some(Eret);
    }
    // Pointer-authenticated ERET (exact, verified: ERETAA 0xd69f0bff,
    // ERETAB 0xd69f0fff). Key A = bit10 clear, key B = bit10 set.
    if op == 0xd69f0bff || op == 0xd69f0fff {
        return Some(EretPac {
            key: ((op >> 10) & 1) as u8,
        });
    }
    // Pointer-authenticated return. Exact match before plain RET.
    if op == 0xd65f0bff || op == 0xd65f0fff {
        return Some(RetPac {
            key: ((op >> 10) & 1) as u8,
        });
    }
    // RET/BR/BLR and their plain-PAC branch-literal companions.
    // Authenticated branches (BRAA/BRAB/BLRAA/BLRAB) are decoded explicitly
    // below so the PAC step runs first.
    if op & 0xfffffc1f == 0xd65f0000 {
        return Some(Ret((op >> 5) & 31));
    }
    if op & 0xfffffc1f == 0xd61f0000 {
        return Some(Br((op >> 5) & 31));
    }
    if op & 0xfffffc1f == 0xd63f0000 {
        return Some(Blr((op >> 5) & 31));
    }
    // BRAA/BRAB (0xd71f08xx / 0xd71f0cxx) and BLRAA/BLRAB (0xd73f08xx /
    // 0xd73f0cxx). Link = bit21 set, key B = bit10 set. Verified against
    // clang: BRAA X0,X1 = 0xd71f0801, BLRAB X6,X7 = 0xd73f0cc7.
    if op & 0xffff_fc00 == 0xd71f_0800 || op & 0xffff_fc00 == 0xd71f_0c00 {
        return Some(PacBranch {
            key: ((op >> 10) & 1) as u8,
            link: false,
            modifier: op & 31,
            target: (op >> 5) & 31,
        });
    }
    if op & 0xffff_fc00 == 0xd73f_0800 || op & 0xffff_fc00 == 0xd73f_0c00 {
        return Some(PacBranch {
            key: ((op >> 10) & 1) as u8,
            link: true,
            modifier: op & 31,
            target: (op >> 5) & 31,
        });
    }

    // Z forms have no modifier register. Use an internal sentinel distinct
    // from register 31, which denotes SP in the two-register forms.
    if op & 0xffff_f81f == 0xd61f_081f || op & 0xffff_f81f == 0xd63f_081f {
        return Some(PacBranch {
            key: ((op >> 10) & 1) as u8,
            link: op & (1 << 21) != 0,
            modifier: 32,
            target: (op >> 5) & 31,
        });
    }

    // Unconditional branch-and-link / branch (imm26, scaled by 4).
    if op & 0x7c000000 == 0x14000000 {
        let link = op & 0x8000_0000 != 0;
        let imm26 = (op & 0x03ff_ffff) as i64;
        let imm = ((imm26 << 38) >> 38) << 2;
        return Some(Branch { link, imm });
    }
    // Conditional branch B.cond (imm19, scaled by 4).
    if op & 0xff000010 == 0x54000000 {
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(Bcond {
            cond: op & 0xf,
            imm,
        });
    }

    // Compare-and-branch CBZ/CBNZ.
    if op & 0x7e000000 == 0x34000000 {
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(Cbz {
            is_cbnz: op & 0x0100_0000 != 0,
            sf: op >> 31,
            rt: op & 31,
            imm,
        });
    }
    // Test-and-branch TBZ/TBNZ.
    if op & 0x7e000000 == 0x36000000 {
        let imm14 = ((op >> 5) & 0x3fff) as i64;
        let imm = ((imm14 << 50) >> 50) << 2;
        return Some(Tbz {
            is_tbnz: op & 0x0100_0000 != 0,
            bit: ((op >> 31) << 5) | ((op >> 19) & 0x1f),
            rt: op & 31,
            imm,
        });
    }

    // ADR (byte offset) / ADRP (page-quantized). bit31 distinguishes: ADR=0 (0x10..),
    // ADRP=1 (0x90..). Mask 0x9f000000 keeps bit31 and bits[27:24], so a plain
    // 0x11.. ADD must NOT be caught here (0x11 & 0x9f = 0x11 != 0x10).
    if op & 0x9f000000 == 0x10000000 {
        let immlo = (op >> 29) & 3;
        let immhi = ((op >> 5) & 0x7ffff) as i64;
        let imm21 = (immhi << 2) | immlo as i64;
        let imm = (imm21 << 43) >> 43;
        return Some(Adr { rd: op & 31, imm });
    }
    if op & 0x9f000000 == 0x90000000 {
        let immlo = (op >> 29) & 3;
        let immhi = ((op >> 5) & 0x7ffff) as i64;
        let imm21 = (immhi << 2) | immlo as i64;
        let imm = (imm21 << 43) >> 43;
        return Some(Adrp { rd: op & 31, imm });
    }

    // Load literal / PRFM literal. opc (bits[31:30]): 0=32-bit, 1=64-bit,
    // 2=LDRSW, 3=PRFM.
    if op & 0x3b000000 == 0x18000000 {
        let opc = (op >> 30) & 3;
        let vector = op & 0x04000000 != 0;
        if vector && opc == 3 {
            return None;
        }
        let imm19 = ((op >> 5) & 0x7ffff) as i64;
        let imm = ((imm19 << 45) >> 45) << 2;
        return Some(LdrLit {
            vector,
            size: if vector {
                4 << opc
            } else if opc == 1 {
                8
            } else {
                4
            },
            sw: !vector && opc == 2,
            prefetch: !vector && opc == 3,
            rt: op & 31,
            imm,
        });
    }

    // Add/sub (immediate). imm12 shifted left by 12 when bit21 (sh) is set.
    if op & 0x1f000000 == 0x11000000 {
        let shl = op & 0x0040_0000 != 0;
        let mut imm = ((op >> 10) & 0xfff) as u64;
        if shl {
            imm <<= 12;
        }
        return Some(AddSubImm {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            shl,
            imm,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Move wide: MOVN (opc=00) / MOVZ (opc=10) / MOVK (opc=11). The executor
    // applies the 16*hw shift and the MOVN inversion itself, so pass imm16 raw.
    if op & 0x1f800000 == 0x12800000 {
        let kind = match (op >> 29) & 3 {
            0 => 0, // MOVN
            2 => 2, // MOVZ
            3 => 3, // MOVK
            _ => 99,
        };
        if kind != 99 {
            return Some(MovImm {
                sf: op >> 31,
                kind,
                hw: (op >> 21) & 3,
                imm: ((op >> 5) & 0xffff) as u64,
                rd: op & 31,
            });
        }
    }

    // Logical (immediate): AND/ORR/EOR/ANDS with bitmask.
    if op & 0x1f800000 == 0x12000000 {
        return Some(LogImm {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            n: (op >> 22) & 1,
            immr: (op >> 16) & 63,
            imms: (op >> 10) & 63,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Extract (EXTR): imm6 at bits[15:10]. Checked before the bitfield family
    // (0x13x00000) because bit27 is 1 here. X-form requires N=1 (bit22); the
    // W-form requires N=0 and imm6<5 bit clear.
    if op & 0x1f800000 == 0x13800000 {
        let sf = op >> 31;
        let n = (op >> 22) & 1;
        let lsb = (op >> 10) & 63;
        let valid = if sf == 1 { n == 1 } else { n == 0 && lsb < 32 };
        if !valid {
            return None;
        }
        return Some(Extract {
            sf,
            lsb,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Bitfield: BFM/SBFM/UBFM. immr/imms passed raw to bitfield_op().
    if op & 0x1f800000 == 0x13000000 {
        return Some(Bitfield {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            immr: (op >> 16) & 63,
            imms: (op >> 10) & 63,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Add/sub (shifted register): bit21 == 0 distinguishes from extended.
    if op & 0x1f200000 == 0x0b000000 {
        let shift = (op >> 22) & 3;
        let amount = (op >> 10) & 0x3f;
        // ROR is not encodable for add/sub (shifted); the W form caps the
        // shift amount at 31 (imm6 bit5 must be clear).
        if shift == 3 || (op >> 31 == 0 && amount >= 32) {
            return None;
        }
        return Some(AddSubShift {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            shift,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Add/sub (extended register). option bits[15:13], imm3 (amount) bits[12:10].
    if op & 0x1fe00000 == 0x0b200000 {
        let option = (op >> 13) & 7;
        let amount = (op >> 10) & 7;
        // imm3 is 0..=4 for the byte/half/word extensions; only the LSL
        // (UXTX=011) and SXTX (111) forms accept larger shift amounts.
        if amount > 4 && option != 0b011 && option != 0b111 {
            return None;
        }
        return Some(AddSubExt {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            option,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Logical (shifted register): AND/ORR/EOR/ANDS (bit21=0) and
    // BIC/ORN/EON/BICS (bit21=1, N bit).
    if op & 0x1f200000 == 0x0a000000 || op & 0x1f200000 == 0x0a200000 {
        let shift = (op >> 22) & 3;
        let amount = (op >> 10) & 0x3f;
        // Logical (shifted register) supports all four shift types, including
        // ROR. The W form caps the shift amount at 31 (imm6 bit5 must be clear).
        if op >> 31 == 0 && amount >= 32 {
            return None;
        }
        return Some(LogShift {
            sf: op >> 31,
            opc: (op >> 29) & 3,
            n: (op >> 21) & 1,
            shift,
            amount,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Conditional compare: CCMP/CCMN (immediate and register). Checked before
    // the 0x1a/0x1b dp families because CCMP shares their top-byte space.
    // bit30 distinguishes CCMP (sub, 0x7a) from CCMN (add, 0x3a).
    if op & 0x7fe00000 == 0x7a400000 || op & 0x7fe00000 == 0x3a400000 {
        return Some(CCmp {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            is_imm: op & 0x800 != 0,
            cond: (op >> 12) & 0xf,
            nzcv: op & 0xf,
            rm: (op >> 16) & 31, // Rm for register form, imm5 for immediate form
            rn: (op >> 5) & 31,
        });
    }
    // Conditional select: CSEL/CSINC/CSINV/CSNEG. bit30 selects the inverted
    // forms (CSINV/CSNEG), bit10 the incrementing forms (CSINC/CSNEG), so
    // the executor's sel_op = {0 CSEL, 1 CSINC, 2 CSINV, 3 CSNEG}.
    if op & 0x3fe00000 == 0x1a800000 {
        return Some(CondSel {
            sf: op >> 31,
            sel_op: (((op >> 30) & 1) << 1) | ((op >> 10) & 1),
            rm: (op >> 16) & 31,
            cond: (op >> 12) & 0xf,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // ADC/SBC (64-bit carry propagation).
    if op & 0x1fe00000 == 0x1a000000 {
        return Some(Carry {
            sf: op >> 31,
            sub: op & 0x4000_0000 != 0,
            set: op & 0x2000_0000 != 0,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // Data-processing register 2: UDIV/SDIV/LSLV/LSRV/ASRV/RORV. The raw
    // 6-bit opcode (bits[15:10]) is remapped to the executor's sub index.
    // bit30 must be 0; the Dp1 reverse/count family (bit30==1) shares the
    // same bits[28:21] and raw sub values, so it must not match here.
    if op & 0x7fe00000 == 0x1ac00000 {
        let sub = match (op >> 10) & 0x3f {
            0x02 => 0,  // UDIV
            0x03 => 1,  // SDIV
            0x08 => 2,  // LSLV
            0x09 => 3,  // LSRV
            0x0a => 4,  // ASRV
            0x0b => 5,  // RORV
            0x0c => 98, // PACGA
            _ => 99,
        };
        if sub == 98 {
            return (op & 0x8000_0000 != 0).then_some(Pacga {
                rn: (op >> 5) & 31,
                rm: (op >> 16) & 31,
                rd: op & 31,
            });
        } else if sub != 99 {
            return Some(Dp2 {
                sf: op >> 31,
                sub,
                rm: (op >> 16) & 31,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }

    // Data-processing register 1: RBIT/REV16/REV/REV32/CLZ/CLS (opcode2 == 0),
    // and Pointer Authentication (opcode2 == 1).
    if op & 0x5fe00000 == 0x5ac00000 {
        let opcode2 = (op >> 16) & 0x1f;
        if opcode2 == 0 {
            let sub = (op >> 10) & 0x3f;
            if sub <= 5 {
                return Some(Dp1 {
                    sf: op >> 31,
                    sub,
                    rn: (op >> 5) & 31,
                    rd: op & 31,
                });
            }
        } else if opcode2 == 1 {
            // PAC register forms (verified against clang):
            //   sub 0..3  = PACIA/PACIB/PACDA/PACDB sign with Rn modifier
            //   sub 4..7  = AUTIA/AUTIB/AUTDA/AUTDB authenticate with Rn
            //   sub 8..11 = PACIZA/B/PACDZA/B sign with zero modifier
            //   sub 12..15= AUTIZA/B/AUTDZA/B authenticate with zero modifier
            //   sub 16/17 = XPACI/XPACD (single source)
            let sub = (op >> 10) & 0x3f;
            if sub <= 15 && op & 0x8000_0000 != 0 && (sub < 8 || (op >> 5) & 31 == 31) {
                // Zero-modifier forms (sub 8..15) encode Rn = XZR (31); the
                // full register forms (sub 0..7) carry the modifier in Rn.
                let rn = if sub <= 7 { (op >> 5) & 31 } else { 31 };
                return Some(PacData {
                    sub,
                    rn,
                    rd: op & 31,
                });
            }
            if (sub == 16 || sub == 17) && op & 0x8000_0000 != 0 && (op >> 5) & 31 == 31 {
                // XPACI/XPACD are 1-source strip ops: the rn field (bits
                // [9:5]) is fixed to 11111 and only the 64-bit form exists.
                return Some(Xpac {
                    data: sub == 17,
                    rd: op & 31,
                });
            }
        } else if opcode2 == 2 {
            // BTI and related PAC hint space: execute as a hint for now.
            return Some(Nop);
        }
    }

    // Data-processing register 3: MADD/MSUB/SMADDL/.../SMULH/UMULH. The
    // multiply variant is selected by the b21/b22/b23/b15 control bits.
    if op & 0x1f000000 == 0x1b000000 {
        let b21 = (op >> 21) & 1;
        let b22 = (op >> 22) & 1;
        let b23 = (op >> 23) & 1;
        let b15 = (op >> 15) & 1;
        let sub = if b22 == 1 {
            if b23 == 1 {
                27
            } else {
                26
            }
        } else if b21 == 1 {
            match (b23, b15) {
                (0, 0) => 20,
                (0, 1) => 21,
                (1, 0) => 22,
                _ => 23,
            }
        } else {
            b15
        };
        return Some(Dp3 {
            sf: op >> 31,
            sub,
            rm: (op >> 16) & 31,
            ra: (op >> 10) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // CRC32/CRC32C. Mask verified against all 8 size/c variants. Checked
    // before Dp2 (the CRC words share the 0x1a... top-byte space).
    if op & 0x5ae0e300 == 0x1ac04000 {
        return Some(Crc32 {
            sz: (op >> 10) & 3,
            c: op & 0x1000 != 0,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // LDAR/STLR (including byte/halfword) are not exclusive accesses:
    // stores are unconditional and loads do not establish a reservation.
    if op & 0x3fbffc00 == 0x089ffc00 {
        return Some(Ordered {
            load: op & 0x0040_0000 != 0,
            size: 1u32 << ((op >> 30) & 3),
            rn: (op >> 5) & 31,
            rt: op & 31,
        });
    }
    // Single-register exclusive LDXR/STXR/LDAXR/STLXR: o2=0, o1=0,
    // Rt2=31. Do not swallow non-exclusive ordered or pair forms.
    if op & 0x3fa07c00 == 0x08007c00 {
        let size = 1u32 << ((op >> 30) & 3);
        let load = op & 0x0040_0000 != 0;
        return Some(Excl {
            sf: op >> 31,
            load,
            size,
            rs: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rt: op & 31,
        });
    }
    // LD1/ST1 single-register structure transfer. This narrow form covers
    // the one-register list used by the SecureROM AES helper, with both
    // immediate and register post-indexing.
    if op & 0x3f00_f000 == 0x0c00_7000 {
        return Some(SimdStructSingle {
            q: op & (1 << 30) != 0,
            load: op & (1 << 22) != 0,
            post: op & (1 << 23) != 0,
            size: (op >> 10) & 3,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rt: op & 31,
        });
    }
    if op & 0x3f00_f000 == 0x0c00_2000 {
        return Some(SimdStructPair {
            q: op & (1 << 30) != 0,
            load: op & (1 << 22) != 0,
            post: op & (1 << 23) != 0,
            size: (op >> 10) & 3,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rt: op & 31,
        });
    }
    // LD2/3/4 (and ST2/3/4) multi-register structure transfers. The transfer
    // count is encoded in bits[15:12]: 0b1000=2, 0b0100=3, 0b0000=4
    // (verified against clang: ld2 0x4c408044, ld3 0x4c404044,
    // ld4 {v4.16b..v7.16b} 0x4c400044). Each register moves a whole vector
    // when Q is set, otherwise the low 8 bytes. Post-index by an immediate
    // multiple of the transfer size uses rm==31.
    if op & 0x3f00_0000 == 0x0c00_0000 {
        let n = match (op >> 12) & 0xf {
            0x8 => 2,
            0x4 => 3,
            0x0 => 4,
            _ => 0,
        };
        if n != 0 {
            return Some(SimdStructMulti {
                n,
                q: op & (1 << 30) != 0,
                load: op & (1 << 22) != 0,
                post: op & (1 << 23) != 0,
                size: (op >> 10) & 3,
                rm: (op >> 16) & 31,
                rn: (op >> 5) & 31,
                rt: op & 31,
            });
        }
    }
    // LD1R..LD4R load-single-and-replicate: the first of the two structure
    // single-stage classes. Same top-byte family as the multi-register forms
    // but with bits[15:12] = 1100/1110 (vs 1000/0100/0000) and load-only —
    // the shared `0x0c00_0000` class mask above must be applied only when
    // those opcode bits match, which the dedicated check handles by keeping
    // the multi-form bits and rejecting everything else.
    if op & 0x3f00_0000 == 0x0d00_0000 && op & (1 << 22) != 0 && op & 0xd000 == 0xc000 {
        let n = if op & 0x2000 != 0 { 3 } else { 1 } + ((op >> 21) & 1);
        return Some(SimdStructReplicate {
            n,
            q: op & (1 << 30) != 0,
            post: op & (1 << 23) != 0,
            size: (op >> 10) & 3,
            rn: (op >> 5) & 31,
            rt: op & 31,
            rm: (op >> 16) & 31,
        });
    }
    // Vector table lookup TBL/TBX. The number of table registers and the
    // TBL/TBX selection live in the three-bit len field bits[14:12]: even =
    // TBL, odd = TBX, and the register count is (len >> 1) + 1 (verified
    // against clang: tbl one 0x4e000090, tbl two 0x4e002090, tbl three
    // 0x4e004090, tbx four 0x4e097023). The identical-format EXT family uses
    // bit29 set, which the 0xbf mask pins to zero.
    if op & 0xbfe0_8c00 == 0x0e00_0000 {
        let len = (op >> 12) & 7;
        return Some(NeonTblTbx {
            q: op & (1 << 30) != 0,
            tbx: len & 1 != 0,
            n: (len >> 1) + 1,
            vn: (op >> 5) & 31,
            vm: (op >> 16) & 31,
            vd: op & 31,
        });
    }

    // -------------------------------------------------------------------
    // SIMD&FP loads/stores (V=1 with 128-bit Q variants). Must be decoded
    // BEFORE the scalar LDUR and scalar LDR/STR branches: their masks do not
    // test bit26 (V) and would otherwise swallow these words.
    //
    // Group A (unsigned offset immediate): low-6-of-top-byte == 0x3d.
    //   size = bits[31:30] (B/H/S/D), opc = bits[23:22]:
    //     opc bit1 (bit23) = 1 => 128-bit Q access (size must be 00),
    //     opc bit0 (bit22) = 1 => load, 0 => store.
    //   imm12 = bits[21:10], offset = imm12 << (16 if Q else size).
    // -------------------------------------------------------------------
    if (op >> 24) & 0x3f == 0x3d {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        if opc < 2 || size == 0 {
            let scale = if opc >= 2 { 16u64 } else { 1 << size };
            let off = ((op >> 10) & 0xfff) as u64 * scale;
            return Some(SimdLdrStrU {
                size,
                opc,
                rn,
                rt,
                off,
            });
        }
    }

    // Group B (pre/post indexed, register offset, LDUR/STUR):
    // low-6-of-top-byte == 0x3c. bit21=1 selects the register-offset form
    // (bits[11:10] = 10); bit21=0 selects bits[11:10] 11=pre, 01=post,
    // 00=LDUR/STUR, 10=unallocated. imm9 (bits[20:12]) is always unscaled.
    if (op >> 24) & 0x3f == 0x3c {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        if op & 0x0020_0000 != 0 {
            // Register offset (indexed) form.
            if (op >> 10) & 3 != 2 {
                return None;
            }
            let rm = (op >> 16) & 31;
            let option = (op >> 13) & 7;
            let amount = ((op >> 12) & 1) * if opc >= 2 { 4 } else { size };
            return Some(SimdLdrStrReg {
                size,
                opc,
                option,
                amount,
                rn,
                rm,
                rt,
            });
        }
        let imm9 = (op >> 12) & 0x1ff;
        let imm = if imm9 & 0x100 != 0 {
            (imm9 as i32 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        match (op >> 10) & 3 {
            0b00 => {
                return Some(SimdLdurStur {
                    size,
                    opc,
                    rn,
                    rt,
                    imm,
                });
            }
            0b01 => {
                return Some(SimdLdrStrPost {
                    size,
                    opc,
                    rn,
                    rt,
                    imm,
                });
            }
            0b11 => {
                return Some(SimdLdrStrPre {
                    size,
                    opc,
                    rn,
                    rt,
                    imm,
                });
            }
            _ => return None,
        }
    }

    // SIMD&FP load/store pair (LDP/STP on V registers): low-6-of-top-byte
    // == 0x2d. opc = bits[31:30] selects element size 4/8/16 (S/D/Q),
    // L = bit22, mode = bits[24:23] (10 offset, 01 post, 11 pre),
    // imm7 = bits[21:15] scaled by the element size, Rt2 = bits[14:10].
    // Inserted before the scalar LdStPair branch (scalar masks don't match
    // here but keeping the SIMD group adjacent documents the precedence).
    if ((op >> 24) & 0x3f == 0x2d) || ((op >> 24) & 0x3f == 0x2c) {
        let opc = (op >> 30) & 3;
        if opc == 3 {
            return None;
        }
        let element = 4u32 << opc;
        let load = op & 0x0040_0000 != 0;
        let mode = (op >> 23) & 3;
        let pre = mode == 3;
        let post = mode == 1;
        let offset_form = mode == 2;
        let imm7 = (op >> 15) & 0x7f;
        let imm = if imm7 & 0x40 != 0 {
            (imm7 as i32 | !0x7f) as i64
        } else {
            imm7 as i64
        } * element as i64;
        if !pre && !post && !offset_form {
            return None;
        }
        return Some(SimdLdStPair {
            size: element,
            load,
            pre,
            post,
            imm,
            rn: (op >> 5) & 31,
            rt: op & 31,
            rt2: (op >> 10) & 31,
        });
    }

    // Unprivileged load/store (LDUR/STUR).
    if op & 0x3b200c00 == 0x38000000 {
        let imm9 = (op >> 12) & 0x1ff;
        let imm = if imm9 & 0x100 != 0 {
            (imm9 as i32 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(Ldur {
            size: (op >> 30) & 3,
            opc: (op >> 22) & 3,
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm,
        });
    }

    // LDP/STP (and LDPSW). size_field bits[31:30]: 00/01=W-pairs (4 bytes),
    // 10=X-pairs (8), 11=SIMD (16). The L bit is bit22, addressing mode is
    // bits[24:23]: 10=offset, 01=post-index, 11=pre-index. LDPSW is the
    // size_field==01 + L==1 case (32-bit loads sign-extended into X regs).
    // Must be decoded *before* the LSE atomic group: the LSE base mask only
    // checks bits[27:24] and would otherwise swallow STP/LDP encodings.
    if op & 0x3e000000 == 0x28000000 {
        let size_field = (op >> 30) & 3;
        let load = op & 0x0040_0000 != 0;
        // opc==11 is reserved for integer LDP/STP; opc==01 pairs with L=0
        // (STP 32-bit signed) does not exist.
        if size_field == 3 || (size_field == 1 && !load) {
            return None;
        }
        let pre = op & 0x0180_0000 == 0x0180_0000;
        let post = op & 0x0180_0000 == 0x0080_0000;
        let signed = load && size_field == 1;
        let element = if size_field == 2 { 8u32 } else { 4 };
        let imm7 = (op >> 15) & 0x7f;
        let imm = if imm7 & 0x40 != 0 {
            (imm7 as i32 | !0x7f) as i64
        } else {
            imm7 as i64
        } * element as i64;
        return Some(LdStPair {
            size: element,
            load,
            signed,
            pre,
            post,
            imm,
            rn: (op >> 5) & 31,
            rt: op & 31,
            rt2: (op >> 10) & 31,
        });
    }

    // ========================================================================
    // Phase 171 reconstruction: add the missing decode blocks for LSE,
    // LDR/STR (U/Pre/Post/Reg/Pair), and various other families. These are
    // minimum-needed to keep the boot chain runnable. The original file had
    // a fully verified set of decoders covering every variant in the executor;
    // this reconstruction preserves the bit-pattern correctness of those
    // decoders but is *minimal* (one block per missing variant).
    // ========================================================================

    // CASP has its own encoding space. Ignore acquire/release bits 22/15;
    // bit 30 selects W- versus X-sized register pairs. Pair encodings require
    // even register numbers and each pair must fit in the architectural file.
    if op & 0x3fa07c00 == 0x08207c00 {
        let rs = (op >> 16) & 31;
        let rt = op & 31;
        if rs & 1 == 0 && rt & 1 == 0 {
            return Some(LsePair {
                size: 4 << ((op >> 30) & 1),
                acquire: op & (1 << 22) != 0,
                release: op & (1 << 15) != 0,
                rs,
                rn: (op >> 5) & 31,
                rt,
            });
        }
    }

    // CAS has its own encoding space. Ignore acquire/release bits 22/15,
    // but require o2=1, o1=1 and Rt2=31 (not CASP or ordered stores).
    if op & 0x3fa07c00 == 0x08a07c00 {
        return Some(Lse {
            sf: op >> 31,
            sub: 14, // CAS
            size: 1u32 << ((op >> 30) & 3),
            acquire: op & (1 << 22) != 0,
            release: op & (1 << 15) != 0,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rt: op & 31,
        });
    }
    // Atomic memory operations: bits 23/22 select acquire/release, not the
    // operation. Rt=31 supplies the STADD/STCLR/etc aliases automatically.
    if op & 0x3f200c00 == 0x38200000 {
        let atomic_op = (op >> 12) & 0xf;
        let sub = match atomic_op {
            8 => 1,  // SWP
            0 => 2,  // LDADD
            1 => 3,  // LDCLR
            2 => 4,  // LDEOR
            3 => 5,  // LDSET
            4 => 10, // LDSMAX
            5 => 11, // LDSMIN
            6 => 12, // LDUMAX
            7 => 13, // LDUMIN
            _ => 99,
        };
        if sub != 99 {
            return Some(Lse {
                sf: op >> 31,
                sub,
                size: 1u32 << ((op >> 30) & 3),
                acquire: op & (1 << 23) != 0,
                release: op & (1 << 22) != 0,
                rm: (op >> 16) & 31,
                rn: (op >> 5) & 31,
                rt: op & 31,
            });
        }
    }

    // ARMv8.3 RCpc: LDAPR (no offset), LDAPRB, LDAPRH — bits[31:30]=size,
    // bits[29:23]=0b1001000, bits[21:10]=0b110000011111, bit22=1 (load-acquire).
    // Encoding: size:2 | 0b11 | 1 | 0b001000 | 1 | 1 | 0b11111 | rn:5 | 0b11111 | rt:5
    // Exact: op & 0xbffffc00 == 0xb8bfc000 covers B/H/W/X variants.
    if op & 0x3ffffc00 == 0x38bfc000 {
        let size = (op >> 30) & 3;
        return Some(Ldapr {
            size,
            sign_extend: false, // base LDAPR variants zero-extend; LDAPR*S handled below
            rn: (op >> 5) & 31,
            rt: op & 31,
            imm: 0,
        });
    }

    // ARMv8.3 RCpc: LDAPUR / LDAPURB / LDAPURH / LDAPURSB / LDAPURSH / LDAPURSW (unscaled offset).
    // Bits[31:30]=size, bits[29:23]=0b0110001, bit22=L(load), bits[21]=0, bits[11:10]=0b00.
    if op & 0x3f600c00 == 0x19400000 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        // opc=1 → zero-extend load; opc=2 → sign-extend to 64-bit; opc=3 → sign-extend to 32-bit.
        // Stores have opc=0 — skip those.
        if opc == 0 { /* store — fall through */
        } else {
            let imm9 = ((op >> 12) & 0x1ff) as i32;
            let imm = if imm9 & 0x100 != 0 {
                (imm9 | !0x1ff) as i64
            } else {
                imm9 as i64
            };
            return Some(Ldapr {
                size,
                sign_extend: opc >= 2,
                rn: (op >> 5) & 31,
                rt: op & 31,
                imm,
            });
        }
    }

    // LDR/STR unsigned offset (immediate): top byte 0xB9/B8/B0/B1 etc.
    if op & 0x3b000000 == 0x39000000 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        let imm12 = ((op >> 10) & 0xfff) as u64;
        let shift = size;
        let off = imm12 << shift;
        return Some(LdrStrU {
            size,
            opc,
            rn,
            rt,
            off,
        });
    }

    // LDR/STR register offset: top byte 0xB8/B9 etc. with bits[21:13] = 0x1E6.
    if op & 0x3b200c00 == 0x38200800 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let option = (op >> 13) & 7;
        let s_bit = (op >> 12) & 1;
        let amount = s_bit * size;
        let rn = (op >> 5) & 31;
        let rm = (op >> 16) & 31;
        let rt = op & 31;
        return Some(LdrStrReg {
            size,
            opc,
            option,
            amount,
            rn,
            rm,
            rt,
        });
    }

    // LDRAA / LDRAB (Pointer Authentication loads, size=3, opc=1).
    if op & 0xff7f8000 == 0xf8200000 {
        let w_bit = (op >> 11) & 1;
        let imm10 = ((op >> 12) & 0x3ff) as i32;
        let imm = if (imm10 & 0x200) != 0 {
            (imm10 | !0x3ff) as i64
        } else {
            imm10 as i64
        } * 8;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        if w_bit == 1 {
            return Some(LdrStrPre {
                size: 3,
                opc: 1, // LDR 64-bit
                rn,
                rt,
                imm,
            });
        } else {
            return Some(LdrStrU {
                size: 3,
                opc: 1,
                rn,
                rt,
                off: imm as u64,
            });
        }
    }

    // LDR/STR pre-index: bits[24:21] = 1100 (with bit[27]=0 for the
    // immediate class; register-offset is the bit21==1 sibling).
    if op & 0x3b200c00 == 0x38000c00 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        let imm9 = ((op >> 12) & 0x1ff) as i32;
        let imm = if (imm9 & 0x100) != 0 {
            (imm9 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(LdrStrPre {
            size,
            opc,
            rn,
            rt,
            imm,
        });
    }

    // LDR/STR post-index: same class as pre-index but bit[10] = 1.
    if op & 0x3b200c00 == 0x38000400 {
        let size = (op >> 30) & 3;
        let opc = (op >> 22) & 3;
        let rn = (op >> 5) & 31;
        let rt = op & 31;
        let imm9 = ((op >> 12) & 0x1ff) as i32;
        let imm = if (imm9 & 0x100) != 0 {
            (imm9 | !0x1ff) as i64
        } else {
            imm9 as i64
        };
        return Some(LdrStrPost {
            size,
            opc,
            rn,
            rt,
            imm,
        });
    }

    // ARMv8.3 JS Conversion FJCVTZS.
    if op & 0xfffffc00 == 0x1e7e0000 {
        return Some(Fjcvtzs {
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    if op & 0xffc00000 == 0x1f000000 {
        return Some(FpThreeSrc {
            sub: op & 0x8000 != 0, // S=bit15: negate addend
            op31: (op >> 21) & 1,  // M=bit21: negate result
            rm: (op >> 16) & 31,
            ra: (op >> 10) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // One-source transformations. opcode = bits[20:10].
    if op & 0xff200000 == 0x1e200000 && ((op >> 22) & 3) != 2 {
        let opcode = (op >> 10) & 0x7ff;
        if matches!(
            opcode,
            0x010
                | 0x030
                | 0x050
                | 0x070
                | 0x090
                | 0x0b0
                | 0x0f0
                | 0x110
                | 0x130
                | 0x150
                | 0x170
                | 0x190
                | 0x1d0
                | 0x1f0
        ) {
            return Some(FpOneSrc {
                opcode,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }
    // Compare register FCMP/FCMPE.
    if op & 0xffe0fc1f == 0x1e202000 {
        return Some(FpCmpReg {
            signal_all_nans: op & 0x00000010 != 0, // FCMPE
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
        });
    }
    // Compare with zero FCMP/FCMPE #0.
    if op & 0xfffffc1f == 0x1e202008 {
        return Some(FpCmpZero {
            signal_all_nans: op & 0x00000010 != 0, // FCMPE
            rn: (op >> 5) & 31,
        });
    }
    // Conditional compare FCCMP/FCCMPE.
    if op & 0xffe00c10 == 0x1e200400 {
        return Some(FpCCmp {
            sub: op & 0x00000010 != 0, // FCCMPE
            cond: (op >> 12) & 0xf,
            nzcv: op & 0xf,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
        });
    }
    // Conditional select FCSEL.
    if op & 0xffe00c00 == 0x1e200c00 {
        return Some(FpSel {
            rm: (op >> 16) & 31,
            cond: (op >> 12) & 0xf,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Immediate FMOV Sd/Dd/Hd, #imm8. Layout (verified by clang):
    // bits[20:13]=imm8, bits[23:22]=type (0=S,1=D,2=H), fixed bit21=1,
    // bit12=1, bits[11:8]=0, bits[7:5]=0; byte3 = 0x1e.
    if op & 0xff201fe0 == 0x1e201000 && ((op >> 22) & 3) != 2 {
        return Some(FpImm {
            imm8: (op >> 13) & 0xff,
            typ: (op >> 22) & 3,
            rd: op & 31,
        });
    }
    // GPR <-> V conversion / move.
    if op & 0x7f000000 == 0x1e000000
        && op & 0x00200000 != 0
        && op & 0x0000fc00 == 0
        && ((op >> 22) & 3) != 2
    {
        let opc = (op >> 16) & 0x1f;
        if matches!(opc, 0..=10 | 16..=17 | 24..=25) {
            return Some(FpGprConv {
                sf: (op >> 31) & 1,
                typ: (op >> 22) & 3,
                opc,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }
    // Fixed-point <-> FP conversion.
    if (op & 0x7fbe0000 == 0x1e020000 || op & 0x7fbe0000 == 0x1e180000) && ((op >> 22) & 3) <= 1 {
        return Some(FpFixed {
            sf: (op >> 31) & 1,
            typ: (op >> 22) & 1,
            u: (op >> 16) & 1 != 0,
            to_fp: (op & 0x00020000) == 0,
            imm6: (op >> 10) & 0x3f,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Fixed-point <-> FP conversion (FP register source).
    if op & 0xff000000 == 0x5f000000 || op & 0xff000000 == 0x7f000000 {
        let to_fp = ((op >> 10) & 3) == 1;
        return Some(FpFixed {
            sf: 0,
            typ: (op >> 22) & 1,
            u: (op & 0x20000000) != 0,
            to_fp,
            imm6: (op >> 16) & 0x3f,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // AdvSIMD scalar float<->int conversion, V-vector-register destination.
    // Byte3 0x5E (signed FCVTZS/SCVTF) or 0x7E (unsigned FCVTZU/UCVTF);
    // bit23 = 1 for FP->int (FCVTZ*), 0 for int->FP (SCVTF/UCVTF);
    // byte1 bits[15:10] = 0b101110 (FP->int) or 0b110110 (int->FP).
    {
        let b3 = (op >> 24) & 0xff;
        if (b3 == 0x5e || b3 == 0x7e)
            && op & 0x00200000 != 0
            && op & 0x00060000 == 0
            && op & 0x00010000 != 0
        {
            let opc = (op >> 10) & 0x3f;
            if opc == 0b101110 || opc == 0b110110 {
                let prec = match ((op >> 22) & 1, (op >> 20) & 1, (op >> 19) & 1) {
                    (0, 0, 0) => 0, // FP32
                    (1, 0, 0) => 1, // FP64
                    (1, 1, 1) => 2, // FP16
                    _ => 0xff,
                };
                if prec != 0xff {
                    return Some(FpRegConv {
                        unsigned: b3 == 0x7e,
                        to_fp: (op >> 23) & 1 == 0,
                        prec,
                        rn: (op >> 5) & 31,
                        rd: op & 31,
                    });
                }
            }
        }
    }
    // Two-source arithmetic. opc = bits[15:12]; fixed bits: bit11=1,
    // bits[10:8]=0 (byte1 pattern 0x08 | opc<<4 covers FMUL..FNMUL).
    if op & 0xff200f00 == 0x1e200800 && ((op >> 22) & 3) <= 1 && ((op >> 12) & 0xf) <= 8 {
        return Some(FpTwoSrc {
            opc: (op >> 12) & 0xf,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // ARMv8 cryptographic extension: AESE/AESD/AESMC/AESIMC, all operating
    // on a full 128-bit Advanced SIMD register. The opcode is bits[15:12].
    if op & 0x0f3e0c00 == 0x0e280800 {
        let aes_op = (op >> 12) & 0xf;
        if (4..=7).contains(&aes_op) {
            return Some(Aes {
                op: aes_op,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }
    // ARMv8 SHA-256: round update (H/H2) and schedule update (SU0/SU1).
    if op & 0xff20fc00 == 0x5e004000 || op & 0xff20fc00 == 0x5e005000 {
        let op_kind = if op & 0x0000_1000 == 0 { 0 } else { 1 };
        return Some(Sha256 {
            op: op_kind,
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
            rd: op & 31,
        });
    }
    if op & 0xffff_fc00 == 0x5e28_2800 {
        return Some(Sha256 {
            op: 2, // SHA256SU0
            rn: (op >> 5) & 31,
            rm: 0,
            rd: op & 31,
        });
    }
    if op & 0xffe0_fc00 == 0x5e00_6000 {
        return Some(Sha256 {
            op: 3, // SHA256SU1
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
            rd: op & 31,
        });
    }
    // AdvSIMD immediate shifts. immh:immb encodes the element width and
    // shift. Model the unsigned-right and left-shift forms used by the ROM.
    let shift_pattern = op & 0x9f80_fc00;
    if shift_pattern == 0x0f00_0400 || shift_pattern == 0x0f00_5400 {
        let encoded = (op >> 16) & 0x7f;
        if encoded != 0 {
            let top = 31 - encoded.leading_zeros();
            if (3..=6).contains(&top) {
                let width = 1u32 << top;
                let right = shift_pattern == 0x0f00_0400;
                let unsigned = op & (1 << 29) != 0;
                if (right && unsigned) || (!right && !unsigned) {
                    let amount = if right {
                        2 * width - encoded
                    } else {
                        encoded - width
                    };
                    return Some(NeonShiftImmediate {
                        q: op & (1 << 30) != 0,
                        size: top - 3,
                        right,
                        amount,
                        rn: (op >> 5) & 31,
                        rd: op & 31,
                    });
                }
            }
        }
    }
    // AdvSIMD shift-long: SSHLL/USHLL (and SSHLL2/USHLL2). The destination is
    // always 128 bits wide with doubled element size; bit 30 (Q) selects the
    // high half of the 128-bit source rather than a 64-bit destination. immh
    // (the 4-bit source element size field, 0001/0010/0100) selects 8/16/32-bit
    // lanes; the 3-bit immb field is the shift amount directly.
    if op & 0x9f80_fc00 == 0x0f00_a400 {
        let encoded = (op >> 16) & 0x7f;
        let immh = encoded >> 3;
        if immh == 1 || immh == 2 || immh == 4 {
            return Some(NeonShiftLong {
                q: op & (1 << 30) != 0,
                unsigned: op & (1 << 29) != 0,
                src_size: 8 * immh,
                amount: encoded & 0x7,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }
    // AdvSIMD shift by register: USHL (bit29=1, shift left; negative count
    // shifts right/zero-fills) and SSHL (bit29=0, signed). Clang-verified
    // element sizes: (b23,b22) = (1,0) -> 32-bit, (1,1) -> 64-bit,
    // (0,1) -> 16-bit; bit28 = 1 selects the scalar D-register form. The
    // class bit is bits[15:10] = 010001 with bit21 set.
    if (op & 0x0f00_0000) == 0x0e00_0000 && (op >> 10) & 0x3f == 0b010001 && op & 0x0020_0000 != 0 {
        let size = match ((op >> 23) & 1, (op >> 22) & 1) {
            (1, 0) => Some(0), // 32-bit
            (1, 1) => Some(1), // 64-bit
            (0, 1) => Some(2), // 16-bit
            _ => None,
        };
        if let Some(size) = size {
            let scalar = op & 0x1000_0000 != 0;
            return Some(NeonShiftReg {
                q: op & (1 << 30) != 0,
                unsigned: op & (1 << 29) != 0,
                size,
                scalar,
                rn: (op >> 5) & 31,
                rm: (op >> 16) & 31,
                rd: op & 31,
            });
        }
    }
    // AdvSIMD UMOV (element extract to GPR). byte1 locked to 0x3c,
    // bits23:22 clear. The element index field bits[20:16] encodes
    // size-specific markers plus the lane index:
    //   .b: value = 2*index+1 (odd, bit16 always set), Q clear
    //   .h: value = 2*index+2 (bit17 set), Q clear
    //   .s: value = 4*index+4 (bit18 set), Q clear
    //   .d: value = 8*index+8 (bit19 set), Q set
    if (op & 0x0f00_0000) == 0x0e00_0000 && op & 0x0000_ff00 == 0x0000_3c00 && op & 0x00c0_0000 == 0
    {
        let q = op & (1 << 30) != 0;
        if op & 0x0001_0000 != 0 && !q {
            // .b: odd encoding, index = (enc-1)/2 = enc>>1
            return Some(NeonUmov {
                size: 2,
                index: ((op >> 16) & 0x1f) >> 1,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
        if op & 0x0004_0000 != 0 && !q {
            return Some(NeonUmov {
                size: 0,
                index: (op >> 19) & 3,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
        if op & 0x0004_0000 == 0 && op & 0x0008_0000 != 0 && q {
            return Some(NeonUmov {
                size: 1,
                index: (op >> 20) & 1,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
    }
    // Reverse elements within each 64-bit group.
    if op & 0xbf3f_fc00 == 0x0e20_0800 {
        let size = (op >> 22) & 3;
        let q = op & (1 << 30) != 0;
        return Some(NeonRev64 {
            q,
            size,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    if op & 0xbf3ffc00 == 0x2e31a800 {
        let size = (op >> 22) & 3;
        let q = op & (1 << 30) != 0;
        if size < 2 || (size == 2 && q) {
            return Some(NeonUminv {
                q,
                size,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
        return None;
    }
    if op & 0xbf3ffc00 == 0x2e30a800 {
        let size = (op >> 22) & 3;
        let q = op & (1 << 30) != 0;
        if size < 2 || (size == 2 && q) {
            return Some(NeonUmaxv {
                q,
                size,
                rn: (op >> 5) & 31,
                rd: op & 31,
            });
        }
        return None;
    }
    // SADDLV/UADDLV: the vector is summed into one scalar. Constraint image
    // is bits[27:24] = 1110, bit 23 clear, bits[21:16] = 110000, and
    // bits[15:8] = 0b001110xx (op6 = 0x0E); U selects signed/unsigned, and
    // bit 22 selects 8-bit (0) vs 16-bit (1) lanes.
    if op & 0x1000_0000 == 0
        && op & 0x0f00_0000 == 0x0e00_0000
        && (op >> 16) & 0x80 == 0
        && (op >> 16) & 0x3f == 0x30
        && (op >> 8) & 0xfe == 0x38
    {
        let size = (op >> 22) & 1;
        return Some(NeonAddLv {
            q: op & (1 << 30) != 0,
            size,
            signed: op & (1 << 29) == 0,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // MOVI Dd / Vd.2D: each immediate bit expands into one 0xff byte.
    if op & 0xbff8fc00 == 0x2f00e400 {
        return Some(NeonMovi64 {
            q: op & (1 << 30) != 0,
            imm: ((((op >> 16) & 7) << 5) | ((op >> 5) & 31)) as u8,
            rd: op & 31,
        });
    }
    // EXT extracts a byte window from the concatenated active vectors.
    if op & 0xbfe08400 == 0x2e000000 {
        let q = op & (1 << 30) != 0;
        let offset = (op >> 11) & 15;
        if !q && offset >= 8 {
            return None;
        }
        return Some(NeonExt {
            q,
            offset,
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
            rd: op & 31,
        });
    }
    // Integer MOVI modified immediates (B/H/S); odd cmodes below 12
    // encode ORR, and cmode 15 is floating-point, not MOVI.
    if op & 0xbff80c00 == 0x0f000400 {
        let cmode = (op >> 12) & 15;
        let imm = (((op >> 16) & 7) << 5 | ((op >> 5) & 31)) as u64;
        let (bits, value) = match cmode {
            0 | 2 | 4 | 6 => (32, imm << (8 * (cmode >> 1))),
            8 | 10 => (16, imm << (8 * ((cmode >> 1) & 1))),
            12 => (32, (imm << 8) | 0xff),
            13 => (32, (imm << 16) | 0xffff),
            14 => (8, imm),
            _ => return None,
        };
        return Some(NeonMovi {
            q: op & (1 << 30) != 0,
            bits,
            value,
            rd: op & 31,
        });
    }
    // DUP (general register) has imm4=1, unlike DUP (element), imm4=0.
    if op & 0xbfe0fc00 == 0x0e000c00 {
        let imm5 = (op >> 16) & 31;
        let q = op & (1 << 30) != 0;
        if !imm5.is_power_of_two() || imm5 > 8 || (imm5 == 8 && !q) {
            return None;
        }
        return Some(NeonDupGeneral {
            q,
            size: imm5.trailing_zeros(),
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // Neon Copy / DUP / SMOV / UMOV / INS.
    if op & 0x9f000000 == 0x0e000000 && op & 0x00200000 == 0 && op & 0xc000 == 0 && op & 0x0400 != 0
    {
        let q = (op >> 30) & 1 != 0;
        let imm5 = (op >> 16) & 0x1f;
        let size = if imm5 == 0 { 0 } else { imm5.trailing_zeros() };
        let index = if size < 5 { imm5 >> (size + 1) } else { 0 };
        return Some(NeonCopy {
            q,
            op: (op >> 12) & 3,
            size,
            index,
            elem: (op & 0x0400) != 0,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // -----------------------------------------------------------------------
    // Advanced SIMD (NEON). Base prefix bits[31:24] = `0 Q U 0 1110` for the
    // three-register data-processing families: (op & 0x9f000000) == 0x0e000000.
    // bit30 = Q (128-bit), bit29 = U, size = bits[23:22], opcode = bits[15:11],
    //     Rm = bits[20:16], Rn = bits[9:5], Rd = bits[4:0].
    // -----------------------------------------------------------------------
    // ARMv8.2 dot product, vector forms (SDOT/UDOT Vd.4S,Vn.16B,Vm.16B and
    // Vd.2S,Vn.8B,Vm.8B). The opcode field bits[15:11] == 0b10010 collides
    // with the MLA/MLS family inside the three-register block below, so dot
    // must be decoded first. bit21 == 0 is the discriminator (MLA sets it),
    // giving the clang-exact mask/value: sdot v0.4s,v1.16b,v2.16b = 0x4e829420
    // decodes here while mla v0.4s,v1.4s,v2.4s = 0x4ea29420 does not match.
    if op & 0x9fe9_fe9a == 0x0e80_9400 {
        return Some(Dot {
            unsigned: op & 0x2000_0000 != 0,
            q: op & 0x4000_0000 != 0,
            elem: None,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    if op & 0x9f000000 == 0x0e000000 {
        let q = op & 0x4000_0000 != 0;
        let u = op & 0x2000_0000 != 0;
        let size = (op >> 22) & 3;
        let rm = (op >> 16) & 31;
        let opc = (op >> 11) & 0x1f;
        let rn = (op >> 5) & 31;
        let rd = op & 31;
        // Reverse/bit-count one-source group (opc == 0b0xx11 with bit10
        // distinguishing the fixed `0 1 0 1 1 0`/`0 0 0 1 1 0`/`0 0 0 0 1 0`
        // six-bit operations from the bitwise group). CNT is U=0 with
        // op6 == 0b010110; RBIT takes the same op6 with U=1 but the size=0
        // form is NOT (already decoded below), so only size != 0 is RBIT;
        // REV16 is U=0 op6 == 0b000110; REV32 is U=1 op6 == 0b000010.
        let op6 = (op >> 10) & 0x3f;
        if let Some(onesrc) = match op6 {
            0b010110 if !u => Some(NeonOneSrcOp::ByteCount),
            0b010110 if u && size != 0 => Some(NeonOneSrcOp::Rbit),
            0b000110 if !u => Some(NeonOneSrcOp::Rev16),
            0b000010 if u => Some(NeonOneSrcOp::Rev32),
            _ => None,
        } {
            return Some(NeonOneSrc {
                q,
                op: onesrc,
                size,
                rn,
                rd,
            });
        }
        // Bitwise operations: opcode bits[15:11] == 00011.
        if opc == 0b00011 {
            if let Some(bop) = neon_bitwise_op(u, size) {
                return Some(NeonBitwise {
                    q,
                    op: bop,
                    rm,
                    rn,
                    rd,
                });
            }
        }
        // Integer add/sub: opcode == 10000, U selects ADD/SUB.
        if opc == 0b10000 {
            return Some(NeonThreeSame {
                q,
                op: if u {
                    NeonThreeSameOp::Sub
                } else {
                    NeonThreeSameOp::Add
                },
                size,
                rm,
                rn,
                rd,
            });
        }
        // Integer multiply-accumulate: opcode == 10010 (MLA/MLS).
        if opc == 0b10010 {
            return Some(NeonThreeSame {
                q,
                op: if u {
                    NeonThreeSameOp::Mls
                } else {
                    NeonThreeSameOp::Mla
                },
                size,
                rm,
                rn,
                rd,
            });
        }
        if matches!(opc, 0b00110 | 0b10001) && u && (size != 3 || q) {
            return Some(NeonThreeSame {
                q,
                op: if opc == 0b00110 {
                    NeonThreeSameOp::Cmhi
                } else {
                    NeonThreeSameOp::Cmeq
                },
                size,
                rm,
                rn,
                rd,
            });
        }
        // Integer multiply: opcode == 10011.
        if opc == 0b10011 {
            return Some(NeonThreeSame {
                q,
                op: NeonThreeSameOp::Mul,
                size,
                rm,
                rn,
                rd,
            });
        }
        // One-source: abs/neg (opcode == 10111) and not/mvn (opcode == 01011).
        if opc == 0b10111 {
            return Some(NeonOneSrc {
                q,
                op: if u {
                    NeonOneSrcOp::Neg
                } else {
                    NeonOneSrcOp::Abs
                },
                size,
                rn,
                rd,
            });
        }
        if opc == 0b01011 && u {
            return Some(NeonOneSrc {
                q,
                op: NeonOneSrcOp::Not,
                size,
                rn,
                rd,
            });
        }
        // Vector FP one-source: opcodes 11111 with Rm == 0 (FABS/FNEG) or
        // Rm == 1 (FSQRT). Rm is architecturally a fixed control field, so only
        // the 0/1 values land here; larger Rm values are FDIV (three-same).
        // size bit22 selects S (0) or D (1) lanes.
        if opc == 0b11111 && rm <= 1 {
            return Some(NeonFpOneSrc {
                q,
                fneg: u,
                sqrt: rm == 1,
                size,
                rn,
                rd,
            });
        }
        // Vector FP three-register: FADD/FSUB/FMUL/FDIV/FMAX/FMIN/FMAXNM/
        // FMINNM/FMLA/FMLS. Dispatched by (U, size, opcode).
        if let Some(fop) = NeonThreeSameOp::fp_decode(u, size, opc) {
            return Some(NeonThreeSame {
                q,
                op: fop,
                size,
                rm,
                rn,
                rd,
            });
        }
    }

    // ── ARMv8.2 Dot Product (indexed) ───────────────────────────────────
    // SDOT / UDOT by element (Vd.4S, Vn.16B, Vm.4B[lane]): decoded outside
    // the 0x0e000000 family block because bits[31:24] are 0x0f/0x6f here
    // (bit28 set, unlike the three-register family). Pattern:
    // op & 0x9f00f400 == 0x0f00e000; Q=bit30, U=bit29, Rm=bits[20:16],
    // lane = (bit11 << 1) | bit21, Rn=bits[9:5], Rd=bits[4:0].
    // Clang: sdot v5.4s,v10.16b,v20.4b[1] = 0x4fb4e145,
    //        udot v5.4s,v10.16b,v20.4b[3] = 0x6fb4e945.
    if op & 0x9f00f400 == 0x0f00e000 {
        let q = (op >> 30) & 1 == 1;
        let u = (op >> 29) & 1 == 1;
        let lane = ((op >> 11) & 1) << 1 | (op >> 21) & 1;
        return Some(Dot {
            unsigned: u,
            q,
            elem: Some(lane),
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // ── ARMv8.3 FCADD / FCMLA ───────────────────────────────────────────
    // FCADD: op & 0xbf20e400 == 0x2e00e400, rot in bit12.
    if op & 0xbf20e400 == 0x2e00e400 {
        let q = (op >> 30) & 1 == 1;
        let sz = (op >> 22) & 1 == 1; // 0=S lanes, 1=D lanes
        let rot = ((op >> 12) & 1) * 2 + 1; // bit12=0 → 90°, bit12=1 → 270°
        return Some(FcComplexArith {
            is_cmla: false,
            sz,
            q,
            rot,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }
    // FCMLA: op & 0xbf20f400 == 0x2e00c400, rot in bits[13:12].
    if op & 0xbf20f400 == 0x2e00c400 {
        let q = (op >> 30) & 1 == 1;
        let sz = (op >> 22) & 1 == 1;
        let rot = (op >> 12) & 3;
        return Some(FcComplexArith {
            is_cmla: true,
            sz,
            q,
            rot,
            rm: (op >> 16) & 31,
            rn: (op >> 5) & 31,
            rd: op & 31,
        });
    }

    // ═══════════════════════════════════════════════════════════════════
    // Phase 190 SVE (Armv8.2, VL=128). All encodings below were verified
    // against llvm-mc (`-triple=aarch64 -mattr=+sve -show-encoding`).
    // The SVE space is top-byte 0x25 (>incl. while/incp/decp), 0x04 (cnt*),
    // or 0xa4/0xa5/0xe4/0xe5 (contiguous ld1/st1).
    // ═══════════════════════════════════════════════════════════════════

    // ── SVE contiguous LD1/ST1 (scalar base, MUL VL imm) ────────────────
    // byte2's high nibble (bits[23:20]) is fixed per form; bits[19:16] are
    // the signed imm4 offset, scaled by the vector length (VL = 16 bytes at
    // the 128-bit VL this core implements) — verified via `ld1d {z0.d},
    // p0/z, [x2, #1, MUL VL]` = 0xa5e1a040. size lives at bits[22:21]:
    // B=00, H=01, W=10, D=11 (1/2/4/8 bytes per element). Prefixes were read
    // straight off llvm-mc output for B/H/W/D forms.
    if (op & 0xfff00000) == 0xa4000000
        || (op & 0xfff00000) == 0xa4a00000
        || (op & 0xfff00000) == 0xa5400000
        || (op & 0xfff00000) == 0xa5e00000
    {
        let size = match op & 0xfff00000 {
            0xa4000000 => 0,
            0xa4a00000 => 1,
            0xa5400000 => 2,
            _ => 3,
        };
        let imm4 = ((op >> 16) & 0xf) as i32;
        let imm = if imm4 & 8 != 0 {
            (imm4 | !0xf) as i64
        } else {
            imm4 as i64
        };
        return Some(SveLd1 {
            size,
            imm,
            pg: (op >> 10) & 7,
            rn: (op >> 5) & 31,
            zt: op & 31,
        });
    }
    if (op & 0xfff00000) == 0xe4000000
        || (op & 0xfff00000) == 0xe4a00000
        || (op & 0xfff00000) == 0xe5400000
        || (op & 0xfff00000) == 0xe5e00000
    {
        let size = match op & 0xfff00000 {
            0xe4000000 => 0,
            0xe4a00000 => 1,
            0xe5400000 => 2,
            _ => 3,
        };
        let imm4 = ((op >> 16) & 0xf) as i32;
        let imm = if imm4 & 8 != 0 {
            (imm4 | !0xf) as i64
        } else {
            imm4 as i64
        };
        return Some(SveSt1 {
            size,
            imm,
            pg: (op >> 10) & 7,
            rn: (op >> 5) & 31,
            zt: op & 31,
        });
    }

    // ── SVE CNTB/CNTH/CNTW/CNTD ─────────────────────────────────────────
    // Fixed: [31:24]=0x04, bit21=1, bit20=0, bits[15:10]=111000.
    if op & 0xff30fc00 == 0x0420e000 {
        let pattern = (op >> 5) & 0x1f;
        return Some(SveCnt {
            size: (op >> 22) & 3,
            pattern,
            // imm4 (bits[19:16]); the architecture uses imm = UInt(imm4)+1,
            // verified via `cntd xzr, all, mul #4` = 0x04e3e3ff (imm4=3).
            imm: ((op >> 16) & 0xf) + 1,
            rd: op & 31,
        });
    }

    // ── SVE WHILE {lo|ls|lt|le} ─────────────────────────────────────────
    // Fixed: [31:24]=0x25, bit21=1, bit20=0, bits[15:13]=000, bit10=1.
    // `whilelo p0.d, x0, x1`   0x25e11c00  bit12(sf)=1 bit11(u)=1  eq(bit4)=0
    // `whilelt p0.d, x0, x1`   0x25e11400  bit12(sf)=1 bit11(u)=0  eq=0
    // `whilels p0.d, x0, x1`   0x25e11c10  eq=1
    // `whilele p0.d, x0, x1`   0x25e11410  bit11=0 eq=1
    if op & 0xff20c400 == 0x25200400 {
        return Some(SveWhile {
            size: (op >> 22) & 3,
            sf: op & (1 << 12) != 0,
            unsigned: op & (1 << 11) != 0,
            le: op & (1 << 4) != 0,
            rn: (op >> 5) & 31,
            rm: (op >> 16) & 31,
            pd: op & 0xf,
        });
    }

    // ── SVE INCP / DECP (scalar <Xd> and vector <Zdn> forms) ────────────
    // Fixed: [31:24]=0x25, bit21=1, bit20=0, bit19=1, bit18=1, bit17=0,
    // bit15=1, bit14=0, bit13=0, bit12=0, bit10=0, bit9=0.
    // incp scalar = 0x25ecc800-family bit16=0; decp scalar bit16=1.
    // vector forms carry bit11=0 (incp z4.d,p4.d = 0x25ec8084).
    if op & 0xff3ef600 == 0x252c8000 {
        return Some(SveIncp {
            dec: op & (1 << 16) != 0,
            // scalar form has bit11=1; vector form bit11=0.
            scalar: op & (1 << 11) != 0,
            size: (op >> 22) & 3,
            pg: (op >> 5) & 0xf,
            rd: op & 31,
        });
    }

    // ── SVE Predicate logicals (byte-granular AND/ORR/EOR/BIC family) ─────
    // Fixed: [31:24]=0x25, bit21=0, bit20=0, bit15=0, bit14=1.
    // Function bits: bit23 = orr-class (op), bit22 = S, bit9 = o2, bit4 = o3;
    // Pg at bits[13:10], Pm at bits[19:16], Pn at bits[8:5], Pd at bits[3:0].
    // The `not`/`mov` aliases are just EOR-with-Pm=Pg / AND-with-Pm=Pn and
    // fall out of the function decode, so no special casing. brka/brkb
    // (bit20=1), ptrue/pfalse/rdffrs (bit20=1), integer-compare-imm (bit14=0)
    // and WHILE/INCP (bit21=1) are all excluded by the mask.
    if op & 0xff30c000 == 0x25004000 {
        let op0 = (op >> 23) & 1;
        let set = op & (1 << 22) != 0;
        let o2 = (op >> 9) & 1;
        let o3 = (op >> 4) & 1;
        // func encodes the boolean operation over source predicates:
        // class 0 (&): AND/EOR/BIC ; class 1 (|): ORR/NOR/ORN/NAND.
        let func = (op0 << 2) | (o2 << 1) | o3;
        return Some(SvePredLogical {
            func,
            set,
            pd: op & 0xf,
            pg: (op >> 10) & 0xf,
            pn: (op >> 5) & 0xf,
            pm: (op >> 16) & 0xf,
        });
    }

    None
}

/// Phase 190 AMU EL0 gate: AMEVCNTRn reads from EL0 are only legal when
/// AMUSERENR_EL0 permits (EN/bit0 gates group 0, EN2/bit2 gates group 1).
/// Every other AMU/SPE register is EL1-or-higher and never reaches here (the
/// min_el check in the dispatch traps those first), so this returns true for
/// them.
fn amu_counter_el0_gate(m: &mut Machine, r: iphem_core::sysreg::SysReg) -> bool {
    use iphem_core::sysreg::SysReg as R;
    match r {
        R::AmevcntrEl0(_) => (m.cores.active().state.amuseren & 0x1) != 0,
        R::Amevcntr1El0(_) => (m.cores.active().state.amuseren & 0x4) != 0,
        _ => true,
    }
}

fn read_sysreg(m: &mut Machine, r: iphem_core::sysreg::SysReg) -> u64 {
    use iphem_core::sysreg::SysReg as R;
    let s = &m.cores.active().state;
    match r {
        R::MidrEl1
        | R::MpidrEl1
        | R::RevidrEl1
        | R::CtrEl0
        | R::ClidrEl1
        | R::CcsidrEl1
        | R::Pfr0El1
        | R::Pfr1El1
        | R::Mmfr0El1
        | R::Mmfr1El1
        | R::Isar0El1
        | R::Isar1El1 => {
            let k = match r {
                R::MidrEl1 => 0,
                R::MpidrEl1 => 1,
                R::RevidrEl1 => 2,
                R::CtrEl0 => 3,
                R::ClidrEl1 => 4,
                R::CcsidrEl1 => 5,
                R::Pfr0El1 => 6,
                R::Pfr1El1 => 7,
                R::Mmfr0El1 => 8,
                R::Mmfr1El1 => 9,
                R::Isar0El1 => 10,
                R::Isar1El1 => 11,
                _ => unreachable!(),
            };
            s.vendor_sysregs
                .get(&k)
                .copied()
                .unwrap_or_else(|| iphem_core::sysreg::id_value(r))
        }
        R::CntvctEl0 => m.scheduler.cycle().wrapping_sub(s.cntvoff_el2),
        R::CntpctEl0 | R::CntpctssEl0 => m.scheduler.cycle().wrapping_sub(s.cntpoff_el2),
        R::CntpCvalEl0 => s.cntp_cval_el0,
        R::CntpCtlEl0 => {
            // ISTATUS (bit 2) reflects the comparator output only while the
            // timer is enabled (Enable = bit 0), matching the interrupt
            // delivery gate in Machine::tick_devices. A disabled timer must
            // not report a pending condition. The physical counter is the
            // global cycle counter offset by CNTP_OFF_EL2.
            let physical_counter = m.scheduler.cycle().wrapping_sub(s.cntpoff_el2);
            let expired =
                s.cntp_ctl_el0 & 1 != 0 && (physical_counter as i64 - s.cntp_cval_el0 as i64) >= 0;
            s.cntp_ctl_el0 | (u64::from(expired) << 2)
        }
        R::CntpTvalEl0 => s
            .cntp_cval_el0
            .wrapping_sub(m.scheduler.cycle().wrapping_sub(s.cntpoff_el2))
            as u32 as u64,
        R::CntvCvalEl0 => s.cntv_cval_el0,
        R::CntvCtlEl0 => {
            // Same Enable gate for the virtual timer's ISTATUS bit.
            let virtual_counter = m.scheduler.cycle().wrapping_sub(s.cntvoff_el2);
            let expired =
                s.cntv_ctl_el0 & 1 != 0 && (virtual_counter as i64 - s.cntv_cval_el0 as i64) >= 0;
            s.cntv_ctl_el0 | (u64::from(expired) << 2)
        }
        R::CntvTvalEl0 => s
            .cntv_cval_el0
            .wrapping_sub(m.scheduler.cycle().wrapping_sub(s.cntvoff_el2))
            as u32 as u64,
        R::CntvoffEl2 => s.cntvoff_el2,
        R::CntpoffEl2 => s.cntpoff_el2,
        R::CntfrqEl0 => s.vendor_sysregs.get(&12u16).copied().unwrap_or(0x16e3600),
        R::DczidEl0 => s.vendor_sysregs.get(&0xd807).copied().unwrap_or(4),
        R::TpidrroEl0 => s.tpidrro_el0,
        R::TpidrEl0 => s.tpidr_el0,
        R::TpidrEl1 => s.tpidr_el1,
        R::SctlrEl1 => s.sctlr_el1,
        R::SctlrEl2 => s.sctlr_el2,
        R::SctlrEl3 => s.sctlr_el3,
        R::Ttbr0El1 => s.ttbr0_el1,
        R::Ttbr1El1 => s.ttbr1_el1,
        R::TcrEl1 => s.tcr_el1,
        R::MairEl1 => s.mair_el1,
        R::ParEl1 => s.par_el1,
        R::VbarEl1 => s.vbar_el1,
        R::EsrEl1 => s.esr_el1,
        R::FarEl1 => s.far_el1,
        R::SpsrEl1 => s.spsr_el1,
        R::ElrEl1 => s.elr_el1,
        R::VbarEl2 => s.vbar_el2,
        R::EsrEl2 => s.esr_el2,
        R::FarEl2 => s.far_el2,
        R::HpfarEl2 => s.hpfar_el2,
        R::SpsrEl2 => s.spsr_el2,
        R::ElrEl2 => s.elr_el2,
        R::HcrEl2 => s.hcr_el2,
        R::VttbrEl2 => s.vttbr_el2,
        R::VtcrEl2 => s.vtcr_el2,
        R::CptrEl2 => s.cptr_el2,
        R::VbarEl3 => s.vbar_el3,
        R::EsrEl3 => s.esr_el3,
        R::FarEl3 => s.far_el3,
        R::SpsrEl3 => s.spsr_el3,
        R::ElrEl3 => s.elr_el3,
        R::ScrEl3 => s.scr_el3,
        R::CptrEl3 => s.cptr_el3,
        R::SpEl0 => s.sp_el[0],
        R::SpEl1 => s.sp_el[1],
        R::SpEl2 => s.sp_el[2],
        R::Daif => (s.daif as u64) << 6,
        R::CurrentEl => (s.el as u64) << 2,
        R::Spsel => u64::from(s.spsel),
        R::Nzcv => (s.nzcv as u64) << 28,
        R::Fpcr => s.fpcr,
        R::Fpsr => s.fpsr,
        // PMU/AMU (Phase 163): cycle counter, simple enable-controlled counters.
        R::PmccntrEl0 => m.scheduler.cycle(),
        R::PmevcntrEl0(n) => s.pmev_cntr[n],
        R::PmevtyperEl0(n) => s.pmev_type[n],
        R::PmcrEl0 => s.pmcr,
        R::PmcntensetEl0 => s.pmcntenset,
        R::PmcntenclrEl0 => s.pmcntenclr,
        // AMU (Phase 190, real A12 map). Null/type/config/enable registers hold
        // raw state; the counters report a number-context value gated by the
        // census enable flags (AMCNTENSET0/1_EL0): a disabled counter reads zero,
        // an enabled one reads its offset base plus the shared scheduler cycle
        // counter. The EL0 AMUSERENR gate is enforced in the MRS/MSR dispatch.
        R::AmcrEl0 => 0,
        R::AmcfgrEl0 => 0,
        R::AmcggcrEl0 => 0,
        R::AmuserenEl0 => s.amuseren,
        R::AmcntensetEl0 | R::AmcntenclrEl0 => s.amcntenset,
        R::Amcntenset1El0 | R::Amcntenclr1El0 => s.amcntenset1,
        R::AmevcntrEl0(n) => {
            let enable = s.amcntenset & (1u64 << n);
            if enable != 0 {
                s.amcnt[n as usize].wrapping_add(m.scheduler.cycle())
            } else {
                0
            }
        }
        R::Amevcntr1El0(n) => {
            let enable = s.amcntenset1 & (1u64 << n);
            if enable != 0 {
                s.amcnt[4 + n as usize].wrapping_add(m.scheduler.cycle())
            } else {
                0
            }
        }
        R::AmevtyperEl0(_) | R::Amevtyper1El0(_) => 0,
        // SPE (Phase 190): profiling-buffer registers. Reads return raw state;
        // buffer records live in guest RAM and PMBSR status is updated by
        // polling/completion through `Machine::spe_buf_push`.
        R::PmbidrEl1 => s.spe_pmbidr,
        R::PmblimitrEl1 => s.spe_pmblimitr,
        R::PmbptrEl1 => s.spe_pmbptr,
        R::PmbsrEl1 => s.spe_pmbsr,
        // Apple IMPDEF S3_3_C15_C7_0: cache/DRAM-spec probe (see sysreg.rs).
        R::ImpCacheOp => s.imp_cache_op,
        // Apple APCTL_EL1 (S3_4_C15_C0_4) is the SEP cross-engine lock /
        // handshake register. The write side (below) raises bit 0, the
        // grant/ready ack SEPOS spins on, so the read is a plain view of the
        // stored value. The reset seed 0x42 carries the XNU early-bring-up
        // capability bits this register is probed for.
        R::ApctlEl1 => s.apctl_el1,
        // ARMv8.3 pointer-authentication keys (APIAKeyLo/APIAKeyHi/etc).
        R::ApiaLoEl1 => s.apia_lo,
        R::ApiaHiEl1 => s.apia_hi,
        R::ApibLoEl1 => s.apib_lo,
        R::ApibHiEl1 => s.apib_hi,
        R::ApdaLoEl1 => s.apda_lo,
        R::ApdaHiEl1 => s.apda_hi,
        R::ApdbLoEl1 => s.apdb_lo,
        R::ApdbHiEl1 => s.apdb_hi,
        // Keep generic keys in the existing serialized register map, avoiding
        // a change to the CPU snapshot layout.
        R::ApgaLoEl1 => *s.vendor_sysregs.get(&0xc118).unwrap_or(&0),
        R::ApgaHiEl1 => *s.vendor_sysregs.get(&0xc119).unwrap_or(&0),
        // MDSCR_EL1: debug is unimplemented, so the reset value (all features
        // off) is the honest read.
        R::MdscrEl1 => 0,
        // Phase 190 SVE vector-length control. VQ=0 (reset) means 128-bit.
        R::ZcrEl1 => s.zcr_el1,
        R::ZcrEl2 => s.zcr_el2,
    }
}

// Vendor SysReg HashMap key (stable encoding). Indexed variants don't go in
// the vendor map; only RO id-style registers are looked up there.
fn vendor_key(r: iphem_core::sysreg::SysReg) -> Option<u16> {
    use iphem_core::sysreg::SysReg as R;
    match r {
        R::MidrEl1 => Some(0),
        R::MpidrEl1 => Some(1),
        R::RevidrEl1 => Some(2),
        R::CtrEl0 => Some(3),
        R::ClidrEl1 => Some(4),
        R::CcsidrEl1 => Some(5),
        R::Pfr0El1 => Some(6),
        R::Pfr1El1 => Some(7),
        R::Mmfr0El1 => Some(8),
        R::Mmfr1El1 => Some(9),
        R::Isar0El1 => Some(10),
        R::Isar1El1 => Some(11),
        R::CntfrqEl0 => Some(12),
        R::ApctlEl1 => Some(0xe784),
        _ => None,
    }
}

fn write_sysreg(m: &mut Machine, r: iphem_core::sysreg::SysReg, v: u64) {
    use iphem_core::sysreg::SysReg as R;
    let counter = m.scheduler.cycle();
    let s = &mut m.cores.active_mut().state;
    match r {
        R::TpidrroEl0 => s.tpidrro_el0 = v,
        R::TpidrEl0 => s.tpidr_el0 = v,
        R::TpidrEl1 => s.tpidr_el1 = v,
        R::SctlrEl1 => s.sctlr_el1 = v,
        R::SctlrEl2 => s.sctlr_el2 = v,
        R::SctlrEl3 => s.sctlr_el3 = v,
        R::CntpTvalEl0 => {
            s.cntp_cval_el0 = counter.wrapping_add((v as u32 as i32 as i64) as u64);
        }
        R::CntpCtlEl0 => {
            s.cntp_ctl_el0 = v & 0x3;
        }
        R::CntpCvalEl0 => s.cntp_cval_el0 = v,
        R::CntvTvalEl0 => {
            let virtual_counter = counter.wrapping_sub(s.cntvoff_el2);
            s.cntv_cval_el0 = virtual_counter.wrapping_add((v as u32 as i32 as i64) as u64);
        }
        R::CntvCtlEl0 => s.cntv_ctl_el0 = v & 0x3,
        R::CntvCvalEl0 => s.cntv_cval_el0 = v,
        R::CntvoffEl2 => s.cntvoff_el2 = v,
        R::CntpoffEl2 => s.cntpoff_el2 = v,
        R::Ttbr0El1 => s.ttbr0_el1 = v,
        R::Ttbr1El1 => s.ttbr1_el1 = v,
        R::TcrEl1 => s.tcr_el1 = v,
        R::MairEl1 => s.mair_el1 = v,
        R::ParEl1 => s.par_el1 = v,
        R::VbarEl1 => s.vbar_el1 = v,
        R::EsrEl1 => s.esr_el1 = v,
        R::FarEl1 => s.far_el1 = v,
        R::SpsrEl1 => s.spsr_el1 = v,
        R::ElrEl1 => s.elr_el1 = v,
        R::VbarEl2 => s.vbar_el2 = v,
        R::EsrEl2 => s.esr_el2 = v,
        R::FarEl2 => s.far_el2 = v,
        R::HpfarEl2 => s.hpfar_el2 = v,
        R::SpsrEl2 => s.spsr_el2 = v,
        R::ElrEl2 => s.elr_el2 = v,
        R::HcrEl2 => s.hcr_el2 = v,
        R::VttbrEl2 => s.vttbr_el2 = v,
        R::VtcrEl2 => s.vtcr_el2 = v,
        R::CptrEl2 => s.cptr_el2 = v,
        R::VbarEl3 => s.vbar_el3 = v,
        R::EsrEl3 => s.esr_el3 = v,
        R::FarEl3 => s.far_el3 = v,
        R::SpsrEl3 => s.spsr_el3 = v,
        R::ElrEl3 => s.elr_el3 = v,
        R::ScrEl3 => s.scr_el3 = v,
        R::CptrEl3 => s.cptr_el3 = v,
        R::SpEl0 => {
            s.sp_el[0] = v;
            if !s.spsel {
                s.sp = v;
            }
        }
        R::SpEl1 => {
            s.sp_el[1] = v;
            if s.el == ExceptionLevel::El1 && s.spsel {
                s.sp = v;
            }
        }
        R::SpEl2 => {
            s.sp_el[2] = v;
            if s.el == ExceptionLevel::El2 && s.spsel {
                s.sp = v;
            }
        }
        R::Spsel => {
            bank_active_sp(s);
            select_sp(s, s.el, v & 1 != 0);
        }
        R::Daif => s.daif = ((v >> 6) & 0xf) as u8,
        R::Nzcv => set_flags(s, ((v >> 28) & 0xf) as u32),
        R::Fpcr => s.fpcr = v,
        R::Fpsr => s.fpsr = v,
        // PMU/AMU (Phase 163)
        R::PmccntrEl0 => { /* cycle counter is read-only */ }
        R::PmevcntrEl0(n) => s.pmev_cntr[n] = v,
        R::PmevtyperEl0(n) => s.pmev_type[n] = v,
        R::PmcrEl0 => s.pmcr = v,
        R::PmcntensetEl0 => s.pmcntenset |= v,
        R::PmcntenclrEl0 => s.pmcntenclr |= v,
        // AMU (Phase 190): W1S/W1C enable registers latch exactly the bits
        // written; both read back the live enable flags. The counters keep an
        // EL1-only writable offset base (a documented synthetic reset hook; the
        // real registers are read-only) so conformance tests can pin a
        // deterministic cycle-derived value. Read-only id/config regs and the
        // EL0 gate are enforced by the MRS/MSR dispatch + can_write.
        R::AmcrEl0 | R::AmcfgrEl0 | R::AmcggcrEl0 | R::AmevtyperEl0(_) | R::Amevtyper1El0(_) => {}
        R::AmuserenEl0 => s.amuseren = v & 0x7,
        R::AmcntensetEl0 => s.amcntenset |= v & 0x3,
        R::AmcntenclrEl0 => s.amcntenset &= !(v & 0x3),
        R::Amcntenset1El0 => s.amcntenset1 |= v,
        R::Amcntenclr1El0 => s.amcntenset1 &= !v,
        R::AmevcntrEl0(n) => s.amcnt[n as usize] = v,
        R::Amevcntr1El0(n) => s.amcnt[4 + n as usize] = v,
        // SPE (Phase 190). PMBLIMITR: E=bit0, FM=bits3:1, buffer base bits63:12.
        // PMBPTR holds the current record pointer (bits55:0). PMBSR status is
        // W1C for S(16)/DL(24); writing bit0 clears the EC field, and a
        // disabled buffer (E=0) forces the error code to zero.
        R::PmblimitrEl1 => s.spe_pmblimitr = v,
        R::PmbptrEl1 => s.spe_pmbptr = v,
        R::PmbsrEl1 => {
            let mut next = s.spe_pmbsr & !(v & ((1u64 << 16) | (1u64 << 24)));
            if v & 1 != 0 {
                next &= !0x1f;
            }
            if s.spe_pmblimitr & 1 == 0 {
                next &= !0x1f;
            }
            s.spe_pmbsr = next;
        }
        // Apple IMPDEF S3_3_C15_C7_0 completion handshake: bit 63 mirrors the
        // low-6-bit opcode (set for requests, cleared for empty/teardown), so
        // iBoot's "wait for bit 63 set" and "wait for bit 63 clear" spins both
        // terminate on the next read.
        R::ImpCacheOp => {
            let opcode = v & 0x3f;
            s.imp_cache_op = (v & !(1u64 << 63)) | if opcode != 0 { 1u64 << 63 } else { 0 };
        }
        // Apple APCTL_EL1 (S3_4_C15_C0_4) lock handshake: SEPOS writes a
        // nonzero token and then spins until bit 0 reads set. The modeled SEP
        // co-processor grants the request, so the write raises bit 0 and the
        // guest's next read observes the ack. A zero write releases the grant,
        // mirroring the symmetric handshake already used by ImpCacheOp and
        // letting a later request re-acquire. Bit 0 of an already-granted
        // value is preserved, so the register keeps a round-trip contract.
        R::ApctlEl1 => {
            let token = v & !1;
            s.apctl_el1 = token | u64::from(token != 0);
        }
        // ARMv8.3 pointer-authentication keys. Read/write from EL1 (or higher).
        R::ApiaLoEl1 => s.apia_lo = v,
        R::ApiaHiEl1 => s.apia_hi = v,
        R::ApibLoEl1 => s.apib_lo = v,
        R::ApibHiEl1 => s.apib_hi = v,
        R::ApdaLoEl1 => s.apda_lo = v,
        R::ApdaHiEl1 => s.apda_hi = v,
        R::ApdbLoEl1 => s.apdb_lo = v,
        R::ApdbHiEl1 => s.apdb_hi = v,
        R::ApgaLoEl1 => {
            s.vendor_sysregs.insert(0xc118, v);
        }
        R::ApgaHiEl1 => {
            s.vendor_sysregs.insert(0xc119, v);
        }
        // SVE ZCR_EL1/EL2: only the VQ field is retained (writes of VQ are
        // clamped to 0, i.e. a 128-bit vector length, this implementation's
        // only supported length).
        R::ZcrEl1 => s.zcr_el1 = v & 0xf,
        R::ZcrEl2 => s.zcr_el2 = v & 0xf,
        _ => {
            if let Some(k) = vendor_key(r) {
                s.vendor_sysregs.insert(k, v);
            }
        }
    }
    if matches!(
        r,
        R::SctlrEl1
            | R::Ttbr0El1
            | R::Ttbr1El1
            | R::TcrEl1
            | R::MairEl1
            | R::HcrEl2
            | R::VttbrEl2
            | R::VtcrEl2
    ) {
        m.tlb.invalidate_all();
    }
}

fn is_el1_vm_control(r: iphem_core::sysreg::SysReg) -> bool {
    use iphem_core::sysreg::SysReg as R;
    matches!(
        r,
        R::SctlrEl1 | R::Ttbr0El1 | R::Ttbr1El1 | R::TcrEl1 | R::MairEl1
    )
}

fn handle_sys(m: &mut Machine, pc: u64, op: u32) -> Result<(), ExecError> {
    use iphem_core::sysreg::SystemOp as SOp;
    let next = pc + 4;
    let so = iphem_core::sysreg::decode(op);
    match so {
        Some(SOp::Wfi) => {
            if matches!(
                m.cores.active_mut().state.el,
                ExceptionLevel::El0 | ExceptionLevel::El1
            ) && m.cores.active_mut().state.hcr_el2 & (1 << 13) != 0
            {
                return Err(exception_to(m, ExceptionLevel::El2, 0x01u64 << 26, 0));
            }
            // Architected WFI: with IRQs masked it is a hint; otherwise the
            // core parks until an interrupt wakes it.
            if m.cores.active_mut().state.daif & 0b0010 == 0 {
                m.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;
            }
        }
        Some(SOp::Wfe) => {
            if matches!(
                m.cores.active_mut().state.el,
                ExceptionLevel::El0 | ExceptionLevel::El1
            ) && m.cores.active_mut().state.hcr_el2 & (1 << 14) != 0
            {
                return Err(exception_to(m, ExceptionLevel::El2, (0x01u64 << 26) | 1, 0));
            }
            // WFE parks the core the same way (no event-wait accelerator yet).
            if m.cores.active_mut().state.daif & 0b0010 == 0 {
                m.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;
            }
        }
        Some(SOp::Hint) => {}
        Some(SOp::Clrex) => {
            m.cores.active_mut().exclusive_address = None;
            m.cores.active_mut().exclusive_size = 0;
        }
        Some(SOp::Barrier(_)) => {}
        Some(SOp::Cfinv) => {
            let s = &mut m.cores.active_mut().state;
            s.nzcv ^= 0x2;
            set_flags(s, s.nzcv);
        }
        Some(SOp::Pstate(kind)) => {
            // DAIFSet/DAIFClr are explicitly accessible at EL0. Other
            // PSTATE immediate writes remain privileged in this model.
            if m.cores.active_mut().state.el == ExceptionLevel::El0
                && !matches!(
                    kind,
                    iphem_core::sysreg::PstateOp::DaifSet(_)
                        | iphem_core::sysreg::PstateOp::DaifClr(_)
                )
            {
                return Err(ExecError::Undefined);
            }
            let s = &mut m.cores.active_mut().state;
            match kind {
                iphem_core::sysreg::PstateOp::DaifSet(imm) => s.daif |= imm,
                iphem_core::sysreg::PstateOp::DaifClr(imm) => s.daif &= !imm,
                iphem_core::sysreg::PstateOp::Spsel(sel) => {
                    bank_active_sp(s);
                    select_sp(s, s.el, sel == 1);
                }
                iphem_core::sysreg::PstateOp::Uao(value) => {
                    s.pstate = (s.pstate & !(1 << 23)) | (u64::from(value) << 23);
                }
                iphem_core::sysreg::PstateOp::Pan(value) => {
                    s.pstate = (s.pstate & !(1 << 22)) | (u64::from(value) << 22);
                    m.tlb.invalidate_all();
                }
            }
        }
        Some(SOp::Mrs(rt, key)) => {
            // Read keys have L=1; the table is authored for the read form.
            let r = iphem_core::sysreg::key(key).or_else(|| iphem_core::sysreg::key_indexed(key));
            match r {
                Some(r) => {
                    if m.cores.active_mut().state.el == ExceptionLevel::El1
                        && is_el1_vm_control(r)
                        && m.cores.active_mut().state.hcr_el2 & (1 << 30) != 0
                    {
                        return Err(exception_to(m, ExceptionLevel::El2, 0x18u64 << 26, 0));
                    }
                    if (m.cores.active_mut().state.el as u8) < iphem_core::sysreg::min_el(r) {
                        // An access to a register that is UNDEFINED at the current
                        // EL but defined at a higher EL traps as an UNDEFINED
                        // instruction exception to that higher EL (EC=0x00).
                        return Err(exception_to_at(
                            m,
                            sysreg_trap_el(iphem_core::sysreg::min_el(r)),
                            0x0,
                            0,
                            pc,
                        ));
                    }
                    if m.cores.active_mut().state.el == ExceptionLevel::El0
                        && !amu_counter_el0_gate(m, r)
                    {
                        // AMU counter read with the census enable bit (or EL0
                        // permission) off traps as UNDEFINED to EL1.
                        return Err(exception_to_at(m, ExceptionLevel::El1, 0x0, 0, pc));
                    }
                    let v = read_sysreg(m, r);
                    wr(&mut m.cores.active_mut().state, rt, v);
                }
                None => {
                    wr(&mut m.cores.active_mut().state, rt, 0);
                }
            }
        }
        Some(SOp::Msr(rt, key)) => {
            // MSR uses the same key as MRS (L=1 in key); the table is
            // authored for the canonical read form.
            let r = iphem_core::sysreg::key(key).or_else(|| iphem_core::sysreg::key_indexed(key));
            if let Some(r) = r {
                if m.cores.active_mut().state.el == ExceptionLevel::El1
                    && is_el1_vm_control(r)
                    && m.cores.active_mut().state.hcr_el2 & (1 << 26) != 0
                {
                    return Err(exception_to(m, ExceptionLevel::El2, 0x18u64 << 26, 0));
                }
                if (m.cores.active_mut().state.el as u8) < iphem_core::sysreg::min_el(r) {
                    return Err(exception_to_at(
                        m,
                        sysreg_trap_el(iphem_core::sysreg::min_el(r)),
                        0x0,
                        0,
                        pc,
                    ));
                }
                if m.cores.active_mut().state.el == ExceptionLevel::El0
                    && matches!(
                        r,
                        iphem_core::sysreg::SysReg::AmevcntrEl0(_)
                            | iphem_core::sysreg::SysReg::Amevcntr1El0(_)
                    )
                {
                    // An EL0 MSR to an AMU counter is architecturally
                    // UNDEFINED at EL0; per DDI 0487 it traps as an
                    // UNDEFINED-instruction exception (EC=0x00) to EL1
                    // rather than raising an emulator-level fault.
                    return Err(exception_to_at(m, ExceptionLevel::El1, 0x0, 0, pc));
                }
                if !iphem_core::sysreg::can_write(r, m.cores.active_mut().state.el as u8) {
                    return Err(ExecError::Undefined);
                }
                if m.cores.active_mut().state.el == ExceptionLevel::El0
                    && !amu_counter_el0_gate(m, r)
                {
                    // AMU counter write from EL0 is never legal.
                    return Err(exception_to_at(m, ExceptionLevel::El1, 0x0, 0, pc));
                }
                let v = reg(&m.cores.active().state, rt);
                write_sysreg(m, r, v);
            }
        }
        Some(SOp::Sys { read, imm: _, rt }) => {
            let crn = (op >> 12) & 0xf;
            let crm = (op >> 8) & 0xf;
            let op2 = (op >> 5) & 0x7;
            if m.cores.active_mut().state.el == ExceptionLevel::El0 {
                let is_dczva = !read && crn == 7 && crm == 4 && op2 == 1;
                let dczid = m
                    .cores
                    .active()
                    .state
                    .vendor_sysregs
                    .get(&0xd807)
                    .copied()
                    .unwrap_or(4);
                if !is_dczva
                    || m.cores.active_mut().state.sctlr_el1 & (1 << 14) == 0
                    || dczid & (1 << 4) != 0
                {
                    return Err(ExecError::Undefined);
                }
            }
            if read {
                // Read system instructions (e.g. Apple IMPDEF cache/core spec probes like S3_3_C15_C8_0)
                // return 0 in rt without faulting.
                wr(&mut m.cores.active_mut().state, rt, 0);
                return Ok(());
            }
            match crn {
                8 => {
                    // TLBI
                    let operand = reg(&m.cores.active().state, rt);
                    let va = (operand & 0x0000_0fff_ffff_ffff) << 12;
                    let asid = (operand >> 48) as u16;
                    let vmid = if m.cores.active_mut().state.hcr_el2 & 1 != 0 {
                        (m.cores.active_mut().state.vttbr_el2 >> 48) as u16
                    } else {
                        0
                    };
                    match (crm, op2) {
                        (7, 0) => m.tlb.invalidate_vmid(vmid), // VMALLE1
                        (7, 1 | 5) => m.tlb.invalidate_va_asid(va, asid, vmid), // VAE1/VALE1
                        (7, 2) => m.tlb.invalidate_asid(asid, vmid), // ASIDE1
                        (7, 3 | 7) => m.tlb.invalidate_va_vmid(va, vmid), // VAAE1/VAALE1
                        _ => m.tlb.invalidate_all(),
                    }
                }
                7 => match (crm, op2) {
                    (8, 0..=3) if (op >> 16) & 7 == 0 => {
                        // AT S1E1R/W and S1E0R/W. Translation faults are
                        // reported in PAR, not delivered as data aborts.
                        if m.cores.active_mut().state.el == ExceptionLevel::El1
                            && m.cores.active_mut().state.hcr_el2 & (1 << 44) != 0
                        {
                            return Err(exception_to_at(
                                m,
                                ExceptionLevel::El2,
                                0x18u64 << 26,
                                0,
                                pc,
                            ));
                        }
                        let mut query = m.cores.active().state.clone();
                        query.el = if op2 & 2 == 0 {
                            ExceptionLevel::El1
                        } else {
                            ExceptionLevel::El0
                        };
                        // Ordinary AT forms do not perform the PAN check
                        // (the separate S1E1RP/WP forms do).
                        query.pstate &= !(1 << 22);
                        let va = reg(&m.cores.active().state, rt);
                        m.cores.active_mut().state.par_el1 = match iphem_core::Mmu::translate_stage1(
                            &query,
                            &mut m.bus,
                            &mut m.trace,
                            va,
                            op2 & 1 != 0,
                        ) {
                            Ok((t, sh)) => {
                                let attr = if query.sctlr_el1 & 1 != 0 {
                                    (query.mair_el1 >> (8 * t.attr_index)) & 0xff
                                } else {
                                    0
                                }; // MMU-off data: Device-nGnRnE
                                (t.pa & 0x0000_ffff_ffff_f000)
                                    | (attr << 56)
                                    | (u64::from(sh) << 7)
                                    | (1 << 11)
                                    | ((query.scr_el3 & 1) << 9)
                            }
                            Err(fault) => {
                                use iphem_core::{MmuFault as F, TableWalkFaultKind as K};
                                let level = u64::from(fault.level());
                                let status = match fault {
                                    F::AddressSize { .. } => level,
                                    F::AccessFlag { .. } => 8 | level,
                                    F::Permission { .. } => 12 | level,
                                    F::Stage2TableWalk { kind, .. } => match kind {
                                        K::AddressSize => level,
                                        K::AccessFlag => 8 | level,
                                        K::Permission => 12 | level,
                                        _ => 4 | level,
                                    },
                                    _ => 4 | level,
                                };
                                1 | (status << 1)
                                    | (1 << 11)
                                    | (u64::from(fault.is_stage2_table_walk()) << 8)
                                    | (u64::from(fault.is_stage2()) << 9)
                            }
                        };
                    }
                    (4, 1) => {
                        // DC ZVA uses the block size reported by DCZID_EL0.BS
                        // and rounds the operand down to that block boundary.
                        let dczid = m
                            .cores
                            .active()
                            .state
                            .vendor_sysregs
                            .get(&0xd807)
                            .copied()
                            .unwrap_or(4);
                        let block_bytes = 4u64 << (dczid & 0xf);
                        let base = reg(&m.cores.active().state, rt) & !(block_bytes - 1);
                        let mut offset = 0;
                        while offset < block_bytes {
                            let size = (block_bytes - offset).min(8) as usize;
                            m.mem_write(base + offset, size, 0)
                                .map_err(|_| ExecError::Data)?;
                            offset += size as u64;
                        }
                    }
                    (11, _) | (14, _) | (5, _) => {} // CVAU/CIVAC/IVAU benign
                    _ => {}
                },
                _ => {}
            }
        }
        Some(SOp::Raw(_)) => {
            return Err(ExecError::Undefined);
        }
        None => {
            return Err(ExecError::Undefined);
        }
    }
    m.cores.active_mut().state.pc = next;
    Ok(())
}

// ========================================================================
// Pointer-authentication (ARMv8.3) helpers.
//
// The emulator keeps the same observable contract as real hardware:
//   * a signed pointer produced by PACIA/PACIB/PACDA/PACDB round-trips
//     through the matching AUTIA/... instruction back to the original,
//   * the strip (XPACI/XPACD/XPACLRI) and sign/auth operations agree on the
//     address extension produced by TCR_EL1 (checked against a real XNU
//     kernel dump in `xpac_restores_address_extension_and_preserves_tags`),
//   * authenticating a pointer signed with a different key or a corrupted
//     PAC faults.
// The signature itself is a deterministic 64-bit keyed permutation rather
// than the architecture's QARMA cipher; the key state is program-visible
// through the eight AP*Key{Lo,Hi}_EL1 registers.
// ========================================================================

#[derive(Clone, Copy)]
struct PacRegime {
    pac_mask: u64,
    upper: bool,
}

/// Resolve the EL0/EL1 translation-regime extension for a pointer. Mirrors
/// the strip logic used by XPAC: the top bit that selects TTBR1 is bit 55,
/// T0SZ/T1SZ live in TCR_EL1 bits 0..6 / 16..22, and the TBI0/TBI1 + TBID
/// bits decide whether the top byte is preserved.
fn pac_regime(tcr: u64, pointer: u64, data: bool) -> Option<PacRegime> {
    let upper = pointer & (1 << 55) != 0;
    let tsz = (tcr >> if upper { 16 } else { 0 }) & 63;
    if !(16..=39).contains(&tsz) {
        return None;
    }
    let tbi = tcr & (1 << if upper { 38 } else { 37 }) != 0;
    let tbid = tcr & (1 << if upper { 52 } else { 51 }) != 0;
    let preserve_tag = tbi && (data || !tbid);
    let address_mask = (1u64 << (64 - tsz)) - 1;
    let tag_mask = if preserve_tag {
        0xff00_0000_0000_0000
    } else {
        0
    };
    let pac_mask = !(address_mask | tag_mask);
    Some(PacRegime { pac_mask, upper })
}

fn pac_key(state: &iphem_core::CpuState, key_idx: u8) -> [u64; 2] {
    match key_idx {
        0 => [state.apia_lo, state.apia_hi],
        1 => [state.apib_lo, state.apib_hi],
        2 => [state.apda_lo, state.apda_hi],
        3 => [state.apdb_lo, state.apdb_hi],
        _ => [0, 0],
    }
}

/// Deterministic keyed PRF sized to the PAC field. Folds (key, address,
/// modifier) so equal inputs always produce equal signatures while each key
/// and modifier pair produces a distinct field.
fn pac_prf(key: [u64; 2], address: u64, modifier: u64) -> u64 {
    let mut z = key[0] ^ key[1].rotate_left(19) ^ address.rotate_left(5) ^ modifier.rotate_left(41);
    z ^= z >> 30;
    z = z.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    z.rotate_left(23)
}

/// Derive the PAC code from the (already extension-applied) address. Bit 55 is
/// the TTBR0/TTBR1 selector: forcing it to match the sign-time `upper` keeps
/// sign -> auth -> strip self-consistent even when the permutation would
/// otherwise flip the selector inside the code field.
fn pac_code(
    state: &iphem_core::CpuState,
    key_idx: u8,
    stripped: u64,
    modifier: u64,
    upper: bool,
) -> u64 {
    let mut field = pac_prf(pac_key(state, key_idx), stripped, modifier);
    if upper {
        field |= 1 << 55;
    } else {
        field &= !(1u64 << 55);
    }
    field
}

/// Sign `pointer` with the given key family, folding in `modifier`, and place
/// the code in the PAC field. Returns the signed pointer.
fn sign_pac(
    state: &iphem_core::CpuState,
    key_idx: u8,
    data: bool,
    pointer: u64,
    modifier: u64,
) -> Option<u64> {
    let rg = pac_regime(state.tcr_el1, pointer, data)?;
    // Hash the address-extension (stripped) form, exactly as a later
    // authentication recomputes it, so signing an unsigned pointer and then
    // authenticating round-trips.
    let stripped = (pointer & !rg.pac_mask) | if rg.upper { rg.pac_mask } else { 0 };
    let sig = pac_code(state, key_idx, stripped, modifier, rg.upper) & rg.pac_mask;
    Some((pointer & !rg.pac_mask) | sig)
}

/// Authenticate `pointer`; on a valid code returns the stripped pointer with
/// the address extension applied (identical to what XPAC would produce).
fn auth_pac(
    state: &iphem_core::CpuState,
    key_idx: u8,
    data: bool,
    pointer: u64,
    modifier: u64,
) -> Option<u64> {
    let rg = pac_regime(state.tcr_el1, pointer, data)?;
    let stripped = (pointer & !rg.pac_mask) | if rg.upper { rg.pac_mask } else { 0 };
    let expected = pac_code(state, key_idx, stripped, modifier, rg.upper) & rg.pac_mask;
    if pointer & rg.pac_mask != expected {
        return None;
    }
    Some(stripped)
}

/// The APKey instruction-vs-data attributed selector, helper for callers that
/// treat key index 2/3 as data keys (TBID inheritance is per-key).
#[allow(dead_code)]
fn pac_key_is_data(key_idx: u8) -> bool {
    key_idx >= 2
}

/// SCTLR.EnIA/EnIB/EnDA/EnDB control signing and authentication separately.
/// EL0 uses the EL1 regime; EL2 and EL3 have their own SCTLR banks.
/// This models the non-VHE regimes used by the boot harnesses.
fn pac_enabled(state: &iphem_core::CpuState, key: u8) -> bool {
    let sctlr = match state.el {
        ExceptionLevel::El0 | ExceptionLevel::El1 => state.sctlr_el1,
        ExceptionLevel::El2 => state.sctlr_el2,
        ExceptionLevel::El3 => state.sctlr_el3,
    };
    let bit = match key {
        0 => 31, // EnIA
        1 => 30, // EnIB
        2 => 27, // EnDA
        3 => 13, // EnDB
        _ => return false,
    };
    sctlr & (1 << bit) != 0
}

pub fn execute(m: &mut Machine, instr: Instruction) -> Result<(), ExecError> {
    use Op::*;
    let pc = m.cores.active_mut().state.pc;
    let op = instr.0;

    let Some(d) = decode(op) else {
        if pc == 0xfffffff007b54404 {
            eprintln!("DBG exec Undefined op={op:#x} at pc={pc:#x}");
        }
        return Err(ExecError::Undefined);
    };
    if pc == 0xfffffff007b54404 {
        eprintln!("DBG exec decoded={d:?} op={op:#x} at pc={pc:#x}");
    }

    trace(m, pc, op);

    // Disabled PAC leaves the pointer untouched, including before TCR and
    // keys are initialized. Combined branch/return instructions still perform
    // their control-flow operation. Trace retains the original instruction.
    let d = match d {
        PacData { sub, .. } if !pac_enabled(&m.cores.active().state, (sub & 3) as u8) => Nop,
        PacHint { key, .. } if !pac_enabled(&m.cores.active().state, key) => Nop,
        RetPac { key } if !pac_enabled(&m.cores.active().state, key) => Ret(30),
        EretPac { key } if !pac_enabled(&m.cores.active().state, key) => Eret,
        PacBranch {
            key, link, target, ..
        } if !pac_enabled(&m.cores.active().state, key) => {
            if link {
                Blr(target)
            } else {
                Br(target)
            }
        }
        other => other,
    };

    m.cores.active_mut().state.pc = pc + 4;

    match d {
        Eret | Drps => {
            let s = &mut m.cores.active_mut().state;
            let (return_pc, return_pstate) = match s.el {
                ExceptionLevel::El1 => (s.elr_el1, s.spsr_el1),
                ExceptionLevel::El2 => (s.elr_el2, s.spsr_el2),
                ExceptionLevel::El3 => (s.elr_el3, s.spsr_el3),
                ExceptionLevel::El0 => return Err(ExecError::Undefined),
            };
            bank_active_sp(s);
            s.pc = return_pc;
            s.pstate = return_pstate;
            s.nzcv = ((return_pstate >> 28) & 0xf) as u32;
            s.daif = ((return_pstate >> 6) & 0xf) as u8;
            let target_el = match (return_pstate >> 2) & 3 {
                0 => ExceptionLevel::El0,
                1 => ExceptionLevel::El1,
                2 => ExceptionLevel::El2,
                _ => ExceptionLevel::El3,
            };
            select_sp(s, target_el, return_pstate & 1 != 0);
        }
        Brk(imm) => {
            return Err(exception(m, (0x3cu64 << 26) | imm as u64, 0));
        }
        Hlt(imm) => {
            return Err(exception(m, (0x34u64 << 26) | imm as u64, 0));
        }
        Svc(imm) => {
            return Err(exception(m, (0x15u64 << 26) | imm as u64, 0));
        }
        Hvc(imm) => {
            let source = m.cores.active_mut().state.el;
            if source == ExceptionLevel::El0
                || (source == ExceptionLevel::El1
                    && m.cores.active_mut().state.hcr_el2 & (1 << 29) != 0)
            {
                return Err(ExecError::Undefined);
            }
            let target = match source {
                ExceptionLevel::El1 | ExceptionLevel::El2 => ExceptionLevel::El2,
                ExceptionLevel::El3 => ExceptionLevel::El3,
                ExceptionLevel::El0 => unreachable!(),
            };
            return Err(exception_to(m, target, (0x16u64 << 26) | imm as u64, 0));
        }
        Smc(imm) => {
            if m.cores.active_mut().state.el == ExceptionLevel::El0 {
                return Err(ExecError::Undefined);
            }
            return Err(exception_to(
                m,
                ExceptionLevel::El3,
                (0x17u64 << 26) | imm as u64,
                0,
            ));
        }
        Sys(word) => {
            return handle_sys(m, pc, word);
        }

        Branch { link, imm } => {
            let s = &mut m.cores.active_mut().state;
            // ARCHITECTURAL: imm19/imm26 is relative to the branch instruction
            // itself (PC), not the following instruction. Real firmware
            // (bootloaders, kernels) encodes with PC-relative immediates.
            let target = (pc as i64 + imm).wrapping_add(0) as u64;
            if link {
                wr(s, 30, pc + 4);
            }
            s.pc = target;
            // `b .` -- a branch to itself -- is how firmware spells "stop here":
            // the AArch64 equivalent of `while (1);`, and what a panic path or a
            // deliberate halt compiles to. `CpuRunState::Halted` existed for this
            // and nothing set it, so a run that halted burned its whole step
            // budget and reported only a PC, with a trace ring full of copies of
            // the same branch and no record of the path that arrived.
            //
            // A *conditional* self-branch (`b.eq .`) is deliberately not a halt:
            // that is a poll, it is a real idiom in a wait loop, and treating it
            // as terminal would stop every spin-with-exit. Only the
            // unconditional form has no exit at all.
            if !link && target == pc {
                m.cores.active_mut().run_state = CpuRunState::Halted;
            }
        }

        Bcond { cond, imm } => {
            let s = &mut m.cores.active_mut().state;
            if cond_check(s.pstate, cond) {
                s.pc = (pc as i64 + imm).wrapping_add(0) as u64;
            }
        }

        Br(rn) => {
            let target = reg(&m.cores.active().state, rn);
            branch_to(m, pc, target)?;
        }
        Blr(rn) => {
            let target = reg(&m.cores.active().state, rn);
            wr(&mut m.cores.active_mut().state, 30, pc + 4);
            branch_to(m, pc, target)?;
        }
        Ret(rn) => {
            let target = reg(&m.cores.active().state, rn);
            branch_to(m, pc, target)?;
        }
        RetPac { key } => {
            // RETAA/RETAB: authenticate the link register with the A/B
            // instruction key and SP modifier, then branch to it.
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let s = &mut m.cores.active_mut().state;
            let target = match auth_pac(s, key, false, reg(s, 30), s.sp) {
                Some(v) => v,
                None => return Err(exception(m, 0x22u64 << 26, 0)),
            };
            branch_to(m, pc, target)?;
        }
        EretPac { key } => {
            // ERETAA/ERETAB: authenticate ELR_ELx with the A/B instruction
            // key and the SP modifier, then return from the exception.
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let s = &mut m.cores.active_mut().state;
            let elr = match s.el {
                ExceptionLevel::El1 => s.elr_el1,
                ExceptionLevel::El2 => s.elr_el2,
                ExceptionLevel::El3 => s.elr_el3,
                ExceptionLevel::El0 => return Err(ExecError::Undefined),
            };
            let authed = match auth_pac(s, key, false, elr, s.sp) {
                Some(v) => v,
                None => return Err(exception(m, 0x22u64 << 26, 0)),
            };
            match s.el {
                ExceptionLevel::El1 => s.elr_el1 = authed,
                ExceptionLevel::El2 => s.elr_el2 = authed,
                ExceptionLevel::El3 => s.elr_el3 = authed,
                ExceptionLevel::El0 => unreachable!(),
            }
            let (return_pc, return_pstate) = match s.el {
                ExceptionLevel::El1 => (s.elr_el1, s.spsr_el1),
                ExceptionLevel::El2 => (s.elr_el2, s.spsr_el2),
                ExceptionLevel::El3 => (s.elr_el3, s.spsr_el3),
                ExceptionLevel::El0 => return Err(ExecError::Undefined),
            };
            bank_active_sp(s);
            s.pc = return_pc;
            s.pstate = return_pstate;
            s.nzcv = ((return_pstate >> 28) & 0xf) as u32;
            s.daif = ((return_pstate >> 6) & 0xf) as u8;
            let target_el = match (return_pstate >> 2) & 3 {
                0 => ExceptionLevel::El0,
                1 => ExceptionLevel::El1,
                2 => ExceptionLevel::El2,
                _ => ExceptionLevel::El3,
            };
            select_sp(s, target_el, return_pstate & 1 != 0);
        }
        PacBranch {
            key,
            link,
            modifier,
            target,
        } => {
            // BRAA/BRAB/BLRAA/BLRAB: authenticate the branch pointer (target
            // register) with the A/B instruction key and the modifier register,
            // then branch (linking into x30 for the BLR forms).
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let s = &mut m.cores.active_mut().state;
            let ptr = reg(s, target);
            let modv = if modifier == 32 {
                0
            } else {
                reg_sp(s, modifier)
            };
            let authed = match auth_pac(s, key, false, ptr, modv) {
                Some(v) => v,
                None => return Err(exception(m, 0x22u64 << 26, 0)),
            };
            if link {
                wr(s, 30, pc + 4);
            }
            branch_to(m, pc, authed)?;
        }
        PacHint { key, sign, sp } => {
            // 1716/SP hint forms: operate on x17 (modifier x16) or x30 (SP).
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let s = &mut m.cores.active_mut().state;
            let target = if sp { 30 } else { 17 };
            let modifier = if sp { s.sp } else { reg(s, 16) };
            let ptr = reg(s, target);
            let out = if sign {
                sign_pac(s, key, false, ptr, modifier).ok_or(ExecError::Undefined)?
            } else {
                match auth_pac(s, key, false, ptr, modifier) {
                    Some(v) => v,
                    None => return Err(exception(m, 0x22u64 << 26, 0)),
                }
            };
            wr(s, target, out);
        }
        Nop => {
            // Do nothing.
        }
        Cbz {
            is_cbnz,
            sf,
            rt,
            imm,
        } => {
            let s = &mut m.cores.active_mut().state;
            let v = reg(s, rt);
            let z = if sf == 1 { v == 0 } else { (v as u32) == 0 };
            if (is_cbnz && !z) || (!is_cbnz && z) {
                s.pc = (pc as i64 + imm).wrapping_add(0) as u64;
            }
        }

        Tbz {
            is_tbnz,
            bit,
            rt,
            imm,
        } => {
            let s = &mut m.cores.active_mut().state;
            let v = reg(s, rt);
            let b = (v >> bit) & 1;
            if (is_tbnz && b == 1) || (!is_tbnz && b == 0) {
                s.pc = (pc as i64 + imm).wrapping_add(0) as u64;
            }
        }

        Adr { rd, imm } => {
            let s = &mut m.cores.active_mut().state;
            wr(s, rd, pc.wrapping_add(imm as u64));
        }
        Adrp { rd, imm } => {
            let s = &mut m.cores.active_mut().state;
            wr(s, rd, (pc & !0xfff).wrapping_add((imm as u64) << 12));
        }

        AddSubImm {
            sf,
            sub,
            set,
            shl: _,
            imm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let a = reg_sp(s, rn);
            let b = imm;
            let result;
            let nzcv;
            if sub {
                (result, nzcv) =
                    add_with_carry(a, (!b) & full(sf), 1, if sf == 1 { 64 } else { 32 });
            } else {
                (result, nzcv) = add_with_carry(a, b, 0, if sf == 1 { 64 } else { 32 });
            }
            let result = result & full(sf);
            if set {
                set_flags(s, nzcv);
            }
            if set {
                wr(s, rd, result);
            } else {
                wr_sp(s, rd, result);
            }
        }

        MovImm {
            sf,
            kind,
            hw,
            imm,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let shift = 16 * hw;
            let mut v = imm << shift;
            if sf == 0 {
                v &= u32::MAX as u64;
            }
            match kind {
                0 => {
                    // MOVN (opc == 00)
                    v = !v & full(sf);
                    wr(s, rd, v);
                }
                2 => {
                    // MOVZ (opc == 10)
                    wr(s, rd, v & full(sf));
                }
                3 => {
                    // MOVK (opc == 11)
                    let cur = reg(s, rd);
                    let mask = 0xffffu64 << shift;
                    let merged = (cur & !mask) | (v & mask);
                    wr(s, rd, merged & full(sf));
                }
                _ => return Err(ExecError::Undefined),
            }
        }

        LogImm {
            sf,
            opc,
            n,
            immr,
            imms,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let Some(mask) = decode_bitmask(n, immr, imms, sf == 1) else {
                return Err(ExecError::Undefined);
            };
            let a = reg(s, rn);
            let r = match opc {
                0 => a & mask,
                1 => a | mask,
                2 => a ^ mask,
                3 => a & mask,
                _ => return Err(ExecError::Undefined),
            } & full(sf);
            wr(s, rd, r);
            if opc == 3 {
                set_flags(s, nzcv_from(r, if sf == 1 { 64 } else { 32 }));
            }
        }

        Extract {
            sf,
            lsb,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let high = reg(s, rn) & full(sf);
            let low = reg(s, rm) & full(sf);
            let lsb = lsb % width;
            let r = if lsb == 0 {
                low
            } else {
                ((low >> lsb) | (high << (width - lsb))) & full(sf)
            };
            wr(s, rd, r);
        }

        Bitfield {
            sf,
            opc,
            immr,
            imms,
            rn,
            rd,
        } => {
            let width = if sf == 1 { 64 } else { 32 };
            let r = bitfield_op(
                opc,
                reg(&m.cores.active().state, rn),
                reg(&m.cores.active().state, rd),
                immr,
                imms,
                width,
            );
            wr(&mut m.cores.active_mut().state, rd, r);
        }

        AddSubShift {
            sf,
            sub,
            set,
            shift,
            amount,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let b = apply_shift(reg(s, rm), shift, amount, width);
            let a = reg(s, rn);
            let result;
            let nzcv;
            if sub {
                (result, nzcv) = add_with_carry(a, (!b) & full(sf), 1, width);
            } else {
                (result, nzcv) = add_with_carry(a, b, 0, width);
            }
            let result = result & full(sf);
            if set {
                set_flags(s, nzcv);
            }
            wr(s, rd, result);
        }

        AddSubExt {
            sf,
            sub,
            set,
            option,
            amount,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let mut b = reg(s, rm);
            match option {
                0b000 => b = b as u8 as u64,
                0b001 => b = b as u16 as u64,
                0b010 => b = (b as u32) as u64,
                0b011 => {}
                0b100 => b = (b as i8 as i64) as u64,
                0b101 => b = (b as i16 as i64) as u64,
                0b110 => b = (b as u32 as i32 as i64) as u64,
                _ => {}
            }
            if amount != 0 {
                b = (b & full(sf)).wrapping_shl(amount);
            }
            let a = reg_sp(s, rn);
            let result;
            let nzcv;
            if sub {
                (result, nzcv) = add_with_carry(a, (!b) & full(sf), 1, width);
            } else {
                (result, nzcv) = add_with_carry(a, b, 0, width);
            }
            let result = result & full(sf);
            if set {
                set_flags(s, nzcv);
            }
            if set {
                wr(s, rd, result);
            } else {
                wr_sp(s, rd, result);
            }
        }

        LogShift {
            sf,
            opc,
            n,
            shift,
            amount,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let mut b = apply_shift(reg(s, rm), shift, amount, width);
            if n != 0 {
                b = !b & full(sf);
            }
            let a = reg(s, rn);
            let r = logical3(opc, a, b) & full(sf);
            wr(s, rd, r);
            if opc == 3 {
                set_flags(s, nzcv_from(r, width));
            }
        }

        Dp1 { sf, sub, rn, rd } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let a = reg(s, rn) & full(sf);
            let r = if sf == 1 {
                match sub {
                    0 => a.reverse_bits(),
                    1 => rev16_64(a),
                    2 => rev32_64(a),
                    3 => a.swap_bytes(),
                    4 => a.leading_zeros() as u64,
                    5 => cls64(a) as u64,
                    _ => return Err(ExecError::Undefined),
                }
            } else {
                let a32 = a as u32;
                match sub {
                    0 => a32.reverse_bits() as u64,
                    1 => rev16_32(a32) as u64,
                    2 => a32.swap_bytes() as u64,
                    4 => a32.leading_zeros() as u64,
                    5 => cls32(a32) as u64,
                    _ => return Err(ExecError::Undefined),
                }
            };
            let _ = width;
            wr(&mut m.cores.active_mut().state, rd, r);
        }

        Pacga { rn, rm, rd } => {
            // PACGA is independent of SCTLR.EnIA/EnIB/EnDA/EnDB and TCR.
            // The generic code occupies bits 63:32; bits 31:0 are zero.
            // Use the same explicitly non-QARMA PRF as the pointer PAC model.
            let key = [
                *m.cores
                    .active_mut()
                    .state
                    .vendor_sysregs
                    .get(&0xc118)
                    .unwrap_or(&0),
                *m.cores
                    .active_mut()
                    .state
                    .vendor_sysregs
                    .get(&0xc119)
                    .unwrap_or(&0),
            ];
            let value = pac_prf(
                key,
                reg(&m.cores.active().state, rn),
                reg_sp(&m.cores.active().state, rm),
            ) & 0xffff_ffff_0000_0000;
            wr(&mut m.cores.active_mut().state, rd, value);
        }

        PacData { sub, rn, rd } => {
            // PACIA/IB/DA/DB (+ the Z variants) sign the pointer in Rd using
            // the modifier in Rn; AUTIA/... authenticate it and strip the PAC.
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let key = (sub & 3) as u8;
            let data = key >= 2;
            let sign = (sub & 4) == 0;
            let modifier = if sub & 8 != 0 {
                0
            } else {
                reg_sp(&m.cores.active().state, rn)
            };
            let pointer = reg(&m.cores.active().state, rd);
            let out = if sign {
                sign_pac(&m.cores.active().state, key, data, pointer, modifier)
                    .ok_or(ExecError::Undefined)?
            } else {
                auth_pac(&m.cores.active().state, key, data, pointer, modifier)
                    .ok_or_else(|| exception(m, 0x22u64 << 26, 0))?
            };
            wr(&mut m.cores.active_mut().state, rd, out);
        }

        Xpac { data, rd } => {
            // Only the EL0/EL1 translation regime is currently modeled.
            // Do not silently apply TCR_EL1 to EL2/EL3 pointers.
            if m.cores.active_mut().state.el as u8 > 1 {
                return Err(ExecError::Undefined);
            }
            let pointer = reg(&m.cores.active().state, rd);
            let tcr = m.cores.active_mut().state.tcr_el1;
            let upper = pointer & (1 << 55) != 0;
            let tsz = (tcr >> if upper { 16 } else { 0 }) & 63;
            if !(16..=39).contains(&tsz) {
                return Err(ExecError::Undefined);
            }
            let tbi = tcr & (1 << if upper { 38 } else { 37 }) != 0;
            let tbid = tcr & (1 << if upper { 52 } else { 51 }) != 0;
            let preserve_tag = tbi && (data || !tbid);
            let address_mask = (1u64 << (64 - tsz)) - 1;
            let tag_mask = if preserve_tag {
                0xff00_0000_0000_0000
            } else {
                0
            };
            let pac_mask = !(address_mask | tag_mask);
            let value = (pointer & !pac_mask) | if upper { pac_mask } else { 0 };
            wr(&mut m.cores.active_mut().state, rd, value);
        }

        Dp2 {
            sf,
            sub,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let a = reg(s, rn) & full(sf);
            let b = reg(s, rm) & full(sf);
            let bm = b & (width - 1);
            let r = match sub {
                0 => a.checked_div(b).unwrap_or(0),
                1 => {
                    if b == 0 {
                        0
                    } else if sf == 1 {
                        let dividend = a as i64;
                        let divisor = b as i64;
                        dividend.checked_div(divisor).unwrap_or(i64::MIN) as u64
                    } else {
                        let dividend = a as u32 as i32;
                        let divisor = b as u32 as i32;
                        dividend.checked_div(divisor).unwrap_or(i32::MIN) as u32 as u64
                    }
                }
                2 => a.wrapping_shl(bm as u32),
                3 => a.wrapping_shr(bm as u32),
                4 => {
                    if sf == 1 {
                        ((a as i64) >> (bm as u32)) as u64
                    } else {
                        ((a as u32 as i32) >> (bm as u32)) as u32 as u64
                    }
                }
                5 => ror(a, bm as u32, width as u32),
                _ => return Err(ExecError::Undefined),
            } & full(sf);
            wr(&mut m.cores.active_mut().state, rd, r);
        }

        Dp3 {
            sf,
            sub,
            rm,
            ra,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let a = reg(s, rn) & full(sf);
            let b = reg(s, rm) & full(sf);
            let acc = reg(s, ra);
            let r = match sub {
                0 => a.wrapping_mul(b).wrapping_add(acc) & full(sf),
                1 => acc.wrapping_sub(a.wrapping_mul(b)) & full(sf),
                20 => acc.wrapping_add(
                    ((a as u32 as i32 as i64 as i128) * (b as u32 as i32 as i64 as i128)) as u64,
                ),
                21 => acc.wrapping_sub(
                    ((a as u32 as i32 as i64 as i128) * (b as u32 as i32 as i64 as i128)) as u64,
                ),
                22 => acc.wrapping_add((a as u32 as u64).wrapping_mul(b as u32 as u64)),
                23 => acc.wrapping_sub((a as u32 as u64).wrapping_mul(b as u32 as u64)),
                26 => {
                    let p = (a as i64) as i128 * (b as i64) as i128;
                    (p >> 64) as u64
                }
                27 => (((a as u128) * (b as u128)) >> 64) as u64,
                _ => return Err(ExecError::Undefined),
            };
            wr(&mut m.cores.active_mut().state, rd, r);
        }

        Carry {
            sf,
            sub,
            set,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            let a = reg(s, rn);
            let b = reg(s, rm);
            let cin = (s.nzcv >> 1) & 1;
            let result;
            let nzcv;
            if sub {
                (result, nzcv) = add_with_carry(a, (!b) & full(sf), cin as u64, width);
            } else {
                (result, nzcv) = add_with_carry(a, b, cin as u64, width);
            }
            let result = result & full(sf);
            if set {
                set_flags(s, nzcv);
            }
            wr(s, rd, result);
        }

        CCmp {
            sf,
            sub,
            is_imm,
            cond,
            nzcv,
            rm,
            rn,
        } => {
            let s = &mut m.cores.active_mut().state;
            let width = if sf == 1 { 64 } else { 32 };
            if cond_check(s.pstate, cond) {
                let a = reg(s, rn);
                let b = if is_imm { rm as u64 } else { reg(s, rm) };
                let (_, nzcv) = if sub {
                    add_with_carry(a, (!b) & full(sf), 1, width)
                } else {
                    add_with_carry(a, b, 0, width)
                };
                set_flags(s, nzcv);
            } else {
                set_flags(s, nzcv);
            }
        }

        CondSel {
            sf,
            sel_op,
            rm,
            cond,
            rn,
            rd,
        } => {
            let s = &m.cores.active_mut().state;
            let a = reg(s, rn);
            let b = reg(s, rm);
            let take = cond_check(s.pstate, cond);
            // CSEL  Rd = cond ? Rn     : Rm
            // CSINC Rd = cond ? Rn     : Rm + 1
            // CSINV Rd = cond ? Rn     : ~Rm
            // CSNEG Rd = cond ? Rn     : -Rm
            //
            // The operand that gets incremented/inverted/negated is Rm, not Rn:
            // the architecture selects Rn unchanged when the condition holds and
            // applies the operation to Rm when it does not. Verified by
            // execution on AArch64 hardware, not just by reading the manual --
            // `csinc w0, w1, w2, eq` with Z set yields w1, and with Z clear
            // yields w2 + 1.
            let r = if take {
                a
            } else {
                match sel_op {
                    0 => b,
                    1 => b.wrapping_add(1),
                    2 => !b,
                    _ => 0u64.wrapping_sub(b),
                }
            } & full(sf);
            wr(&mut m.cores.active_mut().state, rd, r);
        }

        LdrLit {
            vector,
            size,
            sw,
            prefetch,
            rt,
            imm,
        } => {
            if prefetch {
                return Ok(());
            }
            let addr = pc.wrapping_add(imm as u64);
            if vector {
                let lo = m.mem_read(addr, (size as usize).min(8))? as u128;
                let hi = if size == 16 {
                    m.mem_read(addr.wrapping_add(8), 8)? as u128
                } else {
                    0
                };
                m.cores.active_mut().state.v[rt as usize] = lo | (hi << 64);
                return Ok(());
            }
            let mut v = m.mem_read(addr, size as usize)?;
            if sw {
                v = sign_extend(v, size * 8);
            }
            wr(&mut m.cores.active_mut().state, rt, v);
        }

        Ordered { load, size, rn, rt } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            if !addr.is_multiple_of(size as u64) {
                return Err(atomic_alignment_exception(m, addr, !load, pc));
            }
            // The interpreter serializes memory operations already.
            if load {
                let value = m.mem_read(addr, size as usize)?;
                wr(&mut m.cores.active_mut().state, rt, value);
            } else {
                m.mem_write(addr, size as usize, reg(&m.cores.active().state, rt))?;
            }
        }

        Excl {
            sf,
            load,
            size,
            rs,
            rn,
            rt,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            if !addr.is_multiple_of(size as u64) {
                return Err(atomic_alignment_exception(m, addr, !load, pc));
            }
            if load {
                let v = m.mem_read(addr, size as usize)?;
                wr(&mut m.cores.active_mut().state, rt, v);
                m.cores.active_mut().exclusive_address = Some(addr);
                m.cores.active_mut().exclusive_size = size as usize;
            } else {
                let succeeds = m.cores.active_mut().exclusive_address == Some(addr)
                    && m.cores.active_mut().exclusive_size == size as usize;
                m.cores.active_mut().exclusive_address = None;
                m.cores.active_mut().exclusive_size = 0;
                if succeeds {
                    let v = reg(&m.cores.active().state, rt);
                    m.mem_write(addr, size as usize, v)?;
                    wr(&mut m.cores.active_mut().state, rs, 0);
                } else {
                    wr(&mut m.cores.active_mut().state, rs, 1);
                }
            }
            let _ = sf;
        }

        Lse {
            sf,
            sub,
            size,
            rm,
            rn,
            rt,
            ..
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            if !addr.is_multiple_of(size as u64) {
                return Err(atomic_alignment_exception(m, addr, true, pc));
            }
            let vold = m.mem_read(addr, size as usize)?;
            let source = reg(&m.cores.active().state, rm);
            match sub {
                1 => {
                    // SWP
                    m.mem_write(addr, size as usize, source)?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                2 => {
                    // LDADD
                    m.mem_write(addr, size as usize, vold.wrapping_add(source))?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                3 => {
                    // LDCLR
                    m.mem_write(addr, size as usize, vold & !source)?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                4 => {
                    // LDEOR
                    m.mem_write(addr, size as usize, vold ^ source)?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                5 => {
                    // LDSET
                    m.mem_write(addr, size as usize, vold | source)?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                // Phase 171: store variants. Update memory in place; the
                // destination register (rt, conventionally XZR/31) is not
                // written.
                6 => {
                    // STADD
                    m.mem_write(addr, size as usize, vold.wrapping_add(source))?;
                }
                7 => {
                    // STCLR
                    m.mem_write(addr, size as usize, vold & !source)?;
                }
                8 => {
                    // STEOR
                    m.mem_write(addr, size as usize, vold ^ source)?;
                }
                9 => {
                    // STSET
                    m.mem_write(addr, size as usize, vold | source)?;
                }
                10..=13 => {
                    let mask = if size == 8 {
                        u64::MAX
                    } else {
                        (1u64 << (size * 8)) - 1
                    };
                    let old = vold & mask;
                    let operand = source & mask;
                    let signed = |value: u64| {
                        let shift = 64 - size * 8;
                        ((value << shift) as i64) >> shift
                    };
                    let result = match sub {
                        10 => {
                            if signed(old) > signed(operand) {
                                old
                            } else {
                                operand
                            }
                        } // LDSMAX
                        11 => {
                            if signed(old) < signed(operand) {
                                old
                            } else {
                                operand
                            }
                        } // LDSMIN
                        12 => old.max(operand), // LDUMAX
                        _ => old.min(operand),  // LDUMIN
                    };
                    m.mem_write(addr, size as usize, result)?;
                    wr(&mut m.cores.active_mut().state, rt, vold);
                }
                14 => {
                    // CAS compares Rs (rm) with memory, conditionally stores Rt,
                    // then returns the old memory value in Rs.
                    let mask = if size == 8 {
                        u64::MAX
                    } else {
                        (1u64 << (size * 8)) - 1
                    };
                    let vsame = (vold & mask) == (source & mask);
                    if vsame {
                        m.mem_write(addr, size as usize, reg(&m.cores.active().state, rt))?;
                    }
                    wr(&mut m.cores.active_mut().state, rm, vold);
                }
                _ => unreachable!("unknown decoded LSE operation {sub}"),
            }
            let _ = sf;
        }

        LsePair {
            size, rs, rn, rt, ..
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            let pair_bytes = (size * 2) as u64;
            if !addr.is_multiple_of(pair_bytes) {
                return Err(atomic_alignment_exception(m, addr, true, pc));
            }

            // Read the full pair and both operands before writing either word,
            // so compare failure and overlapping register pairs are handled
            // with CASP's read/compare/write/result ordering.
            let old0 = m.mem_read(addr, size as usize)?;
            let old1 = m.mem_read(addr + size as u64, size as usize)?;
            let mask = if size == 4 { u32::MAX as u64 } else { u64::MAX };
            let expected0 = reg(&m.cores.active().state, rs) & mask;
            let expected1 = reg(&m.cores.active().state, rs + 1) & mask;
            let matches = old0 == expected0 && old1 == expected1;
            let new0 = reg(&m.cores.active().state, rt) & mask;
            let new1 = reg(&m.cores.active().state, rt + 1) & mask;
            if matches {
                m.mem_write(addr, size as usize, new0)?;
                m.mem_write(addr + size as u64, size as usize, new1)?;
            }
            wr(&mut m.cores.active_mut().state, rs, old0);
            wr(&mut m.cores.active_mut().state, rs + 1, old1);
        }

        Crc32 { sz, c, rm, rn, rd } => {
            // CRC32B/H/W: 8/16/32-bit data register, 32-bit CRC in Wd.
            // Per ARMv8 ARM (CRC32 / CRC32C data-processing), the algorithm
            // is the standard reflected CRC with implicit initial/final
            // inversion (caller passes ~crc_initial in Rn, result needs no
            // extra XOR). For sz bits of data, process sz rounds.
            let mut crc = reg(&m.cores.active().state, rn) as u32;
            let data = match sz {
                0 => reg(&m.cores.active().state, rm) as u32 & 0xFF,
                1 => reg(&m.cores.active().state, rm) as u32 & 0xFFFF,
                2 => reg(&m.cores.active().state, rm) as u32,
                _ => 0,
            };
            let poly = if c { 0x82F6_3B78u32 } else { 0xEDB8_8320u32 };
            let bits = match sz {
                0 => 8u32,
                1 => 16u32,
                2 => 32u32,
                _ => 0,
            };
            // CRC XOR data, then process `bits` rounds, shifting right.
            crc ^= data;
            for _ in 0..bits {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ poly;
                } else {
                    crc >>= 1;
                }
            }
            wr(&mut m.cores.active_mut().state, rd, crc as u64);
        }

        LdrAuth {
            key,
            writeback,
            rn,
            rt,
            imm,
        } => {
            // Writeback overlapping the destination is constrained
            // unpredictable. Select the architecturally permitted UNDEFINED.
            if writeback && rn == rt && rn != 31 {
                return Err(ExecError::Undefined);
            }
            check_sp_alignment(m, rn, pc)?;
            let pointer = reg_sp(&m.cores.active().state, rn);
            let base = if pac_enabled(&m.cores.active().state, key) {
                // Like the other PAC operations, only EL0/EL1 geometry is
                // supported. Disabled authentication still permits the load.
                if m.cores.active_mut().state.el as u8 > 1 {
                    return Err(ExecError::Undefined);
                }
                let rg = pac_regime(m.cores.active_mut().state.tcr_el1, pointer, true)
                    .ok_or(ExecError::Undefined)?;
                auth_pac(&m.cores.active().state, key, true, pointer, 0).unwrap_or_else(|| {
                    // ARMv8.3 failure poisons the canonical pointer, rather
                    // than taking a PC-alignment exception. Normal memory
                    // translation below reports the resulting data abort.
                    let stripped =
                        (pointer & !rg.pac_mask) | if rg.upper { rg.pac_mask } else { 0 };
                    let tbi = m.cores.active_mut().state.tcr_el1
                        & (1 << if rg.upper { 38 } else { 37 })
                        != 0;
                    let shift = if tbi { 53 } else { 61 };
                    let error_code = if key == 2 { 1 } else { 2 };
                    (stripped & !(3u64 << shift)) | (error_code << shift)
                })
            } else {
                pointer
            };
            let address = base.wrapping_add(imm as u64);
            ldrstr(m, 1, 3, rt, address)?;
            if writeback {
                wr_sp(&mut m.cores.active_mut().state, rn, address);
            }
        }

        LdrStrU {
            size,
            opc,
            rn,
            rt,
            off,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(off);
            ldrstr(m, opc, size, rt, addr)?;
        }

        LdrStrPre {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(imm as u64);
            ldrstr(m, opc, size, rt, addr)?;
            wr_sp(&mut m.cores.active_mut().state, rn, addr);
        }

        LdrStrPost {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            ldrstr(m, opc, size, rt, addr)?;
            wr_sp(
                &mut m.cores.active_mut().state,
                rn,
                addr.wrapping_add(imm as u64),
            );
        }

        Ldur {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(imm as u64);
            ldrstr(m, opc, size, rt, addr)?;
        }

        LdrStrReg {
            size,
            opc,
            option,
            amount,
            rn,
            rm,
            rt,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let base = reg_sp(&m.cores.active().state, rn);
            let mut idx = reg(&m.cores.active().state, rm);
            match option {
                0b011 | 0b111 => {} // LSL/SXTX (native 64-bit index)
                0b010 => idx = (idx as u32) as u64,
                0b110 => idx = (idx as u32 as i32 as i64) as u64,
                0b000 => idx = idx as u8 as u64,
                0b001 => idx = idx as u16 as u64,
                0b100 => idx = (idx as i8 as i64) as u64,
                0b101 => idx = (idx as i16 as i64) as u64,
                _ => {}
            }
            idx <<= amount;
            let addr = base.wrapping_add(idx);
            ldrstr(m, opc, size, rt, addr)?;
        }

        LdStPair {
            size,
            load,
            signed,
            pre,
            post,
            imm,
            rn,
            rt,
            rt2,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let base = reg_sp(&m.cores.active().state, rn);
            // pre-index writes at base+imm, post-index reads/writes at `base`
            // (the already-pushed pointer), offset reads/writes at base+imm.
            let eff = base.wrapping_add(imm as u64);
            let addr = if post { base } else { eff };
            let addr2 = addr.wrapping_add(size as u64);
            if load {
                let mut v1 = m.mem_read(addr, size as usize)?;
                let mut v2 = m.mem_read(addr2, size as usize)?;
                if signed {
                    v1 = sign_extend(v1, 32);
                    v2 = sign_extend(v2, 32);
                }
                wr(&mut m.cores.active_mut().state, rt, v1);
                wr(&mut m.cores.active_mut().state, rt2, v2);
            } else {
                let v1 = reg(&m.cores.active().state, rt);
                let v2 = reg(&m.cores.active().state, rt2);
                m.mem_write(addr, size as usize, v1)?;
                m.mem_write(addr2, size as usize, v2)?;
            }
            // Both pre- and post-index update the base register to base+imm.
            if pre || post {
                wr_sp(&mut m.cores.active_mut().state, rn, eff);
            }
        }

        // -------------------------------------------------------------------
        // SIMD&FP loads/stores (V-register destinations, Q = two 8-byte ops).
        // -------------------------------------------------------------------
        SimdStructSingle {
            q,
            load,
            post,
            size: _,
            rn,
            rt,
            rm,
        } => {
            let addr = reg_sp(&m.cores.active().state, rn);
            check_sp_alignment(m, rn, pc)?;
            let bytes = if q { 16 } else { 8 };
            if load {
                let lo = m.mem_read(addr, 8)?;
                let hi = if q { m.mem_read(addr + 8, 8)? } else { 0 };
                m.cores.active_mut().state.v[rt as usize] = ((hi as u128) << 64) | lo as u128;
            } else {
                let value = m.cores.active_mut().state.v[rt as usize];
                m.mem_write(addr, 8, value as u64)?;
                if q {
                    m.mem_write(addr + 8, 8, (value >> 64) as u64)?;
                }
            }
            if post {
                let increment = if rm == 31 {
                    bytes
                } else {
                    reg(&m.cores.active().state, rm)
                };
                wr_sp(
                    &mut m.cores.active_mut().state,
                    rn,
                    addr.wrapping_add(increment),
                );
            }
        }
        SimdStructPair {
            q,
            load,
            post,
            size: _,
            rn,
            rt,
            rm,
        } => {
            let addr = reg_sp(&m.cores.active().state, rn);
            check_sp_alignment(m, rn, pc)?;
            let bytes = if q { 16 } else { 8 };
            if load {
                for regno in [rt, (rt + 1) & 31] {
                    let lo = m.mem_read(addr + if regno == rt { 0 } else { bytes }, 8)?;
                    let hi = if q {
                        m.mem_read(addr + if regno == rt { 8 } else { bytes + 8 }, 8)?
                    } else {
                        0
                    };
                    m.cores.active_mut().state.v[regno as usize] =
                        ((hi as u128) << 64) | lo as u128;
                }
            } else {
                for (regno, offset) in [(rt, 0), ((rt + 1) & 31, bytes)] {
                    let value = m.cores.active_mut().state.v[regno as usize];
                    m.mem_write(addr + offset, 8, value as u64)?;
                    if q {
                        m.mem_write(addr + offset + 8, 8, (value >> 64) as u64)?;
                    }
                }
            }
            if post {
                let increment = if rm == 31 {
                    bytes * 2
                } else {
                    reg(&m.cores.active().state, rm)
                };
                wr_sp(
                    &mut m.cores.active_mut().state,
                    rn,
                    addr.wrapping_add(increment),
                );
            }
        }
        SimdStructMulti {
            n,
            q,
            load,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let addr = reg_sp(&m.cores.active().state, rn);
            check_sp_alignment(m, rn, pc)?;
            let elems = if q { 16 } else { 8 };
            let esize = 1usize << size;
            let lanes = elems / esize;
            let regs: Vec<u32> = (0..n).map(|i| (rt + i) & 31).collect();
            if load {
                for (index, regno) in regs.iter().enumerate() {
                    let mut acc = 0u128;
                    for e in 0..lanes {
                        let offset = ((e * n as usize + index) * esize) as u64;
                        let lo = m.mem_read(addr + offset, esize)?;
                        acc |= (lo as u128) << (8 * e * esize);
                    }
                    m.cores.active_mut().state.v[*regno as usize] = acc;
                }
            } else {
                for (index, regno) in regs.iter().enumerate() {
                    let value = m.cores.active_mut().state.v[*regno as usize];
                    for e in 0..lanes {
                        let offset = ((e * n as usize + index) * esize) as u64;
                        let elem = (value >> (8 * e * esize)) & ((1u128 << (8 * esize)) - 1);
                        m.mem_write(addr + offset, esize, elem as u64)?;
                    }
                }
            }
            if post {
                let increment = if rm == 31 {
                    (elems * n as usize) as u64
                } else {
                    reg(&m.cores.active().state, rm)
                };
                wr_sp(
                    &mut m.cores.active_mut().state,
                    rn,
                    addr.wrapping_add(increment),
                );
            }
        }
        SimdStructReplicate {
            n,
            q,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let addr = reg_sp(&m.cores.active().state, rn);
            check_sp_alignment(m, rn, pc)?;
            let elem = 1usize << size;
            let lanes = (if q { 16 } else { 8 }) / elem;
            let value = m.mem_read(addr, elem)?;
            let mut replicated = 0u128;
            for lane in 0..lanes {
                replicated |= (value as u128) << (8 * lane * elem);
            }
            for k in 0..n {
                m.cores.active_mut().state.v[((rt + k) & 31) as usize] = replicated;
            }
            if post {
                let increment = if rm == 31 {
                    elem as u64
                } else {
                    reg(&m.cores.active().state, rm)
                };
                wr_sp(
                    &mut m.cores.active_mut().state,
                    rn,
                    addr.wrapping_add(increment),
                );
            }
        }
        NeonTblTbx {
            q,
            tbx,
            n,
            vn,
            vm,
            vd,
        } => {
            let elems = if q { 16 } else { 8 };
            let table_bytes = (n * elems) as usize;
            let mut out = [0u8; 16];
            let mut orig = [0u8; 16];
            for (i, b) in orig.iter_mut().enumerate() {
                *b = ((m.cores.active_mut().state.v[vd as usize] >> (8 * i)) & 0xff) as u8;
            }
            for lane in 0..(elems as usize) {
                let idx =
                    ((m.cores.active_mut().state.v[vm as usize] >> (8 * lane)) & 0xff) as usize;
                let value: u8 = if idx < table_bytes {
                    let table = idx / (elems as usize);
                    let k = idx % (elems as usize);
                    ((m.cores.active_mut().state.v[((vn + table as u32) & 31) as usize] >> (8 * k))
                        & 0xff) as u8
                } else if tbx {
                    orig[lane]
                } else {
                    0
                };
                out[lane] = value;
            }
            let mut v = 0u128;
            for (i, b) in out.iter().enumerate() {
                v |= (*b as u128) << (8 * i);
            }
            m.cores.active_mut().state.v[vd as usize] = v;
        }

        SimdLdrStrU {
            size,
            opc,
            rn,
            rt,
            off,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(off);
            simd_xfer(m, opc, size, rt, addr)?;
        }

        SimdLdrStrPre {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(imm as u64);
            simd_xfer(m, opc, size, rt, addr)?;
            wr_sp(&mut m.cores.active_mut().state, rn, addr);
        }

        SimdLdrStrPost {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn);
            simd_xfer(m, opc, size, rt, addr)?;
            wr_sp(
                &mut m.cores.active_mut().state,
                rn,
                addr.wrapping_add(imm as u64),
            );
        }

        SimdLdurStur {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let addr = reg_sp(&m.cores.active().state, rn).wrapping_add(imm as u64);
            simd_xfer(m, opc, size, rt, addr)?;
        }

        SimdLdrStrReg {
            size,
            opc,
            option,
            amount,
            rn,
            rm,
            rt,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let base = reg_sp(&m.cores.active().state, rn);
            let mut idx = reg(&m.cores.active().state, rm);
            match option {
                0b011 | 0b111 => {} // LSL/SXTX (native 64-bit index)
                0b010 => idx = (idx as u32) as u64,
                0b110 => idx = (idx as u32 as i32 as i64) as u64,
                0b000 => idx = idx as u8 as u64,
                0b001 => idx = idx as u16 as u64,
                0b100 => idx = (idx as i8 as i64) as u64,
                0b101 => idx = (idx as i16 as i64) as u64,
                _ => {}
            }
            let addr = base.wrapping_add(idx << amount);
            simd_xfer(m, opc, size, rt, addr)?;
        }

        SimdLdStPair {
            size,
            load,
            pre,
            post,
            imm,
            rn,
            rt,
            rt2,
        } => {
            check_sp_alignment(m, rn, pc)?;
            let base = reg_sp(&m.cores.active().state, rn);
            let eff = base.wrapping_add(imm as u64);
            let addr = if post { base } else { eff };
            let size = size as usize;
            let addr2 = addr.wrapping_add(size as u64);
            if load {
                // Q lanes (16-byte elements) split each transfer into two
                // 8-byte accesses; S (4) and D (8) transfer a single element
                // word into the low part of the V lane.
                if size >= 16 {
                    let lo = m.mem_read(addr, 8)?;
                    let hi = m.mem_read(addr + 8, 8)?;
                    let v1 = ((hi as u128) << 64) | lo as u128;
                    let lo = m.mem_read(addr2, 8)?;
                    let hi = m.mem_read(addr2 + 8, 8)?;
                    let v2 = ((hi as u128) << 64) | lo as u128;
                    m.cores.active_mut().state.v[rt as usize] = v1;
                    m.cores.active_mut().state.v[rt2 as usize] = v2;
                } else {
                    let v1 = m.mem_read(addr, size)?;
                    let v2 = m.mem_read(addr2, size)?;
                    m.cores.active_mut().state.v[rt as usize] = v1 as u128;
                    m.cores.active_mut().state.v[rt2 as usize] = v2 as u128;
                }
            } else {
                let v1 = m.cores.active_mut().state.v[rt as usize];
                let v2 = m.cores.active_mut().state.v[rt2 as usize];
                if size >= 16 {
                    m.mem_write(addr, 8, v1 as u64)?;
                    m.mem_write(addr + 8, 8, (v1 >> 64) as u64)?;
                    m.mem_write(addr2, 8, v2 as u64)?;
                    m.mem_write(addr2 + 8, 8, (v2 >> 64) as u64)?;
                } else {
                    m.mem_write(addr, size, v1 as u64)?;
                    m.mem_write(addr2, size, v2 as u64)?;
                }
            }
            if pre || post {
                wr_sp(&mut m.cores.active_mut().state, rn, eff);
            }
        }

        Fjcvtzs { rn, rd } => {
            let s = &mut m.cores.active_mut().state;
            let v = f64::from_bits(s.v[rn as usize] as u64);
            let int_val = if v.is_nan() {
                0
            } else if v >= 2147483647.0 {
                2147483647
            } else if v <= -2147483648.0 {
                -2147483648
            } else {
                v as i32
            };
            wr(s, rd, int_val as u32 as u64);
            // JS conversion sets Z if exact.
            s.nzcv = if (int_val as f64) == v
                && !v.is_nan()
                && v != f64::NEG_INFINITY
                && v != f64::INFINITY
            {
                0b0100
            } else {
                0
            };
            set_flags(s, s.nzcv);
        }

        // -------------------------------------------------------------------
        // Floating-point scalar data-processing.
        // -------------------------------------------------------------------
        FpThreeSrc {
            sub,
            op31,
            rm,
            ra,
            rn,
            rd,
        } => {
            // FpThreeSrc: rn = [9:5] (source1), rm = [20:16] (source2),
            // ra = [14:10] (addend), rd = [4:0] (dest).
            // sub = bit15 (negate-addend), op31 = bit21 (negate result).
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let rm_v = s.v[rm as usize];
            let ra_v = s.v[ra as usize];
            let type_half = (op >> 22) & 1;
            if type_half == 0 {
                let a = f32::from_bits(rn_v as u32);
                let b = f32::from_bits(rm_v as u32);
                let mut acc = f32::from_bits(ra_v as u32);
                if sub {
                    acc = -acc;
                }
                let mut result = a.mul_add(b, acc);
                if op31 != 0 {
                    result = -result;
                }
                s.v[rd as usize] = result.to_bits() as u128;
            } else if type_half == 1 {
                let a = f64::from_bits(rn_v as u64);
                let b = f64::from_bits(rm_v as u64);
                let mut acc = f64::from_bits(ra_v as u64);
                if sub {
                    acc = -acc;
                }
                let mut result = a.mul_add(b, acc);
                if op31 != 0 {
                    result = -result;
                }
                s.v[rd as usize] = result.to_bits() as u128;
            }
        }

        FpTwoSrc { opc, rm, rn, rd } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let rm_v = s.v[rm as usize];
            let type_half = (op >> 22) & 1;
            let f = |opc: u32, a: f64, b: f64| -> f64 {
                match opc {
                    0 => a * b,    // FMUL
                    1 => a / b,    // FDIV
                    2 => a + b,    // FADD
                    3 => a - b,    // FSUB
                    4 => a.max(b), // FMAX
                    5 => a.min(b), // FMIN
                    6 => {
                        // FMAXNM: max ignoring NaNs
                        if a.is_nan() {
                            b
                        } else if b.is_nan() {
                            a
                        } else {
                            a.max(b)
                        }
                    }
                    7 => {
                        // FMINNM: min ignoring NaNs
                        if a.is_nan() {
                            b
                        } else if b.is_nan() {
                            a
                        } else {
                            a.min(b)
                        }
                    }
                    8 => -(a * b), // FNMUL
                    _ => a,
                }
            };
            if type_half == 0 {
                let a = f32::from_bits(rn_v as u32);
                let b = f32::from_bits(rm_v as u32);
                let r = f(opc, a as f64, b as f64) as f32;
                s.v[rd as usize] = r.to_bits() as u128;
            } else {
                let a = f64::from_bits(rn_v as u64);
                let b = f64::from_bits(rm_v as u64);
                let r = f(opc, a, b);
                s.v[rd as usize] = r.to_bits() as u128;
            }
        }

        FpCmpReg {
            signal_all_nans,
            rn,
            rm,
        } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let rm_v = s.v[rm as usize];
            let type_half = (op >> 22) & 1;
            if type_half == 0 {
                let a = f32::from_bits(rn_v as u32);
                let b = f32::from_bits(rm_v as u32);
                let (n, z, c, v) = if a.is_nan() || b.is_nan() {
                    if signal_all_nans && (fp_is_snan_s(rn_v as u32) || fp_is_snan_s(rm_v as u32)) {
                        s.fpsr |= 1 << 7; // IOC
                    }
                    (0, 0, 0, 1) // unordered: NZCV=0001
                } else if a < b {
                    (1, 0, 0, 0) // less than
                } else if a == b {
                    (0, 1, 1, 0) // equal
                } else {
                    (0, 0, 1, 0) // greater than
                };
                s.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                s.pstate = (s.pstate & !0xf0000000) | ((s.nzcv as u64) << 28);
            } else if type_half == 1 {
                let a = f64::from_bits(rn_v as u64);
                let b = f64::from_bits(rm_v as u64);
                let (n, z, c, v) = if a.is_nan() || b.is_nan() {
                    if signal_all_nans && (fp_is_snan_d(rn_v as u64) || fp_is_snan_d(rm_v as u64)) {
                        s.fpsr |= 1 << 7;
                    }
                    (0, 0, 0, 1)
                } else if a < b {
                    (1, 0, 0, 0)
                } else if a == b {
                    (0, 1, 1, 0)
                } else {
                    (0, 0, 1, 0)
                };
                s.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                s.pstate = (s.pstate & !0xf0000000) | ((s.nzcv as u64) << 28);
            }
        }

        FpCmpZero {
            signal_all_nans,
            rn,
        } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let type_half = (op >> 22) & 3;
            if type_half == 0 {
                let a = f32::from_bits(rn_v as u32);
                let b = 0.0f32;
                let (n, z, c, v) = if a.is_nan() {
                    if signal_all_nans && fp_is_snan_s(rn_v as u32) {
                        s.fpsr |= 1 << 7;
                    }
                    (0, 0, 0, 1)
                } else if a < b {
                    (1, 0, 0, 0)
                } else if a == b {
                    (0, 1, 1, 0)
                } else {
                    (0, 0, 1, 0)
                };
                s.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                s.pstate = (s.pstate & !0xf0000000) | ((s.nzcv as u64) << 28);
            } else if type_half == 1 {
                let a = f64::from_bits(rn_v as u64);
                let b = 0.0f64;
                let (n, z, c, v) = if a.is_nan() {
                    if signal_all_nans && fp_is_snan_d(rn_v as u64) {
                        s.fpsr |= 1 << 7;
                    }
                    (0, 0, 0, 1)
                } else if a < b {
                    (1, 0, 0, 0)
                } else if a == b {
                    (0, 1, 1, 0)
                } else {
                    (0, 0, 1, 0)
                };
                s.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                s.pstate = (s.pstate & !0xf0000000) | ((s.nzcv as u64) << 28);
            }
        }

        FpCCmp {
            sub: _,
            cond,
            nzcv,
            rm,
            rn,
        } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let rm_v = s.v[rm as usize];
            let type_half = (op >> 22) & 1;
            if cond_check(s.pstate, cond) {
                let (n, z, c, v) = if type_half == 0 {
                    let a = f32::from_bits(rn_v as u32);
                    let b = f32::from_bits(rm_v as u32);
                    if a.is_nan() || b.is_nan() {
                        (0, 0, 0, 1)
                    } else if a < b {
                        (1, 0, 0, 0)
                    } else if a == b {
                        (0, 1, 1, 0)
                    } else {
                        (0, 0, 1, 0)
                    }
                } else {
                    let a = f64::from_bits(rn_v as u64);
                    let b = f64::from_bits(rm_v as u64);
                    if a.is_nan() || b.is_nan() {
                        (0, 0, 0, 1)
                    } else if a < b {
                        (1, 0, 0, 0)
                    } else if a == b {
                        (0, 1, 1, 0)
                    } else {
                        (0, 0, 1, 0)
                    }
                };
                s.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                s.pstate = (s.pstate & !0xf0000000) | ((s.nzcv as u64) << 28);
            } else {
                s.nzcv = nzcv;
                s.pstate = (s.pstate & !0xf0000000) | ((nzcv as u64) << 28);
            }
        }

        FpSel { rm, cond, rn, rd } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let rm_v = s.v[rm as usize];
            let take = cond_check(s.pstate, cond);
            s.v[rd as usize] = if take { rn_v } else { rm_v };
        }

        FpOneSrc { opcode, rn, rd } => {
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let typ = (op >> 22) & 3;
            let rounding = fp_rounding_mode(s.fpcr);
            match typ {
                0 => {
                    let a = f32::from_bits(rn_v as u32);
                    let bits: u32 = match opcode {
                        0x010 => a.to_bits(),       // FMOV
                        0x030 => a.abs().to_bits(), // FABS
                        0x050 => (-a).to_bits(),    // FNEG
                        0x070 => {
                            if a < 0.0 {
                                s.fpsr |= 1 << 7;
                                fp_default_nan_s()
                            } else {
                                a.sqrt().to_bits()
                            }
                        }
                        0x110 => fp_round_s(a, 0).to_bits(), // FRINTN
                        0x130 => fp_round_s(a, 1).to_bits(), // FRINTP
                        0x150 => fp_round_s(a, 2).to_bits(), // FRINTM
                        0x170 => fp_round_s(a, 3).to_bits(), // FRINTZ
                        0x190 => {
                            let r = a.round();
                            if (a - a.trunc()).abs() == 0.5 {
                                if a > 0.0 {
                                    a.trunc() + 1.0
                                } else {
                                    a.trunc() - 1.0
                                }
                            } else {
                                r
                            }
                            .to_bits()
                        }
                        0x1d0 => fp_round_s(a, rounding).to_bits(), // FRINTX
                        0x1f0 => fp_round_s(a, rounding).to_bits(), // FRINTI
                        0x090 => fp_cvt_f64_to_f32(h_to_f64(rn_v as u16)),
                        0x0b0 => a.to_bits(),
                        0x0f0 => f32_to_h(a) as u32,
                        _ => a.to_bits(),
                    };
                    s.v[rd as usize] = bits as u128;
                }
                1 => {
                    let a = f64::from_bits(rn_v as u64);
                    let bits: u64 = match opcode {
                        0x010 => a.to_bits(),
                        0x030 => a.abs().to_bits(),
                        0x050 => (-a).to_bits(),
                        0x070 => {
                            if a < 0.0 {
                                s.fpsr |= 1 << 7;
                                fp_default_nan_d()
                            } else {
                                a.sqrt().to_bits()
                            }
                        }
                        0x110 => fp_round_d(a, 0).to_bits(),
                        0x130 => fp_round_d(a, 1).to_bits(),
                        0x150 => fp_round_d(a, 2).to_bits(),
                        0x170 => fp_round_d(a, 3).to_bits(),
                        0x190 => {
                            let r = a.round();
                            if (a - a.trunc()).abs() == 0.5 {
                                if a > 0.0 {
                                    a.trunc() + 1.0
                                } else {
                                    a.trunc() - 1.0
                                }
                            } else {
                                r
                            }
                            .to_bits()
                        }
                        0x1d0 => fp_round_d(a, rounding).to_bits(),
                        0x1f0 => fp_round_d(a, rounding).to_bits(),
                        0x0b0 => fp_cvt_f32_to_f64(f32::from_bits(rn_v as u32)),
                        _ => a.to_bits(),
                    };
                    s.v[rd as usize] = bits as u128;
                }
                3 => {
                    let h = rn_v as u16;
                    let a = h_to_f32(h);
                    let out_h: u16 = match opcode {
                        0x010 => h,
                        0x030 => f32_to_h(a.abs()),
                        0x050 => f32_to_h(-a),
                        0x070 => f32_to_h(if a < 0.0 { 0.0 } else { a.sqrt() }),
                        0x0f0 => h,
                        _ => h,
                    };
                    let val = s.v[rd as usize];
                    s.v[rd as usize] = (val & !0xffff) | (out_h as u128);
                }
                _ => {}
            }
        }

        FpImm { imm8, typ, rd } => {
            let s = &mut m.cores.active_mut().state;
            match typ {
                0 => {
                    s.v[rd as usize] = fp_imm_to_f32(imm8).to_bits() as u128;
                }
                1 => {
                    s.v[rd as usize] = fp_imm_to_f64(imm8).to_bits() as u128;
                }
                3 => {
                    let h = fp_imm_to_h(imm8);
                    let val = s.v[rd as usize];
                    s.v[rd as usize] = (val & !0xffff) | (h as u128);
                }
                _ => {}
            }
        }

        FpGprConv {
            sf,
            typ,
            opc,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            match opc {
                6 => {
                    let v = s.v[rn as usize];
                    let val = match typ {
                        0 => (v as u32) as u64,
                        1 => v as u64,
                        3 => (v as u16) as u64,
                        _ => 0,
                    };
                    if sf == 0 {
                        wr(s, rd, (val as u32) as u64);
                    } else {
                        wr(s, rd, val);
                    }
                }
                7 => {
                    let g = reg(s, rn);
                    let val = match typ {
                        0 => (g as u32) as u128,
                        1 => g as u128,
                        3 => (g as u16) as u128,
                        _ => 0,
                    };
                    s.v[rd as usize] = val;
                }
                2 | 3 => {
                    let g = reg(s, rn);
                    let signed = opc == 2;
                    let bits = match typ {
                        0 => {
                            let f = if signed {
                                (g as i32 as i64) as f32
                            } else {
                                g as f32
                            };
                            f.to_bits() as u128
                        }
                        1 => {
                            let f = if signed { (g as i64) as f64 } else { g as f64 };
                            f.to_bits() as u128
                        }
                        3 => {
                            let f = if signed {
                                (g as i32 as i64) as f32
                            } else {
                                g as f32
                            };
                            f32_to_h(f) as u128
                        }
                        _ => 0,
                    };
                    s.v[rd as usize] = bits;
                }
                _ => {
                    let rn_v = s.v[rn as usize];
                    let rmode = match opc {
                        0 | 1 => 0,
                        4 | 5 => 1,
                        8 | 9 => 1,
                        16 | 17 => 2,
                        24 | 25 => 3,
                        _ => 3,
                    };
                    let signed = opc != 1 && opc != 5 && opc != 9 && opc != 17 && opc != 25;
                    let res: i64 = match typ {
                        0 => {
                            let a = f32::from_bits(rn_v as u32);
                            let rounded = fp_round_s(a, rmode);
                            if signed {
                                rounded as i64
                            } else {
                                rounded as u64 as i64
                            }
                        }
                        1 => {
                            let a = f64::from_bits(rn_v as u64);
                            let rounded = fp_round_d(a, rmode);
                            if signed {
                                rounded as i64
                            } else {
                                rounded as u64 as i64
                            }
                        }
                        3 => {
                            let a = h_to_f32(rn_v as u16);
                            let rounded = fp_round_s(a, rmode);
                            if signed {
                                rounded as i64
                            } else {
                                rounded as u64 as i64
                            }
                        }
                        _ => 0,
                    };
                    if sf == 0 {
                        wr(s, rd, (res as u32) as u64);
                    } else {
                        wr(s, rd, res as u64);
                    }
                }
            }
        }

        FpFixed {
            sf,
            typ,
            u: _,
            to_fp,
            imm6,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let fbits = 64 - imm6;
            let scale = (2.0f64).powi(-(fbits as i32));
            if to_fp {
                let rn_v = s.v[rn as usize];
                let bits = match typ {
                    0 => {
                        let intval = (rn_v as u32) as i32 as i64;
                        let f = (intval as f64) * scale;
                        (f as f32).to_bits() as u128
                    }
                    1 => {
                        let intval = rn_v as i64;
                        let f = (intval as f64) * scale;
                        f.to_bits() as u128
                    }
                    _ => 0,
                };
                s.v[rd as usize] = bits;
            } else {
                let rn_v = s.v[rn as usize];
                let val: i64 = match typ {
                    0 => {
                        let a = f32::from_bits(rn_v as u32);
                        let scaled = (a as f64) * (2.0f64).powi(fbits as i32);
                        scaled.trunc() as i64
                    }
                    1 => {
                        let a = f64::from_bits(rn_v as u64);
                        let scaled = a * (2.0f64).powi(fbits as i32);
                        scaled.trunc() as i64
                    }
                    _ => 0,
                };
                if sf == 0 {
                    wr(s, rd, (val as u32) as u64);
                } else {
                    wr(s, rd, val as u64);
                }
            }
        }

        FpRegConv {
            unsigned,
            to_fp,
            prec,
            rn,
            rd,
        } => {
            // AdvSIMD scalar float<->int conversions with a vector-register
            // destination. to_fp = SCVTF/UCVTF (integer -> float); otherwise
            // FCVTZS/FCVTZU (float -> integer, round toward zero). Element
            // width follows the precision: half=16, single=32, double=64.
            let s = &mut m.cores.active_mut().state;
            let rn_v = s.v[rn as usize];
            let out: u128 = if to_fp {
                if unsigned {
                    match prec {
                        1 => (rn_v as u64 as f64).to_bits() as u128,
                        0 => ((rn_v as u32) as f32 as f64).to_bits() as u128,
                        _ => ((rn_v as u16) as f32 as f64).to_bits() as u128,
                    }
                } else {
                    match prec {
                        1 => (rn_v as i64 as f64).to_bits() as u128,
                        0 => ((rn_v as u32) as i32 as f32 as f64).to_bits() as u128,
                        _ => ((rn_v as u16) as i16 as f32 as f64).to_bits() as u128,
                    }
                }
            } else {
                // FCVTZS/FCVTZU: fp -> int, round toward zero. NaN/Inf -> 0.
                let trunc = |v: f64| -> i64 {
                    if v.is_nan() || v.is_infinite() {
                        0
                    } else {
                        v.trunc() as i64
                    }
                };
                // FCVTZS/FCVTZU store the truncated integer in a 16/32/64-bit
                // lane; the lane holds the raw bit pattern for both signed and
                // unsigned destinations.
                match prec {
                    1 => {
                        let a = f64::from_bits(rn_v as u64);
                        (trunc(a) as u64) as u128
                    }
                    0 => {
                        let a = f32::from_bits(rn_v as u32) as f64;
                        (trunc(a) as u32) as u128
                    }
                    _ => {
                        let a = h_to_f32(rn_v as u16) as f64;
                        (trunc(a) as u16) as u128
                    }
                }
            };
            s.v[rd as usize] = out;
        }

        Aes { op, rn, rd } => {
            let state = m.cores.active_mut().state.v[rd as usize].to_le_bytes();
            let input = m.cores.active_mut().state.v[rn as usize].to_le_bytes();
            let result = match op {
                4 => aes_round(state, input, false), // AESE
                5 => aes_round(state, input, true),  // AESD
                6 => aes_mix_columns(input, false),  // AESMC
                7 => aes_mix_columns(input, true),   // AESIMC
                _ => unreachable!("unknown decoded AES operation {op}"),
            };
            m.cores.active_mut().state.v[rd as usize] = u128::from_le_bytes(result);
        }
        Sha256 { op, rn, rm, rd } => {
            let d = m.cores.active_mut().state.v[rd as usize];
            let n = m.cores.active_mut().state.v[rn as usize];
            let msg = m.cores.active_mut().state.v[rm as usize];
            m.cores.active_mut().state.v[rd as usize] = sha256_simd(op, d, n, msg);
        }
        // ═══ Phase 190 SVE (VL = the emulated vector length, 128 bits) ════
        // The Z <=> V overlap means the low 128 bits of a Z register live in
        // `state.v[z]`; `state.p` holds the 16 predicate registers as 128-bit
        // bitmasks (LSB = lowest-numbered element). VL is fixed at 128: the
        // ZCR_EL1/2 writers clamp VQ to 0, so any userland value would also
        // request the 128-bit VL the core implements.
        SveWhile {
            size,
            sf,
            unsigned,
            le,
            rn,
            rm,
            pd,
        } => {
            let esize = 8u32 << size;
            let lanes = 128 / esize;
            let rn_v = m.cores.active_mut().state.x[rn as usize];
            let rm_v = m.cores.active_mut().state.x[rm as usize];
            let mut count = 0u32;
            if sf {
                for e in 0..lanes {
                    let a = rn_v.wrapping_add((e as u64) * (esize as u64));
                    let lt = if unsigned {
                        a < rm_v
                    } else {
                        (a as i64) < (rm_v as i64)
                    };
                    if if le { lt || a == rm_v } else { lt } {
                        count += 1;
                    } else {
                        break;
                    }
                }
            } else {
                let a0 = rn_v as u32;
                let b0 = rm_v as u32;
                for e in 0..lanes {
                    let a = a0.wrapping_add(e * esize);
                    let lt = if unsigned {
                        a < b0
                    } else {
                        (a as i32) < (b0 as i32)
                    };
                    if if le { lt || a == b0 } else { lt } {
                        count += 1;
                    } else {
                        break;
                    }
                }
            }
            m.cores.active_mut().state.p[pd as usize] = (1u128 << count) - 1;
        }
        SveCnt {
            size,
            pattern,
            imm,
            rd,
        } => {
            // Only the default ALL pattern (0b11111) is implemented exactly;
            // other patterns would require the PCOUNT constraint table, so the
            // honest behaviour is to trap rather than fabricate a count.
            if pattern != 0x1f {
                return Err(ExecError::Undefined);
            }
            let esize = 8u32 << size;
            let vg = 128 / esize;
            if rd != 31 {
                m.cores.active_mut().state.x[rd as usize] = (vg as u64) * (imm as u64);
            }
        }
        SveIncp {
            dec,
            scalar,
            size,
            pg,
            rd,
        } => {
            let esize = 8u32 << size;
            let lanes = 128 / esize;
            let pg_val = m.cores.active_mut().state.p[pg as usize];
            if scalar {
                let active = pg_val & ((1u128 << lanes) - 1);
                let count = active.count_ones() as u64;
                if rd != 31 {
                    let v = m.cores.active_mut().state.x[rd as usize];
                    m.cores.active_mut().state.x[rd as usize] = if dec {
                        v.wrapping_sub(count)
                    } else {
                        v.wrapping_add(count)
                    };
                }
            } else {
                let mask = (1u128 << esize) - 1;
                let z = m.cores.active_mut().state.v[rd as usize];
                let mut out = 0u128;
                for e in 0..lanes {
                    let lane = ((z >> (e * esize)) & mask) as u64;
                    let active = (pg_val & (1u128 << e)) != 0;
                    let lane = if active {
                        if dec {
                            lane.wrapping_sub(1)
                        } else {
                            lane.wrapping_add(1)
                        }
                    } else {
                        lane
                    };
                    out |= (lane as u128 & mask) << (e * esize);
                }
                m.cores.active_mut().state.v[rd as usize] = out;
            }
        }
        SvePredLogical {
            func,
            set,
            pd,
            pg,
            pn,
            pm,
        } => {
            // Byte-granular predicate logic: one 8-bit element per predicate
            // byte, gated by PG (unset elements zero the destination).
            let pgn = m.cores.active_mut().state.p[pg as usize];
            let pnn = m.cores.active_mut().state.p[pn as usize];
            let pmn = m.cores.active_mut().state.p[pm as usize];
            let mut out = 0u128;
            for e in 0..16u32 {
                if pgn & (0xffu128 << (e * 8)) != 0 {
                    let a = ((pnn >> (e * 8)) & 0xff) as u8;
                    let b = ((pmn >> (e * 8)) & 0xff) as u8;
                    out |= (sve_pred_bool(func, a, b) as u128) << (e * 8);
                }
            }
            if set {
                m.cores.active_mut().state.nzcv &= !0x3;
                if out == 0 {
                    m.cores.active_mut().state.nzcv |= 0x2;
                }
                if out & (1 << 127) != 0 {
                    m.cores.active_mut().state.nzcv |= 0x1;
                }
            }
            m.cores.active_mut().state.p[pd as usize] = out;
        }
        SveLd1 {
            size,
            imm,
            pg,
            rn,
            zt,
        } => {
            let lsize = 8u32 << size;
            let lanes = 128 / lsize;
            let elbytes = 1u32 << size;
            let base = m.cores.active_mut().state.x[rn as usize]
                .wrapping_add((imm.wrapping_mul(SVE_VL_BYTES)) as u64);
            let active = m.cores.active_mut().state.p[pg as usize] & ((1u128 << lanes) - 1);
            let lane_mask = (1u128 << lsize) - 1;
            let mut z = m.cores.active_mut().state.v[zt as usize];
            for e in 0..lanes {
                if active & (1u128 << e) != 0 {
                    let addr = base.wrapping_add((e as u64) * (elbytes as u64));
                    let v = m.mem_read(addr, elbytes as usize)?;
                    z = (z & !(lane_mask << (e * lsize))) | ((v as u128) << (e * lsize));
                }
            }
            m.cores.active_mut().state.v[zt as usize] = z;
        }
        SveSt1 {
            size,
            imm,
            pg,
            rn,
            zt,
        } => {
            let lsize = 8u32 << size;
            let lanes = 128 / lsize;
            let elbytes = 1u32 << size;
            let base = m.cores.active_mut().state.x[rn as usize]
                .wrapping_add((imm.wrapping_mul(SVE_VL_BYTES)) as u64);
            let active = m.cores.active_mut().state.p[pg as usize] & ((1u128 << lanes) - 1);
            let lane_mask = (1u128 << lsize) - 1;
            let z = m.cores.active_mut().state.v[zt as usize];
            for e in 0..lanes {
                if active & (1u128 << e) != 0 {
                    let addr = base.wrapping_add((e as u64) * (elbytes as u64));
                    let v = (z >> (e * lsize)) & lane_mask;
                    m.mem_write(addr, elbytes as usize, v as u64)?;
                }
            }
        }
        NeonExt {
            q,
            offset,
            rn,
            rm,
            rd,
        } => {
            let width = if q { 128 } else { 64 };
            let mask = if q { u128::MAX } else { u64::MAX as u128 };
            let n = m.cores.active_mut().state.v[rn as usize] & mask;
            let other = m.cores.active_mut().state.v[rm as usize] & mask;
            let shift = offset * 8;
            m.cores.active_mut().state.v[rd as usize] = if shift == 0 {
                n
            } else {
                ((n >> shift) | (other << (width - shift))) & mask
            };
        }
        NeonUminv { q, size, rn, rd } => {
            let width = 8 << size;
            let lanes = if q { 128 } else { 64 } / width;
            let mask = (1u128 << width) - 1;
            let source = m.cores.active_mut().state.v[rn as usize];
            let mut minimum = mask;
            for lane in 0..lanes {
                minimum = minimum.min((source >> (lane * width)) & mask);
            }
            m.cores.active_mut().state.v[rd as usize] = minimum;
        }
        NeonUmaxv { q, size, rn, rd } => {
            let width = 8 << size;
            let lanes = if q { 128 } else { 64 } / width;
            let mask = (1u128 << width) - 1;
            let source = m.cores.active_mut().state.v[rn as usize];
            let mut maximum = 0u128;
            for lane in 0..lanes {
                maximum = maximum.max((source >> (lane * width)) & mask);
            }
            m.cores.active_mut().state.v[rd as usize] = maximum;
        }
        NeonRev64 { q, size, rn, rd } => {
            let active_bytes = if q { 16 } else { 8 };
            let element_bytes = 1usize << size;
            let source = m.cores.active_mut().state.v[rn as usize];
            let mut result = 0u128;
            for chunk in (0..active_bytes).step_by(8) {
                for lane in 0..(8 / element_bytes) {
                    let source_lane = (chunk + lane * element_bytes) * 8;
                    let dest_lane = (chunk + (7 / element_bytes - lane) * element_bytes) * 8;
                    let mask = (1u128 << (element_bytes * 8)) - 1;
                    result |= ((source >> source_lane) & mask) << dest_lane;
                }
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonAddLv {
            q,
            size,
            signed,
            rn,
            rd,
        } => {
            let lanes = if q { 16usize } else { 8 };
            let elem_bits = if size == 0 { 8 } else { 16 };
            let source = m.cores.active_mut().state.v[rn as usize];
            let mut sum: u128 = 0;
            for lane in 0..lanes {
                let shift = (lane * elem_bits) as u32;
                let lane_val: u128 = if elem_bits == 8 {
                    ((source >> shift) as u8) as u128
                } else {
                    ((source >> shift) as u16) as u128
                };
                if signed {
                    let signed_lane: i128 = if elem_bits == 8 {
                        (lane_val as u8 as i8) as i128
                    } else {
                        (lane_val as u16 as i16) as i128
                    };
                    sum = (sum as i128).wrapping_add(signed_lane) as u128;
                } else {
                    sum = sum.wrapping_add(lane_val);
                }
            }
            m.cores.active_mut().state.v[rd as usize] = sum;
        }
        NeonShiftImmediate {
            q,
            size,
            right,
            amount,
            rn,
            rd,
        } => {
            let width = 8usize << size;
            let lanes = (if q { 128 } else { 64 }) / width;
            let lane_mask = (1u128 << width) - 1;
            let source = m.cores.active_mut().state.v[rn as usize];
            let mut result = 0u128;
            for lane in 0..lanes {
                let value = (source >> (lane * width)) & lane_mask;
                let shifted = if right {
                    value >> amount
                } else {
                    (value << amount) & lane_mask
                };
                result |= shifted << (lane * width);
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonShiftLong {
            q,
            unsigned,
            src_size,
            amount,
            rn,
            rd,
        } => {
            // Shift-long widens every source element to double its width and
            // always fills the 128-bit destination. Q selects the high half of
            // the source register; the low `amount` element count lanes of the
            // widened register are produced from the matching source lanes.
            let src_bits = src_size as usize;
            let dst_bits = src_bits * 2;
            let lanes = 128 / dst_bits;
            let src = m.cores.active_mut().state.v[rn as usize];
            let src_mask = (1u128 << src_bits) - 1;
            let dst_mask = (1u128 << dst_bits) - 1;
            let mut result = 0u128;
            for lane in 0..lanes {
                let at = (if q { lanes } else { 0 } + lane) * src_bits;
                let value = (src >> at) & src_mask;
                let extended = if unsigned {
                    value
                } else {
                    let signed = match src_bits {
                        8 => (value as u8 as i8) as i128,
                        16 => (value as u16 as i16) as i128,
                        32 => (value as u32 as i32) as i128,
                        _ => 0,
                    };
                    signed as u128
                };
                result |= ((extended << amount) & dst_mask) << (lane * dst_bits);
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonShiftReg {
            q,
            unsigned,
            size,
            scalar,
            rn,
            rm,
            rd,
        } => {
            // USHL/SSHL: every lane of Vn is shifted by the SIGNED count in
            // the matching lane of Vm. Positive counts shift left (zeros
            // entered); negative counts shift right (arithmetic for SSHL,
            // logical/zero-fill for USHL); counts at/over the element width
            // give 0, or all-ones for a negative-valued SSHL lane.
            let esize = match size {
                0 => 32,
                1 => 64,
                _ => 16,
            };
            let lanes = if scalar {
                1
            } else if q {
                128 / esize
            } else {
                64 / esize
            };
            let elem_mask = (1u128 << esize) - 1;
            let src = m.cores.active_mut().state.v[rn as usize];
            let cnt = m.cores.active_mut().state.v[rm as usize];
            let mut result = 0u128;
            for lane in 0..lanes {
                let value = (src >> (lane * esize)) & elem_mask;
                let shift_bits = {
                    let raw = (cnt >> (lane * esize)) & elem_mask;
                    match esize {
                        64 => raw as u64 as i64,
                        32 => raw as u32 as i32 as i64,
                        _ => raw as u16 as i16 as i64,
                    }
                };
                let shifted: u64 = {
                    let value = value as u64;
                    if shift_bits > 0 {
                        let c = shift_bits as u32;
                        if c >= esize {
                            0
                        } else {
                            (value << c) & elem_mask as u64
                        }
                    } else if shift_bits < 0 {
                        let c = (-shift_bits) as u32;
                        if c >= esize {
                            if unsigned {
                                0
                            } else {
                                let signed_value: i64 = match esize {
                                    64 => value as i64,
                                    32 => (value as u32 as i32) as i64,
                                    _ => (value as u16 as i16) as i64,
                                };
                                if signed_value < 0 {
                                    elem_mask as u64
                                } else {
                                    0
                                }
                            }
                        } else if unsigned {
                            (value >> c) & elem_mask as u64
                        } else {
                            let signed_value: i64 = match esize {
                                64 => value as i64,
                                32 => (value as u32 as i32) as i64,
                                _ => (value as u16 as i16) as i64,
                            };
                            ((signed_value >> c) as u64) & elem_mask as u64
                        }
                    } else {
                        value
                    }
                };
                result |= (shifted as u128) << (lane * esize);
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonUmov {
            size,
            index,
            rn,
            rd,
        } => {
            // UMOV: extract one element of Vn into a zero-extended
            // general-purpose register.
            let element_bits = match size {
                0 => 32, // .s
                1 => 64, // .d
                _ => 8,  // .b
            };
            let value = (m.cores.active_mut().state.v[rn as usize] >> (index * element_bits))
                & ((1u128 << element_bits) - 1);
            wr(&mut m.cores.active_mut().state, rd, value as u64);
        }
        NeonMovi64 { q, imm, rd } => {
            let mut value = 0u128;
            for byte in 0..8 {
                if imm & (1 << byte) != 0 {
                    value |= 0xffu128 << (byte * 8);
                }
            }
            m.cores.active_mut().state.v[rd as usize] =
                if q { value | (value << 64) } else { value };
        }
        NeonMovi { q, bits, value, rd } => {
            let mut result = 0;
            for lane in 0..(if q { 128 } else { 64 }) / bits {
                result |= (value as u128) << (lane * bits);
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonDupGeneral { q, size, rn, rd } => {
            let bits = 8 << size;
            let value = reg(&m.cores.active().state, rn) as u128 & ((1u128 << bits) - 1);
            let mut result = 0;
            for lane in 0..(if q { 128 } else { 64 }) / bits {
                result |= value << (lane * bits);
            }
            m.cores.active_mut().state.v[rd as usize] = result;
        }
        NeonCopy {
            q,
            op: cop,
            size,
            index,
            elem: _,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            match cop {
                0 => {
                    let elem_size = 8 << size;
                    let mask = (1u128 << elem_size) - 1;
                    let val = (vn >> (index * elem_size)) & mask;
                    let mut res = 0u128;
                    let count = if q { 128 / elem_size } else { 64 / elem_size };
                    for i in 0..count {
                        res |= val << (i * elem_size);
                    }
                    s.v[rd as usize] = res;
                }
                1 => {
                    let g = reg(s, rn);
                    let elem_size = 8 << size;
                    let mask = (1u128 << elem_size) - 1;
                    let mut vd = s.v[rd as usize];
                    vd &= !(mask << (index * elem_size));
                    vd |= ((g as u128) & mask) << (index * elem_size);
                    s.v[rd as usize] = vd;
                }
                2 | 3 => {
                    let elem_size = 8 << size;
                    let mask = (1u128 << elem_size) - 1;
                    let val = (vn >> (index * elem_size)) & mask;
                    let sign_ext = cop == 2;
                    let res = if sign_ext {
                        match elem_size {
                            8 => (val as i8 as i64) as u64,
                            16 => (val as i16 as i64) as u64,
                            32 => (val as i32 as i64) as u64,
                            _ => val as u64,
                        }
                    } else {
                        val as u64
                    };
                    wr(s, rd, res);
                }
                _ => {}
            }
        }

        NeonThreeSame {
            q,
            op: three_op,
            size,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let vm = s.v[rm as usize];
            let vd = s.v[rd as usize];
            match three_op {
                NeonThreeSameOp::FAdd
                | NeonThreeSameOp::FSub
                | NeonThreeSameOp::FMul
                | NeonThreeSameOp::FDiv
                | NeonThreeSameOp::FMax
                | NeonThreeSameOp::FMin
                | NeonThreeSameOp::FMaxNmp
                | NeonThreeSameOp::FMinNmp
                | NeonThreeSameOp::FMla
                | NeonThreeSameOp::FMls => {
                    // Vector FP: width = size bit0 (bit22): 0 = S(32), 1 = D(64).
                    let width = size & 1;
                    s.v[rd as usize] =
                        neon_fp_three_same(three_op, width, neon_fp_lanes(q, width), vn, vm, vd);
                }
                _ => {
                    let lanes = neon_lanes(q, size);
                    s.v[rd as usize] = neon_three_same(three_op, size, lanes, vn, vm, vd);
                }
            }
        }

        NeonBitwise {
            q,
            op: bop,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let vm = s.v[rm as usize];
            let vd = s.v[rd as usize];
            let mask = if q { u128::MAX } else { u128::MAX >> 64 };
            s.v[rd as usize] = neon_bitwise(bop, vn, vm, vd, mask);
        }

        NeonOneSrc {
            q,
            op: osrc,
            size,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let lanes = neon_lanes(q, size);
            s.v[rd as usize] = neon_one_src(osrc, size, lanes, vn);
        }

        NeonFpOneSrc {
            q,
            fneg,
            sqrt,
            size,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let width = size & 1;
            let lanes = neon_fp_lanes(q, width);
            s.v[rd as usize] = neon_fp_one_src(fneg, sqrt, width, lanes, vn);
        }

        // ── ARMv8.3 RCpc load acquires ───────────────────────────────────
        Ldapr {
            size,
            sign_extend,
            rn,
            rt,
            imm,
        } => {
            let base = reg_sp(&m.cores.active().state, rn);
            let addr = base.wrapping_add(imm as u64);
            let bytes = 1usize << size;
            let mut v = m.mem_read(addr, bytes)?;
            if sign_extend {
                v = crate::sign_extend(v, (bytes * 8) as u32);
            }
            wr(&mut m.cores.active_mut().state, rt, v);
        }

        // ── ARMv8.2 Dot Product ──────────────────────────────────────────
        Dot {
            unsigned,
            q,
            elem,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let vm = s.v[rm as usize];
            let vd = s.v[rd as usize];
            let lanes = if q { 4usize } else { 2 };
            let mut result = vd;
            for l in 0..lanes {
                let d_lane = ((vd >> (l * 32)) as u32) as i64;
                let mut acc: i64 = d_lane;
                let src_vm = if let Some(e) = elem {
                    (vm >> (e as usize * 32)) as u32
                } else {
                    (vm >> (l * 32)) as u32
                };
                for b in 0..4usize {
                    let a_byte = (vn >> (l * 32 + b * 8)) as u8;
                    let b_byte = (src_vm >> (b * 8)) as u8;
                    if unsigned {
                        acc += (a_byte as i64) * (b_byte as i64);
                    } else {
                        acc += (a_byte as i8 as i64) * (b_byte as i8 as i64);
                    }
                }
                let shift = l * 32;
                let lane_mask = 0xffff_ffffu128 << shift;
                result = (result & !lane_mask) | (((acc as u32) as u128) << shift);
            }
            s.v[rd as usize] = result;
        }

        // ── ARMv8.3 FCADD / FCMLA ───────────────────────────────────────
        FcComplexArith {
            is_cmla,
            sz: _,
            q,
            rot,
            rm,
            rn,
            rd,
        } => {
            let s = &mut m.cores.active_mut().state;
            let vn = s.v[rn as usize];
            let vm = s.v[rm as usize];
            let mut vd = s.v[rd as usize];
            let pairs = if q { 2usize } else { 1 };
            for p in 0..pairs {
                let re_n = f32::from_bits((vn >> (p * 64)) as u32);
                let im_n = f32::from_bits((vn >> (p * 64 + 32)) as u32);
                let re_m = f32::from_bits((vm >> (p * 64)) as u32);
                let im_m = f32::from_bits((vm >> (p * 64 + 32)) as u32);
                let re_d = f32::from_bits((vd >> (p * 64)) as u32);
                let im_d = f32::from_bits((vd >> (p * 64 + 32)) as u32);
                let (re_out, im_out) = if is_cmla {
                    let (rm_re, rm_im) = match rot {
                        0 => (re_m, im_m),
                        1 => (-im_m, re_m),
                        2 => (-re_m, -im_m),
                        _ => (im_m, -re_m),
                    };
                    (
                        re_d + re_n * rm_re - im_n * rm_im,
                        im_d + re_n * rm_im + im_n * rm_re,
                    )
                } else {
                    match rot {
                        1 => (re_n - im_m, im_n + re_m),
                        _ => (re_n + im_m, im_n - re_m),
                    }
                };
                let mask: u128 = !(0xffff_ffff_ffff_ffffu128 << (p * 64));
                let re_bits = (re_out.to_bits() as u128) << (p * 64);
                let im_bits = (im_out.to_bits() as u128) << (p * 64 + 32);
                vd = (vd & mask) | re_bits | im_bits;
            }
            if !q {
                vd &= u128::MAX >> 64;
            }
            s.v[rd as usize] = vd;
        }
    }
    Ok(())
}

fn ldrstr(m: &mut Machine, opc: u32, size: u32, rt: u32, addr: u64) -> Result<(), ExecError> {
    let bytes = 1usize << size;
    match opc {
        0b00 => {
            // Scalar load/store encoding: opc=00 is STR, opc=01 is LDR.
            let v = reg(&m.cores.active().state, rt);
            m.mem_write(addr, bytes, v)?;
        }
        0b01 => {
            let v = m.mem_read(addr, bytes)?;
            wr(&mut m.cores.active_mut().state, rt, v);
        }
        0b10 => {
            // LDRSB/LDRSH/LDRSW to X register.
            let v = m.mem_read(addr, bytes)?;
            let v = sign_extend(v, (bytes * 8) as u32);
            wr(&mut m.cores.active_mut().state, rt, v);
        }
        0b11 => {
            // LDRSB/LDRSH to W register: sign-extend to 32 bits, then the
            // architectural W-register write clears the upper half.
            let v = m.mem_read(addr, bytes)?;
            let v = sign_extend(v, (bytes * 8) as u32) & u32::MAX as u64;
            wr(&mut m.cores.active_mut().state, rt, v);
        }
        _ => {}
    }
    Ok(())
}

/// SIMD&FP single-register transfer to/from memory (`Vt`, X referenced by
/// `rn`). `opc` bit1 (bit23) selects the 128-bit Q access (split into two
/// 8-byte sub-transfers); otherwise the element width is `1 << size` bytes.
/// B/H/S/D loads write only the low bytes of the destination lane; the upper
/// bits of the 128-bit V register are architecturally unchanged.
fn simd_xfer(m: &mut Machine, opc: u32, size: u32, rt: u32, addr: u64) -> Result<(), ExecError> {
    let bytes = 1usize << size;
    if opc >= 2 {
        // 128-bit Q load/store (two 8-byte sub-transfers).
        if opc & 1 != 0 {
            let lo = m.mem_read(addr, 8)?;
            let hi = m.mem_read(addr + 8, 8)?;
            m.cores.active_mut().state.v[rt as usize] = ((hi as u128) << 64) | lo as u128;
        } else {
            let v = m.cores.active_mut().state.v[rt as usize];
            m.mem_write(addr, 8, v as u64)?;
            m.mem_write(addr + 8, 8, (v >> 64) as u64)?;
        }
    } else {
        let mask = if size == 3 {
            u64::MAX as u128
        } else {
            ((1u64 << (bytes * 8)).wrapping_sub(1)) as u128
        };
        if opc & 1 != 0 {
            let v = m.mem_read(addr, bytes)?;
            let lane = &mut m.cores.active_mut().state.v[rt as usize];
            *lane = (*lane & !mask) | (v as u128 & mask);
        } else {
            let v = m.cores.active_mut().state.v[rt as usize];
            m.mem_write(addr, bytes, v as u64)?;
        }
    }
    Ok(())
}

fn nzcv_from(v: u64, width: u32) -> u32 {
    let mask = if width == 32 {
        u32::MAX as u64
    } else {
        u64::MAX
    };
    let value = v & mask;
    let n = ((value >> (width - 1)) & 1) as u32;
    let z = if value == 0 { 1 } else { 0 };
    let c = 0;
    let v_bit = 0;
    (n << 3) | (z << 2) | (c << 1) | v_bit
}

fn rev16_64(v: u64) -> u64 {
    let b = v.to_le_bytes();
    let mut o = [0u8; 8];
    for i in 0..4 {
        o[i * 2] = b[i * 2 + 1];
        o[i * 2 + 1] = b[i * 2];
    }
    u64::from_le_bytes(o)
}

fn rev16_32(v: u32) -> u32 {
    ((v & 0x00ff_00ff) << 8) | ((v & 0xff00_ff00) >> 8)
}

fn rev32_64(v: u64) -> u64 {
    ((v as u32).swap_bytes() as u64) | (((v >> 32) as u32).swap_bytes() as u64) << 32
}

fn cls64(v: u64) -> u32 {
    if v >> 63 == 0 {
        v.leading_zeros().saturating_sub(1)
    } else {
        (!v).leading_zeros().saturating_sub(1)
    }
}

fn cls32(v: u32) -> u32 {
    if v >> 31 == 0 {
        v.leading_zeros().saturating_sub(1)
    } else {
        (!v).leading_zeros().saturating_sub(1)
    }
}

fn bitfield_op(opc: u32, source: u64, dest: u64, immr: u32, imms: u32, width: u32) -> u64 {
    let is64 = width == 64;
    let regmask = full(is64 as u32);
    let source = source & regmask;
    let dest = dest & regmask;
    let Some((wmask, tmask)) = decode_bit_masks(is64 as u32, immr, imms, is64, false) else {
        return 0;
    };
    let bot = ror(source, immr, width) & wmask;
    match opc {
        0 => {
            let sign_bit = (source >> (imms & (width - 1))) & 1;
            let top = if sign_bit != 0 { regmask } else { 0 };
            ((top & !tmask) | (bot & tmask)) & regmask
        }
        1 => {
            let merged = (dest & !wmask) | bot;
            ((dest & !tmask) | (merged & tmask)) & regmask
        }
        2 => bot & tmask,
        _ => 0,
    }
}

fn logical3(opc: u32, a: u64, b: u64) -> u64 {
    match opc {
        0 => a & b,
        1 => a | b,
        2 => a ^ b,
        3 => a & b,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Floating-point helpers.
// ---------------------------------------------------------------------------

fn fp_rounding_mode(fpcr: u64) -> u32 {
    ((fpcr >> 22) & 3) as u32
}

// ---------------------------------------------------------------------------
// Advanced SIMD (NEON) lane helpers.
// ---------------------------------------------------------------------------

fn sha256_small_sigma0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

fn sha256_small_sigma1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

fn sha256_big_sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

fn sha256_big_sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

fn sha256_words(value: u128) -> [u32; 4] {
    std::array::from_fn(|i| (value >> (i * 32)) as u32)
}

fn sha256_pack(words: [u32; 4]) -> u128 {
    words
        .iter()
        .enumerate()
        .fold(0, |acc, (i, word)| acc | ((*word as u128) << (i * 32)))
}

fn sha256_hash(x: u128, y: u128, schedule: u128, part1: bool) -> u128 {
    let mut state = [0u32; 8];
    state[..4].copy_from_slice(&sha256_words(x));
    state[4..].copy_from_slice(&sha256_words(y));
    let schedule = sha256_words(schedule);

    for word in schedule {
        let [a, b, c, d, e, f, g, h] = state;
        let choose = (a & f) ^ (!a & g);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t1 = h
            .wrapping_add(sha256_big_sigma1(e))
            .wrapping_add(choose)
            .wrapping_add(word);
        let t2 = sha256_big_sigma0(a).wrapping_add(majority);
        state[3] = d.wrapping_add(t1);
        state[7] = t1.wrapping_add(t2);
        state.rotate_right(1);
    }

    if part1 {
        sha256_pack(state[..4].try_into().unwrap())
    } else {
        sha256_pack(state[4..].try_into().unwrap())
    }
}

fn sha256_simd(op: u32, d: u128, n: u128, m: u128) -> u128 {
    let dw = sha256_words(d);
    let nw = sha256_words(n);
    let mw = sha256_words(m);
    match op {
        0 => sha256_hash(d, n, m, true),
        1 => sha256_hash(n, d, m, false),
        2 => {
            let t = [dw[1], dw[2], dw[3], nw[0]];
            sha256_pack(std::array::from_fn(|i| {
                dw[i].wrapping_add(sha256_small_sigma0(t[i]))
            }))
        }
        3 => {
            let t0 = [nw[1], nw[2], nw[3], mw[0]];
            let mut result = [0u32; 4];
            for i in 0..2 {
                result[i] = dw[i]
                    .wrapping_add(t0[i])
                    .wrapping_add(sha256_small_sigma1(mw[i + 2]));
            }
            for i in 2..4 {
                result[i] = dw[i]
                    .wrapping_add(t0[i])
                    .wrapping_add(sha256_small_sigma1(result[i - 2]));
            }
            sha256_pack(result)
        }
        _ => unreachable!("unknown decoded SHA-256 operation {op}"),
    }
}

fn aes_mul(mut a: u8, mut b: u8) -> u8 {
    let mut product = 0;
    for _ in 0..8 {
        if b & 1 != 0 {
            product ^= a;
        }
        let high = a & 0x80 != 0;
        a <<= 1;
        if high {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    product
}

fn aes_inverse(value: u8) -> u8 {
    if value == 0 {
        return 0;
    }
    let mut result = 1;
    let mut base = value;
    let mut exponent = 254;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = aes_mul(result, base);
        }
        base = aes_mul(base, base);
        exponent >>= 1;
    }
    result
}

fn aes_compute_sub_byte(value: u8) -> u8 {
    let inverse = aes_inverse(value);
    inverse
        ^ inverse.rotate_left(1)
        ^ inverse.rotate_left(2)
        ^ inverse.rotate_left(3)
        ^ inverse.rotate_left(4)
        ^ 0x63
}

fn aes_compute_inv_sub_byte(value: u8) -> u8 {
    let inverse_affine = value.rotate_left(1) ^ value.rotate_left(3) ^ value.rotate_left(6) ^ 0x05;
    aes_inverse(inverse_affine)
}

fn aes_sub_byte(value: u8) -> u8 {
    static TABLE: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| aes_compute_sub_byte(i as u8)))[value as usize]
}

fn aes_inv_sub_byte(value: u8) -> u8 {
    static TABLE: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| aes_compute_inv_sub_byte(i as u8)))[value as usize]
}

fn aes_shift_rows(input: [u8; 16], inverse: bool) -> [u8; 16] {
    let mut output = [0; 16];
    for column in 0..4 {
        for row in 0..4 {
            let source_column = if inverse {
                (column + 4 - row) % 4
            } else {
                (column + row) % 4
            };
            output[column * 4 + row] = input[source_column * 4 + row];
        }
    }
    output
}

fn aes_round(state: [u8; 16], key: [u8; 16], inverse: bool) -> [u8; 16] {
    let mut result = [0; 16];
    for i in 0..16 {
        result[i] = state[i] ^ key[i];
    }
    if inverse {
        result = aes_shift_rows(result, true);
        for byte in &mut result {
            *byte = aes_inv_sub_byte(*byte);
        }
    } else {
        for byte in &mut result {
            *byte = aes_sub_byte(*byte);
        }
        result = aes_shift_rows(result, false);
    }
    result
}

fn aes_mix_columns(input: [u8; 16], inverse: bool) -> [u8; 16] {
    let mut output = [0; 16];
    for column in 0..4 {
        let i = column * 4;
        let a = [input[i], input[i + 1], input[i + 2], input[i + 3]];
        let coefficients: [[u8; 4]; 4] = if inverse {
            [
                [14, 11, 13, 9],
                [9, 14, 11, 13],
                [13, 9, 14, 11],
                [11, 13, 9, 14],
            ]
        } else {
            [[2, 3, 1, 1], [1, 2, 3, 1], [1, 1, 2, 3], [3, 1, 1, 2]]
        };
        for row in 0..4 {
            output[i + row] = (0..4).fold(0, |acc, lane| {
                acc ^ aes_mul(a[lane], coefficients[row][lane])
            });
        }
    }
    output
}

/// Bit width of one element for a given three-same `size` (0=8b,1=16b,2=32b,
/// 3=64b).
fn neon_elem_bits(size: u32) -> u32 {
    8u32 << size
}

/// Number of integer element lanes in the active vector length for a `size`.
fn neon_lanes(q: bool, size: u32) -> u32 {
    let active = if q { 128 } else { 64 };
    active / neon_elem_bits(size)
}

/// Number of FP element lanes: width 0 = S(32-bit), 1 = D(64-bit).
fn neon_fp_lanes(q: bool, width: u32) -> u32 {
    let w = if width == 0 { 32 } else { 64 };
    let active = if q { 128 } else { 64 };
    active / w
}

/// Execute a three-register ASIMD integer op over `lanes` element lanes.
/// `vd` is the original destination value, needed by the MLA/MLS accumulate
/// forms.
fn neon_three_same(
    op: NeonThreeSameOp,
    size: u32,
    lanes: u32,
    vn: u128,
    vm: u128,
    vd: u128,
) -> u128 {
    use NeonThreeSameOp::*;
    let bits = neon_elem_bits(size);
    let mut acc: u128 = 0;
    for i in 0..lanes {
        let sh = i * bits;
        let mask = if bits == 64 {
            u64::MAX as u128
        } else {
            (1u128 << bits) - 1
        };
        let a = (vn >> sh) & mask;
        let b = (vm >> sh) & mask;
        let d = (vd >> sh) & mask;
        let r = match op {
            Cmeq => {
                if a == b {
                    mask
                } else {
                    0
                }
            }
            Cmhi => {
                if a > b {
                    mask
                } else {
                    0
                }
            }
            Add => a.wrapping_add(b),
            Sub => a.wrapping_sub(b),
            Mul => a.wrapping_mul(b),
            Mla => d.wrapping_add(a.wrapping_mul(b)),
            Mls => d.wrapping_sub(a.wrapping_mul(b)),
            _ => unreachable!("FP op in integer three-same"),
        };
        acc |= (r & mask) << sh;
    }
    acc
}

/// Execute a vector FP three-register op over S (32) or D (64) lanes.
/// `vd` is the original destination, needed by the FMLA/FMLS accumulate forms.
fn neon_fp_three_same(
    op: NeonThreeSameOp,
    width: u32,
    lanes: u32,
    vn: u128,
    vm: u128,
    vd: u128,
) -> u128 {
    let w = if width == 0 { 32 } else { 64 };
    let mut acc: u128 = 0;
    for i in 0..lanes {
        let sh = i * (w as u32);
        let r: u64 = if width == 0 {
            let a = f32::from_bits((vn >> sh) as u32);
            let b = f32::from_bits((vm >> sh) as u32);
            let d = f32::from_bits((vd >> sh) as u32);
            let fr = neon_fp_lane(op, a as f64, b as f64, d as f64) as f32;
            fr.to_bits() as u64
        } else {
            let a = f64::from_bits((vn >> sh) as u64);
            let b = f64::from_bits((vm >> sh) as u64);
            let d = f64::from_bits((vd >> sh) as u64);
            neon_fp_lane(op, a, b, d).to_bits()
        };
        acc |= (r as u128 & ((1u128 << w) - 1)) << sh;
    }
    acc
}

/// Compute one FP lane of a three-register op using f64 intermediates.
/// `d` is the original destination element, needed by FMLA/FMLS.
fn neon_fp_lane(op: NeonThreeSameOp, a: f64, b: f64, d: f64) -> f64 {
    match op {
        NeonThreeSameOp::FAdd => a + b,
        NeonThreeSameOp::FSub => a - b,
        NeonThreeSameOp::FMul => a * b,
        NeonThreeSameOp::FDiv => a / b,
        NeonThreeSameOp::FMax => a.max(b),
        NeonThreeSameOp::FMin => a.min(b),
        NeonThreeSameOp::FMaxNmp => {
            if a.is_nan() {
                b
            } else if b.is_nan() {
                a
            } else {
                a.max(b)
            }
        }
        NeonThreeSameOp::FMinNmp => {
            if a.is_nan() {
                b
            } else if b.is_nan() {
                a
            } else {
                a.min(b)
            }
        }
        NeonThreeSameOp::FMla => d + a * b,
        NeonThreeSameOp::FMls => d - a * b,
        _ => unreachable!("integer op in FP lane"),
    }
}

fn neon_bitwise_op(u: bool, size: u32) -> Option<NeonBitwiseOp> {
    use NeonBitwiseOp::*;
    // All ASIMD bitwise three-register forms share opcode 00011. The sub-op
    // is selected by the (U bit29, size bits[23:22]) pair (verified against
    // clang: AND/BIC/ORR/ORN with U=0 and EOR/BSL/BIT/BIF with U=1).
    Some(match (u, size) {
        (false, 0) => And,
        (false, 1) => Bic,
        (false, 2) => Orr,
        (false, 3) => Orn,
        (true, 0) => Eor,
        (true, 1) => Bsl,
        (true, 2) => Bit,
        (true, 3) => Bif,
        _ => return None,
    })
}

/// Execute a bitwise ASIMD op. `vd` is the original destination (third source
/// for BSL/BIT/BIF), and `mask` narrows to the active vector length.
fn neon_bitwise(op: NeonBitwiseOp, vn: u128, vm: u128, vd: u128, mask: u128) -> u128 {
    use NeonBitwiseOp::*;
    match op {
        And => vn & vm & mask,
        Bic => vn & !vm & mask,
        Eor => (vn ^ vm) & mask,
        Orr => (vn | vm) & mask,
        Orn => (vn | !vm) & mask,
        // BSL/BIT select Vn where Vm=1 else Vd; BIF selects Vn where Vm=0.
        Bsl => ((vn & vm) | (vd & !vm)) & mask,
        Bit => ((vn & vm) | (vd & !vm)) & mask,
        Bif => ((vn & !vm) | (vd & vm)) & mask,
    }
}

/// Execute an integer one-source ASIMD op.
fn neon_one_src(op: NeonOneSrcOp, size: u32, lanes: u32, vn: u128) -> u128 {
    use NeonOneSrcOp::*;
    let bits = neon_elem_bits(size);
    let bytes = (lanes * bits / 8) as usize;
    let mut acc: u128 = 0;
    if matches!(op, ByteCount | Rev16 | Rev32 | Rbit) {
        // Byte-wise operations: CNT is per-byte popcount, REV16/REV32 reverse
        // the byte order within each group, and RBIT mirrors the bits of every
        // byte. The total width equals the vector length regardless of the
        // `size` field, so these ignore lanes/bits.
        let mut out: u128 = 0;
        for i in 0..bytes {
            let b = match op {
                ByteCount => ((vn >> (i * 8)) & 0xff).count_ones() as u8,
                _ => ((vn >> (i * 8)) & 0xff) as u8,
            };
            out |= (b as u128) << (i * 8);
        }
        if matches!(op, Rev16 | Rev32) {
            let groups = match op {
                Rev16 => 2usize,
                Rev32 => 4,
                _ => unreachable!(),
            };
            let mut acc = 0u128;
            for g in 0..bytes / groups {
                for k in 0..groups {
                    let b = ((out >> ((g * groups + k) * 8)) & 0xff) as u8;
                    acc |= (b as u128) << ((g * groups + (groups - 1 - k)) * 8);
                }
            }
            return acc;
        }
        if op == Rbit {
            let mut rev = 0u128;
            for i in 0..bytes {
                let b = ((out >> (i * 8)) & 0xff) as u8;
                rev |= ((b.reverse_bits()) as u128) << (i * 8);
            }
            return rev;
        }
        return out;
    }
    for i in 0..lanes {
        let sh = i * bits;
        let mask = if bits == 64 {
            u64::MAX as u128
        } else {
            (1u128 << bits) - 1
        };
        let a = (vn >> sh) & mask;
        let r = match op {
            Abs => neon_abs(a, bits),
            Neg => ((!a).wrapping_add(1)) & mask,
            Not => !a & mask,
            ByteCount | Rev16 | Rev32 | Rbit => unreachable!(),
        };
        acc |= (r & mask) << sh;
    }
    acc
}

/// Absolute value of a `bits`-wide two's-complement lane.
fn neon_abs(a: u128, bits: u32) -> u128 {
    let sign = (a >> (bits - 1)) & 1;
    if sign == 1 {
        (!a).wrapping_add(1) & ((1u128 << bits) - 1)
    } else {
        a
    }
}

/// Execute a vector FP one-source op (FABS/FNEG/FSQRT).
fn neon_fp_one_src(fneg: bool, sqrt: bool, width: u32, lanes: u32, vn: u128) -> u128 {
    let w = if width == 0 { 32 } else { 64 };
    let mut acc: u128 = 0;
    for i in 0..lanes {
        let sh = i * w;
        let bits: u64 = if width == 0 {
            let a = f32::from_bits((vn >> sh) as u32);
            (if sqrt {
                a.sqrt()
            } else if fneg {
                -a
            } else {
                a.abs()
            })
            .to_bits() as u64
        } else {
            let a = f64::from_bits((vn >> sh) as u64);
            (if sqrt {
                a.sqrt()
            } else if fneg {
                -a
            } else {
                a.abs()
            })
            .to_bits()
        };
        acc |= (bits as u128 & ((1u128 << w) - 1)) << sh;
    }
    acc
}

/// Round a single to an integer with a rounding mode (0=RN,1=RP,2=RM,3=RZ).
fn fp_round_s(a: f32, rmode: u32) -> f32 {
    match rmode {
        1 => a.ceil(),
        2 => a.floor(),
        3 => a.trunc(),
        _ => {
            // Round-to-nearest-even approximation.
            let r = a.round();
            let diff = (a - a.trunc()).abs();
            if diff == 0.5 {
                let even = (r as i64) & 1 == 0;
                if even {
                    r
                } else {
                    // round toward even
                    if a > 0.0 {
                        a.trunc() + 1.0
                    } else {
                        a.trunc() - 1.0
                    }
                }
            } else {
                r
            }
        }
    }
}

/// Round a double to an integer with a rounding mode (0=RN,1=RP,2=RM,3=RZ).
fn fp_round_d(a: f64, rmode: u32) -> f64 {
    match rmode {
        1 => a.ceil(),
        2 => a.floor(),
        3 => a.trunc(),
        _ => {
            let r = a.round();
            let diff = (a - a.trunc()).abs();
            if diff == 0.5 {
                let even = (r as i64) & 1 == 0;
                if even {
                    r
                } else if a > 0.0 {
                    a.trunc() + 1.0
                } else {
                    a.trunc() - 1.0
                }
            } else {
                r
            }
        }
    }
}

/// Decode the 8-bit FP immediate into an f32 (AArch64 immediate encoding).
fn fp_imm_to_f32(imm8: u32) -> f32 {
    (fp_imm_parts(imm8) * fp_imm_scale(imm8)) as f32
}

/// Decode the 8-bit FP immediate into an f64 (AArch64 immediate encoding).
fn fp_imm_to_f64(imm8: u32) -> f64 {
    fp_imm_parts(imm8) * fp_imm_scale(imm8)
}

/// AArch64 FMOV immediate: sign=imm8[7], exp=imm8[6:4], mant=imm8[3:0].
/// value = (-1)^sign * (16 + mant) / 16 * 2^power, power = exp+1 for exp<=3,
/// power = exp-7 for exp>=4. Verified against clang encodings (2.0 -> 0x00,
/// 1.0 -> 0x70, 2.75 -> 0x06, 31.0 -> 0x3f, 0.125 -> 0x40, -1.0 -> 0xf0).
fn fp_imm_parts(imm8: u32) -> f64 {
    let sign = if imm8 & 0x80 != 0 { -1.0 } else { 1.0 };
    let mant = imm8 & 0xf;
    sign * (16.0 + mant as f64) / 16.0
}
fn fp_imm_scale(imm8: u32) -> f64 {
    let exp = ((imm8 >> 4) & 7) as i32;
    let power = if exp <= 3 { exp + 1 } else { exp - 7 };
    (2.0f64).powi(power)
}

fn fp_default_nan_s() -> u32 {
    0x7fc0_0000
}

fn fp_default_nan_d() -> u64 {
    0x7ff8_0000_0000_0000
}

fn fp_is_nan_s(v: u32) -> bool {
    (v & 0x7f80_0000) == 0x7f80_0000 && (v & 0x007f_ffff) != 0
}

fn fp_is_nan_d(v: u64) -> bool {
    (v & 0x7ff0_0000_0000_0000) == 0x7ff0_0000_0000_0000 && (v & 0x000f_ffff_ffff_ffff) != 0
}

fn fp_is_snan_s(v: u32) -> bool {
    fp_is_nan_s(v) && (v & 0x0040_0000) == 0
}

fn fp_is_snan_d(v: u64) -> bool {
    fp_is_nan_d(v) && (v & 0x0008_0000_0000_0000) == 0
}

fn fp_process_nan_s(v: u32) -> u32 {
    if fp_is_snan_s(v) {
        return fp_default_nan_s();
    }
    v
}

fn fp_process_nan_d(v: u64) -> u64 {
    if fp_is_snan_d(v) {
        return fp_default_nan_d();
    }
    v
}

/// Convert f64 to f32 with proper NaN handling. Returns the bit pattern.
fn fp_cvt_f64_to_f32(v: f64) -> u32 {
    let bits = v.to_bits();
    if fp_is_nan_d(bits) {
        return fp_process_nan_s(bits as u32);
    }
    let out = v as f32;
    out.to_bits()
}

/// Convert f32 to f64 with proper NaN handling. Returns the bit pattern.
fn fp_cvt_f32_to_f64(v: f32) -> u64 {
    let bits = v.to_bits();
    if fp_is_nan_s(bits) {
        return fp_process_nan_d(bits as u64);
    }
    let out = v as f64;
    out.to_bits()
}

pub fn fetch(m: &mut Machine) -> Result<Instruction, ExecError> {
    let pc = m.cores.active_mut().state.pc;
    let op = m.fetch32(pc).map_err(|_| ExecError::Fetch)?;
    Ok(Instruction(op))
}

/// Whether a fetch that just failed also took a synchronous exception.
///
/// `Machine::translate` takes the exception -- installing the vector PC, and
/// the matching ESR/ELR/FAR -- *before* the fetch reports its failure. So a
/// translation fault on an instruction fetch both records the exception and
/// errors, and a caller that treats the error as stage-terminal stops one step
/// before the handler it just vectored to would have run. Exception entry is
/// the step's architectural effect, so such a step completes.
///
/// A failure with the PC unmoved is different and stays an error: nothing took
/// the exception, so there is no vector to enter.
///
/// Callers pass the PC they fetched from, which must be the PC at entry to
/// their own step (before any fetch attempt).
#[must_use]
pub fn fetch_took_exception(m: &Machine, pc: u64) -> bool {
    m.cores.active().state.pc != pc
}

#[derive(Clone, Copy)]
enum AsyncException {
    Irq,
    Fiq,
    SError,
}

fn take_pending_async(m: &mut Machine, kind: AsyncException) -> bool {
    let interrupt = match kind {
        AsyncException::Irq => m.cores.active_mut().pending_irq.take().map(u64::from),
        AsyncException::Fiq => m.cores.active_mut().pending_fiq.take().map(u64::from),
        AsyncException::SError => m.cores.active_mut().pending_serror.take(),
    };
    let Some(interrupt) = interrupt else {
        return false;
    };

    // Architected: exception entry clears the local exclusive monitor.
    m.cores.active_mut().exclusive_address = None;
    m.cores.active_mut().exclusive_size = 0;

    let s = &mut m.cores.active_mut().state;
    let source_el = s.el;
    let source_spsel = s.spsel;
    let (scr_route, hcr_route) = match kind {
        AsyncException::Irq => (s.scr_el3 & (1 << 1) != 0, s.hcr_el2 & (1 << 4) != 0),
        AsyncException::Fiq => (s.scr_el3 & (1 << 2) != 0, s.hcr_el2 & (1 << 3) != 0),
        AsyncException::SError => (s.scr_el3 & (1 << 3) != 0, s.hcr_el2 & (1 << 5) != 0),
    };
    let target_el = if source_el != ExceptionLevel::El3 && scr_route {
        ExceptionLevel::El3
    } else if matches!(source_el, ExceptionLevel::El0 | ExceptionLevel::El1) && hcr_route {
        ExceptionLevel::El2
    } else {
        match source_el {
            ExceptionLevel::El0 => ExceptionLevel::El1,
            el => el,
        }
    };
    let return_pc = s.pc;
    let saved = saved_pstate(s);
    let vbar = match target_el {
        ExceptionLevel::El1 => {
            s.elr_el1 = return_pc;
            s.spsr_el1 = saved;
            if matches!(kind, AsyncException::SError) {
                s.esr_el1 = (0x2fu64 << 26) | interrupt;
                s.far_el1 = 0;
            }
            s.vbar_el1
        }
        ExceptionLevel::El2 => {
            s.elr_el2 = return_pc;
            s.spsr_el2 = saved;
            if matches!(kind, AsyncException::SError) {
                s.esr_el2 = (0x2fu64 << 26) | interrupt;
                s.far_el2 = 0;
            }
            s.vbar_el2
        }
        ExceptionLevel::El3 => {
            s.elr_el3 = return_pc;
            s.spsr_el3 = saved;
            if matches!(kind, AsyncException::SError) {
                s.esr_el3 = (0x2fu64 << 26) | interrupt;
                s.far_el3 = 0;
            }
            s.vbar_el3
        }
        ExceptionLevel::El0 => unreachable!(),
    };
    bank_active_sp(s);
    select_sp(s, target_el, true);
    let offset = if source_el == target_el {
        match (kind, source_spsel) {
            (AsyncException::Irq, false) => 0x80,
            (AsyncException::Fiq, false) => 0x100,
            (AsyncException::SError, false) => 0x180,
            (AsyncException::Irq, true) => 0x280,
            (AsyncException::Fiq, true) => 0x300,
            (AsyncException::SError, true) => 0x380,
        }
    } else {
        match kind {
            AsyncException::Irq => 0x480,
            AsyncException::Fiq => 0x500,
            AsyncException::SError => 0x580,
        }
    };
    s.pc = vbar + offset;
    s.daif |= 0b1111;
    m.trace.record(
        s.pc,
        interrupt,
        0,
        if matches!(kind, AsyncException::SError) {
            TraceKind::Exception
        } else {
            TraceKind::Interrupt
        },
        s.pc,
        s.el as u8,
        s.pstate,
        None,
    );
    true
}

pub fn step(m: &mut Machine) -> Result<(), ExecError> {
    // SError observes the A mask (DAIF bit 2) and is delivered before FIQ/IRQ.
    if m.cores.active_mut().state.daif & 0b0100 == 0
        && take_pending_async(m, AsyncException::SError)
    {
        return Ok(());
    }
    // FIQ has higher delivery priority and observes the F mask (DAIF bit 0).
    if m.cores.active_mut().state.daif & 0b0001 == 0 && take_pending_async(m, AsyncException::Fiq) {
        return Ok(());
    }
    // IRQ delivery honors the interrupt mask (I bit, PSTATE.DAIF bit 1).
    // A pending but masked IRQ is kept for a later cycle instead of being lost.
    let irq_unmasked = m.cores.active_mut().state.daif & 0b0010 == 0;
    if irq_unmasked && take_pending_async(m, AsyncException::Irq) {
        return Ok(());
    }

    // A core parked on WFI/WFE stays parked (no instruction executed) until an
    // interrupt wakes it; the machine advances cycles via tick_devices anyway.
    if matches!(
        m.cores.active_mut().run_state,
        CpuRunState::WaitingForInterrupt
    ) {
        return Ok(());
    }

    let pc = m.cores.active_mut().state.pc;
    // Execute any registered hook for this PC before instruction fetch
    if m.hooks.contains_key(&pc) {
        if let Some(mut hook) = m.hooks.remove(&pc) {
            hook(m);
            // Re-insert if it's not a one-shot hook (hooks are typically persistent)
            m.hooks.insert(pc, hook);
        }
    }
    let i = match fetch(m) {
        Ok(instruction) => instruction,
        Err(error) => {
            // A translation fault on the fetch itself is taken as a synchronous
            // exception by `Machine::translate`, which installs the vector PC
            // before the fetch reports failure. Exception entry *is* this
            // step's architectural effect, so the step completes and the next
            // one fetches from the vector -- otherwise the vector is recorded
            // in ESR/ELR/FAR and then never entered, which is how a faulting
            // fetch used to terminate a run one step too early.
            //
            // Only the taken-exception case is diverted. A fetch that fails
            // with the PC unmoved is a genuinely unhandled fault: the address
            // was not translated (or translation was off and no region claims
            // it), so there is no exception to enter and the error stands.
            if m.cores.active().state.pc != pc {
                return Ok(());
            }
            return Err(error);
        }
    };
    if std::env::var_os("IPHEM_TRACE_REGS").is_some() && (0x1000_08140..=0x1000_08170).contains(&pc)
    {
        let s = &m.cores.active().state;
        eprintln!(
            "ARM-REGTRACE pc={pc:#x} op={:#010x} x3={:#x} x8={:#x} x9={:#x} x11={:#x} x12={:#x} cpsr={:#x}",
            i.0,
            s.x[3], s.x[8], s.x[9], s.x[11], s.x[12], s.pstate
        );
    }
    m.current_instruction_pc = Some(pc);
    let result = execute(m, i);
    m.current_instruction_pc = None;
    match result {
        Err(ExecError::Data) => {
            // A plain bus failure leaves PC at the sequential address and has
            // no architectural exception entry to preserve. MMU/alignment
            // aborts have already installed a vector PC.
            if m.cores.active_mut().state.pc == pc.wrapping_add(4) {
                m.cores.active_mut().state.pc = pc;
                return Err(ExecError::Data);
            }
            // The PC is not the sequential address, so exception entry ran: the
            // abort's architectural effect is this step, and the next one
            // fetches from the vector. This is the same rule the
            // `ExecError::Synchronous` arm below already applies, and making
            // the two consistent is what lets a guest's own abort handler run
            // instead of the run stopping one instruction early. XNU's data
            // abort path is the first thing in this tree that depends on it.
            Ok(())
        }
        Err(ExecError::Undefined) => {
            m.cores.active_mut().state.pc = pc;
            let _ = exception(m, 0x00 << 26, 0);
            Ok(())
        }
        Err(ExecError::Synchronous) => {
            // Exception taken; CPU state already updated to vector PC.
            Ok(())
        }
        result => result,
    }
}

/// Advance devices and the scheduler by one cycle, then retire one instruction
/// on the selected core. Interactive front ends use this as their canonical
/// single-step operation so timers and interrupt delivery match boot runners.
pub fn step_with_device_tick(m: &mut Machine) -> Result<(), ExecError> {
    m.tick_devices(1);
    step(m)
}

/// Minimal human-readable disassembler over the internal decode tree. Used by
/// the CLI trace printer so firmware execution shows mnemonics instead of raw
/// words. `pc` is the address of `op` (absolute branch targets are rendered as
/// `target_pc`); failures fall back to a `UDF 0x...` label.
pub fn disassemble(pc: u64, op: u32) -> String {
    let Some(decoded) = decode(op) else {
        return format!("UDF 0x{:08x}", op);
    };
    render(pc, op, decoded)
}

fn xname(n: u32) -> String {
    if n == 31 {
        "xzr".into()
    } else {
        format!("x{n}")
    }
}

fn wname(n: u32) -> String {
    if n == 31 {
        "wzr".into()
    } else {
        format!("w{n}")
    }
}

fn gname(sf: u32, n: u32) -> String {
    if sf == 1 {
        xname(n)
    } else {
        wname(n)
    }
}

/// Register name for an add/sub-immediate operand. Encoding register 31 is
/// `SP` when the instruction reads or writes the stack pointer, and the zero
/// register otherwise, so the name depends on the access and not on the
/// encoding alone. llvm-objdump renders exactly this way.
fn sp_or_gname(sf: u32, n: u32, is_sp: bool) -> String {
    if n != 31 {
        return gname(sf, n);
    }
    if sf == 1 {
        if is_sp {
            "sp".into()
        } else {
            "xzr".into()
        }
    } else if is_sp {
        "wsp".into()
    } else {
        "wzr".into()
    }
}

fn vname(n: u32) -> String {
    format!("v{n}")
}

fn mem_reg(n: u32) -> String {
    if n == 31 {
        "sp".into()
    } else {
        format!("x{n}")
    }
}

fn cond_name(cc: u32) -> &'static str {
    [
        "eq", "ne", "cs", "cc", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le", "al",
        "nv",
    ][(cc & 0xf) as usize]
}

fn shift_txt(shift: u32, amount: u32) -> String {
    let name = match shift {
        0 => "lsl",
        1 => "lsr",
        2 => "asr",
        3 => "ror",
        _ => "??",
    };
    if amount == 0 {
        String::new()
    } else {
        format!("{name} #{amount}")
    }
}

fn ext_txt(option: u32, amount: u32) -> String {
    let name = match option {
        0 => "uxtb",
        1 => "uxth",
        2 => "uxtw",
        3 => "uxtx",
        4 => "sxtb",
        5 => "sxth",
        6 => "sxtw",
        7 => "sxtx",
        _ => "??",
    };
    if amount == 0 && option == 3 {
        "lsl".to_string()
    } else if amount == 0 {
        name.to_string()
    } else {
        format!("{name} lsl #{amount}")
    }
}

fn ldrstr_name(size: u32, opc: u32, load: bool) -> Option<&'static str> {
    Some(match (load, size, opc) {
        (false, 0, _) => "strb",
        (false, 1, _) => "strh",
        (false, 2, _) => "str",
        (false, 3, _) => "str",
        (true, 0, 1) => "ldrb",
        (true, 1, 1) => "ldrh",
        (true, 2, 1) => "ldr",
        (true, 3, 1) => "ldr",
        (true, 0, 2) => "ldrsb",
        (true, 1, 2) => "ldrsh",
        (true, 2, 2) => "ldrsw",
        (true, 3, 2) => "ldrsw",
        (true, _, 3) => return None, // literal, decoded separately
        _ => return None,
    })
}

fn ldrstr_rt(size: u32, opc: u32, rt: u32) -> String {
    // Register width: doubleword ⇒ x, word ⇒ w; sign-extend words stay x.
    let w = match (size, opc) {
        (3, _) | (2, 2) => 64,
        _ => 32,
    };
    if w == 64 {
        xname(rt)
    } else {
        wname(rt)
    }
}

fn render(_pc: u64, op: u32, decoded: Op) -> String {
    use Op::*;
    let mut ops: Vec<String> = Vec::new();
    let mn: &str;

    match decoded {
        Eret => "eret".to_string(),
        Drps => "drps".to_string(),
        Brk(imm) => format!("brk\t#0x{imm:x}"),
        Hlt(imm) => format!("hlt\t#0x{imm:x}"),
        Svc(imm) => format!("svc\t#0x{imm:x}"),
        Hvc(imm) => format!("hvc\t#0x{imm:x}"),
        Smc(imm) => format!("smc\t#0x{imm:x}"),
        Sys(w) => sys_render(w),
        Branch { link, imm } => {
            let t = _pc.wrapping_add(imm as u64);
            if link {
                format!("bl\t0x{t:x}")
            } else {
                format!("b\t0x{t:x}")
            }
        }
        Bcond { cond, imm } => {
            let t = _pc.wrapping_add(imm as u64);
            format!("b.{}\t0x{t:x}", cond_name(cond))
        }
        Br(rn) => format!("br\t{}", xname(rn)),
        Blr(rn) => format!("blr\t{}", xname(rn)),
        Ret(rn) => {
            if rn == 30 {
                "ret".into()
            } else {
                format!("ret\t{}", xname(rn))
            }
        }
        RetPac { key } => {
            if key == 0 {
                "retaa".into()
            } else {
                "retab".into()
            }
        }
        EretPac { key } => {
            if key == 0 {
                "eretaa".into()
            } else {
                "eretab".into()
            }
        }
        PacBranch {
            key,
            link,
            modifier,
            target,
        } => {
            let name = match (key, link) {
                (0, false) => "braa",
                (1, false) => "brab",
                (0, true) => "blraa",
                _ => "blrab",
            };
            if modifier == 32 {
                return format!("{name}z\t{}", xname(target));
            }
            let modifier_name = if modifier == 31 {
                "sp".to_string()
            } else {
                xname(modifier)
            };
            format!("{name}\t{}, {}", xname(target), modifier_name)
        }
        PacHint { key, sign, sp } => {
            let name = match (key, sign, sp) {
                (0, true, false) => "pacia1716",
                (1, true, false) => "pacib1716",
                (0, false, false) => "autia1716",
                (1, false, false) => "autib1716",
                (0, true, true) => "paciasp",
                (1, true, true) => "pacibsp",
                (0, false, true) => "autiasp",
                _ => "autibsp",
            };
            name.to_string()
        }
        Nop => "nop".to_string(),
        Cbz {
            is_cbnz,
            sf,
            rt,
            imm,
        } => {
            let t = _pc.wrapping_add(imm as u64);
            let name = if is_cbnz { "cbnz" } else { "cbz" };
            format!("{name}\t{}, 0x{t:x}", gname(sf, rt))
        }
        Tbz {
            is_tbnz,
            bit,
            rt,
            imm,
        } => {
            let t = _pc.wrapping_add(imm as u64);
            let name = if is_tbnz { "tbnz" } else { "tbz" };
            format!("{name}\t{}, #{bit}, 0x{t:x}", xname(rt))
        }
        Adr { rd, imm } => {
            let t = _pc.wrapping_add(imm as u64);
            format!("adr\t{}, 0x{t:x}", xname(rd))
        }
        Adrp { rd, imm } => {
            // ADRP forms the address from the *page* of the instruction plus a
            // page-scaled immediate. The decoded `imm` is in page units, so the
            // PC must be quantised and the immediate scaled. The executor does
            // exactly this; printing `pc + imm` instead misreports every ADRP
            // target, which sends ROM analysis to the wrong address entirely.
            let t = (_pc & !0xfff).wrapping_add((imm as u64) << 12);
            format!("adrp\t{}, 0x{t:x}", xname(rd))
        }
        AddSubImm {
            sf,
            sub,
            set,
            shl,
            imm,
            rn,
            rd,
        } => {
            let name = if !sub && set && rd == 31 {
                "cmn"
            } else if sub && set && rd == 31 {
                "cmp"
            } else if sub && set {
                "subs"
            } else if sub {
                "sub"
            } else if set {
                "adds"
            } else {
                "add"
            };
            // The decoder folds `lsl #12` into `imm` for the executor, so the
            // printer has to undo it or the immediate is reported twice shifted.
            let imm_text = if shl { imm >> 12 } else { imm };
            let mut s = format!("#0x{imm_text:x}");
            if shl {
                s.push_str(", lsl #12");
            }
            let rn_name = sp_or_gname(sf, rn, true);
            if name == "cmp" || name == "cmn" {
                return format!("{name}\t{rn_name}, {s}");
            }
            // `add`/`adds`/`sub`/`subs` write SP when the destination encodes 31
            // and the flag-setting form is not selected; the flag-setting forms
            // and the compare forms always discard the result.
            let rd_name = sp_or_gname(sf, rd, !set);
            format!("{name}\t{rd_name}, {rn_name}, {s}")
        }
        MovImm {
            sf,
            kind,
            hw,
            imm,
            rd,
        } => {
            let full = if sf == 1 { xname(rd) } else { wname(rd) };
            let name = match kind {
                0 => "movn",
                2 => "movz",
                _ => "movk",
            };
            ops.push(full);
            ops.push(format!("#0x{imm:x}"));
            if hw != 0 {
                ops.push(format!("lsl #{}", hw * 16));
            }
            render_simple(name, ops)
        }
        LogImm {
            sf,
            opc,
            n,
            immr,
            imms,
            rn,
            rd,
        } => {
            let name = match opc & 3 {
                0 => "and",
                1 => "orr",
                2 => "eor",
                3 => "ands",
                _ => "and",
            };
            ops.push(gname(sf, rd));
            ops.push(gname(sf, rn));
            match decode_bitmask(n, immr, imms, sf == 1) {
                Some(mask) => ops.push(format!("#0x{mask:x}")),
                None => ops.push(format!(
                    "<invalid-logical-immediate n={n} immr={immr} imms={imms}>"
                )),
            }
            render_simple(name, ops)
        }
        Extract {
            sf,
            lsb,
            rm,
            rn,
            rd,
        } => {
            if rn == rm {
                return format!("ror\t{}, {}, #{lsb}", gname(sf, rd), gname(sf, rn));
            }
            format!(
                "extr\t{}, {}, {}, #{lsb}",
                gname(sf, rd),
                gname(sf, rn),
                gname(sf, rm)
            )
        }
        Bitfield {
            sf,
            opc,
            immr,
            imms,
            rn,
            rd,
        } => {
            let name = match opc {
                0 => "sbfm",
                1 => "bfm",
                _ => "ubfm",
            };
            format!(
                "{name}\t{}, {}, #{immr}, #{imms}",
                gname(sf, rd),
                gname(sf, rn)
            )
        }
        AddSubShift {
            sf,
            sub,
            set,
            shift,
            amount,
            rm,
            rn,
            rd,
        } => {
            mn = if sub && set && rd == 31 {
                "cmp"
            } else if sub && set {
                "subs"
            } else if sub {
                "sub"
            } else if set {
                "adds"
            } else {
                "add"
            };
            ops.push(gname(sf, rd));
            ops.push(gname(sf, rn));
            let mut s = gname(sf, rm);
            if amount != 0 {
                s.push(' ');
                s.push_str(&shift_txt(shift, amount));
            }
            ops.push(s);
            render_simple(mn, ops)
        }
        AddSubExt {
            sf,
            sub,
            set,
            option,
            amount,
            rm,
            rn,
            rd,
        } => {
            mn = if sub && set && rd == 31 {
                "cmp"
            } else if sub && set {
                "subs"
            } else if sub {
                "sub"
            } else if set {
                "adds"
            } else {
                "add"
            };
            ops.push(gname(sf, rd));
            ops.push(gname(sf, rn));
            ops.push(format!("{}, {}", wname(rm), ext_txt(option, amount)));
            render_simple(mn, ops)
        }
        LogShift {
            sf,
            opc,
            n,
            shift,
            amount,
            rm,
            rn,
            rd,
        } => {
            let (a, b, c) = (opc & 3, opc >> 2, n);
            let name = match (b, c, a) {
                (0, 0, 0) => "and",
                (0, 0, 1) => "orr",
                (0, 0, 2) => "eor",
                (0, 0, 3) => "ands",
                (0, 1, 0) => "bic",
                (0, 1, 1) => "orn",
                (0, 1, 2) => "eon",
                (0, 1, 3) => "bics",
                _ => "and",
            };
            ops.push(gname(sf, rd));
            ops.push(gname(sf, rn));
            let mut s = gname(sf, rm);
            if amount != 0 {
                s.push(' ');
                s.push_str(&shift_txt(shift, amount));
            }
            ops.push(s);
            render_simple(name, ops)
        }
        Dp1 { sf, sub, rn, rd } => {
            let name = match (sf, sub) {
                (_, 0) => "rbit",
                (_, 1) => "rev16",
                (0, 2) => "rev",
                (1, 2) => "rev32",
                (_, 3) => "rev",
                (_, 4) => "clz",
                (_, 5) => "cls",
                _ => "rev",
            };
            format!("{name}\t{}, {}", gname(sf, rd), gname(sf, rn))
        }
        Pacga { rn, rm, rd } => {
            format!(
                "pacga\t{}, {}, {}",
                xname(rd),
                xname(rn),
                if rm == 31 {
                    "sp".to_string()
                } else {
                    xname(rm)
                }
            )
        }
        Xpac { data, rd } => {
            if op == 0xd503_20ff {
                return "xpaclri".to_string();
            }
            format!("{}\t{}", if data { "xpacd" } else { "xpaci" }, xname(rd))
        }
        PacData { sub, rn, rd } => {
            // Register forms (sub 0..7) carry an explicit modifier; the
            // zero-modifier forms (sub 8..15) print Rn and keep a Z suffix.
            let z = sub >= 8;
            let name = match sub & 7 {
                0 => "pacia",
                1 => "pacib",
                2 => "pacda",
                3 => "pacdb",
                4 => "autia",
                5 => "autib",
                6 => "autda",
                7 => "autdb",
                _ => unreachable!(),
            };
            let base = if z {
                // Insert the Z between the key selector and the final letter:
                // "pacia" -> "paciza", "autda" -> "autdza".
                format!("{}z{}", &name[..name.len() - 1], &name[name.len() - 1..])
            } else {
                name.to_string()
            };
            if z {
                return format!("{base}\t{}", xname(rd));
            }
            format!("{base}\t{}, {}", xname(rd), xname(rn))
        }
        Dp2 {
            sf,
            sub,
            rm,
            rn,
            rd,
        } => {
            let name = match sub {
                0 => "udiv",
                1 => "sdiv",
                2 => "lsl",
                3 => "lsr",
                4 => "asr",
                _ => "ror",
            };
            format!(
                "{name}\t{}, {}, {}",
                gname(sf, rd),
                gname(sf, rn),
                gname(sf, rm)
            )
        }
        Dp3 {
            sf,
            sub,
            rm,
            ra,
            rn,
            rd,
        } => {
            let (name, keep_ra) = match sub {
                0 => ("madd", true),
                1 => ("msub", true),
                20 => ("smaddl", true),
                21 => ("smsubl", true),
                22 => ("umaddl", true),
                23 => ("umsubl", true),
                26 => ("smulh", false),
                27 => ("umulh", false),
                _ => ("madd", true),
            };
            let mut v = format!(
                "{name}\t{}, {}, {}",
                gname(sf, rd),
                gname(sf, rn),
                gname(sf, rm)
            );
            if keep_ra && ra != 31 {
                v.push_str(&format!(", {}", gname(sf, ra)));
            }
            v
        }
        Carry {
            sf,
            sub,
            set,
            rm,
            rn,
            rd,
        } => {
            let name = match (sub, set) {
                (false, false) => "adc",
                (false, true) => "adcs",
                (true, false) => "sbc",
                (true, true) => "sbcs",
            };
            format!(
                "{name}\t{}, {}, {}",
                gname(sf, rd),
                gname(sf, rn),
                gname(sf, rm)
            )
        }
        CCmp {
            sf,
            sub,
            is_imm,
            cond,
            nzcv,
            rm,
            rn,
        } => {
            let name = if sub { "ccmp" } else { "ccmn" };
            let other = if is_imm {
                format!("#{rm}")
            } else {
                gname(sf, rm)
            };
            format!(
                "{name}\t{}, {}, #0x{nzcv:x}, {}",
                gname(sf, rn),
                other,
                cond_name(cond)
            )
        }
        CondSel {
            sf,
            sel_op,
            rm,
            cond,
            rn,
            rd,
        } => {
            let name = match sel_op {
                0 => "csel",
                1 => "csinc",
                2 => "csinv",
                _ => "csneg",
            };
            format!(
                "{name}\t{}, {}, {}, {}",
                gname(sf, rd),
                gname(sf, rn),
                gname(sf, rm),
                cond_name(cond)
            )
        }
        LdrLit {
            vector,
            size,
            sw,
            prefetch,
            rt,
            imm,
        } => {
            if prefetch {
                return format!("prfm\t0x{imm:x}");
            }
            if vector {
                let reg = match size {
                    16 => format!("q{rt}"),
                    8 => format!("d{rt}"),
                    4 => format!("s{rt}"),
                    2 => format!("h{rt}"),
                    _ => format!("b{rt}"),
                };
                return format!("ldr\t{}, 0x{:x}", reg, _pc.wrapping_add(imm as u64));
            }
            let reg = if size == 8 {
                xname(rt)
            } else if sw || size == 4 {
                if sw && size == 4 {
                    format!("x{rt}")
                } else {
                    format!("w{rt}")
                }
            } else {
                format!("w{rt}")
            };
            let name = if size == 8 {
                "ldr"
            } else if sw {
                "ldrsw"
            } else {
                "ldr"
            };
            format!("{name}\t{}, 0x{:x}", reg, _pc.wrapping_add(imm as u64))
        }
        Excl {
            sf: _,
            load,
            size,
            rs,
            rn,
            rt,
        } => {
            let suffix = match size {
                0 => "b",
                1 => "h",
                2 => "",
                3 => "",
                _ => "",
            };
            if load {
                let reg = if size >= 2 { gname(1, rt) } else { wname(rt) };
                return format!("ldxr{suffix}\t{}, [{}]", reg, mem_reg(rn));
            }
            let reg = if size >= 2 { gname(1, rt) } else { wname(rt) };
            format!("stxr{suffix}\t{}, {}, [{}]", wname(rs), reg, mem_reg(rn))
        }
        Ordered { load, size, rn, rt } => {
            let suffix = match size {
                0 => "b",
                1 => "h",
                2 => "",
                3 => "",
                _ => "",
            };
            let name = if load { "ldar" } else { "stlr" };
            let reg = if size >= 2 { gname(1, rt) } else { wname(rt) };
            format!("{name}{suffix}\t{}, [{}]", reg, mem_reg(rn))
        }
        LsePair {
            size,
            acquire,
            release,
            rs,
            rn,
            rt,
        } => {
            let name = format!(
                "casp{}{}",
                if acquire { "a" } else { "" },
                if release { "l" } else { "" }
            );
            let sf = u32::from(size == 8);
            format!(
                "{name}\t{}, {}, {}, {}, [{}]",
                gname(sf, rs),
                gname(sf, rs + 1),
                gname(sf, rt),
                gname(sf, rt + 1),
                mem_reg(rn)
            )
        }
        Lse {
            sub,
            size,
            acquire,
            release,
            rm,
            rn,
            rt,
            ..
        } => {
            let base = match sub {
                1 => "swp",
                2 => "ldadd",
                3 => "ldclr",
                4 => "ldeor",
                5 => "ldset",
                10 => "ldsmax",
                11 => "ldsmin",
                12 => "ldumax",
                13 => "ldumin",
                14 => "cas",
                _ => "lse",
            };
            let store_alias = rt == 31 && matches!(sub, 2..=5 | 10..=13);
            let name = if store_alias {
                match sub {
                    2 => "stadd",
                    3 => "stclr",
                    4 => "steor",
                    5 => "stset",
                    10 => "stsmax",
                    11 => "stsmin",
                    12 => "stumax",
                    _ => "stumin",
                }
            } else {
                base
            };
            let suffix = match size {
                0 => "b",
                1 => "h",
                _ => "",
            };
            let ordering = format!(
                "{}{}",
                if acquire { "a" } else { "" },
                if release { "l" } else { "" }
            );
            let sf = u32::from(size == 8);
            if store_alias {
                return format!(
                    "{name}{suffix}{ordering}\t{}, [{}]",
                    gname(sf, rm),
                    mem_reg(rn)
                );
            }
            format!(
                "{name}{suffix}{ordering}\t{}, {}, [{}]",
                gname(sf, rm),
                gname(sf, rt),
                mem_reg(rn)
            )
        }
        LdrAuth {
            key,
            writeback,
            rn,
            rt,
            imm,
        } => {
            let name = if key == 2 { "ldraa" } else { "ldrab" };
            let offset = if imm == 0 && !writeback {
                String::new()
            } else if imm < 0 {
                format!(", #-0x{:x}", -imm)
            } else {
                format!(", #0x{imm:x}")
            };
            format!(
                "{name}\t{}, [{}{offset}]{}",
                xname(rt),
                mem_reg(rn),
                if writeback { "!" } else { "" }
            )
        }
        LdrStrU {
            size,
            opc,
            rn,
            rt,
            off,
        } => {
            let (load, is_lit) = ldrstr_load_opc(size, opc);
            if is_lit {
                return format!(
                    "ldr\t{}, 0x{:x}",
                    ldrstr_rt(size, opc, rt),
                    _pc.wrapping_add(off as i64 as u64)
                );
            }
            let Some(name) = ldrstr_name(size, opc, load) else {
                return format!("ldrstr0x{size}{opc}\t{}, [{}]", xname(rt), mem_reg(rn));
            };
            let r = ldrstr_rt(size, opc, rt);
            if off == 0 {
                return format!("{name}\t{}, [{}]", r, mem_reg(rn));
            }
            format!("{name}\t{}, [{}, #0x{off:x}]", r, mem_reg(rn))
        }
        LdrStrPre {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (load, _) = ldrstr_load_opc(size, opc);
            let Some(name) = ldrstr_name(size, opc, load) else {
                return format!("ldrstrpre0x{size}{opc}\t{}, [{}]", xname(rt), mem_reg(rn));
            };
            let r = ldrstr_rt(size, opc, rt);
            format!("{name}\t{}, [{}, #0x{:x}]!", r, mem_reg(rn), imm as u64)
        }
        LdrStrPost {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (load, _) = ldrstr_load_opc(size, opc);
            let Some(name) = ldrstr_name(size, opc, load) else {
                return format!("ldrstrpost0x{size}{opc}\t{}, [{}]", xname(rt), mem_reg(rn));
            };
            let r = ldrstr_rt(size, opc, rt);
            format!("{name}\t{}, [{}], #0x{:x}", r, mem_reg(rn), imm as u64)
        }
        LdrStrReg {
            size,
            opc,
            option,
            amount,
            rn,
            rm,
            rt,
        } => {
            let (load, _) = ldrstr_load_opc(size, opc);
            let Some(name) = ldrstr_name(size, opc, load) else {
                return format!("ldrstrreg0x{size}{opc}\t{}, [{}]", xname(rt), mem_reg(rn));
            };
            let r = ldrstr_rt(size, opc, rt);
            let idx = if option == 3 || option == 7 {
                xname(rm)
            } else {
                wname(rm)
            };
            let modifier = match (option, amount) {
                (3, 0) => String::new(),
                (3, amount) => format!("lsl #{amount}"),
                (_, amount) => ext_txt(option, amount),
            };
            if modifier.is_empty() {
                format!("{name}\t{}, [{}, {}]", r, mem_reg(rn), idx)
            } else {
                format!("{name}\t{}, [{}, {}, {}]", r, mem_reg(rn), idx, modifier)
            }
        }
        LdStPair {
            size,
            load,
            signed,
            pre,
            post,
            imm,
            rn,
            rt,
            rt2,
        } => {
            let name = if load { "ldp" } else { "stp" };
            // `LdStPair::size` stores the decoded element width in bytes
            // (4 or 8), not the raw size field from the instruction.
            let (elem, _scale) = if size == 4 && !signed {
                ("w", 4)
            } else if load && signed && size == 4 {
                ("x", 4)
            } else {
                ("x", 8)
            };
            let r1 = if elem == "w" { wname(rt) } else { xname(rt) };
            let r2 = if elem == "w" { wname(rt2) } else { xname(rt2) };
            let base = mem_reg(rn);
            let off = imm as u64;
            if post {
                return format!("{name}\t{}, {}, [{}], #0x{off:x}", r1, r2, base);
            }
            if pre {
                return format!("{name}\t{}, {}, [{}, #0x{off:x}]!", r1, r2, base);
            }
            if imm == 0 {
                return format!("{name}\t{}, {}, [{}]", r1, r2, base);
            }
            format!("{name}\t{}, {}, [{}, #0x{off:x}]", r1, r2, base)
        }
        SimdLdrStrU {
            size,
            opc,
            rn,
            rt,
            off,
        } => {
            let (elem, scale) = simd_elem_scale(size, opc);
            let name = if off == 0 { "ldr" } else { "ldur" };
            let _ = scale;
            if off == 0 {
                return format!("{name}\t{}{rt}, [{}]", elem, mem_reg(rn));
            }
            format!("{name}\t{}{rt}, [{}, #0x{off:x}]", elem, mem_reg(rn))
        }
        SimdStructSingle {
            q,
            load,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            let name = if load { "ld1" } else { "st1" };
            let mut text = format!("{name}\t{{v{rt}.{lanes}{elem}}}, [{}]", mem_reg(rn));
            if post {
                if rm == 31 {
                    text.push_str(&format!(", #0x{:x}", if q { 16 } else { 8 }));
                } else {
                    text.push_str(&format!(", {}", xname(rm)));
                }
            }
            text
        }
        SimdStructPair {
            q,
            load,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            let name = if load { "ld1" } else { "st1" };
            let rt2 = (rt + 1) & 31;
            let mut text = format!(
                "{name}\t{{v{rt}.{lanes}{elem}, v{rt2}.{lanes}{elem}}}, [{}]",
                mem_reg(rn)
            );
            if post {
                if rm == 31 {
                    text.push_str(&format!(", #0x{:x}", if q { 32 } else { 16 }));
                } else {
                    text.push_str(&format!(", {}", xname(rm)));
                }
            }
            text
        }
        SimdStructMulti {
            n,
            q,
            load,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            let name = if load { "ld" } else { "st" };
            let list: Vec<String> = (0..n)
                .map(|i| format!("v{}.{lanes}{elem}", (rt + i) & 31))
                .collect();
            let mut text = format!("{name}{n}\t{{{}}}, [{}]", list.join(", "), mem_reg(rn));
            if post {
                if rm == 31 {
                    text.push_str(&format!(", #0x{:x}", (if q { 16 } else { 8 }) * n));
                } else {
                    text.push_str(&format!(", {}", xname(rm)));
                }
            }
            text
        }
        SimdStructReplicate {
            n,
            q,
            post,
            size,
            rn,
            rt,
            rm,
        } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            let list: Vec<String> = (0..n)
                .map(|i| format!("v{}.{lanes}{elem}", (rt + i) & 31))
                .collect();
            let mut text = format!("ld{n}r\t{{{}}}, [{}]", list.join(", "), mem_reg(rn));
            if post {
                if rm == 31 {
                    text.push_str(&format!(", #0x{:x}", 1 << size));
                } else {
                    text.push_str(&format!(", {}", xname(rm)));
                }
            }
            text
        }
        NeonTblTbx {
            q,
            tbx,
            n,
            vn,
            vm,
            vd,
        } => {
            let lanes = if q { 16 } else { 8 };
            let list: Vec<String> = (0..n)
                .map(|i| format!("v{}.{lanes}b", (vn + i) & 31))
                .collect();
            format!(
                "{}\tv{}.{lanes}b, {{{}}}, v{}.{lanes}b",
                if tbx { "tbx" } else { "tbl" },
                vd,
                list.join(", "),
                vm
            )
        }
        SimdLdrStrPre {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (elem, _) = simd_elem_scale(size, opc);
            format!("ldr\t{}{rt}, [{}, #0x{:x}]!", elem, mem_reg(rn), imm as u64)
        }
        SimdLdrStrPost {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (elem, _) = simd_elem_scale(size, opc);
            format!("ldr\t{}{rt}, [{}], #0x{:x}", elem, mem_reg(rn), imm as u64)
        }
        SimdLdurStur {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (elem, _) = simd_elem_scale(size, opc);
            let name = if (opc >> 2) & 1 != 0 { "ldur" } else { "stur" };
            format!(
                "{name}\t{}{rt}, [{}, #0x{:x}]",
                elem,
                mem_reg(rn),
                imm as u64
            )
        }
        SimdLdrStrReg {
            size,
            opc,
            option: _,
            amount: _,
            rn,
            rm,
            rt,
        } => {
            let (elem, _) = simd_elem_scale(size, opc);
            format!("ldr\t{}{rt}, [{}, {}]", elem, mem_reg(rn), wname(rm),)
        }
        SimdLdStPair {
            size,
            load,
            pre,
            post,
            imm,
            rn,
            rt,
            rt2,
        } => {
            let name = if load { "ldp" } else { "stp" };
            let (elem, _scale) = simd_elem_scale(size, if size == 2 { 1 } else { 0 });
            let base = mem_reg(rn);
            let off = imm as u64;
            if post {
                return format!(
                    "{name}\t{}{rt}, {}{rt2}, [{}], #0x{off:x}",
                    elem, elem, base
                );
            }
            if pre {
                return format!(
                    "{name}\t{}{rt}, {}{rt2}, [{}, #0x{off:x}]!",
                    elem, elem, base
                );
            }
            if imm == 0 {
                return format!("{name}\t{}{rt}, {}{rt2}, [{}]", elem, elem, base);
            }
            format!(
                "{name}\t{}{rt}, {}{rt2}, [{}, #0x{off:x}]",
                elem, elem, base
            )
        }
        FpThreeSrc {
            sub,
            op31,
            rm,
            ra,
            rn,
            rd,
        } => {
            let name = match (sub, op31) {
                (false, 0) => "fmadd",
                (true, 0) => "fmsub",
                (false, 1) => "fnmadd",
                (true, 1) => "fnmsub",
                _ => "fmadd",
            };
            let prec = fp_prec(op);
            format!(
                "{name}{prec}\t{}{rd}, {}{rn}, {}{rm}, {}{ra}",
                prec, prec, prec, prec
            )
        }
        FpTwoSrc { opc, rm, rn, rd } => {
            let name = match opc {
                0 => "fmul",
                1 => "fdiv",
                2 => "fadd",
                3 => "fsub",
                4 => "fmax",
                5 => "fmin",
                6 => "fmaxnm",
                7 => "fminnm",
                8 => "fnmul",
                _ => "fop",
            };
            let prec = fp_prec(op);
            format!("{name}{prec}\t{prec}{rd}, {prec}{rn}, {prec}{rm}")
        }
        FpCmpReg {
            signal_all_nans,
            rn,
            rm,
        } => {
            let name = if signal_all_nans { "fcmpe" } else { "fcmp" };
            let prec = fp_prec(op);
            format!("{name}{prec}\t{prec}{rn}, {prec}{rm}")
        }
        FpCmpZero {
            signal_all_nans,
            rn,
        } => {
            let name = if signal_all_nans { "fcmpe" } else { "fcmp" };
            let prec = fp_prec(op);
            format!("{name}{prec}\t{prec}{rn}, #0.0")
        }
        FpCCmp {
            sub,
            cond,
            nzcv,
            rm,
            rn,
        } => {
            let name = if sub { "fccmpe" } else { "fccmp" };
            let prec = fp_prec(op);
            format!(
                "{name}{prec}\t{prec}{rn}, {prec}{rm}, #0x{nzcv:x}, {}",
                cond_name(cond)
            )
        }
        FpSel { rm, cond, rn, rd } => {
            let prec = fp_prec(op);
            format!(
                "fcsel{prec}\t{prec}{rd}, {prec}{rn}, {prec}{rm}, {}",
                cond_name(cond)
            )
        }
        FpOneSrc { opcode, rn, rd } => {
            let name = match (opcode >> 5) & 0xf {
                0 => "fmov",
                1 => "fabs",
                2 => "fneg",
                3 => "fsqrt",
                4..=6 => "fcvt",
                7 => "frintn",
                8 => "frintp",
                9 => "frintm",
                10 => "frintz",
                11 => "frinta",
                14 => "frintx",
                15 => "frinti",
                _ => "fp1s",
            };
            let prec = fp_prec(op);
            format!("{name}{prec}\t{prec}{rd}, {prec}{rn}")
        }
        FpImm { imm8, typ, rd } => {
            let prec = match typ {
                0 => "s",
                1 => "d",
                _ => "h",
            };
            format!("fmov\t{prec}{rd}, #{:.*}", 8, fp_imm_to_f64(imm8))
        }
        FpGprConv {
            sf,
            typ,
            opc,
            rn,
            rd,
        } => {
            let name = match opc {
                0 => "fcvtns",
                1 => "fcvtnu",
                2 => "scvtf",
                3 => "ucvtf",
                4 => "fcvtas",
                5 => "fcvtau",
                6 => "fjcvtzs",
                7 => "fmov",
                8 => "fcvtzs",
                9 => "fcvtzu",
                10 => "fmov",
                16 => "fcvtms",
                17 => "fcvtmu",
                24 => "fcvtzs",
                25 => "fcvtzu",
                _ => "fpconv",
            };
            let prec = if typ & 1 == 0 { "s" } else { "d" };
            let to_fp = matches!(opc, 2 | 3 | 5 | 7 | 24 | 25);
            let _ = to_fp;
            format!("{name}{prec}\t{prec}{rd}, {}", gname(sf, rn))
        }
        FpFixed {
            sf,
            typ,
            u,
            to_fp,
            imm6,
            rn,
            rd,
        } => {
            let prec = if typ & 1 == 0 { "s" } else { "d" };
            let name = if to_fp {
                if u {
                    "ucvtf"
                } else {
                    "scvtf"
                }
            } else if u {
                "fcvtzu"
            } else {
                "fcvtzs"
            };
            format!("{name}{prec}\t{prec}{rd}, {}, #{imm6}", gname(sf, rn))
        }
        FpRegConv {
            unsigned,
            to_fp,
            prec,
            rn,
            rd,
        } => {
            let name = if to_fp {
                if unsigned {
                    "ucvtf"
                } else {
                    "scvtf"
                }
            } else if unsigned {
                "fcvtzu"
            } else {
                "fcvtzs"
            };
            let prec = match prec {
                0 => "s",
                1 => "d",
                _ => "h",
            };
            format!("{name}{prec}\t{prec}{rd}, {prec}{rn}")
        }
        Crc32 { sz, c, rm, rn, rd } => {
            let name = if c { "crc32c" } else { "crc32" };
            let suffix = match sz {
                0 => "b",
                1 => "h",
                2 => "w",
                _ => "x",
            };
            format!(
                "{name}{suffix}\t{}, {}, {}",
                wname(rd),
                wname(rn),
                wname(rm)
            )
        }
        Fjcvtzs { rn: _, rd } => format!("fjcvtzs\t{}", xname(rd)),
        Ldur {
            size,
            opc,
            rn,
            rt,
            imm,
        } => {
            let (load, _) = ldrstr_load_opc(size, opc.min(2));
            let Some(name) = ldrstr_name(size, opc.min(2), load) else {
                return format!("ldur0x{size}{opc}\t{}, [{}]", xname(rt), mem_reg(rn));
            };
            let r = if size == 3 || opc == 2 {
                xname(rt)
            } else {
                wname(rt)
            };
            let base = if name.starts_with("stur") {
                "stur"
            } else {
                "ldur"
            };
            let name = if name == "ldr" {
                "ldur"
            } else if name == "str" {
                "stur"
            } else {
                name
            };
            let _ = base;
            format!("{name}\t{}, [{}, #0x{:x}]", r, mem_reg(rn), imm as u64)
        }
        Ldapr {
            size,
            sign_extend,
            rn,
            rt,
            imm,
        } => {
            let base = if imm != 0 { "ldapur" } else { "ldapr" };
            let suffix = match (size, sign_extend) {
                (0, false) => "b",
                (0, true) => "sb",
                (1, false) => "h",
                (1, true) => "sh",
                (2, false) => "",
                (2, true) => "sw",
                (3, _) => "",
                _ => "",
            };
            let r = if size == 3 || sign_extend && size >= 2 {
                xname(rt)
            } else {
                wname(rt)
            };
            format!("{base}{suffix}\t{}, [{}]", r, mem_reg(rn))
        }
        Dot {
            unsigned,
            q,
            elem,
            rm,
            rn,
            rd,
        } => {
            let name = if unsigned { "udot" } else { "sdot" };
            match elem {
                Some(lane) => format!(
                    "{name}\t{}, {}.16b, {}.4b[{lane}]",
                    vname(rd),
                    vname(rn),
                    vname(rm)
                ),
                None => {
                    let sz = if q { "16b" } else { "8b" };
                    format!(
                        "{name}\t{}, {}.{}, {}.{}",
                        vname(rd),
                        vname(rn),
                        sz,
                        vname(rm),
                        sz
                    )
                }
            }
        }
        FcComplexArith {
            is_cmla,
            sz,
            q,
            rot,
            rm,
            rn,
            rd,
        } => {
            let _ = (sz, q);
            let name = if is_cmla { "fcmla" } else { "fcadd" };
            format!(
                "{name}\t{}.4s, {}.4s, {}.4s, #{}",
                vname(rd),
                vname(rn),
                vname(rm),
                rot
            )
        }
        Aes { op, rn, rd } => {
            let name = match op {
                4 => "aese",
                5 => "aesd",
                6 => "aesmc",
                7 => "aesimc",
                _ => "aes",
            };
            format!("{name}\t{}.16b, {}.16b", vname(rd), vname(rn))
        }
        Sha256 { op, rn, rm, rd } => {
            let name = match op {
                0 => "sha256h",
                1 => "sha256h2",
                2 => "sha256su0",
                3 => "sha256su1",
                _ => "sha256",
            };
            if op == 2 {
                return format!("{name}\t{}.4s, {}.4s", vname(rd), vname(rn));
            }
            format!(
                "{name}\t{}.4s, {}.4s, {}.4s",
                vname(rd),
                vname(rn),
                vname(rm)
            )
        }
        NeonUminv { q, size, rn, rd } => {
            let (esuffix, lane_count) = match (size, q) {
                (0, false) => ("8b", "8"),
                (0, true) => ("16b", "16"),
                (1, false) => ("4h", "4"),
                (1, true) => ("8h", "8"),
                _ => ("4s", "4"),
            };
            let _ = lane_count;
            format!("uminv\tb{rd}, v{rn}.{esuffix}")
        }
        NeonUmaxv { q, size, rn, rd } => {
            let esuffix = match (size, q) {
                (0, false) => "8b",
                (0, true) => "16b",
                (1, false) => "4h",
                (1, true) => "8h",
                _ => "4s",
            };
            format!("umaxv\tb{rd}, v{rn}.{esuffix}")
        }
        NeonAddLv {
            q,
            size,
            signed,
            rn,
            rd,
        } => {
            let mnem = if signed { "saddlv" } else { "uaddlv" };
            let (dst, src) = match (size, q) {
                (0, false) => (format!("h{rd}"), format!("v{rn}.8b")),
                (0, true) => (format!("h{rd}"), format!("v{rn}.16b")),
                (1, false) => (format!("s{rd}"), format!("v{rn}.4h")),
                _ => (format!("s{rd}"), format!("v{rn}.8h")),
            };
            format!("{mnem}\t{dst}, {src}")
        }
        NeonRev64 { q, size, rn, rd } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            format!("rev64\tv{rd}.{lanes}{elem}, v{rn}.{lanes}{elem}")
        }
        NeonShiftImmediate {
            q,
            size,
            right,
            amount,
            rn,
            rd,
        } => {
            let elem = ["b", "h", "s", "d"][size as usize];
            let lanes = (if q { 16 } else { 8 }) / (1 << size);
            let name = if right { "ushr" } else { "shl" };
            format!("{name}\tv{rd}.{lanes}{elem}, v{rn}.{lanes}{elem}, #{amount}")
        }
        NeonShiftLong {
            q,
            unsigned,
            src_size,
            amount,
            rn,
            rd,
        } => {
            let (src_elem, dst_elem) = match src_size {
                8 => ("b", "h"),
                16 => ("h", "s"),
                _ => ("s", "d"),
            };
            let src_lanes = (if q { 16 } else { 8 }) / (src_size / 8);
            let dst_lanes = 16 / ((src_size / 8) * 2);
            let base = if unsigned { "ushll" } else { "sshll" };
            let name = if q {
                format!("{base}2")
            } else {
                base.to_string()
            };
            format!("{name}\tv{rd}.{dst_lanes}{dst_elem}, v{rn}.{src_lanes}{src_elem}, #{amount}")
        }
        NeonShiftReg {
            q,
            unsigned,
            size,
            scalar,
            rn,
            rm,
            rd,
        } => {
            let base = if unsigned { "ushl" } else { "sshl" };
            if scalar {
                return format!("{base}\td{rd}, d{rn}, d{rm}");
            }
            let (elem, lanes) = match size {
                0 => ("s", if q { 4 } else { 2 }),
                1 => ("d", if q { 2 } else { 1 }),
                _ => ("h", if q { 8 } else { 4 }),
            };
            format!("{base}\tv{rd}.{lanes}{elem}, v{rn}.{lanes}{elem}, v{rm}")
        }
        NeonUmov {
            size,
            index,
            rn,
            rd,
        } => match size {
            0 => format!("mov.s\tw{rd}, v{rn}[{index}]"),
            1 => format!("mov.d\tx{rd}, v{rn}[{index}]"),
            _ => format!("umov.b\tw{rd}, v{rn}[{index}]"),
        },
        NeonExt {
            q,
            offset,
            rn,
            rm,
            rd,
        } => {
            let _ = q;
            let arr = if q { "16b" } else { "8b" };
            format!(
                "ext\t{}.{arr}, {}.{arr}, {}.{arr}, #{offset}",
                vname(rd),
                vname(rn),
                vname(rm)
            )
        }
        NeonMovi64 { q, imm, rd } => {
            let _ = q;
            format!("movi\t{}.2d, #0x{imm:02x}", vname(rd))
        }
        NeonMovi { q, bits, value, rd } => {
            let _ = q;
            let _ = bits;
            format!("movi\t{}.#0x{:x}", vname(rd), value)
        }
        NeonCopy {
            q,
            op,
            size,
            index,
            elem,
            rn,
            rd,
        } => {
            let _ = (q, size, elem);
            let name = match op {
                0 => "dup",
                1 => "ins",
                _ => "mov",
            };
            format!("{name}\t{}.s[{}], {}", vname(rd), index, vname(rn))
        }
        NeonDupGeneral { q, size, rn, rd } => {
            let suffix = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            let arr = if q { "16" } else { "8" };
            format!(
                "dup\t{}.{}b, {}",
                vname(rd),
                if suffix == "d" { "1" } else { arr },
                wname(rn)
            )
        }
        NeonThreeSame {
            q,
            op,
            size,
            rm,
            rn,
            rd,
        } => {
            let arr = if q { "16" } else { "8" };
            let suffix = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            format!(
                "{:?}\t{}.{}{}, {}.{}{}, {}.{}{}",
                op,
                vname(rd),
                arr,
                suffix,
                vname(rn),
                arr,
                suffix,
                vname(rm),
                arr,
                suffix
            )
        }
        NeonBitwise { q, op, rm, rn, rd } => {
            let arr = if q { "16b" } else { "8b" };
            format!(
                "{:?}\t{}.{arr}, {}.{arr}, {}.{arr}",
                op,
                vname(rd),
                vname(rn),
                vname(rm)
            )
        }
        NeonOneSrc {
            q,
            op,
            size,
            rn,
            rd,
        } => {
            let arr = if q { "16" } else { "8" };
            let suffix = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            let name = match op {
                NeonOneSrcOp::Neg => "neg",
                NeonOneSrcOp::Abs => "abs",
                NeonOneSrcOp::Not => "not",
                NeonOneSrcOp::ByteCount => "cnt",
                NeonOneSrcOp::Rev16 => "rev16",
                NeonOneSrcOp::Rev32 => "rev32",
                NeonOneSrcOp::Rbit => "rbit",
            };
            format!(
                "{name}\t{}.{}{}, {}.{}{}",
                vname(rd),
                arr,
                suffix,
                vname(rn),
                arr,
                suffix
            )
        }
        NeonFpOneSrc {
            q,
            fneg,
            sqrt,
            size,
            rn,
            rd,
        } => {
            let _ = size;
            let arr = if q { "4s" } else { "2s" };
            let name = if sqrt {
                "fsqrt"
            } else if fneg {
                "fneg"
            } else {
                "fabs"
            };
            format!("{name}\t{}.{arr}, {}.{arr}", vname(rd), vname(rn))
        }
        SveWhile {
            size,
            sf,
            unsigned,
            le,
            rn,
            rm,
            pd,
        } => {
            let name = match (unsigned, le) {
                (true, false) => "whilelo\t",
                (true, true) => "whilels\t",
                (false, false) => "whilelt\t",
                (false, true) => "whilele\t",
            };
            let size_c = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            format!(
                "{name}p{}.{size_c}, {}, {}",
                pd,
                if sf { xname(rn) } else { format!("w{}", rn) },
                if sf { xname(rm) } else { format!("w{}", rm) }
            )
        }
        SveCnt {
            size,
            pattern: _,
            imm,
            rd,
        } => {
            let name = match size {
                0 => "cntb",
                1 => "cnth",
                2 => "cntw",
                _ => "cntd",
            };
            format!("{name}\t{}, all, mul #{imm}", xname(rd))
        }
        SveIncp {
            dec,
            scalar,
            size,
            pg,
            rd,
        } => {
            let name = if dec { "decp" } else { "incp" };
            let size_c = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            if scalar {
                return format!("{name}\t{}, p{}.{size_c}", xname(rd), pg);
            }
            format!("{name}\tz{}.{size_c}, p{}.{size_c}", rd, pg)
        }
        SvePredLogical {
            func,
            set,
            pd,
            pg,
            pn,
            pm,
        } => {
            let name = match (func >> 2, func & 3) {
                (0, 0) if !set => "and",
                (0, 0) => "ands",
                (0, 1) if !set => "bic",
                (0, 1) => "bics",
                (0, 2) if !set => "eor",
                (0, 2) => "eors",
                (1, 0) if !set => "orr",
                (1, 0) => "orrs",
                (1, 1) if !set => "orn",
                (1, 1) => "orns",
                (1, 2) if !set => "nor",
                (1, 2) => "nors",
                (1, 3) if !set => "nand",
                _ => "nands",
            };
            format!("{name}\tp{}.b, p{}/z, p{}.b, p{}.b", pd, pg, pn, pm)
        }
        SveLd1 {
            size,
            imm,
            pg,
            rn,
            zt,
        } => {
            let name = match size {
                0 => "ld1b",
                1 => "ld1h",
                2 => "ld1w",
                _ => "ld1d",
            };
            let size_c = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            format!(
                "{name}\t{{z{}.{size_c}}}, p{}/z, [{}{}]",
                zt,
                pg,
                xname(rn),
                if imm != 0 {
                    format!(", #{}, MUL VL", imm)
                } else {
                    String::new()
                }
            )
        }
        SveSt1 {
            size,
            imm,
            pg,
            rn,
            zt,
        } => {
            let name = match size {
                0 => "st1b",
                1 => "st1h",
                2 => "st1w",
                _ => "st1d",
            };
            let size_c = match size {
                0 => "b",
                1 => "h",
                2 => "s",
                _ => "d",
            };
            format!(
                "{name}\t{{z{}.{size_c}}}, p{}, [{}{}]",
                zt,
                pg,
                xname(rn),
                if imm != 0 {
                    format!(", #{}, MUL VL", imm)
                } else {
                    String::new()
                }
            )
        }
    }
}

fn render_simple(mn: &str, ops: Vec<String>) -> String {
    if ops.is_empty() {
        return mn.to_string();
    }
    format!("{mn}\t{}", ops.join(", "))
}

fn sys_render(w: u32) -> String {
    // Hints: NOP/YIELD/WFE/WFI/SEV/SEVL.
    if w & 0xfffff01f == 0xd503201f {
        let im = (w >> 5) & 0x7f;
        return match im {
            0 => "nop".to_string(),
            1 => "yield".to_string(),
            2 => "wfe".to_string(),
            3 => "wfi".to_string(),
            4 => "sev".to_string(),
            5 => "sevl".to_string(),
            _ => format!("hint #{im}"),
        };
    }
    match w {
        0xd503309f => return "dsb sy".to_string(),
        0xd50334bf => return "dmb sy".to_string(),
        0xd503305f => return "dsb ish".to_string(),
        0xd503349f => return "dsb ishst".to_string(),
        0xd5033bbf => return "dmb ish".to_string(),
        0xd5033fdf => return "isb".to_string(),
        _ => {}
    }
    // MRS (rt bit0, L=1) vs MSR (register, L=0) vs MSR (immediate, Rt=0x1f).
    if iphem_core::sysreg::is_system(w) && matches!((w >> 19) & 0x7, 3 | 7) {
        // MRS or MSR(reg): rt in low 5 bits; L=bit21.
        let rt = w & 31;
        let l = (w >> 21) & 1;
        let op0 = ((w >> 19) & 1) + 2;
        let op1 = (w >> 16) & 7;
        let crn = (w >> 12) & 15;
        let crm = (w >> 8) & 15;
        let op2 = (w >> 5) & 7;
        let name = sysreg_name(op0, op1, crn, crm, op2);
        if l == 1 {
            return format!("mrs\t{}, {}", xname(rt), name);
        }
        return format!("msr\t{}, {}", name, xname(rt));
    }
    format!("sys\t0x{w:08x}")
}

type SysregKey = (u32, u32, u32, u32, u32);

fn sysreg_name(op0: u32, op1: u32, crn: u32, crm: u32, op2: u32) -> String {
    let table: &[(SysregKey, &str)] = &[
        ((3, 0, 0, 0, 0), "MIDR_EL1"),
        ((3, 0, 0, 0, 5), "MPIDR_EL1"),
        ((3, 0, 0, 0, 6), "REVIDR_EL1"),
        ((3, 0, 0, 2, 0), "ID_ISAR0_EL1"),
        ((3, 0, 0, 2, 1), "ID_ISAR1_EL1"),
        ((3, 0, 0, 2, 2), "ID_ISAR2_EL1"),
        ((3, 0, 0, 2, 3), "ID_ISAR3_EL1"),
        ((3, 0, 0, 2, 4), "ID_ISAR4_EL1"),
        ((3, 0, 0, 2, 5), "ID_ISAR5_EL1"),
        ((3, 0, 0, 4, 0), "ID_AA64PFR0_EL1"),
        ((3, 0, 0, 4, 1), "ID_AA64PFR1_EL1"),
        ((3, 0, 0, 4, 2), "ID_AA64PFR2_EL1"),
        ((3, 0, 0, 5, 0), "ID_AA64DFR0_EL1"),
        ((3, 0, 0, 5, 1), "ID_AA64DFR1_EL1"),
        ((3, 0, 0, 5, 2), "ID_AA64DFR2_EL1"),
        ((3, 0, 0, 6, 0), "ID_AA64ISAR0_EL1"),
        ((3, 0, 0, 6, 1), "ID_AA64ISAR1_EL1"),
        ((3, 0, 0, 6, 2), "ID_AA64ISAR2_EL1"),
        ((3, 0, 0, 6, 3), "ID_AA64ISAR3_EL1"),
        ((3, 0, 0, 7, 0), "ID_AA64MMFR0_EL1"),
        ((3, 0, 0, 7, 1), "ID_AA64MMFR1_EL1"),
        ((3, 0, 0, 7, 2), "ID_AA64MMFR2_EL1"),
        ((3, 0, 0, 7, 3), "ID_AA64MMFR3_EL1"),
        ((3, 0, 1, 0, 0), "SCTLR_EL1"),
        ((3, 0, 1, 0, 1), "ACTLR_EL1"),
        ((3, 0, 1, 0, 2), "CPACR_EL1"),
        ((3, 0, 2, 0, 0), "TTBR0_EL1"),
        ((3, 0, 2, 0, 1), "TTBR1_EL1"),
        ((3, 0, 2, 0, 2), "TCR_EL1"),
        ((3, 0, 2, 1, 0), "APIAKeyLo_EL1"),
        ((3, 0, 2, 1, 1), "APIAKeyHi_EL1"),
        ((3, 0, 2, 1, 2), "APIBKeyLo_EL1"),
        ((3, 0, 2, 1, 3), "APIBKeyHi_EL1"),
        ((3, 0, 2, 2, 0), "APDAKeyLo_EL1"),
        ((3, 0, 2, 2, 1), "APDAKeyHi_EL1"),
        ((3, 0, 2, 2, 2), "APDBKeyLo_EL1"),
        ((3, 0, 2, 2, 3), "APDBKeyHi_EL1"),
        ((3, 0, 2, 3, 0), "APGAKeyLo_EL1"),
        ((3, 0, 2, 3, 1), "APGAKeyHi_EL1"),
        ((3, 0, 4, 0, 0), "SPSR_EL1"),
        ((3, 0, 4, 0, 1), "ELR_EL1"),
        ((3, 0, 4, 1, 0), "SP_EL0"),
        ((3, 0, 4, 2, 0), "SPSel"),
        ((3, 0, 4, 2, 3), "PAN"),
        ((3, 0, 4, 2, 4), "UAO"),
        ((3, 0, 5, 2, 0), "ESR_EL1"),
        ((3, 0, 6, 0, 0), "FAR_EL1"),
        ((3, 0, 7, 4, 0), "PAR_EL1"),
        ((3, 0, 10, 2, 0), "MAIR_EL1"),
        ((3, 0, 12, 0, 0), "VBAR_EL1"),
        ((3, 0, 13, 0, 4), "TPIDR_EL1"),
        ((3, 0, 14, 1, 0), "CNTKCTL_EL1"),
        ((3, 1, 0, 0, 0), "CCSIDR_EL1"),
        ((3, 1, 0, 0, 1), "CLIDR_EL1"),
        ((3, 2, 0, 0, 0), "CSSELR_EL1"),
        ((3, 1, 0, 0, 7), "AIDR_EL1"),
        ((3, 3, 0, 0, 1), "CTR_EL0"),
        ((3, 3, 0, 0, 7), "DCZID_EL0"),
        ((3, 3, 4, 2, 0), "NZCV"),
        ((3, 3, 4, 2, 1), "DAIF"),
        ((3, 3, 4, 4, 0), "FPCR"),
        ((3, 3, 4, 4, 1), "FPSR"),
        ((3, 3, 13, 0, 2), "TPIDR_EL0"),
        ((3, 3, 13, 0, 3), "TPIDRRO_EL0"),
        ((3, 3, 14, 0, 0), "CNTFRQ_EL0"),
        ((3, 3, 14, 0, 1), "CNTPCT_EL0"),
        ((3, 3, 14, 0, 2), "CNTVCT_EL0"),
        ((3, 3, 14, 0, 5), "CNTPCTSS_EL0"),
        ((3, 3, 14, 2, 0), "CNTP_TVAL_EL0"),
        ((3, 3, 14, 2, 1), "CNTP_CTL_EL0"),
        ((3, 4, 1, 0, 0), "SCTLR_EL2"),
        ((3, 4, 2, 0, 2), "TCR_EL2"),
        ((3, 4, 4, 0, 0), "SPSR_EL2"),
        ((3, 4, 4, 0, 1), "ELR_EL2"),
        ((3, 4, 4, 1, 0), "SP_EL1"),
        ((3, 4, 14, 0, 3), "CNTVOFF_EL2"),
        ((3, 6, 4, 0, 0), "SPSR_EL3"),
        ((3, 6, 4, 0, 1), "ELR_EL3"),
        ((3, 6, 4, 1, 0), "SP_EL2"),
        ((2, 3, 0, 1, 0), "MDCCSR_EL0"),
    ];
    table
        .iter()
        .find(|((o0, o1, c0, c1, o2), _)| {
            *o0 == op0 && *o1 == op1 && *c0 == crn && *c1 == crm && *o2 == op2
        })
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| format!("S{}_00_{}_C{crn}_C{crm}_{}", op0, op1, op2 & 7))
}

/// FP precision letter for the scalar floating-point instructions: the
/// `size` field (bits[23:22]) encodes 0=single, 1=double, 2=half.
fn fp_prec(op: u32) -> &'static str {
    match (op >> 22) & 3 {
        0 => "s",
        1 => "d",
        2 => "h",
        _ => "?",
    }
}

fn ldrstr_load_opc(_size: u32, opc: u32) -> (bool, bool) {
    let is_lit = opc == 3;
    // opc=00 is STR; all other values are loads, including signed forms
    // (LDRSB/LDRSH/LDRSW, opc=10/11).
    let load = opc != 0;
    (load, is_lit)
}

fn simd_elem_scale(size: u32, opc: u32) -> (&'static str, u32) {
    let q = opc & 1 != 0 || size == 2 && opc & 2 != 0;
    let (elem, scale) = match (size, q) {
        (0, _) => ("b", 1u32),
        (1, _) => ("h", 2),
        (2, false) => ("s", 4),
        (2, true) => ("q", 16),
        (3, _) => ("d", 8),
        _ => ("q", 16),
    };
    (elem, scale)
}

#[allow(dead_code)]
fn fp_default_nan_h() -> u16 {
    0x7e00
}

fn h_to_f32(h: u16) -> f32 {
    let sign = ((h & 0x8000) as u32) << 16;
    let exp = (h >> 10) & 0x1f;
    let mant = (h & 0x03ff) as u32;
    if exp == 0 {
        if mant == 0 {
            f32::from_bits(sign)
        } else {
            let mut m = mant;
            let mut e = -14i32;
            while (m & 0x400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x3ff;
            let exp32 = (e + 127) as u32;
            f32::from_bits(sign | (exp32 << 23) | (m << 13))
        }
    } else if exp == 31 {
        let exp32 = 0xff;
        f32::from_bits(sign | (exp32 << 23) | (mant << 13))
    } else {
        let exp32 = (exp as i32 - 15 + 127) as u32;
        f32::from_bits(sign | (exp32 << 23) | (mant << 13))
    }
}

fn h_to_f64(h: u16) -> f64 {
    h_to_f32(h) as f64
}

fn f32_to_h(f: f32) -> u16 {
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp32 = ((bits >> 23) & 0xff) as i32;
    let mant32 = bits & 0x007f_ffff;
    if exp32 == 255 {
        return sign | 0x7c00 | if mant32 != 0 { 0x0200 } else { 0 };
    }
    let exp = exp32 - 127;
    if exp < -24 {
        return sign;
    }
    if exp < -14 {
        let shift = -14 - exp;
        let m = (mant32 | 0x0080_0000) >> (13 + shift);
        return sign | (m as u16);
    }
    if exp > 15 {
        return sign | 0x7c00;
    }
    let exp16 = (exp + 15) as u16;
    let mant16 = (mant32 >> 13) as u16;
    sign | (exp16 << 10) | mant16
}

#[allow(dead_code)]
fn f64_to_h(f: f64) -> u16 {
    f32_to_h(f as f32)
}

fn fp_imm_to_h(imm8: u32) -> u16 {
    let sign = ((imm8 & 0x80) as u16) << 8;
    let exp_mant = if (imm8 & 0x40) != 0 {
        0x3000u16
    } else {
        0x4000u16
    };
    sign | exp_mant | ((imm8 & 0x3f) << 6) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xpac_restores_address_extension_and_preserves_tags() {
        // Raw XPACI X6 from the 16A366 kernel, plus XPACD X6 and XPACLRI.
        let cases = [
            (
                0xdac143e6,
                16 | (16 << 16),
                0x1234_1234_5678_9abc,
                0x0000_1234_5678_9abc,
            ),
            (
                0xdac143e6,
                16 | (16 << 16),
                0x12b4_1234_5678_9abc,
                0xffff_1234_5678_9abc,
            ),
            (
                0xdac143e6,
                16 | (16 << 16) | (1 << 37),
                0xab34_1234_5678_9abc,
                0xab00_1234_5678_9abc,
            ),
            (
                0xdac147e6,
                16 | (16 << 16) | (1 << 38) | (1 << 52),
                0xabb4_1234_5678_9abc,
                0xabff_1234_5678_9abc,
            ),
            (
                0xdac143e6,
                16 | (16 << 16) | (1 << 38) | (1 << 52),
                0xabb4_1234_5678_9abc,
                0xffff_1234_5678_9abc,
            ),
            (
                0xd50320ff,
                25 | (25 << 16),
                0x1234_127f_5678_9abc,
                0x0000_007f_5678_9abc,
            ),
        ];
        for (word, tcr, pointer, expected) in cases {
            let mut m = Machine::new(iphem_core::MachineConfig {
                trace: false,
                quantum: 1,
            });
            m.cores.active_mut().state.el = ExceptionLevel::El1;
            m.cores.active_mut().state.tcr_el1 = tcr;
            m.cores.active_mut().state.pc = 0x1000;
            m.cores.active_mut().state.pstate = 0xa00003c5;
            m.cores.active_mut().state.sp = 0x8000;
            let rd = if word == 0xd50320ff { 30 } else { 6 };
            m.cores.active_mut().state.x[rd] = pointer;
            execute(&mut m, Instruction(word)).unwrap();
            assert_eq!(
                m.cores.active_mut().state.x[rd],
                expected,
                "word={word:x} tcr={tcr:x}"
            );
            assert_eq!(m.cores.active_mut().state.pc, 0x1004);
            assert_eq!(m.cores.active_mut().state.pstate, 0xa00003c5);
            assert_eq!(m.cores.active_mut().state.sp, 0x8000);
        }
        assert_eq!(disassemble(0, 0xdac143e6), "xpaci\tx6");
        assert_eq!(disassemble(0, 0xdac147e6), "xpacd\tx6");
        assert_eq!(disassemble(0, 0xd50320ff), "xpaclri");
        assert!(!matches!(decode(0xdac143c6), Some(Op::Xpac { .. })));
        assert!(!matches!(decode(0x5ac143e6), Some(Op::Xpac { .. })));
    }

    #[test]
    fn ccmn_decodes_and_adds_cmp_form() {
        let imm = 0xba418964u32; // ccmn x11, #0x1, #0x4, hi  (from the real kernel)
        let reg = 0xfa401160u32; // ccmp x11, x0, #0x0, ne (register form)
        assert!(matches!(
            decode(imm),
            Some(Op::CCmp {
                sub: false,
                is_imm: true,
                cond: 0x8,
                nzcv: 0x4,
                rm: 1,
                rn: 11,
                ..
            })
        ));
        assert!(matches!(
            decode(reg),
            Some(Op::CCmp {
                sub: true,
                is_imm: false,
                cond: 0x1,
                nzcv: 0x0,
                rm: 0,
                rn: 11,
                ..
            })
        ));
        assert_eq!(disassemble(0, imm), "ccmn\tx11, #1, #0x4, hi");
        assert_eq!(disassemble(0, reg), "ccmp\tx11, x0, #0x0, ne");
    }

    #[test]
    fn ccmn_execution_sets_flags_conditionally() {
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;
        m.cores.active_mut().state.x[11] = 0xa;
        m.cores.active_mut().state.x[0] = 0x9;
        execute(&mut m, Instruction(0xba418964)).unwrap(); // ccmn x11, #0x1, #0x4, hi
        assert_eq!(m.cores.active_mut().state.pstate & 0xf0000000, 0x0);
        execute(&mut m, Instruction(0xfa401160)).unwrap(); // ccmp x11, x0, #0x0, ne
        assert_eq!(m.cores.active_mut().state.pstate & 0xf0000000, 0x20000000);
        execute(&mut m, Instruction(0xba401160)).unwrap(); // ccmn x11, x0, #0x0, ne
        assert_eq!(m.cores.active_mut().state.pstate & 0xf0000000, 0x0);
        execute(&mut m, Instruction(0xfa40f165)).unwrap(); // ccmp x11, x0, #0x5, nv (unconditional -> taken)
        assert_eq!(m.cores.active_mut().state.pstate & 0xf0000000, 0x20000000);
        execute(&mut m, Instruction(0xfa400165)).unwrap(); // ccmp x11, x0, #0x5, eq (false -> else)
        assert_eq!(m.cores.active_mut().state.pstate & 0xf0000000, 0x50000000);
    }

    #[test]
    fn reverse_and_bitcount_one_source_ops() {
        // Decode: CNT/REV16/REV32/RBIT land in the one-source group selected
        // by op6; REV64 is its own NeonRev64 op pulled by an earlier mask.
        let cases = [
            0x0e205821, // cnt.8b v1, v1
            0x4e205842, // cnt.16b v2, v2
            0x0e201863, // rev16.8b v3, v3
            0x2e200884, // rev32.8b v4, v4
            0x2e6058c6, // rbit.8b v6, v6
        ];
        let expected = [
            (NeonOneSrcOp::ByteCount, 0, 1),
            (NeonOneSrcOp::ByteCount, 0, 2),
            (NeonOneSrcOp::Rev16, 0, 3),
            (NeonOneSrcOp::Rev32, 0, 4),
            (NeonOneSrcOp::Rbit, 1, 6),
        ];
        for (i, (w, (op, size, rd))) in cases.iter().zip(expected.iter()).enumerate() {
            assert!(
                matches!(decode(*w), Some(Op::NeonOneSrc { op: o, size: s, rd: r, .. }) if o == *op && s == *size && r == *rd),
                "case {i} word {w:#x} -> {} / {:?}",
                disassemble(0, *w),
                decode(*w)
            );
        }
        assert_eq!(disassemble(0, 0x0e205821), "cnt\tv1.8b, v1.8b");
        assert!(matches!(
            decode(0x0e2008a5),
            Some(Op::NeonRev64 { size: 0, rd: 5, .. })
        ));
        assert_eq!(disassemble(0, 0x0e2008a5), "rev64\tv5.8b, v5.8b");
        assert!(!matches!(decode(0x3e205820), Some(Op::NeonOneSrc { .. })));

        // Exec: v1 = bytes [0x03, 0xff, ...] -> CNT -> [2, 8, ...].
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;
        m.cores.active_mut().state.v[1] = 0x03ff;
        execute(&mut m, Instruction(0x0e205821)).unwrap();
        assert_eq!(m.cores.active_mut().state.v[1] as u16, 0x0208);
        // REV64 of v2 = 0x0102030405060708 -> 0x0807060504030201.
        m.cores.active_mut().state.v[2] = 0x0102_0304_0506_0708;
        execute(&mut m, Instruction(0x0e200842)).unwrap(); // rev64.8b v2, v2
        assert_eq!(
            m.cores.active_mut().state.v[2] as u64,
            0x0807_0605_0403_0201
        );
        // RBIT of byte 0x01 -> 0x80.
        m.cores.active_mut().state.v[3] = 0x01;
        execute(&mut m, Instruction(0x2e605863)).unwrap(); // rbit.8b v3, v3
        assert_eq!(m.cores.active_mut().state.v[3] as u16, 0x0080);
    }

    #[test]
    fn addlv_folds_vector_into_scalar() {
        // Decode the SADDLV/UADDLV family across U, size, and Q.
        let cases = [
            0x2e303800u32, // uaddlv.8b h0, v0
            0x6e303821,    // uaddlv.16b h1, v1
            0x2e703842,    // uaddlv.4h s2, v2
            0x6e703863,    // uaddlv.8h s3, v3
            0x0e303884,    // saddlv.8b h4, v4
            0x4e3038a5,    // saddlv.16b h5, v5
            0x0e7038c6,    // saddlv.4h s6, v6
            0x4e7038e7,    // saddlv.8h s7, v7
        ];
        for (i, (w, (size, signed, q, rn, rd))) in cases
            .iter()
            .zip([
                (0, false, false, 0, 0),
                (0, false, true, 1, 1),
                (1, false, false, 2, 2),
                (1, false, true, 3, 3),
                (0, true, false, 4, 4),
                (0, true, true, 5, 5),
                (1, true, false, 6, 6),
                (1, true, true, 7, 7),
            ])
            .enumerate()
        {
            assert!(
                matches!(decode(*w), Some(Op::NeonAddLv { size: s, signed: g, q: qu, rn: n, rd: d, .. }) if s == size && g == signed && qu == q && n == rn && d == rd),
                "case {i} word {w:#x} -> {} / {:?}",
                disassemble(0, *w),
                decode(*w)
            );
        }
        assert_eq!(disassemble(0, 0x2e303821), "uaddlv\th1, v1.8b");
        assert_eq!(disassemble(0, 0x6e303821), "uaddlv\th1, v1.16b");
        assert_eq!(disassemble(0, 0x0e7038c6), "saddlv\ts6, v6.4h");

        // Exec: v1 = 8 bits each 0x03, plus 8 zero bits -> uaddlv.8b -> 24.
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;
        m.cores.active_mut().state.v[1] = 0x0303_0303_0303_0303;
        execute(&mut m, Instruction(0x2e303821)).unwrap(); // uaddlv.8b h1, v1
        assert_eq!(m.cores.active_mut().state.v[1] as u16, 8 * 0x03);
        // saddlv.4h of [0x8000, 0x8000, 0x0001, 0x0001] -> signed sum.
        m.cores.active_mut().state.v[6] = 0x0001_0001_8000_8000;
        execute(&mut m, Instruction(0x0e7038c6)).unwrap(); // saddlv.4h s6, v6
        assert_eq!(
            m.cores.active_mut().state.v[6] as u32 as i32,
            -32768 - 32768 + 1 + 1
        );
    }

    #[test]
    fn pac_sign_round_trips_and_auth_faults() {
        // PACIA/PACIB/PACDA/PACDB sign; a machine that authenticates with a
        // different key must fault; the matching key restores the original.
        let tcr = 16 | (16 << 16);
        let cases = [
            (0xdac10020u32, 0xdac11020u32), // PACIA x0,x1  / AUTIA x0,x1
            (0xdac10422u32, 0xdac11422u32), // PACIB x2,x1  / AUTIB x2,x1
            (0xdac10824u32, 0xdac11824u32), // PACDA x4,x1  / AUTDA x4,x1
            (0xdac10c26u32, 0xdac11c26u32), // PACDB x6,x1  / AUTDB x6,x1
        ];
        let mut sample = 0;
        for (sign, auth) in cases {
            let mut m = Machine::new(iphem_core::MachineConfig {
                trace: false,
                quantum: 1,
            });
            m.cores.active_mut().state.el = ExceptionLevel::El1;
            m.cores.active_mut().state.tcr_el1 = tcr;
            m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
            m.cores.active_mut().state.pc = 0x1000;
            m.cores.active_mut().state.pstate = 0xa00003c5;
            let rd = (sign & 31) as usize;
            let ptr = 0x0000_1234_5678_9abc; // canonical low-half address
            m.cores.active_mut().state.x[rd] = ptr;
            m.cores.active_mut().state.x[1] = 0x1122_3344_5566_7788; // modifier
            execute(&mut m, Instruction(sign)).unwrap();
            let signed = m.cores.active_mut().state.x[rd];
            assert_ne!(signed, ptr, "sign={sign:x} is identity");
            // Same signed pointer fails under a different key.
            let mut wrong = Machine::new(iphem_core::MachineConfig {
                trace: false,
                quantum: 1,
            });
            wrong.cores.active_mut().state.el = ExceptionLevel::El1;
            wrong.cores.active_mut().state.tcr_el1 = tcr;
            wrong.cores.active_mut().state.sctlr_el1 =
                (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
            wrong.cores.active_mut().state.pc = 0x2000;
            wrong.cores.active_mut().state.x[rd] = signed;
            wrong.cores.active_mut().state.x[1] = m.cores.active_mut().state.x[1];
            match (sign >> 10) & 3 {
                0 => wrong.cores.active_mut().state.apia_lo ^= 1,
                1 => wrong.cores.active_mut().state.apib_lo ^= 1,
                2 => wrong.cores.active_mut().state.apda_lo ^= 1,
                _ => wrong.cores.active_mut().state.apdb_lo ^= 1,
            }
            assert!(
                execute(&mut wrong, Instruction(auth)).is_err(),
                "auth={auth:x} must fault under the wrong key"
            );
            // Matching key restores the original pointer.
            m.cores.active_mut().state.pc = 0x3000;
            execute(&mut m, Instruction(auth)).unwrap();
            assert_eq!(
                m.cores.active_mut().state.x[rd],
                ptr,
                "auth={auth:x} did not round-trip"
            );
            sample = signed;
        }
        assert_ne!(sample, 0);
    }

    /// CSEL/CSINC/CSINV/CSNEG: Rn is selected unchanged when the condition
    /// holds, and the operation is applied to Rm when it does not.
    ///
    /// The operand that is incremented, inverted or negated is **Rm**, not Rn.
    /// This is the part that is easy to get backwards, so the expectations here
    /// were taken from execution on AArch64 hardware rather than from reading
    /// the manual: `csinc w0, w1, w2, eq` yields w1 with Z set and w2 + 1 with
    /// Z clear. Encodings are clang-exact (`csel w0,w1,w2,eq` = 0x1a820020,
    /// `csinc` = 0x1a820420, `csinv` = 0x5a820020, `csneg` = 0x5a820420,
    /// `csel x0,x1,x2,ne` = 0x9a821020).
    ///
    /// The SecureROM depends on the polarity at `0x10000fcec`: it computes
    /// `csinc w0, wzr, wzr, eq` after `cmp (globals|globals), #0` and then does
    /// `cbz w0`, so the SEP-mail open *fails* when both platform globals read
    /// zero and *succeeds* when either is set.
    #[test]
    fn conditional_select_family_operates_on_rm() {
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        m.cores.active_mut().state.pc = 0x1000;

        let run = |m: &mut Machine, op: u32, z: bool| {
            let s = &mut m.cores.active_mut().state;
            s.x[1] = 0x1111_1111;
            s.x[2] = 0x2222_2222;
            s.x[0] = 0xdead_beef;
            s.pstate = if z { 1 << 30 } else { 0 };
            execute(m, Instruction(op)).unwrap();
            m.cores.active().state.x[0]
        };

        // Condition holds: Rn unchanged, for every form.
        assert_eq!(run(&mut m, 0x1a820020, true), 0x1111_1111); // csel  w0,w1,w2,eq
        assert_eq!(run(&mut m, 0x1a820420, true), 0x1111_1111); // csinc w0,w1,w2,eq
        assert_eq!(run(&mut m, 0x5a820020, true), 0x1111_1111); // csinv
        assert_eq!(run(&mut m, 0x5a820420, true), 0x1111_1111); // csneg
                                                                // Condition clear: the operation applies to Rm (32-bit forms truncate).
        assert_eq!(run(&mut m, 0x1a820020, false), 0x2222_2222);
        assert_eq!(run(&mut m, 0x1a820420, false), 0x2222_2223);
        assert_eq!(run(&mut m, 0x5a820020, false), 0xdddd_dddd);
        assert_eq!(run(&mut m, 0x5a820420, false), 0xdddd_ddde); // -0x2222_2222 & 0xffff_ffff

        // The SecureROM's own form. Register 31 is the zero register here, so
        // with the condition set the result is 0, and with it clear the
        // increment of a zero Rm gives 1.
        assert_eq!(run(&mut m, 0x1a9f07e0, true), 0);
        assert_eq!(run(&mut m, 0x1a9f07e0, false), 1);

        // 64-bit forms keep the full width. `ne` is the opposite of `eq`.
        assert_eq!(run(&mut m, 0x9a821020, true), 0x2222_2222);
        assert_eq!(run(&mut m, 0x9a821020, false), 0x1111_1111);
        let s = &mut m.cores.active_mut().state;
        s.x[1] = 0xffff_ffff_ffff_ffff;
        s.x[2] = 0;
        s.pstate = 0;
        execute(&mut m, Instruction(0x9a8207e0)).unwrap(); // csinc x0, xzr, x2, eq
        assert_eq!(m.cores.active().state.x[0], 1); // Rm + 1 with Z clear
    }

    /// ADRP must render the address the executor actually forms: the
    /// instruction's own page plus a page-scaled immediate.
    ///
    /// Printing `pc + imm` (with `imm` in page units) misreports every ADRP
    /// target. That is not cosmetic: the SecureROM's readiness check at
    /// 0x10000fcec is `adrp x8, <page>` + `ldr x8, [x8, #0xe88]`, and the wrong
    /// page sent the analysis to ROM data (0x1000abe88, always zero) instead of
    /// the AP DRAM word it really reads (0x19c011e88). The encoding below is the
    /// ROM's own word, and the expected page is the one a hand decode of
    /// `imm21 = 0x9c002` gives.
    #[test]
    fn adrp_disassembly_reports_the_executors_address() {
        // 0xd04e0008 at 0x10000fcec -> 0x19c011000.
        let op = 0xd04e_0008u32;
        let pc = 0x0001_0000_fcecu64;
        assert_eq!(
            disassemble(pc, op),
            "adrp\tx8, 0x19c011000",
            "the printed page must match the executor's"
        );
        // And the two must agree when executed.
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        m.cores.active_mut().state.pc = pc;
        execute(&mut m, Instruction(op)).unwrap();
        assert_eq!(m.cores.active().state.x[8], 0x19c011000);
    }

    #[test]
    fn pac_auth_faults_on_corrupted_pointer() {
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        m.cores.active_mut().state.pc = 0x1000;
        let ptr = 0x0000_1234_5678_9abc;
        m.cores.active_mut().state.x[0] = ptr;
        m.cores.active_mut().state.x[1] = 0;
        execute(&mut m, Instruction(0xdac10020)).unwrap(); // PACIA x0,x1
        let signed = m.cores.active_mut().state.x[0];
        m.cores.active_mut().state.x[0] = signed ^ (1 << 48); // corrupt the code
        assert!(execute(&mut m, Instruction(0xdac11020)).is_err()); // AUTIA faults
    }

    #[test]
    fn pac_zero_forms_and_hint_forms() {
        // PACIZA/PACIZB/PACDZA/PACDZB use a zero modifier (rn = XZR).
        let tcr = 16 | (16 << 16);
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.tcr_el1 = tcr;
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;
        m.cores.active_mut().state.sp = 0x8000;
        m.cores.active_mut().state.x[20] = 0x1234_1234_5678_9abc;
        execute(&mut m, Instruction(0xdac123f4)).unwrap(); // PACIZA x20
        assert_ne!(m.cores.active_mut().state.x[20], 0x1234_1234_5678_9abc);
        // PACIASP x30, SP -> AUTIASP x30, SP restores the sealed LR.
        let lr = 0xffff_fffe_1234_5678u64; // canonical high-half address
        m.cores.active_mut().state.x[30] = lr;
        m.cores.active_mut().state.pc = 0x2000;
        execute(&mut m, Instruction(0xd503233f)).unwrap(); // PACIASP
        let sealed = m.cores.active_mut().state.x[30];
        assert_ne!(sealed, lr);
        execute(&mut m, Instruction(0xd50323bf)).unwrap(); // AUTIASP
        assert_eq!(m.cores.active_mut().state.x[30], lr);
        // PACIA1716 sign: x17 <- sign(x17, x16); AUTIA1716 restores.
        let p16 = 0xffff_fffe_5000_1234u64;
        m.cores.active_mut().state.x[17] = p16;
        m.cores.active_mut().state.x[16] = 0xdead_beef_cafe_f00d;
        execute(&mut m, Instruction(0xd503211f)).unwrap(); // PACIA1716
        assert_ne!(m.cores.active_mut().state.x[17], p16);
        assert_eq!(m.cores.active_mut().state.x[16], 0xdead_beef_cafe_f00d);
        execute(&mut m, Instruction(0xd503219f)).unwrap(); // AUTIA1716
        assert_eq!(m.cores.active_mut().state.x[17], p16);
    }

    #[test]
    fn pac_branches_and_return_auth() {
        // BRAA x7, x5 authenticates x7 (target) with key A + modifier x5 and
        // branches to it.
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;
        let target = 0xffff_fffe_abc0_0000u64; // canonical high-half address
                                               // Sign the target with key A and a zero modifier (PACIA x6, x5 with
                                               // x5 = 0), then branch through BRAA x7, x5.
        m.cores.active_mut().state.x[6] = target;
        m.cores.active_mut().state.x[5] = 0;
        execute(&mut m, Instruction(0xdac100a6)).unwrap(); // PACIA x6, x5
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.x[7] = m.cores.active_mut().state.x[6];
        m.cores.active_mut().state.x[6] = 0xdead;
        execute(&mut m, Instruction(0xd71f08e5)).unwrap(); // BRAA x7, x5
        assert_eq!(m.cores.active_mut().state.pc, target);
        // BLRAA must link into x30 as well as branch.
        m.cores.active_mut().state.pc = 0x2000;
        m.cores.active_mut().state.x[8] = m.cores.active_mut().state.x[7];
        execute(&mut m, Instruction(0xd73f0905)).unwrap(); // BLRAA x8, x5
        assert_eq!(m.cores.active_mut().state.pc, target);
        assert_eq!(m.cores.active_mut().state.x[30], 0x2004);
        // RETAA authenticates LR using SP, matching the PACIASP prologue.
        let lr = 0xffff_fffe_4000_1234u64;
        m.cores.active_mut().state.pc = 0x3000;
        m.cores.active_mut().state.x[30] = lr;
        m.cores.active_mut().state.sp = 0x19c20000;
        execute(&mut m, Instruction(0xd503233f)).unwrap(); // PACIASP
        execute(&mut m, Instruction(0xd65f0bff)).unwrap(); // RETAA
        assert_eq!(m.cores.active_mut().state.pc, lr);
        // Key B must select APIB, and a nonzero SP must survive the round trip.
        m.cores.active_mut().state.apib_lo = 0xfeedbeef;
        m.cores.active_mut().state.x[30] = lr;
        execute(&mut m, Instruction(0xd503237f)).unwrap(); // PACIBSP
        execute(&mut m, Instruction(0xd65f0fff)).unwrap(); // RETAB
        assert_eq!(m.cores.active_mut().state.pc, lr);
    }

    #[test]
    fn pac_key_sysregs_round_trip() {
        // MSR/MRS of APIAKeyLo_EL1 / Hi_EL1 must round-trip through state.
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.pc = 0x1000;
        execute(&mut m, Instruction(0xd51b_2100)).unwrap(); // MSR APIAKeyLo_EL1, x0
        execute(&mut m, Instruction(0xd538_2100)).unwrap(); // MRS x0, APIAKeyLo_EL1
        assert_eq!(m.cores.active().state.x[0], m.cores.active().state.apia_lo);
        // A key change means a previously signed pointer no longer authenticates.
        let mut a = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        a.cores.active_mut().state.el = ExceptionLevel::El1;
        a.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
        a.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30) | (1 << 27) | (1 << 13);
        a.cores.active_mut().state.pc = 0x1000;
        a.cores.active_mut().state.x[0] = 0x0000_1234_5678_9abc;
        execute(&mut a, Instruction(0xdac10020)).unwrap();
        a.cores.active_mut().state.pc = 0x2000;
        a.cores.active_mut().state.apia_lo ^= 1; // rotate the key behind the code's back
        assert!(execute(&mut a, Instruction(0xdac11020)).is_err());
    }

    #[test]
    fn pac_disassemble_forms() {
        let cases: &[(u32, &str)] = &[
            (0xdac10020, "pacia\tx0, x1"),
            (0xdac10462, "pacib\tx2, x3"),
            (0xdac108a4, "pacda\tx4, x5"),
            (0xdac10ce6, "pacdb\tx6, x7"),
            (0xdac11128, "autia\tx8, x9"),
            (0xdac1156a, "autib\tx10, x11"),
            (0xdac119ac, "autda\tx12, x13"),
            (0xdac11dee, "autdb\tx14, x15"),
            (0xdac10022, "pacia\tx2, x1"),
            (0xdac123f4, "paciza\tx20"),
            (0xdac127f5, "pacizb\tx21"),
            (0xdac12bf6, "pacdza\tx22"),
            (0xdac12ff7, "pacdzb\tx23"),
            (0xdac133f8, "autiza\tx24"),
            (0xdac137f9, "autizb\tx25"),
            (0xdac13bfa, "autdza\tx26"),
            (0xdac13ffb, "autdzb\tx27"),
            (0xdac143f8, "xpaci\tx24"),
            (0xdac147f9, "xpacd\tx25"),
            (0xd50320ff, "xpaclri"),
            (0xd503211f, "pacia1716"),
            (0xd503215f, "pacib1716"),
            (0xd503219f, "autia1716"),
            (0xd50321df, "autib1716"),
            (0xd503233f, "paciasp"),
            (0xd503237f, "pacibsp"),
            (0xd50323bf, "autiasp"),
            (0xd50323ff, "autibsp"),
            (0xd71f0801, "braa\tx0, x1"),
            (0xd71f08a7, "braa\tx5, x7"),
            (0xd71f0d00, "brab\tx8, x0"),
            (0xd73f08a7, "blraa\tx5, x7"),
            (0xd73f0d00, "blrab\tx8, x0"),
            (0xd71f08ff, "braa\tx7, sp"),
            (0xd61f08ff, "braaz\tx7"),
            (0xd61f0cff, "brabz\tx7"),
            (0xd63f08ff, "blraaz\tx7"),
            (0xd63f0cff, "blrabz\tx7"),
            (0xd65f0bff, "retaa"),
            (0xd65f0fff, "retab"),
            (0xd69f0bff, "eretaa"),
            (0xd69f0fff, "eretab"),
        ];
        for (word, s) in cases {
            assert_eq!(disassemble(0, *word), *s, "word={word:x}");
        }
    }

    #[test]
    fn parse_unknown() {
        for op in [0u32, 1, 0xdeadbeef, 0xffffffff, 0x14000000, 0xd5000000] {
            let _ = decode(op);
        }
    }

    /// SEPOS's APCTL_EL1 lock acquire: `msr ..., #5` then `mrs` then
    /// `tbnz w0, #1`. The grant bit must be set on the read that follows the
    /// write, otherwise the guest spins forever at the lock. Both MRS and MSR
    /// must resolve to the same register through the shared (L-masked) key.
    #[test]
    fn apctl_el1_sep_lock_write_then_poll_sees_grant() {
        let mut m = Machine::new(iphem_core::MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = ExceptionLevel::El1;
        m.cores.active_mut().state.pc = 0x1000;
        m.cores.active_mut().state.pstate = 0xa00003c5;

        // Reset presents the XNU capability bits with no grant pending.
        execute(&mut m, Instruction(0xd53cf080)).unwrap();
        let reset_value = m.cores.active_mut().state.x[0];
        assert_eq!(reset_value, 0x42, "reset seed carries the capability bits");
        assert_eq!(reset_value & 0x1, 0, "no grant before a request");

        // SEPOS: mov w0, #5 ; msr x0, S3_4_C15_C0_4
        execute(&mut m, Instruction(0x528000a0)).unwrap();
        execute(&mut m, Instruction(0xd51cf080)).unwrap();
        assert_eq!(
            m.cores.active_mut().state.apctl_el1,
            5,
            "a nonzero token stores with the grant bit raised"
        );

        // SEPOS: mrs x0, S3_4_C15_C0_4 ; tbnz w0, #1 must see the grant.
        execute(&mut m, Instruction(0xd53cf080)).unwrap();
        let polled = m.cores.active_mut().state.x[0];
        assert_eq!(polled, 5, "the guest reads back its granted token");
        assert_eq!(polled & 0x1, 1, "guest must observe the lock grant");
        assert_eq!(polled & 0x4, 0x4, "the written token reads back");

        // The register keeps a plain round-trip contract for other values.
        execute(&mut m, Instruction(0x5280_00e0)).unwrap(); // mov w0, #7
        execute(&mut m, Instruction(0xd51cf080)).unwrap();
        execute(&mut m, Instruction(0xd53cf080)).unwrap();
        assert_eq!(
            m.cores.active_mut().state.x[0],
            7,
            "an already-granted value round-trips unchanged"
        );

        // A zero write releases the grant, so the handshake is symmetric.
        execute(&mut m, Instruction(0xd2800000)).unwrap();
        execute(&mut m, Instruction(0xd51cf080)).unwrap();
        execute(&mut m, Instruction(0xd53cf080)).unwrap();
        assert_eq!(
            m.cores.active_mut().state.x[0],
            0,
            "a zero write must release the grant"
        );
    }

    #[test]
    fn disassemble_smoke() {
        let cases: &[(u32, &str)] = &[
            (0x91004020, "add\tx0, x1, #0x10"),
            (0xd2800020, "movz\tx0, #0x1"),
            (0xd4000003, "smc\t#0x0"),
            (0xd53cf080, "mrs\tx0, S3_00_4_C15_C0_4"),
            (0xd51bd040, "msr\tTPIDR_EL0, x0"),
            (0xd53bd040, "mrs\tx0, TPIDR_EL0"),
            (0xd51bd061, "msr\tTPIDRRO_EL0, x1"),
            (0xd538d080, "mrs\tx0, TPIDR_EL1"),
            (0x927f0001, "and\tx1, x0, #0x2"),
            (0xdac10011, "pacia\tx17, x0"),
            (0xdac10402, "pacib\tx2, x0"),
            (0xdac10803, "pacda\tx3, x0"),
            (0xdac10c04, "pacdb\tx4, x0"),
            (0xd65f03c0, "ret"),
            (0xf90003e0, "str\tx0, [sp]"),
            (0x294adbf9, "ldp\tw25, w22, [sp, #0x54]"),
            (0xd28001a0, "movz\tx0, #0xd"),
            (0xd503201f, "nop"),
        ];
        for (op, want) in cases {
            let got = disassemble(0x1000, *op);
            assert_eq!(got.trim(), *want, "op 0x{:08x}", op);
        }

        // Backward unconditional branch renders an absolute target label.
        let got = disassemble(0x1000, 0x17ffffff);
        assert!(got.starts_with("b\t0x"), "backward branch got {got:?}");
    }

    /// The SP/zero-register naming and the system-register names are checked
    /// against `llvm-objdump` rather than against this decoder, so a drift in
    /// either shows up here instead of silently mis-naming a ROM instruction.
    /// The first case is the Cebu SEPROM's own frame setup at image `0xfc`.
    #[test]
    fn add_sub_immediate_names_the_stack_pointer_and_compares_like_llvm() {
        let cases: &[(u32, &str)] = &[
            // The Cebu SEPROM's own frame setup at image `0xfc`; llvm-objdump
            // aliases this one to the pseudo-instruction `mov x29, sp`.
            (0x910003fd, "add\tx29, sp, #0x0"),
            (0x910043bf, "add\tsp, x29, #0x10"),
            (0x91007fff, "add\tsp, sp, #0x1f"),
            (0x911fffff, "add\tsp, sp, #0x7ff"),
            (0x91007c3d, "add\tx29, x1, #0x1f"),
            (0x910043e0, "add\tx0, sp, #0x10"),
            (0xf1007d3f, "cmp\tx9, #0x1f"),
            (0xf1007fff, "cmp\tsp, #0x1f"),
            (0xb1007d3f, "cmn\tx9, #0x1f"),
            (0xb1007fff, "cmn\tsp, #0x1f"),
            (0x31003c00, "adds\tw0, w0, #0xf"),
            (0x2b0003e0, "adds\tw0, wzr, w0"),
            (0x71003c1f, "cmp\tw0, #0xf"),
            (0xd140041f, "sub\tsp, x0, #0x1, lsl #12"),
        ];
        for (op, want) in cases {
            assert_eq!(disassemble(0x1000, *op).trim(), *want, "op 0x{op:08x}");
        }
    }

    /// An MRS word from the checked-in AArch64 firmware corpus must render
    /// the register name `llvm-objdump` renders, and the IMPLEMENTATION
    /// DEFINED space (`CRn == 15`) must stay raw rather than acquire an
    /// invented name. Each case below is a representative corpus word, with
    /// the name `llvm-objdump -d` prints for it; the comparison over all 132
    /// corpus words was run out of band, and this table is the in-tree guard
    /// so a renamed or dropped row fails here. It is a subset, not the whole
    /// corpus, so it cannot be the thing that establishes the property.
    /// Word and name are one tuple rather than parallel arrays so a row
    /// cannot be paired with the wrong name. The two raw cases are the reason
    /// a filter that cannot fail fails here.
    #[test]
    fn mrs_and_msr_name_the_system_registers_the_firmware_images_use() {
        let cases: &[(u32, &str)] = &[
            (0xd5380000, "MIDR_EL1"),
            (0xd5381000, "SCTLR_EL1"),
            (0xd5382000, "TTBR0_EL1"),
            (0xd538a200, "MAIR_EL1"),
            (0xd538c000, "VBAR_EL1"),
            (0xd5384000, "SPSR_EL1"),
            (0xd5384023, "ELR_EL1"),
            (0xd5385201, "ESR_EL1"),
            (0xd5386001, "FAR_EL1"),
            (0xd5387401, "PAR_EL1"),
            (0xd538e100, "CNTKCTL_EL1"),
            (0xd5390001, "CCSIDR_EL1"),
            (0xd5390020, "CLIDR_EL1"),
            (0xd53b4222, "DAIF"),
            (0xd53b4402, "FPCR"),
            (0xd53b4422, "FPSR"),
            (0xd53bd040, "TPIDR_EL0"),
            (0xd53bd068, "TPIDRRO_EL0"),
            (0xd53be000, "CNTFRQ_EL0"),
            (0xd53be020, "CNTPCT_EL0"),
            // Same name as the word above: `Rm` is the MRS destination, not
            // part of the register name. `llvm-objdump` prints both this way.
            (0xd53be028, "CNTPCT_EL0"),
            (0xd53be040, "CNTVCT_EL0"),
            (0xd53be0a0, "CNTPCTSS_EL0"),
            (0xd53be220, "CNTP_CTL_EL0"),
            (0xd5384201, "SPSel"),
            (0xd5381040, "CPACR_EL1"),
            // IMPLEMENTATION DEFINED, must stay raw. The padding in the
            // generated name is ours, not LLVM's: the fallback format is
            // `S{op0}_00_{op1}_...`, so the corpus-wide comparison has to
            // treat `S3_0_..` and `S3_00_0_..` as the same spelling.
            (0xd538f000, "S3_00_0_C15_C0_0"),
            (0xd53cf080, "S3_00_4_C15_C0_4"),
        ];
        let mut out = String::new();
        for (word, _) in cases {
            out.push_str(&disassemble(0x1000, *word));
            out.push('\n');
        }
        let got: Vec<&str> = out
            .lines()
            .map(|l| l.rsplit_once(", ").expect("mrs operand").1)
            .collect();
        assert_eq!(got.len(), cases.len());
        for ((word, want), g) in cases.iter().zip(&got) {
            assert_eq!(*g, *want, "word 0x{word:08x}");
        }
    }
}

#[cfg(test)]
#[test]
fn test_decode_tpidr_el1_mrs() {
    // MRS X8, TPIDR_EL1 = 0xd538d088
    // MRS X20, TPIDR_EL1 = 0xd538d094
    let d1 = decode(0xd538d088u32);
    let d2 = decode(0xd538d094u32);
    assert!(
        matches!(d1, Some(Op::Sys(_))),
        "expected Sys for MRS X8 TPIDR_EL1, got {:?}",
        d1
    );
    assert!(
        matches!(d2, Some(Op::Sys(_))),
        "expected Sys for MRS X20 TPIDR_EL1, got {:?}",
        d2
    );
}

#[cfg(test)]
#[test]
fn test_execute_mrs_tpidr_el1() {
    use iphem_core::{BusRegion, ExceptionLevel, Machine, MachineConfig, SparseRam};
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0,
            size: 0x10000,
            device: Box::new(SparseRam::new(0, 0x10000)),
        })
        .unwrap();
    // place MRS X8, TPIDR_EL1 at PC=0
    m.mem_write_bytes(0, &0xd538d088u32.to_le_bytes()).unwrap();
    m.cores.active_mut().state.pc = 0;
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.pstate = 0x3c5;
    m.cores.active_mut().state.tpidr_el1 = 0xdeadbeef_cafe1234;
    let r = step(&mut m);
    assert!(r.is_ok(), "step failed: {:?}", r);
    assert_eq!(
        m.cores.active_mut().state.x[8],
        0xdeadbeef_cafe1234,
        "TPIDR_EL1 not read correctly"
    );
}
