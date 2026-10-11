pub mod bus;
pub mod cpu;
pub mod device;
pub mod dma;
pub mod exception;
pub mod hostmem;
pub mod machine;
pub mod memory;
pub mod mmu;
pub mod scheduler;
pub mod snapshot;
pub mod sysreg;
pub mod tlb;
pub mod trace;
pub use bus::{Bus, BusError, BusRegion, MemTransaction, SecurityState, TransactionKind};
pub use cpu::{Aarch32State, Cpu, CpuException, CpuRunState, CpuState, ExceptionLevel};
pub use device::{Device, DeviceEvent, DeviceId, DeviceResult};
pub use dma::{DartPort, DmaAccess, DmaFault, DmaIommu, DmaRequest, DmaResult, DmaTableReader};
pub use exception::{saved_pstate, take_irq_el1, take_sync, take_sync_el1, ExceptionClass};
pub use hostmem::{MappableRegion, MappingProblem, RamBacking, MIN_MAPPABLE_BYTES};
pub use machine::{CoreSet, Machine, MachineConfig};
pub use memory::{AddressSpace, Ram, Rom, SparseRam};
pub use mmu::{Mmu, MmuFault, TableWalkFaultKind, Translation};
pub use scheduler::Scheduler;
pub use snapshot::{capture, MachineSnapshot, SnapshotError};
pub use tlb::{Tlb, TlbEntry};
pub use trace::{TraceEngine, TraceEvent, TraceKind, Verbosity};

/// Find a checked-in AP SecureROM by silicon family and iBoot build.
///
/// The corpus has been through a rename and is now **half** renamed, which is what makes
/// a hard-coded filename the wrong instrument:
///
/// ```text
/// SecureROM/securerom/`SecureROM_t7000si_iBoot-1992.0.0.1.19`   (renamed)
/// SecureROM/securerom/`SecureROM for s5l8960xsi, iBoot-1585.4`  (not renamed)
/// ```
///
/// So this accepts both spellings and any subdirectory, and reports every candidate it
/// rejected when it finds nothing. That last part matters: 53 call sites across this
/// workspace carried the old spelling, and five `iphem-soc` regressions were failing
/// with `No such file or directory` on a file that had simply been renamed -- an error
/// that says nothing about which of the two spellings was expected.
///
/// Returns the **last** match in sorted order, which is the highest iBoot build for the
/// equal-width build numbers Apple uses in this corpus, matching what the restore
/// launchers resolve. A family with no build given matches on the family alone.
pub fn corpus_rom_path(family: &str, build: &str) -> std::path::PathBuf {
    // `IPHEM_SECUREROM_DIR` overrides the corpus root, as it did when each call site
    // spelled the path out. Reading it here rather than in the call sites is what made
    // those locals dead: three examples kept a `dir` binding for a path they no longer
    // built, which is exactly the half-migration this whole change exists to remove.
    let root = std::env::var("IPHEM_SECUREROM_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../SecureROM")));
    let root = root.to_string_lossy().into_owned();
    let mut names: Vec<String> = Vec::new();
    for dir in [root.clone(), format!("{root}/securerom"), format!("{root}/seprom")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // A SEPROM, or the shared boot-chain image four names alias to one file.
            if name.starts_with("AppleSEPROM-")
                || matches!(name.as_str(), "iBoot" | "LLB" | "iBEC" | "iBSS")
                || !name.contains("iBoot")
            {
                continue;
            }
            let has_family = name.contains(family);
            let has_build = build.is_empty() || name.contains(&format!("iBoot-{build}"));
            if has_family & has_build {
                names.push(format!("{dir}/{name}"));
            }
        }
    }
    names.sort();
    match names.last() {
        Some(path) => std::path::PathBuf::from(path),
        None => panic!(
            "no checked-in SecureROM for family {family:?} build {build:?} under {root}. \
             Looked for a name containing both. Candidates in the corpus: {}",
            std::fs::read_dir(&root)
                .map(|entries| entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(", "))
                .unwrap_or_else(|_| "<the corpus directory is unreadable>".to_string())
        ),
    }
}

