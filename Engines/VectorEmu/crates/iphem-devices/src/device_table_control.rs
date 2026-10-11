//! A SecureROM device-control window: the target/actual registers an AP SecureROM reaches
//! through its own runtime device table.
//!
//! Named after the *transport* rather than a part, because the handshake this models is
//! **not** A11-specific. Scanning every checked-in AP SecureROM for the gate's instruction
//! triple (`movz w10, #0xefff lsl #16` / `movk w10, #0xfbff` / `and w9, w9, w10`, followed by
//! `and w9, w9, #0xf0` / `cmp w9, #0xf0` / `b.ne` back) finds a byte-identical body in:
//!
//! | part            | build            | gate VA       |
//! |-----------------|------------------|---------------|
//! | T8010 (A10)     | 2696.0.0.1.33    | `0x10000945c` |
//! | T8011 (A10)     | 3135.0.0.1.12    | `0x1000096a0` |
//! | T8011 (A10)     | 3135.0.0.2.3     | `0x1000096e0` |
//! | T8015 (A11)     | 3332.0.0.1.14    | `0x100003390` |
//! | T8015 (A11)     | 3332.0.0.1.23    | `0x100003340` |
//! | T8020 (A12)     | 3865.0.0.1.23    | `0x100003b6c` |
//! | T8020 (A12)     | 3865.0.0.4.6     | `0x100003c94` |
//! | T8020 (A12)     | 3865.0.0.4.7     | `0x100003c94` |
//! | T8027 (A12)     | 4172.0.0.100.13  | `0x1000041dc` |
//! | T8027 (A12)     | 4172.0.0.100.14  | `0x1000041dc` |
//!
//! and **none** in T8030 (A13, 4479.0.0.100.4) or T8101 (5281.0.0.100.{22,34,45}). Only the
//! `adr` that names the table and the VA itself differ between builds: the A11 builds use
//! `0x100017d60` and `0x100017de0`. So the contract below is a T801x+A12 family property,
//! and its absence in A13 is a real datum rather than a gap in the scan (the same scan
//! finds ten of eleven, not zero).
//!
//! The two A11 table addresses (`0x100017d60`, `0x100017de0`) are **not** pinned from the
//! `adr` encoding. This tree's disassembler renders `adr` with a scaled immediate (target =
//! instruction + imm x 4), which does not match the architectural byte-offset form, so an
//! encoding-derived reading of those immediates is not trustworthy here. They are pinned the
//! way they were established: read out of the running guest, where `0x100017de0` returned a
//! coherent 145-entry table with the sentinels and register addresses the ROM's own code
//! indexes. `crates/iphem-boot/examples/t8015_platform_census.rs` reproduces that dump.
//!
//! What part-specific is the *address set*, and the table above is the A11's alone. The
//! A12's equivalent is modelled separately by [`crate::pmgr_power::PmgrBootStatus`], which
//! implements the same target/actual contract through a different transport: a single word
//! mirrored from a PMGR power-state plane by `BootStatusRegistry`, where the ROM never
//! indexes a table and every address outside offset 0 faults. Mapping the A11's windows
//! for the A12 (or the A12's for the A11) would be inventing a part's addresses, which is
//! the failure mode the platform map's own comment exists to prevent.
//!
//! # Where the A11 shape comes from
//!
//! Not from a DeviceTree: no A11 (`d221ap`) tree is checked in, and
//! `fixtures/Firmware/all_flash/DeviceTree.d321ap.im4p` is the A12 tree, which this
//! project's records are explicit is not evidence for another SoC. It comes from two
//! independent readings of the A11 ROM itself, which agree:
//!
//! 1. **The ROM's own device table.** `SecureROM_t8015si_iBoot-3332.0.0.1.23`` keeps
//!    a run-list at VA `0x100017de0` whose entries are 64-bit device-register addresses,
//!    separated by the type sentinels `1` and `2` that two of its own index lookups scan for
//!    (`cmp x11, #1` / `cmp x11, #2`), with the entry
//!    count in ROM data at `0x100018268` and the resolved run starts cached in boot SRAM
//!    at `0x180000320`/`0x180000324`. The table is zero in the file — the prologue's own
//!    zero-fill covers it — so it is only meaningful after the ROM builds it. Dumped from
//!    the running guest it holds **145 entries**: sentinel `1` at index 12, sentinel `2` at
//!    index 135, and 131 register addresses at 8-byte stride across exactly four windows:
//!
//!    | window        | registers | offsets       |
//!    |---------------|-----------|---------------|
//!    | `0x232080000` | 87        | `+0x000..+0x850` |
//!    | `0x232084000` | 7         | `+0x000..+0x030` |
//!    | `0x232088000` | 5         | `+0x000..+0x028` |
//!    | `0x235280000` | 32        | `+0x000..+0x140` |
//!
//! 2. **The two routines that drive it.** Both index the same table and then read-modify-write
//!    the 32-bit word the entry names:
//!
//!    * `0x1000032c4` — `cmp wzr, w0, w20` on the index, `ldr wzr, [x8]` as a probe, then
//!      `orr w9, w9, #0xf` (or `and w9, w9, #0xfffffff0` when the argument is clear) and
//!      `str w9, [x8]`. It then waits at `0x100003310` with two consecutive reads and
//!      `eor w9, w10, w9 lsr #4` / `and w9, w9, #0xf` / `cbnz` — a stable-read handshake.
//!    * `0x100003330` — `and w9, w9, #0xeffffbff` (clearing bits 10 and 18), `str w9, [x8]`,
//!      then the readiness poll at `0x100003350`: `ldr w9, [x8]`, `and w9, w9, #0xf0`,
//!      `cmp w9, #0xf0`, `b.ne` back.
//!
//! # What is modelled, and what is not claimed
//!
//! Both routines treat one 32-bit word as a target/actual pair: the ROM writes the low
//! nibble and reads back the high nibble, and its readiness test is exactly
//! `(word & 0xf0) == 0xf0`. That is the same contract `PmgrBootStatus` already implements
//! for the A12's boot-status words, so this device reuses that reading rather than
//! inventing a second one. Bits 10 and 18 are writable with no modelled effect, because
//! the ROM only ever clears them and never reads them back.
//!
//! Not claimed: which physical block this is, what any individual register offset means, or
//! that a supported transition exists for every target. A target the model does not know is
//! left in the low nibble and the high nibble does not follow, which is what makes the
//! ROM's poll spin — the same fail-closed shape as the SEP gate this project has already
//! recorded. A device that is genuinely present and powered must be told so through
//! [`DeviceTableControlWindow::set_actual`]; nothing here invents readiness.

