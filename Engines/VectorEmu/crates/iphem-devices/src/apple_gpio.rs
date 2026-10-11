//! Apple pinctrl/GPIO register subset used by T8020 firmware.
//!
//! Pin configuration words and grouped interrupt status follow the public
//! Apple GPIO binding and Linux driver. The 0x100000-byte aperture is larger
//! than the documented pin/IRQ register windows; unknown reads return zero
//! and unknown writes are ignored so traces can expose ROM-specific accesses.
//!
//! The SecureROM asserts a storage-device-presence strap by writing the
//! undocumented word at offset 0xc48 before sampling bit 0 of offset 0xc4c;
//! the T8020 boot chain refuses to continue to the DFU device table unless
//! bit 0 reads back as 1. The strap position is beyond every documented pin
//! config / IRQ-group window, so it is modelled as a platform-policy latch
//! (device present) rather than a measured silicon reset value.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::sync::{Arc, Mutex};

pub const APPLE_GPIO_PINS: usize = 512;
pub const APPLE_GPIO_IRQ_GROUPS: usize = 7;
pub const APPLE_GPIO_IRQ_BASE: u64 = 0x800;
const APPLE_GPIO_STRAP_SET: u64 = 0xc48;
const APPLE_GPIO_STRAP_READBACK: u64 = 0xc4c;
const GPIO_DATA: u32 = 1;
const GPIO_MODE_MASK: u32 = 0xe;
const GPIO_MODE_OUTPUT: u32 = 1 << 1;
const GPIO_IRQ_GROUP_SHIFT: u32 = 16;
const GPIO_IRQ_GROUP_MASK: u32 = 7 << GPIO_IRQ_GROUP_SHIFT;
const IRQ_WORDS: usize = APPLE_GPIO_PINS.div_ceil(32);

/// The register state of one Apple GPIO block.
///
/// Shared by address rather than copied per core, because the block is one
/// physical peripheral: `AppleGpio::paired` hands each of two `Machine`s a view
/// of the same `GpioState`, so a word one core writes is the word the other core
/// reads. See [`AppleGpio::paired`] for why the SEP needs its own view.
struct GpioState {
    pin_config: [u32; APPLE_GPIO_PINS],
    input_levels: [bool; APPLE_GPIO_PINS],
    irq_status: [[u32; IRQ_WORDS]; APPLE_GPIO_IRQ_GROUPS],
    device_present: bool,
}

impl GpioState {
    fn new() -> Self {
        Self {
            pin_config: [0; APPLE_GPIO_PINS],
            input_levels: [false; APPLE_GPIO_PINS],
            irq_status: [[0; IRQ_WORDS]; APPLE_GPIO_IRQ_GROUPS],
            device_present: true,
        }
    }
}

/// Apple GPIO pinmux and input model. External levels are injected through
/// [`AppleGpio::drive_input`]; output levels are controlled by pin register bit 0.
pub struct AppleGpio {
    pub id: DeviceId,
    state: Arc<Mutex<GpioState>>,
}

impl AppleGpio {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            state: Arc::new(Mutex::new(GpioState::new())),
        }
    }

    /// Two views of one GPIO block, for the two `Machine`s that can address it.
    ///
    /// The SEP and the AP are separate emulated cores with separate address
    /// spaces and separate buses, and the Cebu SEPROM polls **bit 0 of
    /// `0x23c100c4c`** as its very first gate. That address is not an SEP-local
    /// register: `DeviceTree.d321ap.im4p` places `/device-tree/arm-io/gpio` at
    /// arm-io child address `0x3c100000` with a 1 MiB aperture, and the base plus
    /// the arm-io offset is `0x23c100000`, so `+0xc4c` is this block's strap
    /// word -- the same offset the A12 SecureROM samples at `0x1000031a8` and
    /// stores at `0x100003248`. The SEPROM reaches the same SoC band elsewhere
    /// too (`0x23d2bc000`, `0x23d2e8000`), so it addresses the AP's peripherals
    /// rather than a private copy of them.
    ///
    /// Two `Machine`s cannot share a `Box<dyn Device>`, so before this the block
    /// existed once on the AP and the SEP's `0x23c100c4c` was backed by whatever
    /// plain memory happened to be there -- reading zero and leaving the SEPROM
    /// in its bounded wait. Sharing the state is what makes the two views one
    /// register; it does not decide what the register should read. That answer
    /// belongs to `GpioState::device_present`, which predates this method and is
    /// documented at [`APPLE_GPIO_STRAP_READBACK`].
    pub fn paired(id: DeviceId) -> (Self, Self) {
        let state = Arc::new(Mutex::new(GpioState::new()));
        (
            Self {
                id,
                state: Arc::clone(&state),
            },
            Self {
                id,
                state,
            },
        )
    }

    /// Another view of the same block, for a second `Machine`.
    pub fn peer(&self) -> Self {
        Self {
            id: self.id,
            state: Arc::clone(&self.state),
        }
    }

    /// True when a configured pin's mode makes this edge/level a latched IRQ.
    ///
    /// Shared with [`GpioState::latch`] so the mode table has exactly one
    /// definition, whichever view of the block is driven.
    fn is_active(config: u32, previous: bool, level: bool) -> bool {
        match (config & GPIO_MODE_MASK) >> 1 {
            2 => level,              // level high
            3 => !level,             // level low
            4 => !previous && level, // rising edge
            5 => previous && !level, // falling edge
            6 => previous != level,  // either edge
            _ => false,
        }
    }

    /// Set an externally driven pin level and latch configured edge/level IRQs.
    pub fn drive_input(&mut self, pin: usize, level: bool) {
        let mut state = self.state.lock().unwrap();
        state.drive_input(pin, level);
    }

    fn read_word(&self, offset: u64) -> u32 {
        let state = self.state.lock().unwrap();
        state.read_word(offset)
    }
}

