use iphem_arm::{execute, Instruction};
use iphem_core::{Machine, MachineConfig};

fn run(word: u32) -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0;
    execute(&mut m, Instruction(word)).unwrap();
    m
}

fn v(m: &Machine, i: u32) -> u128 {
    m.cores.active().state.v[i as usize]
}

fn setv(m: &mut Machine, i: u32, val: u128) {
    m.cores.active_mut().state.v[i as usize] = val;
}

#[test]
fn scalar_double_fdiv_decodes_and_executes_real_iboot_word() {
    let mut m = run(0xd503201f);
    setv(&mut m, 1, 10.0f64.to_bits() as u128);
    setv(&mut m, 2, 5.0f64.to_bits() as u128);
    let word = 0x1e62_1821; // fdiv d1, d1, d2
    assert_eq!(iphem_arm::disassemble(0, word), "fdivd\td1, d1, d2");
    execute(&mut m, Instruction(word)).unwrap();
    assert_eq!(f64::from_bits(v(&m, 1) as u64), 2.0);
}

#[test]
fn rev64_reverses_lanes_within_each_64_bit_half() {
    let mut m = run(0xd503201f);
    setv(&mut m, 0, 0x0f0e0d0c0b0a0908_0706050403020100);
    let word = 0x4e200800; // rev64 v0.16b, v0.16b
    assert_eq!(iphem_arm::disassemble(0, word), "rev64\tv0.16b, v0.16b");
    execute(&mut m, Instruction(word)).unwrap();
    assert_eq!(v(&m, 0), 0x08090a0b0c0d0e0f_0001020304050607);
}

#[test]
fn observed_rom_unsigned_right_and_left_vector_shifts_execute() {
    let mut m = run(0xd503201f);
    setv(&mut m, 7, 0x8000000000000001_123456789abcdef0);

    let ushr = 0x6f6d04f0; // ushr v16.2d, v7.2d, #19
    assert_eq!(iphem_arm::disassemble(0, ushr), "ushr\tv16.2d, v7.2d, #19");
    execute(&mut m, Instruction(ushr)).unwrap();
    assert_eq!(
        v(&m, 16),
        (0x8000000000000001u128 >> 19) << 64 | (0x123456789abcdef0u128 >> 19)
    );

    let shl = 0x4f4354f1; // shl v17.2d, v7.2d, #3
    assert_eq!(iphem_arm::disassemble(0, shl), "shl\tv17.2d, v7.2d, #3");
    execute(&mut m, Instruction(shl)).unwrap();
    assert_eq!(
        v(&m, 17),
        0x8000000000000001u128.wrapping_shl(3) << 64 | 0x123456789abcdef0u128.wrapping_shl(3)
    );
}

