//! SGX GPU mailbox slice — Phase 190 item 1.
//!
//! The real d321ap DeviceTree declares `/arm-io/sgx` (compatible `gpu,t8020`)
//! at `(off=0x4000000 size=0x100000)` relative to the `0x200000000` arm-io
//! aperture, i.e. physical `0x204000000`, on IRQs 339-342/351/354; a second
//! window `(off=0x4d00000 size=0x20000)` lands at `0x204d00000`. No
//! execution/vertex unit is modeled (real SGX firmware/ring buffers are not
//! available); instead the slice provides the recovered window as a genuine
//! bounded mailbox producer/consumer, mirroring the ANE `kF0…kF7` pattern:
//!
//! - `MSG_SEND` enqueues a 32-bit command word into a 2-deep queue; a push
//!   into the full queue does **not** fabricate a slot, it records the
//!   backpressure bit in `MSG_STAT` (Phase 190 gate G2 requires the polled
//!   completion to be produced by a real drain, not a host override),
//! - a completion leaf `MSG_DONE` inverts **only** on a real engine tick that
//!   drains one queued word — never a fabricated readiness bit,
//! - `SEG_READ` returns a stable 64-bit identity of the recovered window
//!   (gate G1: IOGPU/IOGPUFamily early malloc+segment reads over MMIO must not
//!   fault, and the live read returns the same width a driver preconditions).
//!   The identity value is the measured base itself — a modeled registry value,
//!   not a silicon-measured status.
//!
//! Reproduction in `docs/PHASE190_PROGRESS.md`.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::VecDeque;

/// MSG_SEND (W): enqueue one 32-bit mailbox command word.
pub const SGX_MSG_SEND: u64 = 0x00;
/// MSG_STAT (R): bits 1:0 = occupancy, bit2 = stalled (backpressure).
pub const SGX_MSG_STAT: u64 = 0x04;
/// MSG_DEPTH (R): 2.
pub const SGX_MSG_DEPTH: u64 = 0x08;
/// MSG_FULL (R): 1 when occupancy == 2.
pub const SGX_MSG_FULL: u64 = 0x0c;
/// MSG_CTRL (W): bit0 = reset (clear queue + stalled + completion).
pub const SGX_MSG_CTRL: u64 = 0x10;
/// MSG_DONE (R): completion leaf; bit0 inverts once per genuine engine drain.
pub const SGX_MSG_DONE: u64 = 0x14;
/// SEG_READ (R): stable 64-bit identity of the recovered window. An 8-byte
/// read returns the full identity; a 4-byte read returns its low half. Never
/// faults for a valid size — gate G1.
pub const SGX_SEG_READ: u64 = 0x18;
/// Window stride of the mailbox register set.
pub const SGX_WINDOW: u64 = 0x20;

/// Identity value of the measured d321ap SGX window (`/arm-io/sgx` off
/// 0x4000000 → physical 0x204000000). This is a **modeled registry constant**
/// derived from the recovered DT base; it is not a measured silicon leaf.
pub const SGX_SEG_ID: u64 = 0x204000000;

/// Result of a mailbox push: distinguishes landing vs. slack-stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgxPush {
    Landed,
    QueueFull,
}

/// SGX GPU mailbox window at the recovered `gpu,t8020` base.
pub struct SgxGpu {
    pub id: DeviceId,
    pub irq: u32,
    queue: VecDeque<u32>,
    stalled: bool,
    completion: u32,
}

impl SgxGpu {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            irq: 339,
            queue: VecDeque::new(),
            stalled: false,
            completion: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    pub fn send(&mut self, payload: u32) -> SgxPush {
        if self.queue.len() >= 2 {
            self.stalled = true;
            return SgxPush::QueueFull;
        }
        self.queue.push_back(payload);
        self.stalled = false;
        SgxPush::Landed
    }
    pub fn occupancy(&self) -> usize {
        self.queue.len()
    }
    pub fn is_stalled(&self) -> bool {
        self.stalled
    }
    pub fn completed(&self) -> u32 {
        self.completion
    }
    /// Segment identity returned by an 8-byte `SEG_READ` (gate G1).
    pub fn segment_id(&self) -> u64 {
        SGX_SEG_ID
    }
    /// Genuine engine drain: advances the queue by one word and inverts the
    /// completion leaf. A drain also clears backpressure. Returns the drained
    /// payload.
    pub fn drain_one(&mut self) -> Option<u32> {
        let word = self.queue.pop_front()?;
        self.stalled = false;
        self.completion ^= 1;
        Some(word)
    }
}

fn valid_size(s: usize) -> bool {
    matches!(s, 4 | 8)
}

