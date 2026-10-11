use crate::device::DeviceEvent;
use std::fs::File;
use std::io::{BufWriter, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TraceKind {
    MmioRead,
    MmioWrite,
    Interrupt,
    Exception,
    Dma,
    Instruction,
    Reset,
    SysRegRead,
    SysRegWrite,
    BranchTaken,
    BranchNotTaken,
    CacheOp,
    TlbOp,
    Svc,
    Erat,
    PmuEvent,
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub enum Verbosity {
    None = 0,
    Summary = 1,
    Basic = 2,
    Detailed = 3,
    Verbose = 4,
    Debug = 5,
    Trace = 6,
}

impl Verbosity {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Verbosity::None,
            1 => Verbosity::Summary,
            2 => Verbosity::Basic,
            3 => Verbosity::Detailed,
            4 => Verbosity::Verbose,
            5 => Verbosity::Debug,
            _ => Verbosity::Trace,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraceEvent {
    pub timestamp: u64,
    pub kind: TraceKind,
    pub address: u64,
    pub value: u64,
    pub size: usize,
    pub pc: u64,
    pub el: u8,
    pub cpsr: u64,
    pub disasm: Option<String>,
    /// Monotonic record number within this engine, starting at 0.
    ///
    /// Retention is bounded (see [`DEFAULT_TRACE_EVENT_CAPACITY`]), so a
    /// positional index into the retained buffer is not a stable identity: a
    /// long run drops the oldest events and every index shifts. Callers that
    /// need "what happened while this stage ran" record a
    /// [`TraceEngine::watermark`] and query
    /// [`TraceEngine::events_since`], which stays correct as the window rolls.
    /// `#[serde(default)]` keeps previously captured snapshots loadable.
    #[serde(default)]
    pub seq: u64,
}

/// Default number of `TraceEvent`s retained in memory.
///
/// Retention must be bounded because per-instruction tracing is enabled on
/// most machines and a long run retires tens of millions of instructions. An
/// unbounded buffer grows at roughly 80-100 bytes per guest instruction, so a
/// 1.2M-instruction host budget that also drove a co-scheduled companion core
/// reached several gigabytes and exhausted the host. When the limit is reached
/// the oldest half is discarded in one pass, so resident memory stays bounded
/// while the events nearest the terminal state -- what a trace is read for --
/// are preserved. [`TraceEngine::dropped_events`] reports how many were
/// discarded so no run silently loses information.
pub const DEFAULT_TRACE_EVENT_CAPACITY: usize = 1 << 18;

pub struct TraceEngine {
    events: Vec<TraceEvent>,
    /// Retention limit; the oldest half is released once it is reached.
    event_capacity: usize,
    /// Events discarded because retention was full. Counted, never hidden.
    dropped_events: u64,
    /// Sequence number the next recorded event will carry.
    next_seq: u64,
    cycle: u64,
    enabled: bool,
    verbosity: Verbosity,
    live_output: bool,
    output_file: Option<BufWriter<File>>,
    inst_count: u64,
    branch_count: u64,
    exception_count: u64,
    mmio_count: u64,
}

impl TraceEngine {
    pub fn new(enabled: bool) -> Self {
        Self::with_capacity(enabled, DEFAULT_TRACE_EVENT_CAPACITY)
    }

    /// Build an engine that retains at most `capacity` events. A capacity of
    /// zero counts events without retaining any, which is the cheap mode for
    /// runs that only need the instruction/MMIO/exception counters.
    pub fn with_capacity(enabled: bool, capacity: usize) -> Self {
        Self {
            events: Vec::new(),
            event_capacity: capacity,
            dropped_events: 0,
            next_seq: 0,
            cycle: 0,
            enabled,
            verbosity: Verbosity::Summary,
            live_output: false,
            output_file: None,
            inst_count: 0,
            branch_count: 0,
            exception_count: 0,
            mmio_count: 0,
        }
    }

    /// Change the retention limit, dropping the oldest retained events if the
    /// new limit is smaller than what is already held.
    pub fn set_event_capacity(&mut self, capacity: usize) {
        self.event_capacity = capacity;
        self.trim_to_capacity();
    }

    fn trim_to_capacity(&mut self) {
        if self.events.len() > self.event_capacity {
            let excess = self.events.len() - self.event_capacity;
            self.events.drain(..excess);
            self.dropped_events += excess as u64;
        }
    }

    pub fn event_capacity(&self) -> usize {
        self.event_capacity
    }

    /// Events discarded because the retention window was full.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    /// Record number the next event will carry. Pass this to
    /// [`TraceEngine::events_since`] after running a stage to get exactly that
    /// stage's events, independent of how much was retained.
    pub fn watermark(&self) -> u64 {
        self.next_seq
    }

    /// Retained events recorded at or after `mark`, oldest first.
    ///
    /// Correct across window rolls, unlike a positional index. It can only
    /// return events still inside the retention window: if a stage retired more
    /// instructions than [`TraceEngine::event_capacity`] events, the earliest
    /// of its events have already been discarded and this result is the
    /// retained tail, not the whole stage. Compare
    /// [`TraceEngine::dropped_events`] before and after a stage to detect that.
    pub fn events_since(&self, mark: u64) -> Vec<TraceEvent> {
        self.events
            .iter()
            .filter(|e| e.seq >= mark)
            .cloned()
            .collect()
    }

    pub fn with_verbosity(mut self, v: Verbosity) -> Self {
        self.verbosity = v;
        self
    }

    pub fn with_live_output(mut self, enabled: bool) -> Self {
        self.live_output = enabled;
        self
    }

    /// Whether the trace engine is actively recording events. Hot paths use
    /// this to skip building and pushing trace payloads entirely when tracing
    /// is disabled (the record machinery itself also early-returns).
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn restore_captured_state(
        &mut self,
        events: Vec<TraceEvent>,
        cycle: u64,
        enabled: bool,
        verbosity: u8,
        counts: (u64, u64, u64, u64),
    ) {
        self.events = events;
        self.cycle = cycle;
        self.enabled = enabled;
        self.verbosity = Verbosity::from_u8(verbosity);
        (
            self.inst_count,
            self.branch_count,
            self.exception_count,
            self.mmio_count,
        ) = counts;
        self.trim_to_capacity();
        // Continue numbering past the highest sequence already restored so a
        // subsequent `events_since` watermark cannot alias restored events.
        self.next_seq = self.events.last().map(|e| e.seq + 1).unwrap_or(0);
    }

    pub fn with_output_file(mut self, path: &str) -> std::io::Result<Self> {
        let file = File::create(path)?;
        self.output_file = Some(BufWriter::new(file));
        Ok(self)
    }

    pub fn advance(&mut self, n: u64) {
        self.cycle = self.cycle.wrapping_add(n);
    }

    pub fn cycle(&self) -> u64 {
        self.cycle
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        a: u64,
        v: u64,
        s: usize,
        k: TraceKind,
        pc: u64,
        el: u8,
        cpsr: u64,
        disasm: Option<String>,
    ) {
        if !self.enabled {
            return;
        }

        match k {
            TraceKind::Instruction => self.inst_count += 1,
            TraceKind::BranchTaken | TraceKind::BranchNotTaken => self.branch_count += 1,
            TraceKind::Exception | TraceKind::Svc => self.exception_count += 1,
            TraceKind::MmioRead | TraceKind::MmioWrite => self.mmio_count += 1,
            _ => {}
        }

        let ev = TraceEvent {
            timestamp: self.cycle,
            kind: k,
            address: a,
            value: v,
            size: s,
            pc,
            el,
            cpsr,
            disasm,
            seq: self.next_seq,
        };
        self.next_seq += 1;

        // Bounded retention. When the limit is reached the oldest half is
        // released in a single pass: contiguous storage is preserved (so
        // `events()` can still hand out a slice) and the amortized cost is a
        // couple of element moves per event rather than a per-event shift.
        if self.event_capacity == 0 {
            self.dropped_events += 1;
        } else {
            if self.events.len() >= self.event_capacity {
                let drop_count = (self.event_capacity / 2).max(1);
                let drop_count = drop_count.min(self.events.len());
                self.events.drain(..drop_count);
                self.dropped_events += drop_count as u64;
            }
            self.events.push(ev.clone());
        }

        if self.live_output || self.verbosity as u8 >= Verbosity::Detailed as u8 {
            self.print_live(&ev);
        }

        if let Some(ref mut f) = self.output_file {
            let _ = writeln!(f, "{:?}", ev);
        }
    }

    pub fn record_event(&mut self, e: DeviceEvent, pc: u64, el: u8, cpsr: u64) {
        match e {
            DeviceEvent::Interrupt(i) => {
                self.record(i as u64, 0, 0, TraceKind::Interrupt, pc, el, cpsr, None)
            }
            DeviceEvent::InterruptTo(i, _) => {
                self.record(i as u64, 0, 0, TraceKind::Interrupt, pc, el, cpsr, None)
            }
            DeviceEvent::Fiq(i) | DeviceEvent::FiqTo(i, _) => {
                self.record(i as u64, 0, 0, TraceKind::Interrupt, pc, el, cpsr, None)
            }
            DeviceEvent::SError(iss) | DeviceEvent::SErrorTo(iss, _) => {
                self.record(iss, 0, 0, TraceKind::Exception, pc, el, cpsr, None)
            }
            DeviceEvent::DmaRequest { channel } => {
                self.record(channel as u64, 0, 0, TraceKind::Dma, pc, el, cpsr, None)
            }
        }
    }

    fn print_live(&self, ev: &TraceEvent) {
        let kind_str = match ev.kind {
            TraceKind::Instruction => "INST",
            TraceKind::BranchTaken => "BR_TAKEN",
            TraceKind::BranchNotTaken => "BR_NOT",
            TraceKind::Exception => "EXC",
            TraceKind::Svc => "SVC",
            TraceKind::MmioRead => "MMIO_R",
            TraceKind::MmioWrite => "MMIO_W",
            TraceKind::SysRegRead => "SYSREG_R",
            TraceKind::SysRegWrite => "SYSREG_W",
            TraceKind::Reset => "RESET",
            TraceKind::Interrupt => "IRQ",
            TraceKind::Dma => "DMA",
            TraceKind::CacheOp => "CACHE",
            TraceKind::TlbOp => "TLB",
            TraceKind::Erat => "ERAT",
            TraceKind::PmuEvent => "PMU",
        };

        let disasm_str = ev.disasm.as_deref().unwrap_or("");

        if self.verbosity as u8 >= Verbosity::Verbose as u8 {
            eprintln!(
                "[{:>12}] {:8} PC={:#018x} EL={} CPSR={:#018x} ADDR={:#018x} VAL={:#018x} SIZE={} {}",
                ev.timestamp, kind_str, ev.pc, ev.el, ev.cpsr, ev.address, ev.value, ev.size, disasm_str
            );
        } else {
            eprintln!(
                "[{:>12}] {:8} PC={:#018x} ADDR={:#018x} VAL={:#018x} {}",
                ev.timestamp, kind_str, ev.pc, ev.address, ev.value, disasm_str
            );
        }
    }

    /// Retained events in chronological order, oldest first.
    ///
    /// Only the most recent [`TraceEngine::event_capacity`] events are
    /// present; see [`TraceEngine::dropped_events`] for what was discarded.
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Owned copy of the retained events, for callers that need to outlive the
    /// borrow on the machine.
    pub fn owned_events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    pub fn get_verbosity(&self) -> u8 {
        self.verbosity as u8
    }

    pub fn get_inst_count(&self) -> u64 {
        self.inst_count
    }

    pub fn get_branch_count(&self) -> u64 {
        self.branch_count
    }

    pub fn get_exception_count(&self) -> u64 {
        self.exception_count
    }

    pub fn get_mmio_count(&self) -> u64 {
        self.mmio_count
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.dropped_events = 0;
        self.inst_count = 0;
        self.branch_count = 0;
        self.exception_count = 0;
        self.mmio_count = 0;
    }

    pub fn stats(&self) -> String {
        format!(
            "Instructions: {} | Branches: {} | Exceptions: {} | MMIO: {} | Events: {} | Dropped: {}",
            self.inst_count,
            self.branch_count,
            self.exception_count,
            self.mmio_count,
            self.events.len(),
            self.dropped_events
        )
    }

    pub fn dump_to_file(&self, path: &str) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        for ev in &self.events {
            writeln!(writer, "{:?}", ev)?;
        }
        writeln!(writer, "\n--- SUMMARY ---")?;
        writeln!(writer, "{}", self.stats())?;
        Ok(())
    }

    pub fn filter_events(&self, kind: TraceKind) -> Vec<&TraceEvent> {
        self.events.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn events_in_range(&self, start_pc: u64, end_pc: u64) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| e.pc >= start_pc && e.pc <= end_pc)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(engine: &mut TraceEngine, pc: u64) {
        engine.record(pc, 0, 4, TraceKind::Instruction, pc, 1, 0, None);
    }

    /// Retention is bounded: a long run must not grow the event buffer without
    /// limit, and what it discarded must be reported rather than hidden.
    ///
    /// This is the defect that exhausted the host. Per-instruction tracing is
    /// enabled on most machines, and an unbounded buffer costs ~80-100 bytes
    /// per guest instruction, so a run that also drove a co-scheduled companion
    /// core reached several gigabytes.
    #[test]
    fn retention_is_bounded_and_reports_drops() {
        let mut engine = TraceEngine::with_capacity(true, 64);
        for pc in 0..10_000 {
            record(&mut engine, pc);
        }
        assert!(
            engine.events().len() <= 64,
            "retained {} events, cap is 64",
            engine.events().len()
        );
        assert!(
            engine.dropped_events() > 0,
            "discarded events must be counted"
        );
        // The most recent events are the ones a trace is read for, so they
        // must survive.
        let last = engine.events().last().expect("events retained");
        assert_eq!(last.pc, 9_999);
        // Counters stay exact regardless of retention.
        assert_eq!(engine.get_inst_count(), 10_000);
    }

    /// A zero capacity is the cheap counting mode: no memory, full counts.
    #[test]
    fn zero_capacity_retains_nothing_but_still_counts() {
        let mut engine = TraceEngine::with_capacity(true, 0);
        for pc in 0..100 {
            record(&mut engine, pc);
        }
        assert!(engine.events().is_empty());
        assert_eq!(engine.dropped_events(), 100);
        assert_eq!(engine.get_inst_count(), 100);
    }

    /// `events_since` stays correct after the window rolls, which a positional
    /// index cannot: dropping the oldest half shifts every index.
    ///
    /// It returns the retained window, so a stage longer than the capacity
    /// yields its tail. A caller that needs a complete stage either runs
    /// within the cap or compares `dropped_events()` across the stage.
    #[test]
    fn events_since_survives_window_roll() {
        let mut engine = TraceEngine::with_capacity(true, 64);
        for pc in 0..50 {
            record(&mut engine, pc);
        }
        // Take a watermark, then let the window roll several times.
        let mark = engine.watermark();
        let dropped_before = engine.dropped_events();
        for pc in 50..5_000 {
            record(&mut engine, pc);
        }
        let since = engine.events_since(mark);
        // Everything the window still holds after the mark, and nothing earlier.
        assert!(!since.is_empty());
        assert!(since.iter().all(|e| e.seq >= mark));
        assert!(since.len() <= 64, "bounded by the capacity");
        assert_eq!(
            since.last().unwrap().pc,
            4_999,
            "newest event is always retained"
        );
        // The roll really did discard events, so the incomplete-window case is
        // the one under test, and it is detectable.
        assert!(engine.dropped_events() > dropped_before);
        // Sequence numbers are strictly increasing across the roll.
        assert!(since.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    /// A stage that fits inside the capacity is returned in full, which is the
    /// common case for the stage runners.
    #[test]
    fn events_since_is_complete_within_capacity() {
        let mut engine = TraceEngine::with_capacity(true, 64);
        for pc in 0..10 {
            record(&mut engine, pc);
        }
        let mark = engine.watermark();
        for pc in 10..40 {
            record(&mut engine, pc);
        }
        let since = engine.events_since(mark);
        assert_eq!(since.len(), 30);
        assert_eq!(since[0].pc, 10);
        assert_eq!(since.last().unwrap().pc, 39);
        assert_eq!(engine.dropped_events(), 0);
    }

    /// Lowering the limit discards the oldest events and counts them.
    #[test]
    fn lowering_capacity_trims_and_counts() {
        let mut engine = TraceEngine::with_capacity(true, 100);
        for pc in 0..100 {
            record(&mut engine, pc);
        }
        engine.set_event_capacity(10);
        assert_eq!(engine.events().len(), 10);
        assert_eq!(engine.dropped_events(), 90);
        assert_eq!(engine.events().last().unwrap().pc, 99);
    }

    /// The default engine bounds retention too; the cap is part of the
    /// contract, not an opt-in.
    #[test]
    fn default_engine_bounds_retention() {
        let mut engine = TraceEngine::new(true);
        assert_eq!(engine.event_capacity(), DEFAULT_TRACE_EVENT_CAPACITY);
        for pc in 0..DEFAULT_TRACE_EVENT_CAPACITY as u64 * 2 {
            record(&mut engine, pc);
        }
        assert!(engine.events().len() <= DEFAULT_TRACE_EVENT_CAPACITY);
        assert!(engine.dropped_events() > 0);
    }
}
