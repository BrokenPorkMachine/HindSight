//! Single Apple PMGR power-state register, distinct from the legacy Pmgr map.
//!
//! Layout evidence: Linux drivers/pmdomain/apple/pmgr-pwrstate.c and m1n1
//! src/pmgr.c. T8020 SEP address evidence: docs/DFU_BOOT_PROGRESS.md.
//! This synchronous subset handles software target states 0, 4 and 15.
//! Automatic gating, parent dependencies and downstream resets are not modeled.
//!
//! Also models PMGR boot-status words at 0x23d280058..0x23d280088 and
//! 0x23d284000..0x23d284018 that the ROM polls for device readiness.
//! These return 0xc0 (bits 6:7 set) when the associated device is ready.
//! This model transitions to ready when the associated PMGR power-state
//! register reaches active state (15).
use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

/// T8020 boot-event/control word observed at physical `0x23b0b8000`.
///
/// SecureROM writes this word and iBoot performs read/modify/write operations
/// on bits 5 and 7. Keep exactly that guest-visible register persistent; this
/// is not a fabricated boot-event selector or device-readiness response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmgrBootEvent {
    pub id: DeviceId,
    pub values: [u32; 2],
}

impl PmgrBootEvent {
    pub const fn new(id: DeviceId) -> Self {
        Self { id, values: [0; 2] }
    }
}

impl Device for PmgrBootEvent {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pmgr-boot-event"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if matches!(offset, 0 | 4) && size == 4 {
            DeviceResult::Value(self.values[(offset / 4) as usize] as u64)
        } else {
            DeviceResult::Fault
        }
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if matches!(offset, 0 | 4) && size == 4 {
            self.values[(offset / 4) as usize] = value as u32;
            DeviceResult::Ok
        } else {
            DeviceResult::Fault
        }
    }
    fn reset(&mut self) {
        self.values = [0; 2];
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

/// Shared registry for boot-status devices to enable synchronous updates
/// from power-state writes.
#[derive(Default)]
pub struct BootStatusRegistry {
    // Map from power-state address to list of weak references to boot-status devices
    devices: Mutex<HashMap<u64, Vec<Weak<Mutex<PmgrBootStatus>>>>>,
}

impl BootStatusRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Register a boot-status device for synchronous updates
    pub fn register(&self, power_state_addr: u64, device: Arc<Mutex<PmgrBootStatus>>) {
        let mut map = self.devices.lock().unwrap();
        map.entry(power_state_addr)
            .or_default()
            .push(Arc::downgrade(&device));
    }

    /// Update all boot-status devices associated with a power-state address
    pub fn update_from_power_state(&self, power_state_addr: u64, power_state_value: u32) {
        let map = self.devices.lock().unwrap();
        if let Some(weak_refs) = map.get(&power_state_addr) {
            for weak_ref in weak_refs {
                if let Some(device) = weak_ref.upgrade() {
                    let mut bs = device.lock().unwrap();
                    bs.update_from_power_state(power_state_value);
                }
            }
        }
    }
}

/// PMGR boot-status register that reports device readiness based on power state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PmgrBootStatus {
    pub id: DeviceId,
    /// Associated power-state register address to monitor for readiness
    pub power_state_addr: u64,
    /// Current status value
    pub status: u32,
    /// Whether this device is marked as ready
    pub ready: bool,
}

impl PmgrBootStatus {
    /// Create a new boot-status register for the given device
    pub fn new(id: DeviceId, power_state_addr: u64) -> Self {
        Self {
            id,
            power_state_addr,
            status: 0,
            ready: false,
        }
    }

    /// Create a boot-status register that starts ready (for always-on devices)
    pub fn initially_ready(id: DeviceId, power_state_addr: u64) -> Self {
        Self {
            id,
            power_state_addr,
            status: 0xc0,
            ready: true,
        }
    }

