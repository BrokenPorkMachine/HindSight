use iphem_arm::{execute, Instruction};
use iphem_core::{BusRegion, ExceptionLevel, Machine, MachineConfig, SparseRam};

fn machine(leaf: u64) -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0,
            size: 0x20000,
            device: Box::new(SparseRam::new(0, 0x20000)),
        })
        .unwrap();
    // 39-bit VA, 4 KiB granule, L1 -> L2 -> L3; VA 0x5234 -> PA 0x9234.
    for (pa, value) in [(0x1000, 0x2003), (0x2000, 0x3003), (0x3028, leaf)] {
        m.load_physical_bytes(pa, &value.to_le_bytes()).unwrap();
    }
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.sctlr_el1 = 1;
    m.cores.active_mut().state.tcr_el1 = 25;
    m.cores.active_mut().state.ttbr0_el1 = 0x1000;
    m.cores.active_mut().state.mair_el1 = 0xff;
    m.cores.active_mut().state.x[0] = 0x5234;
    m
}

fn at(m: &mut Machine, op2: u32) -> u64 {
    execute(m, Instruction(0xd5087800 | (op2 << 5))).unwrap();
    execute(m, Instruction(0xd5387401)).unwrap(); // MRS x1, PAR_EL1
    m.cores.active().state.x[1]
}

#[test]
fn translation_reports_real_pa_attributes_and_preserves_context() {
    let mut m = machine(0x9003 | (1 << 10) | (3 << 8));
    m.cores.active_mut().state.esr_el1 = 0x1234;
    m.cores.active_mut().state.far_el1 = 0x5678;
    assert_eq!(at(&mut m, 0), 0xff00_0000_0000_9980);
    assert_eq!(m.cores.active().state.x[0], 0x5234);
    assert_eq!(m.cores.active().state.esr_el1, 0x1234);
    assert_eq!(m.cores.active().state.far_el1, 0x5678);
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El1);
    // XNU reconstructs the low VA bits from the operand.
    assert_eq!(
        (m.cores.active().state.par_el1 & 0x0000_ffff_ffff_f000) | 0x234,
        0x9234
    );
}

#[test]
fn permission_and_translation_faults_update_par_without_aborting() {
    let mut m = machine(0x9003 | (1 << 10) | (2 << 6)); // EL1 RO, no EL0
    assert_eq!(at(&mut m, 0) & 1, 0);
    assert_eq!(at(&mut m, 1) & 0x7f, 1 | (0x0f << 1));
    assert_eq!(at(&mut m, 2) & 0x7f, 1 | (0x0f << 1));
    assert_eq!(m.cores.active().state.esr_el1, 0);
    let mut m = machine(0);
    assert_eq!(at(&mut m, 0) & 0x7f, 1 | (7 << 1));
    let mut m = machine(0x9003); // AF clear
    assert_eq!(at(&mut m, 0) & 0x7f, 1 | (11 << 1));
}

#[test]
fn ordinary_at_ignores_pan_and_el0_query_checks_user_permission() {
    let mut m = machine(0x9003 | (1 << 10) | (1 << 6));
    m.cores.active_mut().state.pstate |= 1 << 22;
    for op2 in 0..4 {
        assert_eq!(at(&mut m, op2) & 1, 0);
    }
    assert_ne!(m.cores.active().state.pstate & (1 << 22), 0);
}

#[test]
fn mmu_off_stage1_query_does_not_apply_final_stage2() {
    let mut m = machine(0);
    m.cores.active_mut().state.sctlr_el1 = 0;
    m.cores.active_mut().state.hcr_el2 = 1; // invalid Stage-2 tables must not affect the IPA result
    assert_eq!(at(&mut m, 0) & 0x0000_ffff_ffff_f001, 0x5000);
}

#[test]
fn par_supports_explicit_save_restore_and_at_requires_privilege() {
    let mut m = machine(0);
    m.cores.active_mut().state.x[0] = 0x1234;
    execute(&mut m, Instruction(0xd5187400)).unwrap(); // MSR PAR_EL1, x0
    execute(&mut m, Instruction(0xd5387401)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 0x1234);
    m.cores.active_mut().state.el = ExceptionLevel::El0;
    assert!(execute(&mut m, Instruction(0xd5087800)).is_err());
    assert_eq!(m.cores.active().state.par_el1, 0x1234);
}

#[test]
fn upper_half_16k_query_returns_the_correct_4k_par_page() {
    let mut m = machine(0);
    for (pa, value) in [(0x4038, 0x8003u64), (0x8000, 0xc003), (0xc008, 0x104403)] {
        m.load_physical_bytes(pa, &value.to_le_bytes()).unwrap();
    }
    m.cores.active_mut().state.tcr_el1 = 25 | (25 << 16) | (1 << 30);
    m.cores.active_mut().state.ttbr1_el1 = 0x4000;
    m.cores.active_mut().state.x[0] = 0xfffffff000005234;
    assert_eq!(at(&mut m, 0) & 0x0000_ffff_ffff_f001, 0x105000);
}

#[test]
fn stage2_table_walk_fault_is_reported_in_par() {
    let mut m = machine(0x9403);
    m.cores.active_mut().state.hcr_el2 = 1;
    m.cores.active_mut().state.vtcr_el2 = 25 | (1 << 6); // 39-bit IPA, start at L1
    m.cores.active_mut().state.vttbr_el2 = 0x10000; // empty table
    assert_eq!(at(&mut m, 0) & 0x37f, 0x300 | 1 | (5 << 1));
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El1);
    assert_eq!(m.cores.active().state.esr_el2, 0);
}

#[test]
fn hcr_at_traps_before_modifying_par() {
    let mut m = machine(0x9403);
    m.cores.active_mut().state.hcr_el2 = 1 << 44;
    m.cores.active_mut().state.par_el1 = 0x1234;
    assert!(execute(&mut m, Instruction(0xd5087800)).is_err());
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0x18);
    assert_eq!(m.cores.active().state.par_el1, 0x1234);
}
