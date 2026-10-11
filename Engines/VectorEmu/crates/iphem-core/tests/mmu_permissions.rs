use iphem_core::{
    Bus, BusRegion, CpuState, ExceptionLevel, Machine, MachineConfig, MemTransaction, Mmu,
    MmuFault, Ram, SecurityState, TraceEngine, TransactionKind,
};

const ROOT: u64 = 0x1000;
const L1: u64 = 0x2000;
const L2: u64 = 0x3000;
const L3: u64 = 0x4000;
const VA: u64 = 0x1234;
const PA_PAGE: u64 = 0x8000;
const AF: u64 = 1 << 10;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const DBM: u64 = 1 << 51;

#[test]
fn upper_39bit_16k_root_ignores_sign_extension() {
    let cpu = CpuState {
        el: ExceptionLevel::El1,
        sctlr_el1: 1,
        tcr_el1: 25 | (25 << 16) | (2 << 14) | (1 << 30),
        ttbr1_el1: 0x4000,
        ..CpuState::default()
    };
    let mut bus = Bus::new();
    let mut trace = TraceEngine::new(false);
    bus.map(BusRegion {
        base: 0,
        size: 0x10000,
        device: Box::new(Ram::new(0, 0x10000)),
    })
    .unwrap();
    let va = 0xfffffff007ba326c;
    let root_index = (va >> 36) & 7;
    let leaf_index = (va >> 25) & 0x7ff;
    write_u64(&mut bus, &mut trace, 0x4000 + root_index * 8, 0x8003);
    write_u64(&mut bus, &mut trace, 0x8000 + leaf_index * 8, AF | 1);
    let translated = Mmu::translate(&cpu, &mut bus, &mut trace, va, false, true).unwrap();
    assert_eq!(translated.pa, va & ((1 << 25) - 1));
}

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
        1,
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
        1,
        0,
    )
    .unwrap()
}

fn mapping(leaf_attributes: u64) -> (CpuState, Bus, TraceEngine) {
    let mut cpu = CpuState {
        sctlr_el1: 1,
        ttbr0_el1: ROOT,
        tcr_el1: 16 | (16 << 16) | (2 << 30),
        ..CpuState::default()
    };
    cpu.el = ExceptionLevel::El1;

    let mut bus = Bus::new();
    bus.map(BusRegion {
        base: 0,
        size: 0x10_000,
        device: Box::new(Ram::new(0, 0x10_000)),
    })
    .unwrap();
    let mut trace = TraceEngine::new(false);
    write_u64(&mut bus, &mut trace, ROOT, L1 | 3);
    write_u64(&mut bus, &mut trace, L1, L2 | 3);
    write_u64(&mut bus, &mut trace, L2, L3 | 3);
    write_u64(
        &mut bus,
        &mut trace,
        L3 + ((VA >> 12) & 0x1ff) * 8,
        PA_PAGE | 3 | leaf_attributes,
    );
    (cpu, bus, trace)
}

#[test]
fn access_flag_is_descriptor_bit_ten() {
    let (cpu, mut bus, mut trace) = mapping(1 << 5);
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::AccessFlag { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF);
    let translation = Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false).unwrap();
    assert_eq!(translation.pa, PA_PAGE + (VA & 0xfff));
}

#[test]
fn tcr_ha_sets_a_clear_leaf_access_flag() {
    let (mut cpu, mut bus, mut trace) = mapping(0);
    cpu.tcr_el1 |= 1 << 39; // HA
    let translation = Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false).unwrap();
    assert_eq!(translation.pa, PA_PAGE + (VA & 0xfff));

    let leaf = L3 + ((VA >> 12) & 0x1ff) * 8;
    assert_ne!(read_u64(&mut bus, &mut trace, leaf) & AF, 0);
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false).is_ok());
}

#[test]
fn tcr_hd_promotes_a_dbm_read_only_leaf_on_write() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | (2 << 6) | DBM);
    cpu.tcr_el1 |= (1 << 39) | (1 << 40); // HA | HD
    let translation = Mmu::translate(&cpu, &mut bus, &mut trace, VA, true, false).unwrap();
    assert!(translation.writable);

    let leaf = L3 + ((VA >> 12) & 0x1ff) * 8;
    let descriptor = read_u64(&mut bus, &mut trace, leaf);
    assert_eq!(descriptor & (1 << 7), 0); // AP[2] now records dirty state.
    assert_ne!(descriptor & DBM, 0);

    let (mut cpu, mut bus, mut trace) = mapping(AF | (2 << 6) | DBM);
    cpu.tcr_el1 |= 1 << 39; // HA without HD
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, true, false),
        Err(MmuFault::Permission { .. })
    ));
}

