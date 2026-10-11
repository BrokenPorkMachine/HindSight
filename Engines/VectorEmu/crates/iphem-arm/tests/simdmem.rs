use iphem_arm::{execute, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn new_machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x100000,
            size: 0x80000,
            device: Box::new(SparseRam::new(0x100000, 0x80000)),
        })
        .unwrap();
    m
}

fn run(m: &mut Machine, w: u32) {
    let r = execute(m, Instruction(w));
    assert!(r.is_ok(), "exec failed for 0x{w:08x}: {r:?}");
}

fn v(m: &Machine, i: usize) -> u128 {
    m.cores.active().state.v[i]
}

fn setv(m: &mut Machine, i: usize, val: u128) {
    m.cores.active_mut().state.v[i] = val;
}

fn rd(m: &Machine, i: usize) -> u64 {
    m.cores.active().state.x[i]
}

const Q1: u128 = 0x1122334455667788_aabbccddeeff0011;
const Q2: u128 = 0xffeeddccbbaa9988_8877665544332211;

#[test]
fn u_offset_roundtrip_q() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    setv(&mut m, 1, Q1);
    // str q1, [x2, #0x40] = 0x3d801041
    run(&mut m, 0x3d801041);
    assert_eq!(m.mem_read(0x100040, 8).unwrap(), 0xaabbccddeeff0011);
    assert_eq!(m.mem_read(0x100048, 8).unwrap(), 0x1122334455667788);
    // ldr q1, [x2, #0x40] = 0x3dc01041
    m.cores.active_mut().state.v[1] = 0;
    run(&mut m, 0x3dc01041);
    assert_eq!(v(&m, 1), Q1);
}

#[test]
fn ld1_single_q_post_index_load_matches_securerom_aes_encoding() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[3] = 0x100000;
    m.mem_write(0x100000, 8, 0x1122334455667788).unwrap();
    m.mem_write(0x100008, 8, 0x99aabbccddeeff00).unwrap();

    // ld1 {v1.4s}, [x3], #16; this is used by the T8020 ROM AES helper.
    let word = 0x4cdf7861;
    assert_eq!(iphem_arm::disassemble(0, word), "ld1\t{v1.4s}, [x3], #0x10");
    run(&mut m, word);
    assert_eq!(v(&m, 1), 0x99aabbccddeeff00_1122334455667788);
    assert_eq!(rd(&m, 3), 0x100010);
}

#[test]
fn ld1_pair_q_post_index_load_matches_securerom_vector_encoding() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    m.mem_write(0x100000, 8, 0x0123456789abcdef).unwrap();
    m.mem_write(0x100008, 8, 0xfedcba9876543210).unwrap();
    m.mem_write(0x100010, 8, 0x1122334455667788).unwrap();
    m.mem_write(0x100018, 8, 0x99aabbccddeeff00).unwrap();

    // ld1 {v0.16b, v1.16b}, [x2], #32.
    let word = 0x4cdf2040;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld1\t{v0.16b, v1.16b}, [x2], #0x20"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 0), 0xfedcba9876543210_0123456789abcdef);
    assert_eq!(v(&m, 1), 0x99aabbccddeeff00_1122334455667788);
    assert_eq!(rd(&m, 2), 0x100020);
}

#[test]
fn u_offset_scalar_sizes() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    setv(&mut m, 1, Q1);
    // str h1, [x2, #0x20] = 0x7d004041 (2-byte store of low half)
    run(&mut m, 0x7d004041);
    assert_eq!(m.mem_read(0x100020, 2).unwrap(), 0x0011);
    // ldr h1, [x2, #0x20] = 0x7d404041 merges low 16, preserves upper.
    m.cores.active_mut().state.v[1] = 0xffffffffffffffff_ffffffffffff0000;
    run(&mut m, 0x7d404041);
    assert_eq!(v(&m, 1), 0xffffffffffffffff_ffffffffffff0011);
    // b1 forms: str b1, [x2, #0x10] = 0x3d004041, ldr b1 = 0x3d404041
    run(&mut m, 0x3d004041);
    assert_eq!(m.mem_read(0x100010, 1).unwrap(), 0x11);
    m.cores.active_mut().state.v[1] = 0xffffffffffffffff_ffffffffffffff00;
    run(&mut m, 0x3d404041);
    assert_eq!(v(&m, 1), 0xffffffffffffffff_ffffffffffffff11);
}