#[test]
fn sshll_ushll_widen_every_source_lane_and_select_high_half_on_q() {
    // ushll v3.8h, v1.8b, #0
    let ushll = 0x2f08a423;
    let mut m = run(0xd503201f);
    setv(&mut m, 1, 0x80_7f_00_fe_ff_02_01_ffu128);
    assert_eq!(iphem_arm::disassemble(0, ushll), "ushll\tv3.8h, v1.8b, #0");
    execute(&mut m, Instruction(ushll)).unwrap();
    let expected = [
        0x00ffu16, 0x0001, 0x0002, 0x00ff, 0x00fe, 0x0000, 0x007f, 0x0080,
    ];
    let mut want = 0u128;
    for (i, h) in expected.into_iter().enumerate() {
        want |= (h as u128) << (16 * i);
    }
    assert_eq!(v(&m, 3), want);

    // sshll v3.4s, v1.4h, #5 (sign-extend 16-bit lanes then shift left)
    let mut m = run(0xd503201f);
    setv(&mut m, 1, 0x0002_ffff_7fff_8000u128); // four 16-bit halfword lanes
    let sshll = 0x0f15a423;
    assert_eq!(iphem_arm::disassemble(0, sshll), "sshll\tv3.4s, v1.4h, #5");
    execute(&mut m, Instruction(sshll)).unwrap();
    let lanes = [0x8000u16 as i16, 0x7fff, -1, 2];
    let mut want = 0u128;
    for (i, h) in lanes.into_iter().enumerate() {
        let wide = ((h as i32) << 5) as u32 as u128;
        want |= wide << (32 * i);
    }
    assert_eq!(v(&m, 3), want);

    // ushll2 v3.4s, v1.8h, #1 widens the HIGH half of the 128-bit source.
    let mut m = run(0xd503201f);
    setv(&mut m, 1, 0x0008_0007_0006_0005_0004_0003_0002_0001u128);
    let ushll2 = 0x6f11a423;
    assert_eq!(
        iphem_arm::disassemble(0, ushll2),
        "ushll2\tv3.4s, v1.8h, #1"
    );
    execute(&mut m, Instruction(ushll2)).unwrap();
    let lanes = [0x0005u32, 0x0006, 0x0007, 0x0008]; // top four halfwords
    let mut want = 0u128;
    for (i, h) in lanes.into_iter().enumerate() {
        want |= ((h as u128) << 1) << (32 * i);
    }
    assert_eq!(v(&m, 3), want);

    // sshll2 v3.2d, v1.4s, #3 sign-extends the top two words to doubles.
    let mut m = run(0xd503201f);
    setv(&mut m, 1, 0x80000000_7fffffff_00000000_12345678u128);
    let sshll2 = 0x4f23a423;
    assert_eq!(
        iphem_arm::disassemble(0, sshll2),
        "sshll2\tv3.2d, v1.4s, #3"
    );
    execute(&mut m, Instruction(sshll2)).unwrap();
    let top = [0x7fffffffi32, 0x80000000u32 as i32];
    let mut want = 0u128;
    for (i, w) in top.into_iter().enumerate() {
        want |= (((w as i64) << 3) as u64 as u128) << (64 * i);
    }
    assert_eq!(v(&m, 3), want);
}

#[test]
fn tbl_every_index_wins_and_tbx_falls_back_to_destination_byte() {
    // tbl v16.16b, {v4.16b}, v0.16b (kernel halt word at 0xfffffff007fc331c).
    // v4 = bytes 0x00..0x0f, v0 = index lanes 0x00..0x0f -> identity copy.
    let tbl = 0x4e000090;
    assert_eq!(
        iphem_arm::disassemble(0, tbl),
        "tbl\tv16.16b, {v4.16b}, v0.16b"
    );
    let mut m = run(0xd503201f);
    let mut table = 0u128;
    for i in 0..16 {
        table |= (i as u128) << (8 * i);
    }
    setv(&mut m, 4, table);
    let mut indexes = 0u128;
    for i in 0..16 {
        indexes |= (i as u128) << (8 * i);
    }
    setv(&mut m, 0, indexes);
    execute(&mut m, Instruction(tbl)).unwrap();
    assert_eq!(v(&m, 16), table);

    // Only lane 0 indexes out of range; the rest take the identity index.
    // Lanes 1..15 map 0x01..0x0f and lane 0 -> 0: byte0 is zeroed (TBL).
    let mut m = run(0xd503201f);
    setv(&mut m, 4, table);
    setv(&mut m, 0, 0x0f0e_0d0c_0b0a_0908_0706_0504_0302_0100u128);
    execute(&mut m, Instruction(tbl)).unwrap();
    assert_eq!(v(&m, 16), 0x0f0e0d0c0b0a0908_0706050403020100u128);

    // tbx v16.16b, {v4.16b}, v0.16b keeps the original byte when out of range.
    let tbx = 0x4e001090;
    assert_eq!(
        iphem_arm::disassemble(0, tbx),
        "tbx\tv16.16b, {v4.16b}, v0.16b"
    );
    let mut m = run(0xd503201f);
    setv(&mut m, 4, table);
    // Lane 0 indexes 0x10 (out of range), lanes 1..15 index 0x01..0x0f.
    setv(&mut m, 0, 0x0f0e_0d0c_0b0a_0908_0706_0504_0302_0110u128);
    setv(&mut m, 16, 0xaaaaaaaa_bbbbbbbb_cccccccc_ddddddddu128);
    execute(&mut m, Instruction(tbx)).unwrap();
    // Lane 0 keeps its original byte 0xdd; the rest are the table bytes 0x01..0x0f.
    assert_eq!(v(&m, 16), 0x0f0e0d0c0b0a0908_07060504030201ddu128);

    // tbl with two table registers indexes the concatenated 32-byte stream.
    let tbl2 = 0x4e002090;
    assert_eq!(
        iphem_arm::disassemble(0, tbl2),
        "tbl\tv16.16b, {v4.16b, v5.16b}, v0.16b"
    );
    let mut m = run(0xd503201f);
    setv(&mut m, 4, table); // 0x00..0x0f
    setv(&mut m, 5, 0x22222222_33333333_44444444_55555555u128); // byte 3 = 0x55
    setv(&mut m, 0, 0x13131313_13131313_13131313_13131313u128); // every lane picks byte 0x13
    execute(&mut m, Instruction(tbl2)).unwrap();
    assert_eq!(v(&m, 16), 0x55555555_55555555_55555555_55555555u128);
}

