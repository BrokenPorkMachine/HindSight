use crate::device::{Device, DeviceEvent, DeviceId};
use crate::trace::{TraceEngine, TraceKind};
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub enum BusError {
    Unmapped(u64),
    AccessViolation,
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::Unmapped(a) => write!(f, "no device at address {:#x}", a),
            BusError::AccessViolation => write!(f, "bus access violation"),
        }
    }
}

impl std::error::Error for BusError {}

#[derive(Debug, Clone, Copy)]
pub struct MemTransaction {
    pub address: u64,
    pub size: usize,
    pub kind: TransactionKind,
    pub security: SecurityState,
    pub source: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub enum SecurityState {
    Secure,
    NonSecure,
}

#[derive(Debug, Clone, Copy)]
pub enum TransactionKind {
    Read,
    Write,
    Execute,
}

pub struct BusRegion {
    pub base: u64,
    pub size: u64,
    pub device: Box<dyn Device>,
}

impl BusRegion {
    pub fn contains(&self, a: u64, s: usize) -> bool {
        let Ok(size) = u64::try_from(s) else {
            return false;
        };
        let Some(end) = a.checked_add(size) else {
            return false;
        };
        let Some(region_end) = self.base.checked_add(self.size) else {
            return false;
        };
        a >= self.base && end <= region_end
    }
}

#[derive(Default)]
pub struct Bus {
    pub regions: Vec<BusRegion>,
    pub fallback_device: Option<Box<dyn Device>>,
    pending_events: Vec<DeviceEvent>,
}

impl Bus {
    /// Route an interrupt line to the device that owns its controller input.
    /// Returns true when a device accepted the signal.
    pub fn signal_interrupt(&mut self, irq: u32, core: Option<usize>) -> bool {
        self.regions
            .iter_mut()
            .any(|region| region.device.signal_interrupt(irq, core))
    }

    /// Update a level-sensitive FIQ source on the owning interrupt device.
    pub fn set_fiq_line(&mut self, line: u32, core: Option<usize>, asserted: bool) -> bool {
        self.regions
            .iter_mut()
            .any(|region| region.device.set_fiq_line(line, core, asserted))
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn map(&mut self, r: BusRegion) -> Result<(), &'static str> {
        if r.size == 0 {
            return Err("zero-sized region");
        };
        let rend = r.base.checked_add(r.size).ok_or("region overflow")?;
        for x in &self.regions {
            let xend = x.base.saturating_add(x.size);
            if r.base < xend && x.base < rend {
                eprintln!(
                    "Overlap: r=({:#x}, {:#x}), x=({:#x}, {:#x})",
                    r.base, rend, x.base, xend
                );
                return Err("overlapping region");
            }
        }
        self.regions.push(r);
        self.regions.sort_by_key(|x| x.base);
        Ok(())
    }

    pub fn reset(&mut self) {
        for r in &mut self.regions {
            r.device.reset()
        }
        self.pending_events.clear();
        if let Some(ref mut d) = self.fallback_device {
            d.reset()
        }
    }

    pub fn find_device_by_id(&mut self, id: DeviceId) -> Option<&mut dyn Device> {
        self.regions
            .iter_mut()
            .find(|r| r.device.id() == id)
            .map(|r| r.device.as_mut())
    }

    fn find_mut(&mut self, a: u64, s: usize) -> Option<&mut BusRegion> {
        // Regions are kept base-sorted by `map`. The current `read`/`write`
        // semantics return the FIRST (lowest-base) containing region, so a
        // binary-search partition followed by a downward scan is exact, not an
        // approximation: every candidate has base <= a, and the first one to
        // contain the address is the one `find` on the sorted slice would hit.
        let base_ok = self.regions.partition_point(|r| r.base <= a);
        self.regions[..base_ok]
            .iter_mut()
            .rev()
            .find(|r| r.contains(a, s))
    }

    pub fn read(
        &mut self,
        tx: MemTransaction,
        trace: &mut TraceEngine,
        pc: u64,
        el: u8,
        pstate: u64,
    ) -> Result<u64, BusError> {
        if let Some(r) = self.find_mut(tx.address, tx.size) {
            let device_name = r.device.name();
            match r.device.read(tx.address - r.base, tx.size) {
                crate::device::DeviceResult::Value(v) => {
                    if !matches!(tx.kind, TransactionKind::Execute)
                        && !matches!(device_name, "sparse-dram" | "rom")
                    {
                        trace.record(
                            tx.address,
                            v,
                            tx.size,
                            TraceKind::MmioRead,
                            pc,
                            el,
                            pstate,
                            Some(device_name.to_string()),
                        );
                    }
                    Ok(v)
                }
                _ => Err(BusError::AccessViolation),
            }
        } else if let Some(ref mut d) = self.fallback_device {
            let device_name = d.name();
            match d.read(tx.address, tx.size) {
                crate::device::DeviceResult::Value(v) => {
                    if !matches!(tx.kind, TransactionKind::Execute) {
                        trace.record(
                            tx.address,
                            v,
                            tx.size,
                            TraceKind::MmioRead,
                            pc,
                            el,
                            pstate,
                            Some(device_name.to_string()),
                        );
                    }
                    Ok(v)
                }
                _ => Err(BusError::AccessViolation),
            }
        } else {
            Err(BusError::Unmapped(tx.address))
        }
    }

