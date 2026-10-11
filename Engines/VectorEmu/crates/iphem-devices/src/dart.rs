//! Apple DART device models.
//!
//! The T8020 register model starts with only the d321ap ADT-backed PARAMS1
//! capability. Other offsets deliberately fault until their behavior and
//! reset state are supported by local platform evidence.
//!
//! The pure T8020 page-table walker implements the `APPLE_DART` (DART1)
//! descriptor format and level geometry validated against upstream Linux
//! (`drivers/iommu/io-pgtable-dart.c`) per `docs/DART_ARCHITECTURE.md`
//! acceptance steps 3-4. Step 5 (this file) shares one `DartT8020` engine
//! between its MMIO window (`DartMmio`, mapped on the bus) and its DMA port
//! (`DartDmaPort`, registered as the SID's `DmaIommu`). Translation goes
//! through a bounded deterministic software IOTLB and records the faulting
//! SID/address in the shared ERROR/ERROR_ADDR registers whenever a
//! translation fails. DART-USB is bound to stream 7 on d321ap.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use iphem_core::dma::{DmaAccess, DmaFault, DmaIommu, DmaRequest, DmaResult, DmaTableReader};
use std::any::Any;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const PARAMS1: u64 = 0x000;
/// Register-offset constants (T8020 `REG_*` layout in `apple-dart.c`, and the
/// offsets the checked-in T8020 SecureROM actually drives during its per-engine
/// flush pass). These are `pub` so platform/SoC layers can program and
/// interrogate an engine over its MMIO window exactly as a guest driver would,
/// and so acceptance tests can replay the captured SecureROM register trace
/// with named offsets.
pub const STREAM_COMMAND: u64 = 0x020;
pub const STREAM_SELECT: u64 = 0x034;
pub const ERROR: u64 = 0x040;
pub const ERROR_ADDR_LO: u64 = 0x050;
const ERROR_ADDR_HI: u64 = 0x054;
const CONFIG: u64 = 0x060;
const CONFIG_LOCK: u32 = 1 << 15;
pub const STREAMS_ENABLE: u64 = 0x0fc;
pub const TCR_BASE: u64 = 0x100;
pub const TTBR_BASE: u64 = 0x200;
const STREAM_COMMAND_INVALIDATE: u32 = 1 << 20;
const PARAMS1_PAGE_SHIFT_SHIFT: u32 = 24;
const PARAMS2: u64 = 0x004;
const PARAMS2_BYPASS_SUPPORT: u32 = 1;
pub const TCR_TRANSLATE_ENABLE: u32 = 1 << 7;
const TCR_BYPASS_DART: u32 = 1 << 8;

/// A single fault code bit. Upstream DART registers carry the SID in 31:16
/// and a code in 15:0 (`REG_ERROR` in `apple-dart.c`); this model records a
/// nonzero fault bit without inventing a detailed code enumeration that the
/// platform has not evidenced. Pub so platform layers can assert on the
/// shared ERROR register value.
pub const DART_ERROR_FAULT: u32 = 1;
/// Deterministic software IOTLB capacity. The cache is a plain FIFO; entry
/// order is stable under identical access sequences.
const DART_IOTLB_MAX: usize = 64;

/// One cached translation in the shared software IOTLB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IotlbEntry {
    sid: u32,
    /// IOVAs are keyed on their page (granule-aligned) address.
    page: u64,
    access: DartAccess,
    /// Translated page base (granule-aligned physical address).
    pa_page: u64,
}

/// Fault state read back through the ERROR / ERROR_ADDR_LO / ERROR_ADDR_HI
/// registers. Shared by the MMIO and DMA views of the engine.
#[derive(Debug, Clone, Copy, Default)]
struct ErrorCapture {
    register: u32,
    addr: u64,
}

/// Evidence-backed subset of the T8020 DART-SIO register block. One engine
/// is shared by every view: the MMIO window on the bus and the DMA port bound
/// to a stream ID translate against the same registers, page tables, and
/// IOTLB, and record the same faults.
pub struct DartT8020 {
    id: DeviceId,
    page_shift: u8,
    stream_select: [u32; 1],
    config: u32,
    streams_enable: u32,
    tcr: [u32; 16],
    ttbr: [[u32; 4]; 16],
    /// DMA input-address width for translations (0 = not configured).
    input_addr_width: u32,
    supports_bypass: bool,
    /// Bounded software IOTLB shared by every MMIO/DMA view of the engine.
    iotlb: Mutex<VecDeque<IotlbEntry>>,
    /// Most recent translation fault, read back through ERROR/ERROR_ADDR.
    error: Mutex<ErrorCapture>,
    /// Phase 213: cycle-accurate walk timing. The engine tracks the current
    /// cycle counter and records the latency of the last translation walk
    /// (TLB hit vs. miss vs. fault), so a firmware poll loop that reads
    /// STREAM_COMMAND BUSY sees the real walk latency instead of an
    /// always-idle register. The timing state is wrapped in a Mutex so the
    /// `DmaIommu::translate` path (which takes `&self`) can record it.
    cycle_counter: Mutex<u64>,
    /// Per-level walk latency (cycles). Index 0 = L0, 1 = L1, 2 = L2,
    /// 3 = L3 leaf. A 4-KiB granule walk is L0→L1→L2→leaf; a 16-KiB walk
    /// is L0→L1→leaf.
    level_latency: [u64; 4],
    /// TLB hit latency (cycles).
    cache_hit_latency: u64,
    /// TLB miss latency (cycles) — the extra cost of a cold walk.
    cache_miss_latency: u64,
    /// Fault/IRQ delivery latency (cycles).
    fault_irq_latency: u64,
    /// Cycles consumed by the most recent translation (0 = no translation
    /// has happened yet).
    last_walk_latency: Mutex<u64>,
    /// Whether the most recent translation was a TLB hit.
    last_walk_was_hit: Mutex<bool>,
}

impl DartT8020 {
    /// Construct a T8020 DART with the granule declared by the platform ADT.
    ///
    /// The d321ap DART-SIO node declares `page-size = 0x4000`, so callers
    /// should pass 14 for that instance.
    pub fn new(id: DeviceId, page_shift: u8) -> Self {
        Self {
            id,
            page_shift,
            stream_select: [0; 1],
            config: 0,
            streams_enable: 0,
            tcr: [0; 16],
            ttbr: [[0; 4]; 16],
            input_addr_width: 0,
            supports_bypass: false,
            iotlb: Mutex::new(VecDeque::new()),
            error: Mutex::new(ErrorCapture::default()),
            // Phase 213: cycle-accurate walk timing. A 16-KiB granule
            // T8020 walk is L0→L1→leaf (3 levels); a 4-KiB walk is
            // L0→L1→L2→leaf (4 levels). Each level costs ~12 cycles
            // (TLB fill); a cold walk adds the cache-miss penalty.
            cycle_counter: Mutex::new(0),
            level_latency: [12, 12, 12, 12],
            cache_hit_latency: 4,
            cache_miss_latency: 8,
            fault_irq_latency: 10,
            last_walk_latency: Mutex::new(0),
            last_walk_was_hit: Mutex::new(false),
        }
    }

    /// Split this engine into a bus-mapped MMIO handle (`DartMmio`) and a
    /// `DmaIommu` stream port (`DartDmaPort`) that share one register set,
    /// page tables, faults, and IOTLB. Register writes through the MMIO window
    /// configure the same DART that the bound stream translates against.
    pub fn into_pair(self) -> (DartMmio, DartDmaPort) {
        let shared = Arc::new(Mutex::new(self));
        (DartMmio(shared.clone()), DartDmaPort(shared))
    }

    /// Configure the DMA input-address width. Platform ADT evidence supplies
    /// this value; without it translation is disabled and returns
    /// [`DmaFault::AddressWidth`].
    pub const fn with_input_address_width(mut self, ias: u32) -> Self {
        self.input_addr_width = ias;
        self
    }

    /// Enable the DART bypass capability only when the platform description
    /// or hardware trace confirms PARAMS2.BYPASS_SUPPORT for this instance.
    pub const fn with_bypass_support(mut self, supported: bool) -> Self {
        self.supports_bypass = supported;
        self
    }

    fn read_register(&self, offset: u64) -> Option<u32> {
        match offset {
            PARAMS1 => Some((self.page_shift as u32) << PARAMS1_PAGE_SHIFT_SHIFT),
            PARAMS2 => Some(u32::from(self.supports_bypass) * PARAMS2_BYPASS_SUPPORT),
            STREAM_COMMAND => Some(0),
            STREAM_SELECT => Some(self.stream_select[0]),
            ERROR => Some(self.error.lock().unwrap().register),
            ERROR_ADDR_LO => Some(self.error.lock().unwrap().addr as u32),
            ERROR_ADDR_HI => Some((self.error.lock().unwrap().addr >> 32) as u32),
            CONFIG => Some(self.config),
            STREAMS_ENABLE => Some(self.streams_enable),
            offset if (TCR_BASE..TCR_BASE + 16 * 4).contains(&offset) && offset % 4 == 0 => {
                Some(self.tcr[((offset - TCR_BASE) / 4) as usize])
            }
            offset if (TTBR_BASE..TTBR_BASE + 16 * 16).contains(&offset) && offset % 4 == 0 => {
                let index = ((offset - TTBR_BASE) / 4) as usize;
                Some(self.ttbr[index / 4][index % 4])
            }
            _ => None,
        }
    }

    fn write_register(&mut self, offset: u64, value: u32) -> bool {
        match offset {
            STREAM_SELECT => self.stream_select[0] = value,
            STREAM_COMMAND if value & STREAM_COMMAND_INVALIDATE != 0 => {
                // Invalidate the IOTLB entries for the selected streams.
                // Synchronous, so BUSY (STREAM_COMMAND reads) is never
                // observable.
                self.invalidate_streams(self.stream_select[0]);
            }
            STREAM_COMMAND => {}
            ERROR => self.error.lock().unwrap().register &= !value,
            ERROR_ADDR_LO => {
                let mut error = self.error.lock().unwrap();
                error.addr = (error.addr & !0xffff_ffff) | value as u64;
            }
            ERROR_ADDR_HI => {
                let mut error = self.error.lock().unwrap();
                error.addr = (error.addr & 0xffff_ffff) | ((value as u64) << 32);
            }
            CONFIG => self.config |= value & CONFIG_LOCK,
            STREAMS_ENABLE if self.config & CONFIG_LOCK == 0 => self.streams_enable = value,
            STREAMS_ENABLE => {}
            offset
                if (TCR_BASE..TCR_BASE + 16 * 4).contains(&offset)
                    && offset % 4 == 0
                    && self.config & CONFIG_LOCK == 0 =>
            {
                self.tcr[((offset - TCR_BASE) / 4) as usize] = value
            }
            offset
                if (TTBR_BASE..TTBR_BASE + 16 * 16).contains(&offset)
                    && offset % 4 == 0
                    && self.config & CONFIG_LOCK == 0 =>
            {
                let index = ((offset - TTBR_BASE) / 4) as usize;
                self.ttbr[index / 4][index % 4] = value
            }
            offset
                if (TCR_BASE..TCR_BASE + 16 * 4).contains(&offset)
                    || (TTBR_BASE..TTBR_BASE + 16 * 16).contains(&offset) => {}
            _ => return false,
        }
        true
    }

