//! Host-visible guest memory: what a host hypervisor can actually map.
//!
//! # Why this exists
//!
//! A hypervisor-backed execution backend cannot reach guest RAM through the
//! interpreter's access path. The hypervisor needs to map the *same bytes* the
//! guest bus serves, so it needs a stable host address per guest region. That is
//! not available for every RAM device in this tree, and the difference is not a
//! detail:
//!
//! * [`Ram`] keeps its bytes in one `Vec<u8>`, so the whole region is one
//!   contiguous host allocation and can be handed to a hypervisor in one call.
//! * [`SparseRam`] keeps 4 KiB pages as individually heap-allocated
//!   `Box<[u8; 4096]>` values in a `BTreeMap`. Those pages are *not* adjacent in
//!   host memory, so no single mapping call can describe the region.
//!
//! The size limit below is measured, not assumed. On the macOS/arm64 host,
//! `Hypervisor.framework`'s `hv_vm_map` accepts a 16 KiB and a 64 KiB mapping
//! and refuses 4 KiB and 8 KiB with `HV_BAD_ARGUMENT`:
//!
//! ```text
//! size 0x1000 ipa 0x0000000000: HV_BAD_ARGUMENT
//! size 0x2000 ipa 0x0000000000: HV_BAD_ARGUMENT
//! size 0x4000 ipa 0x0000000000: accepted
//! size 0x10000 ipa 0x0000000000: accepted
//! ```
//!
//! That is the whole reason `SparseRam` cannot back a hypervisor guest: its
//! unit of allocation is 4 KiB and the framework will not map 4 KiB. This was
//! first recorded as `HV_NO_DEVICE`, which was an artifact of an earlier bug in
//! the probe's foreign-function interface; the size rule survived the fix but
//! the error code did not.
//!
//! **Current constraint:** the A8/T7000 platform configuration is
//! built on `SparseRam`, so a hypervisor backend cannot use it as-is. A hypervisor backend therefore rejects that profile unless the caller supplies
//! contiguous backing for the regions it maps. Nothing here changes the
//! interpreter path or any existing device wiring; this module only *reports*
//! what a backend would find, so the gap is visible in a test rather than in a
//! comment.
//!
//! # Safety
//!
//! [`MappableRegion::ptr`] hands out a mutable pointer into a live bus region.
//! It is sound only while the [`Bus`] is alive and unmapped, and only because
//! the hypervisor writes guest stores *into guest memory*, which is exactly what
//! the interpreter does through `Device::write`. Callers must not use it to
//! reach the region's internals for any other purpose, and must not assume the
//! region's contents are still the interpreter's view after the guest has run:
//! the hypervisor mutates the same bytes the interpreter would.

use std::ptr::NonNull;

use crate::bus::Bus;
use crate::memory::{Ram, SparseRam};

/// The smallest guest-memory mapping the measured host framework accepts.
///
/// Measured on macOS 26.2 / arm64: 4 KiB and 8 KiB are refused with
/// `HV_BAD_ARGUMENT`, 16 KiB and 64 KiB are accepted. The framework makes no
/// promise about other hosts, so this is a floor this tree has verified, not a
/// portable constant -- [`mapping_problem`] is what a backend should consult.
pub const MIN_MAPPABLE_BYTES: u64 = 0x4000;

/// Why a region cannot be handed to a hypervisor as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingProblem {
    /// The region's bytes are split across non-adjacent host allocations.
    PagedBacking,
    /// Smaller than [`MIN_MAPPABLE_BYTES`].
    TooSmall { size: u64 },
    /// Not a whole number of [`MIN_MAPPABLE_BYTES`].
    MisalignedSize { size: u64 },
    /// The guest physical base is not granule-aligned.
    MisalignedBase { base: u64 },
    /// The host allocation is not granule-aligned.
    MisalignedHost { alignment: usize },
}

