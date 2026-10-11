//! Apple S5L-style serial console (research debug console).
//!
//! Models the ARM "serial-io" (s3c-style) register window that XNU's early
//! debug console driver uses: TX holding registers at 0x00/0x18, status at
//! 0x14 (UARTTRSTAT), RX at 0x1c. The device accumulates TX bytes and always
//! reports the TX FIFO empty so firmware never stalls on flow control. No
//! interrupt line is wired; callers drain the log with [`SerialConsole::take_log`].
//!
//! Register map (subset used by the s5l/sio serial drivers):
//!   0x00  UTXD   (write) / URXD (read)
//!   0x04  UBRDIV
//!   0x08  ULCON
//!   0x0c  UCON
//!   0x10  UFCON
//!   0x14  UTRSTAT  bit0 RX-ready, bit1 TX-busy, bit2 TX-empty
//!   0x18  UTXH     (write)
//!   0x1c  URXH     (read)
//!   0x20  UBRDIV2
//!   0x24  UDIVSLOT

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::VecDeque;

// Apple S5L ("s5l0") serial console. The Darwin kernel's early debug console
// driver programs this Samsung s3c-style window on the arm-io UARTs
// (e.g. d321ap `uart-1,samsung` at 0x235204000, `uart-0` at 0x235100000).
// Offset 0x00 here is a *config* write (ULCON), never a transmit character.
//
// Two real firmware images fix this window, and they are the authority for it:
//
//   * The SecureROM console keeps a static register-pointer table in its image
//     data -- `0x10001c5a0` in the T8020 `.4.7` build, returned by the console
//     `get-by-index` helper at `0x100004454` for port 0 -- holding
//     `0x235100000 + {0x00,0x04,0x08,0x0c,0x10,0x20,0x30,0x34,0x38,0x3c,0x40,
//     0x44,0x48,0x4c}` as 64-bit pointers. The identical table appears in the
//     t8010, t8011, t8015, t8020 and t8027 SecureROMs, so the register set is
//     not specific to one chip. That table is what the modelled window
//     accepts; nothing beyond 0x4c is claimed and unknown offsets still fault.
//   * The iOS 12 kernel polls offset 0x10 for a transmit-complete condition
//     (`ldr w8,[x8,#0x10]; and w0,w8,#0x4`, i.e. bit 2).
//
// Together they pin 0x10 as a **combined register**: a store to it transmits a
// character (the SecureROM console's PIO loop does exactly that at
// `0x100004880`/`0x100004884`, with the character coming from the caller's
// buffer), and a load returns the transfer status the kernel polls. 0x20 is
// kept as a second transmit-data store because the Samsung map this device
// descends from places UTXH there and the SecureROM's table still lists it (its
// symbol-send path reads it); nothing observed requires a store there, so
// which of the two is the "real" data register is not settled and both are
// accepted.
//
//   0x00 ULCON    line control       0x04 UCON     control
//   0x08 UFCON    FIFO control       0x0c UMCON    modem control
//   0x10 TXDATA / status (combined) 0x14 UERSTAT  RX error
//   0x18 UFSTAT   FIFO status        0x1c UMSTAT   modem status
//   0x20 UTXH     TX holding (write) 0x24 URXH     RX holding (read)
//   0x28 UBRDIV   baud divisor       0x2c UDIVSLOT baud slot
//   0x30          ROM console clock/baud ratio (see below)
//   0x34..0x48    ROM console aux words, guest-owned storage
//   0x4c          ROM console transmit-outstanding count
//
// 0x14/0x18/0x1c/0x24/0x28/0x2c are not in the SecureROM's table; they are the
// Samsung-map remainder, kept because the kernel's own driver programs 0x28
// (baud divisor) and reads the receive path.
//
// What the SecureROM's own instructions pin down, and what they do not:
//   * 0x30 receives, once per console init, the result of `udiv(freq, baud)`
//     clamped to a minimum of one (`0x100004540..0x100004558`; the T8020 run
//     writes 2). Nothing reads it back on the observed path, so it is modeled
//     as a stored word and does *not* drive the serial engine: the bit time
//     stays derived from 0x28, which the kernel's own driver programs.
//   * 0x4c is written with a byte count *before* the ROM pushes those bytes to
//     the data register, and is then polled until it reads zero
//     (`0x100004844`/`0x100004848` writes it, `0x1000048a4..0x1000048ac` spins
//     on it). A guest write therefore has to arm a hardware countdown that
//     the transmitter retires byte by byte, which is what `tx_outstanding`
//     below implements. The register's exact Apple name is not known; the
//     write-then-drain contract above is all the ROM's code establishes.
//   * 0x08 is read for transmit free space as `16 - (value >> 6)`
//     (`0x10000485c..0x100004870`), i.e. a transmit-FIFO occupancy count, and
//     the depth of 16 is independently documented by the mainline
//     `s5l_serial_drv_data` (`fifosize = 16`). The count is therefore modeled
//     in bits 9:6. The same register is read for a second count in bits 14:11
//     by the console self-test at `0x10000471c..0x100004734`, which bounds a
//     read-back loop over 0x20; that loop cannot terminate when the count is
//     zero (it would recompute the same bound forever), so 0x20 is modeled as
//     a read-back of what the guest has transmitted and bits 14:11 report the
//     depth of that queue. Whether the real part loops its transmitter
//     internally or echoes through a pad is not established -- what *is*
//     established is that a byte stored to 0x10 has to be readable back from
//     0x20, because that is the only source the self-test has. The
//     guest-receive path (0x24 and host-injected input) is untouched, so a
//     guest console driver is unaffected.
//
// The load at 0x10 reports bit0 = TX ready (always, so firmware TX polls never
// stall), bit1 = RX data present (clearing as URXH drains), and bit2 = TX shift
// register empty (only while a byte is mid-shift). The Darwin console driver
// awaits bit2 for TX-complete, so clearing it while shifting is required for
// the direct-kernel run to leave its console poll. Config registers round-trip
// stored values; TX bytes accumulate for host draining.
pub const S5L_ULCON: u64 = 0x00;
pub const S5L_UCON: u64 = 0x04;
pub const S5L_UFCON: u64 = 0x08;
pub const S5L_UMCON: u64 = 0x0c;
/// Combined transmit-data (write) / transfer-status (read) register.
pub const S5L_TXDATA: u64 = 0x10;
pub const S5L_UERSTAT: u64 = 0x14;
pub const S5L_UFSTAT: u64 = 0x18;
pub const S5L_UMSTAT: u64 = 0x1c;
pub const S5L_UTXH: u64 = 0x20;
pub const S5L_URXH: u64 = 0x24;
pub const S5L_UBRDIV: u64 = 0x28;
pub const S5L_UDIVSLOT: u64 = 0x2c;

