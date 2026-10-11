//! Core DMA abstraction: the request/result types, the typed fault set, and
//! the `DmaIommu` trait that translates device-originated requests without
//! making the core depend on any device model crate.
//!
//! DMA is deliberately distinct from CPU address translation: an IOMMU
//! (apple DART on T8020) translates an I/O virtual address using its own
//! page tables, never the CPU Stage-1/Stage-2 MMU. The core exposes the
//! translator through [`DmaIommu`]; `Machine` performs the translation and
//! physical-bus access while retaining the originating master and SID for
//! diagnostics. A failed translation returns a typed [`DmaFault`] and raises
//! the interrupt mapped to that SID — it must never silently return zero or
//! fall back to a host buffer.

/// Direction of a DMA access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DmaAccess {
    Read,
    Write,
}

/// A single device-originated DMA access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DmaRequest {
    /// Emulated initiator identity (retained for diagnostics only).
    pub master: u64,
    /// IOMMU stream ID the master is bound to by the SoC.
    pub sid: u32,
    /// I/O virtual address to translate.
    pub iova: u64,
    /// Access direction.
    pub access: DmaAccess,
    /// Requested transfer size in bytes.
    pub size: u64,
}

impl DmaRequest {
    pub const fn new(master: u64, sid: u32, iova: u64, access: DmaAccess, size: u64) -> Self {
        Self {
            master,
            sid,
            iova,
            access,
            size,
        }
    }
}

/// Typed IOMMU translation fault. The variants mirror the T8020 DART1 walker
/// faults so the core can describe any failure without naming a device crate;
/// `UnboundSid` is produced by the machine when no IOMMU is registered for a
/// stream ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaFault {
    /// IOVA is outside the configured input-address width (or the width is
    /// not configured).
    AddressWidth,
    /// No valid root table is programmed for the stream.
    NoRoot,
    /// An intermediate level entry is zero while the walk is still active.
    MissingIntermediate(u32),
    /// The leaf entry is zero (no mapping).
    MissingLeaf,
    /// A present entry has the valid bit clear, or an intermediate entry
    /// points to address zero.
    PresentInvalid,
    /// Read access denied.
    PermissionRead,
    /// Write access denied.
    PermissionWrite,
    /// The IOVA subpage is outside the mapped subpage range.
    SubpageDisallowed { start: u32, end: u32, subpage: u32 },
    /// A page-table entry could not be fetched from physical memory.
    TableReadFailed(u64),
    /// A payload access at a translated physical address aborted on the bus.
    /// The translation succeeded but the target read/write failed; a
    /// multi-chunk write transfer may already have partially moved data.
    TargetAborted(u64),
    /// No IOMMU is registered for the request's SID.
    UnboundSid(u32),
    /// The IOMMU stream is globally disabled or has neither translation nor
    /// an architecturally supported bypass mode configured.
    StreamBlocked(u32),
}

/// Outcome of an IOMMU translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaResult {
    /// The IOVA mapped to this physical address.
    Translated(u64),
    /// The IOMMU bypassed translation; the caller uses the IOVA as-is.
    Bypass(u64),
    /// Translation failed with a typed fault.
    Fault(DmaFault),
}

/// Physical-memory table reader used by an IOMMU translation. Returns `None`
/// when the address cannot be read from the machine's physical bus.
pub type DmaTableReader<'a> = &'a mut dyn FnMut(u64) -> Option<u64>;

/// Translates and invalidates device-originated DMA requests.
///
/// Implemented per IOMMU instance (an apple DART on T8020). The `read`
/// closure resolves page-table entries from the physical bus so a
/// translation never queries the CPU MMU or derives host pointers.
pub trait DmaIommu {
    /// Translate `request.iova` through this IOMMU's page tables.
    fn translate(&self, request: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult;

    /// Invalidate cached translations for a stream/address range.
    fn invalidate(&mut self, sid: u32, iova: u64, size: u64);
}

/// Association between a stream ID, its IOMMU, and the interrupt to raise
/// when a translation fails.
pub struct DartPort {
    pub sid: u32,
    pub irq: u32,
    pub iommu: Box<dyn DmaIommu>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{Bus, BusRegion, MemTransaction, SecurityState, TransactionKind};
    use std::cell::RefCell;

