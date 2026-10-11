#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Scheduler {
    cycle: u64,
    preempt_active: bool,
    preempt_irq: Option<u32>,
    preempt_interval: u64,
    next_preempt: u64,
}

impl Scheduler {
    pub fn cycle(&self) -> u64 {
        self.cycle
    }

    pub fn tick(&mut self, n: u64) {
        self.cycle = self.cycle.wrapping_add(n);
    }

    /// Arm a periodic preemption interrupt. Every `interval_cycles` the
    /// scheduler reports `preemption_due()` as `Some(irq)`, which the machine
    /// injects on the active core (a virtual timer quantum tick).
    pub fn enable_preemption(&mut self, irq: u32, interval_cycles: u64) {
        self.preempt_active = true;
        self.preempt_irq = Some(irq);
        self.preempt_interval = interval_cycles.max(1);
        self.next_preempt = self.cycle.wrapping_add(self.preempt_interval);
    }

    pub fn disable_preemption(&mut self) {
        self.preempt_active = false;
        self.preempt_irq = None;
    }

    pub fn preemption_enabled(&self) -> bool {
        self.preempt_active
    }

    pub fn preemption_irq(&self) -> Option<u32> {
        self.preempt_irq
    }

    pub fn next_preemption_cycle(&self) -> u64 {
        self.next_preempt
    }

    /// Reports the preemption IRQ exactly once per interval boundary.
    pub fn preemption_due(&mut self) -> Option<u32> {
        if !self.preempt_active {
            return None;
        }
        if self.cycle >= self.next_preempt {
            self.next_preempt = self.next_preempt.wrapping_add(self.preempt_interval);
            return self.preempt_irq;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preemption_fires_periodically() {
        let mut s = Scheduler::default();
        s.enable_preemption(29, 8);
        assert_eq!(s.next_preemption_cycle(), 8);

        let mut fires = Vec::new();
        for _ in 0..64 {
            s.tick(1);
            if let Some(v) = s.preemption_due() {
                fires.push((s.cycle(), v));
            }
        }
        // Cycle 8, 16, 24, ... 64 -> 8 firings of the preemption IRQ.
        assert_eq!(fires.len(), 8);
        for (i, (cycle, irq)) in fires.iter().enumerate() {
            assert_eq!(*cycle, (8 * (i + 1)) as u64);
            assert_eq!(*irq, 29);
        }
    }

    #[test]
    fn test_preemption_disabled_by_default_and_after_disable() {
        let mut s = Scheduler::default();
        assert!(!s.preemption_enabled());
        assert_eq!(s.preemption_due(), None);

        s.enable_preemption(3, 2);
        s.tick(2);
        assert_eq!(s.preemption_due(), Some(3));
        // Second due must not fire this cycle even if polled again.
        assert_eq!(s.preemption_due(), None);

        s.disable_preemption();
        assert!(!s.preemption_enabled());
        s.tick(4);
        assert_eq!(s.preemption_due(), None);
    }
}