#[test]
fn ext_extracts_every_valid_byte_window_with_destination_alias() {
    let n = std::array::from_fn::<_, 16, _>(|i| i as u8);
    let other = std::array::from_fn::<_, 16, _>(|i| (i + 32) as u8);
    for q in [false, true] {
        let len = if q { 16 } else { 8 };
        let joined = [&n[..len], &other[..len]].concat();
        for offset in 0..len {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            setv(&mut m, 0, u128::from_le_bytes(n));
            setv(&mut m, 1, u128::from_le_bytes(other));
            execute(
                &mut m,
                Instruction(0x2e010000 | ((q as u32) << 30) | ((offset as u32) << 11)),
            )
            .unwrap();
            let mut expected = [0; 16];
            expected[..len].copy_from_slice(&joined[offset..offset + len]);
            assert_eq!(v(&m, 0), u128::from_le_bytes(expected));
            assert_eq!(v(&m, 1), u128::from_le_bytes(other));
        }
    }
}

#[test]
fn dup_general_uses_integer_source_and_fills_every_lane() {
    for size in 0..4 {
        for q in [false, true] {
            if size == 3 && !q {
                continue;
            }
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            m.cores.active_mut().state.x[20] = 0xfffffff009145e70;
            setv(&mut m, 20, 0);
            setv(&mut m, 0, u128::MAX);
            let word = 0x0e000e80 | ((q as u32) << 30) | ((1 << size) << 16);
            execute(&mut m, Instruction(word)).unwrap();
            let bits = 8 << size;
            let value = 0xfffffff009145e70u128 & ((1u128 << bits) - 1);
            let mut expected = 0;
            for lane in 0..(if q { 128 } else { 64 }) / bits {
                expected |= value << (lane * bits);
            }
            assert_eq!(v(&m, 0), expected);
            assert_eq!(m.cores.active().state.x[20], 0xfffffff009145e70);
            assert_eq!(v(&m, 20), 0);
        }
    }
}

#[test]
fn movi_64bit_byte_mask_and_upper_lane() {
    let mut m = run(0x6f00e400); // movi v0.2d, #0
    assert_eq!(v(&m, 0), 0);
    setv(&mut m, 0, u128::MAX);
    execute(&mut m, Instruction(0x6f00e400)).unwrap();
    assert_eq!(v(&m, 0), 0);
    // imm8=0x81 selects the lowest and highest bytes of each 64-bit lane.
    execute(&mut m, Instruction(0x6f04e420)).unwrap();
    assert_eq!(v(&m, 0), 0xff000000000000ff_ff000000000000ff);
    execute(&mut m, Instruction(0x2f04e420)).unwrap();
    assert_eq!(v(&m, 0), 0xff000000000000ff);
}

#[test]
fn dup_and_store_initialize_both_queue_links() {
    use iphem_core::{BusRegion, SparseRam};
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x1000, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.x[20] = 0x1800;
    execute(&mut m, Instruction(0x4e080e80)).unwrap(); // dup v0.2d, x20
    execute(&mut m, Instruction(0x3d800280)).unwrap(); // str q0, [x20]
    assert_eq!(m.mem_read(0x1800, 8).unwrap(), 0x1800);
    assert_eq!(m.mem_read(0x1808, 8).unwrap(), 0x1800);
}