#[test]
fn machine_rewalks_a_cached_clean_leaf_for_dirty_update() {
    let (mut cpu, bus, _) = mapping(AF | (2 << 6) | DBM);
    cpu.tcr_el1 |= (1 << 39) | (1 << 40); // HA | HD
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;

    assert_eq!(
        machine.translate(VA, false, false).unwrap(),
        PA_PAGE + (VA & 0xfff)
    );
    assert!(machine.tlb.lookup(VA, 0, 0, false, true, false).is_none());
    assert_eq!(
        machine.translate(VA, true, false).unwrap(),
        PA_PAGE + (VA & 0xfff)
    );
    assert!(machine.tlb.lookup(VA, 0, 0, false, true, false).is_some());
}

#[test]
fn tcr_a1_selects_ttbr1_asid_for_both_va_ranges() {
    let (mut cpu, bus, _) = mapping(AF);
    cpu.ttbr0_el1 |= 0x11 << 48;
    cpu.ttbr1_el1 = ROOT | (0x22 << 48);
    cpu.tcr_el1 |= 1 << 22; // A1
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    machine.tlb.insert(
        VA,
        0x22,
        0,
        false,
        iphem_core::Translation {
            pa: 0xa000 + (VA & 0xfff),
            attr_index: 0,
            writable: true,
            executable: true,
            global: true,
        },
    );
    assert_eq!(
        machine.translate(VA, false, false).unwrap(),
        0xa000 + (VA & 0xfff)
    );
}

#[test]
fn global_leaf_tlb_entries_ignore_asid_but_ng_entries_do_not() {
    let (mut cpu, bus, _) = mapping(AF);
    cpu.ttbr0_el1 |= 0x11 << 48;
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    machine.translate(VA, false, false).unwrap();
    assert!(machine
        .tlb
        .lookup(VA, 0x22, 0, false, false, false)
        .is_some());

    let (mut cpu, bus, _) = mapping(AF | (1 << 11)); // nG
    cpu.ttbr0_el1 |= 0x11 << 48;
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine.cores.active_mut().state = cpu;
    machine.bus = bus;
    machine.translate(VA, false, false).unwrap();
    assert!(machine
        .tlb
        .lookup(VA, 0x22, 0, false, false, false)
        .is_none());
    assert!(machine
        .tlb
        .lookup(VA, 0x11, 0, false, false, false)
        .is_some());
}

#[test]
fn ap_controls_el0_access_and_write_permission() {
    let (mut cpu, mut bus, mut trace) = mapping(AF);
    cpu.el = ExceptionLevel::El0;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::Permission { .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF | (1 << 6));
    cpu.el = ExceptionLevel::El0;
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, true, false).is_ok());

    let (mut cpu, mut bus, mut trace) = mapping(AF | (3 << 6));
    cpu.el = ExceptionLevel::El0;
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false).is_ok());
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, true, false),
        Err(MmuFault::Permission { .. })
    ));
}

#[test]
fn uxn_and_pxn_apply_at_their_respective_exception_levels() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | UXN);
    cpu.el = ExceptionLevel::El0;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true),
        Err(MmuFault::Permission { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF | UXN);
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true).is_ok());

    let (cpu, mut bus, mut trace) = mapping(AF | PXN);
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true),
        Err(MmuFault::Permission { .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF | PXN | (1 << 6));
    cpu.el = ExceptionLevel::El0;
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true).is_ok());
}

#[test]
fn noncanonical_virtual_address_faults_before_table_walk() {
    let (cpu, mut bus, mut trace) = mapping(AF);
    let noncanonical = 0x0001_0000_0000_1234;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, noncanonical, false, false),
        Err(MmuFault::AddressSize {
            level: 0,
            stage2: false,
            ..
        })
    ));
}