#[test]
fn pre_index_writeback_and_negative() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x160000;
    // ldr d1, [x2, #-0x20]! = 0xfc5e0c41: read at 0x15ffe0, base -> 0x15ffe0.
    run(&mut m, 0xfc5e0c41);
    assert_eq!(rd(&m, 2), 0x160000 - 0x20);
    // str q1, [x2, #0x40]! = 0x3c840c41: write at base-0x20+0x40, base -> +0x40.
    setv(&mut m, 1, Q1);
    run(&mut m, 0x3c840c41);
    assert_eq!(rd(&m, 2), 0x160020);
    assert_eq!(m.mem_read(0x160020, 8).unwrap(), 0xaabbccddeeff0011);
}

#[test]
fn post_index_uses_old_base() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    setv(&mut m, 1, Q1);
    // str q1, [x2], #0x40 = 0x3c840441: write at 0x100000, x2 -> 0x100040.
    run(&mut m, 0x3c840441);
    assert_eq!(rd(&m, 2), 0x100040);
    assert_eq!(m.mem_read(0x100000, 8).unwrap(), 0xaabbccddeeff0011);
    // Pre-fill the post-bumped address before ldr s1, [x2], #0x10.
    m.mem_write(0x100040, 4, 0x0badc0de).unwrap();
    run(&mut m, 0xbc410441);
    assert_eq!(rd(&m, 2), 0x100050);
    assert_eq!(v(&m, 1) as u32, 0x0badc0de);
}

#[test]
fn register_offset_forms() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    m.cores.active_mut().state.x[3] = 0x100;
    setv(&mut m, 1, Q1);
    // str q1, [x2, x3] = 0x3ca36841 -> 0x100100.
    run(&mut m, 0x3ca36841);
    // str s1, [x2, x3, lsl #2] = 0xbc237841 -> 0x100400.
    run(&mut m, 0xbc237841);
    assert_eq!(m.mem_read(0x100400, 4).unwrap(), 0xeeff0011);
    m.cores.active_mut().state.v[1] = 0;
    // ldr q1, [x2, x3] = 0x3ce36841.
    run(&mut m, 0x3ce36841);
    assert_eq!(v(&m, 1), Q1);
}

#[test]
fn ldur_stur_unscaled_scalar() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x160000;
    setv(&mut m, 1, Q1);
    // stur s1, [x2, #-0x40] = 0xbc1c0041 -> 0x15ffc0.
    run(&mut m, 0xbc1c0041);
    assert_eq!(m.mem_read(0x160000 - 0x40, 4).unwrap(), 0xeeff0011);
    m.mem_write(0x160040, 8, 0x0d0c0b0a05040302).unwrap();
    // ldur d1, [x2, #0x40] = 0xfc440041 (d loads 8 bytes, upper half zeroed).
    m.cores.active_mut().state.v[1] = 0;
    run(&mut m, 0xfc440041);
    assert_eq!(v(&m, 1), 0x0d0c0b0a05040302);
}

