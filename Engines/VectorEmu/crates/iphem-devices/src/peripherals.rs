//! Peripherals Breadth (Phase 159)
//!
//! Register-level functional models for the peripherals a SecureROM/iBoot
//! probe expects. None of these are cycle-accurate; they only attempt to
//! return architecturally plausible values and to deliver interrupts when
//! the simulated hardware would.

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
// GPIO — 8 banks of 32 pins, each pin has DIR, IN, OUT, CTRL, IRQ.
// ----------------------------------------------------------------------------

pub const GPIO_BANKS: usize = 8;
pub const GPIO_PINS_PER_BANK: usize = 32;

#[derive(Debug, Clone, Copy, Default)]
pub struct GpioPin {
    pub dir: u8,    // 0 = input, 1 = output
    pub out: u8,    // last driven value
    pub in_val: u8, // last sampled input
    pub pull: u8,   // 0=none, 1=up, 2=down
    pub drive: u8,  // 0..3
    pub irq_en: bool,
    pub irq_level: bool, // 0 = edge, 1 = level
    pub irq_pending: bool,
}

pub struct Gpio {
    pub id: DeviceId,
    pub banks: [Vec<GpioPin>; GPIO_BANKS],
    pub bank_irq_en: [u32; GPIO_BANKS],
    pub bank_irq_status: [u32; GPIO_BANKS],
    pub irq: u32,
    last_status: u32,
}

impl Gpio {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            banks: [(); GPIO_BANKS].map(|_| vec![GpioPin::default(); GPIO_PINS_PER_BANK]),
            bank_irq_en: [0; GPIO_BANKS],
            bank_irq_status: [0; GPIO_BANKS],
            irq: 32,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    /// Inject an external level on a pin (simulates a button press, sensor,
    /// etc). Triggers IRQ if the pin's irq_en is set and a transition occurs.
    pub fn drive_input(&mut self, bank: usize, pin: usize, level: u8) {
        if bank >= GPIO_BANKS || pin >= GPIO_PINS_PER_BANK {
            return;
        }
        let p = &mut self.banks[bank][pin];
        let prev = p.in_val;
        let new = level & 1;
        let transition = prev != new;
        p.in_val = new;
        if p.irq_en {
            let fire = if p.irq_level { new == 1 } else { transition };
            if fire && !p.irq_pending {
                p.irq_pending = true;
                self.bank_irq_status[bank] |= 1 << pin;
            }
        }
    }
}

