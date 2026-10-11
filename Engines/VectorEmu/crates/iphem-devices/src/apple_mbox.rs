use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// `0x8108` bit 16: an answer for this descriptor is queued and not yet
/// collected, so the AP must not send the request again. Read by the
/// SecureROM's message-array walk at `0x10000a324`.
pub const SEPMAIL_STATUS_REPLY_PENDING: u64 = 1 << 16;

/// `0x810c` bit 17: no answer is available for this descriptor yet. Read by
/// the same walk at `0x10000a23c`.
pub const SEPMAIL_STATUS_NO_REPLY: u64 = 1 << 17;

/// Mailbox interrupt-**disable** register, at mailbox base `+0x8100`.
///
/// Provenance: `checkra1n/PongoOS`, `src/drivers/sep/sep.c`:
/// ```c
/// struct mailbox_registers64 {
///     volatile uint32_t dis_int;  // 8100
///     volatile uint32_t en_int;   // 8104
///     volatile uint32_t snd_sts;  // 8108
///     volatile uint32_t recv_sts; // 810c
///     … uint64_t snd0;            // @0x8800
///     … uint64_t recv0;           // @0x8830
/// };
/// ```
/// and `sep_setup()` programs `mailboxregs64->dis_int = 0; mailboxregs64->en_int = 0x1000;`
/// on SEP reset. An earlier version of this file placed these two registers at
/// `+0x1000`/`+0x1004`, which is wrong: the value `0x1000` PongoOS writes is the
/// *enable mask*, not the register's offset. The offsets are `0x8100` and `0x8104`.
pub const SEPMAIL_DIS_INT: u64 = 0x8100;

/// Mailbox interrupt-**enable** register, at mailbox base `+0x8104`. Writing a bit
/// here permits the matching mailbox interrupt; [`SEPMAIL_DIS_INT`] clears it.
pub const SEPMAIL_EN_INT: u64 = 0x8104;

/// The enable mask PongoOS programs at SEP reset (`en_int = 0x1000`), i.e. bit 12.
pub const SEPMAIL_INTR_ENABLE: u64 = 0x1000;

/// Per-offset access counts for one mailbox view, keyed by register offset.
pub type OffsetCounts = std::collections::BTreeMap<u64, u64>;

#[derive(Default)]
struct MailboxState {
    ap_to_iop: VecDeque<u64>,
    iop_to_ap: VecDeque<u64>,
    ap_to_iop_pushes: u64,
    iop_to_ap_pushes: u64,
    ap_to_iop_doorbell: bool,
    ap_push: u64,
    iop_push: u64,
    ap_pop: Option<u64>,
    iop_pop: Option<u64>,
    registers: std::collections::BTreeMap<u64, u64>,
    /// Per-offset access counters, keyed by `(view, offset)`.
    ///
    /// Diagnostic only: they answer "which mailbox registers does this guest
    /// actually touch, and how often", which is what identifies an unimplemented
    /// register as a real gap rather than a guess. Bounded by the number of
    /// distinct offsets, never by traffic volume.
    ap_reads: std::collections::BTreeMap<u64, u64>,
    ap_writes: std::collections::BTreeMap<u64, u64>,
    iop_reads: std::collections::BTreeMap<u64, u64>,
    iop_writes: std::collections::BTreeMap<u64, u64>,
}

#[derive(Clone, Copy)]
enum View {
    Ap,
    Iop,
}

/// One side of the T8020 SEP Apple ASC mailbox. The AP and SEP views share
/// FIFO state but use different register offsets, matching the split register
/// windows documented by the Linux Apple mailbox driver and observed in ROM.
pub struct AppleAscMailbox {
    id: DeviceId,
    view: View,
    state: Arc<Mutex<MailboxState>>,
    /// Interrupt number for this mailbox view (AP or SEP).
    /// When a doorbell is written, an interrupt is emitted to the AIC.
    irq: u32,
}

impl AppleAscMailbox {
    pub fn paired(id: DeviceId, ap_irq: u32, iop_irq: u32) -> (Self, Self) {
        let state = Arc::new(Mutex::new(MailboxState::default()));
        (
            Self {
                id,
                view: View::Ap,
                state: Arc::clone(&state),
                irq: ap_irq,
            },
            Self {
                id,
                view: View::Iop,
                state,
                irq: iop_irq,
            },
        )
    }

    pub fn peer(&self) -> Self {
        Self {
            id: self.id,
            view: match self.view {
                View::Ap => View::Iop,
                View::Iop => View::Ap,
            },
            state: Arc::clone(&self.state),
            irq: self.irq,
        }
    }

    pub fn irq(&self) -> u32 {
        self.irq
    }

    fn valid_size(size: usize) -> bool {
        matches!(size, 1 | 2 | 4 | 8)
    }

    fn valid_write_size(size: usize) -> bool {
        matches!(size, 1 | 2 | 4 | 8 | 16)
    }

    /// Number of AP→IOP messages staged but not yet consumed by the IOP (SEP)
    /// view. Used by the multi-core coordinator to advance the SEP harness
    /// only while a request is outstanding.
    pub fn ap_to_iop_pending(&self) -> usize {
        self.state.lock().expect("ASC mailbox lock").ap_to_iop.len()
    }

    /// The AP→IOP message at the head of the queue, without consuming it.
    ///
    /// A consumer that has to *interpret* a staged message before deciding to act on
    /// it needs to see the word, and `ap_to_iop_pending` only gives a depth. This is
    /// read-only and does not disturb the FIFO: the AP view's own read path pops, and
    /// this deliberately does not, so a relay can inspect a message the AP has staged
    /// but not yet had collected.
    pub fn peek_ap_to_iop(&self) -> Option<u64> {
        self.state
            .lock()
            .expect("ASC mailbox lock")
            .ap_to_iop
            .front()
            .copied()
    }

    /// Number of IOP→AP messages staged but not yet consumed by the AP view.
    pub fn iop_to_ap_pending(&self) -> usize {
        self.state.lock().expect("ASC mailbox lock").iop_to_ap.len()
    }

    /// How many AP→IOP doorbell pushes have occurred since reset.
    pub fn ap_to_iop_pushes(&self) -> u64 {
        self.state
            .lock()
            .expect("ASC mailbox lock")
            .ap_to_iop_pushes
    }

    /// Read-only view of the queued AP→IOP payloads, oldest first, without
    /// consuming them. Lets a host-side diagnostic report what the AP actually
    /// sent while the SEP side is still deciding how to consume it.
    pub fn ap_to_iop_snapshot(&self) -> Vec<u64> {
        self.state
            .lock()
            .expect("ASC mailbox lock")
            .ap_to_iop
            .iter()
            .copied()
            .collect()
    }

    /// Consume the AP→IOP doorbell edge. The SEP interrupt controller uses
    /// this edge to wake a parked IOP; the FIFO payload remains available via
    /// the normal IOP receive registers.
    pub fn take_ap_to_iop_doorbell(&self) -> bool {
        let mut state = self.state.lock().expect("ASC mailbox lock");
        let pending = state.ap_to_iop_doorbell;
        state.ap_to_iop_doorbell = false;
        pending
    }

    /// How many IOP→AP reply pushes have occurred since reset.
    pub fn iop_to_ap_pushes(&self) -> u64 {
        self.state
            .lock()
            .expect("ASC mailbox lock")
            .iop_to_ap_pushes
    }