#[test]
fn stp_ldp_simd_pair_roundtrip() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[3] = 0x100000;
    setv(&mut m, 1, Q1);
    setv(&mut m, 2, Q2);
    // stp s1, s2, [x3, #0x10] = 0x2d020861 (s stores low 32 of each lane)
    run(&mut m, 0x2d020861);
    assert_eq!(m.mem_read(0x100010, 4).unwrap(), 0xeeff0011);
    assert_eq!(m.mem_read(0x100014, 4).unwrap(), 0x44332211);
    // stp q1, q2, [x3] = 0xad000861
    run(&mut m, 0xad000861);
    assert_eq!(m.mem_read(0x100000, 8).unwrap(), 0xaabbccddeeff0011);
    assert_eq!(m.mem_read(0x100008, 8).unwrap(), 0x1122334455667788);
    assert_eq!(m.mem_read(0x100010, 8).unwrap(), 0x8877665544332211);
    assert_eq!(m.mem_read(0x100018, 8).unwrap(), 0xffeeddccbbaa9988);
    // ldp d1, d2, [x3] = 0x6d400861: pair of 8-byte elements.
    m.cores.active_mut().state.v[1] = 0;
    m.cores.active_mut().state.v[2] = 0;
    run(&mut m, 0x6d400861);
    assert_eq!(v(&m, 1), 0xaabbccddeeff0011);
    assert_eq!(v(&m, 2), 0x1122334455667788);
    // ldp s1, s2, [x3, #0x10]! = 0x2dc20861 pre writeback.
    m.cores.active_mut().state.x[3] = 0x100000;
    run(&mut m, 0x2dc20861);
    assert_eq!(rd(&m, 3), 0x100010);
    assert_eq!(v(&m, 1) as u32, 0x44332211);
    // stp d1, d2, [x3], #-0x40 = 0x6cbc0861 post writeback.
    m.cores.active_mut().state.x[3] = 0x100040;
    run(&mut m, 0x6cbc0861);
    assert_eq!(rd(&m, 3), 0x100000);
    assert_eq!(m.mem_read(0x100040, 8).unwrap(), 0x44332211);
    assert_eq!(m.mem_read(0x100048, 8).unwrap(), 0x88776655);
    // ldp d1, d2, [x3], #0x40 = 0x6cc40861 post: translate at old base.
    m.cores.active_mut().state.v[1] = 0;
    m.cores.active_mut().state.x[3] = 0x100000;
    run(&mut m, 0x6cc40861);
    assert_eq!(v(&m, 1), 0xaabbccddeeff0011);
    assert_eq!(rd(&m, 3), 0x100040);
}

#[test]
fn scalar_pairs_still_decode() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[3] = 0x100000;
    m.cores.active_mut().state.x[1] = 0x41414141;
    m.cores.active_mut().state.x[2] = 0x42424242;
    // stp x1, x2, [x3] = 0xa9000861
    run(&mut m, 0xa9000861);
    assert_eq!(m.mem_read(0x100000, 8).unwrap(), 0x41414141);
    assert_eq!(m.mem_read(0x100008, 8).unwrap(), 0x42424242);
    // ldp x1, x2, [x3] = 0xa9400861
    m.cores.active_mut().state.x[1] = 0;
    m.cores.active_mut().state.x[2] = 0;
    run(&mut m, 0xa9400861);
    assert_eq!(rd(&m, 1), 0x41414141);
    assert_eq!(rd(&m, 2), 0x42424242);
    // pre: ldp x1, x2, [x3, #0x10]! = 0xa9c10861
    m.cores.active_mut().state.x[1] = 0;
    m.cores.active_mut().state.x[2] = 0;
    run(&mut m, 0xa9c10861);
    assert_eq!(rd(&m, 3), 0x100010);
    assert_eq!(rd(&m, 1), 0);
    assert_eq!(rd(&m, 2), 0);
    // post: ldp x1, x2, [x3], #0x10 = 0xa8c10861
    m.cores.active_mut().state.x[3] = 0x100000;
    run(&mut m, 0xa8c10861);
    assert_eq!(rd(&m, 3), 0x100010);
    assert_eq!(rd(&m, 1), 0x41414141);
}

