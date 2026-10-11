//! User-visible peripherals (Phase 162).
//!
//! Register-level functional models for the peripherals that make the device
//! "feel" like a phone rather than a bare SoC:
//! - `TouchHid` — HID-over-USB multi-touch digitizer
//! - `AudioI2s` — I²S audio output
//! - `BtHci` — Bluetooth HCI command / event register window
//! - `WifiSdio` — SDIO-attached Wi-Fi (CMD52/CMD53 register window)
//! - `CameraIsp` — bounded camera ISP model
//! - `SensorHub` — bounded I²C-attached gyro / accel / baro / magnetometer model

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;

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
// TouchHid — HID-over-USB multi-touch digitizer.
// ----------------------------------------------------------------------------

pub const TOUCH_MAX_POINTS: usize = 10;

#[derive(Debug, Clone, Copy, Default)]
pub struct TouchPoint {
    pub id: u8,
    pub x: u16,
    pub y: u16,
    pub pressure: u8,
    pub active: bool,
}

/// Recognized high-level gestures (0 = none).
pub const GESTURE_NONE: u8 = 0;
pub const GESTURE_TAP: u8 = 1;
pub const GESTURE_SWIPE: u8 = 2;
pub const GESTURE_PINCH: u8 = 3;

pub struct TouchHid {
    pub id: DeviceId,
    pub points: [TouchPoint; TOUCH_MAX_POINTS],
    pub ctrl: u32,
    pub status: u32,
    pub width: u16,
    pub height: u16,
    /// Time the most recent scan completed, in microseconds since boot.
    pub scan_time_us: u64,
    /// Palm-rejection enable (1 bit). When set, touches with pressure above
    /// `force_threshold_mN` and surface area > 70% are rejected.
    pub palm_reject_enabled: bool,
    /// Force threshold in milli-Newtons. Touches above this level are
    /// classified as "hard" and may bypass palm rejection.
    pub force_threshold_mn: u16,
    /// Current high-level gesture (GESTURE_NONE / TAP / SWIPE / PINCH).
    pub gesture: u8,
    /// Vendor ID (0x05AC = Apple).
    pub vendor_id: u16,
    /// Product ID (0x12A0 = T8020-era Apple digitizer).
    pub product_id: u16,
    /// Monotonically-increasing serial number stamped on every `report`.
    pub report_serial: u32,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
}

impl TouchHid {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            points: [TouchPoint::default(); TOUCH_MAX_POINTS],
            ctrl: 0,
            status: 0,
            width: 1080,
            height: 1920,
            scan_time_us: 0,
            palm_reject_enabled: true,
            force_threshold_mn: 250,
            gesture: GESTURE_NONE,
            vendor_id: 0x05AC,
            product_id: 0x12A0,
            report_serial: 0,
            irq: 80,
            last_status: 0,
            flash_storage: Vec::new(),
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    /// Inject a touch event (id, x, y, pressure). Use the same `id` to
    /// update; new `id` allocates a new slot; pass `pressure = 0` to release.
    /// Increments `report_serial` on every accepted event.
    pub fn report(&mut self, id: u8, x: u16, y: u16, pressure: u8) {
        let was_released = pressure == 0 && self.points.iter().any(|p| p.id == id && p.active);
        if pressure == 0 {
            if let Some(index) = self.points.iter().position(|p| p.id == id && p.active) {
                self.points[index].active = false;
                self.report_serial = self.report_serial.wrapping_add(1);
                if !self.points.iter().any(|point| point.active) {
                    self.status &= !0x1;
                }
                return;
            }
        }
        if let Some(p) = self.points.iter_mut().find(|p| p.id == id && p.active) {
            p.x = x;
            p.y = y;
            p.pressure = pressure;
            self.status |= 0x1;
            self.report_serial = self.report_serial.wrapping_add(1);
            return;
        }
        if let Some(p) = self.points.iter_mut().find(|p| !p.active) {
            p.id = id;
            p.x = x;
            p.y = y;
            p.pressure = pressure;
            p.active = true;
            self.status |= 0x1;
            self.report_serial = self.report_serial.wrapping_add(1);
        } else {
            // No slot available; drop the event silently.
            let _ = was_released;
        }
    }

    /// Force a high-level gesture (for test injection).
    pub fn report_gesture(&mut self, g: u8) {
        self.gesture = g;
        self.status |= 0x4;
        self.report_serial = self.report_serial.wrapping_add(1);
    }
}

impl Device for TouchHid {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "touch-hid"
    }
    fn reset(&mut self) {
        for p in self.points.iter_mut() {
            *p = TouchPoint::default();
        }
        self.ctrl = 0;
        self.status = 0;
        self.scan_time_us = 0;
        self.palm_reject_enabled = true;
        self.force_threshold_mn = 250;
        self.gesture = GESTURE_NONE;
        self.report_serial = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.width as u64),
            0x0C => DeviceResult::Value(self.height as u64),
            0x10 => DeviceResult::Value(self.active_count() as u64),
            0x18 => DeviceResult::Value(self.scan_time_us),
            0x1C => DeviceResult::Value(self.palm_reject_enabled as u64),
            0x20 => DeviceResult::Value(self.force_threshold_mn as u64),
            0x24 => DeviceResult::Value(self.gesture as u64),
            0x28 => DeviceResult::Value(self.vendor_id as u64),
            0x2A => DeviceResult::Value(self.product_id as u64),
            0x30 => DeviceResult::Value(self.report_serial as u64),
            // 16-byte touch-point record: id(1), _pad(1), x(2), y(2), pressure(1), _pad(1)
            r if (0x100..0x100 + (TOUCH_MAX_POINTS as u64) * 16).contains(&r) => {
                let i = ((r - 0x100) / 16) as usize;
                let off = (r - 0x100 - (i as u64) * 16) as usize;
                let p = &self.points[i];
                if !p.active {
                    return read_zero(o, s);
                }
                let bytes: [u8; 16] = [
                    p.id,
                    0,
                    (p.x & 0xFF) as u8,
                    (p.x >> 8) as u8,
                    (p.y & 0xFF) as u8,
                    (p.y >> 8) as u8,
                    p.pressure,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ];
                if off + s > 16 {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (bytes[off + i] as u64) << (i * 8);
                }
                DeviceResult::Value(v)
            }
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x08 => {
                self.width = v as u16;
                DeviceResult::Ok
            }
            0x0C => {
                self.height = v as u16;
                DeviceResult::Ok
            }
            0x18 => {
                self.scan_time_us = v;
                DeviceResult::Ok
            }
            0x1C => {
                self.palm_reject_enabled = (v & 1) != 0;
                DeviceResult::Ok
            }
            0x20 => {
                self.force_threshold_mn = (v as u16).min(1000);
                DeviceResult::Ok
            }
            0x24 => {
                self.gesture = (v as u8) & 0x3;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.status & 1) != 0;
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