    /// Whether the SEP-side receive interrupt is armed, i.e. whether the mailbox
    /// is permitted to raise an interrupt at the coprocessor at all.
    ///
    /// The arming state is the retained `en_int` word, which is exactly the
    /// register the mailbox already models: `SEPMAIL_EN_INT` reads back what the
    /// guest stored, and [`SEPMAIL_DIS_INT`] clears bits in it. The bit tested is
    /// [`SEPMAIL_INTR_ENABLE`] (`0x1000`, bit 12) because that is the mask
    /// `checkra1n/PongoOS` `sep_setup()` programs at SEP reset
    /// (`mailboxregs64->dis_int = 0; mailboxregs64->en_int = 0x1000;`).
    ///
    /// This is load-bearing rather than decorative. The AArch64 SecureROM's own
    /// Current-EL IRQ and FIQ vectors (`VBAR_EL1 + 0x80`/`0x100`) are *terminal*:
    /// each stores a small class code to mailbox `+4` and then executes an
    /// unconditional `b .` self-branch with no `eret` path, so a SEPROM that takes
    /// an interrupt cannot continue. Measured on `AppleSEPROM-Cebu-A0`, the ROM
    /// never writes `en_int`: a `movz`/`movk` scan of the image finds mailbox
    /// references at base `+0x4`, `+0x400`, `+0x81c`, `+0xa00`, `+0x4100` and
    /// `+0x410c`, and none at `+0x8100`/`+0x8104`, so its receive interrupt is
    /// never armed. Raising the interrupt regardless would assert that this
    /// firmware is destroyed by the first doorbell, which is a claim about the
    /// ROM rather than about the device, and it is the claim the trace
    /// reproduces exactly: with the interrupt routed unconditionally the Cebu
    /// ROM takes it within nine instructions and stops at `+0xbc` for good.
    ///
    /// The message itself is unaffected: the AP→IOP FIFO push and the doorbell
    /// edge still happen, so anything that *polls* the receive window can still
    /// drain the queue. Only the interrupt is gated. Not claimed: that this ROM
    /// then does drain it -- measured over 2,000,000 AP steps it does not, and
    /// its run loop reads `CNTP_CTL_EL0` rather than the receive window, so the
    /// staged word is still queued at the end. That is a separate gate, not
    /// something this predicate decides.
    pub fn a2i_interrupt_armed(&self) -> bool {
        self.state
            .lock()
            .expect("ASC mailbox lock")
            .registers
            .get(&SEPMAIL_EN_INT)
            .copied()
            .unwrap_or(0)
            & SEPMAIL_INTR_ENABLE
            != 0
    }

    /// Per-offset access counts for both views, for diagnostics that need to
    /// know which registers a guest really touches. Returned as
    /// `(ap_reads, ap_writes, iop_reads, iop_writes)`.
    pub fn offset_access_counts(&self) -> (OffsetCounts, OffsetCounts, OffsetCounts, OffsetCounts) {
        let state = self.state.lock().expect("ASC mailbox lock");
        (
            state.ap_reads.clone(),
            state.ap_writes.clone(),
            state.iop_reads.clone(),
            state.iop_writes.clone(),
        )
    }
}

impl Device for AppleAscMailbox {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "apple-asc-mailbox"
    }

    fn reset(&mut self) {
        *self.state.lock().expect("ASC mailbox lock") = MailboxState::default();
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        if !Self::valid_size(size) {
            return DeviceResult::Fault;
        }
        let mut state = self.state.lock().expect("ASC mailbox lock");
        match self.view {
            View::Ap => *state.ap_reads.entry(offset).or_insert(0) += 1,
            View::Iop => *state.iop_reads.entry(offset).or_insert(0) += 1,
        }
        let value = match (self.view, offset) {
            // SEPOS clears this status/control word and waits for bit 63 to
            // become zero during ASC bring-up. Keep reset asserted until an
            // explicit device-side readiness transition occurs.
            (View::Iop, 0x400) => {
                let value = state.registers.get(&offset).copied().unwrap_or(1u64 << 63);
                if std::env::var_os("IPHEM_TRACE_ASC_CONTROL").is_some() {
                    eprintln!("ASC control read view=iop offset=0x400 value={value:#x}");
                }
                value
            }
            // T8020's SEP-mail status words, decoded from the SecureROM's own
            // poll sites (see docs/USB_RESTORE_PROGRESS.md, 2026-09-25).
            //
            // The AP's message-array walk at 0x10000a314 reads its send status
            // through the descriptor's offset at `[x21+0x48]` (0x8108 here) and
            // its receive status through `[x21+0x4c]` (0x810c here):
            //
            //   0x10000a320: ldr w25, [x9, x8]      ; send status
            //   0x10000a324: tbnz x25, #16, skip    ; bit 16: an answer for this
            //                                           descriptor is already
            //                                           waiting -> do not resend
            //   0x10000a33c: and w8, w25, #0xc0000   ; bits 16 and 18
            //   0x10000a340: cbnz w8, leave_walk    ; either set ends the walk
            //
            //   0x10000a234: ldr w9, [x21, #0x4c]
            //   0x10000a238: ldr w25, [x9, x8]      ; receive status
            //   0x10000a23c: tbnz x25, #17, skip    ; bit 17: no answer yet
            //   0x10000a244: ldp x8, x9, [x8]       ; else fetch 16 bytes at
            //                                           0x8830 into the reply
            //
            // Both bits are statements about this mailbox's own receive queue,
            // which the device already tracks, so they are derived rather than
            // invented:
            //
            //   bit 16 (0x8108) - an answer is queued and not yet collected, so
            //                      the request must not be sent again.
            //   bit 17 (0x810c) - no answer is queued, so the fetch is skipped.
            //
            // They are complementary by construction: exactly one of them is
            // set, which is what lets the AP walk a descriptor to completion --
            // fetch the reply at 0x8830, see the queue drain, and take the
            // `and #0xc0000` exit. Reporting a constant 0 instead (the previous
            // behaviour) asserted "send again" and "an answer is available" for
            // every descriptor forever, so the walk could never finish and the
            // AP never posted an event.
            //
            // This makes the register path correct for *any* producer: a real
            // SEP firmware answering at 0x4820/0x4824 drives the same bits with
            // no further change, because both are computed from `iop_to_ap`.
            //
            // The open sequence at 0x100009fbc and 0x100009fd0 does a
            // read-modify-write on these words (`ldr; orr #1; str`) to raise the
            // device's own enable bit, so the value the guest stored is ORed
            // with the derived status rather than being discarded.
            (View::Ap, SEPMAIL_EN_INT) | (View::Iop, SEPMAIL_EN_INT) => {
                state.registers.get(&offset).copied().unwrap_or(0)
            }
            (View::Ap, 0x8108) => {
                let stored = state.registers.get(&offset).copied().unwrap_or(0);
                let pending = if state.iop_to_ap.is_empty() {
                    0
                } else {
                    SEPMAIL_STATUS_REPLY_PENDING
                };
                stored | pending
            }
            (View::Ap, 0x810c) => {
                let stored = state.registers.get(&offset).copied().unwrap_or(0);
                let none = if state.iop_to_ap.is_empty() {
                    SEPMAIL_STATUS_NO_REPLY
                } else {
                    0
                };
                stored | none
            }
            (View::Ap, 0x8830) => {
                state.ap_pop = state.iop_to_ap.pop_front();
                state.ap_pop.unwrap_or(0)
            }
            (View::Ap, 0x8838) => 0,
            (View::Iop, 0x4108 | 0x410c) => 0,
            (View::Iop, 0x4810) => {
                state.iop_pop = state.ap_to_iop.pop_front();
                state.iop_pop.unwrap_or(0) as u32 as u64
            }
            (View::Iop, 0x4814) => state.iop_pop.unwrap_or(0) >> 32,
            (View::Iop, 0x4818 | 0x481c) => 0,
            (_, other) => state.registers.get(&other).copied().unwrap_or(0),
        };
        DeviceResult::Value(value)
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        if !Self::valid_write_size(size) {
            return DeviceResult::Fault;
        }
        let mut state = self.state.lock().expect("ASC mailbox lock");
        match self.view {
            View::Ap => *state.ap_writes.entry(offset).or_insert(0) += 1,
            View::Iop => *state.iop_writes.entry(offset).or_insert(0) += 1,
        }
        let mut event = None;
        match (self.view, offset, size) {
            (View::Iop, 0x400, _) => {
                state.registers.insert(offset, value);
            }
            // SecureROM may send A2I with a single STP xN,xM,[0x8800]. The
            // first 64-bit word is the FIFO payload; the paired high word is
            // ancillary for the observed zero/control doorbell slot.
            (View::Ap, 0x8800, 16) => {
                state.ap_to_iop.push_back(value);
                state.ap_to_iop_pushes += 1;
                state.ap_to_iop_doorbell = true;
                // Only an armed mailbox may raise an interrupt at the SEP. The
                // push and the doorbell edge above are the transport and are
                // unconditional; see `a2i_interrupt_armed` for why the edge is
                // not.
                if state.registers.get(&SEPMAIL_EN_INT).copied().unwrap_or(0) & SEPMAIL_INTR_ENABLE
                    != 0
                {
                    event = Some(DeviceEvent::InterruptTo(self.irq, Some(1)));
                }
            }
            (View::Ap, 0x8800, 8) => state.ap_push = value,
            // The second AP register is the FIFO push/doorbell. The real
            // SecureROM's A2I send is a 64-bit store pair whose second word
            // lands at 0x8808 as a 4-byte store, so accept any store size
            // here; only the word staged at 0x8800 forms the message.
            (View::Ap, 0x8808, _) => {
                let message = state.ap_push;
                state.ap_to_iop.push_back(message);
                state.ap_to_iop_pushes += 1;
                state.ap_to_iop_doorbell = true;
                // Gated on the armed receive interrupt, as at `0x8800` above.
                if state.registers.get(&SEPMAIL_EN_INT).copied().unwrap_or(0) & SEPMAIL_INTR_ENABLE
                    != 0
                {
                    // Emit interrupt to IOP (SEP core = 1)
                    event = Some(DeviceEvent::InterruptTo(self.irq, Some(1)));
                }
            }
            // The SEP-mail open raises an enable bit in these words with a
            // read-modify-write (`ldr; orr #1; str`), so the guest's own value
            // must be retained for the next read.
            //
            // The derived status bits must be masked off on the way in. The open
            // reads the word (which reports the live queue state), ORs bit 0 and
            // writes the result back; storing that verbatim would bake the
            // status bit into the retained value, and because the read ORs the
            // retained value with a freshly derived one, a "no answer" bit read
            // once at open time would be reported forever afterwards. Masking
            // keeps the two sources independent: the retained word holds only
            // what the guest itself owns, and the status is always recomputed.
            (View::Ap, SEPMAIL_EN_INT, _) | (View::Iop, SEPMAIL_EN_INT, _) => {
                state.registers.insert(offset, value);
            }
            (View::Ap, SEPMAIL_DIS_INT, _) | (View::Iop, SEPMAIL_DIS_INT, _) => {
                let cur = state.registers.get(&SEPMAIL_EN_INT).copied().unwrap_or(0);
                state.registers.insert(SEPMAIL_EN_INT, cur & !value);
            }
            (View::Ap, 0x8108, _) => {
                state
                    .registers
                    .insert(offset, value & !SEPMAIL_STATUS_REPLY_PENDING);
            }
            (View::Ap, 0x810c, _) => {
                state
                    .registers
                    .insert(offset, value & !SEPMAIL_STATUS_NO_REPLY);
            }
            // The IOP-view equivalents stay inert.
            (View::Iop, 0x4108 | 0x410c, _) => {}
            (View::Iop, 0x4820, 4) => {
                state.iop_push = (state.iop_push & 0xffff_ffff_0000_0000) | (value & 0xffff_ffff)
            }
            (View::Iop, 0x4824, 4) => {
                state.iop_push = (state.iop_push & 0xffff_ffff) | ((value & 0xffff_ffff) << 32);
                let message = state.iop_push;
                state.iop_to_ap.push_back(message);
                state.iop_to_ap_pushes += 1;
                // Emit interrupt to AP (core 0)
                event = Some(DeviceEvent::InterruptTo(self.irq, Some(0)));
            }
            (View::Ap, 0x8830 | 0x8838, _) => {}
            (View::Iop, 0x4810 | 0x4814 | 0x4818 | 0x481c, _) => {}
            (_, other, _) => {
                state.registers.insert(other, value);
            }
        }
        if let Some(evt) = event {
            return DeviceResult::Event(evt);
        }
        DeviceResult::Ok
    }

    /// Raise this view's interrupt when a message is waiting for it and the
    /// guest has enabled mailbox interrupts.
    ///
    /// This is the producer the AP-side park has been missing, and it is the
    /// mechanism PongoOS documents: the SEP-side firmware programs `en_int`, and
    /// the mailbox interrupts its peer when a message lands. Previously this
    /// returned `None` unconditionally, so `AppleAscMailbox` had an `irq` that
    /// nothing could ever assert.
    ///
    /// `SEPDevice` order is not assumed: a message is "inbound" for a view when it
    /// is sitting in the queue that view reads. The enable gate is
    /// `SEPMAIL_INTR_ENABLE` in `SEPMAIL_EN_INT`, which is the bit PongoOS writes.
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        let state = self.state.lock().unwrap();
        let enabled =
            state.registers.get(&SEPMAIL_EN_INT).copied().unwrap_or(0) & SEPMAIL_INTR_ENABLE != 0;
        if !enabled {
            return None;
        }
        let inbound = match self.view {
            View::Ap => !state.iop_to_ap.is_empty(),
            View::Iop => !state.ap_to_iop.is_empty(),
        };
        inbound.then_some(DeviceEvent::Interrupt(self.irq))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The SEP-side mailbox **config** protocol, decoded from real SEPOS firmware.
