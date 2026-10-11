use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{ExceptionLevel, Machine, MachineConfig};

fn machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0x1000;
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.x[5] = 0x0123_4567_89ab_cdef;
    m.cores.active_mut().state.x[6] = 0xfedc_ba98_7654_3210;
    // clang -target arm64-apple-macos -march=armv8.3-a
    execute(&mut m, Instruction(0xd5182305)).unwrap(); // MSR APGAKeyLo_EL1,X5
    execute(&mut m, Instruction(0xd5182326)).unwrap(); // MSR APGAKeyHi_EL1,X6
    m
}

#[test]
fn generic_code_writes_destination_and_depends_on_both_keys_and_operands() {
    let mut m = machine();
    m.cores.active_mut().state.x[1] = 0xabc;
    m.cores.active_mut().state.x[2] = 0x123;
    m.cores.active_mut().state.pstate = 0xa00003c5;
    let flags = m.cores.active().state.pstate;
    let pc = m.cores.active().state.pc;
    execute(&mut m, Instruction(0x9ac23020)).unwrap(); // PACGA X0,X1,X2
    let code = m.cores.active().state.x[0];
    assert_ne!(code, 0);
    assert_eq!(code & 0xffff_ffff, 0);
    assert_eq!(m.cores.active().state.pstate, flags);
    assert_eq!(m.cores.active().state.pc, pc + 4);
    for (register, changed) in [(1, 0xabd), (2, 0x124), (5, 0x111), (6, 0x222)] {
        let mut altered = machine();
        altered.cores.active_mut().state.x[1] = 0xabc;
        altered.cores.active_mut().state.x[2] = 0x123;
        altered.cores.active_mut().state.x[register] = changed;
        if register == 5 {
            execute(&mut altered, Instruction(0xd5182305)).unwrap();
        }
        if register == 6 {
            execute(&mut altered, Instruction(0xd5182326)).unwrap();
        }
        execute(&mut altered, Instruction(0x9ac23020)).unwrap();
        assert_ne!(altered.cores.active().state.x[0], code);
    }
    execute(&mut m, Instruction(0x9ac23020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], code);
    // Changing an instruction-address key does not change the generic code.
    m.cores.active_mut().state.apia_lo ^= u64::MAX;
    execute(&mut m, Instruction(0x9ac23020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], code);
}

#[test]
fn zero_and_stack_operands_and_disabled_sctlr() {
    let mut m = machine();
    m.cores.active_mut().state.sp = 0x456;
    m.cores.active_mut().state.x[1] = 0;
    m.cores.active_mut().state.x[2] = m.cores.active().state.sp;
    // All PAC enable controls are zero. Generic codes still execute.
    execute(&mut m, Instruction(0x9adf33e7)).unwrap(); // PACGA X7,XZR,SP
    execute(&mut m, Instruction(0x9ac23020)).unwrap();
    assert_ne!(m.cores.active().state.x[7], 0);
    assert_eq!(m.cores.active().state.x[7], m.cores.active().state.x[0]);
    let registers = m.cores.active().state.x;
    execute(&mut m, Instruction(0x9ac4307f)).unwrap(); // PACGA XZR,X3,X4
    assert_eq!(m.cores.active().state.x, registers);
    assert_eq!(m.cores.active().state.sp, 0x456);
    assert!(execute(&mut m, Instruction(0x1ac23020)).is_err()); // reserved W form
}

#[test]
fn generic_keys_round_trip_and_are_privileged() {
    let mut m = machine();
    execute(&mut m, Instruction(0xd5382308)).unwrap();
    execute(&mut m, Instruction(0xd5382329)).unwrap();
    assert_eq!(m.cores.active().state.x[8], m.cores.active().state.x[5]);
    assert_eq!(m.cores.active().state.x[9], m.cores.active().state.x[6]);
    m.cores.active_mut().state.el = ExceptionLevel::El0;
    assert!(execute(&mut m, Instruction(0xd5382308)).is_err());
    m.cores.active_mut().state.el = ExceptionLevel::El0;
    assert!(execute(&mut m, Instruction(0xd5182305)).is_err());
    // Unprivileged execution can use the key without reading its registers.
    m.cores.active_mut().state.el = ExceptionLevel::El0;
    m.cores.active_mut().state.x[1] = 0xabc;
    m.cores.active_mut().state.x[2] = 0x123;
    execute(&mut m, Instruction(0x9ac23020)).unwrap();
    assert_ne!(m.cores.active().state.x[0], 0);
}

#[test]
fn generic_disassembly_preserves_operand_roles() {
    assert_eq!(disassemble(0x1000, 0x9ac23020), "pacga\tx0, x1, x2");
    assert_eq!(disassemble(0x1000, 0x9adf33e7), "pacga\tx7, xzr, sp");
    assert_eq!(disassemble(0x1000, 0x9ac4307f), "pacga\txzr, x3, x4");
}
