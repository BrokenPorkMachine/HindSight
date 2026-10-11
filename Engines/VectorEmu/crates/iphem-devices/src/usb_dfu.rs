//! USB Device Controller with DFU Class Support
//!
//! The controller presents the Synopsys DWC2 device-mode register window at
//! the `/soc/usb` aperture (`crates/iphem-devices/src/dwc2.rs`): core/device
//! registers, per-endpoint IN/OUT blocks, and FIFO space. The DFU class
//! engine is available only through `new_simulated_firmware` for host-protocol
//! tests. The normal constructor leaves all request handling to guest code.

use crate::dwc2::Dwc2Core;
use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::VecDeque;
use std::time::Instant;

/// DART-USB stream ID on d321ap / T8020 (ADT `sids` property of the
/// `/arm-io/dart-usb` node). The USB device controller announces DMA via this
/// channel so the transport can stage transfers through
/// `Machine::submit_dma`.
pub const USB_DART_SID: u32 = 7;

// DFU types (inline to avoid visibility issues)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DfuState {
    AppIdle,
    AppDetach,
    DfuIdle,
    DfuDownloadSync,
    DfuDownloadBusy,
    DfuDownloadIdle,
    DfuManifestSync,
    DfuManifest,
    DfuManifestWaitReset,
    DfuUploadIdle,
    DfuError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DfuStatus {
    Ok,
    ErrTarget,
    ErrFile,
    ErrWrite,
    ErrErase,
    ErrCheckErase,
    ErrProg,
    ErrVerify,
    ErrAddress,
    ErrNotDone,
    ErrFirmware,
    ErrVendor,
    ErrUsbR,
    ErrPor,
    ErrUnknown,
    ErrStalledPkt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DfuRequest {
    Detach = 0,
    Dnload = 1,
    Upload = 2,
    GetStatus = 3,
    ClrStatus = 4,
    GetState = 5,
    Abort = 6,
}

impl DfuRequest {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Detach),
            1 => Some(Self::Dnload),
            2 => Some(Self::Upload),
            3 => Some(Self::GetStatus),
            4 => Some(Self::ClrStatus),
            5 => Some(Self::GetState),
            6 => Some(Self::Abort),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DfuStatusResponse {
    pub status: DfuStatus,
    pub poll_timeout: u32,
    pub state: DfuState,
    pub string_index: u8,
}

impl DfuStatusResponse {
    pub fn to_bytes(&self) -> [u8; 6] {
        [
            self.status as u8,
            (self.poll_timeout & 0xFF) as u8,
            ((self.poll_timeout >> 8) & 0xFF) as u8,
            ((self.poll_timeout >> 16) & 0xFF) as u8,
            self.state as u8,
            self.string_index,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareType {
    IBSS,
    IBEC,
    IBOOT,
    SEP,
    Baseband,
    Kernel,
    DeviceTree,
    Unknown,
}

/// `bwPollTimeout` a real j71ap (iPhone XS) DFU device reports in
/// `dfuMANIFEST_SYNC` and `dfuMANIFEST`. iTunes 12.7.1 logs
/// `status.bState == DFU_STATE_MANIFEST, PollTimeout: 3000` and then issues
/// `GetStatus` exactly 3.000 s later, so the value is the device's, not the
/// host's.
pub const DFU_MANIFEST_POLL_TIMEOUT_MS: u32 = 3000;

pub struct DfuEngine {
    state: DfuState,
    status: DfuStatus,
    buffer: Vec<u8>,
    block_num: u16,
    total_bytes: u64,
    expected_size: u64,
    firmware_type: FirmwareType,
    progress_callback: Option<Box<dyn Fn(u64, u64) + Send>>,
    start_time: Option<Instant>,
    /// Caller-supplied monotonic millisecond clock the manifestation interval is
    /// measured against. The model never reads a wall clock, so a replay is
    /// deterministic.
    now_ms: u64,
    /// The terminating zero-length DNLOAD has been accepted.
    manifest_started: bool,
    /// `dfuMANIFEST_SYNC` has already been reported once.
    manifest_sync_reported: bool,
    /// When the device entered `dfuMANIFEST`.
    manifest_started_ms: u64,
}

impl DfuEngine {
    pub fn new() -> Self {
        Self {
            state: DfuState::DfuIdle,
            status: DfuStatus::Ok,
            buffer: Vec::new(),
            block_num: 0,
            total_bytes: 0,
            expected_size: 0,
            firmware_type: FirmwareType::Unknown,
            progress_callback: None,
            start_time: None,
            now_ms: 0,
            manifest_started: false,
            manifest_sync_reported: false,
            manifest_started_ms: 0,
        }
    }

    pub fn set_expected_size(&mut self, size: u64, firmware_type: FirmwareType) {
        self.expected_size = size;
        self.firmware_type = firmware_type;
    }

    pub fn state(&self) -> DfuState {
        self.state
    }

    pub fn get_firmware_type(&self) -> FirmwareType {
        self.firmware_type
    }

    pub fn get_firmware(&self) -> &[u8] {
        &self.buffer
    }

    /// Supply the monotonic millisecond clock the manifestation interval is
    /// measured against.
    pub fn set_time_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    pub fn advance_time_ms(&mut self, delta_ms: u64) {
        self.now_ms = self.now_ms.saturating_add(delta_ms);
    }

    pub fn time_ms(&self) -> u64 {
        self.now_ms
    }

    /// Advance the manifestation sequence the way the real device does:
    /// `dfuMANIFEST_SYNC` (6) is reported exactly once, the next `GetStatus` sees
    /// `dfuMANIFEST` (7), and once `DFU_MANIFEST_POLL_TIMEOUT_MS` has elapsed in
    /// `dfuMANIFEST` the device reports `dfuMANIFEST_WAIT_RESET` (8) and waits
    /// for the reset. The 6 -> 7 edge is poll-driven and the 7 -> 8 edge is
    /// clock-driven: the log shows both 6 and 7 returned inside the same
    /// millisecond, then 8 exactly 3.000 s later.
    fn advance_manifest(&mut self) {
        match self.state {
            DfuState::DfuManifestSync => {
                if self.manifest_sync_reported {
                    self.state = DfuState::DfuManifest;
                    self.manifest_started_ms = self.now_ms;
                } else {
                    self.manifest_sync_reported = true;
                }
            }
            DfuState::DfuManifest
                if self.now_ms.saturating_sub(self.manifest_started_ms)
                    >= u64::from(DFU_MANIFEST_POLL_TIMEOUT_MS) =>
            {
                self.state = DfuState::DfuManifestWaitReset;
            }
            _ => {}
        }
    }

    pub fn handle_control_request(
        &mut self,
        request: DfuRequest,
        w_value: u16,
        _w_index: u16,
        w_length: u16,
        data: &[u8],
    ) -> Result<Vec<u8>, DfuStatus> {
        match request {
            DfuRequest::Detach => {
                self.state = DfuState::AppDetach;
                self.status = DfuStatus::Ok;
                Ok(vec![])
            }
            DfuRequest::Dnload => {
                self.block_num = w_value; // DFU block number in wValue

                if w_length == 0 {
                    if self.state == DfuState::DfuDownloadIdle
                        || self.state == DfuState::DfuDownloadSync
                    {
                        self.state = DfuState::DfuManifestSync;
                        self.manifest_started = true;
                        self.manifest_sync_reported = false;
                        self.manifest_started_ms = self.now_ms;
                        self.status = DfuStatus::Ok;
                        return Ok(vec![]);
                    } else {
                        self.status = DfuStatus::ErrTarget;
                        self.state = DfuState::DfuError;
                        return Err(DfuStatus::ErrTarget);
                    }
                }

                if self.state != DfuState::DfuDownloadIdle
                    && self.state != DfuState::DfuDownloadSync
                {
                    if self.state == DfuState::DfuIdle {
                        self.state = DfuState::DfuDownloadSync;
                        self.manifest_started = false;
                        self.manifest_sync_reported = false;
                    } else {
                        self.status = DfuStatus::ErrTarget;
                        self.state = DfuState::DfuError;
                        return Err(DfuStatus::ErrTarget);
                    }
                }

                self.state = DfuState::DfuDownloadBusy;

                if self.total_bytes + data.len() as u64 > self.expected_size
                    && self.expected_size > 0
                {
                    self.status = DfuStatus::ErrFile;
                    self.state = DfuState::DfuError;
                    return Err(DfuStatus::ErrFile);
                }

                self.buffer.extend_from_slice(data);
                self.total_bytes += data.len() as u64;

                if let Some(cb) = &self.progress_callback {
                    cb(self.total_bytes, self.expected_size);
                }

                self.state = DfuState::DfuDownloadIdle;
                self.status = DfuStatus::Ok;
                Ok(vec![])
            }
            DfuRequest::Upload => {
                // Return a chunk of the received firmware. wValue = block number,
                // wLength = bytes requested. A zero-length reply ends the upload.
                let block = w_value as usize;
                let read_len = w_length as usize;
                let offset = block.saturating_mul(read_len.max(1));
                if read_len == 0 || offset >= self.buffer.len() {
                    if matches!(
                        self.state,
                        DfuState::DfuDownloadIdle
                            | DfuState::DfuDownloadSync
                            | DfuState::DfuUploadIdle
                            | DfuState::DfuManifestSync
                    ) || self.state == DfuState::DfuIdle
                    {
                        self.state = DfuState::DfuUploadIdle;
                    }
                    self.status = DfuStatus::Ok;
                    Ok(vec![])
                } else {
                    let end = std::cmp::min(offset + read_len, self.buffer.len());
                    self.status = DfuStatus::Ok;
                    Ok(self.buffer[offset..end].to_vec())
                }
            }
            DfuRequest::GetStatus => {
                self.advance_manifest();
                let resp = self.get_status();
                Ok(resp.to_bytes().to_vec())
            }
            DfuRequest::ClrStatus => {
                if self.state == DfuState::DfuError {
                    self.state = DfuState::DfuIdle;
                    self.status = DfuStatus::Ok;
                }
                Ok(vec![])
            }
            DfuRequest::GetState => Ok(vec![self.state as u8]),
            DfuRequest::Abort => {
                match self.state {
                    DfuState::DfuDownloadIdle | DfuState::DfuDownloadSync => {
                        self.state = DfuState::DfuIdle;
                        self.buffer.clear();
                        self.total_bytes = 0;
                        self.status = DfuStatus::Ok;
                    }
                    DfuState::DfuManifestSync => {
                        self.state = DfuState::DfuIdle;
                        self.buffer.clear();
                        self.total_bytes = 0;
                        self.status = DfuStatus::Ok;
                    }
                    _ => {}
                }
                self.manifest_started = false;
                self.manifest_sync_reported = false;
                Ok(vec![])
            }
        }
    }

    fn get_status(&self) -> DfuStatusResponse {
        DfuStatusResponse {
            status: self.status,
            // `dfuMANIFEST` advertises the measured 3000 ms. The `dfuMANIFEST_SYNC`
            // response carries no evidence in the log: iTunes read state 6 and
            // issued its next `GetStatus` in the same millisecond, so the device
            // either reported 0 there or the host ignores the field in that
            // state. 0 is reported because it is the value consistent with the
            // observed poll cadence.
            poll_timeout: match self.state {
                DfuState::DfuDownloadBusy => 10,
                DfuState::DfuManifest => DFU_MANIFEST_POLL_TIMEOUT_MS,
                _ => 0,
            },
            state: self.state,
            string_index: 0,
        }
    }

    pub fn block_number(&self) -> u16 {
        self.block_num
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn progress(&self) -> f64 {
        if self.expected_size > 0 {
            (self.total_bytes as f64 / self.expected_size as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn set_progress_callback<F>(&mut self, cb: F)
    where
        F: Fn(u64, u64) + Send + 'static,
    {
        self.progress_callback = Some(Box::new(cb));
    }

    /// The terminating zero-length DNLOAD was accepted, so the whole image has
    /// arrived. This stays true across `dfuMANIFEST_SYNC`, `dfuMANIFEST` and
    /// `dfuMANIFEST_WAIT_RESET`, which are post-transfer states, and it is not a
    /// claim that the image was programmed or verified.
    pub fn is_complete(&self) -> bool {
        self.manifest_started && self.total_bytes > 0
    }

    pub fn start_transfer(&mut self) {
        self.start_time = Some(Instant::now());
    }

    /// Transfer rate in MB/s since the last `start_transfer`.
    pub fn get_transfer_rate(&self) -> Option<f64> {
        self.start_time.map(|t| {
            let elapsed = t.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                self.total_bytes as f64 / elapsed / 1_000_000.0
            } else {
                0.0
            }
        })
    }
}

impl Default for DfuEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbPid {
    Setup = 0x2D,
    In = 0x69,
    Out = 0xE1,
    Data0 = 0xC3,
    Data1 = 0x4B,
    Ack = 0xD2,
    Nak = 0x5A,
    Stall = 0x1E,
    Nyet = 0x96,
}
#[derive(Debug, Clone)]
pub struct UsbSetupPacket {
    pub bm_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
}

// Apple-specific DFU commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleImageType {
    IBSS = 0x01,
    IBEC = 0x02,
    IBOOT = 0x03,
    SEP = 0x04,
    Baseband = 0x05,
    Kernel = 0x06,
    DeviceTree = 0x07,
}

impl AppleImageType {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "iBSS" => Some(Self::IBSS),
            "iBEC" => Some(Self::IBEC),
            "iBoot" | "iBOT" => Some(Self::IBOOT),
            "SEP" => Some(Self::SEP),
            "Baseband" => Some(Self::Baseband),
            "Kernel" => Some(Self::Kernel),
            "DeviceTree" => Some(Self::DeviceTree),
            _ => None,
        }
    }

    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x01 => Some(Self::IBSS),
            0x02 => Some(Self::IBEC),
            0x03 => Some(Self::IBOOT),
            0x04 => Some(Self::SEP),
            0x05 => Some(Self::Baseband),
            0x06 => Some(Self::Kernel),
            0x07 => Some(Self::DeviceTree),
            _ => None,
        }
    }

    pub fn to_firmware_type(self) -> FirmwareType {
        match self {
            Self::IBSS => FirmwareType::IBSS,
            Self::IBEC => FirmwareType::IBEC,
            Self::IBOOT => FirmwareType::IBOOT,
            Self::SEP => FirmwareType::SEP,
            Self::Baseband => FirmwareType::Baseband,
            Self::Kernel => FirmwareType::Kernel,
            Self::DeviceTree => FirmwareType::DeviceTree,
        }
    }
}

impl UsbSetupPacket {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            bm_request_type: data[0],
            b_request: data[1],
            w_value: u16::from_le_bytes([data[2], data[3]]),
            w_index: u16::from_le_bytes([data[4], data[5]]),
            w_length: u16::from_le_bytes([data[6], data[7]]),
        })
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.bm_request_type,
            self.b_request,
            (self.w_value & 0xFF) as u8,
            (self.w_value >> 8) as u8,
            (self.w_index & 0xFF) as u8,
            (self.w_index >> 8) as u8,
            (self.w_length & 0xFF) as u8,
            (self.w_length >> 8) as u8,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct UsbEndpoint {
    pub ep_num: u8,
    pub ep_type: u8,
    pub max_packet_size: u16,
    pub rx_buffer: VecDeque<u8>,
    pub tx_buffer: VecDeque<u8>,
    pub stall: bool,
    pub data_toggle: bool,
}

impl UsbEndpoint {
    pub fn new(ep_num: u8, ep_type: u8, max_packet_size: u16) -> Self {
        Self {
            ep_num,
            ep_type,
            max_packet_size,
            rx_buffer: VecDeque::new(),
            tx_buffer: VecDeque::new(),
            stall: false,
            data_toggle: false,
        }
    }

    pub fn max_packet_size(&self) -> u16 {
        self.max_packet_size
    }
}

pub struct UsbDeviceController {
    pub id: DeviceId,
    pub base_addr: u64,
    pub endpoints: [UsbEndpoint; 16],
    pub setup_packet: Option<UsbSetupPacket>,
    pub ep0_state: Ep0State,
    pub device_address: u8,
    pub configuration: u8,
    pub dfu_engine: DfuEngine,
    pub pending_setup: Option<UsbSetupPacket>,
    pub ep0_data_phase: Ep0DataPhase,
    pub remote_wakeup: bool,
    pub self_powered: bool,
    pub apple_mode: bool,
    pub image_type: Option<AppleImageType>,
    pub irq: u32,
    pub interrupt_pending: bool,
    /// Synopsys DWC2 device-mode register surface for the controller.
    pub dwc2: Dwc2Core,
    /// Set to announce one DMA transfer for the DART-USB stream; a single
    /// tick consumes it into a `DeviceEvent::DmaRequest`.
    dma_event_pending: bool,
    emulate_class_firmware: bool,
    pub mmio_reads: u64,
    pub mmio_writes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ep0State {
    Idle,
    Setup,
    DataIn,
    DataOut,
    StatusIn,
    StatusOut,
    Stall,
}

#[derive(Debug, Clone)]
pub enum Ep0DataPhase {
    None,
    In { data: Vec<u8>, sent: usize },
    Out { buffer: Vec<u8>, expected: usize },
}

impl UsbDeviceController {
    pub fn new(id: DeviceId, base_addr: u64, irq: u32) -> Self {
        let mut endpoints = Vec::new();
        for i in 0..16 {
            endpoints.push(UsbEndpoint::new(i, 0, 64));
        }
        endpoints[0] = UsbEndpoint::new(0, 0, 64);
        endpoints[1] = UsbEndpoint::new(1, 2, 64);

        Self {
            id,
            base_addr,
            endpoints: endpoints.try_into().unwrap(),
            setup_packet: None,
            ep0_state: Ep0State::Idle,
            device_address: 0,
            configuration: 0,
            dfu_engine: DfuEngine::new(),
            pending_setup: None,
            ep0_data_phase: Ep0DataPhase::None,
            remote_wakeup: false,
            self_powered: true,
            apple_mode: false,
            image_type: None,
            irq,
            interrupt_pending: false,
            dwc2: Dwc2Core::new(),
            dma_event_pending: false,
            emulate_class_firmware: false,
            mmio_reads: 0,
            mmio_writes: 0,
        }
    }

    /// Isolated protocol-test fixture; never use for an executing boot chain.
    pub fn new_simulated_firmware(id: DeviceId, base_addr: u64, irq: u32) -> Self {
        let mut controller = Self::new(id, base_addr, irq);
        controller.emulate_class_firmware = true;
        controller.dwc2.write(crate::dwc2::DCTL, 4, 0);
        controller
    }

    fn set_irq(&mut self) {
        self.interrupt_pending = true;
    }

    /// Arm a single `DeviceEvent::DmaRequest` for the DART-USB stream. The
    /// event is a transport signal only: the transfer details (IOVA,
    /// direction, size) are staged on the Machine via `Machine::submit_dma`,
    /// not synthesized here, so the device never invents a guest-visible DMA
    /// side effect.
    pub fn raise_dma_event(&mut self) {
        self.dma_event_pending = true;
    }

    fn handle_standard_request(&mut self, setup: &UsbSetupPacket) -> Result<Vec<u8>, DfuStatus> {
        let request = setup.b_request;
        let w_value = setup.w_value;
        let w_index = setup.w_index;
        let w_length = setup.w_length;

        match request {
            0x00 => {
                // GET_STATUS
                if w_length >= 2 {
                    Ok(vec![0, 0])
                } else {
                    Err(DfuStatus::ErrVendor)
                }
            }
            0x01 => {
                // CLEAR_FEATURE
                if w_value == 1 {
                    // ENDPOINT_HALT
                    if w_index == 0 || w_index == 0x80 {
                        self.endpoints[0].stall = false;
                    }
                } else if w_value == 0 {
                    // DEVICE_REMOTE_WAKEUP
                    self.remote_wakeup = false;
                }
                Ok(vec![])
            }
            0x03 => {
                // SET_FEATURE
                if w_value == 1 {
                    // ENDPOINT_HALT
                    if w_index == 0 || w_index == 0x80 {
                        self.endpoints[0].stall = true;
                        self.ep0_state = Ep0State::Stall;
                    }
                } else if w_value == 0 {
                    // DEVICE_REMOTE_WAKEUP
                    self.remote_wakeup = true;
                }
                Ok(vec![])
            }
            0x05 => {
                // SET_ADDRESS
                self.device_address = w_value as u8;
                Ok(vec![])
            }
            0x06 => {
                // GET_DESCRIPTOR
                let dtype = (w_value >> 8) as u8;
                let dindex = (w_value & 0xFF) as u8;
                self.handle_get_descriptor(dtype, dindex, w_length)
            }
            0x08 => {
                // GET_CONFIGURATION
                Ok(vec![self.configuration])
            }
            0x09 => {
                // SET_CONFIGURATION
                self.configuration = w_value as u8;
                self.endpoints[0].stall = false;
                Ok(vec![])
            }
            0x0A => {
                // GET_INTERFACE
                Ok(vec![0])
            }
            0x0B => {
                // SET_INTERFACE
                Ok(vec![])
            }
            _ => Err(DfuStatus::ErrVendor),
        }
    }

    fn handle_get_descriptor(
        &self,
        dtype: u8,
        dindex: u8,
        w_length: u16,
    ) -> Result<Vec<u8>, DfuStatus> {
        match dtype {
            0x01 => {
                // Device descriptor
                let mut desc = Vec::with_capacity(18);
                desc.extend_from_slice(&[
                    18,   // bLength
                    0x01, // bDescriptorType (Device)
                    0x00, 0x02, // bcdUSB 2.0
                    0x00, // bDeviceClass
                    0x00, // bDeviceSubClass
                    0x00, // bDeviceProtocol
                    64,   // bMaxPacketSize0
                    0x5A, 0x05, // idVendor (Apple: 0x05AC)
                    0x01, 0x12, // idProduct (iPhone DFU mode: 0x1227 or similar)
                    0x00, 0x01, // bcdDevice
                    0x01, // iManufacturer
                    0x02, // iProduct
                    0x03, // iSerialNumber
                    0x01, // bNumConfigurations
                ]);
                Ok(desc[..w_length as usize].to_vec())
            }
            0x02 => {
                // Configuration descriptor
                let mut desc = Vec::with_capacity(32);
                desc.extend_from_slice(&[
                    9,    // bLength
                    0x02, // bDescriptorType (Configuration)
                    32, 0,    // wTotalLength
                    1,    // bNumInterfaces
                    1,    // bConfigurationValue
                    0,    // iConfiguration
                    0x80, // bmAttributes (bus powered)
                    50,   // bMaxPower (100mA)
                ]);
                desc.extend_from_slice(&[
                    9,    // bLength
                    0x04, // bDescriptorType (Interface)
                    0,    // bInterfaceNumber
                    0,    // bAlternateSetting
                    0,    // bNumEndpoints (DFU uses only EP0)
                    0xFE, // bInterfaceClass (Application Specific)
                    0x01, // bInterfaceSubClass (DFU)
                    0x02, // bInterfaceProtocol (Runtime/DFU mode)
                    0,    // iInterface
                ]);
                desc.extend_from_slice(&[
                    9,    // bLength
                    0x21, // bDescriptorType (DFU Functional)
                    0x0D, // bmAttributes (bitCanDownload | bitWillDetach | bitManifestationTolerant)
                    0xFF, 0x00, // wDetachTimeout
                    0x00, 0x10, // wTransferSize (4096)
                    0x00, 0x02, // bcdDFUVersion 1.1
                ]);
                Ok(desc[..w_length as usize].to_vec())
            }
            0x03 => {
                // String descriptors
                match dindex {
                    0 => {
                        // Language ID
                        Ok(vec![4, 0x03, 0x09, 0x04])
                    }
                    1 => {
                        // Manufacturer: "Apple Inc."
                        let s = "Apple Inc.";
                        let mut desc = vec![s.len() as u8 * 2 + 2, 0x03];
                        for c in s.chars() {
                            desc.extend_from_slice(&(c as u16).to_le_bytes());
                        }
                        Ok(desc[..w_length as usize].to_vec())
                    }
                    2 => {
                        // Product: "Apple DFU Device"
                        let s = "Apple DFU Device";
                        let mut desc = vec![s.len() as u8 * 2 + 2, 0x03];
                        for c in s.chars() {
                            desc.extend_from_slice(&(c as u16).to_le_bytes());
                        }
                        Ok(desc[..w_length as usize].to_vec())
                    }
                    3 => {
                        // Serial number
                        let s = "000000000000";
                        let mut desc = vec![s.len() as u8 * 2 + 2, 0x03];
                        for c in s.chars() {
                            desc.extend_from_slice(&(c as u16).to_le_bytes());
                        }
                        Ok(desc[..w_length as usize].to_vec())
                    }
                    _ => Err(DfuStatus::ErrVendor),
                }
            }
            _ => Err(DfuStatus::ErrVendor),
        }
    }

    fn handle_class_request(
        &mut self,
        setup: &UsbSetupPacket,
        data: &[u8],
    ) -> Result<Vec<u8>, DfuStatus> {
        let request = DfuRequest::from_u8(setup.b_request).ok_or(DfuStatus::ErrVendor)?;
        let w_value = setup.w_value;
        let w_index = setup.w_index;
        let w_length = setup.w_length;

        match request {
            DfuRequest::Detach => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
            DfuRequest::Dnload => {
                if self.apple_mode
                    && self.image_type.is_none()
                    && self.dfu_engine.total_bytes == 0
                    && setup.w_length == 4
                    && data.len() >= 2
                {
                    if let Some(img_type) =
                        AppleImageType::from_u16(u16::from_le_bytes([data[0], data[1]]))
                    {
                        self.image_type = Some(img_type);
                        self.dfu_engine
                            .set_expected_size(0, img_type.to_firmware_type());
                        return Ok(vec![]);
                    }
                }
                self.dfu_engine
                    .handle_control_request(request, w_value, w_index, w_length, data)
            }
            DfuRequest::Upload => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
            DfuRequest::GetStatus => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
            DfuRequest::ClrStatus => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
            DfuRequest::GetState => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
            DfuRequest::Abort => self
                .dfu_engine
                .handle_control_request(request, w_value, w_index, w_length, data),
        }
    }

    fn handle_apple_request(
        &mut self,
        setup: &UsbSetupPacket,
        _data: &[u8],
    ) -> Result<Vec<u8>, DfuStatus> {
        let request = setup.b_request;
        let w_value = setup.w_value;
        let w_index = setup.w_index;

        match request {
            0x80 => {
                // APPLE_GET_DEVICE_INFO
                let mut resp = Vec::new();
                resp.extend_from_slice(&0x8020u32.to_le_bytes());
                resp.extend_from_slice(&0x0000u32.to_le_bytes());
                resp.extend_from_slice(&0x0000000000000000u64.to_le_bytes());
                resp.push(1);
                resp.push(0);
                Ok(resp)
            }
            0x81 => {
                // APPLE_SET_IMAGE_TYPE
                if let Some(img_type) = AppleImageType::from_u16(w_value) {
                    self.image_type = Some(img_type);
                    self.dfu_engine
                        .set_expected_size(0, img_type.to_firmware_type());
                    self.apple_mode = true;
                }
                Ok(vec![])
            }
            0x82 => {
                // APPLE_GET_IMAGE_INFO
                let mut resp = Vec::new();
                if let Some(img_type) = self.image_type {
                    resp.push(img_type as u8);
                }
                Ok(resp)
            }
            0x83 => {
                // APPLE_SEND_RECEIPT
                let mut resp = Vec::new();
                resp.extend_from_slice(&w_index.to_le_bytes());
                resp.extend_from_slice(&w_value.to_le_bytes());
                resp.push(0xA5); // ack marker
                Ok(resp)
            }
            _ => Err(DfuStatus::ErrVendor),
        }
    }

    pub fn queue_host_request(&mut self, setup: UsbSetupPacket, data: Vec<u8>) {
        if !self.emulate_class_firmware {
            self.host_send_request(setup, data);
            return;
        }
        self.pending_setup = Some(setup);
        self.process_pending_request(data);
    }

    /// Feed the DFU manifestation interval's millisecond clock. The emulated
    /// class firmware is driven by host polls, so nothing in `Device::tick`
    /// (which counts guest cycles, not milliseconds) can supply this.
    pub fn advance_time_ms(&mut self, delta_ms: u64) {
        self.dfu_engine.advance_time_ms(delta_ms);
    }

    /// Absolute form of [`UsbDeviceController::advance_time_ms`].
    pub fn set_time_ms(&mut self, now_ms: u64) {
        self.dfu_engine.set_time_ms(now_ms);
    }

    fn process_pending_request(&mut self, data: Vec<u8>) {
        if let Some(setup) = self.pending_setup.take() {
            let is_standard = (setup.bm_request_type & 0x60) == 0x00;
            let is_class = (setup.bm_request_type & 0x60) == 0x20;
            let is_vendor = (setup.bm_request_type & 0x60) == 0x40;

            let result = if is_standard {
                self.handle_standard_request(&setup)
            } else if is_class {
                self.handle_class_request(&setup, &data)
            } else if is_vendor {
                self.handle_apple_request(&setup, &data)
            } else {
                Err(DfuStatus::ErrVendor)
            };

            match result {
                Ok(resp) => {
                    self.endpoints[0].tx_buffer.clear();
                    self.endpoints[0].tx_buffer.extend(resp);
                    self.ep0_state = Ep0State::DataIn;
                    self.set_irq();
                }
                Err(status) => {
                    self.endpoints[0].stall = true;
                    self.ep0_state = Ep0State::Stall;
                    self.dfu_engine.status = status;
                    self.set_irq();
                }
            }
        }
    }

    pub fn get_dfu_firmware(&self) -> &[u8] {
        self.dfu_engine.get_firmware()
    }

    pub fn get_dfu_type(&self) -> FirmwareType {
        self.dfu_engine.get_firmware_type()
    }

    /// Host attached the device to the bus.
    pub fn host_attach(&mut self) {
        self.dwc2.host_attach();
    }

    /// Host detached / soft-disconnected the device.
    pub fn host_detach(&mut self) {
        self.dwc2.host_detach();
    }

    /// Host issued the USB bus reset that starts device enumeration.
    pub fn host_bus_reset(&mut self) {
        self.dwc2.host_bus_reset();
    }

    /// Host finished enumeration at the given negotiated speed
    /// (`dwc2::ENUMSPD_HS` / `dwc2::ENUMSPD_FS`).
    pub fn host_enum_done(&mut self, speed: u32) {
        self.dwc2.host_enum_done(speed);
    }

    /// Deliver an OUT data burst to an endpoint from the host.
    pub fn host_out_data(&mut self, ep: usize, data: &[u8]) {
        self.dwc2.host_send_out(ep, data);
    }

    /// Consume the IN data a guest staged into an endpoint TX FIFO, as the
    /// host would when receiving an IN packet.
    pub fn host_read_in(&mut self, ep: usize) -> Vec<u8> {
        self.dwc2.host_read_in(ep)
    }

    /// Drive a full host control transfer: enqueue the SETUP packet (and any
    /// host-to-device data phase) into the DWC2 RX side, run the class
    /// firmware over the request, then stage its response into the EP0 IN
    /// FIFO exactly like device firmware would.
    pub fn host_send_request(&mut self, setup: UsbSetupPacket, data: Vec<u8>) {
        if self.emulate_class_firmware {
            self.dwc2.write(
                crate::dwc2::DOEPCTL_BASE,
                4,
                (crate::dwc2::EPCTL_EPENA | crate::dwc2::EPCTL_CNAK) as u64,
            );
        }
        let mut setup_bytes = [0u8; 8];
        setup_bytes[0] = setup.bm_request_type;
        setup_bytes[1] = setup.b_request;
        setup_bytes[2..4].copy_from_slice(&setup.w_value.to_le_bytes());
        setup_bytes[4..6].copy_from_slice(&setup.w_index.to_le_bytes());
        setup_bytes[6..8].copy_from_slice(&setup.w_length.to_le_bytes());
        self.dwc2.host_send_setup(&setup_bytes);
        if setup.bm_request_type & 0x80 == 0 && !data.is_empty() {
            for packet in data.chunks(64) {
                if self.emulate_class_firmware {
                    self.dwc2
                        .write(crate::dwc2::DOEPTSIZ_BASE, 4, packet.len() as u64);
                    self.dwc2.write(
                        crate::dwc2::DOEPCTL_BASE,
                        4,
                        (crate::dwc2::EPCTL_EPENA | crate::dwc2::EPCTL_CNAK) as u64,
                    );
                }
                self.dwc2.host_send_out(0, packet);
            }
        }
        if !self.emulate_class_firmware {
            return;
        }
        self.pending_setup = Some(setup);
        self.process_pending_request(data);
        let response: Vec<u8> = self.endpoints[0].tx_buffer.iter().copied().collect();
        self.dwc2.stage_ep0_in(&response);
    }
}

impl Device for UsbDeviceController {
    fn id(&self) -> DeviceId {
        self.id
    }

    fn name(&self) -> &'static str {
        "usb-dfu"
    }

    fn reset(&mut self) {
        self.ep0_state = Ep0State::Idle;
        self.device_address = 0;
        self.configuration = 0;
        self.dfu_engine = DfuEngine::new();
        self.pending_setup = None;
        self.ep0_data_phase = Ep0DataPhase::None;
        self.remote_wakeup = false;
        self.self_powered = true;
        self.apple_mode = false;
        self.image_type = None;
        self.interrupt_pending = false;
        self.dma_event_pending = false;
        self.dwc2 = Dwc2Core::new();
        if self.emulate_class_firmware {
            self.dwc2.write(crate::dwc2::DCTL, 4, 0);
        }
        for ep in &mut self.endpoints {
            ep.rx_buffer.clear();
            ep.tx_buffer.clear();
            ep.stall = false;
            ep.data_toggle = false;
        }
    }

    fn read(&mut self, offset: u64, size: usize) -> DeviceResult {
        self.mmio_reads = self.mmio_reads.saturating_add(1);
        if let Some(value) = self.dwc2.read(offset, size) {
            return DeviceResult::Value(value);
        }
        if let Some(value) = self.dwc2.read_fifo(offset, size) {
            return DeviceResult::Value(value);
        }
        DeviceResult::Fault
    }

    fn write(&mut self, offset: u64, size: usize, value: u64) -> DeviceResult {
        self.mmio_writes = self.mmio_writes.saturating_add(1);
        if self.dwc2.write(offset, size, value).is_some() {
            return DeviceResult::Ok;
        }
        if self.dwc2.write_fifo(offset, size, value).is_some() {
            return DeviceResult::Ok;
        }
        DeviceResult::Fault
    }

    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        if self.dma_event_pending {
            self.dma_event_pending = false;
            return Some(DeviceEvent::DmaRequest {
                channel: USB_DART_SID,
            });
        }
        if !self.emulate_class_firmware {
            return self
                .dwc2
                .has_unmasked_int()
                .then_some(DeviceEvent::Interrupt(self.irq));
        }
        match &mut self.ep0_data_phase {
            Ep0DataPhase::In { data, sent } => {
                let ep = &mut self.endpoints[0];
                let remaining = data.len() - *sent;
                let chunk_size = std::cmp::min(remaining, ep.max_packet_size as usize);
                if chunk_size == 0 {
                    self.ep0_state = Ep0State::StatusOut;
                    self.ep0_data_phase = Ep0DataPhase::None;
                } else {
                    ep.tx_buffer.extend(&data[*sent..*sent + chunk_size]);
                    *sent += chunk_size;
                    if chunk_size < ep.max_packet_size as usize {
                        self.ep0_state = Ep0State::StatusOut;
                        self.ep0_data_phase = Ep0DataPhase::None;
                    }
                }
            }
            Ep0DataPhase::Out {
                buffer: _,
                expected,
            } => {
                let ep = &mut self.endpoints[0];
                if ep.rx_buffer.len() >= *expected {
                    let data: Vec<u8> = ep.rx_buffer.drain(..*expected).collect();
                    self.ep0_data_phase = Ep0DataPhase::None;
                    if let Some(setup) = self.pending_setup.take() {
                        let _ = self.handle_class_request(&setup, &data);
                        self.ep0_state = Ep0State::StatusIn;
                    }
                }
            }
            Ep0DataPhase::None => {}
        }

        if self.interrupt_pending || self.dwc2.pending_interrupt() {
            if self.interrupt_pending {
                self.interrupt_pending = false;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Register write helper; DWC2 values are u32 word constants.
    fn reg_w(ctl: &mut UsbDeviceController, off: u64, val: u64) {
        assert!(matches!(ctl.write(off, 4, val), DeviceResult::Ok));
    }

    /// Register read helper.
    fn reg_r(ctl: &mut UsbDeviceController, off: u64) -> u64 {
        ctl.read(off, 4).unwrap_u64()
    }

    #[test]
    fn test_dfu_download_to_manifest_and_complete() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(1024, FirmwareType::IBSS);
        engine.start_transfer();

        assert_eq!(engine.state(), DfuState::DfuIdle);
        assert!(!engine.is_complete());
        assert!(engine.get_transfer_rate().is_some());

        let mut expected_total = 0u64;
        for (block, byte) in [(1u16, 0xAAu8), (2, 0xBB)] {
            let resp =
                engine.handle_control_request(DfuRequest::Dnload, block, 0, 512, &vec![byte; 512]);
            assert!(resp.is_ok());
            assert_eq!(engine.block_number(), block);
            expected_total += 512;
            assert_eq!(engine.total_bytes(), expected_total);
        }

        assert_eq!(engine.progress(), 1.0);
        let resp = engine.handle_control_request(DfuRequest::Dnload, 3, 0, 0, &[]);
        assert!(resp.is_ok());
        assert_eq!(engine.state(), DfuState::DfuManifestSync);
        assert!(engine.is_complete());
    }

    /// iTunes 12.7.1 against a real j71ap: after the terminating zero-length
    /// DNLOAD the device reports `dfuMANIFEST_SYNC` (6) once, `dfuMANIFEST` (7)
    /// on the next `GetStatus` — both inside the same millisecond — and
    /// `dfuMANIFEST_WAIT_RESET` (8) after the 3000 ms `bwPollTimeout` it
    /// advertised. The log's own timing is 01:02:22.0830 (6), 01:02:22.0830 (7),
    /// 01:02:25.0831 (8).
    #[test]
    fn test_manifest_handshake_reports_six_seven_eight_with_real_poll_timeout() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(4, FirmwareType::IBSS);
        engine
            .handle_control_request(DfuRequest::Dnload, 1, 0, 4, &[0xde, 0xad, 0xbe, 0xef])
            .unwrap();
        engine.set_time_ms(0);
        engine
            .handle_control_request(DfuRequest::Dnload, 2, 0, 0, &[])
            .unwrap();
        assert_eq!(engine.state(), DfuState::DfuManifestSync);

        let poll = |engine: &mut DfuEngine, now_ms: u64| -> (u8, u32) {
            engine.set_time_ms(now_ms);
            let resp = engine
                .handle_control_request(DfuRequest::GetStatus, 0, 0, 0, &[])
                .unwrap();
            assert_eq!(resp.len(), 6);
            assert_eq!(resp[0], DfuStatus::Ok as u8);
            (resp[4], u32::from_le_bytes([resp[1], resp[2], resp[3], 0]))
        };

        assert_eq!(poll(&mut engine, 0), (DfuState::DfuManifestSync as u8, 0));
        assert_eq!(
            poll(&mut engine, 0),
            (DfuState::DfuManifest as u8, DFU_MANIFEST_POLL_TIMEOUT_MS)
        );
        assert_eq!(
            poll(&mut engine, 2_999),
            (DfuState::DfuManifest as u8, DFU_MANIFEST_POLL_TIMEOUT_MS)
        );
        assert_eq!(
            poll(&mut engine, 3_000),
            (DfuState::DfuManifestWaitReset as u8, 0)
        );
        // The device stays in dfuMANIFEST_WAIT_RESET until it is reset.
        assert_eq!(
            poll(&mut engine, 30_000),
            (DfuState::DfuManifestWaitReset as u8, 0)
        );
        assert_eq!(engine.state(), DfuState::DfuManifestWaitReset);
        // The image was received; manifestation is not a programming claim.
        assert!(engine.is_complete());
    }

    /// In `dfuMANIFEST_WAIT_RESET` a further DNLOAD is a protocol error, not the
    /// start of a second image.
    #[test]
    fn test_manifest_wait_reset_rejects_a_further_download() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(4, FirmwareType::IBSS);
        engine
            .handle_control_request(DfuRequest::Dnload, 1, 0, 4, &[1, 2, 3, 4])
            .unwrap();
        engine
            .handle_control_request(DfuRequest::Dnload, 2, 0, 0, &[])
            .unwrap();
        for now_ms in [0, 0, 3_000] {
            engine.set_time_ms(now_ms);
            engine
                .handle_control_request(DfuRequest::GetStatus, 0, 0, 0, &[])
                .unwrap();
        }
        assert_eq!(engine.state(), DfuState::DfuManifestWaitReset);
        assert_eq!(
            engine.handle_control_request(DfuRequest::Dnload, 3, 0, 4, &[5, 6, 7, 8]),
            Err(DfuStatus::ErrTarget)
        );
        assert_eq!(engine.state(), DfuState::DfuError);
        assert_eq!(engine.total_bytes(), 4);
    }

    /// The same 6 -> 7 -> 8 progression is visible to a host driver through the
    /// DFU interface, and the controller's millisecond clock is what drives the
    /// `dfuMANIFEST` interval.
    #[test]
    fn test_controller_manifest_states_reach_wait_reset_over_the_class_interface() {
        let mut ctl = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);
        ctl.set_time_ms(0);
        ctl.dfu_engine.set_expected_size(4, FirmwareType::IBSS);
        let dnload = UsbSetupPacket {
            bm_request_type: 0x21,
            b_request: 1,
            w_value: 1,
            w_index: 0,
            w_length: 4,
        };
        let zlp = UsbSetupPacket {
            bm_request_type: 0x21,
            b_request: 1,
            w_value: 2,
            w_index: 0,
            w_length: 0,
        };
        let getstatus = UsbSetupPacket {
            bm_request_type: 0xa1,
            b_request: 3,
            w_value: 0,
            w_index: 0,
            w_length: 6,
        };
        ctl.handle_class_request(&dnload, &[0xde, 0xad, 0xbe, 0xef])
            .unwrap();
        ctl.handle_class_request(&zlp, &[]).unwrap();
        assert_eq!(ctl.dfu_engine.state(), DfuState::DfuManifestSync);

        let poll = |ctl: &mut UsbDeviceController| -> (u8, u32) {
            let resp = ctl.handle_class_request(&getstatus, &[]).unwrap();
            assert_eq!(resp.len(), 6);
            (resp[4], u32::from_le_bytes([resp[1], resp[2], resp[3], 0]))
        };

        assert_eq!(poll(&mut ctl), (6, 0));
        assert_eq!(poll(&mut ctl), (7, DFU_MANIFEST_POLL_TIMEOUT_MS));
        ctl.advance_time_ms(DFU_MANIFEST_POLL_TIMEOUT_MS as u64);
        assert_eq!(ctl.dfu_engine.time_ms(), 3_000);
        assert_eq!(poll(&mut ctl), (8, 0));
        assert_eq!(ctl.dfu_engine.state(), DfuState::DfuManifestWaitReset);
    }

    #[test]
    fn test_dfu_upload_after_download() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(8, FirmwareType::IBEC);
        let payload = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        engine
            .handle_control_request(DfuRequest::Dnload, 1, 0, 8, &payload)
            .unwrap();
        engine
            .handle_control_request(DfuRequest::Dnload, 2, 0, 0, &[])
            .unwrap();

        // Upload block 0 -> first 4 bytes
        let chunk = engine
            .handle_control_request(DfuRequest::Upload, 0, 0, 4, &[])
            .unwrap();
        assert_eq!(chunk, payload[0..4].to_vec());

        // Upload block 1 -> last 4 bytes
        let chunk = engine
            .handle_control_request(DfuRequest::Upload, 1, 0, 4, &[])
            .unwrap();
        assert_eq!(chunk, payload[4..8].to_vec());

        // Past end -> zero-length signals end of upload
        let end = engine
            .handle_control_request(DfuRequest::Upload, 2, 0, 4, &[])
            .unwrap();
        assert!(end.is_empty());
        assert_eq!(engine.state(), DfuState::DfuUploadIdle);
    }

    #[test]
    fn test_dfu_get_status_and_abort() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(1024, FirmwareType::IBSS);

        let resp = engine
            .handle_control_request(DfuRequest::GetStatus, 0, 0, 0, &[])
            .unwrap();
        assert_eq!(resp.len(), 6);
        assert_eq!(resp[0], DfuStatus::Ok as u8);
        assert_eq!(resp[4], DfuState::DfuIdle as u8);

        engine
            .handle_control_request(DfuRequest::Dnload, 1, 0, 512, &vec![0xAA; 512])
            .unwrap();
        engine
            .handle_control_request(DfuRequest::Abort, 0, 0, 0, &[])
            .unwrap();
        assert_eq!(engine.state(), DfuState::DfuIdle);
        assert_eq!(engine.total_bytes(), 0);
        assert!(engine.get_firmware().is_empty());
    }

    #[test]
    fn test_dfu_progress_callback() {
        let mut engine = DfuEngine::new();
        engine.set_expected_size(16, FirmwareType::Kernel);

        use std::sync::Arc;
        use std::sync::Mutex;

        let mut engine = DfuEngine::new();
        engine.set_expected_size(16, FirmwareType::Kernel);

        let caught = Arc::new(Mutex::new((0u64, 0u64)));
        let caught_cb = caught.clone();
        engine.set_progress_callback(move |current, total| {
            *caught_cb.lock().unwrap() = (current, total);
        });
        engine
            .handle_control_request(DfuRequest::Dnload, 1, 0, 16, &[0x00; 16])
            .unwrap();
        assert_eq!(*caught.lock().unwrap(), (16, 16));
    }

    #[test]
    fn test_dfu_apple_image_type_helpers() {
        assert_eq!(AppleImageType::from_u16(0x01), Some(AppleImageType::IBSS));
        assert_eq!(AppleImageType::from_u16(0xFF), None);
        assert_eq!(
            AppleImageType::from_name("iBoot"),
            Some(AppleImageType::IBOOT)
        );
        assert_eq!(AppleImageType::IBEC.to_firmware_type(), FirmwareType::IBEC);
    }

    #[test]
    fn test_dfu_controller_dwc2_register_session_download() {
        use crate::dwc2;
        let mut ctl = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);

        // Guest soft-connect, then a real host attach/reset/enumeration.
        reg_w(&mut ctl, dwc2::DCTL, 0);
        reg_w(&mut ctl, dwc2::GAHBCFG, 1);
        ctl.host_attach();
        ctl.host_bus_reset();
        assert_eq!(
            reg_r(&mut ctl, dwc2::GINTSTS),
            dwc2::GINTSTS_USBRESET as u64
        );
        reg_w(&mut ctl, dwc2::GINTSTS, dwc2::GINTSTS_USBRESET as u64);
        assert_eq!(
            reg_r(&mut ctl, dwc2::GINTSTS) & dwc2::GINTSTS_USBRESET as u64,
            0
        );

        ctl.host_enum_done(dwc2::ENUMSPD_FS);
        assert!(reg_r(&mut ctl, dwc2::GINTSTS) & dwc2::GINTSTS_ENUMDONE as u64 != 0);
        assert_eq!(
            (reg_r(&mut ctl, dwc2::DSTS) >> dwc2::DSTS_ENUMSPD_SHIFT) & 0x7,
            dwc2::ENUMSPD_FS as u64
        );
        reg_w(&mut ctl, dwc2::GINTSTS, dwc2::GINTSTS_ENUMDONE as u64);

        // Apple SET_IMAGE_TYPE (IBSS).
        let settp = UsbSetupPacket {
            bm_request_type: 0x40,
            b_request: 0x81,
            w_value: 0x01,
            w_index: 0,
            w_length: 0,
        };
        ctl.host_send_request(settp, vec![]);
        assert_eq!(ctl.image_type, Some(AppleImageType::IBSS));
        assert!(ctl.apple_mode);
        // The guest sees a SETUP packet in the RX status queue and FIFO0.
        assert!(reg_r(&mut ctl, dwc2::GINTSTS) & dwc2::GINTSTS_RXFLVL as u64 != 0);
        let setup_status = reg_r(&mut ctl, dwc2::GRXSTSP);
        assert_eq!(
            setup_status & dwc2::RXSTS_PKTSTS_MASK as u64,
            dwc2::RXSTS_SETUP as u64
        );
        assert_eq!(setup_status & 0xf, 0);
        assert_eq!(reg_r(&mut ctl, dwc2::fifo_base(0)), 0x00018140);
        assert!(matches!(
            ctl.read(dwc2::fifo_base(0) + 4, 4),
            DeviceResult::Value(_)
        ));
        assert_eq!(
            reg_r(&mut ctl, dwc2::GRXSTSP),
            dwc2::RXSTS_SETUP_COMPLETE as u64
        );
        assert_eq!(reg_r(&mut ctl, dwc2::GINTSTS), 0);

        // DNLOAD: host sends block 1 with four bytes.
        ctl.dfu_engine.set_expected_size(4, FirmwareType::IBSS);
        let dnload = UsbSetupPacket {
            bm_request_type: 0x21,
            b_request: 1,
            w_value: 1,
            w_index: 0,
            w_length: 4,
        };
        ctl.host_send_request(dnload, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(reg_r(&mut ctl, dwc2::GINTSTS) & dwc2::GINTSTS_RXFLVL as u64 != 0);
        let setup_status = reg_r(&mut ctl, dwc2::GRXSTSP);
        assert_eq!(
            setup_status & dwc2::RXSTS_PKTSTS_MASK as u64,
            dwc2::RXSTS_SETUP as u64
        );
        assert_eq!((setup_status >> 4) & 0x7ff, 8);
        assert_eq!(reg_r(&mut ctl, dwc2::fifo_base(0)), 0x00010121);
        assert_eq!(reg_r(&mut ctl, dwc2::fifo_base(0) + 4), 0x00040000);
        assert_eq!(
            reg_r(&mut ctl, dwc2::GRXSTSP),
            dwc2::RXSTS_SETUP_COMPLETE as u64
        );
        let out_status = reg_r(&mut ctl, dwc2::GRXSTSP);
        assert_eq!(
            out_status & dwc2::RXSTS_PKTSTS_MASK as u64,
            dwc2::RXSTS_DATA_OUT as u64
        );
        assert_eq!((out_status >> 4) & 0x7ff, 4);
        assert_eq!(reg_r(&mut ctl, dwc2::fifo_base(0)), 0xEFBEADDE);
        assert_eq!(
            reg_r(&mut ctl, dwc2::GRXSTSP),
            dwc2::RXSTS_OUT_COMPLETE as u64
        );
        assert_eq!(reg_r(&mut ctl, dwc2::GRXSTSP), 0);

        assert_eq!(ctl.get_dfu_firmware(), &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(ctl.get_dfu_type(), FirmwareType::IBSS);
        assert_eq!(ctl.dfu_engine.total_bytes, 4);

        // Zero-length DNLOAD -> manifest.
        let manifest = UsbSetupPacket {
            bm_request_type: 0x21,
            b_request: 1,
            w_value: 2,
            w_index: 0,
            w_length: 0,
        };
        ctl.host_send_request(manifest, vec![]);
        assert_eq!(ctl.dfu_engine.state(), DfuState::DfuManifestSync);
        assert!(ctl.dfu_engine.is_complete());
    }

    #[test]
    fn test_dfu_controller_dwc2_descriptor_response_readback() {
        use crate::dwc2;
        let mut ctl = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);
        ctl.host_attach();
        reg_w(&mut ctl, dwc2::GAHBCFG, 1);
        reg_w(&mut ctl, dwc2::DAINTMSK, 1);
        reg_w(&mut ctl, dwc2::GINTMSK, dwc2::GINTSTS_IEPINT as u64);
        reg_w(&mut ctl, dwc2::DIEPMSK, dwc2::EPINT_XFERCOMPL as u64);

        // GET_DESCRIPTOR (device, 18 bytes).
        let getdesc = UsbSetupPacket {
            bm_request_type: 0x80,
            b_request: 6,
            w_value: 0x0100,
            w_index: 0,
            w_length: 18,
        };
        ctl.host_send_request(getdesc, vec![]);
        assert_eq!(ctl.dwc2.tx_fifo_len(0), 18);
        let setup_status = reg_r(&mut ctl, dwc2::GRXSTSP);
        assert_eq!(
            setup_status & dwc2::RXSTS_PKTSTS_MASK as u64,
            dwc2::RXSTS_SETUP as u64
        );

        // Driver arms the EP0 IN transfer and the host consumes the response.
        reg_w(&mut ctl, dwc2::DIEPTSIZ_BASE, 18);
        reg_w(
            &mut ctl,
            dwc2::DIEPCTL_BASE,
            (dwc2::EPCTL_EPENA | dwc2::EPCTL_CNAK | (64 << 11)) as u64,
        );
        let data = ctl.host_read_in(0);
        assert_eq!(data.len(), 18);
        assert_eq!(&data[..4], &[18, 0x01, 0x00, 0x02]);
        assert_eq!(data[7], 64);
        assert_eq!(&data[8..10], &[0x5A, 0x05]);
        assert_eq!(&data[10..12], &[0x01, 0x12]);
        assert!(reg_r(&mut ctl, dwc2::DIEPINT_BASE) & dwc2::EPINT_XFERCOMPL as u64 != 0);
        assert!(reg_r(&mut ctl, dwc2::DAINT) & dwc2::daint_in(0) as u64 != 0);
        assert!(reg_r(&mut ctl, dwc2::GINTSTS) & dwc2::GINTSTS_IEPINT as u64 != 0);
        assert!(ctl.dwc2.has_unmasked_int());

        // W1C the completion: the interrupt line deasserts.
        reg_w(&mut ctl, dwc2::DIEPINT_BASE, dwc2::EPINT_XFERCOMPL as u64);
        reg_w(&mut ctl, dwc2::DAINT, dwc2::daint_in(0) as u64);
        assert!(!ctl.dwc2.has_unmasked_int());
    }

    #[test]
    fn test_dfu_controller_download_via_setup() {
        let mut ctl = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);
        let setup = UsbSetupPacket {
            bm_request_type: 0x21, // class, host-to-device
            b_request: 1,          // DNLOAD
            w_value: 1,
            w_index: 0,
            w_length: 4,
        };
        ctl.dfu_engine.set_expected_size(4, FirmwareType::IBSS);
        ctl.queue_host_request(setup, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(ctl.get_dfu_firmware(), &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(ctl.get_dfu_type(), FirmwareType::IBSS);
        assert_eq!(ctl.ep0_state, Ep0State::DataIn);
        assert!(ctl.interrupt_pending);
    }

    #[test]
    fn test_dfu_controller_apple_set_image_type() {
        let mut ctl = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);
        let setup = UsbSetupPacket {
            bm_request_type: 0x40, // vendor
            b_request: 0x81,       // APPLE_SET_IMAGE_TYPE
            w_value: 0x02,         // IBEC
            w_index: 0,
            w_length: 0,
        };
        ctl.queue_host_request(setup, vec![]);
        assert_eq!(ctl.image_type, Some(AppleImageType::IBEC));
        assert!(ctl.apple_mode);
    }

    #[test]
    fn test_raise_dma_event_emits_single_dma_request_for_usb_stream() {
        let mut ctl = UsbDeviceController::new(1, 0x20a800000, 0x40);
        assert!(ctl.tick(1).is_none());
        ctl.raise_dma_event();
        assert!(matches!(
            ctl.tick(1),
            Some(DeviceEvent::DmaRequest {
                channel: USB_DART_SID
            })
        ));
        // The event is single-shot: the next tick returns to normal behavior.
        assert!(ctl.tick(1).is_none());
        // Reset clears the pending flag.
        ctl.raise_dma_event();
        ctl.reset();
        assert!(ctl.tick(1).is_none());
    }
}