///
/// Provenance: `Firmware/all_flash/sep-firmware.d321.RELEASE.im4p` from the
/// 16A366 restore, decrypted with the checked-in `sep-key`/`sep-iv`. The
/// mailbox driver is the twelfth-image container member at file offset
/// `0x2c0000` (`AppleSEPOS-806.200.118`, `SEPDrivers/Mailbox.cpp`), position
/// linked at `__TEXT` vmaddr `0x8000`. Its `Invalid mailbox config message`
/// path is at VA `0x1d35c`, and decodes to the validation reproduced here.
///
/// # This is a specification, not a responder
///
/// Nothing in this module emits mailbox traffic. It validates a message the
/// caller already has and maintains the SEP-side channel table, so that real SEP
/// execution can later be *checked* against the protocol the firmware actually
/// implements. It deliberately cannot answer the AP: a reply produced by a host
/// model would be a synthesized SEP response, which is exactly what the
/// mailbox is forbidden from doing (see the 2026-09-14 record).
pub mod mailbox_config {
    /// Accepted config opcodes. The driver compares the message's opcode against
    /// each of these and rejects anything else, which is the single condition
    /// that produces `Invalid mailbox config message`.
    pub const OPCODES: [u16; 2] = [0x4082, 0x412c];

    /// Number of channels. The driver reads the low byte of the message's
    /// channel word and requires it to be **less than** this, so the valid
    /// indices are `0..=0x13`.
    pub const CHANNEL_COUNT: u8 = 20;

    /// Per-channel state stride, from the driver's `index * 0x178 + base`.
    pub const CHANNEL_STRIDE: u64 = 0x178;

    /// Message word holding the value that is matched against channel state.
    pub const MSG_VALUE_OFFSET: u64 = 0x00;
    /// Message word holding the channel index in its low byte.
    pub const MSG_CHANNEL_OFFSET: u64 = 0x18;

    /// Channel-state offsets, relative to the channel's own base.
    pub const STATE_EXPECTED_OFFSET: u64 = 0xb90;
    pub const STATE_VALUE_OFFSET: u64 = 0xbe8;
    pub const STATE_COUNTER_A_OFFSET: u64 = 0xbf0;
    pub const STATE_COUNTER_B_OFFSET: u64 = 0xbf4;

    /// The driver's typed rejections: bad index, no such entry, and value
    /// mismatch, as the negative constants it actually returns.
    pub const ERR_BAD_INDEX: i64 = -2;
    pub const ERR_NO_ENTRY: i64 = -4;
    pub const ERR_VALUE_MISMATCH: i64 = -7;