#[test]
fn scalar_ldr_str_forms_still_decode() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    // ldur x1, [x2] = 0xf8400041
    run(&mut m, 0xf8400041);
    assert_eq!(rd(&m, 1), 0);
    // str w1, [x2, #0x20] = 0xb9002041 then ldr w1 = 0xb9402041
    m.cores.active_mut().state.x[1] = 0xdeadbeef;
    run(&mut m, 0xb9002041);
    assert_eq!(m.mem_read(0x100020, 4).unwrap(), 0xdeadbeef);
    m.cores.active_mut().state.x[1] = 0;
    run(&mut m, 0xb9402041);
    assert_eq!(rd(&m, 1), 0xdeadbeef);
}

#[test]
fn st2_element_interleave_stamps_self_referential_queue_heads() {
    // st2 {v16.2d, v17.2d}, [x9]: AArch64 interleaves scalar elements.
    // Memory layout must be: [x9+0]=v16.d0, [x9+8]=v17.d0, [x9+16]=v16.d1, [x9+24]=v17.d1.
    // This is the pattern used by SIMD struct multi-register transfers and is what
    // causes the queue-linkage panic when the emulator stores contiguously instead.
    let mut m = new_machine();
    m.cores.active_mut().state.x[9] = 0x100000;
    setv(&mut m, 16, 0x1122334455667788_aabbccddeeff0011);
    setv(&mut m, 17, 0xffeeddccbbaa9988_8877665544332211);
    // st2 {v16.2d, v17.2d}, [x9] = 0x4c008d30
    // (size=3 for .2d, q=1, n=2, store, no post-index)
    let word = 0x4c008d30;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "st2\t{v16.2d, v17.2d}, [x9]"
    );
    run(&mut m, word);
    // v16.d0 at offset 0..7 (low 64 bits of v16).
    assert_eq!(m.mem_read(0x100000, 8).unwrap(), 0xaabbccddeeff0011);
    // v17.d0 at offset 8..15 (low 64 bits of v17).
    assert_eq!(m.mem_read(0x100008, 8).unwrap(), 0x8877665544332211);
    // v16.d1 at offset 16..23 (high 64 bits of v16).
    assert_eq!(m.mem_read(0x100010, 8).unwrap(), 0x1122334455667788);
    // v17.d1 at offset 24..31 (high 64 bits of v17).
    assert_eq!(m.mem_read(0x100018, 8).unwrap(), 0xffeeddccbbaa9988);
}

#[test]
fn ld2_ld3_ld4_multi_register_structure_lists_transfer_and_post_index() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[2] = 0x100000;
    for (regno, bytes) in [
        (0u32, 0x0001020304050607),
        (1, 0x1011121314151617),
        (2, 0x2021222324252627),
        (3, 0x3031323334353637),
        (4, 0x4041424344454647),
    ] {
        let off = regno as u64 * 16;
        m.mem_write(0x100000 + off, 8, bytes).unwrap();
        m.mem_write(0x100000 + off + 8, 8, bytes.wrapping_add(0x08080808))
            .unwrap();
    }

    // ld4 {v4.16b, v5.16b, v6.16b, v7.16b}, [x2].
    m.cores.active_mut().state.x[2] = 0x100000;
    let word = 0x4c400044;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld4\t{v4.16b, v5.16b, v6.16b, v7.16b}, [x2]"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 4), 0x333f3337232f2327131f1317030f0307);
    assert_eq!(v(&m, 5), 0x323e3236222e2226121e1216020e0206);
    assert_eq!(v(&m, 6), 0x313d3135212d2125111d1115010d0105);
    assert_eq!(v(&m, 7), 0x303c3034202c2024101c1014000c0004);
    assert_eq!(rd(&m, 2), 0x100000);

    // ld2 {v4.4s, v5.4s}, [x2], x7 (register post-index).
    m.cores.active_mut().state.x[2] = 0x100000;
    m.cores.active_mut().state.x[7] = 0x20;
    let word = 0x4cc78844;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld2\t{v4.4s, v5.4s}, [x2], x7"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 4), 0x1c1d1e1f141516170c0d0e0f04050607);
    assert_eq!(v(&m, 5), 0x10111213101112130001020300010203);
    assert_eq!(rd(&m, 2), 0x100020);

    // ld3 {v0.8b, v1.8b, v2.8b}, [x3] (Q=0 half-vector list).
    m.cores.active_mut().state.x[3] = 0x100000;
    let word = 0x0c404060;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld3\t{v0.8b, v1.8b, v2.8b}, [x3]"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 0), 0x0000000000000000121500030e010407);
    assert_eq!(v(&m, 1), 0x0000000000000000111417020d000306);
    assert_eq!(v(&m, 2), 0x0000000000000000101316010c0f0205);
}