impl Device for SgxGpu {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "sgx-gpu"
    }
    fn reset(&mut self) {
        self.queue.clear();
        self.stalled = false;
        self.completion = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            SGX_MSG_DEPTH => DeviceResult::Value(2),
            SGX_MSG_STAT => {
                DeviceResult::Value((self.queue.len() as u64) | ((self.stalled as u64) << 2))
            }
            SGX_MSG_FULL => DeviceResult::Value((self.queue.len() >= 2) as u64),
            SGX_MSG_DONE => DeviceResult::Value(self.completion as u64),
            SGX_SEG_READ if s == 8 => DeviceResult::Value(self.segment_id()),
            SGX_SEG_READ => DeviceResult::Value(self.segment_id() & 0xffff_ffff),
            _ => DeviceResult::Value(0),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            SGX_MSG_SEND => {
                let _ = self.send(v as u32);
                DeviceResult::Ok
            }
            SGX_MSG_CTRL if v & 1 != 0 => {
                self.reset();
                DeviceResult::Ok
            }
            _ => DeviceResult::Ok,
        }
    }
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        // Genuine engine drain: one word per tick; a stalled send may land if a
        // slot opened. The completion leaf only moves on a real drain.
        if !self.queue.is_empty() {
            self.drain_one();
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

    #[test]
    fn sgx_gpu_segment_read_is_stable_64bit_and_never_faults() {
        let mut g = SgxGpu::new(1);
        // Gate G1: an 8-byte live read returns the full 64-bit identity and an
        // 8-byte read is never faulted. Repeated reads are identical.
        for _ in 0..3 {
            assert_eq!(g.read(SGX_SEG_READ, 8).unwrap_u64(), SGX_SEG_ID);
        }
        // A 64-bit read from the same region before and after a send+reset
        // stays stable (identity is window-bound, not workload-dependent).
        g.send(0x01);
        assert_eq!(g.read(SGX_SEG_READ, 8).unwrap_u64(), SGX_SEG_ID);
        g.write(SGX_MSG_CTRL, 4, 1);
        assert_eq!(g.read(SGX_SEG_READ, 8).unwrap_u64(), SGX_SEG_ID);
        // 4-byte low-half read is stable too; invalid sizes still fault.
        assert_eq!(
            g.read(SGX_SEG_READ, 4).unwrap_u64(),
            SGX_SEG_ID & 0xffff_ffff
        );
        assert_eq!(g.read(SGX_SEG_READ, 3), DeviceResult::Fault);
    }

    #[test]
    fn sgx_gpu_mailbox_send_tick_completion_roundtrips() {
        let mut g = SgxGpu::new(2);
        // Reset state: depth 2, empty, completion reset.
        assert_eq!(g.read(SGX_MSG_DEPTH, 4).unwrap_u64(), 2);
        assert_eq!(g.completed(), 0);
        // Gate G2: a single send + tick drains one word and the polled
        // completion leaf changes by the driver-produced value (0 crosses to 1).
        assert_eq!(g.send(0x112233), SgxPush::Landed);
        assert_eq!(g.read(SGX_MSG_STAT, 4).unwrap_u64(), 1);
        assert_eq!(g.completed(), 0);
        g.tick(1);
        assert_eq!(g.completed(), 1);
        assert_eq!(g.occupancy(), 0);
        // A tick on the empty queue must not fake another completion.
        g.tick(1);
        assert_eq!(g.completed(), 1);
    }

    #[test]
    fn sgx_gpu_full_queue_stalls_but_never_moves_completion() {
        let mut g = SgxGpu::new(3);
        assert_eq!(g.send(0x1), SgxPush::Landed);
        assert_eq!(g.send(0x2), SgxPush::Landed);
        // Push into the full queue: stall, no fabricated slot, no completion.
        assert_eq!(g.send(0x3), SgxPush::QueueFull);
        assert!(g.is_stalled());
        assert_eq!(g.read(SGX_MSG_STAT, 4).unwrap_u64(), 2 | 4);
        assert_eq!(g.read(SGX_MSG_DONE, 4).unwrap_u64(), 0);
        // Real tick drains one, clears backpressure; the stalled send lands.
        g.tick(1);
        assert_eq!(g.read(SGX_MSG_STAT, 4).unwrap_u64(), 1);
        assert_eq!(g.send(0x3), SgxPush::Landed);
        assert_eq!(g.read(SGX_MSG_DONE, 4).unwrap_u64(), 1);
        // MSG_CTRL bit0 returns the window to reset state.
        g.write(SGX_MSG_CTRL, 4, 1);
        assert_eq!(g.read(SGX_MSG_STAT, 4).unwrap_u64(), 0);
        assert_eq!(g.read(SGX_MSG_DONE, 4).unwrap_u64(), 0);
    }
}
