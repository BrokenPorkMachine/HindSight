use iphem_arm::{execute, Instruction};
use iphem_core::{ExceptionLevel, Machine, MachineConfig};

#[test]
fn architectural_spsel_immediate_switches_and_preserves_banks() {
    // Independently assembled with clang: msr SPSel,#0 / msr SPSel,#1.
    for el in [
        ExceptionLevel::El1,
        ExceptionLevel::El2,
        ExceptionLevel::El3,
    ] {
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        m.cores.active_mut().state.el = el;
        m.cores.active_mut().state.spsel = false;
        m.cores.active_mut().state.sp = 0x8000;
        m.cores.active_mut().state.sp_el[el as usize] = 0x9000;
        execute(&mut m, Instruction(0xd50041bf)).unwrap();
        assert!(m.cores.active().state.spsel);
        assert_eq!(m.cores.active().state.sp, 0x9000);
        assert_eq!(m.cores.active().state.sp_el[0], 0x8000);
        m.cores.active_mut().state.sp -= 16;
        execute(&mut m, Instruction(0xd50040bf)).unwrap();
        assert!(!m.cores.active().state.spsel);
        assert_eq!(m.cores.active().state.sp, 0x8000);
        assert_eq!(m.cores.active().state.sp_el[el as usize], 0x8ff0);
    }
}
