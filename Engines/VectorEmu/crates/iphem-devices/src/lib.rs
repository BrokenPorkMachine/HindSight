pub use iphem_core::device::DeviceResult;
use iphem_core::device::{Device, DeviceEvent, DeviceId};
use std::any::Any;
use std::collections::HashMap;
use std::time::Instant;

pub mod apple_gpio;
pub mod apple_mbox;
pub mod device_table_control;
pub use apple_mbox::AppleAscMailbox;
pub use device_table_control::{DeviceTableControlWindow, TargetActualRegister};
pub mod sochot;
pub use sochot::SocHot;
pub mod mcc;
pub mod pmgr_power;
pub use mcc::AppleMcc;
pub use pmgr_power::{PmgrBootEvent, PmgrBootStatus, PmgrPowerState};
pub mod dwc2;
pub use dwc2::Dwc2Core;
pub mod uart_output;
pub use apple_gpio::AppleGpio;
pub mod dart;
pub use dart::{
    dart_t8020_ttbr_root_address, DartAccess, DartDmaPort, DartMmio, DartT8020, DartT8020Walker,
    DartWalkFault,
};

// AArch64 Generic Timer registers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum TimerReg {
    CNTVCT_EL0 = 0x0,     // Virtual counter
    CNTV_CVAL_EL0 = 0x8,  // Virtual timer compare value
    CNTV_CTL_EL0 = 0x10,  // Virtual timer control
    CNTPCT_EL0 = 0x18,    // Physical counter
    CNTP_CVAL_EL0 = 0x20, // Physical timer compare value
    CNTP_CTL_EL0 = 0x28,  // Physical timer control
    CNTV_TVAL_EL0 = 0x30, // Virtual timer value
    CNTP_TVAL_EL0 = 0x38, // Physical timer value
    CNTFRQ_EL0 = 0x40,    // Counter frequency
    CNTVOFF_EL2 = 0x48,   // Virtual offset
}

impl TryFrom<u64> for TimerReg {
    type Error = ();
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0x0 => Ok(TimerReg::CNTVCT_EL0),
            0x8 => Ok(TimerReg::CNTV_CVAL_EL0),
            0x10 => Ok(TimerReg::CNTV_CTL_EL0),
            0x18 => Ok(TimerReg::CNTPCT_EL0),
            0x20 => Ok(TimerReg::CNTP_CVAL_EL0),
            0x28 => Ok(TimerReg::CNTP_CTL_EL0),
            0x30 => Ok(TimerReg::CNTV_TVAL_EL0),
            0x38 => Ok(TimerReg::CNTP_TVAL_EL0),
            0x40 => Ok(TimerReg::CNTFRQ_EL0),
            0x48 => Ok(TimerReg::CNTVOFF_EL2),
            _ => Err(()),
        }
    }
}

pub struct Timer {
    pub id: DeviceId,
    pub irq: u32,
    pub counter: u64,
    pub v_compare: u64,
    pub p_compare: u64,
    pub v_ctl: u32,
    pub p_ctl: u32,
    pub v_tval: i32,
    pub p_tval: i32,
    pub freq: u32,
    pub v_offset: i64,
    pub enabled: bool,
    pub last_update: Instant,
    v_signaled: bool,
    p_signaled: bool,
}

impl Timer {
    pub fn new(id: DeviceId, irq: u32) -> Self {
        Self {
            id,
            irq,
            counter: 0,
            v_compare: u64::MAX,
            p_compare: u64::MAX,
            v_ctl: 0,
            p_ctl: 0,
            v_tval: 0,
            p_tval: 0,
            freq: 24_000_000, // 24MHz default
            v_offset: 0,
            enabled: false,
            last_update: Instant::now(),
            v_signaled: false,
            p_signaled: false,
        }
    }

    fn update_counter(&mut self, cycles: u64) {
        self.counter = self.counter.wrapping_add(cycles);
    }

    fn virtual_counter(&self) -> u64 {
        self.counter.wrapping_sub(self.v_offset as u64)
    }

    fn check_timer(ctl: u32, condition: bool, signaled: &mut bool) -> bool {
        let deliverable = ctl & 1 != 0 && ctl & 2 == 0 && condition;
        if !deliverable {
            *signaled = false;
            return false;
        }
        let transition = !*signaled;
        *signaled = true;
        transition
    }

    fn check_v_timer(&mut self) -> bool {
        let condition = self.virtual_counter() >= self.v_compare;
        Self::check_timer(self.v_ctl, condition, &mut self.v_signaled)
    }

    fn check_p_timer(&mut self) -> bool {
        let condition = self.counter >= self.p_compare;
        Self::check_timer(self.p_ctl, condition, &mut self.p_signaled)
    }

    fn read_reg(&self, reg: TimerReg) -> u64 {
        match reg {
            TimerReg::CNTVCT_EL0 => self.virtual_counter(),
            TimerReg::CNTPCT_EL0 => self.counter,
            TimerReg::CNTV_CVAL_EL0 => self.v_compare,
            TimerReg::CNTP_CVAL_EL0 => self.p_compare,
            TimerReg::CNTV_CTL_EL0 => {
                u64::from(self.v_ctl | (u32::from(self.virtual_counter() >= self.v_compare) << 2))
            }
            TimerReg::CNTP_CTL_EL0 => {
                u64::from(self.p_ctl | (u32::from(self.counter >= self.p_compare) << 2))
            }
            TimerReg::CNTV_TVAL_EL0 => {
                u64::from(self.v_compare.wrapping_sub(self.virtual_counter()) as u32)
            }
            TimerReg::CNTP_TVAL_EL0 => u64::from(self.p_compare.wrapping_sub(self.counter) as u32),
            TimerReg::CNTFRQ_EL0 => self.freq as u64,
            TimerReg::CNTVOFF_EL2 => self.v_offset as u64,
        }
    }

    fn write_reg(&mut self, reg: TimerReg, value: u64) {
        match reg {
            TimerReg::CNTV_CVAL_EL0 => {
                self.v_compare = value;
                self.v_signaled = false;
            }
            TimerReg::CNTP_CVAL_EL0 => {
                self.p_compare = value;
                self.p_signaled = false;
            }
            TimerReg::CNTV_CTL_EL0 => {
                self.v_ctl = value as u32 & 0b11;
                self.v_signaled = false;
            }
            TimerReg::CNTP_CTL_EL0 => {
                self.p_ctl = value as u32 & 0b11;
                self.p_signaled = false;
            }
            TimerReg::CNTV_TVAL_EL0 => {
                self.v_tval = value as i32;
                self.v_compare = self
                    .virtual_counter()
                    .wrapping_add((value as u32 as i32 as i64) as u64);
                self.v_signaled = false;
            }
            TimerReg::CNTP_TVAL_EL0 => {
                self.p_tval = value as i32;
                self.p_compare = self
                    .counter
                    .wrapping_add((value as u32 as i32 as i64) as u64);
                self.p_signaled = false;
            }
            TimerReg::CNTFRQ_EL0 => self.freq = value as u32,
            TimerReg::CNTVOFF_EL2 => {
                self.v_offset = value as i64;
                self.v_signaled = false;
            }
            _ => {}
        }
    }
}

impl Device for Timer {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "timer"
    }
    fn reset(&mut self) {
        self.counter = 0;
        self.v_compare = u64::MAX;
        self.p_compare = u64::MAX;
        self.v_ctl = 0;
        self.p_ctl = 0;
        self.v_tval = 0;
        self.p_tval = 0;
        self.freq = 24_000_000;
        self.v_offset = 0;
        self.enabled = false;
        self.v_signaled = false;
        self.p_signaled = false;
        self.last_update = Instant::now();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let reg = match TimerReg::try_from(o) {
            Ok(r) => r,
            Err(_) => return DeviceResult::Fault,
        };
        let val = self.read_reg(reg);
        DeviceResult::Value(val)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let reg = match TimerReg::try_from(o) {
            Ok(r) => r,
            Err(_) => return DeviceResult::Fault,
        };
        self.write_reg(reg, v);
        DeviceResult::Ok
    }
    fn tick(&mut self, c: u64) -> Option<DeviceEvent> {
        self.update_counter(c);

        // Check virtual timer
        if self.check_v_timer() {
            return Some(DeviceEvent::Interrupt(self.irq));
        }
        // Check physical timer
        if self.check_p_timer() {
            return Some(DeviceEvent::Interrupt(self.irq));
        }
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// GICv3 Interrupt Groups
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptGroup {
    Group0 = 0,   // Secure EL1/EL3
    Group1 = 1,   // Non-secure EL1/EL2
    Group1NS = 2, // Non-secure EL1 (NS)
}

// GICv3 Redistributor Affinity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MpAffinity {
    pub aff0: u8,
    pub aff1: u8,
    pub aff2: u8,
    pub aff3: u8,
}

impl MpAffinity {
    pub fn new(aff0: u8, aff1: u8, aff2: u8, aff3: u8) -> Self {
        Self {
            aff0,
            aff1,
            aff2,
            aff3,
        }
    }
    pub fn from_mpidr(mpidr: u64) -> Self {
        Self {
            aff0: (mpidr & 0xFF) as u8,
            aff1: ((mpidr >> 8) & 0xFF) as u8,
            aff2: ((mpidr >> 16) & 0xFF) as u8,
            aff3: ((mpidr >> 32) & 0xFF) as u8,
        }
    }
    pub fn to_mpidr(&self) -> u64 {
        (self.aff0 as u64)
            | ((self.aff1 as u64) << 8)
            | ((self.aff2 as u64) << 16)
            | ((self.aff3 as u64) << 32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptType {
    SPI = 0, // Shared Peripheral Interrupt
    PPI = 1, // Private Peripheral Interrupt
    SGI = 2, // Software Generated Interrupt
    LPI = 3, // Locality-specific Peripheral Interrupt
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IrqPriority(pub u8);

impl IrqPriority {
    pub const HIGHEST: Self = Self(0);
    pub const HIGH: Self = Self(32);
    pub const NORMAL: Self = Self(64);
    pub const LOW: Self = Self(128);
    pub const LOWEST: Self = Self(255);
}

#[derive(Debug, Clone)]
pub struct AicIrq {
    pub id: u32,
    pub int_type: InterruptType,
    pub priority: IrqPriority,
    pub group: InterruptGroup,
    pub enabled: bool,
    pub pending: bool,
    pub active: bool,
    pub target_affinity: Option<MpAffinity>, // For PPI/SGI/LPI routing
    pub config: u8,                          // Edge/level, secure/non-secure
}

impl AicIrq {
    pub fn new_spi(id: u32) -> Self {
        Self {
            id,
            int_type: InterruptType::SPI,
            priority: IrqPriority::NORMAL,
            group: InterruptGroup::Group1NS,
            enabled: false,
            pending: false,
            active: false,
            target_affinity: None,
            config: 0,
        }
    }

    pub fn new_ppi(id: u32, affinity: MpAffinity) -> Self {
        Self {
            id,
            int_type: InterruptType::PPI,
            priority: IrqPriority::NORMAL,
            group: InterruptGroup::Group1NS,
            enabled: false,
            pending: false,
            active: false,
            target_affinity: Some(affinity),
            config: 0,
        }
    }

    pub fn new_sgi(id: u32, affinity: MpAffinity) -> Self {
        Self {
            id,
            int_type: InterruptType::SGI,
            priority: IrqPriority::NORMAL,
            group: InterruptGroup::Group1NS,
            enabled: false,
            pending: false,
            active: false,
            target_affinity: Some(affinity),
            config: 0,
        }
    }

    pub fn new_lpi(id: u32) -> Self {
        Self {
            id,
            int_type: InterruptType::LPI,
            priority: IrqPriority::NORMAL,
            group: InterruptGroup::Group1NS,
            enabled: false,
            pending: false,
            active: false,
            target_affinity: None,
            config: 0,
        }
    }
}

pub struct Aic {
    pub id: DeviceId,
    pub irqs: Vec<AicIrq>,
    pub current_priority: Option<IrqPriority>,
    /// GICC (CPU interface) per-core state: priority mask, binary point,
    /// running-priority stack, and the currently-serviced IRQ. This is the
    /// cycle-accurate priority/preemption model — Phase 207.
    pub gicc: Vec<GiccState>,
    // GICv3 Distributor registers
    pub gicd_ctlr: u32,
    pub gicd_typer: u32,
    pub gicd_iidr: u32,
    // Redistributor
    pub gicr_ctlr: u32,
    pub gicr_typer: u64,
    pub gicr_iidr: u32,
    pub gicr_waker: u32,
    pub gicr_pidr: u32,
    // ITS (Interrupt Translation Service)
    pub its_ctlr: u32,
    pub its_iidr: u32,
    pub its_typer: u32,
    pub its_baser: [u64; 8],
    pub its_cbaser: u64,
    pub its_cwriter: u64,
    pub its_creader: u64,
    pub its_translator_event: u64,
    pub its_translations: HashMap<u64, u32>,
    // AIC2 per-IRQ target config window at 0x2000 (4 bytes per IRQ).
    // Bits [3:0] hold the target CPU the SecureROM's device table is built
    // from; reset is all-zero (no IRQ targeted). max_irq = 2048 leaves the
    // window at 0x2000..0x4000.
    pub aic2_irq_cfg: Vec<u32>,
    // AIC2 enable/mask bitmaps (64 words each, 2048 bits). SW_SET/SW_CLR
    // drive software-set pending; MASK_SET/MASK_CLR drive the enabled state;
    // HW_STATE reflects the raw line level of a die group.
    pub aic2_sw_set: [u32; AIC2_IRQ_BITMAP_WORDS],
    pub aic2_sw_clr: [u32; AIC2_IRQ_BITMAP_WORDS],
    pub aic2_mask_set: [u32; AIC2_IRQ_BITMAP_WORDS],
    pub aic2_mask_clr: [u32; AIC2_IRQ_BITMAP_WORDS],
    pub aic2_hw_state: [u32; AIC2_IRQ_BITMAP_WORDS],
    /// Per-core physical/virtual architectural timer FIQ input levels.
    /// AIC exposes these as FIQ sources rather than GIC PPIs.
    pub timer_fiq_lines: Vec<[bool; 2]>,
    // Per-core redistributors
    pub redistributors: Vec<Redistributor>,
    // LPI support
    pub lpi_pending_table: Option<Vec<u64>>,
    pub lpi_config_table: Option<Vec<u64>>,
}

// AIC2's per-die register banks follow the variable-sized IRQ_CFG array.
// The window is `0x2000 + num_irqs * 4`. Each following bitmap has 64 32-bit
// words (0x100 bytes), as specified by the AICv2 layout, so the banks are at
// 0x4000 (SW_SET), 0x4100 (SW_CLR), 0x4200 (MASK_SET), 0x4300 (MASK_CLR) and
// 0x4400 (HW_STATE).
const AIC2_IRQ_CFG_START: u64 = 0x2000;
const AIC2_IRQ_CFG_END: u64 = 0x2ffc;
/// T8020 per-IRQ mask window: one 32-bit enable word per IRQ, observed at
/// `0x23b103000 + irq*4` in the ROM's own enable sequence.
const AIC2_IRQ_MASK_START: u64 = 0x3000;
const AIC2_IRQ_MASK_END: u64 = 0x3ffc;
const AIC2_IRQ_BITMAP_WORDS: usize = 64;
const AIC2_RESET_OFFSET: u64 = 0x0010;
const AIC2_RESET_BIT: u32 = 1;
const AIC2_SW_SET_START: u64 = 0x4000;
const AIC2_SW_CLR_START: u64 = 0x4100;
const AIC2_MASK_SET_START: u64 = 0x4200;
const AIC2_MASK_CLR_START: u64 = 0x4300;
const AIC2_HW_STATE_START: u64 = 0x4400;

fn aic2_bitmap_word(offset: u64, start: u64) -> usize {
    ((offset - start) / 4) as usize
}

#[derive(Debug, Clone)]
pub struct Redistributor {
    pub affinity: MpAffinity,
    pub ctlr: u32,
    pub typer: u64,
    pub ppi: Vec<AicIrq>, // PPIs for this redistributor
    pub sgi: Vec<AicIrq>, // SGIs for this redistributor
}

/// Per-core GICC (CPU interface) state for the cycle-accurate priority and
/// preemption model. Lower `priority` values win; `IrqPriority::LOWEST.0` is
/// the lowest. The running-priority stack lets nested ISRs restore on EOI.
#[derive(Debug, Clone)]
pub struct GiccState {
    pub core_id: usize,
    /// GICC_PMR: priority mask — only IRQs with priority < pmr are eligible.
    pub pmr: u8,
    /// GICC_BPR: binary point — masks bits below this value.
    pub bpr: u8,
    /// Running-priority stack (nested ISRs). Pushed on ACK, popped on EOI.
    pub running_stack: Vec<IrqPriority>,
    /// The IRQ currently being serviced (None when idle).
    pub running_irq: Option<u32>,
    /// Cycle counter at the moment the current IRQ was acknowledged.
    pub ack_cycle: u64,
    /// GICC_IAR: last acknowledged IRQ id (0 = spurious when no IRQ).
    pub iar: u32,
}

impl GiccState {
    pub fn new(core_id: usize) -> Self {
        Self {
            core_id,
            pmr: 0xff,
            bpr: 0,
            running_stack: Vec::new(),
            running_irq: None,
            ack_cycle: 0,
            iar: 0x3ff, // spurious
        }
    }

    /// Effective running priority: top of the stack, or LOWEST when idle.
    pub fn running_priority(&self) -> IrqPriority {
        self.running_stack
            .last()
            .copied()
            .unwrap_or(IrqPriority::LOWEST)
    }

    /// Whether `new_irq` can preempt the currently-serviced IRQ.
    pub fn can_preempt(&self, new_irq: IrqPriority) -> bool {
        new_irq < self.running_priority()
    }
}

impl Aic {
    pub fn new(id: DeviceId, num_irqs: usize) -> Self {
        let mut irqs = Vec::with_capacity(num_irqs);
        // Create SPIs (Shared Peripheral Interrupts)
        for i in 0..num_irqs {
            irqs.push(AicIrq::new_spi(i as u32));
        }
        // Create PPIs (16 per core, typically 4 cores = 64 PPIs)
        let mut redistributors = Vec::new();
        for core in 0..4 {
            let affinity = MpAffinity::new(core as u8, 0, 0, 0);
            let mut ppi = Vec::new();
            for i in 0..16 {
                ppi.push(AicIrq::new_ppi(16 + i as u32, affinity));
            }
            let mut sgi = Vec::new();
            for i in 0..16 {
                sgi.push(AicIrq::new_sgi(i as u32, affinity));
            }
            redistributors.push(Redistributor {
                affinity,
                ctlr: 0,
                typer: 0,
                ppi,
                sgi,
            });
        }
        // SGIs (Software Generated Interrupts) - 16 global
        for i in 0..16 {
            irqs.push(AicIrq::new_sgi(i as u32, MpAffinity::new(0, 0, 0, 0)));
        }

        Self {
            id,
            irqs,
            current_priority: None,
            gicd_ctlr: 0,
            gicd_typer: 0,
            gicd_iidr: 0x00000001,
            gicr_ctlr: 0,
            gicr_typer: 0,
            gicr_iidr: 0x000E0001,
            gicr_waker: 0,
            gicr_pidr: 0,
            its_ctlr: 0,
            its_iidr: 0xa0000000,
            its_typer: 0,
            its_baser: [0; 8],
            its_cbaser: 0,
            its_cwriter: 0,
            its_creader: 0,
            its_translator_event: 0,
            its_translations: HashMap::new(),
            aic2_irq_cfg: vec![0; 2048], // 0x2000 + 2048*4 ends at 0x4000
            aic2_sw_set: [0; AIC2_IRQ_BITMAP_WORDS],
            aic2_sw_clr: [0; AIC2_IRQ_BITMAP_WORDS],
            aic2_mask_set: [0; AIC2_IRQ_BITMAP_WORDS],
            aic2_mask_clr: [0; AIC2_IRQ_BITMAP_WORDS],
            aic2_hw_state: [0; AIC2_IRQ_BITMAP_WORDS],
            timer_fiq_lines: vec![[false; 2]; 4],
            redistributors,
            lpi_pending_table: None,
            lpi_config_table: None,
            gicc: (0..4).map(GiccState::new).collect(),
        }
    }

    /// Acknowledge the highest-priority pending, enabled, PMR-passing IRQ for
    /// `core`. Pushes its priority onto the running-priority stack and sets
    /// `running_irq`. Returns the IRQ id (or `0x3ff` spurious). This is the
    /// cycle-accurate priority/preemption half of Phase 207.
    pub fn acknowledge_irq(&mut self, core: usize, cycle: u64) -> u32 {
        let gicc = match self.gicc.get_mut(core) {
            Some(g) => g,
            None => return 0x3ff,
        };
        let running = gicc.running_priority();
        // Candidate IRQs: enabled, pending, priority < running (preempt), and
        // below the PMR mask. Lower priority value wins.
        let mut best: Option<(u8, u32)> = None;
        for irq in &self.irqs {
            if !irq.enabled || !irq.pending {
                continue;
            }
            if irq.priority >= running && gicc.running_irq.is_some() {
                continue; // cannot preempt the running IRQ
            }
            if irq.priority.0 >= gicc.pmr {
                continue; // blocked by the priority mask
            }
            match best {
                Some((p, _)) if p <= irq.priority.0 => {}
                _ => best = Some((irq.priority.0, irq.id)),
            }
        }
        // Also consider PPIs on this redistributor.
        if let Some(red) = self.redistributors.get_mut(core) {
            for ppi in &red.ppi {
                if !ppi.enabled || !ppi.pending {
                    continue;
                }
                if ppi.priority >= running && gicc.running_irq.is_some() {
                    continue;
                }
                if ppi.priority.0 >= gicc.pmr {
                    continue;
                }
                match best {
                    Some((p, _)) if p <= ppi.priority.0 => {}
                    _ => best = Some((ppi.priority.0, ppi.id)),
                }
            }
        }
        let Some((prio, irq_id)) = best else {
            gicc.iar = 0x3ff;
            return 0x3ff;
        };
        // Mark the IRQ active.
        if let Some(i) = self.irqs.iter_mut().find(|i| i.id == irq_id) {
            i.active = true;
            i.pending = false;
        }
        if let Some(red) = self.redistributors.get_mut(core) {
            if let Some(ppi) = red.ppi.iter_mut().find(|p| p.id == irq_id) {
                ppi.active = true;
                ppi.pending = false;
            }
        }
        gicc.running_stack.push(IrqPriority(prio));
        gicc.running_irq = Some(irq_id);
        gicc.ack_cycle = cycle;
        gicc.iar = irq_id;
        irq_id
    }

    /// End of interrupt for `core`: pop the running-priority stack and clear
    /// the active state of the serviced IRQ.
    pub fn end_of_interrupt(&mut self, core: usize) {
        let Some(gicc) = self.gicc.get_mut(core) else {
            return;
        };
        if let Some(irq_id) = gicc.running_irq.take() {
            if let Some(i) = self.irqs.iter_mut().find(|i| i.id == irq_id) {
                i.active = false;
            }
            if let Some(red) = self.redistributors.get_mut(core) {
                if let Some(ppi) = red.ppi.iter_mut().find(|p| p.id == irq_id) {
                    ppi.active = false;
                }
            }
        }
        gicc.running_stack.pop();
        gicc.iar = 0x3ff;
    }

    /// Set GICC_PMR (priority mask). Only IRQs with priority < pmr are
    /// eligible for delivery.
    pub fn set_pmr(&mut self, core: usize, pmr: u8) {
        if let Some(g) = self.gicc.get_mut(core) {
            g.pmr = pmr;
        }
    }

    /// Set GICC_BPR (binary point).
    pub fn set_bpr(&mut self, core: usize, bpr: u8) {
        if let Some(g) = self.gicc.get_mut(core) {
            g.bpr = bpr;
        }
    }

    pub fn raise(&mut self, irq: u32) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
            i.pending = true;
        }
    }

    pub fn set_priority(&mut self, irq: u32, priority: IrqPriority) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
            i.priority = priority;
        }
    }

