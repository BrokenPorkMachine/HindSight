use crate::cpu::{CpuRunState, CpuState};
use crate::device::{Device, DeviceId};
use crate::memory::{Ram, Rom, SparseRam};
use crate::trace::TraceEvent;
use std::collections::HashMap;
use std::env;

#[derive(Debug)]
pub enum SnapshotError {
    Io(std::io::Error),
    Format(String),
    VersionMismatch,
    InvalidData,
}
impl Clone for SnapshotError {
    fn clone(&self) -> Self {
        match self {
            SnapshotError::Io(e) => SnapshotError::Io(std::io::Error::new(e.kind(), e.to_string())),
            SnapshotError::Format(s) => SnapshotError::Format(s.clone()),
            SnapshotError::VersionMismatch => SnapshotError::VersionMismatch,
            SnapshotError::InvalidData => SnapshotError::InvalidData,
        }
    }
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) => write!(f, "I/O error: {}", e),
            SnapshotError::Format(e) => write!(f, "Format error: {}", e),
            SnapshotError::VersionMismatch => write!(f, "Snapshot version mismatch"),
            SnapshotError::InvalidData => write!(f, "Invalid snapshot data"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<std::io::Error> for SnapshotError {
    fn from(e: std::io::Error) -> Self {
        SnapshotError::Io(e)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CpuSnapshot {
    pub state: CpuState,
    pub run_state: CpuRunState,
    pub pending_irq: Option<u32>,
    pub pending_fiq: Option<u32>,
    pub pending_serror: Option<u64>,
    pub instructions: u64,
    pub exclusive_address: Option<u64>,
    pub exclusive_size: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceSnapshot {
    pub id: DeviceId,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BusSnapshot {
    pub regions: Vec<BusRegionSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BusRegionSnapshot {
    pub base: u64,
    pub size: u64,
    pub device_name: String,
    pub device_id: DeviceId,
    pub device_data: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraceSnapshot {
    pub events: Vec<TraceEvent>,
    pub cycle: u64,
    pub verbosity: u8,
    pub inst_count: u64,
    pub branch_count: u64,
    pub exception_count: u64,
    pub mmio_count: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TlbSnapshot {
    pub entries: Vec<TlbEntrySnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TlbEntrySnapshot {
    pub va: u64,
    pub pa: u64,
    pub asid: u16,
    pub vmid: u16,
    pub unprivileged: bool,
    pub global: bool,
    pub attr: u8,
    pub writable: bool,
    pub executable: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemorySnapshot {
    pub regions: Vec<MemoryRegionSnapshot>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryRegionSnapshot {
    pub base: u64,
    pub size: u64,
    /// Dense contents for `Ram`/`Rom` backing devices (the whole region).
    pub data: Vec<u8>,
    /// Sparse `(page_index, 4 KiB)` payloads for page-backed `SparseRam`
    /// devices. Kept empty when `data` carries the region.
    pub sparse_pages: Vec<(u64, Vec<u8>)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MachineSnapshot {
    pub version: u32,
    pub timestamp: u64,
    /// State for every emulated core. The selected core is addressed through
    /// `active_cpu`; no second active-core copy is serialized.
    pub cpus: Vec<CpuSnapshot>,
    pub active_cpu: usize,
    pub scheduler: crate::scheduler::Scheduler,
    /// Device models without snapshot support. Such captures are useful for
    /// inspection, but cannot be restored as deterministic machine state.
    pub unsupported_devices: Vec<String>,
    pub dma_pending: Vec<(u32, crate::dma::DmaRequest)>,
    pub has_runtime_hooks: bool,
    pub has_iommu_ports: bool,
    pub has_fallback_device: bool,
    pub legacy_el3_identity_map: bool,
    pub bus: BusSnapshot,
    pub trace: TraceSnapshot,
    pub tlb: TlbSnapshot,
    pub memory: MemorySnapshot,
    pub devices: Vec<DeviceSnapshot>,
    pub metadata: HashMap<String, String>,
}

impl MachineSnapshot {
    /// Current on-disk format revision. Bumping it invalidates older capture
    /// files (see `load_from_file`'s `VersionMismatch` guard).
    pub const CURRENT_VERSION: u32 = 5;

    pub fn active_cpu(&self) -> &CpuSnapshot {
        &self.cpus[self.active_cpu]
    }

    pub fn new(
        machine: &mut crate::Machine,
        metadata: HashMap<String, String>,
    ) -> Result<Self, SnapshotError> {
        let trace_snap = TraceSnapshot {
            events: machine.trace.owned_events(),
            cycle: machine.trace.cycle(),
            verbosity: machine.trace.get_verbosity(),
            inst_count: machine.trace.get_inst_count(),
            branch_count: machine.trace.get_branch_count(),
            exception_count: machine.trace.get_exception_count(),
            mmio_count: machine.trace.get_mmio_count(),
            enabled: machine.trace.is_enabled(),
        };

        let mut bus_regions = Vec::new();
        let mut unsupported_devices = Vec::new();
        for region in &machine.bus.regions {
            let device_data = Self::serialize_core_device(&*region.device);
            if !Self::is_core_memory(&*region.device) {
                unsupported_devices.push(format!("{} at {:#x}", region.device.name(), region.base));
            }

            bus_regions.push(BusRegionSnapshot {
                base: region.base,
                size: region.size,
                device_name: region.device.name().to_string(),
                device_id: region.device.id(),
                device_data,
            });
        }

        let bus_snap = BusSnapshot {
            regions: bus_regions,
        };

        let mut device_snapshots = Vec::new();
        for region in &machine.bus.regions {
            let data = Self::serialize_core_device(&*region.device);
            if !data.is_empty() {
                device_snapshots.push(DeviceSnapshot {
                    id: region.device.id(),
                    name: region.device.name().to_string(),
                    data,
                });
            }
        }

        let mut tlb_entries = Vec::new();
        for entry in machine.tlb.get_entries() {
            tlb_entries.push(TlbEntrySnapshot {
                va: entry.va_page << 12,
                pa: entry.pa_page << 12,
                asid: entry.asid,
                vmid: entry.vmid,
                unprivileged: entry.unprivileged,
                global: entry.global,
                attr: entry.attr_index,
                writable: entry.writable,
                executable: entry.executable,
            });
        }
        let tlb_snap = TlbSnapshot {
            entries: tlb_entries,
        };

        let mut mem_regions = Vec::new();
        for region in &machine.bus.regions {
            let (data, sparse_pages) = Self::capture_memory_device(&*region.device);
            if data.is_empty() && sparse_pages.is_empty() {
                // Pure MMIO / register window — no guest-addressable storage.
                continue;
            }
            mem_regions.push(MemoryRegionSnapshot {
                base: region.base,
                size: region.size,
                data,
                sparse_pages,
            });
        }
        let memory_snap = MemorySnapshot {
            regions: mem_regions,
        };

        let cpus: Vec<_> = machine.cores.iter().map(Self::cpu_snapshot).collect();
        let active_cpu = machine.cores.active_index();

        Ok(MachineSnapshot {
            version: Self::CURRENT_VERSION,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            cpus,
            active_cpu,
            scheduler: machine.scheduler.clone(),
            unsupported_devices,
            dma_pending: machine.dma_pending.clone(),
            has_runtime_hooks: !machine.hooks.is_empty(),
            has_iommu_ports: !machine.dart_ports.is_empty(),
            has_fallback_device: machine.bus.fallback_device.is_some(),
            legacy_el3_identity_map: machine.legacy_el3_identity_map,
            bus: bus_snap,
            trace: trace_snap,
            tlb: tlb_snap,
            memory: memory_snap,
            devices: device_snapshots,
            metadata,
        })
    }

    fn cpu_snapshot(cpu: &crate::cpu::Cpu) -> CpuSnapshot {
        CpuSnapshot {
            state: cpu.state.clone(),
            run_state: cpu.run_state,
            pending_irq: cpu.pending_irq,
            pending_fiq: cpu.pending_fiq,
            pending_serror: cpu.pending_serror,
            instructions: cpu.instructions,
            exclusive_address: cpu.exclusive_address,
            exclusive_size: cpu.exclusive_size,
        }
    }

    fn is_core_memory(device: &dyn Device) -> bool {
        device.as_any().is::<Ram>()
            || device.as_any().is::<Rom>()
            || device.as_any().is::<SparseRam>()
    }

    /// Extract the dense contents + sparse page map of a RAM-like backing
    /// device for snapshotting. Non-memory devices return empty (nothing to
    /// save), which the capture loop skips.
    fn capture_memory_device(device: &dyn Device) -> (Vec<u8>, Vec<(u64, Vec<u8>)>) {
        if let Some(ram) = device.as_any().downcast_ref::<Ram>() {
            (ram.data.clone(), Vec::new())
        } else if let Some(rom) = device.as_any().downcast_ref::<Rom>() {
            (rom.data.clone(), Vec::new())
        } else if let Some(sp) = device.as_any().downcast_ref::<SparseRam>() {
            let mut pages = Vec::with_capacity(sp.get_pages().len());
            for (&page, bytes) in sp.get_pages() {
                let mut copy = [0u8; 4096];
                copy.copy_from_slice(bytes.as_ref());
                pages.push((page, copy.to_vec()));
            }
            (Vec::new(), pages)
        } else {
            (Vec::new(), Vec::new())
        }
    }

    fn serialize_core_device(device: &dyn Device) -> Vec<u8> {
        if let Some(any) = device.as_any().downcast_ref::<Ram>() {
            any.data.clone()
        } else if let Some(any) = device.as_any().downcast_ref::<Rom>() {
            any.data.clone()
        } else if let Some(any) = device.as_any().downcast_ref::<SparseRam>() {
            let mut data = Vec::new();
            for page in any.get_pages().values() {
                data.extend_from_slice(page.as_ref());
            }
            data
        } else {
            Vec::new()
        }
    }

    pub fn inspect(&self) -> String {
        let cpu = self.active_cpu();
        let mut out = String::new();
        out.push_str(&format!("=== MACHINE SNAPSHOT v{} ===\n", self.version));
        out.push_str(&format!("Timestamp: {}\n", self.timestamp));
        out.push_str(&format!("Metadata: {:?}\n", self.metadata));
        out.push_str("\n--- CPU ---\n");
        out.push_str(&format!("PC: {:#018x}\n", cpu.state.pc));
        out.push_str(&format!("SP: {:#018x}\n", cpu.state.sp));
        out.push_str(&format!("EL: {:?}\n", cpu.state.el));
        out.push_str(&format!("PSTATE: {:#018x}\n", cpu.state.pstate));
        out.push_str(&format!("Instructions: {}\n", cpu.instructions));
        out.push_str(&format!("Run State: {:?}\n", cpu.run_state));
        out.push_str(&format!(
            "Active core: {} / {} cores\n",
            self.active_cpu,
            self.cpus.len()
        ));
        out.push_str(&format!("Scheduler cycle: {}\n", self.scheduler.cycle()));
        out.push_str("Registers:\n");
        for i in 0..31 {
            out.push_str(&format!("  X{:2}: {:#018x}\n", i, cpu.state.x[i]));
        }
        out.push_str(&format!(
            "\n--- TLB ({} entries) ---\n",
            self.tlb.entries.len()
        ));
        for entry in &self.tlb.entries {
            out.push_str(&format!(
                "  VA={:#018x} PA={:#018x} ASID={} ATTR={} W={} X={}\n",
                entry.va, entry.pa, entry.asid, entry.attr, entry.writable, entry.executable
            ));
        }
        out.push_str(&format!(
            "\n--- BUS REGIONS ({}) ---\n",
            self.bus.regions.len()
        ));
        for region in &self.bus.regions {
            out.push_str(&format!(
                "  {} [{:#018x}-{:#018x}] ID={} Size={} bytes\n",
                region.device_name,
                region.base,
                region.base + region.size,
                region.device_id,
                region.device_data.len()
            ));
        }
        out.push_str(&format!("\n--- DEVICES ({}) ---\n", self.devices.len()));
        for dev in &self.devices {
            out.push_str(&format!(
                "  {} (ID={}) {} bytes\n",
                dev.name,
                dev.id,
                dev.data.len()
            ));
        }
        if !self.unsupported_devices.is_empty() {
            out.push_str("\n--- UNSNAPSHOTTED DEVICE STATE ---\n");
            for device in &self.unsupported_devices {
                out.push_str(&format!("  {device}\n"));
            }
            out.push_str(
                "Restore is disabled for this capture until these devices support snapshots.\n",
            );
        }
        out.push_str("\n--- TRACE ---\n");
        out.push_str(&format!("  Cycle: {}\n", self.trace.cycle));
        out.push_str(&format!("  Events: {}\n", self.trace.events.len()));
        out.push_str(&format!(
            "\n--- MEMORY REGIONS ({}) ---\n",
            self.memory.regions.len()
        ));
        for region in &self.memory.regions {
            out.push_str(&format!(
                "  [{:#018x}-{:#018x}] {} bytes\n",
                region.base,
                region.base + region.size,
                region.data.len()
            ));
        }
        out
    }

    pub fn diff(&self, other: &MachineSnapshot) -> String {
        let cpu = self.active_cpu();
        let other_cpu = other.active_cpu();
        let mut out = String::new();
        out.push_str("=== SNAPSHOT DIFF ===\n");

        if cpu.state.pc != other_cpu.state.pc {
            out.push_str(&format!(
                "PC: {:#x} -> {:#x}\n",
                cpu.state.pc, other_cpu.state.pc
            ));
        }
        if cpu.state.sp != other_cpu.state.sp {
            out.push_str(&format!(
                "SP: {:#x} -> {:#x}\n",
                cpu.state.sp, other_cpu.state.sp
            ));
        }
        if cpu.state.pstate != other_cpu.state.pstate {
            out.push_str(&format!(
                "PSTATE: {:#x} -> {:#x}\n",
                cpu.state.pstate, other_cpu.state.pstate
            ));
        }

        for i in 0..31 {
            if cpu.state.x[i] != other_cpu.state.x[i] {
                out.push_str(&format!(
                    "X{}: {:#x} -> {:#x}\n",
                    i, cpu.state.x[i], other_cpu.state.x[i]
                ));
            }
        }

        out.push_str(&format!(
            "\nTLB entries: {} -> {}\n",
            self.tlb.entries.len(),
            other.tlb.entries.len()
        ));
        out.push_str(&format!(
            "Bus regions: {} -> {}\n",
            self.bus.regions.len(),
            other.bus.regions.len()
        ));
        out.push_str(&format!(
            "Trace events: {} -> {}\n",
            self.trace.events.len(),
            other.trace.events.len()
        ));

        out
    }
}

impl Default for MachineSnapshot {
    fn default() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            timestamp: 0,
            cpus: vec![CpuSnapshot {
                state: CpuState::default(),
                run_state: CpuRunState::Running,
                pending_irq: None,
                pending_fiq: None,
                pending_serror: None,
                instructions: 0,
                exclusive_address: None,
                exclusive_size: 0,
            }],
            active_cpu: 0,
            scheduler: crate::scheduler::Scheduler::default(),
            unsupported_devices: Vec::new(),
            dma_pending: Vec::new(),
            has_runtime_hooks: false,
            has_iommu_ports: false,
            has_fallback_device: false,
            legacy_el3_identity_map: false,
            bus: BusSnapshot {
                regions: Vec::new(),
            },
            trace: TraceSnapshot {
                events: Vec::new(),
                cycle: 0,
                verbosity: 3,
                inst_count: 0,
                branch_count: 0,
                exception_count: 0,
                mmio_count: 0,
                enabled: false,
            },
            tlb: TlbSnapshot {
                entries: Vec::new(),
            },
            memory: MemorySnapshot {
                regions: Vec::new(),
            },
            devices: Vec::new(),
            metadata: HashMap::new(),
        }
    }
}

pub fn capture(machine: &mut crate::Machine) -> Result<MachineSnapshot, SnapshotError> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "rust_version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    metadata.insert(
        "capture_time".to_string(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string(),
    );

    MachineSnapshot::new(machine, metadata)
}

impl MachineSnapshot {
    pub fn save_to_file(&self, path: &str) -> Result<(), SnapshotError> {
        let data = bincode::serialize(self)
            .map_err(|e| SnapshotError::Format(format!("bincode serialize: {e}")))?;
        std::fs::write(path, data).map_err(SnapshotError::Io)
    }

    pub fn load_from_file(path: &str) -> Result<Self, SnapshotError> {
        let data = std::fs::read(path).map_err(SnapshotError::Io)?;
        let snap: Self = bincode::deserialize(&data)
            .map_err(|e| SnapshotError::Format(format!("bincode deserialize: {e}")))?;
        if snap.version != Self::CURRENT_VERSION {
            return Err(SnapshotError::VersionMismatch);
        }
        if snap.cpus.is_empty() || snap.active_cpu >= snap.cpus.len() {
            return Err(SnapshotError::InvalidData);
        }
        Ok(snap)
    }

    pub fn restore_into(&self, machine: &mut crate::Machine) -> Result<(), SnapshotError> {
        if !self.unsupported_devices.is_empty() {
            return Err(SnapshotError::Format(format!(
                "snapshot omits runtime state for devices: {}",
                self.unsupported_devices.join(", ")
            )));
        }
        if self.has_runtime_hooks || self.has_iommu_ports || self.has_fallback_device {
            return Err(SnapshotError::Format(format!(
                "snapshot cannot restore runtime hooks, IOMMU ports, or a fallback device (hooks={}, ports={}, fallback={})",
                self.has_runtime_hooks, self.has_iommu_ports, self.has_fallback_device
            )));
        }
        if self.cpus.len() != machine.cores.len() || self.active_cpu >= self.cpus.len() {
            return Err(SnapshotError::Format(format!(
                "CPU topology mismatch: snapshot has {} cores, machine has {}",
                self.cpus.len(),
                machine.cores.len()
            )));
        }
        if self.bus.regions.len() != machine.bus.regions.len()
            || self
                .bus
                .regions
                .iter()
                .zip(&machine.bus.regions)
                .any(|(saved, current)| {
                    saved.base != current.base
                        || saved.size != current.size
                        || saved.device_name != current.device.name()
                })
        {
            return Err(SnapshotError::Format(
                "bus topology does not match snapshot".into(),
            ));
        }
        for (saved, cpu) in self.cpus.iter().zip(machine.cores.iter_mut()) {
            Self::restore_cpu(saved, cpu);
        }
        if !machine.cores.select(self.active_cpu) {
            return Err(SnapshotError::Format(format!(
                "snapshot active CPU {} is outside the machine topology",
                self.active_cpu
            )));
        }
        machine.scheduler = self.scheduler.clone();
        machine.legacy_el3_identity_map = self.legacy_el3_identity_map;
        machine.dma_pending = self.dma_pending.clone();
        machine.trace.restore_captured_state(
            self.trace.events.clone(),
            self.trace.cycle,
            self.trace.enabled,
            self.trace.verbosity,
            (
                self.trace.inst_count,
                self.trace.branch_count,
                self.trace.exception_count,
                self.trace.mmio_count,
            ),
        );
        machine.current_instruction_pc = None;
        machine.tlb.invalidate_all();
        for entry in &self.tlb.entries {
            let translation = crate::mmu::Translation {
                pa: entry.pa,
                attr_index: entry.attr,
                writable: entry.writable,
                executable: entry.executable,
                global: entry.global,
            };
            machine.tlb.insert(
                entry.va,
                entry.asid,
                entry.vmid,
                entry.unprivileged,
                translation,
            );
        }
        for region in &self.memory.regions {
            Self::restore_memory_region(machine, region)?;
        }
        Ok(())
    }

    fn restore_cpu(saved: &CpuSnapshot, cpu: &mut crate::cpu::Cpu) {
        cpu.state = saved.state.clone();
        cpu.run_state = saved.run_state;
        cpu.pending_irq = saved.pending_irq;
        cpu.pending_fiq = saved.pending_fiq;
        cpu.pending_serror = saved.pending_serror;
        cpu.instructions = saved.instructions;
        cpu.exclusive_address = saved.exclusive_address;
        cpu.exclusive_size = saved.exclusive_size;
    }

    /// Write a captured physical region straight into the matching RAM backing
    /// device, bypassing the guest MMU and the device bus. Pure MMIO regions
    /// have nothing to restore.
    fn restore_memory_region(
        machine: &mut crate::Machine,
        region: &MemoryRegionSnapshot,
    ) -> Result<(), SnapshotError> {
        for bus_region in &mut machine.bus.regions {
            if bus_region.base != region.base {
                continue;
            }
            if let Some(ram) = bus_region.device.as_any_mut().downcast_mut::<Ram>() {
                if region.data.len() > ram.data.len() {
                    return Err(SnapshotError::InvalidData);
                }
                ram.data[..region.data.len()].copy_from_slice(&region.data);
                return Ok(());
            }
            if bus_region.device.as_any().downcast_ref::<Rom>().is_some() {
                // ROM is guest-immutable; the snapshot merely echoes the image.
                return Ok(());
            }
            if let Some(sp) = bus_region.device.as_any_mut().downcast_mut::<SparseRam>() {
                if !region.data.is_empty() && !sp.load(0, &region.data) {
                    return Err(SnapshotError::InvalidData);
                }
                for (page, bytes) in &region.sparse_pages {
                    if bytes.len() != 4096 || !sp.load(page << 12, bytes) {
                        return Err(SnapshotError::InvalidData);
                    }
                }
                return Ok(());
            }
            // Register-window / MMIO device: no guest storage to restore.
            return Ok(());
        }
        Ok(())
    }
}
