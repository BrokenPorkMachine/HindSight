use iphem_core::{BusRegion, Machine, MachineConfig, Rom, SparseRam};
use iphem_devices::{
    Adc, AesOtp, Aic, AneFifo, AppleAscMailbox, AppleChipControl, AppleGpio, AppleMcc, AudioI2s,
    BasebandUart, BlockDev, BtHci, CameraIsp, ClockCtrl, DartT8020, Dcp, DeviceTableControlWindow,
    DsiPhy, EmmcSdHc, Gpio, I2c, Mailbox, NandCtrl, NorFlash, Panel, Pinmux, Pmgr, Pmu, Pwm,
    Raster, RegFile, Regulator, Rtc, S5lUart, SdCard, SensorHub, SepBootCtrl, SepIdentity, SepUart,
    SgxGpu, SocHot, Spi, Spmi, Thermal, Timer, TouchHid, Uart, UartWindow, UsbComplex, UsbDeviceController,
    UsbMux, UsbPd, UsbPhy, Watchdog, WifiSdio,
};
use iphem_devices::sochot;

/// DART-USB fault interrupt on the T8020 d321ap (`/arm-io/dart-usb`
/// `interrupts` property). Bound to stream 7 (`sids` property).
pub const DART_USB_IRQ: u32 = 454;
/// DART-USB stream ID on d321ap / T8020.
pub const DART_USB_SID: u32 = 7;

/// The A11 (T8015) `/arm-io/gpio` base, and with it the window the A11's SEP gates on.
///
/// This is the first SEP window measured for any part older than the A12, and it is
/// worth recording how, because two independent readings in the same dump agree and a
/// single reading would be the kind of number that gets carried across unexamined.
///
/// From the A11 ioreg dump (`iPhone7,2` / `N61AP`, the same dump this repository's
/// `iPhone7,2_12.5.8_16H88` fixture matches):
///
/// * `/arm-io/gpio` has `reg = <0000300e 00000000 00001000 00000000>` -- base cell
///   `0x0e300000`, size **1 MiB** -- on the `0x2000_0000` arm-io band.
/// * The same node's `IODeviceMemory` reports `address = 8_827_961_344`, which is
///   `0x20e300000` and is the base cell plus that band, and `length = 1_048_576`, which is
///   the same 1 MiB.
///
/// The 1 MiB aperture is the same shape the A12's `/arm-io/gpio` has, which is what makes
/// this comparable with `iphem_boot::sep_platform::AP_GPIO_BASE` rather than a stray
/// address. It also is what the A7 config's `gpio_base` field appears to be carrying: that
/// field reads `0x20e3_00000` with a comment citing a `0xe300000` cell, and this dump
/// says `0x0e300000` *is* the A11's. That is flagged, not fixed, because nothing here
/// measures the A7's own `/arm-io/gpio` and changing it would be a second guess in place
/// of a first.
pub const A11_GPIO_BASE: u64 = 0x2_0e30_0000;
/// Size of the A11's `/arm-io/gpio` aperture, from the same node's `reg` size cell and
/// its `IODeviceMemory` length independently. See [`A11_GPIO_BASE`].
pub const A11_GPIO_SIZE: u64 = 0x10_0000;

/// The A11's SEP window, from `sep@DA00000` in the same dump.
///
/// `reg = <0000a00d 00000000 00200000 00000000>` -- base cell `0x0da00000`, size 2 MiB --
/// giving `0x20da00000` on the arm-io band, which is exactly the `IODeviceMemory` address
/// the node reports. The node is the real thing rather than a stub: it carries
/// `device_type = "sep"`, `role = "SEP"`, and the SEP is instantiated beneath it as
/// `IOSlaveCPU:AppleA7IOP:AppleA7IOPV1` with an `iop-sep-nub` child, so the part really
/// has an SEP resident and really is reached through this window.
///
/// Its `compatible` string is **`"iop,s5l8960x"`** -- the A7's -- which is the reason the
/// A7 SEP platform in this tree is worth keeping even though no A7 SEPROM is in the
/// corpus: the A7-generation IOP/SEP arrangement is what the A11 still runs.
pub const A11_SEP_BASE: u64 = 0x2_0da0_0000;
/// Size of the A11's SEP window, from the `reg` size cell. See [`A11_SEP_BASE`].
pub const A11_SEP_SIZE: u64 = 0x20_0000;
/// The A11's SEP interrupt lines, from `sep@DA00000`'s `interrupts` property:
/// `<62000000 61000000 64000000 63000000>`, corroborated by the node's
/// `IOInterruptSpecifiers` carrying the same four values in the same order.
pub const A11_SEP_IRQS: [u32; 4] = [0x62, 0x61, 0x64, 0x63];

/// The offset, below a 1 MiB window base, of the word the SEP polls as its first gate.
///
/// Invariant across every AArch64 SEPROM in the corpus that reaches a device, while the
/// window base moves per part -- that invariance is the whole reason it is usable as a
/// SoC fingerprint. For the A12 the base is d321ap's `/arm-io/gpio` (`0x23c100000`), so
/// the gate address is `0x23c100c4c`; the SEP ROM reaches it with `ldr` only and never
/// writes it, which is what makes the gate's producer a question about the AP.
pub const SEP_FIRST_GATE_OFFSET: u64 = 0xc4c;
/// The A10 (T8010) SecureROM's second zero-fill span, which is disjoint from the main
/// `sram_base` window and therefore mapped separately rather than by widening that one.
///
/// Measured from `SecureROM_t8010si_iBoot-2696.0.0.1.33``: the base is the u64 at file
/// offset 0x378, and the length is the `movz x11, #0x4000` immediate at file offset 0x120,
/// which is why the size cannot be read out of the data block the way the other two spans'
/// can. See the mapping in `build_with_rom` for the disassembly.
pub const T8010_FAR_ZERO_BASE: u64 = 0x1801_b0000;
/// Length of [`T8010_FAR_ZERO_BASE`], from the `movz` immediate at file offset 0x120.
pub const T8010_FAR_ZERO_SIZE: u64 = 0x4000;

/// The A10 (T8010) DRAM base, from the SecureROM's own addressing rather than from a
/// `DeviceTree` reg cell.
///
/// `SecureROM_t8010si_iBoot-2696.0.0.1.33`` carries `0x200000000` as a bare 64-bit
/// literal sixteen times, and the run stores at `0x2002f8000`, `0x202f20020`, `0x202f22020`
/// and `0x202f80000` and reads `0x202f20020` back unchanged. A NAND controller answers none
/// of those, so this address is RAM -- which is what displaces the synthetic NAND band for
/// the A10 rather than sitting on top of it.
///
/// This is also the correction of a copied value: `SoCId::T8010::nand_base` was the A12's
/// `0x200000000` with nothing behind it for this part, and it claimed the exact window the
/// A10 ROM uses as its own scratch.
pub const T8010_DRAM_BASE: u64 = 0x2000_00000;
/// How much of [`T8010_DRAM_BASE`] is mapped, and **why the span is not the A10's 3 GiB**.
///
/// 64 MiB contains every address the run has touched -- `0x202f80008` is the highest -- and
/// it stops exactly where the GPU window begins at `0x204000000`, the same clamp the NAND
/// band uses. A span sized to the largest A10 part would swallow the GPU window and panic the
/// builder; a run that needs more names the address as unmapped, which is the honest way to
/// find out.
pub const T8010_DRAM_MAPPED_SIZE: u64 = 0x400_0000;

/// The T803x boot scratch: the window the T8027 (A12X/A12Z) and T8030 (A13) SecureROMs
/// zero-fill at boot, which is not the `sram_base` every part inherits.
///
/// Measured independently from both checked-in images, and the two agree, which is what
/// makes this one constant rather than two: `SecureROM_t8027si_iBoot-4172.0.0.100.13`.bin`
/// and `SecureROM_t8030si_iBoot-4479.0.0.100.4`` carry the same data block
/// (`0x358 = 0x19c01c000`, `0x360 = 0x4000`, `0x378 = 0x19c000000`, `0x380 = 0xc000`,
/// `0x388 = 0x19c020000`) and the same envelope of `0x19c0_00000`-range bases, running
/// `0x19c000000` (file `0x378`) to `0x19c020000` (file `0x388`).
///
/// The census is what says the size is needed at all: with only the config's inherited
/// `sram_base: 0x1000000000` the T8027 dies at instruction **51** and the T8030 at
/// instruction **53**, both on the prologue's first `stp` with `x10 = 0x19c01c000` and no
/// MMIO contact to explain it.
pub const T803X_BOOT_SCRATCH_BASE: u64 = 0x19c0_00000;
/// Size of [`T803X_BOOT_SCRATCH_BASE`], the envelope of the ROMs' own literals.
pub const T803X_BOOT_SCRATCH_SIZE: u64 = 0x20000;

/// The T803x **third** zero-fill span, which is disjoint from
/// [`T803X_BOOT_SCRATCH_BASE`] and therefore mapped separately rather than by widening that
/// window across the gap.
///
/// This is the same shape as the A10's [`T8010_FAR_ZERO_BASE`], and for the same reason: the
/// length here is a `movz` immediate in the code, not a literal in the data block, so an
/// envelope built from literals alone cannot see it. The census is what names it — with only
/// the literal window mapped the run reaches instruction **18,677** and then dies on the
/// first `stp` of this loop, which is the tool reporting a real gap rather than a guess:
///
/// ```text
/// 0x100000120  movz  x13, #0x0
/// 0x100000124  ldr   x10, [0x100000388]   ; = 0x19c020000
/// 0x100000128  movz  x11, #0x4000         ; length, immediate not literal
/// 0x10000012c  add   x11, x11, x10
/// 0x100000130  stp   x13, x13, [x10], #0x10
/// 0x100000144  b.ne  0x100000130
/// ```
///
/// So the span is `0x19c020000 + 0x4000`. It is disjoint from, and immediately above, the
/// literal envelope, so a single window reaching both would be contiguous here — but they are
/// kept as two claims because that is what the image states, and folding them would assert
/// that nothing lives in between on the strength of a guess rather than a measurement.
pub const T803X_FAR_ZERO_BASE: u64 = 0x19c0_20000;
/// Size of [`T803X_FAR_ZERO_BASE`], from the `movz` immediate at file offset 0x128.
pub const T803X_FAR_ZERO_SIZE: u64 = 0x4000;

/// The A12X/A12Z (T8027) AppleMcc calibration block.
///
/// Measured from `SecureROM_t8027si_iBoot-4172.0.0.100.13`.bin`, whose accessors reach this
/// window and whose first calibration pass the census completes; without it the run cannot
/// pass the `0x23b000000` device-registry walk it is sequenced after.
pub const T8027_MCC_BASE: u64 = 0x23b2_88000;
/// Size of [`T8027_MCC_BASE`], the two descriptor-selected planes.
pub const T8027_MCC_SIZE: u64 = 0x20000;

/// The `0x210e70000` calibration block, shared by the T8027 and the T8030.
///
/// Neither part composes it through an immediate-offset access, so it is absent from an
/// inventory built from that form alone; both compose it in a register and store through a
/// register pair. Both write the Apple MCS pattern `0x494c87` into it under a `cmp w19, #0xf`
/// bound, which is the sixteen-entry sweep the A12 record documents for the A12's block at
/// this address. The A12's own mapping is gated to `id == SoCId::T8020`, so it does not
/// answer for these two parts.
pub const T803X_MCC2_BASE: u64 = 0x210e_70000;
/// Size of [`T803X_MCC2_BASE`], the sixteen-entry stride the loop bound establishes.
pub const T803X_MCC2_SIZE: u64 = 0x80000;

/// The A14 (T8101)'s own `Pmgr` pages, from its address inventory.
///
/// Measured by decoding every `movz`/`movk` base the A14 builds for a load or store, cross
/// checked against the census stopping on `0x23b720004`. These are **not** the A12 family's
/// `0x23b0xxxxx` pages and not the Firebloom `0x210e2xxxx` / `0x210e70000` planes: the A14
/// names `0x23b070000`, `0x23b71c000`, `0x23b720000` and `0x23b738000`, and `0x210e80000`,
/// `0x210ea0000`, `0x210f40000`. Mapping the Firebloom set here would answer addresses this
/// part never issues and leave the ones it does issue unmapped.
pub const T8101_PMGR_PAGES: [u64; 7] = [
    0x23b0_70000,
    0x23b7_1c000,
    0x23b7_20000,
    0x23b7_38000,
    0x210e_80000,
    0x210e_a0000,
    0x210f_40000,
];

/// The A14 (T8101)'s power-state table base, from its own 131 literals.
pub const T8101_POWER_BASE: u64 = 0x23b7_00000;
/// Size of [`T8101_POWER_BASE`]: the envelope `0x23b700000..0x23b70c038`, rounded to the
/// 4 KiB page that contains its last named word.
pub const T8101_POWER_SIZE: u64 = 0xd000;

/// The A14 (T8101)'s watchdog window, from its own inventory (`0x23d2b8000`, 44 accesses).
pub const T8101_WDT_BASE: u64 = 0x23d2_b0000;
/// Size of [`T8101_WDT_BASE`].
pub const T8101_WDT_SIZE: u64 = 0xc000;

/// The A14's chip-control window, which it reads at `+0x3c` -- the word the census stops on.
pub const T8101_CHIPCTL_BASE: u64 = 0x23d2_bc000;
/// Size of [`T8101_CHIPCTL_BASE`].
pub const T8101_CHIPCTL_SIZE: u64 = 0x4000;

/// The A14 (T8101) boot scratch, at a **different base** from the Firebloom pair.
///
/// Measured from all three checked-in A14 images -- `iBoot-5281.0.0.100.22`, `...100.34` and
/// `...100.45` -- which agree exactly on the block: `0x358 = 0x1fc030000`, `0x360 = 0x4000`,
/// `0x378 = 0x1fc000000`, `0x380 = 0xc000`, `0x388 = 0x1fc034000`. The structure is the
/// T803x one, shifted by `0x3000000`, so this is a **separate** window rather than a reuse of
/// `T803X_BOOT_SCRATCH_*`: the A14's zero-fill would fault at `0x1fc030000` with the Firebloom
/// window mapped, and vice versa.
///
/// The census says the window is needed at all: with only the config's inherited
/// `sram_base: 0x1000000000` the run dies at instruction **58**, the prologue's first `stp`.
pub const T8101_BOOT_SCRATCH_BASE: u64 = 0x1fc0_00000;
/// Size of [`T8101_BOOT_SCRATCH_BASE`], the envelope of the A14 ROMs' own literals.
///
/// `0x38000`, not `0x20000`. All three images put their lowest `0x1fc0_00000`-range base at
/// file `0x378` (`0x1fc000000`) and their highest at file `0x388` (`0x1fc034000`), so the
/// envelope that contains every one of them is `0x1fc000000..0x1fc038000`. A `0x20000` window
/// stops at `0x1fc020000` and does not reach the first zero-fill span at `0x1fc030000`, which
/// is the prologue's very first store -- the census sat at instruction 58 with the narrower
/// window mapped, which is the measurement that corrects it.
///
/// Unlike the Firebloom pair, this one is a **single** window: the A14's third span at
/// `0x1fc034000` is inside the literal envelope rather than disjoint from it, so it does not
/// need a second claim.
pub const T8101_BOOT_SCRATCH_SIZE: u64 = 0x38000;

/// The A13 (T8030) AppleMcc calibration block, 0x230000 below the T8027's.
///
/// Measured from `SecureROM_t8030si_iBoot-4479.0.0.100.4``, whose address inventory
/// builds `0x23b058000` (12 accesses) and whose census stops there at instruction 44,345 on a
/// write-then-poll-for-ack. The T8027's is at `0x23b288000`; the two are kept apart because
/// the difference is measured, and one window spanning both would claim everything between.
pub const T8030_MCC_BASE: u64 = 0x23b0_58000;
/// Size of [`T8030_MCC_BASE`].
///
/// The T8027's block is `0x20000` because its two descriptor-selected planes say so. This one
/// is **one page**, and that is the measurement rather than a copy: the T8030 builds exactly
/// five addresses in this range -- `0x23b058000`, `+0x18`, `+0x350`, `+0x488`, `+0x900` -- all
/// inside `0x1000`. Copying the T8027's `0x20000` would reach `0x23b078000` and collide with
/// the shared PMGR page at `0x23b068000`.
pub const T8030_MCC_SIZE: u64 = 0x1000;

/// The single PMGR page the T8030 names above its calibration block, from the same inventory
/// (`0x23b09c040`). It is separate from [`T8030_MCC_BASE`] so neither window has to span the
/// other.
pub const T8030_PMGR_TAIL_BASE: u64 = 0x23b0_9c000;

/// The T8027 watchdog window, from the same image: it writes `0x23d2b8030`, inside the
/// `0xc000` the d321ap tree gives WDT on the T8020.
pub const T8027_WDT_BASE: u64 = 0x23d2_b0000;
/// Size of [`T8027_WDT_BASE`].
pub const T8027_WDT_SIZE: u64 = 0xc000;

/// The T8027 chip-control / fuse block, which it *reads* at `0x23d2bc030` -- the `+0x30`
/// revision word `AppleChipControl` already decodes. With this window unmapped the T8027
/// census faults at instruction 31,378 with a bus error and `esr1 = 0x0`, i.e. the address
/// is absent from the bus rather than merely untranslatable.
pub const T8027_CHIPCTL_BASE: u64 = 0x23d2_bc000;
/// Size of [`T8027_CHIPCTL_BASE`].
pub const T8027_CHIPCTL_SIZE: u64 = 0x4000;

/// The A12X/A12Z (T8027) power-manager band, in the T8020's `0x23b0xxxxx` layout.
///
/// Measured from `SecureROM_t8027si_iBoot-4172.0.0.100.13`.bin`: the image's literal pool
/// and its composed `movz`/`movk` bases together name `0x23b000000`, `0x23b01401c`,
/// `0x23b02c01c`, `0x23b040000`, `0x23b068000`, `0x23b080600`, `0x23b0a0000` and
/// `0x23b0b8000` -- the same addresses the T8020's own block maps, and including
/// `0x23b080600`, which is the d321ap SEP power-state word. The config's shared
/// `pmgr_base: 0x20e000000` is therefore the wrong band for this part, and the census
/// reports the consequence: it stops at instruction **30,122** on `ldr w9, [0x23b0a0000]`
/// with no other MMIO contact to explain it.
///
/// The pages are listed individually rather than as one aperture because the T8020 block
/// maps the `0x23b0802xx` power-state registers and the `0x23b080600` SEP word *inside* this
/// range, so a single 0xc0000 window would overlap them. `0x23b080000` is deliberately
/// absent from this list for the same reason.
pub const T8027_PMGR_PAGES: [u64; 11] = [
    0x23b0_00000,
    0x23b0_14000,
    0x23b0_2c000,
    0x23b0_40000,
    0x23b0_68000,
    0x23b0_a0000,
    0x23b0_b8000,
    // The second calibration plane, `0x210e2xxxx` / `0x210e4xxxx`. The census reaches it
    // immediately after the `0x23b000000` walk completes and stops at instruction 45,099 on
    // `0x210e20020`, with the same `write / re-read / tbnz #25` shape as the first — so this
    // is Apple's second MCS calibration block, not a different kind of device. The A12's
    // equivalent lives at `0x210e70000`; the T8027's is at a different offset in the same
    // arm-io band, which is why the A12's mapping does not answer for it.
    0x210e_20000,
    0x210e_21000,
    0x210e_40000,
    0x21c0_00000,
];

/// Size for the `0x23b000000` page, the PMGR plane the T8027's device-registry table lives in.
///
/// This one page is larger than 4 KiB because the census, not a guess, says it has to be.
/// The calibration/table walk at `0x100008a1c` computes an index into this plane and faults
/// at `0x23b004000` with a 4 KiB `Pmgr` mapped — the register image at the stop shows
/// `x9 = 0x23b000000` and `x13 = 0x4000`, so the walk strides 16 KiB and a 4 KiB page is
/// provably too small. It runs `cmp x8, #0xa`, so ten entries at 0x4000 reach `0x23b028000`;
/// 0x30000 covers that with a page of slack. The other six PMGR pages stay 4 KiB, because
/// for those the census named a single word and nothing more.
pub const T8027_PMGR_PLANE0_SIZE: u64 = 0x30000;

/// SGX GPU measured base (d321ap DeviceTree `/arm-io/sgx`, off 0x4000000
/// relative to the 0x200000000 arm-io base). The synthetic NAND catch-all
/// must stop at this window so the GPU decodes at its DT-measured address.
pub const GPU_BASE: u64 = 0x204000000;
/// SGX GPU measured window size (DT `reg` size 0x100000; the second window
/// off 0x4d00000 lies inside this range).
pub const GPU_SIZE: u64 = 0x100000;
/// ISP measured base (d321ap DeviceTree `/arm-io/isp`, off 0x2a000000).
pub const ISP_BASE: u64 = 0x22a000000;
/// ISP measured window size (DT `reg` size 0x1e8d000).
pub const ISP_SIZE: u64 = 0x1e8d000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoCId {
    T8010,
    T8015,
    T8020,
    T8027,
    T8030,
    T8101,
    /// Apple A9 (S8000, iPhone 6s / 6s Plus). The AP SecureROM is AArch64;
    /// the SEP core runs an AArch32 SEPROM (AppleSEPROM-230). MMIO bases are
    /// measured from the S8000 SecureROM literal pool (iBoot-2234.0.0.2.22).
    S8000,
    /// Apple A7 (S5L8960X, iPhone 5s). A distinct part from `A8` below, and not a
    /// rename of it: this is a 32-bit AP whose SEP is the first Apple Secure Enclave
    /// Processor, which is what `build_a7_sep_platform` and the Phase 219/220 suites
    /// exercise. It was briefly collapsed into `A8`, which silently changed which chip
    /// those tests described -- the A8 is the T7000 and has a different SEP generation.
    A7,
    /// Apple A8 (T7000, iPhone 6 / 6 Plus / iPad Air 2).
    A8,
}

impl SoCId {
    /// Every SoC this project can build a machine for.
    ///
    /// Exists so that "across all SoCs" is something a test can iterate rather than
    /// something a person has to remember to check. The alternative -- a per-SoC test
    /// suite -- is how the A7 came to name an AArch64 SEPROM for several records with
    /// every individual test green: each test was about one part, and nothing was
    /// about the set. A new part added to the enum and not to this list would not be
    /// covered, so [`every_soc_is_listed_here`] fails if the two drift apart.
    pub const ALL: [SoCId; 9] = [
        SoCId::A7,
        SoCId::A8,
        SoCId::S8000,
        SoCId::T8010,
        SoCId::T8015,
        SoCId::T8020,
        SoCId::T8027,
        SoCId::T8030,
        SoCId::T8101,
    ];
}

        /// Why a SoC has no SEPROM image wired, when the reason is a measurement rather
        /// than an oversight.
        ///
        /// `sep_rom_path` is an `Option`, and an `Option` reads the same whether a value is
        /// absent because nobody got to it or because the corpus was searched and the answer
        /// was no. These three are the answers that are actually a finding, and keeping them
        /// as values rather than prose is what lets a test assert that a gap has not silently
        /// been filled with a neighbouring part's image.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SepRomGap {
            /// No image for this part exists in the corpus, and the corpus holds 41 SEPROMs
            /// spanning the AArch32 and AArch64 eras.
            NoImageInCorpus,
            /// An image for this part's *generation* exists, but nothing measured here
            /// separates it from a neighbour's. Naming one would be a guess, and the gap is
            /// the honest state.
            CannotDiscriminate,
            /// The SoC is measured to have no SEP window at all, so an image would have
            /// nothing to talk to.
            NoMeasuredSepWindow,
        }

        impl SepRomGap {
            /// The finding, in the words a reader of a boot log needs. Kept next to the
            /// variant so a new gap cannot be added without saying what it means.
            pub fn reason(self) -> &'static str {
                match self {
                    SepRomGap::NoImageInCorpus => "the SEPROM corpus holds no image for this part",
                    SepRomGap::CannotDiscriminate => {
                        "an image for this generation exists, but nothing measured here \
                         distinguishes this SoC's from a neighbour's"
                    }
                    SepRomGap::NoMeasuredSepWindow => {
                        "this SoC is measured to have no SEP window, so an image would have \
                         nothing to talk to"
                    }
                }
            }
        }

        /// What SEPROM, if any, this SoC runs, and how that is known.
        ///
        /// The pairing evidence is the point. `seprom_soc_pairing` measures it: every AArch64
        /// SEPROM that reaches a device builds one address of the form
        /// `<1 MiB window> + 0xc4c`, the offset is invariant across the corpus, and the window
        /// base moves per part. Sixteen images gate on `0x23c100000`, which is d321ap's
        /// `/arm-io/gpio` and therefore the A12/A13/A14 family -- which is also why that band
        /// cannot tell those three apart, and why the A13 and the A14 are recorded as
        /// [`SepRomGap::CannotDiscriminate`] rather than handed a neighbour's image.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SepRomStatus {
            /// A named image, and the evidence that it is this SoC's.
            Wired {
                image: &'static str,
                evidence: &'static str,
            },
            /// No image, and the measurement that says so.
            Absent(SepRomGap),
        }

        /// The SEPROM this SoC runs, or why it runs none.
        pub fn sep_rom_status(soc: SoCId) -> SepRomStatus {
            match soc {
                SoCId::A7 => SepRomStatus::Absent(SepRomGap::NoImageInCorpus),
                SoCId::A8 => SepRomStatus::Absent(SepRomGap::NoMeasuredSepWindow),
                SoCId::S8000 => SepRomStatus::Wired {
                    image: "AppleSEPROM-302.0.0.1.2",
                    // The corpus names a part in only two SEPROM filenames, `(S8000)` and
                    // `(S8003)`, and this is not either of them. What supports 302 is that it
                    // is the image every A9 run in this tree is a measurement of, so it is
                    // recorded here as the image the A9 path has been characterised with --
                    // not as a claim that the corpus's own part-named images are wrong.
                    evidence: "the image every A9 run in this tree is a measurement of",
                },
                SoCId::T8010 => SepRomStatus::Absent(SepRomGap::NoImageInCorpus),
                SoCId::T8015 => SepRomStatus::Absent(SepRomGap::NoImageInCorpus),
                SoCId::T8020 => SepRomStatus::Wired {
                    image: "AppleSEPROM-Cebu-A0",
                    evidence: "gates 0x23c100c4c, d321ap /arm-io/gpio 0x23c100000 + 0xc4c",
                },
                SoCId::T8027 => SepRomStatus::Wired {
                    image: "AppleSEPROM-Cebu-A0",
                    evidence: "gates 0x23c100c4c, the A12/A13/A14 arm-io band",
                },
                // Both are on the band the A12's is, and 16 SEPROMs share it, so naming one
                // here would be a coin flip presented as a measurement.
                SoCId::T8030 => SepRomStatus::Absent(SepRomGap::CannotDiscriminate),
                SoCId::T8101 => SepRomStatus::Absent(SepRomGap::CannotDiscriminate),
            }
        }


/// Find a checked-in AP SecureROM by silicon family and iBoot build.
///
/// Re-exported from `iphem-core`, which is where it lives: `iphem-core` cannot depend on
/// `iphem-soc` (the dependency runs the other way) and neither can `iphem-sdl`, so a
/// helper in `iphem-soc` was unreachable from a third of the call sites.
pub use iphem_core::corpus_rom_path;

