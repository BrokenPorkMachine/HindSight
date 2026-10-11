use iphem_core::{BusRegion, ExceptionLevel, Machine, MachineConfig, SparseRam};

fn new_machine() -> Machine {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine
        .bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x1000, 0x1000)),
        })
        .unwrap();
    machine.cores.active_mut().state.sctlr_el1 = 1 << 1; // A, with MMU disabled
    machine.cores.active_mut().state.vbar_el1 = 0x20_000;
    machine
}

#[test]
fn sctlr_a_takes_alignment_abort_before_bus_access() {
    let mut machine = new_machine();
    assert!(machine.mem_read(0x1001, 8).is_err());
    assert_eq!(machine.cores.active().state.far_el1, 0x1001);
    assert_eq!(machine.cores.active().state.esr_el1 >> 26, 0x25);
    assert_eq!(machine.cores.active().state.esr_el1 & 0x3f, 0x21);
    assert_eq!(machine.cores.active().state.esr_el1 & (1 << 6), 0);
}

#[test]
fn alignment_write_sets_wnr_and_disabled_check_allows_access() {
    let mut machine = new_machine();
    assert!(machine.mem_write(0x1001, 4, 0x1234).is_err());
    assert_ne!(machine.cores.active().state.esr_el1 & (1 << 6), 0);

    let mut machine = new_machine();
    machine.cores.active_mut().state.sctlr_el1 &= !(1 << 1);
    assert!(machine.mem_write(0x1001, 4, 0x1234).is_ok());
    assert_eq!(machine.mem_read(0x1001, 4).unwrap(), 0x1234);
}

#[test]
fn higher_exception_levels_use_their_own_sctlr_alignment_control() {
    let mut machine = new_machine();
    machine.cores.active_mut().state.el = ExceptionLevel::El2;
    machine.cores.active_mut().state.currentel = 2;
    machine.cores.active_mut().state.pc = 0x3000;
    machine.cores.active_mut().state.vbar_el2 = 0x30_000;
    machine.cores.active_mut().state.sctlr_el1 = 0;
    machine.cores.active_mut().state.sctlr_el2 = 1 << 1;
    assert!(machine.mem_read(0x1001, 8).is_err());
    assert_eq!(machine.cores.active().state.far_el2, 0x1001);
    assert_eq!(machine.cores.active().state.esr_el2 >> 26, 0x25);
    assert_eq!(machine.cores.active().state.elr_el2, 0x3000);

    machine.cores.active_mut().state.el = ExceptionLevel::El3;
    machine.cores.active_mut().state.currentel = 3;
    machine.cores.active_mut().state.pc = 0x4000;
    machine.cores.active_mut().state.vbar_el3 = 0x40_000;
    machine.cores.active_mut().state.sctlr_el2 = 0;
    machine.cores.active_mut().state.sctlr_el3 = 1 << 1;
    assert!(machine.mem_write(0x1001, 8, 0xaa).is_err());
    assert_eq!(machine.cores.active().state.far_el3, 0x1001);
    assert_eq!(machine.cores.active().state.esr_el3 >> 26, 0x25);
    assert_eq!(machine.cores.active().state.elr_el3, 0x4000);
}