/// Synchronous client for the explicitly simulated class-firmware fixture.
/// It does not step a CPU and cannot establish SecureROM DFU readiness.
pub struct UsbHostDfuClient {
    device: UsbDeviceController,
    attached: bool,
    enumerated: bool,
}

impl UsbHostDfuClient {
    /// Create a new host DFU client attached to the given device controller.
    pub fn new(device: UsbDeviceController) -> Self {
        Self {
            device,
            attached: false,
            enumerated: false,
        }
    }

    /// Perform the full DFU enumeration sequence: attach, bus reset,
    /// enumeration done, and verify the device responds to GET_DESCRIPTOR.
    pub fn enumerate(&mut self) -> Result<UsbEnumerationResult, UsbHostDfuError> {
        if !self.device.emulate_class_firmware {
            return Err(UsbHostDfuError::GuestExecutionRequired);
        }
        // Step 1: Host attaches the device
        self.device.host_attach();
        self.attached = true;

        // Step 2: Host issues bus reset to start enumeration
        self.device.host_bus_reset();

        // Step 3: Host finishes enumeration at Full Speed
        self.device.host_enum_done(crate::dwc2::ENUMSPD_FS);

        // Step 4: Verify device responds to GET_DESCRIPTOR (device)
        let device_desc = self.get_descriptor(0x01, 0, 18)?;
        if device_desc.len() < 18 {
            return Err(UsbHostDfuError::DescriptorTooShort);
        }
        let bcd_device = u16::from_le_bytes([device_desc[12], device_desc[13]]);
        let id_vendor = u16::from_le_bytes([device_desc[8], device_desc[9]]);
        let id_product = u16::from_le_bytes([device_desc[10], device_desc[11]]);

        // Step 5: Verify device responds to GET_DESCRIPTOR (config)
        let config_desc = self.get_descriptor(0x02, 0, 27)?;
        if config_desc.is_empty() {
            return Err(UsbHostDfuError::ConfigDescriptorMissing);
        }

        self.enumerated = true;
        Ok(UsbEnumerationResult {
            device_descriptor: device_desc,
            configuration_descriptor: config_desc,
            bcd_device,
            id_vendor,
            id_product,
        })
    }