impl core::fmt::Display for MappingProblem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PagedBacking => write!(
                f,
                "backed by individually allocated 4 KiB pages, so no single mapping can \
                 describe it"
            ),
            Self::TooSmall { size } => {
                write!(f, "{size:#x} bytes is below the {MIN_MAPPABLE_BYTES:#x} minimum")
            }
            Self::MisalignedSize { size } => {
                write!(f, "{size:#x} bytes is not a multiple of {MIN_MAPPABLE_BYTES:#x}")
            }
            Self::MisalignedBase { base } => {
                write!(f, "guest base {base:#x} is not {MIN_MAPPABLE_BYTES:#x}-aligned")
            }
            Self::MisalignedHost { alignment } => write!(
                f,
                "host allocation is {alignment:#x}-aligned, below the required \
                 {MIN_MAPPABLE_BYTES:#x}"
            ),
        }
    }
}

/// A guest region whose host bytes a hypervisor can map in one call.
#[derive(Debug, Clone, Copy)]
pub struct MappableRegion {
    /// Guest physical address the region answers at.
    pub base: u64,
    /// Region size in bytes.
    pub size: u64,
    /// Host address of the region's first byte, granule-aligned.
    pub ptr: NonNull<u8>,
    /// Number of bytes addressable from `ptr`.
    pub len: usize,
}

impl MappableRegion {
    /// Re-checks the mapping preconditions against a possibly different host
    /// address, for a backend that had to copy or re-align the bytes.
    pub fn mapping_problem(&self) -> Option<MappingProblem> {
        mapping_problem(self.ptr.as_ptr(), self.len, self.base)
    }
}

/// A bus region that looks like RAM, and whether a hypervisor can map it.
#[derive(Debug, Clone, Copy)]
pub struct RamBacking {
    pub base: u64,
    pub size: u64,
    /// `None` when the region is contiguous and satisfies every precondition.
    pub problem: Option<MappingProblem>,
}

impl RamBacking {
    pub fn is_mappable(&self) -> bool {
        self.problem.is_none()
    }
}

/// Decides whether `(ptr, len)` at guest `base` can be mapped in one call.
///
/// Split out from the bus walk so the preconditions are testable with synthetic
/// pointers: the interesting cases (an under-aligned host address, a size that is
/// not a granule multiple) cannot be produced reliably from a real `Vec<u8>`,
/// whose allocator alignment is not under the caller's control.
pub fn mapping_problem(ptr: *const u8, len: usize, base: u64) -> Option<MappingProblem> {
    let size = len as u64;
    if size < MIN_MAPPABLE_BYTES {
        return Some(MappingProblem::TooSmall { size });
    }
    if !size.is_multiple_of(MIN_MAPPABLE_BYTES) {
        return Some(MappingProblem::MisalignedSize { size });
    }
    if !base.is_multiple_of(MIN_MAPPABLE_BYTES) {
        return Some(MappingProblem::MisalignedBase { base });
    }
    // The largest power of two dividing `v` is `v & -v`. The obvious-looking
    // `v & !(-v)` is a different value -- it keeps the *high* bits -- and for a
    // host address such as 0x0000_00b7_9028_0001 it yields 0x0000_00b7_9028_0000,
    // which is far larger than the required granule. That made this check pass
    // for every misaligned pointer on a platform whose user addresses have high
    // bits set, i.e. it failed open on exactly the case it exists to catch.
    let bits = ptr as usize;
    // Stable Rust equivalent of the lowest-set-bit operation: v & -v.
    // `wrapping_neg` is intentional for unsigned two's-complement arithmetic
    // and keeps VectorEmu compatible with the declared Rust 1.90 MSRV.
    let alignment = bits & bits.wrapping_neg();
    if alignment < MIN_MAPPABLE_BYTES as usize {
        return Some(MappingProblem::MisalignedHost { alignment });
    }
    None
}