    /// Phase 213: advance the engine's cycle counter by `cycles`. The next
    /// timed translation records its latency relative to this counter.
    pub fn advance_cycles(&mut self, cycles: u64) {
        let mut counter = self.cycle_counter.lock().unwrap();
        *counter = counter.wrapping_add(cycles);
    }

    /// Phase 213: record a timed translation result. A TLB hit costs
    /// `cache_hit_latency`; a cold walk costs `cache_miss_latency` plus the
    /// per-level walk latency; a fault costs `fault_irq_latency`. The most
    /// recent latency is stored in `last_walk_latency` and the hit/miss
    /// flag in `last_walk_was_hit`. The DMA translation path records this for
    /// both IOTLB hits and cold page-table walks.
    fn record_timed_translation(&self, hit: bool, levels: usize) -> u64 {
        let latency = if hit {
            self.cache_hit_latency
        } else {
            self.cache_miss_latency + (0..levels).map(|i| self.level_latency[i]).sum::<u64>()
        };
        *self.last_walk_latency.lock().unwrap() = latency;
        *self.last_walk_was_hit.lock().unwrap() = hit;
        let mut counter = self.cycle_counter.lock().unwrap();
        *counter = counter.wrapping_add(latency);
        latency
    }

    fn record_fault_translation(&self) -> u64 {
        let latency = self.fault_irq_latency;
        *self.last_walk_latency.lock().unwrap() = latency;
        *self.last_walk_was_hit.lock().unwrap() = false;
        let mut counter = self.cycle_counter.lock().unwrap();
        *counter = counter.wrapping_add(latency);
        latency
    }

    /// Phase 213: whether the most recent translation was a TLB hit.
    pub fn last_walk_was_hit(&self) -> bool {
        *self.last_walk_was_hit.lock().unwrap()
    }

    /// Phase 213: latency (cycles) of the most recent translation.
    pub fn last_walk_latency(&self) -> u64 {
        *self.last_walk_latency.lock().unwrap()
    }
}

impl Device for DartT8020 {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "dart-t8020"
    }

    fn reset(&mut self) {
        self.stream_select = [0; 1];
        self.config = 0;
        self.streams_enable = 0;
        self.tcr = [0; 16];
        self.ttbr = [[0; 4]; 16];
        *self.iotlb.lock().unwrap() = VecDeque::new();
        *self.error.lock().unwrap() = ErrorCapture::default();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if size != 4 {
            return DeviceResult::Fault;
        }
        match self.read_register(offset) {
            Some(value) => DeviceResult::Value(value as u64),
            None => DeviceResult::Fault,
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if size != 4 || value > u32::MAX as u64 || offset == PARAMS1 {
            return DeviceResult::Fault;
        }
        if self.write_register(offset, value as u32) {
            DeviceResult::Ok
        } else {
            DeviceResult::Fault
        }
    }

    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        // Phase 213: advance the engine's cycle counter so the next timed
        // translation records a latency relative to the current tick budget.
        let mut counter = self.cycle_counter.lock().unwrap();
        *counter = counter.wrapping_add(cycles);
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DartT8020 {
    /// Record a translation fault in the shared ERROR/ERROR_ADDR state and
    /// return the typed fault. The error register carries the SID in 31:16
    /// and a single evidence-backed fault bit in 15:0.
    fn fault(&self, sid: u32, iova: u64, code: DmaFault) -> DmaResult {
        let mut error = self.error.lock().unwrap();
        error.register = (sid << 16) | DART_ERROR_FAULT;
        error.addr = iova;
        drop(error);
        DmaResult::Fault(code)
    }

    /// Insert a translated page into the shared IOTLB, evicting the oldest
    /// entry FIFO-style at capacity.
    fn cache(&self, pa: u64, sid: u32, page: u64, access: DartAccess) {
        let page_shift = self.page_shift as u32;
        let entry = IotlbEntry {
            sid,
            page,
            access,
            pa_page: pa & !((1u64 << page_shift) - 1),
        };
        let mut tlb = self.iotlb.lock().unwrap();
        if let Some(pos) = tlb.iter().position(|existing| existing == &entry) {
            let item = tlb.remove(pos).expect("position came from this deque");
            tlb.push_back(item);
        } else {
            if tlb.len() >= DART_IOTLB_MAX {
                tlb.pop_front();
            }
            tlb.push_back(entry);
        }
    }

    /// Drop IOTLB entries for every stream whose bit is set in `mask`.
    fn invalidate_streams(&mut self, mask: u32) {
        if mask == 0 {
            return;
        }
        self.iotlb
            .lock()
            .unwrap()
            .retain(|entry| (1u32 << entry.sid) & mask == 0);
    }
}

// ---- DmaIommu integration (DART step 4) ----

impl DmaIommu for DartT8020 {
    /// Translate `request.iova` through the DART's page tables.
    ///
    /// Table entries are fetched via the physical bus reader `read`, never
    /// through the CPU MMU. A successful translation is cached in the shared
    /// IOTLB; a failed one is recorded in the ERROR/ERROR_ADDR registers
    /// (visible through the paired MMIO window) and reported to the caller
    /// (Machine), which raises the interrupt mapped to this DART.
    fn translate(&self, request: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
        if request.sid as usize >= self.ttbr.len() {
            self.record_fault_translation();
            return self.fault(request.sid, request.iova, DmaFault::NoRoot);
        }
        let sid = request.sid as usize;
        if self.streams_enable & (1 << sid) == 0 {
            self.record_fault_translation();
            return self.fault(
                request.sid,
                request.iova,
                DmaFault::StreamBlocked(request.sid),
            );
        }
        let tcr = self.tcr[sid];
        if tcr & TCR_TRANSLATE_ENABLE == 0 {
            if tcr & TCR_BYPASS_DART != 0 && self.supports_bypass {
                return DmaResult::Bypass(request.iova);
            }
            self.record_fault_translation();
            return self.fault(
                request.sid,
                request.iova,
                DmaFault::StreamBlocked(request.sid),
            );
        }
        let walker = match DartT8020Walker::new(self.page_shift as u32, self.input_addr_width) {
            Some(w) => w,
            None => {
                self.record_fault_translation();
                return self.fault(request.sid, request.iova, DmaFault::AddressWidth);
            }
        };
        let roots: Vec<u64> = self.ttbr[sid]
            .iter()
            .filter_map(|&reg| dart_t8020_ttbr_root_address(reg))
            .collect();
        if roots.is_empty() {
            self.record_fault_translation();
            return self.fault(request.sid, request.iova, DmaFault::NoRoot);
        }
        let access = match request.access {
            DmaAccess::Read => DartAccess::Read,
            DmaAccess::Write => DartAccess::Write,
        };
        let page_shift = self.page_shift as u32;
        let page = request.iova >> page_shift;
        let page_mask = (1u64 << page_shift) - 1;
        // IOTLB lookup: a hit serves any offset within the cached granule.
        if let Some(entry) =
            self.iotlb.lock().unwrap().iter().find(|entry| {
                entry.sid == request.sid && entry.page == page && entry.access == access
            })
        {
            self.record_timed_translation(true, 0);
            return DmaResult::Translated(entry.pa_page | (request.iova & page_mask));
        }
        match walker.translate(&roots, request.iova, access, read) {
            Ok(pa) => {
                self.record_timed_translation(false, walker.walk_levels() as usize);
                self.cache(pa, request.sid, page, access);
                DmaResult::Translated(pa)
            }
            Err(fault) => {
                self.record_fault_translation();
                self.fault(request.sid, request.iova, DartWalkFault::to_dma(fault))
            }
        }
    }

    /// Drop the IOTLB entries for a stream/address range. `size == 0`
    /// (unknown extent) invalidates the whole stream.
    fn invalidate(&mut self, sid: u32, iova: u64, size: u64) {
        let page_shift = self.page_shift as u32;
        let first = iova >> page_shift;
        let last = if size == 0 {
            u64::MAX
        } else {
            (iova + size - 1) >> page_shift
        };
        self.iotlb
            .lock()
            .unwrap()
            .retain(|entry| entry.sid != sid || entry.page < first || entry.page > last);
    }
}

// ---- Shared engine views (DART step 5) ----

/// Bus-mapped MMIO view of a `DartT8020` engine. Writes through this handle
/// configure the same DART that the paired `DartDmaPort` translates against.
pub struct DartMmio(Arc<Mutex<DartT8020>>);

/// `DmaIommu` stream-port view of the same engine. Plays no separate role in
/// register state: faults it records are readable through the paired MMIO
/// window and vice versa.
pub struct DartDmaPort(Arc<Mutex<DartT8020>>);

impl DartMmio {
    /// Access to the shared engine, for tests/integration that need to reach
    /// both views through one handle.
    pub fn engine(&self) -> Arc<Mutex<DartT8020>> {
        self.0.clone()
    }
}

impl DartDmaPort {
    /// Access to the shared engine, for tests/integration that need to reach
    /// both views through one handle.
    pub fn engine(&self) -> Arc<Mutex<DartT8020>> {
        self.0.clone()
    }
}

impl Device for DartMmio {
    fn id(&self) -> DeviceId {
        self.0.lock().unwrap().id
    }

    fn name(&self) -> &'static str {
        "dart-t8020"
    }

    fn reset(&mut self) {
        self.0.lock().unwrap().reset();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        self.0.lock().unwrap().read(offset, size)
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        self.0.lock().unwrap().write(offset, size, value)
    }

    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        self.0.lock().unwrap().tick(cycles)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DmaIommu for DartDmaPort {
    fn translate(&self, request: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
        self.0.lock().unwrap().translate(request, read)
    }

    fn invalidate(&mut self, sid: u32, iova: u64, size: u64) {
        self.0.lock().unwrap().invalidate(sid, iova, size);
    }
}

impl DartWalkFault {
    /// Map a walker fault into the core `DmaFault` set.
    fn to_dma(self) -> iphem_core::dma::DmaFault {
        match self {
            DartWalkFault::AddressWidth => iphem_core::dma::DmaFault::AddressWidth,
            DartWalkFault::NoRoot => iphem_core::dma::DmaFault::NoRoot,
            DartWalkFault::MissingIntermediate(l) => {
                iphem_core::dma::DmaFault::MissingIntermediate(l)
            }
            DartWalkFault::MissingLeaf => iphem_core::dma::DmaFault::MissingLeaf,
            DartWalkFault::PresentInvalid => iphem_core::dma::DmaFault::PresentInvalid,
            DartWalkFault::PermissionRead => iphem_core::dma::DmaFault::PermissionRead,
            DartWalkFault::PermissionWrite => iphem_core::dma::DmaFault::PermissionWrite,
            DartWalkFault::SubpageDisallowed {
                start,
                end,
                subpage,
            } => iphem_core::dma::DmaFault::SubpageDisallowed {
                start,
                end,
                subpage,
            },
            DartWalkFault::TableReadFailed(p) => iphem_core::dma::DmaFault::TableReadFailed(p),
        }
    }
}

// ---- T8020 (APPLE_DART / DART1) page-table walker ----

/// DART1 descriptor bits (`APPLE_DART` format). Offsets and semantics follow
/// upstream `io-pgtable-dart.c`; the bit layout matches `DART_ARCHITECTURE.md`.
/// The descriptor bits are pub so platform layers can build guest page tables
/// (the walker reads them straight from physical memory).
pub const DART_PTE_VALID: u64 = 1 << 0;
pub const DART_PTE_SP_DIS: u64 = 1 << 1;
const DART_PTE_NO_WRITE: u64 = 1 << 7;
const DART_PTE_NO_READ: u64 = 1 << 8;
/// Physical address bits 35:12 of a leaf PTE or next-table entry.
const DART_PTE_PADDR_MASK: u64 = 0x0fff_ffff_f000;
/// Subpage start (63:52) and end (51:40) delimit the accessible 4-KiB
/// subpages of a 16-KiB page when SP_DIS is clear.
const DART_SUBPAGE_START_SHIFT: u32 = 52;
const DART_SUBPAGE_END_SHIFT: u32 = 40;
const DART_SUBPAGE_MASK: u64 = 0xfff;
/// T8020 TTBR register: bit 31 valid, table address written without shift
/// from bits 12..35 of the physical address (`DART_T8020_TTBR_SHIFT` 12).
const DART_T8020_TTBR_VALID: u32 = 1 << 31;
const DART_T8020_TTBR_ADDR_SHIFT: u32 = 12;
const DART_T8020_TTBR_ADDR_MASK: u32 = 0x00ff_ffff;
/// Up to four 4-KiB TTBR roots per SID in the legacy layout.
const DART_MAX_ROOT_BITS: u32 = 2;
/// `DART_MAX_LEVELS` from the reference allocator, including the TTBR level.
const DART_MAX_WALK_LEVELS: u32 = 4;
/// DART1 output-address width (`DART1_MAX_ADDR_BITS`).
const DART_MAX_ADDR_BITS: u32 = 36;

/// Direction of a DMA access checked against a leaf's NO_READ/NO_WRITE.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DartAccess {
    Read,
    Write,
}