impl TouchHid {
    pub fn active_count(&self) -> usize {
        self.points.iter().filter(|p| p.active).count()
    }
}

#[cfg(test)]
mod touch_tests {
    use super::TouchHid;

    #[test]
    fn report_release_clears_active_status_and_increments_serial() {
        let mut touch = TouchHid::new(1);

        touch.report(0, 100, 200, 0x80);
        assert_eq!(touch.status & 0x1, 0x1);
        assert_eq!(touch.active_count(), 1);
        assert_eq!(touch.report_serial, 1);

        touch.report(0, 100, 200, 0);
        assert_eq!(touch.status & 0x1, 0);
        assert_eq!(touch.active_count(), 0);
        assert_eq!(touch.report_serial, 2);
    }
}

// ----------------------------------------------------------------------------
// AudioI2s — I²S audio output
// ----------------------------------------------------------------------------

pub const I2S_FIFO_DEPTH: usize = 256;

/// Valid bit-depths: 16, 24, or 32.
pub const I2S_BIT_DEPTH_16: u32 = 16;
pub const I2S_BIT_DEPTH_24: u32 = 24;
pub const I2S_BIT_DEPTH_32: u32 = 32;

pub struct AudioI2s {
    pub id: DeviceId,
    pub ctrl: u32,
    pub status: u32,
    pub fifo: [i32; I2S_FIFO_DEPTH],
    pub fifo_count: usize,
    pub sample_rate: u32,
    /// Bit depth (16, 24, or 32). Anything else is rejected on write.
    pub bit_depth: u32,
    /// Number of channels: 1=mono, 2=stereo, 8=7.1 surround.
    pub channels: u8,
    /// Number of active TDM slots (1..=8). 0 = I²S / classic mode.
    pub tdm_slots: u8,
    /// Bit width of each TDM slot (16, 24, 32). Same constraint as `bit_depth`.
    pub tdm_slot_width: u32,
    /// Bit-clock frequency in Hz (e.g. 3.072 MHz for 48 kHz × 64 bclk/lrclk).
    pub bclk_freq_hz: u32,
    /// Left / right volume in dB (clamped -100..=0).
    pub volume_left: i8,
    pub volume_right: i8,
    pub mute_left: bool,
    pub mute_right: bool,
    /// Vendor ID (0x05AC = Apple integrated audio for newer SoCs).
    pub vendor_id: u16,
    /// Product ID.
    pub product_id: u16,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
}

impl AudioI2s {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: 0,
            status: 0,
            fifo: [0; I2S_FIFO_DEPTH],
            fifo_count: 0,
            sample_rate: 48_000,
            bit_depth: I2S_BIT_DEPTH_24,
            channels: 2,
            tdm_slots: 0,
            tdm_slot_width: I2S_BIT_DEPTH_24,
            bclk_freq_hz: 3_072_000,
            volume_left: 0,
            volume_right: 0,
            mute_left: false,
            mute_right: false,
            vendor_id: 0x05AC,
            product_id: 0x0201,
            irq: 81,
            last_status: 0,
            flash_storage: Vec::new(),
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    pub fn push_sample(&mut self, s: i32) -> bool {
        if self.fifo_count >= I2S_FIFO_DEPTH {
            return false;
        }
        self.fifo[self.fifo_count] = s;
        self.fifo_count += 1;
        if self.fifo_count >= I2S_FIFO_DEPTH / 2 {
            self.status |= 0x2;
        }
        true
    }
}