impl Bus {
    /// Every region whose backing device is RAM, with the reason each can or
    /// cannot be mapped by a host hypervisor.
    ///
    /// This is a report, not a mutation: it changes no device wiring and no
    /// access path, so the interpreter's behaviour is untouched.
    pub fn ram_backing_report(&self) -> Vec<RamBacking> {
        let mut out = Vec::new();
        for region in &self.regions {
            if let Some(ram) = region.device.as_any().downcast_ref::<Ram>() {
                let problem = mapping_problem(
                    ram.data.as_ptr(),
                    ram.data.len(),
                    region.base,
                );
                out.push(RamBacking {
                    base: region.base,
                    size: region.size,
                    problem,
                });
            } else if region.device.as_any().downcast_ref::<SparseRam>().is_some() {
                out.push(RamBacking {
                    base: region.base,
                    size: region.size,
                    problem: Some(MappingProblem::PagedBacking),
                });
            }
        }
        out
    }

    /// The subset of [`Self::ram_backing_report`] a hypervisor can map directly.
    pub fn mappable_ram(&self) -> Vec<MappableRegion> {
        let mut out = Vec::new();
        for region in &self.regions {
            let Some(ram) = region.device.as_any().downcast_ref::<Ram>() else {
                continue;
            };
            // SAFETY: `ram.data` is a live allocation owned by the bus region
            // and is not reallocated while the borrow holds. The pointer is only
            // reported, never written through here.
            let ptr = NonNull::new(ram.data.as_ptr() as *mut u8).unwrap_or(NonNull::dangling());
            if mapping_problem(ptr.as_ptr(), ram.data.len(), region.base).is_none() {
                out.push(MappableRegion {
                    base: region.base,
                    size: region.size,
                    ptr,
                    len: ram.data.len(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::BusRegion;

    fn aligned_block(len: usize) -> (Vec<u8>, *const u8) {
        // Over-allocate and align by hand so the tests control the host address,
        // which `Vec<u8>`'s allocator does not.
        let raw = vec![0u8; len + MIN_MAPPABLE_BYTES as usize];
        let base = raw.as_ptr() as usize;
        let aligned = (base + MIN_MAPPABLE_BYTES as usize - 1) & !(MIN_MAPPABLE_BYTES as usize - 1);
        (raw, aligned as *const u8)
    }

    #[test]
    fn a_granule_sized_aligned_region_is_mappable() {
        let (_keep, ptr) = aligned_block(MIN_MAPPABLE_BYTES as usize);
        assert_eq!(mapping_problem(ptr, MIN_MAPPABLE_BYTES as usize, 0x4000_0000), None);
    }

    #[test]
    fn a_region_below_the_measured_minimum_is_refused_with_its_size() {
        let (_keep, ptr) = aligned_block(0x1000);
        assert_eq!(
            mapping_problem(ptr, 0x1000, 0x4000_0000),
            Some(MappingProblem::TooSmall { size: 0x1000 })
        );
    }

    #[test]
    fn a_region_whose_size_is_not_a_granule_multiple_is_refused() {
        let (_keep, ptr) = aligned_block(0x4001);
        assert_eq!(
            mapping_problem(ptr, 0x4001, 0x4000_0000),
            Some(MappingProblem::MisalignedSize { size: 0x4001 })
        );
    }

    #[test]
    fn an_unaligned_guest_base_is_refused() {
        let (_keep, ptr) = aligned_block(MIN_MAPPABLE_BYTES as usize);
        assert_eq!(
            mapping_problem(ptr, MIN_MAPPABLE_BYTES as usize, 0x4000_0001),
            Some(MappingProblem::MisalignedBase { base: 0x4000_0001 })
        );
    }

    /// The case that decides the architecture: a host pointer one byte off the
    /// granule is refused even though the size and base are both fine. A backend
    /// that copies `Ram` bytes into a plain `Vec<u8>` will trip this, because a
    /// `Vec<u8>` carries no alignment guarantee.
    #[test]
    fn an_unaligned_host_address_is_refused_even_when_size_and_base_are_fine() {
        let (_keep, ptr) = aligned_block(MIN_MAPPABLE_BYTES as usize);
        let nudged = unsafe { ptr.add(1) };
        assert_eq!(
            mapping_problem(nudged, MIN_MAPPABLE_BYTES as usize, 0x4000_0000),
            Some(MappingProblem::MisalignedHost { alignment: 1 })
        );
    }

    #[test]
    fn sparse_ram_is_reported_as_paged_and_never_as_mappable() {
        let mut bus = Bus::new();
        bus.map(BusRegion {
            base: 0x4000_0000,
            size: 0x1_0000_0000,
            device: Box::new(SparseRam::new(0x4000_0000, 0x1_0000_0000)),
        })
        .expect("map sparse RAM");

        let report = bus.ram_backing_report();
        assert_eq!(report.len(), 1, "the sparse region is still reported");
        assert_eq!(report[0].problem, Some(MappingProblem::PagedBacking));
        assert!(!report[0].is_mappable());
        assert!(
            bus.mappable_ram().is_empty(),
            "a 4 KiB-paged region cannot be handed to a hypervisor in one mapping"
        );
    }

    #[test]
    fn contiguous_ram_is_reported_with_the_reason_it_cannot_be_mapped() {
        let mut bus = Bus::new();
        // Small enough to be refused on size, so the assertion is about the
        // reason rather than about the allocator's alignment, which is not
        // something a test can pin.
        bus.map(BusRegion {
            base: 0x4000_0000,
            size: 0x1000,
            device: Box::new(Ram::new(0x4000_0000, 0x1000)),
        })
        .expect("map ram");

        let report = bus.ram_backing_report();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].problem, Some(MappingProblem::TooSmall { size: 0x1000 }));
        assert_eq!(report[0].base, 0x4000_0000);
        assert!(bus.mappable_ram().is_empty());
    }

    #[test]
    fn a_non_ram_device_is_not_reported_at_all() {
        let mut bus = Bus::new();
        bus.map(BusRegion {
            base: 0x4000_0000,
            size: 0x1000,
            device: Box::new(crate::memory::Rom::new(0x4000_0000, vec![0u8; 0x1000])),
        })
        .expect("map rom");

        assert!(
            bus.ram_backing_report().is_empty(),
            "a ROM is not RAM and must not appear in a mappable-memory report"
        );
    }

    /// The report must describe the interpreter's world, so a device that is
    /// there and readable stays there: this guards against the report itself
    /// becoming a way to observe or mutate device state.
    #[test]
    fn reporting_does_not_disturb_bus_behaviour() {
        let mut bus = Bus::new();
        let mut ram = Ram::new(0x4000_0000, 0x8000);
        ram.load(0x40, &[0xaa, 0xbb, 0xcc, 0xdd]);
        bus.map(BusRegion {
            base: 0x4000_0000,
            size: 0x8000,
            device: Box::new(ram),
        })
        .expect("map ram");

        let before = bus.read(
            crate::MemTransaction {
                address: 0x4000_0040,
                size: 4,
                kind: crate::TransactionKind::Read,
                security: crate::SecurityState::Secure,
                source: None,
            },
            &mut crate::trace::TraceEngine::new(false),
            0,
            0,
            0,
        );
        let _ = bus.ram_backing_report();
        let _ = bus.mappable_ram();
        let after = bus.read(
            crate::MemTransaction {
                address: 0x4000_0040,
                size: 4,
                kind: crate::TransactionKind::Read,
                security: crate::SecurityState::Secure,
                source: None,
            },
            &mut crate::trace::TraceEngine::new(false),
            0,
            0,
            0,
        );
        // `BusError` is not `PartialEq`, so compare the readable outcomes rather
        // than the `Result` values themselves.
        assert_eq!(before.is_ok(), after.is_ok());
        assert_eq!(before.unwrap(), after.unwrap());
        assert_eq!(before.unwrap(), 0xddcc_bbaa);
    }
}