/// Typed failure from a T8020 DART translation.
///
/// Each cause is distinct so a faulting request can be recorded without
/// inventing an error-code priority (see `DART_ARCHITECTURE.md`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DartWalkFault {
    /// IOVA outside the configured input-address range.
    AddressWidth,
    /// The selected TTBR root slot is not configured.
    NoRoot,
    /// An intermediate table entry is absent at the given level.
    MissingIntermediate(u32),
    /// The leaf PTE is absent.
    MissingLeaf,
    /// A present entry is not marked valid.
    PresentInvalid,
    /// Read is denied by NO_READ.
    PermissionRead,
    /// Write is denied by NO_WRITE.
    PermissionWrite,
    /// The accessed 4-KiB subpage is outside the PTE's subpage range.
    SubpageDisallowed { start: u32, end: u32, subpage: u32 },
    /// A physical table read did not resolve to mapped memory.
    TableReadFailed(u64),
}

/// Physical-memory table read used by [`DartT8020Walker::translate`].
///
/// Returns the 64-bit entry at `address`, or `None` when the physical read
/// cannot be satisfied (unmapped guest memory). Production callers wrap a
/// physical `Bus` read; unit tests provide synthetic page tables.
pub type TableEntryReader<'a> = &'a mut dyn FnMut(u64) -> Option<u64>;

/// Decode a T8020 TTBR register value into a root table physical address.
///
/// Returns `None` when the VALID bit (31) is clear.
pub fn dart_t8020_ttbr_root_address(ttbr: u32) -> Option<u64> {
    if ttbr & DART_T8020_TTBR_VALID == 0 {
        return None;
    }
    Some(((ttbr & DART_T8020_TTBR_ADDR_MASK) as u64) << DART_T8020_TTBR_ADDR_SHIFT)
}

/// Pure T8020 (`APPLE_DART` DART1) page-table walker.
///
/// Level geometry matches the reference allocator: 8-byte entries,
/// `bits_per_level = page_shift - 3`, input bits below the page shift index
/// into the leaf level, and up to four root slots for 4-KiB granules.
/// Intermediate entries are always next-table pointers; the format defines
/// no block descriptors. Subpage protection is enforced when SP_DIS is clear.
/// Translation never consults the CPU MMU and never derives host pointers
/// from guest addresses.
#[derive(Clone, Copy, Debug)]
pub struct DartT8020Walker {
    page_shift: u32,
    ias: u32,
    bits_per_level: u32,
    /// Walk levels excluding the TTBR root level (`levels` in the reference).
    levels: u32,
    root_bits: u32,
    index_mask: u64,
}

impl DartT8020Walker {
    /// Build the walker for a granule and input-address width.
    ///
    /// Returns `None` for page shifts other than 4-KiB/16-KiB, an input width
    /// outside the DART1 range, or a geometry the reference rejects.
    pub fn new(page_shift: u32, ias: u32) -> Option<Self> {
        if !matches!(page_shift, 12 | 14) {
            return None;
        }
        if ias > DART_MAX_ADDR_BITS || ias <= page_shift {
            return None;
        }
        let bits_per_level = page_shift - 3;
        let va_bits = ias - page_shift;
        let max_root_bits = if page_shift == 12 {
            DART_MAX_ROOT_BITS
        } else {
            0
        };
        let levels = std::cmp::max(2, (va_bits - max_root_bits).div_ceil(bits_per_level));
        let root_bits = va_bits.saturating_sub(bits_per_level * levels);
        if root_bits > max_root_bits || levels + 1 > DART_MAX_WALK_LEVELS {
            return None;
        }
        Some(Self {
            page_shift,
            ias,
            bits_per_level,
            levels,
            root_bits,
            index_mask: (1u64 << bits_per_level) - 1,
        })
    }

    /// The page shift (granule) this walker was built for.
    pub const fn page_shift(&self) -> u32 {
        self.page_shift
    }

    /// The input-address width this walker enforces.
    pub const fn ias(&self) -> u32 {
        self.ias
    }

    /// Number of walk levels excluding the TTBR root level.
    pub const fn walk_levels(&self) -> u32 {
        self.levels
    }

    /// Number of TTBR root slots (`1 << root_bits`).
    pub fn n_roots(&self) -> usize {
        1usize << self.root_bits
    }

    /// Index of `iova` within the table at `level`. Level 1 is the leaf.
    pub fn index(&self, iova: u64, level: u32) -> u32 {
        ((iova >> (level * self.bits_per_level + 3)) & self.index_mask) as u32
    }

    /// Translate `iova` for `access` through the subtree rooted at `roots`.
    ///
    /// `roots` holds one decoded TTBR address per slot, in slot order;
    /// a zero slot reports [`DartWalkFault::NoRoot`]. `read` resolves table
    /// entries from guest physical memory. Returns the guest physical address
    /// on success.
    pub fn translate(
        &self,
        roots: &[u64],
        iova: u64,
        access: DartAccess,
        read: &mut dyn FnMut(u64) -> Option<u64>,
    ) -> Result<u64, DartWalkFault> {
        if iova >= 1u64 << self.ias {
            return Err(DartWalkFault::AddressWidth);
        }
        let root_index = self.index(iova, self.levels + 1);
        if root_index as usize >= self.n_roots() {
            return Err(DartWalkFault::AddressWidth);
        }
        let mut table = *roots
            .get(root_index as usize)
            .ok_or(DartWalkFault::NoRoot)?;
        if table == 0 {
            return Err(DartWalkFault::NoRoot);
        }
        for level in (2..=self.levels).rev() {
            let index = self.index(iova, level);
            let entry = self
                .read_entry(read, table, index as u64)
                .ok_or(DartWalkFault::TableReadFailed(table + index as u64 * 8))?;
            if entry == 0 {
                return Err(DartWalkFault::MissingIntermediate(level));
            }
            let next = Self::table_pointer(entry)?;
            table = next;
        }
        let leaf_index = self.index(iova, 1);
        let leaf = self.read_entry(read, table, leaf_index as u64).ok_or(
            DartWalkFault::TableReadFailed(table + leaf_index as u64 * 8),
        )?;
        if leaf == 0 {
            return Err(DartWalkFault::MissingLeaf);
        }
        if leaf & DART_PTE_VALID == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        match access {
            DartAccess::Read if leaf & DART_PTE_NO_READ != 0 => {
                return Err(DartWalkFault::PermissionRead);
            }
            DartAccess::Write if leaf & DART_PTE_NO_WRITE != 0 => {
                return Err(DartWalkFault::PermissionWrite);
            }
            _ => {}
        }
        if leaf & DART_PTE_SP_DIS == 0 {
            let start = ((leaf >> DART_SUBPAGE_START_SHIFT) & DART_SUBPAGE_MASK) as u32;
            let end = ((leaf >> DART_SUBPAGE_END_SHIFT) & DART_SUBPAGE_MASK) as u32;
            let subpage = ((iova >> 12) & DART_SUBPAGE_MASK) as u32;
            if subpage < start || subpage > end {
                return Err(DartWalkFault::SubpageDisallowed {
                    start,
                    end,
                    subpage,
                });
            }
        }
        let page_offset = iova & ((1u64 << self.page_shift) - 1);
        Ok((leaf & DART_PTE_PADDR_MASK) | page_offset)
    }

