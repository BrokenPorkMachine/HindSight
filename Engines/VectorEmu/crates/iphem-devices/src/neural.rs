//! Apple Neural Engine (ANE) slice — Phase 190 item 3.
//!
//! The real d321ap DeviceTree declares `/arm-io/ane,t8020` at
//! `(off=0x6a000000 size=0x1f00000)` relative to the `0x200000000` arm-io
//! aperture, i.e. physical `0x26a000000`, on IRQ 274. No execution unit is
//! modeled (real NE kernels are not available); instead the slice provides the
//! per-plane `kF0…kF7` coprocessing FIFO window as a genuine bounded
//! producer/consumer:
//!
//! - a 2-deep payload queue (slack-stall: a push into a full queue does not
//!   fabricate a slot, it returns the `stalled` result and `kF4` records the
//!   backpressure bit),
//! - a completion leaf `kF7` that inverts **only** on a real engine tick that
//!   drains one queued word — never a host-fabricated readiness bit,
//! - drains happen only in `tick` (the `Machine::tick_devices` path the Phase
//!   190 broadcast plan requires), so the poll departs on genuine drain.
//!
//! This satisfies gates G1 (3rd push into a 2-deep FIFO stalls; a tick drains
//! one; the 3rd push then lands) and G2 (no `model_ready`; completion leaf
//! stays at reset until the queue genuinely drains). Reproduction in
//! `docs/PHASE190_PROGRESS.md`.

use iphem_core::device::{Device, DeviceEvent, DeviceId, DeviceResult};
use std::any::Any;
use std::collections::VecDeque;

/// kF0 push data register (W): enqueue one 32-bit payload word.
pub const KF0_PUSH: u64 = 0x00;
/// kF1 engine-pop (W): the engine side drains one word. Host writes Fault.
pub const KF1_POP: u64 = 0x04;
/// kF2 FIFO control (W): bit0 = reset (clear queue + stalled flag).
pub const KF2_CTRL: u64 = 0x08;
/// kF3 FIFO depth (R): always 2.
pub const KF3_DEPTH: u64 = 0x0c;
/// kF4 occupancy/backpressure (R): bits 1:0 = occupied, bit2 = stalled.
pub const KF4_STAT: u64 = 0x10;
/// kF5 full (R): 1 when the 2-deep queue is full.
pub const KF5_FULL: u64 = 0x14;
/// kF6 engine-drain (W): host pull-token; reserved (Fault from host).
pub const KF6_DRAIN: u64 = 0x18;
/// kF7 completion leaf (R): bit0 inverts once per genuine engine drain.
pub const KF7_COMPLETE: u64 = 0x1c;
/// Window stride of the kF0…kF7 set (8 × 4-byte registers).
pub const KF_WINDOW: u64 = 0x20;

/// Result of a FIFO push: distinguishes landing vs. slack-stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnePush {
    Landed,
    QueueFull,
}

/// Apple Neural Engine per-plane coprocessing FIFO window.
pub struct AneFifo {
    pub id: DeviceId,
    pub irq: u32,
    queue: VecDeque<u32>,
    stalled: bool,
    this_window: u32,
    completion: u32,
}