/// Clock/baud ratio the SecureROM console computes once at init.
pub const S5L_UCLKDIV: u64 = 0x30;
/// Guest-owned console word; the ROM writes zero before a message and the byte
/// count in its symbol-send path, and never reads it back.
pub const S5L_TXLEN: u64 = 0x34;
/// First of the five console aux words (0x38..0x48) the ROM's register table
/// lists and its observed console path never touches.
pub const S5L_AUX_FIRST: u64 = 0x38;
/// Last of the five console aux words.
pub const S5L_AUX_LAST: u64 = 0x48;
/// Transmit-outstanding count: written with a byte count, then drains to zero
/// as the transmitter retires those bytes.
pub const S5L_TXOUT: u64 = 0x4c;

/// Width of the transmit-FIFO occupancy field reported at 0x08, expressed as
/// the largest count it can show. The Samsung FIFO-status count this comes from
/// is four bits wide (`UFSTAT_TXCNT = GENMASK(7,4)`) for the same 16-entry
/// FIFO, so a full transmit path reports 15 and the SecureROM's
/// `16 - (value >> 6)` always leaves at least one slot -- the guest can never
/// compute zero room and spin.
pub const S5L_TX_FIFO_COUNT_MAX: u32 = 0xf;
/// Transmit holding-register depth the count field is sized for (16 entries),
/// as declared by the mainline `apple,s5l-uart` driver (`fifosize = 16`).
pub const S5L_TX_FIFO_DEPTH: usize = 16;
/// Shift of the transmit-occupancy count reported at 0x08.
pub const S5L_UFCON_TXFIFO_SHIFT: u32 = 6;
/// Shift of the read-back depth reported at 0x08.
pub const S5L_UFCON_READBACK_SHIFT: u32 = 11;

pub const UART_UTXD: u64 = 0x00;
pub const UART_UBRDIV: u64 = 0x04;
pub const UART_ULCON: u64 = 0x08;
pub const UART_UCON: u64 = 0x0c;
pub const UART_UFCON: u64 = 0x10;
pub const UART_UTRSTAT: u64 = 0x14;
pub const UART_UTXH: u64 = 0x18;
pub const UART_URXH: u64 = 0x1c;
pub const UART_UBRDIV2: u64 = 0x20;
pub const UART_UDIVSLOT: u64 = 0x24;

fn valid_size(s: usize) -> bool {
    matches!(s, 1 | 2 | 4 | 8)
}

/// Capture-only serial console. Every byte written to a TX register is
/// appended to an internal log so the emulator can surface firmware boot
/// output without modeling a host terminal.
pub struct SerialConsole {
    id: DeviceId,
    tx: Vec<u8>,
}

impl SerialConsole {
    pub fn new(id: DeviceId) -> Self {
        Self { id, tx: Vec::new() }
    }

    /// Drain every character produced so far.
    pub fn take_log(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }

    pub fn log_len(&self) -> usize {
        self.tx.len()
    }
}