impl Device for Gpio {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "gpio"
    }
    fn reset(&mut self) {
        for bank in self.banks.iter_mut() {
            for p in bank.iter_mut() {
                *p = GpioPin::default();
            }
        }
        self.bank_irq_en = [0; GPIO_BANKS];
        self.bank_irq_status = [0; GPIO_BANKS];
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        // Layout: per bank, stride 0x40:
        //   +0x00 DIR, +0x04 IN, +0x08 OUT, +0x0C CTRL,
        //   +0x10 IRQ_STATUS, +0x14 IRQ_ENABLE, +0x18 PULL, +0x1C DRIVE
        let bank = (o >> 6) as usize;
        if bank >= GPIO_BANKS {
            return read_zero(o, s);
        }
        let reg = o & 0x3F;
        let mut bits = 0u32;
        for (i, p) in self.banks[bank].iter().enumerate() {
            let v = match reg {
                0x00 => p.dir as u32,
                0x04 => p.in_val as u32,
                0x08 => p.out as u32,
                0x0C => (p.irq_level as u32) | ((p.drive as u32) << 4),
                0x10 => p.irq_pending as u32,
                0x14 => self.bank_irq_en[bank],
                0x18 => p.pull as u32,
                0x1C => p.drive as u32,
                _ => 0,
            };
            bits |= v << i;
        }
        DeviceResult::Value(bits as u64)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        let bank = (o >> 6) as usize;
        if bank >= GPIO_BANKS {
            return drop_write(o, s, v);
        }
        let reg = o & 0x3F;
        for (i, p) in self.banks[bank].iter_mut().enumerate() {
            let bit = ((v >> i) & 1) as u8;
            match reg {
                0x00 => p.dir = bit,
                0x08 => p.out = bit,
                0x10 => {
                    if bit == 1 {
                        p.irq_pending = false;
                    }
                }
                0x14 => {
                    p.irq_en = bit != 0;
                }
                0x0C => {
                    p.irq_level = bit != 0;
                    p.drive = ((v >> 4) & 0x3) as u8;
                }
                0x18 => p.pull = bit,
                0x1C => p.drive = bit,
                _ => {}
            }
        }
        match reg {
            0x10 => self.bank_irq_status[bank] &= !v as u32,
            0x14 => self.bank_irq_en[bank] = v as u32,
            _ => {}
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let mut any = false;
        for bank in 0..GPIO_BANKS {
            let s = self.bank_irq_status[bank] & self.bank_irq_en[bank];
            self.bank_irq_status[bank] = s;
            if s != 0 {
                any = true;
            }
        }
        let cur = if any { self.irq } else { 0 };
        if cur != 0 && cur == self.last_status {
            return None;
        }
        self.last_status = cur;
        if cur != 0 {
            Some(DeviceEvent::Interrupt(cur))
        } else {
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
// I2C — multi-controller, software-driven transactions.
// ----------------------------------------------------------------------------

pub const I2C_FIFO_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cState {
    Idle,
    Tx,
    Rx,
    Stop,
}

pub struct I2c {
    pub id: DeviceId,
    pub index: u32,
    pub ctrl: u32,
    pub status: u32,
    pub addr: u32,
    pub freq_hz: u32,
    pub tx: [u8; I2C_FIFO_DEPTH],
    pub tx_count: usize,
    pub rx: [u8; I2C_FIFO_DEPTH],
    pub rx_count: usize,
    pub irq_en: u32,
    pub irq_status: u32,
    pub state: I2cState,
    pub irq: u32,
    last_status: u32,
}

impl I2c {
    pub fn new(id: DeviceId, index: u32) -> Self {
        Self {
            id,
            index,
            ctrl: 0,
            status: 0x01, // idle bit
            addr: 0,
            freq_hz: 100_000,
            tx: [0; I2C_FIFO_DEPTH],
            tx_count: 0,
            rx: [0; I2C_FIFO_DEPTH],
            rx_count: 0,
            irq_en: 0,
            irq_status: 0,
            state: I2cState::Idle,
            irq: 33,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, index: u32, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id, index)
        }
    }

    /// Inject RX bytes (e.g. an EEPROM response) and raise RX_READY IRQ.
    pub fn inject_rx(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(I2C_FIFO_DEPTH - self.rx_count);
        self.rx[self.rx_count..self.rx_count + n].copy_from_slice(&bytes[..n]);
        self.rx_count += n;
        self.status |= 0x04; // RX_READY
        self.irq_status |= 0x04;
    }

    fn start_tx(&mut self) {
        if self.tx_count == 0 {
            return;
        }
        self.state = I2cState::Tx;
        self.status |= 0x02; // TX_BUSY
                             // Software-driven: instantly drain TX, raise TX_DONE.
        self.tx_count = 0;
        self.status &= !0x02;
        self.status |= 0x08; // TX_DONE
        self.irq_status |= 0x08;
    }
}

impl Device for I2c {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "i2c"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.status = 0x01;
        self.addr = 0;
        self.freq_hz = 100_000;
        self.tx = [0; I2C_FIFO_DEPTH];
        self.tx_count = 0;
        self.rx = [0; I2C_FIFO_DEPTH];
        self.rx_count = 0;
        self.irq_en = 0;
        self.irq_status = 0;
        self.state = I2cState::Idle;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.addr as u64),
            0x0C => DeviceResult::Value(self.freq_hz as u64),
            0x10 => DeviceResult::Value(self.tx_count as u64),
            0x14 => DeviceResult::Value(self.rx_count as u64),
            0x18 => {
                if self.rx_count == 0 {
                    return DeviceResult::Value(0);
                }
                let v = self.rx[0] as u64;
                self.rx.copy_within(1..self.rx_count, 0);
                self.rx_count -= 1;
                if self.rx_count == 0 {
                    self.status &= !0x04;
                }
                DeviceResult::Value(v)
            }
            0x1C => DeviceResult::Value(self.irq_status as u64),
            0x20 => DeviceResult::Value(self.irq_en as u64),
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
                if v & 1 != 0 {
                    self.start_tx();
                }
                DeviceResult::Ok
            }
            0x04 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x08 => {
                self.addr = v as u32;
                DeviceResult::Ok
            }
            0x0C => {
                self.freq_hz = v as u32;
                DeviceResult::Ok
            }
            0x14 => {
                if self.tx_count < I2C_FIFO_DEPTH {
                    self.tx[self.tx_count] = v as u8;
                    self.tx_count += 1;
                }
                DeviceResult::Ok
            }
            0x18 => {
                self.inject_rx(&[v as u8]);
                DeviceResult::Ok
            }
            0x1C => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x20 => {
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
// SPI — 4 chip-select lines, 16-deep TX/RX FIFO.
// ----------------------------------------------------------------------------

pub const SPI_FIFO_DEPTH: usize = 16;

pub struct Spi {
    pub id: DeviceId,
    pub index: u32,
    pub ctrl: u32,
    pub status: u32,
    pub clk_div: u32,
    pub cs: u32,
    pub tx: [u8; SPI_FIFO_DEPTH],
    pub tx_count: usize,
    pub rx: [u8; SPI_FIFO_DEPTH],
    pub rx_count: usize,
    pub irq_en: u32,
    pub irq_status: u32,
    pub irq: u32,
    last_status: u32,
}

impl Spi {
    pub fn new(id: DeviceId, index: u32) -> Self {
        Self {
            id,
            index,
            ctrl: 0,
            status: 0x01,
            clk_div: 0,
            cs: 0xF,
            tx: [0; SPI_FIFO_DEPTH],
            tx_count: 0,
            rx: [0; SPI_FIFO_DEPTH],
            rx_count: 0,
            irq_en: 0,
            irq_status: 0,
            irq: 34,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, index: u32, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id, index)
        }
    }

    /// Drain TX and inject RX bytes (loopback-style response).
    pub fn transfer(&mut self, rx_bytes: &[u8]) {
        let tx_drained = self.tx_count;
        self.tx_count = 0;
        let n = rx_bytes
            .len()
            .min(SPI_FIFO_DEPTH - self.rx_count)
            .min(tx_drained.max(rx_bytes.len()));
        let n = n.min(SPI_FIFO_DEPTH);
        self.rx[..n].copy_from_slice(&rx_bytes[..n]);
        self.rx_count = n;
        self.status |= 0x08; // RX_READY
        self.status &= !0x02; // clear TX_BUSY
        self.status |= 0x10; // TX_DONE
        self.irq_status |= 0x10;
    }
}

impl Device for Spi {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "spi"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.status = 0x01;
        self.clk_div = 0;
        self.cs = 0xF;
        self.tx = [0; SPI_FIFO_DEPTH];
        self.tx_count = 0;
        self.rx = [0; SPI_FIFO_DEPTH];
        self.rx_count = 0;
        self.irq_en = 0;
        self.irq_status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.clk_div as u64),
            0x0C => DeviceResult::Value(self.cs as u64),
            0x10 => DeviceResult::Value(self.tx_count as u64),
            0x14 => DeviceResult::Value(self.rx_count as u64),
            0x18 => {
                if self.rx_count == 0 {
                    return DeviceResult::Value(0);
                }
                let v = self.rx[0] as u64;
                self.rx.copy_within(1..self.rx_count, 0);
                self.rx_count -= 1;
                if self.rx_count == 0 {
                    self.status &= !0x08;
                }
                DeviceResult::Value(v)
            }
            0x1C => DeviceResult::Value(self.irq_status as u64),
            0x20 => DeviceResult::Value(self.irq_en as u64),
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
                self.clk_div = v as u32;
                DeviceResult::Ok
            }
            0x0C => {
                self.cs = v as u32;
                DeviceResult::Ok
            }
            0x14 => {
                if self.tx_count < SPI_FIFO_DEPTH {
                    self.tx[self.tx_count] = v as u8;
                    self.tx_count += 1;
                    self.status |= 0x02; // TX_BUSY
                }
                DeviceResult::Ok
            }
            0x1C => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x20 => {
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
// SPMI — 16 logical channels, per-channel arbitration.
// ----------------------------------------------------------------------------

pub const SPMI_CHANNELS: usize = 16;

pub struct Spmi {
    pub id: DeviceId,
    pub cmd: u64,
    pub data: u64,
    pub status: u32,
    pub owner: [u32; SPMI_CHANNELS],
    pub irq_en: u32,
    pub irq_status: u32,
    pub irq: u32,
    last_status: u32,
}

impl Spmi {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            cmd: 0,
            data: 0,
            status: 0x01,
            owner: [0; SPMI_CHANNELS],
            irq_en: 0,
            irq_status: 0,
            irq: 35,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    /// Complete an outstanding transaction; raises the per-channel IRQ bit.
    pub fn complete_channel(&mut self, channel: usize, data: u64) {
        if channel >= SPMI_CHANNELS {
            return;
        }
        self.owner[channel] = data as u32;
        self.data = data;
        self.status |= 1 << channel;
        self.irq_status |= 1 << channel;
    }
}

impl Device for Spmi {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "spmi"
    }
    fn reset(&mut self) {
        self.cmd = 0;
        self.data = 0;
        self.status = 0x01;
        self.owner = [0; SPMI_CHANNELS];
        self.irq_en = 0;
        self.irq_status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.cmd),
            0x08 => DeviceResult::Value(self.data),
            0x10 => DeviceResult::Value(self.status as u64),
            0x14 => DeviceResult::Value(self.irq_status as u64),
            0x18 => DeviceResult::Value(self.irq_en as u64),
            c if (0x100..0x180).contains(&c) => {
                let ch = ((c - 0x100) / 8) as usize;
                if ch < SPMI_CHANNELS {
                    DeviceResult::Value(self.owner[ch] as u64)
                } else {
                    read_zero(o, s)
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
                self.cmd = v;
                DeviceResult::Ok
            }
            0x08 => {
                self.data = v;
                DeviceResult::Ok
            }
            0x10 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x14 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x18 => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            c if (0x100..0x180).contains(&c) => {
                let ch = ((c - 0x100) / 8) as usize;
                if ch < SPMI_CHANNELS {
                    self.owner[ch] = v as u32;
                }
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
// PMU — event counter with write-1-to-clear IRQ.
// ----------------------------------------------------------------------------

pub struct Pmu {
    pub id: DeviceId,
    pub events: u64,
    pub mask: u64,
    pub req: u64,
    pub ack: u64,
    pub status: u64,
    pub irq: u32,
    last_status: u32,
}

impl Pmu {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            events: 0,
            mask: 0,
            req: 0,
            ack: 0,
            status: 0,
            irq: 36,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
}

impl Device for Pmu {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pmu"
    }
    fn reset(&mut self) {
        self.events = 0;
        self.mask = 0;
        self.req = 0;
        self.ack = 0;
        self.status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.events),
            0x08 => DeviceResult::Value(self.mask),
            0x10 => DeviceResult::Value(self.req),
            0x18 => DeviceResult::Value(self.ack),
            0x20 => DeviceResult::Value(self.status),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x08 => {
                self.mask = v;
                self.status = self.events & self.mask;
                DeviceResult::Ok
            }
            0x10 => {
                self.req = v;
                DeviceResult::Ok
            }
            0x18 => {
                self.ack = v;
                DeviceResult::Ok
            }
            0x20 => {
                // W1C: clear acknowledged event bits.
                self.events &= !v;
                self.status = self.events & self.mask;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.events & self.mask) != 0;
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

impl Pmu {
    /// External helper for tests / driver code: set an event bit.
    pub fn raise_event(&mut self, bit: u64) {
        self.events |= bit;
        self.status = self.events & self.mask;
    }
}

// ----------------------------------------------------------------------------
// Thermal — 8 zones, threshold IRQ.
// ----------------------------------------------------------------------------

pub const THERMAL_ZONES: usize = 8;

pub struct Thermal {
    pub id: DeviceId,
    pub temps: [i32; THERMAL_ZONES], // deci-degrees C
    pub hi_thresh: [i32; THERMAL_ZONES],
    pub lo_thresh: [i32; THERMAL_ZONES],
    pub irq_status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
}

impl Thermal {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            temps: [2500; THERMAL_ZONES],
            hi_thresh: [8000; THERMAL_ZONES],
            lo_thresh: [-4000; THERMAL_ZONES],
            irq_status: 0,
            irq_en: 0,
            irq: 37,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn set_temp(&mut self, zone: usize, deci_c: i32) {
        if zone >= THERMAL_ZONES {
            return;
        }
        self.temps[zone] = deci_c;
        self.recompute_irq(zone);
    }

    fn recompute_irq(&mut self, zone: usize) {
        let bit = 1u32 << zone;
        let t = self.temps[zone];
        if t >= self.hi_thresh[zone] || t <= self.lo_thresh[zone] {
            self.irq_status |= bit;
        } else {
            self.irq_status &= !bit;
        }
    }
}

impl Device for Thermal {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "thermal"
    }
    fn reset(&mut self) {
        self.temps = [2500; THERMAL_ZONES];
        self.hi_thresh = [8000; THERMAL_ZONES];
        self.lo_thresh = [-4000; THERMAL_ZONES];
        self.irq_status = 0;
        self.irq_en = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            t if (0x00..0x20).contains(&t) && (t % 4 == 0) => {
                DeviceResult::Value(self.temps[(t / 4) as usize] as i64 as u64)
            }
            h if (0x80..0xA0).contains(&h) && (h % 4 == 0) => {
                DeviceResult::Value(self.hi_thresh[((h - 0x80) / 4) as usize] as i64 as u64)
            }
            l if (0xC0..0xE0).contains(&l) && (l % 4 == 0) => {
                DeviceResult::Value(self.lo_thresh[((l - 0xC0) / 4) as usize] as i64 as u64)
            }
            0x100 => DeviceResult::Value(self.irq_status as u64),
            0x104 => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            t if (0x00..0x20).contains(&t) && (t % 4 == 0) => {
                self.temps[(t / 4) as usize] = v as i32;
                self.recompute_irq((t / 4) as usize);
                DeviceResult::Ok
            }
            h if (0x80..0xA0).contains(&h) && (h % 4 == 0) => {
                self.hi_thresh[((h - 0x80) / 4) as usize] = v as i32;
                DeviceResult::Ok
            }
            l if (0xC0..0xE0).contains(&l) && (l % 4 == 0) => {
                self.lo_thresh[((l - 0xC0) / 4) as usize] = v as i32;
                DeviceResult::Ok
            }
            0x100 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x104 => {
                self.irq_en = v as u32;
                for z in 0..THERMAL_ZONES {
                    self.recompute_irq(z);
                }
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
// NandCtrl — command/address/data interface with completion IRQ.
// ----------------------------------------------------------------------------

pub struct NandCtrl {
    pub id: DeviceId,
    pub cmd: u32,
    pub addr: u32,
    pub data: u64,
    pub ecc: u32,
    pub status: u32,
    pub irq_en: u32,
    pub irq_status: u32,
    pub irq: u32,
    pub page_buf: [u8; crate::storage::NAND_PAGE_SIZE],
    pub page_buf_len: usize,
    last_status: u32,
    pub array: Option<Box<crate::storage::NandArray>>,
}

impl NandCtrl {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            cmd: 0,
            addr: 0,
            data: 0,
            ecc: 0,
            status: 0x01, // RDY
            irq_en: 0,
            irq_status: 0,
            irq: 38,
            page_buf: [0xFF; crate::storage::NAND_PAGE_SIZE],
            page_buf_len: 0,
            last_status: 0,
            array: None,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn with_array(id: DeviceId, array: crate::storage::NandArray) -> Self {
        let mut s = Self::new(id);
        s.array = Some(Box::new(array));
        s
    }

    pub fn with_irq_and_array(id: DeviceId, irq: u32, array: crate::storage::NandArray) -> Self {
        let mut s = Self::with_irq(id, irq);
        s.array = Some(Box::new(array));
        s
    }

    /// Submit a complete read result for the current addr/cmd pair.
    pub fn complete_read(&mut self, data: u64, ecc: u32) {
        self.data = data;
        self.ecc = ecc;
        self.status |= 0x02; // DONE
        self.status &= !0x01; // clear RDY
        self.irq_status |= 0x02;
    }

    pub fn complete_write(&mut self) {
        self.status |= 0x02;
        self.status &= !0x01;
        self.irq_status |= 0x02;
    }

    /// Execute a CMD against a `NandArray` backend. `addr` encodes
    /// `block << 16 | page` (matching what most on-SoC drivers write).
    pub fn execute_on(&mut self, array: &mut crate::storage::NandArray) {
        let block = (self.addr >> 16) as usize;
        let page = (self.addr & 0xFFFF) as usize;
        self.status &= !0x01; // clear RDY
        match self.cmd & 0xFF {
            0x00 | 0xFF => {
                // RESET / READ_ID — return a plausible ONFI/JEDEC ID string in
                // the page buffer so firmware ID probes succeed.
                const ID: &[u8] = b"IPHEMNAND\x00\x00\x00\x00\x00\x00\x00";
                self.page_buf[..ID.len()].copy_from_slice(ID);
                self.page_buf_len = ID.len();
                self.status |= 0x02; // done
                self.irq_status |= 0x02;
            }
            0x30 => {
                // READ_PAGE
                if let Some(buf) = array.read_page(block, page) {
                    self.page_buf[..buf.len()].copy_from_slice(buf);
                    self.page_buf_len = buf.len();
                    let mut sum: u32 = 0;
                    for chunk in buf.chunks(4) {
                        let mut v = [0u8; 4];
                        for (i, b) in chunk.iter().enumerate() {
                            v[i] = *b;
                        }
                        sum ^= u32::from_le_bytes(v);
                    }
                    self.ecc = sum;
                } else {
                    self.status |= 0x04; // FAIL
                }
                self.status |= 0x02; // DONE
                self.irq_status |= 0x02;
            }
            0x80 => {
                // PROGRAM_PAGE — uses page_buf
                if array.program_page(block, page, &self.page_buf[..self.page_buf_len]) {
                    self.status |= 0x02;
                } else {
                    self.status |= 0x04;
                }
                self.irq_status |= 0x02;
            }
            0x60 => {
                // ERASE_BLOCK
                if array.erase_block(block) {
                    self.status |= 0x02;
                } else {
                    self.status |= 0x04;
                }
                self.irq_status |= 0x02;
            }
            _ => {
                self.status |= 0x04;
                self.irq_status |= 0x02;
            }
        }
        self.status |= 0x01; // restore RDY
    }

    /// Write a full APFS image directly to the internal NandArray.
    /// This bypasses the register interface and writes blocks directly.
    /// `image` should be a flat APFS container image.
    pub fn write_apfs_image(&mut self, image: &[u8]) -> Result<(), &'static str> {
        let array = self.array.as_mut().ok_or("no NandArray attached")?;
        let _block_size = crate::storage::NAND_BLOCK_SIZE;
        let page_size = crate::storage::NAND_PAGE_SIZE;
        let _pages_per_block = crate::storage::NAND_PAGES_PER_BLOCK;

        // Erase all blocks first
        for b in 0..array.block_count() {
            array.erase_block(b);
        }

        // Write image page by page
        for (i, chunk) in image.chunks(page_size).enumerate() {
            let block = i / crate::storage::NAND_PAGES_PER_BLOCK;
            let page = i % crate::storage::NAND_PAGES_PER_BLOCK;
            if block >= array.block_count() {
                break;
            }
            array.program_page(block, page, chunk);
        }
        Ok(())
    }
}

impl Device for NandCtrl {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "nand"
    }
    fn reset(&mut self) {
        self.cmd = 0;
        self.addr = 0;
        self.data = 0;
        self.ecc = 0;
        self.status = 0x01;
        self.irq_en = 0;
        self.irq_status = 0;
        self.page_buf = [0xFF; crate::storage::NAND_PAGE_SIZE];
        self.page_buf_len = 0;
        self.last_status = 0;
        // Don't reset the array - it's persistent storage
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.cmd as u64),
            0x04 => DeviceResult::Value(self.addr as u64),
            0x08 => DeviceResult::Value(self.data),
            0x0C => DeviceResult::Value(self.ecc as u64),
            0x10 => DeviceResult::Value(self.status as u64),
            0x14 => DeviceResult::Value(self.irq_status as u64),
            0x18 => DeviceResult::Value(self.irq_en as u64),
            // Page-buffer window at 0x20..0x1020
            p if (0x20..0x1020).contains(&p) => {
                let off = (p - 0x20) as usize;
                if off + s > self.page_buf_len {
                    return read_zero(o, s);
                }
                let mut v = 0u64;
                for i in 0..s {
                    v |= (self.page_buf[off + i] as u64) << (i * 8);
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
                self.cmd = v as u32;
                // Auto-execute if we have an array and the command is one that
                // operates on the array (READ_PAGE=0x30, PROGRAM_PAGE=0x80, ERASE_BLOCK=0x60)
                if let Some(array) = self.array.as_mut() {
                    match self.cmd & 0xFF {
                        0x30 => {
                            // READ_PAGE
                            let block = (self.addr >> 16) as usize;
                            let page = (self.addr & 0xFFFF) as usize;
                            self.status &= !0x01; // clear RDY
                            if let Some(buf) = array.read_page(block, page) {
                                self.page_buf[..buf.len()].copy_from_slice(buf);
                                self.page_buf_len = buf.len();
                                let mut sum: u32 = 0;
                                for chunk in buf.chunks(4) {
                                    if chunk.len() == 4 {
                                        sum = sum.wrapping_add(u32::from_le_bytes(
                                            chunk.try_into().unwrap(),
                                        ));
                                    }
                                }
                                self.ecc = sum;
                                self.status |= 0x02; // DONE
                                self.status &= !0x01; // clear RDY
                                self.irq_status |= 0x02;
                            } else {
                                self.status |= 0x04; // ERR
                                self.irq_status |= 0x04;
                            }
                        }
                        0x80 => {
                            // PROGRAM_PAGE
                            let block = (self.addr >> 16) as usize;
                            let page = (self.addr & 0xFFFF) as usize;
                            self.status &= !0x01; // clear RDY
                            let mut buf = [0xFFu8; crate::storage::NAND_PAGE_SIZE];
                            buf[..self.page_buf_len]
                                .copy_from_slice(&self.page_buf[..self.page_buf_len]);
                            if array.program_page(block, page, &buf[..self.page_buf_len]) {
                                self.status |= 0x02; // DONE
                                self.status &= !0x01;
                                self.irq_status |= 0x02;
                            } else {
                                self.status |= 0x04; // ERR
                                self.irq_status |= 0x04;
                            }
                        }
                        0x60 => {
                            // ERASE_BLOCK
                            let block = (self.addr >> 16) as usize;
                            self.status &= !0x01;
                            array.erase_block(block);
                            self.status |= 0x02; // DONE
                            self.status &= !0x01;
                            self.irq_status |= 0x02;
                        }
                        _ => {}
                    }
                }
                DeviceResult::Ok
            }
            0x04 => {
                self.addr = v as u32;
                DeviceResult::Ok
            }
            0x08 => {
                self.data = v;
                DeviceResult::Ok
            }
            0x0C => {
                self.ecc = v as u32;
                DeviceResult::Ok
            }
            0x10 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0x14 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x18 => {
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
// Watchdog
// ----------------------------------------------------------------------------

pub struct Watchdog {
    pub id: DeviceId,
    pub ctrl: u32,
    pub timeout: u32,
    pub kick: u32,
    pub counter: u32,
    pub status: u32,
    pub irq: u32,
    pub reset_pending: bool,
}

impl Watchdog {
    pub const KICK_MAGIC: u32 = 0x5A5A_AA55;

    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: 0,
            timeout: 0,
            kick: 0,
            counter: 0,
            status: 0,
            irq: 39,
            reset_pending: false,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
}

impl Device for Watchdog {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "watchdog"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.kick = 0;
        self.counter = 0;
        self.status = 0;
        self.reset_pending = false;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.timeout as u64),
            0x08 => DeviceResult::Value(self.counter as u64),
            0x0C => DeviceResult::Value(self.status as u64),
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
                self.timeout = v as u32;
                self.counter = 0;
                DeviceResult::Ok
            }
            0x08 => {
                self.kick = v as u32;
                if self.kick == Self::KICK_MAGIC {
                    self.counter = 0;
                    self.status &= !0x1;
                }
                DeviceResult::Ok
            }
            0x0C => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, c: u64) -> Option<DeviceEvent> {
        if self.ctrl & 1 == 0 {
            return None;
        }
        let inc = (c & 0xFFFF) as u32;
        self.counter = self.counter.saturating_add(inc);
        if self.timeout != 0 && self.counter >= self.timeout {
            self.status |= 0x1;
            self.reset_pending = true;
            return Some(DeviceEvent::Interrupt(self.irq));
        }
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Chip-control/fuse window adjacent to WDT on d321ap. The DeviceTree also
/// aliases this address through `apcie` and `sochot0`; the reset values have
/// not been measured, so production construction deliberately leaves them 0.
pub struct AppleChipControl {
    id: DeviceId,
    pub fuse0_raw: u32,
    pub security_raw: u32,
    pub revision_raw: u32,
}

impl AppleChipControl {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            fuse0_raw: 0,
            security_raw: 0,
            revision_raw: 0,
        }
    }
}

impl Device for AppleChipControl {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "apple-chip-control"
    }

    fn reset(&mut self) {
        // Fuse/identity words are not software-reset state.
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !valid_size(size) {
            return DeviceResult::Fault;
        }
        match offset {
            0x10 => DeviceResult::Value(self.fuse0_raw as u64),
            0x20 => DeviceResult::Value(self.security_raw as u64),
            0x30 => DeviceResult::Value(self.revision_raw as u64),
            _ => read_zero(offset, size),
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        // None of these words have a proven software-write protocol. Do not
        // turn guest stores into an invented readiness or identity change.
        drop_write(offset, size, value)
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

// ----------------------------------------------------------------------------
// RTC — battery-backed real-time clock with alarm.
// ----------------------------------------------------------------------------

pub struct Rtc {
    pub id: DeviceId,
    pub seconds: u64,
    pub alarm: u64,
    pub ctrl: u32,
    pub status: u32,
    pub irq: u32,
}

impl Rtc {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            seconds: 0,
            alarm: u64::MAX,
            ctrl: 0,
            status: 0,
            irq: 40,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
}

impl Device for Rtc {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "rtc"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.seconds),
            0x08 => DeviceResult::Value(self.alarm),
            0x10 => DeviceResult::Value(self.ctrl as u64),
            0x14 => DeviceResult::Value(self.status as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.seconds = v;
                DeviceResult::Ok
            }
            0x08 => {
                self.alarm = v;
                DeviceResult::Ok
            }
            0x10 => {
                self.ctrl = v as u32;
                DeviceResult::Ok
            }
            0x14 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, c: u64) -> Option<DeviceEvent> {
        let secs = c / 1_000_000;
        self.seconds = self.seconds.wrapping_add(secs);
        if self.ctrl & 1 != 0 && self.seconds >= self.alarm {
            self.status |= 0x1;
            return Some(DeviceEvent::Interrupt(self.irq));
        }
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ----------------------------------------------------------------------------
// PWM — backlight / haptic / fan channels.
// ----------------------------------------------------------------------------

pub const PWM_CHANNELS: usize = 8;

pub struct Pwm {
    pub id: DeviceId,
    pub ctrl: [u32; PWM_CHANNELS],
    pub period: [u32; PWM_CHANNELS],
    pub duty: [u32; PWM_CHANNELS],
    pub enable: u32,
    pub status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
}

impl Pwm {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: [0; PWM_CHANNELS],
            period: [0; PWM_CHANNELS],
            duty: [0; PWM_CHANNELS],
            enable: 0,
            status: 0,
            irq_en: 0,
            irq: 41,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn set_duty(&mut self, ch: usize, duty: u32) {
        if ch < PWM_CHANNELS {
            self.duty[ch] = duty.min(self.period[ch]);
            self.status |= 1 << ch;
        }
    }
}

impl Device for Pwm {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pwm"
    }
    fn reset(&mut self) {
        self.ctrl = [0; PWM_CHANNELS];
        self.period = [0; PWM_CHANNELS];
        self.duty = [0; PWM_CHANNELS];
        self.enable = 0;
        self.status = 0;
        self.irq_en = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            e if (0x00..0x20).contains(&e) && (e % 4 == 0) => {
                DeviceResult::Value(self.ctrl[(e / 4) as usize] as u64)
            }
            p if (0x40..0x60).contains(&p) && (p % 4 == 0) => {
                DeviceResult::Value(self.period[((p - 0x40) / 4) as usize] as u64)
            }
            d if (0x80..0xA0).contains(&d) && (d % 4 == 0) => {
                DeviceResult::Value(self.duty[((d - 0x80) / 4) as usize] as u64)
            }
            0xC0 => DeviceResult::Value(self.enable as u64),
            0xC4 => DeviceResult::Value(self.status as u64),
            0xC8 => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            e if (0x00..0x20).contains(&e) && (e % 4 == 0) => {
                self.ctrl[(e / 4) as usize] = v as u32;
                DeviceResult::Ok
            }
            p if (0x40..0x60).contains(&p) && (p % 4 == 0) => {
                self.period[((p - 0x40) / 4) as usize] = v as u32;
                DeviceResult::Ok
            }
            d if (0x80..0xA0).contains(&d) && (d % 4 == 0) => {
                let ch = ((d - 0x80) / 4) as usize;
                self.duty[ch] = (v as u32).min(self.period[ch]);
                DeviceResult::Ok
            }
            0xC0 => {
                self.enable = v as u32;
                DeviceResult::Ok
            }
            0xC4 => {
                self.status &= !(v as u32);
                DeviceResult::Ok
            }
            0xC8 => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        let active = (self.status & self.irq_en & self.enable) != 0;
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
// ADC — battery / thermistor / accessory detect.
// ----------------------------------------------------------------------------

pub const ADC_CHANNELS: usize = 8;

pub struct Adc {
    pub id: DeviceId,
    pub ctrl: u32,
    pub channel: u32,
    pub values: [u16; ADC_CHANNELS],
    pub irq_en: u32,
    pub irq_status: u32,
    pub irq: u32,
    last_status: u32,
}

impl Adc {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: 0,
            channel: 0,
            values: [0; ADC_CHANNELS],
            irq_en: 0,
            irq_status: 0,
            irq: 42,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn set_value(&mut self, ch: usize, val: u16) {
        if ch < ADC_CHANNELS {
            self.values[ch] = val;
            self.irq_status |= 1 << ch;
        }
    }
}

impl Device for Adc {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "adc"
    }
    fn reset(&mut self) {
        self.ctrl = 0;
        self.channel = 0;
        self.values = [0; ADC_CHANNELS];
        self.irq_en = 0;
        self.irq_status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.ctrl as u64),
            0x04 => DeviceResult::Value(self.channel as u64),
            0x08 => DeviceResult::Value(self.values[(self.channel as usize) % ADC_CHANNELS] as u64),
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
            0x00 => {
                self.ctrl = v as u32;
                if v & 1 != 0 {
                    self.irq_status |= 1 << self.channel;
                }
                DeviceResult::Ok
            }
            0x04 => {
                self.channel = v as u32 % ADC_CHANNELS as u32;
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
// Pinmux — per-pin function select.
// ----------------------------------------------------------------------------

pub const PINMUX_PINS: usize = 64;

pub struct Pinmux {
    pub id: DeviceId,
    pub sel: [u8; PINMUX_PINS],
    pub drive: [u8; PINMUX_PINS],
    pub pull: [u8; PINMUX_PINS],
}

impl Pinmux {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            sel: [0; PINMUX_PINS],
            drive: [0; PINMUX_PINS],
            pull: [0; PINMUX_PINS],
        }
    }
}

impl Device for Pinmux {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "pinmux"
    }
    fn reset(&mut self) {
        self.sel = [0; PINMUX_PINS];
        self.drive = [0; PINMUX_PINS];
        self.pull = [0; PINMUX_PINS];
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        let bank = (o / PINMUX_PINS as u64) as usize;
        let pin = (o % PINMUX_PINS as u64) as usize;
        if pin >= PINMUX_PINS || bank > 2 {
            return read_zero(o, s);
        }
        let v = match bank {
            0 => self.sel[pin],
            1 => self.drive[pin],
            2 => self.pull[pin],
            _ => 0,
        };
        DeviceResult::Value(v as u64)
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        let bank = (o / PINMUX_PINS as u64) as usize;
        let pin = (o % PINMUX_PINS as u64) as usize;
        if pin >= PINMUX_PINS || bank > 2 {
            return drop_write(o, s, v);
        }
        match bank {
            0 => self.sel[pin] = v as u8,
            1 => self.drive[pin] = v as u8,
            2 => self.pull[pin] = v as u8,
            _ => {}
        }
        DeviceResult::Ok
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ----------------------------------------------------------------------------
// ClockCtrl — PLL / clock gate control.
// ----------------------------------------------------------------------------

pub const CLOCKCTL_PLLS: usize = 8;

pub struct ClockCtrl {
    pub id: DeviceId,
    /// Per-PLL divisor.
    pub div: [u32; CLOCKCTL_PLLS],
    /// Per-PLL status (bit 0 = locked, bit 1 = enabled).
    pub stat: [u32; CLOCKCTL_PLLS],
    /// Per-PLL enable (1 = enabled, 0 = disabled).
    pub enable: [u32; CLOCKCTL_PLLS],
    /// Per-PLL target frequency in Hz (0 = default).
    pub freq_hz: [u64; CLOCKCTL_PLLS],
    /// Per-PLL actual frequency in Hz (mirror of `freq_hz` after lock).
    pub freq_actual: [u64; CLOCKCTL_PLLS],
    /// Per-clock-domain gate (bit i = gated).
    pub gate: u32,
    /// Per-PLL IRQ enable.
    pub irq_en: u32,
    /// Per-PLL IRQ status (W1C).
    pub irq_status: u32,
    /// IRQ line number.
    pub irq: u32,
    /// Device-id / revision-id.
    pub devid: u32,
    pub revid: u32,
    last_status: u32,
}

impl ClockCtrl {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            div: [1; CLOCKCTL_PLLS],
            stat: [0x1; CLOCKCTL_PLLS],
            enable: [1; CLOCKCTL_PLLS],
            freq_hz: [0; CLOCKCTL_PLLS],
            freq_actual: [0; CLOCKCTL_PLLS],
            gate: 0,
            irq_en: 0,
            irq_status: 0,
            irq: 43,
            devid: 0xA121_0000,
            revid: 0x0000_0001,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
}

impl Device for ClockCtrl {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "clockctl"
    }
    fn reset(&mut self) {
        self.div = [1; CLOCKCTL_PLLS];
        self.stat = [0x1; CLOCKCTL_PLLS];
        self.enable = [1; CLOCKCTL_PLLS];
        self.freq_hz = [0; CLOCKCTL_PLLS];
        self.freq_actual = [0; CLOCKCTL_PLLS];
        self.gate = 0;
        self.irq_en = 0;
        self.irq_status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            d if (0x00..0x20).contains(&d) && (d % 4 == 0) => {
                DeviceResult::Value(self.div[(d / 4) as usize] as u64)
            }
            en if (0x20..0x40).contains(&en) && (en % 4 == 0) => {
                DeviceResult::Value(self.enable[((en - 0x20) / 4) as usize] as u64)
            }
            st if (0x40..0x60).contains(&st) && (st % 4 == 0) => {
                DeviceResult::Value(self.stat[((st - 0x40) / 4) as usize] as u64)
            }
            fh if (0x60..0x80).contains(&fh) && (fh % 4 == 0) => {
                DeviceResult::Value(self.freq_hz[((fh - 0x60) / 4) as usize])
            }
            fa if (0xA0..0xC0).contains(&fa) && (fa % 4 == 0) => {
                DeviceResult::Value(self.freq_actual[((fa - 0xA0) / 4) as usize])
            }
            0x80 => DeviceResult::Value(self.gate as u64),
            0x84 => DeviceResult::Value(self.irq_status as u64),
            0x88 => DeviceResult::Value(self.irq_en as u64),
            0xE0 => DeviceResult::Value(self.devid as u64),
            0xE4 => DeviceResult::Value(self.revid as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            d if (0x00..0x20).contains(&d) && (d % 4 == 0) => {
                let pll = (d / 4) as usize;
                self.div[pll] = v as u32;
                self.stat[pll] &= !0x1;
                DeviceResult::Ok
            }
            en if (0x20..0x40).contains(&en) && (en % 4 == 0) => {
                let pll = ((en - 0x20) / 4) as usize;
                self.enable[pll] = (v as u32) & 0x1;
                DeviceResult::Ok
            }
            st if (0x40..0x60).contains(&st) && (st % 4 == 0) => {
                let pll = ((st - 0x40) / 4) as usize;
                if (v as u32) & 0x1 != 0 {
                    self.stat[pll] |= 0x1;
                    self.irq_status |= 1 << pll;
                }
                DeviceResult::Ok
            }
            fh if (0x60..0x80).contains(&fh) && (fh % 4 == 0) => {
                let pll = ((fh - 0x60) / 4) as usize;
                self.freq_hz[pll] = v;
                // Lock and mirror actual frequency on next write.
                self.stat[pll] |= 0x1;
                self.freq_actual[pll] = v;
                self.irq_status |= 1 << pll;
                DeviceResult::Ok
            }
            0x80 => {
                self.gate = v as u32;
                DeviceResult::Ok
            }
            0x84 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0x88 => {
                self.irq_en = v as u32;
                DeviceResult::Ok
            }
            0xE0 | 0xE4 => DeviceResult::Ok, // RO
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
// Regulator — voltage regulator on/off and voltage.
// ----------------------------------------------------------------------------

pub const REGULATOR_CHANNELS: usize = 8;

pub struct Regulator {
    pub id: DeviceId,
    pub ctrl: [u32; REGULATOR_CHANNELS],
    pub voltage: [u32; REGULATOR_CHANNELS], // microvolts
    pub status: [u32; REGULATOR_CHANNELS],
    pub irq_en: u32,
    pub irq_status: u32,
    pub irq: u32,
    last_status: u32,
}

impl Regulator {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            ctrl: [0; REGULATOR_CHANNELS],
            voltage: [1_100_000; REGULATOR_CHANNELS],
            status: [0x1; REGULATOR_CHANNELS], // STABLE
            irq_en: 0,
            irq_status: 0,
            irq: 44,
            last_status: 0,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn set_voltage(&mut self, ch: usize, uv: u32) {
        if ch < REGULATOR_CHANNELS {
            self.voltage[ch] = uv;
            self.status[ch] |= 0x1;
            self.irq_status |= 1 << ch;
        }
    }
}

impl Device for Regulator {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "regulator"
    }
    fn reset(&mut self) {
        self.ctrl = [0; REGULATOR_CHANNELS];
        self.voltage = [1_100_000; REGULATOR_CHANNELS];
        self.status = [0x1; REGULATOR_CHANNELS];
        self.irq_en = 0;
        self.irq_status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            c if (0x00..0x20).contains(&c) && (c % 4 == 0) => {
                DeviceResult::Value(self.ctrl[(c / 4) as usize] as u64)
            }
            v if (0x40..0x60).contains(&v) && (v % 4 == 0) => {
                DeviceResult::Value(self.voltage[((v - 0x40) / 4) as usize] as u64)
            }
            st if (0x80..0xA0).contains(&st) && (st % 4 == 0) => {
                DeviceResult::Value(self.status[((st - 0x80) / 4) as usize] as u64)
            }
            0xC0 => DeviceResult::Value(self.irq_status as u64),
            0xC4 => DeviceResult::Value(self.irq_en as u64),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            c if (0x00..0x20).contains(&c) && (c % 4 == 0) => {
                let ch = (c / 4) as usize;
                self.ctrl[ch] = v as u32;
                if v & 1 != 0 {
                    self.status[ch] |= 0x1;
                    self.irq_status |= 1 << ch;
                }
                DeviceResult::Ok
            }
            v if (0x40..0x60).contains(&v) && (v % 4 == 0) => {
                self.voltage[((v - 0x40) / 4) as usize] = v as u32;
                DeviceResult::Ok
            }
            st if (0x80..0xA0).contains(&st) && (st % 4 == 0) => {
                self.status[((st - 0x80) / 4) as usize] &= !(v as u32);
                DeviceResult::Ok
            }
            0xC0 => {
                self.irq_status &= !(v as u32);
                DeviceResult::Ok
            }
            0xC4 => {
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
