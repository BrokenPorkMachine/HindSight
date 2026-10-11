use iphem_core::{
    Bus, BusRegion, MemTransaction, SecurityState, SparseRam, TraceEngine, TransactionKind,
};
#[test]
fn sparse_ram_allocates_on_write() {
    let mut b = Bus::new();
    b.map(BusRegion {
        base: 0,
        size: 0x1_0000_0000,
        device: Box::new(SparseRam::new(0, 0x1_0000_0000)),
    })
    .unwrap();
    let mut t = TraceEngine::new(false);
    b.write(
        MemTransaction {
            address: 0x8000_1000,
            size: 8,
            kind: TransactionKind::Write,
            security: SecurityState::Secure,
            source: None,
        },
        0xdeadbeef,
        &mut t,
        0,
        0,
        0,
    )
    .unwrap();
    let v = b
        .read(
            MemTransaction {
                address: 0x8000_1000,
                size: 8,
                kind: TransactionKind::Read,
                security: SecurityState::Secure,
                source: None,
            },
            &mut t,
            0,
            0,
            0,
        )
        .unwrap();
    assert_eq!(v, 0xdeadbeef)
}
