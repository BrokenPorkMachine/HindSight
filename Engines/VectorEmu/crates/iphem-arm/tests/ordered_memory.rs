use iphem_arm::{execute, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0,
            size: 0x1000,
            device: Box::new(SparseRam::new(0, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.x[0] = 0x100;
    m
}

#[test]
fn stlr_is_unconditional_at_every_width_and_ldar_does_not_reserve() {
    for width in 0..4 {
        let mut m = machine();
        let size = 1usize << width;
        let mask = u64::MAX >> (64 - size * 8);
        m.cores.active_mut().state.x[1] = 0x8877665544332211;
        execute(&mut m, Instruction(0x089ffc01 | (width << 30))).unwrap();
        assert_eq!(m.mem_read(0x100, size).unwrap(), 0x8877665544332211 & mask);
        execute(&mut m, Instruction(0x08dffc02 | (width << 30))).unwrap();
        assert_eq!(m.cores.active().state.x[2], 0x8877665544332211 & mask);
        assert_eq!(m.cores.active().exclusive_address, None);
    }
}

#[test]
fn xnu_lock_release_does_not_require_an_exclusive_reservation() {
    let mut m = machine();
    m.cores.active_mut().state.x[1] = 0xfffffff0091f5d49;
    execute(&mut m, Instruction(0xc85f7c09)).unwrap(); // LDXR x9, [x0]
    execute(&mut m, Instruction(0xc8e97c01)).unwrap(); // CASA x9, x1, [x0]
    assert_eq!(m.cores.active().state.x[9], 0);
    assert_eq!(m.mem_read(0x100, 8).unwrap(), m.cores.active().state.x[1]);
    execute(&mut m, Instruction(0xd5033f5f)).unwrap(); // CLREX (intervening work)
    execute(&mut m, Instruction(0xc89ffc1f)).unwrap(); // STLR XZR, [x0]
    assert_eq!(m.mem_read(0x100, 8).unwrap(), 0);
    execute(&mut m, Instruction(0xc85f7c09)).unwrap();
    execute(&mut m, Instruction(0xc8e97c01)).unwrap();
    assert_eq!(m.cores.active().state.x[9], 0, "lock can be reacquired");
}

#[test]
fn ordered_accesses_still_require_natural_alignment() {
    for op in [0xc89ffc01, 0xc8dffc02] {
        let mut m = machine();
        m.cores.active_mut().state.x[0] = 0x101;
        assert!(execute(&mut m, Instruction(op)).is_err());
    }
}

#[test]
fn lse_operations_decode_all_orderings_and_widths_without_becoming_cas() {
    for width in 0..4 {
        for ordering in 0..4 {
            for opc in [0, 1, 2, 3, 8] {
                let mut m = machine();
                let size = 1usize << width;
                m.mem_write(0x100, size, 0x55).unwrap();
                m.cores.active_mut().state.x[1] = 0x11;
                let word = 0x38210002 | (width << 30) | (ordering << 22) | (opc << 12);
                execute(&mut m, Instruction(word)).unwrap();
                assert_eq!(m.cores.active().state.x[2], 0x55);
                assert_eq!(m.cores.active().state.x[1], 0x11);
                let expected = match opc {
                    0 => 0x66,
                    1 | 2 => 0x44,
                    3 => 0x55,
                    _ => 0x11,
                };
                assert_eq!(m.mem_read(0x100, size).unwrap(), expected);
            }
        }
    }
}

#[test]
fn real_stclrl_clears_bits_without_a_destination_register() {
    let mut m = machine();
    m.cores.active_mut().state.x[24] = 0x100;
    m.cores.active_mut().state.x[8] = 0x20000000;
    m.mem_write(0x100, 4, 0x20000080).unwrap();
    execute(&mut m, Instruction(0xb868131f)).unwrap(); // STCLRL w8, [x24]
    assert_eq!(m.mem_read(0x100, 4).unwrap(), 0x80);
    assert_eq!(m.cores.active().state.x[8], 0x20000000);
}