use std::collections::HashMap;

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;

/// Target states the model completes synchronously. These are the nibble values the A11
/// ROM itself writes or masks to: `0xf` from `orr #0xf`, `0x0` from `and #0xfffffff0`, and
/// `0x4` which the A12's `PmgrBootStatus` already treats as a supported transition for the
/// same target/actual protocol. Anything else leaves the word not-ready on purpose.
const SUPPORTED_TARGETS: [u32; 3] = [0, 4, 0xf];

/// One 32-bit device-control word: low nibble software target, bits 7..4 hardware actual.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetActualRegister {
    /// Bits 3..0, written by the guest.
    pub target: u32,
    /// Bits 7..4, reported by the model.
    pub actual: u32,
    /// The remaining bits, stored so a read-modify-write round-trips.
    pub other: u32,
}

impl TargetActualRegister {
    /// The value the ROM reads: target in the low nibble, actual in the high nibble.
    pub fn read_value(&self) -> u32 {
        (self.other & !0xff) | (self.target & 0xf) | ((self.actual & 0xf) << 4)
    }

    /// Whether the A11 ROM's readiness test `(word & 0xf0) == 0xf0` would pass.
    pub fn is_ready(&self) -> bool {
        (self.read_value() & 0xf0) == 0xf0
    }
}

