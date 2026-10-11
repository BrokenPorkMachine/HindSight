use crate::{
    cpu::{Cpu, CpuState, ExceptionLevel},
    trace::{TraceEngine, TraceKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionClass {
    UnknownInstruction,
    Svc,
    DataAbort,
    InstructionAbort,
    Interrupt,
}

pub fn saved_pstate(state: &CpuState) -> u64 {
    let mode = ((state.el as u64) << 2) | u64::from(state.el != ExceptionLevel::El0 && state.spsel);
    (state.pstate & !0x3cf) | ((state.daif as u64) << 6) | mode
}

pub fn take_sync(
    cpu: &mut Cpu,
    trace: &mut TraceEngine,
    target: ExceptionLevel,
    esr: u64,
    far: u64,
    return_address: u64,
) {
    // Architected: taking an exception puts the local exclusive monitor back
    // into the Open Access state (ARM B2.3.7), so a Load/Store-Exclusive pair
    // split by any exception entry fails its store and retries.
    cpu.exclusive_address = None;
    cpu.exclusive_size = 0;
    let state = &mut cpu.state;
    assert_ne!(target, ExceptionLevel::El0);
    let source_el = state.el;
    let source_spsel = state.spsel;
    let saved = saved_pstate(state);
    let vbar = match target {
        ExceptionLevel::El1 => {
            state.elr_el1 = return_address;
            state.esr_el1 = esr;
            state.far_el1 = far;
            state.spsr_el1 = saved;
            state.vbar_el1
        }
        ExceptionLevel::El2 => {
            state.elr_el2 = return_address;
            state.esr_el2 = esr;
            state.far_el2 = far;
            state.spsr_el2 = saved;
            state.vbar_el2
        }
        ExceptionLevel::El3 => {
            state.elr_el3 = return_address;
            state.esr_el3 = esr;
            state.far_el3 = far;
            state.spsr_el3 = saved;
            state.vbar_el3
        }
        ExceptionLevel::El0 => unreachable!(),
    };
    let source_bank = if source_spsel { source_el as usize } else { 0 };
    state.sp_el[source_bank] = state.sp;
    state.el = target;
    state.currentel = target as u8;
    state.spsel = true;
    let target_bank = target as usize;
    if state.sp_el[target_bank] == 0 {
        state.sp_el[target_bank] = 0x19c20000;
    }
    state.sp = state.sp_el[target_bank];
    state.pstate = (state.pstate & !0xf) | ((target as u64) << 2) | 1;
    state.daif |= 0b1111;
    let offset = if source_el == target {
        if source_spsel {
            0x200
        } else {
            0
        }
    } else {
        0x400
    };
    state.pc = vbar + offset;
    trace.record(
        state.pc,
        esr,
        0,
        TraceKind::Exception,
        state.pc,
        state.el as u8,
        state.pstate,
        None,
    );
}

pub fn take_sync_el1(
    cpu: &mut Cpu,
    trace: &mut TraceEngine,
    _class: ExceptionClass,
    esr: u64,
    far: u64,
) {
    let return_address = cpu.state.pc;
    take_sync(cpu, trace, ExceptionLevel::El1, esr, far, return_address);
}

pub fn take_irq_el1(cpu: &mut Cpu, trace: &mut TraceEngine) {
    let return_address = cpu.state.pc;
    take_sync(cpu, trace, ExceptionLevel::El1, 0, 0, return_address);
}