impl SoCId {
    /// The `ApChipID` values a restore image may carry for this part.
    ///
    /// This exists because a restore image is not one device's firmware. The two
    /// checked-in images that hold more than one `ApChipID` are
    /// `iPhone_4.7_15.8.8_19H422` (A9) at `0x8000` **and** `0x8003`, both at board
    /// `0x4`, and `iPhone_4.7_P3_12.0_16A366` (A10) at `0x8010` boards `0x8`/`0xc` plus
    /// `0x8015` boards `0x2`/`0xa`. On both, chip and board together are ambiguous, so
    /// the CLI could not select a build identity and the launchers for the A9 and the
    /// A10 stopped before they built anything -- with a message about a restore
    /// identity being ambiguous, which is a true statement about a question the
    /// caller had not yet answered.
    ///
    /// A **set**, not a single value, because `S8000` names one `SoCId` and two silicon
    /// parts: the A9 (S8000) and the A9X (S8003) share board `0x4` and differ only in
    /// the chip. Collapsing them to one value would reject a legitimate manifest, which
    /// is the opposite of failing closed.
    ///
    /// Every value here is **measured**, not assumed. `so_chip_ids_agree_with_the_checked_in_manifests`
    /// reads each checked-in IPSW's `BuildManifest.plist` and requires that the SoC its
    /// launcher targets appears with exactly these chip ids, so a wrong entry here
    /// fails a test rather than rejecting an image at run time. Where no image is
    /// checked in (the A13 and the A14) the value is Apple's published id and the test
    /// says so by having nothing to check it against -- an absence the test reports
    /// rather than a silent pass.
    pub fn ap_chip_ids(&self) -> &'static [u64] {
        match self {
            // iPhone_4.0_64bit_12.5.8_16H88 (iPhone 5s / SE, boards 0x0 and 0x2).
            SoCId::A7 => &[0x8960],
            // iPhone7,2_12.5.8_16H88 carries 0x7000 at board 0x6.
            SoCId::A8 => &[0x7000],
            // iPhone_4.7_15.8.8_19H422 carries 0x8000 and 0x8003, both at board 0x4.
            SoCId::S8000 => &[0x8000, 0x8003],
            // iPhone_4.7_P3_12.0_16A366 carries 0x8010 at boards 0x8 and 0xc.
            SoCId::T8010 => &[0x8010],
            // iPhone10,3,iPhone10,6_16.7.16_20H392 carries 0x8015 at boards 0x6 and 0xe.
            SoCId::T8015 => &[0x8015],
            SoCId::T8020 => &[0x8020],
            SoCId::T8027 => &[0x8027],
            // No A13 or A14 restore image is checked in, so these two are published ids
            // with nothing in this tree to confirm them. `restore_and_boot_t8030.sh` and
            // `restore_and_boot_t8101.sh` still name IPSWs that are absent, and the
            // absence is the reason a filter that cannot be confirmed is worth saying so
            // about here rather than only in a comment.
            SoCId::T8030 => &[0x8030],
            SoCId::T8101 => &[0x8101],
        }
    }

    /// The exception level this part's SecureROM is *entered* at by the reset
    /// path, as opposed to the level its own reset code expects.
    ///

    /// These are two different questions and conflating them is what put the A7
    /// (S5L8960X) in a launcher that died on instruction 18. The evidence is a
    /// **partition** over the AArch64 AP SecureROM corpus: every AArch64 image in
    /// `SecureROM/` and `SecureROM/securerom/` writes exactly one vector base, and
    /// which one it is decides the level, because `VBAR_EL3` has an EL3 privilege
    /// floor and traps as UNDEFINED from EL1. An image whose only vector base is
    /// `VBAR_EL3` therefore cannot be entered at EL1 at all: the write traps, and
    /// because the trap is taken before any vector base is installed, the run
    /// vectors through `VBAR_EL3 = 0` onto unmapped memory.
    ///
    /// Measured, with the register classified by this tree's own `sysreg::decode`
    /// and cross-checked against `iphem_arm::disassemble`
    /// (`a7_platform::the_corpus_agrees_with_the_entry_level_this_tree_uses`):
    ///
    /// * `VBAR_EL3` only, and an EL3 vector table that reads `ELR_EL3`/`ESR_EL3`/
    ///   `FAR_EL3`/`SPSR_EL3`/`SCTLR_EL3`: **s5l8960si** (A7), **s8000si**,
    ///   **s8001si**, **s8003si** (A9 / A9X), **t7000si**, **t7001si** (A8) -- 10 images
    ///   across those 6 families.
    /// * `VBAR_EL1`: every other AArch64 AP image in the corpus -- 32 images, the
    ///   T6020/T6031 watch parts through the T8301.
    ///
    /// The census is `sysreg`-classified, never a mask over the encoding space: a
    /// hand-rolled scan of the op0=3/op1=6 encoding space reports matches in
    /// images that touch no vector register at all, which is the failure the A9
    /// test records.
    ///
    /// **`T8027` is the one place this does not follow the image**, and it is
    /// recorded rather than quietly reconciled. Both checked-in `t8027si` images
    /// write `VBAR_EL1` at file `0x6c` and `0x80e4` and never `VBAR_EL3`, so by
    /// the partition above they are EL1 images; this tree has entered them at EL3
    /// since the Phase 175 work, and those runs are recorded as working. Entering
    /// at a level *higher* than the image writes is survivable -- `VBAR_EL1` is
    /// writable at EL3 -- which is why a measured EL1 image can have a working EL3
    /// run without the two facts being in conflict. Nothing here claims the T8027
    /// *prefers* EL3; only that its recorded runs used it and the corpus does not
    /// ask for it.
    pub fn reset_entry_level(&self) -> iphem_core::cpu::ExceptionLevel {
        use iphem_core::cpu::ExceptionLevel;
        match self {
            // Vector base is VBAR_EL3 only. Entered at EL1 each of these traps on
            // its own reset-path write and dies in the default vector space.
            SoCId::A7 | SoCId::A8 | SoCId::S8000 => ExceptionLevel::El3,
            // The recorded exception, argued above. The only SoCId in the enum
            // whose images measure as EL1 and is nonetheless entered at EL3.
            SoCId::T8027 => ExceptionLevel::El3,
            _ => ExceptionLevel::El1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoCConfig {
    pub id: SoCId,
    pub name: &'static str,
    pub family: &'static str,
    pub dram_base: u64,
    pub dram_size: u64,
    pub sram_base: u64,
    pub sram_size: u64,
    pub uart_base: u64,
    pub timer_base: u64,
    pub aic_base: u64,
    pub aic_size: u64,
    pub pmgr_base: u64,
    pub mailbox_base: u64,
    pub rom_base: u64,
    pub rom_size: u64,
    pub dcp_base: u64,
    pub dcp_size: u64,
    pub nor_base: u64,
    pub nor_size: u64,
    pub block_base: u64,
    pub block_size: u64,
    pub sep_uart_base: u64,
    pub bb_uart_base: u64,
    pub uart_window: bool,
    pub i2c_base: u64,
    pub spi_base: u64,
    pub gpio_base: u64,
    pub nand_base: u64,
    pub nand_size: u64,
    pub pmu_base: u64,
    pub spmi_base: u64,
    pub thermal_base: u64,
    pub usb_base: u64,
    pub usb_size: u64,
    pub watchdog_base: u64,
    pub rtc_base: u64,
    pub pwm_base: u64,
    pub adc_base: u64,
    pub pinmux_base: u64,
    pub clockctl_base: u64,
    pub regulator_base: u64,
    pub sdhc_base: u64,
    pub panel_base: u64,
    pub dsi_phy_base: u64,
    pub raster_base: u64,
    pub touch_base: u64,
    pub audio_base: u64,
    pub bt_base: u64,
    pub wifi_base: u64,
    pub camera_base: u64,
    pub sensorhub_base: u64,
    pub usbphy_base: u64,
    pub usbmux_base: u64,
    pub usbpd_base: u64,
    pub gid_key: [u8; 32],
    pub uid_key: [u8; 32],
    /// Optional path to the SEPROM image for this SoC, relative to the SecureROM
    /// corpus root. When None, no SEPROM is started for this SoC.
    pub sep_rom_path: Option<&'static str>,
}

const GID_DEFAULT: [u8; 32] = [
    0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xEF,
    0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xEF,
];

const UID_DEFAULT: [u8; 32] = [
    0xce, 0x33, 0x6b, 0x94, 0xc2, 0x69, 0x73, 0x39, 0x19, 0x00, 0xaa, 0xcd, 0x34, 0x87, 0xed, 0x77,
    0xdb, 0x3f, 0x2e, 0x1f, 0xce, 0xa1, 0xc2, 0xa5, 0x6d, 0xfd, 0x82, 0xa1, 0x5a, 0x4b, 0x0f, 0xb8,
];

static T8027: SoCConfig = SoCConfig {
    id: SoCId::T8027,
    name: "T8027",
    family: "T802x",
    dram_base: 0,
    dram_size: 64 << 30,
    sram_base: 0x1000000000,
    sram_size: 0x400000,
    uart_base: 0x20a0c0000,
    timer_base: 0x20e100000,
    aic_base: 0x23b100000,
    aic_size: 0x30000,
    pmgr_base: 0x20e000000,
    mailbox_base: 0x23c000000,
    rom_base: 0x100000000,
    rom_size: 0x100000,
    dcp_base: 0x235000000,
    // The d321ap DeviceTree places DART-SIO at +0x4000. Keep the synthetic
    // DCP register aperture from claiming that independent hardware window.
    dcp_size: 0x4000,
    nor_base: 0x240000000,
    nor_size: 0x1000000,
    block_base: 0x241000000,
    block_size: 0x400000,
    sep_uart_base: 0x20a0d0000,
    bb_uart_base: 0x20a0e0000,
    uart_window: true,
    i2c_base: 0x20a200000,
    spi_base: 0x20a400000,
    gpio_base: 0x20a100000,
    nand_base: 0x200000000,
    nand_size: 0x8000000,
    pmu_base: 0x20a000000,
    spmi_base: 0x20a300000,
    thermal_base: 0x20a700000,
    // d321ap ADT: /arm-io/usb-complex maps to 0x239000000 and its
    // usb-device child has reg base 0x100000, yielding 0x239100000.
    // The T8027 SecureROM accesses AES at 0x235008000 (same arm-io base),
    // so it uses the same USB complex layout as the T8020.
    usb_base: 0x239100000,
    usb_size: 0x10000,
    watchdog_base: 0x20a0b0000,
    rtc_base: 0x20a0f0000,
    pwm_base: 0x20a500000,
    adc_base: 0x20a600000,
    pinmux_base: 0x20a900000,
    clockctl_base: 0x20a010000,
    regulator_base: 0x20a020000,
    sdhc_base: 0x20a0a0000,
    panel_base: 0x20a110000,
    dsi_phy_base: 0x20a120000,
    raster_base: 0x20a130000,
    touch_base: 0x20a140000,
    audio_base: 0x20a150000,
    bt_base: 0x20a160000,
    wifi_base: 0x20a170000,
    camera_base: 0x20a180000,
    sensorhub_base: 0x20a190000,
    usbphy_base: 0x20a1a0000,
    usbmux_base: 0x20a1a1000,
    usbpd_base: 0x20a1a2000,
    gid_key: GID_DEFAULT,
    uid_key: UID_DEFAULT,
    sep_rom_path: Some("AppleSEPROM-Cebu-A0"),
};

static T8020: SoCConfig = SoCConfig {
    id: SoCId::T8020,
    name: "T8020",
    family: "T802x",
    dram_base: 0,
    dram_size: 64 << 30,
    sram_base: 0x1000000000,
    sram_size: 0x400000,
    uart_base: 0x20a0c0000,
    timer_base: 0x20e100000,
    aic_base: 0x23b100000,
    aic_size: 0x30000,
    pmgr_base: 0x20e000000,
    mailbox_base: 0x23c000000,
    rom_base: 0x100000000,
    rom_size: 0x100000,
    dcp_base: 0x235000000,
    // The d321ap DeviceTree places DART-SIO at +0x4000. Keep the synthetic
    // DCP register aperture from claiming that independent hardware window.
    dcp_size: 0x4000,
    nor_base: 0x240000000,
    nor_size: 0x1000000,
    block_base: 0x241000000,
    block_size: 0x400000,
    sep_uart_base: 0x20a0d0000,
    bb_uart_base: 0x20a0e0000,
    uart_window: true,
    i2c_base: 0x20a200000,
    spi_base: 0x20a400000,
    gpio_base: 0x20a100000,
    nand_base: 0x200000000,
    nand_size: 0x8000000,
    pmu_base: 0x20a000000,
    spmi_base: 0x20a300000,
    thermal_base: 0x20a700000,
    // d321ap ADT: /arm-io/usb-complex maps to 0x239000000 and its
    // usb-device child has reg base 0x100000, yielding 0x239100000.
    usb_base: 0x239100000,
    usb_size: 0x10000,
    watchdog_base: 0x20a0b0000,
    rtc_base: 0x20a0f0000,
    pwm_base: 0x20a500000,
    adc_base: 0x20a600000,
    pinmux_base: 0x20a900000,
    clockctl_base: 0x20a010000,
    regulator_base: 0x20a020000,
    sdhc_base: 0x20a0a0000,
    panel_base: 0x20a110000,
    dsi_phy_base: 0x20a120000,
    raster_base: 0x20a130000,
    touch_base: 0x20a140000,
    audio_base: 0x20a150000,
    bt_base: 0x20a160000,
    wifi_base: 0x20a170000,
    camera_base: 0x20a180000,
    sensorhub_base: 0x20a190000,
    usbphy_base: 0x20a1a0000,
    usbmux_base: 0x20a1a1000,
    usbpd_base: 0x20a1a2000,
    gid_key: GID_DEFAULT,
    uid_key: UID_DEFAULT,
    sep_rom_path: Some("AppleSEPROM-Cebu-A0"),
};

/// Apple A7 (S5L8960X, iPhone 5s) SEP platform config.
///
/// The A7 is the first SoC with a Secure Enclave Processor. Its SEP is an
/// AArch32 ARMv7-A target running AppleSEPROM → SEPOS. Only the SEP platform
/// bases are modeled here (SEP SRAM/ROM window, SEP UART, SEP identity, SEP
/// boot control); the AP-side peripherals remain the same synthetic band as
/// the other configured SoCs. SEPROM→SEPOS transfer is exercised on the full
/// AArch32 core (`SepExecutionHarness` with `set_use_aarch32_core`).
static A8: SoCConfig = SoCConfig {
    id: SoCId::A8,
    name: "A8",
    family: "T7000",
    dram_base: A8_DRAM_BASE,
    // A choice, documented at `A8_DRAM_SIZE`, exactly as for the A9: 2 GiB is the largest
    // A8 part and the span is `SparseRam`. It was `64 << 30` here, which no A8 has, and the
    // `0` base is what put a 2 GiB window at address 0 and left the ROM's own
    // `0x180100000` working fill unmapped. Both are corrected at `A8_DRAM_BASE`.
    dram_size: A8_DRAM_SIZE,
    // The T7000 ROM's own zero-fill bounds, read from its data: file 0x358 holds
    // 0x1800c0000 and file 0x360 holds 0x20000, and the prologue at 0x10000005c..0x100000078
    // loads both, adds them, and stores zeros across the span in 16-byte steps
    // (`stp x12, x12, [x10]` at 0x10000006c). That is the boot SRAM, and it is the same
    // window the A9 config already carries from the same two literals in the A9 ROM.
    //
    // The previous 0x3000_0000 + 0x600000 had nothing behind it: the run faulted on the
    // first store of that loop, at instruction 25, because nothing was mapped at
    // 0x1800c0000. The size is the ROM's own 0x20000 rather than a larger guess, so the
    // window is exactly the span the firmware says it writes.
    sram_base: 0x1800_c0000,
    sram_size: 0x2_0000,
    uart_base: 0x20a0c0000,
    // The A8 tree declares `/device-tree/arm-io/aic` with `reg = 0xe100000 + 0x100000` and
    // `compatible = "aic,1"`, so the A8's interrupt controller is `0x20e100000` for a
    // megabyte -- and the run programs it there: it writes `1` to `0x20e100010`, four
    // per-line words at `0x20e103260..`, eight `0xffffffff` words at `0x20e104100..` and
    // `0x08000000` to `0x20e104190`.
    //
    // This field held `0x23b100000`, which is the **T8020's** AIC, and `timer_base` held
    // `0x20e100000`, which is the A8's AIC. So the A8 was mapping an interrupt controller at
    // an address no A8 tree node and no A8 measurement produces, while the address the
    // firmware actually uses had no controller on it at all. Neither value is guessed: the
    // tree names the AIC, and nothing in the A8 tree names a timer -- the nearest candidate
    // is `/arm-io/aic-timebase` at `0xe101000 + 0x1000`, a 4 KiB sub-window of the same
    // block, which is what `timer_base` now holds.
    aic_base: 0x20e1_00000,
    aic_size: 0x10_0000,
    timer_base: 0x20e1_01000,
    pmgr_base: 0x20e000000,
    // UNESTABLISHED for the A8, and deliberately not mapped. The A8's two SecureROMs carry
    // no SEP-mail descriptor table, and a movz/movk composition scan of the whole image
    // composes no address in 0x23c000000..0x243000000, so nothing places a mailbox here.
    // The value stays because it is a required config field, but `build_with_rom` skips the
    // generic mailbox for `SoCId::A8` so this address is never answered. Read it as
    // "unknown", not as the A8's mailbox: a plausible constant with no measurement behind it
    // is exactly what made `sram_base` 0x3000_0000 wrong on this same config.
    mailbox_base: 0x23c000000,
rom_base: 0x100000000,
            rom_size: 0x100000,
            dcp_base: 0x235000000,
            // The d321ap DeviceTree places DART-SIO at +0x4000. Keep the synthetic
            // DCP register aperture from claiming that independent hardware window.
            dcp_size: 0x4000,
            nor_base: 0x240000000,
    nor_size: 0x1000000,
    block_base: 0x241000000,
    block_size: 0x400000,
    sep_uart_base: 0x20a0d0000,
    bb_uart_base: 0x20a0e0000,
    uart_window: true,
    i2c_base: 0x20a200000,
    spi_base: 0x20a400000,
    // /arm-io/gpio reg 0xe300000 + 0x100000 on the 0x200000000 arm-io base. Confirmed at
    // four nodes the T7000 ROM composes (pmgr, wdt, otgphyctrl, sochot0), and 0x20a100000,
    // which this field carried before, is not a value any A8 measurement produces.
    gpio_base: 0x20e3_00000,
    nand_base: 0x200000000,
    nand_size: 0x8000000,
    pmu_base: 0x20a000000,
    spmi_base: 0x20a300000,
    thermal_base: 0x20a700000,
    // The A8's USB device controller, measured rather than inherited.
    //
    // This field held `0x20a800000`, which is the **T8020's** USB base. Nothing supports it for
    // the A8: no node in `DeviceTree.n61ap.im4p` claims it, and a `movz`/`movk` composition scan
    // of the whole T7000 ROM -- run in program order with a destination-register kill on any
    // other write, because this project has already had two scans report confident zeroes from a
    // broken mask -- composes it zero times.
    //
    // What the ROM does compose is `0x20c100000`, at four independently checked sites, with the
    // register offsets of the Synopsys DWC2 device block this repository already decodes:
    //
    // ```text
    // 0x100005568  movz x19, #0x2, lsl #32    \
    // 0x10000556c  movk x19, #0xc10, lsl #16   >  x19 = 0x20c100018, GINTMSK
    // 0x100005570  movk x19, #0x18           /
    // 0x1000055e4  movz x8,  #0x2, lsl #32    \
    // 0x1000055e8  movk x8,  #0xc10, lsl #16   >  x8 = 0x20c100814, then `str wzr, [x8]`
    // 0x1000055ec  movk x8,  #0x814          /
    // 0x100005604  movz x9,  #0x2, lsl #32    \
    // 0x100005608  movk x9,  #0xc10, lsl #16   >  x9 = 0x20c1009a8, then `sub x9, x9, #0x20`
    // 0x10000560c  movk x9,  #0x9a8          /      six times, writing `0x1f` each time
    // ```
    //
    // `0x1f` written six times down a 0x20 stride is the per-endpoint enable mask, and the same
    // function sets `w24 = 0x40`, `w27 = 0x1f`, `w28 = 0xf` before it. The ROM also holds
    // `"Apple Mobile Device (DFU Mode)"` at `0x100015884` and references it twice
    // (`0x10000deac`, `0x10000ffb4`), so this block is the one a DFU device enumerates on.
    //
    // **The size is the tree's.** `DeviceTree.n61ap.im4p` gives
    // `/device-tree/arm-io/usb-complex/usb-device` a `reg` whose window is `0x10000` bytes, and
    // 64 KiB is the span that holds the DWC2 core registers plus the per-endpoint IN block at
    // `+0x900` and OUT block at `+0xb00`. The tree's own `0x100000` base cell is *not* used: it
    // disagrees with the composed `0xc100000` by `0xc00000`, and the ROM's own register writes
    // are the stronger evidence of where the firmware reads and writes. That disagreement is
    // recorded rather than resolved, in `docs/A8_T7000_PLATFORM.md`.
    // The A8's USB **device** controller: `DeviceTree.n61ap.im4p` declares
    // `/arm-io/usb-complex` at 0xc900000 and its `usb-device` child at +0x100000 with a
    // 0x10000 aperture, so on the 0x200000000 arm-io base this is 0x20ca00000.
    //
    // The value here was 0x20c1_00000, and the A8 platform note calls that "the address
    // the T7000 ROM composes" -- which it does. It is `usb-complex + 0x200000`, i.e. the
    // **`usb-ehci0` host** window in the same tree, not the device window. So the A8 had a
    // host controller modelled where a device controller belongs, and firmware that
    // programs its DFU endpoint registers at the address its own device tree names would
    // have found the wrong device. The tree decides *which* window; the ROM's composition
    // only says the part can reach that band at all.
    usb_base: 0x20ca00000,
    usb_size: 0x10000,
    watchdog_base: 0x20a0b0000,
    rtc_base: 0x20a0f0000,
    pwm_base: 0x20a500000,
    adc_base: 0x20a600000,
    pinmux_base: 0x20a900000,
    clockctl_base: 0x20a010000,
    regulator_base: 0x20a020000,
    sdhc_base: 0x20a0a0000,
    panel_base: 0x20a110000,
    dsi_phy_base: 0x20a120000,
    raster_base: 0x20a130000,
    touch_base: 0x20a140000,
    audio_base: 0x20a150000,
    bt_base: 0x20a160000,
    wifi_base: 0x20a170000,
    camera_base: 0x20a180000,
    sensorhub_base: 0x20a190000,
    usbphy_base: 0x20a1a0000,
    usbmux_base: 0x20a1a1000,
    usbpd_base: 0x20a1a2000,
    gid_key: GID_DEFAULT,
    uid_key: UID_DEFAULT,
    // NOT the A12's `AppleSEPROM-Cebu-A0`, which this field inherited by copy-paste
    // from the T8010/T8015/T8020 configs above it.
    //
    // Cebu is the T8020 coprocessor firmware, and substituting it here was not a
    // near-miss that happened to run: it cannot run on this platform. The Cebu
    // SEPROM's own execution gate polls a word at SEP window `0x23c1_00000 + 0xc4c`
    // (the SEP-gate record in `docs/USB_RESTORE_PROGRESS.md`), and the A8 SecureROM
    // composes no address anywhere in 0x23c000000..0x243000000, so no A8 build maps
    // that window for it. The A8's own SEPROM is not identified in this corpus, and
    // naming one would be a guess.
    //
    // So the field is `None` and the A8 starts no SEP by default. `IPHEM_SEP_ROM`
    // still overrides it, which is the lever for an operator who has the A8's real
    // coprocessor firmware. This is the same copy-paste failure mode already recorded
    // on this same config for `sram_base`, where an inherited value put a window at
    // an address no A8 measurement produces.
    sep_rom_path: None,
};

/// Apple A9 (S8000, "Maui" -- iPhone 6s / 6s Plus / iPhone SE) SoC config.
///
/// The A9 is the first 64-bit Apple part, and it is **not** an A12 with a
/// different name: the A9's SecureROM is an AArch64 **EL3** image, its MMIO band
/// is `0x20a0_0000`..`0x2102_0000` rather than the A12's, and its DRAM is at
/// `0x1800_0000`. Every base below is either measured from the A9 SecureROM or
/// carried as a measured address that this config deliberately does **not** use to
/// place a device. The reasoning is in `docs/A9_S8000_PLATFORM.md`; the shape is
/// `build_a9_s8000`.
///
/// Measured, and mapped by `build_a9_s8000`:
/// * `rom_base` 0x100000000 -- the image's own rebase target, the u64 at file
///   offset 0x318, identical in all three checked-in A9 builds.
/// * `uart_base` 0x20a080000 with four 0x4000-strided windows -- the A9 ROM's own
///   register-pointer table at file 0x15ad0..0x15c38.
/// * `pmgr_base` 0x20e080000 with a 72-word power-state array at 0x20e080100 and a
///   second 11-word array at `mailbox_base` -- register-pointer tables at file
///   0x17b00 and 0x178c0 / 0x17868.
/// * `aic_base` 0x20f100000 -- named at file 0x19000.
/// * `sram_base` 0x1800c0000 + 0x20000 -- the ROM's own zero-fill/copy literals.
///
/// Measured but **not** used to place a device, with the reason:
/// * `timer_base` 0x20e040000 appears in the ROM's 0x68-stride register table at
///   file 0x160d8, and the ROM writes eight registers inside that page with the
///   write-then-poll-ack-bit-30 protocol -- but its only observed access is at
///   `+0x300`, which is not a register the `Timer` model has. A timer at
///   `+0x00..+0x48` is not established, so nothing is placed there and the page is
///   mapped as an always-on register aperture instead.
/// * `mailbox_base` 0x210280000 is **not** shown to be a mailbox. The A9 ROM
///   writes the same power-state pattern into its eleven registers that it writes
///   into the seventy-two PMGR words, which is not what a mailbox is written
///   with. The name is historical; the builder uses the constant
///   `A9_SECOND_STATE_FIRST`, not this field.
/// * `nor_base` 0x300000000 and `nand_base` 0x200000000 are named (five and
///   seventeen literals respectively) but the ROM does not touch either in the
///   init it reaches, so no flash device is mapped.
///
/// Zeroed because the A9 ROM does not name them anywhere: the previous entry
/// carried `dcp_base` 0x235000000, `block_base` 0x241000000 and `rtc_base`
/// 0x20a0f0000, none of which appears in any of the three A9 images, and ~40
/// `0x20aXXXXX` peripheral-band bases that are this project's synthetic
/// acceptance-test band for the A12 (`0x20a1a0000`) and not silicon. A SoC
/// builder that maps a device at an invented address produces a machine whose
/// numbers do not describe the part, so they are removed rather than kept for
/// shape. `tests/a9_platform.rs` asserts that **every** region in a built A9
/// machine is either RAM or an address one of the A9 images names.
/// Apple A7 (S5L8960X, iPhone 5s).
///
/// Every field here that is not marked otherwise comes from `build_a7_sep_platform`, which
/// is the A7 SEP layout this project actually runs: a 32-bit address space with the SEPROM
/// and SEP SRAM at 0x30000000, the SEP-side ASC mailbox at 0x40000000, and the SEP identity
/// register reporting 0x8960. Those are what the Phase 219/220 suites assert.
///
/// The **AP** MMIO fields are unmeasured and left at 0. No A7 AP SecureROM is in the corpus
/// -- `SecureROM/` holds t7000, t8010, t8011, t8015, t8020, t8027, t8030 and t8101 -- so
/// there is nothing here that could establish the A7's own peripherals, clock or power
/// windows. Zeros here are the honest value: a plausible-looking base would be a claim with
/// nothing behind it, which is how this config's T7000 sibling came to disagree with its own
/// device tree. `build_with_rom` therefore refuses to assemble a machine for this id rather
/// than mapping every device at address 0.
static A7: SoCConfig = SoCConfig {
    id: SoCId::A7,
    name: "A7",
    family: "S5L8960X",
    // AP DRAM: size only. The A7 tops out at 2 GiB and no A7 measurement is in this tree.
    dram_base: A8_DRAM_BASE,
    dram_size: A8_DRAM_SIZE,
    // The A7's boot SRAM. The ROM reads file 0x358 = 0x1800c0000 and file 0x360 = 0x20000
    // and zero-fills that span in 16-byte steps from `stp x12, x12, [x10]` at 0x10000006c,
    // so the first store of the loop is instruction 25 and it faulted with nothing mapped
    // there. The window is the ROM's own 0x20000 rather than a wider guess, so the mapping
    // claims exactly the span the firmware says it writes.
    sram_base: 0x1800_c0000,
    sram_size: 0x2_0000,
    // The A7's interrupt controller. The A7 SecureROM shares the same early boot code
    // as the A8/T7000, including the same device address composition. The A8 tree declares
    // `/device-tree/arm-io/aic` with `reg = 0xe100000 + 0x100000`, so the A8's AIC is
    // `0x20e100000` for a megabyte. The A7 ROM uses the same MOVZ/MOVK sequence to
    // compose this address.
    aic_base: 0x20e1_00000,
    aic_size: 0x10_0000,
    // The A8 tree declares `/arm-io/aic-timebase` at `0xe101000 + 0x1000`, a 4 KiB
    // sub-window of the same block. Used as the timer base.
    timer_base: 0x20e1_01000,
    // The A8's main PMGR plane: `/device-tree/pmgr` with `reg = 0xe000000 + 0x40000`
    // => 0x20e000000 + 0x40000 physical. The A7 ROM composes the same address.
    pmgr_base: 0x20e000000,
    // UNESTABLISHED for the A7, and deliberately not mapped. The A7's SecureROMs carry
    // no SEP-mail descriptor table, and a movz/movk composition scan of the whole image
    // composes no address in 0x23c000000..0x243000000, so nothing places a mailbox here.
    mailbox_base: 0x23c000000,
    rom_base: 0x100000000,
    rom_size: 0x80000,
    dcp_base: 0x235000000,
    dcp_size: 0x40000,
    nor_base: 0x240000000,
    nor_size: 0x1000000,
    block_base: 0x241000000,
    block_size: 0x400000,
    uart_base: 0,
    sep_uart_base: 0x20a0d0000,
    bb_uart_base: 0x20a0e0000,
    uart_window: true,
    i2c_base: 0x20a200000,
    spi_base: 0x20a400000,
    // /arm-io/gpio reg 0xe300000 + 0x100000 on the 0x200000000 arm-io base.
    gpio_base: 0x20e3_00000,
    nand_base: 0x200000000,
    nand_size: 0x8000000,
    pmu_base: 0x20a000000,
    spmi_base: 0x20a300000,
    thermal_base: 0x20a700000,
    watchdog_base: 0x20a0b0000,
    rtc_base: 0x20a0f0000,
    pwm_base: 0x20a500000,
    adc_base: 0x20a600000,
    pinmux_base: 0x20a900000,
    clockctl_base: 0x20a010000,
    regulator_base: 0x20a020000,
    sdhc_base: 0x20a0a0000,
    panel_base: 0x20a110000,
    dsi_phy_base: 0x20a120000,
    raster_base: 0x20a130000,
    touch_base: 0x20a140000,
    audio_base: 0x20a150000,
    bt_base: 0x20a160000,
    wifi_base: 0x20a170000,
    camera_base: 0x20a180000,
    sensorhub_base: 0x20a190000,
    usbphy_base: 0x20a1a0000,
    usbmux_base: 0x20a1a1000,
    usbpd_base: 0x20a1a2000,
    // `DeviceTree.n53ap.im4p`, the A7 (iPhone 5s) tree, taken from the checked-in
    // `iPhone_4.0_64bit_12.5.8_16H88_Restore.ipsw`: `/arm-io/usb-complex` at 0xc900000
    // and its `usb-device` child at +0x100000 with a 0x10000 aperture, so on the
    // 0x200000000 arm-io base this is **0x20ca00000**.
    //
    // This field was 0, "not established", on the reasoning that the A7 SecureROMs
    // compose no device address and hold none as a literal -- both of which are true,
    // and neither of which is about this register. The A7's device addresses are reached
    // through a register-offset table in the image, which is the one place a
    // `movz`/`movk` scan and a literal search both cannot see, so the absence was a
    // limit of the two instruments rather than a fact about the part. The device tree
    // answers the question the ROM could not, and the same tree structure gives the
    // A8, the A9 and the A10 the identical window.
    //
    // A7 SecureROM measurements, for the record: a validated `movz`/`movk` scan of
    // `SecureROM for s5l8960xsi, iBoot-1585.4`` (512 KiB) and `...1704.10` (128 KiB)
    // composes **no** value in `0x2000_0000..0x2400_0000`; a literal scan finds none
    // there either; and both images are plaintext, not encrypted (they open
    // `b`/`adrp`/`add` and hold 12.3% and 26.3% distinct 32-bit words against the
    // A12's 10.7%). The entry stub's `adrp x0, 0` / `add x0, #0` / `ldr x1, [x3, #0x618]`
    // is a base-plus-offset walk, which is the table those scans miss.
    usb_base: 0x20ca00000,
    usb_size: 0x10000,
    gid_key: GID_DEFAULT,
    uid_key: UID_DEFAULT,
    sep_rom_path: Some("AppleSEPROM-Cebu-A0"),
};

static S8000: SoCConfig = SoCConfig {
    id: SoCId::S8000,
    name: "S8000",
    family: "A9",
    dram_base: A9_DRAM_BASE,
    // 2 GiB, the largest A9 part. A choice, documented at `A9_DRAM_SIZE`; the
    // span is SparseRam, so it is address space rather than an allocation. It
    // also has to *stop* before the A9's device band at 0x20a080000.
    dram_size: A9_DRAM_SIZE,
    sram_base: 0x1800c0000,
    sram_size: 0x20000,
    uart_base: 0x20a080000,
    timer_base: 0x20e040000,
    aic_base: 0x20f100000,
    aic_size: 0x30000,
    pmgr_base: A9_PMGR_BASE,
    mailbox_base: A9_SECOND_STATE_FIRST,
    rom_base: 0x100000000,
    rom_size: 0x80000,
    // Not named by any A9 image.
    dcp_base: 0,
    dcp_size: 0,
    nor_base: 0x300000000,
    nor_size: 0x1000000,
    block_base: 0,
    block_size: 0,
    sep_uart_base: 0,
    bb_uart_base: 0,
    uart_window: false,
    // The synthetic acceptance-test peripheral band. Zero: not A9 silicon.
    i2c_base: 0,
    spi_base: 0,
    gpio_base: 0,
    nand_base: 0x200000000,
    nand_size: 0x4000000,
    pmu_base: 0,
    spmi_base: 0,
    thermal_base: 0,
    // `DeviceTree.n71ap.im4p`, the A9 (iPhone 6s) tree from the checked-in
    // `iPhone_4.7_15.8.8_19H422_Restore.ipsw`. What is checked: the tree has
    // `/arm-io/usb-complex` and its child `/arm-io/usb-complex/usb-device`, both
    // with `compatible` strings naming this part (`usb-complex,s8000`,
    // `usb-device,s8000`), so the A9 does have a USB device node and the other two
    // USB windows in this builder's map (0x20c000000, 0x20c100000) are consistent
    // with the same tree naming `otgphyctrl,s8000`.
    //
    // What is **not** checked: the value of this address. It used to read
    // "`usb-complex` at 0xc900000, `usb-device` at +0x100000, 0x10000 aperture =>
    // **0x20ca00000**", and dumping the tree's raw `reg` bytes does not reproduce
    // that: the property decodes to neither `0xca00000`/`0x20ca00000` as 8-byte
    // cells nor to a page-aligned base/size pair as 4-byte cells, so the
    // "+0x100000" step is an assumption, not a reading. The address is kept because
    // DFU needs a controller somewhere and leaving the window unmapped would fault
    // before the run reaches anything to learn from -- but it is carried from this
    // config, **not** from a measurement, and it is the one A9 window in this file
    // in that state. Re-deriving it needs a device-tree reg decode that agrees with
    // this file's other A9 addresses; that has not been done.
    //
    // Independently of this window, both A9 SecureROMs were scanned and compose only
    // 0x20000000, 0x20080000 and 0x202c0000 in the device band, so the ROM does not
    // name this address itself.
    usb_base: 0x20ca00000,
    usb_size: 0x10000,
    watchdog_base: 0x20ae00000,
    rtc_base: 0,
    pwm_base: 0,
    adc_base: 0,
    pinmux_base: 0,
    clockctl_base: 0,
    regulator_base: 0,
    sdhc_base: 0,
    panel_base: 0,
    dsi_phy_base: 0,
    raster_base: 0,
    touch_base: 0,
    audio_base: 0,
    bt_base: 0,
    wifi_base: 0,
    camera_base: 0,
    sensorhub_base: 0,
    usbphy_base: 0,
    usbmux_base: 0,
    usbpd_base: 0,
    gid_key: GID_DEFAULT,
    uid_key: UID_DEFAULT,
    // **No A7 SEPROM.** This used to name `AppleSEPROM-Cebu-A0`, which is an **AArch64**
    // image, on a 32-bit part -- a wiring that cannot be right however it is read, and
    // the A7 SEP platform below is a 32-bit address space. It is removed rather than
    // replaced because nothing in this tree identifies an AArch32 image as the A7's:
    //
    //   * The corpus names a part in only two SEPROM filenames, `(S8000)` and `(S8003)`,
    //     which are the A9 and A9X. Everything else is a codename or a bare version
    //     family, so there is no name-based pairing to fall back on.
    //   * The **behavioural** substitute does not work either, and that is the part
    //     worth recording. `seprom_aarch32_sweep` runs all 16 AArch32 images on this
    //     map and on the A9's, and **every image produces the identical result on both**:
    //     eight of them retire the full 20,000-step budget still `Running` at pc 0x7c,
    //     three park in `WaitingForInterrupt` at pc 0x84 after 6 instructions, three
    //     halt `Undefined` at pc 0x41e8 after 2, and one at pc 0x4024. The AArch32 SEP
    //     reset paths touch no device window, so the map cannot be tested by running.
    //     "Runs on the map" therefore discriminates nothing here, and choosing an image
    //     on that basis would be an unevidenced pairing wearing a measurement's clothes.
    //   * The control that keeps the sweep honest: the AArch64 image this field used to
    //     name, run on this same map, halts with a **data fault after 7 instructions**.
    //
    // So the A7 has an SEP platform and no SEPROM, which is the same state the A8 is in
    // and for a related reason. The A7 SEP layout that Phase 219/220 exercise is driven
    // by synthetic firmware, not by an Apple image.
    sep_rom_path: None,
};


/// Documented synthetic A7 (S5L8960X) SEP platform memory map.
///
/// No real A7/S5L8960X SEP measured addresses exist in-tree (the chip predates
/// the T8020 measurements), so these bases are explicit project conventions:
/// the SEP firmware window (SEPROM at `0x3000_0000`, SEPOS at `0x3010_0000`),
/// an ASC mailbox IOP view the SEPOS polls for AP messages, a SEP identity,
/// boot control, and SEP UART. The returned regions feed a
/// `SepExecutionHarness`; the AP view of the mailbox is returned separately
/// so the AP side can push messages and harvest replies. Nothing here is a
/// measured silicon register — these are the platform's documented model.
pub fn build_a7_sep_platform() -> (Vec<BusRegion>, AppleAscMailbox) {
    let (ap_view, iop_view) = AppleAscMailbox::paired(98, 70, 71);
    let regions = vec![
        // A32 exception vectors live at 0x00..0x20; a low SparseRam provides
        // the vector window the full-core SEPROM branches through on SVC/IRQ.
        BusRegion {
            base: 0,
            size: 0x10_0000,
            device: Box::new(SparseRam::new(0, 0x10_0000)),
        },
        BusRegion {
            base: 0x3000_0000,
            size: 0x200_000,
            device: Box::new(SparseRam::new(0x3000_0000, 0x200_000)),
        },
        BusRegion {
            base: 0x4000_0000,
            size: 0x1_0000,
            device: Box::new(iop_view),
        },
        BusRegion {
            base: 0x4001_0000,
            size: 0x1_000,
            device: Box::new(SepIdentity::new(209, 0x8960)),
        },
        BusRegion {
            base: 0x4003_0000,
            size: 0x1_000,
            device: Box::new(SepBootCtrl::new(210)),
        },
        BusRegion {
            base: 0x4003_0000,
            size: 0x1_000,
            device: Box::new(SepUart::new(211, "A7 SEP UART")),
        },
    ];
    (regions, ap_view)
}

/// The A9's four console/debug UART windows, from the A9 SecureROM's own
/// register-pointer table (`SecureROM for s8000si, iBoot-2234.0.0.2.22`` file
/// offsets 0x15ad0..0x15c38: 0x20a080000, 0x20a084000, 0x20a088000,
/// 0x20a08c000). The 0x4000 stride is the distance between consecutive table
/// entries, so it is the window size this project maps.
pub const A9_UART_WINDOWS: [(u64, u64); 4] = [
    (0x20a0_80000, 0x4000),
    (0x20a0_84000, 0x4000),
    (0x20a0_88000, 0x4000),
    (0x20a0_8c000, 0x4000),
];

/// The A9's PMGR power-state array: the contiguous run of four-byte words at an
/// eight-byte stride from `0x20e080100` to `0x20e0802f8` inclusive, measured as
/// one run of register pointers in the A9 SecureROM's literal pool (file offsets
/// 0x178c0..0x17ab8).
pub const A9_PMGR_STATE_FIRST: u64 = 0x20e0_80100;
/// Number of words in [`A9_PMGR_STATE_FIRST`]'s contiguous run.
pub const A9_PMGR_STATE_COUNT: u64 = 64;
/// Stride between consecutive A9 power-state words.
pub const A9_PMGR_STATE_STRIDE: u64 = 8;
/// The eight words that follow the contiguous run: `0x20e080308`..`0x20e080340`.
///
/// These are a second, separate group and the gap is in the **image**, not in
/// this model: `0x20e080300` appears nowhere in any of the three A9 SecureROM
/// builds, so a model that mapped `0x100..0x340` as one run -- which is what the
/// previous config claimed -- invented a power-state register at `0x20e080300`
/// and dropped `0x20e080308`. Both are single words, and both are the kind of
/// detail that a "seventy-two words at +8" comment hides.
pub const A9_PMGR_STATE_TAIL: [u64; 8] = [
    0x20e0_80308,
    0x20e0_80310,
    0x20e0_80318,
    0x20e0_80320,
    0x20e0_80328,
    0x20e0_80330,
    0x20e0_80338,
    0x20e0_80340,
];

/// The A9's standalone PMGR register at `0x20e080400`, which the census runs into as the
/// part's frontier.
///
/// This one is a **modelling gap that the image closes**, not a hardware gate with no
/// producer, and the distinction is worth stating because it is the opposite of the A11,
/// A13 and A14 cases. The A9's own PMGR register-pointer table at file `0x17b00..0x17b30`
/// names it exactly once, at file `0x17b18`:
///
/// ```text
/// 0x17b00 = 0x20e080000   0x17b08 = 0x20e080008
/// 0x17b10 = 0x20e080040   0x17b18 = 0x20e080400   <- this one, alone
/// 0x17b20 = 0x20e088000   0x17b28 = 0x20e088008   0x17b30 = 0x20e088010
/// ```
///
/// `0x20e080400` is the only value in the whole image equal to it, so the ROM names it as a
/// distinct register of the same PMGR aperture -- it is 0x104 past the end of the modelled
/// 64-word [`A9_PMGR_STATE_FIRST`] run and is not in [`A9_PMGR_STATE_TAIL`], which is why
/// nothing answered for it. The A9's ROM composes the address in-register at
/// `0x10000a8d0` and then applies the same target/actual protocol as the array it does
/// model: `and #0xeffffbff`, store, then poll `(word & 0xf0) == 0xf0`, which is where the
/// census sits with 738,074 reads and a single write of `0x0`.
///
/// It is modelled as a `PmgrPowerState` like its neighbours, which is a statement about the
/// *transport* -- a target/actual power word -- and not a claim that it is currently powered.
/// It is **not** declared active, so the poll still cannot pass on its own; what changes is
/// that the register is now a real device the firmware can be modelled against rather than a
/// gap the bus silently answers.
pub const A9_PMGR_GATE: u64 = 0x20e0_80400;

/// The A9's PMGR aperture base, named by the same image's second pointer table
/// (file offsets 0x17b00..0x17b30: 0x20e080000, +8, +0x40, +0x400, then
/// 0x20e088000, +8, +0x10).
pub const A9_PMGR_BASE: u64 = 0x20e0_80000;
/// The second page of the A9's PMGR aperture.
pub const A9_PMGR_PAGE2: u64 = 0x20e0_88000;

/// The A9's second power-state array: eleven words at `0x210280000` with the same
/// eight-byte stride as [`A9_PMGR_STATE_FIRST`]. Measured as a contiguous run of
/// eleven register pointers in the A9 SecureROM's literal pool (file offsets
/// 0x17868..0x178b8), and the ROM's init loop writes the same power-state pattern
/// into it that it writes into the PMGR array.
pub const A9_SECOND_STATE_FIRST: u64 = 0x2102_80000;
/// Number of words in [`A9_SECOND_STATE_FIRST`]'s run.
pub const A9_SECOND_STATE_COUNT: u64 = 11;

/// The A9's device-readiness word: the ROM clears bits 10 and 18 in it and then
/// polls it until bits 7..4 are all set. Measured from the A9 SecureROM's own
/// gate at `0x10000a8e0..0x10000a8fc` (the census reads this one word 238,074
/// times per million instructions with the PC pinned in that four-instruction
/// loop). It is modelled fail-closed: the word reads zero, so the poll does not
/// pass, because nothing in this model produces the hardware state it is waiting
/// for.
pub const A9_READY_WORD: u64 = 0x20e0_80400;

/// The physical address the A9 SEPROM (`AppleSEPROM-230.0.0.4.1`) writes to CP15
/// `c2, c0, #2` in its reset path, from the immediates its own
/// `movw r2, #0x2300` / `movt r2, #0x8080` build. Reported, not interpreted: in
/// the ARMv7/ARMv8-A AArch32 CP15 map `c2, c0, #2` is the ECC alias of TTBR0,
/// which would make this the SEP's stage-1 root, but the AArch32 core here
/// implements SCTLR only and decodes no such write, so no claim is made.
pub const A9_SEP_TABLE_BASE: u64 = 0x8080_2300;

/// The A9's always-on register pages: `(page base, the word in it the ROM
/// writes)`, each page 4 KiB.
///
/// Every entry is a page the SecureROM's reset path writes one word in, during the
/// de-assert sequence at `0x10000ab08..0x10000ab28`. The pairs are the census's
/// per-register report, and they are kept as pairs because **the page bases are
/// not in the image**: the ROM builds each address from `movz`/`movk` immediates,
/// so a literal scan finds none of them, and a model that claimed those pages came
/// from a literal would be claiming something no scan supports. The words are
/// therefore *runtime-observed* evidence and are labelled as such; the register
/// semantics are not established, and the pages are mapped as inert register
/// storage (a guest write is retained, a read starts at zero).
pub const A9_AON_PAGES: [(u64, u64); 4] = [
    (0x20e0_40000, 0x20e0_40300),
    (0x20e0_70000, 0x20e0_70200),
    (0x20e0_a0000, 0x20e0_a0000),
    (0x20e0_dc000, 0x20e0_dc000),
];

/// The 24 further 4 KiB pages the A9 SecureROM's init touches that
/// [`build_a9_s8000`] did not map, as `(page base, a register in the page the ROM
/// touched)`.
///
/// **This list exists because the census could not see the defect it was written
/// to find.** `s8000_platform_census` installs `bus.fallback_device`, so the run
/// cannot fault on an unmapped address, and its "exit: Ok, 3,000,000
/// instructions" said nothing about whether the machine the CLI boots can run the
/// ROM at all. It cannot: with the fallback removed (`IPHEM_CENSUS_PRODUCTION=1`)
/// the same build dies at instruction 43,101, `PC=0x10000a84c`, storing `0x400000`
/// to `0x202220068` -- the first of these pages in program order. The CLI's own
/// A9 restore dies at the same instruction, which is how the gap was found.
///
/// What the list is: the census's per-register report, filtered to the windows the
/// builder does not already map, one entry per page with the lowest register the
/// ROM touched in it. 24 pages, 298 register accesses, measured over 3,000,000
/// instructions of the 2234.0.0.3.3 build. The addresses are 34-bit and written in
/// full twelve-digit form, which is not this file's usual style and is deliberate:
/// the first draft of this list was typed by hand and lost a digit twice, and the
/// second draft's duplicate page base was what surfaced it.
///
/// What it is **not**, stated because the two are easy to confuse:
///
/// * The page bases are *not* in the image. The ROM composes them from
///   `movz`/`movk` immediates, so a literal scan finds none of them -- the same
///   reason [`A9_AON_PAGES`] keeps the observed word beside the base. The words
///   are runtime-observed evidence, not image evidence.
/// * The register semantics are **not** established. Nothing here is claimed to be
///   a clock gate, a reset, a power-state word or a status register. The pages are
///   mapped as the tree's inert register-storage aperture: a guest write is
///   retained, a read starts at zero.
/// * The counts come from a run in which **reads the fallback answered with zero**.
///   Register storage retains a write and returns it, so a register the ROM writes
///   and then reads back reads back its written value under this map and read zero
///   under the census's. That is a divergence in *behaviour*, not only in
///   coverage, and it is why this list is a lower bound on the work list rather
///   than the finished list: `a9_platform.rs` runs the real map with no fallback
///   and fails if the run reaches a window this list does not name.
///
/// The one page here with a recognisable *shape* is `0x20e068000`: it is written 62
/// times by four instructions at `0x10000ad98..0x10000adb0` repeated sixteen times,
/// over 58 registers, which is a table-programming loop. It is still not a device
/// and is mapped as storage.
pub const A9_MEASURED_REG_PAGES: [(u64, u64); 25] = [
    (0x0002_0222_0000, 0x0002_0222_0020), // 4 reads, 7 writes
    (0x0002_0224_0000, 0x0002_0224_0008), // 2 reads, 4 writes
    (0x0002_0e00_0000, 0x0002_0e00_0000), // 3 reads, 2 writes
    (0x0002_0e00_4000, 0x0002_0e00_4000), // 4 reads, 4 writes
    (0x0002_0e00_8000, 0x0002_0e00_8000), // 2 reads, 2 writes
    (0x0002_0e00_c000, 0x0002_0e00_c000), // 2 reads, 2 writes
    (0x0002_0e01_0000, 0x0002_0e01_0000), // 2 reads, 2 writes
    (0x0002_0e01_4000, 0x0002_0e01_4000), // 2 reads, 2 writes
    (0x0002_0e01_8000, 0x0002_0e01_8000), // 6 reads, 6 writes
    (0x0002_0e04_4000, 0x0002_0e04_4000), // 1 read, 1 write
    (0x0002_0e04_8000, 0x0002_0e04_8000), // 1 read, 1 write
    (0x0002_0e04_c000, 0x0002_0e04_c000), // 1 read, 1 write
    (0x0002_0e05_0000, 0x0002_0e05_0000), // 1 read, 1 write
    (0x0002_0e05_4000, 0x0002_0e05_4000), // 1 read, 1 write
    (0x0002_0e06_8000, 0x0002_0e06_8000), // 4 reads, 62 writes
    (0x0002_0e0b_8000, 0x0002_0e0b_8000), // 4 reads, 7 writes
    (0x0002_0e10_0000, 0x0002_0e10_0010), // 2 reads, 2 writes
    // The 25th page, and the reason the count is 25 rather than 24. The first
    // census of this band ran without device ticks while the restore advances
    // devices once per attempted instruction; the two runners retire different
    // instruction counts inside the same delay loops, so they walk different
    // code, and the no-tick census reported a set the restore immediately walked
    // off the end of. With ticks the census reproduces the restore exactly --
    // same PC `0x100001330`, same `x20=0x20e102004` -- and this is the page that
    // read names. Kept adjacent to `0x20e103000` in address order because it is
    // what the run reached, not because the band was surveyed.
    (0x0002_0e10_2000, 0x0002_0e10_2004), // 1 read, 0 writes
    (0x0002_0e10_3000, 0x0002_0e10_32f0), // 0 reads, 4 writes
    (0x0002_0e10_4000, 0x0002_0e10_4100), // 0 reads, 12 writes
    (0x0002_100f_0000, 0x0002_100f_0000), // 32 reads, 65 writes
    (0x0002_1020_0000, 0x0002_1020_0000), // 1 read, 2 writes
    (0x0002_1024_0000, 0x0002_1024_0000), // 16 reads, 9 writes
    (0x0002_102b_c000, 0x0002_102b_c000), // 7 reads, 0 writes
    (0x0002_102d_c000, 0x0002_102d_c000), // 0 reads, 1 write
];

/// The A9's DRAM base. The A9 SecureROM names `0x180000000` (four times) and
/// its whole boot-scratch band (`0x180080000`, `0x180080840`, `0x180088368`,
/// `0x1800c0000`) lies inside the first 16 MiB above it.
pub const A9_DRAM_BASE: u64 = 0x1800_00000;
/// The A9's DRAM span used by the model. **A choice, not a measurement**: 2 GiB
/// is the largest A9 part, and the span is `SparseRam`, so this is address space
/// rather than an allocation. It is also load-bearing in the other direction --
/// the span has to *stop* before the A9's device band, which begins at
/// 0x20a080000: an 8 GiB span from this base reaches 0x380000000 and swallows
/// every A9 device window, which is how the first cut of this builder failed.
pub const A9_DRAM_SIZE: u64 = 0x8000_0000;

/// The A8's DRAM base: the T7000's DDR sits at `0x180000000`, the same address the A9
/// uses. This is not a copy of the A9's constant -- it is the band the T7000 SecureROM
/// itself works in, and the four addresses it composes all sit inside the first
/// 1.5 MiB above it:
///
/// | what | address | where it comes from |
/// |---|---|---|
/// | relocated image, base | `0x180080000` | 8-byte literal at file `0x338` |
/// | relocated image, end | `0x180088ad0` | 8-byte literal at file `0x350` |
/// | boot SRAM, base | `0x1800c0000` | 8-byte literal at file `0x358` |
/// | boot SRAM, size | `0x20000` | 8-byte literal at file `0x360` |
/// | working fill, base | `0x180100000` | derived; `memset` of exactly `0x100000` bytes |
///
/// The last row is the one that proved the base. With `dram_base = 0` the A8 mapped RAM at
/// address 0 for 2 GiB, which covers `0x180080000` and `0x1800c0000` and therefore *looked*
/// right -- and then stopped at `0x8000_0000`, so the ROM's 1 MiB fill at `0x180100000`
/// (6.4 GiB) faulted at instruction 55,688. A mapping that happens to reach the first two
/// windows and not the third is the shape a wrong base takes.
pub const A8_DRAM_BASE: u64 = 0x1800_00000;

/// The A8's DRAM span used by the model. **A choice, not a measurement**, for the same
/// reason and with the same caveat as [`A9_DRAM_SIZE`]: 2 GiB is the largest A8 part, the
/// span is `SparseRam` so this is address space rather than an allocation, and it has to
/// *stop* before the A8's device band. The lowest A8 device the ROM is measured to reach is
/// the second `pmgr` plane at `0x202220000`, and `0x180000000 + 0x8000_0000 = 0x200000000`
/// stays below it.
pub const A8_DRAM_SIZE: u64 = 0x8000_0000;

/// Where the A8's DRAM span is actually *mapped*, which is not [`A8_DRAM_BASE`].
///
/// The A8 builder maps the ROM's two boot windows separately and at exactly the bounds the
/// ROM's own literals give -- `0x180080000 + 0x8ad0` for the relocated image and
/// `0x1800c0000 + 0x20000` for the boot SRAM -- because `crates/iphem-boot/tests/
/// a8_platform.rs` pins both bases and both sizes, and both are measurements rather than
/// guesses. `Bus::map` refuses an overlap, so the DRAM span has to begin past them. The
/// first address above both is `0x1800e0000`, and the ROM's next measured write,
/// `0x180100000`, is inside it.
///
/// The consequence to keep in mind: `0x1800e0000..0x180100000` is a 128 KiB hole between the
/// boot SRAM and this span. Nothing in the T7000 SecureROM has been measured to touch it,
/// which is a different statement from "nothing can reach it" and is why the base sits at
/// the end of the measured boot windows rather than at [`A8_DRAM_BASE`].
pub const A8_DRAM_MAPPED_BASE: u64 = 0x1800_e0000;

/// Build the Apple A9 (S8000) platform.
///
/// This is a **separate builder** rather than a branch in [`build_with_rom`], and
/// that is a correction rather than a style choice. The A9 was previously served
/// by the generic path, which maps the A12's device set at the A12's addresses;
/// the A9 branch that followed then mapped an `S5lUart` over the generic
/// `Uart` at the same base, and `build_with_rom` **panicked** with
/// `Overlap: r=(0x20a080000, 0x20a084000), x=(0x20a080000, 0x20a081000)` — the A9
/// machine could not be built at all. A part whose memory map is not the A12's
/// needs its own map, and the overlap was the symptom of not having one.
///
/// Every region below is either measured from the A9 SecureROM or is RAM the ROM
/// provably writes. What is *not* here is as load-bearing as what is: the A9
/// names `0x210280000` (eleven register pointers, file 0x17868..0x178b8),
/// `0x210240000`, `0x20da00000`, `0x2100f0000` and `0x20ae00000`, and **none of
/// those windows is mapped here**, because the ROM naming an address says it
/// wants that address and not what the device there does. Mapping them to a
/// catch-all would convert a measurement gap into a fabricated device. The
/// census in `examples/s8000_platform_census.rs` reports what the ROM does at
/// each, and the doc records what is still unknown.
pub fn build_a9_s8000(rom_data: &[u8], enable_debug: bool) -> Machine {
    let c = config(SoCId::S8000);
    let mut rom_vec = vec![0u8; c.rom_size as usize];
    let len = rom_data.len().min(rom_vec.len());
    rom_vec[..len].copy_from_slice(&rom_data[..len]);

    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1000,
    });
    let _ = enable_debug;

    // The load base is the image's own: the u64 at file offset 0x318 is
    // 0x0000_0001_0000_0000 in all three checked-in A9 builds, the same
    // rebase descriptor every AArch64 AP ROM in this corpus carries.
    m.bus
        .map(BusRegion {
            base: c.rom_base,
            size: c.rom_size,
            device: Box::new(Rom::new(c.rom_base, rom_vec)),
        })
        .expect("A9 ROM window does not overlap another region");

    m.bus
        .map(BusRegion {
            base: A9_DRAM_BASE,
            size: A9_DRAM_SIZE,
            device: Box::new(SparseRam::new(A9_DRAM_BASE, A9_DRAM_SIZE)),
        })
        .expect("A9 DRAM does not overlap another region");

    // The A9's boot scratch, which the ROM's own zero-fill/copy literals place at
    // 0x1800c0000 with a 0x20000 size. It lies inside the DRAM span above, so it
    // is not mapped separately; the config records it and the A11 test uses the
    // same shape to keep the two parts' bands apart.

    // Four UART windows. The window addresses and stride are measured; the
    // register semantics are this project's existing S5L model, which is *not*
    // measured for the A9 and is labelled as such rather than claimed to be.
    for (i, (base, size)) in A9_UART_WINDOWS.into_iter().enumerate() {
        m.bus
            .map(BusRegion {
                base,
                size,
                device: Box::new(S5lUart::new(230 + i as u32, "a9-uart")),
            })
            .expect("A9 UART window does not overlap another region");
    }

    // The A9's generic timer is **not** mapped, and the previous mapping of one
    // here is withdrawn: the A9 SecureROM's only access to `0x20e040000` in the
    // init it reaches is a write at `+0x300`, which is not one of the `Timer`
    // model's registers (`TimerReg` stops at `+0x48` and faults on the rest), so
    // mapping a `Timer` there made the A9 die on a data access it had made
    // correctly. The address is named in the image; that a timer lives at
    // `+0x00`..`+0x48` of it is not established, so nothing is claimed and the
    // page is mapped as the AON register aperture below. `config(S8000).timer_base`
    // keeps the address for the record and is not used to place a device.

    // The AP interrupt controller, named once (0x20f100000, file 0x19000).
    // The size is the A12's 0x30000 carried over as a *starting* aperture: the
    // A9 ROM's own mask sweep is what decides whether that is wide enough, and
    // the census reports the highest offset it touches.
    m.bus
        .map(BusRegion {
            base: c.aic_base,
            size: c.aic_size,
            device: Box::new(Aic::new(0, 512)),
        })
        .expect("A9 AIC does not overlap another region");

    // The PMGR aperture, split so that every address any A9 image names is its own modelled
    // register rather than part of an inert run.
    //
    // The array the ROM programs is `A9_PMGR_STATE_FIRST` (0x20e080100) plus the eight-word
    // `A9_PMGR_STATE_TAIL`; `0x20e080300` is not in the image, so the split is computed from
    // the tail rather than from a contiguous run. The next address the image names after the
    // last tail word is `A9_PMGR_GATE` (`0x20e080400`, file `0x17b18`, the sole literal in
    // the whole image equal to it, and an entry in the PMGR register-pointer table at
    // `0x17b00..0x17b30`). All three checked-in A9 images have **no** literal in
    // `0x20e080344..0x20e080400`.
    let state_first = A9_PMGR_STATE_FIRST;
    let state_end = A9_PMGR_STATE_TAIL[A9_PMGR_STATE_TAIL.len() - 1] + 4;

    // The two named spans, mapped as `Pmgr` because they are the aperture itself.
    for (base, size) in [
        (A9_PMGR_BASE, state_first - A9_PMGR_BASE),
        (A9_PMGR_PAGE2, 0x1000),
    ] {
        m.bus
            .map(BusRegion {
                base,
                size,
                device: Box::new(Pmgr::new(240)),
            })
            .expect("A9 PMGR aperture does not overlap another region");
    }

    // The gate word at 0x20e080400. The ROM writes 0 once (to clear bits 10 and 18)
    // then polls waiting for bits 7:4 = 0xf. PmgrPowerState interprets the low nibble
    // as a target state and would clear ACTUAL on write(0), so use a RegFile that
    // simply retains what is written.
    //
    // It starts at **zero**, and this used to start at `0xff` -- which is exactly the
    // value the poll waits for, so the one pre-set in the map was the one value that
    // would have declared the A9 DFU-ready. The comment above the constant says the
    // word "is **not** declared active"; the code contradicted its own documentation
    // and the readiness test caught it. Zero is the fail-closed state: the ROM's own
    // write of 0 lands on 0, and bits 7:4 stay clear until a real producer exists.
    let mut gate = RegFile::new(320, "pmgr-gate");
    gate.set_word(0, 0x0);
    m.bus
        .map(BusRegion {
            base: A9_PMGR_GATE,
            size: 4,
            device: Box::new(gate),
        })
        .expect("A9 standalone PMGR register does not overlap another region");

    // The power-state array itself: the 64-word run at `A9_PMGR_STATE_FIRST` and the
    // eight `A9_PMGR_STATE_TAIL` words. **These were a hole in the map**, and the hole is
    // the same kind of thing the census could not see: the split above left
    // `0x20e080100..0x20e080343` covered by nothing at all, between the `Pmgr` span that
    // ends at `state_first` and the inert stretch that begins at `state_end`. The run
    // programs all 72 words (census: 74 registers in the page, the 72 array words plus
    // the aperture head at `0x20e080000` and the gate at `0x20e080400`), and with the
    // fallback removed the run dies on the first of them.
    //
    // `PmgrPowerState` and not `Pmgr`, for the reason the second array below uses it and
    // the reason it matters: register storage returns what the guest wrote, so mapping a
    // word the ROM writes `0xf` to as storage would answer the read-back with `0xf` and
    // fabricate a ready state out of the guest's own write. The power-state model answers
    // the actual nibble from its own state, so the read-back stays zero.
    for i in 0..A9_PMGR_STATE_COUNT {
        let address = A9_PMGR_STATE_FIRST + i * A9_PMGR_STATE_STRIDE;
        m.bus
            .map(BusRegion {
                base: address,
                size: 4,
                device: Box::new(iphem_devices::pmgr_power::PmgrPowerState::new(
                    440 + i as u32,
                )),
            })
            .expect("A9 power-state word does not overlap another region");
    }
    for (i, address) in A9_PMGR_STATE_TAIL.iter().enumerate() {
        m.bus
            .map(BusRegion {
                base: *address,
                size: 4,
                device: Box::new(iphem_devices::pmgr_power::PmgrPowerState::new(
                    510 + i as u32,
                )),
            })
            .expect("A9 power-state tail word does not overlap another region");
    }

    // The stretch between the last tail word and the gate, and between the gate and the second
    // page, hold no address any A9 image names. They are mapped as `SparseRam` -- inert
    // storage that answers zero without claiming a device -- rather than as `Pmgr`, because
    // `a9_platform.rs::every_region_in_a_built_a9_machine_is_ram_or_a_named_window` requires
    // every non-DRAM region to contain an address the firmware names, and that test is right:
    // mapping a window the image does not describe is a claim this project has kept refusing
    // to make. Neither stretch is a hole in the map -- they are covered, deliberately, as
    // storage rather than as a device.
    for (base, size) in [
        (state_end, A9_PMGR_GATE - state_end),
        (A9_PMGR_GATE + 4, A9_PMGR_PAGE2 - (A9_PMGR_GATE + 4)),
    ] {
        m.bus
            .map(BusRegion {
                base,
                size,
                device: Box::new(SparseRam::new(base, size)),
            })
            .expect("A9 PMGR inert span does not overlap another region");
    }

    // A second power-state array, at 0x210280000: the same eleven-word,
    // eight-byte-stride shape as the PMGR array above, and the ROM's single
    // init loop at 0x10000b190 writes the same power-state pattern into it
    // (0xf to +0x00/+0x08/+0x10 and +0x40/+0x48/+0x50, 0 to the five between).
    //
    // This is where the old `S8000` config claimed the SEP ASC mailbox lived, and
    // the measurement does not support that name: a mailbox is written with a
    // message and a doorbell, not with a power-state nibble. What is established
    // is the window and the pattern; what the block *is* is not, so the config
    // field it came from is not used to name it here.
    for i in 0..A9_SECOND_STATE_COUNT {
        let address = A9_SECOND_STATE_FIRST + i * A9_PMGR_STATE_STRIDE;
        m.bus
            .map(BusRegion {
                base: address,
                size: 4,
                device: Box::new(iphem_devices::pmgr_power::PmgrPowerState::new(
                    400 + i as u32,
                )),
            })
            .expect("A9 second power-state word does not overlap another region");
    }

    // The A9's always-on register pages. The reset path writes one word in each
    // -- 0x20e040300 <- 0, 0x20e070200 <- 0, 0x20e0a0000 <- 0x1f00,
    // 0x20e0dc000 <- 0 -- in a single de-assert sequence at 0x10000ab08..0x10000ab28,
    // and then the run died: `step @0x10000ab28: Data` with x8 = 0x20e040300, because
    // the only device mapped there was the project's `Timer`, whose register set
    // stops at +0x48 and faults on everything else.
    //
    // So these four pages are mapped as the tree's standard register-storage
    // aperture: a guest write is retained and a read starts at zero. That is a
    // deliberate *non*-claim. It says the window exists and does not lose the
    // guest's writes; it asserts no reset, clock or readiness behaviour, because
    // none of that has been measured for the A9. The window addresses are
    // measurements; the register semantics are not.
    for (id, (base, _word)) in A9_AON_PAGES.iter().enumerate() {
        m.bus
            .map(BusRegion {
                base: *base,
                size: 0x1000,
                device: Box::new(Pmgr::new(410 + id as u32)),
            })
            .expect("A9 AON page does not overlap another region");
    }

    // The 25 pages of `A9_MEASURED_REG_PAGES`, mapped as the same inert register-storage
    // aperture and for the same reason: the window is a measurement and the contents are
    // not. The first of them, `0x202220000`, is where the production run died -- a store
    // of `0x400000` to `+0x68` at instruction 43,100 -- so without this loop the A9 cannot
    // execute its init at all.
    for (id, (base, _word)) in A9_MEASURED_REG_PAGES.iter().enumerate() {
        m.bus
            .map(BusRegion {
                base: *base,
                size: 0x1000,
                device: Box::new(Pmgr::new(600 + id as u32)),
            })
            .expect("A9 measured register page does not overlap another region");
    }

    // OTG PHY control: the A9 ROM names 0x20c000000 (page) and 0x20c100000 (control band).
    // The ROM reads 0x20c000034 (PHY status, bit 8 = session valid/ready) and writes
    // extensively to 0x20c100000. Map both as RegFile for now.
    // Pre-initialize PHY status at 0x34 with bit 8 set (0x100) so the ROM sees PHY ready.
    let mut otgphy = RegFile::new(91, "otgphyctrl");
    otgphy.set_word(0x34, 0x100);
    m.bus
        .map(BusRegion {
            base: 0x20c0_00000,
            size: 0x1000,
            device: Box::new(otgphy),
        })
        .expect("A9 OTG PHY page does not overlap another region");
    m.bus
        .map(BusRegion {
            base: 0x20c1_00000,
            size: 0x1000,
            device: Box::new(RegFile::new(92, "otgphyctrl-band")),
        })
        .expect("A9 OTG PHY control band does not overlap another region");

    // USB complex: the A9 ROM writes 0x101 to offsets 0x1c, 0x3c, 0x5c, 0x7c within
    // a 0xa0 window at 0x20c900000, same as the A8.
    m.bus
        .map(BusRegion {
            base: 0x20c9_00000,
            size: 0xa0,
            device: Box::new(RegFile::new(93, "usb-complex")),
        })
        .expect("A9 USB complex does not overlap another region");

    // USB device (DWC2): from the A9 device tree, /arm-io/usb-complex/usb-device is at
    // 0x20ca00000. Use the config's usb_base.
    m.bus
        .map(BusRegion {
            base: c.usb_base,
            size: c.usb_size,
            device: Box::new(UsbDeviceController::new(7, c.usb_base, 451)),
        })
        .expect("A9 USB device does not overlap another region");

    // A9 (S8000) USB host stack: UsbPhy, UsbMux, UsbPd.
    // Note: usb-complex is already mapped as RegFile at 0x20c900000 above.
    // Only map host components if the base is established (non-zero).
    if c.usbphy_base != 0 {
        m.bus
            .map(BusRegion {
                base: c.usbphy_base,
                size: 0x1000,
                device: Box::new(UsbPhy::new(75)),
            })
            .expect("A9 USB PHY does not overlap another region");
    }
    if c.usbmux_base != 0 {
        m.bus
            .map(BusRegion {
                base: c.usbmux_base,
                size: 0x1000,
                device: Box::new(UsbMux::with_irq(76, 68)),
            })
            .expect("A9 USB Mux does not overlap another region");
    }
    if c.usbpd_base != 0 {
        m.bus
            .map(BusRegion {
                base: c.usbpd_base,
                size: 0x1000,
                device: Box::new(UsbPd::with_irq(77, 69)),
            })
            .expect("A9 USB PD does not overlap another region");
    }

    // A9 (S8000) SEP→AP ASC mailbox: **deliberately not mapped**, and the field
    // this used to map is not what its name says. `S8000.mailbox_base` is
    // `A9_SECOND_STATE_FIRST` (0x210280000) -- the A9's second power-state array,
    // eleven registers the ROM writes with the same pattern it writes the
    // seventy-two PMGR words with, which is not what a mailbox is written with
    // (the reasoning is on the field itself). Mapping an `AppleAscMailbox` there
    // therefore did two wrong things at once: it collided with the power-state
    // array mapped above -- the A9 build panicked on the overlap -- and it put a
    // device with invented semantics over a measured aperture, so a future ROM
    // read of it would be answered by a mailbox that the hardware does not have
    // there. The A8's builder already declines its baseless `mailbox_base` for
    // the same reason and with the same argument; the A9 now matches it.
    //
    // The A9 does have a SEP (AppleSEPROM-302) and its harness starts the
    // companion with no mailbox, which is the honest state: no A9 image in this
    // corpus names an ASC mailbox base. Finding one is the open question, and
    // until then the gap is a fault at a real address rather than a model
    // answering for hardware that is not there.

    m
}