    /// Identity IOMMU: translates IOVA → PA (IOVA identity-mapped).
    struct IdentityIommu {
        requests: RefCell<Vec<DmaRequest>>,
    }

    impl DmaIommu for IdentityIommu {
        fn translate(&self, request: &DmaRequest, _read: DmaTableReader<'_>) -> DmaResult {
            self.requests.borrow_mut().push(*request);
            DmaResult::Translated(request.iova)
        }

        fn invalidate(&mut self, _sid: u32, _iova: u64, _size: u64) {}
    }

    /// IOMMU that fails a specific IOVA with an explicit fault.
    struct FaultingIommu {
        fail_iova: u64,
        fault: DmaFault,
    }

    impl DmaIommu for FaultingIommu {
        fn translate(&self, request: &DmaRequest, _read: DmaTableReader<'_>) -> DmaResult {
            if request.iova == self.fail_iova {
                DmaResult::Fault(self.fault)
            } else {
                DmaResult::Translated(request.iova)
            }
        }
        fn invalidate(&mut self, _sid: u32, _iova: u64, _size: u64) {}
    }

    #[test]
    fn dma_request_carry_metadata() {
        let req = DmaRequest::new(0x1_0000, 0x12, 0x4000_0000, DmaAccess::Write, 64);
        assert_eq!(req.master, 0x1_0000);
        assert_eq!(req.sid, 0x12);
        assert_eq!(req.iova, 0x4000_0000);
        assert_eq!(req.access, DmaAccess::Write);
        assert_eq!(req.size, 64);
        assert_ne!(DmaAccess::Read, DmaAccess::Write);
    }

    #[test]
    fn dma_fault_is_distinct_from_zero_or_bypass() {
        let fault = DmaFault::PermissionRead;
        assert_eq!(
            DmaResult::Fault(fault),
            DmaResult::Fault(DmaFault::PermissionRead)
        );
        assert_ne!(DmaResult::Fault(fault), DmaResult::Translated(0));
        assert_ne!(DmaResult::Fault(fault), DmaResult::Bypass(0));
    }