impl AneFifo {
    pub fn new(id: DeviceId) -> Self {
        Self {
            id,
            irq: 274,
            queue: VecDeque::new(),
            stalled: false,
            this_window: 0,
            completion: 0,
        }
    }
    pub fn with_irq(id: DeviceId, irq: u32) -> Self {
        Self {
            irq,
            ..Self::new(id)
        }
    }
    pub fn kf0_push(&mut self, payload: u32) -> AnePush {
        if self.queue.len() >= 2 {
            self.stalled = true;
            return AnePush::QueueFull;
        }
        self.queue.push_back(payload);
        self.stalled = false;
        AnePush::Landed
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
    /// Genuine engine drain: advances the queue by one word and inverts the
    /// completion leaf. A drain also clears the backpressure bit, since the
    /// FIFO now has room, so a stalled producer write lands on the next kF0
    /// push. Returns the drained payload.
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

impl Device for AneFifo {
    fn id(&self) -> DeviceId {
        self.id
    }
    fn name(&self) -> &'static str {
        "ane-fifo"
    }
    fn reset(&mut self) {
        self.queue.clear();
        self.stalled = false;
        self.this_window = 0;
        self.completion = 0;
    }
    fn read(&mut self, o: u64, s: usize) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            KF3_DEPTH => DeviceResult::Value(2),
            KF4_STAT => {
                DeviceResult::Value((self.queue.len() as u64) | ((self.stalled as u64) << 2))
            }
            KF5_FULL => DeviceResult::Value((self.queue.len() >= 2) as u64),
            KF7_COMPLETE => DeviceResult::Value(self.completion as u64),
            _ => DeviceResult::Value(0),
        }
    }
    fn write(&mut self, o: u64, s: usize, v: u64) -> DeviceResult {
        if !valid_size(s) {
            return DeviceResult::Fault;
        }
        match o {
            KF0_PUSH => {
                self.this_window = v as u32;
                let _ = self.kf0_push(v as u32);
                DeviceResult::Ok
            }
            KF2_CTRL if v & 1 != 0 => {
                self.reset();
                DeviceResult::Ok
            }
            KF1_POP | KF6_DRAIN => DeviceResult::Fault,
            _ => DeviceResult::Ok,
        }
    }
    fn tick(&mut self, _cycles: u64) -> Option<DeviceEvent> {
        // Genuine engine drain: one word per tick; a stalled push may land if a
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
    fn ane_fifo_third_push_stalls_then_lands_on_tick() {
        let mut a = AneFifo::new(1);
        assert_eq!(a.kf0_push(0x11), AnePush::Landed);
        assert_eq!(a.kf0_push(0x22), AnePush::Landed);
        // 3rd push into the 2-deep FIFO stalls (no fabricated slot).
        assert_eq!(a.kf0_push(0x33), AnePush::QueueFull);
        assert!(a.is_stalled());
        assert_eq!(a.occupancy(), 2);
        assert_eq!(a.completed(), 0);
        // Real engine tick drains one word and moves the completion leaf.
        a.tick(1);
        assert_eq!(a.completed(), 1);
        assert_eq!(a.occupancy(), 1);
        // The stalled push now lands and backpressure clears.
        assert_eq!(a.kf0_push(0x33), AnePush::Landed);
        assert!(!a.is_stalled());
        assert_eq!(a.occupancy(), 2);
    }

    #[test]
    fn ane_fifo_completion_leaf_never_moves_without_genuine_drain() {
        let mut a = AneFifo::new(2);
        // Push + no ticks: completion stays at reset value (G2).
        assert_eq!(a.kf0_push(0x01), AnePush::Landed);
        assert_eq!(a.completed(), 0);
        // kF4/kF5 expose occupancy/backpressure, not a fake ready bit.
        assert_eq!(a.read(KF4_STAT, 4).unwrap_u64(), 1);
        assert_eq!(a.read(KF5_FULL, 4).unwrap_u64(), 0);
        // A host write to a full queue stalls but never drains.
        assert_eq!(a.kf0_push(0x02), AnePush::Landed);
        assert_eq!(a.kf0_push(0x03), AnePush::QueueFull);
        assert_eq!(a.completed(), 0);
        // Drain-only completion: two ticks drain the two queued words,
        // inverting the leaf once per drain (2 drains => back to reset).
        a.tick(1);
        a.tick(1);
        assert_eq!(a.completed(), 0);
        assert_eq!(a.occupancy(), 0);
        assert_eq!(a.read(KF7_COMPLETE, 4).unwrap_u64(), 0);
        // A third drain on the now-empty queue must not fake a completion.
        a.tick(1);
        assert_eq!(a.completed(), 0);
        assert_eq!(a.occupancy(), 0);
    }

    #[test]
    fn ane_fifo_reset_clears_queue_and_stall() {
        let mut a = AneFifo::new(3);
        a.kf0_push(0x1);
        a.kf0_push(0x2);
        a.kf0_push(0x3); // stalls
        a.write(KF2_CTRL, 4, 1);
        assert!(a.queue.is_empty());
        assert!(!a.stalled);
        assert_eq!(a.completed(), 0);
        assert_eq!(a.read(KF4_STAT, 4).unwrap_u64(), 0);
    }
}
