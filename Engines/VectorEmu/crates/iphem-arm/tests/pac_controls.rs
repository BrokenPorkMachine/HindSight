use iphem_arm::{execute, Instruction};
use iphem_core::{cpu::ExceptionLevel, Machine, MachineConfig};

fn machine(el: ExceptionLevel) -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = el;
    m.cores.active_mut().state.pc = 0x1000;
    m.cores.active_mut().state.sp = 0x8000;
    m
}

#[test]
fn disabled_pac_works_before_translation_and_key_initialization() {
    for el in [
        ExceptionLevel::El0,
        ExceptionLevel::El1,
        ExceptionLevel::El2,
        ExceptionLevel::El3,
    ] {
        for word in [
            0xdac10020, 0xdac11020, 0xd503233f, 0xd503237f, 0xd50323bf, 0xd50323ff,
        ] {
            let mut m = machine(el);
            m.cores.active_mut().state.x[0] = 0x123456789abcdef0;
            m.cores.active_mut().state.x[30] = 0x2000;
            execute(&mut m, Instruction(word)).unwrap();
            assert_eq!(m.cores.active().state.x[0], 0x123456789abcdef0);
            assert_eq!(m.cores.active().state.x[30], 0x2000);
            assert_eq!(m.cores.active().state.pc, 0x1004);
        }
        for word in [0xd65f0bff, 0xd65f0fff] {
            let mut m = machine(el);
            m.cores.active_mut().state.x[30] = 0x2000;
            execute(&mut m, Instruction(word)).unwrap();
            assert_eq!(m.cores.active().state.pc, 0x2000);
        }
    }
}

#[test]
fn each_key_uses_its_own_enable_control() {
    for el in [ExceptionLevel::El0, ExceptionLevel::El1] {
        for (selected, bit) in [(0, 31), (1, 30), (2, 27), (3, 13)] {
            for key in 0..4 {
                let mut m = machine(el);
                m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
                m.cores.active_mut().state.sctlr_el1 = 1 << bit;
                m.cores.active_mut().state.x[0] = 0x12345678;
                m.cores.active_mut().state.x[1] = 0x87654321;
                execute(&mut m, Instruction(0xdac10020 | (key << 10))).unwrap();
                assert_eq!(m.cores.active().state.x[0] != 0x12345678, key == selected);
            }
        }
    }
}

#[test]
fn higher_el_disable_controls_do_not_inherit_el1() {
    for el in [ExceptionLevel::El2, ExceptionLevel::El3] {
        let mut m = machine(el);
        m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30);
        m.cores.active_mut().state.x[30] = 0x2000;
        execute(&mut m, Instruction(0xd503237f)).unwrap(); // PACIBSP disabled
        execute(&mut m, Instruction(0xd65f0fff)).unwrap(); // RETAB still returns
        assert_eq!(m.cores.active().state.pc, 0x2000);
    }
}

#[test]
fn disabled_pac_keeps_exception_return_semantics() {
    let mut m = machine(ExceptionLevel::El3);
    m.cores.active_mut().state.sctlr_el1 = 1 << 31;
    m.cores.active_mut().state.elr_el3 = 0x2000;
    m.cores.active_mut().state.spsr_el3 = 0x3c5;
    execute(&mut m, Instruction(0xd69f0bff)).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x2000);
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El1);
}

#[test]
fn register_sp_and_zero_forms_use_distinct_modifiers() {
    let mut m = machine(ExceptionLevel::El1);
    m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
    m.cores.active_mut().state.sctlr_el1 = 1 << 31;
    m.cores.active_mut().state.x[0] = 0x12345678;
    m.cores.active_mut().state.x[1] = 0x12345678;
    execute(&mut m, Instruction(0xdac103e0)).unwrap(); // PACIA x0, SP
    execute(&mut m, Instruction(0xdac123e1)).unwrap(); // PACIZA x1
    assert_ne!(m.cores.active().state.x[0], m.cores.active().state.x[1]);
    execute(&mut m, Instruction(0xdac113e0)).unwrap(); // AUTIA x0, SP
    execute(&mut m, Instruction(0xdac133e1)).unwrap(); // AUTIZA x1
    assert_eq!(m.cores.active().state.x[0], 0x12345678);
    assert_eq!(m.cores.active().state.x[1], 0x12345678);
    assert!(execute(&mut m, Instruction(0x5ac10020)).is_err()); // reserved W form
    assert!(execute(&mut m, Instruction(0xdac12020)).is_err()); // Z form with Rn != 31
}