    /// Update readiness based on the associated power-state register
    pub fn update_from_power_state(&mut self, power_state: u32) {
        // Device is ready when power state is active (15 = 0xf)
        let is_active = (power_state & 0xf0) == 0xf0;
        if is_active && !self.ready {
            self.ready = true;
            let target = self.status & 0xf;
            self.status = if target == 0 {
                0xc0
            } else {
                target | (target << 4)
            };
        } else if !is_active && self.ready {
            self.ready = false;
            self.status = 0;
        }
    }
}

impl Device for PmgrBootStatus {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pmgr-boot-status"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if offset != 0 || size != 4 {
            return DeviceResult::Fault;
        }
        DeviceResult::Value(self.status as u64)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if offset != 0 || size != 4 {
            return DeviceResult::Fault;
        }
        // Real d321 iBoot also treats these words as PMGR target/actual-state
        // mirrors: low nibble is the software target and the high nibble is
        // the hardware actual state. Preserve the SecureROM-ready 0xc0 image
        // until software requests a supported transition, then complete that
        // transition synchronously like PmgrPowerState.
        let target = value as u32 & 0xf;
        if matches!(target, 0 | 4 | 15) {
            self.status = (self.status & !0xff) | target | (target << 4);
            self.ready = target == 15;
        }
        DeviceResult::Ok
    }
    fn reset(&mut self) {
        self.status = 0;
        self.ready = false;
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

/// Wrapper to allow Arc<Mutex<PmgrBootStatus>> to be used as a Device
pub struct PmgrBootStatusDevice {
    inner: Arc<Mutex<PmgrBootStatus>>,
}

impl PmgrBootStatusDevice {
    pub fn new(inner: Arc<Mutex<PmgrBootStatus>>) -> Self {
        Self { inner }
    }
}

impl Device for PmgrBootStatusDevice {
    fn id(&self) -> DeviceId {
        self.inner.lock().unwrap().id
    }
    fn name(&self) -> &'static str {
        "pmgr-boot-status"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        self.inner.lock().unwrap().read(offset, size)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        self.inner.lock().unwrap().write(offset, size, value)
    }
    fn reset(&mut self) {
        self.inner.lock().unwrap().reset()
    }
    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        self.inner.lock().unwrap().tick(cycles)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

type PmgrOnWrite = dyn Fn(u32) + Send;

pub struct PmgrPowerState {
    pub id: DeviceId,
    pub register: u32,
    pub reset_register: u32,
    /// Optional callback invoked when the power-state register is written.
    /// Receives the new register value. Used to synchronously update
    /// associated boot-status registers.
    pub on_write: Option<Arc<Mutex<PmgrOnWrite>>>,
}

impl PmgrPowerState {
    const ACTUAL: u32 = 0xf0;
    const HISTORY: u32 = 0x300;
    const AUTO_ENABLE: u32 = 1 << 28;

    /// Start powered off. This is a conservative emulator default, not a
    /// measured T8020 reset value or evidence that SEP boot has completed.
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            register: 0,
            reset_register: 0,
            on_write: None,
        }
    }

    /// A platform boot policy for a domain described as powered on by its
    /// firmware. This establishes power state only, never firmware readiness.
    pub fn initially_active(id: DeviceId) -> Self {
        Self {
            id,
            register: 0xff,
            reset_register: 0xff,
            on_write: None,
        }
    }

    /// Set a callback to be invoked when the power-state register is written.
    pub fn set_on_write<F>(&mut self, callback: F)
    where
        F: Fn(u32) + Send + 'static,
    {
        self.on_write = Some(Arc::new(Mutex::new(callback)));
    }
}