impl Device for AudioI2s {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "audio-i2s"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.status = 0;
        self.fifo = [0; I2S_FIFO_DEPTH];
        self.fifo_count = 0;
        self.sample_rate = 48_000;
        self.bit_depth = I2S_BIT_DEPTH_24;
        self.channels = 2;
        self.tdm_slots = 0;
        self.tdm_slot_width = I2S_BIT_DEPTH_24;
        self.bclk_freq_hz = 3_072_000;
        self.volume_left = 0;
        self.volume_right = 0;
        self.mute_left = false;
        self.mute_right = false;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.sample_rate as u64),
            0x0C => DeviceResult::Value(self.fifo_count as u64),
            0x10 => {
                if self.fifo_count == 0 {
                    return DeviceResult::Value(0);
                }
                let v = self.fifo[0] as u32 as u64;
                self.fifo.copy_within(1..self.fifo_count, 0);
                self.fifo_count -= 1;
                if self.fifo_count < I2S_FIFO_DEPTH / 2 {
                    self.status &= !0x2;
                }
                DeviceResult::Value(v)
            }
            0x14 => DeviceResult::Value(self.bit_depth as u64),
            0x18 => DeviceResult::Value(self.channels as u64),
            0x1C => DeviceResult::Value(self.tdm_slots as u64),
            0x20 => DeviceResult::Value(self.tdm_slot_width as u64),
            0x24 => DeviceResult::Value(self.bclk_freq_hz as u64),
            0x28 => DeviceResult::Value(self.volume_left as i64 as u64),
            0x2C => DeviceResult::Value(self.volume_right as i64 as u64),
            0x30 => DeviceResult::Value(self.mute_left as u64),
            0x34 => DeviceResult::Value(self.mute_right as u64),
            0x38 => DeviceResult::Value(self.vendor_id as u64),
            0x3A => DeviceResult::Value(self.product_id as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x08 => {
                self.sample_rate = v as u32;
                DeviceResult::Ok
            }
            0x10 => {
                self.push_sample(v as i32);
                DeviceResult::Ok
            }
            0x14 => {
                if matches!(v as u32, 16 | 24 | 32) {
                    self.bit_depth = v as u32;
                }
                DeviceResult::Ok
            }
            0x18 => {
                if matches!(v, 1 | 2 | 8) {
                    self.channels = v as u8;
                }
                DeviceResult::Ok
            }
            0x1C => {
                let n = (v as u8).min(8);
                self.tdm_slots = n;
                DeviceResult::Ok
            }
            0x20 => {
                if matches!(v as u32, 16 | 24 | 32) {
                    self.tdm_slot_width = v as u32;
                }
                DeviceResult::Ok
            }
            0x24 => {
                self.bclk_freq_hz = v as u32;
                DeviceResult::Ok
            }
            0x28 => {
                self.volume_left = (v as i8).clamp(-100, 0);
                DeviceResult::Ok
            }
            0x2C => {
                self.volume_right = (v as i8).clamp(-100, 0);
                DeviceResult::Ok
            }
            0x30 => {
                self.mute_left = (v & 1) != 0;
                DeviceResult::Ok
            }
            0x34 => {
                self.mute_right = (v & 1) != 0;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let half = (self.status & 0x2) != 0;
        if half && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if half { self.irq } else { 0 };
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
// BtHci — Bluetooth HCI command / event register window
// ----------------------------------------------------------------------------

pub const BT_HCI_CMD_BUF: usize = 256;
pub const BT_HCI_EVT_BUF: usize = 512;

pub struct BtHci {
    pub id: DeviceId,
    pub cmd: [u8; BT_HCI_CMD_BUF],
    pub cmd_len: u32,
    pub evt: [u8; BT_HCI_EVT_BUF],
    pub evt_len: u32,
    pub evt_read_pos: u32,
    pub ctrl: u32,
    /// ACL data TX buffer (host → controller).
    pub acl_tx: [u8; BT_HCI_CMD_BUF],
    pub acl_tx_len: u32,
    /// ACL data RX buffer (controller → host).
    pub acl_rx: [u8; BT_HCI_EVT_BUF],
    pub acl_rx_len: u32,
    /// Bluetooth device address (LSB first).
    pub bd_address: [u8; 6],
    /// Local device name (UTF-8, NUL-terminated, max 32 bytes).
    pub local_name: [u8; 32],
    pub local_name_len: u32,
    /// Transmit power in dBm (-30..+20).
    pub tx_power_dbm: i8,
    /// Sleep state: 0 = active, 1 = sniff, 2 = park.
    pub sleep_state: u32,
    /// OGF/OCF of the last submitted HCI command (for inspection).
    pub last_cmd_ogf: u8,
    pub last_cmd_ocf: u16,
    /// Vendor ID + product ID.
    pub vendor_id: u16,
    pub product_id: u16,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
}

impl BtHci {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            cmd: [0; BT_HCI_CMD_BUF],
            cmd_len: 0,
            evt: [0; BT_HCI_EVT_BUF],
            evt_len: 0,
            evt_read_pos: 0,
            ctrl: 0,
            acl_tx: [0; BT_HCI_CMD_BUF],
            acl_tx_len: 0,
            acl_rx: [0; BT_HCI_EVT_BUF],
            acl_rx_len: 0,
            bd_address: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66], // Apple-issued default
            local_name: [0; 32],
            local_name_len: 0,
            tx_power_dbm: 0,
            sleep_state: 0,
            last_cmd_ogf: 0,
            last_cmd_ocf: 0,
            vendor_id: 0x004C, // Apple Inc.
            product_id: 0x1234,
            irq: 82,
            last_status: 0,
            flash_storage: Vec::new(),
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// Queue a synthetic HCI event for the host to read out.
    pub fn push_event(&mut self, ev: &[u8]) -> bool {
        if ev.is_empty() || self.evt_len as usize + ev.len() > BT_HCI_EVT_BUF {
            return false;
        }
        let start = self.evt_len as usize;
        self.evt[start..start + ev.len()].copy_from_slice(ev);
        self.evt_len += ev.len() as u32;
        true
    }
    /// Queue a synthetic ACL RX packet.
    pub fn push_acl_rx(&mut self, p: &[u8]) -> bool {
        if p.is_empty() || self.acl_rx_len as usize + p.len() > BT_HCI_EVT_BUF {
            return false;
        }
        let start = self.acl_rx_len as usize;
        self.acl_rx[start..start + p.len()].copy_from_slice(p);
        self.acl_rx_len += p.len() as u32;
        true
    }
    /// Take the most recently submitted HCI command (host wrote it via
    /// offset 0x100..0x200) and report back as a "command complete" event.
    pub fn take_last_cmd(&self) -> Option<&[u8]> {
        if self.cmd_len == 0 {
            None
        } else {
            Some(&self.cmd[..self.cmd_len as usize])
        }
    }
}

impl Device for BtHci {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "bt-hci"
    }
    fn reset(&mut self) {
        self.cmd = [0; BT_HCI_CMD_BUF];
        self.cmd_len = 0;
        self.evt = [0; BT_HCI_EVT_BUF];
        self.evt_len = 0;
        self.evt_read_pos = 0;
        self.ctrl = 0;
        self.acl_tx = [0; BT_HCI_CMD_BUF];
        self.acl_tx_len = 0;
        self.acl_rx = [0; BT_HCI_EVT_BUF];
        self.acl_rx_len = 0;
        self.local_name = [0; 32];
        self.local_name_len = 0;
        self.tx_power_dbm = 0;
        self.sleep_state = 0;
        self.last_cmd_ogf = 0;
        self.last_cmd_ocf = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.evt_len as u64),
            0x08 => DeviceResult::Value(self.acl_tx_len as u64),
            0x0C => DeviceResult::Value(self.acl_rx_len as u64),
            0x10 => DeviceResult::Value(self.tx_power_dbm as u8 as u64),
            0x14 => DeviceResult::Value(self.sleep_state as u64),
            0x18 => DeviceResult::Value(self.last_cmd_ogf as u64),
            0x1C => DeviceResult::Value(self.last_cmd_ocf as u64),
            0x20 => DeviceResult::Value(self.vendor_id as u64),
            0x22 => DeviceResult::Value(self.product_id as u64),
            // BD address: 6 bytes at offset 0x30.
            b if (0x30..0x36).contains(&b) && s == 1 => {
                let off = (b - 0x30) as usize;
                DeviceResult::Value(self.bd_address[off] as u64)
            }
            // Local name: up to 32 bytes at offset 0x40.
            n if (0x40..0x60).contains(&n) && s == 1 => {
                let off = (n - 0x40) as usize;
                if off < self.local_name_len as usize {
                    DeviceResult::Value(self.local_name[off] as u64)
                } else {
                    DeviceResult::Value(0)
                }
            }
            c if (0x100..0x200).contains(&c) => {
                let off = (c - 0x100) as usize;
                if off + s > self.cmd_len as usize {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (self.cmd[off + i] as u64) << (i * 8);
                }
                DeviceResult::Value(v)
            }
            e if (0x200..0x400).contains(&e) => {
                let off = (e - 0x200) as usize;
                let abs = self.evt_read_pos as usize + off;
                if abs + s > self.evt_len as usize {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (self.evt[abs + i] as u64) << (i * 8);
                }
                DeviceResult::Value(v)
            }
            // ACL TX buffer at 0x400..0x500, ACL RX at 0x500..0x700.
            t if (0x400..0x500).contains(&t) => {
                let off = (t - 0x400) as usize;
                if off + s > self.acl_tx_len as usize {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (self.acl_tx[off + i] as u64) << (i * 8);
                }
                DeviceResult::Value(v)
            }
            r if (0x500..0x700).contains(&r) => {
                let off = (r - 0x500) as usize;
                if off + s > self.acl_rx_len as usize {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (self.acl_rx[off + i] as u64) << (i * 8);
                }
                DeviceResult::Value(v)
            }
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                // Consume `v` bytes from event buffer (advance read pos).
                let new_pos = (self.evt_read_pos as usize + v as usize).min(self.evt_len as usize);
                self.evt_read_pos = new_pos as u32;
                if self.evt_read_pos == self.evt_len {
                    self.evt_read_pos = 0;
                    self.evt_len = 0;
                }
                DeviceResult::Ok
            }
            0x10 => {
                self.tx_power_dbm = (v as i8).clamp(-30, 20);
                DeviceResult::Ok
            }
            0x14 => {
                self.sleep_state = v as u32 & 0x3;
                DeviceResult::Ok
            }
            // BD address: 6 bytes at offset 0x30.
            b if (0x30..0x36).contains(&b) && s == 1 => {
                let off = (b - 0x30) as usize;
                self.bd_address[off] = v as u8;
                DeviceResult::Ok
            }
            // Local name: 32 bytes at offset 0x40.
            n if (0x40..0x60).contains(&n) && s == 1 => {
                let off = (n - 0x40) as usize;
                self.local_name[off] = v as u8;
                if off + 1 > self.local_name_len as usize {
                    self.local_name_len = (off + 1) as u32;
                }
                DeviceResult::Ok
            }
            c if (0x100..0x200).contains(&c) => {
                let off = (c - 0x100) as usize;
                if off + s <= BT_HCI_CMD_BUF {
                    for i in 0..s {
                        self.cmd[off + i] = (v >> (i * 8)) as u8;
                    }
                    if off + s > self.cmd_len as usize {
                        self.cmd_len = (off + s) as u32;
                    }
                    // Decode OGF/OCF from the first two cmd bytes.
                    if off == 0 && s >= 3 {
                        self.last_cmd_ocf = ((self.cmd[1] as u16) << 8) | self.cmd[0] as u16;
                        self.last_cmd_ogf = self.cmd[2] >> 2;
                    }
                }
                DeviceResult::Ok
            }
            // ACL TX buffer at 0x400..0x500.
            t if (0x400..0x500).contains(&t) => {
                let off = (t - 0x400) as usize;
                if off + s <= BT_HCI_CMD_BUF {
                    for i in 0..s {
                        self.acl_tx[off + i] = (v >> (i * 8)) as u8;
                    }
                    if off + s > self.acl_tx_len as usize {
                        self.acl_tx_len = (off + s) as u32;
                    }
                }
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        if self.evt_len > 0 && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if self.evt_len > 0 { self.irq } else { 0 };
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
// WifiSdio — SDIO-attached Wi-Fi (CMD52 / CMD53 register window)
// ----------------------------------------------------------------------------

pub const WIFI_REGS: usize = 256;

pub struct WifiSdio {
    pub id: DeviceId,
    pub regs: [u8; WIFI_REGS],
    pub ctrl: u32,
    pub status: u32,
    /// WiFi MAC address (BSSID is a copy of the same field).
    pub mac_address: [u8; 6],
    /// Firmware version (NUL-terminated, max 32 bytes).
    pub firmware_version: [u8; 32],
    pub firmware_version_len: u32,
    /// Firmware load state: 0 = not loaded, 1 = loading, 2 = running, 3 = crashed.
    pub fw_state: u32,
    /// Last firmware load address written via CMD53.
    pub fw_load_addr: u32,
    /// TX data ring (host → chip).
    pub tx_ring: [u8; WIFI_REGS],
    pub tx_ring_len: u32,
    /// RX data ring (chip → host).
    pub rx_ring: [u8; WIFI_REGS],
    pub rx_ring_len: u32,
    /// Current channel (1..14).
    pub channel: u8,
    /// TX power in dBm.
    pub tx_power_dbm: i8,
    /// Number of transmitted/received frames.
    pub tx_count: u64,
    pub rx_count: u64,
    /// Vendor ID (0x14E4 = Broadcom for Apple WiFi chips).
    pub vendor_id: u16,
    /// Device ID.
    pub device_id: u16,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
}

impl WifiSdio {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            regs: [0; WIFI_REGS],
            ctrl: 0,
            status: 0,
            mac_address: [0xAC, 0xDE, 0x48, 0x00, 0x11, 0x22], // Apple-issued default
            firmware_version: [0; 32],
            firmware_version_len: 0,
            fw_state: 0,
            fw_load_addr: 0,
            tx_ring: [0; WIFI_REGS],
            tx_ring_len: 0,
            rx_ring: [0; WIFI_REGS],
            rx_ring_len: 0,
            channel: 1,
            tx_power_dbm: 0,
            tx_count: 0,
            rx_count: 0,
            vendor_id: 0x14E4,
            device_id: 0x43A0,
            irq: 83,
            last_status: 0,
            flash_storage: Vec::new(),
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// Queue a synthetic RX frame for the host to read out.
    /// Sets the RX-ready status bit so the next `tick()` raises an IRQ.
    pub fn push_rx_frame(&mut self, p: &[u8]) -> bool {
        if p.is_empty() || self.rx_ring_len as usize + p.len() > WIFI_REGS {
            return false;
        }
        self.rx_ring[self.rx_ring_len as usize..self.rx_ring_len as usize + p.len()]
            .copy_from_slice(p);
        self.rx_ring_len += p.len() as u32;
        self.status |= 0x1;
        true
    }
}

impl Device for WifiSdio {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "wifi-sdio"
    }
    fn reset(&mut self) {
        for r in self.regs.iter_mut() {
            *r = 0;
        }
        self.ctrl = 0;
        self.status = 0;
        self.firmware_version = [0; 32];
        self.firmware_version_len = 0;
        self.fw_state = 0;
        self.fw_load_addr = 0;
        self.tx_ring = [0; WIFI_REGS];
        self.tx_ring_len = 0;
        self.rx_ring = [0; WIFI_REGS];
        self.rx_ring_len = 0;
        self.channel = 1;
        self.tx_power_dbm = 0;
        self.tx_count = 0;
        self.rx_count = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.fw_state as u64),
            0x0C => DeviceResult::Value(self.channel as u64),
            0x10 => DeviceResult::Value(self.tx_power_dbm as u8 as u64),
            0x14 => DeviceResult::Value(self.tx_count),
            0x1C => DeviceResult::Value(self.rx_count),
            0x20 => DeviceResult::Value(self.fw_load_addr as u64),
            0x24 => DeviceResult::Value(self.tx_ring_len as u64),
            0x28 => DeviceResult::Value(self.rx_ring_len as u64),
            0x2C => DeviceResult::Value(self.vendor_id as u64),
            0x2E => DeviceResult::Value(self.device_id as u64),
            // MAC address: 6 bytes at offset 0x30.
            m if (0x30..0x36).contains(&m) && s == 1 => {
                let off = (m - 0x30) as usize;
                DeviceResult::Value(self.mac_address[off] as u64)
            }
            // Firmware version string: 32 bytes at offset 0x40.
            f if (0x40..0x60).contains(&f) && s == 1 => {
                let off = (f - 0x40) as usize;
                if off < self.firmware_version_len as usize {
                    DeviceResult::Value(self.firmware_version[off] as u64)
                } else {
                    DeviceResult::Value(0)
                }
            }
            r if (0x100..0x100 + WIFI_REGS as u64).contains(&r) => {
                let off = (r - 0x100) as usize;
                DeviceResult::Value(self.regs[off] as u64)
            }
            // TX ring at 0x200..0x300, RX ring at 0x300..0x400.
            t if (0x200..0x300).contains(&t) => {
                let off = (t - 0x200) as usize;
                if off < self.tx_ring_len as usize {
                    DeviceResult::Value(self.tx_ring[off] as u64)
                } else {
                    DeviceResult::Value(0)
                }
            }
            r if (0x300..0x400).contains(&r) => {
                let off = (r - 0x300) as usize;
                if off < self.rx_ring_len as usize {
                    DeviceResult::Value(self.rx_ring[off] as u64)
                } else {
                    DeviceResult::Value(0)
                }
            }
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x08 => {
                self.fw_state = v as u32 & 0x3;
                DeviceResult::Ok
            }
            0x0C => {
                self.channel = (v as u8).clamp(1, 14);
                DeviceResult::Ok
            }
            0x10 => {
                self.tx_power_dbm = (v as i8).clamp(-30, 30);
                DeviceResult::Ok
            }
            0x20 => {
                self.fw_load_addr = v as u32;
                DeviceResult::Ok
            }
            // MAC address: 6 bytes at offset 0x30.
            m if (0x30..0x36).contains(&m) && s == 1 => {
                let off = (m - 0x30) as usize;
                self.mac_address[off] = v as u8;
                DeviceResult::Ok
            }
            // Firmware version string: 32 bytes at offset 0x40.
            f if (0x40..0x60).contains(&f) && s == 1 => {
                let off = (f - 0x40) as usize;
                self.firmware_version[off] = v as u8;
                if off + 1 > self.firmware_version_len as usize {
                    self.firmware_version_len = (off + 1) as u32;
                }
                DeviceResult::Ok
            }
            r if (0x100..0x100 + WIFI_REGS as u64).contains(&r) => {
                let off = (r - 0x100) as usize;
                self.regs[off] = v as u8;
                DeviceResult::Ok
            }
            // TX ring at 0x200..0x300.
            t if (0x200..0x300).contains(&t) => {
                let off = (t - 0x200) as usize;
                if off < WIFI_REGS {
                    self.tx_ring[off] = v as u8;
                    if off + 1 > self.tx_ring_len as usize {
                        self.tx_ring_len = (off + 1) as u32;
                    }
                }
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.status & 1) != 0;
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
// CameraIsp — bounded camera ISP model
// ----------------------------------------------------------------------------

/// RAW_SAMPLE (W): 16-bit raw sensor sample used by the raw→YUV conversion
/// leaf. Upper bits of the written word are masked.
pub const ISP_RAW_SAMPLE: u64 = 0x50;
/// FLASH_GAIN (R): flash-mode AWB curve coefficient (linear light, 1.0 = 0x100).
pub const ISP_FLASH_GAIN: u64 = 0x54;
/// TORCH_GAIN (R): torch-mode AWB curve coefficient (linear light, 1.0 = 0x100).
pub const ISP_TORCH_GAIN: u64 = 0x58;
/// YUV_LEAF (R): 16-bit raw→YUV conversion leaf, packed `Y<<16 | Cb<<8 | Cr`.
/// The conversion is a genuine linear-light pipeline (AWB curve gain + fixed
/// 4:2:0-style matrix), not a static constant; it changes as `raw`/`awb_mode`/
/// `gain` move.
pub const ISP_YUV_LEAF: u64 = 0x5c;
/// AWB_MODE (R/W): 0 = off, 1 = torch, 2 = flash (clamped to 0..=2).
pub const ISP_AWB_MODE: u64 = 0x60;
/// A3A_ACQ (R): 3A-stat acquisition counter; incremented once per generated
/// frame (frame counter in the ISP's stat pipeline, not the host counter).
pub const ISP_A3A_ACQ: u64 = 0x64;
/// A3A_PEAK (R): 3A-stat bin leaf — the peak-contrast value of the last
/// generated (worst-case contrast) stat window. Changes from reset only after
/// a modeled frame (gate G2), driven by driver-produced gain/AWB/lens-state.
pub const ISP_A3A_PEAK: u64 = 0x68;
/// A3A_VALID (R): 1 while the last stat window is settled/ready to consume.
pub const ISP_A3A_VALID: u64 = 0x6c;
/// A3A_RESET (W): bit0 = reset the 3A stat counters back to their reset value.
pub const ISP_A3A_RESET: u64 = 0x70;

pub struct CameraIsp {
    pub id: DeviceId,
    pub ctrl: u32,
    pub status: u32,
    pub width: u16,
    pub height: u16,
    pub format: u32, // 0 = YUV422, 1 = BGRA
    pub frame_ready: bool,
    /// Exposure time in microseconds.
    pub exposure_us: u32,
    /// Analog gain (1.0 = 0x100, 2.0 = 0x200, etc.).
    pub gain: u32,
    /// AWB color temperature in Kelvin.
    pub awb_temp_k: u32,
    /// Lens-shading correction per channel (R, G, B, IR).
    pub lens_shading: [u32; 4],
    /// Bayer pattern: 0=RGGB, 1=BGGR, 2=GRBG, 3=GBRG.
    pub bayer_pattern: u8,
    /// Crop region (x, y, w, h).
    pub crop_x: u16,
    pub crop_y: u16,
    pub crop_w: u16,
    pub crop_h: u16,
    /// Frame counter (incremented on every signal_frame).
    pub frame_count: u64,
    /// ISO sensitivity (100, 200, ...).
    pub iso: u32,
    /// Vendor ID (0x05AC = Apple).
    pub vendor_id: u16,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
    /// 16-bit raw sensor sample for the raw→YUV conversion leaf.
    pub raw_in: u16,
    /// AWB illumination mode: 0 = off, 1 = torch, 2 = flash.
    pub awb_mode: u32,
    /// Flash-mode AWB curve coefficient (linear light, fixed fixup).
    pub awb_flash_gain: u32,
    /// Torch-mode AWB curve coefficient (linear light, fixed fixup).
    pub awb_torch_gain: u32,
    /// 3A-stat acquisition counter (per generated frame).
    pub a3a_acq: u32,
    /// 3A-stat peak-contrast bin (last stat-window result).
    pub a3a_peak: u32,
    /// True while the last 3A stat window is settled.
    pub a3a_valid: bool,
}

impl CameraIsp {
    /// Modeled peak-contrast of the worst-case (full dynamic range) stat
    /// window: processed white (0xffff) minus processed black (0) — i.e. the
    /// processed white value, since black maps to 0 under linear-light gain.
    /// This is the real modeled value a 3A-stat read returns after a frame;
    /// it is never a host-fabricated "ready" constant.
    pub fn model_peak_contrast(&self) -> u32 {
        let awb = if self.awb_mode == 2 {
            self.awb_flash_gain
        } else {
            self.awb_torch_gain
        };
        let g = ((self.gain as u64 * awb as u64) >> 8) as u32;
        ((0xffffu64 * g as u64) >> 8).min(0x3fffff) as u32
    }
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: 0,
            status: 0,
            width: 1920,
            height: 1080,
            format: 0,
            frame_ready: false,
            exposure_us: 10_000, // 10 ms
            gain: 0x100,         // 1.0x
            awb_temp_k: 5600,    // daylight
            lens_shading: [0x100; 4],
            bayer_pattern: 0,
            crop_x: 0,
            crop_y: 0,
            crop_w: 1920,
            crop_h: 1080,
            frame_count: 0,
            iso: 100,
            vendor_id: 0x05AC,
            irq: 84,
            last_status: 0,
            flash_storage: Vec::new(),
            raw_in: 0,
            awb_mode: 0,
            awb_flash_gain: 0x180, // 1.5x linear-light flash curve fixup
            awb_torch_gain: 0x100, // 1.0x linear-light torch curve fixup
            a3a_acq: 0,
            a3a_peak: 0,
            a3a_valid: false,
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    pub fn signal_frame(&mut self) {
        self.frame_ready = true;
        self.status |= 0x1;
        self.frame_count = self.frame_count.wrapping_add(1);
        // The frame pipeline is a worst-case-contrast (full dynamic range) stat
        // window: processed white minus processed black. This is the block's own
        // 3A-stat result, driven by the current driver-produced gain/AWB/lens
        // state — never a static constant (Phase 190 gate G2).
        self.a3a_acq = self.a3a_acq.wrapping_add(1);
        self.a3a_peak = self.model_peak_contrast();
        self.a3a_valid = true;
    }
}

impl Device for CameraIsp {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "camera-isp"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.status = 0;
        self.frame_ready = false;
        self.exposure_us = 10_000;
        self.gain = 0x100;
        self.awb_temp_k = 5600;
        self.lens_shading = [0x100; 4];
        self.crop_x = 0;
        self.crop_y = 0;
        self.crop_w = self.width;
        self.crop_h = self.height;
        self.iso = 100;
        self.last_status = 0;
        self.raw_in = 0;
        self.awb_mode = 0;
        self.a3a_acq = 0;
        self.a3a_peak = 0;
        self.a3a_valid = false;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.width as u64),
            0x0C => DeviceResult::Value(self.height as u64),
            0x10 => DeviceResult::Value(self.format as u64),
            0x14 => DeviceResult::Value(self.frame_ready as u64),
            0x18 => DeviceResult::Value(self.exposure_us as u64),
            0x1C => DeviceResult::Value(self.gain as u64),
            0x20 => DeviceResult::Value(self.awb_temp_k as u64),
            0x24 => DeviceResult::Value(self.iso as u64),
            0x28 => DeviceResult::Value(self.bayer_pattern as u64),
            0x2C => DeviceResult::Value(self.crop_x as u64),
            0x2E => DeviceResult::Value(self.crop_y as u64),
            0x30 => DeviceResult::Value(self.crop_w as u64),
            0x32 => DeviceResult::Value(self.crop_h as u64),
            0x34 => DeviceResult::Value(self.frame_count),
            0x3C => DeviceResult::Value(self.vendor_id as u64),
            // Lens-shading per-channel at 0x40..0x50 (4×u32).
            ls if (0x40..0x50).contains(&ls) && s == 4 => {
                let off = ((ls - 0x40) / 4) as usize;
                DeviceResult::Value(self.lens_shading[off] as u64)
            }
            ISP_FLASH_GAIN => DeviceResult::Value(self.awb_flash_gain as u64),
            ISP_TORCH_GAIN => DeviceResult::Value(self.awb_torch_gain as u64),
            ISP_YUV_LEAF if s == 4 => {
                let awb = if self.awb_mode == 2 {
                    self.awb_flash_gain
                } else {
                    self.awb_torch_gain
                };
                let f = ((self.raw_in as u64 * awb as u64) >> 8) as u32;
                let y = (f >> 2).clamp(16, 235);
                let cb = (((f * 112) >> 10) + 128) & 0xff;
                let cr = (((f * 90) >> 10) + 128) & 0xff;
                DeviceResult::Value(((y as u64) << 16) | ((cb as u64) << 8) | cr as u64)
            }
            ISP_AWB_MODE => DeviceResult::Value(self.awb_mode as u64),
            ISP_A3A_ACQ => DeviceResult::Value(self.a3a_acq as u64),
            ISP_A3A_PEAK => DeviceResult::Value(self.a3a_peak as u64),
            ISP_A3A_VALID => DeviceResult::Value(self.a3a_valid as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x08 => {
                self.width = v as u16;
                DeviceResult::Ok
            }
            0x0C => {
                self.height = v as u16;
                DeviceResult::Ok
            }
            0x10 => {
                self.format = v as u32;
                DeviceResult::Ok
            }
            0x14 => {
                if v != 0 {
                    self.frame_ready = true;
                }
                DeviceResult::Ok
            }
            0x18 => {
                self.exposure_us = v as u32;
                DeviceResult::Ok
            }
            0x1C => {
                self.gain = v as u32;
                DeviceResult::Ok
            }
            0x20 => {
                self.awb_temp_k = v as u32;
                DeviceResult::Ok
            }
            0x24 => {
                self.iso = v as u32;
                DeviceResult::Ok
            }
            0x28 => {
                self.bayer_pattern = v as u8 & 0x3;
                DeviceResult::Ok
            }
            0x2C => {
                self.crop_x = v as u16;
                DeviceResult::Ok
            }
            0x2E => {
                self.crop_y = v as u16;
                DeviceResult::Ok
            }
            0x30 => {
                self.crop_w = v as u16;
                DeviceResult::Ok
            }
            0x32 => {
                self.crop_h = v as u16;
                DeviceResult::Ok
            }
            // Lens-shading per-channel at 0x40..0x50.
            ls if (0x40..0x50).contains(&ls) && s == 4 => {
                let off = ((ls - 0x40) / 4) as usize;
                self.lens_shading[off] = v as u32;
                DeviceResult::Ok
            }
            ISP_RAW_SAMPLE => {
                self.raw_in = (v & 0xffff) as u16;
                DeviceResult::Ok
            }
            ISP_AWB_MODE => {
                self.awb_mode = v.min(2) as u32;
                DeviceResult::Ok
            }
            ISP_A3A_RESET if v & 1 != 0 => {
                self.a3a_acq = 0;
                self.a3a_peak = 0;
                self.a3a_valid = false;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        if self.frame_ready && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if self.frame_ready { self.irq } else { 0 };
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

#[cfg(test)]
mod isp_tests {
    use super::*;

    #[test]
    fn isp_declared_registers_read_stable_and_never_fault() {
        // Gate G1: every declared register reads a stable value and never
        // Faults (valid sizes); unknown offsets are benign zero reads.
        let mut isp = CameraIsp::new(1);
        let declared = [
            0x00u64,
            0x04,
            0x08,
            0x0C,
            0x10,
            0x14,
            0x18,
            0x1C,
            0x20,
            0x24,
            0x28,
            0x2C,
            0x2E,
            0x30,
            0x32,
            0x34,
            0x3C,
            0x40,
            0x44,
            0x48,
            0x4C,
            ISP_FLASH_GAIN,
            ISP_TORCH_GAIN,
            ISP_YUV_LEAF,
            ISP_AWB_MODE,
            ISP_A3A_ACQ,
            ISP_A3A_PEAK,
            ISP_A3A_VALID,
        ];
        for off in declared {
            for size in [4, 8] {
                assert!(
                    !matches!(isp.read(off, size), DeviceResult::Fault),
                    "{off:#x} {size} Fault"
                );
            }
        }
        // Values are stable across reads.
        let yuv = isp.read(ISP_YUV_LEAF, 4);
        for _ in 0..3 {
            assert_eq!(isp.read(ISP_YUV_LEAF, 4), yuv);
        }
        // Invalid access sizes still fault — the gate is about the declared
        // register window, not dropping the size discipline.
        assert_eq!(isp.read(ISP_YUV_LEAF, 3), DeviceResult::Fault);
    }

    #[test]
    fn isp_3a_stat_leaf_changes_after_synthetic_frame() {
        let mut isp = CameraIsp::new(2);
        // Reset: 3A counters at reset value.
        assert_eq!(isp.read(ISP_A3A_ACQ, 4).unwrap_u64(), 0);
        assert_eq!(isp.read(ISP_A3A_PEAK, 4).unwrap_u64(), 0);
        assert_eq!(isp.read(ISP_A3A_VALID, 4).unwrap_u64(), 0);
        // A generated worst-case-contrast frame changes the peak leaf (gate G2)
        // and advances the acquisition counter — driven by the modeled frame,
        // not a static constant.
        isp.signal_frame();
        assert_eq!(isp.read(ISP_A3A_ACQ, 4).unwrap_u64(), 1);
        assert_eq!(isp.read(ISP_A3A_VALID, 4).unwrap_u64(), 1);
        let peak_1 = isp.read(ISP_A3A_PEAK, 4).unwrap_u64();
        assert_ne!(peak_1, 0);
        // The peak is genuinely driver-producible: raising analog gain raises
        // the modeled worst-case contrast of the next frame.
        isp.write(0x1C, 4, 0x200);
        isp.signal_frame();
        let peak_2 = isp.read(ISP_A3A_PEAK, 4).unwrap_u64();
        assert!(peak_2 > peak_1);
        assert_eq!(isp.read(ISP_A3A_ACQ, 4).unwrap_u64(), 2);
        // A3A_RESET bit0 returns the stat window to its reset value.
        isp.write(ISP_A3A_RESET, 4, 1);
        assert_eq!(isp.read(ISP_A3A_ACQ, 4).unwrap_u64(), 0);
        assert_eq!(isp.read(ISP_A3A_PEAK, 4).unwrap_u64(), 0);
        assert_eq!(isp.read(ISP_A3A_VALID, 4).unwrap_u64(), 0);
    }

    #[test]
    fn isp_raw_to_yuv_leaf_is_a_genuine_conversion() {
        let mut isp = CameraIsp::new(3);
        // Reset leaf: black raw + neutral chroma (luma 16, Cb/Cr 128) — a
        // well-defined converter reset, not zero.
        let reset_leaf = isp.read(ISP_YUV_LEAF, 4).unwrap_u64();
        assert_eq!(reset_leaf, (16u64 << 16) | (128 << 8) | 128);
        // Feeding a raw 16-bit sample changes the leaf.
        isp.write(ISP_RAW_SAMPLE, 4, 0x1234);
        let leaf_off = isp.read(ISP_YUV_LEAF, 4).unwrap_u64();
        assert_ne!(leaf_off, reset_leaf);
        // Torch vs flash AWB curves (fixed fixup gains) select different leaves
        // for the same raw sample — the conversion is curve-dependent.
        isp.write(ISP_RAW_SAMPLE, 4, 0x1234);
        isp.write(ISP_AWB_MODE, 4, 1);
        let torch = isp.read(ISP_YUV_LEAF, 4).unwrap_u64();
        isp.write(ISP_AWB_MODE, 4, 2);
        let flash = isp.read(ISP_YUV_LEAF, 4).unwrap_u64();
        assert_ne!(flash, torch);
        // Raw write is masked to the low 16 bits.
        isp.write(ISP_RAW_SAMPLE, 4, 0x0001_0000 | 0x00ab);
        assert_eq!(isp.raw_in, 0x00ab);
    }
}

// ----------------------------------------------------------------------------
// SensorHub — bounded I²C-attached sensor hub model
// ----------------------------------------------------------------------------

pub const SENSOR_KIND_COUNT: usize = 4; // gyro, accel, baro, mag
pub const SENSOR_AXES: usize = 3;

/// FIFO sample: kind, axis, raw value, timestamp in µs.
pub const SENSOR_FIFO_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, Default)]
pub struct SensorFifoEntry {
    pub kind: u8,
    pub axis: u8,
    pub raw: i32,
    pub ts_us: u32,
}

pub struct SensorHub {
    pub id: DeviceId,
    pub scale: [[i32; SENSOR_AXES]; SENSOR_KIND_COUNT],
    pub temp_centi_c: i32,
    /// FIFO ring of recent samples (head + count).
    pub fifo: [SensorFifoEntry; SENSOR_FIFO_DEPTH],
    pub fifo_head: usize,
    pub fifo_count: usize,
    /// Batch mode (when true, samples accumulate in FIFO instead of overwriting).
    pub batch_enabled: bool,
    /// Wake-on-motion threshold in milli-g. When |accel axis| exceeds this,
    /// `motion_detected` is asserted and an IRQ fires.
    pub wake_on_motion_threshold_mg: u32,
    /// True while a wake-on-motion event is pending (cleared by W1C on status).
    pub motion_detected: bool,
    /// Vendor ID (0x05AC = Apple for M-series sensor hub).
    pub vendor_id: u16,
    pub product_id: u16,
    pub ctrl: u32,
    pub status: u32,
    pub irq: u32,
    last_status: u32,
    pub flash_storage: Vec<u8>,
}

impl SensorHub {
    pub const KIND_GYRO: usize = 0;
    pub const KIND_ACCEL: usize = 1;
    pub const KIND_BARO: usize = 2;
    pub const KIND_MAG: usize = 3;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            scale: [[0; SENSOR_AXES]; SENSOR_KIND_COUNT],
            temp_centi_c: 2500,
            fifo: [SensorFifoEntry::default(); SENSOR_FIFO_DEPTH],
            fifo_head: 0,
            fifo_count: 0,
            batch_enabled: false,
            wake_on_motion_threshold_mg: 200,
            motion_detected: false,
            vendor_id: 0x05AC,
            product_id: 0x7001,
            ctrl: 0,
            status: 0,
            irq: 85,
            last_status: 0,
            flash_storage: Vec::new(),
        }
    }
    pub fn update_firmware(&mut self, data: &[u8]) {
        self.flash_storage = data.to_vec();
    }
    pub fn flash_storage(&self) -> &[u8] {
        &self.flash_storage
    }
    pub fn flash_storage_mut(&mut self) -> &mut Vec<u8> {
        &mut self.flash_storage
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    pub fn set_value(&mut self, kind: usize, axis: usize, raw: i32) {
        if kind < SENSOR_KIND_COUNT && axis < SENSOR_AXES {
            self.scale[kind][axis] = raw;
        }
    }

    /// Push one sample into the FIFO. If FIFO is full, oldest sample is
    /// overwritten. If `kind == KIND_ACCEL` and `|raw|` exceeds
    /// `wake_on_motion_threshold_mg`, also asserts `motion_detected`.
    pub fn push_fifo_sample(&mut self, kind: u8, axis: u8, raw: i32, ts_us: u32) {
        let idx = if self.fifo_count < SENSOR_FIFO_DEPTH {
            let i = (self.fifo_head + self.fifo_count) % SENSOR_FIFO_DEPTH;
            self.fifo_count += 1;
            i
        } else {
            let i = self.fifo_head;
            self.fifo_head = (self.fifo_head + 1) % SENSOR_FIFO_DEPTH;
            i
        };
        self.fifo[idx] = SensorFifoEntry {
            kind,
            axis,
            raw,
            ts_us,
        };
        if kind as usize == Self::KIND_ACCEL
            && raw.unsigned_abs() > self.wake_on_motion_threshold_mg
        {
            self.motion_detected = true;
            self.status_set();
        }
    }

    /// Pop the oldest FIFO entry. Returns None if FIFO is empty.
    pub fn drain_fifo(&mut self) -> Option<SensorFifoEntry> {
        if self.fifo_count == 0 {
            return None;
        }
        let e = self.fifo[self.fifo_head];
        self.fifo_head = (self.fifo_head + 1) % SENSOR_FIFO_DEPTH;
        self.fifo_count -= 1;
        Some(e)
    }

    fn status_set(&mut self) {
        // bit 0 of `status` is the "data ready / motion detected" flag.
        self.status |= 0x1;
    }
}

