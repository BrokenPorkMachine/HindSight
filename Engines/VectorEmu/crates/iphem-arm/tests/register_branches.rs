use iphem_arm::{execute, Instruction};
use iphem_core::{Machine, MachineConfig};

#[test]
fn register_calls_link_and_branches_preserve_lr() {
    for base in [
        0xd61f0060u32,
        0xd61f087f,
        0xd61f0c7f,
        0xd71f0864,
        0xd71f0c64,
    ] {
        for link in [false, true] {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            m.cores.active_mut().state.pc = 0x1000;
            m.cores.active_mut().state.x[3] = 0x2000;
            m.cores.active_mut().state.x[30] = 0x3000;
            execute(&mut m, Instruction(base | ((link as u32) << 21))).unwrap();
            assert_eq!(m.cores.active().state.pc, 0x2000);
            assert_eq!(
                m.cores.active().state.x[30],
                if link { 0x1004 } else { 0x3000 }
            );
        }
    }
}