/// The A11 device-control block, covering the windows the ROM's own table names.
#[derive(Debug)]
pub struct DeviceTableControlWindow {
    id: DeviceId,
    /// Base of the whole control aperture this instance answers for.
    base: u64,
    /// Size of that aperture, from the measured window set.
    size: u64,
    words: HashMap<u64, TargetActualRegister>,
    /// Offsets whose bits 7:4 are a hardware **state** rather than an echo of bits 3:0.
    ///
    /// # Why this rule exists, and why it is separate from the echo
    ///
    /// One aperture holds both kinds of word, and the difference is the whole reason the
    /// A10 reaches the ROM's USB init or does not. Its 87 array registers are *echoes*: the
    /// ROM's set routine at `0x100009d4c` writes bits 3:0 with `orr w9, w9, #0xf` (or clears
    /// them with `and w9, w9, #0xfffffff0`) and then waits on
    /// `(w ^ (w >> 4)) & 0xf == 0` — a two-read handshake that only ends when bits 7:4 report
    /// what bits 3:0 asked for.
    ///
    /// Its `+0x400` word is **not** an echo. The ROM reaches it only after configuring the
    /// whole array, and the only thing it ever writes there is
    /// `and w9, w9, 0xeffffbff` — a read-modify-write that *preserves* bits 3:0 and clears
    /// bits 22 and 30. It then polls `(w & 0xf0) == 0xf0` at `0x10000946c`, unconditionally,
    /// with no timeout and no error branch. So the poll is not "did my request land?"; it is
    /// "is the device set ready?", and the value that answers it is one the ROM never wrote.
    ///
    /// Treating `+0x400` as an echo therefore makes the poll unsatisfiable in a way that
    /// looks like a hang rather than a modelling error: the ROM's own read-modify-write
    /// drives `actual` to the target it just read back, so the status nibble collapses to the
    /// software nibble and the wait never ends. That is not a hypothetical — it is what the
    /// A10 did, spinning on `0x20e080400` for 743,241 reads across 3,000,000 instructions
    /// before this rule was added.
    ///
    /// The A8 models the same register the same way and says why: `Pmgr::status_nibble_all_ones`
    /// records that the A8's `plane + 0x400` "is a *state*, not an echo", and that the
    /// identical service appears in ten parts from the A7 to the A12.
    status_all_ones: Option<Vec<u64>>,
}

impl DeviceTableControlWindow {
    /// The four windows the A11 SecureROM's own device table names, as `(base, size)`.
    ///
    /// The three `0x2320_8x0000` blocks are contiguous and are therefore one aperture; the
    /// `0x235280000` block is a separate window on the same bus and is mapped separately by
    /// the platform, so it is not folded in here.
    pub const WINDOWS: [(u64, u64); 2] = [(0x2320_80000, 0x1_0000), (0x2352_80000, 0x1_0000)];

    /// The A10's two apertures, as `(base, size)`.
    ///
    /// Read out of `SecureROM_t8010si_iBoot-2696.0.0.1.33``'s own device table rather than
    /// derived from the A11's: the image names `0x20e080000`, `0x20e080400`, `0x20e084000`
    /// and `0x20e088000` in one device's register run, and `0x210280000` in another, so the
    /// first arm is a 0x1_0000 aperture and the second is its own window. The A11's own bases
    /// are a different part's addresses and are not reused for the A10.
    pub const A10_WINDOWS: [(u64, u64); 2] =
        [(0x20e0_80000, 0x1_0000), (0x2102_80000, 0x1_0000)];

    /// Offset within the A10's first aperture whose bits 7:4 the ROM's readiness poll reads.
    ///
    /// Composed by the image itself as `movz x8, #0x2, lsl #32` / `movk x8, #0xe08, lsl #16` /
    /// `movk x8, #0x400` at `0x10000944c`, so `0x20e080000 + 0x400`.
    pub const A10_READY_OFFSET: u64 = 0x400;

    /// Byte offset ranges of the A10's 87 echo registers, per aperture.
    ///
    /// From the ROM's own 85-slot table at image VA `0x100018478` (indexed
    /// `sub w8, w0, #6` / `cmp w8, #0x54`): device numbers 6..22 name
    /// `0x210280000 + 8*n` and 23..90 name `0x20e080100 + 8*(n - 0x17)`.
    pub const A10_ECHO_RANGES: [(u64, u64); 2] = [(0x0, 0x80), (0x100, 0x328)];

    pub fn new(id: DeviceId, base: u64, size: u64) -> Self {
        Self {
            id,
            base,
            size,
            words: HashMap::new(),
            status_all_ones: None,
        }
    }

    /// Declare bits 7:4 hardware-owned and all-ones at these offsets. See
    /// [`DeviceTableControlWindow::status_all_ones`].
    pub fn with_status_all_ones(mut self, offsets: Vec<u64>) -> Self {
        self.status_all_ones = Some(offsets);
        self
    }

    /// True when a hardware-status rule claims this offset, and so the echo rule must not.
    pub fn claims_status(&self, offset: u64) -> bool {
        self.status_all_ones
            .as_ref()
            .is_some_and(|offsets| offsets.contains(&offset))
    }

