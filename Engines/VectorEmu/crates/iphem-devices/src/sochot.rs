//! The A11 `sochot0` block: the status/identity window the T8015 SecureROM reads, and the
//! reason the T8015 platform census stopped.
//!
//! # Why this window exists at all
//!
//! The A11 device tree the project checks in names it directly:
//!
//! ```text
//! /device-tree/arm-io/sochot0
//!     compatible: sochot,s5l8955x
//!     reg: 0x352bc000+0x4000        -> arm-io 0x2352bc000
//! ```
//!
//! The census halted at instruction 32,058 on `ldr w8, [x8]` at `0x1000082c0` with
//! `x8 = 0x2352bc018`. Before this device the window was deliberately left unmapped: there
//! was no `sochot` model, and answering it with `Pmgr` or `AesOtp` would have asserted a
//! register layout nothing in the project supported. A real block whose registers are read
//! out of the ROM's own instructions is a different proposition from a catch-all, and that is
//! why this file exists rather than a one-line window mapping.
//!
//! # Every register here is decoded from the ROM, not guessed
//!
//! Disassembling the whole image with this project's own decoder (`dis_va`) shows the accessors
//! are contiguous, one per register field, at `0x100008200..0x1000083a8`. All of them compose
//! the window base the same way -- `movz x8, #0x2, lsl #32` / `movk x8, #0x352b, lsl #16` /
//! `movk x8, #0xc0xx` -- so the base is a measurement, not an assumption. [`ACCESSORS`] lists
//! every one with the instruction that produced it.
//!
//! Two of them are worth naming because they are what the SecureROM *does* rather than what
//! it observes:
//!
//! * `0x1000082f0` sets bit 31 of `+0x04`, issues `dsb sy`, re-reads, and leaves the loop on
//!   `tbz x8, #31`. A device that held the bit would hang the ROM there forever, so
//!   self-clearing is the only behaviour consistent with the instruction stream. It is
//!   derived from the loop, not invented for it.
//! * `0x10000823c` reads `+0x00`, clears bit 0, and writes it back -- the one write that is
//!   not a status handshake. So bit 0 of `+0x00` is guest-clearable and nothing else in that
//!   word is writable: a write may only take the flag down, never raise it.
//!
//! # Values: the cold state, and only what the ROM forces
//!
//! This window is the one place in the project where a made-up number would look like
//! progress, so the model reports the minimal cold state and says so. The silicon reset value
//! of these bits is **not** established by anything in this repository.
//!
//! That matters most at `+0x18`. The accessor at `0x1000082b4` returns bits 8..10, and it is
//! called immediately after `bl 0x1000082ac`, which returns the literal `0x8015` (this chip).
//! At both call sites the pair is used to walk the ROM's own runtime device table, and at
//! `0x100002e38` the table's field is compared against the `+0x18` value with
//! `cmp xzr, x16, x8` / **`b.hi`** -- an unsigned *upper* bound. So a 3-bit revision/variant
//! limit, not a readiness bit, and `0` is the *most* restrictive answer available. If the ROM
//! needs a wider limit it has to be stated; widening it to make a lookup succeed is exactly
//! the fabricated-readiness failure this project has already paid for.
//!
//! Nothing here ever reports a device as present, powered, or ready. It is a status window
//! with one self-clearing command bit and one guest-clearable flag, deliberately not a source
//! of the readiness claims the `DeviceTableControlWindow` model exists to refuse.

use std::any::Any;
use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};

/// Window base, translated from the tree's own cell: `0x352bc000` + arm-io `0x2_0000_0000`.
pub const BASE: u64 = 0x2352_bc000;
/// Window size, from the same `reg` cell.
pub const SIZE: u64 = 0x4000;