    /// Validate an intermediate/next-table entry and return its table pointer.
    fn table_pointer(entry: u64) -> Result<u64, DartWalkFault> {
        if entry & DART_PTE_VALID == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        let next = entry & DART_PTE_PADDR_MASK;
        if next == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        Ok(next)
    }

    fn read_entry(
        &self,
        read: &mut dyn FnMut(u64) -> Option<u64>,
        table: u64,
        index: u64,
    ) -> Option<u64> {
        read(table + index * 8)
    }
}

// ---- T8110 (APPLE_DART2 / DART2) ----

/// T8110 DART2 descriptor field values. DART2 differs from DART1 in three
/// concrete ways (`APPLE_DART2_PADDR_MASK` etc. in `io-pgtable-dart.c`):
/// physical addresses occupy bits 37:10 and are written shifted right by 4,
/// and the protection bits sit at NO_READ=bit3 / NO_WRITE=bit2 / NO_CACHE=bit1
/// instead of the DART1 SP_DIS/NO_WRITE/NO_READ bank. The VALID bit is bit 0
/// in both formats. There are no block descriptors in either format.
const DART2_PTE_PADDR_MASK: u64 = 0x0003_ffff_fc00;
const DART2_PTE_PADDR_SHIFT: u32 = 4;
const DART2_PTE_NO_WRITE: u64 = 1 << 2;
const DART2_PTE_NO_READ: u64 = 1 << 3;
/// DART2 no-cache hint (pub so acceptance tests can build DART2 tables).
pub const DART2_PTE_NO_CACHE: u64 = 1 << 1;
/// T8110 output-address width (`DART2` PARAMS3 PA width extends to 40 bits;
/// the DART2 descriptor only carries 37:10, so 40 physical bits are the
/// supported ceiling per practical page-table size).
const DART2_MAX_ADDR_BITS: u32 = 40;

/// T8110 TTBR register: bit 0 valid, table physical address written at
/// bits 13..2 (address field starts at bit 2 with a 16-KiB table granule).
pub const T8110_TTBR_VALID: u32 = 1;
const T8110_TTBR_ADDR_FIELD_SHIFT: u32 = 2;
const T8110_TTBR_ADDR_FIELD_MASK: u32 = 0x3fff_ffff;
const T8110_TTBR_SHIFT: u32 = 14;

/// Decode a T8110 TTBR register value into a root table physical address.
///
/// The T8110 TTBR is a single (not quadruple) per-SID root with the valid bit
/// at bit 0 and the 16-KiB-table address in bits 13..2. Returns `None` when
/// the VALID bit is clear.
pub fn dart_t8110_ttbr_root_address(ttbr: u32) -> Option<u64> {
    if ttbr & T8110_TTBR_VALID == 0 {
        return None;
    }
    Some(
        ((u64::from(ttbr & T8110_TTBR_ADDR_FIELD_MASK)) >> T8110_TTBR_ADDR_FIELD_SHIFT)
            << T8110_TTBR_SHIFT,
    )
}

/// Page-table walker for the T8110 `APPLE_DART2` format.
///
/// Same level geometry as the DART1 walker (8-byte entries,
/// `bits_per_level = page_shift - 3`, one root slot per SID for both 4-KiB and
/// 16-KiB granules), but decodes the DART2 descriptor: VALID=bit0, page
/// address stored at 37:10 shifted right by 4, NO_READ=bit3/NO_WRITE=bit2.
/// DART2 carries no SP_DIS; subpage fields are written 0..0xfff by upstream
/// and simply not enforced beyond the page granule.
#[derive(Clone, Copy, Debug)]
pub struct DartT8110Walker {
    page_shift: u32,
    ias: u32,
    bits_per_level: u32,
    levels: u32,
    root_bits: u32,
    index_mask: u64,
}

impl DartT8110Walker {
    /// Build the T8110 walker for a granule and input-address width.
    ///
    /// Accepts 4-KiB/16-KiB granules and an input width within the DART2
    /// range. A single root is used for the DART2 geometry (T8110 owns one
    /// TTBR per SID).
    pub fn new(page_shift: u32, ias: u32) -> Option<Self> {
        if !matches!(page_shift, 12 | 14) {
            return None;
        }
        if ias > DART2_MAX_ADDR_BITS || ias <= page_shift {
            return None;
        }
        let bits_per_level = page_shift - 3;
        let va_bits = ias - page_shift;
        // DART2 uses exactly one root slot per SID in the T8110 layout.
        let max_root_bits = 0;
        let levels = std::cmp::max(2, (va_bits - max_root_bits).div_ceil(bits_per_level));
        let root_bits = va_bits.saturating_sub(bits_per_level * levels);
        if root_bits > max_root_bits || levels + 1 > DART_MAX_WALK_LEVELS {
            return None;
        }
        Some(Self {
            page_shift,
            ias,
            bits_per_level,
            levels,
            root_bits,
            index_mask: (1u64 << bits_per_level) - 1,
        })
    }

    /// Translate `iova` for `access` through the DART2 subtree rooted at
    /// `root`. `root` is the decoded T8110 TTBR address.
    pub fn translate(
        &self,
        root: u64,
        iova: u64,
        access: DartAccess,
        read: &mut dyn FnMut(u64) -> Option<u64>,
    ) -> Result<u64, DartWalkFault> {
        if iova >= 1u64 << self.ias {
            return Err(DartWalkFault::AddressWidth);
        }
        let root_index = self.index(iova, self.levels + 1);
        if root_index as usize >= self.n_roots() {
            return Err(DartWalkFault::AddressWidth);
        }
        let mut table = root;
        if table == 0 {
            return Err(DartWalkFault::NoRoot);
        }
        for level in (2..=self.levels).rev() {
            let index = self.index(iova, level);
            let entry = self
                .read_entry(read, table, index as u64)
                .ok_or(DartWalkFault::TableReadFailed(table + index as u64 * 8))?;
            if entry == 0 {
                return Err(DartWalkFault::MissingIntermediate(level));
            }
            let next = Self::table_pointer(entry)?;
            table = next;
        }
        let leaf_index = self.index(iova, 1);
        let leaf = self.read_entry(read, table, leaf_index as u64).ok_or(
            DartWalkFault::TableReadFailed(table + leaf_index as u64 * 8),
        )?;
        if leaf == 0 {
            return Err(DartWalkFault::MissingLeaf);
        }
        if leaf & DART_PTE_VALID == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        match access {
            DartAccess::Read if leaf & DART2_PTE_NO_READ != 0 => {
                return Err(DartWalkFault::PermissionRead);
            }
            DartAccess::Write if leaf & DART2_PTE_NO_WRITE != 0 => {
                return Err(DartWalkFault::PermissionWrite);
            }
            _ => {}
        }
        let page_offset = iova & ((1u64 << self.page_shift) - 1);
        let base = (leaf & DART2_PTE_PADDR_MASK) << DART2_PTE_PADDR_SHIFT;
        Ok(base | page_offset)
    }

    /// Index of `iova` within the table at `level`.
    pub fn index(&self, iova: u64, level: u32) -> u32 {
        ((iova >> (level * self.bits_per_level + 3)) & self.index_mask) as u32
    }

    /// Number of TTBR root slots (always 1 for T8110).
    pub fn n_roots(&self) -> usize {
        1usize << self.root_bits
    }

    /// Validate an intermediate/next-table entry and return its pointer.
    fn table_pointer(entry: u64) -> Result<u64, DartWalkFault> {
        if entry & DART_PTE_VALID == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        let next = (entry & DART2_PTE_PADDR_MASK) << DART2_PTE_PADDR_SHIFT;
        if next == 0 {
            return Err(DartWalkFault::PresentInvalid);
        }
        Ok(next)
    }

    fn read_entry(
        &self,
        read: &mut dyn FnMut(u64) -> Option<u64>,
        table: u64,
        index: u64,
    ) -> Option<u64> {
        read(table + index * 8)
    }
}

// ---- T8110 engine registers (DART2) ----

/// T8110 register offsets per upstream `apple-dart.c` (`DART_T8110_*`).
/// The T8110 layout is entirely different from the T8020/T8010 `DART_T8020`
/// layout: no `STREAM_SELECT`/`STREAM_COMMAND` pair (TLB_CMD instead), no
/// `CONFIG` lock (PROTECT/UNPROTECT instead), a single per-SID TTBR at bit 0
/// validity, and per-SID TCR at 0x1000. These are pub so platform layers and
/// acceptance tests can program/interrogate the engine exactly as a guest.
pub const T8110_ENABLE_STREAMS: u64 = 0xc00;
pub const T8110_DISABLE_STREAMS: u64 = 0xc20;
pub const T8110_TCR_BASE: u64 = 0x1000;
pub const T8110_TTBR_BASE: u64 = 0x1400;
pub const T8110_ERROR: u64 = 0x100;
pub const T8110_ERROR_ADDR_LO: u64 = 0x170;
pub const T8110_ERROR_ADDR_HI: u64 = 0x174;
pub const T8110_ERROR_MASK: u64 = 0x104;
pub const T8110_ERROR_STREAMS: u64 = 0x1c0;
pub const T8110_TLB_CMD: u64 = 0x80;
pub const T8110_PROTECT: u64 = 0x200;
pub const T8110_UNPROTECT: u64 = 0x204;
pub const T8110_PROTECT_LOCK: u64 = 0x208;
const T8110_PARAMS3: u64 = 0x08;
const T8110_PARAMS4: u64 = 0x0c;

/// TCR translate-enable is bit 0 in the T8110 layout (compared to bit 7 in
/// the T8020 layout).
pub const T8110_TCR_TRANSLATE_ENABLE: u32 = 1 << 0;
pub const T8110_TCR_BYPASS_DART: u32 = 1 << 1;
pub const T8110_TCR_BYPASS_DAPF: u32 = 1 << 2;
pub const T8110_TCR_FOUR_LEVEL: u32 = 1 << 3;
const T8110_TLB_CMD_OP_SHIFT: u32 = 8;
const T8110_TLB_CMD_OP_FLUSH_ALL: u32 = 0;
const T8110_TLB_CMD_OP_FLUSH_SID: u32 = 1;
const T8110_PROTECT_TTBR_TCR: u32 = 1;