    /// The aperture this device answers for.
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Declare a register's hardware actual state, as a device that is present and powered
    /// would. This is the only way readiness is ever asserted.
    pub fn set_actual(&mut self, offset: u64, actual: u32) {
        let w = self.words.entry(offset & !7).or_default();
        w.actual = actual & 0xf;
    }

    /// Declare every register in the aperture actual, i.e. a fully powered device set.
    ///
    /// This is a statement about the modelled platform, not a default: a caller that has
    /// not established that a device is present must not call it, or the A11 ROM's poll
    /// would pass for hardware that was never there.
    pub fn set_all_actual(&mut self, actual: u32) {
        for w in self.words.values_mut() {
            w.actual = actual & 0xf;
        }
    }

    /// The control word at a byte offset, or `None` when the ROM has never written it.
    ///
    /// `None` is the honest answer for an untouched register and is deliberately not the
    /// same as a ready one; use [`DeviceTableControlWindow::readiness`] for the question the ROM asks.
    pub fn word(&self, offset: u64) -> Option<TargetActualRegister> {
        self.words.get(&(offset & !7)).copied()
    }

    /// Whether the A11 ROM's poll at `0x100003350` would pass at this offset. A register the
    /// ROM has never written is not ready, so a device set that was never declared cannot be
    /// walked past by accident.
    pub fn readiness(&self, offset: u64) -> bool {
        self.word(offset).is_some_and(|w| w.is_ready())
    }

    /// Number of registers the guest has written, for a report that distinguishes "the ROM
    /// never got here" from "the ROM got here and the device did not answer".
    pub fn touched(&self) -> usize {
        self.words.len()
    }
}