impl Device for PmgrPowerState {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pmgr-power-state"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if offset != 0 || size != 4 {
            return DeviceResult::Fault;
        }
        DeviceResult::Value(self.register as u64)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if offset != 0 || size != 4 {
            return DeviceResult::Fault;
        }
        let value = value as u32;
        // Actual state is hardware-owned; history flags are write-one-clear.
        let actual = self.register & Self::ACTUAL;
        let history = self.register & Self::HISTORY & !value;
        self.register = (value & !(Self::ACTUAL | Self::HISTORY)) | actual | history;
        let target = value & 0xf;
        if value & Self::AUTO_ENABLE == 0 && matches!(target, 0 | 4 | 15) {
            self.register = (self.register & !Self::ACTUAL) | (target << 4);
        }
        // Invoke the write callback if set
        if let Some(cb) = &self.on_write {
            if let Ok(cb) = cb.lock() {
                cb(self.register);
            }
        }
        DeviceResult::Ok
    }
    fn reset(&mut self) {
        self.register = self.reset_register;
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
    fn boot_event_word_persists_firmware_read_modify_write() {
        let mut event = PmgrBootEvent::new(9);
        assert_eq!(event.read(0, 4), DeviceResult::Value(0));
        assert_eq!(event.write(0, 4, 0x80), DeviceResult::Ok);
        assert_eq!(event.read(0, 4), DeviceResult::Value(0x80));
        assert_eq!(event.read(4, 4), DeviceResult::Value(0));
        assert_eq!(event.write(4, 4, 0x20), DeviceResult::Ok);
        assert_eq!(event.read(4, 4), DeviceResult::Value(0x20));
        assert_eq!(event.read(8, 4), DeviceResult::Fault);
        assert_eq!(event.write(0, 8, 0), DeviceResult::Fault);
        event.reset();
        assert_eq!(event.read(0, 4), DeviceResult::Value(0));
    }

    #[test]
    fn boot_status_accepts_iboot_target_actual_state_handshake() {
        let mut status = PmgrBootStatus::initially_ready(10, 0x23b080600);
        assert_eq!(status.read(0, 4), DeviceResult::Value(0xc0));
        assert_eq!(status.write(0, 4, 0xf), DeviceResult::Ok);
        assert_eq!(status.read(0, 4), DeviceResult::Value(0xff));
        assert_eq!(status.write(0, 4, 4), DeviceResult::Ok);
        assert_eq!(status.read(0, 4), DeviceResult::Value(0x44));
    }

    fn read(device: &mut PmgrPowerState) -> u64 {
        match device.read(0, 4) {
            DeviceResult::Value(value) => value,
            _ => panic!("power-state read faulted"),
        }
    }

    #[test]
    fn explicit_requests_transition_but_status_writes_cannot_power_on() {
        let mut device = PmgrPowerState::new(1);
        assert_eq!(read(&mut device), 0);
        device.write(0, 4, 0xf0);
        assert_eq!(read(&mut device), 0);
        for (target, expected) in [(15, 0xff), (4, 0x44), (0, 0)] {
            device.write(0, 4, target);
            assert_eq!(read(&mut device), expected);
        }
    }

    #[test]
    fn unsupported_modes_and_automatic_gating_do_not_fake_completion() {
        let mut device = PmgrPowerState::new(1);
        device.write(0, 4, 0xf);
        device.write(0, 4, 3);
        assert_eq!(read(&mut device), 0xf3);
        device.write(0, 4, PmgrPowerState::AUTO_ENABLE as u64);
        assert_eq!(read(&mut device) & 0xff, 0xf0);
        device.write(0, 4, 0);
        assert_eq!(read(&mut device), 0);
        device.write(0, 4, 15);
        device.reset();
        assert_eq!(read(&mut device), 0);
    }

    #[test]
    fn invalid_accesses_do_not_mutate_state() {
        let mut device = PmgrPowerState::new(1);
        for (offset, size) in [(1, 4), (4, 4), (0, 1), (0, 2), (0, 8)] {
            assert!(matches!(
                device.write(offset, size, 15),
                DeviceResult::Fault
            ));
            assert!(matches!(device.read(offset, size), DeviceResult::Fault));
        }
        assert_eq!(read(&mut device), 0);
    }

    #[test]
    fn platform_boot_state_survives_reset_without_forcing_reads() {
        let mut device = PmgrPowerState::initially_active(1);
        assert_eq!(read(&mut device), 0xff);
        device.write(0, 4, 4);
        assert_eq!(read(&mut device), 0x44);
        device.write(0, 4, 0);
        assert_eq!(read(&mut device), 0);
        device.reset();
        assert_eq!(read(&mut device), 0xff);
    }
}
