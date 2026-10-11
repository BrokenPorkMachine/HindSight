use iphem_core::{Bus, BusRegion, Machine, MachineConfig, Ram};
#[test]
fn ram_roundtrip() {
    let mut b = Bus::new();
    b.map(BusRegion {
        base: 0,
        size: 0x1000,
        device: Box::new(Ram::new(0, 0x1000)),
    })
    .unwrap();
    let mut t = iphem_core::TraceEngine::new(true);
    b.write(
        iphem_core::MemTransaction {
            address: 8,
            size: 8,
            kind: iphem_core::TransactionKind::Write,
            security: iphem_core::SecurityState::Secure,
            source: None,
        },
        0x1122334455667788,
        &mut t,
        0,
        0,
        0,
    )
    .unwrap();
    let v = b
        .read(
            iphem_core::MemTransaction {
                address: 8,
                size: 8,
                kind: iphem_core::TransactionKind::Read,
                security: iphem_core::SecurityState::Secure,
                source: None,
            },
            &mut t,
            0,
            0,
            0,
        )
        .unwrap();
    assert_eq!(v, 0x1122334455667788)
}
#[test]
fn machine_starts() {
    let m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    assert_eq!(m.cores.active().state.pc, 0)
}
