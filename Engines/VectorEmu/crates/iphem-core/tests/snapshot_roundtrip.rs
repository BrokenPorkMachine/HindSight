use iphem_core::{
    BusRegion, CpuRunState, Machine, MachineConfig, MachineSnapshot, Ram, SnapshotError, SparseRam,
};
use std::collections::HashMap;

struct UnsnapshottedDevice;

impl iphem_core::Device for UnsnapshottedDevice {
    fn id(&self) -> u32 {
        900
    }
    fn name(&self) -> &'static str {
        "UnsnapshottedDevice"
    }
    fn reset(&mut self) {}
    fn read(&mut self, _offset: u64, _size: usize) -> iphem_core::DeviceResult {
        iphem_core::DeviceResult::Value(0)
    }
    fn write(&mut self, _offset: u64, _size: usize, _value: u64) -> iphem_core::DeviceResult {
        iphem_core::DeviceResult::Ok
    }
    fn tick(&mut self, _cycles: u64) -> Option<iphem_core::DeviceEvent> {
        None
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn build_test_machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x0100_0000,
            size: 0x0001_0000,
            device: Box::new(Ram::new(0x0100_0000, 0x0001_0000)),
        })
        .unwrap();
    m.bus
        .map(BusRegion {
            base: 0x2000_0000,
            size: 0x0002_0000,
            device: Box::new(SparseRam::new(0x2000_0000, 0x0002_0000)),
        })
        .unwrap();

    // Populate dense RAM and sparse pages (identity translate while MMU off).
    m.mem_write(0x0100_0000, 8, 0xdead_beef_1234_5678).unwrap();
    m.mem_write(0x0100_0010, 4, 0x1122_3344).unwrap();
    m.mem_write(0x2000_0000, 8, 0xaaaaaaaa_aaaaaaaa).unwrap();
    m.mem_write(0x2000_1ff0, 8, 0xbbbbbbbb_bbbbbbbb).unwrap();
    m.mem_write(0x2001_1000, 4, 0xcafe).unwrap();

    // Distinctive CPU state.
    m.cores.active_mut().state.pc = 0x2000_0000;
    m.cores.active_mut().state.sp = 0x2000_2000;
    m.cores.active_mut().state.x[1] = 0x0102_0304_0506_0708;
    m.cores.active_mut().state.esr_el3 = 0x8420;
    m.cores.active_mut().state.ttbr0_el1 = 0x2000_0000;
    m.cores.active_mut().instructions = 1234;
    m.cores.active_mut().run_state = CpuRunState::Running;
    m.cores.active_mut().pending_irq = Some(42);
    m
}

fn build_restore_target() -> Machine {
    let mut b = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    b.bus
        .map(BusRegion {
            base: 0x0100_0000,
            size: 0x0001_0000,
            device: Box::new(Ram::new(0x0100_0000, 0x0001_0000)),
        })
        .unwrap();
    b.bus
        .map(BusRegion {
            base: 0x2000_0000,
            size: 0x0002_0000,
            device: Box::new(SparseRam::new(0x2000_0000, 0x0002_0000)),
        })
        .unwrap();
    b
}

#[test]
fn snapshot_capture_save_load_restore_roundtrip() {
    let mut a = build_test_machine();
    let snap = MachineSnapshot::new(&mut a, HashMap::new()).unwrap();

    assert_eq!(snap.version, MachineSnapshot::CURRENT_VERSION);
    // Only RAM-like regions are captured.
    assert!(!snap.memory.regions.is_empty());

    let path =
        std::env::temp_dir().join(format!("iphem-snap-roundtrip-{}.bin", std::process::id()));
    snap.save_to_file(path.to_str().unwrap()).unwrap();
    let loaded = MachineSnapshot::load_from_file(path.to_str().unwrap()).unwrap();
    std::fs::remove_file(&path).ok();

    // Byte-identical after a save/load cycle.
    let raw_snap = bincode::serialize(&snap).unwrap();
    let raw_loaded = bincode::serialize(&loaded).unwrap();
    assert_eq!(raw_snap, raw_loaded);

    // Restore into a fresh machine with the same bus layout.
    let mut b = build_restore_target();
    b.restore_snapshot(&loaded).unwrap();

    // CPU state matches.
    assert_eq!(b.cores.active().state.pc, a.cores.active().state.pc);
    assert_eq!(b.cores.active().state.sp, a.cores.active().state.sp);
    assert_eq!(b.cores.active().state.x[1], a.cores.active().state.x[1]);
    assert_eq!(
        b.cores.active().state.esr_el3,
        a.cores.active().state.esr_el3
    );
    assert_eq!(
        b.cores.active().state.ttbr0_el1,
        a.cores.active().state.ttbr0_el1
    );
    assert_eq!(b.cores.active().instructions, a.cores.active().instructions);
    assert_eq!(b.cores.active().pending_irq, a.cores.active().pending_irq);
    assert_eq!(b.cores.active_index(), a.cores.active_index());
    assert_eq!(b.cores.len(), a.cores.len());
    assert_eq!(b.scheduler.cycle(), a.scheduler.cycle());

    // Dense RAM content restored.
    assert_eq!(b.mem_read(0x0100_0000, 8).unwrap(), 0xdead_beef_1234_5678);
    assert_eq!(b.mem_read(0x0100_0010, 4).unwrap(), 0x1122_3344);

    // Sparse page content restored at every touched page.
    assert_eq!(b.mem_read(0x2000_0000, 8).unwrap(), 0xaaaaaaaa_aaaaaaaa);
    assert_eq!(b.mem_read(0x2000_1ff0, 8).unwrap(), 0xbbbbbbbb_bbbbbbbb);
    assert_eq!(b.mem_read(0x2001_1000, 4).unwrap(), 0xcafe);
}

