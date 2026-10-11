use iphem_core::{
    Bus, BusRegion, CpuState, Machine, MachineConfig, MemTransaction, Mmu, MmuFault, Ram,
    SecurityState, TableWalkFaultKind, TraceEngine, TransactionKind,
};

const ROOT: u64 = 0x1000;
const L2: u64 = 0x2000;
const L3: u64 = 0x3000;
const S1_ROOT: u64 = 0x5000;
const S1_L1: u64 = 0x6000;
const S1_L2: u64 = 0x7000;
const S1_L3: u64 = 0x9000;
const IPA: u64 = 0x1234;
const PA: u64 = 0x8000;
const AF: u64 = 1 << 10;
const S2_READ: u64 = 1 << 6;
const S2_WRITE: u64 = 1 << 7;
const XN: u64 = 1 << 54;
const DBM: u64 = 1 << 51;
const VTCR_4K_L1_39BIT: u64 = 25 | (1 << 6);

fn write_u64(bus: &mut Bus, trace: &mut TraceEngine, address: u64, value: u64) {
    bus.write(
        MemTransaction {
            address,
            size: 8,
            kind: TransactionKind::Write,
            security: SecurityState::Secure,
            source: None,
        },
        value,
        trace,
        0,
        2,
        0,
    )
    .unwrap();
}

fn read_u64(bus: &mut Bus, trace: &mut TraceEngine, address: u64) -> u64 {
    bus.read(
        MemTransaction {
            address,
            size: 8,
            kind: TransactionKind::Read,
            security: SecurityState::Secure,
            source: None,
        },
        trace,
        0,
        2,
        0,
    )
    .unwrap()
}

fn mapping(attributes: u64) -> (CpuState, Bus, TraceEngine) {
    let cpu = CpuState {
        hcr_el2: 1,
        vtcr_el2: VTCR_4K_L1_39BIT,
        vttbr_el2: ROOT,
        ..CpuState::default()
    };
    let mut bus = Bus::new();
    bus.map(BusRegion {
        base: 0,
        size: 0x10_000,
        device: Box::new(Ram::new(0, 0x10_000)),
    })
    .unwrap();
    let mut trace = TraceEngine::new(false);
    write_u64(&mut bus, &mut trace, ROOT, L2 | 3);
    write_u64(&mut bus, &mut trace, L2, L3 | 3);
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((IPA >> 12) & 0x1ff) * 8,
        PA | 3 | attributes,
    );
    (cpu, bus, trace)
}

fn add_identity_stage1(cpu: &mut CpuState, bus: &mut Bus, trace: &mut TraceEngine) {
    cpu.sctlr_el1 = 1;
    cpu.ttbr0_el1 = S1_ROOT;
    cpu.tcr_el1 = 16 | (16 << 16) | (2 << 30);
    write_u64(bus, trace, S1_ROOT, S1_L1 | 3);
    write_u64(bus, trace, S1_L1, S1_L2 | 3);
    write_u64(bus, trace, S1_L2, S1_L3 | 3);
    write_u64(
        bus,
        trace,
        S1_L3 + ((IPA >> 12) & 0x1ff) * 8,
        (IPA & !0xfff) | AF | 3,
    );
    // Guest Stage-1 table walks are themselves translated by Stage-2.
    for table_page in [S1_ROOT, S1_L1, S1_L2, S1_L3] {
        write_u64(
            bus,
            trace,
            L3 + ((table_page >> 12) & 0x1ff) * 8,
            table_page | AF | S2_READ | S2_WRITE | 3,
        );
    }
}

#[test]
fn stage2_requires_access_flag_and_read_permission() {
    let (cpu, mut bus, mut trace) = mapping(S2_READ);
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::AccessFlag { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF);
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::Permission { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    assert_eq!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false).unwrap(),
        PA + (IPA & 0xfff)
    );
}

#[test]
fn vtcr_ha_sets_a_clear_stage2_access_flag() {
    let (mut cpu, mut bus, mut trace) = mapping(S2_READ);
    cpu.vtcr_el2 |= 1 << 39; // HA
    assert_eq!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false).unwrap(),
        PA + (IPA & 0xfff)
    );
    let leaf = L3 + ((IPA >> 12) & 0x1ff) * 8;
    assert_ne!(read_u64(&mut bus, &mut trace, leaf) & AF, 0);
}

#[test]
fn vtcr_hd_promotes_a_dbm_stage2_leaf_on_write() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | DBM);
    cpu.vtcr_el2 |= (1 << 39) | (1 << 40); // HA | HD
    assert_eq!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, true).unwrap(),
        PA + (IPA & 0xfff)
    );
    let leaf = L3 + ((IPA >> 12) & 0x1ff) * 8;
    let descriptor = read_u64(&mut bus, &mut trace, leaf);
    assert_ne!(descriptor & S2_WRITE, 0);
    assert_ne!(descriptor & DBM, 0);

    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | DBM);
    cpu.vtcr_el2 |= 1 << 39; // HA without HD
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, true),
        Err(MmuFault::Permission { stage2: true, .. })
    ));
}