#[test]
fn tcr_ips_limits_table_and_output_physical_addresses() {
    let (mut cpu, mut bus, mut trace) = mapping(AF);
    cpu.ttbr0_el1 = 1 << 32;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::AddressSize { stage2: false, .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF);
    let high_pa = 1u64 << 32;
    let leaf = L3 + ((VA >> 12) & 0x1ff) * 8;
    write_u64(&mut bus, &mut trace, leaf, high_pa | AF | 3);
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::AddressSize { stage2: false, .. })
    ));
    cpu.tcr_el1 |= 2 << 32; // IPS=40 bits
    assert_eq!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false)
            .unwrap()
            .pa,
        high_pa + (VA & 0xfff)
    );
}

#[test]
fn tbi0_tag_with_high_bit_set_still_selects_ttbr0() {
    let (mut cpu, mut bus, mut trace) = mapping(AF);
    cpu.tcr_el1 |= 1 << 37;
    let tagged_va = 0xab00_0000_0000_1234;
    let translation = Mmu::translate(&cpu, &mut bus, &mut trace, tagged_va, false, false).unwrap();
    assert_eq!(translation.pa, PA_PAGE + (VA & 0xfff));
}

#[test]
fn epd0_and_epd1_disable_the_selected_translation_walk() {
    let (mut cpu, mut bus, mut trace) = mapping(AF);
    cpu.tcr_el1 |= 1 << 7; // EPD0
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::Translation {
            level: 0,
            stage2: false,
            ..
        })
    ));

    // Reuse the same tables through TTBR1 for the canonical upper range.
    let upper_va = 0xffff_0000_0000_1234;
    cpu.tcr_el1 &= !(1 << 7);
    cpu.tcr_el1 |= 1 << 23; // EPD1
    cpu.ttbr1_el1 = ROOT;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, upper_va, false, false),
        Err(MmuFault::Translation {
            level: 0,
            stage2: false,
            ..
        })
    ));
}

#[test]
fn parent_table_permissions_restrict_leaf_access() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | (1 << 6));
    write_u64(&mut bus, &mut trace, ROOT, L1 | 3 | (1 << 61)); // APTable: no EL0
    cpu.el = ExceptionLevel::El0;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::Permission { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF);
    write_u64(&mut bus, &mut trace, ROOT, L1 | 3 | (1 << 62)); // APTable: read-only
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, true, false),
        Err(MmuFault::Permission { .. })
    ));

    let (cpu, mut bus, mut trace) = mapping(AF);
    write_u64(&mut bus, &mut trace, ROOT, L1 | 3 | (1 << 59)); // PXNTable
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true),
        Err(MmuFault::Permission { .. })
    ));

    let (mut cpu, mut bus, mut trace) = mapping(AF | (1 << 6));
    write_u64(&mut bus, &mut trace, ROOT, L1 | 3 | (1 << 60)); // UXNTable
    cpu.el = ExceptionLevel::El0;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true),
        Err(MmuFault::Permission { .. })
    ));
}

#[test]
fn pan_blocks_privileged_data_access_to_user_mapping() {
    let (mut cpu, mut bus, mut trace) = mapping(AF | (1 << 6));
    cpu.pstate |= 1 << 22;
    assert!(matches!(
        Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, false),
        Err(MmuFault::Permission { .. })
    ));
    // PAN does not prohibit privileged instruction fetch by itself.
    assert!(Mmu::translate(&cpu, &mut bus, &mut trace, VA, false, true).is_ok());
}

#[test]
fn pte_write_watch_and_permission_flip() {
    use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0,
            size: 0x10000,
            device: Box::new(SparseRam::new(0, 0x10000)),
        })
        .unwrap();

    let pte_pa = 0x4000;
    // Set write watch on PTE address
    m.write_watch_pa = Some((pte_pa, 8));
    m.write_watch_pa_log.clear();

    // Write initial writable PTE (AP[2:1]=00)
    m.mem_write(pte_pa, 8, 0x00600000_00000303).unwrap();
    // Flip PTE to read-only (AP[2:1]=10 -> bit 7 set)
    m.mem_write(pte_pa, 8, 0x00600000_00000383).unwrap();

    assert_eq!(m.write_watch_pa_log.len(), 2);
    assert_eq!(m.write_watch_pa_log[0].3, 0x4000); // PA
    assert_eq!(m.write_watch_pa_log[0].5, 0x00600000_00000303);
    assert_eq!(m.write_watch_pa_log[1].3, 0x4000); // PA
    assert_eq!(m.write_watch_pa_log[1].5, 0x00600000_00000383);
}