    /// Send a GET_DESCRIPTOR request and return the response.
    fn get_descriptor(
        &mut self,
        dtype: u8,
        dindex: u8,
        w_length: u16,
    ) -> Result<Vec<u8>, UsbHostDfuError> {
        let setup = UsbSetupPacket {
            bm_request_type: 0x80, // device-to-host, standard, device
            b_request: 0x06,       // GET_DESCRIPTOR
            w_value: (dtype as u16) << 8 | (dindex as u16),
            w_index: 0,
            w_length,
        };
        self.device.host_send_request(setup, vec![]);

        // Arm the EP0 IN transfer so host_read_in can read the response.
        // This mirrors the existing test pattern: write DIEPTSIZ and DIEPCTL
        // to arm the transfer, then read from the TX FIFO.
        use crate::dwc2;
        self.device
            .dwc2
            .write(dwc2::DIEPTSIZ_BASE, 4, w_length as u64);
        self.device.dwc2.write(
            dwc2::DIEPCTL_BASE,
            4,
            (dwc2::EPCTL_EPENA | dwc2::EPCTL_CNAK | (64 << 11)) as u64,
        );

        // Read the response from EP0 IN FIFO
        let response = self.device.host_read_in(0);
        if response.is_empty() && dtype != 0x03 {
            // String descriptor 0 is optional; others should not be empty
            return Err(UsbHostDfuError::DescriptorReadFailed);
        }
        Ok(response)
    }