    /// What the driver does with a config message.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Outcome {
        /// Accepted, and the channel now holds this value.
        Configured { channel: u8, value: u64 },
        /// Rejected, carrying the driver's own error constant.
        Rejected(i64),
    }

    /// The SEP-side channel table: twenty slots, each with the expected value
    /// the driver matches against and the value last written.
    #[derive(Clone, Debug, Default)]
    pub struct Channels {
        entries: [Option<Channel>; CHANNEL_COUNT as usize],
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Channel {
        expected: u64,
        value: u64,
    }

    impl Channels {
        /// Apply a config message exactly as the SEPOS mailbox driver does.
        ///
        /// `message` is the raw config message; only the two documented words
        /// are read, so a short message is a rejected index rather than a panic.
        pub fn configure(&mut self, opcode: u16, message: &[u8]) -> Outcome {
            if !OPCODES.contains(&opcode) {
                return Outcome::Rejected(ERR_BAD_INDEX);
            }
            // The driver loads the channel word and keeps only its low byte
            // (`and w9, w8, #0xff`); reading that byte directly is the same
            // thing, and it is why a message whose upper bytes are garbage
            // still routes correctly.
            let Some(&channel) = message.get(MSG_CHANNEL_OFFSET as usize) else {
                return Outcome::Rejected(ERR_BAD_INDEX);
            };
            if channel >= CHANNEL_COUNT {
                return Outcome::Rejected(ERR_BAD_INDEX);
            }
            let Some(slot) = self.entries[channel as usize] else {
                return Outcome::Rejected(ERR_NO_ENTRY);
            };
            let value = u64::from_le_bytes(
                message[MSG_VALUE_OFFSET as usize..MSG_VALUE_OFFSET as usize + 8]
                    .try_into()
                    .expect("8 bytes"),
            );
            if slot.expected != value {
                return Outcome::Rejected(ERR_VALUE_MISMATCH);
            }
            self.entries[channel as usize] = Some(Channel {
                expected: slot.expected,
                value,
            });
            Outcome::Configured { channel, value }
        }

        /// Establish a channel's expected value, as a prior `enabling mailboxes`
        /// step would. Without this every configure is `ERR_NO_ENTRY`, which is
        /// the driver's `Failed to look up mailbox driver` case.
        pub fn enable(&mut self, channel: u8, expected: u64) {
            if channel < CHANNEL_COUNT {
                self.entries[channel as usize] = Some(Channel { expected, value: 0 });
            }
        }

        /// The value last configured into a channel.
        pub fn value(&self, channel: u8) -> Option<u64> {
            self.entries
                .get(channel as usize)
                .copied()
                .flatten()
                .map(|c| c.value)
        }

        /// The byte offset of a channel's expected-value field, as the driver's
        /// `index * stride + STATE_EXPECTED_OFFSET` computes it.
        pub fn expected_offset(channel: u8) -> Option<u64> {
            (channel < CHANNEL_COUNT)
                .then(|| channel as u64 * CHANNEL_STRIDE + STATE_EXPECTED_OFFSET)
        }
    }
}

/// The AOP-side mailbox property store, decoded from real AOP firmware.
///
/// Provenance: `Firmware/AOP/aopfw-iphone11aop.im4p` from the 16A366 restore
/// (`RTKit_iOS-973.200.93.debug`, `dev_mailbox128.c`). The inbound-size check is
/// at VA `0x105e64c`..`0x105e65c` and the lookup it calls is at `0x106e33c`,
/// which decodes to a lock plus a **runtime-built chained hash table**: a
/// last-found cache at `+0x28`, a hash function pointer at `+0x0`, a bucket count
/// at `+0x8` divided into a bucket index with `udiv`/`msub`, a bucket array at
/// `+0x10` scaled by 8, and a chain walk comparing the key at each node's `+0x4`.
/// The value lives at the node's `+0x8`.
///
/// # Why this exists, and what it must not do
///
/// Because the table is populated at runtime, the payload limit is not a constant
/// anywhere in AOP -- and that is the whole point. The SecureROM blocks on the
/// number at `0x100003de4`, and this store is where that number has to come from.
/// Modelling it is legitimate because it holds no value of its own: it starts
/// empty, and until something configures it, [`AopPropertyStore::payload_limit`]
/// is `None` and [`AopPropertyStore::accepts_payload`] is `None` too. It cannot
/// invent a limit, and so it cannot unblock the DFU gate by itself -- which is the
/// correct behaviour, not a missing feature.
pub mod aop_properties {
    /// The property the mailbox driver increments for each received message.
    pub const IDX_RECEIVED_COUNT: u32 = 0x6c;
    /// `MailboxProperties::kMaxPayloadBytes` -- the limit the inbound size is
    /// checked against.
    pub const IDX_MAX_PAYLOAD_BYTES: u32 = 0xc8;

    /// Offset of a node's key from the node pointer, from the chain walk's
    /// `ldr w8, [x0, #4]`.
    pub const NODE_KEY_OFFSET: usize = 0x4;
    /// Offset of a node's value from the node pointer, from the `ldr w8, [x0, #8]`
    /// that every caller performs on the returned record.
    pub const NODE_VALUE_OFFSET: usize = 0x8;

    /// What the check decided about a payload.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum PayloadVerdict {
        /// `kBytesIn <= kMaxPayloadBytes`.
        Accepted,
        /// `kBytesIn > kMaxPayloadBytes`, which is exactly the condition the
        /// firmware's `!(kBytesIn > ...)` assertion reports.
        TooLarge { limit: u64 },
        /// The limit has not been configured, so the store cannot answer. The
        /// firmware has no answer either at this point: the property genuinely
        /// does not exist until the config path runs.
        Unconfigured,
    }

    /// The runtime property store: a chained hash table over property indices.
    ///
    /// Deterministic and self-contained; the hash is a real mix rather than the
    /// firmware's function pointer, because the pointer's identity is not
    /// evidence and what matters is that a key always lands in a stable bucket.
    #[derive(Clone, Debug)]
    pub struct AopPropertyStore {
        bucket_count: u32,
        buckets: Vec<Vec<(u32, u64)>>,
    }

    impl Default for AopPropertyStore {
        fn default() -> Self {
            Self::new(16)
        }
    }

    impl AopPropertyStore {
        /// A store with `bucket_count` buckets. Zero is coerced to one, since a
        /// hash table with no buckets cannot hold anything.
        pub fn new(bucket_count: u32) -> Self {
            let n = bucket_count.max(1);
            Self {
                bucket_count: n,
                buckets: vec![Vec::new(); n as usize],
            }
        }

        fn hash(&self, key: u32) -> u32 {
            // A 32-bit finalizer, in the spirit of a real string hash. The
            // firmware's hash function is reached through a pointer we cannot
            // read, so this reproduces its *role* (spread keys over buckets) and
            // not its value.
            let mut h = key.wrapping_mul(0x9E37_79B9);
            h ^= h >> 16;
            h = h.wrapping_mul(0x85EB_CA6B);
            h ^= h >> 13;
            h
        }

        fn bucket_of(&self, key: u32) -> usize {
            (self.hash(key) % self.bucket_count) as usize
        }

        /// Look a property up, as the lookup at `0x106e33c` does.
        pub fn get(&self, key: u32) -> Option<u64> {
            let bucket = &self.buckets[self.bucket_of(key)];
            bucket.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
        }

        /// Set a property, as the configuration path does once it has a value.
        pub fn set(&mut self, key: u32, value: u64) {
            let bucket = self.bucket_of(key);
            match self.buckets[bucket].iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => self.buckets[bucket].push((key, value)),
            }
        }

        /// The configured maximum payload size, or `None` if never configured.
        pub fn payload_limit(&self) -> Option<u64> {
            self.get(IDX_MAX_PAYLOAD_BYTES)
        }

        /// Increment and return the received-message counter, mirroring
        /// `ldr; add #1; str` at `0x105e638`..`0x105e648`.
        pub fn note_received(&mut self) -> u64 {
            let next = self.get(IDX_RECEIVED_COUNT).unwrap_or(0).saturating_add(1);
            self.set(IDX_RECEIVED_COUNT, next);
            next
        }

        /// Apply the firmware's inbound-size check to `kBytesIn`.
        pub fn accepts_payload(&self, k_bytes_in: u64) -> PayloadVerdict {
            match self.payload_limit() {
                None => PayloadVerdict::Unconfigured,
                Some(limit) if k_bytes_in > limit => PayloadVerdict::TooLarge { limit },
                Some(_) => PayloadVerdict::Accepted,
            }
        }

        /// Number of populated buckets, so a caller can confirm the hash is
        /// actually distributing keys rather than piling them into one.
        pub fn populated_buckets(&self) -> usize {
            self.buckets.iter().filter(|b| !b.is_empty()).count()
        }

        /// Number of properties held.
        pub fn len(&self) -> usize {
            self.buckets.iter().map(Vec::len).sum()
        }

        /// Whether the store holds no properties at all.
        pub fn is_empty(&self) -> bool {
            self.len() == 0
        }

        /// The byte offset of a key within a node, and of its value, as the
        /// firmware addresses them. Exposed so the layout is testable rather than
        /// only implied.
        pub fn node_offsets() -> (usize, usize) {
            (NODE_KEY_OFFSET, NODE_VALUE_OFFSET)
        }
    }
}