#[test]
fn movi_integer_modified_immediate_forms() {
    for (cmode, bits, value) in [
        (0, 32, 0x81u128),
        (2, 32, 0x8100),
        (4, 32, 0x810000),
        (6, 32, 0x81000000),
        (8, 16, 0x81),
        (10, 16, 0x8100),
        (12, 32, 0x81ff),
        (13, 32, 0x81ffff),
        (14, 8, 0x81),
    ] {
        for q in [false, true] {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            setv(&mut m, 0, u128::MAX);
            execute(
                &mut m,
                Instruction(0x0f040420 | ((q as u32) << 30) | (cmode << 12)),
            )
            .unwrap();
            let mut expected = 0;
            for lane in 0..(if q { 128 } else { 64 }) / bits {
                expected |= value << (lane * bits);
            }
            assert_eq!(v(&m, 0), expected);
        }
    }
    assert_eq!(v(&run(0x0f000420), 0), 0x0000000100000001);
}

#[test]
fn uminv_finds_zero_in_upper_half_and_clears_destination() {
    let mut m = run(0x6e31a801);
    setv(&mut m, 0, 0xff00ffffffffffff_ffffffffffffffff);
    setv(&mut m, 1, u128::MAX);
    execute(&mut m, Instruction(0x6e31a801)).unwrap();
    assert_eq!(v(&m, 1), 0);
    execute(&mut m, Instruction(0x2e31a801)).unwrap();
    assert_eq!(v(&m, 1), 255);
}

#[test]
fn cmhi_compares_unsigned_bytes_and_preserves_sources() {
    let mut m = run(0x6e213400); // cmhi v0.16b, v0.16b, v1.16b
    setv(&mut m, 0, 0x807fff00);
    setv(&mut m, 1, 0x7f800000);
    execute(&mut m, Instruction(0x6e213400)).unwrap();
    assert_eq!(v(&m, 0), 0xff00ff00);
    assert_eq!(v(&m, 1), 0x7f800000);
}

// -- Integer three-register ---------------------------------------------------

#[test]
fn cmeq_vector_lane_sizes_and_aliasing() {
    for size in 0..4 {
        for q in [false, true] {
            if size == 3 && !q {
                continue;
            }
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            let bits = 8 << size;
            let lanes = if q { 128 } else { 64 } / bits;
            let mask = (1u128 << bits) - 1;
            setv(&mut m, 0, u128::MAX);
            setv(&mut m, 1, u128::MAX ^ mask);
            let word = 0x2e218c01 | ((q as u32) << 30) | (size << 22);
            execute(&mut m, Instruction(word)).unwrap();
            let mut expected = 0;
            for lane in 1..lanes {
                expected |= mask << (lane * bits);
            }
            assert_eq!(v(&m, 1), expected);
            assert_eq!(v(&m, 0), u128::MAX);
        }
    }
}

#[test]
fn vector_literal_loads_preserve_integer_registers() {
    use iphem_core::{BusRegion, SparseRam};
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x1000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x1000, 0x1000)),
        })
        .unwrap();
    m.mem_write(0x1100, 8, 0x8877665544332211).unwrap();
    m.mem_write(0x1108, 8, 0xffeeddccbbaa0099).unwrap();
    for opc in 0..3 {
        for pc in [0x1000u64, 0x1200] {
            m.cores.active_mut().state.pc = pc;
            m.cores.active_mut().state.x[1] = 0x1234;
            setv(&mut m, 1, u128::MAX);
            let imm = (((0x1100i64 - pc as i64) >> 2) as u32) & 0x7ffff;
            execute(&mut m, Instruction(0x1c000001 | (opc << 30) | (imm << 5))).unwrap();
            let expected = match opc {
                0 => 0x44332211,
                1 => 0x8877665544332211,
                _ => 0xffeeddccbbaa00998877665544332211,
            };
            assert_eq!(v(&m, 1), expected);
            assert_eq!(m.cores.active().state.x[1], 0x1234);
        }
    }
}

#[test]
fn neon_add_4s_128() {
    // add v5.4s, v10.4s, v20.4s  => 0x4eb48545
    let mut m = run(0x4eb48545);
    // overwrite operands first
    setv(&mut m, 10, 0x00000001_00000002_00000003_00000004);
    setv(&mut m, 20, 0x10000000_20000000_30000000_40000000);
    execute(&mut m, Instruction(0x4eb48545)).unwrap();
    assert_eq!(v(&m, 5), 0x10000001_20000002_30000003_40000004);
}

#[test]
fn neon_sub_4s_wraps() {
    // sub v5.4s, v10.4s, v20.4s  => 0x6eb48545
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x00000000_00000000_00000000_00000001);
    setv(&mut m, 20, 0x00000000_00000000_00000000_00000002);
    execute(&mut m, Instruction(0x6eb48545)).unwrap();
    assert_eq!(v(&m, 5), 0x00000000_00000000_00000000_ffffffff);
}

