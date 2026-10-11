use iphem_arm::step;
use iphem_core::{BusRegion, CpuRunState, Machine, MachineConfig, Rom};

const WFI: u32 = 0xd503207f;
const WFE: u32 = 0xd503205f;

fn machine_at(pc: u64, word: u32, masked: bool) -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(Rom::new(0x1000, word.to_le_bytes().to_vec())),
        })
        .unwrap();
    m.cores.active_mut().state.pc = pc;
    m.cores.active_mut().state.daif = if masked { 0b0010 } else { 0 };
    m
}

#[test]
fn wfi_parks_core_when_unmasked() {
    let mut m = machine_at(0x1000, WFI, false);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().run_state, CpuRunState::WaitingForInterrupt);
    assert_eq!(m.cores.active().state.pc, 0x1004);

    // While parked no instruction executes.
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1004);

    // Wake it (as tick_devices would) and ensure IRQ is taken on next step.
    m.cores.active_mut().pending_irq = Some(7);
    m.cores.active_mut().run_state = CpuRunState::Running;
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x280); // current EL using SP_EL1
    assert_eq!(m.cores.active().state.el as u8, 1);
    assert!(
        m.cores.active().state.daif & 0b0010 != 0,
        "IRQ must be masked on entry"
    );
}

#[test]
fn wfi_with_masked_irq_is_nop() {
    let mut m = machine_at(0x1000, WFI, true);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().run_state, CpuRunState::Running);
    assert_eq!(m.cores.active().state.pc, 0x1004);
}

#[test]
fn hcr_twi_and_twe_trap_guest_waits_to_el2() {
    for (word, control, iss) in [(WFI, 1u64 << 13, 0), (WFE, 1u64 << 14, 1)] {
        let mut m = machine_at(0x1000, word, false);
        m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
        m.cores.active_mut().state.currentel = 1;
        m.cores.active_mut().state.spsel = true;
        m.cores.active_mut().state.vbar_el2 = 0x20_000;
        m.cores.active_mut().state.hcr_el2 = control;

        assert_eq!(step(&mut m), Ok(()));
        assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
        assert_eq!(m.cores.active().state.pc, 0x20_400);
        assert_eq!(m.cores.active().state.elr_el2, 0x1004);
        assert_eq!(m.cores.active().state.esr_el2, (0x01u64 << 26) | iss);
        assert_eq!(m.cores.active().run_state, CpuRunState::Running);
    }
}

#[test]
fn wfe_parks_core_when_unmasked() {
    let mut m = machine_at(0x1000, WFE, false);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().run_state, CpuRunState::WaitingForInterrupt);
}

#[test]
fn masked_irq_is_not_consumed() {
    let mut m = machine_at(0x1000, 0xd503201f, true); // NOP with I bit set
    m.cores.active_mut().pending_irq = Some(9);
    step(&mut m).unwrap();
    // Masked: the NOP ran, IRQ stays pending.
    assert_eq!(m.cores.active().state.pc, 0x1004);
    assert_eq!(m.cores.active().pending_irq, Some(9));

    // Clear the I mask: next step takes the IRQ.
    m.cores.active_mut().state.daif &= !0b0010;
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x280);
    assert_eq!(m.cores.active().pending_irq, None);
}

#[test]
fn unmasked_irq_is_taken() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().pending_irq = Some(5);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x280);
    assert_eq!(m.cores.active().pending_irq, None);
}

#[test]
fn irq_at_el2_uses_el2_exception_bank_and_vector() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El2;
    m.cores.active_mut().state.currentel = 2;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.vbar_el2 = 0x20_000;
    m.cores.active_mut().state.sp = 0x8200;
    m.cores.active_mut().state.sp_el[2] = 0x8200;
    m.cores.active_mut().pending_irq = Some(11);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x20_280);
    assert_eq!(m.cores.active().state.elr_el2, 0x1000);
    assert_eq!(m.cores.active().state.spsr_el2 & 0xf, 0x9); // EL2h
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().pending_irq, None);
}

#[test]
fn fiq_uses_f_mask_and_distinct_vector_slot() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.vbar_el1 = 0x20_000;
    m.cores.active_mut().state.daif = 0b0010; // IRQ masked, FIQ unmasked
    m.cores.active_mut().pending_irq = Some(5);
    m.cores.active_mut().pending_fiq = Some(6);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x20_300);
    assert_eq!(m.cores.active().pending_fiq, None);
    assert_eq!(m.cores.active().pending_irq, Some(5));
}

#[test]
fn masked_fiq_remains_pending() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.daif = 0b0001;
    m.cores.active_mut().pending_fiq = Some(6);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1004);
    assert_eq!(m.cores.active().pending_fiq, Some(6));
}

#[test]
fn fiq_routes_to_el2_and_el3_using_fmo_and_fiq_controls() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.vbar_el2 = 0x20_000;
    m.cores.active_mut().state.vbar_el3 = 0x30_000;
    m.cores.active_mut().state.hcr_el2 = 1 << 3; // FMO
    m.cores.active_mut().pending_fiq = Some(6);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x20_500);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x1100;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.scr_el3 = 1 << 2; // FIQ
    m.cores.active_mut().pending_fiq = Some(7);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El3);
    assert_eq!(m.cores.active().state.pc, 0x30_500);
}

