//! Synopsys DWC2 device-mode register window (the register map a real
//! USB device driver programs). T8020's active ROM aperture still needs a
//! real guest trace; mapping this model does not establish that identity.
//!
//! Register offsets follow the Synopsys DWC2 (OTG HS) programmer's map as
//! referenced by the Linux `dwc2` driver: global core registers at `0x000`,
//! device registers at `0x800`, per-endpoint IN registers at
//! `0x900 + 0x20*ep`, per-endpoint OUT registers at `0xb00 + 0x20*ep`,
//! `PCGCCTL` at `0xe00`, and FIFO space at `0x1000 * (ep+1)`.
//!
//! The core only exposes the I/O surface: registers round-trip, write-1-toclear
//! status registers clear, FIFO space behaves like FIFO space, and a host-side
//! session API can present a real USB host (attach/reset/enumeration, SETUP and
//! OUT data, IN consume). It never synthesizes a guest-visible readiness bit.
//!
//! Reference subset used by device-mode drivers:
//! - reset: `GRSTCTL.CSRST` (self-clearing) with `AHBIDLE` asserted.
//! - connect: clear `DCTL.SFTDISCON` after `GUSBCFG`/`GAHBCFG`/`DCFG`.
//! - enumeration: `GINTSTS` bits `USBRESET`, `ENUMDONE`; speed in `DSTS`.
//! - transfers: `GRXSTSP` packet-status queue + `FIFO0` (SETUP and OUT data),
//!   `DIEPTSIZ`/`DIEPCTL` + per-EP FIFO for IN, completed via `DIEPINT`/
//!   `DOEPINT` `XferCompl` through `DAINT`/`GINTSTS` `IEPINT`/`OEPINT`.

use iphem_core::dma::{DmaAccess, DmaFault};
use std::collections::VecDeque;

/// The platform resolves this IOVA through its USB IOMMU, never the CPU MMU.
pub type UsbDmaTransfer<'a> = &'a mut dyn FnMut(u32, DmaAccess, &mut [u8]) -> Result<(), DmaFault>;

/// Core / global registers.
pub const GOTGCTL: u64 = 0x000;
pub const GOTGINT: u64 = 0x004;
pub const GAHBCFG: u64 = 0x008;
pub const GUSBCFG: u64 = 0x00c;
pub const GRSTCTL: u64 = 0x010;
pub const GINTSTS: u64 = 0x014;
pub const GINTMSK: u64 = 0x018;
pub const GRXSTSR: u64 = 0x01c;
pub const GRXSTSP: u64 = 0x020;
pub const GRXFSIZ: u64 = 0x024;
pub const GNPTXFSIZ: u64 = 0x028;
pub const GNPTXSTS: u64 = 0x02c;
pub const GI2CCTL: u64 = 0x030;
pub const GPVNDCTL: u64 = 0x034;
pub const GUID: u64 = 0x03c;
pub const GSNPSID: u64 = 0x040;
pub const GHWCFG1: u64 = 0x044;
pub const GHWCFG2: u64 = 0x048;
pub const GHWCFG3: u64 = 0x04c;
pub const GHWCFG4: u64 = 0x050;
pub const GDFIFOCFG: u64 = 0x05c;
pub const GADPCTL: u64 = 0x060;
pub const HPTXFSIZ: u64 = 0x100;

/// Device-mode registers.
pub const DCFG: u64 = 0x800;
pub const DCTL: u64 = 0x804;
pub const DSTS: u64 = 0x808;
pub const DIEPMSK: u64 = 0x810;
pub const DOEPMSK: u64 = 0x814;
pub const DAINT: u64 = 0x818;
pub const DAINTMSK: u64 = 0x81c;
pub const DTKNQR4: u64 = 0x834;
pub const DVBUSDIS: u64 = 0x828;
pub const DVBUSPULSE: u64 = 0x82c;
pub const DIEPTXF0: u64 = GNPTXFSIZ;
/// Dedicated TX FIFO 1; FIFO n is at `0x104 + 4*(n-1)`.
pub const DIEPTXF_BASE: u64 = 0x104;
pub const PCGCCTL: u64 = 0xe00;

/// Per-endpoint register blocks.
pub const DIEPCTL_BASE: u64 = 0x900;
pub const DIEPINT_BASE: u64 = 0x908;
pub const DIEPTSIZ_BASE: u64 = 0x910;
pub const DIEPDMA_BASE: u64 = 0x914;
pub const DTXFSTS_BASE: u64 = 0x918;
pub const DOEPCTL_BASE: u64 = 0xb00;
pub const DOEPINT_BASE: u64 = 0xb08;
pub const DOEPTSIZ_BASE: u64 = 0xb10;
pub const DOEPDMA_BASE: u64 = 0xb14;
/// Endpoint stride in the per-EP blocks.
pub const EP_STRIDE: u64 = 0x20;
/// FIFO space base for endpoint `ep` (including EP0).
pub fn fifo_base(ep: usize) -> u64 {
    0x1000 * (ep as u64 + 1)
}

/// Number of device IN/OUT endpoints modeled (EP0 control + EP1..=EP15).
pub const DWC2_EP_COUNT: usize = 16;

/// `GRSTCTL` fields.
pub const GRSTCTL_CSRST: u32 = 1 << 0;
pub const GRSTCTL_AHBIDLE: u32 = 1 << 31;
pub const GAHBCFG_GLBL_INTR_EN: u32 = 1;

/// `GINTSTS` / `GINTMSK` interrupt bits used by device-mode drivers.
pub const GINTSTS_CURMODE: u32 = 1 << 0;
pub const GINTSTS_MODEMIS: u32 = 1 << 1;
pub const GINTSTS_SOF: u32 = 1 << 3;
pub const GINTSTS_RXFLVL: u32 = 1 << 4;
pub const GINTSTS_USBRESET: u32 = 1 << 12;
pub const GINTSTS_ENUMDONE: u32 = 1 << 13;
pub const GINTSTS_USBSUSP: u32 = 1 << 11;
pub const GINTSTS_USBRESUME: u32 = 1 << 31;
pub const GINTSTS_OTGINT: u32 = 1 << 2;
pub const GINTSTS_IEPINT: u32 = 1 << 18;
pub const GINTSTS_OEPINT: u32 = 1 << 19;
pub const GINTSTS_CONIDSTSCHNG: u32 = 1 << 28;

/// `DAINT` layout: bits `[15:0]` are IN endpoint interrupts, bits `[31:16]`
/// are OUT endpoint interrupts.
pub fn daint_in(ep: usize) -> u32 {
    1 << ep
}
pub fn daint_out(ep: usize) -> u32 {
    1 << (ep + 16)
}

/// `DIEPINT` / `DOEPINT` fields.
pub const EPINT_XFERCOMPL: u32 = 1 << 0;
pub const EPINT_EPDISBLD: u32 = 1 << 1;
pub const EPINT_TIMEOUT: u32 = 1 << 3;
pub const EPINT_AHBERR: u32 = 1 << 2;
pub const EPINT_STUPPKT: u32 = 1 << 3;
pub const EPINT_NAKINTRPT: u32 = 1 << 13;

/// `DIEPCTL` / `DOEPCTL` fields used by the driver control path.
pub const EPCTL_EPENA: u32 = 1 << 31;
pub const EPCTL_EPDIS: u32 = 1 << 30;
pub const EPCTL_SNAK: u32 = 1 << 27;
pub const EPCTL_CNAK: u32 = 1 << 26;
pub const EPCTL_SETD0PIDFRM: u32 = 1 << 28;
pub const EPCTL_SETD1PIDFRM: u32 = 1 << 29;
pub const EPCTL_USBAEP: u32 = 1 << 15;
pub const EPCTL_NAKSTS: u32 = 1 << 17;
pub const EPCTL_STALL: u32 = 1 << 21;

/// `DIEPTSIZ` / `DOEPTSIZ` device-mode XferSize field mask (bits `[18:0]`).
const TSIZ_XFERSIZE_MASK: u32 = 0x7ffff;

/// `DSTS` fields.
pub const DSTS_SUSPSTS: u32 = 1 << 0;
pub const DSTS_ENUMSPD_MASK: u32 = 0x3 << 1;
pub const DSTS_ENUMSPD_SHIFT: u32 = 1;
pub const ENUMSPD_HS: u32 = 0;
pub const ENUMSPD_FS: u32 = 1;

/// `DCTL` fields.
pub const DCTL_SFTDISCON: u32 = 1 << 1;

/// `DCFG.DESCDMA` enables descriptor DMA for the whole device core.
pub const DCFG_DESCDMA: u32 = 1 << 23;

/// Device-mode DMA descriptor (8 bytes). Matches `struct dwc2_dma_desc`
/// (`{status, buf}`) in `drivers/usb/dwc2/hw.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dwc2Desc {
    /// Status quadlet. Buffer-status `[31:30]`, transfer-status `[29:28]`,
    /// `L`/`SHORT`/`IOC`/`SR` flags, and `NBYTES [15:0]` (remaining capacity
    /// or remaining count after the hardware decrements it).
    pub status: u32,
    /// Buffer address this descriptor's bytes are DMAd to/from.
    pub buf: u32,
}

/// Device-mode descriptor buffer-status field (`DEV_DMA_BUFF_STS`).
pub const DEV_DMA_BUFF_STS_MASK: u32 = 0x3 << 30;
pub const DEV_DMA_BUFF_STS_HREADY: u32 = 0 << 30;
pub const DEV_DMA_BUFF_STS_DMABUSY: u32 = 1 << 30;
pub const DEV_DMA_BUFF_STS_DMADONE: u32 = 2 << 30;
pub const DEV_DMA_BUFF_STS_HBUSY: u32 = 3 << 30;
/// Device-mode descriptor transfer-status field (`DEV_DMA_STS`).
pub const DEV_DMA_STS_MASK: u32 = 0x3 << 28;
pub const DEV_DMA_STS_SUCC: u32 = 0 << 28;
pub const DEV_DMA_STS_BUFF_FLUSH: u32 = 1 << 28;
pub const DEV_DMA_STS_BUFF_ERR: u32 = 3 << 28;
/// Last descriptor in the chain.
pub const DEV_DMA_L: u32 = 1 << 27;
/// Short/zero-length packet detected on this descriptor.
pub const DEV_DMA_SHORT: u32 = 1 << 26;
/// Interrupt on complete for this descriptor.
pub const DEV_DMA_IOC: u32 = 1 << 25;
/// Setup-request descriptor.
pub const DEV_DMA_SR: u32 = 1 << 24;
/// Multi-transfer descriptor.
pub const DEV_DMA_MTRF: u32 = 1 << 23;
/// Generic `NBYTES` field (bits `[15:0]`).
pub const DEV_DMA_NBYTES_MASK: u32 = 0xffff;

/// Safety bound on a single descriptor-chain walk. A chain that never sets
/// `L` (or an all-zeroed chain) must not spin the DMA walkers forever, so the
/// probes stop after this many descriptors and report whatever capacity they
/// summed (a shortfall the host turns into a NAK). Real chains are a handful
/// of descriptors; the bound only guards an un-terminated guest chain.
const MAX_DESC_WALK_DESC: usize = 0x1000;

/// Encode a DMA descriptor into its little-endian 8-byte memory image.
pub fn dwc2_desc_bytes(desc: &Dwc2Desc) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[0..4].copy_from_slice(&desc.status.to_le_bytes());
    bytes[4..8].copy_from_slice(&desc.buf.to_le_bytes());
    bytes
}

/// Decode a little-endian 8-byte memory image into a DMA descriptor.
pub fn dwc2_desc_from_bytes(bytes: &[u8; 8]) -> Dwc2Desc {
    Dwc2Desc {
        status: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        buf: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
    }
}