#[test]
fn neon_add_2d() {
    // add v5.2d, v10.2d, v20.2d  => 0x4ef48545
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0xffffffffffffffff_0000000000000001);
    setv(&mut m, 20, 0x0000000000000001_0000000000000001);
    execute(&mut m, Instruction(0x4ef48545)).unwrap();
    assert_eq!(v(&m, 5), 0x0000000000000000_0000000000000002);
}

#[test]
fn neon_add_8h() {
    // add v5.8h, v10.8h, v20.8h  => 0x4e748545 (from earlier: add.8h opc=16,size=1)
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x0001000200030004_0005000600070008);
    setv(&mut m, 20, 0x0001000100010001_0001000100010001);
    execute(&mut m, Instruction(0x4e748545)).unwrap();
    assert_eq!(v(&m, 5), 0x0002000300040005_0006000700080009);
}

#[test]
fn neon_mul_4s() {
    // mul v5.4s, v10.4s, v20.4s  => 0x4eb49d45
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x00000002_00000003_00000004_00000005);
    setv(&mut m, 20, 0x00000006_00000007_00000008_00000009);
    execute(&mut m, Instruction(0x4eb49d45)).unwrap();
    assert_eq!(v(&m, 5), 0x0000000c_00000015_00000020_0000002d);
}

#[test]
fn neon_mla_accumulates_into_dest() {
    // mla v5.4s, v10.4s, v20.4s  => 0x4eb49545
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x00000002_00000003_00000004_00000005);
    setv(&mut m, 20, 0x00000006_00000007_00000008_00000009);
    setv(&mut m, 5, 0x00000010_00000010_00000010_00000010);
    execute(&mut m, Instruction(0x4eb49545)).unwrap();
    assert_eq!(v(&m, 5), 0x0000001c_00000025_00000030_0000003d);
}

#[test]
fn neon_mls_subtracts_into_dest() {
    // mls v5.4s, v10.4s, v20.4s  => 0x6eb49545
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x00000002_00000003_00000004_00000005);
    setv(&mut m, 20, 0x00000006_00000007_00000008_00000009);
    setv(&mut m, 5, 0x00000020_00000020_00000020_00000020);
    execute(&mut m, Instruction(0x6eb49545)).unwrap();
    assert_eq!(v(&m, 5), 0x00000014_0000000b_00000000_fffffff3);
}

// -- Bitwise ------------------------------------------------------------------

#[test]
fn neon_bitwise_and_orr_eor() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0);
    setv(&mut m, 20, 0xf0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0);
    // and v5.16b -> 0x4e341d45
    execute(&mut m, Instruction(0x4e341d45)).unwrap();
    assert_eq!(v(&m, 5), 0x00f0_00f0_00f0_00f0_00f0_00f0_00f0_00f0);
    // eor v5.16b -> 0x6e341d45
    execute(&mut m, Instruction(0x6e341d45)).unwrap();
    assert_eq!(v(&m, 5), 0xff00_ff00_ff00_ff00_ff00_ff00_ff00_ff00);
}

#[test]
fn neon_bitwise_bic_orn() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff);
    setv(&mut m, 20, 0xf0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0_f0f0);
    // bic v5.16b -> 0x4e741d45
    execute(&mut m, Instruction(0x4e741d45)).unwrap();
    assert_eq!(v(&m, 5), 0x0f0f_0f0f_0f0f_0f0f_0f0f_0f0f_0f0f_0f0f);
    // orn v5.16b -> 0x4ef41d45
    execute(&mut m, Instruction(0x4ef41d45)).unwrap();
    assert_eq!(v(&m, 5), 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff);
}