    pub fn enable(&mut self, irq: u32) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
            i.enabled = true;
        }
    }

    pub fn disable(&mut self, irq: u32) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
            i.enabled = false;
        }
    }

    fn sync_aic2_sw_set(&mut self) {
        for word in 0..64 {
            let bits = self.aic2_sw_set[word];
            let base = (word * 32) as u32;
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    self.raise(base + bit);
                }
            }
        }
    }

    fn sync_aic2_sw_clr(&mut self) {
        for word in 0..64 {
            let bits = self.aic2_sw_clr[word];
            let base = (word * 32) as u32;
            for bit in 0..32 {
                if bits & (1 << bit) != 0 {
                    let irq = base + bit;
                    if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
                        i.pending = false;
                    }
                    // W1C: a software clear retires the latched software-set
                    // bit too, so a later SW_SET write's sync does not re-raise
                    // this line alongside the newly-set one.
                    self.aic2_sw_set[word] &= !(1 << bit);
                }
            }
        }
    }

    fn sync_aic2_mask(&mut self) {
        for word in 0..64 {
            let set = self.aic2_mask_set[word];
            let clr = self.aic2_mask_clr[word];
            let base = (word * 32) as u32;
            for bit in 0..32 {
                let irq = base + bit;
                if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
                    if set & (1 << bit) != 0 {
                        i.enabled = true;
                    }
                    if clr & (1 << bit) != 0 {
                        i.enabled = false;
                    }
                }
            }
        }
    }

    pub fn acknowledge(&mut self) -> Option<u32> {
        // Find highest priority pending enabled IRQ that can preempt
        let irq = self
            .irqs
            .iter()
            .filter(|i| i.enabled && i.pending && !i.active)
            .filter(|i| self.current_priority.is_none_or(|cp| i.priority < cp))
            .min_by_key(|i| i.priority);

        if let Some(irq) = irq {
            let id = irq.id;
            if let Some(i) = self.irqs.iter_mut().find(|x| x.id == id) {
                i.pending = false;
                i.active = true;
                self.current_priority = Some(i.priority);
                return Some(id);
            }
        }
        None
    }

    pub fn complete(&mut self, irq: u32) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
            i.active = false;
            self.recalculate_current_priority();
        }
    }

    /// Read a GICC register for `core`. Phase 207: PMR, BPR, IAR, EOIR. The IAR
    /// read is a side-effecting acknowledge: it returns the id of the
    /// highest-priority pending, enabled, PMR-passing IRQ for this core and
    /// pushes its priority onto the running-priority stack (0x3ff = spurious).
    pub fn read_gicc(&mut self, core: usize, reg: u64) -> DeviceResult {
        match reg {
            0x00 => {
                // GICC_PMR
                DeviceResult::Value(self.gicc.get(core).map(|g| g.pmr as u64).unwrap_or(0xff))
            }
            0x04 => {
                // GICC_BPR
                DeviceResult::Value(self.gicc.get(core).map(|g| g.bpr as u64).unwrap_or(0))
            }
            0x0c => {
                // GICC_IAR: side-effecting acknowledge.
                let irq = self.acknowledge_irq(core, 0);
                DeviceResult::Value(irq as u64)
            }
            0x10 => DeviceResult::Value(0), // GICC_EOIR (read is RAZ/WI)
            _ => DeviceResult::Fault,
        }
    }

    fn recalculate_current_priority(&mut self) {
        self.current_priority = self
            .irqs
            .iter()
            .filter(|irq| irq.active)
            .min_by_key(|irq| irq.priority)
            .map(|irq| irq.priority);
    }

    pub fn get_highest_pending(&self) -> Option<u32> {
        self.irqs
            .iter()
            .filter(|i| i.enabled && i.pending && !i.active)
            .filter(|i| self.current_priority.is_none_or(|cp| i.priority < cp))
            .min_by_key(|i| i.priority)
            .map(|i| i.id)
    }

    // Helper methods for GICD register range operations
    fn enable_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.enabled = true;
            }
        }
    }

    fn disable_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.enabled = false;
            }
        }
    }

    fn set_pending_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.pending = true;
            }
        }
    }

    fn clear_pending_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.pending = false;
            }
        }
    }

    fn set_active_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.active = true;
            }
        }
        self.recalculate_current_priority();
    }

    fn clear_active_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && (bits >> (irq.id - first_irq)) & 1 == 1
            {
                irq.active = false;
            }
        }
        self.recalculate_current_priority();
    }

    fn bitmap(&self, first_irq: u32, predicate: impl Fn(&AicIrq) -> bool) -> u32 {
        self.irqs.iter().fold(0, |bits, irq| {
            if irq.int_type == InterruptType::SPI
                && irq.id >= first_irq
                && irq.id < first_irq + 32
                && predicate(irq)
            {
                bits | (1 << (irq.id - first_irq))
            } else {
                bits
            }
        })
    }

    fn set_priority_range(&mut self, first_irq: u32, priorities: u32) {
        for byte in 0..4_u32 {
            if let Some(irq) = self
                .irqs
                .iter_mut()
                .find(|irq| irq.int_type == InterruptType::SPI && irq.id == first_irq + byte)
            {
                irq.priority = IrqPriority((priorities >> (byte * 8)) as u8);
            }
        }
        self.recalculate_current_priority();
    }

    fn priority_word(&self, first_irq: u32) -> u32 {
        let mut value = 0;
        for byte in 0..4_u32 {
            if let Some(irq) = self
                .irqs
                .iter()
                .find(|irq| irq.int_type == InterruptType::SPI && irq.id == first_irq + byte)
            {
                value |= u32::from(irq.priority.0) << (byte * 8);
            }
        }
        value
    }

    fn set_config_range(&mut self, first_irq: u32, configs: u32) {
        for slot in 0..16_u32 {
            if let Some(irq) = self
                .irqs
                .iter_mut()
                .find(|irq| irq.int_type == InterruptType::SPI && irq.id == first_irq + slot)
            {
                irq.config = ((configs >> (slot * 2)) & 0b11) as u8;
            }
        }
    }

    fn config_word(&self, first_irq: u32) -> u32 {
        let mut value = 0;
        for slot in 0..16_u32 {
            if let Some(irq) = self
                .irqs
                .iter()
                .find(|irq| irq.int_type == InterruptType::SPI && irq.id == first_irq + slot)
            {
                value |= u32::from(irq.config & 0b11) << (slot * 2);
            }
        }
        value
    }

    fn set_group_range(&mut self, first_irq: u32, bits: u32) {
        for irq in &mut self.irqs {
            if irq.int_type == InterruptType::SPI && irq.id >= first_irq && irq.id < first_irq + 32
            {
                irq.group = if (bits >> (irq.id - first_irq)) & 1 != 0 {
                    InterruptGroup::Group1NS
                } else {
                    InterruptGroup::Group0
                };
            }
        }
    }

    fn send_sgi(&mut self, sgi_id: u32, target_affinity: u32) {
        // Find SGI with matching ID and send to target affinity
        for irq in &mut self.irqs {
            if irq.id == sgi_id
                && irq.int_type == InterruptType::SGI
                && irq
                    .target_affinity
                    .is_none_or(|aff| aff.to_mpidr() & 0xFFFFFFFF == target_affinity as u64)
            {
                irq.pending = true;
            }
        }
    }

    pub fn raise_lpi(&mut self, lpi: u32) {
        // GICR_SETLPIR / GITS_INT path: mark the LPI pending in the AIC irq bank.
        if let Some(i) = self
            .irqs
            .iter_mut()
            .find(|x| x.id == lpi && x.int_type == InterruptType::LPI)
        {
            i.pending = true;
        } else {
            let mut irq = AicIrq::new_lpi(lpi);
            irq.pending = true;
            self.irqs.push(irq);
        }
        if let Some(table) = self.lpi_pending_table.as_mut() {
            let word = (lpi as usize) / 64;
            let bit = (lpi as usize) % 64;
            if word < table.len() {
                table[word] |= 1u64 << bit;
            }
        }
    }

    pub fn clear_lpi(&mut self, lpi: u32) {
        if let Some(i) = self.irqs.iter_mut().find(|x| x.id == lpi) {
            i.pending = false;
        }
        if let Some(table) = self.lpi_pending_table.as_mut() {
            let word = (lpi as usize) / 64;
            let bit = (lpi as usize) % 64;
            if word < table.len() {
                table[word] &= !(1u64 << bit);
            }
        }
    }

    // ITS: record a device->device/event->LPID translation.
    pub fn its_map(&mut self, devid: u32, eventid: u32, lpi: u32) {
        let key = ((devid as u64) << 32) | eventid as u64;
        self.its_translations.insert(key, lpi);
    }

    pub fn its_translate(&self, devid: u32, eventid: u32) -> Option<u32> {
        self.its_translations
            .get(&(((devid as u64) << 32) | eventid as u64))
            .copied()
    }
}