#[test]
fn mapped_aic_timer_fiq_wakes_wfe_core_into_fiq_vector() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(Rom::new(0x1000, WFE.to_le_bytes().to_vec())),
        })
        .unwrap();
    // T8020 AIC window (d321ap aic-version 02): the same wiring the direct
    // kernel runner now maps so XNU has an interrupt controller to acknowledge.
    m.bus
        .map(BusRegion {
            base: 0x23b100000,
            size: 0x30000,
            device: Box::new(iphem_devices::Aic::new(3, 128)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x1000;
    m.cores.active_mut().state.daif = 0; // FIQ unmasked
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;
    m.cores.active_mut().state.sp = 0x8200;
    m.cores.active_mut().state.sp_el[1] = 0x8200;
    m.cores.active_mut().state.cntp_ctl_el0 = 1; // physical timer enabled, not masked
    m.cores.active_mut().state.cntp_cval_el0 = 1; // expires once the counter reaches 1

    // The kernel's cpu-idle self-loop parks here (the real 16A366 kernel:
    // wfe/b -4 at 0xfffffff007e9162c, docs/REAL_KERNEL_PROGRESS.md).
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().run_state, CpuRunState::WaitingForInterrupt);
    assert!(
        matches!(m.cores.active().state.pc, 0x1000 | 0x1004),
        "parked pc = {:#x}",
        m.cores.active().state.pc
    );

    // Architectural timer expiry propagates through the mapped AIC's timer
    // FIQ line and wakes the parked core (pending_fiq = line 0).
    m.tick_devices(1); // counter 1: compare true, FIQ level asserted into AIC
    m.tick_devices(1); // AIC tick delivers FiqTo(0, core 0) to the parked core
    assert_eq!(m.cores.active().run_state, CpuRunState::Running);
    assert_eq!(m.cores.active().pending_fiq, Some(0));

    // The next step enters the FIQ vector (vbar_el1 + 0x300, current EL with
    // SP_EL1) instead of continuing the idle loop.
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x20_300);
    assert_eq!(m.cores.active().pending_fiq, None);
    assert!(
        m.cores.active().state.daif & 0b0001 != 0,
        "FIQ masked on entry"
    );
}

#[test]
fn canonical_interactive_step_advances_device_time_once_before_instruction() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    iphem_arm::step_with_device_tick(&mut m).unwrap();
    assert_eq!(m.scheduler.cycle(), 1);
    assert_eq!(m.trace.cycle(), 1);
    assert_eq!(m.cores.active().instructions, 1);
    assert_eq!(m.cores.active().state.pc, 0x1004);
}

#[test]
fn serror_uses_a_mask_vector_and_syndrome() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.vbar_el1 = 0x20_000;
    m.cores.active_mut().pending_serror = Some(0x155);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x20_380);
    assert_eq!(m.cores.active().state.esr_el1, (0x2fu64 << 26) | 0x155);
    assert_eq!(m.cores.active().state.elr_el1, 0x1000);
    assert_eq!(m.cores.active().pending_serror, None);

    m.cores.active_mut().state.daif = 0b0100;
    m.cores.active_mut().state.pc = 0x1000;
    m.cores.active_mut().pending_serror = Some(1);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1004);
    assert_eq!(m.cores.active().pending_serror, Some(1));
}

#[test]
fn serror_routes_using_amo_and_ea_controls() {
    let mut m = machine_at(0x1000, 0xd503201f, false);
    m.cores.active_mut().state.vbar_el2 = 0x20_000;
    m.cores.active_mut().state.vbar_el3 = 0x30_000;
    m.cores.active_mut().state.hcr_el2 = 1 << 5; // AMO
    m.cores.active_mut().pending_serror = Some(2);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x20_580);
    assert_eq!(m.cores.active().state.esr_el2, (0x2fu64 << 26) | 2);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x1100;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.scr_el3 = 1 << 3; // EA
    m.cores.active_mut().pending_serror = Some(3);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El3);
    assert_eq!(m.cores.active().state.pc, 0x30_580);
    assert_eq!(m.cores.active().state.esr_el3, (0x2fu64 << 26) | 3);
}

#[test]
fn preemption_fires_on_active_core() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.enable_preemption(29, 8);

    // 8 ticks puts the scheduler at cycle 8; the preemption fires on the 9th
    // tick_devices call (checked before the sweep).
    for _ in 0..8 {
        m.tick_devices(1);
        assert_eq!(
            m.cores[0].pending_irq, None,
            "preemption must not fire early"
        );
    }
    m.tick_devices(1);
    assert_eq!(m.cores[0].pending_irq, Some(29));

    // Second interval boundary (next_preempt 16) fires during the next 9 ticks.
    for _ in 0..8 {
        m.tick_devices(1);
    }
    m.tick_devices(1);
    assert_eq!(m.cores[0].pending_irq, Some(29));
}