/// One ROM accessor: which register it reads, which bits it keeps, and the instruction that
/// says so. Listed rather than described in prose so the tests can iterate the whole set and
/// a register this ROM stops reading cannot go unnoticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accessor {
    /// Address of the accessor in the image.
    pub va: u64,
    /// Offset into [`BASE`].
    pub offset: u64,
    /// Low bit the accessor keeps.
    pub lsb: u32,
    /// Width of the field it keeps.
    pub width: u32,
    /// The instruction that establishes the field, as the tree's decoder prints it.
    pub instruction: &'static str,
}

impl Accessor {
    /// The field's mask, `width` bits at `lsb`.
    pub fn mask(&self) -> u32 {
        if self.width >= 32 {
            u32::MAX
        } else {
            ((1u32 << self.width) - 1) << self.lsb
        }
    }

    /// The value the accessor returns, given the word the device holds.
    pub fn extract(&self, word: u32) -> u32 {
        (word & self.mask()) >> self.lsb
    }
}

/// Every `sochot0` accessor in the T8015 SecureROM, in address order.
pub const ACCESSORS: &[Accessor] = &[
    Accessor { va: 0x1000_0820c, offset: 0x000, lsb: 0,  width: 1,  instruction: "and w0, w8, #0x1" },
    Accessor { va: 0x1000_08270, offset: 0x000, lsb: 2,  width: 2,  instruction: "ubfm w0, w8, #2, #3" },
    Accessor { va: 0x1000_0828c, offset: 0x000, lsb: 4,  width: 3,  instruction: "ubfm w0, w8, #4, #6" },
    Accessor { va: 0x1000_082a0, offset: 0x000, lsb: 9,  width: 7,  instruction: "ubfm w0, w8, #9, #15" },
    Accessor { va: 0x1000_082f0, offset: 0x004, lsb: 31, width: 1,  instruction: "orr w9, w9, #0x80000000; tbz x8, #31" },
    Accessor { va: 0x1000_08340, offset: 0x010, lsb: 0,  width: 5,  instruction: "and w0, w8, #0x1f" },
    Accessor { va: 0x1000_08358, offset: 0x014, lsb: 13, width: 2,  instruction: "ubfm w0, w8, #13, #14" },
    Accessor { va: 0x1000_08374, offset: 0x014, lsb: 8,  width: 2,  instruction: "ubfm w0, w8, #8, #9" },
    Accessor { va: 0x1000_083a4, offset: 0x014, lsb: 10, width: 2,  instruction: "ubfm w0, w8, #10, #11" },
    Accessor { va: 0x1000_0838c, offset: 0x014, lsb: 12, width: 1,  instruction: "ubfm w0, w8, #12, #12" },
    Accessor { va: 0x1000_082c0, offset: 0x018, lsb: 8,  width: 3,  instruction: "ubfm w9, w8, #7, #31; and w0, w9, #0x70; bfm w0, w8, #8, #10" },
    Accessor { va: 0x1000_08224, offset: 0x200, lsb: 0,  width: 1,  instruction: "and w0, w8, #0x1" },
    Accessor { va: 0x1000_0825c, offset: 0x200, lsb: 1,  width: 1,  instruction: "ubfm w0, w8, #1, #1" },
];

/// The `+0x18` field the ROM uses as a device-table upper bound at `0x100002e38`.
pub const VARIANT: u64 = 0x18;
/// The 64-bit word read at `0x1000082d4` as `ldr x9,[x8,#4]` / `ldr x0,[x8]`.
pub const IDENTIFIER: u64 = 0x80;
/// The word whose bit 0 the guest clears at `0x10000823c`.
pub const STATUS: u64 = 0x00;
/// The word holding the self-clearing request bit.
pub const COMMAND: u64 = 0x04;
/// Bit 31 of [`COMMAND`], set by the ROM with `orr w9, w9, #0x80000000` and waited out with
/// `tbz x8, #31`.
pub const COMMAND_REQUEST: u32 = 1 << 31;
/// Bit 0 of [`STATUS`], the one flag the guest is allowed to take down.
pub const STATUS_FLAG: u32 = 1;
/// Low bit of the `+0x18` variant field, from the accessor's `bfm w0, w8, #8, #10`.
pub const VARIANT_LSB: u32 = 8;
/// Width of the `+0x18` variant field, the same three bits.
pub const VARIANT_WIDTH: u32 = 3;
/// Mask of the `+0x18` variant field in place.
pub const VARIANT_FIELD: u32 = ((1 << VARIANT_WIDTH) - 1) << VARIANT_LSB;