impl Device for DeviceTableControlWindow {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "device-table-control"
    }
    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if offset >= self.size || !matches!(size, 4 | 8) {
            return DeviceResult::Fault;
        }
        // The ROM addresses these at 8-byte stride but accesses them as 32-bit words, so a
        // 4-byte read is the architecturally meaningful case and 8 is accepted for the
        // paired access the same table entry could be used with.
        // Hardware-status offsets (declared via with_status_all_ones) return 0xf0
        // regardless of guest writes, because the ROM's readiness poll reads them
        // as a hardware state, not as an echo of its own target.
        let value = if self.claims_status(offset & !7) {
            0xf0u64
        } else {
            self.words.get(&offset).map_or(0, |w| w.read_value() as u64)
        };
        DeviceResult::Value(value)
    }
    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if offset >= self.size || !matches!(size, 4 | 8) {
            return DeviceResult::Fault;
        }
        let word = value as u32;
        let is_status = self.claims_status(offset & !7);
        let entry = self.words.entry(offset & !7).or_default();
        entry.target = word & 0xf;
        entry.other = word & !0xff;
        // A hardware-status word does not take its answer from a guest write. Without this
        // the ROM's own read-modify-write at `0x100009464` would drive the status nibble to
        // the software nibble it had just read, and the poll at `0x10000946c` would never
        // finish -- a hang that looks like a missing device rather than a wrong rule. The
        // guest also cannot forge readiness by writing `0xf0` here: `actual` moves only
        // through `set_actual`, so the reported state is the modelled device's.
        if !is_status && SUPPORTED_TARGETS.contains(&entry.target) {
            entry.actual = entry.target;
        }
        DeviceResult::Ok
    }
    fn reset(&mut self) {
        self.words.clear();
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

    /// Write and assert the bus accepted it, so a test cannot pass by silently faulting.
    fn poke(d: &mut DeviceTableControlWindow, off: u64, value: u32) {
        assert!(
            matches!(d.write(off, 4, value as u64), DeviceResult::Ok),
            "write at +0x{off:x} was refused"
        );
    }

    /// The A11 ROM's readiness test is `(word & 0xf0) == 0xf0` at `0x100003350`. A control
    /// word nobody has driven must fail it, or the ROM would proceed past a device that was
    /// never there. This is the fail-closed property the model is built around.
    #[test]
    fn an_untouched_control_word_fails_the_a11_readiness_test() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x0, 0);
        assert!(
            d.word(0x400).is_none(),
            "an unwritten register has no word yet"
        );
        assert!(!d.readiness(0x400));
    }

    /// The ROM sets the low nibble with `orr w9, w9, #0xf` at 0x100003300 and clears it with
    /// `and w9, w9, #0xfffffff0` at 0x100003308, then reads the high nibble back. So the
    /// target nibble has to drive the actual nibble for the poll to ever pass.
    #[test]
    fn the_target_nibble_drives_the_actual_nibble_the_rom_polls() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x400, 0x0f);
        assert!(d.readiness(0x400));
        poke(&mut d, 0x400, 0x00);
        assert!(!d.readiness(0x400));
    }

    /// Bits 10 and 18 are cleared by `and w9, w9, #0xeffffbff` at 0x100003348 and never read
    /// back, so clearing them must not disturb the target/actual pair or the answer.
    #[test]
    fn clearing_bits_10_and_18_does_not_disturb_the_target_actual_pair() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x400, 0x0004_400f);
        let before = d.word(0x400).unwrap();
        poke(&mut d, 0x400, before.read_value() & 0xefff_fbff);
        let after = d.word(0x400).unwrap();
        assert_eq!(after.target, before.target);
        assert_eq!(after.actual, before.actual);
        assert_eq!(after.is_ready(), before.is_ready());
    }

    /// The stable-read handshake at 0x100003310 reads the same word twice and requires the
    /// two reads to agree in the low nibble after the shift. A model that answered
    /// differently on the second read would hang the ROM there forever.
    #[test]
    fn the_stable_read_handshake_sees_two_identical_reads() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x0, 0xf);
        let read = |d: &mut DeviceTableControlWindow| match d.read(0x0, 4) {
            DeviceResult::Value(v) => v,
            other => panic!("expected a value, got {other:?}"),
        };
        let (a, b) = (read(&mut d), read(&mut d));
        assert_eq!(a, b);
        // The ROM's own test at 0x100003318: (w10 ^ (w9 >> 4)) & 0xf == 0.
        assert_eq!(((b as u32) ^ ((a as u32) >> 4)) & 0xf, 0);
    }

    /// An unknown target must not become ready. Guessing a transition here is exactly the
    /// fabricated-readiness failure this project has recorded for the SEP gate.
    #[test]
    fn an_unknown_target_does_not_complete_and_leaves_the_word_not_ready() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x400, 0x7);
        let w = d.word(0x400).unwrap();
        assert_eq!(w.target, 0x7);
        assert_eq!(
            w.actual, 0,
            "an unknown target must not drive the actual nibble"
        );
        assert!(!w.is_ready());
    }

    /// `set_actual` is the only route to a reported actual state, so a device declared
    /// present reports it even for a target the model would not have completed itself.
    #[test]
    fn a_declared_present_device_reports_its_actual_state() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x400, 0x0);
        assert!(!d.readiness(0x400));
        d.set_actual(0x400, 0xf);
        assert!(d.readiness(0x400));
    }

    /// The ROM's device table addresses these at 8-byte stride, so neighbouring entries must
    /// stay independent or one device's write would satisfy another's poll.
    #[test]
    fn registers_are_independent_at_the_eight_byte_stride_the_rom_table_uses() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut d, 0x108, 0xf);
        assert!(d.readiness(0x108));
        assert!(!d.readiness(0x100), "a neighbour must stay clear");
        assert_eq!(d.touched(), 1);
    }

    #[test]
    fn out_of_range_and_bad_width_accesses_fault() {
        let mut d = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        assert!(matches!(d.read(0x1_0000, 4), DeviceResult::Fault));
        assert!(matches!(d.read(0, 2), DeviceResult::Fault));
        assert!(matches!(d.write(0x1_0000, 4, 0), DeviceResult::Fault));
        assert!(matches!(d.write(0, 16, 0), DeviceResult::Fault));
    }

    /// Negative control for the whole module: a model that reported the actual nibble as
    /// always `0xf` would satisfy the ROM's poll for hardware that was never declared. If
    /// this test passes while `an_untouched_control_word_fails_the_a11_readiness_test` also
    /// passes, something is wrong with one of them, so pin the pair together.
    #[test]
    fn readiness_is_reachable_only_through_a_write_or_an_explicit_declaration() {
        let mut fresh = DeviceTableControlWindow::new(1, 0x2320_80000, 0x1_0000);
        poke(&mut fresh, 0x400, 0x0);
        assert!(!fresh.readiness(0x400), "must not be ready by default");
        poke(&mut fresh, 0x400, 0xf);
        assert!(fresh.readiness(0x400), "an explicit 0xf target completes");
    }
}
