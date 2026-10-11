//! Apple memory-controller-complex (MCC/AMCC) register subset.
//!
//! The d321 iBoot image initializes planes at `0x23d200000` and
//! `0x23d240000`. Registers without evidence-backed side effects intentionally behave as reset-zero
//! storage registers and retain guest writes; they are explicit pass-through state, not fabricated completion. Command ports use bit 25 as a busy strobe; the
//! functional model completes those commands synchronously by clearing it.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::HashMap;

pub struct AppleMcc {
    id: DeviceId,
    registers: HashMap<u64, u64>,
    reset_registers: HashMap<u64, u64>,
}

impl AppleMcc {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            registers: HashMap::new(),
            reset_registers: HashMap::new(),
        }
    }

    pub fn with_reset_register(mut self, offset: u64, value: u64) -> Self {
        self.registers.insert(offset, value);
        self.reset_registers.insert(offset, value);
        self
    }
}

impl Device for AppleMcc {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "apple-mcc"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        DeviceResult::Value(*self.registers.get(&offset).unwrap_or(&0))
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4 | 8) {
            return DeviceResult::Fault;
        }
        let value = if matches!(offset, 0 | 0x2000) {
            value & !(1 << 25)
        } else {
            value
        };
        self.registers.insert(offset, value);
        DeviceResult::Ok
    }
    fn reset(&mut self) {
        self.registers.clone_from(&self.reset_registers);
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
mod tests {
    use super::*;

    #[test]
    fn command_ports_clear_busy_and_ordinary_registers_round_trip() {
        let mut mcc = AppleMcc::new(1).with_reset_register(0x20, 0xe);
        assert_eq!(mcc.read(0x20, 4), DeviceResult::Value(0xe));
        assert_eq!(mcc.write(0, 4, 0x8200_1234), DeviceResult::Ok);
        assert_eq!(mcc.read(0, 4), DeviceResult::Value(0x8000_1234));
        assert_eq!(mcc.write(0x300, 4, 0x55aa), DeviceResult::Ok);
        assert_eq!(mcc.read(0x300, 4), DeviceResult::Value(0x55aa));
        mcc.reset();
        assert_eq!(mcc.read(0x300, 4), DeviceResult::Value(0));
        assert_eq!(mcc.read(0x20, 4), DeviceResult::Value(0xe));
    }
}