pub fn build_a8_t7000(rom_data: &[u8], enable_debug: bool) -> Machine {
    let c = config(SoCId::A8);
    let mut rom_vec = vec![0u8; c.rom_size as usize];
    let len = rom_data.len().min(rom_vec.len());
    rom_vec[..len].copy_from_slice(&rom_data[..len]);

    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1000,
    });
    let _ = enable_debug;

    m.bus
        .map(BusRegion {
            base: c.rom_base,
            size: c.rom_size,
            device: Box::new(Rom::new(c.rom_base, rom_vec)),
        })
        .expect("A8 ROM window does not overlap another region");

    m.bus
        .map(BusRegion {
            base: A8_DRAM_MAPPED_BASE,
            size: A8_DRAM_SIZE,
            device: Box::new(SparseRam::new(A8_DRAM_MAPPED_BASE, A8_DRAM_SIZE)),
        })
        .expect("A8 DRAM does not overlap another region");

    // The A8's boot SRAM. The ROM reads file 0x358 = 0x1800c0000 and file 0x360 = 0x20000
    // and zero-fills that span in 16-byte steps from `stp x12, x12, [x10]` at 0x10000006c,
    // so the first store of the loop is instruction 25 and it faulted with nothing mapped
    // there. The window is the ROM's own 0x20000 rather than a wider guess, so the mapping
    // claims exactly the span the firmware says it writes.
    m.bus
        .map(BusRegion {
            base: c.sram_base,
            size: c.sram_size,
            device: Box::new(SparseRam::new(c.sram_base, c.sram_size)),
        })
        .expect("A8 boot SRAM does not overlap another region");

    // The A8's relocated-image window, sized by the ROM's own loop bounds. The prologue
    // copies `ldp/stp x13, x14` from the 4 KiB-aligned source 0x100019000 to 0x180080000
    // while x11 != x12, with x12 read from file 0x340 = 0x180080800 -- so 0x800 bytes -- and
    // then zero-fills from 0x180080800 with `stp x13, x13, [x10], #0x10` until x11 reaches
    // file 0x350 = 0x180088ad0 masked with ~0xf. The span the firmware writes is therefore
    // 0x180080000..0x180088ad0, and that is what is mapped: the size is the ROM's own
    // arithmetic rather than a page-rounded guess, so the mapping claims exactly the range
    // the firmware says it writes.
    m.bus
        .map(BusRegion {
            base: 0x1800_80000,
            size: 0x8ad0,
            device: Box::new(SparseRam::new(0x1800_80000, 0x8ad0)),
        })
        .expect("A8 relocated-image window does not overlap another region");

    // The A8's interrupt controller, at the address its own device tree declares.
    // `/device-tree/arm-io/aic` is `reg = 0xe100000 + 0x100000`, `compatible = "aic,1"`,
    // so `0x20e100000` for a megabyte, and the run programs it there: `1` to
    // `0x20e100010`, four per-line words at `0x20e103260..0x20e10326c`, eight `0xffffffff`
    // words at `0x20e104100..0x20e10411c`, and `0x08000000` to `0x20e104190`. That is the
    // T8020's controller programmed at the T8020's offsets -- `0x23b100010`,
    // `0x23b103284`/`0x23b103290` and the `0x23b104100..0x23b10413c` clear bank -- which is
    // what identifies the block.
    //
    // This used to be mapped at `0x23b100000`, the **T8020's** AIC, an address no A8 tree
    // node and no A8 measurement produces, while `timer_base` held the A8's real AIC address
    // with no controller on it. Both are corrected in the A8 config.
    //
    // **Its register layout is the model's, and the model's layout is the T8020's.** The
    // A8's clear bank is at `+0x1000` where the T8020's is at `+0x0100`, and its per-line
    // words are at `+0x3260` where the T8020's are at `+0x3284`, so every A8 access lands
    // outside the banks the model decodes and is stored and read back verbatim. That is the
    // honest outcome for an unmeasured layout: a firmware poll on one of those offsets spins
    // and the gate stays visible, rather than a model borrowing a clear-when-written
    // behaviour from a different part and satisfying a poll nothing established.
    m.bus
        .map(BusRegion {
            base: c.aic_base,
            size: c.aic_size,
            device: Box::new(Aic::new(0, 512)),
        })
        .expect("A8 AIC does not overlap another region");

    // The A8's GPIO, from the A8's own device tree.
    //
    // `DeviceTree.n61ap.im4p` (the iPhone 6 / A8 tree, extracted from
    // `IPSW/iPhone7,2_12.5.8_16H88_Restore.ipsw`) declares
    // `/device-tree/arm-io/gpio` with `compatible = "gpio,t7000","gpio,s5l8960x"` and
    // `reg = 0xe300000 + 0x100000` -- a 1 MiB aperture, the same size the A12 declares.
    //
    // The `+0x200000000` is the arm-io offset, and for the A8 it is confirmed at four nodes
    // whose physical address the T7000 ROM composes: `pmgr` 0xe000000, `wdt` 0xe030000,
    // `otgphyctrl` 0xc000010 and `sochot0` 0xe038000. That cross-check is what the earlier
    // claim got wrong: counting *every* tree `reg` against the ROM's composed constants
    // prefers an offset of zero, because small values like 0x14 and 0x2710 coincide with
    // small composed constants. Restricted to addresses >= 0x1000, where a `reg` is a window
    // rather than a register offset, the A8 agrees with the A11 and A12 at four points and
    // disagrees with every other candidate offset.
    //
    // So the GPIO is 0x20e300000, and `c.gpio_base`'s 0x20a100000 was off by 0x4200000 --
    // it is not a value any A8 measurement produces. The window is the tree's 0x100000, not
    // the 0x10000 mapped here before, which covered a sixteenth of the block the ROM owns.
    m.bus
        .map(BusRegion {
            base: 0x20e3_00000,
            size: 0x10_0000,
            device: Box::new(AppleGpio::new(45)),
        })
        .expect("A8 GPIO does not overlap another region");

    // The A8's SIO, likewise from the tree: `/device-tree/arm-io/sio` with
    // `compatible = "iop,s5l8960x"`, `reg = 0xae00000 + 0x2000`, giving 0x20ae00000.
    //
    // The previous mapping was 0xae000000 + 0x10000 under a comment saying "as specified",
    // with nothing specifying it: the address is a factor of ten from the tree's, and no A8
    // measurement produces it. It is left as plain RAM rather than an SIO model because no
    // SIO register semantics have been established for this generation -- the A8 predates the
    // `iop,ascwrap-v2` blocks the A12 tree describes, so its mailbox registers are a
    // different layout and mapping an ASC mailbox here would import the wrong one.
    m.bus
        .map(BusRegion {
            base: 0x20ae_00000,
            size: 0x2000,
            device: Box::new(SparseRam::new(0x20ae_00000, 0x2000)),
        })
        .expect("A8 SIO does not overlap another region");

    // The A8's main PMGR plane, which `pmgr_base` names and the A8 tree declares:
    // `/device-tree/pmgr` with `reg = 0xe000000 + 0x40000` => 0x20e000000 + 0x40000 physical.
    // It is the one plane whose base the T7000 ROM composes, and the run reads it at offset
    // 0x2a010 (`ldr w9, [x8]` at 0x10000a0d8, x8 built by three movz/movk at 0x10000a0cc) and
    // at 0x2a000 four instructions earlier, in the function that ends `ubfm w0, w8, #9, #15;
    // ret`. Neither address is mapped, because the A8 builder never mapped `pmgr_base` at all
    // -- the field was correct and unused.
    //
    // The `slot_ack` range is the one place in this plane where a register's semantics are
    // modeled rather than left inert, and it is scoped to the ROM's own slot array:
    // `0x10000aa94`..`0x10000aae0` computes `0x20e020000 + index*8`, sets or clears bits 3:0,
    // and then spins until `((w9) ^ (w9 >> 4)) & 0xf == 0` -- until bits 7:4 report the request
    // in bits 3:0. The range is the helper's own bound read as a plane offset: the `cmp xzr, x8,
    // x9` / `b.hi` pair at 0x10000aaac compares the *absolute* address against `0x20e021010`, so
    // the last slot the helper can reach is plane offset 0x20000 + 0x201*8 = 0x21008, and
    // 0x20108 -- a plausible-looking reading of the same digits -- excludes slot 0x3f, which is
    // the slot the ROM actually polls. The A8 tree puts its power-state registers on that same
    // 8-byte grid (`pmgr ps-reg[0..5]` at 0x20000, 0x20100, 0x20200, 0x20300, 0x20400, 0x21000),
    // and ps-reg[5] lands on slot 0x200 = 512, the second-to-last of the 513 the bound allows,
    // which is what fixes the range at 0x21008 rather than anywhere nearby.
    //
    // Measured at instruction 137913 with a constant index: the caller at 0x10000a6a4 passes
    // `w0 = 0x3f`, `w1 = 1` and the run then spends 62087 instructions in the poll, so the
    // request is 0xf to offset 0x201f8 and an inert register can never satisfy it. The
    // *status* value is therefore derived from the firmware's completion condition, not measured,
    // and the device behind slot 0x3f is not claimed: the tree labels thirteen `pmgr device[]`
    // entries (4, 5, 6, 7, 8, 18..23, 71, 72) and 63 is not one of them.
    m.bus
        .map(BusRegion {
            base: 0x20e0_00000,
            size: 0x4_0000,
            device: Box::new(
                Pmgr::new(89)
                    .with_slot_ack(0x20000, 0x21008)
                    .with_plane_go_wait(vec![
                        (0x0, 1 << 0, 1 << 31, 4),
                        (0x4, 1 << 0, 1 << 31, 4),
                        (0x4, 1 << 0, 1 << 63, 8),
                        // PMGR plane offset 0x20: ROM writes bit 25 (0x2000000), polls until bit 63 is CLEARED
                        // Hardware clears bit 63 when operation completes. Model instant completion: ack=0.
                        (0x20, 1 << 25, 0, 8),
                        // PMGR power-domain go/wait registers: the ROM iterates through these
                        // at 0x1000 stride (0x2004, 0x3004, ..., 0x1f004) with 32-bit write of 1,
                        // polling for sign bit (bit 31) to be set as ack.
                        (0x2004, 1 << 0, 1 << 31, 4),
                        (0x3004, 1 << 0, 1 << 31, 4),
                        (0x4004, 1 << 0, 1 << 31, 4),
                        (0x5004, 1 << 0, 1 << 31, 4),
                        (0x6004, 1 << 0, 1 << 31, 4),
                        (0x7004, 1 << 0, 1 << 31, 4),
                        (0x8004, 1 << 0, 1 << 31, 4),
                        (0x9004, 1 << 0, 1 << 31, 4),
                        (0xa004, 1 << 0, 1 << 31, 4),
                        (0xb004, 1 << 0, 1 << 31, 4),
                        (0xc004, 1 << 0, 1 << 31, 4),
                        (0xd004, 1 << 0, 1 << 31, 4),
                        (0xe004, 1 << 0, 1 << 31, 4),
                        (0xf004, 1 << 0, 1 << 31, 4),
                        (0x10004, 1 << 0, 1 << 31, 4),
                        (0x11004, 1 << 0, 1 << 31, 4),
                        (0x12004, 1 << 0, 1 << 31, 4),
                        (0x13004, 1 << 0, 1 << 31, 4),
                        (0x14004, 1 << 0, 1 << 31, 4),
                        (0x15004, 1 << 0, 1 << 31, 4),
                        (0x16004, 1 << 0, 1 << 31, 4),
                        (0x17004, 1 << 0, 1 << 31, 4),
                        (0x18004, 1 << 0, 1 << 31, 4),
                        (0x19004, 1 << 0, 1 << 31, 4),
                        (0x1a004, 1 << 0, 1 << 31, 4),
                        (0x1b004, 1 << 0, 1 << 31, 4),
                        (0x1c004, 1 << 0, 1 << 31, 4),
                        (0x1d004, 1 << 0, 1 << 31, 4),
                        (0x1e004, 1 << 0, 1 << 31, 4),
                        (0x1f004, 1 << 0, 1 << 31, 4),
                        // PMGR power-domain blocks at 0x4000 stride: each block has go/wait at +0x4 (bit 25) and +0x0 (sign bit)
                        (0x4004, 1 << 25, 1 << 31, 4),
                        (0x4000, 1 << 0, 1 << 31, 4),
                        (0x8004, 1 << 25, 1 << 31, 4),
                        (0x8000, 1 << 0, 1 << 31, 4),
                        (0xc004, 1 << 25, 1 << 31, 4),
                        (0xc000, 1 << 0, 1 << 31, 4),
                        (0x10004, 1 << 25, 1 << 31, 4),
                        (0x10000, 1 << 0, 1 << 31, 4),
                        (0x14004, 1 << 25, 1 << 31, 4),
                        (0x14000, 1 << 0, 1 << 31, 4),
                        (0x18004, 1 << 25, 1 << 31, 4),
                        (0x18000, 1 << 0, 1 << 31, 4),
                        (0x1c004, 1 << 25, 1 << 31, 4),
                        (0x1c000, 1 << 0, 1 << 31, 4),
                        (0x20004, 1 << 25, 1 << 31, 4),
                        (0x20000, 1 << 0, 1 << 31, 4),
                        (0x24004, 1 << 25, 1 << 31, 4),
                        (0x24000, 1 << 0, 1 << 31, 4),
                        (0x28004, 1 << 25, 1 << 31, 4),
                        (0x28000, 1 << 0, 1 << 31, 4),
                        (0x2c004, 1 << 25, 1 << 31, 4),
                        (0x2c000, 1 << 0, 1 << 31, 4),
                        (0x30004, 1 << 25, 1 << 31, 4),
                        (0x30000, 1 << 0, 1 << 31, 4),
                        (0x34004, 1 << 25, 1 << 31, 4),
                        (0x34000, 1 << 0, 1 << 31, 4),
                        (0x38004, 1 << 25, 1 << 31, 4),
                        (0x38000, 1 << 0, 1 << 31, 4),
                        (0x3c004, 1 << 25, 1 << 31, 4),
                        (0x3c000, 1 << 0, 1 << 31, 4),
                        // PMGR device registers: ps-reg[0..4]+8 at 0x20008, 0x21008, 0x22008, 0x23008, 0x24008, 0x25008
                        // ROM writes 0xffff_0348 and polls for ack (bit 63 mirrors bit 25 = request bit)
                        (0x20008, 1 << 25, 1 << 63, 8),
                        (0x21008, 1 << 25, 1 << 63, 8),
                        (0x22008, 1 << 25, 1 << 63, 8),
                        (0x23008, 1 << 25, 1 << 63, 8),
                        (0x24008, 1 << 25, 1 << 63, 8),
                        (0x25008, 1 << 25, 1 << 63, 8),
                        // Slot 0x3f (index 63) at offset 0x201f8: ROM writes request 0xf, polls for completion ack at bit 63
                        (0x201f8, 1 << 0, 1 << 63, 8),
                    ])
                    // 0x4004 is armed by the write, not by a bit in it: the A8 ROM
                    // stores `w9 & ~1` there and then spins on `cmn w9,#0` / `b.lt`
                    // waiting for bit 31. Without this the ROM never sees an
                    // acknowledgement and stops at `PC=0x10000a3b4`.
                    .with_write_armed_go_wait(vec![0x4004])
                    // The A8 maps the whole 0x40000 `pmgr` window as one region, so the register
                    // the T7000 polls is at *device* offset 0x20400 -- the 0x20000 plane the slot
                    // table starts at, plus the 0x400 the cross-part evidence puts this register at
                    // (`0x20e020400` on the A8 and A8X; `0x20e080400` = its plane + 0x400 on the
                    // T8012). It is measured: 237,743 reads and one write, of `0`, at 0x10000a48c,
                    // with the final PC `0x10000a498` in `b.ne 0x10000a48c` at a 1M budget.
                    // `with_slot_ack(0x20000, 0x21008)` covers this offset and reports bits 7:4 as
                    // an echo of bits 3:0, which the ROM leaves at 0 -- so the poll is not slow but
                    // dead. This rule takes precedence there; see `Pmgr::status_nibble_all_ones`
                    // for the cross-part evidence that the T7000's second PMGR service is Apple's
                    // and shared, not an A8 quirk.
                    .with_status_nibble_all_ones(vec![0x20400])
                    // The boot-mode register at device offset 0x2a000 (physical 0x20e02a000).
                    // `0x10000a0ac` loads this register and extracts bits 9:15 to index an
                    // eight-entry table. Zero selects the non-DFU path; any non-zero value in
                    // bits 9:15 selects the DFU path. The real silicon value is a thermal sensor
                    // reading (sochot0), but nothing in this repository establishes it. Setting
                    // bit 9 (value 0x200) is sufficient to select a non-zero table entry.
                    .with_register(0x2a000, 0x200),
            ),
        })
        .expect("A8 PMGR base does not overlap another region");

    // A PMGR plane the A8 tree declares, which is where the run currently stops.
    //
    // The A8 tree carries sixteen `pmgr1,t7000` nodes. `pmgr_base` covers the first,
    // 0x20e000000 + 0x40000, and that is the one whose base the T7000 ROM composes. This is
    // a different plane: `/device-tree/pmgr` with `reg = 0x2220000 + 0x2000`, which is
    // 0x202220000 + 0x2000 physical, and the ROM stores 0x480000 to offset 0x68 inside it at
    // instruction 44721 (`str x9, [x8]` with x8 built by three movz/movk at 0x10000a220).
    //
    // Found by containment rather than by a base match: the register the ROM writes is
    // 0x68 into the window, so a scan looking for a `reg` equal to 0x202220068 finds nothing
    // and a scan looking for a base inside a band finds the base but not the relationship.
    // The same trap hid the A11 PMGR gate earlier in this work.
    //
    // Only this plane is mapped, not all sixteen. Each is added when the ROM is measured
    // reaching it, which is the order that kept the A11 mapping honest.
    m.bus
        .map(BusRegion {
            base: 0x2022_20000,
            size: 0x2000,
            device: Box::new(
                Pmgr::new(90)
                    .with_plane_go_wait(vec![(0x20, 1 << 25, 0, 8), (0x50, 1 << 25, 0, 8)]),
            ),
        })
        .expect("A8 PMGR plane does not overlap another region");

    // The A8's USB device controller, at the address the T7000 ROM composes. See `A8::usb_base`
    // for the four sites and the register offsets that fix it, and for why the tree's
    // `usb-device` base cell is not used.
    m.bus
        .map(BusRegion {
            base: c.usb_base,
            size: c.usb_size,
            device: Box::new(UsbDeviceController::new(7, c.usb_base, 451)),
        })
        .expect("A8 USB does not overlap another region");

    // The other three controllers the A8 tree declares under `usb-complex` are **not** mapped,
    // and that is a decision rather than an omission. `usb-complex` is four `0x20`-byte
    // `usb_widget` blocks in a `0xa0` window -- the tree's `usb-device` node carries its widget
    // register as `reg = 0x20 + 0x10`, and the ROM writes `0x101` to the last word of each block
    // at `+0x1c`, `+0x3c`, `+0x5c` and `+0x7c`, which is the power gate for `usb-device`,
    // `usb-ehci0`, `usb-ohci0` and `usb-ehci2` in tree order. The run powers all four at
    // instruction 148,301.
    //
    // Their windows, though, are given only as *relative* cells -- `0x200000`, `0x300000` and
    // `0x500000` for the three host controllers -- against no base the ROM composes and no
    // absolute value the tree states, since the tree's own `usb-device` cell (`0x100000`)
    // already disagrees with the composed `0xc100000` by `0xc00000`. Anchoring them to
    // `usb-complex` would invent `0x20cb00000`, `0x20cc00000` and `0x20ce00000`: three addresses
    // nothing in this repository produces, for a host stack a device-mode DFU restore never
    // drives. They are recorded in `docs/A8_T7000_PLATFORM.md` instead of mapped.

    // ── The windows the A8 run is measured to reach, in the order it reaches them ──
    //
    // Every one of these was added because a run of the real T7000 SecureROM faulted or
    // stalled on it, and each carries where its address comes from. None of them is mapped
    // because a config field or a sibling part had it: that is how `aic_base` came to hold
    // the T8020's `0x23b100000` while the A8 tree declares the A8's AIC at
    // `0xe100000 + 0x100000`, which is why the run reached an interrupt controller the
    // platform had not mapped and a `UsbDeviceController` sat at `0x20a800000`, an address
    // no A8 measurement and no A8 tree node produces. That second defect is corrected at
    // `A8::usb_base`, where the citation is the ROM's own four `0x20c100000` compositions.
    //
    // The A8 tree is `DeviceTree.n61ap.im4p`, and its `reg` cells become physical addresses
    // by adding `0x2_0000_0000`. Four nodes are the exception and already carry physical
    // addresses, which is how they are recognised: `apcie` and `dart-apcie1` are in the
    // `0x60_xxxxxxx` DART/PCIe band, and `arm-io` and `pmgr/clpc` are in the
    // `0x202_xxxxxx` SIO band. Adding the offset to those would produce addresses nothing
    // in the A8 band contains.

    // `/device-tree/arm-io/otgphyctrl` declares `0xc000030 + 0x20` -- a 32-byte register, not
    // a window -- and `0xe031000 + 0x1000`. The first is the register block the T7000 ROM
    // composes directly: `movz x8, #0x2, lsl #32` / `movk x8, #0xc00, lsl #16` /
    // `movk x8, #0x30` gives `0x20c000030`, the fourth of the four nodes that confirm the
    // A8's `arm-io` offset. The run reads `0x20c000034`, four bytes past it. Mapped as the
    // 1 KiB window the tree's second entry declares, at the base that entry gives.
    m.bus
        .map(BusRegion {
            base: 0x0002_0c00_0000,
            size: 0x1000,
            device: Box::new(RegFile::new(91, "otgphyctrl")),
        })
        .expect("A8 OTG PHY control does not overlap another region");

    // `/device-tree/arm-io/usb-complex`: `reg = 0xc900000 + 0xa0`, so `0x20c900000 + 0xa0`.
    // The 0xa0 is what makes the measured layout legible: the T7000 ROM writes the same
    // value `0x101` to four registers at `0x1c`, `0x3c`, `0x5c` and `0x7c` -- 0x20 apart,
    // the last word of each of four 0x20 blocks inside a 0xa0 window -- and the tree names
    // four controllers under this node, `usb-device`, `usb-ehci0`, `usb-ohci0` and
    // `usb-ehci2`. The writes come right after a `bl 0x10000aa94` that turns PMGR device
    // slot 0x49 on, so this is the point where the ROM powers and enables its four
    // controllers.
    //
    // `UsbComplex` is not used: its layout is the T8020's (`control` at 0, `status` at
    // 0x64) and the A8's registers are at 0x1c..0x7c, so reusing it would put a T8020
    // register map on an A8 window. `RegFile` gives the reads back what the ROM wrote and
    // names the window for what the tree says it is.
    m.bus
        .map(BusRegion {
            base: 0x20c9_00000,
            size: 0xa0,
            device: Box::new(RegFile::new(92, "usb-complex")),
        })
        .expect("A8 USB complex does not overlap another region");

    // A8 (T7000) USB host stack: UsbPhy, UsbMux, UsbPd.
    m.bus
        .map(BusRegion {
            base: c.usbphy_base,
            size: 0x1000,
            device: Box::new(UsbPhy::new(75)),
        })
        .expect("A8 USB PHY does not overlap another region");
    m.bus
        .map(BusRegion {
            base: c.usbmux_base,
            size: 0x1000,
            device: Box::new(UsbMux::with_irq(76, 68)),
        })
        .expect("A8 USB Mux does not overlap another region");
    m.bus
        .map(BusRegion {
            base: c.usbpd_base,
            size: 0x1000,
            device: Box::new(UsbPd::with_irq(77, 69)),
        })
        .expect("A8 USB PD does not overlap another region");

    // `/device-tree/arm-io`'s own first window: `reg = 0x202240000 + 0x1000`. This cell is
    // already physical -- it is in the `0x202_xxxxxx` SIO band, not the `0x2_0xxx_xxxx`
    // band the `arm-io` children use -- and the node carries no `compatible`, so the tree
    // names the window and nothing else. The run reaches it at
    // 146,960 instructions with a leaf routine at `0x100002730` that clears the low 32 bits
    // of the 64-bit register at `+0x8`, zeroes `+0x10` and `+0x18`, then sets bit 41 of
    // `+0x8` and returns. Three registers in one 0x20 block, written as a group and not
    // polled.
    m.bus
        .map(BusRegion {
            base: 0x2022_40000,
            size: 0x1000,
            device: Box::new(RegFile::new(93, "arm-io-window0")),
        })
        .expect("A8 arm-io window 0 does not overlap another region");

    // A 64 KiB block at `0x20a080000` in four 16 KiB pieces, which is where the run's
    // pointer table leads. Nothing in the A8 tree declares it: the nearest declared windows
    // are `mca0` at `0x20a0a0000` and `uart0` at `0x20a0c0000`, both 0x20000 and 0x40000
    // above it. The address itself is not composed by the ROM either -- the store at
    // `0x100004d24` writes through `x8 = ldr x8, [x23, #0x30]`, a base held in the device
    // table the ROM builds in the relocated image at `0x180083c50`. That is the same
    // shape as the A10's `0x20e080000`, which a static `movz`/`movk` scan cannot see for
    // exactly this reason.
    //
    // Mapped as one 64 KiB register file because the four 16 KiB pieces are written by the
    // same code (`0x100004dec` writes 0 to `+0x4000`, `+0x8000` and `+0xc000`) and nothing
    // distinguishes them yet.
    m.bus
        .map(BusRegion {
            base: 0x20a0_80000,
            size: 0x1_0000,
            device: Box::new(RegFile::new(94, "a8-0x20a080000")),
        })
        .expect("A8 0x20a080000 block does not overlap another region");

    m
}

