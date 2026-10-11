//! USB host-side stack: USB-PHY, USB-MUX, USB-PD (Phase 166).
//!
//! These models complement the existing `UsbDeviceController` (DFU device
//! mode) by providing the physical-layer, mux, and USB-PD-message hardware
//! that real Apple SoCs expose. They are register-level functional models
//! only — no real protocol engine, no actual USB traffic.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;

/// Narrow d321ap USB-complex register window observed by SecureROM.
/// Offsets 0x48 (control) and 0x64 (status) are kept separate from the
/// endpoint controller because the ADT places this parent block at
/// 0x239000000.
pub struct UsbComplex {
    pub id: DeviceId,
    pub control: u32,
    /// OTG PHY status register at offset 0x64. Bit 16 indicates PHY ready.
    /// This transitions to ready when the associated PMGR power-states
    /// (USBCTLREG at 0x23b0802a8 and USB2HOST0 at 0x23b0802b0) reach active.
    pub status: u32,
    /// Whether the PHY has been marked ready
    phy_ready: bool,
}

impl UsbComplex {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            control: 0,
            status: 0,
            phy_ready: false,
        }
    }

    /// Call this when PMGR power-states indicate USB is powered up
    pub fn update_phy_ready(&mut self, usbctlreg_active: bool, usb2host0_active: bool) {
        let should_be_ready = usbctlreg_active && usb2host0_active;
        if should_be_ready && !self.phy_ready {
            self.phy_ready = true;
            // Set bit 16 (PHY ready) and bit 0 (session valid) as observed
            self.status = 0x10001;
        } else if !should_be_ready && self.phy_ready {
            self.phy_ready = false;
            self.status = 0;
        }
    }
}

impl Device for UsbComplex {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "usb-complex"
    }
    fn reset(&mut self) {
        self.control = 0;
        self.status = 0;
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !valid_size(size) {
            return DeviceResult::Fault;
        }
        match offset {
            0x48 => DeviceResult::Value(self.control as u64),
            0x64 => DeviceResult::Value(self.status as u64),
            _ => read_zero(offset, size),
        }
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !valid_size(size) {
            return DeviceResult::Fault;
        }
        match offset {
            0x48 => {
                self.control = value as u32;
                DeviceResult::Ok
            }
            _ => drop_write(offset, size, value),
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

fn valid_size(s: usize) -> bool {
    matches!(s, 1 | 2 | 4 | 8)
}

fn read_zero(_o: u64, s: usize) -> DeviceResult {
    if valid_size(s) {
        DeviceResult::Value(0)
    } else {
        DeviceResult::Fault
    }
}

fn drop_write(_o: u64, s: usize, _v: u64) -> DeviceResult {
    if valid_size(s) {
        DeviceResult::Ok
    } else {
        DeviceResult::Fault
    }
}

// ----------------------------------------------------------------------------
// UsbPhy — physical-layer controller (VBUS, ID-pin, role-swap).
// ----------------------------------------------------------------------------

/// USB-PHY role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbPhyRole {
    /// Device mode (VBUS-powered; ID pin high). Default after reset.
    Device,
    /// Host mode (we supply VBUS; ID pin low).
    Host,
    /// OTG mode (negotiate role).
    Otg,
}

pub struct UsbPhy {
    pub id: DeviceId,
    /// Bit 0: VBUS present. Bit 1: ID-pin state. Bit 2: VBUS valid.
    pub status: u32,
    /// Bit 0: role-swap request. Bit 1: assert VBUS (host). Bit 2: OTG enable.
    pub control: u32,
    pub role: UsbPhyRole,
    pub irq_status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
}

impl UsbPhy {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            status: 0x1, // VBUS present at boot (assuming charger plugged in)
            control: 0,
            role: UsbPhyRole::Device,
            irq_status: 0,
            irq_en: 0,
            irq: 70,
            last_status: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// External helper for tests: simulate VBUS plug-in.
    pub fn set_vbus_present(&mut self, present: bool) {
        if present {
            self.status |= 0x1;
        } else {
            self.status &= !0x1;
        }
        self.irq_status |= 0x1;
    }
}

impl Device for UsbPhy {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "usbphy"
    }
    fn reset(&mut self) {
        self.status = 0x1;
        self.control = 0;
        self.role = UsbPhyRole::Device;
        self.irq_status = 0;
        self.irq_en = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.status as u64),
            0x04 => DeviceResult::Value(self.control as u64),
            0x08 => DeviceResult::Value(self.role as u64),
            0x0C => DeviceResult::Value(self.irq_status as u64),
            0x10 => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x04 => {
                self.control = v as u32;
                // Bit 1 (assert VBUS) flips role to Host.
                if v & 0x2 != 0 {
                    self.role = UsbPhyRole::Host;
                } else {
                    self.role = UsbPhyRole::Device;
                }
                DeviceResult::Ok
            }
            0x0C => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x10 => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.irq_status & self.irq_en) != 0;
        if active && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if active { self.irq } else { 0 };
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

// ----------------------------------------------------------------------------
// UsbMux — switch between charger / host / device paths.
// ----------------------------------------------------------------------------

/// USB-MUX path selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbMuxPath {
    /// Nothing connected (open switch).
    None = 0,
    /// Charger path (Vbus to battery charger).
    Charger = 1,
    /// USB host path (SoC is device, host reads from us).
    Host = 2,
    /// USB device path (SoC is device, we read from host).
    Device = 3,
    /// USB-PD path (PD messaging).
    UsbPd = 4,
}