    pub fn write(
        &mut self,
        tx: MemTransaction,
        value: u64,
        trace: &mut TraceEngine,
        pc: u64,
        el: u8,
        pstate: u64,
    ) -> Result<(), BusError> {
        #[cfg(debug_assertions)]
        if tx.address >= 0x20a0c0000 && tx.address < 0x20a0c4000 {
            eprintln!(
                "UART WRITE: addr={:#x} val={:#x} size={}",
                tx.address, value, tx.size
            );
        }
        if let Some(r) = self.find_mut(tx.address, tx.size) {
            let device_name = r.device.name();
            let result = r.device.write(tx.address - r.base, tx.size, value);
            match result {
                crate::device::DeviceResult::Ok => {
                    if !matches!(device_name, "sparse-dram" | "rom") {
                        trace.record(
                            tx.address,
                            value,
                            tx.size,
                            TraceKind::MmioWrite,
                            pc,
                            el,
                            pstate,
                            Some(device_name.to_string()),
                        );
                    }
                    Ok(())
                }
                crate::device::DeviceResult::Event(event) => {
                    self.pending_events.push(event);
                    if !matches!(device_name, "sparse-dram" | "rom") {
                        trace.record(
                            tx.address,
                            value,
                            tx.size,
                            TraceKind::MmioWrite,
                            pc,
                            el,
                            pstate,
                            Some(device_name.to_string()),
                        );
                    }
                    Ok(())
                }
                _ => Err(BusError::AccessViolation),
            }
        } else if let Some(ref mut d) = self.fallback_device {
            let device_name = d.name();
            let result = d.write(tx.address, tx.size, value);
            match result {
                crate::device::DeviceResult::Ok => {
                    trace.record(
                        tx.address,
                        value,
                        tx.size,
                        TraceKind::MmioWrite,
                        pc,
                        el,
                        pstate,
                        Some(device_name.to_string()),
                    );
                    Ok(())
                }
                crate::device::DeviceResult::Event(event) => {
                    self.pending_events.push(event);
                    trace.record(
                        tx.address,
                        value,
                        tx.size,
                        TraceKind::MmioWrite,
                        pc,
                        el,
                        pstate,
                        Some(device_name.to_string()),
                    );
                    Ok(())
                }
                _ => Err(BusError::AccessViolation),
            }
        } else {
            Err(BusError::Unmapped(tx.address))
        }
    }

    pub fn tick(
        &mut self,
        cycle: u64,
        _trace: &mut TraceEngine,
        _pc: u64,
        _el: u8,
        _pstate: u64,
    ) -> Vec<DeviceEvent> {
        let mut events = std::mem::take(&mut self.pending_events);
        for r in &mut self.regions {
            if let Some(e) = r.device.tick(cycle) {
                events.push(e);
            }
        }
        if let Some(ref mut d) = self.fallback_device {
            if let Some(e) = d.tick(cycle) {
                events.push(e);
            }
        }
        events
    }
}
pub fn debug_stores() -> Vec<(u64, usize, u64)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceResult;

    struct EventOnWrite;

    impl Device for EventOnWrite {
        fn id(&self) -> DeviceId {
            0xfeed
        }

        fn name(&self) -> &'static str {
            "event-on-write"
        }

        fn reset(&mut self) {}

        fn read(&mut self, _: u64, _: usize) -> DeviceResult {
            DeviceResult::Value(0)
        }

        fn write(&mut self, _: u64, _: usize, _: u64) -> DeviceResult {
            DeviceResult::Event(DeviceEvent::InterruptTo(70, Some(1)))
        }

        fn tick(&mut self, _: u64) -> Option<DeviceEvent> {
            None
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn region_contains_rejects_overflowing_access() {
        let region = BusRegion {
            base: 0x1000,
            size: 0x100,
            device: Box::new(EventOnWrite),
        };
        assert!(region.contains(0x1000, 1));
        assert!(region.contains(0x10f0, 0x10));
        assert!(!region.contains(0x10f1, 0x10));
        assert!(!region.contains(u64::MAX - 3, 8));
    }

    #[test]
    fn write_side_device_events_are_queued_for_tick_delivery() {
        let mut bus = Bus::new();
        bus.map(BusRegion {
            base: 0x1000,
            size: 0x100,
            device: Box::new(EventOnWrite),
        })
        .expect("map event device");
        let mut trace = TraceEngine::new(false);

        bus.write(
            MemTransaction {
                address: 0x1000,
                size: 8,
                kind: TransactionKind::Write,
                security: SecurityState::Secure,
                source: None,
            },
            0x1234,
            &mut trace,
            0,
            0,
            0,
        )
        .expect("write event must not fault");

        assert_eq!(
            bus.tick(1, &mut trace, 0, 0, 0),
            vec![DeviceEvent::InterruptTo(70, Some(1))]
        );
        assert!(bus.tick(1, &mut trace, 0, 0, 0).is_empty());
    }
}
