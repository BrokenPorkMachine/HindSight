use iphem_arm::{execute, ExecError, Instruction};
use iphem_core::{Machine, MachineConfig};

#[test]
fn shifted_compare_accepts_lsr_and_asr_at_both_widths() {
    for sf in [0, 1] {
        for shift in [1, 2] {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            m.cores.active_mut().state.x[9] = if sf == 0 {
                0xdeadbeef80000000
            } else {
                0x8000000000000000
            };
            m.cores.active_mut().state.x[10] = match (sf, shift) {
                (0, 1) => 0x800000,
                (0, 2) => 0xff800000,
                (1, 1) => 0x80000000000000,
                _ => 0xff80000000000000,
            };
            let word = 0x6b09215f | (sf << 31) | (shift << 22);
            execute(&mut m, Instruction(word)).unwrap();
            assert_eq!(m.cores.active().state.nzcv, 6, "sf={sf} shift={shift}");
        }
    }
}

#[test]
fn shifted_add_sub_keep_reserved_forms_undefined() {
    for word in [0x6bc9215f, 0x6b49815f] {
        // ROR and W-register shift >= 32
        let mut m = Machine::new(MachineConfig {
            trace: false,
            quantum: 1,
        });
        assert!(matches!(
            execute(&mut m, Instruction(word)),
            Err(ExecError::Undefined)
        ));
    }
}