/// Packet-status values in `GRXSTSP`.
pub const RXSTS_PKTSTS_MASK: u32 = 0xf << 17;
pub const RXSTS_SETUP: u32 = 6 << 17;
pub const RXSTS_SETUP_COMPLETE: u32 = 4 << 17;
pub const RXSTS_DATA_OUT: u32 = 2 << 17;
pub const RXSTS_OUT_COMPLETE: u32 = 3 << 17;

/// Encode an RX packet-status word.
fn rx_status(ep: usize, byte_count: usize, pktsts: u32) -> u32 {
    ((byte_count as u32) << 4) | ((ep as u32) & 0xf) | pktsts
}

/// Result of a host token. An empty DATA packet is distinct from NAK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbPacketResult {
    Ack,
    Data(Vec<u8>),
    Nak,
    Stall,
    Disconnected,
    /// No platform DMA callback was provided, or descriptor DMA was selected.
    UnsupportedDma,
    DmaFault(DmaFault),
}

/// Synopsys DWC2 device-mode register surface: registers, the RX
/// packet-status queue, and FIFO byte storage.
pub struct Dwc2Core {
    gotgctl: u32,
    gotgint: u32,
    gahbcfg: u32,
    gusbcfg: u32,
    grstctl: u32,
    gintsts: u32,
    gintmsk: u32,
    grxfsiz: u32,
    gnptxfsiz: u32,
    gi2cctl: u32,
    gpvndctl: u32,
    guid: u32,
    gsnpsid: u32,
    ghwcfg1: u32,
    ghwcfg2: u32,
    ghwcfg3: u32,
    ghwcfg4: u32,
    gdfifocfg: u32,
    gadpctl: u32,
    hptxfsiz: u32,
    dcfg: u32,
    dctl: u32,
    dsts: u32,
    diepmsk: u32,
    doepmsk: u32,
    daint: u32,
    daintmsk: u32,
    dtknqr4: u32,
    dvbusdis: u32,
    dvbuspulse: u32,
    dieptxf: [u32; DWC2_EP_COUNT],
    pcgcctl: u32,
    diepctl: [u32; DWC2_EP_COUNT],
    diepint: [u32; DWC2_EP_COUNT],
    dieptsiz: [u32; DWC2_EP_COUNT],
    diepdma: [u32; DWC2_EP_COUNT],
    doepctl: [u32; DWC2_EP_COUNT],
    doepint: [u32; DWC2_EP_COUNT],
    doeptsiz: [u32; DWC2_EP_COUNT],
    doepdma: [u32; DWC2_EP_COUNT],
    /// Descriptor-DMA walk state: bytes already placed into/consumed from the
    /// current OUT descriptor buffer. Reset when DOEPDMA is (re)written.
    desc_out_off: [u32; DWC2_EP_COUNT],
    /// Descriptor-DMA walk state: bytes already consumed from the current IN
    /// descriptor buffer. Reset when DIEPDMA is (re)written.
    desc_in_off: [u32; DWC2_EP_COUNT],
    rx_status: VecDeque<u32>,
    rx_fifo: VecDeque<u8>,
    tx_fifo: Vec<VecDeque<u8>>,
    attached: bool,
}

impl Dwc2Core {
    pub fn new() -> Self {
        let mut tx_fifo = Vec::new();
        for _ in 0..DWC2_EP_COUNT {
            tx_fifo.push(VecDeque::new());
        }
        Self {
            gotgctl: 0,
            gotgint: 0,
            gahbcfg: 0,
            gusbcfg: 0,
            grstctl: GRSTCTL_AHBIDLE,
            // CURMODE=0 is device mode; 1 means host mode.
            gintsts: 0,
            gintmsk: 0,
            grxfsiz: 0x40,
            gnptxfsiz: 0x0040_0040,
            gi2cctl: 0,
            gpvndctl: 0,
            // Synopsys OTG 2.80a identifier (GSNPSID = 0x4f54280a)
            guid: 0x12345678,
            gsnpsid: 0x4f54280a,
            // Hardware config: OTG HS, device mode, 16 endpoints, internal DMA
            ghwcfg1: 0x004c0112,
            ghwcfg2: 0x22448d4c,
            ghwcfg3: 0x00000000,
            ghwcfg4: 0x00000020,
            gdfifocfg: 0x00000120,
            gadpctl: 0,
            hptxfsiz: 0,
            dcfg: 0,
            dctl: DCTL_SFTDISCON,
            dsts: 0,
            diepmsk: 0,
            doepmsk: 0,
            daint: 0,
            daintmsk: 0,
            dtknqr4: 0,
            dvbusdis: 0,
            dvbuspulse: 0,
            dieptxf: [0; DWC2_EP_COUNT],
            pcgcctl: 0,
            diepctl: [0; DWC2_EP_COUNT],
            diepint: [0; DWC2_EP_COUNT],
            dieptsiz: [0; DWC2_EP_COUNT],
            diepdma: [0; DWC2_EP_COUNT],
            doepctl: [0; DWC2_EP_COUNT],
            doepint: [0; DWC2_EP_COUNT],
            doeptsiz: [0; DWC2_EP_COUNT],
            doepdma: [0; DWC2_EP_COUNT],
            desc_out_off: [0; DWC2_EP_COUNT],
            desc_in_off: [0; DWC2_EP_COUNT],
            rx_status: VecDeque::new(),
            rx_fifo: VecDeque::new(),
            tx_fifo,
            attached: false,
        }
    }

    /// Recompute the aggregated IN/OUT endpoint GINT bits from per-EP
    /// interrupt registers and the per-EP masks.
    fn recompute_endpoint_interrupts(&mut self) {
        self.gintsts &= !(GINTSTS_IEPINT | GINTSTS_OEPINT);
        self.daint = 0;
        for ep in 0..DWC2_EP_COUNT {
            if self.diepint[ep] & self.diepmsk != 0 {
                self.daint |= daint_in(ep);
            }
            if self.doepint[ep] & self.doepmsk != 0 {
                self.daint |= daint_out(ep);
            }
        }
        let pending = self.daint & self.daintmsk;
        if pending & 0xffff != 0 {
            self.gintsts |= GINTSTS_IEPINT;
        }
        if pending >> 16 != 0 {
            self.gintsts |= GINTSTS_OEPINT;
        }
        if self.rx_status.is_empty() {
            self.gintsts &= !GINTSTS_RXFLVL;
        } else {
            self.gintsts |= GINTSTS_RXFLVL;
        }
    }

    /// Clear an endpoint completion and re-derive the GINT aggregation bits.
    fn clear_endpoint(&mut self, ep: usize) {
        self.diepctl[ep] &= !EPCTL_EPENA;
        self.doepctl[ep] &= !EPCTL_EPENA;
        self.recompute_endpoint_interrupts();
    }

    /// True while any enabled GINT bit is set (the device interrupt line).
    pub fn has_unmasked_int(&self) -> bool {
        self.gahbcfg & GAHBCFG_GLBL_INTR_EN != 0 && (self.gintsts & self.gintmsk) != 0
    }

    /// Existing engine-level interrupt latch or an unmasked DWC2 condition.
    pub fn pending_interrupt(&self) -> bool {
        self.has_unmasked_int()
    }

    /// Soft-reset the core: clears all DWC2 register state (reset values),
    /// then reports AHB idle. Kept separate from the engine/DFU state so a
    /// core reset never drops staged firmware.
    pub fn soft_reset(&mut self) {
        let attached = self.attached;
        *self = Dwc2Core::new();
        if attached {
            self.host_attach();
        }
    }