    /// Set the DFU configuration via SET_CONFIGURATION.
    pub fn set_configuration(&mut self, config: u8) -> Result<(), UsbHostDfuError> {
        let setup = UsbSetupPacket {
            bm_request_type: 0x00, // host-to-device, standard, device
            b_request: 0x09,       // SET_CONFIGURATION
            w_value: config as u16,
            w_index: 0,
            w_length: 0,
        };
        self.device.host_send_request(setup, vec![]);
        Ok(())
    }

    /// Poll GET_STATUS until the device reports a stable DFU state.
    /// Returns the status and state after polling.
    pub fn get_dfu_status(&mut self) -> Result<(DfuStatus, DfuState), UsbHostDfuError> {
        let setup = UsbSetupPacket {
            bm_request_type: 0xA0, // device-to-host, class, interface
            b_request: 0x03,       // DFU_GETSTATUS
            w_value: 0,
            w_index: 0,
            w_length: 6,
        };
        self.device.host_send_request(setup, vec![]);

        // Arm the EP0 IN transfer so host_read_in can read the response.
        use crate::dwc2;
        self.device.dwc2.write(dwc2::DIEPTSIZ_BASE, 4, 6);
        self.device.dwc2.write(
            dwc2::DIEPCTL_BASE,
            4,
            (dwc2::EPCTL_EPENA | dwc2::EPCTL_CNAK | (64 << 11)) as u64,
        );

        let status_resp = self.device.host_read_in(0);
        if status_resp.len() < 6 {
            return Err(UsbHostDfuError::StatusReadFailed);
        }

        let status = match status_resp[0] {
            0 => DfuStatus::Ok,
            1 => DfuStatus::ErrTarget,
            2 => DfuStatus::ErrFile,
            3 => DfuStatus::ErrWrite,
            4 => DfuStatus::ErrErase,
            5 => DfuStatus::ErrCheckErase,
            6 => DfuStatus::ErrProg,
            7 => DfuStatus::ErrVerify,
            8 => DfuStatus::ErrAddress,
            9 => DfuStatus::ErrNotDone,
            10 => DfuStatus::ErrFirmware,
            11 => DfuStatus::ErrVendor,
            12 => DfuStatus::ErrUsbR,
            13 => DfuStatus::ErrPor,
            14 => DfuStatus::ErrUnknown,
            15 => DfuStatus::ErrStalledPkt,
            _ => return Err(UsbHostDfuError::InvalidStatus),
        };

        let state = match status_resp[4] {
            0 => DfuState::AppIdle,
            1 => DfuState::AppDetach,
            2 => DfuState::DfuIdle,
            3 => DfuState::DfuDownloadSync,
            4 => DfuState::DfuDownloadBusy,
            5 => DfuState::DfuDownloadIdle,
            6 => DfuState::DfuManifestSync,
            7 => DfuState::DfuManifest,
            8 => DfuState::DfuManifestWaitReset,
            9 => DfuState::DfuUploadIdle,
            10 => DfuState::DfuError,
            _ => return Err(UsbHostDfuError::InvalidState),
        };

        Ok((status, state))
    }