impl Device for Aic {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "aic"
    }
    fn reset(&mut self) {
        for i in &mut self.irqs {
            i.pending = false;
            i.active = false;
            i.enabled = false;
            i.priority = IrqPriority::NORMAL;
            i.group = InterruptGroup::Group1NS;
            i.config = 0;
        }
        for redistributor in &mut self.redistributors {
            redistributor.ctlr = 0;
            for irq in redistributor
                .ppi
                .iter_mut()
                .chain(redistributor.sgi.iter_mut())
            {
                irq.pending = false;
                irq.active = false;
                irq.enabled = false;
                irq.priority = IrqPriority::NORMAL;
                irq.group = InterruptGroup::Group1NS;
                irq.config = 0;
            }
        }
        self.current_priority = None;
        self.gicd_ctlr = 0;
        self.gicr_ctlr = 0;
        self.gicr_waker = 0;
        self.its_ctlr = 0;
        self.its_baser = [0; 8];
        self.its_cbaser = 0;
        self.its_cwriter = 0;
        self.its_creader = 0;
        self.its_translator_event = 0;
        self.its_translations.clear();
        self.aic2_irq_cfg.fill(0);
        self.aic2_sw_set.fill(0);
        self.aic2_sw_clr.fill(0);
        self.aic2_mask_set.fill(0);
        self.aic2_mask_clr.fill(0);
        self.aic2_hw_state.fill(0);
        self.timer_fiq_lines.fill([false; 2]);
        self.lpi_pending_table = None;
        self.lpi_config_table = None;
    }
    fn read(&mut self, o: u64, _: usize) -> DeviceResult {
        match o {
            // AICv2 reset is a command register; reserved bits read as zero.
            AIC2_RESET_OFFSET => DeviceResult::Value(0),
            // GICD (Distributor) registers
            0x0000 => DeviceResult::Value(self.gicd_ctlr as u64), // GICD_CTLR
            0x0004 => DeviceResult::Value(self.gicd_typer as u64), // GICD_TYPER
            0x0008 => DeviceResult::Value(self.gicd_iidr as u64), // GICD_IIDR
            0x000c => DeviceResult::Value(0), // GICD_STATUSR (GICv3)
            // Reserved / GICv3 SPI registers (read as zero). The range starts one
            // word above `AIC2_RESET_OFFSET` because that constant is `0x0010`, so
            // `0x0010..=0x007c` overlapped the reset arm above. The overlap was
            // harmless -- the reset arm is first and a reserved read is also zero --
            // but an arm that can never be reached is a lie about the register map,
            // so the one offset the reset owns is excluded rather than allowed.
            0x0014..=0x007c => DeviceResult::Value(0),
            0x0080..=0x00fc => {
                let first = ((o - 0x0080) / 4) as u32 * 32;
                DeviceResult::Value(
                    self.bitmap(first, |irq| irq.group == InterruptGroup::Group1NS) as u64,
                )
            }
            0x0100..=0x017c | 0x0180..=0x01fc => {
                let base = if o < 0x0180 { 0x0100 } else { 0x0180 };
                let first = ((o - base) / 4) as u32 * 32;
                DeviceResult::Value(self.bitmap(first, |irq| irq.enabled) as u64)
            }
            0x0200..=0x027c | 0x0280..=0x02fc => {
                let base = if o < 0x0280 { 0x0200 } else { 0x0280 };
                let first = ((o - base) / 4) as u32 * 32;
                DeviceResult::Value(self.bitmap(first, |irq| irq.pending) as u64)
            }
            0x0300..=0x037c | 0x0380..=0x03fc => {
                let base = if o < 0x0380 { 0x0300 } else { 0x0380 };
                let first = ((o - base) / 4) as u32 * 32;
                DeviceResult::Value(self.bitmap(first, |irq| irq.active) as u64)
            }
            0x0400..=0x07fc => {
                let first = (o - 0x0400) as u32;
                DeviceResult::Value(self.priority_word(first) as u64)
            }
            0x0800 => DeviceResult::Value(
                self.irqs
                    .iter()
                    .filter(|i| i.group == InterruptGroup::Group0)
                    .count() as u64,
            ), // GICD_IGROUPR
            0x0c00..=0x0cfc => {
                let first = ((o - 0x0c00) / 4) as u32 * 16;
                DeviceResult::Value(self.config_word(first) as u64)
            }
            0x0f00 => DeviceResult::Value(
                self.irqs
                    .iter()
                    .filter(|i| i.int_type == InterruptType::SGI)
                    .count() as u64,
            ), // GICD_SGIR
            0x1000 => DeviceResult::Value(self.gicd_typer as u64), // GICD_TYPER
            0x1004 => DeviceResult::Value(self.gicd_iidr as u64),  // GICD_IIDR

            // GICR (Redistributor) registers
            0x10000 => DeviceResult::Value(self.gicr_ctlr as u64), // GICR_CTLR
            0x10004 => DeviceResult::Value(self.gicr_iidr as u64), // GICR_IIDR
            0x10008 => DeviceResult::Value(self.gicr_typer),       // GICR_TYPER
            0x10014 => DeviceResult::Value(self.gicr_waker as u64), // GICR_WAKER
            0x10010 => DeviceResult::Value(self.gicr_pidr as u64), // GICR_PIDR2
            0x10020 => DeviceResult::Value(self.gicr_pidr as u64), // GICR_PIDR3
            0x1007c => DeviceResult::Value(0),                     // GICR_PROPBASER mirror
            0x1ffe8 => DeviceResult::Value(self.gicr_pidr as u64), // GICR_PIDR2 (canonical)

            // ITS (Interrupt Translation Service)
            0x20000 => DeviceResult::Value(self.its_ctlr as u64), // GITS_CTLR
            0x20004 => DeviceResult::Value(self.its_iidr as u64), // GITS_IIDR
            0x20008 => DeviceResult::Value(self.its_cbaser),      // GITS_CBASER
            0x2000c => DeviceResult::Value(self.its_typer as u64), // GITS_TYPER
            0x20010 => DeviceResult::Value(self.its_cwriter),     // GITS_CWRITER
            0x20018 => DeviceResult::Value(self.its_creader),     // GITS_CREADR
            0x20080..=0x200b8 => DeviceResult::Value(self.its_baser[((o - 0x20080) / 8) as usize]), // GITS_BASERn
            0x21000 => DeviceResult::Value(self.its_translator_event), // GITS_TRANSLATER latch

            // Legacy Apple AIC interface
            0x0E00 => DeviceResult::Value(self.get_highest_pending().unwrap_or(0) as u64),
            0x0E08 => DeviceResult::Value(self.irqs.len() as u64),
            0x0E10 => DeviceResult::Value(self.irqs.iter().filter(|i| i.enabled).count() as u64),
            0x0E18 => DeviceResult::Value(self.irqs.iter().filter(|i| i.pending).count() as u64),
            0x0E20 => DeviceResult::Value(self.irqs.iter().filter(|i| i.active).count() as u64),

            // AIC2 per-IRQ target config window (4 bytes per IRQ). The d321ap
            // device tree declares /arm-io/aic with aic-version = 02; m1n1's
            // AIC2_IRQ_CFG places this window at 0x2000.
            AIC2_IRQ_CFG_START..=AIC2_IRQ_CFG_END => DeviceResult::Value(
                self.aic2_irq_cfg[((o - AIC2_IRQ_CFG_START) / 4) as usize] as u64,
            ),
            AIC2_SW_SET_START..=0x40fc => {
                DeviceResult::Value(self.aic2_sw_set[aic2_bitmap_word(o, AIC2_SW_SET_START)] as u64)
            }
            AIC2_SW_CLR_START..=0x41fc => {
                DeviceResult::Value(self.aic2_sw_clr[aic2_bitmap_word(o, AIC2_SW_CLR_START)] as u64)
            }
            AIC2_MASK_SET_START..=0x42fc => DeviceResult::Value(
                self.aic2_mask_set[aic2_bitmap_word(o, AIC2_MASK_SET_START)] as u64,
            ),
            AIC2_MASK_CLR_START..=0x43fc => DeviceResult::Value(
                self.aic2_mask_clr[aic2_bitmap_word(o, AIC2_MASK_CLR_START)] as u64,
            ),
            AIC2_HW_STATE_START..=0x44fc => DeviceResult::Value(
                self.aic2_hw_state[aic2_bitmap_word(o, AIC2_HW_STATE_START)] as u64,
            ),
            // GICC (CPU interface) per-core registers at 0x8000 + core*0x1000.
            // Phase 207: PMR (0x00), BPR (0x04), IAR (0x0C), EOIR (0x10).
            // The window covers all four cores (0x8000..=0xbffc).
            0x8000..=0xbffc => {
                let core = ((o - 0x8000) / 0x1000) as usize;
                let reg = (o - 0x8000) % 0x1000;
                self.read_gicc(core, reg)
            }
            _ => DeviceResult::Fault,
        }
    }

    fn write(&mut self, o: u64, _: usize, v: u64) -> DeviceResult {
        match o {
            AIC2_RESET_OFFSET => {
                if v as u32 & AIC2_RESET_BIT != 0 {
                    self.reset();
                }
                DeviceResult::Ok
            }
            // GICD (Distributor) registers
            0x0000 => {
                self.gicd_ctlr = v as u32;
                DeviceResult::Ok
            } // GICD_CTLR
            0x0004 => DeviceResult::Ok, // GICD_TYPER (read-only, ignore writes)
            0x0008 => DeviceResult::Ok, // GICD_IIDR (read-only, ignore writes)
            0x000c => DeviceResult::Ok, // GICD_STATUSR (read-only, ignore writes)
            0x0014..=0x007c => DeviceResult::Ok, // Reserved / GICv3 SPI registers (ignore writes)
            0x0080..=0x00FC => {
                let first = ((o - 0x0080) / 4) as u32 * 32;
                self.set_group_range(first, v as u32);
                DeviceResult::Ok
            }
            0x0100..=0x017C => {
                /* GICD_ISENABLER */
                self.enable_range(((o - 0x0100) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0180..=0x01FC => {
                /* GICD_ICENABLER */
                self.disable_range(((o - 0x0180) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0200..=0x027C => {
                /* GICD_ISPENDR */
                self.set_pending_range(((o - 0x0200) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0280..=0x02FC => {
                /* GICD_ICPENDR */
                self.clear_pending_range(((o - 0x0280) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0300..=0x037C => {
                /* GICD_ISACTIVER */
                self.set_active_range(((o - 0x0300) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0380..=0x03FC => {
                /* GICD_ICACTIVER */
                self.clear_active_range(((o - 0x0380) / 4) as u32 * 32, v as u32);
                DeviceResult::Ok
            }
            0x0400..=0x07FC => {
                /* GICD_IPRIORITYR */
                self.set_priority_range((o - 0x0400) as u32, v as u32);
                DeviceResult::Ok
            }
            0x0800..=0x08FC => {
                /* GICD_ITARGETSR compatibility window */
                DeviceResult::Ok
            }
            0x0c00..=0x0cfc => {
                /* GICD_ICFGR */
                self.set_config_range(((o - 0x0c00) / 4) as u32 * 16, v as u32);
                DeviceResult::Ok
            }
            0x0f00 => {
                /* GICD_SGIR */
                self.send_sgi((v & 0xFF) as u32, ((v >> 16) & 0xFF) as u32);
                DeviceResult::Ok
            }

            // GICR (Redistributor) registers
            0x10000 => {
                self.gicr_ctlr = v as u32;
                DeviceResult::Ok
            } // GICR_CTLR
            0x10004 => {
                self.gicr_iidr = v as u32;
                DeviceResult::Ok
            } // GICR_IIDR
            0x10014 => {
                /* GICR_WAKER: host clears ProcessorSleep; SGIs/PPIs stay in sleep state */
                self.gicr_waker = v as u32;
                DeviceResult::Ok
            }
            0x10040 => {
                /* GICR_SETLPIR: bits[23:0] select the LPI to raise */
                self.raise_lpi((v & 0x00ff_ffff) as u32);
                DeviceResult::Ok
            }
            0x10048 => {
                /* GICR_CLRLPIR: bits[23:0] select the LPI to clear */
                self.clear_lpi((v & 0x00ff_ffff) as u32);
                DeviceResult::Ok
            }
            0x10080 => {
                /* GICR_WAKER legacy */
                DeviceResult::Ok
            }

            // ITS (Interrupt Translation Service)
            0x20000 => {
                self.its_ctlr = v as u32;
                DeviceResult::Ok
            } // GITS_CTLR
            0x20004 => {
                self.its_iidr = v as u32;
                DeviceResult::Ok
            } // GITS_IIDR
            0x20008 => {
                self.its_cbaser = v;
                DeviceResult::Ok
            } // GITS_CBASER
            0x2000c => {
                self.its_typer = v as u32;
                DeviceResult::Ok
            } // GITS_TYPER
            0x20010 => {
                self.its_cwriter = v;
                DeviceResult::Ok
            } // GITS_CWRITER
            0x20018 => {
                self.its_creader = v;
                DeviceResult::Ok
            } // GITS_CREADR
            0x20080..=0x200b8 => {
                self.its_baser[((o - 0x20080) / 8) as usize] = v;
                DeviceResult::Ok
            } // GITS_BASERn
            0x21000 => {
                /* GITS_TRANSLATER: v[47:32] DeviceID, v[31:0] EventID -> raise mapped LPI */
                let devid = ((v >> 32) & 0xffff) as u32;
                let eventid = (v & 0xffff_ffff) as u32;
                self.its_translator_event = v;
                if let Some(lpi) = self.its_translate(devid, eventid) {
                    self.raise_lpi(lpi);
                }
                DeviceResult::Ok
            }

            // Legacy Apple AIC interface
            0x0E00 => {
                // Acknowledge
                if let Some(irq) = self.acknowledge() {
                    DeviceResult::Value(irq as u64)
                } else {
                    DeviceResult::Value(0)
                }
            }
            0x0E08 => {
                // Enable IRQ
                self.enable(v as u32);
                DeviceResult::Ok
            }
            0x0E10 => {
                // Disable IRQ
                self.disable(v as u32);
                DeviceResult::Ok
            }
            0x0E18 => {
                // Set priority
                let irq = (v >> 32) as u32;
                let priority = IrqPriority((v & 0xFF) as u8);
                self.set_priority(irq, priority);
                DeviceResult::Ok
            }
            0x0E20 => {
                // Complete IRQ
                self.complete(v as u32);
                DeviceResult::Ok
            }
            AIC2_IRQ_CFG_START..=AIC2_IRQ_CFG_END => {
                self.aic2_irq_cfg[((o - AIC2_IRQ_CFG_START) / 4) as usize] = v as u32;
                DeviceResult::Ok
            }
            // T8020 ROM per-IRQ mask window. The ROM's own enable sequence at
            // 0x100002880 loads the base `0x23b102000`, reads the config slot
            // there, computes `1 << value`, and stores it to `base + 0x1000 +
            // irq`, i.e. one 32-bit word per IRQ in the 0x3000 page. The T8020
            // ROM enables its two SEP-mail lines this way: irq 161 lands at
            // 0x23b103284 and irq 164 at 0x23b103290, which is exactly what an
            // 8M-instruction run records. This is the only path in the whole
            // ROM trace that unmask-enables an interrupt line, so without it
            // no device could ever reach the AP and the post-init scheduler
            // could never post an event.
            AIC2_IRQ_MASK_START..=AIC2_IRQ_MASK_END => {
                let irq = ((o - AIC2_IRQ_MASK_START) / 4) as u32;
                if let Some(i) = self.irqs.iter_mut().find(|x| x.id == irq) {
                    i.enabled = v as u32 != 0;
                }
                DeviceResult::Ok
            }
            // AIC2 SW_SET / SW_CLR / MASK_SET / MASK_CLR / HW_STATE banks.
            AIC2_SW_SET_START..=0x40fc => {
                let word = aic2_bitmap_word(o, AIC2_SW_SET_START);
                self.aic2_sw_set[word] |= v as u32;
                self.sync_aic2_sw_set();
                DeviceResult::Ok
            }
            AIC2_SW_CLR_START..=0x41fc => {
                let word = aic2_bitmap_word(o, AIC2_SW_CLR_START);
                self.aic2_sw_clr[word] |= v as u32;
                self.sync_aic2_sw_clr();
                DeviceResult::Ok
            }
            AIC2_MASK_SET_START..=0x42fc => {
                let word = aic2_bitmap_word(o, AIC2_MASK_SET_START);
                self.aic2_mask_set[word] |= v as u32;
                self.sync_aic2_mask();
                DeviceResult::Ok
            }
            AIC2_MASK_CLR_START..=0x43fc => {
                let word = aic2_bitmap_word(o, AIC2_MASK_CLR_START);
                self.aic2_mask_clr[word] |= v as u32;
                self.sync_aic2_mask();
                DeviceResult::Ok
            }
            AIC2_HW_STATE_START..=0x44fc => {
                self.aic2_hw_state[aic2_bitmap_word(o, AIC2_HW_STATE_START)] = v as u32;
                DeviceResult::Ok
            }
            // GICC (CPU interface) per-core registers at 0x8000 + core*0x1000.
            // Phase 207: PMR (0x00), BPR (0x04), IAR (0x0C), EOIR (0x10).
            // The window covers all four cores (0x8000..=0xbffc).
            0x8000..=0xbffc => {
                let core = ((o - 0x8000) / 0x1000) as usize;
                let reg = (o - 0x8000) % 0x1000;
                match reg {
                    0x00 => {
                        self.set_pmr(core, v as u8); // GICC_PMR
                    }
                    0x04 => {
                        self.set_bpr(core, v as u8); // GICC_BPR
                    }
                    0x0c => { /* GICC_IAR is read-only; writes are ignored. */ }
                    0x10 => self.end_of_interrupt(core), // GICC_EOIR
                    _ => {}
                }
                DeviceResult::Ok
            }
            _ => DeviceResult::Fault,
        }
    }
    fn signal_interrupt(&mut self, irq: u32, core: Option<usize>) -> bool {
        // Try PPI first (core-specific)
        if let Some(core_idx) = core {
            if let Some(redistributor) = self.redistributors.get_mut(core_idx) {
                if let Some(ppi) = redistributor.ppi.iter_mut().find(|ppi| ppi.id == irq) {
                    ppi.pending = true;
                    return true;
                }
            }
        }
        // Try SPI (shared peripheral interrupts)
        for irq_obj in &mut self.irqs {
            if irq_obj.int_type == InterruptType::SPI && irq_obj.id == irq {
                irq_obj.pending = true;
                return true;
            }
        }
        false
    }

    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        // Apple AIC delivers architectural timer sources as per-core FIQs.
        // The AIC FIQ handler identifies them by reading the timer controls.
        for (core, lines) in self.timer_fiq_lines.iter().enumerate() {
            if lines[0] || lines[1] {
                return Some(DeviceEvent::FiqTo(if lines[0] { 0 } else { 1 }, Some(core)));
            }
        }
        // Distributor-level IRQs (SPIs plus the global SGI block). SPIs carry
        // no affinity and are broadcast; anything with an affinity is routed.
        if let Some(irq) = self.get_highest_pending() {
            let target = self
                .irqs
                .iter()
                .find(|i| i.id == irq)
                .and_then(|i| i.target_affinity)
                .map(|aff| aff.aff0 as usize);
            if matches!(target, Some(c) if c < self.redistributors.len()) {
                return Some(DeviceEvent::InterruptTo(irq, target));
            }
            return Some(DeviceEvent::InterruptTo(irq, None));
        }

        // Per-redistributor PPIs/SGIs: pick the highest-priority pending
        // enabled bank interrupt and deliver it to its owning core only.
        let mut best: Option<(IrqPriority, u32, usize)> = None;
        for (bank, clear_and_r) in self.redistributors.iter().enumerate() {
            for bank_irq in clear_and_r.ppi.iter().chain(clear_and_r.sgi.iter()) {
                if !bank_irq.enabled || !bank_irq.pending || bank_irq.active {
                    continue;
                }
                if self
                    .current_priority
                    .is_none_or(|cp| bank_irq.priority < cp)
                    && best.is_none_or(|(bp, _, _)| bank_irq.priority < bp)
                {
                    best = Some((bank_irq.priority, bank_irq.id, bank));
                }
            }
        }
        if let Some((_, id, bank)) = best {
            return Some(DeviceEvent::InterruptTo(id, Some(bank)));
        }
        None
    }
    fn set_fiq_line(&mut self, line: u32, core: Option<usize>, asserted: bool) -> bool {
        if line > 1 {
            return false;
        }
        let Some(lines) = core.and_then(|index| self.timer_fiq_lines.get_mut(index)) else {
            return false;
        };
        lines[line as usize] = asserted;
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Inherent Aic methods. `read_gicc` is the Phase 207 per-core GICC read path
/// (PMR/BPR/IAR/EOIR); the IAR read is a side-effecting acknowledge that
/// pushes the serviced IRQ's priority onto the running-priority stack.
impl Aic {}

pub struct Uart {
    pub id: DeviceId,
    pub tx: Vec<u8>,
    pub rx: Vec<u8>,
    pub baud_rate: u32,
    pub ctrl: u32,
    pub status: u32,
    pub baud_div: u32,
    pub enabled: bool,
    pub irq: u32,
    rx_irq_signaled: bool,
}
impl Uart {
    pub fn new(id: DeviceId, irq: u32) -> Self {
        Self {
            id,
            tx: Vec::new(),
            rx: Vec::new(),
            baud_rate: 115200,
            ctrl: 0,
            status: 0,
            baud_div: 26, // 115200 @ 3MHz
            enabled: false,
            irq,
            rx_irq_signaled: false,
        }
    }

    pub fn output(&self) -> &[u8] {
        &self.tx
    }

    pub fn inject_rx(&mut self, bytes: &[u8]) {
        self.rx.extend_from_slice(bytes);
        self.rx_irq_signaled = false;
    }

    /// Drain and return all pending TX bytes, clearing the internal buffer.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }

    fn set_baud_rate(&mut self, baud: u32) {
        self.baud_rate = baud.max(1);
        // Assuming 3MHz clock
        self.baud_div = (3_000_000 / self.baud_rate.saturating_mul(16).max(1)).max(1);
    }
}
impl Device for Uart {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "uart"
    }
    fn reset(&mut self) {
        self.tx.clear();
        self.rx.clear();
        self.ctrl = 0;
        self.status = 0;
        self.enabled = false;
        self.baud_rate = 115_200;
        self.baud_div = 26;
        self.rx_irq_signaled = false;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                // RX FIFO
                if !self.rx.is_empty() {
                    let v = self.rx.remove(0) as u64;
                    if self.rx.is_empty() {
                        self.rx_irq_signaled = false;
                    }
                    DeviceResult::Value(v)
                } else {
                    DeviceResult::Value(0)
                }
            }
            0x04 => DeviceResult::Value(self.tx.len() as u64), // TX FIFO level
            0x08 => DeviceResult::Value(self.rx.len() as u64), // RX FIFO level
            0x0C => DeviceResult::Value(self.ctrl as u64),     // Control register
            0x10 => {
                let mut status = self.status | 0x02;
                if self.rx.is_empty() {
                    status &= !0x01;
                } else {
                    status |= 0x01;
                }
                if !self.tx.is_empty() {
                    status |= 0x20;
                }
                DeviceResult::Value(status as u64)
            }
            // Apple/S5L UTRSTAT compatibility: bit 0 reports RX-ready, bit 1
            // TX-busy, and bit 2 TX-empty. Keep the configured divisor in the
            // public baud_div field; firmware needs status at this address.
            0x14 => {
                let mut status = self.status & !0x07;
                if !self.rx.is_empty() {
                    status |= 0x01;
                }
                if !self.tx.is_empty() {
                    status |= 0x02;
                } else {
                    status |= 0x04;
                }
                DeviceResult::Value(status as u64)
            }
            0x1C => {
                if self.rx.is_empty() {
                    DeviceResult::Value(0)
                } else {
                    let value = self.rx.remove(0) as u64;
                    if self.rx.is_empty() {
                        self.rx_irq_signaled = false;
                    }
                    DeviceResult::Value(value)
                }
            }
            _ => DeviceResult::Fault,
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                // TX FIFO
                let byte = v as u8;
                self.tx.push(byte);
                DeviceResult::Ok
            }
            0x18 => {
                // Apple/S5L UTXH transmit holding register alias.
                let byte = v as u8;
                if byte != 0 {
                    self.tx.push(byte);
                }
                DeviceResult::Ok
            }
            0x0C => {
                // Control register
                self.ctrl = v as u32;
                self.enabled = (v & 1) != 0;
                DeviceResult::Ok
            }
            0x14 => {
                // Baud rate (Hz)
                self.set_baud_rate(v as u32);
                DeviceResult::Ok
            }
            _ => DeviceResult::Fault,
        }
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        if self.enabled && self.ctrl & 0x02 != 0 && !self.rx.is_empty() {
            if !self.rx_irq_signaled {
                self.rx_irq_signaled = true;
                return Some(DeviceEvent::Interrupt(self.irq));
            }
        } else {
            self.rx_irq_signaled = false;
        }
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
/// Host-visible UART window side-channel.
///
/// Firmware (or a test harness) can write characters into this device; the
/// host polls `drain()` / `output()` and feeds the bytes into the SDL
/// `UartConsoleWindow`.  This replaces the previous empty `push` stub.
pub struct UartWindow {
    pub id: DeviceId,
    pub enabled: bool,
    pub title: &'static str,
    /// Pending TX bytes that have not yet been consumed by the host renderer.
    pub pending: Vec<u8>,
}
impl UartWindow {
    pub fn new(id: DeviceId, title: &'static str) -> Self {
        Self {
            id,
            enabled: true, // host console is ready by default
            title,
            pending: Vec::new(),
        }
    }
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }
    /// Queue a single byte for the host console.
    pub fn push(&mut self, b: u8) {
        if self.enabled {
            self.pending.push(b);
        }
    }
    /// Drain and return all pending bytes, clearing the internal buffer.
    pub fn drain(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
    pub fn output(&self) -> &[u8] {
        &self.pending
    }
}
impl Device for UartWindow {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "uart-window"
    }
    fn reset(&mut self) {
        self.enabled = false;
        self.pending.clear();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.enabled as u64),
            0x04 => DeviceResult::Value(self.pending.len() as u64),
            _ => DeviceResult::Value(0),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                // bit0 = enable
                self.enabled = (v & 1) != 0;
                DeviceResult::Ok
            }
            0x08 => {
                // Write a byte into the pending queue (guest → host)
                self.push(v as u8);
                DeviceResult::Ok
            }
            _ => DeviceResult::Ok,
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Power-Management Controller (PMGR). Real PMGR exposes a register window
/// with per-power-state ack/req/divisor/clock-enable/reset-control fields
/// across multiple domains (PS1/PS2/GPU/AOP/...). The Phase 165 model
/// expands the original 2-register stub into a multi-register register map
/// while keeping the public `clocks` / `resets` fields for backward
/// compatibility with existing tests.
pub const PMGR_PS_COUNT: usize = 8;
pub const PMGR_RESET_COUNT: usize = 16;
pub const PMGR_CLOCK_COUNT: usize = 16;

/// Whether per-access PMGR tracing is enabled (`IPHEM_TRACE_PMGR_ACCESS=1`).
///
/// Resolved once and cached: the PMGR write path is hot, and an environment
/// lookup on every access would be a real cost in a run that makes thousands of
/// them.
fn pmgr_access_trace() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("IPHEM_TRACE_PMGR_ACCESS").is_some())
}

pub struct Pmgr {
    pub id: DeviceId,
    /// Legacy fields used by Phase 159 tests.
    pub clocks: u64,
    pub resets: u64,
    /// Per-power-state ack bits (RO): domain has acknowledged the requested
    /// state. Mirrors the PSx_ACK register.
    pub ps_ack: [u32; PMGR_PS_COUNT],
    /// Per-power-state request bits (RW): domain's requested power state
    /// (0 = off, 1 = on, 2 = retention). Mirrors the PSx_REQ register.
    pub ps_req: [u32; PMGR_PS_COUNT],
    /// Per-clock enable / divisor (bit i = enabled, low byte = divisor).
    pub clock_enable: [u32; PMGR_CLOCK_COUNT],
    /// Per-reset-rail assert (bit i = asserted).
    pub reset_ctrl: [u32; PMGR_RESET_COUNT],
    /// Device-id / revision-id / variant-id.
    pub devid: u32,
    pub revid: u32,
    pub variant: u32,
    /// Guest-written values for PMGR registers without modeled side effects.
    /// Unwritten registers read as zero; no completion bits are synthesized.
    pub registers: std::collections::HashMap<u64, u64>,
    /// An inclusive `(first, last)` offset range in which a 32-bit read reports the
    /// request nibble back in the status nibble. `None` — the default, and the
    /// behaviour every other SoC in the tree sees — leaves such registers inert.
    ///
    /// This exists for the T7000 SecureROM's slot helper, which is the only place
    /// where a completion condition has been *measured* rather than assumed:
    /// `0x10000aa94`..`0x10000aae0` writes bits 3:0 of `base + index*8` and then spins
    /// on `((w9) ^ (w9 >> 4)) & 0xf` until it is zero, i.e. until bits 7:4 report what
    /// bits 3:0 requested. A model that returns the written value can never satisfy
    /// that, so the poll is not slow but dead.
    ///
    /// What is derived here is the register's *shape* (the firmware's own completion
    /// condition), not a measurement of silicon, and the range is the helper's own
    /// bound. Nothing here says which device a given slot is: the A8 DeviceTree
    /// labels thirteen `pmgr device[]` entries and slot 0x3f is not one of them.
    pub slot_ack: Option<(u64, u64)>,
    /// Plane offsets whose 32-bit read reports `0xf` in the status nibble (bits 7:4),
    /// keeping every other bit as the guest left it. `None` — the default — leaves such
    /// registers inert.
    ///
    /// # Why this is a separate rule from `slot_ack`
    ///
    /// `slot_ack` says the status nibble *echoes the request nibble*. That is what the
    /// T7000's own slot helper demands: it writes bits 3:0 and spins on
    /// `((w9) ^ (w9 >> 4)) & 0xf`, which is only zero when bits 7:4 equal bits 3:0. One
    /// rule fits the whole `0x20000..=0x21008` table for that helper.
    ///
    /// The T7000 has a **second** PMGR service in the same table, at plane offset `0x400`,
    /// whose completion condition is different. At `0x10000a47c`..`0x10000a498` the ROM
    /// composes `0x20e020400`, reads it, writes back `w9 & ~0x04001000`, and then spins
    /// on `(w9 & 0xf0) == 0xf0`. It never writes bits 3:0 at this offset — the census
    /// records one write to the register in the whole run, and its value is `0` — so under
    /// `slot_ack` the status nibble reports `0` and the poll is not slow but dead. The
    /// status nibble here is a *state*, not an echo.
    ///
    /// The offsets are **device** offsets, which is not the same as the plane offset the
    /// cross-part evidence is stated in: the A8 maps the whole `0xe000000 + 0x40000` `pmgr`
    /// window as a single region, so plane `0x20000` + `0x400` is device `0x20400`.
    ///
    /// # What makes this a measurement rather than a guess
    ///
    /// The completion condition is read out of the image rather than chosen to make the
    /// ROM move, and the three instructions that state it -- `and w9, w9, #0xf0` /
    /// `cmp w9, #0xf0` / `b.ne`, preceded by a write of `w9 & ~0xFBFFEFFF` -- are Apple's,
    /// not the A8's.
    ///
    /// **19 of the 29 AP SecureROMs in `SecureROM/` carry it**, across ten parts from the A7
    /// to the A12: `s5l8960x` (A7), `s8000`/`s8001`/`s8003` (A9), `t7000` (A8), `t7001`
    /// (A8X), `t8010`/`t8011`/`t8012`/`t8015` (A10/A11/A12) and `t8020` (A12). The ten that
    /// do not are the A13 and later -- `t8027`, `t8030`, `t8101`, `t8103`, `t8301` -- which
    /// is the shape a block being replaced takes, not the shape a coincidence takes.
    ///
    /// The two instructions the A8 builds that mask with are
    /// `movn w10, #0x1000, lsl #16` then `movk w10, #0xfbff`, giving `0xFBFFEFFF`; the
    /// later parts spell the same value `movz w10, #0xefff, lsl #16` then
    /// `movk w10, #0xfbff`. Both spellings assemble to the same 32-bit constant, and both
    /// were checked against LLVM's own assembler rather than against this repository's
    /// disassembler, because the argument rests on them.
    ///
    /// | part | images | how the address is reached |
    /// |---|---|---|
    /// | A8 / T7000 | `t7000si` 1873, 1992 | composed directly; both compose `0x20e020400` |
    /// | A8X / T7001 | `t7001si` 1991 | composed directly; composes the same `0x20e020400` |
    /// | A12 / T8012 | `t8012si` 3401 | literal table at image VA `0x100018d08` names `0x20e080400` |
    /// | A12 / T8020 | `t8020si` 1.23, 4.6, 4.7 | literal table at `0x10001f6a8`, PMGR plane `0x23b080000` |
    ///
    /// So `plane + 0x400` is where three parts agree this register lives, and the service
    /// that drives it is Apple's and shared across ten parts.
    ///
    /// # What is not claimed
    ///
    /// No device is named. The A8 tree describes the containing window only as
    /// `/device-tree/arm-io/pmgr` `0xe000000 + 0x40000`, and two nodes claim the first
    /// `0x20000` of that plane -- `pmgr` and `mipi-dsim` -- so the tree cannot say what
    /// `+0x400` is either. (`mcc`, `amc` and `error-handler` are *not* among them: their
    /// `reg` cell is zero, which the offset turns into `0x200000000`, the DART and IO band
    /// 448 MiB above where the ROM is. Reading them as claimants is an easy and wrong
    /// inference, and `a8_platform.rs` pins the correct list.) The surrounding code programs 55 calibration words to
    /// `0x20e010000` under a bit-30 acknowledgement and then runs a divide-and-scale
    /// computation over eight channels at `0x20e000000 + n*0x1000`, which is the shape of
    /// memory training -- but that is an inference from context and is recorded as one, not
    /// as silicon. What is established is the register's *shape* and the firmware's own
    /// completion condition for it, which is exactly the basis `slot_ack` rests on too.
    pub status_nibble_all_ones: Option<Vec<u64>>,
    /// When true, a 32-bit write to plane offset 0 with bit 0 set causes subsequent
    /// 32-bit reads of offset 0 to return the written value with bit 31 (the sign bit)
    /// OR'd in. This models the T7000 ROM's go-wait loop at 0x10000a884..0x10000a890:
    /// `str w10, [x9]` (w10=1) / `ldr w10, [x9]` / `cmn w10, #0` / `b.ge` — it polls
    /// until the value becomes negative (sign bit set). The A8 tree's `pmgr` node does
    /// not declare this register; it is only visible in the ROM's code.
    ///
    /// Off by default; used only by the A8 plane-0 `Pmgr`.
    pub plane_base_go: bool,
    /// A set of plane offsets that implement a go/wait handshake: a write with
    /// `request_bit` set causes subsequent reads (of `read_size` bytes) to return the
    /// value with `ack_bit` also set, so the firmware's poll (`cmn`/`b.ge` for
    /// 32-bit sign-bit, or `ubfm`/`cbnz` for 64-bit bit 63) can exit.
    ///
    /// This models the T7000 ROM's measured go/wait loops:
    /// - Plane 0 (0x20e000000): offsets 0 and 4, 32-bit, request=bit 0, ack=bit 31
    ///   (0x10000a884..0x10000a890 and a second loop at offset 4 measured at ~500k instructions)
    /// - Plane 1 (0x202220000): offsets 0x20 and 0x50, 64-bit, request=bit 25, ack=bit 63
    ///   (0x10000a92c..0x10000a93c and 0x10000a99c..0x10000a9ac)
    ///
    /// Off by default; used only by the A8 plane-0 and plane-1 `Pmgr` instances.
    pub plane_go_wait: Option<Vec<(u64, u64, u64, usize)>>,
    /// Go/wait offsets written since their last acknowledged read.
    ///
    /// Set by any write to a claimed handshake offset, consumed by the next read
    /// of that offset. See the write path for why the write, rather than a bit
    /// in the written value, is the request.
    go_wait_armed: std::collections::BTreeMap<u64, bool>,
    /// Offsets whose handshake is armed by the *write* rather than by the
    /// request bit being set in the written value.
    ///
    /// Both conventions are firmware-measured and they disagree, so this is
    /// per-offset opt-in rather than one global rule:
    ///
    /// * At plane offset `0x0` the ROM writes `1` and polls the sign bit, and
    ///   writing `0` must leave the read non-negative -- the request bit *is*
    ///   the condition.
    /// * At plane offset `0x4004` the ROM stores `w9 & ~1`, clearing the
    ///   request bit, and still waits for the sign bit. A bit-conditioned model
    ///   never sees a request there, which is why the A8 SecureROM spun at
    ///   `PC=0x10000a3b4` until the step budget ran out.
    go_wait_write_armed: std::collections::BTreeSet<u64>,
}
impl Pmgr {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            clocks: 0,
            resets: 0,
            ps_ack: [0x1; PMGR_PS_COUNT], // all PS domains acknowledged at boot
            ps_req: [0; PMGR_PS_COUNT],
            clock_enable: [0; PMGR_CLOCK_COUNT],
            reset_ctrl: [0; PMGR_RESET_COUNT],
            devid: 0xA111_0000, // Apple PMGR device-id
            revid: 0x0000_0001,
            variant: 0x0000_0007,
            registers: std::collections::HashMap::new(),
            slot_ack: None,
            status_nibble_all_ones: None,
            plane_base_go: false,
            plane_go_wait: None,
            go_wait_armed: std::collections::BTreeMap::new(),
            go_wait_write_armed: std::collections::BTreeSet::new(),
        }
    }

    /// Report the request nibble back in the status nibble for 32-bit reads inside
    /// `first..=last`. See [`Pmgr::slot_ack`].
    pub fn with_slot_ack(mut self, first: u64, last: u64) -> Self {
        self.slot_ack = Some((first, last));
        self
    }

    /// Report `0xf` in the status nibble (bits 7:4) of the listed 32-bit offsets.
    /// See [`Pmgr::status_nibble_all_ones`].
    pub fn with_status_nibble_all_ones(mut self, offsets: Vec<u64>) -> Self {
        self.status_nibble_all_ones = Some(offsets);
        self
    }

    /// True when a configured status-nibble offset claims this one.
    ///
    /// Checked before `slot_ack`, because the two disagree about the same register and only
    /// one of them can be right: the T7000's slot helper wants the status nibble to *echo*
    /// the request nibble at `0x20000..=0x21008`, and its other service wants it to read
    /// `0xf` at `0x400`. A range rule and a per-offset rule that both matched would make
    /// the `0x400` poll unsatisfiable again, which is the state this replaced.
    pub fn claims_status_nibble(&self, o: u64) -> bool {
        self.status_nibble_all_ones
            .as_ref()
            .is_some_and(|offsets| offsets.contains(&o))
    }

    /// Enable the plane-base go/wait handshake at offset 0. See [`Pmgr::plane_base_go`].
    pub fn with_plane_base_go(mut self) -> Self {
        self.plane_base_go = true;
        self
    }

    /// Enable the go/wait handshake at specific offsets. Each tuple is
    /// `(offset, request_bit, ack_bit, read_size)`. See [`Pmgr::plane_go_wait`].
    /// Declare offsets whose go/wait handshake is armed by any matching write.
    /// See [`Pmgr::go_wait_write_armed`].
    pub fn with_write_armed_go_wait(mut self, offsets: Vec<u64>) -> Self {
        self.go_wait_write_armed = offsets.into_iter().collect();
        self
    }

    pub fn with_plane_go_wait(mut self, entries: Vec<(u64, u64, u64, usize)>) -> Self {
        self.plane_go_wait = Some(entries);
        self
    }
    /// Pre-set a register value that will be returned for reads at `offset`.
    pub fn with_register(mut self, offset: u64, value: u64) -> Self {
        self.registers.insert(offset, value);
        self
    }
    /// True when a configured plane go/wait entry claims this offset.
    ///
    /// Two real contracts meet at offsets `0x20` and `0x50`. The Phase 159 per-power-state
    /// map puts PS0 ack at `0x20` and PS4 req at `0x50`; the T7000 second PMGR plane uses
    /// 64-bit go/wait at exactly those two offsets. Both are measured, and a single `Pmgr`
    /// instance cannot be both at once, so the tie is broken by which plane is configured
    /// rather than by deleting the offsets from the legacy map unconditionally.
    ///
    /// That unconditional exclusion was a real defect rather than a style choice: it made
    /// PS0 ack unreadable on a *default* `Pmgr`, which has no go/wait entries and therefore
    /// no claim on the offset. `pmgr_ps_request_triggers_ack` in `tests/phase165.rs` reads
    /// `0x20` and expects the reset ack of 1; it read 0 instead, so the reset state set by
    /// `ps_ack = [0x1; PMGR_PS_COUNT]` was unreachable. An offset a go/wait entry claims
    /// belongs to the plane register file at every access size; an unclaimed offset keeps the
    /// legacy per-power-state meaning.
    pub fn plane_claims_offset(&self, o: u64) -> bool {
        self.plane_go_wait
            .as_ref()
            .is_some_and(|entries| entries.iter().any(|(offset, ..)| *offset == o))
    }
}
pub struct SepUart {
    pub id: DeviceId,
    pub tx: Vec<u8>,
    pub rx: Vec<u8>,
    pub console_output: bool,
    pub window_title: &'static str,
    pub flash_storage: Vec<u8>,
}
impl SepUart {
    pub fn new(id: DeviceId, title: &'static str) -> Self {
        Self {
            id,
            tx: Vec::new(),
            rx: Vec::new(),
            console_output: true,
            window_title: title,
            flash_storage: Vec::new(),
        }
    }
    pub fn set_console_output(&mut self, enabled: bool) {
        self.console_output = enabled;
    }
    pub fn output(&self) -> &[u8] {
        &self.tx
    }
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }
    pub fn inject_rx(&mut self, bytes: &[u8]) {
        self.rx.extend_from_slice(bytes);
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
}
impl Device for SepUart {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "sep-uart"
    }
    fn reset(&mut self) {
        self.tx.clear();
        self.rx.clear();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                let v = if self.rx.is_empty() {
                    0
                } else {
                    self.rx.remove(0)
                } as u64;
                DeviceResult::Value(v)
            }
            0x04 => DeviceResult::Value(self.tx.len() as u64),
            0x08 => DeviceResult::Value(self.rx.len() as u64),
            0x0C => DeviceResult::Value(if self.console_output { 1 } else { 0 }),
            _ => DeviceResult::Fault,
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.tx.push(v as u8);
                DeviceResult::Ok
            }
            0x0C => {
                self.console_output = v != 0;
                DeviceResult::Ok
            }
            _ => DeviceResult::Fault,
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct BasebandUart {
    pub id: DeviceId,
    pub tx: Vec<u8>,
    pub rx: Vec<u8>,
    pub console_output: bool,
    pub window_title: &'static str,
    pub flash_storage: Vec<u8>,
}
impl BasebandUart {
    pub fn new(id: DeviceId, title: &'static str) -> Self {
        Self {
            id,
            tx: Vec::new(),
            rx: Vec::new(),
            console_output: true,
            window_title: title,
            flash_storage: Vec::new(),
        }
    }
    pub fn set_console_output(&mut self, enabled: bool) {
        self.console_output = enabled;
    }
    pub fn output(&self) -> &[u8] {
        &self.tx
    }
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }
    pub fn inject_rx(&mut self, bytes: &[u8]) {
        self.rx.extend_from_slice(bytes);
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
}
impl Device for BasebandUart {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "bb-uart"
    }
    fn reset(&mut self) {
        self.tx.clear();
        self.rx.clear();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                let v = if self.rx.is_empty() {
                    0
                } else {
                    self.rx.remove(0)
                } as u64;
                DeviceResult::Value(v)
            }
            0x04 => DeviceResult::Value(self.tx.len() as u64),
            0x08 => DeviceResult::Value(self.rx.len() as u64),
            0x0C => DeviceResult::Value(if self.console_output { 1 } else { 0 }),
            _ => DeviceResult::Fault,
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.tx.push(v as u8);
                DeviceResult::Ok
            }
            0x0C => {
                self.console_output = v != 0;
                DeviceResult::Ok
            }
            _ => DeviceResult::Fault,
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
impl Device for Pmgr {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pmgr"
    }
    fn reset(&mut self) {
        self.clocks = 0;
        self.resets = 0;
        self.ps_ack = [0x1; PMGR_PS_COUNT];
        self.ps_req = [0; PMGR_PS_COUNT];
        self.clock_enable = [0; PMGR_CLOCK_COUNT];
        self.reset_ctrl = [0; PMGR_RESET_COUNT];
        self.registers.clear();
        self.go_wait_armed.clear();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        // Plane go/wait handshakes: a write with request_bit makes the ack_bit
        // read back as 1. Exact offsets/sizes/bits are firmware-measured.
        if let Some(entries) = &self.plane_go_wait {
            for (offset, req_bit, ack_bit, rd_size) in entries {
                if *offset == o && *rd_size == s {
                    // The backing store depends on the offset: legacy fields for 0/8,
                    // HashMap for everything else.
                    let v = match o {
                        0x00 => self.clocks,
                        0x08 => self.resets,
                        _ => *self.registers.get(&o).unwrap_or(&0),
                    };
                    // Either the stored request bit is set, or a write has
                    // happened since the last read. The second condition is what
                    // the A8 0x4004 sequence needs: it writes bit 0 *cleared*.
                    // Two rules, and which one applies depends on the register:
                    // a write-armed offset is driven purely by the *direction* its
                    // request bit last moved, while every other claimed offset
                    // keeps the older "the stored request bit is set" rule. Letting
                    // the older rule also apply to a write-armed offset is what
                    // pinned the A8 ROM in the `b.ge` loop: the stored bit 0 stayed
                    // set, so every poll re-acknowledged "high" forever.
                    let armed = self.go_wait_armed.get(&o).copied();
                    if armed.is_some()
                        || (!self.go_wait_write_armed.contains(&o) && (v & req_bit) != 0)
                    {
                        // The status bit belongs to the hardware, not to the
                        // guest: the ROM's own write of `0x80000000` to this
                        // offset must not read back as a permanent "still
                        // busy" for a `b.lt` poll, which is exactly what
                        // happened and left the A8 ROM spinning at
                        // `PC=0x10000a3b4` for three million instructions.
                        // Read the polarity *before* consuming the arm, or every
                        // acknowledgement silently falls back to "sets".
                        let sets = armed.unwrap_or(true);
                        let ret = if sets { v | ack_bit } else { v & !ack_bit };
                        self.go_wait_armed.remove(&o);
                        if pmgr_access_trace() {
                            eprintln!(
                                "PMGR go/wait read: o={o:#x} stored={v:#x} \
                                 req_bit={req_bit:#x} -> {ret:#x} ack_bit={ack_bit:#x} \
                                 (ack {} bit)",
                                if sets { "sets" } else { "clears" }
                            );
                        }
                        return DeviceResult::Value(ret);
                    }
                    if pmgr_access_trace() {
                        eprintln!(
                            "PMGR go/wait read: o={o:#x} stored={v:#x} no request, \
                             returning it verbatim (ack withheld)"
                        );
                    }
                }
            }
        }
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            // Legacy Phase 159 register window. Offset 0 also carries the plane-base
            // go/wait handshake when enabled: a write with bit 0 set makes the sign
            // bit read back as 1, so the ROM's `cmn`/`b.ge` loop can exit.
            0x00 => {
                if self.plane_base_go && s == 4 && (self.clocks & 1) != 0 {
                    DeviceResult::Value(self.clocks | 0x8000_0000)
                } else {
                    DeviceResult::Value(self.clocks)
                }
            }
            0x08 => DeviceResult::Value(self.resets),
            // Identification.
            0x10 => DeviceResult::Value(self.devid as u64),
            0x14 => DeviceResult::Value(self.revid as u64),
            0x18 => DeviceResult::Value(self.variant as u64),
            // Per-power-state ack: 0x20 + 4*idx (read-only).
            // An offset a configured go/wait plane claims belongs to the plane register file
            // instead, which is what the T7000 second plane's 64-bit go/wait at 0x20 needs.
            o if s == 4
                && (0x20..0x40).contains(&o)
                && o % 4 == 0
                && !self.plane_claims_offset(o) =>
            {
                let idx = ((o - 0x20) / 4) as usize;
                DeviceResult::Value(self.ps_ack[idx] as u64)
            }
            // Per-power-state request: 0x40 + 4*idx, under the same claimed-offset rule.
            o if s == 4
                && (0x40..0x60).contains(&o)
                && o % 4 == 0
                && !self.plane_claims_offset(o) =>
            {
                let idx = ((o - 0x40) / 4) as usize;
                DeviceResult::Value(self.ps_req[idx] as u64)
            }
            // Per-clock enable: 0x60 + 4*idx.
            o if s == 4 && (0x60..0xA0).contains(&o) && o % 4 == 0 => {
                let idx = ((o - 0x60) / 4) as usize;
                DeviceResult::Value(self.clock_enable[idx] as u64)
            }
            // Per-reset-rail control: 0xA0 + 4*idx.
            o if s == 4 && (0xA0..0xE0).contains(&o) && o % 4 == 0 => {
                let idx = ((o - 0xA0) / 4) as usize;
                DeviceResult::Value(self.reset_ctrl[idx] as u64)
            }
            _ => {
                let v = *self.registers.get(&o).unwrap_or(&0);
                match self.slot_ack {
                    // 32-bit only: the ROM's helper is `ldr w9` / `str w9`, and an
                    // 8-byte read of the same slot is a different access that nothing
                    // in the image has been measured to perform.
                    Some((first, last))
                        if s == 4
                            && (first..=last).contains(&o)
                            && !self.claims_status_nibble(o) =>
                    {
                        let req = v & 0xf;
                        DeviceResult::Value((v & !0xf0) | (req << 4))
                    }
                    _ => {
                        // The status nibble belongs to the hardware, not to the guest: the
                        // stored value is masked so no write can forge it, and only the
                        // 32-bit access the firmware has been measured to perform is
                        // rewritten.
                        if s == 4 && self.claims_status_nibble(o) {
                            return DeviceResult::Value(((v as u32 & !0xf0) as u64) | 0xf0);
                        }
                        DeviceResult::Value(v)
                    }
                }
            }
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        // Arm a go/wait handshake on the *write*, with the ack polarity taken
        // from the direction the request bit moved. See the comment inside for
        // why neither a fixed polarity nor "armed when the request bit is set"
        // works: the A8 ROM drives one register from two loops that want
        // opposite things.
        if self.go_wait_write_armed.contains(&o) {
            if let Some(entries) = self.plane_go_wait.clone() {
                for (offset, req_bit, ack_bit, rd_size) in entries {
                    if offset == o && rd_size == s {
                        // The status bit's polarity follows the *direction* the
                        // request bit moved. That is the only rule consistent
                        // with both loops the A8 ROM runs against this one
                        // register:
                        //
                        //   0x10000a3ac  and  w9, w9, #0xfffffffe  ; bit 0 high -> low
                        //   0x10000a3bc  b.lt                        ; wants bit 31 CLEAR
                        //
                        //   0x10000a880  orr  w10, w10, #0x1        ; bit 0 low -> high
                        //   0x10000a890  b.ge                        ; wants bit 31 SET
                        //
                        // A single fixed polarity satisfies one of those loops and
                        // spins the other forever, and both were observed doing
                        // exactly that before this rule existed: with the ack bit
                        // always set the ROM parked at 0x10000a3b4, and with it
                        // always clear it parks in the other loop instead.
                        let previous = *self.registers.get(&o).unwrap_or(&0);
                        let was_set = (previous & req_bit) != 0;
                        let now_set = (v & req_bit) != 0;
                        if was_set != now_set {
                            self.go_wait_armed.insert(o, now_set);
                        }
                        // Store the guest's command field without the
                        // hardware-owned status bit, so a value the guest wrote
                        // there cannot masquerade as device state.
                        let masked = if s == 4 {
                            (v as u32 & !(ack_bit as u32)) as u64
                        } else {
                            v & !ack_bit
                        };
                        self.registers.insert(o, masked);
                        // Stop here: the catch-all arm below would store the
                        // guest's raw value again and put the status bit back.
                        return DeviceResult::Ok;
                    }
                }
            }
        }
        match o {
            // Legacy Phase 159 register window.
            0x00 => self.clocks = v,
            0x08 => self.resets = v,
            // Identification is RO.
            0x10 | 0x14 | 0x18 => return DeviceResult::Ok,
            // PS ack is RO, under the same claimed-offset rule as the read path.
            o if s == 4
                && (0x20..0x40).contains(&o)
                && o % 4 == 0
                && !self.plane_claims_offset(o) =>
            {
                return DeviceResult::Ok
            }
            // PS request: 0x40 + 4*idx, under the same claimed-offset rule.
            o if s == 4
                && (0x40..0x60).contains(&o)
                && o % 4 == 0
                && !self.plane_claims_offset(o) =>
            {
                let idx = ((o - 0x40) / 4) as usize;
                let req = (v as u32) & 0x3;
                self.ps_req[idx] = req;
                // Auto-ack: if requested state is 0 (off) or 1 (on), the
                // ack bit is set on the next write to mirror real PMGR
                // behavior (ack follows req after one cycle).
                if req <= 1 {
                    self.ps_ack[idx] = req;
                }
            }
            // Per-clock enable: 0x60 + 4*idx.
            o if s == 4 && (0x60..0xA0).contains(&o) && o % 4 == 0 => {
                let idx = ((o - 0x60) / 4) as usize;
                self.clock_enable[idx] = v as u32;
            }
            // Per-reset-rail control: 0xA0 + 4*idx.
            o if s == 4 && (0xA0..0xE0).contains(&o) && o % 4 == 0 => {
                let idx = ((o - 0xA0) / 4) as usize;
                self.reset_ctrl[idx] = v as u32;
            }
            _ => {
                // d321 iBoot uses bit 25 as the command/busy strobe at these
                // PMGR offsets and polls until hardware clears it. Complete
                // the functional command synchronously; preserve all payload
                // bits and do not assert any unrelated readiness state.
                let mut stored = if matches!(o, 0x14000 | 0x2c000 | 0x30000) {
                    v & !(1u64 << 25)
                } else {
                    v
                };
                // A hardware-owned status nibble is not storable. Without this the guest
                // could write `0xf0` into a claimed offset and then read back the value it
                // wrote, which would satisfy the T7000's `(w & 0xf0) == 0xf0` poll without
                // the device having done anything -- the read path would report the same
                // `0xf0` whether or not it had, so the poll would pass either way and the
                // rule would be untestable from the outside.
                if s == 4 && self.claims_status_nibble(o) {
                    stored = (stored as u32 & !0xf0) as u64;
                }
                // Per-access tracing, off by default. This arm is the catch-all
                // for every PMGR offset without a dedicated behaviour, so an
                // unconditional print fired on each of the thousands of writes a
                // real boot makes and buried the run's own output. Opt in with
                // IPHEM_TRACE_PMGR_ACCESS=1, matching the other IPHEM_TRACE_*
                // diagnostics. The flag is read once: `env::var_os` per access
                // would itself be a measurable cost in this path.
                if pmgr_access_trace() {
                    eprintln!("PMGR write: o={o:#x} v={stored:#x}");
                }
                self.registers.insert(o, stored);
            }
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod pmgr_register_file_tests {
    use super::*;

    /// The T7000 slot helper, 0x10000aa94..0x10000aae0, instruction for instruction:
    /// `ldr w9,[x8]` / `orr w9,w9,#0xf` / `str w9,[x8]` to make the request, then
    /// `ldr w9,[x8]` / `ldr w10,[x8]` / `eor w9,w9,w10 lsr #4` / `and w9,w9,#0xf` /
    /// `cbnz w9` as the completion test. Returns the value the `cbnz` branches on, so a
    /// register the model cannot satisfy is visible as a non-zero result rather than as a
    /// claim that the poll finished.
    fn t7000_slot_set_then_poll(pmgr: &mut Pmgr, offset: u64) -> u64 {
        assert_eq!(pmgr.read(offset, 4), DeviceResult::Value(0));
        let requested = match pmgr.read(offset, 4) {
            DeviceResult::Value(v) => v | 0xf,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(pmgr.write(offset, 4, requested), DeviceResult::Ok);
        let (a, b) = match (pmgr.read(offset, 4), pmgr.read(offset, 4)) {
            (DeviceResult::Value(a), DeviceResult::Value(b)) => (a as u32, b as u32),
            _ => panic!("the helper reads the slot twice, 32 bits at a time"),
        };
        ((a ^ (b >> 4)) & 0xf) as u64
    }

    #[test]
    fn t7000_slot_poll_completes_only_because_the_status_nibble_reports_the_request() {
        // The negative control first: without the modelled status nibble the firmware's own
        // completion test can never be satisfied, which is what made the measured run a dead
        // poll rather than a slow one. If this ever passes, the control has stopped meaning
        // anything and the positive case below is no longer evidence of anything.
        let mut inert = Pmgr::new(89);
        assert_ne!(t7000_slot_set_then_poll(&mut inert, 0x201f8), 0);

        let mut pmgr = Pmgr::new(89).with_slot_ack(0x20000, 0x21008);
        assert_eq!(t7000_slot_set_then_poll(&mut pmgr, 0x201f8), 0);
    }

    #[test]
    fn slot_ack_is_confined_to_its_range_to_32_bit_reads_and_preserves_the_rest_of_the_word() {
        let mut pmgr = Pmgr::new(89).with_slot_ack(0x20000, 0x21008);
        // The helper's own bound: 513 slots of 8 bytes from plane offset 0x20000, so 0x21008
        // is the last one. 0x201f8 is slot 0x3f -- the slot the ROM is measured polling -- and
        // 0x21000 is slot 0x200, which the A8 tree gives a ps-reg reset value.
        for offset in [0x20000, 0x200f8, 0x20100, 0x201f8, 0x21000, 0x21008] {
            assert_eq!(pmgr.write(offset, 4, 0x0f), DeviceResult::Ok);
            // 0xff, not 0x0f: the request comes back in bits 3:0 *and* in the status nibble.
            assert_eq!(pmgr.read(offset, 4), DeviceResult::Value(0xff));
        }
        // One slot past the declared range, and an offset before it: both inert.
        for offset in [0x21010, 0x21018, 0x1fff8, 0x23000] {
            assert_eq!(pmgr.write(offset, 4, 0x0f), DeviceResult::Ok);
            assert_eq!(pmgr.read(offset, 4), DeviceResult::Value(0x0f));
        }
        // Only the status nibble is modelled. A request of 0xf means "bits 3:0 are 0xf", so
        // bits 8..31 and bit 4 must come back as written, with bits 7:4 == bits 3:0.
        assert_eq!(pmgr.write(0x201f8, 4, 0x1234_a00f), DeviceResult::Ok);
        // Read returns request (0xf) in both nibbles: low 0xf, status 0xf0 = 0xff,
        // preserving the original 0xa0 in bits 8..15.
        assert_eq!(pmgr.read(0x201f8, 4), DeviceResult::Value(0x1234_a0ff));
        // An 8-byte read is a different access, and nothing in the image has been measured to
        // make one here, so it is not modelled.
        assert_eq!(pmgr.read(0x201f8, 8), DeviceResult::Value(0x1234_a00f));
        // A cleared request reads back cleared, so the helper's other path also terminates.
        assert_eq!(pmgr.write(0x201f8, 4, 0x1234_af0f & !0xf), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x201f8, 4), DeviceResult::Value(0x1234_af00));
        // The mode is configuration, not state: a bus reset drops the guest's words and the
        // request then reads back as zero in both nibbles.
        pmgr.reset();
        assert_eq!(pmgr.read(0x201f8, 4), DeviceResult::Value(0));
        assert_eq!(t7000_slot_set_then_poll(&mut pmgr, 0x201f8), 0);
    }

    #[test]
    fn plane_base_go_completes_the_go_wait_loop_at_offset_zero() {
        // The negative control: without the modelled sign-bit mirror, the ROM's poll
        // of `cmn`/`b.ge` at 0x10000a888..0x10000a890 can never exit.
        let mut inert = Pmgr::new(89);
        assert_eq!(inert.write(0x0, 4, 1), DeviceResult::Ok);
        assert_eq!(inert.read(0x0, 4), DeviceResult::Value(1)); // positive -> loop forever

        let mut pmgr = Pmgr::new(89).with_plane_go_wait(vec![(0x0, 1u64 << 0, 1u64 << 31, 4)]);
        assert_eq!(pmgr.write(0x0, 4, 1), DeviceResult::Ok);
        // Read returns the written value with bit 31 set (negative), so cmn/b.ge exits.
        assert_eq!(pmgr.read(0x0, 4), DeviceResult::Value(0x8000_0001));
        // Writing 0 clears the request; reads return 0 (not negative).
        assert_eq!(pmgr.write(0x0, 4, 0), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x0, 4), DeviceResult::Value(0));
        // The flag is configuration: a bus reset preserves the mode, clears the register.
        pmgr.reset();
        assert_eq!(pmgr.read(0x0, 4), DeviceResult::Value(0));
        assert_eq!(pmgr.write(0x0, 4, 1), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x0, 4), DeviceResult::Value(0x8000_0001));
    }

    /// The directional rule, asserted from both ends.
    ///
    /// One register, two loops, opposite requirements. Setting bit 0 must
    /// acknowledge with bit 31 **set** (the `b.ge` poll at 0x10000a890); clearing
    /// it must acknowledge with bit 31 **clear** (the `b.lt` poll at
    /// 0x10000a3bc). A model with one fixed polarity satisfies one loop and spins
    /// the other, and both failures were observed before this rule existed.
    #[test]
    fn go_wait_ack_polarity_follows_the_direction_of_the_request_bit() {
        let mut pmgr = Pmgr::new(89)
            .with_plane_go_wait(vec![(0x4004, 1u64 << 0, 1u64 << 31, 4)])
            .with_write_armed_go_wait(vec![0x4004]);
        const SET: u64 = 1u64 << 31;

        // Low -> high: acknowledge by setting the status bit.
        assert_eq!(pmgr.write(0x4004, 4, 1), DeviceResult::Ok);
        match pmgr.read(0x4004, 4) {
            DeviceResult::Value(v) => assert_ne!(v & SET, 0, "bit 0 rising acknowledges high"),
            other => panic!("expected a value, got {other:?}"),
        }

        // High -> low: acknowledge by clearing it. This is the `b.lt` loop.
        assert_eq!(pmgr.write(0x4004, 4, 0), DeviceResult::Ok);
        match pmgr.read(0x4004, 4) {
            DeviceResult::Value(v) => assert_eq!(v & SET, 0, "bit 0 falling acknowledges low"),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    /// The status bit is hardware-owned, so the guest's own value for it must not
    /// survive in the stored register. The ROM writes `0x80000000` into this word
    /// itself, and a model that echoes it back holds the `b.ge` loop forever.
    #[test]
    fn a_handshake_register_never_stores_the_guests_status_bit() {
        let mut pmgr = Pmgr::new(89)
            .with_plane_go_wait(vec![(0x4004, 1u64 << 0, 1u64 << 31, 4)])
            .with_write_armed_go_wait(vec![0x4004]);
        assert_eq!(pmgr.write(0x4004, 4, 0x8000_0001), DeviceResult::Ok);
        assert_eq!(
            pmgr.registers.get(&0x4004).copied().unwrap_or(0) & (1u64 << 31),
            0,
            "the hardware-owned status bit must not be stored"
        );
        assert_eq!(
            pmgr.registers.get(&0x4004).copied().unwrap_or(0) & 1,
            1,
            "the command field is stored verbatim"
        );
    }

    /// A write that does not move the request bit is not a request, so it must not
    /// arm a handshake. This is the negative control for the rule above: arming
    /// on every write is what the first attempt did, and it hangs the `b.lt` loop
    /// by acknowledging in the wrong direction.
    #[test]
    fn a_write_that_does_not_move_the_request_bit_does_not_acknowledge() {
        let mut pmgr = Pmgr::new(89)
            .with_plane_go_wait(vec![(0x4004, 1u64 << 0, 1u64 << 31, 4)])
            .with_write_armed_go_wait(vec![0x4004]);
        // The first write takes bit 0 from its reset value of 0 up to 1, which
        // *is* a transition, so it legitimately acknowledges high.
        assert_eq!(pmgr.write(0x4004, 4, 1), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x4004, 4), DeviceResult::Value(0x8000_0001));
        // The second write repeats the same value: no transition, so no request.
        assert_eq!(pmgr.write(0x4004, 4, 1), DeviceResult::Ok);
        match pmgr.read(0x4004, 4) {
            DeviceResult::Value(v) => assert_eq!(
                v & (1u64 << 31),
                0,
                "an unchanged request bit must not manufacture an acknowledgement"
            ),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    #[test]
    fn plane_go_wait_64bit_completes_the_second_plane_go_wait_loops() {
        // The T7000 ROM's second PMGR plane (0x202220000) uses 64-bit go/wait at
        // offsets 0x20 and 0x50: write bit 25 (0x2000000), poll until bit 63 is set.
        // Negative control: without the model the `ubfm`/`cbnz` poll never exits.
        let mut inert = Pmgr::new(90);
        assert_eq!(inert.write(0x20, 8, 1u64 << 25), DeviceResult::Ok);
        assert_eq!(inert.read(0x20, 8), DeviceResult::Value(1u64 << 25)); // bit 63 clear -> loop forever

        let mut pmgr = Pmgr::new(90).with_plane_go_wait(vec![
            (0x20, 1u64 << 25, 1u64 << 63, 8),
            (0x50, 1u64 << 25, 1u64 << 63, 8),
        ]);
        assert_eq!(pmgr.write(0x20, 8, 1u64 << 25), DeviceResult::Ok);
        // Read returns the written value with bit 63 set, so ubfm/cbnz exits.
        let expected = (1u64 << 25) | (1u64 << 63);

        assert_eq!(pmgr.read(0x20, 8), DeviceResult::Value(expected));
        // Offset 0x50 works the same way.
        assert_eq!(pmgr.write(0x50, 8, 1u64 << 25), DeviceResult::Ok);
        assert_eq!(
            pmgr.read(0x50, 8),
            DeviceResult::Value((1u64 << 25) | (1u64 << 63))
        );
        // Other offsets and sizes are unaffected.
        assert_eq!(pmgr.write(0x20, 4, 1u64 << 25), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x20, 4), DeviceResult::Value(1u64 << 25));
        // Configuration survives reset; registers are cleared.
        pmgr.reset();
        assert_eq!(pmgr.read(0x20, 8), DeviceResult::Value(0));
        assert_eq!(pmgr.write(0x20, 8, 1u64 << 25), DeviceResult::Ok);
        assert_eq!(
            pmgr.read(0x20, 8),
            DeviceResult::Value((1u64 << 25) | (1u64 << 63))
        );
    }

    #[test]
    fn a_go_wait_plane_claim_is_what_reserves_offset_20_not_the_offset_number_itself() {
        // The two contracts meet at 0x20 and 0x50, and each of them needs to survive:
        //
        //   * a default `Pmgr` has no go/wait entries, so 0x20 is PS0 ack and must report the
        //     reset ack. This is what `tests/phase165.rs::pmgr_ps_request_triggers_ack`
        //     asserts, and it was reading 0 because the offset was excluded from the
        //     per-power-state map unconditionally.
        //   * a `Pmgr` configured with a go/wait entry at 0x20 owns that offset at every
        //     access size, so the plane register file answers instead. This is what
        //     `plane_go_wait_64bit_completes_the_second_plane_go_wait_loops` asserts for a
        //     4-byte read.
        //
        // The claim is what reserves the offset. Keying the reservation on the *offset
        // number* satisfies neither reading honestly: it can only be the second one, and
        // only by making PS0 ack unreadable everywhere.
        let mut default_plane = Pmgr::new(1);
        assert!(!default_plane.plane_claims_offset(0x20));
        assert_eq!(default_plane.read(0x20, 4), DeviceResult::Value(1));
        assert_eq!(default_plane.read(0x24, 4), DeviceResult::Value(1));
        assert_eq!(default_plane.read(0x40, 4), DeviceResult::Value(0));
        assert_eq!(default_plane.write(0x40, 4, 0x1), DeviceResult::Ok);
        assert_eq!(default_plane.read(0x20, 4), DeviceResult::Value(1));
        assert_eq!(default_plane.read(0x40, 4), DeviceResult::Value(0x1));

        // Claiming 0x20 moves the whole offset onto the plane register file.
        let mut claimed = Pmgr::new(1).with_plane_go_wait(vec![(0x20, 1u64 << 25, 1u64 << 63, 8)]);
        assert!(claimed.plane_claims_offset(0x20));
        assert!(!claimed.plane_claims_offset(0x50));
        // 0x50 keeps the legacy per-power-state meaning when nothing claims it: PS4 req.
        assert_eq!(claimed.read(0x50, 4), DeviceResult::Value(0));
        assert_eq!(claimed.write(0x50, 4, 0x3), DeviceResult::Ok);
        assert_eq!(claimed.read(0x50, 4), DeviceResult::Value(0x3));
        // And 0x20 answers from the plane file rather than from the ack.
        assert_eq!(claimed.write(0x20, 4, 0x2), DeviceResult::Ok);
        assert_eq!(claimed.read(0x20, 4), DeviceResult::Value(0x2));
    }

    #[test]
    fn unmodeled_pmgr_registers_preserve_guest_writes_without_ready_defaults() {
        let mut pmgr = Pmgr::new(75);
        assert_eq!(pmgr.read(0x98000, 4), DeviceResult::Value(0));
        assert_eq!(pmgr.write(0x98000, 4, 0x80), DeviceResult::Ok);
        assert_eq!(pmgr.read(0x98000, 4), DeviceResult::Value(0x80));
        assert_eq!(pmgr.read(0xa0000, 4), DeviceResult::Value(0));
        pmgr.reset();
        assert_eq!(pmgr.read(0x98000, 4), DeviceResult::Value(0));
    }

    #[test]
    fn d321_command_register_clears_busy_strobe_and_preserves_payload() {
        let mut pmgr = Pmgr::new(75);
        let command = 0x020a_f014;
        assert_eq!(pmgr.write(0x30000, 4, command), DeviceResult::Ok);
        assert_eq!(
            pmgr.read(0x30000, 4),
            DeviceResult::Value(command & !(1u64 << 25))
        );
    }
}

/// Minimal Apple SEP boot-control/status block used by early SEPROM bring-up.
///
/// Cebu SEPROM touches `0x242140118`, sets control bit 2, then polls bit 63
/// until hardware reports the SEP fabric is ready. Returning a plain register
/// mirror leaves the ROM in that first wait loop forever, so this model keeps
/// guest-written bits but auto-asserts the hardware-ready bit on reads.
pub struct SepBootCtrl {
    pub id: DeviceId,
    regs: std::collections::BTreeMap<u64, u64>,
    ready: bool,
}

impl SepBootCtrl {
    pub const READY_BIT: u64 = 1u64 << 63;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            regs: std::collections::BTreeMap::new(),
            ready: true,
        }
    }

    pub fn new_with_ready(id: DeviceId, ready: bool) -> Self {
        Self {
            id,
            regs: std::collections::BTreeMap::new(),
            ready,
        }
    }
}

/// The d321ap DeviceTree's `/arm-io/aes` node (`aes-version = 3`).
///
/// Every register semantic here is derived from Apple's own consumer: the
/// SecureROM AES service, whose status routine (`0x1000026a0` in the t8020
/// build) is compiled byte-identically into the t8020, t8027 and t8030
/// SecureROMs. That driver touches five registers, and none of the ones in
/// circulation as an `apple,aes` map.
///
/// | Offset  | Access the driver performs                                        |
/// |---------|-------------------------------------------------------------------|
/// | `0x00`  | write, configuration (the ROM's builders set bits 31/29/28/27/26)  |
/// | `0x04`  | write, configuration (`0x80000000`/`0x88000000` \| `0x04000000`)   |
/// | `0x08`  | write bit 0 to start an operation, bit 1 on exit                   |
/// | `0x0c`  | read status: bits 12..7 required set, bit 13 returned               |
/// | `0x18`  | read bit 5 for completion, write `0x20` to clear it                |
/// | `0x24`  | read and discard                                                  |
/// | `0x200` | write, word-wide data-port push (20 words per operation)            |
///
/// The circulated map -- `AES_CTRL` at 0x00 with enable/decrypt/keysize/start
/// in bits 0..4, keys at 0x10-0x2c, IV at 0x40-0x4c, `FIFO_IN` 0x80,
/// `FIFO_OUT` 0x90, "under a compatible like `apple,aes`" -- is **refuted by
/// primary evidence**: the d321ap node has neither `compatible` nor
/// `interrupts` properties, the driver touches none of those offsets, and the
/// ROM's control builders set bits 31/29/28/27/26 rather than anything in bits
/// 0..4, so there is no enable/decrypt/keysize/start pattern to find.
///
/// There is deliberately **no key, IV or output register, and no cipher
/// datapath**. Apple's Platform Security documentation states that this class of
/// engine receives its keying material over a channel the Application Processor
/// cannot read, and the driver corroborates it: it pushes words in and never
/// reads a result back. An invented `FIFO_OUT` would be a register the firmware
/// has no use for, fed by a key the firmware cannot supply.
pub struct AesOtp {
    pub id: DeviceId,
    /// Words the guest has pushed to the `+0x200` data port, bounded so a
    /// looping guest cannot grow it without limit.
    pushed: Vec<u32>,
    /// Guest-driven completion flag (`+0x18` bit 5), cleared by the driver's own
    /// write-one-to-clear.
    done: bool,
    /// Last value written to each configuration register, retained so the
    /// driver's programming stays observable without giving a register a
    /// meaning the firmware does not give it.
    cfg: std::collections::BTreeMap<u64, u64>,
}

impl AesOtp {
    /// `+0x00`, written with configuration.
    pub const CONFIG_0: u64 = 0x00;
    /// `+0x04`, written with configuration.
    pub const CONFIG_1: u64 = 0x04;
    /// `+0x08`: bit 0 starts an operation, bit 1 is written on exit.
    pub const CONTROL: u64 = 0x08;
    /// `+0x0c`: the status word, and the driver's gate. Read-only.
    pub const STATUS: u64 = 0x0c;
    /// `+0x18`: the completion flag the driver polls (bit 5) and clears by
    /// writing `0x20`.
    pub const STATUS2: u64 = 0x18;
    /// `+0x24`: read by the driver and discarded.
    pub const FLUSH: u64 = 0x24;
    /// `+0x200`: the word-wide data port every pushed word goes to.
    pub const DATA: u64 = 0x200;
    /// `+0x20`: AES completion status read by the A11 ROM (reads as done flag).
    pub const COMPLETION: u64 = 0x20;

    /// Write-one-to-clear bit in `+0x18`: "operation complete".
    pub const DONE_BIT: u32 = 0x20;
    /// Bits 12..8 of `+0x0c`, each of which the driver rejects when clear.
    pub const CAPABILITY: u32 = 0x1f00;
    /// Bit 7 of `+0x0c`, which the driver requires **set**: its `tbnz x9,#7`
    /// is the branch that reaches the return.
    pub const READY_BIT: u32 = 0x0080;
    /// Bit 13 of `+0x0c`, which the driver returns (`ubfm w0, w8, #13, #13`)
    /// and which its only two callers branch on as the pass condition.
    pub const RETURNED_BIT: u32 = 0x2000;

    /// Bits 12..7, the field the driver requires.
    const DRIVER_REQUIRED: u32 = Self::CAPABILITY | Self::READY_BIT;

    /// Upper bound on retained pushed words; the driver pushes 20 per
    /// operation.
    const MAX_PUSHED: usize = 4096;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            pushed: Vec::new(),
            done: false,
            cfg: std::collections::BTreeMap::new(),
        }
    }

    /// The status word as the driver reads it at `+0x0c`: the required field
    /// plus the bit it returns, and nothing else.
    pub fn status(&self) -> u32 {
        Self::DRIVER_REQUIRED | Self::RETURNED_BIT
    }

    /// Whether the status word satisfies the driver's own condition.
    pub fn reports_identity(&self) -> bool {
        self.status() & Self::DRIVER_REQUIRED == Self::DRIVER_REQUIRED
    }

    /// Whether the `+0x18` completion flag is set.
    pub fn operation_complete(&self) -> bool {
        self.done
    }

    /// The words the guest has pushed to the data port.
    pub fn pushed_words(&self) -> &[u32] {
        &self.pushed
    }
}

impl Device for AesOtp {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "aes"
    }

    fn reset(&mut self) {
        self.pushed.clear();
        self.done = false;
        self.cfg.clear();
    }

    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            Self::STATUS => DeviceResult::Value(u64::from(self.status())),
            Self::STATUS2 => DeviceResult::Value(if self.done {
                u64::from(Self::DONE_BIT)
            } else {
                0
            }),
            Self::COMPLETION => DeviceResult::Value(if self.done {
                u64::from(Self::DONE_BIT)
            } else {
                0
            }),
            // The driver discards this word, so no value for it is evidenced
            // and the model reports the only one it can justify.
            Self::FLUSH => DeviceResult::Value(0),
            // The driver never reads the data port back, so a value here would
            // be invented.
            Self::DATA => DeviceResult::Value(0),
            _ => DeviceResult::Value(*self.cfg.get(&o).unwrap_or(&0)),
        }
    }

    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let value = v & match s {
            1 => 0xff,
            2 => 0xffff,
            4 => 0xffff_ffff,
            _ => u64::MAX,
        };
        match o {
            Self::CONTROL => {
                if value & 0x1 != 0 {
                    self.done = true;
                }
                self.cfg.insert(o, value);
            }
            // The status word is read-only: a guest write is accepted so a
            // read-modify-write cannot fault, and cannot create the gate.
            Self::STATUS => {}
            Self::STATUS2 => {
                if value & u64::from(Self::DONE_BIT) != 0 {
                    self.done = false;
                }
            }
            Self::DATA => {
                if self.pushed.len() < Self::MAX_PUSHED {
                    self.pushed.push(value as u32);
                }
            }
            _ => {
                self.cfg.insert(o, value);
            }
        }
        DeviceResult::Ok
    }

    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Device for SepBootCtrl {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "sep-boot-ctrl"
    }

    fn reset(&mut self) {
        self.regs.clear();
    }

    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mut value = self.regs.get(&o).copied().unwrap_or(0);
        if o == 0x108 && self.ready {
            value |= Self::READY_BIT;
        }
        DeviceResult::Value(value)
    }

    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mask = match s {
            1 => 0xff,
            2 => 0xffff,
            4 => 0xffff_ffff,
            _ => u64::MAX,
        };
        self.regs.insert(o, v & mask);
        DeviceResult::Ok
    }

    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Minimal Apple SEP identity/status block used by Cebu SEPROM.