#[test]
fn ld1r_ld4r_replicate_structure_loads() {
    let mut m = new_machine();
    m.cores.active_mut().state.x[8] = 0x100000;
    m.mem_write(0x100000, 8, 0x1122334455667788).unwrap();

    // The exact word the iOS 12 kernel traps at in its pmap/VM init:
    // ld1r.2s {v8}, [x8].
    let word = 0x0d40c908;
    assert_eq!(iphem_arm::disassemble(0, word), "ld1r\t{v8.2s}, [x8]");
    run(&mut m, word);
    assert_eq!(v(&m, 8), 0x55667788_55667788);
    assert_eq!(rd(&m, 8), 0x100000);

    // ld1r.4s {v8}, [x8] (Q=1, four 32-bit lanes).
    let word = 0x4d40c908;
    assert_eq!(iphem_arm::disassemble(0, word), "ld1r\t{v8.4s}, [x8]");
    run(&mut m, word);
    assert_eq!(v(&m, 8), 0x55667788_55667788_55667788_55667788);

    // ld1r.2d {v8}, [x8] (Q=1, two 64-bit lanes).
    let word = 0x4d40cd08;
    assert_eq!(iphem_arm::disassemble(0, word), "ld1r\t{v8.2d}, [x8]");
    run(&mut m, word);
    assert_eq!(v(&m, 8), 0x1122334455667788_1122334455667788);

    // ld2r.2s {v8, v9}, [x8] replicates into both registers.
    let word = 0x0d60c908;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld2r\t{v8.2s, v9.2s}, [x8]"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 8), 0x55667788_55667788);
    assert_eq!(v(&m, 9), 0x55667788_55667788);

    // ld3r.8b {v16, v17, v18}, [x0] (Q=0, eight 8-bit lanes across 3 regs).
    m.cores.active_mut().state.x[0] = 0x100000;
    m.mem_write(0x100000, 1, 0xab).unwrap();
    let word = 0x0d40e010;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld3r\t{v16.8b, v17.8b, v18.8b}, [x0]"
    );
    run(&mut m, word);
    for r in [16u32, 17, 18] {
        assert_eq!(v(&m, r as usize), 0xab_ab_ab_ab_ab_ab_ab_ab);
    }

    // ld4r.2s {v8, v9, v10, v11}, [x8], x9 (register post-index).
    m.cores.active_mut().state.x[8] = 0x100000;
    m.cores.active_mut().state.x[9] = 0x38;
    m.mem_write(0x100000, 8, 0x1122334455667788).unwrap();
    let word = 0x0de9e908;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld4r\t{v8.2s, v9.2s, v10.2s, v11.2s}, [x8], x9"
    );
    run(&mut m, word);
    for r in [8u32, 9, 10, 11] {
        assert_eq!(v(&m, r as usize), 0x55667788_55667788);
    }
    assert_eq!(rd(&m, 8), 0x100038);

    // ld1r.16b {v8}, [x8], #0x1 (immediate post-index = element size).
    m.cores.active_mut().state.x[8] = 0x100000;
    m.mem_write(0x100000, 1, 0x7f).unwrap();
    let word = 0x4ddfc108;
    assert_eq!(
        iphem_arm::disassemble(0, word),
        "ld1r\t{v8.16b}, [x8], #0x1"
    );
    run(&mut m, word);
    assert_eq!(v(&m, 8), 0x7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f_7f);
    assert_eq!(rd(&m, 8), 0x100001);
}