    #[test]
    fn translate_retains_master_and_sid_through_call() {
        let iommu = IdentityIommu {
            requests: RefCell::new(Vec::new()),
        };
        let req = DmaRequest::new(0x5_0000, 0x34, 0x2000_0000, DmaAccess::Read, 0x1000);
        let mut read = |_a| None;
        let result = iommu.translate(&req, &mut read);
        assert_eq!(result, DmaResult::Translated(0x2000_0000));
        let logged = iommu.requests.borrow();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].master, 0x5_0000);
        assert_eq!(logged[0].sid, 0x34);
    }

    #[test]
    fn translate_with_bypass_result() {
        struct BypassIommu;
        impl DmaIommu for BypassIommu {
            fn translate(&self, request: &DmaRequest, _read: DmaTableReader<'_>) -> DmaResult {
                DmaResult::Bypass(request.iova)
            }
            fn invalidate(&mut self, _: u32, _: u64, _: u64) {}
        }
        let req = DmaRequest::new(0x9, 0x1, 0x1_0000, DmaAccess::Read, 4);
        let iommu = BypassIommu;
        let mut read = |_a| None;
        assert_eq!(
            iommu.translate(&req, &mut read),
            DmaResult::Bypass(0x1_0000)
        );
    }

    #[test]
    fn faulting_iommu_returns_typed_fault() {
        let iommu = FaultingIommu {
            fail_iova: 0,
            fault: DmaFault::AddressWidth,
        };
        let req = DmaRequest::new(1, 0x22, 0, DmaAccess::Write, 8);
        let mut read = |_a| None;
        let result = iommu.translate(&req, &mut read);
        assert_eq!(result, DmaResult::Fault(DmaFault::AddressWidth));
    }

    #[test]
    fn translate_resolves_table_entries_via_reader_not_cpu_mmu() {
        // Page table leaf at physical 0x1000_0000 stores PA 0x4000_0000.
        // The reader closes over a physical table snapshot — never the CPU MMU.
        let table: [(u64, u64); 1] = [(0x1000_0000, 0x4000_0000)];

        struct TableIommu {
            root: u64,
        }
        impl DmaIommu for TableIommu {
            fn translate(&self, _req: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
                let leaf = match read(self.root) {
                    Some(v) => v,
                    None => return DmaResult::Fault(DmaFault::TableReadFailed(self.root)),
                };
                if leaf == 0 {
                    return DmaResult::Fault(DmaFault::MissingLeaf);
                }
                DmaResult::Translated(leaf)
            }
            fn invalidate(&mut self, _: u32, _: u64, _: u64) {}
        }

        let iommu = TableIommu { root: 0x1000_0000 };
        let req = DmaRequest::new(0xA, 0x7, 0x0, DmaAccess::Read, 0x1000);
        let mut read = |addr| table.iter().find(|(a, _)| *a == addr).map(|(_, v)| *v);
        assert_eq!(
            iommu.translate(&req, &mut read),
            DmaResult::Translated(0x4000_0000)
        );
    }

    #[test]
    fn missing_table_read_returns_table_read_fault() {
        struct TableIommu {
            root: u64,
        }
        impl DmaIommu for TableIommu {
            fn translate(&self, _req: &DmaRequest, read: DmaTableReader<'_>) -> DmaResult {
                match read(self.root) {
                    Some(0) => DmaResult::Fault(DmaFault::MissingLeaf),
                    None => DmaResult::Fault(DmaFault::TableReadFailed(self.root)),
                    Some(v) => DmaResult::Translated(v),
                }
            }
            fn invalidate(&mut self, _: u32, _: u64, _: u64) {}
        }
        let iommu = TableIommu { root: 0x2000_0000 };
        let req = DmaRequest::new(1, 5, 0, DmaAccess::Read, 8);
        let mut read = |_addr: u64| None;
        assert_eq!(
            iommu.translate(&req, &mut read),
            DmaResult::Fault(DmaFault::TableReadFailed(0x2000_0000))
        );
    }

    #[test]
    fn bus_backed_dma_table_read_is_physical() {
        // DMA table entries are fetched from physical addresses on the bus.
        // The CPU's MMU configuration is irrelevant. Set up a tiny bus with
        // 8 bytes of data at physical 0x1000_0000 and prove the closure can
        // read the value without going through the CPU.
        let mut bus = Bus::new();
        let mut data = vec![0u8; 0x1000];
        data[0] = 0x42;
        bus.map(BusRegion {
            base: 0x1000_0000,
            size: 0x1000,
            device: Box::new(crate::Rom::new(0x1000_0000, data)),
        })
        .expect("bus map");
        let mut trace = crate::trace::TraceEngine::new(false);

        let mut reader = |addr: u64| -> Option<u64> {
            let tx = MemTransaction {
                address: addr,
                size: 8,
                kind: TransactionKind::Read,
                security: SecurityState::NonSecure,
                source: None,
            };
            bus.read(tx, &mut trace, 0, 0, 0).ok()
        };

        // Reading the physical address through the reader succeeds and
        // yields the value written — independent of any CPU MMU state.
        let val = reader(0x1000_0000).expect("bus-backed table read");
        // Little-endian: 0x42 in byte 0, zeros elsewhere.
        assert_eq!(val, 0x42);
    }

    #[test]
    fn unbound_sid_fault_is_distinct_from_dart_faults() {
        assert_ne!(
            DmaResult::Fault(DmaFault::UnboundSid(99)),
            DmaResult::Fault(DmaFault::NoRoot)
        );
        assert_ne!(
            DmaResult::Fault(DmaFault::UnboundSid(99)),
            DmaResult::Fault(DmaFault::AddressWidth)
        );
    }
}
