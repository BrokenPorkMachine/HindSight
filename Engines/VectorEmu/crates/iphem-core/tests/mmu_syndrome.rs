use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn machine_with_empty_tables() -> Machine {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine
        .bus
        .map(BusRegion {
            base: 0x4000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x4000, 0x1000)),
        })
        .unwrap();
    machine.cores.active_mut().state.sctlr_el1 = 1;
    machine.cores.active_mut().state.ttbr0_el1 = 0x4000;
    machine.cores.active_mut().state.tcr_el1 = 16 | (16 << 16) | (2 << 30);
    machine
}

#[test]
fn read_translation_fault_has_architectural_dfsc() {
    let mut machine = machine_with_empty_tables();
    assert!(machine.translate(0x1234, false, false).is_err());
    assert_eq!(machine.cores.active().state.esr_el1 >> 26, 0x25);
    assert_ne!(machine.cores.active().state.esr_el1 & (1 << 25), 0);
    assert_eq!(machine.cores.active().state.esr_el1 & 0x3f, 0x04);
    assert_eq!(machine.cores.active().state.esr_el1 & (1 << 6), 0);
}

#[test]
fn write_translation_fault_sets_wnr() {
    let mut machine = machine_with_empty_tables();
    assert!(machine.translate(0x1234, true, false).is_err());
    assert_eq!(machine.cores.active().state.esr_el1 & 0x3f, 0x04);
    assert_ne!(machine.cores.active().state.esr_el1 & (1 << 6), 0);
}

#[test]
fn instruction_translation_fault_uses_instruction_abort_ec() {
    let mut machine = machine_with_empty_tables();
    assert!(machine.translate(0x1234, false, true).is_err());
    assert_eq!(machine.cores.active().state.esr_el1 >> 26, 0x21);
    assert_ne!(machine.cores.active().state.esr_el1 & (1 << 25), 0);
    assert_eq!(machine.cores.active().state.esr_el1 & 0x3f, 0x04);
}
