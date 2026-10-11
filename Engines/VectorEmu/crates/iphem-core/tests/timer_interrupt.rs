use iphem_core::{CpuRunState, Machine, MachineConfig};

#[test]
fn physical_timer_compare_wakes_wfi_with_generic_timer_ppi() {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state.cntp_cval_el0 = 3;
    machine.cores.active_mut().state.cntp_ctl_el0 = 1;
    machine.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;

    machine.tick_devices(2);
    assert_eq!(machine.cores.active().pending_irq, None);
    assert_eq!(
        machine.cores.active().run_state,
        CpuRunState::WaitingForInterrupt
    );

    machine.tick_devices(1);
    assert_eq!(machine.cores.active().pending_irq, Some(30));
    assert_eq!(machine.cores.active().run_state, CpuRunState::Running);
}

#[test]
fn masked_timer_does_not_assert_irq_but_keeps_compare_status_due() {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state.cntp_cval_el0 = 1;
    machine.cores.active_mut().state.cntp_ctl_el0 = 0b11;
    machine.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;

    machine.tick_devices(1);
    assert_eq!(machine.cores.active().pending_irq, None);
    assert_eq!(
        machine.cores.active().run_state,
        CpuRunState::WaitingForInterrupt
    );

    machine.cores.active_mut().state.cntp_ctl_el0 = 1;
    machine.tick_devices(1);
    assert_eq!(machine.cores.active().pending_irq, Some(30));
    assert_eq!(machine.cores.active().run_state, CpuRunState::Running);
}

/// CNTP_OFF_EL2 shifts the physical counter, so the same CVAL compare fires
/// later by exactly the offset value. This validates the cycle-accurate
/// compare match under an EL2 physical-counter offset.
#[test]
fn cntpoff_el2_delays_physical_compare_match_by_the_offset() {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    // Without offset, compare at cycle 3 fires at tick 3.
    machine.cores.active_mut().state.cntp_cval_el0 = 3;
    machine.cores.active_mut().state.cntp_ctl_el0 = 1;
    machine.cores.active_mut().state.cntpoff_el2 = 0;
    machine.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;

    machine.tick_devices(3);
    assert_eq!(machine.cores.active().pending_irq, Some(30));
    assert_eq!(machine.cores.active().run_state, CpuRunState::Running);

    // With offset 2, the physical counter is cycle - 2, so the same CVAL=3
    // compare fires at tick 5 (physical counter reaches 3 at cycle 5).
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state.cntp_cval_el0 = 3;
    machine.cores.active_mut().state.cntp_ctl_el0 = 1;
    machine.cores.active_mut().state.cntpoff_el2 = 2;
    machine.cores.active_mut().run_state = CpuRunState::WaitingForInterrupt;

    machine.tick_devices(4);
    assert_eq!(machine.cores.active().pending_irq, None);
    machine.tick_devices(1);
    assert_eq!(machine.cores.active().pending_irq, Some(30));
    assert_eq!(machine.cores.active().run_state, CpuRunState::Running);
}