/// One mailbox, with both of its ends: the SEP-side channel table and the
/// IOP-side property store.
///
/// This exists to make the dependency that gates the DFU path *structural* rather
/// than documentary. From the two decodes:
///
/// * SEPOS `0x2c0000` accepts a config message on opcodes `0x4082`/`0x412c`
///   over 20 channels, and matches the message's value against the channel's
///   expected value (`mailbox_config`).
/// * AOP `0x106e33c` reads a per-mailbox property store and checks an inbound
///   payload against `kMaxPayloadBytes` (`aop_properties`).
///
/// So the SecureROM's `cmp w8, #0xa0` at `0x100003de4` can only pass if a limit
/// was configured, and the configuration is the part that has no witness in any
/// firmware image -- both stores are populated at runtime.
///
/// # The invariant, and why it is enforced by construction
///
/// [`MailboxEndpoint::configure`] takes the property values as an argument. It
/// has no default, no fallback, and no source for them, so the only way to get a
/// limit into this type is for a caller to supply one. Supplying one is a claim
/// about runtime state that some real execution has to support; nothing in the
/// type system or the defaults can make that claim on its own.
pub struct MailboxEndpoint {
    channels: mailbox_config::Channels,
    properties: aop_properties::AopPropertyStore,
}

impl Default for MailboxEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl MailboxEndpoint {
    /// A mailbox with both ends empty: no channels enabled, no properties set.
    pub fn new() -> Self {
        Self {
            channels: mailbox_config::Channels::default(),
            properties: aop_properties::AopPropertyStore::default(),
        }
    }

    /// The SEP-side channel table.
    pub fn channels(&self) -> &mailbox_config::Channels {
        &self.channels
    }

    /// The SEP-side channel table, mutably.
    pub fn channels_mut(&mut self) -> &mut mailbox_config::Channels {
        &mut self.channels
    }

    /// The IOP-side property store.
    pub fn properties(&self) -> &aop_properties::AopPropertyStore {
        &self.properties
    }

    /// The configured maximum payload size, or `None`.
    pub fn payload_limit(&self) -> Option<u64> {
        self.properties.payload_limit()
    }

    /// Apply a configuration to the mailbox.
    ///
    /// `channel` is enabled with `expected` on the SEP side, and `properties` is
    /// written into the IOP-side store verbatim. `properties` is a caller-supplied
    /// list precisely so that this function has no way to invent a value: pass an
    /// empty slice and the store stays empty, and the payload check stays
    /// [`aop_properties::PayloadVerdict::Unconfigured`].
    pub fn configure(&mut self, channel: u8, expected: u64, properties: &[(u32, u64)]) {
        self.channels.enable(channel, expected);
        for &(key, value) in properties {
            self.properties.set(key, value);
        }
    }

    /// The verdict AOP's size check would return for a payload of this length.
    pub fn payload_verdict(&self, k_bytes_in: u64) -> aop_properties::PayloadVerdict {
        self.properties.accepts_payload(k_bytes_in)
    }

    /// Record an inbound message on the IOP side and return the new count.
    pub fn note_received(&mut self) -> u64 {
        self.properties.note_received()
    }

    /// Whether a payload of the SecureROM's expected length would be accepted.
    ///
    /// This is the gate, expressed as a question the model can only answer
    /// truthfully: it is `Some(true)` only when a limit was actually configured
    /// and the payload fits it. Nothing in this crate can turn it to `Some(true)`
    /// on its own.
    pub fn accepts_securerom_descriptor(&self, k_bytes_in: u64) -> Option<bool> {
        match self.payload_verdict(k_bytes_in) {
            aop_properties::PayloadVerdict::Unconfigured => None,
            aop_properties::PayloadVerdict::Accepted => Some(true),
            aop_properties::PayloadVerdict::TooLarge { .. } => Some(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iphem_core::device::DeviceEvent;

    /// The mailbox interrupt producer, pinned to the mechanism PongoOS documents.
    ///
    /// `checkra1n/PongoOS` `src/drivers/sep/sep.c`, `sep_setup()`, programs
    /// `dis_int = 0; en_int = 0x1000;` on the mailbox at SEP reset, and binds a task
    /// to the SEP node's `interrupts`. This test pins the three properties that
    /// makes possible, and pins the fourth -- that nothing is raised before the
    /// enable is programmed -- because the previous model returned `None` from
    /// `tick` unconditionally, leaving an `irq` nothing could assert.
    #[test]
    fn mailbox_interrupts_its_peer_only_once_enabled_and_only_when_a_message_waits() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(7, 0xa4, 0xa4);

        // Nothing inbound, and not enabled: silent.
        assert_eq!(ap.tick(0), None);

        // A reply is staged low word then high word; the high-word write is what
        // enqueues it for the AP view (`iop_to_ap.push_back`) and what already
        // emitted the edge interrupt. `en_int` is still 0, so the *level* the
        // scheduler would test stays silent -- these are different mechanisms and
        // only the enable-gated one is being added here.
        assert_eq!(iop.write(0x4820, 4, 0x5566_7788), DeviceResult::Ok);
        assert_eq!(ap.tick(0), None, "staging a low word must not enqueue");
        assert_eq!(
            iop.write(0x4824, 4, 0x1122_3344),
            DeviceResult::Event(DeviceEvent::InterruptTo(0xa4, Some(0)))
        );
        assert_eq!(ap.iop_to_ap_pending(), 1, "the reply is now waiting");
        assert_eq!(ap.tick(0), None, "still not enabled");

        // Enabling the documented bit raises the AP view's mapped line.
        assert_eq!(
            ap.write(SEPMAIL_EN_INT, 8, SEPMAIL_INTR_ENABLE),
            DeviceResult::Ok
        );
        assert_eq!(
            ap.read(SEPMAIL_EN_INT, 8),
            DeviceResult::Value(SEPMAIL_INTR_ENABLE)
        );
        assert_eq!(
            ap.tick(0),
            Some(DeviceEvent::Interrupt(0xa4)),
            "an enabled mailbox with a message waiting must raise its IRQ"
        );

        // `dis_int` writing the same bit clears the enable and silences it again.
        assert_eq!(
            ap.write(SEPMAIL_DIS_INT, 8, SEPMAIL_INTR_ENABLE),
            DeviceResult::Ok
        );
        assert_eq!(ap.read(SEPMAIL_EN_INT, 8), DeviceResult::Value(0));
        assert_eq!(ap.tick(0), None);

        // The enable is per-view: the IOP view is not enabled by the AP's write.
        assert_eq!(iop.tick(0), None);
    }

    #[test]
    fn ap_and_iop_views_exchange_fifo_messages() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(0, 0, 1);
        // Enable AP→IOP interrupt so doorbell writes return Events.
        assert_eq!(ap.write(0x8104, 8, 0x1000), DeviceResult::Ok);
        assert_eq!(ap.write(0x8800, 8, 0x1122_3344_aabb_ccdd), DeviceResult::Ok);
        assert_eq!(
            ap.write(0x8808, 8, 0),
            DeviceResult::Event(DeviceEvent::InterruptTo(0, Some(1)))
        );
        assert_eq!(iop.read(0x4810, 4), DeviceResult::Value(0xaabb_ccdd));
        assert_eq!(iop.read(0x4814, 4), DeviceResult::Value(0x1122_3344));

        assert_eq!(iop.write(0x4820, 4, 0x5566_7788), DeviceResult::Ok);
        assert_eq!(
            iop.write(0x4824, 4, 0x99aa_bbcc),
            DeviceResult::Event(DeviceEvent::InterruptTo(1, Some(0)))
        );
        assert_eq!(
            ap.read(0x8830, 8),
            DeviceResult::Value(0x99aa_bbcc_5566_7788)
        );
        assert_eq!(ap.read(0x8838, 8), DeviceResult::Value(0));
    }

    #[test]
    fn unknown_t8020_control_registers_remain_zero_during_fifo_probe() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(0, 0, 1);
        assert_eq!(ap.read(0x8108, 4), DeviceResult::Value(0));
        assert_eq!(iop.read(0x4108, 4), DeviceResult::Value(0));
    }