    /// Perform a DNLOAD request with the given firmware data.
    pub fn dfu_dnload(&mut self, block_num: u16, data: &[u8]) -> Result<(), UsbHostDfuError> {
        let setup = UsbSetupPacket {
            bm_request_type: 0x21, // host-to-device, class, interface
            b_request: 0x01,       // DNLOAD
            w_value: block_num,
            w_index: 0,
            w_length: data.len() as u16,
        };
        self.device.host_send_request(setup, data.to_vec());
        Ok(())
    }

    /// Perform a DFU_DETACH request.
    pub fn dfu_detach(&mut self, timeout: u16) -> Result<(), UsbHostDfuError> {
        let setup = UsbSetupPacket {
            bm_request_type: 0x21, // host-to-device, class, interface
            b_request: 0x00,       // DETACH
            w_value: timeout,
            w_index: 0,
            w_length: 0,
        };
        self.device.host_send_request(setup, vec![]);
        Ok(())
    }

    /// Check if the device is attached and enumerated.
    pub fn is_ready(&self) -> bool {
        self.attached && self.enumerated
    }
}

/// Result of a successful USB enumeration.
pub struct UsbEnumerationResult {
    pub device_descriptor: Vec<u8>,
    pub configuration_descriptor: Vec<u8>,
    pub bcd_device: u16,
    pub id_vendor: u16,
    pub id_product: u16,
}

