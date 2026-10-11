use crate::{
    bus::{Bus, BusError, MemTransaction, SecurityState, TransactionKind},
    cpu::{Cpu, CpuRunState, ExceptionLevel},
    dma::{DartPort, DmaAccess, DmaFault, DmaIommu, DmaRequest, DmaResult},
    mmu::MmuFault,
    scheduler::Scheduler,
    tlb::Tlb,
    trace::{TraceEngine, TraceKind},
};

pub type MachineHook = Box<dyn FnMut(&mut Machine)>;
pub type PmgrSyncCallback = Box<dyn FnMut(&mut Bus)>;

pub struct MachineConfig {
    pub trace: bool,
    pub quantum: u64,
}

/// Single owner for processor cores and the currently selected core.
#[derive(Clone, Debug)]
pub struct CoreSet {
    cpus: Vec<Cpu>,
    active: usize,
}

impl CoreSet {
    fn new(cpus: Vec<Cpu>) -> Self {
        assert!(
            !cpus.is_empty(),
            "a machine must have at least one CPU core"
        );
        Self { cpus, active: 0 }
    }

    pub fn active(&self) -> &Cpu {
        &self.cpus[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Cpu {
        &mut self.cpus[self.active]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn len(&self) -> usize {
        self.cpus.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cpus.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Cpu> {
        self.cpus.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Cpu> {
        self.cpus.iter_mut()
    }

    pub fn get(&self, index: usize) -> Option<&Cpu> {
        self.cpus.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Cpu> {
        self.cpus.get_mut(index)
    }

    #[must_use]
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.cpus.len() {
            return false;
        }
        self.active = index;
        true
    }
}

impl std::ops::Index<usize> for CoreSet {
    type Output = Cpu;

    fn index(&self, index: usize) -> &Self::Output {
        &self.cpus[index]
    }
}

impl std::ops::IndexMut<usize> for CoreSet {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.cpus[index]
    }
}

pub struct Machine {
    /// Authoritative processor state and the core selected by the interpreter.
    pub cores: CoreSet,
    pub bus: Bus,
    pub scheduler: Scheduler,
    pub trace: TraceEngine,
    pub quantum: u64,
    /// Timer ticks the architectural counters advance per host tick.
    ///
    /// `CNTVCT_EL0`/`CNTPCT_EL0` derive from the scheduler cycle, which advances
    /// once per host tick. At 1 the modelled counter runs one tick per retired
    /// guest instruction, so a guest timeout costs its whole tick count in host
    /// instructions: the T8020 SecureROM's first `CNTVCT_EL0` delay loop wants
    /// 0x960 = 2400 ticks, and a boot containing many such waits cannot finish
    /// in any practical instruction budget.
    ///
    /// Real silicon runs the generic timer at CNTFRQ (24 MHz on this SoC) while
    /// the core retires instructions far faster, so one guest instruction spans
    /// many counter ticks. Raising this lets guest timeouts elapse in a
    /// realistic number of host instructions while preserving their ordering and
    /// relative durations. Defaults to 1, which is the previous behaviour.
    pub counter_ticks_per_tick: u64,
    pub tlb: Tlb,
    pub hooks: std::collections::HashMap<u64, MachineHook>,
    /// Optional callback invoked after device ticks to synchronize PMGR
    /// boot-status with power-state registers. Provided by the SoC layer
    /// to avoid circular dependency between iphem-core and iphem-devices.
    pub pmgr_sync: Option<PmgrSyncCallback>,
    /// Faulting instruction address while the interpreter executes one A64 instruction.
    pub current_instruction_pc: Option<u64>,
    /// DMA IOMMU ports, one per bound stream ID.
    pub dart_ports: Vec<DartPort>,
    /// Device-originated DMA staged by the transport/host layer, drained when
    /// a bus device raises `DeviceEvent::DmaRequest { channel }`.
    pub dma_pending: Vec<(u32, DmaRequest)>,
    /// Optional guest-VA write-watch: when set to `(base, size)`, every CPU
    /// store whose VA range overlaps the window appends
    /// `(step_counter, pc, va, size, value)` to `write_watch_log`. Host-side
    /// diagnostic support for locating the writer that corrupts a heap object;
    /// `step_counter` is advanced by the host loop, not by the CPU.
    pub write_watch_va: Option<(u64, u64)>,
    pub write_watch_log: Vec<(u64, u64, u64, usize, u64)>,
    /// Optional physical-address write-watch: when set to `(base, size)`, every
    /// CPU store whose translated PA range overlaps the window appends
    /// `(step_counter, pc, va, pa, size, value)` to `write_watch_pa_log`.
    pub write_watch_pa: Option<(u64, u64)>,
    pub write_watch_pa_log: Vec<(u64, u64, u64, u64, usize, u64)>,
    /// Host-advanced instruction counter, recorded in `write_watch_log`.
    pub step_counter: u64,
    /// Enables the historically broad EL3 identity-map workaround used by
    /// early boot experiments. Disabled by default so missing page tables fault.
    pub legacy_el3_identity_map: bool,
}

impl Machine {
    pub fn new(c: MachineConfig) -> Self {
        let mut cpus = Vec::new();
        // 4 SMP cores for Apple T8020
        for _ in 0..4 {
            cpus.push(Cpu::new());
        }
        Self {
            cores: CoreSet::new(cpus),
            bus: Bus::new(),
            scheduler: Scheduler::default(),
            trace: TraceEngine::new(c.trace),
            quantum: c.quantum.max(1),
            counter_ticks_per_tick: 1,
            tlb: Tlb::new(256),
            hooks: std::collections::HashMap::new(),
            pmgr_sync: None,
            current_instruction_pc: None,
            dart_ports: Vec::new(),
            dma_pending: Vec::new(),
            write_watch_va: None,
            write_watch_log: Vec::new(),
            write_watch_pa: None,
            write_watch_pa_log: Vec::new(),
            step_counter: 0,
            legacy_el3_identity_map: false,
        }
    }

    /// Set how many timer ticks the architectural counters advance per host
    /// tick. See [`Machine::counter_ticks_per_tick`]; values below 1 are clamped.
    pub fn set_counter_ticks_per_tick(&mut self, ticks: u64) {
        self.counter_ticks_per_tick = ticks.max(1);
    }

    /// Select the core used by interpreter operations. Returns `false` and
    /// leaves the current selection unchanged when `idx` is out of range.
    #[must_use]
    pub fn select_cpu(&mut self, idx: usize) -> bool {
        self.cores.select(idx)
    }

    pub fn reset(&mut self) {
        for cpu in self.cores.iter_mut() {
            *cpu = Cpu::new();
        }
        let selected = self.cores.select(0);
        debug_assert!(selected);
        self.scheduler = Scheduler::default();
        self.bus.reset();
        self.trace.clear();
        self.tlb.invalidate_all();
        self.dma_pending.clear();
        self.current_instruction_pc = None;
        self.step_counter = 0;
        self.write_watch_log.clear();
        self.write_watch_pa_log.clear();
        let pc = self.cores.active().state.pc;
        let el = self.cores.active().state.el as u8;
        let pstate = self.cores.active().state.pstate;
        self.trace
            .record(0, 0, 0, TraceKind::Reset, pc, el, pstate, None);
    }

    pub fn load_rom(&mut self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let data = std::fs::read(path)?;
        let rom = crate::memory::Rom::new(0x100000000, data.clone());
        self.bus
            .map(crate::bus::BusRegion {
                base: 0x100000100,
                size: data.len() as u64,
                device: Box::new(rom),
            })
            .map_err(|e| std::io::Error::other(String::from(e)))?;
        Ok(())
    }

    pub fn register_hook(&mut self, pc: u64, f: Box<dyn FnMut(&mut Machine)>) {
        self.hooks.insert(pc, f);
    }

    pub fn translate(&mut self, a: u64, w: bool, x: bool) -> Result<u64, BusError> {
        // EL3 uses its own translation regime. If SCTLR_EL3.M is clear, EL3
        // accesses are physical/identity-mapped even when lower exception
        // levels have already programmed and enabled SCTLR_EL1.M. Real
        // SecureROM/SEPROM code commonly prepares EL1 translation state while
        // still running at EL3; those EL1 settings must not affect EL3 data
        // accesses.
        if self.cores.active().state.el == ExceptionLevel::El3
            && self.cores.active().state.sctlr_el3 & 1 == 0
        {
            return Ok(a);
        }

        // Optional compatibility fallback: if at EL3 with MMU on but no EL3 page tables,
        // use identity mapping for the bootloader DRAM region, USB device
        // controller, and all firmware load regions used by the DFU host path.
        if self.legacy_el3_identity_map && self.cores.active().state.el == ExceptionLevel::El3 {
            if (0x19c000000..0x1a4000000).contains(&a) {
                return Ok(a);
            }
            if (0x20a800000..0x20a900000).contains(&a) {
                return Ok(a);
            }
            // SecureROM / iBoot instruction ROM window (execute-through while
            // the EL1 MMU registers are being programmed at EL3).
            if (0x100000000..0x101000000).contains(&a) {
                return Ok(a);
            }
            // iBSS / iBoot load region
            if (0x1000000000..0x1000400000).contains(&a) {
                return Ok(a);
            }
            // iBEC load region
            if (0x180000000..0x180400000).contains(&a) {
                return Ok(a);
            }
            // Kernel load region (58MB at 0x800000)
            if (0x800000..0x4000000).contains(&a) {
                return Ok(a);
            }
            // DeviceTree load region
            if (0x80000000..0x80800000).contains(&a) {
                return Ok(a);
            }
        }
        // Standard MMU-off identity mapping for EL1/EL2
        if self.cores.active().state.sctlr_el1 & 1 == 0
            && self.cores.active().state.hcr_el2 & 1 == 0
        {
            return Ok(a);
        }
        let asid_ttbr = if self.cores.active().state.tcr_el1 & (1 << 22) != 0 {
            self.cores.active().state.ttbr1_el1 // TCR_EL1.A1
        } else {
            self.cores.active().state.ttbr0_el1
        };
        let asid = ((asid_ttbr >> 48) & 0xffff) as u16;
        let vmid = if self.cores.active().state.hcr_el2 & 1 != 0 {
            ((self.cores.active().state.vttbr_el2 >> 48) & 0xffff) as u16
        } else {
            0
        };
        let unprivileged = self.cores.active().state.el == ExceptionLevel::El0;
        if let Some(t) = self.tlb.lookup(a, asid, vmid, unprivileged, w, x) {
            return Ok(t.pa);
        }
        let t = crate::mmu::Mmu::translate(
            &self.cores.active().state,
            &mut self.bus,
            &mut self.trace,
            a,
            w,
            x,
        )
        .map_err(|fault| {
            let level = u64::from(fault.level() & 0x3);
            let fault_status = match fault {
                MmuFault::Translation { .. } | MmuFault::InvalidDescriptor { .. } => 0x04 | level,
                MmuFault::AccessFlag { .. } => 0x08 | level,
                MmuFault::Permission { .. } => 0x0c | level,
                MmuFault::AddressSize { .. } | MmuFault::Disabled => level,
                MmuFault::Stage2TableWalk { kind, .. } => match kind {
                    crate::mmu::TableWalkFaultKind::Translation
                    | crate::mmu::TableWalkFaultKind::InvalidDescriptor => 0x04 | level,
                    crate::mmu::TableWalkFaultKind::AccessFlag => 0x08 | level,
                    crate::mmu::TableWalkFaultKind::Permission => 0x0c | level,
                    crate::mmu::TableWalkFaultKind::AddressSize => level,
                },
            };
            let source_el = self.cores.active().state.el;
            let target = match (fault.is_stage2(), source_el) {
                (true, ExceptionLevel::El0 | ExceptionLevel::El1) => ExceptionLevel::El2,
                (false, ExceptionLevel::El0) => ExceptionLevel::El1,
                _ => source_el,
            };
            let current_el = source_el == target;
            let ec = if x {
                if current_el {
                    0x21
                } else {
                    0x20
                }
            } else if current_el {
                0x25
            } else {
                0x24
            };
            let write_not_read = u64::from(w && !x) << 6;
            let s1ptw = u64::from(fault.is_stage2_table_walk()) << 7;
            // AArch64 instructions are always 32-bit, so ESR.IL must
            // identify the faulting instruction as 32-bit.
            let esr = (ec << 26) | (1 << 25) | s1ptw | write_not_read | fault_status;
            let return_address = self
                .current_instruction_pc
                .unwrap_or(self.cores.active().state.pc);
            crate::exception::take_sync(
                self.cores.active_mut(),
                &mut self.trace,
                target,
                esr,
                a,
                return_address,
            );
            if fault.is_stage2() {
                // HPFAR_EL2.FIPA[39:4] contains IPA[47:12].
                let ipa = fault.fault_address().unwrap_or(a);
                self.cores.active_mut().state.hpfar_el2 = (ipa & 0x0000_ffff_ffff_f000) >> 8;
            }
            BusError::Unmapped(a)
        })?;
        self.tlb.insert(a, asid, vmid, unprivileged, t);
        Ok(t.pa)
    }

    pub fn mem_read(&mut self, a: u64, s: usize) -> Result<u64, BusError> {
        self.check_data_alignment(a, s, false)?;
        let pa = self.translate(a, false, false)?;
        let pc = self.cores.active().state.pc;
        let el = self.cores.active().state.el as u8;
        let pstate = self.cores.active().state.pstate;
        self.bus.read(
            MemTransaction {
                address: pa,
                size: s,
                kind: TransactionKind::Read,
                security: SecurityState::Secure,
                source: None,
            },
            &mut self.trace,
            pc,
            el,
            pstate,
        )
    }

    pub fn mem_write(&mut self, a: u64, s: usize, v: u64) -> Result<(), BusError> {
        self.check_data_alignment(a, s, true)?;
        let pa = self.translate(a, true, false)?;
        self.cores.active_mut().exclusive_address = None;
        self.cores.active_mut().exclusive_size = 0;
        let pc = self.cores.active().state.pc;
        let watch_step = self.step_counter;
        let mut logged_watch = false;
        if let Some((wbase, wsize)) = self.write_watch_va {
            if ranges_overlap(wbase, wsize, a, s as u64) && self.write_watch_log.len() < 1_000_000 {
                self.write_watch_log.push((
                    watch_step,
                    self.current_instruction_pc.unwrap_or(pc),
                    a,
                    s,
                    v,
                ));
                logged_watch = true;
            }
        }
        if let Some((wbase, wsize)) = self.write_watch_pa {
            if ranges_overlap(wbase, wsize, pa, s as u64)
                && self.write_watch_pa_log.len() < 1_000_000
            {
                self.write_watch_pa_log.push((
                    watch_step,
                    self.current_instruction_pc.unwrap_or(pc),
                    a,
                    pa,
                    s,
                    v,
                ));
                logged_watch = true;
            }
        }
        if logged_watch {
            self.step_counter += 1;
        }
        let el = self.cores.active().state.el as u8;
        let pstate = self.cores.active().state.pstate;
        self.bus.write(
            MemTransaction {
                address: pa,
                size: s,
                kind: TransactionKind::Write,
                security: SecurityState::Secure,
                source: None,
            },
            v,
            &mut self.trace,
            pc,
            el,
            pstate,
        )
    }

    pub fn fetch32(&mut self, a: u64) -> Result<u32, BusError> {
        let pa = self.translate(a, false, true)?;
        let pc = self.cores.active().state.pc;
        let el = self.cores.active().state.el as u8;
        let pstate = self.cores.active().state.pstate;
        let val = self.bus.read(
            MemTransaction {
                address: pa,
                size: 4,
                kind: TransactionKind::Execute,
                security: SecurityState::Secure,
                source: None,
            },
            &mut self.trace,
            pc,
            el,
            pstate,
        )? as u32;
        Ok(val)
    }

    fn check_data_alignment(
        &mut self,
        address: u64,
        size: usize,
        write: bool,
    ) -> Result<(), BusError> {
        let sctlr = match self.cores.active().state.el {
            ExceptionLevel::El0 | ExceptionLevel::El1 => self.cores.active().state.sctlr_el1,
            ExceptionLevel::El2 => self.cores.active().state.sctlr_el2,
            ExceptionLevel::El3 => self.cores.active().state.sctlr_el3,
        };
        let applies = sctlr & (1 << 1) != 0;
        if !applies || size <= 1 || address.is_multiple_of(size as u64) {
            return Ok(());
        }
        let source = self.cores.active().state.el;
        let target = if source == ExceptionLevel::El0 {
            ExceptionLevel::El1
        } else {
            source
        };
        let ec = if source == target { 0x25 } else { 0x24 };
        let esr = (ec << 26) | (1 << 25) | (u64::from(write) << 6) | 0x21;
        let return_address = self
            .current_instruction_pc
            .unwrap_or(self.cores.active().state.pc);
        crate::exception::take_sync(
            self.cores.active_mut(),
            &mut self.trace,
            target,
            esr,
            address,
            return_address,
        );
        Err(BusError::Unmapped(address))
    }

    pub fn mem_write_bytes(&mut self, a: u64, data: &[u8]) -> Result<(), BusError> {
        for (i, &b) in data.iter().enumerate() {
            self.mem_write(a + i as u64, 1, b as u64)?;
        }
        Ok(())
    }

    /// Explicitly enable the legacy EL3 identity-map workaround for firmware
    /// experiments that depend on the old permissive boot behavior.
    pub fn enable_legacy_el3_identity_map(&mut self) {
        self.legacy_el3_identity_map = true;
    }

    /// Phase 190 SPE profiling-buffer record push. Models statistical-profiling
    /// hardware completing a sample record: the record bytes are written through
    /// the physical bus at the current PMBPTR_EL1 (guest RAM-backed), the pointer
    /// advances by the amount written, and the PMBSR_EL1 completion bits latch
    /// (S=bit16 requested-service; DL=bit24 set when the wrap limit is crossed,
    /// in which case the record is truncated at the window end and the pointer
    /// wraps back to the PMBLIMITR base, the single-writer fill-mode behaviour).
    /// The buffer window is `1 << (12 + Align)` bytes from the PMBLIMITR base
    /// (Align = PMBIDR bits[4:3]); the default PMBIDR value 0x1 selects a 4KiB
    /// window, so a synthetic record stream lands in the DT RAM mapping.
    ///
    /// A write that walks off the mapped physical memory (no bus region covers
    /// the pointer) aborts like a data access: the faulting address is returned
    /// as the error, recorded in FAR_EL1, and PMBSR latches EC=buffer-management
    /// error. A disabled buffer (E=0) drops the record and reports success.
    pub fn spe_buf_push(&mut self, data: &[u8]) -> Result<(), u64> {
        const S: u64 = 1 << 16;
        const DL: u64 = 1 << 24;
        const EC_MSK: u64 = 0x1f;
        if self.cores.active().state.spe_pmblimitr & 1 == 0 {
            return Ok(());
        }
        let align = (self.cores.active().state.spe_pmbidr >> 3) & 0x3;
        let base = self.cores.active().state.spe_pmblimitr & 0xffff_ffff_ffff_f000;
        let window = 1u64 << (12 + align);
        let ptr = self.cores.active().state.spe_pmbptr & 0x00ff_ffff_ffff_ffff;
        let offset = ptr.saturating_sub(base);
        if data.is_empty() {
            return Ok(());
        }
        // A pointer outside the buffer window (before base or past base+window)
        // is a buffer-management error, exactly like a record write that cannot
        // be placed in the buffer.
        if offset >= window {
            let st = &mut self.cores.active_mut().state;
            st.spe_pmbsr = (st.spe_pmbsr & !EC_MSK) | (S | 0x2);
            st.far_el1 = ptr;
            return Err(ptr);
        }
        let room = (window - offset) as usize;
        let take = room.min(data.len());
        if take > 0 {
            if let Err(e) = self.mem_write_bytes(ptr, &data[..take]) {
                let far = match e {
                    BusError::Unmapped(a) => a,
                    _ => ptr.saturating_add(take as u64),
                };
                let st = &mut self.cores.active_mut().state;
                st.spe_pmbsr = (st.spe_pmbsr & !EC_MSK) | (S | 0x2);
                st.far_el1 = far;
                return Err(far);
            }
        }
        let next = ptr.saturating_add(take as u64);
        let crossed = next >= base.saturating_add(window);
        let st = &mut self.cores.active_mut().state;
        st.spe_pmbptr = (st.spe_pmbptr & !0x00ff_ffff_ffff_ffff)
            | (if crossed {
                base
            } else {
                next & 0x00ff_ffff_ffff_ffff
            });
        st.spe_pmbsr = (st.spe_pmbsr & !EC_MSK) | if crossed { S | DL } else { S };
        Ok(())
    }

    /// Load bytes into physical RAM before guest execution without emitting a
    /// bus transaction for every byte of a large firmware image.
    pub fn load_physical_bytes(&mut self, address: u64, data: &[u8]) -> Result<(), BusError> {
        let end = address
            .checked_add(data.len() as u64)
            .ok_or(BusError::Unmapped(address))?;
        for region in &mut self.bus.regions {
            let region_end = region.base.saturating_add(region.size);
            if address >= region.base && end <= region_end {
                let offset = address - region.base;
                if let Some(ram) = region
                    .device
                    .as_any_mut()
                    .downcast_mut::<crate::SparseRam>()
                {
                    return if ram.load(offset, data) {
                        Ok(())
                    } else {
                        Err(BusError::Unmapped(address))
                    };
                }
                if let Some(ram) = region.device.as_any_mut().downcast_mut::<crate::Ram>() {
                    return if ram.load(offset as usize, data) {
                        Ok(())
                    } else {
                        Err(BusError::Unmapped(address))
                    };
                }
                break;
            }
        }
        // A host physical load must never fall back to translated guest writes
        // or to an MMIO device that silently discards the firmware bytes.
        Err(BusError::Unmapped(address))
    }

    /// Copy bytes from a physical RAM/ROM region without issuing guest bus
    /// transactions. This is intended for host-side trace/capture artifacts:
    /// unlike `mem_read_bytes`, it neither translates through the guest MMU nor
    /// records MMIO side effects while taking snapshots.
    pub fn copy_physical_bytes(&self, address: u64, out: &mut [u8]) -> Result<(), BusError> {
        let end = address
            .checked_add(out.len() as u64)
            .ok_or(BusError::Unmapped(address))?;
        for region in &self.bus.regions {
            let region_end = region.base.saturating_add(region.size);
            if address >= region.base && end <= region_end {
                let offset = address - region.base;
                if let Some(ram) = region.device.as_any().downcast_ref::<crate::SparseRam>() {
                    return if ram.copy_out(offset, out) {
                        Ok(())
                    } else {
                        Err(BusError::Unmapped(address))
                    };
                }
                if let Some(ram) = region.device.as_any().downcast_ref::<crate::Ram>() {
                    return if ram.copy_out(offset, out) {
                        Ok(())
                    } else {
                        Err(BusError::Unmapped(address))
                    };
                }
                if let Some(rom) = region.device.as_any().downcast_ref::<crate::Rom>() {
                    let Some(start) = usize::try_from(offset).ok() else {
                        return Err(BusError::Unmapped(address));
                    };
                    let Some(end) = start.checked_add(out.len()) else {
                        return Err(BusError::Unmapped(address));
                    };
                    if end <= rom.data.len() {
                        out.copy_from_slice(&rom.data[start..end]);
                        return Ok(());
                    }
                    return Err(BusError::Unmapped(address));
                }
                return Err(BusError::AccessViolation);
            }
        }
        Err(BusError::Unmapped(address))
    }

    pub fn mem_read_bytes(&mut self, a: u64, size: usize) -> Result<Vec<u8>, BusError> {
        let mut buf = vec![0u8; size];
        for (i, byte) in buf.iter_mut().enumerate().take(size) {
            let val = self.mem_read(a + i as u64, 1)?;
            *byte = val as u8;
        }
        Ok(buf)
    }

    /// Register the IOMMU and interrupt for a stream ID. A DMA request whose
    /// SID is not registered faults as [`DmaFault::UnboundSid`].
    pub fn register_dma_iommu(&mut self, sid: u32, irq: u32, iommu: Box<dyn DmaIommu>) {
        self.dart_ports.retain(|port| port.sid != sid);
        self.dart_ports.push(DartPort { sid, irq, iommu });
    }

    /// Stage a device-originated DMA request under `channel`. When a bus
    /// device raises `DeviceEvent::DmaRequest { channel }`, [`Self::tick_devices`]
    /// drains the staged request through [`Self::dma_translate`]. The
    /// transport/host layer supplies the transfer details because the device
    /// model has no hands on the machine; the device only announces readiness.
    /// Staging a second request for the same channel replaces the first.
    pub fn submit_dma(&mut self, channel: u32, request: DmaRequest) {
        self.dma_pending.retain(|(c, _)| *c != channel);
        self.dma_pending.push((channel, request));
    }

    fn drain_dma(&mut self, channel: u32) {
        if let Some(pos) = self.dma_pending.iter().position(|(c, _)| *c == channel) {
            let (_, request) = self.dma_pending.remove(pos);
            let _ = self.dma_translate(&request);
        }
    }

    /// Translate a device-originated DMA request through the IOMMU bound to
    /// its SID, then (on success) leaves the caller with the physical
    /// address. Translation reads page-table entries from the physical bus,
    /// never through the CPU MMU. A failed translation raises the interrupt
    /// mapped to the SID and returns the typed fault — it never silently
    /// resolves to zero.
    pub fn dma_translate(&mut self, request: &DmaRequest) -> DmaResult {
        let Some(index) = self
            .dart_ports
            .iter()
            .position(|port| port.sid == request.sid)
        else {
            return DmaResult::Fault(DmaFault::UnboundSid(request.sid));
        };
        let irq = self.dart_ports[index].irq;
        let iommu = &self.dart_ports[index].iommu;
        let bus = &mut self.bus;
        let trace = &mut self.trace;
        let sid = request.sid;
        let mut read = |address: u64| -> Option<u64> {
            let tx = MemTransaction {
                address,
                size: 8,
                kind: TransactionKind::Read,
                security: SecurityState::NonSecure,
                source: Some(sid),
            };
            bus.read(tx, trace, 0, 0, 0).ok()
        };
        let result = iommu.translate(request, &mut read);
        if matches!(result, DmaResult::Fault(_)) {
            // Prefer the interrupt controller's own line if one owns it (the
            // AIC on a full SoC), so the fault reaches the CPU through the
            // controller; fall back to direct delivery on a bare machine.
            if !self
                .bus
                .signal_interrupt(irq, Some(self.cores.active_index()))
            {
                self.deliver_interrupt(irq, None);
            }
        }
        result
    }

    /// Translate `request` and move its payload over the physical bus at the
    /// translated address. For a [`DmaAccess::Read`] the payload is copied
    /// from guest memory into `buf`; for a [`DmaAccess::Write`] the bytes in
    /// `buf` are written into guest memory. `buf` must hold at least
    /// `request.size` bytes. The accesses go through the physical bus directly
    /// (never the CPU Stage-1/Stage-2 MMU) and carry the SID as the
    /// transaction source. The transfer is chunked at the bus word width
    /// little-endian, so multi-word transfers stay architectural and no host
    /// pointer is ever derived from a guest address. A translation failure
    /// raises the mapped interrupt and returns the typed fault without
    /// touching the payload; a payload access that aborts mid-transfer
    /// returns [`DmaFault::TargetAborted`] with the offending address (the
    /// target may be partially written for a write).
    pub fn dma_transfer(&mut self, request: &DmaRequest, buf: &mut [u8]) -> DmaResult {
        let pa = match self.dma_translate(request) {
            DmaResult::Translated(pa) | DmaResult::Bypass(pa) => pa,
            fault => return fault,
        };
        let size = usize::try_from(request.size).unwrap_or(usize::MAX);
        if buf.len() < size {
            return DmaResult::Fault(DmaFault::TargetAborted(pa));
        }
        let mut offset = 0usize;
        while offset < size {
            let chunk = (size - offset).min(8);
            let address = pa.saturating_add(offset as u64);
            let kind = match request.access {
                DmaAccess::Read => TransactionKind::Read,
                DmaAccess::Write => TransactionKind::Write,
            };
            let tx = MemTransaction {
                address,
                size: chunk,
                kind,
                security: SecurityState::NonSecure,
                source: Some(request.sid),
            };
            let pc = self.cores.active().state.pc;
            let el = self.cores.active().state.el as u8;
            let pstate = self.cores.active().state.pstate;
            if request.access == DmaAccess::Write {
                let mut value = 0u64;
                for i in 0..chunk {
                    value |= (buf[offset + i] as u64) << (i * 8);
                }
                if self
                    .bus
                    .write(tx, value, &mut self.trace, pc, el, pstate)
                    .is_err()
                {
                    return DmaResult::Fault(DmaFault::TargetAborted(address));
                }
                offset += chunk;
                continue;
            }
            let value = match self.bus.read(tx, &mut self.trace, pc, el, pstate) {
                Ok(v) => v,
                Err(_) => return DmaResult::Fault(DmaFault::TargetAborted(address)),
            };
            for i in 0..chunk {
                buf[offset + i] = (value >> (i * 8)) as u8;
            }
            offset += chunk;
        }
        DmaResult::Translated(pa)
    }

    /// Translate with invalidation of the IOMMU's cached state for the
    /// stream/address range.
    pub fn dma_invalidate(&mut self, sid: u32, iova: u64, size: u64) {
        if let Some(index) = self.dart_ports.iter().position(|port| port.sid == sid) {
            self.dart_ports[index].iommu.invalidate(sid, iova, size);
        }
    }

    pub fn tick_devices(&mut self, c: u64) {
        let pc = self.cores.active().state.pc;
        let el = self.cores.active().state.el as u8;
        let pstate = self.cores.active().state.pstate;

        for e in self.bus.tick(c, &mut self.trace, pc, el, pstate) {
            match e {
                crate::device::DeviceEvent::Interrupt(i) => self.deliver_interrupt(i, None),
                crate::device::DeviceEvent::InterruptTo(i, core) => self.deliver_interrupt(i, core),
                crate::device::DeviceEvent::Fiq(i) => self.deliver_fiq(i, None),
                crate::device::DeviceEvent::FiqTo(i, core) => self.deliver_fiq(i, core),
                crate::device::DeviceEvent::SError(iss) => self.deliver_serror(iss, None),
                crate::device::DeviceEvent::SErrorTo(iss, core) => self.deliver_serror(iss, core),
                crate::device::DeviceEvent::DmaRequest { channel } => self.drain_dma(channel),
            }
        }

        if let Some(sync) = self.pmgr_sync.as_mut() {
            sync(&mut self.bus);
        }

        // Periodic scheduler preemption: delivered on the active core.
        if let Some(irq) = self.scheduler.preemption_due() {
            self.deliver_interrupt(irq, Some(self.cores.active_index()));
        }

        self.scheduler
            .tick(c.wrapping_mul(self.counter_ticks_per_tick));
        let counter = self.scheduler.cycle();
        let physical_counter = counter.wrapping_sub(self.cores.active().state.cntpoff_el2);
        let physical_timer_expired = self.cores.active().state.cntp_ctl_el0 & 1 != 0
            && (physical_counter.wrapping_sub(self.cores.active().state.cntp_cval_el0) as i64) >= 0;
        let virtual_counter = counter.wrapping_sub(self.cores.active().state.cntvoff_el2);
        let virtual_timer_expired = self.cores.active().state.cntv_ctl_el0 & 1 != 0
            && (virtual_counter.wrapping_sub(self.cores.active().state.cntv_cval_el0) as i64) >= 0;
        // Apple AIC routes the physical/virtual architectural timers as FIQ
        // sources. Other interrupt-controller implementations use the
        // architectural timer PPIs (27/30). Lines are levels, so every tick
        // updates the source state and lets the controller deassert it when
        // the compare or mask changes.
        let virtual_timer_fiq =
            virtual_timer_expired && self.cores.active().state.cntv_ctl_el0 & 2 == 0;
        let physical_timer_fiq =
            physical_timer_expired && self.cores.active().state.cntp_ctl_el0 & 2 == 0;
        let virtual_fiq_routed =
            self.bus
                .set_fiq_line(1, Some(self.cores.active_index()), virtual_timer_fiq);
        let physical_fiq_routed =
            self.bus
                .set_fiq_line(0, Some(self.cores.active_index()), physical_timer_fiq);
        if virtual_fiq_routed && !virtual_timer_fiq {
            self.clear_pending_fiq_for_core(1, self.cores.active_index());
        }
        if physical_fiq_routed && !physical_timer_fiq {
            self.clear_pending_fiq_for_core(0, self.cores.active_index());
        }
        if virtual_timer_fiq
            && !virtual_fiq_routed
            && !self
                .bus
                .signal_interrupt(27, Some(self.cores.active_index()))
        {
            self.deliver_interrupt(27, Some(self.cores.active_index()));
        }
        if physical_timer_fiq
            && !physical_fiq_routed
            && !self
                .bus
                .signal_interrupt(30, Some(self.cores.active_index()))
        {
            self.deliver_interrupt(30, Some(self.cores.active_index()));
        }
        self.trace.advance(c);
    }

    fn clear_pending_fiq_for_core(&mut self, fiq: u32, core: usize) {
        if self.cores.active_index() == core && self.cores.active().pending_fiq == Some(fiq) {
            self.cores.active_mut().pending_fiq = None;
        }
        if let Some(cpu) = self.cores.get_mut(core) {
            if cpu.pending_fiq == Some(fiq) {
                cpu.pending_fiq = None;
            }
        }
    }

    /// Route an interrupt to one core (by index) or broadcast it. A targeted
    /// interrupt for a core that is not represented by this `Machine` is left
    /// for an external companion harness; it must not be converted into an AP
    /// broadcast. Any represented target that is idle in WFI/WFE is woken back
    /// to Running.
    fn deliver_interrupt(&mut self, irq: u32, core: Option<usize>) {
        let wake = |c: &mut Cpu| {
            c.pending_irq = Some(irq);
            if matches!(c.run_state, CpuRunState::WaitingForInterrupt) {
                c.run_state = CpuRunState::Running;
            }
        };
        match core {
            Some(idx) if idx < self.cores.len() => {
                wake(&mut self.cores[idx]);
                if self.cores.active_index() == idx {
                    wake(self.cores.active_mut());
                }
            }
            Some(_) => {}
            None => {
                for c in self.cores.iter_mut() {
                    wake(c);
                }
                wake(self.cores.active_mut());
            }
        }
    }

    fn deliver_fiq(&mut self, fiq: u32, core: Option<usize>) {
        self.deliver_to_cpus(core, |cpu| cpu.pending_fiq = Some(fiq));
    }

    fn deliver_serror(&mut self, iss: u64, core: Option<usize>) {
        self.deliver_to_cpus(core, |cpu| cpu.pending_serror = Some(iss));
    }

    fn deliver_to_cpus(&mut self, core: Option<usize>, mut deliver: impl FnMut(&mut Cpu)) {
        let mut wake = |cpu: &mut Cpu| {
            deliver(cpu);
            if matches!(cpu.run_state, CpuRunState::WaitingForInterrupt) {
                cpu.run_state = CpuRunState::Running;
            }
        };
        match core {
            Some(idx) if idx < self.cores.len() => {
                wake(&mut self.cores[idx]);
                if self.cores.active_index() == idx {
                    wake(self.cores.active_mut());
                }
            }
            Some(_) => {}
            None => {
                for cpu in self.cores.iter_mut() {
                    wake(cpu);
                }
                wake(self.cores.active_mut());
            }
        }
    }

    /// Restore CPU, TLB, and physical-RAM state from a previously captured
    /// `MachineSnapshot`. The machine's bus must already be mapped so the
    /// captured physical regions land in the matching RAM backing devices.
    pub fn restore_snapshot(
        &mut self,
        snapshot: &crate::snapshot::MachineSnapshot,
    ) -> Result<(), crate::snapshot::SnapshotError> {
        snapshot.restore_into(self)
    }

    /// Convenience wrapper over the free `snapshot::capture` helper.
    pub fn capture_snapshot(
        &mut self,
    ) -> Result<crate::snapshot::MachineSnapshot, crate::snapshot::SnapshotError> {
        crate::snapshot::capture(self)
    }

    /// Arm a periodic preemption interrupt on the active core. See
    /// `Scheduler::enable_preemption`.
    pub fn enable_preemption(&mut self, irq: u32, interval_cycles: u64) {
        self.scheduler.enable_preemption(irq, interval_cycles);
    }
}

fn ranges_overlap(base: u64, size: u64, address: u64, access_size: u64) -> bool {
    let Some(end) = base.checked_add(size) else {
        return false;
    };
    let Some(access_end) = address.checked_add(access_size) else {
        return false;
    };
    address < end && base < access_end
}

#[cfg(test)]
mod tests {
    /// The architectural counters must advance in proportion to
    /// `counter_ticks_per_tick`, and the knob must default to the previous
    /// behaviour of one tick per host tick.
    ///
    /// `CNTVCT_EL0`/`CNTPCT_EL0` are derived from the scheduler cycle, which is
    /// what every guest timeout polls. The T8020 SecureROM's first
    /// `CNTVCT_EL0` delay loop wants 0x960 = 2400 ticks, so at one tick per
    /// instruction a boot containing such waits cannot finish in a practical
    /// instruction budget. Scaling the counter changes only how many host
    /// instructions a guest timeout costs -- ordering and relative durations are
    /// preserved.
    #[test]
    fn counter_ticks_per_tick_scales_the_architectural_counters() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        assert_eq!(
            m.counter_ticks_per_tick, 1,
            "default preserves old behaviour"
        );

        m.tick_devices(1);
        let one = m.scheduler.cycle();
        m.tick_devices(1);
        assert_eq!(m.scheduler.cycle() - one, 1, "one tick per host tick");

        m.set_counter_ticks_per_tick(64);
        m.tick_devices(1);
        let before = m.scheduler.cycle();
        m.tick_devices(1);
        assert_eq!(
            m.scheduler.cycle() - before,
            64,
            "the counters must advance in proportion to the scale"
        );

        // Values below 1 are clamped so the counter can never stall or go
        // backwards, which would make a guest timeout unreachable.
        m.set_counter_ticks_per_tick(0);
        assert_eq!(m.counter_ticks_per_tick, 1);
    }

    use super::*;
    use crate::bus::BusRegion;
    use crate::device::{Device, DeviceEvent, DeviceId, DeviceResult};
    use crate::dma::{DmaAccess, DmaFault, DmaIommu, DmaRequest, DmaResult, DmaTableReader};
    use crate::memory::Ram;
    use crate::SparseRam;
    use std::any::Any;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn le64(v: u64) -> [u8; 8] {
        v.to_le_bytes()
    }

    #[test]
    fn reset_clears_host_execution_and_watch_history() {
        let mut machine = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        machine.step_counter = 99;
        machine.current_instruction_pc = Some(0x1000);
        machine.write_watch_log.push((1, 2, 3, 4, 5));
        machine.write_watch_pa_log.push((1, 2, 3, 4, 5, 6));
        machine.reset();
        assert_eq!(machine.step_counter, 0);
        assert_eq!(machine.current_instruction_pc, None);
        assert!(machine.write_watch_log.is_empty());
        assert!(machine.write_watch_pa_log.is_empty());
    }

    #[test]
    fn el3_identity_map_workaround_requires_explicit_opt_in() {
        let mut machine = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        machine.cores.active_mut().state.el = ExceptionLevel::El3;
        machine.cores.active_mut().state.sctlr_el3 |= 1;
        machine.cores.active_mut().state.sctlr_el1 |= 1;
        let address = 0x0001_9c00_0000;

        assert!(machine.translate(address, false, false).is_err());
        machine.enable_legacy_el3_identity_map();
        assert_eq!(machine.translate(address, false, false).unwrap(), address);
    }

    /// The EL3 regime is selected by the current exception level, so an EL3
    /// access is governed by `SCTLR_EL3` and is **not** translated by the EL1
    /// regime however completely the EL1 registers are programmed.
    ///
    /// This is the mechanism behind a withdrawn record. A run of the
    /// AArch64 `AppleSEPROM-323` was reported to retire 11,161 instructions and
    /// park in a doorbell poll; it had been entered at EL3. The image's own
    /// page-table constructor ends with `msr sctlr_el1, x2`, which sets
    /// `SCTLR_EL1.M`, but at EL3 with `SCTLR_EL3.M` clear that has no effect on
    /// the instruction stream being fetched — the MMU was never on for it, so
    /// a table that cannot translate the ROM's own code window was never
    /// exercised and the run "succeeded". Entered at EL1, the same image faults
    /// at the first fetch after that `msr`.
    ///
    /// The pair of expectations below is the whole mechanism: the identical
    /// register image resolves identity at EL3 and faults at EL1. Without both
    /// halves, either the false success can be re-derived or the correct fault
    /// looks like a regression.
    #[test]
    fn el1_translation_state_does_not_apply_to_an_el3_access_while_sctlr_el3_m_is_clear() {
        // TCR_EL1: T0SZ = 28 (a 36-bit VA), TG0 = 0b10 (16 KiB), IPS = 0b001
        // (36-bit PA) -- the value `AppleSEPROM-323` and `AppleSEPROM-Cebu-A0`
        // both program at their MMU enable. The three decoded fields are
        // asserted below because the walk geometry this test relies on comes
        // from them: a truncated constant would silently describe a 4 KiB
        // granule and still fault on a zeroed root descriptor, i.e. pass for
        // the wrong reason.
        const TCR_EL1: u64 = 0x1408_0a51c;
        const TTBR0_EL1: u64 = 0x2409_0400;
        // The ROM's own code window, VA 4 GiB. Its root descriptor is zero.
        const CODE_VA: u64 = 0x1_0000_0000;

        assert_eq!(TCR_EL1 & 0x3f, 28, "T0SZ = 28, so a 36-bit VA");
        assert_eq!((TCR_EL1 >> 14) & 0x3, 0b10, "TG0 = 0b10, a 16 KiB granule");
        assert_eq!((TCR_EL1 >> 32) & 0x7, 0b001, "IPS = 0b001, a 36-bit PA");

        let build = |el: ExceptionLevel| {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            let s = &mut m.cores.active_mut().state;
            s.el = el;
            s.currentel = el as u8;
            s.tcr_el1 = TCR_EL1;
            s.ttbr0_el1 = TTBR0_EL1;
            s.sctlr_el1 = 0x100d; // M | I | A, caches off
            m.bus
                .map(crate::bus::BusRegion {
                    base: TTBR0_EL1,
                    size: 0x4000,
                    device: Box::new(crate::memory::Ram::new(TTBR0_EL1, 0x4000)),
                })
                .unwrap();
            m
        };

        // EL1: the EL1 regime applies, the root descriptor is zero, and the
        // fetch cannot be translated. This is the ROM's real behaviour.
        let mut el1 = build(ExceptionLevel::El1);
        assert!(
            el1.translate(CODE_VA, false, true).is_err(),
            "at EL1 the zeroed root descriptor must fault the fetch"
        );

        // EL3 with SCTLR_EL3.M clear: the same register image resolves
        // identity, so the EL1 table is never consulted. `SCTLR_EL3` is
        // architecturally 0 at reset, which is the state the harness starts
        // the SEP companion in.
        let mut el3 = build(ExceptionLevel::El3);
        assert_eq!(
            el3.cores.active().state.sctlr_el3 & 1,
            0,
            "SCTLR_EL3.M is clear out of reset"
        );
        assert_eq!(
            el3.translate(CODE_VA, false, true).unwrap(),
            CODE_VA,
            "at EL3 with SCTLR_EL3.M clear the access is not translated"
        );

        // And with SCTLR_EL3.M set the EL3 access is translated, which is the
        // condition under which the EL1 tables would be consulted (the model
        // applies the EL1 registers there, which is a separate known gap and
        // not what this test pins).
        let mut el3_on = build(ExceptionLevel::El3);
        el3_on.cores.active_mut().state.sctlr_el3 |= 1;
        assert!(
            el3_on.translate(CODE_VA, false, true).is_err(),
            "setting SCTLR_EL3.M must stop the identity shortcut"
        );
    }

    /// IOMMU that reads a root PTE at a fixed physical address and records
    /// every request it receives (via the shared log).
    struct RecordingIommu {
        log: Rc<RefCell<Vec<DmaRequest>>>,
    }

    impl DmaIommu for RecordingIommu {
        fn translate(&self, request: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
            self.log.borrow_mut().push(*request);
            match read(0x3000_0000) {
                Some(pa) if pa != 0 => DmaResult::Translated(pa),
                Some(_) => DmaResult::Fault(DmaFault::MissingLeaf),
                None => DmaResult::Fault(DmaFault::TableReadFailed(0x3000_0000)),
            }
        }
        fn invalidate(&mut self, _: u32, _: u64, _: u64) {}
    }

    /// IOMMU that always fails with AddressWidth.
    struct FaultingIommu;

    impl DmaIommu for FaultingIommu {
        fn translate(&self, _: &DmaRequest, _: DmaTableReader<'_>) -> DmaResult {
            DmaResult::Fault(DmaFault::AddressWidth)
        }
        fn invalidate(&mut self, _: u32, _: u64, _: u64) {}
    }

    /// Build a Machine with a RAM region at 0x3000_0000 whose first 8 bytes
    /// contain a little-endian PTE value `pa`.
    fn machine_with_pa(pa: u64) -> Machine {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        let mut ram = Ram::new(0x3000_0000, 0x1000);
        ram.load(0, &le64(pa));
        m.bus
            .map(BusRegion {
                base: 0x3000_0000,
                size: ram.size(),
                device: Box::new(ram),
            })
            .expect("map RAM");
        m
    }

    /// Build a Machine with a larger RAM at 0x3000_0000: byte 0..8 is a
    /// little-endian PTE for `translated_pa` and byte 8 onward holds a copied
    /// `payload` (the DMA data region, so a >8-byte transfer spans chunks).
    fn machine_with_payload(translated_pa: u64, payload: &[u8]) -> Machine {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        let mut ram = Ram::new(0x3000_0000, 0x8000);
        ram.load(0, &le64(translated_pa));
        ram.load(8, payload);
        m.bus
            .map(BusRegion {
                base: 0x3000_0000,
                size: ram.size(),
                device: Box::new(ram),
            })
            .expect("map RAM");
        m
    }

    #[test]
    fn dma_transfer_read_moves_physical_payload_chunked() {
        let payload: Vec<u8> = (0..20u8).collect();
        let mut m = machine_with_payload(0x3000_0008, &payload);
        let log = Rc::new(RefCell::new(Vec::new()));
        m.register_dma_iommu(0x12, 0x2A, Box::new(RecordingIommu { log: log.clone() }));
        let req = DmaRequest::new(0x5000, 0x12, 0x1000_0000, DmaAccess::Read, 20);
        let mut out = vec![0xEEu8; 20];
        // A 20-byte read issues 8/8/4-byte little-endian physical-bus chunks
        // and reassembles the payload in order.
        assert_eq!(
            m.dma_transfer(&req, &mut out),
            DmaResult::Translated(0x3000_0008)
        );
        assert_eq!(&out[..], &payload[..]);
    }

    #[test]
    fn dma_transfer_write_lands_physical_payload_chunked() {
        let mut m = machine_with_payload(0x3000_0040, &[]);
        let log = Rc::new(RefCell::new(Vec::new()));
        m.register_dma_iommu(0x12, 0x2A, Box::new(RecordingIommu { log: log.clone() }));
        let payload: Vec<u8> = (0..20u8).rev().collect();
        let req = DmaRequest::new(0x5000, 0x12, 0x1000_0000, DmaAccess::Write, 20);
        assert_eq!(
            m.dma_transfer(&req, &mut payload.clone()),
            DmaResult::Translated(0x3000_0040)
        );
        for (i, &b) in payload.iter().enumerate() {
            assert_eq!(m.mem_read(0x3000_0040 + i as u64, 1).unwrap(), u64::from(b));
        }
    }

    #[test]
    fn dma_transfer_fault_raises_irq_and_leaves_payload_untouched() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        // RAM placed at the IOVA so an undetected failure would corrupt it.
        let mut ram = Ram::new(0x1000_0000, 0x1000);
        ram.load(0, &[0x5A; 8]);
        m.bus
            .map(BusRegion {
                base: 0x1000_0000,
                size: ram.size(),
                device: Box::new(ram),
            })
            .expect("map RAM");
        m.register_dma_iommu(0x22, 0x3F, Box::new(FaultingIommu));
        let req = DmaRequest::new(0xA, 0x22, 0x2000_0000, DmaAccess::Write, 8);
        let mut buf = [0x5A; 8];
        assert_eq!(
            m.dma_transfer(&req, &mut buf),
            DmaResult::Fault(DmaFault::AddressWidth)
        );
        assert_eq!(m.cores.active().pending_irq, Some(0x3F));
        assert_eq!(&buf[..], &[0x5A; 8]);
    }

    #[test]
    fn dma_translate_resolves_physical_page_table_via_bus() {
        let mut m = machine_with_pa(0x4000_0000);
        let log = Rc::new(RefCell::new(Vec::new()));
        m.register_dma_iommu(0x12, 0x2A, Box::new(RecordingIommu { log: log.clone() }));
        let req = DmaRequest::new(0x5000, 0x12, 0x1000_0000, DmaAccess::Read, 0x1000);
        assert_eq!(m.dma_translate(&req), DmaResult::Translated(0x4000_0000));
    }

    #[test]
    fn dma_translate_bypasses_cpu_mmu() {
        let mut m = machine_with_pa(0x4000_0000);
        let log = Rc::new(RefCell::new(Vec::new()));
        m.register_dma_iommu(0x12, 0x2A, Box::new(RecordingIommu { log: log.clone() }));
        // Enable CPU Stage-1 MMU (SCTLR_EL1.M = 1). The CPU has no page
        // mapping for the PTE address, so a CPU read would fault; DMA must
        // still succeed because the reader resolves through the physical bus,
        // never through the CPU MMU.
        m.cores.active_mut().state.sctlr_el1 = 1;
        let req = DmaRequest::new(0x5000, 0x12, 0x1000_0000, DmaAccess::Read, 0x1000);
        assert_eq!(m.dma_translate(&req), DmaResult::Translated(0x4000_0000));
    }

    #[test]
    fn dma_fault_raises_mapped_irq_and_returns_typed_fault() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        m.register_dma_iommu(0x22, 0x3F, Box::new(FaultingIommu));
        let req = DmaRequest::new(0xA, 0x22, 0x2000_0000, DmaAccess::Write, 8);
        assert_eq!(
            m.dma_translate(&req),
            DmaResult::Fault(DmaFault::AddressWidth)
        );
        // The mapped IRQ was raised on the active core.
        assert_eq!(m.cores.active().pending_irq, Some(0x3F));
    }

    #[test]
    fn dma_unbound_sid_returns_fault_without_irq() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        let req = DmaRequest::new(0xB, 0xFF, 0x2000_0000, DmaAccess::Read, 4);
        assert_eq!(
            m.dma_translate(&req),
            DmaResult::Fault(DmaFault::UnboundSid(0xFF))
        );
        // No interrupt should be raised for an unbound SID.
        assert!(m.cores.active().pending_irq.is_none());
    }