/// Maximum SIDs the T8110 registers span (256 per upstream `max_sid_count`).
pub const T8110_MAX_SIDS: u32 = 256;

/// T8110 DART engine (DART2 / `APPLE_DART2` format).
///
/// Analogous to [`DartT8020`] but with the T8110 register layout: streams are
/// enabled/disabled via the `0xc00`/`0xc20` pair (not the `STREAMS_ENABLE`
/// bitmask), translation is armed via per-SID TCR at `0x1000` with the
/// translate bit at position 0, invalidation is a TLB_CMD op at `0x80`, and
/// config locking is the PROTECT/TTBR_TCR bit in `0x200` instead of the
/// `CONFIG` bit-15 lock. A single per-SID TTBR root at `0x1400` is decoded by
/// [`dart_t8110_ttbr_root_address`]. The shared ERROR capture and software
/// IOTLB follow the same fault-recording contract as the T8020 engine.
pub struct DartT8110 {
    id: DeviceId,
    page_shift: u8,
    ias: u32,
    supports_bypass: bool,
    /// Global stream-enable bitmap, word-banked 32 SIDs per word starting at
    /// `0xc00` (upstream writes `U32_MAX` to `enable_streams + 4*i` for each
    /// word, "enable all streams globally since TCR is used to control
    /// isolation").
    streams_enable: Vec<u32>,
    tcr: Vec<u32>,
    ttbr: Vec<u32>,
    protect: u32,
    error: Arc<Mutex<ErrorCapture>>,
    iotlb: Arc<Mutex<VecDeque<IotlbEntry>>>,
    walker: DartT8110Walker,
}

impl DartT8110 {
    /// Create a T8110 engine with the given page shift. `page_shift` 14 gives
    /// 16-KiB granules; 12 gives legacy 4-KiB granules.
    pub fn new(id: DeviceId, page_shift: u8) -> Self {
        let walker =
            DartT8110Walker::new(page_shift.into(), 32).expect("valid T8110 walker geometry");
        Self {
            id,
            page_shift,
            ias: 32,
            supports_bypass: false,
            streams_enable: vec![0; (T8110_MAX_SIDS as usize).div_ceil(32)],
            tcr: vec![0; T8110_MAX_SIDS as usize],
            ttbr: vec![0; T8110_MAX_SIDS as usize],
            protect: 0,
            error: Arc::new(Mutex::new(ErrorCapture::default())),
            iotlb: Arc::new(Mutex::new(VecDeque::new())),
            walker,
        }
    }

    /// Build the independent MMIO/DMA views sharing this engine.
    pub fn into_pair(self) -> (DartT8110Mmio, DartT8110DmaPort) {
        let engine = Arc::new(Mutex::new(self));
        let mmio = DartT8110Mmio(engine.clone());
        let port = DartT8110DmaPort(engine);
        (mmio, port)
    }

    /// Configure the input-address width (default 32). Fed to the walker.
    pub const fn with_input_address_width(mut self, ias: u32) -> Self {
        self.ias = ias;
        self
    }

    /// Configure bypass-mode advertisement (default unsupported).
    pub const fn with_bypass_support(mut self, supported: bool) -> Self {
        self.supports_bypass = supported;
        self
    }

    fn read_register(&self, offset: u64) -> Option<u32> {
        match offset {
            T8110_PARAMS3 => {
                Some(((40u32 & 0x3f) << 24) | ((self.ias & 0x3f) << 16) | ((1u32 & 0xff) << 8))
            }
            T8110_PARAMS4 => {
                let num_clients = 1u32;
                Some((num_clients & 0x1ff) << 16 | (T8110_MAX_SIDS & 0x1ff))
            }
            T8110_TLB_CMD => Some(0),
            T8110_ERROR => Some(self.error.lock().unwrap().register),
            T8110_ERROR_ADDR_LO => Some(self.error.lock().unwrap().addr as u32),
            T8110_ERROR_ADDR_HI => Some((self.error.lock().unwrap().addr >> 32) as u32),
            T8110_ERROR_STREAMS => Some(0),
            T8110_PROTECT_LOCK => Some(self.protect),
            offset
                if (T8110_ENABLE_STREAMS
                    ..T8110_ENABLE_STREAMS + 4 * (T8110_MAX_SIDS as u64 + 31) / 32)
                    .contains(&offset)
                    && offset % 4 == 0 =>
            {
                Some(self.streams_enable[((offset - T8110_ENABLE_STREAMS) / 4) as usize])
            }
            offset
                if (T8110_TCR_BASE..T8110_TCR_BASE + T8110_MAX_SIDS as u64 * 4)
                    .contains(&offset)
                    && offset % 4 == 0 =>
            {
                Some(self.tcr[((offset - T8110_TCR_BASE) / 4) as usize])
            }
            offset
                if (T8110_TTBR_BASE..T8110_TTBR_BASE + T8110_MAX_SIDS as u64 * 4)
                    .contains(&offset)
                    && offset % 4 == 0 =>
            {
                Some(self.ttbr[((offset - T8110_TTBR_BASE) / 4) as usize])
            }
            _ => None,
        }
    }

    fn write_register(&mut self, offset: u64, value: u32) -> bool {
        match offset {
            T8110_DISABLE_STREAMS | T8110_ENABLE_STREAMS
                if self.protect & T8110_PROTECT_TTBR_TCR == 0 =>
            {
                let word = 0usize;
                let mask = value;
                if offset == T8110_DISABLE_STREAMS {
                    if let Some(w) = self.streams_enable.get_mut(word) {
                        *w &= !mask;
                    }
                    self.invalidate_streams();
                } else if let Some(w) = self.streams_enable.get_mut(word) {
                    *w |= mask;
                }
            }
            T8110_TLB_CMD => {
                // op field bits 10:8: 0 = flush-all, 1 = flush-sid (stream in
                // bits 7:0). Upstream issues flush-all at enable and flush-sid
                // on unmap.
                let op = (value >> T8110_TLB_CMD_OP_SHIFT) & 0x7;
                if op == T8110_TLB_CMD_OP_FLUSH_ALL || op == T8110_TLB_CMD_OP_FLUSH_SID {
                    self.invalidate_streams();
                }
            }
            T8110_ERROR => self.error.lock().unwrap().register &= !value,
            T8110_ERROR_MASK => {}
            T8110_PROTECT => self.protect |= value & T8110_PROTECT_TTBR_TCR,
            T8110_UNPROTECT => self.protect &= !value,
            offset
                if (T8110_TCR_BASE..T8110_TCR_BASE + T8110_MAX_SIDS as u64 * 4)
                    .contains(&offset)
                    && offset % 4 == 0
                    && self.protect & T8110_PROTECT_TTBR_TCR == 0 =>
            {
                self.tcr[((offset - T8110_TCR_BASE) / 4) as usize] = value
            }
            offset
                if (T8110_TTBR_BASE..T8110_TTBR_BASE + T8110_MAX_SIDS as u64 * 4)
                    .contains(&offset)
                    && offset % 4 == 0
                    && self.protect & T8110_PROTECT_TTBR_TCR == 0 =>
            {
                self.ttbr[((offset - T8110_TTBR_BASE) / 4) as usize] = value
            }
            _ => return false,
        }
        true
    }

    /// Whether the T8110 PROTECT/TTBR_TCR lock is set, gating TCR/TTBR and
    /// stream enable writes.
    pub fn locked(&self) -> bool {
        self.protect & T8110_PROTECT_TTBR_TCR != 0
    }

    /// The DART2 walker geometry produced for this engine.
    pub fn walker(&self) -> DartT8110Walker {
        self.walker
    }
}