    #[test]
    fn ap_4byte_doorbell_triggers_fifo_push() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(0, 0, 1);
        // Enable AP→IOP interrupt so doorbell writes return Events.
        assert_eq!(ap.write(0x8104, 8, 0x1000), DeviceResult::Ok);
        assert_eq!(ap.write(0x8800, 8, 0x1122_3344_aabb_ccdd), DeviceResult::Ok);
        assert_eq!(
            ap.write(0x8808, 4, 0),
            DeviceResult::Event(DeviceEvent::InterruptTo(0, Some(1)))
        );
        assert_eq!(iop.read(0x4810, 4), DeviceResult::Value(0xaabb_ccdd));
        assert_eq!(iop.read(0x4814, 4), DeviceResult::Value(0x1122_3344));
    }

    /// The AArch64 SecureROM's Current-EL IRQ and FIQ vectors are terminal
    /// (`VBAR_EL1 + 0x80`/`0x100` store a class code to mailbox `+4` and then
    /// `b .` with no `eret` path), and `AppleSEPROM-Cebu-A0` never programs
    /// `en_int`. The doorbell must therefore still carry the message and must
    /// not raise the coprocessor's line: raising it is a claim about the ROM.
    #[test]
    fn an_unarmed_doorbell_still_stages_the_message_and_raises_no_interrupt() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(0xa4, 0xa4, 1);
        assert!(!ap.a2i_interrupt_armed());
        // The AP's real 64-bit send: the first word is staged at `0x8800` and
        // the second lands on the doorbell at `0x8808`, which pushes the pair.
        assert_eq!(
            ap.write(0x8800, 8, 0x0003_67ff),
            DeviceResult::Ok,
            "an unarmed mailbox must not report an interrupt"
        );
        assert_eq!(
            ap.write(0x8808, 8, 0),
            DeviceResult::Ok,
            "an unarmed mailbox must not report an interrupt for the doorbell"
        );
        // The transport is untouched: the word is queued and readable by the SEP.
        assert_eq!(ap.ap_to_iop_pushes(), 1);
        assert_eq!(ap.ap_to_iop_pending(), 1);
        assert_eq!(iop.read(0x4810, 4), DeviceResult::Value(0x0003_67ff));
        assert_eq!(iop.read(0x4814, 4), DeviceResult::Value(0));
    }

    /// The other half of the same contract: arming the documented enable bit
    /// is what restores the interrupt, and clearing it takes it away again.
    #[test]
    fn the_receive_enable_governs_the_doorbell_edge_in_both_directions() {
        let (mut ap, _iop) = AppleAscMailbox::paired(0xa4, 0xa4, 1);
        assert_eq!(
            ap.write(SEPMAIL_EN_INT, 8, SEPMAIL_INTR_ENABLE),
            DeviceResult::Ok
        );
        assert!(ap.a2i_interrupt_armed());
        assert_eq!(
            ap.write(0x8800, 16, 0x0000_0000_0003_67ff),
            DeviceResult::Event(DeviceEvent::InterruptTo(0xa4, Some(1)))
        );
        // `dis_int` is a clear-bits write, so it silences the same doorbell.
        assert_eq!(
            ap.write(SEPMAIL_DIS_INT, 8, SEPMAIL_INTR_ENABLE),
            DeviceResult::Ok
        );
        assert!(!ap.a2i_interrupt_armed());
        assert_eq!(
            ap.write(0x8800, 16, 0x0000_0000_0003_6801),
            DeviceResult::Ok
        );
    }

    #[test]
    fn ap_16byte_store_pair_at_push_register_triggers_fifo_push() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(0, 0, 1);
        // Enable AP→IOP interrupt so the 16-byte store returns an Event.
        assert_eq!(ap.write(0x8104, 8, 0x1000), DeviceResult::Ok);
        assert_eq!(
            ap.write(0x8800, 16, 0x0000_0000_0003_67ff),
            DeviceResult::Event(DeviceEvent::InterruptTo(0, Some(1)))
        );
        assert_eq!(iop.read(0x4810, 4), DeviceResult::Value(0x0003_67ff));
        assert_eq!(iop.read(0x4814, 4), DeviceResult::Value(0));
    }

    /// The T8020 build pairs the SEP (ASC) mailbox at 0xa4 (164) — the only
    /// nonempty handlers in the SecureROM's interrupt table whose context is
    /// the mailbox base (0x19c0081c0). An IOP→AP push must assert exactly
    /// that SEP-mail line toward AP core 0.
    #[test]
    fn t8020_iop_push_asserts_sep_mail_line_0xa4() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        assert_eq!(ap.irq(), 0xa4);
        assert_eq!(iop.irq(), 0xa4);
        assert_eq!(iop.write(0x4820, 4, 0x5566_7788), DeviceResult::Ok);
        assert_eq!(
            iop.write(0x4824, 4, 0x99aa_bbcc),
            DeviceResult::Event(DeviceEvent::InterruptTo(0xa4, Some(0)))
        );
        assert_eq!(
            ap.read(0x8830, 8),
            DeviceResult::Value(0x99aa_bbcc_5566_7788)
        );
        assert_eq!(iop.irq(), 0xa4);
    }

    /// Read a mailbox register and return the raw value, panicking on a fault.
    fn read_value(dev: &mut AppleAscMailbox, offset: u64) -> u64 {
        match dev.read(offset, 4) {
            DeviceResult::Value(v) => v,
            other => panic!("mailbox read of {offset:#x} gave {other:?}"),
        }
    }

    /// The AP-view SEP-mail status words are derived from the mailbox's own
    /// receive queue, and exactly one of the two guest-tested bits is set.
    ///
    /// The SecureROM's message-array walk reads bit 16 of the `0x8108` send
    /// status to decide whether the request may be sent again
    /// (`0x10000a324: tbnz x25, #16`) and bit 17 of the `0x810c` receive status
    /// to decide whether an answer can be fetched
    /// (`0x10000a23c: tbnz x25, #17`). Both describe this device's receive
    /// queue, so they are computed from `iop_to_ap` rather than assumed:
    ///
    /// * queue empty     -> bit 17 set ("no answer yet"), bit 16 clear
    /// * answer queued   -> bit 16 set ("do not resend"), bit 17 clear
    ///
    /// They are complementary by construction, which is what lets the AP walk a
    /// descriptor to completion instead of spinning. Reporting a constant zero
    /// (the previous behaviour) asserted "send again" and "an answer is
    /// available" simultaneously and forever, so the walk could never finish.
    #[test]
    fn sepmail_status_words_report_the_receive_queue_state() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(1, 0xa4, 0xa4);

        // Nothing queued: no answer available, nothing pending.
        assert_eq!(read_value(&mut ap, 0x810c), SEPMAIL_STATUS_NO_REPLY);
        assert_eq!(
            read_value(&mut ap, 0x8108) & SEPMAIL_STATUS_REPLY_PENDING,
            0,
            "no queued answer means nothing is pending"
        );

        // Queue an answer the way a real SEP would, via its own reply registers.
        // A real SEP answer is written through the IOP-view reply registers.
        // The first word also raises the SEP-mail line toward the AP core, which
        // is how the AP is told to look; either return is acceptable here, the
        // point is that the answer lands in the shared receive queue.
        for (offset, word) in [(0x4820u64, 0x1111_1111u64), (0x4824, 0x2222_2222)] {
            assert!(matches!(
                iop.write(offset, 4, word),
                DeviceResult::Ok | DeviceResult::Event(_)
            ));
        }

        assert_eq!(
            read_value(&mut ap, 0x8108) & SEPMAIL_STATUS_REPLY_PENDING,
            SEPMAIL_STATUS_REPLY_PENDING,
            "a queued answer must suppress a resend"
        );
        assert_eq!(
            read_value(&mut ap, 0x810c) & SEPMAIL_STATUS_NO_REPLY,
            0,
            "an answer is available, so the fetch is not skipped"
        );

        // Collecting it at 0x8830 drains the queue and restores the empty state,
        // which is the transition that lets the walk finish.
        let _ = ap.read(0x8830, 8);
        assert_eq!(read_value(&mut ap, 0x810c), SEPMAIL_STATUS_NO_REPLY);
        assert_eq!(
            read_value(&mut ap, 0x8108) & SEPMAIL_STATUS_REPLY_PENDING,
            0,
            "a collected answer is no longer pending"
        );
    }

    /// The guest's own enable bit, raised by the open sequence's
    /// read-modify-write on these words, must survive the derived status bits.
    #[test]
    fn sepmail_status_words_retain_the_guest_enable_bit() {
        let (mut ap, _iop) = AppleAscMailbox::paired(1, 0xa4, 0xa4);
        assert_eq!(ap.read(0x8108, 4), DeviceResult::Value(0));
        assert_eq!(ap.write(0x8108, 4, 1), DeviceResult::Ok);
        let value = match ap.read(0x8108, 4) {
            DeviceResult::Value(v) => v,
            other => panic!("expected a value, got {other:?}"),
        };
        assert_eq!(value & 1, 1, "the guest's enable bit must round-trip");
    }

    /// A guest write must not be able to latch a derived status bit.
    ///
    /// The SEP-mail open does a read-modify-write on these words
    /// (`ldr; orr #1; str`). It reads the live status, ORs its enable bit and
    /// writes the whole result back, so a naive "retain what the guest wrote"
    /// stores the status bit too. Since the read ORs the retained value with a
    /// freshly derived one, a "no answer" bit observed once at open time would
    /// then be reported forever: the AP polled the receive status 671 times and
    /// never saw the queued reply, so it never fetched it and the exchange never
    /// completed. Masking the derived bits on the way in keeps the retained word
    /// limited to what the guest owns and makes the status always recomputed.
    #[test]
    fn guest_writes_cannot_latch_a_derived_status_bit() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(1, 0xa4, 0xa4);

        // Reproduce the open's read-modify-write with the queue empty, so the
        // value the guest reads already carries the "no answer" bit.
        let read_empty = read_value(&mut ap, 0x810c);
        assert_eq!(
            read_empty & SEPMAIL_STATUS_NO_REPLY,
            SEPMAIL_STATUS_NO_REPLY,
            "an empty queue must report no answer"
        );
        assert_eq!(ap.write(0x810c, 4, read_empty | 1), DeviceResult::Ok);

        // A reply arrives. The status must follow the queue, not the latched
        // value the guest wrote back.
        for (offset, word) in [(0x4820u64, 0x1111_1111u64), (0x4824, 0x2222_2222)] {
            assert!(matches!(
                iop.write(offset, 4, word),
                DeviceResult::Ok | DeviceResult::Event(_)
            ));
        }
        let after = read_value(&mut ap, 0x810c);
        assert_eq!(
            after & SEPMAIL_STATUS_NO_REPLY,
            0,
            "a queued reply must clear the no-answer bit despite the latched value"
        );
        assert_eq!(after & 1, 1, "the guest's own enable bit is still retained");
    }

    /// The same must hold for the send status word.
    #[test]
    fn guest_writes_cannot_latch_the_send_status_bit() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(1, 0xa4, 0xa4);
        let read_empty = read_value(&mut ap, 0x8108);
        assert_eq!(read_empty & SEPMAIL_STATUS_REPLY_PENDING, 0);
        // The guest writes back the whole word it read, plus its enable bit.
        assert_eq!(ap.write(0x8108, 4, read_empty | 1), DeviceResult::Ok);
        for (offset, word) in [(0x4820u64, 0x3333_3333u64), (0x4824, 0x4444_4444)] {
            assert!(matches!(
                iop.write(offset, 4, word),
                DeviceResult::Ok | DeviceResult::Event(_)
            ));
        }
        assert_eq!(
            read_value(&mut ap, 0x8108) & SEPMAIL_STATUS_REPLY_PENDING,
            SEPMAIL_STATUS_REPLY_PENDING,
            "a queued reply must be reported even though the guest wrote the word back"
        );
    }

    /// Per-offset access accounting must report the registers a guest really
    /// touches, which is how the unimplemented status window was identified.
    #[test]
    fn offset_access_counts_report_touched_registers() {
        let (mut ap, mut iop) = AppleAscMailbox::paired(1, 0xa4, 0xa4);
        let _ = ap.read(0x8108, 4);
        let _ = ap.read(0x8108, 4);
        let _ = ap.read(0x810c, 4);
        assert_eq!(ap.write(0x8800, 8, 0x1000_ffff), DeviceResult::Ok);
        let _ = iop.read(0x4810, 4);

        let (ap_r, ap_w, iop_r, _iop_w) = ap.offset_access_counts();
        assert_eq!(ap_r.get(&0x8108), Some(&2));
        assert_eq!(ap_r.get(&0x810c), Some(&1));
        assert_eq!(ap_r.get(&0x8800), None, "a write is not a read");
        assert_eq!(ap_w.get(&0x8800), Some(&1));
        assert_eq!(iop_r.get(&0x4810), Some(&1));
        assert!(!ap_r.is_empty());
    }

    /// The decoded SEPOS mailbox config protocol, pinned to the disassembly.
    mod decoded_config_protocol {
        use super::mailbox_config::{
            Channels, Outcome, CHANNEL_COUNT, ERR_BAD_INDEX, ERR_NO_ENTRY, ERR_VALUE_MISMATCH,
            OPCODES,
        };

        fn message(value: u64, channel: u8) -> Vec<u8> {
            let mut m = vec![0u8; 0x20];
            m[0x00..0x08].copy_from_slice(&value.to_le_bytes());
            m[0x18] = channel;
            m
        }

        /// Both opcodes the driver accepts, and nothing else.
        #[test]
        fn only_the_two_decoded_opcodes_are_accepted() {
            let mut ch = Channels::default();
            ch.enable(0, 0xaa);
            for op in OPCODES {
                assert_eq!(
                    ch.configure(op, &message(0xaa, 0)),
                    Outcome::Configured {
                        channel: 0,
                        value: 0xaa
                    },
                    "opcode {op:#06x} is one the driver accepts"
                );
            }
            for op in [0u16, 1, 0x4081, 0x4083, 0x412b, 0x412d] {
                assert_eq!(
                    ch.configure(op, &message(0xaa, 0)),
                    Outcome::Rejected(ERR_BAD_INDEX),
                    "opcode {op:#06x} is not one of the two"
                );
            }
        }

        /// The index bound is a strict `< 0x14`, so 0x13 is the last valid
        /// channel and 0x14 is the first rejected one.
        #[test]
        fn the_channel_bound_is_exactly_twenty() {
            assert_eq!(CHANNEL_COUNT, 20);
            let mut ch = Channels::default();
            ch.enable(0x13, 1);
            assert_eq!(
                ch.configure(OPCODES[0], &message(1, 0x13)),
                Outcome::Configured {
                    channel: 0x13,
                    value: 1
                }
            );
            ch.enable(0x14, 1);
            assert_eq!(
                ch.configure(OPCODES[0], &message(1, 0x14)),
                Outcome::Rejected(ERR_BAD_INDEX),
                "0x14 is one past the last valid index"
            );
        }

        /// A channel that was never enabled is the driver's no-entry case, which
        /// is distinct from a bad index and from a value mismatch.
        #[test]
        fn a_channel_that_was_never_enabled_is_rejected_distinctly() {
            let mut ch = Channels::default();
            assert_eq!(
                ch.configure(OPCODES[0], &message(0, 0)),
                Outcome::Rejected(ERR_NO_ENTRY)
            );
        }

        /// The value is matched against the channel's expected value, so a
        /// mismatch is its own error and does not disturb the stored value.
        #[test]
        fn a_value_mismatch_is_rejected_and_changes_nothing() {
            let mut ch = Channels::default();
            ch.enable(3, 0x1234);
            assert_eq!(
                ch.configure(OPCODES[0], &message(0x9999, 3)),
                Outcome::Rejected(ERR_VALUE_MISMATCH)
            );
            assert_eq!(ch.value(3), Some(0), "a rejected message must not write");
            assert_eq!(
                ch.configure(OPCODES[0], &message(0x1234, 3)),
                Outcome::Configured {
                    channel: 3,
                    value: 0x1234
                }
            );
            assert_eq!(ch.value(3), Some(0x1234));
        }

        /// The only byte of the channel word that is read is its low byte, so a
        /// message whose upper bytes are garbage still routes correctly.
        #[test]
        fn only_the_low_byte_of_the_channel_word_is_used() {
            let mut ch = Channels::default();
            ch.enable(5, 7);
            let mut m = message(7, 5);
            m[0x19] = 0xff;
            m[0x1a] = 0xff;
            m[0x1b] = 0xff;
            assert_eq!(
                ch.configure(OPCODES[0], &m),
                Outcome::Configured {
                    channel: 5,
                    value: 7
                }
            );
        }

        /// A short message is a rejected index, not a panic: the driver reads
        /// the channel word before the value word, so a truncated message never
        /// reaches the 8-byte value load.
        #[test]
        fn a_short_message_is_rejected_rather_than_panicking() {
            let mut ch = Channels::default();
            ch.enable(0, 0);
            assert_eq!(
                ch.configure(OPCODES[0], &[0u8; 4]),
                Outcome::Rejected(ERR_BAD_INDEX)
            );
        }

        /// The per-channel state offsets follow the driver's
        /// `index * 0x178 + 0xb90` arithmetic.
        #[test]
        fn channel_state_offsets_follow_the_decoded_stride() {
            assert_eq!(Channels::expected_offset(0), Some(0xb90));
            assert_eq!(Channels::expected_offset(1), Some(0xb90 + 0x178));
            assert_eq!(Channels::expected_offset(19), Some(0xb90 + 19 * 0x178));
            assert_eq!(Channels::expected_offset(20), None);
        }
    }

    /// The AOP-side property store, pinned to the decoded lookup.
    mod decoded_aop_property_store {
        use super::aop_properties::{
            AopPropertyStore, PayloadVerdict, IDX_MAX_PAYLOAD_BYTES, IDX_RECEIVED_COUNT,
        };

        /// A fresh store holds nothing, so it cannot report a limit. This is the
        /// property that keeps the store from being a back door for the gate.
        #[test]
        fn an_unconfigured_store_cannot_report_a_limit() {
            let store = AopPropertyStore::default();
            assert!(store.is_empty());
            assert_eq!(store.payload_limit(), None);
            assert_eq!(
                store.accepts_payload(0),
                PayloadVerdict::Unconfigured,
                "an unconfigured store must not pass a payload it knows nothing about"
            );
            assert_eq!(store.accepts_payload(0xa0), PayloadVerdict::Unconfigured);
        }

        /// The firmware compares with `b.hi`, so a payload *equal* to the limit
        /// is accepted and only strictly larger is rejected.
        #[test]
        fn the_limit_is_inclusive_and_160_is_accepted_at_160() {
            let mut store = AopPropertyStore::default();
            store.set(IDX_MAX_PAYLOAD_BYTES, 0xa0);
            assert_eq!(store.accepts_payload(0xa0), PayloadVerdict::Accepted);
            assert_eq!(store.accepts_payload(0x9f), PayloadVerdict::Accepted);
            assert_eq!(
                store.accepts_payload(0xa1),
                PayloadVerdict::TooLarge { limit: 0xa0 },
                "one byte over is the condition the assertion reports"
            );
        }

        /// The two decoded indices are the ones the driver actually uses.
        #[test]
        fn the_decoded_property_indices_are_used() {
            assert_eq!(IDX_RECEIVED_COUNT, 0x6c);
            assert_eq!(IDX_MAX_PAYLOAD_BYTES, 0xc8);
            let mut store = AopPropertyStore::default();
            store.note_received();
            assert_eq!(store.get(0x6c), Some(1));
            assert_eq!(store.get(0xc8), None);
        }

        /// The counter is read, incremented by one, and written back.
        #[test]
        fn the_received_counter_increments_monotonically() {
            let mut store = AopPropertyStore::default();
            assert_eq!(store.note_received(), 1);
            assert_eq!(store.note_received(), 2);
            assert_eq!(store.note_received(), 3);
            assert_eq!(store.get(IDX_RECEIVED_COUNT), Some(3));
        }

        /// Setting an existing key overwrites in place rather than growing the
        /// table, so reconfiguration does not accumulate duplicate entries.
        #[test]
        fn reconfiguring_a_key_overwrites_it() {
            let mut store = AopPropertyStore::default();
            store.set(IDX_MAX_PAYLOAD_BYTES, 0x40);
            store.set(IDX_MAX_PAYLOAD_BYTES, 0xa0);
            assert_eq!(store.payload_limit(), Some(0xa0));
            assert_eq!(store.len(), 1);
        }

        /// The hash must actually spread keys: a store whose every key lands in
        /// one bucket would still be correct but would not be the chained table
        /// the firmware decodes to.
        #[test]
        fn the_hash_distributes_keys_across_buckets() {
            let mut store = AopPropertyStore::new(16);
            for key in 0..64u32 {
                store.set(key, key as u64);
            }
            assert_eq!(store.len(), 64, "every key must be held exactly once");
            assert!(
                store.populated_buckets() > 8,
                "64 keys over 16 buckets left only {} populated",
                store.populated_buckets()
            );
            for key in 0..64u32 {
                assert_eq!(store.get(key), Some(key as u64));
            }
        }

        /// A degenerate bucket count must not panic or lose data.
        #[test]
        fn a_degenerate_bucket_count_still_works() {
            let mut one = AopPropertyStore::new(0);
            one.set(IDX_MAX_PAYLOAD_BYTES, 0xa0);
            assert_eq!(one.payload_limit(), Some(0xa0));
            let mut single = AopPropertyStore::new(1);
            for key in 0..8u32 {
                single.set(key, key as u64);
            }
            assert_eq!(single.len(), 8);
            assert_eq!(single.get(7), Some(7));
        }

        /// The key and value offsets the lookup addresses are the decoded ones.
        #[test]
        fn node_layout_matches_the_decoded_offsets() {
            assert_eq!(AopPropertyStore::node_offsets(), (0x4, 0x8));
        }
    }

    /// The two mailbox halves, tied together, and the invariant that matters.
    mod mailbox_endpoint_gate {
        use super::{
            aop_properties::IDX_MAX_PAYLOAD_BYTES, mailbox_config::Outcome, MailboxEndpoint,
        };

        /// A fresh endpoint cannot say whether the SecureROM's descriptor is
        /// acceptable. This is the single most important assertion in the file:
        /// the gate is `None` until a value is supplied, and nothing in the crate
        /// supplies one.
        #[test]
        fn a_fresh_endpoint_cannot_answer_the_securerom_gate() {
            let endpoint = MailboxEndpoint::new();
            assert_eq!(endpoint.payload_limit(), None);
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), None);
            assert_eq!(endpoint.accepts_securerom_descriptor(0), None);
            assert!(endpoint.properties().is_empty());
        }

        /// Configuring with no properties leaves the limit unconfigured, so an
        /// empty slice cannot be mistaken for a wildcard that passes anything.
        #[test]
        fn configuring_with_no_properties_does_not_create_a_limit() {
            let mut endpoint = MailboxEndpoint::new();
            endpoint.configure(0, 0xaa, &[]);
            assert_eq!(endpoint.payload_limit(), None);
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), None);
            assert_eq!(endpoint.channels().value(0), Some(0));
        }

        /// With a limit actually configured, the gate answers, and it answers the
        /// firmware's way: inclusive at the limit.
        #[test]
        fn a_configured_limit_answers_the_gate() {
            let mut endpoint = MailboxEndpoint::new();
            endpoint.configure(0, 0xaa, &[(IDX_MAX_PAYLOAD_BYTES, 0xa0)]);
            assert_eq!(endpoint.payload_limit(), Some(0xa0));
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), Some(true));
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa1), Some(false));
            assert_eq!(endpoint.accepts_securerom_descriptor(0x9f), Some(true));
        }

        /// The SEP half still validates independently: enabling a channel does
        /// not by itself make a config message acceptable.
        #[test]
        fn the_sep_half_keeps_its_own_typed_rejections() {
            let mut endpoint = MailboxEndpoint::new();
            endpoint.configure(3, 0x1234, &[]);
            let channels = endpoint.channels_mut();
            let mut msg = vec![0u8; 0x20];
            msg[0x00..0x08].copy_from_slice(&0x1234u64.to_le_bytes());
            msg[0x18] = 3;
            assert_eq!(
                channels.configure(0x4082, &msg),
                Outcome::Configured {
                    channel: 3,
                    value: 0x1234
                }
            );
            msg[0x00..0x08].copy_from_slice(&0x9999u64.to_le_bytes());
            assert_eq!(channels.configure(0x4082, &msg), Outcome::Rejected(-7));
        }

        /// The received counter is shared state on the endpoint, independent of
        /// whether a payload limit exists.
        #[test]
        fn traffic_is_counted_even_with_no_limit_configured() {
            let mut endpoint = MailboxEndpoint::new();
            assert_eq!(endpoint.note_received(), 1);
            assert_eq!(endpoint.note_received(), 2);
            assert_eq!(endpoint.properties().get(0x6c), Some(2));
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), None);
        }

        /// Reconfiguring a different limit changes the answer, so the gate
        /// tracks configuration rather than caching a first answer.
        #[test]
        fn reconfiguring_the_limit_changes_the_answer() {
            let mut endpoint = MailboxEndpoint::new();
            endpoint.configure(0, 0, &[(IDX_MAX_PAYLOAD_BYTES, 0x40)]);
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), Some(false));
            endpoint.configure(0, 0, &[(IDX_MAX_PAYLOAD_BYTES, 0xa0)]);
            assert_eq!(endpoint.accepts_securerom_descriptor(0xa0), Some(true));
        }
    }
}