#[test]
fn neon_bitwise_bsl_bit_bif() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    // d = 0xA..A 1..1, n = 0xF0..0F, m = 0x00FF..FF00
    setv(&mut m, 5, 0xaaaaaaaa_aaaaaaaa_11111111_11111111);
    setv(&mut m, 10, 0xf0f0f0f0_f0f0f0f0_0f0f0f0f_0f0f0f0f);
    setv(&mut m, 20, 0x00ff00ff_00ff00ff_ff00ff00_ff00ff00);
    // bsl v5.16b -> 0x6e741d45 : select Vn where Vm=1 else Vd
    execute(&mut m, Instruction(0x6e741d45)).unwrap();
    let bsl = v(&m, 5);
    assert_eq!(
        bsl,
        (0xf0f0f0f0_f0f0f0f0_0f0f0f0f_0f0f0f0f & 0x00ff00ff_00ff00ff_ff00ff00_ff00ff00)
            | (0xaaaaaaaa_aaaaaaaa_11111111_11111111 & !0x00ff00ff_00ff00ff_ff00ff00_ff00ff00)
    );
    // bif v5.16b -> 0x6ef41d45 : select Vn where Vm=0 else Vd
    execute(&mut m, Instruction(0x6ef41d45)).unwrap();
    let bif = v(&m, 5);
    assert_eq!(
        bif,
        (0xf0f0f0f0_f0f0f0f0_0f0f0f0f_0f0f0f0f & !0x00ff00ff_00ff00ff_ff00ff00_ff00ff00)
            | (bsl & 0x00ff00ff_00ff00ff_ff00ff00_ff00ff00)
    );
}

// -- One-source ---------------------------------------------------------------

#[test]
fn neon_abs_neg() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0xffffffff_00000000_fffffffe_00000007);
    // abs v5.4s -> 0x4ea0b945 (Rn=10,Rd=5,size=2,Q=1)
    execute(&mut m, Instruction(0x4ea0b945)).unwrap();
    assert_eq!(v(&m, 5), 0x00000001_00000000_00000002_00000007);
    // neg v5.4s (U=1) -> 0x6ea0b945
    execute(&mut m, Instruction(0x6ea0b945)).unwrap();
    assert_eq!(v(&m, 5), 0x00000001_00000000_00000002_fffffff9);
}

#[test]
fn neon_not() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, 0x0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0);
    // not v5.16b (mvn) -> 0x6e205945
    execute(&mut m, Instruction(0x6e205945)).unwrap();
    assert_eq!(v(&m, 5), !0x0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0_0ff0);
}

// -- Vector FP ----------------------------------------------------------------

#[test]
fn neon_fadd_4s() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack4s(1.0, 2.0, 3.0, 4.0));
    setv(&mut m, 20, pack4s(10.0, 20.0, 30.0, 40.0));
    // fadd v5.4s, v10.4s, v20.4s -> 0x4e34d545
    execute(&mut m, Instruction(0x4e34d545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(11.0, 22.0, 33.0, 44.0));
}

#[test]
fn neon_fsub_fmul_4s() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack4s(5.0, 6.0, 7.0, 8.0));
    setv(&mut m, 20, pack4s(2.0, 3.0, 4.0, 5.0));
    // fsub v5.4s -> 0x4eb4d545
    execute(&mut m, Instruction(0x4eb4d545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(3.0, 3.0, 3.0, 3.0));
    // fmul v5.4s -> 0x6e34dd45
    execute(&mut m, Instruction(0x6e34dd45)).unwrap();
    assert_eq!(v(&m, 5), pack4s(10.0, 18.0, 28.0, 40.0));
}

#[test]
fn neon_fdiv_fmax_fmin_4s() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack4s(8.0, 9.0, 1.0, -2.0));
    setv(&mut m, 20, pack4s(2.0, 3.0, 5.0, -7.0));
    // fdiv v5.4s -> 0x6e34fd45
    execute(&mut m, Instruction(0x6e34fd45)).unwrap();
    assert_eq!(v(&m, 5), pack4s(4.0, 3.0, 0.2, 2.0 / 7.0));
    // fmax v5.4s -> 0x4e34f545
    execute(&mut m, Instruction(0x4e34f545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(8.0, 9.0, 5.0, -2.0));
    // fmin v5.4s -> 0x4eb4f545
    execute(&mut m, Instruction(0x4eb4f545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(2.0, 3.0, 1.0, -7.0));
}

#[test]
fn neon_fmla_accumulates() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack4s(2.0, 3.0, 4.0, 5.0));
    setv(&mut m, 20, pack4s(10.0, 10.0, 10.0, 10.0));
    setv(&mut m, 5, pack4s(100.0, 100.0, 100.0, 100.0));
    // fmla v5.4s -> 0x4e34cd45
    execute(&mut m, Instruction(0x4e34cd45)).unwrap();
    assert_eq!(v(&m, 5), pack4s(120.0, 130.0, 140.0, 150.0));
    // fmls v5.4s -> 0x4eb4cd45
    execute(&mut m, Instruction(0x4eb4cd45)).unwrap();
    assert_eq!(v(&m, 5), pack4s(100.0, 100.0, 100.0, 100.0));
}