    /// DWC2 register read; `None` for unknown offsets or unsupported widths.
    pub fn read(&mut self, offset: u64, size: usize) -> Option<u64> {
        if size != 4 {
            return None;
        }
        if let Some(ep) = in_ep_block(offset, DIEPCTL_BASE) {
            return Some(self.diepctl[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DIEPINT_BASE) {
            return Some(self.diepint[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DIEPTSIZ_BASE) {
            return Some(self.dieptsiz[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DIEPDMA_BASE) {
            return Some(self.diepdma[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DTXFSTS_BASE) {
            let used = (self.tx_fifo[ep].len() as u32).div_ceil(4);
            let cap = (self.tx_fifo[ep].capacity().max(4096) as u32) / 4;
            return Some(cap.saturating_sub(used) as u64);
        }
        if let Some(ep) = in_ep_block(offset, DOEPCTL_BASE) {
            return Some(self.doepctl[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DOEPINT_BASE) {
            return Some(self.doepint[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DOEPTSIZ_BASE) {
            return Some(self.doeptsiz[ep] as u64);
        }
        if let Some(ep) = in_ep_block(offset, DOEPDMA_BASE) {
            return Some(self.doepdma[ep] as u64);
        }
        if (DIEPTXF_BASE..DIEPTXF_BASE + 4 * (DWC2_EP_COUNT as u64 - 1)).contains(&offset)
            && offset.is_multiple_of(4)
        {
            let idx = ((offset - DIEPTXF_BASE) / 4) as usize + 1;
            return Some(self.dieptxf[idx] as u64);
        }
        let value = match offset {
            GOTGCTL => self.gotgctl,
            GOTGINT => self.gotgint,
            GAHBCFG => self.gahbcfg,
            GUSBCFG => self.gusbcfg,
            GRSTCTL => self.grstctl,
            GINTSTS => self.gintsts,
            GINTMSK => self.gintmsk,
            GRXSTSR => self.rx_status.front().copied().unwrap_or(0),
            GRXSTSP => {
                let value = self.rx_status.pop_front().unwrap_or(0);
                self.recompute_endpoint_interrupts();
                value
            }
            GRXFSIZ => self.grxfsiz,
            GNPTXFSIZ => self.gnptxfsiz,
            GNPTXSTS => self.gnptx_free(),
            GI2CCTL => self.gi2cctl,
            GPVNDCTL => self.gpvndctl,
            GUID => self.guid,
            GSNPSID => self.gsnpsid,
            GHWCFG1 => self.ghwcfg1,
            GHWCFG2 => self.ghwcfg2,
            GHWCFG3 => self.ghwcfg3,
            GHWCFG4 => self.ghwcfg4,
            GDFIFOCFG => self.gdfifocfg,
            GADPCTL => self.gadpctl,
            HPTXFSIZ => self.hptxfsiz,
            DCFG => self.dcfg,
            DCTL => self.dctl,
            DSTS => self.dsts,
            DIEPMSK => self.diepmsk,
            DOEPMSK => self.doepmsk,
            DAINT => self.daint,
            DAINTMSK => self.daintmsk,
            DTKNQR4 => self.dtknqr4,
            DVBUSDIS => self.dvbusdis,
            DVBUSPULSE => self.dvbuspulse,
            PCGCCTL => self.pcgcctl,
            _ => return None,
        };
        Some(value as u64)
    }

    /// DWC2 register write; `None` for unknown offsets or unsupported widths.
    pub fn write(&mut self, offset: u64, size: usize, value: u64) -> Option<()> {
        if size != 4 {
            return None;
        }
        let value = value as u32;
        if let Some(ep) = in_ep_block(offset, DIEPCTL_BASE) {
            // NAKSTS is hardware state; only CNAK/SNAK change it.
            self.diepctl[ep] = (value & !EPCTL_NAKSTS) | (self.diepctl[ep] & EPCTL_NAKSTS);
            self.on_diep_ctl(ep);
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DIEPINT_BASE) {
            self.diepint[ep] &= !value;
            self.recompute_endpoint_interrupts();
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DIEPTSIZ_BASE) {
            self.dieptsiz[ep] = value;
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DIEPDMA_BASE) {
            self.diepdma[ep] = value;
            self.desc_in_off[ep] = 0;
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DOEPCTL_BASE) {
            self.doepctl[ep] = (value & !EPCTL_NAKSTS) | (self.doepctl[ep] & EPCTL_NAKSTS);
            self.on_doep_ctl(ep);
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DOEPINT_BASE) {
            self.doepint[ep] &= !value;
            self.recompute_endpoint_interrupts();
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DOEPTSIZ_BASE) {
            self.doeptsiz[ep] = value;
            return Some(());
        }
        if let Some(ep) = in_ep_block(offset, DOEPDMA_BASE) {
            self.doepdma[ep] = value;
            self.desc_out_off[ep] = 0;
            return Some(());
        }
        if (DIEPTXF_BASE..DIEPTXF_BASE + 4 * (DWC2_EP_COUNT as u64 - 1)).contains(&offset)
            && offset.is_multiple_of(4)
        {
            let idx = ((offset - DIEPTXF_BASE) / 4) as usize + 1;
            self.dieptxf[idx] = value;
            return Some(());
        }
        match offset {
            // B-session validity comes from physical VBUS, not a guest write.
            GOTGCTL => self.gotgctl = (value & !(1 << 19)) | (self.gotgctl & (1 << 19)),
            GOTGINT => self.gotgint &= !value,
            GAHBCFG => self.gahbcfg = value,
            GUSBCFG => self.gusbcfg = value,
            GRSTCTL => {
                if value & GRSTCTL_CSRST != 0 {
                    self.soft_reset();
                } else {
                    if value & (1 << 4) != 0 {
                        self.rx_fifo.clear();
                        self.rx_status.clear();
                    }
                    if value & (1 << 5) != 0 {
                        let fifo = ((value >> 6) & 0x1f) as usize;
                        for (index, bytes) in self.tx_fifo.iter_mut().enumerate() {
                            if fifo == 0x10 || fifo == index {
                                bytes.clear();
                            }
                        }
                    }
                    self.grstctl = GRSTCTL_AHBIDLE;
                }
            }
            GINTSTS => {
                // Level/summary bits are cleared at their source, never W1C.
                let read_only = GINTSTS_CURMODE | GINTSTS_RXFLVL | GINTSTS_IEPINT | GINTSTS_OEPINT;
                self.gintsts &= !(value & !read_only);
            }
            GINTMSK => self.gintmsk = value,
            GRXFSIZ => self.grxfsiz = value,
            GNPTXFSIZ => self.gnptxfsiz = value,
            GI2CCTL => self.gi2cctl = value,
            GPVNDCTL => self.gpvndctl = value,
            GDFIFOCFG => self.gdfifocfg = value,
            GADPCTL => self.gadpctl = value,
            HPTXFSIZ => self.hptxfsiz = value,
            DCFG => self.dcfg = value,
            DCTL => self.dctl = value,
            DIEPMSK => self.diepmsk = value,
            DOEPMSK => self.doepmsk = value,
            DAINT => {} // Read-only endpoint interrupt summary.
            DAINTMSK => self.daintmsk = value,
            DTKNQR4 => self.dtknqr4 = value,
            DVBUSDIS => self.dvbusdis = value,
            DVBUSPULSE => self.dvbuspulse = value,
            PCGCCTL => self.pcgcctl = value,
            _ => return None,
        }
        self.recompute_endpoint_interrupts();
        Some(())
    }

    /// Guest write into FIFO space (`FIFO0` stores EP0 TX/IN data; higher
    /// FIFOs store their endpoint's IN data). `None` for unknown offsets.
    pub fn write_fifo(&mut self, offset: u64, size: usize, value: u64) -> Option<()> {
        if size != 4 {
            return None;
        }
        let ep = fifo_ep(offset)?;
        let bytes = (value as u32).to_le_bytes();
        self.tx_fifo[ep].extend(bytes.iter().copied());
        Some(())
    }

    /// Guest read of FIFO space. `FIFO0` is the shared RX FIFO; reads pop
    /// word-granular RX data. Other FIFO offsets are not readable.
    pub fn read_fifo(&mut self, offset: u64, size: usize) -> Option<u64> {
        if size != 4 {
            return None;
        }
        if fifo_ep(offset)? != 0 {
            return None;
        }
        let mut word = 0u32;
        for i in 0..4 {
            if let Some(byte) = self.rx_fifo.pop_front() {
                word |= (byte as u32) << (8 * i);
            }
        }
        Some(word as u64)
    }

    /// `GNPTXSTS`: FIFO words available in bits 15:0, request slots in 23:16.
    fn gnptx_free(&self) -> u32 {
        let used = (self.tx_fifo[0].len() as u32).div_ceil(4);
        let cap = self.gnptxfsiz >> 16;
        let free = cap.saturating_sub(used).min(0xffff);
        (8 << 16) | free
    }

    /// Host attached: the device is on the bus. No status change until the
    /// host drives a reset.
    pub fn host_attach(&mut self) {
        self.attached = true;
        self.gotgctl |= 1 << 19; // B-session valid from physical VBUS.
        self.dsts &= !DSTS_SUSPSTS;
    }

    pub fn is_connected(&self) -> bool {
        self.attached && self.dctl & DCTL_SFTDISCON == 0
    }

    /// Physical disconnect drops VBUS; it does not write software DCTL state.
    pub fn host_detach(&mut self) {
        self.attached = false;
        self.gotgctl &= !(1 << 19);
        self.dsts |= DSTS_SUSPSTS;
        for ep in 0..DWC2_EP_COUNT {
            self.clear_endpoint(ep);
            self.tx_fifo[ep].clear();
        }
        self.rx_status.clear();
        self.rx_fifo.clear();
        self.daint = 0;
        self.diepint = [0; DWC2_EP_COUNT];
        self.doepint = [0; DWC2_EP_COUNT];
        self.recompute_endpoint_interrupts();
    }

    /// Host issued a bus reset: raises `GINTSTS.USBRESET` and re-arms EP0.
    pub fn host_bus_reset(&mut self) {
        if !self.is_connected() {
            return;
        }
        self.gintsts |= GINTSTS_USBRESET;
        self.dsts &= !DSTS_SUSPSTS;
        self.device_address_clear();
        for ep in 0..DWC2_EP_COUNT {
            self.clear_endpoint(ep);
            self.tx_fifo[ep].clear();
        }
        self.diepint = [0; DWC2_EP_COUNT];
        self.doepint = [0; DWC2_EP_COUNT];
        self.dieptsiz = [0; DWC2_EP_COUNT];
        self.doeptsiz = [0; DWC2_EP_COUNT];
        self.desc_in_off = [0; DWC2_EP_COUNT];
        self.desc_out_off = [0; DWC2_EP_COUNT];
        self.rx_status.clear();
        self.rx_fifo.clear();
        self.recompute_endpoint_interrupts();
    }

    /// Enumeration completed: raises `GINTSTS.ENUMDONE` and publishes the
    /// negotiated speed in `DSTS.ENUMSPD`.
    pub fn host_enum_done(&mut self, speed: u32) {
        if !self.is_connected() {
            return;
        }
        self.gintsts |= GINTSTS_ENUMDONE;
        self.dsts &= !DSTS_ENUMSPD_MASK;
        self.dsts |= (speed & (DSTS_ENUMSPD_MASK >> DSTS_ENUMSPD_SHIFT)) << DSTS_ENUMSPD_SHIFT;
    }

    /// Host delivered a (already-parsed) SETUP packet: enqueue the packet
    /// status and eight setup bytes into the RX side and raise `RXFLVL`.
    pub fn host_send_setup(&mut self, setup: &[u8; 8]) -> UsbPacketResult {
        self.host_send_setup_with_dma(setup, None)
    }

    pub fn host_send_setup_with_dma(
        &mut self,
        setup: &[u8; 8],
        dma: Option<UsbDmaTransfer<'_>>,
    ) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        let buffer_dma = self.gahbcfg & (1 << 5) != 0;
        if self.dcfg & DCFG_DESCDMA != 0 {
            return match dma {
                None => UsbPacketResult::UnsupportedDma,
                Some(mut d) => self.host_send_setup_desc(setup, &mut d),
            };
        }
        if buffer_dma && dma.is_none() {
            return UsbPacketResult::UnsupportedDma;
        }
        if self.doepctl[0] & EPCTL_EPENA == 0 || (!buffer_dma && !self.rx_has_space(8, 2)) {
            return UsbPacketResult::Nak;
        }
        if buffer_dma {
            // Buffer-DMA SETUP reception uses the OUT buffer, not FIFO0.
            let remaining = self.doeptsiz[0] & 0x7f;
            let setup_count = (self.doeptsiz[0] >> 29) & 3;
            if remaining < 8 || setup_count == 0 {
                return UsbPacketResult::Nak;
            }
            let mut bytes = *setup;
            if let Err(fault) = dma.unwrap()(self.doepdma[0], DmaAccess::Write, &mut bytes) {
                return self.dma_fault(0, false, fault);
            }
            self.doepdma[0] = self.doepdma[0].wrapping_add(8);
            self.doeptsiz[0] = (self.doeptsiz[0] & !((3 << 29) | (1 << 19) | 0x7f))
                | ((setup_count - 1) << 29)
                | (remaining - 8);
        }
        // SETUP starts a new control transfer, including recovery from STALL.
        self.diepctl[0] &= !(EPCTL_STALL | EPCTL_EPENA);
        self.doepctl[0] &= !EPCTL_STALL;
        self.tx_fifo[0].clear();
        if !buffer_dma {
            self.rx_status.push_back(rx_status(0, 8, RXSTS_SETUP));
            self.rx_fifo.extend(setup.iter().copied());
            self.rx_status
                .push_back(rx_status(0, 0, RXSTS_SETUP_COMPLETE));
        }
        self.doepint[0] |= EPINT_STUPPKT;
        self.doepctl[0] &= !EPCTL_EPENA;
        self.recompute_endpoint_interrupts();
        UsbPacketResult::Ack
    }

    /// Host delivered an OUT data burst on `ep`: enqueue per-packet status
    /// words and byte data and raise `RXFLVL`. Completes the armed OUT
    /// transfer when the delivered bytes reach the programmed `XferSize`.
    pub fn host_send_out(&mut self, ep: usize, data: &[u8]) -> UsbPacketResult {
        self.host_send_out_with_dma(ep, data, None)
    }

    pub fn host_send_out_with_dma(
        &mut self,
        ep: usize,
        data: &[u8],
        dma: Option<UsbDmaTransfer<'_>>,
    ) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        let buffer_dma = self.gahbcfg & (1 << 5) != 0;
        if self.dcfg & DCFG_DESCDMA != 0 {
            return match dma {
                None => UsbPacketResult::UnsupportedDma,
                Some(mut d) => self.host_send_out_desc(ep, data, &mut d),
            };
        }
        if buffer_dma && dma.is_none() {
            return UsbPacketResult::UnsupportedDma;
        }
        if ep >= DWC2_EP_COUNT {
            return UsbPacketResult::Stall;
        }
        let ctl = self.doepctl[ep];
        if ctl & EPCTL_STALL != 0 {
            return UsbPacketResult::Stall;
        }
        if ctl & EPCTL_EPENA == 0 || ctl & EPCTL_NAKSTS != 0 {
            return UsbPacketResult::Nak;
        }
        let remaining = self.doeptsiz[ep] & TSIZ_XFERSIZE_MASK;
        let mps = self.max_packet_size(ep, false);
        if data.len() > mps
            || data.len() > remaining as usize
            || (!buffer_dma && !self.rx_has_space(data.len(), 2))
        {
            return UsbPacketResult::Nak;
        }
        if buffer_dma {
            if !data.is_empty() {
                if let Err(fault) =
                    dma.unwrap()(self.doepdma[ep], DmaAccess::Write, &mut data.to_vec())
                {
                    return self.dma_fault(ep, false, fault);
                }
                self.doepdma[ep] = self.doepdma[ep].wrapping_add(data.len() as u32);
            }
        } else {
            self.rx_status
                .push_back(rx_status(ep, data.len(), RXSTS_DATA_OUT));
            self.rx_fifo.extend(data.iter().copied());
            // FIFO is word-granular: padding must not consume the next packet.
            self.rx_fifo
                .extend(std::iter::repeat_n(0, (4 - data.len() % 4) % 4));
        }
        let left = remaining - data.len() as u32;
        self.doeptsiz[ep] = Self::retire_packet(self.doeptsiz[ep], left);
        if left == 0 || data.len() < mps {
            self.doepint[ep] |= EPINT_XFERCOMPL;
            self.doepctl[ep] &= !EPCTL_EPENA;
            if !buffer_dma {
                self.rx_status
                    .push_back(rx_status(ep, 0, RXSTS_OUT_COMPLETE));
            }
        }
        self.recompute_endpoint_interrupts();
        UsbPacketResult::Ack
    }

    /// Host consumed an IN packet: completes the armed IN transfer on `ep`
    /// once the host consumed the programmed `XferSize` bytes.
    pub fn host_in_packet(&mut self, ep: usize) -> UsbPacketResult {
        self.host_in_packet_with_dma(ep, None)
    }

    pub fn host_in_packet_with_dma(
        &mut self,
        ep: usize,
        dma: Option<UsbDmaTransfer<'_>>,
    ) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        let buffer_dma = self.gahbcfg & (1 << 5) != 0;
        if self.dcfg & DCFG_DESCDMA != 0 {
            return match dma {
                None => UsbPacketResult::UnsupportedDma,
                Some(mut d) => self.host_in_packet_desc(ep, &mut d),
            };
        }
        if buffer_dma && dma.is_none() {
            return UsbPacketResult::UnsupportedDma;
        }
        if ep >= DWC2_EP_COUNT {
            return UsbPacketResult::Stall;
        }
        let ctl = self.diepctl[ep];
        if ctl & EPCTL_STALL != 0 {
            return UsbPacketResult::Stall;
        }
        if ctl & EPCTL_EPENA == 0 || ctl & EPCTL_NAKSTS != 0 {
            return UsbPacketResult::Nak;
        }
        let remaining = self.dieptsiz[ep] & TSIZ_XFERSIZE_MASK;
        let take = (remaining as usize).min(self.max_packet_size(ep, true));
        if !buffer_dma && self.tx_fifo[ep].len() < take {
            return UsbPacketResult::Nak;
        }
        let out = if buffer_dma {
            let mut bytes = vec![0; take];
            if take != 0 {
                if let Err(fault) = dma.unwrap()(self.diepdma[ep], DmaAccess::Read, &mut bytes) {
                    return self.dma_fault(ep, true, fault);
                }
                self.diepdma[ep] = self.diepdma[ep].wrapping_add(take as u32);
            }
            bytes
        } else {
            self.tx_fifo[ep].drain(..take).collect()
        };
        let left = remaining - take as u32;
        self.dieptsiz[ep] = Self::retire_packet(self.dieptsiz[ep], left);
        if left == 0 {
            self.diepint[ep] |= EPINT_XFERCOMPL;
            self.diepctl[ep] &= !EPCTL_EPENA;
            // Discard only the final word's padding, not future transfer data.
            if !buffer_dma {
                for _ in 0..((4 - take % 4) % 4) {
                    self.tx_fifo[ep].pop_front();
                }
            }
        }
        self.recompute_endpoint_interrupts();
        UsbPacketResult::Data(out)
    }

    /// Descriptor-DMA SETUP reception (`DCFG.DESCDMA`): writes the eight
    /// setup bytes through the chain pointed to by `DOEPDMA[0]`, then raises
    /// `STUPPKT` and clears EPENA exactly like the FIFO path — no RX FIFO
    /// status is produced for the setup data itself.
    fn host_send_setup_desc(
        &mut self,
        setup: &[u8; 8],
        dma: &mut UsbDmaTransfer<'_>,
    ) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        if self.doepctl[0] & EPCTL_EPENA == 0 {
            return UsbPacketResult::Nak;
        }
        match self.desc_out_place(0, setup, true, dma) {
            Err(fault) => self.dma_fault(0, false, fault),
            Ok(placed) if placed < setup.len() => UsbPacketResult::Nak,
            Ok(_) => {
                // SETUP starts a new control transfer, including STALL recovery.
                self.diepctl[0] &= !(EPCTL_STALL | EPCTL_EPENA);
                self.doepctl[0] &= !EPCTL_STALL;
                self.tx_fifo[0].clear();
                self.doepint[0] |= EPINT_STUPPKT;
                self.doepctl[0] &= !EPCTL_EPENA;
                self.recompute_endpoint_interrupts();
                UsbPacketResult::Ack
            }
        }
    }

    /// Descriptor-DMA OUT reception (`DCFG.DESCDMA`): places the packet into
    /// the guest-programmed descriptor chain (no RX FIFO data/status) and
    /// completes the armed transfer via XferSize/short-packet bookkeeping.
    fn host_send_out_desc(
        &mut self,
        ep: usize,
        data: &[u8],
        dma: &mut UsbDmaTransfer<'_>,
    ) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        if ep >= DWC2_EP_COUNT {
            return UsbPacketResult::Stall;
        }
        let ctl = self.doepctl[ep];
        if ctl & EPCTL_STALL != 0 {
            return UsbPacketResult::Stall;
        }
        if ctl & EPCTL_EPENA == 0 || ctl & EPCTL_NAKSTS != 0 {
            return UsbPacketResult::Nak;
        }
        let remaining = self.doeptsiz[ep] & TSIZ_XFERSIZE_MASK;
        let mps = self.max_packet_size(ep, false);
        if data.len() > mps || data.len() > remaining as usize {
            return UsbPacketResult::Nak;
        }
        let transfer_complete = remaining == data.len() as u32 || data.len() < mps;
        if data.is_empty() {
            match Self::read_desc(dma, self.doepdma[ep]) {
                Err(fault) => return self.dma_fault(ep, false, fault),
                Ok(desc)
                    if !matches!(
                        desc.status & DEV_DMA_BUFF_STS_MASK,
                        DEV_DMA_BUFF_STS_HREADY | DEV_DMA_BUFF_STS_DMABUSY
                    ) =>
                {
                    return UsbPacketResult::Nak
                }
                _ => {}
            }
        }
        match self.desc_out_place(ep, data, transfer_complete, dma) {
            Err(fault) => self.dma_fault(ep, false, fault),
            Ok(placed) if placed != data.len() => UsbPacketResult::Nak,
            Ok(_) => {
                let left = remaining - data.len() as u32;
                self.doeptsiz[ep] = Self::retire_packet(self.doeptsiz[ep], left);
                if left == 0 || data.len() < mps {
                    self.doepint[ep] |= EPINT_XFERCOMPL;
                    self.doepctl[ep] &= !EPCTL_EPENA;
                }
                self.recompute_endpoint_interrupts();
                UsbPacketResult::Ack
            }
        }
    }

    /// Descriptor-DMA IN packet consumption (`DCFG.DESCDMA`): reads the
    /// packet from the guest-programmed chain (no TX FIFO staging) and
    /// completes the armed IN transfer when `XferSize` is exhausted.
    fn host_in_packet_desc(&mut self, ep: usize, dma: &mut UsbDmaTransfer<'_>) -> UsbPacketResult {
        if !self.is_connected() {
            return UsbPacketResult::Disconnected;
        }
        if ep >= DWC2_EP_COUNT {
            return UsbPacketResult::Stall;
        }
        let ctl = self.diepctl[ep];
        if ctl & EPCTL_STALL != 0 {
            return UsbPacketResult::Stall;
        }
        if ctl & EPCTL_EPENA == 0 || ctl & EPCTL_NAKSTS != 0 {
            return UsbPacketResult::Nak;
        }
        let remaining = self.dieptsiz[ep] & TSIZ_XFERSIZE_MASK;
        let take = (remaining as usize).min(self.max_packet_size(ep, true));
        let transfer_complete = remaining == take as u32;
        if take == 0 {
            match Self::read_desc(dma, self.diepdma[ep]) {
                Err(fault) => return self.dma_fault(ep, true, fault),
                Ok(desc)
                    if !matches!(
                        desc.status & DEV_DMA_BUFF_STS_MASK,
                        DEV_DMA_BUFF_STS_HREADY | DEV_DMA_BUFF_STS_DMABUSY
                    ) =>
                {
                    return UsbPacketResult::Nak
                }
                _ => {}
            }
        }
        match self.desc_in_take(ep, take, transfer_complete, dma) {
            Err(fault) => self.dma_fault(ep, true, fault),
            Ok(out) if out.len() != take => UsbPacketResult::Nak,
            Ok(out) => {
                let left = remaining - take as u32;
                self.dieptsiz[ep] = Self::retire_packet(self.dieptsiz[ep], left);
                if left == 0 {
                    self.diepint[ep] |= EPINT_XFERCOMPL;
                    self.diepctl[ep] &= !EPCTL_EPENA;
                }
                self.recompute_endpoint_interrupts();
                UsbPacketResult::Data(out)
            }
        }
    }

    fn retire_packet(size: u32, left: u32) -> u32 {
        let count = (size >> 19) & 0x3ff;
        (size & !(TSIZ_XFERSIZE_MASK | (0x3ff << 19))) | left | (count.saturating_sub(1) << 19)
    }

    /// Read the 8-byte DMA descriptor at `addr` through the host DMA closure.
    fn read_desc(dma: &mut UsbDmaTransfer<'_>, addr: u32) -> Result<Dwc2Desc, DmaFault> {
        let mut raw = [0u8; 8];
        dma(addr, DmaAccess::Read, &mut raw)?;
        Ok(dwc2_desc_from_bytes(&raw))
    }

    /// Write the 8-byte DMA descriptor at `addr` through the host DMA closure.
    fn write_desc(
        dma: &mut UsbDmaTransfer<'_>,
        addr: u32,
        desc: &Dwc2Desc,
    ) -> Result<(), DmaFault> {
        let mut raw = dwc2_desc_bytes(desc);
        dma(addr, DmaAccess::Write, &mut raw)
    }

    /// Probe the OUT chain's remaining headroom (until `L` is consumed).
    ///
    /// Only the current descriptor has a nonzero `desc_out_off` (a partial
    /// previous packet); every later descriptor is counted at its full
    /// `NBYTES`. The walk stops at [`MAX_DESC_WALK_DESC`] descriptors so an
    /// unterminated chain cannot spin forever.
    fn desc_out_chain_capacity(
        &self,
        ep: usize,
        dma: &mut UsbDmaTransfer<'_>,
    ) -> Result<usize, DmaFault> {
        let mut capacity = 0usize;
        let mut addr = self.doepdma[ep];
        for walked in 0..MAX_DESC_WALK_DESC {
            let desc = Self::read_desc(dma, addr)?;
            let state = desc.status & DEV_DMA_BUFF_STS_MASK;
            if state != DEV_DMA_BUFF_STS_HREADY && state != DEV_DMA_BUFF_STS_DMABUSY {
                break;
            }
            let nbytes = desc.status & DEV_DMA_NBYTES_MASK;
            let headroom = if walked == 0 {
                nbytes.saturating_sub(self.desc_out_off[ep])
            } else {
                nbytes
            };
            capacity += headroom as usize;
            if desc.status & DEV_DMA_L != 0 {
                break;
            }
            addr = addr.wrapping_add(8);
        }
        Ok(capacity)
    }

    /// Probe the IN chain's remaining headroom (until `L` is consumed).
    fn desc_in_chain_capacity(
        &self,
        ep: usize,
        dma: &mut UsbDmaTransfer<'_>,
    ) -> Result<usize, DmaFault> {
        let mut capacity = 0usize;
        let mut addr = self.diepdma[ep];
        for walked in 0..MAX_DESC_WALK_DESC {
            let desc = Self::read_desc(dma, addr)?;
            let state = desc.status & DEV_DMA_BUFF_STS_MASK;
            if state != DEV_DMA_BUFF_STS_HREADY && state != DEV_DMA_BUFF_STS_DMABUSY {
                break;
            }
            let nbytes = desc.status & DEV_DMA_NBYTES_MASK;
            let headroom = if walked == 0 {
                nbytes.saturating_sub(self.desc_in_off[ep])
            } else {
                nbytes
            };
            capacity += headroom as usize;
            if desc.status & DEV_DMA_L != 0 {
                break;
            }
            addr = addr.wrapping_add(8);
        }
        Ok(capacity)
    }

    /// Place one USB OUT packet into the descriptor chain programmed by the
    /// guest under `DCFG.DESCDMA`. Returns the number of bytes placed (always
    /// `data.len()` when the chain headroom probe passes; the caller turns a
    /// short placement into a NAK, i.e. the host token was not accepted).
    ///
    /// `transfer_complete` marks the final descriptor `DMADONE|SUCC` so the
    /// guest's leftover-NBYTES scan sees a completed chain exactly like the
    /// Linux gadget (`dwc2_gadget_get_xfersize_ddma`). A short packet always
    /// ends the armed transfer; the caller folds `data.len() < mps` into
    /// `transfer_complete` before the placement.
    fn desc_out_place(
        &mut self,
        ep: usize,
        data: &[u8],
        transfer_complete: bool,
        dma: &mut UsbDmaTransfer<'_>,
    ) -> Result<usize, DmaFault> {
        if self.desc_out_chain_capacity(ep, dma)? < data.len() {
            return Ok(0);
        }
        let mut placed = 0usize;
        let mut advances = 0usize;
        loop {
            let addr = self.doepdma[ep];
            let mut desc = Self::read_desc(dma, addr)?;
            // The probe only counts HREADY/DMABUSY descriptors; a descriptor
            // that has settled (DMADONE/HBUSY) is a stale chain the guest must
            // re-arm. Stop rather than consume it, so the caller NAKs the
            // token instead of writing into a completed transfer.
            let state = desc.status & DEV_DMA_BUFF_STS_MASK;
            if state != DEV_DMA_BUFF_STS_HREADY && state != DEV_DMA_BUFF_STS_DMABUSY {
                break;
            }
            let capacity = (desc.status & DEV_DMA_NBYTES_MASK) as usize;
            let headroom = capacity.saturating_sub(self.desc_out_off[ep] as usize);
            if headroom == 0 && placed < data.len() {
                // This descriptor's buffer is full; advance to the next one.
                if desc.status & DEV_DMA_L != 0 || advances >= MAX_DESC_WALK_DESC {
                    break;
                }
                advances += 1;
                let next = addr.wrapping_add(8);
                self.doepdma[ep] = next;
                self.desc_out_off[ep] = 0;
                continue;
            }
            let take = headroom.min(data.len() - placed);
            let mut chunk = data[placed..placed + take].to_vec();
            if take != 0 {
                dma(
                    desc.buf.wrapping_add(self.desc_out_off[ep]),
                    DmaAccess::Write,
                    &mut chunk,
                )?;
            }
            placed += take;
            self.desc_out_off[ep] += take as u32;
            let leftover = headroom - take;
            let last = placed == data.len();
            let done = leftover == 0 || (last && transfer_complete);
            // Keep the programmed capacity while DMA owns the descriptor.
            // desc_out_off tracks progress; publish residual NBYTES only on
            // retirement, otherwise the next packet subtracts progress twice.
            desc.status = (desc.status
                & !(DEV_DMA_NBYTES_MASK | DEV_DMA_BUFF_STS_MASK | DEV_DMA_STS_MASK))
                | if done {
                    leftover as u32
                } else {
                    capacity as u32
                }
                | if done {
                    DEV_DMA_BUFF_STS_DMADONE | DEV_DMA_STS_SUCC
                } else {
                    DEV_DMA_BUFF_STS_DMABUSY | DEV_DMA_STS_SUCC
                };
            Self::write_desc(dma, addr, &desc)?;
            if leftover == 0 && !transfer_complete && desc.status & DEV_DMA_L == 0 {
                self.doepdma[ep] = addr.wrapping_add(8);
                self.desc_out_off[ep] = 0;
            }
            if last {
                break;
            }
            // Packet continues; this descriptor's buffer is now full, so walk
            // the chain to the next descriptor and stop if it is the last one.
            let next = addr.wrapping_add(8);
            self.doepdma[ep] = next;
            self.desc_out_off[ep] = 0;
            if desc.status & DEV_DMA_L != 0 {
                break;
            }
        }
        Ok(placed)
    }

    /// Consume one USB IN packet from the descriptor chain. Returns the bytes
    /// read from the guest buffers. `transfer_complete` marks the final
    /// descriptor `DMADONE|SUCC`; a short placement (`< take`) is turned into
    /// a NAK by the caller (the host token is not consumed).
    fn desc_in_take(
        &mut self,
        ep: usize,
        take: usize,
        transfer_complete: bool,
        dma: &mut UsbDmaTransfer<'_>,
    ) -> Result<Vec<u8>, DmaFault> {
        if self.desc_in_chain_capacity(ep, dma)? < take {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut advances = 0usize;
        loop {
            let addr = self.diepdma[ep];
            let mut desc = Self::read_desc(dma, addr)?;
            // Consistent with the capacity probe: a settled descriptor is a
            // stale chain and is NOT consumed (caller NAKs the token).
            let state = desc.status & DEV_DMA_BUFF_STS_MASK;
            if state != DEV_DMA_BUFF_STS_HREADY && state != DEV_DMA_BUFF_STS_DMABUSY {
                break;
            }
            let capacity = (desc.status & DEV_DMA_NBYTES_MASK) as usize;
            let headroom = capacity.saturating_sub(self.desc_in_off[ep] as usize);
            if headroom == 0 && out.len() < take {
                if desc.status & DEV_DMA_L != 0 || advances >= MAX_DESC_WALK_DESC {
                    break;
                }
                advances += 1;
                let next = addr.wrapping_add(8);
                self.diepdma[ep] = next;
                self.desc_in_off[ep] = 0;
                continue;
            }
            let n = headroom.min(take - out.len());
            let mut chunk = vec![0u8; n];
            if n != 0 {
                dma(
                    desc.buf.wrapping_add(self.desc_in_off[ep]),
                    DmaAccess::Read,
                    &mut chunk,
                )?;
            }
            out.extend_from_slice(&chunk);
            self.desc_in_off[ep] += n as u32;
            let leftover = headroom - n;
            let last = out.len() == take;
            let done = leftover == 0 || (last && transfer_complete);
            desc.status = (desc.status
                & !(DEV_DMA_NBYTES_MASK | DEV_DMA_BUFF_STS_MASK | DEV_DMA_STS_MASK))
                | if done {
                    leftover as u32
                } else {
                    capacity as u32
                }
                | if done {
                    DEV_DMA_BUFF_STS_DMADONE | DEV_DMA_STS_SUCC
                } else {
                    DEV_DMA_BUFF_STS_DMABUSY | DEV_DMA_STS_SUCC
                };
            Self::write_desc(dma, addr, &desc)?;
            if leftover == 0 && !transfer_complete && desc.status & DEV_DMA_L == 0 {
                self.diepdma[ep] = addr.wrapping_add(8);
                self.desc_in_off[ep] = 0;
            }
            if last {
                break;
            }
            // Packet continues; this descriptor's buffer is now full, so walk
            // the chain to the next descriptor and stop if it is the last one.
            let next = addr.wrapping_add(8);
            self.diepdma[ep] = next;
            self.desc_in_off[ep] = 0;
            if desc.status & DEV_DMA_L != 0 {
                break;
            }
        }
        Ok(out)
    }

    fn dma_fault(&mut self, ep: usize, input: bool, fault: DmaFault) -> UsbPacketResult {
        if input {
            self.diepint[ep] |= EPINT_AHBERR;
            self.diepctl[ep] &= !EPCTL_EPENA;
        } else {
            self.doepint[ep] |= EPINT_AHBERR;
            self.doepctl[ep] &= !EPCTL_EPENA;
        }
        self.recompute_endpoint_interrupts();
        UsbPacketResult::DmaFault(fault)
    }

    /// Read one IN packet, reporting exactly how the transfer ended.
    ///
    /// [`Self::host_read_in`] collapses every non-data outcome into an empty
    /// buffer, which is lossy in a way that matters wherever a stall *is* the
    /// answer. Recovery mode's `getenv` is the case in point: a device with no
    /// such key stalls the reply transfer rather than answering, and
    /// `libirecovery` reads that as a successful query with no value
    /// (`irecv_getenv` maps `IRECV_E_PIPE` to `IRECV_E_SUCCESS`). A host that
    /// cannot see the stall has to guess, and the natural guess — no bytes means
    /// "unset" — is indistinguishable from a device that really did answer with
    /// an empty value.
    pub fn host_read_in_result(&mut self, ep: usize) -> UsbPacketResult {
        self.host_in_packet(ep)
    }

    /// Compatibility helper for the explicitly simulated class-firmware client.
    pub fn host_read_in(&mut self, ep: usize) -> Vec<u8> {
        match self.host_in_packet(ep) {
            UsbPacketResult::Data(data) => data,
            _ => Vec::new(),
        }
    }

    fn rx_has_space(&self, bytes: usize, statuses: usize) -> bool {
        self.rx_status.len() + statuses <= 64
            && self.rx_fifo.len() + bytes.next_multiple_of(4) <= self.grxfsiz as usize * 4
    }

    fn max_packet_size(&self, ep: usize, input: bool) -> usize {
        let ctl = if input {
            self.diepctl[ep]
        } else {
            self.doepctl[ep]
        };
        if ep == 0 {
            64 >> (ctl & 3)
        } else {
            (ctl as usize & 0x7ff).max(1)
        }
    }

    /// Firmware-side staging of an EP0 IN response (what class firmware
    /// writes into `FIFO0` before the driver arms the IN transfer).
    pub fn stage_ep0_in(&mut self, data: &[u8]) {
        self.stage_ep_in(0, data);
    }

    /// Stage bytes into an IN endpoint's TX FIFO, the way firmware writes a
    /// reply before the host reads it.
    ///
    /// The per-endpoint form exists because recovery mode does not answer on
    /// endpoint zero: libirecovery claims interface 1 alt 1 for the command
    /// exchange, so a model of that protocol has to be able to reply there.
    /// Programming the endpoint's transfer size and enabling it remain the
    /// firmware's job -- this only fills the FIFO, exactly as
    /// `stage_ep0_in` does.
    pub fn stage_ep_in(&mut self, ep: usize, data: &[u8]) {
        if ep >= DWC2_EP_COUNT {
            return;
        }
        self.tx_fifo[ep].clear();
        self.tx_fifo[ep].extend(data.iter().copied());
    }

    /// Bytes the guest has written into an endpoint TX FIFO.
    pub fn tx_fifo_len(&self, ep: usize) -> usize {
        self.tx_fifo[ep].len()
    }

    fn device_address_clear(&mut self) {
        self.dcfg &= !(0x7f << 4);
    }

    fn on_diep_ctl(&mut self, ep: usize) {
        if self.diepctl[ep] & EPCTL_CNAK != 0 {
            self.diepctl[ep] &= !EPCTL_NAKSTS;
        }
        if self.diepctl[ep] & EPCTL_SNAK != 0 {
            self.diepctl[ep] |= EPCTL_NAKSTS;
        }
        self.diepctl[ep] &= !(EPCTL_SNAK | EPCTL_CNAK);
        if self.diepctl[ep] & EPCTL_EPDIS != 0 {
            self.diepint[ep] |= EPINT_EPDISBLD;
            self.diepctl[ep] &= !(EPCTL_EPENA | EPCTL_EPDIS);
            self.recompute_endpoint_interrupts();
        }
    }

    fn on_doep_ctl(&mut self, ep: usize) {
        if self.doepctl[ep] & EPCTL_CNAK != 0 {
            self.doepctl[ep] &= !EPCTL_NAKSTS;
        }
        if self.doepctl[ep] & EPCTL_SNAK != 0 {
            self.doepctl[ep] |= EPCTL_NAKSTS;
        }
        self.doepctl[ep] &= !(EPCTL_SNAK | EPCTL_CNAK);
        if self.doepctl[ep] & EPCTL_EPDIS != 0 {
            self.doepint[ep] |= EPINT_EPDISBLD;
            self.doepctl[ep] &= !(EPCTL_EPENA | EPCTL_EPDIS);
            self.recompute_endpoint_interrupts();
        }
    }
}

impl Default for Dwc2Core {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve an endpoint index for `offset` inside the IN/OUT register block
/// starting at `block` (stride `EP_STRIDE`).
fn in_ep_block(offset: u64, block: u64) -> Option<usize> {
    let rel = offset.checked_sub(block)?;
    if rel % EP_STRIDE != 0 {
        return None;
    }
    let ep = (rel / EP_STRIDE) as usize;
    if ep >= DWC2_EP_COUNT {
        return None;
    }
    Some(ep)
}

/// Resolve the endpoint owning a FIFO-space offset (`0x1000 * (ep+1)`),
/// word-indexed within each FIFO.
fn fifo_ep(offset: u64) -> Option<usize> {
    if offset < 0x1000 || offset >= 0x1000 * (DWC2_EP_COUNT as u64 + 1) {
        return None;
    }
    let fifo = (offset / 0x1000) as usize - 1;
    if !(offset % 0x1000).is_multiple_of(4) {
        return None;
    }
    Some(fifo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected_core() -> Dwc2Core {
        let mut core = Dwc2Core::new();
        core.host_attach();
        w(&mut core, DCTL, 0);
        w(&mut core, GAHBCFG, GAHBCFG_GLBL_INTR_EN);
        core
    }

    /// Register write helper: values are u32 register words, offsets u64.
    fn w(core: &mut Dwc2Core, off: u64, val: u32) {
        assert_eq!(core.write(off, 4, val as u64), Some(()));
    }

    /// Register read helper returning the u32 word.
    fn r(core: &mut Dwc2Core, off: u64) -> u32 {
        core.read(off, 4).unwrap() as u32
    }

    #[test]
    fn dma_mode_cannot_acknowledge_tokens_without_a_memory_transfer() {
        for (register, enable) in [(GAHBCFG, 1 << 5), (DCFG, 1 << 23)] {
            let mut core = connected_core();
            w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
            w(&mut core, DIEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
            w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 8);
            w(&mut core, DIEPTSIZ_BASE, (1 << 19) | 4);
            core.write_fifo(0x1000, 4, 0x04030201).unwrap();
            w(&mut core, register, enable);
            assert_eq!(
                core.host_send_setup(&[0; 8]),
                UsbPacketResult::UnsupportedDma
            );
            assert_eq!(
                core.host_send_out(0, &[1; 8]),
                UsbPacketResult::UnsupportedDma
            );
            assert_eq!(core.host_in_packet(0), UsbPacketResult::UnsupportedDma);
            assert_eq!(r(&mut core, DOEPTSIZ_BASE), (1 << 19) | 8);
            assert_eq!(r(&mut core, DIEPTSIZ_BASE), (1 << 19) | 4);
            assert_eq!(r(&mut core, DOEPINT_BASE), 0);
            assert_eq!(r(&mut core, DIEPINT_BASE), 0);
            assert_eq!(r(&mut core, GRXSTSP), 0);
            w(&mut core, register, 0);
            assert_eq!(
                core.host_in_packet(0),
                UsbPacketResult::Data(vec![1, 2, 3, 4])
            );
        }
    }

    #[test]
    fn register_offsets_round_trip_and_fault() {
        let mut core = Dwc2Core::new();
        assert_eq!(core.read(GOTGCTL, 4), Some(0));
        w(&mut core, GOTGCTL, 0x1);
        assert_eq!(r(&mut core, GOTGCTL), 0x1);
        w(&mut core, GUSBCFG, 0x140000);
        assert_eq!(r(&mut core, GUSBCFG), 0x140000);
        w(&mut core, DCFG, 0x10001);
        assert_eq!(r(&mut core, DCFG), 0x10001);
        w(&mut core, DIEPTXF0, 0x40);
        assert_eq!(r(&mut core, DIEPTXF0), 0x40);
        assert!(core.read(0x1234, 4).is_none());
        assert!(core.read(GOTGCTL, 2).is_none());
        assert!(core.write(0x1234, 4, 1).is_none());
    }

    #[test]
    fn w1c_status_registers_clear_selectively() {
        let mut core = Dwc2Core::new();
        core.rx_status.push_back(0x000c0080);
        core.gintsts = GINTSTS_RXFLVL | GINTSTS_USBRESET | GINTSTS_ENUMDONE;
        w(&mut core, GINTSTS, GINTSTS_USBRESET);
        // only USBRESET cleared; RXFLVL/ENUMDONE survived.
        assert_eq!(core.gintsts, GINTSTS_RXFLVL | GINTSTS_ENUMDONE);
        w(&mut core, GINTSTS, GINTSTS_RXFLVL);
        assert_ne!(r(&mut core, GINTSTS) & GINTSTS_RXFLVL, 0);
        core.diepint[2] = EPINT_XFERCOMPL;
        w(&mut core, DIEPMSK, EPINT_XFERCOMPL);
        w(&mut core, DAINT, daint_in(2));
        assert_eq!(core.daint, daint_in(2));
        w(&mut core, DIEPINT_BASE + 2 * EP_STRIDE, EPINT_XFERCOMPL);
        assert_eq!(core.daint, 0);
    }

    #[test]
    fn per_ep_block_offsets_address_single_endpoints() {
        let mut core = Dwc2Core::new();
        w(&mut core, DIEPTSIZ_BASE + EP_STRIDE * 3, (44 << 19) | 44);
        assert_eq!(r(&mut core, DIEPTSIZ_BASE + EP_STRIDE * 3), (44 << 19) | 44);
        assert_eq!(r(&mut core, DIEPTSIZ_BASE), 0);
        // Misaligned EP-block addresses are unmapped.
        assert!(core.write(DIEPTSIZ_BASE + 9, 4, 1).is_none());
        assert!(core.read(DIEPTSIZ_BASE + 9, 4).is_none());
        w(&mut core, DCTL, DCTL_SFTDISCON);
        assert_eq!(r(&mut core, DCTL), DCTL_SFTDISCON);
    }

    #[test]
    fn soft_reset_reinitializes_and_reports_ahb_idle() {
        let mut core = Dwc2Core::new();
        w(&mut core, DCFG, 0xdead_beef);
        w(&mut core, GINTMSK, 0x1);
        core.host_bus_reset();
        w(&mut core, GRSTCTL, GRSTCTL_CSRST);
        assert_eq!(r(&mut core, DCFG), 0);
        assert_eq!(r(&mut core, GINTMSK), 0);
        assert!(r(&mut core, GRSTCTL) & GRSTCTL_CSRST == 0);
        assert!(r(&mut core, GRSTCTL) & GRSTCTL_AHBIDLE != 0);
    }

    #[test]
    fn setup_and_out_flow_through_rx_queue_and_fifo() {
        let mut core = connected_core();
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        assert_eq!(
            core.host_send_setup(&[1, 2, 3, 4, 5, 6, 7, 8]),
            UsbPacketResult::Ack
        );
        assert!(core.gintsts & GINTSTS_RXFLVL != 0);
        // SETUP received and SETUP complete are separate status words.
        let setup_status = r(&mut core, GRXSTSP);
        assert_eq!(setup_status & RXSTS_PKTSTS_MASK, RXSTS_SETUP);
        // Reading FIFO0 pops the eight setup bytes as two words.
        assert_eq!(core.read_fifo(fifo_base(0), 4), Some(0x04030201));
        assert_eq!(core.read_fifo(fifo_base(0) + 4, 4), Some(0x08070605));
        assert_eq!(r(&mut core, GRXSTSP), RXSTS_SETUP_COMPLETE);
        w(&mut core, DOEPTSIZ_BASE, 4);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        assert_eq!(
            core.host_send_out(0, &[9, 10, 11, 12]),
            UsbPacketResult::Ack
        );
        let out_status = r(&mut core, GRXSTSP);
        assert_eq!(out_status & RXSTS_PKTSTS_MASK, RXSTS_DATA_OUT);
        let word = core.read_fifo(fifo_base(0), 4).unwrap();
        assert_eq!(word & 0xff, 9);
        assert_eq!(r(&mut core, GRXSTSP), RXSTS_OUT_COMPLETE);
        // RXFLVL is a level: consuming the final status clears it.
        assert_eq!(r(&mut core, GRXSTSP), 0);
        assert_eq!(r(&mut core, GINTSTS) & GINTSTS_RXFLVL, 0);
        assert_eq!(core.read_fifo(fifo_base(0), 4), Some(0));
    }

    #[test]
    fn armed_out_transfer_completes_and_sets_daint_oepint() {
        let mut core = connected_core();
        w(&mut core, DAINTMSK, daint_out(0));
        w(&mut core, GINTMSK, GINTSTS_OEPINT);
        w(&mut core, DOEPMSK, EPINT_XFERCOMPL | EPINT_STUPPKT);
        w(&mut core, DOEPTSIZ_BASE, 4);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        core.host_send_out(0, &[1, 2, 3, 4]);
        assert!(core.doepint[0] & EPINT_XFERCOMPL != 0);
        assert!(core.daint & daint_out(0) != 0);
        assert!(core.gintsts & GINTSTS_OEPINT != 0);
        assert!(core.has_unmasked_int());
        assert!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA == 0);
    }

    #[test]
    fn armed_in_transfer_completes_on_host_consume() {
        let mut core = connected_core();
        w(&mut core, DAINTMSK, daint_in(0));
        w(&mut core, GINTMSK, GINTSTS_IEPINT);
        w(&mut core, DIEPMSK, EPINT_XFERCOMPL);
        core.stage_ep0_in(&[0xAA; 18]);
        w(&mut core, DIEPTSIZ_BASE, 18);
        w(
            &mut core,
            DIEPCTL_BASE,
            EPCTL_EPENA | EPCTL_CNAK | (64 << 11),
        );
        assert!(!core.has_unmasked_int());
        let data = core.host_read_in(0);
        assert_eq!(data, vec![0xAA; 18]);
        assert!(core.diepint[0] & EPINT_XFERCOMPL != 0);
        assert!(core.daint & daint_in(0) != 0);
        assert!(core.gintsts & GINTSTS_IEPINT != 0);
        assert!(core.has_unmasked_int());
        // W1C the completion and confirm the line drops.
        w(&mut core, DIEPINT_BASE, EPINT_XFERCOMPL);
        w(&mut core, DAINT, daint_in(0));
        assert!(!core.has_unmasked_int());
    }

    #[test]
    fn host_events_raise_and_clear_gint() {
        let mut core = connected_core();
        w(&mut core, GINTMSK, GINTSTS_USBRESET | GINTSTS_ENUMDONE);
        core.host_bus_reset();
        assert!(core.gintsts & GINTSTS_USBRESET != 0);
        assert!(core.has_unmasked_int());
        w(&mut core, GINTSTS, GINTSTS_USBRESET);
        assert!(!core.has_unmasked_int());
        core.host_enum_done(ENUMSPD_FS);
        assert!(core.gintsts & GINTSTS_ENUMDONE != 0);
        assert!(core.has_unmasked_int());
        assert_eq!((r(&mut core, DSTS) >> DSTS_ENUMSPD_SHIFT) & 0x7, ENUMSPD_FS);
        w(&mut core, GINTSTS, GINTSTS_ENUMDONE);
        assert!(!core.has_unmasked_int());
    }

    #[test]
    fn linux_register_contract_uses_literal_offsets_and_status_encodings() {
        // Independent numeric expectations from drivers/usb/dwc2/hw.h.
        let mut core = Dwc2Core::new();
        assert_eq!(r(&mut core, 0x010), 0x8000_0000);
        assert_eq!(r(&mut core, 0x014) & 1, 0); // Device, not host.
        core.host_attach();
        w(&mut core, 0x804, 0);
        w(&mut core, 0x008, 1);
        w(&mut core, 0x018, 0x3010);
        core.host_bus_reset();
        assert_eq!(r(&mut core, 0x014), 0x1000);
        core.host_enum_done(1);
        assert_eq!(r(&mut core, 0x808), 2);
        w(&mut core, 0x014, 0x3000);
        w(&mut core, 0xb00, 0x8400_0000);
        assert_eq!(
            core.host_send_setup(&[0x80, 6, 0, 1, 0, 0, 18, 0]),
            UsbPacketResult::Ack
        );
        assert_eq!(r(&mut core, 0x01c), 0x000c_0080);
        assert_eq!(r(&mut core, 0x020), 0x000c_0080);
        assert_eq!(r(&mut core, 0x020), 0x0008_0000);
        assert_eq!(r(&mut core, 0x014) & 0x10, 0);
        assert_eq!(r(&mut core, 0xb08) & 8, 8);
        w(&mut core, 0x814, 8);
        assert_eq!(r(&mut core, 0x818), 0x10000);
        assert_eq!(r(&mut core, 0x014) & 0x80000, 0);
        w(&mut core, 0x81c, 0x10000);
        w(&mut core, 0x018, 0x80000);
        assert!(core.has_unmasked_int());
        w(&mut core, 0x008, 0);
        assert!(!core.has_unmasked_int());
    }

    #[test]
    fn packet_handshakes_distinguish_nak_stall_and_zero_length_data() {
        let mut core = Dwc2Core::new();
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Disconnected);
        core.host_attach();
        w(&mut core, DCTL, 0);
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Nak);
        w(&mut core, DIEPCTL_BASE, EPCTL_STALL);
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Stall);
        w(&mut core, DIEPTSIZ_BASE, 1 << 19);
        w(&mut core, DIEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Data(vec![]));
        assert_ne!(r(&mut core, DIEPINT_BASE) & EPINT_XFERCOMPL, 0);
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Nak);
        assert_eq!(core.host_in_packet(16), UsbPacketResult::Stall);
        assert_eq!(core.host_send_out(16, &[]), UsbPacketResult::Stall);
    }

    #[test]
    fn short_out_packet_completes_and_padding_does_not_eat_next_packet() {
        let mut core = connected_core();
        for bytes in [&[1, 2, 3][..], &[4, 5, 6][..]] {
            w(&mut core, DOEPTSIZ_BASE, 64);
            w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
            assert_eq!(core.host_send_out(0, bytes), UsbPacketResult::Ack);
            assert_eq!(r(&mut core, DOEPTSIZ_BASE) & TSIZ_XFERSIZE_MASK, 61);
            assert_eq!(core.host_send_out(0, bytes), UsbPacketResult::Nak);
        }
        assert_eq!(core.read_fifo(0x1000, 4), Some(0x00030201));
        assert_eq!(core.read_fifo(0x1000, 4), Some(0x00060504));
        w(&mut core, DOEPTSIZ_BASE, 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        assert_ne!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
        assert_eq!(core.host_send_out(0, &[]), UsbPacketResult::Ack);
        assert_eq!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
    }

    #[test]
    fn reset_clears_address_and_stale_transfers_without_losing_speed_configuration() {
        let mut core = connected_core();
        w(&mut core, DCFG, (0x55 << 4) | 1 | (2 << 11));
        w(&mut core, DIEPTSIZ_BASE, 4);
        w(&mut core, DIEPCTL_BASE, EPCTL_EPENA);
        core.stage_ep0_in(&[1, 2, 3, 4]);
        core.host_bus_reset();
        assert_eq!(r(&mut core, DCFG), 1 | (2 << 11));
        assert_eq!(core.host_in_packet(0), UsbPacketResult::Nak);
        assert_eq!(core.tx_fifo_len(0), 0);
    }

    /// RAM-backed DMA closure standing in for the guest-visible memory the
    /// host side submits via `poll_with_dma`/`transfer_usb_dma`.
    fn ram_dma<'a>(ram: &'a mut [u8]) -> UsbDmaTransfer<'a> {
        Box::leak(Box::new(
            move |addr: u32, access: DmaAccess, bytes: &mut [u8]| {
                let start = addr as usize;
                let end = start + bytes.len();
                if end > ram.len() {
                    return Err(DmaFault::UnboundSid(0));
                }
                match access {
                    DmaAccess::Read => bytes.copy_from_slice(&ram[start..end]),
                    DmaAccess::Write => ram[start..end].copy_from_slice(bytes),
                }
                Ok(())
            },
        ))
    }

    fn desc_at(ram: &mut [u8], desc_addr: u32, buf_addr: u32, nbytes: u32, last: bool) {
        let status = (nbytes & DEV_DMA_NBYTES_MASK)
            | DEV_DMA_BUFF_STS_HREADY
            | DEV_DMA_STS_SUCC
            | if last { DEV_DMA_L } else { 0 };
        let si = desc_addr as usize;
        ram[si..si + 4].copy_from_slice(&status.to_le_bytes());
        ram[si + 4..si + 8].copy_from_slice(&buf_addr.to_le_bytes());
    }

    #[test]
    fn init_writes_preserve_vbus_and_endpoint_nak_status() {
        let mut core = connected_core();
        w(&mut core, GOTGCTL, 0);
        assert_ne!(r(&mut core, GOTGCTL) & (1 << 19), 0);
        for reg in [DIEPCTL_BASE, DOEPCTL_BASE] {
            w(&mut core, reg, EPCTL_SNAK);
            w(&mut core, reg, EPCTL_EPENA);
            assert_ne!(r(&mut core, reg) & EPCTL_NAKSTS, 0);
            w(&mut core, reg, EPCTL_EPENA | EPCTL_CNAK);
            assert_eq!(r(&mut core, reg) & EPCTL_NAKSTS, 0);
            w(&mut core, reg, EPCTL_NAKSTS);
            assert_eq!(r(&mut core, reg) & EPCTL_NAKSTS, 0);
        }
        core.host_detach();
        w(&mut core, GOTGCTL, 1 << 19);
        assert_eq!(r(&mut core, GOTGCTL) & (1 << 19), 0);
    }

    #[test]
    fn descriptor_dma_moves_full_dfu_blocks_in_both_directions() {
        // Exercise both a single 2 KiB descriptor and a chain whose boundary
        // coincides with a USB packet boundary. Old code NAKed midway through.
        for input in [false, true] {
            for capacity in [64usize, 128, 2048] {
                let mut ram = vec![0xa5; 0x4000];
                let payload: Vec<u8> = (0..2048).map(|n| (n % 251) as u8).collect();
                for i in 0..2048 / capacity {
                    desc_at(
                        &mut ram,
                        0x1000 + i as u32 * 8,
                        0x2000 + (i * capacity) as u32,
                        capacity as u32,
                        i + 1 == 2048 / capacity,
                    );
                }
                if input {
                    ram[0x2000..0x2800].copy_from_slice(&payload);
                }
                let mut core = connected_core();
                w(&mut core, DCFG, DCFG_DESCDMA);
                let (tsiz, ctl, address, interrupt) = if input {
                    (DIEPTSIZ_BASE, DIEPCTL_BASE, DIEPDMA_BASE, DIEPINT_BASE)
                } else {
                    (DOEPTSIZ_BASE, DOEPCTL_BASE, DOEPDMA_BASE, DOEPINT_BASE)
                };
                w(&mut core, address, 0x1000);
                w(&mut core, tsiz, (32 << 19) | 2048);
                w(&mut core, ctl, EPCTL_EPENA | EPCTL_CNAK);
                {
                    let mut dma = ram_dma(&mut ram);
                    for packet in payload.chunks(64) {
                        if input {
                            assert_eq!(
                                core.host_in_packet_with_dma(0, Some(&mut dma)),
                                UsbPacketResult::Data(packet.to_vec())
                            );
                        } else {
                            assert_eq!(
                                core.host_send_out_with_dma(0, packet, Some(&mut dma)),
                                UsbPacketResult::Ack
                            );
                        }
                    }
                }
                assert_eq!(&ram[0x2000..0x2800], payload.as_slice());
                assert_eq!(ram[0x2800], 0xa5);
                for i in 0..2048 / capacity {
                    let at = 0x1000 + i * 8;
                    let status = u32::from_le_bytes(ram[at..at + 4].try_into().unwrap());
                    assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
                    assert_eq!(status & DEV_DMA_NBYTES_MASK, 0);
                }
                assert_ne!(r(&mut core, interrupt) & EPINT_XFERCOMPL, 0);
                assert_eq!(r(&mut core, ctl) & EPCTL_EPENA, 0);
            }
        }
    }

    #[test]
    fn descriptor_setup_recovers_from_stall() {
        let mut ram = vec![0; 0x3000];
        desc_at(&mut ram, 0x1000, 0x2000, 8, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_STALL);
        w(&mut core, DIEPCTL_BASE, EPCTL_EPENA | EPCTL_STALL);
        let setup = [0x80, 6, 0, 1, 0, 0, 18, 0];
        assert_eq!(
            core.host_send_setup_with_dma(&setup, Some(&mut ram_dma(&mut ram))),
            UsbPacketResult::Ack
        );
        assert_eq!(&ram[0x2000..0x2008], &setup);
        assert_eq!(r(&mut core, DIEPCTL_BASE) & (EPCTL_STALL | EPCTL_EPENA), 0);
        assert_eq!(r(&mut core, DOEPCTL_BASE) & EPCTL_STALL, 0);
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_STUPPKT, 0);
    }

    #[test]
    fn descriptor_zero_length_status_requires_owned_descriptor() {
        for input in [false, true] {
            for state in [
                DEV_DMA_BUFF_STS_HREADY,
                DEV_DMA_BUFF_STS_DMADONE,
                DEV_DMA_BUFF_STS_HBUSY,
            ] {
                let mut ram = vec![0; 0x1008];
                // No payload access is permitted for a zero-length status.
                desc_at(&mut ram, 0x1000, u32::MAX, 0, true);
                let status = state | DEV_DMA_L;
                ram[0x1000..0x1004].copy_from_slice(&status.to_le_bytes());
                let mut core = connected_core();
                w(&mut core, DCFG, DCFG_DESCDMA);
                let (ctl, address, interrupt) = if input {
                    (DIEPCTL_BASE, DIEPDMA_BASE, DIEPINT_BASE)
                } else {
                    (DOEPCTL_BASE, DOEPDMA_BASE, DOEPINT_BASE)
                };
                w(&mut core, address, 0x1000);
                w(&mut core, ctl, EPCTL_EPENA | EPCTL_CNAK);
                let result = if input {
                    core.host_in_packet_with_dma(0, Some(&mut ram_dma(&mut ram)))
                } else {
                    core.host_send_out_with_dma(0, &[], Some(&mut ram_dma(&mut ram)))
                };
                if state == DEV_DMA_BUFF_STS_HREADY {
                    assert_eq!(
                        result,
                        if input {
                            UsbPacketResult::Data(vec![])
                        } else {
                            UsbPacketResult::Ack
                        }
                    );
                    assert_ne!(r(&mut core, interrupt) & EPINT_XFERCOMPL, 0);
                    let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
                    assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
                } else {
                    assert_eq!(result, UsbPacketResult::Nak);
                    assert_eq!(r(&mut core, interrupt), 0);
                }
            }
        }
    }

    #[test]
    fn descriptor_dma_out_places_packet_into_chain_and_completes_transfer() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x400, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        let data = (0..64).map(|i| i as u8).collect::<Vec<_>>();
        assert_eq!(
            core.host_send_out_with_dma(0, &data, Some(&mut dma)),
            UsbPacketResult::Ack
        );
        // Payload landed in the guest buffer.
        assert_eq!(&ram[0x2000..0x2040], &data[..]);
        // Descriptor written back with DMADONE|SUCC and leftover NBYTES.
        let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(status & DEV_DMA_NBYTES_MASK, 0x400 - 64);
        assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_eq!(status & DEV_DMA_L, DEV_DMA_L);
        // Armed transfer completes: XFERCOMPL + EPENA cleared.
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_XFERCOMPL, 0);
        assert_eq!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
        assert_eq!(r(&mut core, GRXSTSP), 0);
    }

    #[test]
    fn descriptor_dma_short_out_marks_dmadone_and_ends_transfer() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x400, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        assert_eq!(
            core.host_send_out_with_dma(0, &[1, 2, 3], Some(&mut dma)),
            UsbPacketResult::Ack
        );
        let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(status & DEV_DMA_NBYTES_MASK, 0x400 - 3);
        assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_XFERCOMPL, 0);
        assert_eq!(&ram[0x2000..0x2003], &[1, 2, 3]);
    }

    #[test]
    fn descriptor_dma_setup_can_only_pass_as_control() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x400, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 8);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        assert_eq!(
            core.host_send_setup_with_dma(&[0x80, 0, 0, 0, 8, 0, 0, 0], Some(&mut dma)),
            UsbPacketResult::Ack
        );
        let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(status & DEV_DMA_NBYTES_MASK, 0x400 - 8);
        assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_STUPPKT, 0);
        assert_eq!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
        assert_eq!(r(&mut core, GRXSTSP), 0);
        assert_eq!(&ram[0x2000..0x2008], &[0x80, 0, 0, 0, 8, 0, 0, 0]);
    }

    #[test]
    fn descriptor_dma_in_reads_packet_from_chain() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x400, true);
        let reply = (0..18).map(|i| i as u8).collect::<Vec<_>>();
        ram[0x2000..0x2012].copy_from_slice(&reply);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DIEPTSIZ_BASE, (1 << 19) | 18);
        w(&mut core, DIEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DIEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        assert_eq!(
            core.host_in_packet_with_dma(0, Some(&mut dma)),
            UsbPacketResult::Data(reply)
        );
        let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(status & DEV_DMA_NBYTES_MASK, 0x400 - 18);
        assert_eq!(status & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_ne!(r(&mut core, DIEPINT_BASE) & EPINT_XFERCOMPL, 0);
        assert_eq!(r(&mut core, DIEPCTL_BASE) & EPCTL_EPENA, 0);
    }

    #[test]
    fn descriptor_dma_chain_walks_multiple_descriptors() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x20, false);
        desc_at(&mut ram, 0x1008, 0x2030, 0x20, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 48);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        let data = (0..48).map(|i| i as u8).collect::<Vec<_>>();
        assert_eq!(
            core.host_send_out_with_dma(0, &data, Some(&mut dma)),
            UsbPacketResult::Ack
        );
        // First descriptor: exactly filled, chain continues (DMADONE, no L).
        let s0 = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(s0 & DEV_DMA_NBYTES_MASK, 0);
        assert_eq!(s0 & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_eq!(s0 & DEV_DMA_L, 0);
        // Second descriptor: final (DMADONE, L preserved), ep DMA advanced to it.
        let s1 = u32::from_le_bytes(ram[0x1008..0x100c].try_into().unwrap());
        assert_eq!(s1 & DEV_DMA_NBYTES_MASK, 0x20 - 16);
        assert_eq!(s1 & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_eq!(s1 & DEV_DMA_L, DEV_DMA_L);
        assert_eq!(r(&mut core, DOEPDMA_BASE), 0x1008);
        assert_eq!(&ram[0x2000..0x2020], &data[..32]);
        assert_eq!(&ram[0x2030..0x2040], &data[32..]);
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_XFERCOMPL, 0);
    }

    #[test]
    fn descriptor_dma_returns_nak_when_chain_headroom_is_insufficient() {
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x10, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        assert_eq!(
            core.host_send_out_with_dma(0, &[9; 64], Some(&mut dma)),
            UsbPacketResult::Nak
        );
        // No payload was placed, no completion state written.
        assert_eq!(ram[0x2000], 0);
        let status = u32::from_le_bytes(ram[0x1000..0x1004].try_into().unwrap());
        assert_eq!(status & DEV_DMA_NBYTES_MASK, 0x10);
        assert_eq!(r(&mut core, DOEPINT_BASE), 0);
        assert_ne!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
    }

    #[test]
    fn descriptor_dma_probe_terminates_on_unterminated_chain_without_fault() {
        // An all-zeroed chain (HREADY, NBYTES=0, `L` never set) must not spin
        // the capacity probe forever or escalate into an AHBERR fault: it is a
        // NAK because there is no headroom, nothing is placed.
        let mut ram = vec![0u8; 0x9000];
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let mut dma = ram_dma(&mut ram);
        assert_eq!(
            core.host_send_out_with_dma(0, &[9; 64], Some(&mut dma)),
            UsbPacketResult::Nak
        );
        // No DmaFault (no AHBERR), no descriptor consumed, EP still armed.
        assert_eq!(r(&mut core, DOEPINT_BASE) & EPINT_AHBERR, 0);
        assert_eq!(r(&mut core, DOEPINT_BASE), 0);
        assert_ne!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
    }

    #[test]
    fn descriptor_dma_chain_capacity_counts_partial_first_descriptor_once() {
        // The guest re-arms a chain whose current descriptor was partially
        // consumed by the previous transfer (desc_out_off>0) without rewriting
        // the chain bases. The probe must subtract the offset from the first
        // descriptor only; later descriptors count their full NBYTES.
        let mut ram = vec![0u8; 0x4000];
        desc_at(&mut ram, 0x1000, 0x2000, 0x40, false);
        desc_at(&mut ram, 0x1008, 0x2040, 0x40, true);
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        let mut dma = ram_dma(&mut ram);

        // Transfer 1: a short 48-byte packet partially fills desc1 (off=0x30)
        // and completes, leaving EP armed-state cleared but DOEPDMA/off intact.
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 48);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        w(&mut core, DOEPDMA_BASE, 0x1000);
        let t1 = (0..48).map(|i| i as u8).collect::<Vec<_>>();
        assert_eq!(
            core.host_send_out_with_dma(0, &t1, Some(&mut dma)),
            UsbPacketResult::Ack
        );
        let mut buf = vec![0u8; 0x30];
        assert_eq!(dma(0x2000, DmaAccess::Read, &mut buf), Ok(()));
        assert_eq!(&buf, &t1[..]);
        assert_eq!(core.desc_out_off[0], 0x30);

        // Guest re-preps the chain (HREADY) through DMA but does NOT rewrite
        // DOEPDMA, so desc_out_off stays 0x30 on the current descriptor.
        let mut d0 = dwc2_desc_bytes(&Dwc2Desc {
            status: (0x40 & DEV_DMA_NBYTES_MASK) | DEV_DMA_BUFF_STS_HREADY | DEV_DMA_STS_SUCC,
            buf: 0x2000,
        });
        let mut d1 = dwc2_desc_bytes(&Dwc2Desc {
            status: (0x40 & DEV_DMA_NBYTES_MASK)
                | DEV_DMA_BUFF_STS_HREADY
                | DEV_DMA_STS_SUCC
                | DEV_DMA_L,
            buf: 0x2040,
        });
        assert_eq!(dma(0x1000, DmaAccess::Write, &mut d0), Ok(()));
        assert_eq!(dma(0x1008, DmaAccess::Write, &mut d1), Ok(()));
        w(&mut core, DOEPINT_BASE, EPINT_XFERCOMPL);

        // Transfer 2: 64 bytes. Real headroom = (0x40-0x30) + 0x40 = 0x50, so
        // the packet fits; a probe subtracting the offset from BOTH descriptors
        // would see only 0x20 and NAK spuriously.
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 64);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        let t2 = (200..264).map(|i| i as u8).collect::<Vec<_>>();
        assert_eq!(
            core.host_send_out_with_dma(0, &t2, Some(&mut dma)),
            UsbPacketResult::Ack
        );
        // First 16 bytes land in desc1's remaining 0x10 bytes at buf+0x30.
        let mut buf = vec![0u8; 0x10];
        assert_eq!(dma(0x2030, DmaAccess::Read, &mut buf), Ok(()));
        assert_eq!(&buf, &t2[..0x10]);
        // Remaining 48 bytes land in desc2's buffer (0x2040, after desc1).
        let mut buf = vec![0u8; 0x30];
        assert_eq!(dma(0x2040, DmaAccess::Read, &mut buf), Ok(()));
        assert_eq!(&buf, &t2[0x10..]);
        let mut raw = [0u8; 8];
        assert_eq!(dma(0x1008, DmaAccess::Read, &mut raw), Ok(()));
        let s2 = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        assert_eq!(s2 & DEV_DMA_NBYTES_MASK, 0x40 - 48);
        assert_eq!(s2 & DEV_DMA_BUFF_STS_MASK, DEV_DMA_BUFF_STS_DMADONE);
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_XFERCOMPL, 0);
    }

    #[test]
    fn descriptor_dma_fault_routes_to_epint_ahberr_and_clears_epena() {
        let mut core = connected_core();
        w(&mut core, DCFG, DCFG_DESCDMA);
        w(&mut core, DOEPTSIZ_BASE, (1 << 19) | 8);
        w(&mut core, DOEPCTL_BASE, EPCTL_EPENA | EPCTL_CNAK);
        let mut failing: UsbDmaTransfer<'_> =
            Box::leak(Box::new(|_, _, _| Err(DmaFault::UnboundSid(0))));
        assert!(matches!(
            core.host_send_out_with_dma(0, &[1; 8], Some(&mut failing)),
            UsbPacketResult::DmaFault(DmaFault::UnboundSid(_))
        ));
        assert_ne!(r(&mut core, DOEPINT_BASE) & EPINT_AHBERR, 0);
        assert_eq!(r(&mut core, DOEPCTL_BASE) & EPCTL_EPENA, 0);
    }
}
