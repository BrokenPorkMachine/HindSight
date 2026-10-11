//! Operand-sensitive PAC branch regressions. Encodings cross-checked with
//! clang -target arm64-apple-macos -march=armv8.3-a.
use iphem_arm::{execute, Instruction};
use iphem_core::{cpu::ExceptionLevel, Machine, MachineConfig};

fn machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
    m.cores.active_mut().state.sctlr_el1 = (1 << 31) | (1 << 30);
    m.cores.active_mut().state.sp = 0x19c20000;
    m.cores.active_mut().state.apia_lo = 0x1122334455667788;
    m.cores.active_mut().state.apib_lo = 0x8877665544332211;
    m.cores.active_mut().state.pc = 0x1000;
    m
}

#[test]
fn authenticated_register_branches_use_target_then_modifier() {
    for (sign, branch, target_register, modifier_register, link) in [
        (0xdac100a7, 0xd71f08e5, 7, 5, false), // PACIA/BRAA x7,x5
        (0xdac10408, 0xd71f0d00, 8, 0, false), // PACIB/BRAB x8,x0
        (0xdac100a8, 0xd73f0905, 8, 5, true),  // PACIA/BLRAA x8,x5
        (0xdac10408, 0xd73f0d00, 8, 0, true),  // PACIB/BLRAB x8,x0
    ] {
        let mut m = machine();
        let target = 0xffff_fffe_4000_1234;
        m.cores.active_mut().state.x[target_register] = target;
        m.cores.active_mut().state.x[modifier_register] = 0xdeadbeef;
        m.cores.active_mut().state.x[30] = 0x5555;
        execute(&mut m, Instruction(sign)).unwrap();
        m.cores.active_mut().state.pc = 0x2000;
        execute(&mut m, Instruction(branch)).unwrap();
        assert_eq!(m.cores.active().state.pc, target);
        assert_eq!(m.cores.active().state.x[modifier_register], 0xdeadbeef);
        assert_eq!(
            m.cores.active().state.x[30],
            if link { 0x2004 } else { 0x5555 }
        );
    }
}

#[test]
fn zero_modifier_branches_authenticate_instead_of_plain_branching() {
    for (sign, branch) in [
        (0xdac123e7, 0xd61f08ff),
        (0xdac127e7, 0xd61f0cff),
        (0xdac123e7, 0xd63f08ff),
        (0xdac127e7, 0xd63f0cff),
    ] {
        let mut m = machine();
        let target = 0xffff_fffe_4000_1234;
        m.cores.active_mut().state.x[7] = target;
        execute(&mut m, Instruction(sign)).unwrap();
        let signed = m.cores.active().state.x[7];
        execute(&mut m, Instruction(branch)).unwrap();
        assert_eq!(m.cores.active().state.pc, target);
        m.cores.active_mut().state.x[7] = signed ^ (1 << 48);
        assert!(execute(&mut m, Instruction(branch)).is_err());
    }
}

#[test]
fn sp_modifier_is_distinct_from_zero_and_ret_selects_the_matching_key() {
    for (prologue, ret) in [(0xd503233f, 0xd65f0bff), (0xd503237f, 0xd65f0fff)] {
        let mut m = machine();
        let target = 0xffff_fffe_4000_1234;
        m.cores.active_mut().state.x[30] = target;
        execute(&mut m, Instruction(prologue)).unwrap();
        execute(&mut m, Instruction(ret)).unwrap();
        assert_eq!(m.cores.active().state.pc, target);
    }
    let mut m = machine();
    m.cores.active_mut().state.x[30] = 0xffff_fffe_4000_1234;
    execute(&mut m, Instruction(0xd503233f)).unwrap();
    m.cores.active_mut().state.x[7] = m.cores.active().state.x[30];
    execute(&mut m, Instruction(0xd71f08ff)).unwrap(); // BRAA x7,SP
    assert_eq!(m.cores.active().state.pc, 0xffff_fffe_4000_1234);
}