/// Errors that can occur during host DFU operations.
#[derive(Debug)]
pub enum UsbHostDfuError {
    GuestExecutionRequired,
    DescriptorReadFailed,
    DescriptorTooShort,
    ConfigDescriptorMissing,
    StatusReadFailed,
    InvalidStatus,
    InvalidState,
}

impl std::fmt::Display for UsbHostDfuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsbHostDfuError::GuestExecutionRequired => write!(f, "USB responses require guest execution; the synchronous client is a simulated-firmware fixture"),
            UsbHostDfuError::DescriptorReadFailed => write!(f, "descriptor read failed"),
            UsbHostDfuError::DescriptorTooShort => write!(f, "descriptor too short"),
            UsbHostDfuError::ConfigDescriptorMissing => {
                write!(f, "configuration descriptor missing")
            }
            UsbHostDfuError::StatusReadFailed => write!(f, "status read failed"),
            UsbHostDfuError::InvalidStatus => write!(f, "invalid DFU status"),
            UsbHostDfuError::InvalidState => write!(f, "invalid DFU state"),
        }
    }
}

impl std::error::Error for UsbHostDfuError {}

#[cfg(test)]
mod host_dfu_tests {
    use super::*;

    fn create_host_client() -> UsbHostDfuClient {
        let device = UsbDeviceController::new_simulated_firmware(1, 0x20a800000, 0x40);
        UsbHostDfuClient::new(device)
    }