    #[test]
    fn dma_translate_preserves_master_and_sid_metadata() {
        let mut m = machine_with_pa(0x4000_0000);
        let log = Rc::new(RefCell::new(Vec::new()));
        m.register_dma_iommu(0x33, 0x44, Box::new(RecordingIommu { log: log.clone() }));
        let req = DmaRequest::new(0xBEEF, 0x33, 0x5000_0000, DmaAccess::Write, 0x100);
        let _ = m.dma_translate(&req);
        let recorded = log.borrow();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].master, 0xBEEF);
        assert_eq!(recorded[0].sid, 0x33);
    }

    #[test]
    fn dma_invalidate_routes_to_bound_iommu() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        // FaultingIommu::invalidate is a no-op; this exercises the route
        // without a panic for both bound and unbound cases.
        m.register_dma_iommu(0x11, 0x20, Box::new(FaultingIommu));
        m.dma_invalidate(0x11, 0x4000_0000, 0x1000);
        // Unbound SID is silently skipped.
        m.dma_invalidate(0x99, 0, 0x1000);
    }

    /// Synthetic device that raises a single DmaRequest event on its first tick.
    struct DmaRaisingDevice {
        id: DeviceId,
        channel: u32,
        raise: bool,
    }

    impl Device for DmaRaisingDevice {
        fn id(&self) -> DeviceId {
            self.id
        }
        fn name(&self) -> &'static str {
            "dma-raise"
        }
        fn reset(&mut self) {}
        fn read(&mut self, _offset: u64, _size: usize) -> DeviceResult {
            DeviceResult::Fault
        }
        fn write(&mut self, _offset: u64, _size: usize, _value: u64) -> DeviceResult {
            DeviceResult::Fault
        }
        fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
            if self.raise {
                self.raise = false;
                Some(DeviceEvent::DmaRequest {
                    channel: self.channel,
                })
            } else {
                None
            }
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn tick_devices_drains_staged_device_dma_through_bound_ioommu() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        m.register_dma_iommu(0x22, 0x3F, Box::new(FaultingIommu));
        m.bus
            .map(BusRegion {
                base: 0x1000_0000,
                size: 0x1000,
                device: Box::new(DmaRaisingDevice {
                    id: 5,
                    channel: 0x22,
                    raise: true,
                }),
            })
            .expect("map synthetic DMA raiser");
        // Stage the transfer under the channel the device will announce.
        m.submit_dma(
            0x22,
            DmaRequest::new(0xA, 0x22, 0x2000_0000, DmaAccess::Write, 8),
        );
        m.tick_devices(1);
        // The staged request was drained through the faulting IOMMU: the
        // AddressWidth fault raised the bound interrupt.
        assert_eq!(m.cores.active().pending_irq, Some(0x3F));
        assert!(m.dma_pending.is_empty());
        // The device raises the event once; a second tick delivers nothing.
        m.cores.active_mut().pending_irq = None;
        m.tick_devices(1);
        assert!(m.cores.active().pending_irq.is_none());
    }

    #[test]
    fn submit_dma_for_same_channel_replaces_prior_request() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1000,
        });
        m.register_dma_iommu(0x22, 0x3F, Box::new(FaultingIommu));
        m.submit_dma(0x22, DmaRequest::new(1, 0x22, 0x1000, DmaAccess::Read, 8));
        m.submit_dma(0x22, DmaRequest::new(2, 0x22, 0x2000, DmaAccess::Write, 8));
        assert_eq!(m.dma_pending.len(), 1);
        assert_eq!(m.dma_pending[0].1.master, 2);
    }

    /// RAM-backed Machine used for SPE buffer tests: a 16MiB window at base 4GiB.
    fn spe_test_machine() -> Machine {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.bus
            .map(BusRegion {
                base: 0x1_0000_0000,
                size: 0x100_0000,
                device: Box::new(SparseRam::new(0x1_0000_0000, 0x100_0000)),
            })
            .expect("map DT RAM window");
        m.cores.active_mut().state.spe_pmblimitr = 0x1_0000_0000 | 1; // E=1, base 4GiB
        m.cores.active_mut().state.spe_pmbptr = 0x1_0000_0000;
        m.cores.active_mut().state.spe_pmbidr = 0x1; // P=1, Align=0 -> 4KiB window
        m
    }

    /// SPE record write lands in the RAM-backed buffer, advances PMBPTR, and a
    /// 4KiB wrap sets S+DL and returns the pointer to base.
    #[test]
    fn spe_buf_push_stores_record_and_advances_pointer() {
        let mut m = spe_test_machine();
        m.spe_buf_push(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88])
            .unwrap();
        assert_eq!(m.cores.active().state.spe_pmbptr, 0x1_0000_0008);
        assert_eq!(
            m.cores.active().state.spe_pmbsr & (1 << 16),
            1 << 16,
            "S bit set on completion"
        );
        let mut got = [0u8; 8];
        let bytes = m.mem_read_bytes(0x1_0000_0000, 8).unwrap();
        got.copy_from_slice(&bytes);
        assert_eq!(got, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    }

    /// A record that crosses the 4KiB window end is truncated, DL+S latch, and
    /// the pointer wraps to the PMBLIMITR base (single-writer fill mode).
    #[test]
    fn spe_buf_push_wraps_at_limit_and_sets_dl() {
        let mut m = spe_test_machine();
        m.cores.active_mut().state.spe_pmbptr = 0x1_0000_0ff8;
        m.spe_buf_push(&[0x11; 32]).unwrap();
        assert_eq!(
            m.cores.active().state.spe_pmbsr & ((1 << 24) | (1 << 16)),
            (1 << 24) | (1 << 16),
            "crossing the limit latches S and DL"
        );
        assert_eq!(
            m.cores.active().state.spe_pmbptr,
            0x1_0000_0000,
            "pointer wraps to base"
        );
        // The first-up-to-window bytes landed in RAM before the wrap.
        let got = m.mem_read_bytes(0x1_0000_0ff8, 8).unwrap();
        assert_eq!(got, [0x11; 8]);
    }

    /// A disabled buffer (E=0) drops the record without touching RAM or PMBSR.
    #[test]
    fn spe_buf_push_disabled_drops_record() {
        let mut m = spe_test_machine();
        m.cores.active_mut().state.spe_pmblimitr = 0x1_0000_0000; // E=0
        m.spe_buf_push(&[0x11; 8]).unwrap();
        assert_eq!(m.cores.active().state.spe_pmbptr, 0x1_0000_0000);
        assert_eq!(m.cores.active().state.spe_pmbsr, 0);
    }

    /// A pointer walked beyond mapped memory aborts like a data access: the
    /// faulting address is returned and PMBSR latches EC=buffer-management error.
    #[test]
    fn spe_buf_push_walk_off_mapped_memory_faults() {
        let mut m = spe_test_machine();
        m.cores.active_mut().state.spe_pmbptr = 0x2_0000_0000; // outside the 16MiB window
        let err = m.spe_buf_push(&[0x11; 8]).unwrap_err();
        assert_eq!(err, 0x2_0000_0000, "fault address returned");
        assert_eq!(
            m.cores.active().state.spe_pmbsr & 0x1f,
            0x2,
            "EC = buffer management error"
        );
        assert_eq!(
            m.cores.active().state.far_el1,
            0x2_0000_0000,
            "FAR_EL1 records the address"
        );
    }

    #[test]
    fn write_watch_logs_overlapping_writes_with_increments() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.bus
            .map(BusRegion {
                base: 0x1000,
                size: 0x1000,
                device: Box::new(SparseRam::new(0x1000, 0x1000)),
            })
            .expect("map RAM");
        m.write_watch_va = Some((0x1008, 16));
        m.write_watch_log.clear();
        m.step_counter = 0;

        m.mem_write(0x1008, 8, 0xDEADBEEF).unwrap();
        m.mem_write(0x1010, 8, 0xCAFEBABE).unwrap();
        m.mem_write(0x1000, 4, 0x11111111).unwrap();
        m.mem_write(0x1020, 4, 0x22222222).unwrap();
        m.mem_write(0x1014, 8, 0x33333333).unwrap();

        assert_eq!(m.write_watch_log.len(), 3);
        assert_eq!(m.write_watch_log[0].0, 0);
        assert_eq!(m.write_watch_log[0].2, 0x1008);
        assert_eq!(m.write_watch_log[0].3, 8);
        assert_eq!(m.write_watch_log[0].4, 0xDEADBEEF);
        assert_eq!(m.write_watch_log[1].0, 1);
        assert_eq!(m.write_watch_log[1].2, 0x1010);
        assert_eq!(m.write_watch_log[2].0, 2);
        assert_eq!(m.write_watch_log[2].2, 0x1014);
    }

    #[test]
    fn write_watch_pa_logs_translated_physical_writes() {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.bus
            .map(BusRegion {
                base: 0x4000,
                size: 0x1000,
                device: Box::new(SparseRam::new(0x4000, 0x1000)),
            })
            .expect("map RAM");
        m.write_watch_pa = Some((0x4008, 16));
        m.write_watch_pa_log.clear();
        m.step_counter = 0;

        m.mem_write(0x4008, 8, 0xDEADBEEF).unwrap();
        m.mem_write(0x4011, 8, 0xCAFEBABE).unwrap();
        m.mem_write(0x4000, 4, 0x11111111).unwrap();
        m.mem_write(0x4020, 4, 0x22222222).unwrap();
        m.mem_write(0x4014, 8, 0x33333333).unwrap();

        assert_eq!(m.write_watch_pa_log.len(), 3);
        assert_eq!(
            m.write_watch_pa_log[0],
            (0, 0, 0x4008, 0x4008, 8, 0xDEADBEEF)
        );
        assert_eq!(m.write_watch_pa_log[1].0, 1);
        assert_eq!(m.write_watch_pa_log[1].2, 0x4011);
        assert_eq!(m.write_watch_pa_log[1].3, 0x4011);
        assert_eq!(m.write_watch_pa_log[2].0, 2);
        assert_eq!(m.write_watch_pa_log[2].2, 0x4014);
        assert_eq!(m.write_watch_pa_log[2].3, 0x4014);
    }
}