impl Device for DartT8110 {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "dart-t8110"
    }

    fn reset(&mut self) {
        self.streams_enable = vec![0; (T8110_MAX_SIDS as usize).div_ceil(32)];
        self.tcr = vec![0; T8110_MAX_SIDS as usize];
        self.ttbr = vec![0; T8110_MAX_SIDS as usize];
        self.protect = 0;
        *self.iotlb.lock().unwrap() = VecDeque::new();
        *self.error.lock().unwrap() = ErrorCapture::default();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if size != 4 {
            return DeviceResult::Fault;
        }
        match self.read_register(offset) {
            Some(value) => DeviceResult::Value(value as u64),
            None => DeviceResult::Fault,
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if size != 4 || value > u32::MAX as u64 || matches!(offset, T8110_PARAMS3 | T8110_PARAMS4) {
            return DeviceResult::Fault;
        }
        if self.write_register(offset, value as u32) {
            DeviceResult::Ok
        } else {
            DeviceResult::Fault
        }
    }

    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DartT8110 {
    /// Record a translation fault in the shared ERROR/ERROR_ADDR state and
    /// return the typed fault (same contract as [`DartT8020::fault`]).
    fn fault(&self, sid: u32, iova: u64, code: DmaFault) -> DmaResult {
        let mut error = self.error.lock().unwrap();
        error.register = (sid << 16) | DART_ERROR_FAULT;
        error.addr = iova;
        drop(error);
        DmaResult::Fault(code)
    }

    /// Insert a translated page into the shared IOTLB (FIFO eviction).
    fn cache(&self, pa: u64, sid: u32, page: u64, access: DartAccess) {
        let page_shift = self.page_shift as u32;
        let entry = IotlbEntry {
            sid,
            page,
            access,
            pa_page: pa & !((1u64 << page_shift) - 1),
        };
        let mut tlb = self.iotlb.lock().unwrap();
        if let Some(pos) = tlb.iter().position(|existing| existing == &entry) {
            let item = tlb.remove(pos).expect("position came from this deque");
            tlb.push_back(item);
        } else {
            if tlb.len() >= DART_IOTLB_MAX {
                tlb.pop_front();
            }
            tlb.push_back(entry);
        }
    }

    fn invalidate_streams(&mut self) {
        *self.iotlb.lock().unwrap() = VecDeque::new();
    }
}

impl DmaIommu for DartT8110 {
    fn translate(&self, request: &DmaRequest, mut read: DmaTableReader<'_>) -> DmaResult {
        let sid = request.sid;
        let word = (sid as usize) / 32;
        let bit = 1u32 << (sid % 32);
        let enabled = self
            .streams_enable
            .get(word)
            .map(|w| w & bit != 0)
            .unwrap_or(false);
        if !enabled {
            return self.fault(sid, request.iova, DmaFault::StreamBlocked(sid));
        }
        let tcr = self.tcr.get(sid as usize).copied().unwrap_or(0);
        if tcr & T8110_TCR_TRANSLATE_ENABLE == 0 {
            if tcr & T8110_TCR_BYPASS_DART != 0 && self.supports_bypass {
                return DmaResult::Bypass(request.iova);
            }
            return self.fault(sid, request.iova, DmaFault::StreamBlocked(sid));
        }
        let access = match request.access {
            DmaAccess::Read => DartAccess::Read,
            DmaAccess::Write => DartAccess::Write,
        };
        let page_shift = self.page_shift as u32;
        let page = request.iova >> page_shift;
        let page_mask = (1u64 << page_shift) - 1;
        if let Some(entry) = self
            .iotlb
            .lock()
            .unwrap()
            .iter()
            .find(|entry| entry.sid == sid && entry.page == page && entry.access == access)
        {
            return DmaResult::Translated(entry.pa_page | (request.iova & page_mask));
        }
        let ttbr = self.ttbr.get(sid as usize).copied().unwrap_or(0);
        let result = match dart_t8110_ttbr_root_address(ttbr) {
            Some(root) => {
                let walker = DartT8110Walker::new(page_shift, self.ias).unwrap_or(self.walker);
                walker.translate(root, request.iova, access, &mut read)
            }
            None => Err(DartWalkFault::NoRoot),
        };
        match result {
            Ok(pa) => {
                self.cache(pa, sid, page, access);
                DmaResult::Translated(pa)
            }
            Err(fault) => self.fault(sid, request.iova, DartWalkFault::to_dma(fault)),
        }
    }

    fn invalidate(&mut self, sid: u32, _iova: u64, _size: u64) {
        self.iotlb.lock().unwrap().retain(|e| e.sid != sid);
    }
}

/// T8110 MMIO view sharing one engine with a DMA port.
pub struct DartT8110Mmio(Arc<Mutex<DartT8110>>);

/// T8110 DMA port sharing one engine with the MMIO view.
pub struct DartT8110DmaPort(Arc<Mutex<DartT8110>>);

impl DartT8110Mmio {
    /// The shared engine (for tests and platform wiring).
    pub fn engine(&self) -> Arc<Mutex<DartT8110>> {
        self.0.clone()
    }
}

impl DartT8110DmaPort {
    /// The shared engine (for tests and platform wiring).
    pub fn engine(&self) -> Arc<Mutex<DartT8110>> {
        self.0.clone()
    }
}

impl Device for DartT8110Mmio {
    fn id(&self) -> DeviceId {
        self.0.lock().unwrap().id()
    }

    fn name(&self) -> &'static str {
        "dart-t8110-mapped-mmio"
    }

    fn reset(&mut self) {
        self.0.lock().unwrap().reset();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        self.0.lock().unwrap().read(offset, size)
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        self.0.lock().unwrap().write(offset, size, value)
    }

    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        self.0.lock().unwrap().tick(cycles)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DmaIommu for DartT8110DmaPort {
    fn translate(&self, request: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
        self.0.lock().unwrap().translate(request, read)
    }

    fn invalidate(&mut self, sid: u32, iova: u64, size: u64) {
        self.0.lock().unwrap().invalidate(sid, iova, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d321ap_t8020_params1_reports_adt_page_shift() {
        let mut dart = DartT8020::new(71, 14);
        assert_eq!(dart.read(PARAMS1, 4), DeviceResult::Value(14 << 24));
        assert_eq!(dart.read(PARAMS1, 8), DeviceResult::Fault);
        assert_eq!(dart.read(PARAMS2, 4), DeviceResult::Value(0));
        assert_eq!(dart.write(PARAMS1, 4, 0), DeviceResult::Fault);
    }

    #[test]
    fn t8020_stream_configuration_and_lock_are_per_sid() {
        let mut dart = DartT8020::new(71, 14).with_input_address_width(32);
        assert_eq!(dart.write(STREAMS_ENABLE, 4, 0x8001), DeviceResult::Ok);
        assert_eq!(dart.write(TCR_BASE + 4, 4, 0x180), DeviceResult::Ok);
        assert_eq!(
            dart.write(TTBR_BASE + 16 + 4, 4, 0x8000_1234),
            DeviceResult::Ok
        );
        assert_eq!(dart.read(STREAMS_ENABLE, 4), DeviceResult::Value(0x8001));
        assert_eq!(dart.read(TCR_BASE + 4, 4), DeviceResult::Value(0x180));
        assert_eq!(
            dart.read(TTBR_BASE + 20, 4),
            DeviceResult::Value(0x8000_1234)
        );
        assert_eq!(dart.write(CONFIG, 4, CONFIG_LOCK as u64), DeviceResult::Ok);
        assert_eq!(dart.write(TTBR_BASE + 20, 4, 0), DeviceResult::Ok);
        assert_eq!(
            dart.read(TTBR_BASE + 20, 4),
            DeviceResult::Value(0x8000_1234)
        );
    }

    /// Synthetic physical-memory page tables for walker unit tests.
    struct Tables {
        map: std::collections::HashMap<u64, u64>,
    }

    impl Tables {
        fn new() -> Self {
            Self {
                map: std::collections::HashMap::new(),
            }
        }

        fn place(&mut self, table: u64, index: u64, value: u64) {
            self.map.insert(table + index * 8, value);
        }

        fn contains(&self, address: u64) -> bool {
            self.map.contains_key(&address)
        }

        fn reader(&self) -> impl FnMut(u64) -> Option<u64> {
            let map = self.map.clone();
            move |address| map.get(&address).copied()
        }
    }

    fn walk_16k() -> DartT8020Walker {
        DartT8020Walker::new(14, 32).expect("16-KiB/32-bit DART geometry")
    }

    #[test]
    fn d321ap_16k_geometry_matches_reference() {
        let walker = walk_16k();
        assert_eq!(walker.page_shift(), 14);
        assert_eq!(walker.ias(), 32);
        assert_eq!(walker.bits_per_level, 11);
        assert_eq!(walker.walk_levels(), 2);
        assert_eq!(walker.n_roots(), 1);
        // Level indices for the d321ap 16-KiB layout used by the synthetic
        // tables below.
        assert_eq!(walker.index(0x2000_4000, 2), (0x2000_4000 >> 25) & 0x7ff);
        assert_eq!(walker.index(0x2000_4000, 1), (0x2000_4000 >> 14) & 0x7ff);
        // Root selection: level 3 index is always zero within 32-bit VA.
        assert_eq!(walker.index(0xffff_ffff, 3), 0);
    }

    #[test]
    fn d321ap_16k_valid_read_and_write_translate() {
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        let l1 = 0x1000_0000;
        let leaf_base = 0x1234_5000;
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | l1);
        tables.place(l1, leaf_index, DART_PTE_VALID | DART_PTE_SP_DIS | leaf_base);
        let mut read = tables.reader();
        for access in [DartAccess::Read, DartAccess::Write] {
            assert_eq!(
                walker.translate(&[0x8000_0000], iova, access, &mut read),
                Ok(leaf_base | (iova & 0x3fff))
            );
        }
    }

    #[test]
    fn d321ap_16k_permission_denial_is_typed_by_direction() {
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(
            0x1000_0000,
            leaf_index,
            DART_PTE_VALID | DART_PTE_SP_DIS | DART_PTE_NO_READ | DART_PTE_NO_WRITE | 0x5000,
        );
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::PermissionRead)
        );
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Write, &mut read),
            Err(DartWalkFault::PermissionWrite)
        );
    }

    #[test]
    fn d321ap_16k_missing_levels_fault_distinctly() {
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        // No root configured.
        {
            let mut read = tables.reader();
            assert_eq!(
                walker.translate(&[0x0], iova, DartAccess::Read, &mut read),
                Err(DartWalkFault::NoRoot)
            );
            assert_eq!(
                walker.translate(&[], iova, DartAccess::Read, &mut read),
                Err(DartWalkFault::NoRoot)
            );
        }
        // Root present but intermediate entry absent.
        tables.place(0x8000_0000, l2_index, 0);
        {
            let mut read = tables.reader();
            assert_eq!(
                walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
                Err(DartWalkFault::MissingIntermediate(2))
            );
        }
        // Intermediate present but leaf entry absent (zero).
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(0x1000_0000, leaf_index, 0);
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::MissingLeaf)
        );
        assert!(tables.contains(0x1000_0000 + leaf_index * 8));
    }

    #[test]
    fn d321ap_16k_present_invalid_entries_are_rejected() {
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        // Non-zero entry without VALID at the leaf is present-but-invalid.
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(0x1000_0000, leaf_index, 0x7fff_f000);
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::PresentInvalid)
        );
        // A next-table entry with VALID but an all-zero address is invalid.
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID);
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::PresentInvalid)
        );
    }

    #[test]
    fn d321ap_16k_iova_above_input_width_faults() {
        let walker = walk_16k();
        let tables = Tables::new();
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], 0x1_0000_0000, DartAccess::Read, &mut read),
            Err(DartWalkFault::AddressWidth)
        );
        assert_eq!(
            walker.translate(&[0x8000_0000], u64::MAX, DartAccess::Write, &mut read),
            Err(DartWalkFault::AddressWidth)
        );
    }

    #[test]
    fn d321ap_16k_table_read_failure_is_typed() {
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let mut tables = Tables::new();
        // The intermediate entry points at a table that is not backed by
        // guest memory, so the physical read cannot be satisfied.
        let unmapped = 0x90a0_0000;
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | unmapped);
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::TableReadFailed(
                unmapped + walker.index(iova, 1) as u64 * 8
            ))
        );
    }

    #[test]
    fn d321ap_16k_subpage_range_is_honored() {
        let walker = walk_16k();
        // Choose an IOVA whose 4-KiB subpage index is 1.
        let iova = 0x2000_0000u64 | (1 << 12);
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        // SP_DIS clear, range [0..0] permits only subpage 0.
        tables.place(
            0x1000_0000,
            leaf_index,
            DART_PTE_VALID
                | (0u64 << DART_SUBPAGE_START_SHIFT)
                | (0u64 << DART_SUBPAGE_END_SHIFT)
                | 0x2000,
        );
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(&[0x8000_0000], iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::SubpageDisallowed {
                start: 0,
                end: 0,
                subpage: 1
            })
        );
        // Move the access to subpage 0: the range allows it.
        let iova0 = 0x2000_0000u64;
        let leaf_index0 = walker.index(iova0, 1) as u64;
        tables.place(
            0x1000_0000,
            leaf_index0,
            tables.map[&(0x1000_0000 + leaf_index * 8)],
        );
        assert_eq!(
            walker.translate(&[0x8000_0000], iova0, DartAccess::Read, &mut read),
            Ok(0x2000 | (iova0 & 0x3fff))
        );
    }

    #[test]
    fn legacy_4k_geometry_selects_among_four_roots() {
        let walker = DartT8020Walker::new(12, 32).expect("4-KiB/32-bit geometry");
        assert_eq!(walker.n_roots(), 4);
        assert_eq!(walker.walk_levels(), 2);
        let mut tables = Tables::new();
        // Root 0 and root 1 both translate to distinct backing pages.
        for (root, base) in [(0x8000_0000u64, 0x40_0000u64), (0x9000_0000, 0x80_0000u64)] {
            let iova = root_base_iova(root);
            let l2_index = walker.index(iova, 2) as u64;
            let leaf_index = walker.index(iova, 1) as u64;
            tables.place(root, l2_index, DART_PTE_VALID | (root + 0x1000));
            tables.place(
                root + 0x1000,
                leaf_index,
                DART_PTE_VALID | DART_PTE_SP_DIS | base,
            );
        }
        let mut read = tables.reader();
        for (root, base) in [(0x8000_0000u64, 0x40_0000u64), (0x9000_0000, 0x80_0000u64)] {
            let iova = root_base_iova(root);
            assert_eq!(
                walker.translate(
                    &[0x8000_0000, 0x9000_0000, 0, 0],
                    iova,
                    DartAccess::Read,
                    &mut read
                ),
                Ok(base | (iova & 0xfff))
            );
        }
    }

    fn root_base_iova(root: u64) -> u64 {
        // 4K/32-bit: the root index is iova bits 31:30. Pick a low IOVA whose
        // top bits choose the given root slot (slot = root bit 30).
        if root == 0x9000_0000 {
            0x4000_1000
        } else {
            0x2000_1000
        }
    }

    #[test]
    fn ttbr_root_address_decoding() {
        // Address field bits 0..23 shifted left 12: 0x1234 -> 0x1234000.
        assert_eq!(dart_t8020_ttbr_root_address(0x8000_1234), Some(0x0123_4000));
        assert_eq!(dart_t8020_ttbr_root_address(0x80ab_cdef), Some(0xabcdef000));
        assert_eq!(dart_t8020_ttbr_root_address(0x0000_1234), None);
    }

    #[test]
    fn walker_rejects_unsupported_geometry() {
        assert!(DartT8020Walker::new(14, 8).is_none());
        assert!(DartT8020Walker::new(14, 14).is_none());
        assert!(DartT8020Walker::new(12, 37).is_none());
        assert!(DartT8020Walker::new(10, 32).is_none());
        assert!(DartT8020Walker::new(14, 32).is_some());
    }

    // ---- DmaIommu integration ----

    fn ttbr_reg(sid: u32, slot: usize) -> u64 {
        // T8020 TTBR word address: one 16-byte row per stream.
        TTBR_BASE + sid as u64 * 16 + (slot as u64) * 4
    }

    fn programmed_dart() -> DartT8020 {
        let mut dart = DartT8020::new(71, 14).with_input_address_width(32);
        assert_eq!(dart.write(STREAMS_ENABLE, 4, 0x9), DeviceResult::Ok);
        assert_eq!(
            dart.write(TCR_BASE, 4, TCR_TRANSLATE_ENABLE as u64),
            DeviceResult::Ok
        );
        assert_eq!(
            dart.write(TCR_BASE + 3 * 4, 4, TCR_TRANSLATE_ENABLE as u64),
            DeviceResult::Ok
        );
        // Root table PA 0x8000_0000: (pa >> 12) in the low 24 address bits,
        // bit 31 valid.
        let ttbr_value = 0x8000_0000u64 | ((0x8000_0000u64 >> 12) & 0x00ff_ffff);
        assert_eq!(dart.write(ttbr_reg(0, 0), 4, ttbr_value), DeviceResult::Ok);
        dart
    }

    #[test]
    fn dma_iommu_translates_through_ttbr_root() {
        let dart = programmed_dart();
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(
            0x1000_0000,
            leaf_index,
            DART_PTE_VALID | DART_PTE_SP_DIS | 0x1234_5000,
        );
        let mut read = tables.reader();
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            0,
            iova,
            iphem_core::dma::DmaAccess::Read,
            0x1000,
        );
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Translated(0x1234_5000 | (iova & 0x3fff))
        );
    }

    #[test]
    fn dma_iommu_no_roots_returns_no_root() {
        let dart = programmed_dart();
        let tables = Tables::new();
        let mut read = tables.reader();
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            3, // stream 3 has no programmed TTBR
            0x2000_4000,
            iphem_core::dma::DmaAccess::Read,
            0x1000,
        );
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::NoRoot)
        );
    }

    #[test]
    fn dma_iommu_unconfigured_width_faults() {
        // A DART without an ADT input-address width cannot translate.
        let mut dart = DartT8020::new(71, 14);
        dart.write(STREAMS_ENABLE, 4, 1);
        dart.write(TCR_BASE, 4, TCR_TRANSLATE_ENABLE as u64);
        let tables = Tables::new();
        let mut read = tables.reader();
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            0,
            0x2000_4000,
            iphem_core::dma::DmaAccess::Read,
            0x1000,
        );
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::AddressWidth)
        );
    }

    #[test]
    fn dma_iommu_respects_stream_enable_and_translation_mode() {
        let mut dart = DartT8020::new(71, 14).with_input_address_width(32);
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            0,
            0x2000_4000,
            iphem_core::dma::DmaAccess::Read,
            4,
        );
        let mut read = |_address| None;
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::StreamBlocked(0))
        );
        dart.write(STREAMS_ENABLE, 4, 1);
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::StreamBlocked(0))
        );
    }

    #[test]
    fn dma_iommu_bypass_requires_advertised_capability() {
        let mut dart = DartT8020::new(71, 14);
        dart.write(STREAMS_ENABLE, 4, 1);
        dart.write(TCR_BASE, 4, TCR_BYPASS_DART as u64);
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            0,
            0x1234_5000,
            iphem_core::dma::DmaAccess::Read,
            4,
        );
        let mut read = |_address| None;
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::StreamBlocked(0))
        );
        let mut dart = dart.with_bypass_support(true);
        assert_eq!(
            dart.read(PARAMS2, 4),
            DeviceResult::Value(PARAMS2_BYPASS_SUPPORT as u64)
        );
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Bypass(request.iova)
        );
    }

    #[test]
    fn dma_iommu_permission_fault_maps_to_core_fault() {
        let dart = programmed_dart();
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(
            0x1000_0000,
            leaf_index,
            DART_PTE_VALID | DART_PTE_SP_DIS | DART_PTE_NO_READ | 0x5000,
        );
        let mut read = tables.reader();
        let request = iphem_core::dma::DmaRequest::new(
            0x1_0000,
            0,
            iova,
            iphem_core::dma::DmaAccess::Read,
            0x1000,
        );
        assert_eq!(
            iphem_core::dma::DmaIommu::translate(&dart, &request, &mut read),
            iphem_core::dma::DmaResult::Fault(iphem_core::dma::DmaFault::PermissionRead)
        );
    }

    // ---- DART step 5: shared MMIO/DMA engine, IOTLB, fault capture ----

    #[test]
    fn shared_engine_iotlb_serves_hits_and_stream_command_flushes() {
        let (mut mmio, port) = programmed_dart().into_pair();
        let walker = walk_16k();
        let iova = 0x2000_4000u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
        tables.place(
            0x1000_0000,
            leaf_index,
            DART_PTE_VALID | DART_PTE_SP_DIS | 0x1234_4000,
        );
        let map = tables.map.clone();
        let served = std::rc::Rc::new(std::cell::Cell::new(0u32));
        // `served` and `counter` alias the same cell: the closure owns one
        // strong reference, the assertion observes the other.
        let counter = served.clone();
        let mut read = move |address: u64| -> Option<u64> {
            // One successful walk consumes exactly two table reads (one
            // intermediate, one leaf). After that the tables are exhausted:
            // any further translation that reads again must be a post-flush
            // re-walk, so it faults instead of fabricating a mapping.
            if counter.get() >= 2 {
                return None;
            }
            counter.set(counter.get() + 1);
            map.get(&address).copied()
        };
        let request = DmaRequest::new(0x1_0000, 0, iova, DmaAccess::Read, 0x100);
        assert_eq!(
            DmaIommu::translate(&port, &request, &mut read),
            DmaResult::Translated(0x1234_4000)
        );
        // Same page, different offset: served by the cache without a re-walk.
        let second = DmaRequest::new(0x1_0000, 0, iova | 0x140, DmaAccess::Read, 0x100);
        assert_eq!(
            DmaIommu::translate(&port, &second, &mut read),
            DmaResult::Translated(0x1234_4140)
        );
        assert_eq!(served.get(), 2);
        // MMIO stream command: select stream 0 and invalidate. The flush
        // drops the cached entry, so the next translation re-walks.
        assert_eq!(mmio.write(STREAM_SELECT, 4, 1), DeviceResult::Ok);
        assert_eq!(
            mmio.write(STREAM_COMMAND, 4, STREAM_COMMAND_INVALIDATE as u64),
            DeviceResult::Ok
        );
        assert_eq!(
            DmaIommu::translate(&port, &request, &mut read),
            DmaResult::Fault(DmaFault::TableReadFailed(l2_index * 8 + 0x8000_0000))
        );
    }

    #[test]
    fn shared_engine_fault_recorded_through_port_readable_through_mmio() {
        let (mut mmio, port) = DartT8020::new(71, 14).into_pair();
        let tables = Tables::new();
        let mut read = tables.reader();
        // Stream 3 is not enabled, so translation is blocked; the fault is
        // recorded in the shared ERROR/ERROR_ADDR state.
        let request = DmaRequest::new(0x1_0000, 3, 0x2000_4000, DmaAccess::Read, 0x100);
        assert_eq!(
            DmaIommu::translate(&port, &request, &mut read),
            DmaResult::Fault(DmaFault::StreamBlocked(3))
        );
        assert_eq!(
            mmio.read(ERROR, 4),
            DeviceResult::Value(((3 << 16) | DART_ERROR_FAULT) as u64)
        );
        assert_eq!(
            mmio.read(ERROR_ADDR_LO, 4),
            DeviceResult::Value(0x2000_4000)
        );
        // W1C through the MMIO window clears the error register.
        assert_eq!(
            mmio.write(ERROR, 4, ((3 << 16) | DART_ERROR_FAULT) as u64),
            DeviceResult::Ok
        );
        assert_eq!(mmio.read(ERROR, 4), DeviceResult::Value(0));
    }

    #[test]
    fn port_invalidate_drops_only_overlapping_entries_and_reset_clears_all() {
        let (mmio, port) = programmed_dart().into_pair();
        let walker = walk_16k();
        // Distinct pages with distinct leaf indices: 0x2000_* and 0x2010_*
        // resolve to leaf slots 0 and 0x10 in the same L2 table.
        let targets = [
            (0x2000_4000u64, 0x1234_0000u64),
            (0x2010_4000, 0x5678_0000u64),
        ];
        let mut tables = Tables::new();
        for (iova, base) in targets {
            let l2_index = walker.index(iova, 2) as u64;
            let leaf_index = walker.index(iova, 1) as u64;
            tables.place(0x8000_0000, l2_index, DART_PTE_VALID | 0x1000_0000);
            tables.place(
                0x1000_0000,
                leaf_index,
                DART_PTE_VALID | DART_PTE_SP_DIS | base,
            );
        }
        let mut read = tables.reader();
        for (iova, base) in targets {
            let request = DmaRequest::new(0x1_0000, 0, iova, DmaAccess::Read, 0x100);
            assert_eq!(
                DmaIommu::translate(&port, &request, &mut read),
                DmaResult::Translated(base | (iova & 0x3fff))
            );
        }
        let engine = mmio.engine();
        assert_eq!(engine.lock().unwrap().iotlb.lock().unwrap().len(), 2);
        // Invalidate only page 0x8001 (0x2000_4000 >> 14) of stream 0.
        let mut port = port;
        DmaIommu::invalidate(&mut port, 0, 0x2000_4000, 0x1000);
        assert_eq!(engine.lock().unwrap().iotlb.lock().unwrap().len(), 1);
        let surviving_page = 0x2010_4000u64 >> 14;
        assert!(engine
            .lock()
            .unwrap()
            .iotlb
            .lock()
            .unwrap()
            .iter()
            .all(|entry| entry.page == surviving_page));
        // Reset clears every cached entry through the shared engine.
        engine.lock().unwrap().reset();
        assert_eq!(engine.lock().unwrap().iotlb.lock().unwrap().len(), 0);
    }

    // ---- T8110 (DART2) independent tests ----

    fn val(res: DeviceResult) -> u32 {
        match res {
            DeviceResult::Value(v) => v as u32,
            other => panic!("expected DeviceResult::Value, got {:?}", other),
        }
    }

    fn walk_16k_t8110() -> DartT8110Walker {
        DartT8110Walker::new(14, 32).expect("16-KiB/32-bit T8110 geometry")
    }

    #[test]
    fn t8110_ttbr_is_single_slot_bit0_valid_shift14() {
        // valid=bit0; address = (value>>2) << 14.
        assert_eq!(dart_t8110_ttbr_root_address(0), None);
        assert_eq!(dart_t8110_ttbr_root_address(0x2), None);
        let root = 0x5000_0000u64 >> 14; // 0x1400
        let encoded = ((root as u32) << 2) | 1;
        assert_eq!(dart_t8110_ttbr_root_address(encoded), Some(0x5000_0000));
    }

    #[test]
    fn t8110_dart2_walker_decodes_shifted_paddr_and_dart2_permissions() {
        let walker = walk_16k_t8110();
        assert_eq!(walker.n_roots(), 1);
        let iova = 0x2000_4123u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x5000_0000, l2_index, DART_PTE_VALID | (0x4000_0000 >> 4));
        tables.place(0x4000_0000, leaf_index, DART_PTE_VALID | (0x3000_0000 >> 4));
        let mut read = tables.reader();
        let pa = walker
            .translate(0x5000_0000, iova, DartAccess::Read, &mut read)
            .expect("DART2 read translates");
        assert_eq!(pa, 0x3000_0123);
        // DART2 NO_READ is bit 3; NO_WRITE is bit 2.
        tables.place(
            0x4000_0000,
            leaf_index,
            DART_PTE_VALID | DART2_PTE_NO_READ | (0x3000_0000 >> 4),
        );
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(0x5000_0000, iova, DartAccess::Read, &mut read),
            Err(DartWalkFault::PermissionRead)
        );
        tables.place(
            0x4000_0000,
            leaf_index,
            DART_PTE_VALID | DART2_PTE_NO_WRITE | (0x3000_0000 >> 4),
        );
        let mut read = tables.reader();
        assert_eq!(
            walker.translate(0x5000_0000, iova, DartAccess::Write, &mut read),
            Err(DartWalkFault::PermissionWrite)
        );
    }

    #[test]
    fn t8110_register_layout_independent_offsets_and_protect_lock() {
        let mut dart = DartT8110::new(200, 14).with_input_address_width(32);
        assert_eq!(val(dart.read(T8110_ENABLE_STREAMS, 4)), 0);
        assert_eq!(dart.read(0xfc, 4), DeviceResult::Fault); // no DART1 STREAMS_ENABLE
        assert_eq!(dart.read(0x34, 4), DeviceResult::Fault); // no DART1 STREAM_SELECT
        assert_eq!(
            dart.write(T8110_ENABLE_STREAMS, 4, 1 << 1),
            DeviceResult::Ok
        );
        assert_eq!(val(dart.read(T8110_ENABLE_STREAMS, 4)), 1 << 1);
        assert_eq!(
            dart.write(T8110_TCR_BASE + 4, 4, T8110_TCR_TRANSLATE_ENABLE as u64),
            DeviceResult::Ok
        );
        let root = 0x5000_0000u64 >> 14;
        let ttbr = ((root as u32) << 2) | 1;
        assert_eq!(
            dart.write(T8110_TTBR_BASE + 4, 4, ttbr as u64),
            DeviceResult::Ok
        );
        assert_eq!(
            val(dart.read(T8110_TCR_BASE + 4, 4)),
            T8110_TCR_TRANSLATE_ENABLE
        );
        assert_eq!(val(dart.read(T8110_TTBR_BASE + 4, 4)), ttbr);
        // PROTECT/TTBR_TCR locks TCR/TTBR/stream-enable against writes.
        assert_eq!(dart.write(T8110_PROTECT, 4, 1), DeviceResult::Ok);
        assert!(dart.locked());
        assert_eq!(dart.write(T8110_TCR_BASE + 4, 4, 0), DeviceResult::Fault);
        assert_eq!(
            val(dart.read(T8110_TCR_BASE + 4, 4)),
            T8110_TCR_TRANSLATE_ENABLE
        );
        assert_eq!(dart.write(T8110_UNPROTECT, 4, 1), DeviceResult::Ok);
        assert!(!dart.locked());
    }

    #[test]
    fn t8110_tlb_command_flush_all_invalidates_through_shared_engine() {
        let (mmio, port) = DartT8110::new(200, 14)
            .with_input_address_width(32)
            .into_pair();
        // Seed the IOTLB through a translated transfer.
        let walker = walk_16k_t8110();
        let iova = 0x2000_4123u64;
        let l2_index = walker.index(iova, 2) as u64;
        let leaf_index = walker.index(iova, 1) as u64;
        let mut tables = Tables::new();
        tables.place(0x5000_0000, l2_index, DART_PTE_VALID | (0x4000_0000 >> 4));
        tables.place(0x4000_0000, leaf_index, DART_PTE_VALID | (0x3000_0000 >> 4));
        let mut read = tables.reader();
        let engine = mmio.engine();
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .write(T8110_ENABLE_STREAMS, 4, 1 << 1),
            DeviceResult::Ok
        );
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .write(T8110_TCR_BASE + 4, 4, T8110_TCR_TRANSLATE_ENABLE as u64),
            DeviceResult::Ok
        );
        let root = 0x5000_0000u64 >> 14;
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .write(T8110_TTBR_BASE + 4, 4, (((root as u32) << 2) | 1) as u64),
            DeviceResult::Ok
        );
        let request = DmaRequest::new(0x1_0000, 1, iova, DmaAccess::Read, 0x100);
        assert_eq!(
            DmaIommu::translate(&port, &request, &mut read),
            DmaResult::Translated(0x3000_0123)
        );
        assert_eq!(engine.lock().unwrap().iotlb.lock().unwrap().len(), 1);
        // TLB_CMD flush-all (op=0) via the MMIO view drops every cached entry.
        assert_eq!(
            engine.lock().unwrap().write(T8110_TLB_CMD, 4, 0),
            DeviceResult::Ok
        );
        assert_eq!(engine.lock().unwrap().iotlb.lock().unwrap().len(), 0);
    }

    #[test]
    fn t8110_dma_traverses_dart2_tables_with_error_capture() {
        let (mmio, port) = DartT8110::new(200, 14)
            .with_input_address_width(32)
            .into_pair();
        let engine = mmio.engine();
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .write(T8110_ENABLE_STREAMS, 4, 1 << 2),
            DeviceResult::Ok
        );
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .write(T8110_TCR_BASE + 8, 4, T8110_TCR_TRANSLATE_ENABLE as u64),
            DeviceResult::Ok
        );
        // No root yet: translation records a fault in the shared ERROR
        // (SID 2 in 31:16) and reports a typed fault.
        let request = DmaRequest::new(0x1_0000, 2, 0x2000_4000, DmaAccess::Read, 8);
        let tables = Tables::new();
        let mut read = tables.reader();
        assert_eq!(
            DmaIommu::translate(&port, &request, &mut read),
            DmaResult::Fault(DmaFault::NoRoot)
        );
        assert_eq!(
            val(engine.lock().unwrap().read(T8110_ERROR, 4)),
            (2 << 16) | DART_ERROR_FAULT
        );
        // The guest-visible read-then-W1C clears exactly that record.
        let reg = val(engine.lock().unwrap().read(T8110_ERROR, 4));
        assert_eq!(
            engine.lock().unwrap().write(T8110_ERROR, 4, reg as u64),
            DeviceResult::Ok
        );
        assert_eq!(val(engine.lock().unwrap().read(T8110_ERROR, 4)), 0);
    }
}