#[test]
fn machine_rewalks_a_cached_clean_stage2_leaf_for_dirty_update() {
    let (mut cpu, bus, _) = mapping(AF | S2_READ | DBM);
    cpu.vtcr_el2 |= (1 << 39) | (1 << 40); // HA | HD
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;

    assert_eq!(
        machine.translate(IPA, false, false).unwrap(),
        PA + (IPA & 0xfff)
    );
    assert!(machine.tlb.lookup(IPA, 0, 0, false, true, false).is_none());
    assert_eq!(
        machine.translate(IPA, true, false).unwrap(),
        PA + (IPA & 0xfff)
    );
    assert!(machine.tlb.lookup(IPA, 0, 0, false, true, false).is_some());
}

#[test]
fn stage2_rejects_inconsistent_geometry_and_out_of_range_ipa() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    cpu.vtcr_el2 = 0; // SL0=L2 cannot describe a 64-bit IPA regime.
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::AddressSize { stage2: true, .. })
    ));

    cpu.vtcr_el2 = VTCR_4K_L1_39BIT;
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, 1 << 40, false),
        Err(MmuFault::AddressSize { stage2: true, .. })
    ));
}

#[test]
fn vtcr_ps_limits_table_and_output_physical_addresses() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    cpu.vttbr_el2 = 1 << 32;
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::AddressSize { stage2: true, .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    let high_pa = 1u64 << 32;
    let leaf = L3 + ((IPA >> 12) & 0x1ff) * 8;
    write_u64(&mut bus, &mut trace, leaf, high_pa | AF | S2_READ | 3);
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::AddressSize { stage2: true, .. })
    ));
    cpu.vtcr_el2 |= 2 << 16; // PS=40 bits
    assert_eq!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false).unwrap(),
        high_pa + (IPA & 0xfff)
    );
}

#[test]
fn stage2_distinguishes_read_and_write_permission() {
    let (cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, true),
        Err(MmuFault::Permission { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF | S2_WRITE);
    assert!(Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, true).is_ok());
    assert!(matches!(
        Mmu::stage2_walk(&cpu, &mut bus, &mut trace, IPA, false),
        Err(MmuFault::Permission { .. })
    ));
}

#[test]
fn combined_walk_enforces_stage2_execute_never() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | XN);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, true),
        Err(MmuFault::Permission { .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, true).is_ok());
}

#[test]
fn stage2_remains_active_when_guest_stage1_is_disabled() {
    let (cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    assert_eq!(cpu.sctlr_el1 & 1, 0);
    assert_eq!(
        Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, false)
            .unwrap()
            .pa,
        PA + (IPA & 0xfff)
    );

    let (cpu, mut bus, mut trace) = mapping(AF | S2_READ | XN);
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, true),
        Err(MmuFault::Permission { .. })
    ));
}

#[test]
fn machine_access_does_not_bypass_stage2_when_stage1_is_disabled() {
    let (cpu, bus, _) = mapping(AF | S2_READ | S2_WRITE);
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert_eq!(machine.translate(IPA, false, false).unwrap(), PA + 0x234);
}

#[test]
fn stage2_permission_abort_routes_guest_to_el2_with_level() {
    let (cpu, bus, _) = mapping(AF | S2_READ);
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.cores.active_mut().state.vbar_el2 = 0x20_000;
    machine.bus = bus;

    assert!(machine.translate(IPA, true, false).is_err());
    assert_eq!(
        machine.cores.active().state.el,
        iphem_core::ExceptionLevel::El2
    );
    assert_eq!(machine.cores.active().state.far_el2, IPA);
    assert_eq!(machine.cores.active().state.hpfar_el2, (IPA & !0xfff) >> 8);
    assert_eq!(machine.cores.active().state.esr_el2 >> 26, 0x24);
    assert_eq!(machine.cores.active().state.esr_el2 & 0x3f, 0x0f);
    assert_ne!(machine.cores.active().state.esr_el2 & (1 << 6), 0);
    assert_eq!(machine.cores.active().state.pc, 0x20_400);
}

#[test]
fn hpfar_uses_translated_ipa_not_original_va() {
    const NONIDENTITY_IPA: u64 = 0xe234;
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    write_u64(
        &mut bus,
        &mut trace,
        S1_L3 + ((IPA >> 12) & 0x1ff) * 8,
        (NONIDENTITY_IPA & !0xfff) | AF | 3,
    );
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((NONIDENTITY_IPA >> 12) & 0x1ff) * 8,
        PA | AF | S2_READ | 3,
    );

    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert!(machine.translate(IPA, true, false).is_err());
    assert_eq!(machine.cores.active().state.far_el2, IPA);
    assert_eq!(
        machine.cores.active().state.hpfar_el2,
        (NONIDENTITY_IPA & !0xfff) >> 8
    );
}

