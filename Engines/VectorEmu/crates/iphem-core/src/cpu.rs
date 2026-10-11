#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExceptionLevel {
    El0 = 0,
    El1 = 1,
    El2 = 2,
    El3 = 3,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum CpuRunState {
    #[default]
    Running,
    WaitingForInterrupt,
    Halted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuException {
    Undefined,
    Synchronous { esr: u64, far: u64 },
    Interrupt,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CpuState {
    pub x: [u64; 31],
    pub sp: u64,
    pub pc: u64,
    pub pstate: u64,
    pub el: ExceptionLevel,
    pub ttbr0_el1: u64,
    pub ttbr1_el1: u64,
    pub tcr_el1: u64,
    pub mair_el1: u64,
    pub par_el1: u64,
    pub sctlr_el1: u64,
    pub sctlr_el2: u64,
    pub sctlr_el3: u64,
    pub vbar_el1: u64,
    pub far_el1: u64,
    pub esr_el1: u64,
    pub spsr_el1: u64,
    pub elr_el1: u64,
    pub vbar_el2: u64,
    pub far_el2: u64,
    pub hpfar_el2: u64,
    pub esr_el2: u64,
    pub spsr_el2: u64,
    pub elr_el2: u64,
    pub vbar_el3: u64,
    pub far_el3: u64,
    pub esr_el3: u64,
    pub spsr_el3: u64,
    pub elr_el3: u64,
    pub nzcv: u32,
    pub tpidr_el0: u64,
    pub tpidr_el1: u64,
    pub tpidrro_el0: u64,
    /// Generic timer compare/control state. Counters share the machine scheduler.
    pub cntp_cval_el0: u64,
    pub cntp_ctl_el0: u64,
    pub cntv_cval_el0: u64,
    pub cntv_ctl_el0: u64,
    pub cntvoff_el2: u64,
    pub cntpoff_el2: u64,
    pub daif: u8,
    pub currentel: u8,
    /// False selects SP_EL0; true selects SP_ELx for the current exception level.
    pub spsel: bool,
    pub sp_el: [u64; 4],
    pub vtcr_el2: u64,
    pub vttbr_el2: u64,
    pub hcr_el2: u64,
    pub scr_el3: u64,
    pub cptr_el2: u64,
    pub cptr_el3: u64,
    pub vendor_sysregs: std::collections::HashMap<u16, u64>,
    pub v: [u128; 32],
    pub fpcr: u64,
    pub fpsr: u64,
    // Phase 163: PMU + AMU state
    pub pmev_cntr: [u64; 16],
    pub pmev_type: [u64; 16],
    pub pmcr: u64,
    pub pmcntenset: u64,
    pub pmcntenclr: u64,
    pub amcnt: [u64; 16],
    pub amuseren: u64,
    pub amcntenset: u64,
    pub amcntenclr: u64,
    // Phase 190: AMUv1.1 group-1 (counters 4..19) enable flags.
    pub amcntenset1: u64,
    // Phase 190 SPE (Armv8.2 statistical profiling) profiling-buffer state.
    // RAM-backed: the software driver points PMBLIMITR/PMBPTR into guest RAM;
    // `Machine::spe_buf_push` walks the pointer, advances it, and sets
    // status bits (S/DL/EC) exactly like profiling hardware completing a record.
    pub spe_pmbidr: u64,
    pub spe_pmblimitr: u64,
    pub spe_pmbptr: u64,
    pub spe_pmbsr: u64,
    // Apple IMPDEF S3_3_C15_C7_0 (cache/DRAM-spec probe). Default matches an
    // A12-class L2: 1 bank, 2^23 bytes (8 MiB), branch-cap 1.
    pub imp_cache_op: u64,
    // Apple IMPDEF S3_4_C15_C0_4 (`APCTL_EL1`). SEPOS uses this as its
    // cross-engine lock/handshake register: it writes a nonzero token, then
    // spins on bit 0 (`msr ...,#5` / `mrs` / `tbnz w0,#1`). Bit 0 is the
    // grant/ready ack the SEP co-processor raises, so the read side presents
    // the grant for any nonzero request or the guest never leaves the spin.
    // The written token is otherwise stored verbatim, so the register keeps a
    // plain round-trip contract. Reset is 0x42: bit 1 is the capability bit
    // XNU's early bring-up masks for, and no lock request is outstanding.
    pub apctl_el1: u64,
    // Phase 190 SVE (Armv8.2+) architectural state: predicate registers p0-p15
    // (VL bits each, stored LSB-first in u128), the first-fault register FFR,
    // and the SVE vector-length control regs. Z0-Z31 overlap the Neon V
    // registers (`v`). VL derives from ZCR_EL1.VQ (0 => 128 bits).
    pub p: [u128; 16],
    pub ffr: u128,
    pub zcr_el1: u64,
    pub zcr_el2: u64,
    // ARMv8.3 pointer-authentication key material (AP*KeyLo_EL1 / Hi_EL1).
    // The keys are 128-bit split into a Lo (ROCR/Table 0) and Hi half stored
    // here; `pac_*` helpers fold them into a deterministic signature.
    pub apia_lo: u64,
    pub apia_hi: u64,
    pub apib_lo: u64,
    pub apib_hi: u64,
    pub apda_lo: u64,
    pub apda_hi: u64,
    pub apdb_lo: u64,
    pub apdb_hi: u64,
    // ARMv7-A AArch32 processor state for the Apple A7 SEP core. Banked
    // registers are indexed by mode: usr=0, fiq=1, irq=2, svc=3, abt=4,
    // und=5 (usr and sys share bank 0). `cpsr`/`pc` are the live A32 CSRs;
    // the A64 core never touches this field.
    pub a32: Aarch32State,
}
/// ARMv7-A AArch32 banked processor state used by the A7 SEP core.
///
/// Modes (CPSR.M) map to bank indices: usr/sys=0, fiq=1, irq=2, svc=3,
/// abt=4, und=5. `spsr[1..=5]` cover the five exception modes in the same
/// order. `r8_12_fiq` is the FIQ-private r8-r12 bank; all other modes use
/// r0-r12 directly from `CpuState::x`.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Aarch32State {
    pub cpsr: u32,
    pub spsr: [u32; 5],
    pub r8_12_fiq: [u32; 5],
    pub sp: [u32; 6],
    pub lr: [u32; 6],
    /// CP15 CR14 reset-signature pair, read by `mrrc p15, #0, Rd, Rn, c14` in
    /// the SEPROM reset sequence (`0x7c` in the T8020 SEPROM image). The value
    /// is set by the SoC reset fusing; without hardware measurement the model
    /// default is reset zero, so no signature is invented. `mrrc` maps the low
    /// word to the first syntax register (Rt) and the high word to the second
    /// (Rt2); `mcrr` builds the pair in the same order.
    pub cr14: u64,
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            x: [0; 31],
            sp: 0x19c20000,
            pc: 0,
            pstate: 0,
            el: ExceptionLevel::El1,
            ttbr0_el1: 0,
            ttbr1_el1: 0,
            tcr_el1: 0,
            mair_el1: 0,
            par_el1: 0,
            sctlr_el1: 0,
            sctlr_el2: 0,
            sctlr_el3: 0,
            vbar_el1: 0,
            far_el1: 0,
            esr_el1: 0,
            spsr_el1: 0,
            elr_el1: 0,
            vbar_el2: 0,
            far_el2: 0,
            hpfar_el2: 0,
            esr_el2: 0,
            spsr_el2: 0,
            elr_el2: 0,
            vbar_el3: 0,
            far_el3: 0,
            esr_el3: 0,
            spsr_el3: 0,
            elr_el3: 0,
            nzcv: 0,
            tpidr_el0: 0,
            tpidr_el1: 0,
            tpidrro_el0: 0,
            cntp_cval_el0: 0,
            cntp_ctl_el0: 0,
            cntv_cval_el0: 0,
            cntv_ctl_el0: 0,
            cntvoff_el2: 0,
            cntpoff_el2: 0,
            daif: 0,
            currentel: 1,
            spsel: true,
            sp_el: [0x19c20000; 4],
            vtcr_el2: 0,
            vttbr_el2: 0,
            hcr_el2: 0,
            scr_el3: 0,
            cptr_el2: 0,
            cptr_el3: 0,
            vendor_sysregs: std::collections::HashMap::new(),
            v: [0u128; 32],
            fpcr: 0,
            fpsr: 0,
            pmev_cntr: [0; 16],
            pmev_type: [0; 16],
            pmcr: 0,
            pmcntenset: 0,
            pmcntenclr: 0,
            amcnt: [0; 16],
            amuseren: 0,
            amcntenset: 0,
            amcntenclr: 0,
            amcntenset1: 0,
            spe_pmbidr: 0x1,
            spe_pmblimitr: 0,
            spe_pmbptr: 0,
            spe_pmbsr: 0,
            imp_cache_op: 0x1_1701,
            // Reset presents 0x42: bit 1 is the capability bit XNU's early
            // bring-up probe masks for, bit 6 is the high capability bit that
            // must be nonzero at default, and bit 0 (the lock grant) is clear
            // because no request is outstanding.
            apctl_el1: 0x42,
            p: [0u128; 16],
            ffr: 0,
            zcr_el1: 0,
            zcr_el2: 0,
            // Deterministic default PAC keys (nonzero so pre-configured
            // firmware that never programs APKey still round-trips).
            apia_lo: 0x8712_3456_789a_bcd0,
            apia_hi: 0x0123_4567_89ab_cdef,
            apib_lo: 0x2414_50c2_4d5c_2b8a,
            apib_hi: 0x1d5d_5c0b_f2a3_9b91,
            apda_lo: 0xe703_7ed1_a0b4_28db,
            apda_hi: 0xcecc_e7c7_bb0a_4453,
            apdb_lo: 0xde72_1c9d_9f3c_482e,
            apdb_hi: 0x206b_3c5b_3a9c_7e29,
            a32: Aarch32State::default(),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Cpu {
    pub state: CpuState,
    pub run_state: CpuRunState,
    pub pending_irq: Option<u32>,
    pub pending_fiq: Option<u32>,
    pub pending_serror: Option<u64>,
    pub instructions: u64,
    pub exclusive_address: Option<u64>,
    pub exclusive_size: usize,
}
impl Cpu {
    pub fn new() -> Self {
        Self {
            state: CpuState::default(),
            run_state: CpuRunState::Running,
            pending_irq: None,
            pending_fiq: None,
            pending_serror: None,
            instructions: 0,
            exclusive_address: None,
            exclusive_size: 0,
        }
    }
    pub fn halted(&self) -> bool {
        matches!(self.run_state, CpuRunState::Halted)
    }
    pub fn set_pc(&mut self, pc: u64) {
        self.state.pc = pc;
    }
    pub fn set_reg(&mut self, reg: usize, val: u64) {
        if reg < 31 {
            self.state.x[reg] = val;
        }
    }
}