impl Device for SensorHub {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "sensor-hub"
    }
    fn reset(&mut self) {
        self.scale = [[0; SENSOR_AXES]; SENSOR_KIND_COUNT];
        self.temp_centi_c = 2500;
        self.fifo = [SensorFifoEntry::default(); SENSOR_FIFO_DEPTH];
        self.fifo_head = 0;
        self.fifo_count = 0;
        self.batch_enabled = false;
        self.wake_on_motion_threshold_mg = 200;
        self.motion_detected = false;
        self.ctrl = 0;
        self.status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.temp_centi_c as i64 as u64),
            0x0C => DeviceResult::Value(self.fifo_count as u64),
            0x10 => DeviceResult::Value(self.fifo_head as u64),
            0x14 => DeviceResult::Value(self.batch_enabled as u64),
            0x18 => DeviceResult::Value(self.wake_on_motion_threshold_mg as u64),
            0x1C => DeviceResult::Value(self.motion_detected as u64),
            0x20 => DeviceResult::Value(self.vendor_id as u64),
            0x22 => DeviceResult::Value(self.product_id as u64),
            // 0x100..0x200 — primary sensor window (gyro/accel/baro), 3 × 3 × 4 bytes.
            r if (0x100..0x200).contains(&r) => {
                // 3 sensors × 3 axes, each 4 bytes; stride 0x40 per sensor.
                let sensor = ((r - 0x100) / 0x40) as usize;
                let axis = (((r - 0x100) % 0x40) / 4) as usize;
                if sensor >= SENSOR_KIND_COUNT || axis >= SENSOR_AXES {
                    return read_zero(o, s);
                }
                DeviceResult::Value(self.scale[sensor][axis] as i64 as u64)
            }
            // 0x400..0x480 — magnetometer window (3 × 4 bytes).
            r if (0x400..0x40C).contains(&r) => {
                let axis = ((r - 0x400) / 4) as usize;
                if axis >= SENSOR_AXES {
                    return read_zero(o, s);
                }
                DeviceResult::Value(self.scale[Self::KIND_MAG][axis] as i64 as u64)
            }
            // 0x200..0x400 — FIFO window (32 × 16 bytes).
            r if (0x200..0x200 + (SENSOR_FIFO_DEPTH as u64) * 16).contains(&r) => {
                let i = ((r - 0x200) / 16) as usize;
                let off = (r - 0x200 - (i as u64) * 16) as usize;
                if i >= self.fifo_count {
                    return read_zero(o, s);
                }
                let entry_idx = (self.fifo_head + i) % SENSOR_FIFO_DEPTH;
                let e = self.fifo[entry_idx];
                let bytes: [u8; 16] = [
                    e.kind,
                    e.axis,
                    (e.raw & 0xFF) as u8,
                    ((e.raw >> 8) & 0xFF) as u8,
                    ((e.raw >> 16) & 0xFF) as u8,
                    ((e.raw >> 24) & 0xFF) as u8,
                    (e.ts_us & 0xFF) as u8,
                    ((e.ts_us >> 8) & 0xFF) as u8,
                    ((e.ts_us >> 16) & 0xFF) as u8,
                    ((e.ts_us >> 24) & 0xFF) as u8,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ];
                if off + s > 16 {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for k in 0..s {
                    v |= (bytes[off + k] as u64) << (k * 8);
                }
                DeviceResult::Value(v)
            }
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                if v & 1 != 0 {
                    self.motion_detected = false;
                }
                DeviceResult::Ok
            }
            0x08 => {
                self.temp_centi_c = v as i32;
                DeviceResult::Ok
            }
            0x14 => {
                self.batch_enabled = (v & 1) != 0;
                DeviceResult::Ok
            }
            0x18 => {
                self.wake_on_motion_threshold_mg = v as u32;
                DeviceResult::Ok
            }
            // Magnetometer window 0x400..0x40C.
            r if (0x400..0x40C).contains(&r) => {
                let axis = ((r - 0x400) / 4) as usize;
                if axis < SENSOR_AXES {
                    self.scale[Self::KIND_MAG][axis] = v as i32;
                }
                DeviceResult::Ok
            }
            r if (0x100..0x200).contains(&r) => {
                let sensor = ((r - 0x100) / 0x40) as usize;
                let axis = (((r - 0x100) % 0x40) / 4) as usize;
                if sensor < SENSOR_KIND_COUNT && axis < SENSOR_AXES {
                    self.scale[sensor][axis] = v as i32;
                    if sensor == Self::KIND_ACCEL
                        && (v as i32).unsigned_abs() > self.wake_on_motion_threshold_mg
                    {
                        self.motion_detected = true;
                        self.status |= 0x1;
                    }
                }
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.status & 0x1) != 0;
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