///
/// During early bring-up the ROM reads `0x23d2e8000`, masks the low 16 bits,
/// and accepts known SoC identity values. A generic zero/one fallback sends it
/// into the diagnostic error path, so expose the expected low identity while
/// still preserving any guest writes to the rest of the register word.
pub struct SepIdentity {
    pub id: DeviceId,
    chip_id_low16: u16,
    regs: std::collections::BTreeMap<u64, u64>,
}

impl SepIdentity {
    pub fn new(id: DeviceId, chip_id_low16: u16) -> Self {
        Self {
            id,
            chip_id_low16,
            regs: std::collections::BTreeMap::new(),
        }
    }
}

impl Device for SepIdentity {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "sep-identity"
    }

    fn reset(&mut self) {
        self.regs.clear();
    }

    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mut value = self.regs.get(&o).copied().unwrap_or(0);
        if o == 0 {
            value = (value & !0xffff) | u64::from(self.chip_id_low16);
        }
        DeviceResult::Value(value)
    }

    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mask = match s {
            1 => 0xff,
            2 => 0xffff,
            4 => 0xffff_ffff,
            _ => u64::MAX,
        };
        self.regs.insert(o, v & mask);
        DeviceResult::Ok
    }

    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Minimal SEP init/status block used by Cebu SEPROM.