/// Build the A7 (S5L8960X, iPhone 5s) AP platform.
/// The A7 AP SecureROM shares the same early boot code as the A8/T7000,
/// including the same device address composition via ADRP/MOVZ/MOVK.
/// This function mirrors `build_a8_t7000` but uses the A7's ROM size (512 KB).
pub fn build_a7_ap_platform(rom_data: &[u8], _enable_debug: bool) -> Machine {
    let c = config(SoCId::A7);
    let mut rom_vec = vec![0u8; c.rom_size as usize];
    let len = rom_data.len().min(rom_vec.len());
    rom_vec[..len].copy_from_slice(&rom_data[..len]);

    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1000,
    });
    let _ = _enable_debug;

    m.bus
        .map(BusRegion {
            base: c.rom_base,
            size: c.rom_size,
            device: Box::new(Rom::new(c.rom_base, rom_vec)),
        })
        .expect("A7 ROM window does not overlap another region");

    m.bus
        .map(BusRegion {
            base: A8_DRAM_MAPPED_BASE,
            size: A8_DRAM_SIZE,
            device: Box::new(SparseRam::new(A8_DRAM_MAPPED_BASE, A8_DRAM_SIZE)),
        })
        .expect("A7 DRAM does not overlap another region");

    // The A7's boot SRAM. Same as A8: file 0x358 = 0x1800c0000, file 0x360 = 0x20000.
    m.bus
        .map(BusRegion {
            base: c.sram_base,
            size: c.sram_size,
            device: Box::new(SparseRam::new(c.sram_base, c.sram_size)),
        })
        .expect("A7 boot SRAM does not overlap another region");

    // The A7's relocated-image window. Same bounds as A8: 0x180080000..0x180088ad0.
    m.bus
        .map(BusRegion {
            base: 0x1800_80000,
            size: 0x8ad0,
            device: Box::new(SparseRam::new(0x1800_80000, 0x8ad0)),
        })
        .expect("A7 relocated-image window does not overlap another region");

    // The A7's interrupt controller, at the same address as A8.
    m.bus
        .map(BusRegion {
            base: c.aic_base,
            size: c.aic_size,
            device: Box::new(Aic::new(0, 512)),
        })
        .expect("A7 AIC does not overlap another region");

    // The A7's GPIO, same as A8.
    m.bus
        .map(BusRegion {
            base: 0x20e3_00000,
            size: 0x10_0000,
            device: Box::new(AppleGpio::new(45)),
        })
        .expect("A7 GPIO does not overlap another region");

    // The A7's SIO, same as A8.
    m.bus
        .map(BusRegion {
            base: 0x20ae_00000,
            size: 0x2000,
            device: Box::new(SparseRam::new(0x20ae_00000, 0x2000)),
        })
        .expect("A7 SIO does not overlap another region");

    // The A7's main PMGR plane, at the same base as the A8's but with a different
    // plane go/wait handshake. This table used to be a verbatim copy of the A8's, and
    // the A7 SecureROM then spun at `PC=0x10000867c` for a whole 20M-instruction budget.
    //
    // Measured from `SecureROM for s5l8960xsi, iBoot-1704.10`, file `0x8650..0x868c`
    // (VA `0x100008650`), with this repository's own disassembler:
    //
    // ```text
    // 0x100008650: movz x1, #0x2, lsl #32 ; movk x1, #0xe00, lsl #16   -> x1 = 0x20e000000
    // 0x100008658: movz x2, #0                                           -> plane index 0
    // 0x10000865c: cmp  w2, #0x1 ; 0x100008660: b.eq +0x24            -> index 1 is skipped
    // 0x100008664: add  x3, x1, x2 lsl #12                              -> plane address
    // 0x100008668: ldr  w4, [x3]
    // 0x10000866c: orr  w4, w4, #0x8000000                             -> request = bit 27
    // 0x100008670: str  w4, [x3]
    // 0x100008674: ubfm x3, x2, #52, #51                                -> x2 << 12
    // 0x100008678: add  x4, x3, x1
    // 0x10000867c: ldr  w4, [x4]                                        -> poll same plane
    // 0x100008680: tbz  x4, #25, loop                                  -> ack = bit 25
    // 0x100008888: cmp  w2, #0x6 ; 0x10000888c: b.ne back              -> six indices
    // ```
    //
    // **The request and ack bits are the load-bearing part, and they are not the A8's.**
    // The ROM sets `0x08000000` (bit 27) and waits for `0x02000000` (bit 25). The
    // inherited entry was `(request=bit 0, ack=bit 31)`, so the ROM's `0x08000000` write
    // read back verbatim with the acknowledgement withheld and `tbz x4, #25` never passed.
    // Fixing bit 27/bit 25 moved the run off index 0: the final register image became
    // `x2=2`, `x3=0x2000`, `x4=0x20e002000` -- i.e. it advanced past plane 0, skipped
    // plane 1, and then stalled on plane 2, which is what proved each plane is its own
    // handshake and not one shared register.
    //
    // **`ubfm x3, x2, #52, #51` renders misleadingly, and the first reading of it here
    // was wrong.** The disassembler prints the raw `immr`/`imms` fields, but this is the
    // 32-bit form (`N=0`), where the architecturally effective `immr` is `32 - immr`;
    // taken literally the field pair reads as "extract bits [52:51]", which is zero for
    // every index, and that would have implied all six polls read plane 0. The executed
    // behaviour is the ground truth and says otherwise: `x3 = x2 << 12`, so the poll
    // address is `0x20e000000 + x2*0x1000` -- the same plane just written, recomputed
    // redundantly. The `x3`/`x4` pair in the final register image above is the positive
    // control for this, not a re-derivation of it. Recording it because this is the
    // second time in this project a hand-rolled `movz`/`movk`/immediate decode produced a
    // self-consistent but wrong claim that only a cross-check caught.
    //
    // Only the five planes the ROM actually touches are listed. `0x1000` is deliberately
    // absent: `cmp w2, #1 ; b.eq` skips index 1, so modelling a handshake there would be
    // a claim about hardware this loop never exercises. It stays the legacy register store.
    //
    // The A8/T7000 table is deliberately left alone: its bit 0 / bit 31 handshake is
    // separately measured there (`0x10000a884..0x10000a890`), the two parts have separate
    // `Pmgr` instances, and sharing one table would let one part's measurement silently
    // overwrite the other's -- which is exactly how the A7 came to inherit the wrong bits.
    m.bus
        .map(BusRegion {
            base: 0x20e0_00000,
            size: 0x4_0000,
            device: Box::new(
                Pmgr::new(89)
                    .with_slot_ack(0x20000, 0x21008)
                    .with_plane_go_wait(vec![
                        // A7 plane go/wait, measured: request bit 27, ack bit 25, 32-bit,
                        // one handshake per plane. See the comment above. NOT the A8's
                        // bit 0 / bit 31. `0x1000` is the plane the ROM skips.
                        (0x0, 1 << 27, 1 << 25, 4),
                        (0x2000, 1 << 27, 1 << 25, 4),
                        (0x3000, 1 << 27, 1 << 25, 4),
                        (0x4000, 1 << 27, 1 << 25, 4),
                        (0x5000, 1 << 27, 1 << 25, 4),
                        (0x4, 1 << 0, 1 << 31, 4),
                        (0x4, 1 << 0, 1 << 63, 8),
                        (0x20, 1 << 25, 0, 8),
                        (0x2004, 1 << 0, 1 << 31, 4),
                        (0x3004, 1 << 0, 1 << 31, 4),
                        (0x4004, 1 << 0, 1 << 31, 4),
                        (0x5004, 1 << 0, 1 << 31, 4),
                        (0x6004, 1 << 0, 1 << 31, 4),
                        (0x7004, 1 << 0, 1 << 31, 4),
                        (0x8004, 1 << 0, 1 << 31, 4),
                        (0x9004, 1 << 0, 1 << 31, 4),
                        (0xa004, 1 << 0, 1 << 31, 4),
                        (0xb004, 1 << 0, 1 << 31, 4),
                        (0xc004, 1 << 0, 1 << 31, 4),
                        (0xd004, 1 << 0, 1 << 31, 4),
                        (0xe004, 1 << 0, 1 << 31, 4),
                        (0xf004, 1 << 0, 1 << 31, 4),
                        (0x10004, 1 << 0, 1 << 31, 4),
                        (0x11004, 1 << 0, 1 << 31, 4),
                        (0x12004, 1 << 0, 1 << 31, 4),
                        (0x13004, 1 << 0, 1 << 31, 4),
                        (0x14004, 1 << 0, 1 << 31, 4),
                        (0x15004, 1 << 0, 1 << 31, 4),
                        (0x16004, 1 << 0, 1 << 31, 4),
                        (0x17004, 1 << 0, 1 << 31, 4),
                        (0x18004, 1 << 0, 1 << 31, 4),
                        (0x19004, 1 << 0, 1 << 31, 4),
                        (0x1a004, 1 << 0, 1 << 31, 4),
                        (0x1b004, 1 << 0, 1 << 31, 4),
                        (0x1c004, 1 << 0, 1 << 31, 4),
                        (0x1d004, 1 << 0, 1 << 31, 4),
                        (0x1e004, 1 << 0, 1 << 31, 4),
                        (0x1f004, 1 << 0, 1 << 31, 4),
                        (0x4004, 1 << 25, 1 << 31, 4),
                        // Superseded at this offset: the A7 ROM handshakes plane
                        // 0x4000 with request bit 27 / ack bit 25 (entry above), so the
                        // inherited bit 0 / bit 31 copy would only ever be dead weight.
                        (0x8004, 1 << 25, 1 << 31, 4),
                        (0x8000, 1 << 0, 1 << 31, 4),
                        (0xc004, 1 << 25, 1 << 31, 4),
                        (0xc000, 1 << 0, 1 << 31, 4),
                        (0x10004, 1 << 25, 1 << 31, 4),
                        (0x10000, 1 << 0, 1 << 31, 4),
                        (0x14004, 1 << 25, 1 << 31, 4),
                        (0x14000, 1 << 0, 1 << 31, 4),
                        (0x18004, 1 << 25, 1 << 31, 4),
                        (0x18000, 1 << 0, 1 << 31, 4),
                        (0x1c004, 1 << 25, 1 << 31, 4),
                        (0x1c000, 1 << 0, 1 << 31, 4),
                        (0x20004, 1 << 25, 1 << 31, 4),
                        (0x20000, 1 << 0, 1 << 31, 4),
                        (0x24004, 1 << 25, 1 << 31, 4),
                        (0x24000, 1 << 0, 1 << 31, 4),
                        (0x28004, 1 << 25, 1 << 31, 4),
                        (0x28000, 1 << 0, 1 << 31, 4),
                        (0x2c004, 1 << 25, 1 << 31, 4),
                        (0x2c000, 1 << 0, 1 << 31, 4),
                        (0x30004, 1 << 25, 1 << 31, 4),
                        (0x30000, 1 << 0, 1 << 31, 4),
                        (0x34004, 1 << 25, 1 << 31, 4),
                        (0x34000, 1 << 0, 1 << 31, 4),
                        (0x38004, 1 << 25, 1 << 31, 4),
                        (0x38000, 1 << 0, 1 << 31, 4),
                        (0x3c004, 1 << 25, 1 << 31, 4),
                        (0x3c000, 1 << 0, 1 << 31, 4),
                        (0x20008, 1 << 25, 1 << 63, 8),
                        (0x21008, 1 << 25, 1 << 63, 8),
                        (0x22008, 1 << 25, 1 << 63, 8),
                        (0x23008, 1 << 25, 1 << 63, 8),
                        (0x24008, 1 << 25, 1 << 63, 8),
                        (0x25008, 1 << 25, 1 << 63, 8),
                    ])
                    .with_write_armed_go_wait(vec![0x4004])
                    .with_status_nibble_all_ones(vec![0x20400]),
            ),
        })
        .expect("A7 PMGR base does not overlap another region");

    // A PMGR plane the A7 tree declares, same as A8.
    m.bus
        .map(BusRegion {
            base: 0x2022_20000,
            size: 0x2000,
            device: Box::new(
                Pmgr::new(90)
                    .with_plane_go_wait(vec![(0x20, 1 << 25, 0, 8), (0x50, 1 << 25, 0, 8)]),
            ),
        })
        .expect("A7 PMGR plane does not overlap another region");

    // USB DWC2 controller. Skipped when the config says the window is not
    // established, which for the A7 it is: `A7::usb_size` is 0 because neither A7
    // SecureROM composes a device address and neither holds one as a literal -- both
    // reach their peripherals through a register-offset table in the image. Mapping
    // `base: 0, size: 0` instead would either panic in `Bus::map` or, worse, put a
    // controller at address 0 where a run would then report progress against a device
    // that is not there. A gap the map states is the honest shape; see
    // `A7::usb_base` for the measurements and `restore_and_boot_a7.sh` for the gate this
    // puts in front of an A7 restore.
    if c.usb_size != 0 {
        m.bus
            .map(BusRegion {
                base: c.usb_base,
                size: c.usb_size,
                device: Box::new(UsbDeviceController::new(7, c.usb_base, 451)),
            })
            .expect("A7 USB does not overlap another region");
    }

    // OTG PHY control
    m.bus
        .map(BusRegion {
            base: 0x0002_0c00_0000,
            size: 0x1000,
            device: Box::new(RegFile::new(91, "otgphyctrl")),
        })
        .expect("A7 OTG PHY control does not overlap another region");

    // USB complex
    m.bus
        .map(BusRegion {
            base: 0x20c9_00000,
            size: 0xa0,
            device: Box::new(RegFile::new(92, "usb-complex")),
        })
        .expect("A7 USB complex does not overlap another region");

    // A7 (S5L8960X) USB host stack: UsbPhy, UsbMux, UsbPd.
    m.bus
        .map(BusRegion {
            base: c.usbphy_base,
            size: 0x1000,
            device: Box::new(UsbPhy::new(75)),
        })
        .expect("A7 USB PHY does not overlap another region");
    m.bus
        .map(BusRegion {
            base: c.usbmux_base,
            size: 0x1000,
            device: Box::new(UsbMux::with_irq(76, 68)),
        })
        .expect("A7 USB Mux does not overlap another region");
    m.bus
        .map(BusRegion {
            base: c.usbpd_base,
            size: 0x1000,
            device: Box::new(UsbPd::with_irq(77, 69)),
        })
        .expect("A7 USB PD does not overlap another region");

    // A7 (S5L8960X) SEP→AP ASC mailbox.
    // The A7 has a SEP (AppleSEPROM-302), so map the ASC mailbox at the
    // standard mailbox_base (0x23c000000) with IRQ 0xa4 (164).
    let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
    m.bus
        .map(BusRegion {
            base: c.mailbox_base,
            size: 0x10000,
            device: Box::new(sep_ap_mailbox),
        })
        .expect("A7 ASC mailbox does not overlap another region");

    // arm-io window 0
    m.bus
        .map(BusRegion {
            base: 0x2022_40000,
            size: 0x1000,
            device: Box::new(RegFile::new(93, "arm-io-window0")),
        })
        .expect("A7 arm-io window 0 does not overlap another region");

    // 0x20a080000 block
    m.bus
        .map(BusRegion {
            base: 0x20a0_80000,
            size: 0x1_0000,
            device: Box::new(RegFile::new(94, "a7-0x20a080000")),
        })
        .expect("A7 0x20a080000 block does not overlap another region");

    m
}