impl Device for SerialConsole {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "serial-console"
    }

    fn reset(&mut self) {
        self.tx.clear();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !valid_size(size) {
            return DeviceResult::Fault;
        }
        match offset {
            UART_UTXD | UART_URXH => DeviceResult::Value(0),
            // UTRSTAT: RX empty, TX not busy, TX empty — firmware can always
            // transmit and never has data to read back.
            UART_UTRSTAT => DeviceResult::Value(0x5),
            _ => DeviceResult::Value(0),
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !valid_size(size) {
            return DeviceResult::Fault;
        }
        match offset {
            UART_UTXD | UART_UTXH => {
                let byte = value as u8;
                // Drivers may write either a byte or a whole word; emit the
                // low byte. A NUL is harmless to skip — most consoles treat
                // it as terminal output and it dirties the capture.
                if byte != 0 {
                    self.tx.push(byte);
                }
                DeviceResult::Ok
            }
            _ => DeviceResult::Ok,
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

impl Default for SerialConsole {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Capture-only Apple S5L serial console with the `s5l0` register map.
///
/// Phase 208 adds a cycle-accurate TX shift register: a byte written to UTXH
/// enters a holding queue, then a shift register drains it one bit per
/// `bit_cycles` (derived from the baud divisor). UTRSTAT bit2 (TX shift
/// empty) only clears while a byte is mid-shift, so a firmware poll loop
/// that waits on bit2 sees the real serial-engine delay instead of an
/// always-empty register. The RX path remains immediate (host-injected
/// bytes are available on the next URXH read).
pub struct S5lUart {
    id: DeviceId,
    tx: Vec<u8>,
    rx: VecDeque<u8>,
    ulcon: u32,
    ucon: u32,
    ufcon: u32,
    umcon: u32,
    brdiv: u32,
    divslot: u32,
    /// 0x30: the clock/baud ratio the SecureROM console programs at init.
    clkdiv: u32,
    /// 0x34: guest-owned console word.
    txlen: u32,
    /// 0x38..0x48: the console aux words the ROM's register table lists.
    aux: [u32; 5],
    /// 0x4c: transmit-outstanding count, armed by a guest write and retired
    /// byte by byte as the shift register drains.
    tx_outstanding: u32,
    /// Bytes the guest has transmitted that the console self-test can read back
    /// at 0x20.
    tx_readback: VecDeque<u8>,
    // Phase 208: cycle-accurate TX shift register.
    /// UART input clock frequency (Hz). The S5L arm-io UARTs run off a
    /// 100 MHz peripheral clock; the SecureROM programs UBRDIV/UDIVSLOT
    /// relative to this.
    pub uart_clock_hz: u64,
    /// Bytes waiting in the TX holding register (FIFO), not yet shifted.
    tx_hold: VecDeque<u8>,
    /// Byte currently being shifted out (None when idle).
    tx_shift: Option<u8>,
    /// Bits remaining in the current shift (8 = start bit, then 8 data bits,
    /// then stop bit; we count data bits 0..=7 for simplicity).
    shift_bits: u8,
    /// Cycles remaining within the current bit period.
    bit_remain: u64,
    /// Cycles per bit at the current baud rate. For the S5L Exynos UART,
    /// baud = uart_clock / (brdiv * 16), so cycles per bit = brdiv * 16.
    pub bit_cycles: u64,
    /// Whether the TX shift register is empty (UTRSTAT bit2).
    tx_shift_empty: bool,
}

impl S5lUart {
    pub fn new(id: DeviceId, _name: &'static str) -> Self {
        Self {
            id,
            tx: Vec::new(),
            rx: VecDeque::new(),
            ulcon: 0,
            ucon: 0,
            ufcon: 0,
            umcon: 0,
            brdiv: 0,
            divslot: 0,
            clkdiv: 0,
            txlen: 0,
            aux: [0; 5],
            tx_outstanding: 0,
            tx_readback: VecDeque::new(),
            // 100 MHz peripheral clock; 115200 baud => brdiv 34 => 544 cycles/bit.
            uart_clock_hz: 100_000_000,
            tx_hold: VecDeque::new(),
            tx_shift: None,
            shift_bits: 0,
            bit_remain: 544,
            bit_cycles: 544,
            tx_shift_empty: true,
        }
    }

    /// Recompute `bit_cycles` from the current baud divisor. The S5L baud
    /// rate is `uart_clock / (brdiv * 16)`, so cycles per bit = brdiv * 16.
    fn recompute_baud(&mut self) {
        if self.brdiv == 0 {
            return;
        }
        self.bit_cycles = self.brdiv as u64 * 16;
        self.bit_remain = self.bit_cycles;
    }

    /// Drain bytes the transmitter has started sending. Host observation must
    /// never remove bytes from the guest-visible TX holding register: those
    /// still have to advance through the shift register on device ticks.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.tx)
    }

    pub fn output_len(&self) -> usize {
        self.tx.len()
    }

    /// A copy of the captured transmit bytes, without draining them. Host
    /// diagnostics use this so observing the guest's console output cannot
    /// change what the guest still has to shift out.
    pub fn captured_bytes(&self) -> Vec<u8> {
        self.tx.clone()
    }

    /// Queue host/system input for the guest RX holding register.
    pub fn inject_rx(&mut self, bytes: &[u8]) {
        self.rx.extend(bytes.iter().copied());
    }

    /// Number of bytes still waiting in the TX holding register.
    pub fn tx_hold_len(&self) -> usize {
        self.tx_hold.len()
    }

    /// Whether the TX shift register is currently busy (UTRSTAT bit2 clear).
    pub fn tx_shift_busy(&self) -> bool {
        !self.tx_shift_empty
    }

    /// The clock/baud ratio the guest programmed at 0x30.
    pub fn clkdiv(&self) -> u32 {
        self.clkdiv
    }

    /// The guest-owned console word at 0x34.
    pub fn txlen(&self) -> u32 {
        self.txlen
    }

    /// Transmit-outstanding count at 0x4c: the byte count the guest armed with
    /// its last write, less the bytes the transmitter has finished sending.
    pub fn tx_outstanding(&self) -> u32 {
        self.tx_outstanding
    }

    /// Bytes the guest has queued in the transmit FIFO but which have not yet
    /// started shifting.
    pub fn tx_pending(&self) -> usize {
        self.tx_hold.len()
    }

    /// Transmit free space as the SecureROM console computes it: `16 - count`
    /// where count is the saturating occupancy the guest reads from 0x08.
    pub fn tx_free_space(&self) -> u32 {
        let count = (self.tx_hold.len() as u32).min(S5L_TX_FIFO_COUNT_MAX);
        (S5L_TX_FIFO_DEPTH as u32).saturating_sub(count)
    }

    /// Index into the 0x38..0x48 aux words, if the offset is one of them.
    fn aux_index(offset: u64) -> Option<usize> {
        if (S5L_AUX_FIRST..=S5L_AUX_LAST).contains(&offset) && offset.is_multiple_of(4) {
            Some(((offset - S5L_AUX_FIRST) / 4) as usize)
        } else {
            None
        }
    }

    /// Load the next byte from the holding register into the shift register.
    /// The shift register then drains one bit per `bit_cycles` via `tick_tx`.
    fn start_shift(&mut self) {
        if let Some(byte) = self.tx_hold.pop_front() {
            self.tx_shift = Some(byte);
            self.shift_bits = 8;
            self.bit_remain = self.bit_cycles;
            self.tx_shift_empty = false;
            // The shifted byte is also appended to the capture log so the
            // emulator can surface firmware boot output.
            self.tx.push(byte);
        } else {
            // Nothing left to shift: clear the in-flight byte as well as the
            // busy flag, so a later tick cannot keep draining a byte that has
            // already been transmitted.
            self.tx_shift = None;
            self.shift_bits = 0;
            self.tx_shift_empty = true;
        }
    }

    /// Advance the TX shift register by `cycles` machine cycles. One bit is
    /// consumed every `bit_cycles`; after 8 bits the byte is complete and the
    /// next byte in the holding register begins shifting (or the shift
    /// register goes empty). Each completed byte also retires one unit of the
    /// transmit-outstanding count the guest armed at 0x4c.
    pub fn tick_tx(&mut self, cycles: u64) {
        if self.tx_shift.is_none() {
            return;
        }
        let mut remaining = cycles;
        while remaining > 0 && self.tx_shift.is_some() {
            if self.bit_remain > remaining {
                self.bit_remain -= remaining;
                remaining = 0;
            } else {
                remaining -= self.bit_remain;
                self.bit_remain = self.bit_cycles;
                self.shift_bits -= 1;
                if self.shift_bits == 0 {
                    // The byte has left the shift register, so it is no longer
                    // outstanding from the transmitter's point of view.
                    self.tx_outstanding = self.tx_outstanding.saturating_sub(1);
                    // Current byte fully shifted; start the next one.
                    self.start_shift();
                }
            }
        }
    }
}

const fn uart_mask(size: usize) -> u64 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        _ => 0xffff_ffff,
    }
}

impl Device for S5lUart {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "s5l-uart"
    }

    fn reset(&mut self) {
        self.tx.clear();
        self.rx.clear();
        self.ulcon = 0;
        self.ucon = 0;
        self.ufcon = 0;
        self.umcon = 0;
        self.brdiv = 0;
        self.divslot = 0;
        self.clkdiv = 0;
        self.txlen = 0;
        self.aux = [0; 5];
        self.tx_outstanding = 0;
        self.tx_readback.clear();
        self.tx_hold.clear();
        self.tx_shift = None;
        self.shift_bits = 0;
        self.bit_remain = 544;
        self.tx_shift_empty = true;
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4) {
            return DeviceResult::Fault;
        }
        match offset {
            S5L_ULCON => DeviceResult::Value(u64::from(self.ulcon) & uart_mask(size)),
            S5L_UCON => DeviceResult::Value(u64::from(self.ucon) & uart_mask(size)),
            S5L_UFCON => {
                // The guest's own bits read back, with the live transmit
                // occupancy reported in bits 9:6 -- the field the SecureROM
                // console derives its free space from (`16 - (value >> 6)`).
                // The field saturates, so the guest's subtraction can never
                // reach zero (which its loop would spin on) and a queued
                // message is never mistaken for an empty one.
                let fifo = (self.tx_hold.len() as u32).min(S5L_TX_FIFO_COUNT_MAX);
                let readback = (self.tx_readback.len() as u32).min(S5L_TX_FIFO_COUNT_MAX);
                let value = (self.ufcon
                    & !((0xf << S5L_UFCON_TXFIFO_SHIFT) | (0xf << S5L_UFCON_READBACK_SHIFT)))
                    | (fifo << S5L_UFCON_TXFIFO_SHIFT)
                    | (readback << S5L_UFCON_READBACK_SHIFT);
                DeviceResult::Value(u64::from(value) & uart_mask(size))
            }

            S5L_UMCON => DeviceResult::Value(u64::from(self.umcon) & uart_mask(size)),
            S5L_TXDATA => {
                // Load side: bit0 TX ready (always), bit1 RX data present,
                // bit2 TX shift empty. Bit 2 is what the iOS 12 console driver
                // polls for transmit-complete.
                let rx_pending = if self.rx.is_empty() { 0 } else { 0x2 };
                let tx_shift_empty = if self.tx_shift_empty { 0x4 } else { 0 };
                DeviceResult::Value(0x1 | rx_pending | tx_shift_empty)
            }
            S5L_UERSTAT => DeviceResult::Value(0),
            // FIFO state: RX bytes count in bits 15:8, TX queue always empty.
            S5L_UFSTAT => DeviceResult::Value((self.rx.len() as u64 & 0xff) << 8),
            S5L_UMSTAT => DeviceResult::Value(0),
            // 0x20 reads back what the guest has transmitted, oldest first.
            S5L_UTXH => DeviceResult::Value(self.tx_readback.pop_front().unwrap_or(0) as u64),
            S5L_URXH => DeviceResult::Value(self.rx.pop_front().unwrap_or(0) as u64),
            S5L_UBRDIV => DeviceResult::Value(u64::from(self.brdiv) & uart_mask(size)),
            S5L_UDIVSLOT => DeviceResult::Value(u64::from(self.divslot) & uart_mask(size)),
            S5L_UCLKDIV => DeviceResult::Value(u64::from(self.clkdiv) & uart_mask(size)),
            S5L_TXLEN => DeviceResult::Value(u64::from(self.txlen) & uart_mask(size)),
            S5L_TXOUT => DeviceResult::Value(u64::from(self.tx_outstanding) & uart_mask(size)),
            _ => match Self::aux_index(offset) {
                Some(index) => DeviceResult::Value(u64::from(self.aux[index]) & uart_mask(size)),
                None => DeviceResult::Fault,
            },
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !matches!(size, 1 | 2 | 4) {
            return DeviceResult::Fault;
        }
        let value = value as u32;
        match offset {
            S5L_ULCON => {
                self.ulcon = value;
                DeviceResult::Ok
            }
            S5L_UCON => {
                self.ucon = value;
                DeviceResult::Ok
            }
            S5L_UFCON => {
                self.ufcon = value;
                DeviceResult::Ok
            }
            S5L_UMCON => {
                self.umcon = value;
                DeviceResult::Ok
            }
            // Status/interrupt registers: error and interrupt-pending sets are
            // treated as W1C no-ops (no error or interrupt is ever raised).
            S5L_UERSTAT | S5L_UFSTAT | S5L_UMSTAT => DeviceResult::Ok,
            S5L_TXDATA | S5L_UTXH => {
                // A whole-word store transacts its low byte (one character),
                // as a real data register would. Every byte the guest offers is
                // transmitted, including NUL: the SecureROM console writes a
                // single 0x00 byte while arming its 0x4c count, and a model
                // that discarded it would leave that countdown armed forever
                // with nothing in the pipe to retire it. (The older
                // `SerialConsole` capture model still drops NULs, because
                // nothing executes against it.)
                let byte = value as u8;
                self.tx_hold.push_back(byte);
                self.tx_readback.push_back(byte);
                if self.tx_shift.is_none() {
                    self.start_shift();
                }
                DeviceResult::Ok
            }

            // URXH is read-only from the guest.
            S5L_URXH => DeviceResult::Ok,
            S5L_UBRDIV => {
                self.brdiv = value;
                self.recompute_baud();
                DeviceResult::Ok
            }
            S5L_UDIVSLOT => {
                self.divslot = value;
                DeviceResult::Ok
            }
            S5L_UCLKDIV => {
                self.clkdiv = value;
                DeviceResult::Ok
            }
            S5L_TXLEN => {
                self.txlen = value;
                DeviceResult::Ok
            }
            S5L_TXOUT => {
                // Arms the transmit-outstanding countdown the SecureROM
                // console polls until it reads zero.
                self.tx_outstanding = value;
                DeviceResult::Ok
            }
            _ => match Self::aux_index(offset) {
                Some(index) => {
                    self.aux[index] = value;
                    DeviceResult::Ok
                }
                None => DeviceResult::Fault,
            },
        }
    }

    fn tick(&mut self, cycles: u64) -> Option<DeviceEvent> {
        // Phase 208: advance the TX shift register by the tick budget.
        self.tick_tx(cycles);
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
    fn captures_tx_bytes_at_both_holding_registers() {
        let mut uart = SerialConsole::new(7);
        assert_eq!(uart.log_len(), 0);
        assert_eq!(uart.take_log(), Vec::<u8>::new());

        assert_eq!(uart.write(UART_UTXH, 1, b'H' as u64), DeviceResult::Ok);
        // A word write transacts only its low byte (one character), as a
        // real UTXH would.
        assert_eq!(
            uart.write(UART_UTXH, 4, u64::from(u32::from_le_bytes(*b"ez\0\0"))),
            DeviceResult::Ok
        );
        assert_eq!(uart.write(UART_UTXH, 1, b'l' as u64), DeviceResult::Ok);
        assert_eq!(uart.write(UART_UTXD, 1, b'o' as u64), DeviceResult::Ok);
        assert_eq!(uart.write(UART_URXH, 1, b'x' as u64), DeviceResult::Ok);

        // NULs (written inside a word) are suppressed; URXH is RX, not TX.
        assert_eq!(uart.take_log(), b"Helo".to_vec());
        assert_eq!(uart.log_len(), 0);

        uart.reset();
        assert_eq!(uart.take_log(), Vec::<u8>::new());
    }

    #[test]
    fn status_reports_tx_empty_and_rx_empty() {
        let mut uart = SerialConsole::new(7);
        assert_eq!(uart.read(UART_UTRSTAT, 4), DeviceResult::Value(0x5));
        assert_eq!(uart.read(UART_UTXH, 1), DeviceResult::Value(0));
        assert_eq!(uart.read(UART_URXH, 1), DeviceResult::Value(0));
        // Config registers read back as zero (driver writes are drops).
        assert_eq!(uart.write(UART_UCON, 4, 0x1234), DeviceResult::Ok);
        assert_eq!(uart.write(UART_UBRDIV, 4, 0x30), DeviceResult::Ok);
        assert_eq!(uart.read(UART_UCON, 4), DeviceResult::Value(0));
    }

    #[test]
    fn invalid_sizes_fault_like_devices() {
        let mut uart = SerialConsole::new(7);
        assert_eq!(uart.read(UART_UTRSTAT, 3), DeviceResult::Fault);
        assert_eq!(uart.write(UART_UTXH, 16, 0), DeviceResult::Fault);
    }

    #[test]
    fn s5l_config_writes_do_not_transmit_and_round_trip() {
        let mut uart = S5lUart::new(7, "uart1");
        // The kernel's UART1 init sequence at 0x235204000: ULCON/UCON/UFCON/
        // UMCON/UBRDIV/UDIVSLOT are configuration stores — none may appear in
        // the TX capture.
        assert_eq!(uart.write(S5L_ULCON, 4, 0x3), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UCON, 4, 0x1245), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UFCON, 4, 0x11), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UMCON, 4, 0), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UBRDIV, 4, 0x30), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UDIVSLOT, 4, 0x5555), DeviceResult::Ok);
        assert_eq!(uart.output_len(), 0);
        assert_eq!(uart.read(S5L_ULCON, 4), DeviceResult::Value(0x3));
        assert_eq!(uart.read(S5L_UCON, 4), DeviceResult::Value(0x1245));
        assert_eq!(uart.read(S5L_UBRDIV, 4), DeviceResult::Value(0x30));
        assert_eq!(uart.read(S5L_UDIVSLOT, 4), DeviceResult::Value(0x5555));
    }

    #[test]
    fn s5l_tx_status_and_rx_flow_control() {
        let mut uart = S5lUart::new(7, "uart1");
        // UTRSTAT: TX ready (bit0) + TX shift empty (bit2), RX empty.
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
        // Inject input: bit1 (RX data present) sets and UFSTAT counts it.
        uart.inject_rx(b"H");
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x7));
        assert_eq!(uart.read(S5L_UFSTAT, 4), DeviceResult::Value(0x1 << 8));
        // URXH pops one byte; status returns to RX empty.
        assert_eq!(uart.read(S5L_URXH, 1), DeviceResult::Value(b'H' as u64));
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
        assert_eq!(uart.read(S5L_URXH, 1), DeviceResult::Value(0));
        uart.inject_rx(b"ab");
        assert_eq!(uart.drain_output(), Vec::<u8>::new());
    }

    #[test]
    fn s5l_tx_only_from_tx_holding_register() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_UTXH, 1, b'D' as u64), DeviceResult::Ok);
        // Whole-word store transacts the low byte; NULs are suppressed.
        assert_eq!(
            uart.write(S5L_UTXH, 4, u64::from(u32::from_le_bytes(*b"arm\0"))),
            DeviceResult::Ok
        );
        // ULCON (offset 0x00) is configuration, never a transmit character.
        assert_eq!(
            uart.write(S5L_ULCON, 4, u64::from(u32::from_le_bytes(*b"cfg\0"))),
            DeviceResult::Ok
        );
        assert_eq!(uart.drain_output(), b"D".to_vec());
        assert_eq!(uart.tx_hold_len(), 1);
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.drain_output(), b"a".to_vec());
    }

    #[test]
    fn s5l_invalid_accesses_and_unknown_offsets_fault() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.read(S5L_TXDATA, 8), DeviceResult::Fault);
        assert_eq!(uart.write(S5L_UTXH, 16, 0), DeviceResult::Fault);
        // The modelled window ends where the SecureROM console's register
        // table ends (0x4c); an offset past it is still a fault, as is an
        // unaligned one inside the aux block.
        assert_eq!(uart.read(0x50, 4), DeviceResult::Fault);
        assert_eq!(uart.write(0x50, 4, 1), DeviceResult::Fault);
        assert_eq!(uart.read(0x3a, 4), DeviceResult::Fault);
        uart.reset();
        assert_eq!(uart.output_len(), 0);
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
    }

    /// The SecureROM console's register table at image `0x10001c5a0` lists
    /// exactly fourteen offsets for this window. Each must be readable and
    /// writable, and 0x4c must report the count the guest armed.
    #[test]
    fn rom_console_register_table_offsets_are_all_modeled() {
        let mut uart = S5lUart::new(7, "uart1");
        // The storage words echo; 0x08/0x10/0x20/0x4c keep their own behaviour
        // and are checked separately below.
        for (index, offset) in [
            S5L_ULCON,
            S5L_UCON,
            S5L_UMCON,
            S5L_UCLKDIV,
            S5L_TXLEN,
            S5L_AUX_FIRST,
            S5L_AUX_FIRST + 4,
            S5L_AUX_FIRST + 8,
            S5L_AUX_FIRST + 12,
            S5L_AUX_FIRST + 16,
        ]
        .into_iter()
        .enumerate()
        {
            let value = 0x11 * (index as u64 + 1);
            assert_eq!(
                uart.write(offset, 4, value),
                DeviceResult::Ok,
                "write at {offset:#x}"
            );
            assert_eq!(
                uart.read(offset, 4),
                DeviceResult::Value(value & uart_mask(4)),
                "read at {offset:#x}"
            );
        }
        // The remaining table entries are accepted too, with their own
        // behaviour: a live occupancy word and the transmit count.
        for offset in [S5L_UFCON, S5L_TXOUT] {
            assert_eq!(uart.write(offset, 4, 2), DeviceResult::Ok, "{offset:#x}");
            assert_eq!(
                uart.read(offset, 4),
                DeviceResult::Value(2),
                "read at {offset:#x}"
            );
        }
        // 0x10 is the combined data/status register: the store transmits, the
        // load reports the transfer status the kernel polls.
        assert_eq!(uart.write(S5L_TXDATA, 4, 2), DeviceResult::Ok);
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x1));
        assert_eq!(uart.write(S5L_UTXH, 4, 3), DeviceResult::Ok);
        assert_eq!(uart.clkdiv(), 0x44, "0x30 holds the ROM clock ratio");
        assert_eq!(uart.txlen(), 0x55, "0x34 holds the guest console word");
        assert_eq!(uart.tx_outstanding(), 2, "0x4c holds the armed count");
        assert_eq!(uart.drain_output(), vec![2]);
        // The second character is queued behind the first, as on real
        // hardware: it reaches the capture only when it starts shifting.
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.drain_output(), vec![3]);
    }
    /// A store to 0x10 transmits a character (SecureROM console PIO loop) and a
    /// load of 0x10 returns the transfer status (iOS 12 console driver poll).
    /// The same offset therefore has to do both.
    #[test]
    fn s5l_combined_data_and_status_register_serves_both_firmwares() {
        let mut uart = S5lUart::new(7, "uart1");
        // Empty: TX ready (bit0) and TX shift empty (bit2).
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
        // The ROM stores a character here; it goes into the transmit path.
        assert_eq!(uart.write(S5L_TXDATA, 4, u64::from(b'K')), DeviceResult::Ok);
        assert_eq!(uart.drain_output(), b"K".to_vec());
        // And the status load now reports the byte in flight (bit2 clear).
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x1));
        // Once the byte has drained, transmit-complete returns.
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
    }

    /// 0x4c is written with a byte count before the ROM pushes those bytes,
    /// then polled until it reads zero, so it must drain with the transmitter
    /// rather than staying latched at the armed value.
    #[test]
    fn rom_console_transmit_count_drains_with_the_transmitter() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_TXOUT, 4, 2), DeviceResult::Ok);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(2));
        // The ROM arms the count, then pushes the bytes to its data register.
        assert_eq!(uart.write(S5L_TXDATA, 1, b'A' as u64), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_TXDATA, 1, b'B' as u64), DeviceResult::Ok);
        // Pushing does not retire the count: the bytes are still in the pipe.
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(2));

        let bit_cycles = uart.bit_cycles;
        // First byte completes: one unit retires.
        uart.tick_tx(bit_cycles * 8);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(1));
        // Second byte completes: the count the ROM polls reaches zero.
        uart.tick_tx(bit_cycles * 8);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(0));
        assert_eq!(uart.drain_output(), b"AB".to_vec());
        // The count cannot be driven negative by further draining.
        uart.tick_tx(bit_cycles * 8);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(0));
    }

    /// The SecureROM console's first transmit is a single `0x00` byte with the
    /// 0x4c count armed to 1. A model that drops NUL bytes leaves that
    /// countdown with nothing to retire, so the guest spins on 0x4c forever.
    #[test]
    fn rom_console_transmits_a_nul_byte_and_retires_its_count() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_TXOUT, 4, 1), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_TXDATA, 4, 0), DeviceResult::Ok);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(1));
        assert!(uart.tx_shift_busy());
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.read(S5L_TXOUT, 4), DeviceResult::Value(0));
        assert_eq!(uart.drain_output(), vec![0]);
    }

    /// The ROM derives its transmit free space as `16 - (0x08 >> 6)`, so 0x08
    /// must report the live transmit occupancy in bits 9:6 while leaving the
    /// guest's own bits intact.
    #[test]
    fn rom_console_free_space_matches_the_transmit_occupancy_at_0x08() {
        let mut uart = S5lUart::new(7, "uart1");
        // Empty path: the ROM's arithmetic yields a full 16 bytes of room.
        assert_eq!(uart.read(S5L_UFCON, 4), DeviceResult::Value(0));
        assert_eq!(uart.tx_free_space(), 16);
        // A guest word of its own survives the read-modify-write.
        assert_eq!(uart.write(S5L_UFCON, 4, 0x11), DeviceResult::Ok);

        // The first byte goes straight into the shift register, so it is not
        // queued; every later byte joins the holding register. The read-back
        // depth in bits 14:11 is reported alongside it.
        for index in 0..40u32 {
            assert_eq!(
                uart.write(S5L_TXDATA, 1, b'x' as u64),
                DeviceResult::Ok,
                "byte {index}"
            );
            let occupancy = index.min(S5L_TX_FIFO_COUNT_MAX) as u64;
            let readback = (index + 1).min(S5L_TX_FIFO_COUNT_MAX) as u64;
            assert_eq!(
                uart.read(S5L_UFCON, 4),
                DeviceResult::Value(
                    0x11 | (occupancy << S5L_UFCON_TXFIFO_SHIFT)
                        | (readback << S5L_UFCON_READBACK_SHIFT)
                ),
                "occupancy after {index} bytes"
            );
        }
        // The count field saturates rather than wrapping, so a queued message
        // never reads back as an empty transmit path and the guest's
        // `16 - count` never reaches zero (its loop would spin on that).
        assert_eq!(uart.tx_hold_len(), 39);
        assert_eq!(uart.tx_free_space(), 1);
        // No queued byte is lost while the path is saturated.
        assert_eq!(uart.write(S5L_TXDATA, 1, b'y' as u64), DeviceResult::Ok);
        assert_eq!(uart.tx_hold_len(), 40);
        assert_eq!(uart.drain_output().len(), 1);
    }

    /// The SecureROM console self-test stores a pattern to the data register
    /// and reads it back from 0x20, bounded by the count in 0x08 bits 14:11.
    /// That loop cannot terminate while the count is zero, so 0x20 has to
    /// deliver what the guest transmitted.
    #[test]
    fn rom_console_self_test_reads_back_what_it_transmitted() {
        let mut uart = S5lUart::new(7, "uart1");
        for byte in b"abc" {
            assert_eq!(
                uart.write(S5L_TXDATA, 1, u64::from(*byte)),
                DeviceResult::Ok
            );
        }
        // Three bytes are available to read back (the shift register holds the
        // first, so only two are still queued), oldest first.
        assert_eq!(
            uart.read(S5L_UFCON, 4),
            DeviceResult::Value(3 << S5L_UFCON_READBACK_SHIFT | 2 << S5L_UFCON_TXFIFO_SHIFT)
        );
        assert_eq!(uart.read(S5L_UTXH, 1), DeviceResult::Value(u64::from(b'a')));
        assert_eq!(uart.read(S5L_UTXH, 1), DeviceResult::Value(u64::from(b'b')));
        assert_eq!(
            uart.read(S5L_UFCON, 4),
            DeviceResult::Value(1 << S5L_UFCON_READBACK_SHIFT | 2 << S5L_UFCON_TXFIFO_SHIFT)
        );
        assert_eq!(uart.read(S5L_UTXH, 1), DeviceResult::Value(u64::from(b'c')));
        // Drained: the read-back count is zero again and a read yields nothing.
        // The transmit-occupancy count is independent of it -- reading back
        // does not consume the bytes still queued for the transmitter.
        assert_eq!(
            uart.read(S5L_UFCON, 4),
            DeviceResult::Value(2 << S5L_UFCON_TXFIFO_SHIFT)
        );
        assert_eq!(uart.read(S5L_UTXH, 1), DeviceResult::Value(0));
    }

    // Phase 208: cycle-accurate TX shift register.

    /// A byte written to UTXH enters the holding register and immediately
    /// starts shifting; UTRSTAT bit2 (TX shift empty) clears until the byte
    /// has drained one bit per `bit_cycles`.
    #[test]
    fn s5l_tx_shift_register_clears_utrstat_bit2_until_byte_drains() {
        let mut uart = S5lUart::new(7, "uart1");
        // Default 115200 baud => brdiv 34 => 544 cycles/bit.
        assert_eq!(uart.write(S5L_UTXH, 1, b'A' as u64), DeviceResult::Ok);
        assert!(uart.tx_shift_busy(), "shift register should be busy");
        // TX ready (bit0) set, TX shift empty (bit2) clear while shifting.
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x1));

        let bit_cycles = uart.bit_cycles;
        // Drain one bit period; bit2 stays clear until 8 bits have shifted.
        for _ in 0..7 {
            uart.tick_tx(bit_cycles);
            assert!(uart.tx_shift_busy(), "still shifting");
        }
        // The 8th bit period completes the byte.
        uart.tick_tx(bit_cycles);
        assert!(!uart.tx_shift_busy(), "shift register empty after byte");
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
        assert_eq!(uart.drain_output(), b"A".to_vec());
    }

    /// A second byte queued while the first is shifting waits in the holding
    /// register and begins shifting immediately after the first completes.
    #[test]
    fn s5l_tx_hold_queue_drains_after_current_byte() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_UTXH, 1, b'A' as u64), DeviceResult::Ok);
        assert_eq!(uart.write(S5L_UTXH, 1, b'B' as u64), DeviceResult::Ok);
        assert_eq!(uart.tx_hold_len(), 1, "second byte waits in hold");

        let bit_cycles = uart.bit_cycles;
        // Drain the first byte; the second should start shifting.
        for _ in 0..8 {
            uart.tick_tx(bit_cycles);
        }
        assert_eq!(uart.tx_hold_len(), 0, "hold drained");
        assert!(uart.tx_shift_busy(), "second byte now shifting");
        // Drain the second byte.
        for _ in 0..8 {
            uart.tick_tx(bit_cycles);
        }
        assert!(!uart.tx_shift_busy(), "shift register empty");
        assert_eq!(uart.drain_output(), b"AB".to_vec());
    }

    #[test]
    fn s5l_host_drain_does_not_steal_pending_guest_tx_bytes() {
        let mut uart = S5lUart::new(7, "uart1");
        for byte in b"ABC" {
            assert_eq!(uart.write(S5L_UTXH, 1, u64::from(*byte)), DeviceResult::Ok);
        }
        assert_eq!(uart.drain_output(), b"A");
        assert_eq!(uart.tx_hold_len(), 2);
        assert!(uart.tx_shift_busy());
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.drain_output(), b"B");
        assert_eq!(uart.tx_hold_len(), 1);
        uart.tick_tx(uart.bit_cycles * 8);
        assert_eq!(uart.drain_output(), b"C");
        uart.tick_tx(uart.bit_cycles * 8);
        assert!(!uart.tx_shift_busy());
        assert_eq!(uart.read(S5L_TXDATA, 4), DeviceResult::Value(0x5));
    }

    /// The Device::tick path advances the TX shift register by the tick
    /// budget, so a firmware poll loop that calls tick_devices sees the real
    /// serial-engine delay rather than an always-empty shift register.
    #[test]
    fn s5l_device_tick_advances_tx_shift() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_UTXH, 1, b'Z' as u64), DeviceResult::Ok);
        assert!(uart.tx_shift_busy());

        let bit_cycles = uart.bit_cycles;
        for _ in 0..7 {
            uart.tick(bit_cycles);
            assert!(uart.tx_shift_busy());
        }
        uart.tick(bit_cycles);
        assert!(!uart.tx_shift_busy());
    }

    /// Programming UBRDIV recomputes the per-bit cycle count, so a faster
    /// baud rate drains the shift register in fewer ticks.
    #[test]
    fn s5l_baud_divisor_changes_bit_cycles() {
        let mut uart = S5lUart::new(7, "uart1");
        assert_eq!(uart.write(S5L_UBRDIV, 4, 0x30), DeviceResult::Ok);
        let slow = uart.bit_cycles;
        assert_eq!(uart.write(S5L_UBRDIV, 4, 0x18), DeviceResult::Ok);
        let fast = uart.bit_cycles;
        assert!(fast < slow, "smaller divisor => fewer cycles/bit");
    }
}