#[test]
fn snapshot_preserves_secondary_cores_and_scheduler_state() {
    let mut machine = build_test_machine();
    machine.cores[1].state.pc = 0x1234;
    machine.cores[1].instructions = 77;
    assert!(machine.select_cpu(2));
    assert!(!machine.select_cpu(usize::MAX));
    assert_eq!(machine.cores.active_index(), 2);
    machine.cores.active_mut().state.pc = 0x5678;
    machine.scheduler.enable_preemption(29, 8);
    machine.scheduler.tick(11);

    let snapshot = MachineSnapshot::new(&mut machine, HashMap::new()).unwrap();
    let mut restored = build_restore_target();
    restored.restore_snapshot(&snapshot).unwrap();

    assert_eq!(restored.cores.active_index(), 2);
    assert_eq!(restored.cores.active().state.pc, 0x5678);
    assert_eq!(restored.cores[1].state.pc, 0x1234);
    assert_eq!(restored.cores[1].instructions, 77);
    assert_eq!(restored.scheduler.cycle(), 11);
    assert!(restored.scheduler.preemption_enabled());
    assert_eq!(restored.scheduler.preemption_irq(), Some(29));
    assert_eq!(restored.scheduler.next_preemption_cycle(), 8);
}

#[test]
fn restore_rejects_snapshots_with_unsnapshotted_mmio_state() {
    let mut source = build_test_machine();
    source
        .bus
        .map(BusRegion {
            base: 0x3000_0000,
            size: 0x1000,
            device: Box::new(UnsnapshottedDevice),
        })
        .unwrap();
    let snapshot = MachineSnapshot::new(&mut source, HashMap::new()).unwrap();

    let mut target = build_restore_target();
    let error = target.restore_snapshot(&snapshot).unwrap_err();
    assert!(error
        .to_string()
        .contains("UnsnapshottedDevice at 0x30000000"));
}

#[test]
fn snapshot_version_mismatch_is_rejected() {
    let mut m = build_test_machine();
    let mut snap = MachineSnapshot::new(&mut m, HashMap::new()).unwrap();
    snap.version = 0xFFFF;

    let path = std::env::temp_dir().join(format!("iphem-snap-version-{}.bin", std::process::id()));
    snap.save_to_file(path.to_str().unwrap()).unwrap();
    let err = MachineSnapshot::load_from_file(path.to_str().unwrap());
    std::fs::remove_file(&path).ok();

    assert!(matches!(err, Err(SnapshotError::VersionMismatch)));
}

#[test]
fn restore_into_repopulates_tlb_and_overwrites_ram() {
    let mut a = build_test_machine();
    // Push a TLB entry so it round-trips through the snapshot.
    a.tlb.insert(
        0x2000_0000,
        0x77,
        0,
        false,
        iphem_core::Translation {
            pa: 0x2000_0000,
            attr_index: 2,
            writable: true,
            executable: true,
            global: true,
        },
    );
    let snap = MachineSnapshot::new(&mut a, HashMap::new()).unwrap();
    assert!(!snap.tlb.entries.is_empty());

    let mut b = build_restore_target();
    // Pollute the target machine's RAM first; restore must overwrite it.
    b.mem_write(0x0100_0000, 8, 0).unwrap();
    b.restore_snapshot(&snap).unwrap();

    assert!(!b.tlb.get_entries().is_empty());
    assert_eq!(b.mem_read(0x0100_0000, 8).unwrap(), 0xdead_beef_1234_5678);
}
