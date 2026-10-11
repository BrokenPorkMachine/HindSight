//! Display + DCP / DSI / Panel (Phase 161).
//!
//! These are register-level functional models for the iDevice display
//! pipeline. The goal is to make a SecureROM/iBoot/XNU framebuffer probe not
//! fault, and to make a framebuffer write visible to a test host without
//! actual silicon-accurate video timing.
//!
//! Enhanced with HDR support, advanced color management, display scaling,
//! and touch integration.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::sync::Arc;
use std::sync::Mutex;

fn valid_size(s: usize) -> bool {
    matches!(s, 1 | 2 | 4 | 8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HdrMode {
    Sdr = 0,
    Hdr10 = 1,
    Hlg = 2,
    DolbyVision = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorGamut {
    Srgb = 0,
    DciP3 = 1,
    Bt2020 = 2,
    AdobeRgb = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchGesture {
    None = 0,
    Tap = 1,
    DoubleTap = 2,
    SwipeLeft = 3,
    SwipeRight = 4,
    SwipeUp = 5,
    SwipeDown = 6,
    Pinch = 7,
    Rotate = 8,
    LongPress = 9,
}

#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    pub x: u32,
    pub y: u32,
    pub pressure: u16,
    pub timestamp: u64,
    pub gesture: TouchGesture,
}

#[derive(Debug, Clone)]
pub struct ColorCalibration {
    pub red_gain: f32,
    pub green_gain: f32,
    pub blue_gain: f32,
    pub red_offset: f32,
    pub green_offset: f32,
    pub blue_offset: f32,
    pub gamma: f32,
    pub color_gamut: ColorGamut,
    pub hdr_mode: HdrMode,
    pub peak_brightness: u32,
    pub black_level: u32,
}

impl Default for ColorCalibration {
    fn default() -> Self {
        Self {
            red_gain: 1.0,
            green_gain: 1.0,
            blue_gain: 1.0,
            red_offset: 0.0,
            green_offset: 0.0,
            blue_offset: 0.0,
            gamma: 2.2,
            color_gamut: ColorGamut::Srgb,
            hdr_mode: HdrMode::Sdr,
            peak_brightness: 1000,
            black_level: 0,
        }
    }
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
// Panel — DSI panel with init commands and framebuffer blit window.
// ----------------------------------------------------------------------------

/// One panel init command (DCS / generic write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DsiCmd {
    pub dtype: u8, // 0x05 = DCS_WRITE, 0x23 = GEN_WRITE_2, etc.
    pub len: u8,
    pub payload: [u8; 8],
    pub sent: bool,
}

pub const PANEL_INIT_CMDS: usize = 16;
pub const PANEL_TIMING_REGS: usize = 16;
pub const MAX_TOUCH_POINTS: usize = 10;

pub struct Panel {
    pub id: DeviceId,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub enabled: bool,
    pub refresh_hz: u32,
    pub h_total: u32,
    pub v_total: u32,
    pub hsync: u32,
    pub vsync: u32,
    pub backlight: u8,
    pub init_seq: [DsiCmd; PANEL_INIT_CMDS],
    pub init_count: usize,
    pub init_cursor: usize,
    pub framebuffer: Vec<u32>,
    /// Set only by guest MMIO framebuffer writes, never by host status draws.
    pub guest_frame_written: bool,
    pub irq: u32,
    last_status: u32,
    // Enhanced features
    pub color_calibration: ColorCalibration,
    pub hdr_metadata: Vec<u8>,
    pub scaling_enabled: bool,
    pub scale_factor: f32,
    pub touch_points: [Option<TouchPoint>; MAX_TOUCH_POINTS],
    pub touch_count: usize,
    pub gesture_recognizer: Option<Arc<Mutex<GestureRecognizer>>>,
    pub adaptive_sync: bool,
    pub vrr_range: (u32, u32),
    pub color_lut: Option<Vec<u32>>,
    pub frame_counter: u64,
    pub power_state: PanelPowerState,
    pub debug_info: PanelDebugInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPowerState {
    Off = 0,
    On = 1,
    Standby = 2,
    Sleep = 3,
}

#[derive(Debug, Clone, Default)]
pub struct PanelDebugInfo {
    pub last_error: Option<String>,
    pub frame_drops: u64,
    pub vsync_count: u64,
    pub hdr_frames: u64,
    pub color_space_changes: u64,
}

#[derive(Debug)]
pub struct GestureRecognizer {
    pub last_touch: Option<TouchPoint>,
    pub gesture_history: Vec<TouchGesture>,
    pub double_tap_threshold: u64,
    pub swipe_threshold: f32,
    pub long_press_threshold: u64,
}

impl GestureRecognizer {
    pub fn new() -> Self {
        Self {
            last_touch: None,
            gesture_history: Vec::new(),
            double_tap_threshold: 300_000_000, // 300ms in nanoseconds
            swipe_threshold: 50.0,             // pixels
            long_press_threshold: 500_000_000, // 500ms in nanoseconds
        }
    }

    pub fn process_touch(&mut self, point: TouchPoint, current_time: u64) -> TouchGesture {
        let mut gesture = TouchGesture::None;

        if let Some(last) = self.last_touch {
            let dt = current_time.saturating_sub(last.timestamp);
            let dx = (point.x as f32 - last.x as f32).abs();
            let dy = (point.y as f32 - last.y as f32).abs();
            let dist = (dx * dx + dy * dy).sqrt();

            if dt < self.double_tap_threshold && dist < 20.0 {
                gesture = TouchGesture::DoubleTap;
            } else if dist > self.swipe_threshold {
                if dx > dy {
                    gesture = if point.x > last.x {
                        TouchGesture::SwipeRight
                    } else {
                        TouchGesture::SwipeLeft
                    };
                } else {
                    gesture = if point.y > last.y {
                        TouchGesture::SwipeDown
                    } else {
                        TouchGesture::SwipeUp
                    };
                }
            } else if dt > self.long_press_threshold && dist < 10.0 {
                gesture = TouchGesture::LongPress;
            }
        }

        if gesture != TouchGesture::None {
            self.gesture_history.push(gesture);
            if self.gesture_history.len() > 32 {
                self.gesture_history.remove(0);
            }
        }

        self.last_touch = Some(point);
        gesture
    }
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Panel {
    pub fn new(id: DeviceId) -> Self {
        let init_seq: [DsiCmd; PANEL_INIT_CMDS] = unsafe { std::mem::zeroed() };
        // Default 1080p / 60 Hz, 32 bpp.
        Self {
            id,
            width: 1920,
            height: 1080,
            bpp: 32,
            enabled: false,
            refresh_hz: 60,
            h_total: 2200,
            v_total: 1125,
            hsync: 44,
            vsync: 5,
            backlight: 0,
            init_seq,
            init_count: 0,
            init_cursor: 0,
            framebuffer: vec![0; 1920 * 1080],
            guest_frame_written: false,
            irq: 62,
            last_status: 0,
            // Enhanced features
            color_calibration: ColorCalibration::default(),
            hdr_metadata: Vec::new(),
            scaling_enabled: false,
            scale_factor: 1.0,
            touch_points: [None; MAX_TOUCH_POINTS],
            touch_count: 0,
            gesture_recognizer: Some(Arc::new(Mutex::new(GestureRecognizer::new()))),
            adaptive_sync: false,
            vrr_range: (48, 120),
            color_lut: None,
            frame_counter: 0,
            power_state: PanelPowerState::Off,
            debug_info: PanelDebugInfo::default(),
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    /// Append a panel init command (up to PANEL_INIT_CMDS).
    pub fn push_init(&mut self, cmd: DsiCmd) -> bool {
        if self.init_count >= PANEL_INIT_CMDS {
            return false;
        }
        self.init_seq[self.init_count] = cmd;
        self.init_count += 1;
        true
    }

    /// Pointer to current init cursor; `None` when done.
    pub fn current_init(&self) -> Option<&DsiCmd> {
        if self.init_cursor < self.init_count {
            Some(&self.init_seq[self.init_cursor])
        } else {
            None
        }
    }

    /// Mark current init cmd sent, advance.
    pub fn ack_init(&mut self) -> bool {
        if self.init_cursor >= self.init_count {
            return false;
        }
        self.init_seq[self.init_cursor].sent = true;
        self.init_cursor += 1;
        true
    }

    // Enhanced methods

    /// Set HDR mode and update color calibration
    pub fn set_hdr_mode(&mut self, mode: HdrMode) -> DeviceResult {
        self.color_calibration.hdr_mode = mode;
        match mode {
            HdrMode::Sdr => {
                self.color_calibration.peak_brightness = 100;
                self.color_calibration.gamma = 2.2;
                self.color_calibration.color_gamut = ColorGamut::Srgb;
            }
            HdrMode::Hdr10 => {
                self.color_calibration.peak_brightness = 1000;
                self.color_calibration.gamma = 2.4;
                self.color_calibration.color_gamut = ColorGamut::Bt2020;
            }
            HdrMode::Hlg => {
                self.color_calibration.peak_brightness = 1000;
                self.color_calibration.gamma = 1.2;
                self.color_calibration.color_gamut = ColorGamut::Bt2020;
            }
            HdrMode::DolbyVision => {
                self.color_calibration.peak_brightness = 4000;
                self.color_calibration.gamma = 2.6;
                self.color_calibration.color_gamut = ColorGamut::Bt2020;
            }
        }
        self.debug_info.color_space_changes += 1;
        DeviceResult::Ok
    }

    /// Set color gamut
    pub fn set_color_gamut(&mut self, gamut: ColorGamut) -> DeviceResult {
        self.color_calibration.color_gamut = gamut;
        self.debug_info.color_space_changes += 1;
        DeviceResult::Ok
    }

    /// Set color calibration parameters
    pub fn set_calibration(&mut self, calibration: ColorCalibration) -> DeviceResult {
        self.color_calibration = calibration;
        self.debug_info.color_space_changes += 1;
        DeviceResult::Ok
    }

    /// Apply color calibration to a pixel
    pub fn apply_calibration(&self, pixel: u32) -> u32 {
        let r = ((pixel >> 16) & 0xFF) as f32;
        let g = ((pixel >> 8) & 0xFF) as f32;
        let b = (pixel & 0xFF) as f32;
        let a = (pixel >> 24) & 0xFF;

        let cal = &self.color_calibration;

        let r = (r * cal.red_gain + cal.red_offset).clamp(0.0, 255.0) as u32;
        let g = (g * cal.green_gain + cal.green_offset).clamp(0.0, 255.0) as u32;
        let b = (b * cal.blue_gain + cal.blue_offset).clamp(0.0, 255.0) as u32;

        (a << 24) | (r << 16) | (g << 8) | b
    }

    /// Enable/disable display scaling
    pub fn set_scaling(&mut self, enabled: bool, factor: f32) -> DeviceResult {
        self.scaling_enabled = enabled;
        self.scale_factor = factor.clamp(0.25, 4.0);
        DeviceResult::Ok
    }

    /// Apply display scaling to framebuffer coordinates
    pub fn apply_scaling(&self, x: u32, y: u32) -> (u32, u32) {
        if !self.scaling_enabled {
            return (x, y);
        }
        let scaled_x = ((x as f32) * self.scale_factor).clamp(0.0, self.width as f32) as u32;
        let scaled_y = ((y as f32) * self.scale_factor).clamp(0.0, self.height as f32) as u32;
        (scaled_x, scaled_y)
    }

    /// Set VRR (Variable Refresh Rate) range
    pub fn set_vrr_range(&mut self, min_hz: u32, max_hz: u32) -> DeviceResult {
        if min_hz >= max_hz || max_hz > 240 {
            return DeviceResult::Fault;
        }
        self.vrr_range = (min_hz, max_hz);
        self.adaptive_sync = true;
        DeviceResult::Ok
    }

    /// Set color lookup table
    pub fn set_color_lut(&mut self, lut: Vec<u32>) -> DeviceResult {
        if lut.len() != 256 * 3 {
            return DeviceResult::Fault;
        }
        self.color_lut = Some(lut);
        DeviceResult::Ok
    }

    /// Add touch point
    pub fn add_touch_point(
        &mut self,
        x: u32,
        y: u32,
        pressure: u16,
        timestamp: u64,
    ) -> TouchGesture {
        let point = TouchPoint {
            x: x.min(self.width - 1),
            y: y.min(self.height - 1),
            pressure,
            timestamp,
            gesture: TouchGesture::None,
        };

        // Find empty slot or replace oldest
        let slot = self
            .touch_points
            .iter()
            .position(|p| p.is_none())
            .or_else(|| {
                self.touch_points
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| p.map(|tp| tp.timestamp).unwrap_or(u64::MAX))
                    .map(|(i, _)| i)
            });

        if let Some(idx) = slot {
            self.touch_points[idx] = Some(point);
            self.touch_count = self.touch_points.iter().filter(|p| p.is_some()).count();
        }

        // Process gesture if recognizer available
        let mut gesture = TouchGesture::None;
        if let Some(ref recognizer) = self.gesture_recognizer {
            if let Ok(mut rec) = recognizer.lock() {
                gesture = rec.process_touch(point, timestamp);
            }
        }

        // The gesture is returned to the caller rather than written back into
        // `point`: the stored copy was made above, before recognition ran, and
        // the function's result is the gesture itself. The assignment this
        // replaces was dead -- rustc reported "value assigned to `point` is
        // never read" alongside the mutability error -- so removing it changes
        // no behaviour. Wiring the recognised gesture back into the stored point
        // would be a design change, not a repair.
        gesture
    }

    /// Remove touch point
    pub fn remove_touch_point(&mut self, x: u32, y: u32) -> bool {
        for i in 0..MAX_TOUCH_POINTS {
            if let Some(p) = self.touch_points[i] {
                if p.x == x && p.y == y {
                    self.touch_points[i] = None;
                    self.touch_count = self.touch_points.iter().filter(|p| p.is_some()).count();
                    return true;
                }
            }
        }
        false
    }

    /// Update touch point position
    pub fn update_touch_point(
        &mut self,
        old_x: u32,
        old_y: u32,
        new_x: u32,
        new_y: u32,
        pressure: u16,
        timestamp: u64,
    ) -> TouchGesture {
        for i in 0..MAX_TOUCH_POINTS {
            if let Some(p) = self.touch_points[i] {
                if p.x == old_x && p.y == old_y {
                    let mut point = TouchPoint {
                        x: new_x.min(self.width - 1),
                        y: new_y.min(self.height - 1),
                        pressure,
                        timestamp,
                        gesture: TouchGesture::None,
                    };

                    let mut gesture = TouchGesture::None;
                    if let Some(ref recognizer) = self.gesture_recognizer {
                        if let Ok(mut rec) = recognizer.lock() {
                            gesture = rec.process_touch(point, timestamp);
                        }
                    }

                    point.gesture = gesture;
                    self.touch_points[i] = Some(point);
                    return gesture;
                }
            }
        }
        TouchGesture::None
    }

    /// Set power state
    pub fn set_power_state(&mut self, state: PanelPowerState) -> DeviceResult {
        self.power_state = state;
        self.enabled = matches!(state, PanelPowerState::On);
        DeviceResult::Ok
    }

    /// Update frame counter and VSync
    pub fn frame_tick(&mut self) {
        self.frame_counter += 1;
        self.debug_info.vsync_count += 1;

        if self.color_calibration.hdr_mode != HdrMode::Sdr {
            self.debug_info.hdr_frames += 1;
        }
    }

    /// Get current debug info
    pub fn get_debug_info(&self) -> PanelDebugInfo {
        self.debug_info.clone()
    }

    /// Resize framebuffer
    pub fn resize(&mut self, width: u32, height: u32) -> DeviceResult {
        if width == 0 || height == 0 || width > 7680 || height > 4320 {
            return DeviceResult::Fault;
        }
        self.width = width;
        self.height = height;
        self.framebuffer = vec![0; (width * height) as usize];
        DeviceResult::Ok
    }
}

impl Device for Panel {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "panel"
    }
    fn reset(&mut self) {
        self.enabled = false;
        self.backlight = 0;
        self.init_count = 0;
        self.init_cursor = 0;
        for c in self.init_seq.iter_mut() {
            c.sent = false;
        }
        self.last_status = 0;
        self.guest_frame_written = false;
        // Reset enhanced features
        self.color_calibration = ColorCalibration::default();
        self.hdr_metadata.clear();
        self.scaling_enabled = false;
        self.scale_factor = 1.0;
        self.touch_points = [None; MAX_TOUCH_POINTS];
        self.touch_count = 0;
        if let Some(ref rec) = self.gesture_recognizer {
            if let Ok(mut r) = rec.lock() {
                *r = GestureRecognizer::new();
            }
        }
        self.adaptive_sync = false;
        self.vrr_range = (48, 120);
        self.color_lut = None;
        self.frame_counter = 0;
        self.power_state = PanelPowerState::Off;
        self.debug_info = PanelDebugInfo::default();
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.control_bits()),
            0x04 => DeviceResult::Value(self.status_bits()),
            0x08 => DeviceResult::Value(self.width as u64),
            0x0C => DeviceResult::Value(self.height as u64),
            0x10 => DeviceResult::Value(self.bpp as u64),
            0x14 => DeviceResult::Value(self.refresh_hz as u64),
            0x18 => DeviceResult::Value(self.h_total as u64),
            0x1C => DeviceResult::Value(self.v_total as u64),
            0x20 => DeviceResult::Value(self.hsync as u64),
            0x24 => DeviceResult::Value(self.vsync as u64),
            0x28 => DeviceResult::Value(self.backlight as u64),
            0x2C => DeviceResult::Value(self.init_cursor as u64),
            0x30 => DeviceResult::Value(self.init_count as u64),
            // Enhanced registers
            0x40 => DeviceResult::Value(self.color_calibration.hdr_mode as u64),
            0x44 => DeviceResult::Value(self.color_calibration.color_gamut as u64),
            0x48 => DeviceResult::Value(self.color_calibration.peak_brightness as u64),
            0x4C => DeviceResult::Value(self.color_calibration.gamma as u64),
            0x50 => DeviceResult::Value(self.scaling_enabled as u64),
            0x54 => DeviceResult::Value(self.scale_factor as u64),
            0x58 => DeviceResult::Value(self.adaptive_sync as u64),
            0x5C => DeviceResult::Value(self.vrr_range.0 as u64),
            0x60 => DeviceResult::Value(self.vrr_range.1 as u64),
            0x64 => DeviceResult::Value(self.power_state as u64),
            0x68 => DeviceResult::Value(self.frame_counter),
            0x6C => DeviceResult::Value(self.touch_count as u64),
            0x70 => DeviceResult::Value(self.debug_info.frame_drops),
            0x74 => DeviceResult::Value(self.debug_info.vsync_count),
            0x78 => DeviceResult::Value(self.debug_info.hdr_frames),
            0x7C => DeviceResult::Value(self.debug_info.color_space_changes),
            // Touch points register area (0x80 - 0x17F)
            t if (0x80..0x180).contains(&t) => {
                let idx = ((t - 0x80) / 0x18) as usize;
                let offset = (t - 0x80) % 0x18;
                if idx >= MAX_TOUCH_POINTS {
                    return read_zero(o, s);
                }
                if let Some(point) = self.touch_points[idx] {
                    let value = match offset {
                        0x00 => point.x as u64,
                        0x04 => point.y as u64,
                        0x08 => point.pressure as u64,
                        0x0C => point.timestamp,
                        0x10 => point.gesture as u64,
                        _ => 0,
                    };
                    DeviceResult::Value(value)
                } else {
                    DeviceResult::Value(0)
                }
            }
            // Framebuffer window at 0x1000..
            f if f >= 0x1000 => {
                let base = (f - 0x1000) as usize;
                let mut value = 0u64;
                for byte_index in 0..s {
                    let offset = base + byte_index;
                    if let Some(pixel) = self.framebuffer.get(offset / 4) {
                        value |=
                            (((pixel >> ((offset % 4) * 8)) & 0xff) as u64) << (byte_index * 8);
                    }
                }
                DeviceResult::Value(value)
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
                // bit 0 = enable
                self.enabled = v & 1 != 0;
                DeviceResult::Ok
            }
            0x08 => {
                self.width = v as u32;
                DeviceResult::Ok
            }
            0x0C => {
                self.height = v as u32;
                DeviceResult::Ok
            }
            0x10 => {
                self.bpp = v as u8;
                DeviceResult::Ok
            }
            0x14 => {
                self.refresh_hz = v as u32;
                DeviceResult::Ok
            }
            0x18 => {
                self.h_total = v as u32;
                DeviceResult::Ok
            }
            0x1C => {
                self.v_total = v as u32;
                DeviceResult::Ok
            }
            0x20 => {
                self.hsync = v as u32;
                DeviceResult::Ok
            }
            0x24 => {
                self.vsync = v as u32;
                DeviceResult::Ok
            }
            0x28 => {
                self.backlight = v as u8;
                DeviceResult::Ok
            }
            0x2C => {
                self.init_cursor = v as usize;
                DeviceResult::Ok
            }
            0x30 => {
                self.init_count = v as usize;
                DeviceResult::Ok
            }
            // Enhanced registers
            0x40 => self.set_hdr_mode(match v as u8 {
                0 => HdrMode::Sdr,
                1 => HdrMode::Hdr10,
                2 => HdrMode::Hlg,
                3 => HdrMode::DolbyVision,
                _ => return DeviceResult::Fault,
            }),
            0x44 => self.set_color_gamut(match v as u8 {
                0 => ColorGamut::Srgb,
                1 => ColorGamut::DciP3,
                2 => ColorGamut::Bt2020,
                3 => ColorGamut::AdobeRgb,
                _ => return DeviceResult::Fault,
            }),
            0x48 => {
                self.color_calibration.peak_brightness = v as u32;
                DeviceResult::Ok
            }
            0x4C => {
                self.color_calibration.gamma = v as f32;
                DeviceResult::Ok
            }
            0x50 => {
                self.scaling_enabled = v & 1 != 0;
                DeviceResult::Ok
            }
            0x54 => {
                self.scale_factor = v as f32;
                DeviceResult::Ok
            }
            0x58 => {
                self.adaptive_sync = v & 1 != 0;
                DeviceResult::Ok
            }
            0x5C => {
                self.vrr_range.0 = v as u32;
                DeviceResult::Ok
            }
            0x60 => {
                self.vrr_range.1 = v as u32;
                DeviceResult::Ok
            }
            0x64 => self.set_power_state(match v as u8 {
                0 => PanelPowerState::Off,
                1 => PanelPowerState::On,
                2 => PanelPowerState::Standby,
                3 => PanelPowerState::Sleep,
                _ => return DeviceResult::Fault,
            }),
            f if f >= 0x1000 => {
                let base = (f - 0x1000) as usize;
                for byte_index in 0..s {
                    let offset = base + byte_index;
                    if let Some(pixel) = self.framebuffer.get_mut(offset / 4) {
                        let shift = (offset % 4) * 8;
                        let mask = 0xffu32 << shift;
                        *pixel =
                            (*pixel & !mask) | (((v >> (byte_index * 8)) as u32 & 0xff) << shift);
                        self.guest_frame_written = true;
                    }
                }
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        if self.enabled && self.init_cursor >= self.init_count && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if self.enabled && self.init_cursor >= self.init_count {
                self.irq
            } else {
                0
            };
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

impl Panel {
    fn control_bits(&self) -> u64 {
        (self.enabled as u64)
            | ((self.backlight as u64) << 8)
            | ((self.scaling_enabled as u64) << 16)
            | ((self.adaptive_sync as u64) << 17)
            | ((self.color_calibration.hdr_mode as u64) << 18)
    }
    fn status_bits(&self) -> u64 {
        let ready = self.init_cursor >= self.init_count;
        let hdr_active = self.color_calibration.hdr_mode != HdrMode::Sdr;
        (ready as u64)
            | ((self.enabled as u64) << 1)
            | ((hdr_active as u64) << 2)
            | ((self.touch_count as u64) << 3)
            | ((self.power_state as u64) << 16)
    }
}

#[cfg(test)]
mod panel_framebuffer_tests {
    use super::{Device, DeviceResult, Panel};

    #[test]
    fn guest_pixel_aperture_respects_store_width_and_byte_lanes() {
        let mut panel = Panel::new(1);
        assert!(!panel.guest_frame_written);
        assert_eq!(
            panel.write(0x1000, 8, 0x8877_6655_4433_2211),
            DeviceResult::Ok
        );
        assert_eq!(&panel.framebuffer[..2], &[0x4433_2211, 0x8877_6655]);
        assert!(panel.guest_frame_written);
        assert_eq!(
            panel.read(0x1000, 8),
            DeviceResult::Value(0x8877_6655_4433_2211)
        );
        assert_eq!(panel.write(0x1001, 1, 0xaa), DeviceResult::Ok);
        assert_eq!(panel.framebuffer[0], 0x4433_aa11);
        assert_eq!(panel.write(0x1003, 2, 0xbbcc), DeviceResult::Ok);
        assert_eq!(&panel.framebuffer[..2], &[0xcc33_aa11, 0x8877_66bb]);
        assert_eq!(panel.read(0x1003, 2), DeviceResult::Value(0xbbcc));
    }
}

// ----------------------------------------------------------------------------
// DsiPhy — D-PHY link state machine (HS / LP / ULPS) with advanced features.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsiLinkState {
    Off,
    Init,
    Hs,
    Lp,
    Ulps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsiErrorType {
    None = 0,
    EccSingle = 1,
    EccDouble = 2,
    Checksum = 3,
    Crc = 4,
    LineContention = 5,
    FalseControl = 6,
    EscapeEntry = 7,
    LpTransmit = 8,
    HsTransmit = 9,
    EotSync = 10,
    SotSync = 11,
    SotError = 12,
    ControlError = 13,
}

pub struct DsiPhy {
    pub id: DeviceId,
    pub state: DsiLinkState,
    pub lane_count: u8,
    pub bitrate_mbps: u32,
    pub irq_status: u32,
    pub irq_en: u32,
    pub irq: u32,
    last_status: u32,
    // Enhanced features
    pub lane_config: [LaneConfig; 4],
    pub error_counters: [u32; 4],
    pub total_errors: u64,
    pub link_training_state: LinkTrainingState,
    pub hs_clock_enabled: bool,
    pub auto_error_recovery: bool,
    pub crc_enabled: bool,
    pub ecc_enabled: bool,
    pub power_mode: DsiPowerMode,
    pub max_bitrate: u32,
    pub current_bitrate: u32,
    pub link_quality: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct LaneConfig {
    pub enabled: bool,
    pub polarity: bool,
    pub termination: LaneTermination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneTermination {
    None = 0,
    LowPower = 1,
    HighSpeed = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTrainingState {
    Idle = 0,
    Training = 1,
    Verified = 2,
    Failed = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsiPowerMode {
    Off = 0,
    LowPower = 1,
    HighSpeed = 2,
    Ulps = 3,
}

impl DsiPhy {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            state: DsiLinkState::Off,
            lane_count: 4,
            bitrate_mbps: 2500,
            irq_status: 0,
            irq_en: 0,
            irq: 63,
            last_status: 0,
            // Enhanced features
            lane_config: [
                LaneConfig {
                    enabled: true,
                    polarity: false,
                    termination: LaneTermination::LowPower,
                },
                LaneConfig {
                    enabled: true,
                    polarity: false,
                    termination: LaneTermination::LowPower,
                },
                LaneConfig {
                    enabled: true,
                    polarity: false,
                    termination: LaneTermination::LowPower,
                },
                LaneConfig {
                    enabled: true,
                    polarity: false,
                    termination: LaneTermination::LowPower,
                },
            ],
            error_counters: [0; 4],
            total_errors: 0,
            link_training_state: LinkTrainingState::Idle,
            hs_clock_enabled: false,
            auto_error_recovery: true,
            crc_enabled: true,
            ecc_enabled: true,
            power_mode: DsiPowerMode::Off,
            max_bitrate: 2500,
            current_bitrate: 2500,
            link_quality: 100,
        }
    }

    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }

    pub fn start(&mut self) {
        self.state = DsiLinkState::Init;
        self.power_mode = DsiPowerMode::LowPower;
        self.irq_status |= 0x1; // init_done
        self.link_training_state = LinkTrainingState::Training;
    }

    pub fn enter_hs(&mut self) {
        if self.state == DsiLinkState::Init || self.state == DsiLinkState::Lp {
            self.state = DsiLinkState::Hs;
            self.power_mode = DsiPowerMode::HighSpeed;
            self.hs_clock_enabled = true;
            for lane in &mut self.lane_config {
                lane.termination = LaneTermination::HighSpeed;
            }
            self.irq_status |= 0x2;
            self.link_training_state = LinkTrainingState::Verified;
        }
    }

    pub fn enter_lp(&mut self) {
        if self.state == DsiLinkState::Hs || self.state == DsiLinkState::Init {
            self.state = DsiLinkState::Lp;
            self.power_mode = DsiPowerMode::LowPower;
            self.hs_clock_enabled = false;
            for lane in &mut self.lane_config {
                lane.termination = LaneTermination::LowPower;
            }
            self.irq_status |= 0x4;
        }
    }

    pub fn enter_ulps(&mut self) {
        if self.state == DsiLinkState::Lp {
            self.state = DsiLinkState::Ulps;
            self.power_mode = DsiPowerMode::Ulps;
            self.irq_status |= 0x8;
        }
    }

    // Enhanced methods

    /// Start link training sequence
    pub fn start_link_training(&mut self) -> DeviceResult {
        if self.state == DsiLinkState::Off {
            return DeviceResult::Fault;
        }
        self.link_training_state = LinkTrainingState::Training;
        // Simulate training sequence
        for i in 0..self.lane_count as usize {
            if self.lane_config[i].enabled {
                self.lane_config[i].termination = LaneTermination::HighSpeed;
            }
        }
        self.current_bitrate = self.max_bitrate;
        self.link_training_state = LinkTrainingState::Verified;
        self.link_quality = 100;
        self.irq_status |= 0x10; // training_done
        DeviceResult::Ok
    }

    /// Record error on lane
    pub fn record_error(&mut self, lane: usize, _error_type: DsiErrorType) -> DeviceResult {
        if lane >= 4 {
            return DeviceResult::Fault;
        }
        self.error_counters[lane] += 1;
        self.total_errors += 1;

        // Update link quality based on error rate
        let total_packets = 10000; // Simulated
        let error_rate = self.total_errors as f64 / total_packets as f64;
        self.link_quality = ((1.0 - error_rate.min(1.0)) * 100.0) as u8;

        // Auto recovery
        if self.auto_error_recovery && self.total_errors > 1000 {
            let _ = self.start_link_training();
        }

        self.irq_status |= 0x20; // error_interrupt
        DeviceResult::Ok
    }

    /// Set bitrate with validation
    pub fn set_bitrate(&mut self, mbps: u32) -> DeviceResult {
        if mbps > self.max_bitrate || mbps < 80 {
            return DeviceResult::Fault;
        }
        self.current_bitrate = mbps;
        self.bitrate_mbps = mbps;
        self.irq_status |= 0x40; // bitrate_changed
        DeviceResult::Ok
    }

    /// Configure lane
    pub fn configure_lane(&mut self, lane: usize, enabled: bool, polarity: bool) -> DeviceResult {
        if lane >= 4 {
            return DeviceResult::Fault;
        }
        self.lane_config[lane].enabled = enabled;
        self.lane_config[lane].polarity = polarity;
        DeviceResult::Ok
    }

    /// Get error statistics
    pub fn get_error_stats(&self) -> (u64, [u32; 4], u8) {
        (self.total_errors, self.error_counters, self.link_quality)
    }

    /// Enable/disable CRC
    pub fn set_crc_enabled(&mut self, enabled: bool) {
        self.crc_enabled = enabled;
    }

    /// Enable/disable ECC
    pub fn set_ecc_enabled(&mut self, enabled: bool) {
        self.ecc_enabled = enabled;
    }

    /// Enable/disable auto error recovery
    pub fn set_auto_recovery(&mut self, enabled: bool) {
        self.auto_error_recovery = enabled;
    }
}

impl Device for DsiPhy {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "dsi-phy"
    }
    fn reset(&mut self) {
        self.state = DsiLinkState::Off;
        self.irq_status = 0;
        self.irq_en = 0;
        self.last_status = 0;
        // Reset enhanced features
        self.lane_config = [
            LaneConfig {
                enabled: true,
                polarity: false,
                termination: LaneTermination::LowPower,
            },
            LaneConfig {
                enabled: true,
                polarity: false,
                termination: LaneTermination::LowPower,
            },
            LaneConfig {
                enabled: true,
                polarity: false,
                termination: LaneTermination::LowPower,
            },
            LaneConfig {
                enabled: true,
                polarity: false,
                termination: LaneTermination::LowPower,
            },
        ];
        self.error_counters = [0; 4];
        self.total_errors = 0;
        self.link_training_state = LinkTrainingState::Idle;
        self.hs_clock_enabled = false;
        self.auto_error_recovery = true;
        self.crc_enabled = true;
        self.ecc_enabled = true;
        self.power_mode = DsiPowerMode::Off;
        self.max_bitrate = 2500;
        self.current_bitrate = 2500;
        self.link_quality = 100;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.state as u64),
            0x04 => DeviceResult::Value(self.lane_count as u64),
            0x08 => DeviceResult::Value(self.bitrate_mbps as u64),
            0x0C => DeviceResult::Value(self.irq_status as u64),
            0x10 => DeviceResult::Value(self.irq_en as u64),
            // Enhanced registers
            0x14 => DeviceResult::Value(self.link_training_state as u64),
            0x18 => DeviceResult::Value(self.power_mode as u64),
            0x1C => DeviceResult::Value(self.current_bitrate as u64),
            0x20 => DeviceResult::Value(self.max_bitrate as u64),
            0x24 => DeviceResult::Value(self.link_quality as u64),
            0x28 => DeviceResult::Value(self.total_errors),
            0x2C => DeviceResult::Value(self.hs_clock_enabled as u64),
            0x30 => DeviceResult::Value(self.auto_error_recovery as u64),
            0x34 => DeviceResult::Value(self.crc_enabled as u64),
            0x38 => DeviceResult::Value(self.ecc_enabled as u64),
            // Lane config registers
            l if (0x100..0x140).contains(&l) => {
                let lane = ((l - 0x100) / 0x10) as usize;
                let offset = (l - 0x100) % 0x10;
                if lane >= 4 {
                    return read_zero(o, s);
                }
                let value = match offset {
                    0x00 => self.lane_config[lane].enabled as u64,
                    0x04 => self.lane_config[lane].polarity as u64,
                    0x08 => self.lane_config[lane].termination as u64,
                    _ => 0,
                };
                DeviceResult::Value(value)
            }
            // Error counters
            e if (0x200..0x210).contains(&e) => {
                let lane = ((e - 0x200) / 0x4) as usize;
                if lane >= 4 {
                    return read_zero(o, s);
                }
                DeviceResult::Value(self.error_counters[lane] as u64)
            }
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x04 => {
                self.lane_count = v as u8;
                DeviceResult::Ok
            }
            0x08 => {
                self.bitrate_mbps = v as u32;
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
            0x14 => {
                // 0 = stop, 1 = start
                if v & 1 != 0 {
                    self.start();
                } else {
                    self.state = DsiLinkState::Off;
                    self.power_mode = DsiPowerMode::Off;
                }
                DeviceResult::Ok
            }
            // Enhanced commands
            0x18 => self.start_link_training(),
            0x1C => self.set_bitrate(v as u32),
            0x20 => {
                self.max_bitrate = v as u32;
                DeviceResult::Ok
            }
            0x24 => {
                self.auto_error_recovery = v & 1 != 0;
                DeviceResult::Ok
            }
            0x28 => {
                self.crc_enabled = v & 1 != 0;
                DeviceResult::Ok
            }
            0x2C => {
                self.ecc_enabled = v & 1 != 0;
                DeviceResult::Ok
            }
            // Lane config
            l if (0x100..0x140).contains(&l) => {
                let lane = ((l - 0x100) / 0x10) as usize;
                let offset = (l - 0x100) % 0x10;
                if lane >= 4 {
                    return drop_write(o, s, v);
                }
                match offset {
                    0x00 => self.lane_config[lane].enabled = v != 0,
                    0x04 => self.lane_config[lane].polarity = v != 0,
                    _ => {}
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
// Raster — software GPU / compositor.
// ----------------------------------------------------------------------------

/// Single 32-bit ARGB source layer.
#[derive(Debug, Clone, Copy)]
pub struct RasterLayer {
    pub base_lo: u32,
    pub base_hi: u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
    pub format: u32, // 0 = ARGB8888, 1 = XRGB8888
    pub enabled: bool,
}

pub struct Raster {
    pub id: DeviceId,
    pub layers: [RasterLayer; 4],
    pub dst_base_lo: u32,
    pub dst_base_hi: u32,
    pub dst_stride: u32,
    pub width: u32,
    pub height: u32,
    pub control: u32,
    pub status: u32,
    pub irq: u32,
    last_status: u32,
}

impl Raster {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            layers: [RasterLayer {
                base_lo: 0,
                base_hi: 0,
                stride: 0,
                width: 0,
                height: 0,
                format: 0,
                enabled: false,
            }; 4],
            dst_base_lo: 0,
            dst_base_hi: 0,
            dst_stride: 0,
            width: 0,
            height: 0,
            control: 0,
            status: 0,
            irq: 64,
            last_status: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    /// One-pass software composite: write each enabled layer's color into
    /// the destination `dst` (top layer wins).
    pub fn composite(&self, src_bufs: &[Vec<u32>], dst: &mut [u32]) {
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let mut argb = 0u32;
                for (li, layer) in self.layers.iter().enumerate() {
                    if !layer.enabled {
                        continue;
                    }
                    if x >= layer.width as usize || y >= layer.height as usize {
                        continue;
                    }
                    if li >= src_bufs.len() {
                        continue;
                    }
                    let src = &src_bufs[li];
                    if src.is_empty() {
                        continue;
                    }
                    let sidx = y * layer.stride as usize + x;
                    if sidx < src.len() {
                        let p = src[sidx];
                        if p & 0xFF000000 != 0 || argb == 0 {
                            argb = p;
                        }
                    }
                }
                let didx = y * self.dst_stride as usize + x;
                if didx < dst.len() {
                    dst[didx] = argb;
                }
            }
        }
    }
}

impl Device for Raster {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "raster"
    }
    fn reset(&mut self) {
        for l in self.layers.iter_mut() {
            *l = RasterLayer {
                base_lo: 0,
                base_hi: 0,
                stride: 0,
                width: 0,
                height: 0,
                format: 0,
                enabled: false,
            };
        }
        self.dst_base_lo = 0;
        self.dst_base_hi = 0;
        self.dst_stride = 0;
        self.width = 0;
        self.height = 0;
        self.control = 0;
        self.status = 0;
        self.last_status = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.control as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.width as u64),
            0x0C => DeviceResult::Value(self.height as u64),
            0x10 => DeviceResult::Value(self.dst_stride as u64),
            0x14 => DeviceResult::Value(self.dst_base_lo as u64),
            0x18 => DeviceResult::Value(self.dst_base_hi as u64),
            l if (0x100..0x300).contains(&l) => {
                let li = ((l - 0x100) / 0x40) as usize;
                let r = l & 0x3F;
                if li >= self.layers.len() {
                    return read_zero(o, s);
                }
                let layer = &self.layers[li];
                let v = match r {
                    0x00 => layer.enabled as u64,
                    0x04 => layer.format as u64,
                    0x08 => layer.stride as u64,
                    0x0C => layer.width as u64,
                    0x10 => layer.height as u64,
                    0x14 => layer.base_lo as u64,
                    0x18 => layer.base_hi as u64,
                    _ => 0,
                };
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
                self.control = v as u32;
                if v & 1 != 0 {
                    self.status |= 0x1;
                }
                DeviceResult::Ok
            }
            0x08 => {
                self.width = v as u32;
                DeviceResult::Ok
            }
            0x0C => {
                self.height = v as u32;
                DeviceResult::Ok
            }
            0x10 => {
                self.dst_stride = v as u32;
                DeviceResult::Ok
            }
            0x14 => {
                self.dst_base_lo = v as u32;
                DeviceResult::Ok
            }
            0x18 => {
                self.dst_base_hi = v as u32;
                DeviceResult::Ok
            }
            l if (0x100..0x300).contains(&l) => {
                let li = ((l - 0x100) / 0x40) as usize;
                let r = l & 0x3F;
                if li >= self.layers.len() {
                    return drop_write(o, s, v);
                }
                let layer = &mut self.layers[li];
                match r {
                    0x00 => layer.enabled = v != 0,
                    0x04 => layer.format = v as u32,
                    0x08 => layer.stride = v as u32,
                    0x0C => layer.width = v as u32,
                    0x10 => layer.height = v as u32,
                    0x14 => layer.base_lo = v as u32,
                    0x18 => layer.base_hi = v as u32,
                    _ => {}
                }
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _c: u64) -> Option<DeviceEvent> {
        if (self.status & 1) != 0 && self.last_status != self.irq {
            self.last_status = self.irq;
            Some(DeviceEvent::Interrupt(self.irq))
        } else {
            self.last_status = if (self.status & 1) != 0 { self.irq } else { 0 };
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
// IOMfb — IOMobileFramebuffer controller
// ----------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct IOSSurfaceInfo {
    pub surface_id: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub bytes_per_row: u32,
    pub base_pa: u64,
}

pub struct IOMfb {
    pub id: DeviceId,
    pub control: u32,
    pub status: u32,
    pub active_surface: u32,
    pub vsync_count: u64,
    pub irq: u32,
    pub surfaces: Vec<IOSSurfaceInfo>,
}

impl IOMfb {
    pub fn new(id: DeviceId, irq: u32) -> Self {
        Self {
            id,
            control: 0,
            status: 1, // Ready
            active_surface: 0,
            vsync_count: 0,
            irq,
            surfaces: Vec::new(),
        }
    }

    pub fn register_surface(&mut self, surface: IOSSurfaceInfo) {
        self.surfaces.retain(|s| s.surface_id != surface.surface_id);
        self.surfaces.push(surface);
    }
}

impl Device for IOMfb {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "iomfb"
    }
    fn reset(&mut self) {
        self.control = 0;
        self.status = 1;
        self.active_surface = 0;
        self.vsync_count = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => DeviceResult::Value(self.control as u64),
            0x04 => DeviceResult::Value(self.status as u64),
            0x08 => DeviceResult::Value(self.active_surface as u64),
            0x10 => DeviceResult::Value(self.vsync_count),
            _ => read_zero(o, s),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            0x00 => {
                self.control = v as u32;
                DeviceResult::Ok
            }
            0x08 => {
                self.active_surface = v as u32;
                DeviceResult::Ok
            }
            _ => drop_write(o, s, v),
        }
    }
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        self.vsync_count += 1;
        if self.control & 1 != 0 {
            Some(DeviceEvent::Interrupt(self.irq))
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