#[test]
fn neon_fmaxnm_fminnm_ignore_nan() {
    let nan = f32::NAN.to_bits();
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(
        &mut m,
        10,
        (nan as u128)
            | ((9.0f32.to_bits() as u128) << 32)
            | ((nan as u128) << 64)
            | ((9.0f32.to_bits() as u128) << 96),
    );
    setv(
        &mut m,
        20,
        (5.0f32.to_bits() as u128)
            | ((nan as u128) << 32)
            | ((5.0f32.to_bits() as u128) << 64)
            | ((nan as u128) << 96),
    );
    // fmaxnm v5.4s -> 0x4e34c545
    execute(&mut m, Instruction(0x4e34c545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(5.0, 9.0, 5.0, 9.0));
    // fminnm v5.4s -> 0x4eb4c545
    execute(&mut m, Instruction(0x4eb4c545)).unwrap();
    assert_eq!(v(&m, 5), pack4s(5.0, 9.0, 5.0, 9.0));
}

#[test]
fn neon_fadd_2d() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack2d(1.5, 2.5));
    setv(&mut m, 20, pack2d(0.5, 0.25));
    // fadd v5.2d -> 0x4e74d545
    execute(&mut m, Instruction(0x4e74d545)).unwrap();
    assert_eq!(v(&m, 5), pack2d(2.0, 2.75));
}

#[test]
fn neon_fabs_fneg_fsqrt() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    setv(&mut m, 10, pack4s(-4.0, 9.0, -16.0, 25.0));
    // fneg v5.4s -> 0x4ea1f945? use fneg.4s.
    execute(&mut m, Instruction(0x6ea0f945)).unwrap();
    assert_eq!(v(&m, 5), pack4s(4.0, -9.0, 16.0, -25.0));
    // fabs v5.4s -> 0x4ea0f945
    execute(&mut m, Instruction(0x4ea0f945)).unwrap();
    assert_eq!(v(&m, 5), pack4s(4.0, 9.0, 16.0, 25.0));
    // fsqrt v5.4s -> 0x6ea1f945 (fsqrt.4s v5, v10)
    setv(&mut m, 10, pack4s(4.0, 9.0, 16.0, 25.0));
    execute(&mut m, Instruction(0x6ea1f945)).unwrap();
    assert_eq!(v(&m, 5), pack4s(2.0, 3.0, 4.0, 5.0));
}

fn pack4s(a: f32, b: f32, c: f32, d: f32) -> u128 {
    (a.to_bits() as u128)
        | ((b.to_bits() as u128) << 32)
        | ((c.to_bits() as u128) << 64)
        | ((d.to_bits() as u128) << 96)
}

fn pack4u(a: u32, b: u32, c: u32, d: u32) -> u128 {
    (a as u128) | ((b as u128) << 32) | ((c as u128) << 64) | ((d as u128) << 96)
}

fn pack2u(a: u64, b: u64) -> u128 {
    (a as u128) | ((b as u128) << 64)
}

#[test]
fn neon_ushl_vector_and_scalar() {
    // ushl v5.4s, v10.4s, v20.4s -> 0x6eb44545
    let mut m = run(0x6eb44545);
    setv(&mut m, 10, pack4u(0x3, 0x10000000, 0xabcd, 0xffffffff));
    setv(&mut m, 20, pack4u(1, 0xffffffff, 3, 0xfffffff0));
    execute(&mut m, Instruction(0x6eb44545)).unwrap();
    // lane0: 0x3<<1 = 0x6; lane1: 0x10000000 >> 1; lane2: 0xabcd<<3 masked;
    // lane3: count -16, unsigned right shift of 0xffffffff -> 0xffff.
    assert_eq!(v(&m, 5), pack4u(0x6, 0x08000000, 0xabcd << 3, 0xffff));

    // ushl d9, d14, d22 -> 0x7ef645c9, positive shift.
    let mut m = run(0x7ef645c9);
    setv(&mut m, 14, pack2u(0x0000_0000_ffff_ffff, 0));
    setv(&mut m, 22, pack2u(4, 0));
    execute(&mut m, Instruction(0x7ef645c9)).unwrap();
    assert_eq!(v(&m, 9), pack2u(0x0000_000f_ffff_fff0, 0));
}