#[test]
fn stage1_table_walk_uses_stage2_relocated_table_page() {
    const RELOCATED_ROOT_PA: u64 = 0xf000;
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    write_u64(&mut bus, &mut trace, RELOCATED_ROOT_PA, S1_L1 | 3);
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((S1_ROOT >> 12) & 0x1ff) * 8,
        RELOCATED_ROOT_PA | AF | S2_READ | S2_WRITE | 3,
    );
    // If the Stage-1 walker incorrectly reads the root IPA directly, this
    // invalid descriptor makes the access fail.
    write_u64(&mut bus, &mut trace, S1_ROOT, 0);

    let translation = Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, false).unwrap();
    assert_eq!(translation.pa, PA + (IPA & 0xfff));
}

#[test]
fn stage2_fault_during_stage1_walk_sets_s1ptw() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    cpu.sctlr_el1 = 1;
    cpu.ttbr0_el1 = S1_ROOT;
    cpu.tcr_el1 = 16 | (16 << 16) | (2 << 30);
    // Populate guest tables physically but deliberately leave their IPAs
    // unmapped at Stage-2.
    write_u64(&mut bus, &mut trace, S1_ROOT, S1_L1 | 3);

    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.cores.active_mut().state.vbar_el2 = 0x20_000;
    machine.bus = bus;
    assert!(machine.translate(IPA, false, false).is_err());
    assert_eq!(
        machine.cores.active().state.el,
        iphem_core::ExceptionLevel::El2
    );
    assert_ne!(machine.cores.active().state.esr_el2 & (1 << 7), 0);
    assert_eq!(machine.cores.active().state.esr_el2 & 0x3f, 0x07);
    assert_eq!(machine.cores.active().state.far_el2, IPA);
    assert_eq!(machine.cores.active().state.hpfar_el2, S1_ROOT >> 8);
}

#[test]
fn stage1_hardware_af_update_requires_stage2_write_permission() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    cpu.tcr_el1 |= 1 << 39; // HA
    let leaf = S1_L3 + ((IPA >> 12) & 0x1ff) * 8;
    write_u64(&mut bus, &mut trace, leaf, (IPA & !0xfff) | 3); // AF clear
                                                               // Reading the Stage-1 leaf is allowed, but its hardware AF writeback is not.
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((S1_L3 >> 12) & 0x1ff) * 8,
        S1_L3 | AF | S2_READ | 3,
    );

    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, IPA, false, false),
        Err(MmuFault::Stage2TableWalk {
            kind: TableWalkFaultKind::Permission,
            ..
        })
    ));
}

#[test]
fn stage1_hardware_dirty_update_requires_stage2_write_permission() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    cpu.tcr_el1 |= (1 << 39) | (1 << 40); // HA | HD
    let leaf = S1_L3 + ((IPA >> 12) & 0x1ff) * 8;
    write_u64(
        &mut bus,
        &mut trace,
        leaf,
        (IPA & !0xfff) | AF | (2 << 6) | DBM | 3,
    );
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((S1_L3 >> 12) & 0x1ff) * 8,
        S1_L3 | AF | S2_READ | 3,
    );

    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, IPA, true, false),
        Err(MmuFault::Stage2TableWalk {
            kind: TableWalkFaultKind::Permission,
            ..
        })
    ));
}

#[test]
fn tlb_does_not_upgrade_stage2_read_only_or_xn_permissions() {
    let (cpu, bus, _) = mapping(AF | S2_READ);
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert!(machine.translate(IPA, false, false).is_ok());
    assert!(machine.translate(IPA, true, false).is_err());

    let (cpu, bus, _) = mapping(AF | S2_READ | XN);
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert!(machine.translate(IPA, false, false).is_ok());
    assert!(machine.translate(IPA, false, true).is_err());
}

#[test]
fn tlb_does_not_reuse_privileged_entry_at_el0() {
    let (mut cpu, bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    let mut bus = bus;
    add_identity_stage1(&mut cpu, &mut bus, &mut trace);
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert!(machine.translate(IPA, false, false).is_ok());

    machine.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    machine.cores.active_mut().state.currentel = 0;
    assert!(machine.translate(IPA, false, false).is_err());
    assert_eq!(
        machine.cores.active().state.el,
        iphem_core::ExceptionLevel::El1
    );
}

#[test]
fn tlb_does_not_reuse_stage2_entry_across_vmids() {
    const ROOT_2: u64 = 0xa000;
    const L2_2: u64 = 0xb000;
    const L3_2: u64 = 0xc000;
    const PA_2: u64 = 0xd000;

    let (cpu, mut bus, mut trace) = mapping(AF | S2_READ | S2_WRITE);
    write_u64(&mut bus, &mut trace, ROOT_2, L2_2 | 3);
    write_u64(&mut bus, &mut trace, L2_2, L3_2 | 3);
    write_u64(
        &mut bus,
        &mut trace,
        L3_2 + ((IPA >> 12) & 0x1ff) * 8,
        PA_2 | 3 | AF | S2_READ | S2_WRITE,
    );

    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    assert_eq!(machine.translate(IPA, false, false).unwrap(), PA + 0x234);

    machine.cores.active_mut().state.vttbr_el2 = (1 << 48) | ROOT_2;
    assert_eq!(machine.translate(IPA, false, false).unwrap(), PA_2 + 0x234);
}