impl GpioState {
    fn drive_input(&mut self, pin: usize, level: bool) {
        let Some(config) = self.pin_config.get(pin).copied() else {
            return;
        };
        let previous = self.input_levels[pin];
        self.input_levels[pin] = level;
        if AppleGpio::is_active(config, previous, level) {
            let group = ((config & GPIO_IRQ_GROUP_MASK) >> GPIO_IRQ_GROUP_SHIFT) as usize;
            self.irq_status[group][pin / 32] |= 1 << (pin % 32);
        }
    }

    fn read_word(&self, offset: u64) -> u32 {
        if offset < APPLE_GPIO_IRQ_BASE {
            let pin = (offset / 4) as usize;
            let Some(config) = self.pin_config.get(pin).copied() else {
                return 0;
            };
            if config & GPIO_MODE_MASK == GPIO_MODE_OUTPUT {
                config
            } else {
                (config & !GPIO_DATA) | u32::from(self.input_levels[pin])
            }
        } else if offset < APPLE_GPIO_IRQ_BASE + APPLE_GPIO_IRQ_GROUPS as u64 * 0x40 {
            let relative = offset - APPLE_GPIO_IRQ_BASE;
            let group = (relative / 0x40) as usize;
            let word = ((relative % 0x40) / 4) as usize;
            self.irq_status
                .get(group)
                .and_then(|words| words.get(word))
                .copied()
                .unwrap_or(0)
        } else if offset == APPLE_GPIO_STRAP_READBACK {
            u32::from(self.device_present)
        } else {
            0
        }
    }
}

impl Device for AppleGpio {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "apple-gpio"
    }

    fn reset(&mut self) {
        *self.state.lock().unwrap() = GpioState::new();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if size != 4 || offset & 3 != 0 {
            return DeviceResult::Fault;
        }
        DeviceResult::Value(self.read_word(offset) as u64)
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if size != 4 || offset & 3 != 0 {
            return DeviceResult::Fault;
        }
        let value = value as u32;
        let mut state = self.state.lock().unwrap();
        if offset < APPLE_GPIO_IRQ_BASE {
            let pin = (offset / 4) as usize;
            if let Some(config) = state.pin_config.get_mut(pin) {
                *config = value;
            }
        } else if offset < APPLE_GPIO_IRQ_BASE + APPLE_GPIO_IRQ_GROUPS as u64 * 0x40 {
            let relative = offset - APPLE_GPIO_IRQ_BASE;
            let group = (relative / 0x40) as usize;
            let word = ((relative % 0x40) / 4) as usize;
            if let Some(status) = state
                .irq_status
                .get_mut(group)
                .and_then(|words| words.get_mut(word))
            {
                *status &= !value;
            }
        } else if offset == APPLE_GPIO_STRAP_SET {
            // The ROM writes the strap assertion (bit 0) here; the device
            // latch reflects physical storage presence.
            state.device_present = value & 1 != 0;
        } else if offset == APPLE_GPIO_STRAP_READBACK {
            // Readback-only: writes are ignored (owned by read_word path).
        }
        DeviceResult::Ok
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_config_and_external_input_follow_apple_register_layout() {
        let mut gpio = AppleGpio::new(1);
        let reg = 123 * 4;
        assert_eq!(gpio.read(reg, 4), DeviceResult::Value(0));
        assert_eq!(gpio.write(reg, 4, 1 << 9), DeviceResult::Ok); // input-enable
        assert_eq!(gpio.read(reg, 4), DeviceResult::Value(1 << 9));
        gpio.drive_input(123, true);
        assert_eq!(
            gpio.read(reg, 4),
            DeviceResult::Value((1 << 9) | GPIO_DATA as u64)
        );
        gpio.write(reg, 4, GPIO_MODE_OUTPUT as u64 | GPIO_DATA as u64);
        gpio.drive_input(123, false);
        assert_eq!(gpio.read(reg, 4), DeviceResult::Value(3));
    }

    #[test]
    fn grouped_gpio_irqs_latch_and_clear_write_one_to_clear() {
        let mut gpio = AppleGpio::new(1);
        let pin = 68;
        let group = 3;
        gpio.write(
            (pin * 4) as u64,
            4,
            ((group as u32) << GPIO_IRQ_GROUP_SHIFT | (6 << 1)) as u64,
        );
        gpio.drive_input(pin, true);
        let status = APPLE_GPIO_IRQ_BASE + group as u64 * 0x40 + (pin / 32 * 4) as u64;
        assert_eq!(gpio.read(status, 4), DeviceResult::Value(1 << (pin % 32)));
        gpio.write(status, 4, 1 << (pin % 32));
        assert_eq!(gpio.read(status, 4), DeviceResult::Value(0));
    }

    #[test]
    fn device_present_strap_reads_back_after_assert_and_bad_accesses_fault() {
        let mut gpio = AppleGpio::new(1);
        assert_eq!(gpio.read(0xc4c, 4), DeviceResult::Value(1));
        gpio.write(0xc4c, 4, 0);
        assert_eq!(gpio.read(0xc4c, 4), DeviceResult::Value(1));
        gpio.write(0xc48, 4, 0); // device strapped absent
        assert_eq!(gpio.read(0xc4c, 4), DeviceResult::Value(0));
        gpio.write(0xc48, 4, 1); // device strapped present again
        assert_eq!(gpio.read(0xc4c, 4), DeviceResult::Value(1));
        assert_eq!(gpio.read(1, 4), DeviceResult::Fault);
        assert_eq!(gpio.read(0, 8), DeviceResult::Fault);
    }
}