#[test]
fn neon_sshl_vector_signed() {
    // sshl v6.2d, v11.2d, v21.2d -> 0x4ef54566
    let mut m = run(0x4ef54566);
    setv(&mut m, 11, pack2u(0xffffffffffffffff, 0x0010));
    setv(&mut m, 21, pack2u(0xfffffffffffffffe, 0xffffffffffffffff));
    execute(&mut m, Instruction(0x4ef54566)).unwrap();
    // lane0: -1 >> 2 (arithmetic) = all ones.
    // lane1: 0x10 >> 1 = 0x8.
    assert_eq!(v(&m, 6), pack2u(0xffffffffffffffff, 0x8));
}

#[test]
fn neon_unov_sd_lane_extract() {
    // mov w7, v12.s[2] -> 0x0e143d87
    let mut m = run(0x0e143d87);
    setv(&mut m, 12, pack4u(0xaa, 0xbb, 0xccdd, 0xeeff));
    execute(&mut m, Instruction(0x0e143d87)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 0xccdd);

    // mov x8, v13.d[1] -> 0x4e183da8
    let mut m = run(0x4e183da8);
    setv(&mut m, 13, pack2u(0x11112222, 0x33334444));
    execute(&mut m, Instruction(0x4e183da8)).unwrap();
    assert_eq!(m.cores.active().state.x[8], 0x33334444);
}

#[test]
fn neon_umov_byte_lane_extract() {
    // umov w7, v1.b[0] -> 0x0e013c27
    let mut m = run(0x0e013c27);
    setv(&mut m, 1, pack4u(0xaabbccdd, 0x11223344, 0, 0));
    execute(&mut m, Instruction(0x0e013c27)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 0xdd);

    // umov w7, v1.b[5] -> 0x0e0b3c27
    let mut m = run(0x0e0b3c27);
    setv(&mut m, 1, pack4u(0xaabbccdd, 0x11223344, 0, 0));
    execute(&mut m, Instruction(0x0e0b3c27)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 0x33);

    // umov w7, v1.b[15] -> 0x0e1f3c27
    let mut m = run(0x0e1f3c27);
    setv(
        &mut m,
        1,
        pack4u(0xaabbccdd, 0x11223344, 0x55667788, 0x99aabbcc),
    );
    execute(&mut m, Instruction(0x0e1f3c27)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 0x99);
}

#[test]
fn neon_umaxv_16b_finds_max_across_lanes() {
    // umaxv.16b b0, v4 -> 0x6e30a880
    let mut m = run(0x6e30a880);
    setv(
        &mut m,
        4,
        pack4u(0x01020304, 0xff060708, 0x090a0b0c, 0x0d0e0f10),
    );
    execute(&mut m, Instruction(0x6e30a880)).unwrap();
    // v0 (b8..b15 partial) holds the scalar result at byte 0, zero elsewhere
    let result = m.cores.active().state.v[0];
    assert_eq!((result & 0xff) as u8, 0xff);
}

#[test]
fn neon_umaxv_4s_finds_max_across_lanes() {
    // umaxv.4s s0, v4 -> 0x6eb0a880
    let mut m = run(0x6eb0a880);
    setv(&mut m, 4, pack4u(100, 500, 200, 300));
    execute(&mut m, Instruction(0x6eb0a880)).unwrap();
    assert_eq!((m.cores.active().state.v[0] & 0xffffffff) as u32, 500);
}

fn pack2d(a: f64, b: f64) -> u128 {
    (a.to_bits() as u128) | ((b.to_bits() as u128) << 64)
}

#[test]
fn fpcr_fpsr_and_scalar_fp_conversions() {
    let mut m = run(0xd503201f);
    m.cores.active_mut().state.fpcr = 0x00000000;
    m.cores.active_mut().state.fpsr = 0;
    m.cores.active_mut().state.x[1] = 42;
    assert_eq!(m.cores.active().state.fpcr, 0);
    assert_eq!(m.cores.active().state.fpsr, 0);
}