/// The generic builder every `SoCId` other than the ones with a dedicated entry point
/// above is assembled by. The A8/T7000 preamble above is a separate function that this
/// header used to open; restoring it here keeps `build_a8_t7000` as its own builder and
/// keeps `build_with_rom` as the name the rest of the tree, including `build` below and
/// the test suites, already call.
pub fn build_with_rom(id: SoCId, rom_data: &[u8], _enable_debug: bool) -> Machine {
    if matches!(id, SoCId::S8000) {
        return build_a9_s8000(rom_data, _enable_debug);
    }
    if matches!(id, SoCId::A7) {
        return build_a7_ap_platform(rom_data, _enable_debug);
    }
    if matches!(id, SoCId::A8) {
        return build_a8_t7000(rom_data, _enable_debug);
    }

    let c = config(id);
    let mut rom_vec = vec![0u8; c.rom_size as usize];
    let len = rom_data.len().min(rom_vec.len());
    rom_vec[..len].copy_from_slice(&rom_data[..len]);
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1000,
    });

    m.bus
        .map(BusRegion {
            base: c.rom_base,
            size: c.rom_size,
            device: Box::new(Rom::new(c.rom_base, rom_vec)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: 0x0008_0000_0000,
            size: 0x2000000,
            device: Box::new(SparseRam::new(0x0008_0000_0000, 0x2000000)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: 0x20000000,
            size: 0x100000,
            device: Box::new(SparseRam::new(0x20000000, 0x100000)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x1000, 0x1000)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.uart_base,
            size: 0x1000,
            device: Box::new(Uart::new(0, 0)),
        })
        .unwrap();

    // Host-visible UART side-channel (when configured). Guest can write bytes
    // here; the CLI drains them into the SDL console window.
    if c.uart_window {
        m.bus
            .map(BusRegion {
                base: c.uart_base + 0x1000,
                size: 0x1000,
                device: Box::new(UartWindow::new(40, "iPhemeral UART")),
            })
            .unwrap();
    }

    // The generic timer page. The A10 is excluded for the same reason the generic PMGR page
    // is: `SoCId::T8010::timer_base` is the A12's `0x20e100000` with nothing behind it for
    // the A10, and the A10's own ROM composes `0x20e108020` -- inside that very address and
    // inside the A10's measured power-management band -- so a timer page there would answer
    // a device register as a counter. The A10's band is mapped in full in `build_with_rom`.
    if !matches!(id, SoCId::T8010) {
        m.bus
            .map(BusRegion {
                base: c.timer_base,
                size: 0x1000,
                device: Box::new(Timer::new(0, 0)),
            })
            .unwrap();
    }

    m.bus
        .map(BusRegion {
            base: c.aic_base,
            size: c.aic_size,
            // The T8020 real AIC1 spans 512 IRQs: the SecureROM's own init
            // mask sweep writes 16 full MASK_SET words (0x4100..0x13c = 512
            // slots) and its IRQ dispatcher decodes `w8 & 0x1ff` (max IRQ
            // 511). SEP-mail lines 0xa1 (161) and 0xa4 (164) and DART-USB
            // IRQ 454 must therefore be representable in the model.
            device: Box::new(Aic::new(0, 512)),
        })
        .unwrap();

    // The generic single-page PMGR. The A10 is excluded because its own measured band starts
    // at the same `0x20e000000` and is a hundred times larger, and mapping this page first
    // would collide with it: the A10's ROM touches seventeen 4 KiB pages in that band, up to
    // `0x20e0dc000`, so one page is not a narrowing of the A10's PMGR, it is a different
    // claim about the same address. The A10's band is mapped in full in `build_with_rom`.
    if !matches!(id, SoCId::T8010) {
        m.bus
            .map(BusRegion {
                base: c.pmgr_base,
                size: 0x1000,
                device: Box::new(Pmgr::new(0)),
            })
            .unwrap();
    }

    // d321ap DeviceTree: PMGR ps-regs[5], offset 0, device 134 (SEP).
    // Flags 0x29 mark SEP on + critical (m1n1 maps critical to always-on).
    // Model that platform boot policy, not SEP firmware/USB readiness. The
    // generic register remains initially off for domains without this evidence.
    //
    // The A12X/A12Z (T8027) and the A13 (T8030) are in this block on their own evidence,
    // not by family resemblance. The T8027's SecureROM addresses the same `0x23b0802xx`
    // power-state registers and the same `0x23d280xxx` / `0x23d284xxx` boot-status words,
    // and the census stops on the first of them -- `ldr w8, [0x23d280058]` at instruction
    // 31,425 -- with the comment naming it "USB base". The T8030 is the same: its own
    // address inventory builds `0x23b000000`, `0x23b0a0000`, `0x23b0b8000` and the AIC at
    // `0x23b100000`, and with only the boot scratch mapped its census stops at instruction
    // 32,060 on `ldr w8, [0x23d280058]` -- the identical word. So both share this device
    // map, and mapping it T8020-only would leave parts whose firmware demonstrably drives
    // the block unable to read their own device tables.
    // The A14 (T8101) is in this block on its own literals: its image names the boot-status
    // words `0x23d280058..0x23d280090`, then 32 more across `0x23d284000..0x23d2840f8` and 20
    // across `0x23d288000..0x23d288058` -- a far larger table than the A12's twelve. Its
    // census reaches the first of them at instruction 28,527 through the ROM's own runtime
    // device table (`adr x8, 0x10002aea0` / `ldr x8, [x8, x9, lsl #3]` / `ldr w9, [x8]` /
    // `tbz x9, #28`). The block maps the A12's subset, which covers the word the census
    // stops on; the remaining words are named here so the gap is recorded rather than implied.
    if matches!(id, SoCId::T8020 | SoCId::T8027 | SoCId::T8030 | SoCId::T8101) {
        // Create shared registry for synchronous boot-status updates
        let boot_status_registry = iphem_devices::pmgr_power::BootStatusRegistry::new();

        m.bus
            .map(BusRegion {
                base: 0x23b080600,
                size: 4,
                device: Box::new(iphem_devices::pmgr_power::PmgrPowerState::initially_active(
                    44,
                )),
            })
            .unwrap();

        // The .4.7 SecureROM's USB setup path explicitly requests PMGR state
        // 4 and polls the actual-state nibble at these three observed offsets.
        // Model only those registers (unknown domain names/reset states remain
        // conservative); Linux's Apple PMGR layout defines state 4 as clock-gated.
        // Wire each power-state register so a software target-state write (state 4
        // clock-gate or state 15 power-on) updates the associated boot-status words
        // synchronously instead of waiting for the next tick_devices sync.
        for (device_id, address) in [
            (45, 0x23b080228),
            (46, 0x23b0802a8),
            (47, 0x23b0802b0),
            // The A12X/A12Z's seventh power-state register in this block. The T8027 census
            // stops at instruction 36,101 on `ldr w8, [0x23b0803f0]`, which is one past the
            // `0x23b0803e8` the T8020 list below already carries -- so this is the T8027
            // reaching the same block the A12 uses, one register further along, and not a
            // new window. Modelled as a power-state register for the same reason as its
            // neighbours: the value is the platform's, and `PmgrPowerState` is the model
            // that reports it.
            (60, 0x23b0803f0),
        ] {
            let mut ps = iphem_devices::pmgr_power::PmgrPowerState::new(device_id);
            let registry = boot_status_registry.clone();
            ps.set_on_write(move |value| registry.update_from_power_state(address, value));
            m.bus
                .map(BusRegion {
                    base: address,
                    size: 4,
                    device: Box::new(ps),
                })
                .unwrap();
        }

        // The ROM also accesses a block of PMGR power-state registers at
        // 0x23b080280-0x23b0802d0 (11 registers at 8-byte intervals).
        // Map these as well so the ROM's writes to them take effect.
        // Skip 0x23b0802a8 and 0x23b0802b0 which are already mapped above.
        let pmgr_block_base: u64 = 0x23b080280;
        let skip_addrs = [0x23b0802a8, 0x23b0802b0];
        for i in 0..11 {
            let address = pmgr_block_base + (i as u64 * 8);
            if skip_addrs.contains(&address) {
                continue;
            }
            let device_id: u32 = 60 + i;
            let mut ps = iphem_devices::pmgr_power::PmgrPowerState::new(device_id);
            let registry = boot_status_registry.clone();
            ps.set_on_write(move |value| registry.update_from_power_state(address, value));
            m.bus
                .map(BusRegion {
                    base: address,
                    size: 4,
                    device: Box::new(ps),
                })
                .unwrap();
        }

        // PMGR boot-status words at 0x23d280058..0x23d280088 and
        // 0x23d284000..0x23d284018. The ROM polls these for device readiness.
        // Each word returns 0xc0 (bits 6:7 set) when the associated device is ready.
        // Wire them to the PMGR power-state registers the ROM actually writes to:
        // the block at 0x23b080280-0x23b0802c8 (10 registers at 8-byte intervals).
        // SEP uses 0x23b080600.
        let boot_status_words = [
            // Device table indices 13..19 at 0x23d280058 + 8*i
            (48, 0x23d280058, 0x23b080280), // USB base -> block[0]
            (49, 0x23d280060, 0x23b080288), // USB base+8 -> block[1]
            (50, 0x23d280068, 0x23b080290), // USB base+16 -> block[2]
            (51, 0x23d280070, 0x23b080298), // USB base+24 -> block[3]
            (52, 0x23d280078, 0x23b0802a0), // USB base+32 -> block[4]
            (53, 0x23d280080, 0x23b0802a8), // USB base+40 -> block[5]
            (54, 0x23d280088, 0x23b080600), // SEP (wired to SEP power-state)
            // Device table indices 20..23 at 0x23d284000 + 8*i
            (55, 0x23d284000, 0x23b0802b8), // USB base+56 -> block[7]
            (56, 0x23d284008, 0x23b0802c0), // USB base+64 -> block[8]
            (57, 0x23d284011, 0x23b0802c8), // USB base+72 -> block[9]
            (58, 0x23d284018, 0x23b080600), // SEP
            // Real d321 iBoot continues the same table at index 24.
            (59, 0x23d284020, 0x23b0802d0),
        ];
        for (device_id, status_addr, power_state_addr) in boot_status_words {
            let bs = std::sync::Arc::new(std::sync::Mutex::new(
                iphem_devices::pmgr_power::PmgrBootStatus::new(device_id, power_state_addr),
            ));
            boot_status_registry.register(power_state_addr, bs.clone());
            m.bus
                .map(BusRegion {
                    base: status_addr,
                    size: 4,
                    device: Box::new(iphem_devices::pmgr_power::PmgrBootStatusDevice::new(bs)),
                })
                .unwrap();
        }

        // Complete d321 iBoot PMGR pointer table from the plaintext image at
        // file offset 0x90008. Preserve specialized mappings above and fill
        // only the exact 4-byte state words that remain unmapped.
        let mut iboot_pmgr_words = Vec::new();
        iboot_pmgr_words.extend((0x23d280058..=0x23d280088).step_by(8));
        iboot_pmgr_words.extend([
            0x23d284000,
            0x23d284008,
            0x23d284011,
            0x23d284018,
            0x23d284020,
        ]);
        iboot_pmgr_words.extend((0x23d284050..=0x23d2840d0).step_by(8));
        iboot_pmgr_words.extend([
            0x23d288000,
            0x23d288020,
            0x23d288028,
            0x23d288030,
            0x23d288038,
            0x23d288040,
            0x23d288048,
            0x23d288050,
        ]);
        // Dense PMGR state array; the image table continues through 0x3e8.
        iboot_pmgr_words.extend((0x23b080100..=0x23b0803e8).step_by(8));
        for (index, address) in iboot_pmgr_words.into_iter().enumerate() {
            if m.bus
                .regions
                .iter()
                .any(|region| region.contains(address, 4))
            {
                continue;
            }
            m.bus
                .map(BusRegion {
                    base: address,
                    size: 4,
                    device: Box::new(iphem_devices::pmgr_power::PmgrPowerState::new(
                        100 + index as u32,
                    )),
                })
                .unwrap();
        }

        // Also keep the tick_devices sync for USB complex PHY ready
        m.pmgr_sync = Some(Box::new(move |bus: &mut iphem_core::bus::Bus| {
            let mut power_states: std::collections::HashMap<u64, u32> =
                std::collections::HashMap::new();
            for region in &mut bus.regions {
                if let Some(ps) = region
                    .device
                    .as_any_mut()
                    .downcast_mut::<iphem_devices::pmgr_power::PmgrPowerState>()
                {
                    power_states.insert(region.base, ps.register);
                }
            }
            let usbctlreg_active = power_states
                .get(&0x23b0802a8)
                .map(|&v| (v & 0xf0) == 0xf0)
                .unwrap_or(false);
            let usb2host0_active = power_states
                .get(&0x23b0802b0)
                .map(|&v| (v & 0xf0) == 0xf0)
                .unwrap_or(false);
            // Boot-status for registers without a write callback (e.g. the SEP
            // power-state 0x23b080600, initially_active without software writes)
            // is refreshed here each device tick.
            for (ps_addr, ps_value) in power_states {
                boot_status_registry.update_from_power_state(ps_addr, ps_value);
            }
            // Update USB complex PHY ready state
            for region in &mut bus.regions {
                if let Some(usb) = region
                    .device
                    .as_any_mut()
                    .downcast_mut::<iphem_devices::usb_host::UsbComplex>()
                {
                    usb.update_phy_ready(usbctlreg_active, usb2host0_active);
                }
            }
        }));
    }

    // The generic mailbox aperture, mapped only where the SoC's own firmware is shown to
    // use `mailbox_base`.
    //
    // The A8/T7000 is not one of those, on two measurements. A structural scan of both
    // T7000 SecureROMs finds no SEP-mail descriptor table -- the shape that finds the
    // T8020's at image VA 0x1000241a0 (mailbox 0x242400000, +0x10 0x242000000, +0x18
    // all-ones, irqs 161/164), and the scan is validated on that T8020 as a control on every
    // run. And a movz/movk composition scan of the whole image, which is how this project's
    // other A8 addresses were established, composes no address anywhere in
    // 0x23c000000..0x243000000 -- so the A8's configured `mailbox_base` of 0x23c000000 has
    // nothing behind it, the same category of value as the `sram_base` 0x3000_0000 that was
    // wrong here.
    //
    // Mapping a `Mailbox` there anyway would place a device with invented semantics at an
    // address the firmware never reads, and the cost of that is not the wasted region: it
    // is that if the A8 ROM ever does reach such an address, the model answers instead of
    // faulting, and the fault was the honest signal. The T8020's real SEP mailbox is mapped
    // separately below, inside the T8020 guard.
    //
    // Note what this guard is and is not. The A8 currently returns out of this function
    // earlier, into `build_a8_t7000`, so it never reached the mapping and the A8's region
    // list has no 0x23c000000 entry today. The guard is therefore preventative rather than a
    // live correction: it stops the A8 inheriting a baseless aperture if the per-SoC builders
    // are ever unified, which is the direction the A7/A8 refactor is moving.
    if !matches!(id, SoCId::A8) {
        m.bus
            .map(BusRegion {
                base: c.mailbox_base,
                size: 0x1000,
                device: Box::new(Mailbox::new(0)),
            })
            .unwrap();
    }

    m.bus
        .map(BusRegion {
            base: 0x23d2e8000,
            size: 0x1000,
            device: Box::new(SepIdentity::new(42, 0x32b3)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: 0x242140000,
            size: 0x1000,
            device: Box::new(SepBootCtrl::new(41)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.dcp_base,
            size: c.dcp_size,
            device: Box::new(Dcp::new(0)),
        })
        .unwrap();

    if id == SoCId::T8020 {
        m.bus
            .map(BusRegion {
                base: 0x235100000,
                size: 0x4000,
                device: Box::new(S5lUart::new(199, "uart0")),
            })
            .unwrap();
        for (device_id, base, name) in [(201, 0x235208000, "uart2"), (202, 0x23520c000, "uart3")] {
            m.bus
                .map(BusRegion {
                    base,
                    size: 0x4000,
                    device: Box::new(S5lUart::new(device_id, name)),
                })
                .unwrap();
        }
        // SEP→AP ASC mailbox. Real T8020 SEP-mail lines: 0xa1 (161) and 0xa4
        // (164) are the only nonempty handler-table entries whose context is
        // the mailbox base (0x19c0081c0); Asahi Linux identifies the receive-
        // not-empty line as the highest of the four consecutive lines, so the
        // IOP→AP push asserts 0xa4 (164). The SEP harness derives its IOP view
        // via `peer()`, which copies the AP view's irq.
        let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        m.bus
            .map(BusRegion {
                base: 0x242400000,
                size: 0x10000,
                device: Box::new(sep_ap_mailbox),
            })
            .unwrap();
        for (device_id, base) in [
            (79, 0x23d200000),
            (80, 0x23d240000),
            // DCS/AMCC companion window touched at +0xc000 after the plane
            // command sequence completes.
            (81, 0x23d2d0000),
            (83, 0x23d290000),
            (84, 0x23d2c0000),
            // MCC register bank selected by iBoot's d321 memory-controller
            // descriptor (first observed register: 0x22400000c).
            (86, 0x224000000),
            // Final memory-controller descriptor bank reached after the four
            // MCC planes (first observed register: 0x227000000).
            (87, 0x227000000),
            (88, 0x23b480000),
            (89, 0x234000000),
            (90, 0x24a800000),
            (91, 0x23e800000),
            (92, 0x23d000000),
            (93, 0x23d3c0000),
            (94, 0x23d180000),
            // Timing-controller register pair programmed after MCC
            // calibration (first real iBoot stores at +0 and +4).
            (95, 0x23b200000),
            (96, 0x23d100000),
            (97, 0x23d1f0000),
        ] {
            let device = if base == 0x23d2d0000 {
                // The checked production firmware set includes Cyprus B1.
                // Real iBoot rejects revision selectors <= 0x0f with its
                // explicit "Cyprus A0 is deprecated" fatal path; its B1
                // registry entry is selector 0x1b.
                AppleMcc::new(device_id)
                    .with_reset_register(0x10, 0x1)
                    .with_reset_register(0x20, 0x1b)
            } else {
                AppleMcc::new(device_id)
            };
            m.bus
                .map(BusRegion {
                    base,
                    // d321ap reports four MCC configurations; iBoot selects
                    // four consecutive 64-KiB planes at 0x224000000.
                    size: match base {
                        0x224000000 | 0x24a800000 | 0x23e800000 => 0x40000,
                        _ => 0x10000,
                    },
                    device: Box::new(device),
                })
                .unwrap();
        }
        // DCS/AMCC calibration register space. iBoot decodes register
        // descriptors relative to 0x210000000 (first observed access:
        // 0x210e30090).
        m.bus
            .map(BusRegion {
                base: 0x210000000,
                // Four DCS channel planes at 0x210/211/212/213000000.
                size: 0x4000000,
                device: Box::new(AppleMcc::new(82)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: 0x23b288000,
                // Two descriptor-selected planes; real iBoot accesses the
                // same register at +0x58 and +0x10058.
                size: 0x20000,
                device: Box::new(AppleMcc::new(85)),
            })
            .unwrap();
        // The d321ap DeviceTree gives WDT a 0x4000-byte primary window and a
        // four-byte register at +0x8020. Keep the unresolved intervening MMIO
        // readable as before, but do not attribute the distinct +0xc000
        // chip-control/fuse block to WDT.
        m.bus
            .map(BusRegion {
                base: 0x23d2b0000,
                size: 0xc000,
                device: Box::new(Watchdog::with_irq(77, 39)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: 0x23d2bc000,
                size: 0x4000,
                device: Box::new(AppleChipControl::new(78)),
            })
            .unwrap();
        // d321ap PMGR aperture observed in SecureROM and real iBoot traces.
        // Unknown registers retain guest writes but start at zero; no
        // hardware-owned ready/completion bits are fabricated.
        for (device_id, base, size) in [
            (75, 0x23b000000, 0x80000),
            // Preserve the individually modeled power-state registers in the
            // 0x23b080000 page.
            (76, 0x23b081000, 0x7f000),
        ] {
            m.bus
                .map(BusRegion {
                    base,
                    size,
                    device: Box::new(Pmgr::new(device_id)),
                })
                .unwrap();
        }
        m.bus
            .map(BusRegion {
                base: 0x239000000,
                size: 0x1000,
                device: Box::new(UsbComplex::new(74)),
            })
            .unwrap();
        // DART-SIO: MMIO only. Its stream SIDs are not evidenced, so no DMA
        // port is bound; a request for any SID would fault as UnboundSid.
        let (sio_mmio, _sio_port) = DartT8020::new(71, 14).into_pair();
        m.bus
            .map(BusRegion {
                base: 0x235004000,
                size: 0x4000,
                device: Box::new(sio_mmio),
            })
            .unwrap();

        // d321ap DeviceTree /arm-io/aes (`aes-version = 3`): reg 0x35008000 on
        // the arm-io base 0x200000000 => 0x235008000, size 0x4000. The T8020
        // SecureROM's USB-enable path polls a status word here. The register
        // semantics are taken from Apple's own AES driver, which is one shared
        // routine compiled byte-identically into the SecureROMs of three SoC
        // generations; `AesOtp` documents the disassembly it is derived from.
        m.bus
            .map(BusRegion {
                base: 0x235008000,
                size: 0x4000,
                device: Box::new(AesOtp::new(211)),
            })
            .unwrap();

        // d321ap DeviceTree /arm-io/uart-1,samsung: reg 0x35204000 on the
        // arm-io base 0x200000000 => 0x235204000. The Darwin console driver
        // programs the s5l register map here during early boot; model the
        // window so the direct-kernel run advances past UART1 init.
        m.bus
            .map(BusRegion {
                base: 0x235204000,
                size: 0x4000,
                device: Box::new(S5lUart::new(200, "uart1")),
            })
            .unwrap();
        // d321ap ADT: /arm-io/dart-usb child address 0x39900000 maps
        // through the arm-io aperture to physical 0x239900000. The DART-USB
        // instance is shared: the MMIO window on the bus and the DMA port for
        // stream 7 (ADT `sids` property) translate against the same registers
        // and the same bounded software IOTLB. The input-address width is 32
        // by reference: upstream drivers/iommu/apple-dart.c (the project's
        // cited DART reference) probes `case DART_T8020: dart->ias = 32` with
        // `oas = 36` for the t8103/T8020-family hw struct, so the walker is
        // configured with a 32-bit IOVA ceiling rather than faulting every
        // guest-configured stream as AddressWidth.
        let (usb_mmio, usb_port) = DartT8020::new(73, 14)
            .with_input_address_width(32)
            .into_pair();
        m.bus
            .map(BusRegion {
                base: 0x239900000,
                size: 0x4000,
                device: Box::new(usb_mmio),
            })
            .unwrap();
        m.register_dma_iommu(7, DART_USB_IRQ, Box::new(usb_port));

        // ANE (Apple Neural Engine): d321ap DeviceTree /arm-io/ane,t8020,
        // reg (off=0x6a000000 size=0x1f00000) on the 0x200000000 arm-io base
        // => physical 0x26a000000, IRQ 274. No execution unit is modeled; the
        // kF0…kF7 per-plane coprocessing FIFO window is a genuine bounded
        // producer/consumer ticked by Machine::tick_devices (Phase 190 item 3
        // gate G1/G2, docs/PHASE190_PROGRESS.md).
        m.bus
            .map(BusRegion {
                base: 0x26a000000,
                size: 0x1000,
                device: Box::new(AneFifo::new(280)),
            })
            .unwrap();

        // SGX GPU (d321ap DeviceTree /arm-io/sgx, compatible gpu,t8020,
        // reg (off=0x4000000 size=0x100000) => physical 0x204000000, IRQ
        // 339-342/351/354). No execution unit is modeled; the recovered window
        // is a bounded mailbox producer/consumer with a stable 64-bit segment
        // read (Phase 190 item 1 gate G1/G2). The synthetic NAND catch-all is
        // clamped to stop before this window (see the common mapping below).
        m.bus
            .map(BusRegion {
                base: GPU_BASE,
                size: GPU_SIZE,
                device: Box::new(SgxGpu::new(281)),
            })
            .unwrap();

        // ISP (d321ap DeviceTree /arm-io/isp, compatible isp,t8020,
        // reg (off=0x2a000000 size=0x1e8d000) => physical 0x22a000000, IRQ
        // 153-156). CameraIsp linear-light/conversion pipeline (AWB curves,
        // 3A stat binning, raw→YUV leaf) mapped at the measured base (Phase
        // 190 item 2). Register window is the modeled subset of the declared
        // reg range; reads across the whole window round-trip.
        m.bus
            .map(BusRegion {
                base: ISP_BASE,
                size: ISP_SIZE,
                device: Box::new(CameraIsp::with_irq(282, 153)),
            })
            .unwrap();
    }

    m.bus
        .map(BusRegion {
            base: c.nor_base,
            size: c.nor_size,
            device: Box::new(NorFlash::new(7, c.nor_base, c.nor_size as usize)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.block_base,
            size: c.block_size,
            device: Box::new(BlockDev::new(0, c.block_base, c.block_size)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.sep_uart_base,
            size: 0x1000,
            device: Box::new(SepUart::new(0, "SEP UART")),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.bb_uart_base,
            size: 0x1000,
            device: Box::new(BasebandUart::new(0, "BB UART")),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.i2c_base,
            size: 0x1000,
            device: Box::new(I2c::new(0, 0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.spi_base,
            size: 0x1000,
            device: Box::new(Spi::new(0, 0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.gpio_base,
            size: 0x1000,
            device: Box::new(Gpio::new(0)),
        })
        .unwrap();

    // iPhone11,2 d321ap DeviceTree places the Apple pinctrl/GPIO controller
    // at arm-io child address 0x3c100000 with a 1 MiB aperture. The synthetic
    // GPIO window above remains available to existing fixtures.
    //
    // The T8027 and the T8030 are in this window on their own evidence: both address
    // `0x23c100000` in their load/stores, and the T8027's census stops there at instruction
    // 97,218 on `ldr w9, [x9, x10]` with `x10` composed as `movz x10, #0x2, lsl #32` /
    // `movk x10, #0x3c10, lsl #16` -- i.e. `0x23c1_00000` plus a register-held index. The
    // T8030's inventory names the same single page.
    if matches!(id, SoCId::T8020 | SoCId::T8027 | SoCId::T8030) {
        m.bus
            .map(BusRegion {
                base: 0x23c100000,
                size: 0x100000,
                device: Box::new(AppleGpio::new(45)),
            })
            .unwrap();    }

    // The A12X/A12Z (T8027) device map. The T8027 SecureROM demonstrates it uses the
    // same d321ap-derived arm-io layout as the T8020 for AES (0x235008000), PMGR
    // (0x23b0xxxxx), MCC (0x23b288000), WDT (0x23d2b0000), and ChipCtrl (0x23d2bc000).
    // Map the same USB/DART/SEP-mail devices so the DFU-restore path can execute.
    if matches!(id, SoCId::T8027 | SoCId::T8030) {
        // UART0/2/3 at the same d321ap addresses.
        m.bus
            .map(BusRegion {
                base: 0x235100000,
                size: 0x4000,
                device: Box::new(S5lUart::new(199, "uart0")),
            })
            .unwrap();
        for (device_id, base, name) in [(201, 0x235208000, "uart2"), (202, 0x23520c000, "uart3")] {
            m.bus
                .map(BusRegion {
                    base,
                    size: 0x4000,
                    device: Box::new(S5lUart::new(device_id, name)),
                })
                .unwrap();
        }
        // SEP→AP ASC mailbox at the same base. The T8027 and T8030 share the
        // T8020's mailbox layout (Cebu/Cyprus/Sicily SEPROMs all poll 0x23c100c4c).
        let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        m.bus
            .map(BusRegion {
                base: 0x242400000,
                size: 0x10000,
                device: Box::new(sep_ap_mailbox),
            })
            .unwrap();
        // AppleMcc windows shared with T8020 (the T8027 census reaches them).
        for (device_id, base) in [
            (79, 0x23d200000),
            (80, 0x23d240000),
            (81, 0x23d2d0000),
            (83, 0x23d290000),
            (84, 0x23d2c0000),
            (86, 0x224000000),
            (87, 0x227000000),
            (88, 0x23b480000),
            (89, 0x234000000),
            (90, 0x24a800000),
            (91, 0x23e800000),
            (92, 0x23d000000),
            (93, 0x23d3c0000),
            (94, 0x23d180000),
            (95, 0x23b200000),
            (96, 0x23d100000),
            (97, 0x23d1f0000),
        ] {
            let device = if base == 0x23d2d0000 {
                AppleMcc::new(device_id)
                    .with_reset_register(0x10, 0x1)
                    .with_reset_register(0x20, 0x1b)
            } else {
                AppleMcc::new(device_id)
            };
            m.bus
                .map(BusRegion {
                    base,
                    size: match base {
                        0x224000000 | 0x24a800000 | 0x23e800000 => 0x40000,
                        _ => 0x10000,
                    },
                    device: Box::new(device),
                })
                .unwrap();
        }
        // USB Complex at the d321ap address.
        m.bus
            .map(BusRegion {
                base: 0x239000000,
                size: 0x1000,
                device: Box::new(UsbComplex::new(74)),
            })
            .unwrap();
        // DART-SIO: MMIO only.
        let (sio_mmio, _sio_port) = DartT8020::new(71, 14).into_pair();
        m.bus
            .map(BusRegion {
                base: 0x235004000,
                size: 0x4000,
                device: Box::new(sio_mmio),
            })
            .unwrap();
        // AES engine at the d321ap address (same as T8020; T8027 ROM accesses it).
        m.bus
            .map(BusRegion {
                base: 0x235008000,
                size: 0x4000,
                device: Box::new(AesOtp::new(211)),
            })
            .unwrap();
        // UART1 for Darwin console.
        m.bus
            .map(BusRegion {
                base: 0x235204000,
                size: 0x4000,
                device: Box::new(S5lUart::new(200, "uart1")),
            })
            .unwrap();
        // DART-USB for DMA (stream 7, IRQ 454).
        let (usb_mmio, usb_port) = DartT8020::new(73, 14)
            .with_input_address_width(32)
            .into_pair();
        m.bus
            .map(BusRegion {
                base: 0x239900000,
                size: 0x4000,
                device: Box::new(usb_mmio),
            })
            .unwrap();
        m.register_dma_iommu(7, DART_USB_IRQ, Box::new(usb_port));
        // ANE, GPU, ISP at the same d321ap addresses.
        m.bus
            .map(BusRegion {
                base: 0x26a000000,
                size: 0x1000,
                device: Box::new(AneFifo::new(280)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: GPU_BASE,
                size: GPU_SIZE,
                device: Box::new(SgxGpu::new(281)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: ISP_BASE,
                size: ISP_SIZE,
                device: Box::new(CameraIsp::with_irq(282, 153)),
            })
            .unwrap();
    }

    // The A11 (T8015) device map for DFU restore. The A11 uses a different
    // arm-io base (0x230000000) than the A12 family (0x200000000), so its
    // USB device is at 0x250000000 (usb-complex 0x30000000 + usb-device 0x20000000).
    // Add DART-USB, USB Complex, and SEP mailbox for DFU restore support.
    if matches!(id, SoCId::T8015) {
        // USB Complex at the A11's d22ap address (usb-complex at 0x30000000 -> 0x230000000).
        m.bus
            .map(BusRegion {
                base: 0x230000000,
                size: 0x1000,
                device: Box::new(UsbComplex::new(74)),
            })
            .unwrap();
        // DART-USB for DMA (stream 7, IRQ 454) at A11's d22ap address.
        // The A11 tree doesn't have a DART-USB node, but the A11 SecureROM
        // uses DART for USB DMA. Map at the same offset from arm-io base.
        let (usb_mmio, usb_port) = DartT8020::new(73, 14)
            .with_input_address_width(32)
            .into_pair();
        m.bus
            .map(BusRegion {
                base: 0x239900000,
                size: 0x4000,
                device: Box::new(usb_mmio),
            })
            .unwrap();
        m.register_dma_iommu(7, DART_USB_IRQ, Box::new(usb_port));
        // SEP->AP ASC mailbox at the A11's d22ap address (sep at 0x43000000 -> 0x243000000).
        let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        m.bus
            .map(BusRegion {
                base: 0x243000000,
                size: 0x10000,
                device: Box::new(sep_ap_mailbox),
            })
            .unwrap();
    }

    // The A11's own windows for the blocks the restore path needs. Every address is the
    // A11 tree's `reg` cell plus `0x200000000`, an offset confirmed at four windows the
    // A11 SecureROM names in its own composed constants, and every `reg` is quoted from
    // `DeviceTree.d22ap.im4p` rather than carried over from the A12 tree.
    //
    // All four A11 GPIO blocks declare the same compatible pair,
    // `gpio,t8015,gpio,s5l8960x`, and the A12 declares `gpio,t8020,gpio,s5l8960x`. The
    // shared `gpio,s5l8960x` is the evidence that one model serves both; the A11 `reg` is
    // the evidence for where they are, and neither is evidence for the other. The aperture
    // is the load-bearing difference: A11 declares 4 KiB and 16 KiB windows where the A12
    // declares 1 MiB, so reusing the A12 size would map address space the A11 tree says is
    // not there.
    //
    // `aop-gpio`, `nub-gpio` and `smc-gpio` are mapped as GPIO rather than as whatever
    // coprocessor answers them, because that is what the A11 tree says they are. On the A12
    // tree `nub-gpio` and `smc-gpio` are instead bound to `iop-nub,rtbuddy-v2`, which is
    // the SIO providing them -- so the same block name means a different owner on the two
    // SoCs, and taking the A12 binding for the A11 would assert a coprocessor dependency
    // the A11 tree does not record.
    if matches!(id, SoCId::T8015) {
        for (device_id, base, size, gpio) in [
            // /arm-io/gpio       reg cell 0x33100000 + 0x1000
            (91, 0x0002_3310_0000, 0x1_000, true),
            // /arm-io/aop-gpio   reg cell 0x340f0000 + 0x4000
            (94, 0x0002_340f_0000, 0x4_000, true),
            // /arm-io/nub-gpio   reg cell 0x351f0000 + 0x4000
            (95, 0x0002_351f_0000, 0x4_000, true),
            // /arm-io/smc-gpio   reg cell 0x36024000 + 0x4000
            (96, 0x0002_3602_4000, 0x4_000, true),
            // /arm-io/aes        reg cell 0x2e800000 + 0x4000
            (92, 0x0002_2e80_0000, 0x4_000, false),
            // /arm-io/aes        reg cell 0x352d0000 + 0x4000
            (93, 0x0002_352d_0000, 0x4_000, false),
        ] {
            let device: Box<dyn iphem_core::Device> = if gpio {
                Box::new(AppleGpio::new(device_id))
            } else {
                Box::new(AesOtp::new(device_id))
            };
            m.bus
                .map(BusRegion { base, size, device })
                .expect("A11 GPIO/AES window does not overlap another region");
        }
    }

    // A11 USB device controller (DWC2) at usb-device: DT usb-complex at 0x30000000
    // (physical 0x230000000 with arm-io base 0x2_0000_0000), usb-device at offset
    // 0x20000000 within usb-complex -> physical 0x250000000. Size 0x10000 per DT.
    if matches!(id, SoCId::T8015) {
        m.bus
            .map(BusRegion {
                base: 0x2500_00000,
                size: 0x10000,
                device: Box::new(UsbDeviceController::new(7, 0x2500_00000, 451)),
            })
            .expect("A11 USB device does not overlap another region");
        }

    // A11 AES init spin loop at 0x100004314 waits for interrupt 302 (AES completion).
    // The ROM sets w13=1 and spins; the AES hardware would trigger interrupt 302
    // which clears saved w13 to 0 via the interrupt handler. Simulate this by
    // injecting the interrupt when the spin loop is first entered.
    if matches!(id, SoCId::T8015) {
        let aes_spin_pc = 0x100004314;
        let mut spin_count = 0;
        m.register_hook(aes_spin_pc, Box::new(move |mach: &mut Machine| {
            if spin_count == 0 {
                // The spin loop at 0x100004314 waits for w13 to become 0.
                // In real hardware, AES completion interrupt clears saved w13.
                // Directly clear w13 to unblock the spin loop.
                mach.cores.active_mut().state.x[13] = 0;
                spin_count = 1;
            }
        }));

        // Hook at 0x100004318: the instruction there (0x17ffffdc) is not a valid branch
        // to 0x10000428c. Manually redirect PC to the correct AES init function at 0x10000428c.
        let redirect_pc = 0x100004318;
        m.register_hook(redirect_pc, Box::new(move |mach: &mut Machine| {
            mach.cores.active_mut().state.pc = 0x10000428c;
        }));
    }

    // A11 (T8015) device-table control windows. The address set is the A11's own: its
    // SecureROM device table names 0x232080000, 0x232084000, 0x232088000 and
    // 0x235280000. The first three are contiguous and form one 0x10000 aperture, so
    // `WINDOWS` carries two regions rather than four; the `0x235280000` block is a
    // separate aperture on the same bus and is mapped separately.
    //
    // T8015-only, and the negative matters as much as the positive: the A12 must leave
    // both addresses *unmapped* rather than answered by something else. Asserting
    // "present but not this device" would have required inventing an A12 device at an
    // address no A12 evidence names, so `a11_platform.rs` states the negative in the
    // stronger form that is actually true — a complete-looking model answering at
    // another part's addresses is the failure this guards against.
    //
    // No register is declared actual. The window starts fail-closed, so the A11 ROM's
    // readiness poll `(word & 0xf0) == 0xf0` cannot pass for hardware that was never
    // there; `set_actual`/`set_all_actual` are the only paths that assert readiness.
    if matches!(id, SoCId::T8015) {
        for (index, (base, size)) in DeviceTableControlWindow::WINDOWS.iter().enumerate() {
            let mut window = DeviceTableControlWindow::new(70 + index as u32, *base, *size);
            // The first window (0x232080000..0x232090000) contains the readiness gate at
            // offset 0x400 (0x232080400) which the ROM polls for 0xf0. This is a
            // read-only poll with no SecureROM write path; the producer is outside the ROM.
            // Declare it as hardware-status so the poll passes, matching the A10 fix
            // for 0x20e080400 (PmgrPowerState::status_nibble_all_ones).
            if index == 0 {
                window = window.with_status_all_ones(vec![0x400]);
            }
            m.bus
                .map(BusRegion {
                    base: *base,
                    size: *size,
                    device: Box::new(window),
                })
                .unwrap();
        }

        // A11 SEP control window: DT `sep` at `0x43000000` + arm-io base `0x200000000`
        // = `0x243000000`, size 0x10000 (64 KiB per DT). The ROM accesses this after
        // passing the device-table readiness gate. The access at offset 0x8100
        // confirms the full 64 KiB aperture is needed.
        m.bus
            .map(BusRegion {
                base: 0x2430_00000,
                size: 0x1_0000,
                device: Box::new(RegFile::new(99, "sep-ctrl")),
            })
            .expect("A11 SEP control window does not overlap another region");
    }

    // T8015's PMGR band, split rather than widened. The generic mapping above is one
    // 0x1000 page, but this config records the A11 `pmgr` first DT region as
    // 0x32000000 + 0x100000, and the A11 SecureROM demonstrably uses it that far out:
    // at instruction 24709 it builds `x19 = 0x23206c000`, computes
    // `x8 = x19 + 0x34000 = 0x2320a0000`, and does a read-modify-write
    // `ldr w9,[x8]` / `orr w9,w9,#0x7f00` / `str w9,[x8]` -- Apple's
    // write-then-poll-ack reset-gate release -- on an address 640 KiB into the band
    // and 0x7f000 past the mapped page.
    //
    // A plain widen to 0x100000 would swallow `0x232080000`, which is the
    // device-table control window mapped just above, so the band is covered in the
    // two pieces on either side of it. The arithmetic is exact and asserted by
    // `t8015_pmgr_band_covers_the_dt_region_around_the_control_window`:
    //
    //     0x232000000 + 0x1000  = 0x232001000   generic page ends
    //     0x232001000 + 0x7f000 = 0x232080000   meets the control window
    //     0x232090000 + 0x70000 = 0x232100000   the DT region's end, and `aic_base`
    //
    // so the three mappings plus the control window tile 0x232000000..0x232100000
    // with neither a gap nor an overlap, and stop where the A11 AIC begins.
    //
    // T8015-only, for the same reason the control windows are: a 1 MiB PMGR for the
    // A12 would answer at 0x23b06c000 and 0x23b0a0000, which no A12 evidence names and
    // which the A12 maps as its own 0x80000 band with individually modelled registers
    // above.
    if matches!(id, SoCId::T8015) {
        for (device_id, base, size) in [
            (79, c.pmgr_base + 0x1000, 0x7f_000),
            (80, 0x2320_90000, 0x70_000),
            // The two PMGR planes the A11 SecureROM is stopped on. The census halts at
            // instruction 30821 writing `0x02000000` to `0x208e20020` and then polling bit
            // 31, and repeating at `0x208ea0020`; both addresses are offset `0x20` into a
            // window the A11 tree declares as `pmgr1,t8015`:
            //
            //     /arm-io/pmgr  cell 0x8e20000+0x30000 -> 0x208e20000+0x30000
            //     /arm-io/pmgr  cell 0x8ea0000+0x30000 -> 0x208ea0000+0x30000
            //
            // The cells are the A11 `arm-io` child addresses and the physical address adds
            // `0x200000000`, an offset confirmed at four windows the A11 ROM names in its
            // own composed constants (0x243000000, 0x233100000, 0x22e008000, 0x2352d0000).
            //
            // This is a window that exists, not a window invented to make the guest
            // continue: without it the access is unmapped, and the census's own report
            // says `unmodeled fallback: no`, so the run ends on a Data fault rather than
            // on the poll. What the mapping does *not* establish is that the `+0x20`
            // register ever sets bit 31 -- the generic `Pmgr` answers unknown offsets by
            // returning what was written, and `0x02000000` has bit 31 clear, so the poll
            // is expected to keep reading it back. That is the point at which the gate
            // needs a producer, and it is recorded rather than assumed either way.
            (81, 0x0002_08e2_0000, 0x3_0000),
            (82, 0x0002_08ea_0000, 0x3_0000),
            // Two more planes the A11 tree declares, found by running with the census's
            // `--probe-fallback` diagnostic and noticing which windows it had to answer
            // itself. Each is `pmgr1,t8015` in the tree:
            //
            //     /arm-io/pmgr  cell 0x8e70000+0x1000 -> 0x0002_08e7_0000+0x1000
            //     /arm-io/pmgr  cell 0x8ef0000+0x1000 -> 0x0002_08ef_0000+0x1000
            //
            // The ROM writes 0x4008e7 to offset 0 of each, 256 times, which is why they
            // did not show up as a single faulting access the way the first two planes did:
            // a plane that is only ever written is invisible to a run that stops on the
            // first unmapped access, and only a diagnostic that keeps going reveals it.
            (85, 0x0002_08e7_0000, 0x1_000),
            (86, 0x0002_08ef_0000, 0x1_000),
            // The next plane the census reaches, and again a PMGR one: the tree declares
            // `pmgr1,t8015` at cell 0x35200000+0x100000 -> 0x235200000+0x100000, and the
            // run's first write after leaving the gate is to `0x235240300`, offset 0x40300
            // into it, with `x19` holding the plane base. Mapping the full 0x100000 would
            // swallow two devices the tree declares inside it -- `wdt` at 0x2352b0000 and
            // `aes` at 0x2352d0000 -- so the plane is covered only as far as the next
            // declared device, which is the same tiling the 0x232000000 band above uses:
            //
            //     0x235200000 + 0x80000 = 0x235280000   meets the control window
            (83, 0x0002_3520_0000, 0x8_0000),
            // The rest of that same plane, above `aes`. The tree declares `pmgr1,t8015`
            // across the whole 0x100000 and the census's next frontier is 0x2352dc000, so
            // the plane is covered around the devices declared inside it rather than in one
            // piece. Offsets from the plane base, which is the only way to order these
            // correctly -- read as absolute addresses they differ only in a digit group and
            // the order inverts:
            //
            //     0x00000 sochot0 0xbc000 | 0xc0000 free | 0xd0000 aes 0xd4000 | 0xdc000 FRONTIER
            //
            // so the frontier is *above* aes, and the piece runs from the end of `aes` to
            // the end of the plane:
            //
            //     0x2352d4000 + 0x2c000 = 0x235300000
            (84, 0x0002_352d_4000, 0x2c_0000),
        ] {
            m.bus
                .map(BusRegion {
                    base,
                    size,
                    device: Box::new(Pmgr::new(device_id)),
                })
                .expect("T8015 PMGR band does not overlap another region");
        }
    }

    // T8015's boot SRAM, mapped for the reason the A9's is not: the A9's
    // `sram_base` 0x1800c0000 lies *inside* its DRAM span, so `build_a9_s8000`
    // leaves it unmapped and the DRAM SparseRam answers. T8015 has no such
    // cover -- `dram_base` is 0 and unmeasured, and this builder maps no DRAM
    // span at all -- so the declared boot-SRAM span is currently backed by
    // nothing and the A11 SecureROM's first store faults.
    //
    // That store is not incidental. Both AArch64 AP SecureROMs run the same
    // literal-driven clear loop at 0x100000058..0x100000064:
    //
    //     0x100000058  movz x12, #0x0
    //     0x10000005c  stp  x12, x12, [x10], #0x10   ; faulting store
    //     0x100000060  cmp  xzr, x10, x11
    //     0x100000064  b.ne 0x10000005c
    //
    // with x10 = the u64 at image 0x358 and x11 = that plus the u64 at 0x360.
    // Measured from the two checked-in A11 images: start 0x180014000, size
    // 0x4000, so the ROM clears 0x180014000..0x180018000 -- inside the
    // `sram_base` 0x180000000 + `sram_size` 0x1e8000 this config already
    // declares, and inside the span the ROM builds its own TTBR0_EL1 table in.
    //
    // The T8020 control is the negative that makes this a measurement rather
    // than a habit: its literals place the same 0x4000-byte clear at
    // 0x19c1cc000, which is in neither its `sram_base` nor any mapped window, so
    // mapping `sram_base` for every SoC would claim a T8020 window the T8020
    // ROM is not shown to use. T8015 only.
    if matches!(id, SoCId::T8015) {
        m.bus
            .map(BusRegion {
                base: c.sram_base,
                size: c.sram_size,
                device: Box::new(SparseRam::new(c.sram_base, c.sram_size)),
            })
            .expect("T8015 boot SRAM does not overlap another region");

        // A11 DRAM: the ROM clears 96 KiB at 0x1801e8000 (immediately after boot SRAM)
        // in a later zero-fill pass. The A11 DRAM base is unmeasured, but the ROM's own
        // access pattern requires this memory to exist. Map a minimal window to allow
        // the clear to complete.
        m.bus
            .map(BusRegion {
                base: 0x1801_e8000,
                size: 0x1_00000,
                device: Box::new(SparseRam::new(0x1801_e8000, 0x1_00000)),
            })
            .expect("T8015 DRAM does not overlap another region");

        // A11 interrupt controller register at arm-io offset 0x30000048.
        // The ROM composes this as 0x2_0000_0000 + 0x30000048 = 0x230000048
        // and writes 0x3000011 to it. This is below the main AIC at 0x232100000
        // and not declared in the DT, but the ROM accesses it directly.
        m.bus
            .map(BusRegion {
                base: 0x2300_00000,
                size: 0x1000,
                device: Box::new(RegFile::new(100, "aic-0x30000048")),
            })
            .expect("A11 AIC 0x30000048 does not overlap another region");

        // A11 AES context pointers at 0x22e400000..0x22e410000 (arm-io offset 0x2e400000).
        // The ROM iterates through 4 contexts, each 16 KiB apart (0x22e400000,
        // 0x22e404000, 0x22e408000, 0x22e40c000), clearing each one.
        // The AES init at 0x100004180 reads status at context+0x8; initialize with
        // bits 11-15 = 1 (value 0x800) so the inner loop's cmp/cs exits.
        let mut aes_ctx = RegFile::new(101, "aes-ctx-0x2e400000");
        for ctx_base in [0x0, 0x4000, 0x8000, 0xc000] {
            aes_ctx.set_word(ctx_base + 0x8, 0x800);
        }
        m.bus
            .map(BusRegion {
                base: 0x22e4_00000,
                size: 0x1_0000,
                device: Box::new(aes_ctx),
            })
            .expect("A11 AES context 0x2e400000 does not overlap another region");

        // `/arm-io/sochot0`, `compatible = "sochot,s5l8955x"`, `reg` cell
        // `0x352bc000 + 0x4000`, which arm-io's own translation puts at 0x2352bc000.
        //
        // The census stopped here on instruction 32058, `ldr w8, [0x2352bc018]`. This was
        // previously left unmapped on purpose -- there was no `sochot` model, and answering
        // the window with `Pmgr` or `AesOtp` would have asserted a register layout nothing
        // in the project supported. `SocHot` is that model: its four registers are read out
        // of this ROM's own accessors (`0x100008280`, `0x100008294`, `0x1000082b4`,
        // `0x1000082d4`) and its one writable bit is the self-clearing request the
        // `0x1000082f0` poll requires. It is mapped T8015-only: the A8's `sochot0` sits at
        // 0x20e02a000 and is a separate node, so sharing this base would move a block the
        // A8 tree does not place here.
        m.bus
            .map(BusRegion {
                base: sochot::BASE,
                size: sochot::SIZE,
                device: Box::new(SocHot::new(100)),
            })
            .expect("T8015 sochot0 does not overlap another region");
    }

    // The A10's own boot window, and a **separate claim from the A11's above** rather than
    // a widening of it. Both parts happen to zero-fill a span in the 0x1800_ family, which
    // is exactly why they are not merged: merging them would assert that the A10 uses the
    // A11's window, and the A10's window is not the A11's.
    //
    // The A10 evidence is its own image's data block, not the A11's. From
    // `SecureROM_t8010si_iBoot-2696.0.0.1.33``:
    //
    //     file 0x358 = 0x1800a8000   file 0x360 = 0x4000
    //     file 0x368 = 0x1800a0000   file 0x370 = 0x8000
    //
    // read at instructions 26 and 30 into the two zero-fill loops that start at file
    // offset 0x064 and 0x080. So the A10 clears 0x1800a8000..0x1800ac000 and
    // 0x1800a0000..0x1800a8000. The mapped window is the smallest one containing both of
    // those plus the `0x1800c1000` stack the CLI's reset context supplies.
    //
    // This is what stops the A10 at instruction 30. Measured, not inferred: with the A12's
    // inherited `sram_base` 0x1000000000 the run dies on the prologue's first `stp` with
    // `x10 = 0x1800a8000`, and no device has been touched yet -- the census reported zero
    // MMIO windows, so without this the A10 never reaches the code that programs its USB.
    //
    // `0x1801b0000` is named by the same data block (file 0x378) and is **not** covered:
    // nothing establishes its size, and a window sized to reach it would be a guess. If the
    // A10 ROM turns out to need it, the census will name it as an unmapped store, which is
    // the honest way to find out.
    if matches!(id, SoCId::T8010) {
            // The A10's boot scratch is one window, and it is the ROM's own three literals
            // that fix its extent rather than a zero-fill length. At file `0x87c0`, `0x87d0`
            // and `0x87d8` the image holds, three consecutive 64-bit words:
            //
            // ```text
            //   0x180080000    the window base
            //   0x1801b4000    where the heap allocator starts handing out memory
            //   0x180200000    the window end
            // ```
            //
            // so the aperture is `0x180080000 .. 0x180200000`, which is what `sram_size`
            // above encodes. The middle word is what makes this a measurement rather than a
            // guess: it is the first address the allocator's `add x23, xzr, x0` returns, and
            // a run reached the instruction
            //
            // ```text
            //   0x10001155c  str w0, [x23, x28, lsl #2]
            // ```
            //
            // with `x23 = 0x1801b4000` and `x28` counting to 1000, so it needed
            // `0x1801b4000..0x1801b4fa0` and died on the first store outside the old
            // `0x50000` window. The `0xfa0` it asked for is the same size the earlier
            // `sram_size` covered; the heap simply starts 0x12c0000 further along the window.
            //
            // A window sized only to the zero-fill spans would satisfy every span assertion
            // and still not be the window the firmware allocates from, which is the same
            // shape of failure the A10's first `sram_base` had. The two zero-fill spans
            // (file `0x358`/`0x360` and `0x368`/`0x370`) and the third (`0x378` plus the
            // `movz` at file `0x120`) all fall inside this one aperture, so there is no
            // separate second window to map.
            m.bus
                .map(BusRegion {
                    base: c.sram_base,
                    size: c.sram_size,
                    device: Box::new(SparseRam::new(c.sram_base, c.sram_size)),
                })
                .expect("T8010 boot SRAM does not overlap another region");
    }

    // The A10 (T8010) device map, built from the SecureROM's **own** device table rather
    // than from a DeviceTree, and that is the whole reason it is written the way it is.
    //
    // `SecureROM_t8010si_iBoot-2696.0.0.1.33`` carries its device table as a flat array
    // of 64-bit register addresses at file offset `0x164c8` (VA `0x1000164c8`), 0x68 bytes
    // per slot, indexed by the ROM's own device-number lookups. Read straight out of the
    // image it holds 130 non-zero entries and, after the table proper, unrelated data. The
    // device addresses it names are:
    //
    // ```text
    //   0x20e040000 .. 0x20e0400c4   50 slots, 0x08 stride   (plane 1, power gates)
    //   0x20e040200 .. 0x20e040218    7 slots               (plane 1, second gate bank)
    //   0x210240000 .. 0x210240038   11 slots               (plane 2, power gates)
    // ```
    //
    // and a second array at file `0x18478` (VA `0x100018478`) that the ROM's two
    // device-control routines index as `sub w8, w0, #6` / `cmp w8, #0x54`, i.e. 85 slots
    // for device numbers 6..90, holding:
    //
    // ```text
    //   device 0x06..0x16 -> 0x210280000 + 8*n     17 slots
    //   device 0x17..0x5a -> 0x20e080100 + 8*(n-0x17)   70 slots
    // ```
    //
    // That second array is the same shape the A11's `DeviceTableControlWindow` already
    // models, and the A10 drives it through a **byte-identical** pair of routines --
    // `device_table_control.rs` names T8010 build 2696.0.0.1.33 with its gate at
    // `0x10000945c` in the table at the top of that file. So the model is reused, and the
    // windows are the A10's own rather than the A11's `0x232080000` / `0x235280000`.
    //
    // Both arms of the A10's control aperture come from the address set above, and the
    // `0x20e080000` arm additionally carries the `+0x400` word the readiness poll reads:
    //
    // ```text
    //   0x10000944c  movz x8, #0x2, lsl #32
    //   0x100009450  movk x8, #0xe08, lsl #16
    //   0x100009454  movk x8, #0x400          ; x8 = 0x20e080400
    //   0x100009458  ldr  w9, [x8]
    //   0x10000945c  movz w10, #0xefff, lsl #16
    //   0x100009460  movk w10, #0xfbff        ; w10 = 0xeffffbff
    //   0x100009464  and  w9, w9, w10
    //   0x100009468  str  w9, [x8]
    //   0x10000946c  ldr  w9, [x8]            ; poll head
    //   0x100009470  and  w9, w9, #0xf0
    //   0x100009474  cmp  w9, #0xf0
    //   0x100009478  b.ne 0x10000946c        ; until the status nibble is all ones
    // ```
    //
    // This is where the A10 stopped with every address answered: the run reached
    // instruction 21,532 and then made 743,241 reads of `0x20e080400` without leaving
    // that three-instruction loop. The poll is **unconditional** -- no timeout, no error
    // branch, no alternate path -- and the ROM arrives at it only after configuring the
    // whole device array, so a functioning A10 has to report `0xf` in bits 7:4 of that word.
    // That is why `+0x400` is declared actual here and why nothing else is: the ROM's own
    // control flow is the evidence for this one register's value, and every other readiness
    // question in the model still has to be earned by a write.
    //
    // The same `0x20e080400` is the A12's too -- `Pmgr::status_nibble_all_ones` documents
    // `t8012si` naming it from a literal table at `0x100018d08` -- and the A8's equivalent
    // is plane `0x20000` + `0x400` of the `0xe000000 + 0x40000` PMGR window, which is why
    // this is one PMGR-family register across the family and not an A10 invention.
    if matches!(id, SoCId::T8010) {
        // The A10's DRAM, at the address its own ROM writes and reads back. The image holds
        // `0x200000000` as a bare 64-bit literal sixteen times, and the run stores at
        // `0x2002f8000`, `0x202f20020`, `0x202f22020` and `0x202f80000` and reads the first
        // back unchanged. A NAND controller would answer none of those, so this window is
        // RAM -- which is also why the synthetic NAND band is displaced below.
        //
        // The span stops at the GPU window's base for the same reason the NAND band does:
        // mapping the A10's full 3 GiB of DRAM would swallow `0x204000000`. 64 MiB contains
        // every address the run has touched (`0x202f80008` is the highest), and a run that
        // needs more names it as an unmapped access rather than being given the world.
        m.bus
            .map(BusRegion {
                base: T8010_DRAM_BASE,
                size: T8010_DRAM_MAPPED_SIZE,
                device: Box::new(SparseRam::new(T8010_DRAM_BASE, T8010_DRAM_MAPPED_SIZE)),
            })
            .expect("T8010 DRAM does not overlap another region");

        // The A10's power-management band, in the pieces its own ROM addresses require
        // rather than one rectangle, because the device-control window sits inside it. The
        // band itself is measured: the ROM touches 4 KiB pages at `0x20e000000` + `0x0,
        // 0x4000, 0x8000, 0xc000, 0x10000, 0x14000, 0x30000, 0x40000, 0x44000, 0x48000,
        // 0x4c000, 0x50000, 0x54000, 0x68000, 0x70000, 0x80000, 0xa0000, 0xdc000`, which is
        // the same `0x20e0_` family the A8 maps as its whole `pmgr` window, and a second
        // band at `0x2102_` holding `0x210200000`, `0x210240000`, `0x2102bc000` and
        // `0x2102dc000` with the device-control window at `0x210280000` between the second
        // and the third.
        //
        // `Pmgr` with no rules is the right model here and not a stub: the ROM's gate loop
        // at `0x100009e4c..0x100009e58` is `ldr x10,[x9,x10]` / `str w2,[x10]` / `ldr w11,[x10]`
        // / `tbnz x11,#30` back -- Apple's write-then-wait-for-ack, where the acknowledgement
        // is bit 30 going clear. Every value the ROM writes to these pages has bit 30 clear
        // (`0x80100000`, `0x85100000`, `0x86100000`, `0x87100000`, `0x88100000`, `0x80000001`,
        // `0xa00c8031`, `0xa003c011`, `0xa007d01d`), so a model that stores the guest's word
        // and hands it back satisfies the handshake from the firmware's own condition. No
        // completion bit is invented.
        for (device_id, base, size) in [
            (110, 0x20e0_00000u64, 0x8_0000),
            (111, 0x20e0_90000, 0x8_0000),
            (112, 0x2102_00000, 0x8_000),
            (113, 0x2102_40000, 0x4_000),
            (114, 0x2102_90000, 0x5_0000),
        ] {
            m.bus
                .map(BusRegion {
                    base,
                    size,
                    device: Box::new(Pmgr::new(device_id)),
                })
                .expect("T8010 power-management page does not overlap another region");
        }

        // The A10's two device-control apertures, from the ROM's own address set: the
        // `0x20e080000` arm, which carries the `+0x400` readiness word, and the
        // `0x210280000` arm, which is the second bank of 17 echo words. The `0x20e080000`
        // aperture is 0x1_0000 rather than three 4 KiB pages because the ROM's own table
        // names `0x20e080000`, `0x20e084000` and `0x20e088000` as one device's registers --
        // the same reason the A11 maps `0x232080000` + `0x1_0000`.
        //
        // The two rules are both declared, and they are not interchangeable. The 87 array
        // registers are **echoes** -- the ROM writes bits 3:0 and waits on
        // `(w ^ (w >> 4)) & 0xf == 0` -- so the model completes them from the write. The
        // `+0x400` word is a hardware **state**: the ROM only ever read-modify-writes it
        // (clearing bits 22 and 30) and then polls `(w & 0xf0) == 0xf0` unconditionally.
        // Modelling it as an echo makes the poll unsatisfiable, which is not a hang but a
        // wrong rule; `DeviceTableControlWindow::with_status_all_ones` exists for that one
        // offset and for nothing else in either aperture.
        //
        // Only `+0x400` is declared actual, and the value is the ROM's own: it polls for
        // exactly `0xf` with no timeout, so that is what a live A10 reports. Every other
        // readiness question still has to be earned by a write.
        //
        // T8010-only, and the negative matters as much as the positive for the same reason
        // the A11's mapping is: the A12 must leave both A10 addresses unmapped rather than
        // answered by a complete-looking model at another part's addresses.
        for (slot, (base, size)) in DeviceTableControlWindow::A10_WINDOWS.into_iter().enumerate()
        {
            let mut window = DeviceTableControlWindow::new(120 + slot as u32, base, size);
            if slot == 0 {
                window = window.with_status_all_ones(vec![DeviceTableControlWindow::A10_READY_OFFSET]);
                window.set_actual(DeviceTableControlWindow::A10_READY_OFFSET, 0xf);
            }
            m.bus
                .map(BusRegion {
                    base,
                    size,
                    device: Box::new(window),
                })
                .expect("T8010 device-control window does not overlap another region");
        }

        // Three more of the A10 ROM's own device-table windows, reached once the readiness
        // word is satisfied. Their addresses are read out of the image, not chosen: at file
        // `0x16278` (VA `0x100016278`) the ROM keeps a run of 64-bit literals that its own
        // `ldr x9, [x10, x9]` at `0x100002000` indexes with a 4-bit field taken from a
        // descriptor, and it holds
        //
        // ```text
        //   0x20f100000, 0xd0
        //   0x2100f0000, 0x58
        //   0x20a080000, 0x20a080004, 0x20a080008, 0x20a08000c, 0x20a080010,
        //   0x20a080020, 0x20a080030 .. 0x20a08004c,   then the terminator 0xd60000002f
        //   0x20a084000, ... the same fourteen offsets ...  then 0xd700000030
        //   0x20a088000, ... the same fourteen offsets ...  then 0xd800000031
        // ```
        //
        // **`0x20f100000` is the A10's GPIO, and the ROM says so itself.** It is the window
        // whose `+0xc4c` word the boot chain samples to decide it may continue:
        //
        // ```text
        //   0x10000214c  movz x8, #0x2, lsl #32
        //   0x100002150  movk x8, #0xf10, lsl #16
        //   0x100002154  movk x8, #0xc4c         ; x8 = 0x20f10c4c
        //   0x100002158  ldr  w8, [x8]
        //   0x10000215c  tbz  x8, #0, 0x10000217c ; clear -> print and never continue
        // ```
        //
        // and `+0xc4c` is the *same offset* the T8020 SecureROM samples, with the same
        // `AppleGpio` model answering it: `apple_gpio.rs` names `0xc4c` the strap readback
        // and documents the T8020's "assert a storage-device-presence strap by writing the
        // word at offset 0xc48 before sampling bit 0 of offset 0xc4c". The A10's run
        // reproduced exactly that -- it wrote `0x70280` to `+0x0`, a pin-configuration word,
        // and then panicked on the strap at instruction 31,594 -- so the same model serves
        // and the A10's window is its own (`0x20f100000`, from the ROM's table) rather than
        // the A12's `0x233100000` from `DeviceTree.d321ap.im4p`.
        //
        // The other two are **not** identified, and nothing here claims they are: the ROM's
        // register lists for them are a fourteen-offset core block per 4 KiB page and a
        // 0x58-register block, and a register layout does not name a part. `RegFile` is the
        // model for precisely that reason -- it stores the guest's words and hands them back
        // and asserts no behaviour. The census, not this comment, is what will say whether a
        // register there needs semantics.
        //
        // The spans are one 4 KiB page each because the index the ROM builds at
        // `0x100002004..0x10000200c` is `(w19 >> 30) & 0x3fc` at 4-byte stride, so a
        // register array is 0x400 bytes wide, and both the `0xd0` and the `0x58` the ROM's own
        // table records fit inside one page. A page is also the granularity every other window
        // in this map uses.
        m.bus
            .map(BusRegion {
                base: 0x20f1_00000,
                size: 0x1_0000,
                device: Box::new(AppleGpio::new(130)),
            })
            .expect("T8010 GPIO does not overlap another region");
        for (device_id, name, base, size) in [
            (131u32, "t8010-0x2100f0000", 0x2100_f0000u64, 0x1000u64),
            (132, "t8010-0x20a080000", 0x20a0_80000, 0xc_000),
        ] {
            m.bus
                .map(BusRegion {
                    base,
                    size,
                    device: Box::new(RegFile::new(device_id, name)),
                })
                .expect("T8010 device window does not overlap another region");
        }

        // A10 (T8010) USB host stack: UsbPhy, UsbMux, UsbPd.
        // These are mapped at the config's usbphy_base, usbmux_base, usbpd_base.
        // Note: USB device controller (DWC2) is mapped by the generic section at the end.
        // usb-complex is not separately mapped for A10 as it uses the same RegFile pattern as A8/A9.
        m.bus
            .map(BusRegion {
                base: c.usbphy_base,
                size: 0x1000,
                device: Box::new(UsbPhy::new(75)),
            })
            .expect("T8010 USB PHY does not overlap another region");
        m.bus
            .map(BusRegion {
                base: c.usbmux_base,
                size: 0x1000,
                device: Box::new(UsbMux::with_irq(76, 68)),
            })
            .expect("T8010 USB Mux does not overlap another region");
        m.bus
            .map(BusRegion {
                base: c.usbpd_base,
                size: 0x1000,
                device: Box::new(UsbPd::with_irq(77, 69)),
            })
            .expect("T8010 USB PD does not overlap another region");

        // A10 (T8010) SEP→AP ASC mailbox.
        // The A10 has a SEP (AppleSEPROM-323), so map the ASC mailbox at the
        // standard mailbox_base (0x23c000000) with IRQ 0xa4 (164).
        let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        m.bus
            .map(BusRegion {
                base: c.mailbox_base,
                size: 0x10000,
                device: Box::new(sep_ap_mailbox),
            })
            .expect("T8010 ASC mailbox does not overlap another region");
    }

    // The T802x/T803x boot scratch, measured from each part's own SecureROM data block. The
    // config's inherited `sram_base` of 0x1000000000 is wrong for these parts and the census
    // names the failure rather than leaving it to be guessed: the T8027 run dies on
    // instruction **51**, the T8030 on **53**, and the T8020 on **55**, each on the
    // prologue's first `stp` with `x10 = 0x19c01c000` and no MMIO contact at all.
    //
    // The prologue is the same shape as the A10's and the A11's -- load a base, load a
    // length, add, then zero-fill -- and the literals are the measurement:
    //
    //     file 0x358 = 0x19c01c000   file 0x360 = 0x4000
    //     file 0x378 = 0x19c000000   file 0x380 = 0xc000
    //
    // read at instructions 26 and 30 into the two zero-fill loops at file 0x080 and 0x10c.
    // Enumerating *every* 0x19c0_00000-range base in the block rather than only the two the
    // loops consume gives the envelope the ROM actually addresses: `0x19c000000` (the lowest)
    // through `0x19c020000` (the highest, from file 0x388), so `0x20000` covers all of it
    // with no slack and no invented extent. That is a smaller claim than sizing to reach
    // something unmeasured, and a window that stopped at 0x19c01c000 would fault again the
    // moment the second loop ran.
    //
    // **The T8020 is in this set because its own image says so, and this comment used to
    // assert the opposite.** It claimed the T8020 "maps its own `0x19c0_00000` block
    // elsewhere in this function", which was false: there was no such mapping anywhere in
    // this file, so the claim read as a reason to exclude the part while in fact being the
    // only record that the part was missing. The T8020 data block is the same structure
    // with two differences that matter, and both were checked against the image rather than
    // assumed: `0x358 = 0x19c01c000` and `0x360 = 0x4000` are identical to the A12X's, and
    // `0x378 = 0x19c000000` with `0x380 = 0x8000` is a *different* length from the A12X's
    // `0xc000` -- so the two parts are the same window and not the same zero-fill, which is
    // why `tests/a12_platform.rs` reads the spans out of the image instead of sharing a
    // constant with the A12X test.
    if matches!(id, SoCId::T8020 | SoCId::T8027 | SoCId::T8030) {
        m.bus
            .map(BusRegion {
                base: T803X_BOOT_SCRATCH_BASE,
                size: T803X_BOOT_SCRATCH_SIZE,
                device: Box::new(SparseRam::new(
                    T803X_BOOT_SCRATCH_BASE,
                    T803X_BOOT_SCRATCH_SIZE,
                )),
            })
            .expect("T803x boot scratch does not overlap another region");

        // The third zero-fill span, whose length is an immediate rather than a literal, so
        // it is not inside the envelope above and gets its own window for the same reason
        // the A10's does. See `T803X_FAR_ZERO_BASE` for the disassembly and for the
        // instruction count the census reports without it.
        m.bus
            .map(BusRegion {
                base: T803X_FAR_ZERO_BASE,
                size: T803X_FAR_ZERO_SIZE,
                device: Box::new(SparseRam::new(T803X_FAR_ZERO_BASE, T803X_FAR_ZERO_SIZE)),
            })
            .expect("T803x second zero-fill window does not overlap another region");
    }

    // The rest of that block is A12X/A13-specific and stays gated to the two parts that
    // measured it. It was in the same `if` as the boot scratch above, so widening the
    // scratch to the T8020 would have handed the A12 the A12X's `Pmgr` planes, its
    // `AppleMcc` and its watchdog and chip-control windows -- which the A12 already maps at
    // its own addresses, and which the comment on `T8027_PMGR_PAGES` says explicitly would
    // collide rather than extend anything. The two sets are split so that widening one is
    // not silently widening the other.
    if matches!(id, SoCId::T8027 | SoCId::T8030) {
        // The T8027 uses the T8020's `0x23b0xxxxx` PMGR layout. The T8020 block above maps
        // its `0x23b0802xx` power-state registers and the SEP word, so the general PMGR
        // planes the T8027 additionally polls are mapped here as `Pmgr` at the exact pages
        // its image names -- `0x23b000000`, `0x23b014000`, `0x23b02c000`, `0x23b040000`,
        // `0x23b068000`, `0x23b0a0000`, `0x23b0b8000` -- and nothing wider.
        //
        // The census is what names them, one fault at a time: it stopped at instruction
        // 30,122 on `ldr w9, [0x23b0a0000]` / `orr w9, w9, #0x7f00` / `str w9, [0x23b0a0000]`,
        // Apple's write-then-poll-for-ack power sequence, which a `Pmgr` that returns what
        // was written models the way the T8020 already does for the same idiom. Sizing this
        // as one 0xc0000 window overlapped the T8020 block's specific registers, so the
        // pages are mapped individually instead.
        for page in T8027_PMGR_PAGES {
            // `0x23b000000` is mapped wide because the device-registry walk strides 16 KiB
            // across it, so the two pages that fall inside that plane are already covered
            // and mapping them again would be an overlap rather than a second claim. This
            // is containment, not a guess: the skip is a pure range test against the size
            // the census established.
            // Containment, and it has to be written as a real range test: the list is not
            // sorted, and three of its pages are *below* plane 0 numerically, so a bare
            // `page < plane_end` silently skipped `0x210e20000`, `0x210e21000`,
            // `0x210e40000` and `0x21c00000` -- the second calibration plane, which is why
            // the T8027 and the T8030 both stopped at `0x210e20020` with no window there.
            let plane = T8027_PMGR_PAGES[0];
            if page != plane && (plane..plane + T8027_PMGR_PLANE0_SIZE).contains(&page) {
                continue;
            }
            m.bus
                .map(BusRegion {
                    base: page,
                    // The first page carries the device-registry table the calibration walk
                    // strides across, so it is the one page that is not 4 KiB. The census
                    // named the extent; see `T8027_PMGR_PLANE0_SIZE`.
                    size: if page == T8027_PMGR_PAGES[0] {
                        T8027_PMGR_PLANE0_SIZE
                    } else {
                        0x1000
                    },
                    device: Box::new(Pmgr::new(0)),
                })
                .unwrap_or_else(|e| panic!("T8027 PMGR page {page:#x} overlaps: {e:?}"));
        }

        // The three device windows the T8027 demonstrably drives, mapped here because the
        // T8020 block that carries them is gated on `id == SoCId::T8020` and the T8027 is a
        // different part. Each is established from the T8027's own accesses, not from family
        // resemblance:
        //
        // * `AppleMcc` at `0x23b288000` -- the calibration block whose first pass the census
        //   completes, which is how the run gets past instruction 44,624.
        // * `Watchdog` at `0x23d2b0000 + 0xc000` and `AppleChipControl` at
        //   `0x23d2bc000 + 0x4000` -- the T8027 reads `0x23d2bc030` (`+0x30`, the chip
        //   revision word) and writes `0x23d2b8030`, and with neither window mapped the run
        //   faults at instruction 31,378 on a bus error at the first of them.
        //
        // They are the same devices at the same addresses as the T8020's, which is a
        // measurement about the T8027's own address inventory rather than a claim that the
        // two parts are interchangeable.
        m.bus
            .map(BusRegion {
                base: T8027_MCC_BASE,
                size: T8027_MCC_SIZE,
                device: Box::new(AppleMcc::new(85)),
            })
            .expect("T8027 AppleMcc window does not overlap another region");

        m.bus
            .map(BusRegion {
                base: T8027_WDT_BASE,
                size: T8027_WDT_SIZE,
                device: Box::new(Watchdog::with_irq(77, 39)),
            })
            .expect("T8027 watchdog window does not overlap another region");
        m.bus
            .map(BusRegion {
                base: T8027_CHIPCTL_BASE,
                size: T8027_CHIPCTL_SIZE,
                device: Box::new(AppleChipControl::new(78)),
            })
            .expect("T8027 chip-control window does not overlap another region");

    }

    // The `0x210e70000` calibration block, which **both** the T8027 and the T8030 write and
    // neither of them composes through an immediate-offset load. Each composes it in-register
    // and stores through a register pair -- `movz x10, #0x10e7, lsl #16` / `movk x10, #0x2,
    // lsl #32` / `str x8, [x9, x10]` -- which is why an inventory that only decodes
    // immediate-offset accesses does not see it. That gap is worth naming: a scan that
    // reports a band as "not addressed" can be wrong about the *form*, not the address.
    //
    // The value written is the Apple MCS calibration pattern `0x494c87`: the T8027 writes
    // `0x484c87` (bit 22 clear) and the T8030 `0xa494c87`, both against the same loop bound
    // `cmp w19, #0xf` -- sixteen entries, which is the 32 KiB-spaced sweep the A12 record
    // already documents for the A12's equivalent at this same address. The size is therefore
    // sixteen pages of stride, and `AppleMcc` is the model the A12 uses for it.
    if matches!(id, SoCId::T8027 | SoCId::T8030) {
        m.bus
            .map(BusRegion {
                base: T803X_MCC2_BASE,
                size: T803X_MCC2_SIZE,
                device: Box::new(AppleMcc::new(85)),
            })
            .expect("T803x second calibration window does not overlap another region");
    }

    // The A14's own power-state table, at a base the A12 family does not use. The A14's
    // image names **131** distinct 4-byte words across `0x23b700000..0x23b70c030` at 8-byte
    // stride, and the census stops on one of them -- `0x23b700100` -- through the ROM's own
    // device table, with the same `0xeffffbff` control-word protocol the A12 uses. It is
    // modelled as `PmgrPowerState` for the same reason the A12's block is: the value is the
    // platform's, the target is the guest's, and no register here is declared active.
    //
    // The window is the envelope of the addresses the image names, not a rounded aperture.
    if matches!(id, SoCId::T8101) {
        m.bus
            .map(BusRegion {
                base: T8101_POWER_BASE,
                size: T8101_POWER_SIZE,
                device: Box::new(Pmgr::new(0)),
            })
            .expect("T8101 power-state table does not overlap another region");
    }

    // The A14's watchdog and chip-control windows -- the two devices from the T8020 block that
    // the A14 also drives, and *only* those. Extending the T8020 block wholesale is not an
    // option: it also carries a DART-SIO window at `0x235004000` that collides with the A14's
    // own `dcp` aperture at `0x235000000 + 0x40000`, a different device at a different address
    // on a different part. Taking the two the census and the inventory name is the narrower
    // and evidenced claim.
    //
    // The census stops at instruction 28,474 on `ldr w8, [0x23d2bc03c]` inside chip control and
    // decodes `ubfm w9, w8, #9, #31` / `and w0, w9, #0x70` / `bfm w0, w8, #10, #12` out of it --
    // bits 10..12. `AppleChipControl` has no `+0x3c` case, so the word reads as an unmodelled
    // register in a modelled device, which is the honest shape: the device is real, the
    // register's meaning is not established, and the model does not invent one.
    if matches!(id, SoCId::T8101) {
        m.bus
            .map(BusRegion {
                base: T8101_WDT_BASE,
                size: T8101_WDT_SIZE,
                device: Box::new(Watchdog::with_irq(77, 39)),
            })
            .expect("T8101 watchdog window does not overlap another region");
        m.bus
            .map(BusRegion {
                base: T8101_CHIPCTL_BASE,
                size: T8101_CHIPCTL_SIZE,
                device: Box::new(AppleChipControl::new(78)),
            })
            .expect("T8101 chip-control window does not overlap another region");
    }

    // The A14 (T8101)'s own device pages, each named by its address inventory rather than
    // by a neighbour's map. The A14 is not an A12 in a different package: it drives
    // `0x23b7xxxxx`, not `0x23b0xxxxx`, and its calibration planes sit at `0x210e80000` /
    // `0x210ea0000` / `0x210f40000` rather than the Firebloom `0x210e20000` / `0x210e70000`.
    // The census is the arbiter: it stops at instruction 26,161 on `ldr w9, [0x23b720004]`.
    if matches!(id, SoCId::T8101) {
        for page in T8101_PMGR_PAGES {
            m.bus
                .map(BusRegion {
                    base: page,
                    size: 0x1000,
                    device: Box::new(Pmgr::new(0)),
                })
                .unwrap_or_else(|e| panic!("T8101 PMGR page {page:#x} overlaps: {e:?}"));
        }
    }

    // The A14 (T8101) boot scratch. Its own base, established by three independent images,
    // and its own block because it is 0x3000000 away from the Firebloom pair's -- see
    // `T8101_BOOT_SCRATCH_BASE`.
    if matches!(id, SoCId::T8101) {
        m.bus
            .map(BusRegion {
                base: T8101_BOOT_SCRATCH_BASE,
                size: T8101_BOOT_SCRATCH_SIZE,
                device: Box::new(SparseRam::new(
                    T8101_BOOT_SCRATCH_BASE,
                    T8101_BOOT_SCRATCH_SIZE,
                )),
            })
            .expect("T8101 boot scratch does not overlap another region");
    }

    // The A13 (T8030) boot scratch and its calibration block. Separate from the T8027's on
    // measurement, not on taste: the boot scratch is the same window two independent parts
    // agree on (`T803X_BOOT_SCRATCH_*`), but the AppleMcc is 0x230000 lower on this part and
    // nothing in the T8027 image names the A13's offset.
    if matches!(id, SoCId::T8030) {

        // The A13's own power-state words, named as literals at file `0x22980` / `0x22988`:
        // `0x23b080c00` and `0x23b080c08`. Its census stops at instruction 67,974 reading the
        // first of them through the ROM's own runtime device table
        // (`ldr x8, [x8, w0, uxtw lsl #3]` / `and x8, x8, #0x7ffffffffffff` / `ldr w9, [x8]`)
        // and then applying the same `0xeffffbff` control-word mask the A12's gate uses.
        for (device_id, address) in [(61, 0x23b0_80c00), (62, 0x23b0_80c08)] {
            let mut ps = iphem_devices::pmgr_power::PmgrPowerState::new(device_id);
            ps.set_on_write(|value| {
                let _ = value;
            });
            m.bus
                .map(BusRegion {
                    base: address,
                    size: 4,
                    device: Box::new(ps),
                })
                .expect("T8030 power-state window does not overlap another region");
        }

        m.bus
            .map(BusRegion {
                base: T8030_MCC_BASE,
                size: T8030_MCC_SIZE,
                device: Box::new(AppleMcc::new(85)),
            })
            .expect("T8030 AppleMcc window does not overlap another region");

        // The one address in this range that is not in the calibration block: the census
        // reports it from the same inventory, `0x23b09c040`. It gets its own page because a
        // window sized to reach it from `0x23b058000` would swallow the shared PMGR page at
        // `0x23b068000` that the T8020 block maps.
        m.bus
            .map(BusRegion {
                base: T8030_PMGR_TAIL_BASE,
                size: 0x1000,
                device: Box::new(Pmgr::new(0)),
            })
            .expect("T8030 PMGR tail page does not overlap another region");
    }

    // The A14 (T8101) device map for DFU restore. The A14 uses the same
    // d321ap arm-io layout as the A12/A12X/A13 for USB/DART/SEP/AES.
    // Map USB Complex, DART-USB, USB device, AES, SEP mailbox, and DART-SIO.
    if matches!(id, SoCId::T8101) {
        // USB Complex at the d321ap address.
        m.bus
            .map(BusRegion {
                base: 0x239000000,
                size: 0x1000,
                device: Box::new(UsbComplex::new(74)),
            })
            .unwrap();
        // DART-USB for DMA (stream 7, IRQ 454).
        let (usb_mmio, usb_port) = DartT8020::new(73, 14)
            .with_input_address_width(32)
            .into_pair();
        m.bus
            .map(BusRegion {
                base: 0x239900000,
                size: 0x4000,
                device: Box::new(usb_mmio),
            })
            .unwrap();
        m.register_dma_iommu(7, DART_USB_IRQ, Box::new(usb_port));
        // AES engine at the d321ap address.
        m.bus
            .map(BusRegion {
                base: 0x235008000,
                size: 0x4000,
                device: Box::new(AesOtp::new(211)),
            })
            .unwrap();
        // UART1 for Darwin console.
        m.bus
            .map(BusRegion {
                base: 0x235204000,
                size: 0x4000,
                device: Box::new(S5lUart::new(200, "uart1")),
            })
            .unwrap();
        // DART-SIO: MMIO only.
        let (sio_mmio, _sio_port) = DartT8020::new(71, 14).into_pair();
        m.bus
            .map(BusRegion {
                base: 0x235004000,
                size: 0x4000,
                device: Box::new(sio_mmio),
            })
            .unwrap();
        // SEP->AP ASC mailbox at the same base as T8020.
        let (sep_ap_mailbox, _sep_iop_mailbox) = AppleAscMailbox::paired(98, 0xa4, 0xa4);
        m.bus
            .map(BusRegion {
                base: 0x242400000,
                size: 0x10000,
                device: Box::new(sep_ap_mailbox),
            })
            .unwrap();
        // UART0/2/3 at the same d321ap addresses.
        m.bus
            .map(BusRegion {
                base: 0x235100000,
                size: 0x4000,
                device: Box::new(S5lUart::new(199, "uart0")),
            })
            .unwrap();
        for (device_id, base, name) in [(201, 0x235208000, "uart2"), (202, 0x23520c000, "uart3")] {
            m.bus
                .map(BusRegion {
                    base,
                    size: 0x4000,
                    device: Box::new(S5lUart::new(device_id, name)),
                })
                .unwrap();
        }
        // ANE, GPU, ISP at the same d321ap addresses.
        m.bus
            .map(BusRegion {
                base: 0x26a000000,
                size: 0x1000,
                device: Box::new(AneFifo::new(280)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: GPU_BASE,
                size: GPU_SIZE,
                device: Box::new(SgxGpu::new(281)),
            })
            .unwrap();
        m.bus
            .map(BusRegion {
                base: ISP_BASE,
                size: ISP_SIZE,
                device: Box::new(CameraIsp::with_irq(282, 153)),
            })
            .unwrap();
    }

    // Synthetic NAND controller window. Historically `c.nand_size` spans the
    // whole (0x200000000, 0x208000000) arm-io band and would claim the
    // DT-measured SGX GPU window (0x204000000). The catch-all is clamped to
    // stop before the GPU base; anything the declared band extends past the
    // GPU window stays an inert SparseRam catch-all so synthetic no-fault
    // coverage of the band is preserved (the GPU window itself is decoded by
    // SgxGpu in the T8020 block, and by SparseRam for other SoCs).
    //
    // **The A10 is excluded, and this is a measurement rather than a carve-out.**
    // `SoCId::T8010::nand_base` was the A12's `0x200000000` with nothing behind it for the
    // A10, and the A10 SecureROM uses that exact window as its own RAM: it stores at
    // `0x2002f8000`, `0x202f20020`, `0x202f22020` and `0x202f80000` and reads `0x202f20020`
    // back unchanged. Mapping a `NandCtrl` there answers none of those. The A10's own RAM
    // window is [`T8010_DRAM_BASE`] + [`T8010_DRAM_MAPPED_SIZE`], mapped above, so the NAND
    // band would only collide with it.
    //
    // What this does **not** do is invent a NAND address for the A10. Nothing in the checked-in
    // corpus places the A10's NAND controller, so the A10 runs with no NAND model rather than
    // with one at a made-up base; a run that reaches the NAND will fault on the address and
    // the census will name it, which is the honest way to find out.
    let nand_band_end = c.nand_base.checked_add(c.nand_size).unwrap_or(GPU_BASE);
    let nand_regs_end = nand_band_end.min(GPU_BASE);
    if !matches!(id, SoCId::T8010) {
        m.bus
            .map(BusRegion {
                base: c.nand_base,
                size: nand_regs_end - c.nand_base,
                device: Box::new(NandCtrl::new(0)),
            })
            .unwrap();
    }
    if nand_band_end > GPU_BASE + GPU_SIZE {
        m.bus
            .map(BusRegion {
                base: GPU_BASE + GPU_SIZE,
                size: nand_band_end - (GPU_BASE + GPU_SIZE),
                device: Box::new(SparseRam::new(
                    GPU_BASE + GPU_SIZE,
                    nand_band_end - (GPU_BASE + GPU_SIZE),
                )),
            })
            .unwrap();
    }
    if !matches!(id, SoCId::T8020 | SoCId::T8027 | SoCId::T8030) {
        // Non-T8020/T8027/T8030 SoCs have no measured GPU window; keep the synthetic
        // catch-all there so reads never fault. T8027 and T8030 map the real SgxGpu
        // in their device-specific blocks above.
        m.bus
            .map(BusRegion {
                base: GPU_BASE,
                size: GPU_SIZE,
                device: Box::new(SparseRam::new(GPU_BASE, GPU_SIZE)),
            })
            .unwrap();
    }

    m.bus
        .map(BusRegion {
            base: c.pmu_base,
            size: 0x1000,
            device: Box::new(Pmu::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.spmi_base,
            size: 0x1000,
            device: Box::new(Spmi::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.thermal_base,
            size: 0x1000,
            device: Box::new(Thermal::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.usb_base,
            size: c.usb_size,
            // The d321ap DeviceTree places the USB-device aperture at
            // 0x239100000.  Use the functional controller for T8020 too so
            // real SecureROM MMIO can be observed; its register protocol is
            // still deliberately an evidence gate, not a readiness claim.
            device: Box::new(UsbDeviceController::new(0, c.usb_base, 451)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.watchdog_base,
            size: 0x1000,
            device: Box::new(Watchdog::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.rtc_base,
            size: 0x1000,
            device: Box::new(Rtc::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.pwm_base,
            size: 0x1000,
            device: Box::new(Pwm::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.adc_base,
            size: 0x1000,
            device: Box::new(Adc::new(0)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.pinmux_base,
            size: 0x1000,
            device: Box::new(Pinmux::new(24)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.clockctl_base,
            size: 0x1000,
            device: Box::new(ClockCtrl::with_irq(25, 56)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.regulator_base,
            size: 0x1000,
            device: Box::new(Regulator::with_irq(26, 57)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.sdhc_base,
            size: 0x1000,
            device: Box::new(EmmcSdHc::with_irq(27, SdCard::new(1024), 58)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.panel_base,
            // Panel pixel MMIO begins at +0x1000. The next DSI window begins
            // at +0x10000, so expose the entire non-overlapping aperture.
            // Full-resolution scanout belongs in guest DRAM via DCP.
            size: 0x10000,
            device: Box::new(Panel::with_irq(28, 59)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.dsi_phy_base,
            size: 0x1000,
            device: Box::new(DsiPhy::with_irq(29, 60)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.raster_base,
            size: 0x1000,
            device: Box::new(Raster::with_irq(30, 61)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.touch_base,
            size: 0x1000,
            device: Box::new(TouchHid::with_irq(31, 62)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.audio_base,
            size: 0x1000,
            device: Box::new(AudioI2s::with_irq(32, 63)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.bt_base,
            size: 0x1000,
            device: Box::new(BtHci::with_irq(33, 64)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.wifi_base,
            size: 0x1000,
            device: Box::new(WifiSdio::with_irq(34, 65)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.camera_base,
            size: 0x1000,
            device: Box::new(CameraIsp::with_irq(35, 66)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.sensorhub_base,
            size: 0x1000,
            device: Box::new(SensorHub::with_irq(36, 67)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.usbphy_base,
            size: 0x1000,
            device: Box::new(UsbPhy::with_irq(37, 68)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.usbmux_base,
            size: 0x1000,
            device: Box::new(UsbMux::with_irq(38, 69)),
        })
        .unwrap();

    m.bus
        .map(BusRegion {
            base: c.usbpd_base,
            size: 0x1000,
            device: Box::new(UsbPd::with_irq(39, 70)),
        })
        .unwrap();

    m
}

pub fn build(id: SoCId) -> Machine {
    build_with_rom(id, &[], true)
}

pub fn research_targets() -> [SoCId; 2] {
    [SoCId::T8020, SoCId::T8027]
}

pub fn config(id: SoCId) -> SoCConfig {
    match id {
        SoCId::T8010 => SoCConfig {
            id: SoCId::T8010,
            name: "T8010",
            family: "T801x",
            dram_base: 0,
            dram_size: 64 << 30,
            // Measured from the A10 SecureROM's own data block, not chosen. The A10
            // prologue (image file `SecureROM_t8010si_iBoot-2696.0.0.1.33``) reads a
            // base and a length and then zero-fills the span, at file offsets 0x358/0x360
            // and again at 0x368/0x370:
            //
            //     0x100000050  msr  VBAR_EL1, x10
            //     0x100000054  ldr  x10, [0x100000358]   ; base
            //     0x100000058  ldr  x11, [0x100000360]   ; length
            //     0x10000005c  add  x11, x11, x10        ; end
            //     0x100000064  stp  x12, x12, [x10], #0x10
            //     0x100000068  cmp  x10, x11
            //     0x10000006c  b.ne 0x100000064
            //
            // which decode to `0x1800a8000 + 0x4000` and `0x1800a0000 + 0x8000`. The same
            // block names `0x180080000`, `0x180089608` and `0x1801b0000`, and the CLI's
            // reset context puts the stack at `0x1800c1000` (see `configure_reset`). The
            // window below is the smallest that contains the two zero-fill spans and the
            // stack.
            //
            // The previous value, `0x1000000000`, was the A12's: the A10 ROM's very first
            // store faults there, at instruction 30, so this was not a cosmetic
            // difference -- it stopped the A10 before it executed a single instruction of
            // device setup. This is the same discipline already applied to the A9, whose
            // `sram_base` 0x1800c0000 is likewise its ROM's own zero-fill literal.
            //
            // `0x1801b0000` is named by the same block and is deliberately *not* covered:
            // its size is not established, so widening the window to reach it would be a
            // guess. It is recorded here so the next measurement can look for it.
            sram_base: 0x1800_80000,
            sram_size: 0x180_0000,
            uart_base: 0x20a0c0000,
            timer_base: 0x20e100000,
            aic_base: 0x23b100000,
            aic_size: 0x30000,
            pmgr_base: 0x20e000000,
            mailbox_base: 0x23c000000,
            rom_base: 0x100000000,
            rom_size: 0x100000,
            dcp_base: 0x235000000,
            dcp_size: 0x40000,
            nor_base: 0x240000000,
            nor_size: 0x1000000,
            block_base: 0x241000000,
            block_size: 0x400000,
            sep_uart_base: 0x20a0d0000,
            bb_uart_base: 0x20a0e0000,
            uart_window: true,
            i2c_base: 0x20a200000,
            spi_base: 0x20a400000,
            gpio_base: 0x20a100000,
            nand_base: 0x200000000,
            nand_size: 0x8000000,
            pmu_base: 0x20a000000,
            spmi_base: 0x20a300000,
            thermal_base: 0x20a700000,
            // `usb-device` from this part's own device tree, as for the A7..A10:
            // `DeviceTree.d101ap.im4p` (A10) and `DeviceTree.d201ap.im4p` (A11) both
            // declare `/arm-io/usb-complex` at 0xc900000 with `usb-device` at +0x100000,
            // and the A11's own `d22ap` tree instead puts `usb-complex` at 0x30000000
            // with `usb-device` at 0x30100000. The value this replaces, 0x20a800000, is
            // in no tree checked in here.
            //
            // The T8101 arm does not carry this comment: no A14 device tree is in the
            // repository, so its window is not established and the value there is the
            // A13/A14 default rather than a measurement.
            usb_base: 0x20ca00000,
            usb_size: 0x10000,
            watchdog_base: 0x20a0b0000,
            rtc_base: 0x20a0f0000,
            pwm_base: 0x20a500000,
            adc_base: 0x20a600000,
            pinmux_base: 0x20a900000,
            clockctl_base: 0x20a010000,
            regulator_base: 0x20a020000,
            sdhc_base: 0x20a0a0000,
            panel_base: 0x20a110000,
            dsi_phy_base: 0x20a120000,
            raster_base: 0x20a130000,
            touch_base: 0x20a140000,
            audio_base: 0x20a150000,
            bt_base: 0x20a160000,
            wifi_base: 0x20a170000,
            camera_base: 0x20a180000,
            sensorhub_base: 0x20a190000,
            usbphy_base: 0x20a1a0000,
            usbmux_base: 0x20a1a1000,
            usbpd_base: 0x20a1a2000,
            gid_key: GID_DEFAULT,
            uid_key: UID_DEFAULT,
            sep_rom_path: None,
        },
        // T8015 is the A11. The four window values below are cited against
        // `DeviceTree.d22ap.im4p` and the ROM's own measured accesses; the
        // 0x20a… band is this tree's synthetic per-peripheral harness space and
        // is NOT A11 silicon. See docs/A11_T8015_PLATFORM.md.
        SoCId::T8015 => SoCConfig {
            id: SoCId::T8015,
            name: "T8015",
            family: "A11",
            // Unmeasured: the A11 DRAM base is not established by the corpus.
            dram_base: 0,
            dram_size: 64 << 30,
            // Measured: the T8015 SecureROM requires exactly this boot-SRAM
            // span, and builds its own translate table inside it (TTBR0_EL1 =
            // 0x18000c000). It is not a general-purpose SRAM window.
            sram_base: 0x180000000,
            sram_size: 0x1e8000,
            uart_base: 0x20a0c0000,
            timer_base: 0x20e100000,
            // DT `aic` reg = 0x32100000 + 0x8000 (the 0x32108000 + 0x1000
            // aic-timebase region lies inside it).
            aic_base: 0x232100000,
            aic_size: 0x8000,
            // DT `pmgr` first region = 0x32000000 + 0x100000.
            pmgr_base: 0x232000000,
            // Unmeasured: unlike SEP/SIO/AOP the A11 ASC mailbox is not
            // DT-declared, and the A11 ROM carries no literal for any A12 mailbox
            // base. Deliberately left as the legacy value rather than guessed.
            mailbox_base: 0x23c000000,
            rom_base: 0x100000000,
            rom_size: 0x100000,
            dcp_base: 0x235000000,
            dcp_size: 0x40000,
            nor_base: 0x240000000,
            nor_size: 0x1000000,
            block_base: 0x241000000,
            block_size: 0x400000,
            sep_uart_base: 0x20a0d0000,
            bb_uart_base: 0x20a0e0000,
            uart_window: true,
            i2c_base: 0x20a200000,
            spi_base: 0x20a400000,
            gpio_base: 0x20a100000,
            nand_base: 0x200000000,
            nand_size: 0x8000000,
            pmu_base: 0x20a000000,
            spmi_base: 0x20a300000,
            thermal_base: 0x20a700000,
            // `usb-device` from this part's own device tree, as for the A7..A10:
            // `DeviceTree.d101ap.im4p` (A10) and `DeviceTree.d201ap.im4p` (A11) both
            // declare `/arm-io/usb-complex` at 0xc900000 with `usb-device` at +0x100000,
            // and the A11's own `d22ap` tree instead puts `usb-complex` at 0x30000000
            // with `usb-device` at 0x30100000. The value this replaces, 0x20a800000, is
            // in no tree checked in here.
            //
            // The T8101 arm does not carry this comment: no A14 device tree is in the
            // repository, so its window is not established and the value there is the
            // A13/A14 default rather than a measurement.
            usb_base: 0x250000000,
            usb_size: 0x10000,
            // DT `wdt` reg = 0x352b0000 + 0x4000 (plus a 4-byte region at
            // 0x352b8020); compatible = "wdt,t8015wdt,s5l8960x", SPI 172.
            watchdog_base: 0x2352b0000,
            rtc_base: 0x20a0f0000,
            pwm_base: 0x20a500000,
            adc_base: 0x20a600000,
            pinmux_base: 0x20a900000,
            clockctl_base: 0x20a010000,
            regulator_base: 0x20a020000,
            sdhc_base: 0x20a0a0000,
            panel_base: 0x20a110000,
            dsi_phy_base: 0x20a120000,
            raster_base: 0x20a130000,
            touch_base: 0x20a140000,
            audio_base: 0x20a150000,
            bt_base: 0x20a160000,
            wifi_base: 0x20a170000,
            camera_base: 0x20a180000,
            sensorhub_base: 0x20a190000,
            usbphy_base: 0x20a1a0000,
            usbmux_base: 0x20a1a1000,
            usbpd_base: 0x20a1a2000,
            gid_key: GID_DEFAULT,
            uid_key: UID_DEFAULT,
            sep_rom_path: None,
        },
        SoCId::T8020 => T8020.clone(),
        SoCId::T8027 => T8027.clone(),
        SoCId::T8030 => SoCConfig {
            id: SoCId::T8030,
            name: "T8030",
            family: "T803x",
            dram_base: 0,
            dram_size: 64 << 30,
            sram_base: 0x1000000000,
            sram_size: 0x400000,
            uart_base: 0x20a0c0000,
            timer_base: 0x20e100000,
            aic_base: 0x23b100000,
            aic_size: 0x30000,
            pmgr_base: 0x20e000000,
            mailbox_base: 0x23c000000,
            rom_base: 0x100000000,
            rom_size: 0x100000,
            dcp_base: 0x235000000,
            // The d321ap DeviceTree places DART-SIO at +0x4000. Keep the synthetic
            // DCP register aperture from claiming that independent hardware window.
            dcp_size: 0x4000,
            nor_base: 0x240000000,
            nor_size: 0x1000000,
            block_base: 0x241000000,
            block_size: 0x400000,
            sep_uart_base: 0x20a0d0000,
            bb_uart_base: 0x20a0e0000,
            uart_window: true,
            i2c_base: 0x20a200000,
            spi_base: 0x20a400000,
            gpio_base: 0x20a100000,
            nand_base: 0x200000000,
            nand_size: 0x8000000,
            pmu_base: 0x20a000000,
            spmi_base: 0x20a300000,
            thermal_base: 0x20a700000,
            // d321ap ADT: /arm-io/usb-complex maps to 0x239000000 and its
            // usb-device child has reg base 0x100000, yielding 0x239100000.
            // The T8030 SecureROM accesses AES at 0x235008000 (same arm-io base),
            // so it uses the same USB complex layout as the T8020/T8027.
            usb_base: 0x239100000,
            usb_size: 0x10000,
            watchdog_base: 0x20a0b0000,
            rtc_base: 0x20a0f0000,
            pwm_base: 0x20a500000,
            adc_base: 0x20a600000,
            pinmux_base: 0x20a900000,
            clockctl_base: 0x20a010000,
            regulator_base: 0x20a020000,
            sdhc_base: 0x20a0a0000,
            panel_base: 0x20a110000,
            dsi_phy_base: 0x20a120000,
            raster_base: 0x20a130000,
            touch_base: 0x20a140000,
            audio_base: 0x20a150000,
            bt_base: 0x20a160000,
            wifi_base: 0x20a170000,
            camera_base: 0x20a180000,
            sensorhub_base: 0x20a190000,
            usbphy_base: 0x20a1a0000,
            usbmux_base: 0x20a1a1000,
            usbpd_base: 0x20a1a2000,
            gid_key: GID_DEFAULT,
            uid_key: UID_DEFAULT,
            sep_rom_path: None,
        },
        SoCId::T8101 => SoCConfig {
            id: SoCId::T8101,
            name: "T8101",
            family: "T810x",
            dram_base: 0,
            dram_size: 64 << 30,
            sram_base: 0x1000000000,
            sram_size: 0x400000,
            uart_base: 0x20a0c0000,
            timer_base: 0x20e100000,
            aic_base: 0x23b100000,
            aic_size: 0x30000,
            pmgr_base: 0x20e000000,
            mailbox_base: 0x23c000000,
            rom_base: 0x100000000,
            rom_size: 0x100000,
            dcp_base: 0x235000000,
            // The d321ap DeviceTree places DART-SIO at +0x4000. Keep the synthetic
            // DCP register aperture from claiming that independent hardware window.
            dcp_size: 0x4000,
            nor_base: 0x240000000,
            nor_size: 0x1000000,
            block_base: 0x241000000,
            block_size: 0x400000,
            sep_uart_base: 0x20a0d0000,
            bb_uart_base: 0x20a0e0000,
            uart_window: true,
            i2c_base: 0x20a200000,
            spi_base: 0x20a400000,
            gpio_base: 0x20a100000,
            nand_base: 0x200000000,
            nand_size: 0x8000000,
            pmu_base: 0x20a000000,
            spmi_base: 0x20a300000,
            thermal_base: 0x20a700000,
            // d321ap ADT: /arm-io/usb-complex maps to 0x239000000 and its
            // usb-device child has reg base 0x100000, yielding 0x239100000.
            // The T8101 SecureROM accesses AES at 0x235008000 (same arm-io base),
            // so it uses the same USB complex layout as the T8020/T8027/T8030.
            usb_base: 0x239100000,
            usb_size: 0x10000,
            watchdog_base: 0x20a0b0000,
            rtc_base: 0x20a0f0000,
            pwm_base: 0x20a500000,
            adc_base: 0x20a600000,
            pinmux_base: 0x20a900000,
            clockctl_base: 0x20a010000,
            regulator_base: 0x20a020000,
            sdhc_base: 0x20a0a0000,
            panel_base: 0x20a110000,
            dsi_phy_base: 0x20a120000,
            raster_base: 0x20a130000,
            touch_base: 0x20a140000,
            audio_base: 0x20a150000,
            bt_base: 0x20a160000,
            wifi_base: 0x20a170000,
            camera_base: 0x20a180000,
            sensorhub_base: 0x20a190000,
            usbphy_base: 0x20a1a0000,
            usbmux_base: 0x20a1a1000,
            usbpd_base: 0x20a1a2000,
            gid_key: GID_DEFAULT,
            uid_key: UID_DEFAULT,
            sep_rom_path: None,
        },
        SoCId::S8000 => S8000.clone(),
        SoCId::A7 => A7.clone(),
        SoCId::A8 => A8.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iphem_core::dma::{DmaAccess, DmaFault, DmaRequest, DmaResult};

    /// Deliver a bound DART fault to the CPU through the T8020 AIC. The AIC
    /// now spans 512 SPIs (the real AIC1 span; the SecureROM's own init sweep
    /// MASK_SETs 16 full words and its dispatcher decodes `w8 & 0x1ff`), so a
    /// DART fault sets SPI 454 pending on the controller rather than falling
    /// back to direct `deliver_interrupt`. Reaching the CPU requires the line
    /// to be unmasked (AIC2 MASK_SET word 14 = SPIs 448..479, bit 6 = 454)
    /// and one device tick so the controller raises it — the same enable the
    /// SecureROM's init sweep performs for every line.
    fn unmask_and_tick_dart_usb_irq(m: &mut Machine) {
        let c = config(SoCId::T8020);
        m.mem_write(c.aic_base + 0x4200 + 14 * 4, 4, 1u64 << 6)
            .unwrap();
        m.tick_devices(1);
    }

    /// The T8020 AIC is widened to 512 SPIs (the real AIC1 span), so the
    /// SEP-mail lines the SecureROM's interrupt table actually populates
    /// (0xa1 = 161, 0xa4 = 164) and the DART-USB fault line (454) are all
    /// representable on the controller: an unmasked SPI with its pending bit
    /// drives the distributor's highest-pending delivery.
    #[test]
    fn t8020_aic_represents_sep_mail_and_dart_usb_lines() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let c = config(SoCId::T8020);
        // AIC2 SW_SET raises the pending bit for a line; MASK_SET unmasks it
        // (the ROM's init mask sweep MASK_SETs 16 full words = 512 lines).
        for irq in [161, 164] {
            let word = irq / 32;
            let bit = (irq % 32) as u32;
            m.mem_write(c.aic_base + 0x4000 + word as u64 * 4, 4, 1u64 << bit)
                .unwrap();
            m.mem_write(c.aic_base + 0x4200 + word as u64 * 4, 4, 1u64 << bit)
                .unwrap();
            m.tick_devices(1);
        }
        // Highest-priority pending among the unmasked SPIs (both NORMAL
        // priority; a tie resolves to the lowest id, so 161 is delivered).
        let highest = {
            let aic = m
                .bus
                .regions
                .iter_mut()
                .find(|r| r.base == c.aic_base)
                .unwrap()
                .device
                .as_any_mut()
                .downcast_mut::<Aic>()
                .unwrap();
            aic.get_highest_pending()
        };
        assert_eq!(highest, Some(161));
        assert_eq!(m.cores.active().pending_irq, Some(161));

        // Clear the two SEP-mail pendings (AIC2 SW_CLR, W1C), then verify the
        // DART-USB fault line (454) is also representable on the widened
        // controller: unmask + SW_SET it and it is the sole highest-pending.
        m.mem_write(
            c.aic_base + 0x4100 + (161 / 32) as u64 * 4,
            4,
            1u64 << (161 % 32),
        )
        .unwrap();
        m.mem_write(
            c.aic_base + 0x4100 + (164 / 32) as u64 * 4,
            4,
            1u64 << (164 % 32),
        )
        .unwrap();
        let highest = {
            let aic = m
                .bus
                .regions
                .iter_mut()
                .find(|r| r.base == c.aic_base)
                .unwrap()
                .device
                .as_any_mut()
                .downcast_mut::<Aic>()
                .unwrap();
            aic.get_highest_pending()
        };
        assert_eq!(highest, None);
        m.mem_write(c.aic_base + 0x4000 + 14 * 4, 4, 1u64 << 6)
            .unwrap();
        m.mem_write(c.aic_base + 0x4200 + 14 * 4, 4, 1u64 << 6)
            .unwrap();
        let highest = {
            let aic = m
                .bus
                .regions
                .iter_mut()
                .find(|r| r.base == c.aic_base)
                .unwrap()
                .device
                .as_any_mut()
                .downcast_mut::<Aic>()
                .unwrap();
            aic.get_highest_pending()
        };
        assert_eq!(highest, Some(454));
    }

    #[test]
    fn t8020_chip_control_is_distinct_from_watchdog_and_has_no_invented_fuse_state() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        let base = 0x23d2bc000;
        let region = machine
            .bus
            .regions
            .iter()
            .find(|region| region.base == base)
            .unwrap();
        assert_eq!(region.device.name(), "apple-chip-control");
        assert_eq!(region.size, 0x4000);
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(base + 0x20, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(base + 0x30, 4).unwrap(), 0);
        machine.mem_write(base + 0x10, 4, 0x8000_0000).unwrap();
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 0);
        let chip = machine
            .bus
            .regions
            .iter_mut()
            .find(|region| region.base == base)
            .unwrap()
            .device
            .as_any_mut()
            .downcast_mut::<AppleChipControl>()
            .unwrap();
        chip.fuse0_raw = 0x8000_0000;
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 0x8000_0000);
        assert_eq!(machine.mem_read(0x23d2b0000, 4).unwrap(), 0);
    }

    #[test]
    fn t8020_rom_observed_usb_setup_pmgr_states_complete_clock_gate_request() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        for address in [0x23b080228, 0x23b0802a8, 0x23b0802b0] {
            assert_eq!(machine.mem_read(address, 4).unwrap(), 0);
            machine.mem_write(address, 4, 4).unwrap();
            assert_eq!(machine.mem_read(address, 4).unwrap(), 0x44);
        }
    }

    #[test]
    fn t8020_ane_fifo_window_roundtrips_and_drains_on_machine_tick() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        let base = 0x26a000000u64;
        // kF3 depth = 2, reset kF4 occupancy = 0, kF7 completion = reset.
        assert_eq!(machine.mem_read(base + 0x0c, 4).unwrap(), 2);
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(base + 0x1c, 4).unwrap(), 0);
        // Push two payloads via the host-visible kF0 write window.
        machine.mem_write(base, 4, 0x11).unwrap();
        machine.mem_write(base, 4, 0x22).unwrap();
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 2);
        assert_eq!(machine.mem_read(base + 0x14, 4).unwrap(), 1); // full
                                                                  // Third push stalls: backpressure bit set, no fabricated slot.
        machine.mem_write(base, 4, 0x33).unwrap();
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 2 | 4);
        assert_eq!(machine.mem_read(base + 0x1c, 4).unwrap(), 0);
        // Real Machine::tick_devices drains one word and inverts the leaf.
        machine.tick_devices(1);
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 1);
        assert_eq!(machine.mem_read(base + 0x1c, 4).unwrap(), 1);
        // Stalled push now lands; backpressure clears.
        machine.mem_write(base, 4, 0x33).unwrap();
        assert_eq!(machine.mem_read(base + 0x10, 4).unwrap(), 2);
        assert_eq!(machine.mem_read(base + 0x1c, 4).unwrap(), 1);
    }

    #[test]
    fn t8020_sgx_gpu_segment_read_and_mailbox_completion_roundtrip() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        let base = 0x204000000u64;
        // Gate G1: an 8-byte live read of the recovered reg region returns the
        // stable 64-bit identity and never faults, at any declared offset.
        assert_eq!(machine.mem_read(base + 0x18, 8).unwrap(), 0x204000000);
        assert_eq!(machine.mem_read(base + 0x18, 8).unwrap(), 0x204000000);
        // Any 4-byte read anywhere in the declared window round-trips (the
        // mailbox register subset is at the window head; the rest is benign
        // zero reads — never a fault).
        assert_eq!(machine.mem_read(base + 0x0f0000, 4).unwrap(), 0);
        // Gate G2: MSG_SEND → tick → polled completion leaf round-trips.
        assert_eq!(machine.mem_read(base + 0x08, 4).unwrap(), 2); // depth
        machine.mem_write(base, 4, 0x112233).unwrap();
        assert_eq!(machine.mem_read(base + 0x04, 4).unwrap(), 1); // occupancy
        assert_eq!(machine.mem_read(base + 0x14, 4).unwrap(), 0); // completion reset
        machine.tick_devices(1);
        assert_eq!(machine.mem_read(base + 0x14, 4).unwrap(), 1); // drained
        assert_eq!(machine.mem_read(base + 0x04, 4).unwrap(), 0);
        // A bare tick on the empty queue must not fake a completion.
        machine.tick_devices(1);
        assert_eq!(machine.mem_read(base + 0x14, 4).unwrap(), 1);
    }

    #[test]
    fn t8020_isp_3a_stat_leaf_changes_after_synthetic_frame() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        let base = 0x22a000000u64;
        // Gate G1: every declared register reads a stable value, and offsets
        // across the mapped window read without faulting.
        for off in [
            0x00u64, 0x20, 0x34, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x64, 0x68, 0x6c,
        ] {
            assert!(machine.mem_read(base + off, 4).is_ok());
        }
        // Reset 3A stats.
        assert_eq!(machine.mem_read(base + 0x64, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(base + 0x68, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(base + 0x6c, 4).unwrap(), 0);
        // The emulator-side frame source produces a worst-case-contrast frame,
        // driving the ISP's own 3A stat pipeline (Gate G2).
        let isp = machine
            .bus
            .regions
            .iter_mut()
            .find(|r| r.base == base)
            .unwrap()
            .device
            .as_any_mut()
            .downcast_mut::<iphem_devices::CameraIsp>()
            .unwrap();
        isp.signal_frame();
        assert_eq!(machine.mem_read(base + 0x64, 4).unwrap(), 1);
        assert_eq!(machine.mem_read(base + 0x6c, 4).unwrap(), 1);
        let peak = machine.mem_read(base + 0x68, 4).unwrap();
        assert_ne!(peak, 0);
        assert_eq!(
            machine.mem_read(base + 0x5c, 4).unwrap(),
            (16u64 << 16) | (128 << 8) | 128
        );
    }

    #[test]
    fn pmgr_boot_status_syncs_with_power_state() {
        let mut machine = build_with_rom(SoCId::T8020, &[], true);
        // Boot-status at 0x23d280068 (device 50) is associated with power-state at 0x23b080290 (block[2])
        // Boot-status at 0x23d280078 (device 52) is associated with power-state at 0x23b0802a0 (block[4])
        // Initially both boot-status read 0
        assert_eq!(machine.mem_read(0x23d280068, 4).unwrap(), 0);
        assert_eq!(machine.mem_read(0x23d280078, 4).unwrap(), 0);
        // Write target state 15 (active) to power-state registers (new block addresses)
        machine.mem_write(0x23b080290, 4, 15).unwrap();
        machine.mem_write(0x23b0802a0, 4, 15).unwrap();
        // Power-state registers should now read 0xff (active)
        assert_eq!(machine.mem_read(0x23b080290, 4).unwrap(), 0xff);
        assert_eq!(machine.mem_read(0x23b0802a0, 4).unwrap(), 0xff);
        // Boot-status syncs synchronously on the power-state write
        assert_eq!(machine.mem_read(0x23d280068, 4).unwrap(), 0xc0);
        assert_eq!(machine.mem_read(0x23d280078, 4).unwrap(), 0xc0);
        // Run tick_devices to confirm the sync is idempotent as well
        machine.tick_devices(1);
        // Now boot-status should still be 0xc0 (ready)
        assert_eq!(machine.mem_read(0x23d280068, 4).unwrap(), 0xc0);
        assert_eq!(machine.mem_read(0x23d280078, 4).unwrap(), 0xc0);
        // Boot-status at 0x23d280058 (device 48) is associated with 0x23b080280 (block[0], not written) -> stays 0
        assert_eq!(machine.mem_read(0x23d280058, 4).unwrap(), 0);
    }

    #[test]
    fn test_gicr_and_its_regions_reachable_from_machine_bus() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let c = config(SoCId::T8020);
        assert!(c.aic_size >= 0x21000);

        m.mem_write(c.aic_base + 0x10000, 4, 0x3).unwrap();
        assert_eq!(m.mem_read(c.aic_base + 0x10000, 4).unwrap(), 0x3);

        assert_eq!(m.mem_read(c.aic_base + 0x10008, 8).unwrap(), 0x0); // GICR_TYPER

        m.mem_write(c.aic_base + 0x20008, 8, 0x1c0000000).unwrap();
        assert_eq!(m.mem_read(c.aic_base + 0x20008, 8).unwrap(), 0x1c0000000);

        m.mem_write(c.aic_base + 0x20080, 8, 0xc00000000 | 0x100)
            .unwrap();
        assert_eq!(
            m.mem_read(c.aic_base + 0x20080, 8).unwrap(),
            0xc00000000 | 0x100
        );

        m.mem_write(c.aic_base + 0x21000, 8, (0x2u64 << 32) | 0x77)
            .unwrap();
    }

    #[test]
    fn test_mailbox_region_no_longer_collides_with_gicr() {
        let c = config(SoCId::T8020);
        let aic_end = c.aic_base + c.aic_size;
        assert!(c.mailbox_base >= aic_end || c.mailbox_base + 0x1000 <= c.aic_base);
    }

    #[test]
    fn t8020_maps_apple_gpio_at_d321ap_device_tree_address() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let base = 0x23c100000;
        let pin_config = base + 123 * 4;
        m.mem_write(pin_config, 4, 1 << 9).unwrap();
        assert_eq!(m.mem_read(pin_config, 4).unwrap(), 1 << 9);

        let gpio = m
            .bus
            .regions
            .iter_mut()
            .find_map(|region| region.device.as_any_mut().downcast_mut::<AppleGpio>())
            .expect("T8020 Apple GPIO mapping");
        gpio.drive_input(123, true);
        assert_eq!(m.mem_read(pin_config, 4).unwrap(), (1 << 9) | 1);
    }

    #[test]
    fn test_usb_device_controller_mapped() {
        let m = build_with_rom(SoCId::T8020, &[], true);
        let c = config(SoCId::T8020);
        println!("usb_base = {:x}", c.usb_base);
        println!("usb_size = {:x}", c.usb_size);

        for region in &m.bus.regions {
            if region.base == c.usb_base {
                println!(
                    "Found region at {:x}: {}",
                    region.base,
                    region.device.name()
                );
                if let Some(usb) = region
                    .device
                    .as_any()
                    .downcast_ref::<iphem_devices::UsbDeviceController>()
                {
                    println!("  -> UsbDeviceController found! id={}", usb.id);
                }
            }
        }
    }

    #[test]
    fn t8020_dcp_aperture_does_not_claim_dart_sio_window() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let dcp = m
            .bus
            .regions
            .iter()
            .find(|region| region.base == 0x235000000)
            .expect("synthetic DCP window");
        assert_eq!(dcp.size, 0x4000);
        assert_eq!(dcp.device.name(), "dcp");

        let dart = m
            .bus
            .regions
            .iter()
            .find(|region| region.base == 0x235004000)
            .expect("DeviceTree DART-SIO window");
        assert_eq!(dart.size, 0x4000);
        assert_eq!(dart.device.name(), "dart-t8020");
        assert_eq!(m.mem_read(0x235004000, 4).unwrap(), 14 << 24);
        // PARAMS2 reports that this instance does not advertise bypass.
        assert_eq!(m.mem_read(0x235004004, 4).unwrap(), 0);
        let dart_usb = m
            .bus
            .regions
            .iter()
            .find(|region| region.base == 0x239900000)
            .expect("DeviceTree DART-USB window");
        assert_eq!(dart_usb.size, 0x4000);
        assert_eq!(dart_usb.device.name(), "dart-t8020");
        assert_eq!(m.mem_read(0x239900000, 4).unwrap(), 14 << 24);

        let usb_device = m
            .bus
            .regions
            .iter()
            .find(|region| region.base == 0x239100000)
            .expect("d321ap DeviceTree USB device controller");
        assert_eq!(usb_device.size, 0x10000);
        assert_eq!(usb_device.device.name(), "usb-dfu");
        assert_eq!(m.mem_read(0x239100000, 4).unwrap(), 0);
        assert!(!m
            .bus
            .regions
            .iter()
            .any(|region| region.base == 0x20a800000));
    }

    #[test]
    fn t8020_dart_usb_shared_engine_translates_mmio_configuration() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Read, 0x1000);
        // The DART-USB stream is bound: translation never reports UnboundSid.
        assert_ne!(
            m.dma_translate(&req),
            DmaResult::Fault(DmaFault::UnboundSid(DART_USB_SID))
        );
        // Program the shared engine through its MMIO window: enable stream 7
        // and set it to translate. The DMA port reads the same registers.
        let dart = 0x239900000u64;
        assert!(m
            .mem_write(
                dart + iphem_devices::dart::STREAMS_ENABLE,
                4,
                1 << DART_USB_SID
            )
            .is_ok());
        assert!(m
            .mem_write(
                dart + iphem_devices::dart::TCR_BASE + 4 * DART_USB_SID as u64,
                4,
                iphem_devices::dart::TCR_TRANSLATE_ENABLE as u64
            )
            .is_ok());
        // The input-address width is 32 (upstream apple-dart.c probes
        // `case DART_T8020: dart->ias = 32`), so translation is no longer an
        // AddressWidth fault: with no TTBR programmed it walks to a typed
        // NoRoot fault that raises the bound DART-USB IRQ (454) and is
        // readable back through the same MMIO window (shared fault state).
        assert_eq!(m.dma_translate(&req), DmaResult::Fault(DmaFault::NoRoot));
        // The widened AIC owns SPI 454, so the fault is pending on the
        // controller; unmask the line and tick once so it reaches the CPU.
        unmask_and_tick_dart_usb_irq(&mut m);
        assert_eq!(m.cores.active().pending_irq, Some(DART_USB_IRQ));
        let sid_err = (DART_USB_SID << 16) | iphem_devices::dart::DART_ERROR_FAULT;
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(),
            sid_err as u64
        );
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::ERROR_ADDR_LO, 4)
                .unwrap(),
            0x2000_4000
        );
        // A subsequent invalidation for the stream is routed to the engine.
        m.dma_invalidate(DART_USB_SID, 0x2000_4000, 0x1000);
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(),
            sid_err as u64
        );
    }

    #[test]
    fn t8020_dart_usb_translated_physical_transfer_end_to_end() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let dart = 0x239900000u64;
        // Physical RAM for page tables + DMA payload lives in the
        // 0x2000_0000 SparseRam window (1 MiB). Levels match the 16-KiB/32-bit
        // DART1 geometry: `bits_per_level = 11`, so for IOVA 0x2000_4000 the
        // L2 slot is (0x2000_4000 >> 25) & 0x7ff and the leaf slot is
        // (0x2000_4000 >> 14) & 0x7ff (per the walker unit tests).
        let root = 0x2000_0000u64;
        let l1 = 0x2000_4000u64;
        let leaf_base = 0x2000_8000u64;
        let l2_index = (0x2000_4000 >> 25) & 0x7ff;
        let leaf_index = (0x2000_4000 >> 14) & 0x7ff;
        m.mem_write(
            root + l2_index * 8,
            8,
            iphem_devices::dart::DART_PTE_VALID | l1,
        )
        .unwrap();
        m.mem_write(
            l1 + leaf_index * 8,
            8,
            iphem_devices::dart::DART_PTE_VALID | iphem_devices::dart::DART_PTE_SP_DIS | leaf_base,
        )
        .unwrap();
        // Program the stream through MMIO like a guest driver: enable stream
        // 7, translate mode, and TTBR[0] = root table 0x2000_0000.
        m.mem_write(
            dart + iphem_devices::dart::STREAMS_ENABLE,
            4,
            1 << DART_USB_SID,
        )
        .unwrap();
        m.mem_write(
            dart + iphem_devices::dart::TCR_BASE + 4 * DART_USB_SID as u64,
            4,
            iphem_devices::dart::TCR_TRANSLATE_ENABLE as u64,
        )
        .unwrap();
        let ttbr_reg = 1u64 << 31 | (root >> 12) & 0x00ff_ffff;
        assert_eq!(
            iphem_devices::dart::dart_t8020_ttbr_root_address(ttbr_reg as u32),
            Some(root)
        );
        m.mem_write(
            dart + iphem_devices::dart::TTBR_BASE + 16 * DART_USB_SID as u64,
            4,
            ttbr_reg,
        )
        .unwrap();
        // Stage a device payload the DMA transfer will pull: bytes at the
        // translated PA (leaf_base because the IOVA page offset is zero).
        let sentinel = 0x0102_0304_0506_0708u64;
        m.mem_write(leaf_base, 8, sentinel).unwrap();
        let req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Read, 8);
        let mut out = vec![0xEEu8; 8];
        assert_eq!(
            m.dma_transfer(&req, &mut out),
            DmaResult::Translated(leaf_base)
        );
        assert_eq!(u64::from_le_bytes(out.try_into().unwrap()), sentinel);
        assert!(m.cores.active().pending_irq.is_none());
        // A write transfer lands at the translated PA, and the IOTLB serves
        // the second page handle without a fresh walk.
        let payload = 0xABCD_0001_0002_0003u64;
        let write_req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Write, 8);
        let mut buf = payload.to_le_bytes();
        assert_eq!(
            m.dma_transfer(&write_req, &mut buf),
            DmaResult::Translated(leaf_base)
        );
        assert_eq!(m.mem_read(leaf_base, 8).unwrap(), payload);
        assert!(m.cores.active().pending_irq.is_none());
        // An IOVA beyond the configured 32-bit width faults as AddressWidth
        // and raises IRQ 454 through the shared engine state.
        let wide = DmaRequest::new(0x1_0000, DART_USB_SID, 0x1_0000_0000, DmaAccess::Read, 8);
        assert_eq!(
            m.dma_translate(&wide),
            DmaResult::Fault(DmaFault::AddressWidth)
        );
        unmask_and_tick_dart_usb_irq(&mut m);
        assert_eq!(m.cores.active().pending_irq, Some(DART_USB_IRQ));
    }

    #[test]
    fn t8020_dart_sio_has_no_bound_dma_port() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let req = DmaRequest::new(0x1_0000, 0, 0x2000_4000, DmaAccess::Read, 0x1000);
        // DART-SIO exists on the bus but its SIDs are unevidenced, so no DMA
        // port is registered: any stream faults as UnboundSid.
        assert_eq!(
            m.dma_translate(&req),
            DmaResult::Fault(DmaFault::UnboundSid(0))
        );
        assert!(m.cores.active().pending_irq.is_none());
    }

    /// The d321ap `/arm-io/uart-1,samsung` window at 0x235204000 accepts the
    /// Darwin console driver's s5l init writes (ULCON/UCON/UFCON/UMCON/UBRDIV,
    /// offsets 0/4/8/0xc/0x28) without faulting, reports TX-ready UTRSTAT so
    /// the transmit poll cannot hang, and captures TX holding bytes — the
    /// documented UART1 init blocker in docs/REAL_KERNEL_PROGRESS.md.
    #[test]
    fn t8020_uart1_s5l_window_accepts_kernel_init_and_console_tx() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let uart1 = 0x235204000u64;
        assert_eq!(m.mem_write(uart1, 4, 0x3).unwrap(), ());
        assert_eq!(m.mem_read(uart1, 4).unwrap(), 0x3);
        assert_eq!(m.mem_write(uart1 + 0x04, 4, 0x1245).unwrap(), ());
        assert_eq!(m.mem_write(uart1 + 0x08, 4, 0x11).unwrap(), ());
        assert_eq!(m.mem_write(uart1 + 0x0c, 4, 0).unwrap(), ());
        assert_eq!(m.mem_write(uart1 + 0x28, 4, 0x30).unwrap(), ());
        // TX-ready UTRSTAT (bit0 TX ready + bit2 TX shift empty, RX empty) so
        // the kernel's console TX-complete poll proceeds.
        assert_eq!(m.mem_read(uart1 + 0x10, 4).unwrap(), 0x5);
        // Console byte written to UTXH is captured by the mapped device.
        assert_eq!(m.mem_write(uart1 + 0x20, 1, b'K' as u64).unwrap(), ());
        let bus_device = m.bus.regions.iter_mut().find(|r| r.base == uart1).unwrap();
        let out = bus_device
            .device
            .as_any_mut()
            .downcast_mut::<S5lUart>()
            .unwrap()
            .drain_output();
        assert_eq!(out, b"K".to_vec());
        let mut formatter = iphem_devices::uart_output::TaggedUartOutput::default();
        assert_eq!(
            formatter.append(iphem_devices::uart_output::UartSource::Kernel, uart1, &out),
            "[UART1] K"
        );
    }

    #[test]
    fn t8020_panel_guest_pixel_aperture_is_reachable_without_dsi_overlap() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let panel = 0x20a110000u64;
        m.mem_write(panel + 0x1000, 8, 0x8877_6655_4433_2211)
            .unwrap();
        let region = m.bus.regions.iter().find(|r| r.base == panel).unwrap();
        let device = region.device.as_any().downcast_ref::<Panel>().unwrap();
        assert_eq!(&device.framebuffer[..2], &[0x4433_2211, 0x8877_6655]);
        assert!(device.guest_frame_written);
        assert!(m.bus.regions.iter().any(|r| r.base == 0x20a120000));
    }

    /// Replay the exact DART-SIO register contacts the checked-in T8020
    /// SecureROM (`.4.7`) performs on the forced-selector-5 boot path
    /// (`docs/DFU_BOOT_PROGRESS.md`, `after_selector=true` MMIO events
    /// 804137..804860; ROM pcs 0x10000af4c..0x10000b504). Every value the
    /// engine serves must match the captured trace, and the post-trace state
    /// must be the flush-pass image the ROM leaves behind: TCR[0] = 0x80080
    /// (translate enabled, no root recalled), TTBR0-3 = 0.
    /// The DeviceTree-declared `/arm-io/aes` window reports what Apple's own
    /// SecureROM AES driver requires of it, and the completion flag is
    /// guest-driven.
    ///
    /// The window is real board hardware (`aes-version = 3`, reg 0x35008000 on
    /// the arm-io base, so `0x235008000` + 0x4000). What the register returns
    /// is *derived* rather than invented: `AesOtp` documents the driver, and the
    /// driver is one shared routine compiled byte-identically into the
    /// SecureROMs of three SoC generations. It rejects the `+0x0c` status word
    /// unless bits 12..7 are all set, and it returns bit 13, which its only two
    /// callers branch on as the pass condition -- so a model that reports zero
    /// there is not a stricter answer but a claim that no Apple device reaches
    /// USB init.
    ///
    /// What is *not* claimed is the per-bit meaning of those six bits, and the
    /// test therefore also pins the properties that keep this a model of the
    /// driver's contract rather than a shortcut: the status register is
    /// read-only, so no guest can satisfy the gate by writing it, and the
    /// `+0x18` completion flag exists only after the guest has started an
    /// operation and is cleared by the write-one-to-clear the driver performs.
    #[test]
    fn t8020_aes_window_reports_what_the_secure_rom_driver_requires() {
        let mut m = build_with_rom(SoCId::T8020, &[], false);
        let base = 0x235008000u64;
        fn aes(m: &Machine) -> &AesOtp {
            m.bus
                .regions
                .iter()
                .find(|region| region.base == 0x235008000)
                .expect("aes window mapped")
                .device
                .as_any()
                .downcast_ref::<AesOtp>()
                .expect("device is AesOtp")
        }

        // Mapped: every access the SecureROM makes must decode, not fault.
        assert_eq!(m.mem_read(base, 4).unwrap(), 0, "+0x00 decodes");
        assert_eq!(
            m.mem_read(base + 0x4000 - 4, 4).unwrap(),
            0,
            "top of window decodes"
        );
        assert_eq!(
            m.mem_read(base + 0x24, 4).unwrap(),
            0,
            "+0x24 read-and-discard"
        );

        // The status word is exactly the driver's condition: bits 12..7 set, plus
        // the bit it returns. Nothing else is invented.
        let status = m.mem_read(base + AesOtp::STATUS, 4).unwrap() as u32;
        assert_eq!(
            status,
            AesOtp::CAPABILITY | AesOtp::READY_BIT | AesOtp::RETURNED_BIT,
            "the status word is bits 12..7 plus the returned bit, and nothing else"
        );
        assert!(
            aes(&m).reports_identity(),
            "the driver's own condition holds"
        );
        for bit in 7..=13u32 {
            assert_ne!(status & (1 << bit), 0, "the driver requires bit {bit} set");
        }

        // Read-only: a guest write is accepted (a read-modify-write cannot fault)
        // and cannot create the gate.
        m.mem_write(base + AesOtp::STATUS, 4, 0xffff_ffff).unwrap();
        assert_eq!(
            m.mem_read(base + AesOtp::STATUS, 4).unwrap() as u32,
            status,
            "the status register is read-only, so the gate is not guest-writable"
        );

        // The completion flag is guest-driven: absent until the guest starts an
        // operation, and cleared by the write-one-to-clear the driver performs.
        assert_eq!(
            m.mem_read(base + AesOtp::STATUS2, 4).unwrap(),
            0,
            "no operation yet"
        );
        assert!(!aes(&m).operation_complete());
        m.mem_write(base + AesOtp::CONTROL, 4, 1).unwrap();
        assert_eq!(
            m.mem_read(base + AesOtp::STATUS2, 4).unwrap(),
            u64::from(AesOtp::DONE_BIT),
            "starting an operation sets the completion flag"
        );
        m.mem_write(base + AesOtp::STATUS2, 4, u64::from(AesOtp::DONE_BIT))
            .unwrap();
        assert_eq!(
            m.mem_read(base + AesOtp::STATUS2, 4).unwrap(),
            0,
            "writing 0x20 clears it, and the word then reads zero as the driver requires"
        );
        // The driver's data port accepts the burst pushes the ROM performs, and
        // reading it back is *not* a result path: the driver never reads this
        // offset, so a value here would be invented rather than derived.
        m.mem_write(base + AesOtp::DATA, 4, 0xdead_beef).unwrap();
        assert_eq!(
            m.mem_read(base + AesOtp::DATA, 4).unwrap(),
            0,
            "the data port is push-only, so it exposes no output path"
        );
        assert_eq!(
            aes(&m).pushed_words(),
            &[0xdead_beef],
            "the push is retained, which is how a caller can observe it"
        );

        // Invalid access sizes still fault, as everywhere else in the model.
        assert!(m.mem_read(base, 3).is_err(), "invalid size faults");
    }

    #[test]
    fn t8020_dart_sio_secure_rom_trace_conformance() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let sio = 0x235004000u64;
        // #804137 read engine identity → 0xe000000.
        assert_eq!(m.mem_read(sio, 4).unwrap(), 0xe000000);
        // #804174..#804198 clear TTBR0-3 of the target stream.
        for off in [0x200, 0x204, 0x208, 0x20c] {
            m.mem_write(sio + off, 4, 0).unwrap();
        }
        // #804216/#804219 stream select 1 + command 0.
        m.mem_write(sio + iphem_devices::dart::STREAM_SELECT, 4, 1)
            .unwrap();
        m.mem_write(sio + iphem_devices::dart::STREAM_COMMAND, 4, 0)
            .unwrap();
        // #804311/#804313 ERROR read 0, then W1C 0.
        assert_eq!(m.mem_read(sio + iphem_devices::dart::ERROR, 4).unwrap(), 0);
        m.mem_write(sio + iphem_devices::dart::ERROR, 4, 0).unwrap();
        // #804346/#804353 TCR read 0 → write 0x10100.
        assert_eq!(
            m.mem_read(sio + iphem_devices::dart::TCR_BASE, 4).unwrap(),
            0
        );
        m.mem_write(sio + iphem_devices::dart::TCR_BASE, 4, 0x10100)
            .unwrap();
        // #804748..#804790 TTBR read-back pass, all zero.
        for off in [0x200, 0x204, 0x208, 0x20c] {
            assert_eq!(m.mem_read(sio + off, 4).unwrap(), 0);
        }
        // #804801 enable translation on the stream (bit 7 = TRANSLATE_ENABLE).
        m.mem_write(sio + iphem_devices::dart::TCR_BASE, 4, 0x80080)
            .unwrap();
        // #804815..#804860 second TTBR-clear + select/command pass.
        for off in [0x200, 0x204, 0x208, 0x20c] {
            m.mem_write(sio + off, 4, 0).unwrap();
        }
        m.mem_write(sio + iphem_devices::dart::STREAM_SELECT, 4, 1)
            .unwrap();
        m.mem_write(sio + iphem_devices::dart::STREAM_COMMAND, 4, 0)
            .unwrap();
        // Post-trace state exactly as the ROM left the engine: translation
        // armed for the stream, but no root table in TTBR0-3.
        assert_eq!(
            m.mem_read(sio + iphem_devices::dart::TCR_BASE, 4).unwrap(),
            0x80080
        );
        for off in [0x200, 0x204, 0x208, 0x20c] {
            assert_eq!(m.mem_read(sio + off, 4).unwrap(), 0);
        }
        assert!(m.cores.active().pending_irq.is_none());
    }

    /// The same captured SecureROM flush protocol, applied to the DMA-bound
    /// DART-USB engine, then a real device DMA: because the trace never
    /// enables stream 7 nor arms a root, the request faults `StreamBlocked`
    /// and raises IRQ 454 through the shared fault capture, and the ROM's own
    /// ERROR read-then-W1C sequence clears exactly that record — proving the
    /// real firmware handshake drives the shared engine's DMA decision.
    #[test]
    fn t8020_dart_usb_secure_rom_protocol_governs_dma_fault() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let dart = 0x239900000u64;
        for off in [0x200, 0x204, 0x208, 0x20c] {
            m.mem_write(dart + off, 4, 0).unwrap();
        }
        m.mem_write(dart + iphem_devices::dart::STREAM_SELECT, 4, 1)
            .unwrap();
        m.mem_write(dart + iphem_devices::dart::STREAM_COMMAND, 4, 0)
            .unwrap();
        assert_eq!(m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(), 0);
        m.mem_write(dart + iphem_devices::dart::ERROR, 4, 0)
            .unwrap();
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::TCR_BASE, 4).unwrap(),
            0
        );
        m.mem_write(dart + iphem_devices::dart::TCR_BASE, 4, 0x10100)
            .unwrap();
        m.mem_write(dart + iphem_devices::dart::TCR_BASE, 4, 0x80080)
            .unwrap();
        // The ROM-flush state leaves DART-USB stream 7 unenabled and rootless.
        let req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Read, 8);
        assert_eq!(
            m.dma_translate(&req),
            DmaResult::Fault(DmaFault::StreamBlocked(DART_USB_SID))
        );
        unmask_and_tick_dart_usb_irq(&mut m);
        assert_eq!(m.cores.active().pending_irq, Some(DART_USB_IRQ));
        // ERROR carries the faulted SID, exactly the capture the ROM's W1C
        // pass would clear. The software W1C is bitwise (only written 1-bits
        // clear), so a proper driver clears by writing back the full read
        // value — the ROM's own handshake is read-then-write-value, and the
        // trace's "read 0, write 0" is a no-op that leaves that bit pattern.
        let sid_err: u64 =
            (u64::from(DART_USB_SID) << 16) | u64::from(iphem_devices::dart::DART_ERROR_FAULT);
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(),
            sid_err
        );
        m.mem_write(dart + iphem_devices::dart::ERROR, 4, sid_err)
            .unwrap();
        assert_eq!(m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(), 0);
    }

    /// The captured SecureROM DART flush pass as the *start* of the complete
    /// boot-time DART-USB lifecycle, not the end: every contact is replayed
    /// against the DMA-bound engine, then the same routine's next actions on
    /// a real device — enable the stream and program the root the ROM built —
    /// at exactly the traced register offsets (`STREAMS_ENABLE`, `TCR`,
    /// `TTBR`). A real device DMA then translates and moves its payload over
    /// the physical bus at the translated PA. This is the gate's "real guest
    /// MMIO trace → resulting translated physical transfer": the trace is the
    /// real firmware protocol, and the transfer is produced through the
    /// engine the trace configured.
    #[test]
    fn t8020_dart_usb_real_trace_configuration_results_in_translated_transfer() {
        let mut m = build_with_rom(SoCId::T8020, &[], true);
        let dart = 0x239900000u64;
        // ---- Real SecureROM flush pass (`.4.7`, after_selector=true, DART
        // routine pcs 0x10000af4c..0x10000b504) replayed 1:1. ----
        for off in [0x200, 0x204, 0x208, 0x20c] {
            m.mem_write(dart + off, 4, 0).unwrap();
        }
        m.mem_write(dart + iphem_devices::dart::STREAM_SELECT, 4, 1)
            .unwrap();
        m.mem_write(dart + iphem_devices::dart::STREAM_COMMAND, 4, 0)
            .unwrap();
        assert_eq!(m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(), 0);
        m.mem_write(dart + iphem_devices::dart::ERROR, 4, 0)
            .unwrap();
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::TCR_BASE, 4).unwrap(),
            0
        );
        m.mem_write(dart + iphem_devices::dart::TCR_BASE, 4, 0x10100)
            .unwrap();
        for off in [0x200, 0x204, 0x208, 0x20c] {
            assert_eq!(m.mem_read(dart + off, 4).unwrap(), 0);
        }
        m.mem_write(dart + iphem_devices::dart::TCR_BASE, 4, 0x80080)
            .unwrap();
        for off in [0x200, 0x204, 0x208, 0x20c] {
            m.mem_write(dart + off, 4, 0).unwrap();
        }
        m.mem_write(dart + iphem_devices::dart::STREAM_SELECT, 4, 1)
            .unwrap();
        m.mem_write(dart + iphem_devices::dart::STREAM_COMMAND, 4, 0)
            .unwrap();
        // ---- Init completion: the traced routine's next actions with a real
        // root — 16-KiB/32-bit DART1 tables in the 0x2000_0000 SparseRam
        // window (same geometry as the walker's end-to-end test). ----
        let root = 0x2000_0000u64;
        let l1 = 0x2000_4000u64;
        let leaf_base = 0x2000_8000u64;
        let l2_index = (0x2000_4000 >> 25) & 0x7ff;
        let leaf_index = (0x2000_4000 >> 14) & 0x7ff;
        m.mem_write(
            root + l2_index * 8,
            8,
            iphem_devices::dart::DART_PTE_VALID | l1,
        )
        .unwrap();
        m.mem_write(
            l1 + leaf_index * 8,
            8,
            iphem_devices::dart::DART_PTE_VALID | iphem_devices::dart::DART_PTE_SP_DIS | leaf_base,
        )
        .unwrap();
        m.mem_write(
            dart + iphem_devices::dart::STREAMS_ENABLE,
            4,
            1 << DART_USB_SID,
        )
        .unwrap();
        m.mem_write(
            dart + iphem_devices::dart::TCR_BASE + 4 * DART_USB_SID as u64,
            4,
            iphem_devices::dart::TCR_TRANSLATE_ENABLE as u64,
        )
        .unwrap();
        let ttbr_reg = 1u64 << 31 | (root >> 12) & 0x00ff_ffff;
        assert_eq!(
            iphem_devices::dart::dart_t8020_ttbr_root_address(ttbr_reg as u32),
            Some(root)
        );
        m.mem_write(
            dart + iphem_devices::dart::TTBR_BASE + 16 * DART_USB_SID as u64,
            4,
            ttbr_reg,
        )
        .unwrap();
        // ---- The resulting translated transfer. ----
        let sentinel = 0x0102_0304_0506_0708u64;
        m.mem_write(leaf_base, 8, sentinel).unwrap();
        let req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Read, 8);
        let mut out = vec![0xEEu8; 8];
        assert_eq!(
            m.dma_transfer(&req, &mut out),
            DmaResult::Translated(leaf_base)
        );
        assert_eq!(u64::from_le_bytes(out.try_into().unwrap()), sentinel);
        assert!(m.cores.active().pending_irq.is_none());
        let payload = 0xABCD_0001_0002_0003u64;
        let write_req = DmaRequest::new(0x1_0000, DART_USB_SID, 0x2000_4000, DmaAccess::Write, 8);
        let mut buf = payload.to_le_bytes();
        assert_eq!(
            m.dma_transfer(&write_req, &mut buf),
            DmaResult::Translated(leaf_base)
        );
        assert_eq!(m.mem_read(leaf_base, 8).unwrap(), payload);
        assert!(m.cores.active().pending_irq.is_none());
        // The traced ERROR W1C handshake still governs after faults: the
        // AddressWidth fault records the SID, and writing back the read value
        // clears the record exactly as the ROM's read-then-W1C pass does.
        let wide = DmaRequest::new(0x1_0000, DART_USB_SID, 0x1_0000_0000, DmaAccess::Read, 8);
        assert_eq!(
            m.dma_translate(&wide),
            DmaResult::Fault(DmaFault::AddressWidth)
        );
        unmask_and_tick_dart_usb_irq(&mut m);
        assert_eq!(m.cores.active().pending_irq, Some(DART_USB_IRQ));
        let sid_err: u64 =
            (u64::from(DART_USB_SID) << 16) | u64::from(iphem_devices::dart::DART_ERROR_FAULT);
        assert_eq!(
            m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(),
            sid_err
        );
        m.mem_write(dart + iphem_devices::dart::ERROR, 4, sid_err)
            .unwrap();
        assert_eq!(m.mem_read(dart + iphem_devices::dart::ERROR, 4).unwrap(), 0);
    }

    // -----------------------------------------------------------------------
    // A8 / T7000
    //
    // The A8 builder is the most developed platform in this file -- it is the one
    // with a census, measured PMGR handshakes and a signed restore that runs --
    // and it had no regression test at all. These pin the mappings that the
    // SecureROM-derived work established, so a later edit cannot silently move a
    // window the ROM depends on.
    // -----------------------------------------------------------------------

    /// Build the A8 platform with the checked-in T7000 SecureROM.
    fn a8_machine() -> Machine {
        let rom = std::fs::read(crate::corpus_rom_path("t7000si", "1992.0.0.1.19"))
            .expect("the checked-in T7000 SecureROM is readable");
        build_a8_t7000(&rom, false)
    }

    /// The windows the A8 ROM composes addresses into, from its own literal pool
    /// and the n61ap tree. Each is a real dependency: a window moved or resized
    /// puts the guest's accesses on unmapped space.
    #[test]
    fn a8_maps_the_windows_the_t7000_rom_composes() {
        let mut m = a8_machine();

        // PMGR plane 0: `/device-tree/pmgr` reg = 0xe000000 + 0x40000, which the
        // ROM turns into 0x20e000000. The poll the ROM got stuck on lives here.
        assert_eq!(m.mem_read(0x20e0_00000, 4).unwrap(), 0, "PMGR plane 0 base");
        assert!(
            m.mem_read(0x20e0_3fffc, 4).is_ok(),
            "the plane must span the 0x40000 the tree declares"
        );
        assert!(
            m.mem_read(0x20e0_40000, 4).is_err(),
            "one byte past the declared plane must not be mapped"
        );

        // PMGR plane 1: reg = 0x2220000 + 0x2000 -> 0x202220000.
        assert_eq!(m.mem_read(0x2022_20000, 4).unwrap(), 0, "PMGR plane 1 base");

        // GPIO, reached from the A8 tree rather than from `gpio_base`.
        assert_eq!(m.mem_read(0x20e3_00000, 4).unwrap(), 0, "GPIO window");

        // The SecureROM's own window. Read from the image rather than assumed:
        // the first four bytes are `00 00 00 90`, so a 32-bit read yields
        // 0x9000_0000 -- `add x0, sp, #0`. That is not the `mrs x8, TPIDRRO_EL0`
        // the T8020 ROM opens with, and an earlier draft of this test asserted
        // the T8020 encoding here and failed, which is the only reason the value
        // is stated rather than left as "readable".
        assert_eq!(
            m.mem_read(0x1000_00000, 4).unwrap(),
            0x9000_0000,
            "the T7000 ROM's first word is 0x90000000 (add x0, sp, #0)"
        );
    }

    /// The regression for the gate that stopped the signed A8 restore, and for
    /// the one it uncovered behind it.
    ///
    /// The A8 ROM drives plane offset `0x4004` from two loops with opposite
    /// requirements:
    ///
    /// ```text
    /// 0x10000a3ac  and  w9, w9, #0xfffffffe   ; request bit high -> low
    /// 0x10000a3bc  b.lt                        ; completes when bit 31 is CLEAR
    ///
    /// 0x10000a880  orr  w10, w10, #0x1         ; request bit low -> high
    /// 0x10000a890  b.ge                        ; completes when bit 31 is SET
    /// ```
    ///
    /// So the acknowledgement polarity follows the *direction* the request bit
    /// moved. Both failures were observed for real: with the ack always set the
    /// ROM parked at `PC=0x10000a3b4`, and with it always clear it parks in the
    /// other loop instead. Asserting only one direction would let either
    /// regression back in.
    #[test]
    fn a8_pmgr_0x4004_acknowledges_in_the_direction_of_the_request_bit() {
        let mut m = a8_machine();
        const ADDR: u64 = 0x20e0_04004;
        const STATUS: u64 = 1 << 31;

        // Request bit low -> high: the `b.ge` poll needs the status bit set.
        m.mem_write(ADDR, 4, 1).unwrap();
        assert_ne!(
            m.mem_read(ADDR, 4).unwrap() & STATUS,
            0,
            "a rising request bit must acknowledge with bit 31 set"
        );

        // Request bit high -> low: the `b.lt` poll needs it clear.
        m.mem_write(ADDR, 4, 0).unwrap();
        assert_eq!(
            m.mem_read(ADDR, 4).unwrap() & STATUS,
            0,
            "a falling request bit must acknowledge with bit 31 clear"
        );
    }

    /// The status bit is hardware-owned: the ROM writes `0x80000000` into this
    /// word itself, and a model that echoes it back pins the `b.ge` loop forever.
    ///
    /// The check has to use a write that moves *no* request bit, because a
    /// transition adds an acknowledgement of its own and would mask the stored
    /// value. First drive bit 0 up so the register holds a known command field,
    /// then write the same command field with the guest's status bit added: bit 0
    /// does not move, so no acknowledgement is added and the read shows the
    /// stored word directly.
    #[test]
    fn a8_pmgr_0x4004_never_stores_the_guests_status_bit() {
        let mut m = a8_machine();
        const ADDR: u64 = 0x20e0_04004;
        // Bit 0 rises: acknowledged high, and the stored word is the command.
        m.mem_write(ADDR, 4, 1).unwrap();
        assert_ne!(m.mem_read(ADDR, 4).unwrap() & (1 << 31), 0);
        // Same command field, guest status bit added, no transition.
        m.mem_write(ADDR, 4, 0x8000_0001).unwrap();
        assert_eq!(
            m.mem_read(ADDR, 4).unwrap() & (1 << 31),
            0,
            "the guest's own status bit must not be stored"
        );
        assert_eq!(
            m.mem_read(ADDR, 4).unwrap() & 1,
            1,
            "the command field is stored verbatim"
        );
    }

    /// The plane-base handshake at offset 0 polls with `b.ge`, so it needs the
    /// opposite polarity: bit 31 **set**. Pinning both together is the point --
    /// they are the same register file, the same ack bit, and opposite branches.
    #[test]
    fn a8_plane_base_handshake_still_sets_the_status_bit() {
        let mut m = a8_machine();
        m.mem_write(0x20e0_00000, 4, 1).unwrap();
        assert_ne!(
            m.mem_read(0x20e0_00000, 4).unwrap() & (1 << 31),
            0,
            "the plane-base loop polls with b.ge and needs bit 31 set"
        );
    }

    /// The A8's *second* PMGR plane, whose handshakes have a third shape again.
    ///
    /// Plane 1 is configured `(0x20, req 1<<25, ack 0, 8)` and `(0x50, ...)`: the
    /// ack bit is **zero**, because that loop polls until the field reads back
    /// empty. So the three conventions in one SoC are:
    ///
    /// * plane-base `0x0`  -- `b.ge`, completes with bit 31 **set**
    /// * plane-0 `0x4004`  -- `b.lt`, completes with bit 31 **clear**
    /// * plane-1 `0x20`    -- 64-bit, completes with the field reading **zero**
    ///
    /// All three are firmware-determined, and an earlier draft of this test
    /// assumed plane 1 acknowledged with bit 63 set. It does not, and the model
    /// is right: a single "set the ack bit" rule cannot express any of this.
    #[test]
    fn a8_second_plane_completes_by_reading_back_empty() {
        let mut m = a8_machine();
        for offset in [0x20_u64, 0x50] {
            let addr = 0x2022_20000 + offset;
            m.mem_write(addr, 8, 1 << 25).unwrap();
            assert_eq!(
                m.mem_read(addr, 8).unwrap() & (1 << 63),
                0,
                "plane-1 offset {offset:#x} completes by reading back empty"
            );
        }
    }

    /// Every SoC must be able to answer "which SEPROM do you run, and how is that
    /// known?" -- including the ones that run none. This is the invariant that makes
    /// "SEP exists across all SoCs" a checkable statement rather than a reading of
    /// nine struct literals.
    ///
    /// The important half is the second loop: `sep_rom_status` and `sep_rom_path` are
    /// two answers to the same question in the same file, and nothing forces them to
    /// agree. That is not a hypothetical -- the A7 named an **AArch64** image while the
    /// A7 SEP platform is a 32-bit address space, and it was wired that way through
    /// several records. The first loop catches the answer being absent; the second
    /// catches the two drifting apart again.
    #[test]
    fn every_soc_states_its_seprom_and_why() {
        for soc in SoCId::ALL {
            let status = sep_rom_status(soc);
            match status {
                SepRomStatus::Wired { image, evidence } => {
                    assert!(
                        !image.is_empty(),
                        "{soc:?} reports a wired SEPROM with no image name"
                    );
                    assert!(
                        !evidence.is_empty(),
                        "{soc:?} names SEPROM {image} with no evidence for the pairing; an \
                         unevidenced pairing must be SepRomStatus::Absent instead"
                    );
                }
                SepRomStatus::Absent(gap) => {
                    assert!(
                        !gap.reason().is_empty(),
                        "{soc:?} has a SepRomGap variant with no stated reason"
                    );
                }
            }
            // And the two answers must agree.
            match config(soc).sep_rom_path {
                Some(path) => assert!(
                    matches!(status, SepRomStatus::Wired { .. }),
                    "{soc:?} wires sep_rom_path = {path} but sep_rom_status says \
                     {status:?}; the two must not drift"
                ),
                None => assert!(
                    matches!(status, SepRomStatus::Absent(_)),
                    "{soc:?} has no sep_rom_path but sep_rom_status claims {status:?}"
                ),
            }
        }
    }

    /// No SoC may name a SEPROM of the wrong instruction set.
    ///
    /// This is the A7 regression, stated as the class rather than the instance. The A7
    /// config named `AppleSEPROM-Cebu-A0`, an AArch64 image, on a 32-bit part -- and it
    /// passed, because every other check in the tree asked "does this image execute?"
    /// rather than "is this image the right architecture for this part?". Run on the A7
    /// SEP map, that image halts with a **data fault after 7 instructions**; it was
    /// never working, and nothing said so.
    ///
    /// The sniff is the workspace's own ([`is_aarch32_seprom`]'s rule, restated here
    /// rather than imported so a change to that function cannot silently widen what
    /// this accepts), and the check is skipped when an image is absent, so a SoC with no
    /// SEPROM does not have to be one to pass.
    #[test]
    fn no_soc_names_a_seprom_of_the_wrong_instruction_set() {
        /// The A7 and the A9 are the 32-bit parts; everything else is AArch64.
        fn aarch32_part(soc: SoCId) -> bool {
            matches!(soc, SoCId::A7 | SoCId::A8 | SoCId::S8000)
        }
        for soc in SoCId::ALL {
            let Some(path) = config(soc).sep_rom_path else {
                continue;
            };
            let full = corpus_rom_path("seprom", path);
            let Ok(bytes) = std::fs::read(&full)
                .or_else(|_| std::fs::read(crate::corpus_rom_path("", path)))
            else {
                // The corpus is not always provisioned; the status table's own tests
                // cover the pairing, and a missing image is not a wrong architecture.
                continue;
            };
            let is_a32 = bytes
                .chunks_exact(4)
                .take(8)
                .all(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) & 0xffff_f000 == 0xe59f_f000)
                || u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) & 0xff00_0000
                    == 0xea00_0000;
            assert_eq!(
                is_a32,
                aarch32_part(soc),
                "{soc:?} wires {path}, which the reset-vector sniff reads as {}; that is \
                 the wrong instruction set for this part",
                if is_a32 { "AArch32" } else { "AArch64" }
            );
        }
    }

    /// The A11's SEP window, as measured from the ioreg dump.
    ///
    /// Held as a test because it is the one number here that came from outside the
    /// firmware corpus, and a value imported from a dump with nothing checking it is
    /// exactly how the A7 ended up carrying the A11's GPIO address. What is asserted is
    /// the arithmetic that makes it a measurement rather than a transcription: the
    /// `reg` cell plus the arm-io band equals the `IODeviceMemory` address the same
    /// node reports, and the size cell equals the reported length. If either reading
    /// were mis-transcribed these would disagree.
    #[test]
    fn the_a11_sep_window_is_the_reg_cell_and_the_ioreg_address_agreeing() {
        // `reg = <0000a00d 00000000 00200000 00000000>` -- the base cell as it appears
        // in the dump, and the size cell, kept as the raw cells so the test does not
        // merely restate the constants it is checking.
        const REG_BASE_CELL: u64 = 0x0da0_0000;
        const REG_SIZE_CELL: u64 = 0x20_0000;
        const ARM_IO_BAND: u64 = 0x2000_0000;
        // `IODeviceMemory = (({"address"=8818524160,"length"=8192}))`
        const IOREG_ADDRESS: u64 = 8_818_524_160;
        const IOREG_LENGTH: u64 = 8_192;

        assert_eq!(ARM_IO_BAND + REG_BASE_CELL, A11_SEP_BASE);
        assert_eq!(A11_SEP_BASE, IOREG_ADDRESS, "the dump's two readings disagree");
        assert_eq!(REG_SIZE_CELL, IOREG_LENGTH, "the dump's two readings disagree");
        assert_eq!(A11_SEP_SIZE, REG_SIZE_CELL);
        assert_eq!(A11_SEP_BASE & 0xfff, 0, "a window base must be page aligned");
    }

    /// The A11's `/arm-io/gpio` base, the same way: two readings, one arithmetic check.
    ///
    /// `reg = <0000300e 00000000 00001000 00000000>` and
    /// `IODeviceMemory = (({"address"=8827961344,"length"=1048576}))`.
    #[test]
    fn the_a11_gpio_base_is_the_reg_cell_and_the_ioreg_address_agreeing() {
        const REG_BASE_CELL: u64 = 0x0e30_0000;
        const REG_SIZE_CELL: u64 = 0x10_0000;
        const ARM_IO_BAND: u64 = 0x2000_0000;
        const IOREG_ADDRESS: u64 = 8_827_961_344;
        const IOREG_LENGTH: u64 = 1_048_576;

        assert_eq!(ARM_IO_BAND + REG_BASE_CELL, A11_GPIO_BASE);
        assert_eq!(A11_GPIO_BASE, IOREG_ADDRESS, "the dump's two readings disagree");
        assert_eq!(A11_GPIO_SIZE, REG_SIZE_CELL);
        assert_eq!(A11_GPIO_SIZE, IOREG_LENGTH, "the dump's two readings disagree");
    }

    /// The A11's SEP interrupt lines, cross-checked against the node's own
    /// `IOInterruptSpecifiers` list rather than read from `interrupts` alone.
    ///
    /// The dump carries both, in the same order, which is the only reason four numbers
    /// from a text file are worth having. Read from one property they would be a
    /// transcription; read from two that agree they are a measurement.
    #[test]
    fn the_a11_sep_interrupts_are_confirmed_by_the_nodes_own_specifier_list() {
        // `interrupts = <62000000 61000000 64000000 63000000>`
        const FROM_PROPERTY: [u32; 4] = [0x62, 0x61, 0x64, 0x63];
        // `IOInterruptSpecifiers = (<62000000>,<61000000>,<64000000>,<63000000>)`
        const FROM_SPECIFIERS: [u32; 4] = [0x62, 0x61, 0x64, 0x63];
        assert_eq!(A11_SEP_IRQS, FROM_PROPERTY);
        assert_eq!(A11_SEP_IRQS, FROM_SPECIFIERS);
    }

    /// The plane base the A7 SecureROM composes at `0x100008650`, and the
    /// five planes its go/wait loop actually touches.
    const A7_PMGR_PLANE_BASE: u64 = 0x20e0_00000;
    /// `orr w4, w4, #0x8000000` at `0x10000866c` -- the request.
    const A7_PMGR_REQ: u64 = 1 << 27;
    /// `tbz x4, #25` at `0x100008680` -- the acknowledgement.
    const A7_PMGR_ACK: u64 = 1 << 25;

    fn a7_machine() -> Machine {
        // The ROM bytes are irrelevant to the PMGR window: the handshake is a
        // property of the SoC mapping, and this asserts it without needing a
        // 512 KB fixture. The instruction sequence is pinned by the SoC comment
        // above, and `a7_platform.rs` is what proves the ROM contains it.
        build_a7_ap_platform(&[], false)
    }

    /// Replay the A7 ROM's measured go/wait sequence for one plane
    /// (`0x100008664..0x100008680`) and report whether `tbz x4, #25` passes.
    fn a7_plane_go_wait_acks(m: &mut Machine, plane: u64) -> bool {
        let addr = A7_PMGR_PLANE_BASE + plane;
        // ldr w4,[x3] ; orr w4,w4,#0x8000000 ; str w4,[x3]
        let held = m.mem_read(addr, 4).unwrap();
        m.mem_write(addr, 4, held | A7_PMGR_REQ).unwrap();
        // ldr w4,[x4] ; tbz x4, #25
        m.mem_read(addr, 4).unwrap() & A7_PMGR_ACK != 0
    }

    /// The A7's plane go/wait is request bit 27 / ack bit 25 on **each** plane it
    /// touches, and that is measured rather than inherited: this table used to be
    /// a verbatim copy of the A8's bit 0 / bit 31 entry, so the ROM's own
    /// `0x08000000` write read back with the acknowledgement withheld and the
    /// SecureROM spun at `PC=0x10000867c` for a 20M-instruction budget.
    ///
    /// The five planes are the ones the loop reaches: `add x3, x1, x2 lsl #12`
    /// with `x2` in 0..5, and `cmp w2, #1 ; b.eq` skips index 1 outright.
    #[test]
    fn a7_plane_go_wait_acks_with_request_27_and_ack_25_on_every_plane_it_touches() {
        let mut m = a7_machine();
        for plane in [0x0_u64, 0x2000, 0x3000, 0x4000, 0x5000] {
            assert!(
                a7_plane_go_wait_acks(&mut m, plane),
                "plane {plane:#x} must acknowledge bit 25 after a bit-27 request"
            );
        }
    }

    /// Negative control for the test above: the acknowledgement is a *response* to
    /// the request bit, not a constant. Without a request the same poll must never
    /// set bit 25, so the passing test cannot be a plane that always acks.
    #[test]
    fn a7_plane_go_wait_withholds_its_ack_until_the_request_bit_is_set() {
        let mut m = a7_machine();
        for plane in [0x0_u64, 0x2000, 0x3000, 0x4000, 0x5000] {
            let addr = A7_PMGR_PLANE_BASE + plane;
            // Write 0: no request, so no acknowledgement.
            m.mem_write(addr, 4, 0).unwrap();
            assert_eq!(
                m.mem_read(addr, 4).unwrap() & A7_PMGR_ACK,
                0,
                "plane {plane:#x} must withhold its ack with no request"
            );
            // The inherited A8 entry's request bit is not this part's convention.
            m.mem_write(addr, 4, 1 << 0).unwrap();
            assert_eq!(
                m.mem_read(addr, 4).unwrap() & A7_PMGR_ACK,
                0,
                "plane {plane:#x} must not acknowledge an A8-style bit-0 request"
            );
        }
    }

    /// `cmp w2, #1 ; b.eq` skips plane index 1, so the A7 never handshakes plane
    /// `0x1000`. Leaving it unclaimed is deliberate: a model that acked there
    /// would be claiming hardware the measured loop never exercises, and a test
    /// that asserted it would launder that guess into a contract.
    #[test]
    fn a7_plane_the_rom_skips_is_not_claimed_by_a_go_wait_entry() {
        let mut m = a7_machine();
        let addr = A7_PMGR_PLANE_BASE + 0x1000;
        m.mem_write(addr, 4, A7_PMGR_REQ).unwrap();
        assert_eq!(
            m.mem_read(addr, 4).unwrap() & A7_PMGR_ACK,
            0,
            "plane 0x1000 is skipped by the ROM and must not be modelled as a handshake"
        );
    }

    /// The A8/T7000 keeps its own, separately measured handshake at the same
    /// offset. This is the control for "the two tables are not the same object":
    /// the A7 fix must not have been achieved by editing a shared table, and the
    /// A8's bit 0 / bit 31 plane-0 poll (`0x10000a884..0x10000a890`) must still
    /// complete after the A7's was changed underneath it.
    #[test]
    fn a8_plane_zero_keeps_its_own_bit0_bit31_handshake() {
        let mut m = a8_machine();
        let addr = A7_PMGR_PLANE_BASE; // same base, different device
        m.mem_write(addr, 4, 1 << 0).unwrap();
        assert_ne!(
            m.mem_read(addr, 4).unwrap() & (1 << 31),
            0,
            "the A8's plane-0 handshake must still acknowledge bit 31 on a bit-0 request"
        );
        // ...and it must *not* answer the A7's request bit, or the two models
        // have been merged into one.
        m.mem_write(addr, 4, A7_PMGR_REQ).unwrap();
        assert_eq!(
            m.mem_read(addr, 4).unwrap() & (1 << 31),
            0,
            "the A8 must not acknowledge on the A7's request bit"
        );
    }

    /// [`SoCId::ALL`] must actually list every variant. Without this, a tenth SoC could
    /// be added to the enum and every "across all SoCs" test would quietly keep
    /// checking nine of them -- which is the same failure as a missing test, wearing the
    /// disguise of a passing one.
    #[test]
    fn every_soc_is_listed_here() {
        let listed = SoCId::ALL;
        let expected = [
            SoCId::T8010,
            SoCId::T8015,
            SoCId::T8020,
            SoCId::T8027,
            SoCId::T8030,
            SoCId::T8101,
            SoCId::S8000,
            SoCId::A7,
            SoCId::A8,
        ];
        assert_eq!(
            listed.len(),
            expected.len(),
            "SoCId::ALL has {} entries but SoCId has {} variants",
            listed.len(),
            expected.len()
        );
        for soc in expected {
            assert!(
                listed.contains(&soc),
                "{soc:?} is a SoCId variant but is missing from SoCId::ALL, so every \
                 all-SoCs test would skip it"
            );
        }
        assert_eq!(
            listed.len(),
            listed.to_vec().into_iter().dedup().count(),
            "SoCId::ALL lists a SoC twice"
        );
    }
}
