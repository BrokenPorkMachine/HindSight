//! Architectural system-register identifiers and system-instruction decode.
//!
//! All encodings here were verified against a clang-assembled instruction
//! battery (`enc4_words.json`), including the exact field layout:
//!   system space   : (op & 0xffc00000) == 0xd5000000
//!   op0f = bits[21:19]: 7=MRS, 3=MSR(register), 1=SYS, 5=SYSL, 0=MSR-imm/CFINV
//!   op1 = bits[18:16], CRn = bits[15:12], CRm = bits[11:8], op2 = bits[7:5]
//!   key = (op >> 5) & 0xffff  (op1:CRn:CRm:op2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysReg {
    SctlrEl1,
    SctlrEl2,
    SctlrEl3,
    Ttbr0El1,
    Ttbr1El1,
    TcrEl1,
    MairEl1,
    ParEl1,
    VbarEl1,
    EsrEl1,
    FarEl1,
    ElrEl1,
    SpsrEl1,
    VbarEl2,
    EsrEl2,
    FarEl2,
    HpfarEl2,
    ElrEl2,
    SpsrEl2,
    HcrEl2,
    VttbrEl2,
    VtcrEl2,
    CptrEl2,
    VbarEl3,
    EsrEl3,
    FarEl3,
    ElrEl3,
    SpsrEl3,
    ScrEl3,
    CptrEl3,
    Daif,
    TpidrEl0,
    TpidrEl1,
    TpidrroEl0,
    CurrentEl,
    Spsel,
    Nzcv,
    MidrEl1,
    MpidrEl1,
    RevidrEl1,
    CtrEl0,
    DczidEl0,
    CntfrqEl0,
    CntvctEl0,
    CntpctEl0,
    CntpTvalEl0,
    CntpCtlEl0,
    CntpCvalEl0,
    CntvTvalEl0,
    CntvCtlEl0,
    CntvCvalEl0,
    CntvoffEl2,
    /// CNTPCTSS_EL0: Secure physical counter (S3_3_C2_C3_0, key 0xDF03).
    /// Reads the same global physical counter as CNTPCT_EL0 but from the
    /// Secure security state view; the emulator returns the raw cycle
    /// counter regardless of security state.
    CntpctssEl0,
    /// CNTP_OFF_EL2: physical counter offset for EL2 (S3_4_C2_C3_0, key
    /// 0xE320). The EL1/EL0 physical counter reads `cycle - cntpoff_el2`.
    CntpoffEl2,
    ClidrEl1,
    CcsidrEl1,
    Pfr0El1,
    Pfr1El1,
    Mmfr0El1,
    Mmfr1El1,
    Isar0El1,
    Isar1El1,
    SpEl0,
    SpEl1,
    SpEl2,
    Fpcr,
    Fpsr,
    // Phase 163: PMU + AMU registers (indexed variants encode the counter
    // number in their MRS key CRm/op2; the `key()` decoder pulls that index
    // out and returns these as fully-resolved instances).
    PmcrEl0,
    // Apple IMPDEF S3_3_C15_C7_0 (key 0xDFB8): cache/DRAM spec probe used by
    // iBoot bootloaders to size the per-core L2/fabric before enabling it.
    // Read layout (real A12+ hardware): bits[5:0] = number of banks,
    // bits[13:8] = log2(per-bank bytes), bits[21:16] = branch-cap. bit 63 is
    // a completion/ack flag: writes with a non-zero low-6-bit opcode set it,
    // writes clearing the opcode clear it (symmetric HW handshake).
    ImpCacheOp,
    /// Apple pointer-authentication control (S3_4_C15_C0_4).
    ApctlEl1,
    // ARMv8.3 pointer-authentication key registers (S3_0_C2_C1_* / C2_C2_*).
    ApiaLoEl1,
    ApiaHiEl1,
    ApibLoEl1,
    ApibHiEl1,
    ApdaLoEl1,
    ApdaHiEl1,
    ApdbLoEl1,
    ApdbHiEl1,
    ApgaLoEl1,
    ApgaHiEl1,
    PmcntensetEl0,
    PmcntenclrEl0,
    PmccntrEl0,
    PmevcntrEl0(usize),
    PmevtyperEl0(usize),
    // AMUv1 (Phase 190): Activity Monitors, Armv8.4 optional feature. The
    // encodings below are the A12-lineage register map cross-checked against
    // the Apple toolchain system-register table (llvm-objdump) and the ARM
    // DDI 0487 AMU space (op0=3, op1=3, CRn=13). The earlier prototype used
    // fabricated S3_3_C13_C6_0..2 keys and a CRn=0xC indexed decoder; those
    // replaced these exact encodings, so the old 0xDEB0/0xDEB1/0xDEB2 keys no
    // longer resolve to AMU registers (0xDEB0 now decodes to AMEVTYPER00_EL0).
    AmcrEl0,
    AmcfgrEl0,
    AmcggcrEl0,
    AmuserenEl0,
    AmcntenclrEl0,
    AmcntensetEl0,
    Amcntenclr1El0,
    Amcntenset1El0,
    AmevcntrEl0(u8),
    AmevtyperEl0(u8),
    Amevcntr1El0(u8),
    Amevtyper1El0(u8),
    // SPE (Phase 190): Armv8.2 Statistical Profiling, PMB* profiling-buffer
    // registers (verified encodings: MRS PMBIDR_EL1 = 0xd5389ae0).
    PmbidrEl1,
    PmblimitrEl1,
    PmbptrEl1,
    PmbsrEl1,
    /// MDSCR_EL1 (debug Monitor Debug System Control Register, op0=2 space).
    MdscrEl1,
    /// Phase 190 SVE vector-length control registers. ZCR_EL1 (op0=3 op1=0
    /// CRn=1 CRm=2 op2=0, MRS = 0xd5321400) governs EL1; ZCR_EL2 (op0=3
    /// op1=4 CRn=1 CRm=2 op2=0) is the EL2 view. Only the VQ field (bits 3:0)
    /// is meaningful; a zero VQ yields a 128-bit vector length.
    ZcrEl1,
    ZcrEl2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Barrier {
    Dsb,
    Dmb,
    Isb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PstateOp {
    DaifSet(u8),
    DaifClr(u8),
    Spsel(u32),
    Uao(u32),
    Pan(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemOp {
    /// Benign HINT (NOP, BTIBT, PAC/BTI, YIELD, SEV, ...) or unknown HINT.
    Hint,
    Wfe,
    Wfi,
    Clrex,
    Barrier(Barrier),
    Cfinv,
    Pstate(PstateOp),
    /// MRS: (`rt`, `key`)
    Mrs(u32, u16),
    /// MSR (register): (`rt`, `key`)
    Msr(u32, u16),
    /// SYS/SYSL: (`read`, `imm16` = CRn:CRm:op2, `rt`)
    Sys {
        read: bool,
        imm: u16,
        rt: u32,
    },
    /// MSR/Cfinv/Hint family; the word itself is kept for trace purposes.
    Raw(u32),
}

/// True when `op` lies in the system-instruction space.
pub fn is_system(op: u32) -> bool {
    (op & 0xffc00000) == 0xd5000000
}

/// Decode any word in the system space (0xd5....). Caller must only pass
/// words for which `is_system` is true (or an Enc counter holds).
pub fn decode(op: u32) -> Option<SystemOp> {
    if !is_system(op) {
        return None;
    }
    // CFINV (exact)
    if op == 0xd500401f {
        return Some(SystemOp::Cfinv);
    }
    // HINT space
    if (op & 0xfffff01f) == 0xd503201f {
        return Some(match op {
            0xd503205f => SystemOp::Wfe,
            0xd503207f => SystemOp::Wfi,
            _ => SystemOp::Hint,
        });
    }
    // CLREX accepts an architecturally ignored immediate in CRm.
    if op & 0xfffff0ff == 0xd503305f {
        return Some(SystemOp::Clrex);
    }
    // Barriers (CRn == 3, Rt == 31)
    if op & 0xfffff0ff == 0xd503309f {
        return Some(SystemOp::Barrier(Barrier::Dsb));
    }
    if op & 0xfffff0ff == 0xd50330bf {
        return Some(SystemOp::Barrier(Barrier::Dmb));
    }
    if op & 0xfffff0ff == 0xd50330df {
        return Some(SystemOp::Barrier(Barrier::Isb));
    }
    // MSR-imm / MRS / MSR(reg) / SYS / SYSL via op0f. op0f encodes bit21
    // (0 = write / 1 = read) combined with op0: op0 in {2,3} select whether
    // the register lives in the debug (0b10) or architectural (0b11) space.
    let op0f = (op >> 19) & 0x7;
    let key = ((op >> 5) & 0xffff) as u16;
    let rt = op & 31;
    match op0f {
        1 => Some(SystemOp::Sys {
            read: false,
            imm: key,
            rt,
        }),
        5 => Some(SystemOp::Sys {
            read: true,
            imm: key,
            rt,
        }),
        7 | 6 => Some(SystemOp::Mrs(rt, key)),
        3 => Some(SystemOp::Msr(rt, key)),
        2 => {
            // op0f=2 is an op0=2 write. This is the Apple PSTATE-immediate
            // variant when CRn is 1 or 4 and Rt == 31; otherwise it is a
            // write to an op0=2 (debug-space) system register such as
            // MDSCR_EL1.
            let crn = (op >> 12) & 0xf;
            if (crn == 4 || crn == 1) && rt == 31 {
                return Some(SystemOp::Pstate(match (op >> 5) & 0x7 {
                    6 => PstateOp::DaifSet(((op >> 8) & 0xf) as u8),
                    7 => PstateOp::DaifClr(((op >> 8) & 0xf) as u8),
                    5 => PstateOp::Spsel((op >> 8) & 1),
                    3 => PstateOp::Uao((op >> 8) & 1),
                    4 => PstateOp::Pan((op >> 8) & 1),
                    _ => return Some(SystemOp::Raw(op)),
                }));
            }
            Some(SystemOp::Msr(rt, key))
        }
        0 => {
            // MSR (immediate): CRn == 4 (architectural) or CRn == 1 (Apple variant), Rt == 31
            let crn = (op >> 12) & 0xf;
            if (crn != 4 && crn != 1) || rt != 31 {
                return Some(SystemOp::Raw(op));
            }
            Some(SystemOp::Pstate(match (op >> 5) & 0x7 {
                6 => PstateOp::DaifSet(((op >> 8) & 0xf) as u8),
                7 => PstateOp::DaifClr(((op >> 8) & 0xf) as u8),
                5 => PstateOp::Spsel((op >> 8) & 1),
                3 => PstateOp::Uao((op >> 8) & 1),
                4 => PstateOp::Pan((op >> 8) & 1),
                _ => return Some(SystemOp::Raw(op)),
            }))
        }
        _ => Some(SystemOp::Raw(op)),
    }
}

/// Map an MRS/MSR key to a known system register.
pub fn key(r: u16) -> Option<SysReg> {
    Some(match r {
        0xc080 => SysReg::SctlrEl1,
        0xe080 => SysReg::SctlrEl2,
        0xf080 => SysReg::SctlrEl3,
        0xc100 => SysReg::Ttbr0El1,
        0xc101 => SysReg::Ttbr1El1,
        0xc102 => SysReg::TcrEl1,
        0xc510 => SysReg::MairEl1,
        0xc3a0 => SysReg::ParEl1,
        0xc600 => SysReg::VbarEl1,
        0xc290 => SysReg::EsrEl1,
        0xc300 => SysReg::FarEl1,
        0xc201 => SysReg::ElrEl1,
        0xc200 => SysReg::SpsrEl1,
        0xe600 => SysReg::VbarEl2,
        0xe290 => SysReg::EsrEl2,
        0xe300 => SysReg::FarEl2,
        0xe304 => SysReg::HpfarEl2,
        0xe201 => SysReg::ElrEl2,
        0xe200 => SysReg::SpsrEl2,
        // HCR_EL2 is S3_4_C1_C0_2, which this decoder's key -- `(op >> 5) & 0xffff` of the
        // instruction -- makes 0xc0a2. The table used to map 0xe088 here instead, and 0xe088
        // is S3_4_C1_C1_0: a *different* register in the same op0=3/op1=4 opaque space.
        //
        // That is not a harmless typo. The op0=3, op1=4 group is the architectural opaque
        // (IMPDEF) space, which Apple uses for its own registers, and `HcrEl2` has
        // `min_el = EL2`. So the wrong key gave an Apple register a spurious EL2 privilege
        // floor: the checked-in Apple A10 SecureROMs (`SecureROM for t8101si,
        // iBoot-5281.0.0.100.{22,34,45}`) open by reading and writing exactly that
        // S3_4_C1_C1_0 pair -- it is the reset vector's first and fourth instruction -- and
        // under the old mapping the very first instruction trapped to EL2 with VBAR_EL2
        // still zero, killing the run on instruction 2. That is what made the A10 look like
        // an EL2 monitor image.
        //
        // It is not one. Across all three A10 builds the S3_4_C1_C1_* group appears exactly
        // four times, as two read/write pairs (op2=0 at the reset vector, op2=1 near file
        // offset 0x8bf8), and the images contain no canonical EL2 register access at all --
        // zero occurrences of MRS/MSR CPTR_EL2, VBAR_EL2 or the real HCR_EL2. An image that
        // configured stage 2 would program VBAR_EL2 and CPTR_EL2. The T8020 ROM, measured
        // in this project's records to be an EL1 image, has zero instructions in this group
        // and boots correctly.
        0xc0a2 => SysReg::HcrEl2,
        0xe108 => SysReg::VttbrEl2,
        0xe10a => SysReg::VtcrEl2,
        0x4112 => SysReg::CptrEl2,
        0xf600 => SysReg::VbarEl3,
        0xf290 => SysReg::EsrEl3,
        0xf300 => SysReg::FarEl3,
        0xf201 => SysReg::ElrEl3,
        0xf200 => SysReg::SpsrEl3,
        0xf088 => SysReg::ScrEl3,
        0x6112 => SysReg::CptrEl3,
        0xf08a => SysReg::CptrEl3,
        0xda11 => SysReg::Daif,
        0xde82 => SysReg::TpidrEl0,
        0xc684 => SysReg::TpidrEl1,
        0xde83 => SysReg::TpidrroEl0,
        0xc212 => SysReg::CurrentEl,
        0xc210 => SysReg::Spsel,
        0xda10 => SysReg::Nzcv,
        0xc000 => SysReg::MidrEl1,
        0xc005 => SysReg::MpidrEl1,
        0xc006 => SysReg::RevidrEl1,
        0xd801 => SysReg::CtrEl0,
        0xd807 => SysReg::DczidEl0,
        0xdf00 => SysReg::CntfrqEl0,
        0xdf02 => SysReg::CntvctEl0,
        0xdf01 => SysReg::CntpctEl0,
        0xdf03 => SysReg::CntpctssEl0,
        0xdf10 => SysReg::CntpTvalEl0,
        0xdf11 => SysReg::CntpCtlEl0,
        0xdf12 => SysReg::CntpCvalEl0,
        0xdf18 => SysReg::CntvTvalEl0,
        0xdf19 => SysReg::CntvCtlEl0,
        0xdf1a => SysReg::CntvCvalEl0,
        0xe703 => SysReg::CntvoffEl2,
        // CNTP_OFF_EL2: S3_4_C2_C3_0 -> key 0xE320 (op1=4, CRn=2, CRm=3, op2=0).
        0xe320 => SysReg::CntpoffEl2,
        0xc800 => SysReg::ClidrEl1,
        0xc801 => SysReg::CcsidrEl1,
        0xc020 => SysReg::Pfr0El1,
        0xc021 => SysReg::Pfr1El1,
        0xc038 => SysReg::Mmfr0El1,
        0xc039 => SysReg::Mmfr1El1,
        0xc030 => SysReg::Isar0El1,
        0xc031 => SysReg::Isar1El1,
        0xc208 => SysReg::SpEl0,
        0xe208 => SysReg::SpEl1,
        0xf208 => SysReg::SpEl2,
        0xda20 => SysReg::Fpcr,
        0xda21 => SysReg::Fpsr,
        // PMU (key = L:o0:op1:CRn:CRm:op2, 16 bits; L=1 in key for both MRS and MSR).
        // PMCR_EL0: S9_3_C9_C12_0 -> key 0xDCE0
        0xdce0 => SysReg::PmcrEl0,
        // PMCNTENSET_EL0: S9_3_C9_C12_1 -> key 0xDCE1
        0xdce1 => SysReg::PmcntensetEl0,
        // PMCNTENCLR_EL0: S9_3_C9_C12_2 -> key 0xDCE2
        0xdce2 => SysReg::PmcntenclrEl0,
        // PMCCNTR_EL0: S3_3_C9_C14_0 -> key 0xDCF0
        0xdcf0 => SysReg::PmccntrEl0,
        // SPE (Phase 190): S3_0_C9_C10_* profiling-buffer registers. Encodings
        // verified against clang/llvm-mc (`-mattr=+spe`): MRS PMBIDR_EL1 =
        // 0xd5389ae0, so key 0xC4D7; MRS PMBLIMITR_EL1 = 0xd5389a01 (0xC4D0),
        // PMBPTR_EL1 = 0xd5389a22 (0xC4D1), PMBSR_EL1 = 0xd5389a63 (0xC4D3).
        // Note: these live in the op0=2 (debug/SPE) space, unlike the AMU/PMU
        // op0=3 entries above.
        0xc4d0 => SysReg::PmblimitrEl1,
        0xc4d1 => SysReg::PmbptrEl1,
        0xc4d3 => SysReg::PmbsrEl1,
        0xc4d7 => SysReg::PmbidrEl1,
        // AMU (Phase 190, real A12-lineage map; old fabricated 0xDEB0/1/2 keys
        // removed). Key = L:o0:op1:CRn:CRm:op2; op1=3, CRn=13 base 0xDE80.
        // AMCR_EL0: S3_3_C13_C2_0 -> 0xDE90
        0xde90 => SysReg::AmcrEl0,
        // AMCFGR_EL0: S3_3_C13_C2_1 -> 0xDE91
        0xde91 => SysReg::AmcfgrEl0,
        // AMCGCR_EL0: S3_3_C13_C2_2 -> 0xDE92
        0xde92 => SysReg::AmcggcrEl0,
        // AMUSERENR_EL0: S3_3_C13_C2_3 -> 0xDE93
        0xde93 => SysReg::AmuserenEl0,
        // AMCNTENCLR0_EL0: S3_3_C13_C2_4 -> 0xDE94 (W1C; reads = enable flags)
        0xde94 => SysReg::AmcntenclrEl0,
        // AMCNTENSET0_EL0: S3_3_C13_C2_5 -> 0xDE95 (W1S)
        0xde95 => SysReg::AmcntensetEl0,
        // AMCNTENCLR1_EL0: S3_3_C13_C3_0 -> 0xDE98 (AMUv1.1 group 1)
        0xde98 => SysReg::Amcntenclr1El0,
        // AMCNTENSET1_EL0: S3_3_C13_C3_1 -> 0xDE99 (AMUv1.1 group 1)
        0xde99 => SysReg::Amcntenset1El0,
        // Apple IMPDEF S3_3_C15_C7_0 -> key 0xDFB8 (op0=3, op1=3, CRn=15, CRm=7, op2=0).
        // Defaulted to an A12-class L2 spec: 1 bank of 2^23 bytes (8 MiB), cap 1.
        0xdfb8 => SysReg::ImpCacheOp,
        // Apple IMPDEF S3_3_C15_C8_0 (key 0xDFC0): additional core/cluster control register read by XNU init.
        0xdfc0 => SysReg::ImpCacheOp,
        // Apple APCTL_EL1 (S3_*_C15_C0_4) SEP lock/handshake register. The key
        // omits the MRS/MSR L bit, so both forms resolve here: MRS
        // 0xd53cf080 and MSR 0xd51cf080. Both must reach the same register or
        // the guest's write-then-poll lock acquire never completes.
        0xe784 => SysReg::ApctlEl1,
        // AP*KeyLo_EL1 / Hi_EL1: S3_0_C2_C1_0..3 and S3_0_C2_C2_0..3
        // (verified against clang: MRS APIAKeyLo_EL1 = 0xd5382100).
        0xc108 => SysReg::ApiaLoEl1,
        0xc109 => SysReg::ApiaHiEl1,
        0xc10a => SysReg::ApibLoEl1,
        0xc10b => SysReg::ApibHiEl1,
        0xc110 => SysReg::ApdaLoEl1,
        0xc111 => SysReg::ApdaHiEl1,
        0xc112 => SysReg::ApdbLoEl1,
        0xc113 => SysReg::ApdbHiEl1,
        0xc118 => SysReg::ApgaLoEl1,
        0xc119 => SysReg::ApgaHiEl1,
        // MDSCR_EL1: op0=2 (debug space) op1=0 CRn=0 CRm=2 op2=2 -> 0x8012.
        // XNU reads it during early arm64e debug init.
        0x8012 => SysReg::MdscrEl1,
        // Phase 190 SVE ZCR_EL1 / ZCR_EL2: S3_0_C1_C2_0 and S3_4_C1_C2_0.
        // Verified word: MRS Xt, ZCR_EL1 = 0xd5321400 (key 0xC090);
        // ZCR_EL2 (op1=4) -> key 0xE090.
        0xc090 => SysReg::ZcrEl1,
        0xe090 => SysReg::ZcrEl2,
        // Indexed variants are not returned here; use key_indexed().
        _ => return None,
    })
}

/// Map an MRS/MSR key to an indexed register. The index is encoded in the
/// CRm / op2 fields per the Arm ARM (PMEVCNTR<n>, PMEVTYPER<n>, AMEVCNTR<n>).
pub fn key_indexed(r: u16) -> Option<SysReg> {
    // key layout: bit 15 = L, bit 14 = o0, bits 13:11 = op1, bits 10:7 = CRn,
    // bits 6:3 = CRm, bits 2:0 = op2
    let o0 = ((r >> 14) & 0x1) as u32;
    let op1 = ((r >> 11) & 0x7) as u32;
    let crn = ((r >> 7) & 0xf) as u32;
    let crm = ((r >> 3) & 0xf) as u32;
    let op2 = (r & 0x7) as u32;
    // op0 implied by o0: 2 if o0=0, 3 if o0=1.
    // PMEVCNTR<n>: op0=2 o0=0 op1=0 CRn=0xC CRm=n op2=2
    if o0 == 0 && op1 == 0 && crn == 0xc && op2 == 2 && crm < 0xF {
        return Some(SysReg::PmevcntrEl0(crm as usize));
    }
    // PMEVTYPER<n>: op0=2 o0=0 op1=0 CRn=0xD CRm=n op2=2
    if o0 == 0 && op1 == 0 && crn == 0xd && op2 == 2 && crm < 0xF {
        return Some(SysReg::PmevtyperEl0(crm as usize));
    }
    // AMU (Phase 190 real map; the single-counter AMEVCNTR0_EL0 is CRm=4).
    // AMEVCNTR00..03_EL0: op0=3 o0=1 op1=3 CRn=0xD CRm=C4 op2=0..3
    if o0 == 1 && op1 == 3 && crn == 0xd && crm == 4 && op2 < 4 {
        return Some(SysReg::AmevcntrEl0(op2 as u8));
    }
    // AMEVTYPER00..03_EL0: CRn=0xD CRm=C6 op2=0..3
    if o0 == 1 && op1 == 3 && crn == 0xd && crm == 6 && op2 < 4 {
        return Some(SysReg::AmevtyperEl0(op2 as u8));
    }
    // AMEVCNTR10..115_EL0 (AMUv1.1 group 1): CRn=0xD CRm=C12..C13 op2=0..7
    if o0 == 1 && op1 == 3 && crn == 0xd && (crm == 0xc || crm == 0xd) {
        return Some(SysReg::Amevcntr1El0(((crm - 0xc) * 8 + op2) as u8));
    }
    // AMEVTYPER10..115_EL0 (AMUv1.1 group 1): CRn=0xD CRm=C14..C15 op2=0..7
    if o0 == 1 && op1 == 3 && crn == 0xd && (crm == 0xe || crm == 0xf) {
        return Some(SysReg::Amevtyper1El0(((crm - 0xe) * 8 + op2) as u8));
    }
    None
}

/// Minimum exception level required to access a system register.
pub fn min_el(r: SysReg) -> u8 {
    use SysReg::*;
    match r {
        TpidrEl0 | TpidrroEl0 | CtrEl0 | DczidEl0 | CntfrqEl0 | CntvctEl0 | CntpctEl0
        | CntpctssEl0 | CntpTvalEl0 | CntpCtlEl0 | CntpCvalEl0 | CntvTvalEl0 | CntvCtlEl0
        | CntvCvalEl0 | CurrentEl | Nzcv => 0,
        Fpcr | Fpsr => 1,
        SctlrEl1 | Ttbr0El1 | Ttbr1El1 | TcrEl1 | MairEl1 | ParEl1 | VbarEl1 | EsrEl1 | FarEl1
        | ElrEl1 | SpsrEl1 | TpidrEl1 | MidrEl1 | MpidrEl1 | RevidrEl1 | ClidrEl1 | CcsidrEl1
        | Pfr0El1 | Pfr1El1 | Mmfr0El1 | Mmfr1El1 | Isar0El1 | Isar1El1 | SpEl0 | Spsel | Daif => 1,
        SctlrEl2 | VbarEl2 | EsrEl2 | FarEl2 | HpfarEl2 | ElrEl2 | SpsrEl2 | HcrEl2 | VttbrEl2
        | VtcrEl2 | CptrEl2 | SpEl1 | CntvoffEl2 | CntpoffEl2 => 2,
        SctlrEl3 | VbarEl3 | EsrEl3 | FarEl3 | ElrEl3 | SpsrEl3 | ScrEl3 | CptrEl3 | SpEl2 => 3,
        // Apple IMPDEF S3_3_C15_C7_0 cache/DRAM-spec probe is read by the
        // bootloaders (SecureROM mainapp at EL1) before the EL3 monitor is
        // engaged, so it must be EL1-accessible despite its S3_3 encoding.
        ImpCacheOp | ApctlEl1 => 1,
        ApiaLoEl1 | ApiaHiEl1 | ApibLoEl1 | ApibHiEl1 | ApdaLoEl1 | ApdaHiEl1 | ApdbLoEl1
        | ApdbHiEl1 | ApgaLoEl1 | ApgaHiEl1 => 1,
        MdscrEl1 => 1,
        PmcrEl0 | PmcntensetEl0 | PmcntenclrEl0 | PmccntrEl0 | PmevcntrEl0(_) | PmevtyperEl0(_) => {
            0
        }
        // AMU: config/enable/typer registers are EL1-or-higher; the counters
        // are EL0-readable only when AMUSERENR_EL0 permits (checked in the
        // MRS/MSR dispatch), so their min_el is 0 with a per-access gate.
        AmevcntrEl0(_) | Amevcntr1El0(_) => 0,
        AmcrEl0 | AmcfgrEl0 | AmcggcrEl0 | AmuserenEl0 | AmcntenclrEl0 | AmcntensetEl0
        | Amcntenclr1El0 | Amcntenset1El0 | AmevtyperEl0(_) | Amevtyper1El0(_) => 1,
        // SPE profiling-buffer registers are EL1-or-higher.
        PmbidrEl1 | PmblimitrEl1 | PmbptrEl1 | PmbsrEl1 => 1,
        // SVE vector-length control requires EL1.
        ZcrEl1 | ZcrEl2 => 1,
    }
}

/// Whether an MSR write is permitted at the current exception level.
pub fn can_write(r: SysReg, current_el: u8) -> bool {
    use SysReg::*;
    match r {
        CurrentEl | MidrEl1 | MpidrEl1 | RevidrEl1 | CtrEl0 | CntvctEl0 | CntpctEl0
        | CntpctssEl0 | ClidrEl1 | CcsidrEl1 | DczidEl0 | Pfr0El1 | Pfr1El1 | Mmfr0El1
        | Mmfr1El1 | Isar0El1 | Isar1El1 => false,
        CntfrqEl0 => current_el >= 3,
        TpidrroEl0 => current_el >= 1,
        PmccntrEl0 | PmevcntrEl0(_) => current_el >= 1,
        // AMU counters are architecturally read-only; the emulator keeps an
        // EL1-only "offset base" write hook (documented synthetic) so tests can
        // pin a deterministic cycle-derived counter. EL0 writes are UNDEFINED.
        AmevcntrEl0(_) | Amevcntr1El0(_) => current_el >= 1,
        // RO AMU/SPE id registers.
        AmcrEl0 | AmcfgrEl0 | AmcggcrEl0 | AmevtyperEl0(_) | Amevtyper1El0(_) | PmbidrEl1 => false,
        _ => current_el >= min_el(r),
    }
}

/// Feature/id register values so that MRS Id_* returns plausible values.
pub fn id_value(r: SysReg) -> u64 {
    use SysReg::*;
    match r {
        MidrEl1 => 0x0000000000000000,
        MpidrEl1 => 0x8000000000000000,
        RevidrEl1 => 0x0000000000000000,
        CtrEl0 => 0x00000000b3600064,
        ClidrEl1 => 0x0000000008000003,
        CcsidrEl1 => 0x0000000000000000,
        Pfr0El1 => 0x0000000000000031,
        Pfr1El1 => 0x0000000000000000,
        Mmfr0El1 => 0x0000000000000131,
        Mmfr1El1 => 0x0000000000000000,
        Isar0El1 => 0x0003100001113330,
        Isar1El1 => 0x0000000001111111,
        _ => 0x8000000000000000,
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdscr_el1_reads_decode_and_resolve() {
        // MRS X12, MDSCR_EL1 = 0xd530024c (op0=2 debug space, bit21=1 read).
        let op = 0xd530024cu32;
        let decoded = decode(op);
        assert!(matches!(decoded, Some(SystemOp::Mrs(12, 0x8012))));
        let SystemOp::Mrs(rt, k) = decoded.unwrap() else {
            unreachable!()
        };
        assert_eq!(rt, 12);
        assert_eq!(key(k).unwrap(), SysReg::MdscrEl1);
        assert_eq!(min_el(SysReg::MdscrEl1), 1);
        assert!(can_write(SysReg::MdscrEl1, 1));

        // MSR MDSCR_EL1, X1 decodes as an op0=2 write and resolves too.
        let write = 0xd5100241u32;
        assert!(matches!(decode(write), Some(SystemOp::Msr(1, 0x8012))));

        // PSTATE-immediate behavior is untouched: an MSR-imm word stays out of
        // the Msr/Mrs paths (MSR DAIFSet #0 keeps CRn=4 Rt=31; Raw is fine).
        assert!(!matches!(
            decode(0xd500401f),
            Some(SystemOp::Msr(..)) | Some(SystemOp::Mrs(..))
        ));
    }

    /// The op0=3, op1=4 group is the architectural *opaque* (IMPDEF) space, which Apple
    /// uses for its own registers, so a key in it can be an Apple register that is
    /// architecturally unprivileged. This pins the collision that made the Apple A10
    /// SecureROM look like an EL2 monitor image.
    ///
    /// `0xe088` is S3_4_C1_C1_0 and the A10 reset vector's first instruction is
    /// `MRS x2, S3_4_C1_C1_0` (`0xd53c1102`). When this key resolved to `HcrEl2`, whose
    /// `min_el` is EL2, the A10 took an UNDEFINED-instruction exception on its very first
    /// instruction and died at VBAR_EL2 + 0x400 -- which read as a property of the part
    /// rather than a mis-mapped register. With the key correct, the same image is an EL1h
    /// image and runs indefinitely.
    ///
    /// The real HCR_EL2 is S3_4_C1_C0_2, so this asserts both halves: the Apple register is
    /// no longer HCR_EL2, and HCR_EL2 is still reachable at its architectural encoding so
    /// the model has not simply dropped it.
    #[test]
    fn apple_opaque_system_register_is_not_mapped_onto_hcr_el2() {
        // The A10 reset vector's first instruction, and its key.
        let a10_first = 0xd53c1102u32;
        assert_eq!(
            decode(a10_first),
            Some(SystemOp::Mrs(2, 0xe088)),
            "A10 reset vector no longer decodes as expected"
        );
        assert_eq!(
            key(0xe088),
            None,
            "S3_4_C1_C1_0 is an Apple opaque register; mapping it to HCR_EL2 gave it an \
             EL2 privilege floor and killed the A10 SecureROM on instruction 1"
        );

        // HCR_EL2 keeps its architectural encoding S3_4_C1_C0_2 -> key 0xc0a2. Both forms
        // resolve to the same SysReg, so a guest that reads and then programs it still
        // reaches the register rather than falling through to a zeroed opaque read.
        //   MRS x2, HCR_EL2 = 0xd5381442 (L=1, op0f=7)
        //   MSR HCR_EL2, x2 = 0xd5181442 (L=0, op0f=3)
        assert_eq!(decode(0xd5381442), Some(SystemOp::Mrs(2, 0xc0a2)));
        assert_eq!(decode(0xd5181442), Some(SystemOp::Msr(2, 0xc0a2)));
    }

    /// A10/EL2 sanity control for the test above: the T8020 SecureROM, measured in this
    /// project's records to be an EL1 image, contains no instruction in the S3_4_C1_C1_*
    /// group at all. If a future change made that key an EL2 register again, the A10 would
    /// trap at reset while the T8020 kept booting -- so the asymmetry is what the previous
    /// test is really about, and it is cheap to state.
    #[test]
    fn t8020_secure_rom_never_touches_the_a10_opaque_register_key() {
        // `iphem-core` cannot depend on `iphem-soc` -- the dependency runs the other
        // way -- so this cannot use `iphem_core::corpus_rom_path` and accepts either
        // spelling of the corpus name itself. The corpus is half-renamed
        // (`SecureROM_t7000si_iBoot-...` but `SecureROM for s5l8960xsi, iBoot-...`),
        // so a single hard-coded name is a coin toss.
        let mut bytes = None;
        for candidate in [
            "SecureROM/SecureROM_t8020si_iBoot-3865.0.0.4.7.bin",
            "SecureROM for t8020si, iBoot-3865.0.0.4.7",
        ] {
            for dir in ["../", "../../"] {
                if let Ok(found) = std::fs::read(format!("{dir}{candidate}")) {
                    bytes = Some(found);
                    break;
                }
            }
            if bytes.is_some() {
                break;
            }
        }
        let Some(bytes) = bytes else {
            return; // corpus is optional
        };
        let mut hits = 0;
        let (words, _) = bytes.as_chunks::<4>();
        for chunk in words.iter() {
            let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if (w & 0xffc0_0000) == 0xd500_0000
                && matches!((w >> 19) & 7, 3 | 6 | 7)
                && ((w >> 16) & 7) == 4
                && ((w >> 12) & 0xf) == 1
                && ((w >> 8) & 0xf) == 1
            {
                hits += 1;
            }
        }
        assert_eq!(
            hits, 0,
            "T8020 ROM unexpectedly touches S3_4_C1_C1_*; the A10 and T8020 images would \
             then disagree about the entry EL, which is the premise this test rests on"
        );
    }
}