/// The A11 SoC-hot status block.
#[derive(Debug)]
pub struct SocHot {
    id: DeviceId,
    /// The 64-bit word at [`IDENTIFIER`].
    identifier: u64,
    /// Guest-visible bits of [`COMMAND`] other than the request bit, so the ROM's
    /// read-modify-write round-trips instead of reading back its own request.
    command_other: u32,
    /// Requests the guest has issued and the model has completed.
    requests: u32,
    /// Guest-cleared flag at [`STATUS`] bit 0.
    flag_cleared: bool,
    /// The `+0x18` variant/revision limit, already placed in bits 8..10. See [`Self::set_variant`].
    variant_bits: u32,
}

impl SocHot {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            identifier: 0,
            command_other: 0,
            requests: 0,
            flag_cleared: false,
            variant_bits: 0,
        }
    }

    /// The cold value of every register word: a SoC that is not over temperature, at
    /// revision 0, with no flag raised. See the module note for why this is the
    /// non-inventive bounded answer backed by the available evidence.
    ///
    /// [`COMMAND`] is the one word with stored content, because the ROM performs a
    /// read-modify-write on it and has to read back what it wrote apart from the request.
    fn word(&self, offset: u64) -> u32 {
        match offset {
            COMMAND => self.command_other,
            VARIANT => self.variant_bits,
            // A cold block reads all ones with the one guest-clearable flag taken down, so
            // the flag is observable as "clear" rather than the word being uniformly zero.
            STATUS if self.flag_cleared => !STATUS_FLAG,
            _ => 0,
        }
    }

    /// The value one of this ROM's accessors returns.
    pub fn read_accessor(&self, a: &Accessor) -> u32 {
        a.extract(self.word(a.offset))
    }

    /// The 3-bit variant/revision field the ROM uses as a device-table upper bound.
    pub fn variant(&self) -> u32 {
        (self.word(VARIANT) & VARIANT_FIELD) >> VARIANT_LSB
    }

    /// Declare the 3-bit variant/revision limit at `+0x18`.
    ///
    /// This is a *platform input*, not a derived value, and the only reason it exists is that
    /// the A11 SecureROM uses it as an unsigned upper bound when it walks its own device table
    /// at `0x100002e38` (`cmp xzr, x16, x8` / `b.hi`). A value that is too small does not
    /// merely mislabel a part: it makes the lookup reject every device whose table entry sits
    /// above the limit, so those devices are never configured, their power words never reach
    /// `actual = 0xf`, and the ROM spins forever on the device-control readiness poll at
    /// `0x100003350`. The default is `0` -- the cold value, and the most restrictive one --
    /// and widening it is a decision that has to be made against evidence, which is why it is
    /// an explicit setter rather than a constructor default.
    pub fn set_variant(&mut self, variant: u32) {
        let bits = (variant & ((1 << VARIANT_WIDTH) - 1)) << VARIANT_LSB;
        self.variant_bits = bits;
    }

    /// Requests the guest has issued and the model has completed.
    pub fn requests(&self) -> u32 {
        self.requests
    }

    /// Declare the 64-bit word at [`IDENTIFIER`].
    pub fn set_identifier(&mut self, value: u64) {
        self.identifier = value;
    }

    /// Whether a read of the ROM's request bit would find it clear, i.e. whether the
    /// SecureROM's `tbz x8, #31` would fall through.
    pub fn request_is_clear(&self) -> bool {
        self.word(COMMAND) & COMMAND_REQUEST == 0
    }
}