    #[test]
    fn host_client_enumerates_device() {
        let mut client = create_host_client();
        let result = client.enumerate();
        assert!(
            result.is_ok(),
            "enumerate should succeed: {:?}",
            result.err()
        );
        let info = result.unwrap();
        assert!(!info.device_descriptor.is_empty());
        assert!(!info.configuration_descriptor.is_empty());
    }

    #[test]
    fn host_client_is_ready_after_enumeration() {
        let mut client = create_host_client();
        assert!(!client.is_ready());
        client.enumerate().unwrap();
        assert!(client.is_ready());
    }

    #[test]
    fn host_client_get_dfu_status() {
        let mut client = create_host_client();
        client.enumerate().unwrap();
        let result = client.get_dfu_status();
        assert!(result.is_ok());
        let (status, state) = result.unwrap();
        assert_eq!(status, DfuStatus::Ok);
        assert!(matches!(
            state,
            DfuState::DfuIdle | DfuState::DfuDownloadSync
        ));
    }

    #[test]
    fn host_client_dfu_dnload() {
        let mut client = create_host_client();
        client.enumerate().unwrap();
        let result = client.dfu_dnload(1, &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(result.is_ok());
    }

    #[test]
    fn host_client_dfu_detach() {
        let mut client = create_host_client();
        client.enumerate().unwrap();
        let result = client.dfu_detach(0);
        assert!(result.is_ok());
    }

    #[test]
    fn host_client_full_dfu_sequence() {
        let mut client = create_host_client();

        // Step 1: Enumerate
        let info = client.enumerate().unwrap();
        assert!(info.device_descriptor.len() >= 18);

        // Step 2: Set configuration
        client.set_configuration(1).unwrap();

        // Step 3: Get initial DFU status
        let (status, _state) = client.get_dfu_status().unwrap();
        assert_eq!(status, DfuStatus::Ok);

        // Step 4: DNLOAD firmware block
        client.dfu_dnload(1, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

        // Step 5: Check status again
        let (status, _state) = client.get_dfu_status().unwrap();
        assert_eq!(status, DfuStatus::Ok);
    }
}