pub struct UsbMux {
    pub id: DeviceId,
    /// Currently active path (0..4, see `UsbMuxPath`).
    pub select: u32,
    /// Bitmask of paths that are connected (0..4).
    pub status: u32,
    pub irq_status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
}

impl UsbMux {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            select: UsbMuxPath::None as u32,
            status: 0,
            irq_status: 0,
            irq_en: 0,
            irq: 71,
            last_status: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// External helper: simulate a path becoming connected.
    pub fn connect(&mut self, path: UsbMuxPath) {
        let bit = 1u32 << (path as u32);
        self.status |= bit;
        self.irq_status |= bit;
    }
}

impl Device for UsbMux {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "usbmux"
    }
    fn reset(&mut self) {
        self.select = UsbMuxPath::None as u32;
        self.status = 0;
        self.irq_status = 0;
        self.irq_en = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.select as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.irq_status as u64),
            0x0C => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.select = v as u32 & 0x7; // 3-bit path select
                DeviceResult::Ok
            }
            0x08 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x0C => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.irq_status & self.irq_en) != 0;
        if active && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if active { self.irq } else { 0 };
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

// ----------------------------------------------------------------------------
// UsbPd — USB Power Delivery (PD) message engine.
// ----------------------------------------------------------------------------

/// USB-PD state machine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbPdState {
    /// Waiting for source/sink connection.
    Idle = 0,
    /// Source-capabilities message sent.
    SourceCaps = 1,
    /// Request message sent.
    Request = 2,
    /// Power contract accepted.
    Accept = 3,
    /// Ready (PS_RDY received/sent).
    Ready = 4,
    /// Hard-reset in progress.
    HardReset = 5,
}

pub struct UsbPd {
    pub id: DeviceId,
    /// 32-byte message-in buffer (host writes).
    pub message_in: [u8; 32],
    /// 32-byte message-out buffer (device writes).
    pub message_out: [u8; 32],
    /// Negotiated voltage in mV (0 = no contract).
    pub contract_voltage_mv: u32,
    /// Negotiated current in mA (0 = no contract).
    pub contract_current_ma: u32,
    pub state: UsbPdState,
    pub irq_status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
}

impl UsbPd {
    pub const MSG_BUF_SIZE: usize = 32;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            message_in: [0; Self::MSG_BUF_SIZE],
            message_out: [0; Self::MSG_BUF_SIZE],
            contract_voltage_mv: 0,
            contract_current_ma: 0,
            state: UsbPdState::Idle,
            irq_status: 0,
            irq_en: 0,
            irq: 72,
            last_status: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// External helper: simulate a PD source-cap message arriving.
    pub fn receive_source_caps(&mut self, voltage_mv: u32, current_ma: u32) {
        self.contract_voltage_mv = voltage_mv;
        self.contract_current_ma = current_ma;
        self.state = UsbPdState::SourceCaps;
        self.irq_status |= 0x1;
    }
}

impl Device for UsbPd {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "usbpd"
    }
    fn reset(&mut self) {
        self.message_in = [0; Self::MSG_BUF_SIZE];
        self.message_out = [0; Self::MSG_BUF_SIZE];
        self.contract_voltage_mv = 0;
        self.contract_current_ma = 0;
        self.state = UsbPdState::Idle;
        self.irq_status = 0;
        self.irq_en = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            // Message-in buffer (0x00..0x20) — read by host.
            m if (0x00..0x20).contains(&m) => {
                let idx = m as usize;
                DeviceResult::Value(self.message_in[idx] as u64)
            }
            // Message-out buffer (0x20..0x40) — read by device.
            m if (0x20..0x40).contains(&m) => {
                let idx = (m - 0x20) as usize;
                DeviceResult::Value(self.message_out[idx] as u64)
            }
            0x40 => DeviceResult::Value(self.contract_voltage_mv as u64),
            0x44 => DeviceResult::Value(self.contract_current_ma as u64),
            0x48 => DeviceResult::Value(self.state as u64),
            0x4C => DeviceResult::Value(self.irq_status as u64),
            0x50 => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            // Message-in buffer (0x00..0x20) — written by device.
            m if (0x00..0x20).contains(&m) => {
                let idx = m as usize;
                self.message_in[idx] = v as u8;
                DeviceResult::Ok
            }
            // Message-out buffer (0x20..0x40) — written by host.
            m if (0x20..0x40).contains(&m) => {
                let idx = (m - 0x20) as usize;
                self.message_out[idx] = v as u8;
                DeviceResult::Ok
            }
            0x40 => {
                self.contract_voltage_mv = v as u32;
                DeviceResult::Ok
            }
            0x44 => {
                self.contract_current_ma = v as u32;
                DeviceResult::Ok
            }
            0x48 => {
                self.state = match (v as u32) & 0x7 {
                    0 => UsbPdState::Idle,
                    1 => UsbPdState::SourceCaps,
                    2 => UsbPdState::Request,
                    3 => UsbPdState::Accept,
                    4 => UsbPdState::Ready,
                    5 => UsbPdState::HardReset,
                    _ => UsbPdState::Idle,
                };
                DeviceResult::Ok
            }
            0x4C => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x50 => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.irq_status & self.irq_en) != 0;
        if active && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if active { self.irq } else { 0 };
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
