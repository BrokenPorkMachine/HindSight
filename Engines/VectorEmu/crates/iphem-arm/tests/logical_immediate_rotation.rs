use iphem_arm::{execute, Instruction};
use iphem_core::{Machine, MachineConfig};

#[test]
fn ios12_ptd_bootstrap_constant_produces_nonzero_descriptor_count() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[21] = 0x500000;
    // Real 16A366 kernel at 0xfffffff007b98e60. Form the reciprocal for /40.
    execute(&mut m, Instruction(0xb202e7e9)).unwrap(); // ORR X9,XZR,#0xcccccccccccccccc
    assert_eq!(m.cores.active().state.x[9], 0xcccc_cccc_cccc_cccc);
    execute(&mut m, Instruction(0xf29999a9)).unwrap(); // MOVK X9,#0xcccd
    assert_eq!(m.cores.active().state.x[9], 0xcccc_cccc_cccc_cccd);
    execute(&mut m, Instruction(0x9bc97ea9)).unwrap(); // UMULH X9,X21,X9
    execute(&mut m, Instruction(0xd345fd29)).unwrap(); // LSR X9,X9,#5
    assert_eq!(m.cores.active().state.x[9], 0x20000);
}

#[test]
fn logical_immediates_rotate_within_each_repeated_element() {
    // Build every encodable repeated element using bit placement, independently
    // of the executor's mask/rotate implementation. Exercise W and X forms.
    for width in [32u32, 64] {
        for log_element in 1..=width.ilog2() {
            let element = 1u32 << log_element;
            for ones in 1..element {
                for rotate in 0..element {
                    let n = u32::from(element == 64);
                    let imms = ((!(element * 2 - 1)) & 63) | (ones - 1);
                    let word = 0x320003e0
                        | (u32::from(width == 64) << 31)
                        | (n << 22)
                        | (rotate << 16)
                        | (imms << 10);
                    let mut expected = 0u64;
                    for base in (0..width).step_by(element as usize) {
                        for bit in 0..ones {
                            expected |= 1u64 << (base + (bit + element - rotate) % element);
                        }
                    }
                    let mut m = Machine::new(MachineConfig {
                        trace: false,
                        quantum: 1,
                    });
                    m.cores.active_mut().state.x[0] = u64::MAX;
                    m.cores.active_mut().state.pstate = 0xa00003c5;
                    execute(&mut m, Instruction(word)).unwrap(); // ORR Rd,ZR,#mask
                    assert_eq!(m.cores.active().state.x[0], expected, "word={word:08x}");
                    assert_eq!(m.cores.active().state.pstate, 0xa00003c5);
                }
            }
        }
    }
}