///
/// After writing diagnostic/mailbox metadata, Cebu SEPROM polls
/// `0x241180010` until bit 0 becomes set. This represents another early
/// hardware-ready gate; expose that bit while preserving guest-written state.
pub struct SepReadyStatus {
    pub id: DeviceId,
    regs: std::collections::BTreeMap<u64, u64>,
}

impl SepReadyStatus {
    pub const READY_BIT: u64 = 1;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            regs: std::collections::BTreeMap::new(),
        }
    }
}

impl Device for SepReadyStatus {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "sep-ready-status"
    }

    fn reset(&mut self) {
        self.regs.clear();
    }

    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mut value = self.regs.get(&o).copied().unwrap_or(0);
        if o == 0x10 {
            value |= Self::READY_BIT;
        }
        DeviceResult::Value(value)
    }

    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mask = match s {
            1 => 0xff,
            2 => 0xffff,
            4 => 0xffff_ffff,
            _ => u64::MAX,
        };
        self.regs.insert(o, v & mask);
        DeviceResult::Ok
    }

    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Mailbox {
    pub id: DeviceId,
    pub irq: Option<u32>,
    pub host_to_sep: u64,
    pub sep_to_host: u64,
    pub host_to_bb: u64,
    pub bb_to_host: u64,
    pub int_status: u64,
    irq_asserted: bool,
}
impl Mailbox {
    pub const HOST_TO_SEP_PENDING: u64 = 1u64 << 0;
    pub const SEP_TO_HOST_PENDING: u64 = 1 << 1;
    pub const HOST_TO_BB_PENDING: u64 = 1 << 2;
    pub const BB_TO_HOST_PENDING: u64 = 1 << 3;
    const HOST_PENDING: u64 = Self::SEP_TO_HOST_PENDING | Self::BB_TO_HOST_PENDING;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            irq: None,
            host_to_sep: 0,
            sep_to_host: 0,
            host_to_bb: 0,
            bb_to_host: 0,
            int_status: 0,
            irq_asserted: false,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        let mut mailbox = Self::new(id);
        mailbox.irq = Some(irq);
        mailbox
    }
}
impl Device for Mailbox {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "mailbox"
    }
    fn reset(&mut self) {
        self.host_to_sep = 0;
        self.sep_to_host = 0;
        self.host_to_bb = 0;
        self.bb_to_host = 0;
        self.int_status = 0;
        self.irq_asserted = false;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let v = match o {
            0x00 => self.host_to_sep,
            0x04 => self.sep_to_host,
            0x08 => self.host_to_bb,
            0x0C => self.bb_to_host,
            0x10 => self.int_status,
            _ => return DeviceResult::Fault,
        };
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.host_to_sep = v;
                self.int_status |= Self::HOST_TO_SEP_PENDING;
            }
            0x04 => {
                self.sep_to_host = v;
                self.int_status |= Self::SEP_TO_HOST_PENDING;
            }
            0x08 => {
                self.host_to_bb = v;
                self.int_status |= Self::HOST_TO_BB_PENDING;
            }
            0x0C => {
                self.bb_to_host = v;
                self.int_status |= Self::BB_TO_HOST_PENDING;
            }
            0x10 => {
                self.int_status &= !v;
                if self.int_status & Self::HOST_PENDING == 0 {
                    self.irq_asserted = false;
                }
            }
            _ => return DeviceResult::Fault,
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        let host_pending = self.int_status & Self::HOST_PENDING != 0;
        if !host_pending {
            self.irq_asserted = false;
            return None;
        }
        if self.irq_asserted {
            return None;
        }
        self.irq_asserted = true;
        self.irq.map(DeviceEvent::Interrupt)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Dcp {
    pub id: DeviceId,
    pub control: u64,
    pub status: u64,
    pub stride: u64,
    pub format: u64,
    pub fb_base_lo: u64,
    pub fb_base_hi: u64,
    /// Guest MMIO, as distinct from host diagnostic setup, programmed scanout.
    pub guest_control_written: bool,
    pub guest_fb_base_written: bool,
    pub width: u32,
    pub height: u32,
    pub framebuffer: Vec<u32>,
}
impl Dcp {
    const DEFAULT_WIDTH: u32 = 1024;
    const DEFAULT_HEIGHT: u32 = 768;
    const MAX_DIMENSION: u32 = 8192;
    const MAX_FRAMEBUFFER_PIXELS: usize = 16 * 1024 * 1024;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            control: 0,
            status: 0,
            stride: 0,
            format: 0,
            fb_base_lo: 0,
            fb_base_hi: 0,
            guest_control_written: false,
            guest_fb_base_written: false,
            width: Self::DEFAULT_WIDTH,
            height: Self::DEFAULT_HEIGHT,
            framebuffer: vec![0; (Self::DEFAULT_WIDTH * Self::DEFAULT_HEIGHT) as usize],
        }
    }

    fn set_mode(&mut self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 || width > Self::MAX_DIMENSION || height > Self::MAX_DIMENSION
        {
            return false;
        }
        let Some(pixels) = (width as usize).checked_mul(height as usize) else {
            return false;
        };
        if pixels > Self::MAX_FRAMEBUFFER_PIXELS {
            return false;
        }
        self.framebuffer.resize(pixels, 0);
        self.width = width;
        self.height = height;
        true
    }
}
impl Device for Dcp {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "dcp"
    }
    fn reset(&mut self) {
        self.control = 0;
        self.status = 0;
        self.stride = 0;
        self.format = 0;
        self.fb_base_lo = 0;
        self.fb_base_hi = 0;
        self.guest_control_written = false;
        self.guest_fb_base_written = false;
        self.width = Self::DEFAULT_WIDTH;
        self.height = Self::DEFAULT_HEIGHT;
        self.framebuffer
            .resize((Self::DEFAULT_WIDTH * Self::DEFAULT_HEIGHT) as usize, 0);
        self.framebuffer.fill(0);
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let v = match o {
            0x00 => self.control,
            0x04 => self.status,
            0x08 => self.stride,
            0x0C => self.format,
            0x10 => self.fb_base_lo,
            0x14 => self.fb_base_hi,
            0x18 => self.width as u64,
            0x1C => self.height as u64,
            _ => return DeviceResult::Fault,
        };
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.control = v;
                self.guest_control_written = true;
            }
            0x04 => self.status = v,
            0x08 => self.stride = v,
            0x0C => self.format = v,
            0x10 => {
                self.fb_base_lo = v;
                self.guest_fb_base_written = true;
            }
            0x14 => {
                self.fb_base_hi = v;
                self.guest_fb_base_written = true;
            }
            0x18 => {
                let Ok(width) = u32::try_from(v) else {
                    return DeviceResult::Fault;
                };
                if !self.set_mode(width, self.height) {
                    return DeviceResult::Fault;
                }
            }
            0x1C => {
                let Ok(height) = u32::try_from(v) else {
                    return DeviceResult::Fault;
                };
                if !self.set_mode(self.width, height) {
                    return DeviceResult::Fault;
                }
            }
            _ => return DeviceResult::Fault,
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct NorFlash {
    pub id: DeviceId,
    pub base: u64,
    pub data: Vec<u8>,
}
impl NorFlash {
    pub fn new(id: DeviceId, base: u64, size: usize) -> Self {
        Self {
            id,
            base,
            data: vec![0xFF; size],
        }
    }
    pub fn load_image(&mut self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let data = std::fs::read(path)?;
        self.data.fill(0xff);
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        Ok(())
    }
}
impl Device for NorFlash {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "nor-flash"
    }
    fn reset(&mut self) {
        // A controller reset does not erase non-volatile flash contents.
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let off = o as usize;
        if off + s > self.data.len() {
            return DeviceResult::Fault;
        }
        let mut v = 0u64;
        for i in 0..s {
            v |= (self.data[off + i] as u64) << (i * 8);
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let off = o as usize;
        if off + s > self.data.len() {
            return DeviceResult::Fault;
        }
        for i in 0..s {
            self.data[off + i] &= (v >> (i * 8)) as u8;
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct BlockDev {
    pub id: DeviceId,
    pub base: u64,
    pub sector_size: u64,
    pub sectors: u64,
    pub data: Vec<u8>,
}
impl BlockDev {
    pub fn new(id: DeviceId, base: u64, size: u64) -> Self {
        let sector_size = 512;
        let sectors = size / sector_size;
        Self {
            id,
            base,
            sector_size,
            sectors,
            data: vec![0; size as usize],
        }
    }
    pub fn load_image(&mut self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let data = std::fs::read(path)?;
        self.data.fill(0);
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        Ok(())
    }
}
impl Device for BlockDev {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "block"
    }
    fn reset(&mut self) {
        // Controller reset preserves the non-volatile backing store.
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let off = o as usize;
        if off + s > self.data.len() {
            return DeviceResult::Fault;
        }
        let mut v = 0u64;
        for i in 0..s {
            v |= (self.data[off + i] as u64) << (i * 8);
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !matches!(s, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let off = o as usize;
        if off + s > self.data.len() {
            return DeviceResult::Fault;
        }
        for i in 0..s {
            self.data[off + i] = (v >> (i * 8)) as u8;
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub mod apfs;
pub mod display;
pub mod peripherals;
pub mod storage;
pub mod uart;
pub use uart::S5lUart;
pub use uart::SerialConsole;
pub mod usb_host;
pub mod user_peripherals;
pub use apfs::{ApfsError, Reader as ApfsReader, BLOCK_SIZE as APFS_BLOCK_SIZE};
pub use display::{
    DsiCmd, DsiLinkState, DsiPhy, IOMfb, IOSSurfaceInfo, Panel, Raster, RasterLayer,
    PANEL_INIT_CMDS,
};
pub use peripherals::{
    Adc, AppleChipControl, ClockCtrl, Gpio, I2c, I2cState, NandCtrl, Pinmux, Pmu, Pwm, Regulator,
    Rtc, Spi, Spmi, Thermal, Watchdog, ADC_CHANNELS, CLOCKCTL_PLLS, GPIO_BANKS, GPIO_PINS_PER_BANK,
    I2C_FIFO_DEPTH, PINMUX_PINS, PWM_CHANNELS, REGULATOR_CHANNELS, SPI_FIFO_DEPTH, SPMI_CHANNELS,
    THERMAL_ZONES,
};
pub use storage::{
    EmmcSdHc, NandArray, SdCard, NAND_BLOCK_SIZE, NAND_PAGES_PER_BLOCK, NAND_PAGE_SIZE,
    NAND_SPARE_SIZE,
};
pub use usb_host::{UsbComplex, UsbMux, UsbMuxPath, UsbPd, UsbPdState, UsbPhy, UsbPhyRole};
pub use user_peripherals::{
    AudioI2s, BtHci, CameraIsp, SensorFifoEntry, SensorHub, TouchHid, WifiSdio, BT_HCI_CMD_BUF,
    BT_HCI_EVT_BUF, GESTURE_NONE, GESTURE_PINCH, GESTURE_SWIPE, GESTURE_TAP, I2S_BIT_DEPTH_16,
    I2S_BIT_DEPTH_24, I2S_BIT_DEPTH_32, I2S_FIFO_DEPTH, SENSOR_AXES, SENSOR_FIFO_DEPTH,
    SENSOR_KIND_COUNT, TOUCH_MAX_POINTS, WIFI_REGS,
};

pub mod usb_dfu;
pub use usb_dfu::{
    AppleImageType, DfuEngine, DfuRequest, DfuState, DfuStatus, DfuStatusResponse, Ep0DataPhase,
    Ep0State, FirmwareType, UsbDeviceController, UsbEndpoint, UsbSetupPacket,
};

pub mod neural;
pub use neural::{
    AneFifo, AnePush, KF0_PUSH, KF1_POP, KF2_CTRL, KF3_DEPTH, KF4_STAT, KF5_FULL, KF6_DRAIN,
    KF7_COMPLETE, KF_WINDOW,
};

pub mod gpu;
pub use gpu::{
    SgxGpu, SgxPush, SGX_MSG_CTRL, SGX_MSG_DEPTH, SGX_MSG_DONE, SGX_MSG_FULL, SGX_MSG_SEND,
    SGX_MSG_STAT, SGX_SEG_ID, SGX_SEG_READ, SGX_WINDOW,
};

/// A device that always returns a fixed value for any read and ignores writes.
/// Useful for spoofing hardware identifiers during boot.
pub struct FixedValueDevice {
    pub id: u32,
    pub value: u64,
}

impl FixedValueDevice {
    pub fn new(id: u32, value: u64) -> Self {
        Self { id, value }
    }
}

impl Device for FixedValueDevice {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        "FixedValueDevice"
    }
    fn read(&mut self, _offset: u64, _size: usize) -> DeviceResult {
        DeviceResult::Value(self.value)
    }
    fn write(&mut self, _offset: u64, _size: usize, _value: u64) -> DeviceResult {
        DeviceResult::Ok
    }
    fn reset(&mut self) {}
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &(dyn std::any::Any + 'static) {
        self
    }
    fn as_any_mut(&mut self) -> &mut (dyn std::any::Any + 'static) {
        self
    }
}

/// A plain register file with no established semantics: a 32-bit read returns the 32-bit
/// word last written to that offset, and an unwritten offset reads as zero.
///
/// This exists so a platform window that the firmware is *measured* to touch, but whose
/// registers nothing has established, can be mapped under its own name instead of under a
/// memory device's. `SparseRam` does the same job and was used for it, and the cost of that
/// is a window labelled `sparse-dram` in every census row and platform test — which reads
/// as a claim that the block is RAM, and is how the A8's SIO came to be attributed to
/// `sparse-dram` rather than to a SIO.
///
/// What it deliberately does not do is invent a value. There is no reset value beyond zero,
/// no completion bit, and no interrupt: a firmware poll on one of these offsets spins, and
/// that is the correct outcome for a register whose behaviour is unknown, because it keeps
/// the gate visible instead of hiding it behind a model that agrees with the guest.
pub struct RegFile {
    pub id: u32,
    name: &'static str,
    /// Byte-addressed store, so a 1/2/4/8-byte access at any alignment round-trips and a
    /// pair of adjacent 32-bit writes to one 64-bit register keeps both halves.
    bytes: std::collections::BTreeMap<u64, u8>,
}

impl RegFile {
    pub fn new(id: u32, name: &'static str) -> Self {
        Self {
            id,
            name,
            bytes: std::collections::BTreeMap::new(),
        }
    }

    /// Set a 32-bit word at `offset` (little-endian) for initial register values.
    pub fn set_word(&mut self, offset: u64, value: u32) {
        for i in 0..4 {
            self.bytes
                .insert(offset + i, ((value >> (8 * i)) & 0xff) as u8);
        }
    }

    /// The 32-bit word at `offset`, zero-extended.
    pub fn word(&self, offset: u64) -> u32 {
        let mut v = 0u32;
        for i in 0..4 {
            v |= (*self.bytes.get(&(offset + i)).unwrap_or(&0) as u32) << (8 * i);
        }
        v
    }
}

impl Device for RegFile {
    fn id(&self) -> u32 {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn reset(&mut self) {
        self.bytes.clear();
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let mut v = 0u64;
        for i in 0..size {
            v |= (*self.bytes.get(&(offset + i as u64)).unwrap_or(&0) as u64) << (8 * i);
        }
        DeviceResult::Value(v)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        for i in 0..size {
            self.bytes
                .insert(offset + i as u64, ((value >> (8 * i)) & 0xff) as u8);
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &(dyn std::any::Any + 'static) {
        self
    }
    fn as_any_mut(&mut self) -> &mut (dyn std::any::Any + 'static) {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iphem_core::{BusRegion, Machine, MachineConfig};

    /// The T8020 ROM unmask-enables its two SEP-mail interrupt lines through
    /// a per-IRQ 32-bit window at `0x23b103000 + irq*4`, as shown by its own
    /// enable sequence at `0x100002880`: it loads base `0x23b102000`, reads
    /// the config slot, computes `1 << value`, then stores to
    /// `base + 0x1000 + irq`. An 8M-instruction SecureROM trace records those
    /// stores at `0x23b103284` (irq 161 / 0xa1) and `0x23b103290` (irq
    /// 164 / 0xa4) — the two SEP-mail lines the ASC mailbox and the SEP-mail
    /// descriptor table use. Without this window the ROM could never enable
    /// any line, so no device could ever raise an interrupt into the AP and
    /// the post-init scheduler could never post an event.
    #[test]
    fn t8020_sepmail_lines_enable_through_observed_per_irq_mask_window() {
        let mut aic = Aic::new(0, 512);
        for irq in [161u32, 164] {
            let offset = AIC2_IRQ_MASK_START + (irq as u64) * 4;
            assert_eq!(offset, if irq == 161 { 0x3284 } else { 0x3290 });
            assert_eq!(aic.write(offset, 4, 1), DeviceResult::Ok);
            let line = aic.irqs.iter().find(|i| i.id == irq).unwrap();
            assert!(
                line.enabled,
                "IRQ {irq} must be enabled after its mask word write"
            );
        }

        // Writing zero must disable again, and must not disturb a neighbour.
        assert_eq!(
            aic.write(AIC2_IRQ_MASK_START + 161 * 4, 4, 0),
            DeviceResult::Ok
        );
        assert!(!aic.irqs.iter().find(|i| i.id == 161).unwrap().enabled);
        assert!(aic.irqs.iter().find(|i| i.id == 164).unwrap().enabled);

        // An enabled line is selectable by acknowledge().
        assert_eq!(
            aic.write(AIC2_IRQ_MASK_START + 164 * 4, 4, 1),
            DeviceResult::Ok
        );
        aic.irqs.iter_mut().find(|i| i.id == 164).unwrap().pending = true;
        assert_eq!(aic.acknowledge(), Some(164));
    }

    #[test]
    fn aic2_register_banks_are_contiguous_and_non_overlapping() {
        let mut aic = Aic::new(0, 2048);

        for (offset, expected) in [
            (AIC2_SW_SET_START, 0x11),
            (AIC2_SW_CLR_START, 0x22),
            (AIC2_MASK_SET_START, 0x33),
            (AIC2_MASK_CLR_START, 0x44),
            (AIC2_HW_STATE_START, 0x55),
        ] {
            assert_eq!(aic.write(offset, 4, expected), DeviceResult::Ok);
            assert_eq!(aic.read(offset, 4), DeviceResult::Value(expected));
        }

        assert_eq!(aic.aic2_sw_set[0], 0x11);
        assert_eq!(aic.aic2_sw_clr[0], 0x22);
        assert_eq!(aic.aic2_mask_set[0], 0x33);
        assert_eq!(aic.aic2_mask_clr[0], 0x44);
        assert_eq!(aic.aic2_hw_state[0], 0x55);

        // The final word of each bank remains in that bank, including the
        // boundaries that were previously shadowed by overlapping ranges.
        for (offset, expected) in [
            (0x40fc, 0x66),
            (0x41fc, 0x77),
            (0x42fc, 0x88),
            (0x43fc, 0x99),
            (0x44fc, 0xaa),
        ] {
            assert_eq!(aic.write(offset, 4, expected), DeviceResult::Ok);
            assert_eq!(aic.read(offset, 4), DeviceResult::Value(expected));
        }
        assert_eq!(aic.aic2_sw_clr[63], 0x77);
        assert_eq!(aic.aic2_mask_clr[63], 0x99);
        assert_eq!(aic.aic2_hw_state[63], 0xaa);
    }

    #[test]
    fn aic2_reset_command_ignores_reserved_bits_and_clears_state() {
        let mut aic = Aic::new(0, 2048);
        aic.aic2_irq_cfg[3] = 0x55aa;
        aic.aic2_sw_clr[0] = 0xffff;

        assert_eq!(aic.read(AIC2_RESET_OFFSET, 4), DeviceResult::Value(0));
        assert_eq!(
            aic.write(AIC2_RESET_OFFSET, 4, 0x8000_0000),
            DeviceResult::Ok
        );
        assert_eq!(aic.aic2_irq_cfg[3], 0x55aa);
        assert_eq!(
            aic.write(AIC2_RESET_OFFSET, 4, AIC2_RESET_BIT as u64),
            DeviceResult::Ok
        );
        assert_eq!(aic.aic2_irq_cfg[3], 0);
        assert_eq!(aic.aic2_sw_clr[0], 0);
    }

    /// Apple's own AES driver, replayed instruction for instruction against the
    /// modelled block.
    ///
    /// The SecureROM routine at `0x1000026a0` is the only description of the
    /// `+0x0c` status word in the corpus, and it is compiled byte-identically
    /// into the t8020, t8027 and t8030 SecureROMs. This replays it literally --
    /// six bit tests, then the returned bit -- and then the whole operation
    /// handshake the enclosing service performs, so a model that drifted from
    /// the driver would fail here rather than at a five-million-instruction
    /// trace.
    #[test]
    fn aes_otp_satisfies_the_secure_rom_driver_register_for_register() {
        let mut aes = AesOtp::new(211);

        // `ldr w9,[x8]` / `tbz x9,#k` for k = 12, 11, 10, 9, 8, then
        // `ldr w9,[x8]` / `tbnz x9,#7`, exactly as the driver reads it.
        let status = match aes.read(AesOtp::STATUS, 4) {
            DeviceResult::Value(v) => v as u32,
            other => panic!("status read returned {other:?}"),
        };
        for bit in [12u32, 11, 10, 9, 8] {
            assert_ne!(
                status & (1 << bit),
                0,
                "the driver rejects a clear bit {bit}"
            );
        }
        assert_ne!(
            status & (1 << 7),
            0,
            "the driver's `tbnz x9,#7` must reach the return, so bit 7 is required set"
        );
        // `ubfm w0, w8, #13, #13` -- the value its two callers branch on.
        let returned = (status >> 13) & 1;
        assert_eq!(
            returned, 1,
            "the driver returns bit 13, and it is the pass condition"
        );

        // The service's operation handshake, in the order the ROM performs it:
        // clear the completion flag, start, programme the block, poll.
        assert_eq!(aes.read(AesOtp::STATUS2, 4), DeviceResult::Value(0));
        assert_eq!(
            aes.write(AesOtp::STATUS2, 4, AesOtp::DONE_BIT.into()),
            DeviceResult::Ok
        );
        assert_eq!(aes.write(AesOtp::CONTROL, 4, 1), DeviceResult::Ok);
        // `ldr w9,[x8]` / `tbz x9,#5` -- the completion poll the ROM spins on.
        let done = match aes.read(AesOtp::STATUS2, 4) {
            DeviceResult::Value(v) => v as u32,
            other => panic!("completion read returned {other:?}"),
        };
        assert_eq!(
            done & AesOtp::DONE_BIT,
            AesOtp::DONE_BIT,
            "the first poll after the start must already see bit 5"
        );
        // `orr w9, wzr, #0x20` / `str w9, [x8]`, then the read-and-discard, then
        // the `cbnz` that requires the word to read back zero.
        assert_eq!(
            aes.write(AesOtp::STATUS2, 4, AesOtp::DONE_BIT.into()),
            DeviceResult::Ok
        );
        assert_eq!(aes.read(AesOtp::FLUSH, 4), DeviceResult::Value(0));
        assert_eq!(
            aes.read(AesOtp::STATUS2, 4),
            DeviceResult::Value(0),
            "the driver requires +0x18 to read zero once it has written 0x20"
        );

        // The burst writer's data port accepts the driver's pushes, and the
        // status register cannot be written into satisfying the gate.
        for word in [0x0000_0000u64, 0x5a82_0000, 0x0f0f_0f0f] {
            assert_eq!(aes.write(AesOtp::DATA, 4, word), DeviceResult::Ok);
        }
        assert_eq!(aes.write(AesOtp::STATUS, 4, 0), DeviceResult::Ok);
        assert!(
            aes.reports_identity(),
            "a guest write cannot un-satisfy the gate"
        );
        assert_eq!(
            aes.status(),
            AesOtp::CAPABILITY | AesOtp::READY_BIT | AesOtp::RETURNED_BIT
        );

        // Reset clears the guest-driven state and nothing else.
        aes.reset();
        assert!(!aes.operation_complete());
        assert_eq!(aes.read(AesOtp::STATUS2, 4), DeviceResult::Value(0));
        assert_eq!(aes.read(AesOtp::DATA, 4), DeviceResult::Value(0));
    }

    #[test]
    fn sep_boot_ctrl_preserves_writes_and_asserts_ready_bit() {
        let mut ctrl = SepBootCtrl::new(41);

        assert_eq!(ctrl.write(0x108, 8, 0x5), DeviceResult::Ok);
        assert_eq!(
            ctrl.read(0x108, 8),
            DeviceResult::Value(SepBootCtrl::READY_BIT | 0x5)
        );
        assert_eq!(ctrl.read(0x110, 8), DeviceResult::Value(0));
        assert_eq!(ctrl.write(0x108, 3, 0), DeviceResult::Fault);
    }

    #[test]
    fn sep_identity_exposes_expected_low_chip_id() {
        let mut ident = SepIdentity::new(42, 0x32b3);

        assert_eq!(ident.read(0, 4), DeviceResult::Value(0x32b3));
        assert_eq!(ident.write(0, 8, 0xffff_0000), DeviceResult::Ok);
        assert_eq!(ident.read(0, 8), DeviceResult::Value(0xffff_32b3));
        assert_eq!(ident.read(4, 4), DeviceResult::Value(0));
        assert_eq!(ident.read(0, 3), DeviceResult::Fault);
    }

    #[test]
    fn sep_ready_status_asserts_poll_bit() {
        let mut status = SepReadyStatus::new(43);

        assert_eq!(status.write(0x10, 4, 0x20), DeviceResult::Ok);
        assert_eq!(
            status.read(0x10, 4),
            DeviceResult::Value(SepReadyStatus::READY_BIT | 0x20)
        );
        assert_eq!(status.read(0x14, 4), DeviceResult::Value(0));
        assert_eq!(status.write(0x10, 3, 0), DeviceResult::Fault);
    }

    #[test]
    fn test_phase58_timer_mask_status_and_enable_semantics() {
        let mut timer = Timer::new(2, 30);
        timer.write(TimerReg::CNTV_CVAL_EL0 as u64, 8, 5);
        timer.write(TimerReg::CNTV_CTL_EL0 as u64, 4, 0b11);

        assert!(timer.tick(5).is_none());
        assert_eq!(
            timer.read(TimerReg::CNTV_CTL_EL0 as u64, 4),
            DeviceResult::Value(0b111)
        );

        timer.write(TimerReg::CNTV_CTL_EL0 as u64, 4, 0b01);
        assert!(matches!(timer.tick(0), Some(DeviceEvent::Interrupt(30))));
        assert!(timer.tick(1).is_none());
        assert_eq!(
            timer.read(TimerReg::CNTV_CTL_EL0 as u64, 4),
            DeviceResult::Value(0b101)
        );
    }

    #[test]
    fn test_phase58_virtual_offset_and_dynamic_tval() {
        let mut timer = Timer::new(2, 30);
        timer.tick(100);
        timer.write(TimerReg::CNTVOFF_EL2 as u64, 8, 40);
        assert_eq!(
            timer.read(TimerReg::CNTVCT_EL0 as u64, 8),
            DeviceResult::Value(60)
        );

        timer.write(TimerReg::CNTV_TVAL_EL0 as u64, 4, 10);
        assert_eq!(
            timer.read(TimerReg::CNTV_CVAL_EL0 as u64, 8),
            DeviceResult::Value(70)
        );
        timer.tick(4);
        assert_eq!(
            timer.read(TimerReg::CNTV_TVAL_EL0 as u64, 4),
            DeviceResult::Value(6)
        );

        timer.write(TimerReg::CNTV_TVAL_EL0 as u64, 4, u32::MAX as u64);
        assert_eq!(
            timer.read(TimerReg::CNTV_TVAL_EL0 as u64, 4),
            DeviceResult::Value(u32::MAX as u64)
        );
    }

    #[test]
    fn test_phase59_nor_programming_is_one_way_and_reset_persistent() {
        let mut flash = NorFlash::new(7, 0x2400_0000, 16);

        assert_eq!(flash.write(4, 4, 0x0ff0_55aa), DeviceResult::Ok);
        assert_eq!(flash.read(4, 4), DeviceResult::Value(0x0ff0_55aa));
        assert_eq!(flash.write(4, 4, u32::MAX as u64), DeviceResult::Ok);
        assert_eq!(flash.read(4, 4), DeviceResult::Value(0x0ff0_55aa));

        flash.reset();
        assert_eq!(flash.read(4, 4), DeviceResult::Value(0x0ff0_55aa));
    }

    #[test]
    fn test_phase60_block_storage_survives_controller_reset() {
        let mut disk = BlockDev::new(8, 0x2410_0000, 1024);

        assert_eq!(disk.write(510, 4, 0x4433_2211), DeviceResult::Ok);
        assert_eq!(disk.read(510, 4), DeviceResult::Value(0x4433_2211));
        disk.reset();
        assert_eq!(disk.read(510, 4), DeviceResult::Value(0x4433_2211));
        assert_eq!(disk.sector_size, 512);
        assert_eq!(disk.sectors, 2);
    }

    #[test]
    fn test_phase61_primary_uart_rx_fifo_status_and_irq() {
        let mut uart = Uart::new(1, 7);
        uart.inject_rx(b"ABC");
        uart.write(0x0c, 4, 0b11);

        assert_eq!(uart.read(0x10, 4), DeviceResult::Value(0x03));
        assert!(matches!(uart.tick(1), Some(DeviceEvent::Interrupt(7))));
        assert!(uart.tick(1).is_none());
        assert_eq!(uart.read(0x00, 1), DeviceResult::Value(b'A' as u64));
        assert_eq!(uart.read(0x00, 1), DeviceResult::Value(b'B' as u64));
        assert_eq!(uart.read(0x00, 1), DeviceResult::Value(b'C' as u64));
        assert_eq!(uart.read(0x10, 4), DeviceResult::Value(0x02));

        assert_eq!(uart.write(0x14, 4, 0), DeviceResult::Ok);
        assert_eq!(uart.baud_rate, 1);
        assert!(uart.baud_div >= 1);
    }

    #[test]
    fn apple_uart_register_aliases_capture_guest_console_output() {
        let mut uart = Uart::new(1, 7);
        assert_eq!(uart.read(0x14, 4), DeviceResult::Value(0x04));
        uart.write(0x18, 1, b'X' as u64);
        assert_eq!(uart.drain_output(), b"X");

        uart.inject_rx(b"k");
        assert_eq!(uart.read(0x14, 4), DeviceResult::Value(0x05));
        assert_eq!(uart.read(0x1c, 1), DeviceResult::Value(b'k' as u64));
        assert_eq!(uart.read(0x14, 4), DeviceResult::Value(0x04));
    }

    #[test]
    fn test_phase61_auxiliary_uart_rx_is_fifo_ordered() {
        let mut sep = SepUart::new(2, "SEP");
        let mut baseband = BasebandUart::new(3, "BB");
        sep.inject_rx(&[1, 2]);
        baseband.inject_rx(&[3, 4]);

        assert_eq!(sep.read(0, 1), DeviceResult::Value(1));
        assert_eq!(sep.read(0, 1), DeviceResult::Value(2));
        assert_eq!(baseband.read(0, 1), DeviceResult::Value(3));
        assert_eq!(baseband.read(0, 1), DeviceResult::Value(4));
    }

    #[test]
    fn test_phase62_mailbox_directional_status_w1c_and_host_irq() {
        let mut mailbox = Mailbox::with_irq(5, 31);

        assert_eq!(mailbox.write(0x00, 8, 0x1111), DeviceResult::Ok);
        assert_eq!(mailbox.write(0x08, 8, 0x2222), DeviceResult::Ok);
        assert_eq!(
            mailbox.read(0x10, 8),
            DeviceResult::Value(Mailbox::HOST_TO_SEP_PENDING | Mailbox::HOST_TO_BB_PENDING)
        );
        assert!(mailbox.tick(1).is_none());

        assert_eq!(mailbox.write(0x04, 8, 0x3333), DeviceResult::Ok);
        assert!(matches!(mailbox.tick(1), Some(DeviceEvent::Interrupt(31))));
        assert!(mailbox.tick(1).is_none());
        assert_eq!(
            mailbox.write(0x10, 8, Mailbox::SEP_TO_HOST_PENDING),
            DeviceResult::Ok
        );

        assert_eq!(mailbox.write(0x0c, 8, 0x4444), DeviceResult::Ok);
        assert!(matches!(mailbox.tick(1), Some(DeviceEvent::Interrupt(31))));
        assert_eq!(mailbox.read(0x14, 4), DeviceResult::Fault);
        assert_eq!(mailbox.write(0x14, 4, 0), DeviceResult::Fault);

        mailbox.reset();
        assert_eq!(mailbox.read(0x10, 8), DeviceResult::Value(0));
        assert!(mailbox.tick(1).is_none());
    }

    #[test]
    fn test_phase65_dcp_mode_updates_are_bounded_and_transactional() {
        let mut dcp = Dcp::new(6);

        assert_eq!(dcp.write(0x18, 4, 1920), DeviceResult::Ok);
        assert_eq!(dcp.write(0x1c, 4, 1080), DeviceResult::Ok);
        assert_eq!(dcp.width, 1920);
        assert_eq!(dcp.height, 1080);
        assert_eq!(dcp.framebuffer.len(), 1920 * 1080);

        let previous = (dcp.width, dcp.height, dcp.framebuffer.len());
        assert_eq!(dcp.write(0x18, 8, u64::MAX), DeviceResult::Fault);
        assert_eq!(dcp.write(0x1c, 4, 0), DeviceResult::Fault);
        assert_eq!((dcp.width, dcp.height, dcp.framebuffer.len()), previous);
        assert_eq!(dcp.read(0x20, 4), DeviceResult::Fault);
        assert_eq!(dcp.write(0x20, 4, 0), DeviceResult::Fault);

        dcp.reset();
        assert_eq!((dcp.width, dcp.height), (1024, 768));
        assert_eq!(dcp.framebuffer.len(), 1024 * 768);
    }

    #[test]
    fn test_phase159_peripherals_decode_all_offsets_and_fault_bad_size() {
        // Phase 159: every peripheral in `peripherals.rs` decodes every
        // register without faulting. Invalid access sizes still fault.
        let mut i2c = I2c::new(1, 0);
        let mut spi = Spi::new(2, 0);
        let mut gpio = Gpio::new(3);
        let mut nand = NandCtrl::new(4);
        let mut pmu = Pmu::new(5);
        let mut spmi = Spmi::new(6);
        let mut thermal = Thermal::new(7);

        // Bad sizes still fault.
        for sz in [3usize, 5, 7, 16] {
            assert_eq!(i2c.read(0, sz), DeviceResult::Fault);
            assert_eq!(spi.read(0, sz), DeviceResult::Fault);
            assert_eq!(gpio.read(0, sz), DeviceResult::Fault);
            assert_eq!(nand.read(0, sz), DeviceResult::Fault);
            assert_eq!(pmu.read(0, sz), DeviceResult::Fault);
            assert_eq!(spmi.read(0, sz), DeviceResult::Fault);
            assert_eq!(thermal.read(0, sz), DeviceResult::Fault);
        }

        // Decoded register reads work and write round-trips.
        assert_eq!(gpio.write(0x00, 4, 0xFFFF_FFFF), DeviceResult::Ok);
        assert_eq!(gpio.read(0x00, 4), DeviceResult::Value(0xFFFF_FFFF));
        assert_eq!(nand.read(0x10, 4), DeviceResult::Value(1)); // status RDY bit
        assert_eq!(pmu.read(0x20, 4), DeviceResult::Value(0));
        assert_eq!(spmi.read(0x10, 4), DeviceResult::Value(1));
        assert_eq!(thermal.read(0x00, 4), DeviceResult::Value(2500));
    }

    #[test]
    fn test_aic_gicr_setlpi_raises_pending() {
        let mut aic = Aic::new(3, 128);

        assert_eq!(aic.read(0x10000, 4), DeviceResult::Value(0)); // GICR_CTLR
        assert_eq!(aic.write(0x10000, 4, 0x3), DeviceResult::Ok);
        assert_eq!(aic.read(0x10000, 4), DeviceResult::Value(0x3));

        // GICR_SETLPIR raises LPI 8192 (first device LPI)
        assert_eq!(aic.write(0x10040, 4, 8192), DeviceResult::Ok);
        assert!(
            aic.get_highest_pending().is_none()
                || aic.irqs.iter().any(|i| i.id == 8192 && i.pending)
        );
        assert!(aic
            .irqs
            .iter()
            .any(|i| i.id == 8192 && i.int_type == InterruptType::LPI));
        assert_eq!(aic.read(0x10008, 8), DeviceResult::Value(0)); // GICR_TYPER

        // GICR_WAKER / IIDR
        assert_eq!(aic.write(0x10014, 4, 0x4), DeviceResult::Ok); // clear ProcessorSleep
        assert_eq!(aic.read(0x10014, 4), DeviceResult::Value(0x4));
        assert_eq!(aic.read(0x10004, 4), DeviceResult::Value(0x000E0001));

        // GICR_CLRLPIR clears it
        assert_eq!(aic.write(0x10048, 4, 8192), DeviceResult::Ok);
        assert!(aic
            .irqs
            .iter()
            .find(|i| i.id == 8192)
            .map(|i| !i.pending)
            .unwrap_or(true));
    }

    #[test]
    fn test_aic_its_register_file_and_translation() {
        let mut aic = Aic::new(3, 128);

        // GITS_CTLR / IIDR / TYPER
        assert_eq!(aic.write(0x20000, 4, 0x1), DeviceResult::Ok);
        assert_eq!(aic.read(0x20000, 4), DeviceResult::Value(0x1));
        assert_eq!(aic.read(0x20004, 4), DeviceResult::Value(0xa0000000));

        // GITS_CBASER / CWRITER / CREADR
        assert_eq!(aic.write(0x20008, 8, 0x1c0000000), DeviceResult::Ok);
        assert_eq!(aic.read(0x20008, 8), DeviceResult::Value(0x1c0000000));
        assert_eq!(aic.write(0x20010, 8, 0x100), DeviceResult::Ok);
        assert_eq!(aic.write(0x20018, 8, 0), DeviceResult::Ok);
        assert_eq!(aic.read(0x20010, 8), DeviceResult::Value(0x100));
        assert_eq!(aic.read(0x20018, 8), DeviceResult::Value(0));

        // GITS_BASER0 carved for device table, BASER1 for collection table
        assert_eq!(
            aic.write(0x20080, 8, 0xc00000000 | (1 << 8)),
            DeviceResult::Ok
        );
        assert_eq!(aic.write(0x20088, 8, 0x2000), DeviceResult::Ok);
        assert_eq!(
            aic.read(0x20080, 8),
            DeviceResult::Value(0xc00000000 | (1 << 8))
        );
        assert_eq!(aic.read(0x20088, 8), DeviceResult::Value(0x2000));

        // Map device 0x2 event 0x77 -> LPI 9000, translate, inject via TRANSLATER
        aic.its_map(0x2, 0x77, 9000);
        assert_eq!(aic.its_translate(0x2, 0x77), Some(9000));
        let inject = (2u64 << 32) | 0x77;
        assert_eq!(aic.write(0x21000, 8, inject), DeviceResult::Ok);
        assert_eq!(aic.read(0x21000, 8), DeviceResult::Value(inject));
        assert!(aic
            .irqs
            .iter()
            .any(|i| i.id == 9000 && i.pending && i.int_type == InterruptType::LPI));

        // A pending LPI is deliverable once enabled (GICD_ISENABLER path)
        aic.enable(9000);
        assert_eq!(aic.get_highest_pending(), Some(9000));

        // Un-mapped event must not raise anything
        let before = aic.irqs.iter().filter(|i| i.pending).count();
        let inject_unmapped = (7u64 << 32) | 0x99;
        assert_eq!(aic.write(0x21000, 8, inject_unmapped), DeviceResult::Ok);
        assert_eq!(aic.irqs.iter().filter(|i| i.pending).count(), before);
    }

    #[test]
    fn test_aic_gicd_and_legacy_regions_still_reachable() {
        let mut aic = Aic::new(3, 128);

        assert_eq!(aic.write(0x0100, 4, 0xf), DeviceResult::Ok); // GICD_ISENABLER
        assert_eq!(aic.write(0x0F00, 4, 0x1), DeviceResult::Ok); // GICD_SGIR
        assert_eq!(aic.read(0x0E00, 4), DeviceResult::Value(0)); // legacy ack
        assert_eq!(aic.write(0x0E08, 4, 0x21), DeviceResult::Ok); // legacy enable irq 33
        assert!(aic.irqs.iter().any(|i| i.id == 33 && i.enabled));
    }

    #[test]
    fn test_phase52_gicd_bitmap_register_offsets_select_irq_banks() {
        let mut aic = Aic::new(3, 128);

        assert_eq!(aic.write(0x0104, 4, 1 << 1), DeviceResult::Ok);
        assert!(aic.irqs.iter().any(|irq| irq.id == 33 && irq.enabled));
        assert!(aic.irqs.iter().any(|irq| irq.id == 1 && !irq.enabled));
        assert_eq!(aic.read(0x0104, 4), DeviceResult::Value(1 << 1));
        assert_eq!(aic.read(0x0184, 4), DeviceResult::Value(1 << 1));

        assert_eq!(aic.write(0x0208, 4, 1 << 5), DeviceResult::Ok);
        assert!(aic.irqs.iter().any(|irq| irq.id == 69 && irq.pending));
        assert_eq!(aic.read(0x0208, 4), DeviceResult::Value(1 << 5));
        assert_eq!(aic.write(0x0288, 4, 1 << 5), DeviceResult::Ok);
        assert_eq!(aic.read(0x0208, 4), DeviceResult::Value(0));
    }

    #[test]
    fn test_phase52_gicd_group_bitmap_round_trips() {
        let mut aic = Aic::new(3, 128);

        assert_eq!(aic.write(0x0084, 4, 1 << 2), DeviceResult::Ok);
        assert_eq!(aic.read(0x0084, 4), DeviceResult::Value(1 << 2));
        assert!(aic
            .irqs
            .iter()
            .any(|irq| irq.id == 33 && irq.group == InterruptGroup::Group0));
        assert!(aic
            .irqs
            .iter()
            .any(|irq| irq.id == 34 && irq.group == InterruptGroup::Group1NS));
    }

    #[test]
    fn test_phase53_gicd_priority_register_controls_arbitration() {
        let mut aic = Aic::new(3, 128);
        let priorities = 0x80_10_40_80_u32;

        assert_eq!(aic.write(0x0428, 4, priorities as u64), DeviceResult::Ok);
        assert_eq!(aic.read(0x0428, 4), DeviceResult::Value(priorities as u64));
        assert_eq!(
            aic.irqs.iter().find(|irq| irq.id == 41).unwrap().priority,
            IrqPriority(0x40)
        );
        assert_eq!(
            aic.irqs.iter().find(|irq| irq.id == 42).unwrap().priority,
            IrqPriority(0x10)
        );

        aic.enable(41);
        aic.enable(42);
        aic.raise(41);
        aic.raise(42);
        assert_eq!(aic.acknowledge(), Some(42));
    }

    #[test]
    fn test_phase54_gicd_configuration_and_sgir_have_distinct_offsets() {
        let mut aic = Aic::new(3, 128);
        let edge_for_irq40 = 0b10_u32 << (8 * 2);

        assert_eq!(
            aic.write(0x0c08, 4, edge_for_irq40 as u64),
            DeviceResult::Ok
        );
        assert_eq!(
            aic.read(0x0c08, 4),
            DeviceResult::Value(edge_for_irq40 as u64)
        );
        assert_eq!(
            aic.irqs.iter().find(|irq| irq.id == 40).unwrap().config,
            0b10
        );

        assert_eq!(aic.write(0x0f00, 4, 1), DeviceResult::Ok);
        assert!(aic
            .irqs
            .iter()
            .any(|irq| irq.id == 1 && irq.int_type == InterruptType::SGI && irq.pending));
        assert_eq!(aic.read(0x0c00, 4), DeviceResult::Value(0));
    }

    #[test]
    fn test_phase56_aic_reset_restores_power_on_state() {
        let mut aic = Aic::new(3, 128);
        aic.write(0x0000, 4, 1);
        aic.write(0x0104, 4, 2);
        aic.write(0x0204, 4, 2);
        aic.write(0x0304, 4, 2);
        aic.write(0x0084, 4, 0);
        aic.write(0x0420, 4, 0x0102_0304);
        aic.write(0x0c08, 4, 0xaaaa_aaaa);
        aic.write(0x10000, 4, 3);
        aic.write(0x10014, 4, 4);
        aic.write(0x20000, 4, 1);
        aic.its_map(2, 3, 9000);

        aic.reset();

        assert_eq!(aic.gicd_ctlr, 0);
        assert_eq!(aic.gicr_ctlr, 0);
        assert_eq!(aic.gicr_waker, 0);
        assert_eq!(aic.its_ctlr, 0);
        assert!(aic.its_translations.is_empty());
        assert!(aic.current_priority.is_none());
        assert!(aic.irqs.iter().all(|irq| {
            !irq.enabled
                && !irq.pending
                && !irq.active
                && irq.priority == IrqPriority::NORMAL
                && irq.group == InterruptGroup::Group1NS
                && irq.config == 0
        }));
    }

    #[test]
    fn test_phase57_active_bitmap_updates_preemption_priority() {
        let mut aic = Aic::new(3, 128);
        aic.write(0x0428, 4, 0x8080_4020);

        aic.write(0x0304, 4, (1 << 8) | (1 << 9));
        assert_eq!(aic.current_priority, Some(IrqPriority(0x20)));

        aic.write(0x0384, 4, 1 << 8);
        assert_eq!(aic.current_priority, Some(IrqPriority(0x40)));

        aic.write(0x0384, 4, 1 << 9);
        assert_eq!(aic.current_priority, None);
    }

    fn aic_machine(aic: Aic) -> Machine {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.bus
            .map(BusRegion {
                base: 0x23b100000,
                size: 0x30000,
                device: Box::new(aic),
            })
            .unwrap();
        m
    }

    fn aic_of(m: &mut Machine) -> &mut Aic {
        m.bus
            .find_device_by_id(3)
            .unwrap()
            .as_any_mut()
            .downcast_mut::<Aic>()
            .unwrap()
    }

    #[test]
    fn test_aic_ppi_targets_only_its_core() {
        let mut m = aic_machine(Aic::new(3, 128));

        // Raise PPI 16 in the core-2 redistributor bank only.
        let aic = aic_of(&mut m);
        aic.redistributors[2].ppi[0].enabled = true;
        aic.redistributors[2].ppi[0].pending = true;

        m.tick_devices(1);

        assert_eq!(
            m.cores[2].pending_irq,
            Some(16),
            "targeted core must receive the PPI"
        );
        assert_eq!(
            m.cores.active().pending_irq,
            None,
            "active core (0) must not be perturbed"
        );
        assert_eq!(m.cores[0].pending_irq, None);
        assert_eq!(m.cores[1].pending_irq, None);
        assert_eq!(m.cores[3].pending_irq, None);
    }

    #[test]
    fn test_aic_spi_is_broadcast() {
        let mut m = aic_machine(Aic::new(3, 128));

        let aic = aic_of(&mut m);
        aic.enable(40);
        aic.raise(40);

        m.tick_devices(1);

        assert_eq!(
            m.cores.active().pending_irq,
            Some(40),
            "SPI is shared, active core notified"
        );
        assert_eq!(m.cores[0].pending_irq, Some(40));
        assert_eq!(m.cores[1].pending_irq, Some(40));
        assert_eq!(m.cores[2].pending_irq, Some(40));
        assert_eq!(m.cores[3].pending_irq, Some(40));
    }

    #[test]
    fn test_aic_wakes_parked_core() {
        let mut m = aic_machine(Aic::new(3, 128));

        // Core 2 parked on WFI.
        m.cores[2].run_state = iphem_core::CpuRunState::WaitingForInterrupt;
        assert!(m.select_cpu(2));
        m.cores.active_mut().run_state = iphem_core::CpuRunState::WaitingForInterrupt;

        let aic = aic_of(&mut m);
        aic.redistributors[2].ppi[0].enabled = true;
        aic.redistributors[2].ppi[0].pending = true;

        m.tick_devices(1);

        assert_eq!(m.cores[2].pending_irq, Some(16));
        assert_eq!(
            m.cores[2].run_state,
            iphem_core::CpuRunState::Running,
            "core 2 must wake on its PPI"
        );
    }

    #[test]
    fn test_generic_timer_uses_aic_timer_fiq_line() {
        let mut m = aic_machine(Aic::new(3, 128));
        m.cores.active_mut().state.cntp_cval_el0 = 1;
        m.cores.active_mut().state.cntp_ctl_el0 = 1;

        // Timer output enters the AIC's physical timer FIQ source, independent
        // of PPI configuration.
        m.tick_devices(1);
        m.tick_devices(1);
        assert_eq!(m.cores.active().pending_fiq, Some(0));
        assert_eq!(m.cores.active().pending_irq, None);
        assert!(aic_of(&mut m).timer_fiq_lines[0][0]);

        // Masking the timer deasserts the level rather than latching a stale
        // FIQ forever.
        m.cores.active_mut().state.cntp_ctl_el0 = 0b11;
        m.tick_devices(1);
        assert!(!aic_of(&mut m).timer_fiq_lines[0][0]);
        m.tick_devices(1);
        assert_eq!(m.cores.active().pending_fiq, None);
    }

    // Phase 207: cycle-accurate AIC priority, preemption, and EOI model.

    /// GICC_PMR / GICC_BPR / GICC_IAR / GICC_EOIR windows are reachable at
    /// 0x8000 + core*0x1000 and round-trip through the Device read/write path.
    #[test]
    fn gicc_pmr_bpr_ear_eoir_windows_round_trip_per_core() {
        let mut aic = Aic::new(0, 16);

        // Core 0: set PMR and BPR via the GICC write path.
        assert_eq!(aic.write(0x8000, 4, 0x40), DeviceResult::Ok); // GICC_PMR
        assert_eq!(aic.write(0x8004, 4, 0x04), DeviceResult::Ok); // GICC_BPR
        assert_eq!(aic.read(0x8000, 4), DeviceResult::Value(0x40));
        assert_eq!(aic.read(0x8004, 4), DeviceResult::Value(0x04));

        // Core 1 has its own PMR/BPR at 0x9000/0x9004 (the GICC window is
        // 0x8000 + core*0x1000; core 1 starts at 0x9000).
        assert_eq!(aic.write(0x9000, 4, 0x10), DeviceResult::Ok);
        assert_eq!(aic.read(0x9000, 4), DeviceResult::Value(0x10));
        assert_eq!(aic.read(0x8000, 4), DeviceResult::Value(0x40)); // core 0 unchanged
    }

    /// Reading GICC_IAR acknowledges the highest-priority pending, enabled,
    /// PMR-passing IRQ for that core and pushes its priority onto the
    /// running-priority stack. A lower-priority pending IRQ cannot preempt it
    /// until EOIR pops the stack.
    #[test]
    fn gicc_iar_acknowledges_highest_priority_and_eoir_restores() {
        let mut aic = Aic::new(0, 32);
        // Two SPIs: IRQ 5 (HIGH=32) and IRQ 9 (NORMAL=64).
        aic.set_priority(5, IrqPriority::HIGH);
        aic.set_priority(9, IrqPriority::NORMAL);
        aic.enable(5);
        aic.enable(9);
        aic.raise(5);
        aic.raise(9);

        // First IAR read takes the higher-priority IRQ 5.
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(5));
        // A second IAR read while IRQ 5 is still active returns spurious
        // (nothing higher can preempt it; the pending IRQ 9 is blocked by
        // the running priority).
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(0x3ff));
        // EOIR pops the stack; IRQ 5 is no longer active.
        assert_eq!(aic.write(0x8010, 4, 5), DeviceResult::Ok);
        // Now the next IAR read takes IRQ 9.
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(9));
        assert_eq!(aic.write(0x8010, 4, 9), DeviceResult::Ok);
        // Nothing left: spurious.
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(0x3ff));
    }

    /// GICC_PMR blocks IRQs whose priority is >= pmr, even when pending and
    /// enabled.
    #[test]
    fn gicc_pmr_blocks_low_priority_irqs() {
        let mut aic = Aic::new(0, 32);
        aic.set_priority(7, IrqPriority::NORMAL);
        aic.enable(7);
        aic.raise(7);
        // PMR = 0x40: only priority < 0x40 passes. NORMAL (64) is blocked.
        assert_eq!(aic.write(0x8000, 4, 0x40), DeviceResult::Ok);
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(0x3ff));
        // Lower PMR lets it through.
        assert_eq!(aic.write(0x8000, 4, 0xff), DeviceResult::Ok);
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(7));
    }

    /// A higher-priority IRQ preempts a lower one mid-service: the running
    /// priority stack nests, and EOIR unwinds in order.
    #[test]
    fn higher_priority_irq_preempts_lower_and_eoir_unwinds() {
        let mut aic = Aic::new(0, 32);
        aic.set_priority(1, IrqPriority::NORMAL);
        aic.set_priority(2, IrqPriority::HIGH);
        aic.enable(1);
        aic.enable(2);
        aic.raise(1);
        aic.raise(2);

        // IRQ 2 (HIGH) is acknowledged first.
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(2));
        // A second IAR read while IRQ 2 is still active returns spurious
        // (nothing higher can preempt it).
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(0x3ff));
        // EOIR for IRQ 2 unwinds to the lower priority.
        assert_eq!(aic.write(0x8010, 4, 2), DeviceResult::Ok);
        // IRQ 1 is now servicable.
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(1));
        assert_eq!(aic.write(0x8010, 4, 1), DeviceResult::Ok);
        assert_eq!(aic.read_gicc(0, 0x0c), DeviceResult::Value(0x3ff));
    }
}