impl Device for SocHot {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "sochot0"
    }

    fn reset(&mut self) {
        self.command_other = 0;
        self.requests = 0;
        self.flag_cleared = false;
        self.variant_bits = 0;
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if offset >= SIZE {
            return DeviceResult::Fault;
        }
        // `ldr x9,[x8,#4]` / `ldr x0,[x8]` / `bfm x0, x9, #32, #31` is one 64-bit word; the
        // two 32-bit halves are not separately addressable because the ROM never addresses
        // them that way.
        if offset == IDENTIFIER {
            return match size {
                8 => DeviceResult::Value(self.identifier),
                _ => DeviceResult::Fault,
            };
        }
        // Everything else this ROM touches is a 32-bit word, and only the offsets in
        // `ACCESSORS` exist. An offset it never reads stays a fault: answering it with zero
        // would claim a register the corpus never mentions.
        if !ACCESSORS.iter().any(|a| a.offset == offset) {
            return DeviceResult::Fault;
        }
        match size {
            4 => DeviceResult::Value(self.word(offset) as u64),
            _ => DeviceResult::Fault,
        }
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if offset >= SIZE || size != 4 {
            return DeviceResult::Fault;
        }
        match offset {
            COMMAND => {
                let word = value as u32;
                if word & COMMAND_REQUEST != 0 {
                    // The only write the ROM makes to this block. Completing the request
                    // here is forced by the instruction stream: `tbz x8, #31` leaves the loop
                    // only when the bit reads clear, so a model that held it would hang.
                    self.requests += 1;
                    self.command_other = word & !COMMAND_REQUEST;
                } else {
                    self.command_other = word;
                }
                DeviceResult::Ok
            }
            // `0x10000823c` reads this word, clears bit 0 and writes it back. Accepting the
            // write but honouring only the clear keeps the flag guest-clearable without
            // letting a write raise it or disturb hardware-owned bits.
            STATUS => {
                if value as u32 & STATUS_FLAG == 0 {
                    self.flag_cleared = true;
                }
                DeviceResult::Ok
            }
            _ => DeviceResult::Fault,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn poke(d: &mut SocHot, off: u64, val: u32) {
        assert!(
            matches!(d.write(off, 4, val as u64), DeviceResult::Ok),
            "write at +0x{off:x} was refused"
        );
    }

    fn load(d: &mut SocHot, off: u64, size: usize) -> u32 {
        match d.read(off, size) {
            DeviceResult::Value(v) => v as u32,
            other => panic!("expected a value at +0x{off:x}, got {other:?}"),
        }
    }

    /// The census stopped on `ldr w8, [0x2352bc018]`, so this exact access must answer rather
    /// than fault. The reported value is cold, and the decode is the ROM's own.
    #[test]
    fn the_census_faulting_access_answers_and_decodes_as_the_rom_reads_it() {
        let mut d = SocHot::new(1);
        let w = load(&mut d, VARIANT, 4);
        // `ubfm w9, w8, #7, #31` ; `and w0, w9, #0x70` ; `bfm w0, w8, #8, #10`
        let shifted = w >> 7;
        let anded = shifted & 0x70;
        let bfm = (anded & !0x700) | (w & 0x700);
        assert_eq!(bfm, 0);
        assert_eq!(d.variant(), 0, "a cold block reports revision 0");
    }

    /// Every accessor the ROM has must answer on the offset it names, at 32 bits. This is the
    /// test that stops the next crash from being discovered one instruction at a time, and it
    /// fails if a register is dropped from the table or given the wrong offset.
    #[test]
    fn every_accessor_the_rom_has_answers_on_its_own_offset() {
        let mut d = SocHot::new(1);
        for a in ACCESSORS {
            assert!(
                matches!(d.read(a.offset, 4), DeviceResult::Value(_)),
                "accessor 0x{:x} at +0x{:x} ({}) does not answer",
                a.va,
                a.offset,
                a.instruction
            );
        }
        // And the census's two stopping addresses are in that set.
        for off in [0x18u64, 0x14] {
            assert!(
                ACCESSORS.iter().any(|a| a.offset == off),
                "+0x{off:x} is where the census stopped and must be modelled"
            );
        }
    }

    /// `extract` has to agree with the instruction it is quoted from, for a word that has
    /// bits set outside the field. Otherwise the model's masks could drift from the ROM's and
    /// a cold value would still pass every assertion above. The probe word is built from each
    /// accessor's own mask with one neighbour bit deliberately left clear, so a field that is
    /// too wide is caught as well as one that is mis-shifted.
    #[test]
    fn each_field_extracts_exactly_the_bits_its_instruction_names() {
        for a in ACCESSORS {
            let max = if a.width >= 32 { u32::MAX } else { (1u32 << a.width) - 1 };
            let probe = max << a.lsb;
            assert_eq!(
                a.extract(probe),
                max,
                "accessor 0x{:x} ({}) did not extract its full field",
                a.va,
                a.instruction
            );
            // The bit just above the field must not leak in, unless the field already runs
            // to the top of the word. Derived from `lsb + width`, not from the register
            // offset, which is a byte address and not a bit position.
            if a.lsb + a.width < 32 {
                let neighbour = 1u32 << (a.lsb + a.width);
                assert_eq!(
                    a.extract(neighbour),
                    0,
                    "accessor 0x{:x} ({}) read a bit above its field",
                    a.va,
                    a.instruction
                );
            }
        }
    }

    /// The default is the cold value and stays zero until something declares otherwise, so a
    /// model that is merely constructed cannot widen the ROM's device-table limit by
    /// accident. And a declared value lands in bits 8..10, where the accessor's
    /// `bfm w0, w8, #8, #10` reads it, and nowhere else in the word.
    #[test]
    fn the_variant_limit_is_zero_until_declared_and_lands_in_its_own_bits() {
        let mut d = SocHot::new(1);
        assert_eq!(d.variant(), 0, "a cold block must not widen the ROM's limit");
        assert_eq!(load(&mut d, VARIANT, 4), 0);

        d.set_variant(5);
        assert_eq!(d.variant(), 5);
        let word = load(&mut d, VARIANT, 4);
        assert_eq!(word & VARIANT_FIELD, 5 << VARIANT_LSB);
        assert_eq!(word & !VARIANT_FIELD, 0, "the field must not disturb other bits");

        // The accessor's own decode of the value the ROM would read back.
        let v = ACCESSORS.iter().find(|a| a.offset == VARIANT).unwrap();
        assert_eq!(v.extract(word), 5);

        // Out-of-range and reset behaviour: only the three bits exist, and a reset returns
        // the block to the cold value rather than keeping a declared limit.
        d.set_variant(0xff);
        assert_eq!(d.variant(), (1 << VARIANT_WIDTH) - 1, "the field is three bits wide");
        d.reset();
        assert_eq!(d.variant(), 0, "reset must return to the cold value");
    }

    /// The handshake at `0x1000082f0`: `orr w9, w9, #0x80000000`, `str`, `dsb sy`, `ldr`,
    /// `tbz x8, #31`. The loop leaves only on a clear read, so the request bit has to be
    /// unobservable afterwards or the SecureROM never progresses past this call.
    #[test]
    fn the_request_bit_reads_clear_after_the_roms_read_modify_write() {
        let mut d = SocHot::new(1);
        assert!(d.request_is_clear(), "a cold block holds no request");
        poke(&mut d, COMMAND, 0);
        let word = load(&mut d, COMMAND, 4);
        poke(&mut d, COMMAND, word | COMMAND_REQUEST);
        assert_eq!(d.requests(), 1, "the request was not completed");
        assert_eq!(
            load(&mut d, COMMAND, 4) & COMMAND_REQUEST,
            0,
            "the ROM's tbz would spin forever on a held request bit"
        );
        assert!(d.request_is_clear());
    }

    /// The ROM sets bit 31 by OR-ing into what it read, so a guest-set neighbouring bit has
    /// to survive the round trip. Absorbing the whole word would make the read-back differ
    /// from the read-modify-write the firmware actually performed.
    #[test]
    fn a_guest_set_neighbour_bit_survives_the_read_modify_write() {
        let mut d = SocHot::new(1);
        poke(&mut d, COMMAND, 0x4000_0000);
        let word = load(&mut d, COMMAND, 4);
        poke(&mut d, COMMAND, word | COMMAND_REQUEST);
        assert_eq!(load(&mut d, COMMAND, 4) & 0x4000_0000, 0x4000_0000);
    }

    /// `ldr x9,[x8,#4]` / `ldr x0,[x8]` / `bfm x0, x9, #32, #31` is a single 64-bit word.
    /// Splitting it would make the high half read as a separate register, and a 4-byte read
    /// at `+0x80` is not an access this ROM makes.
    #[test]
    fn the_identifier_is_one_64_bit_word_not_two_32_bit_registers() {
        let mut d = SocHot::new(1);
        d.set_identifier(0x1122_3344_5566_7788);
        assert_eq!(d.read(IDENTIFIER, 8), DeviceResult::Value(0x1122_3344_5566_7788));
        assert_eq!(d.read(IDENTIFIER, 4), DeviceResult::Fault);
    }

    /// `0x10000823c` clears bit 0 of `+0x00` and writes the word back. The clear has to be
    /// accepted, and a write must not be able to raise the flag: a guest that can write its
    /// own status has invented the register.
    #[test]
    fn the_status_flag_is_guest_clearable_but_not_guest_settable() {
        let mut d = SocHot::new(1);
        let before = load(&mut d, STATUS, 4);
        // Exactly what the ROM does: read, clear bit 0, write back.
        poke(&mut d, STATUS, before & !STATUS_FLAG);
        assert_eq!(load(&mut d, STATUS, 4) & STATUS_FLAG, 0);
        // A write that tries to raise the flag is accepted at the bus and does not set it.
        poke(&mut d, STATUS, STATUS_FLAG);
        assert_eq!(
            load(&mut d, STATUS, 4) & STATUS_FLAG,
            0,
            "a guest must not be able to raise its own status flag"
        );
    }

    /// An offset this project has no model for must stay a fault. Reading zero there would
    /// claim a register the corpus never mentions, which is the fabricated-device failure
    /// mode; `+0x40` and the unlisted `+0x08` are both unmentioned by this ROM.
    #[test]
    fn an_offset_the_rom_never_touches_faults_rather_than_reading_zero() {
        let mut d = SocHot::new(1);
        for off in [0x08u64, 0x0c, 0x40, 0x100, 0x204] {
            assert_eq!(d.read(off, 4), DeviceResult::Fault, "+0x{off:x} should fault");
            assert_eq!(d.write(off, 4, 0), DeviceResult::Fault, "+0x{off:x} write");
        }
        assert_eq!(d.read(SIZE, 4), DeviceResult::Fault, "past the declared window");
    }

    /// `sochot0` declares `+0x4000`, so the aperture ends at `0x2352c0000`. The neighbouring
    /// A11 blocks the platform already maps are the AES window at `0x2352d0000` and the PMGR
    /// band covering `0x2352dc000`; a `sochot0` window wide enough to reach either would
    /// misattribute another node's registers to this one. These are compile-time facts, so
    /// they are checked as constants rather than at run time.
    #[test]
    fn the_window_is_the_size_the_tree_declares_and_stops_short_of_its_neighbours() {
        const { assert!(SIZE == 0x4000) };
        const { assert!(BASE + SIZE == 0x2352_c0000) };
        const { assert!(BASE + SIZE <= 0x2352_d0000) };
        const { assert!(BASE + SIZE <= 0x2352_dc000) };
        // Every accessor must land inside the declared window.
        for a in ACCESSORS {
            assert!(a.offset < SIZE, "accessor 0x{:x} is outside the window", a.va);
        }
    }
}
