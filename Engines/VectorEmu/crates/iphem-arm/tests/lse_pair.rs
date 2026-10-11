use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn machine() -> Machine {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine
        .bus
        .map(BusRegion {
            base: 0x4000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x4000, 0x1000)),
        })
        .unwrap();
    machine
}

#[test]
fn casp_pair_updates_memory_and_returns_the_old_pair() {
    let mut machine = machine();
    let address = 0x4080;
    machine
        .mem_write(address, 8, 0x1122_3344_5566_7788)
        .unwrap();
    machine
        .mem_write(address + 8, 8, 0x99aa_bbcc_ddee_ff00)
        .unwrap();
    machine.cores.active_mut().state.x[0] = 0x1122_3344_5566_7788;
    machine.cores.active_mut().state.x[1] = 0x99aa_bbcc_ddee_ff00;
    machine.cores.active_mut().state.x[2] = 0xaabb_ccdd_eeff_0011;
    machine.cores.active_mut().state.x[3] = 0x1234_5678_90ab_cdef;
    machine.cores.active_mut().state.x[4] = address;

    // CASP X0, X1, X2, X3, [X4]. Encoded by clang for armv8.1-a+lse.
    execute(&mut machine, Instruction(0x4820_7c82)).unwrap();
    assert_eq!(machine.mem_read(address, 8).unwrap(), 0xaabb_ccdd_eeff_0011);
    assert_eq!(
        machine.mem_read(address + 8, 8).unwrap(),
        0x1234_5678_90ab_cdef
    );
    assert_eq!(machine.cores.active().state.x[0], 0x1122_3344_5566_7788);
    assert_eq!(machine.cores.active().state.x[1], 0x99aa_bbcc_ddee_ff00);
}

#[test]
fn casp_mismatch_preserves_memory_but_returns_the_observed_pair() {
    let mut machine = machine();
    let address = 0x4080;
    machine.mem_write(address, 8, 0x1111).unwrap();
    machine.mem_write(address + 8, 8, 0x2222).unwrap();
    machine.cores.active_mut().state.x[0] = 0x1111;
    machine.cores.active_mut().state.x[1] = 0x3333; // second compare lane mismatches
    machine.cores.active_mut().state.x[2] = 0xaaaa;
    machine.cores.active_mut().state.x[3] = 0xbbbb;
    machine.cores.active_mut().state.x[4] = address;

    execute(&mut machine, Instruction(0x4820_7c82)).unwrap();
    assert_eq!(machine.mem_read(address, 8).unwrap(), 0x1111);
    assert_eq!(machine.mem_read(address + 8, 8).unwrap(), 0x2222);
    assert_eq!(
        (
            machine.cores.active().state.x[0],
            machine.cores.active().state.x[1]
        ),
        (0x1111, 0x2222)
    );
}

#[test]
fn casp_w_pair_zero_extends_and_uses_eight_byte_alignment() {
    let mut machine = machine();
    let address = 0x4088;
    machine.mem_write(address, 4, 0xffff_ffff).unwrap();
    machine.mem_write(address + 4, 4, 0x1234_5678).unwrap();
    machine.cores.active_mut().state.x[0] = 0x1_ffff_ffff;
    machine.cores.active_mut().state.x[1] = 0x1234_5678;
    machine.cores.active_mut().state.x[2] = 0x2_aaaa_aaaa;
    machine.cores.active_mut().state.x[3] = 0xbbbb_cccc;
    machine.cores.active_mut().state.x[4] = address;

    // CASP W0, W1, W2, W3, [X4].
    execute(&mut machine, Instruction(0x0820_7c82)).unwrap();
    assert_eq!(machine.mem_read(address, 4).unwrap(), 0xaaaa_aaaa);
    assert_eq!(machine.mem_read(address + 4, 4).unwrap(), 0xbbbb_cccc);
    assert_eq!(machine.cores.active().state.x[0], 0xffff_ffff);
}

#[test]
fn casp_ordering_variants_decode_and_disassemble() {
    let cases = [
        (0x4820_7c82, "casp\t"),
        (0x4866_7d48, "caspa\t"),
        (0x482c_fe0e, "caspl\t"),
        (0x4872_fed4, "caspal\t"),
    ];
    for (word, mnemonic) in cases {
        assert!(disassemble(0, word).contains(mnemonic), "{word:#010x}");
    }
}

#[test]
fn casp_rejects_odd_register_pairs_and_faults_misaligned_addresses() {
    let mut machine = machine();
    assert!(execute(&mut machine, Instruction(0x4821_7c82)).is_err()); // odd Rs

    machine.cores.active_mut().state.x[0] = 1;
    machine.cores.active_mut().state.x[1] = 2;
    machine.cores.active_mut().state.x[2] = 3;
    machine.cores.active_mut().state.x[3] = 4;
    machine.cores.active_mut().state.x[4] = 0x4088; // 8-byte aligned, but not 16-byte aligned for X pairs
    assert!(execute(&mut machine, Instruction(0x4820_7c82)).is_err());
}
