use iphem_arm::{disassemble, execute, step, Instruction};
use iphem_core::{Machine, MachineConfig};
#[test]
fn nop_advances() {
    let mut m = Machine::new(MachineConfig {
        trace: true,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0x1000;
    execute(&mut m, Instruction(0xd503201f)).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1004)
}
#[test]
fn movz_add() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0;
    execute(&mut m, Instruction(0xd28000a0)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 5);
    execute(&mut m, Instruction(0x91001400)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 10)
}
#[test]
fn str_post_index_updates_base_and_writes() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x1000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.x[1] = 0x1122334455667788;
    m.cores.active_mut().state.x[3] = base;
    m.cores.active_mut().state.pc = 0x2000;
    execute(&mut m, Instruction(0xf8008461)).unwrap();
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x1122334455667788);
    assert_eq!(m.cores.active().state.x[3], base + 8)
}

#[test]
fn t8027_epilogue_ldp_offset_loads_saved_registers() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x1000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.sp = base;
    m.mem_write(base + 0x70, 8, 0x20).unwrap();
    m.mem_write(base + 0x78, 8, 0x19).unwrap();
    execute(&mut m, Instruction(0xa9474ff4)).unwrap();
    assert_eq!(m.cores.active().state.x[20], 0x20);
    assert_eq!(m.cores.active().state.x[19], 0x19);
    assert_eq!(m.cores.active().state.sp, base);
}

#[test]
fn cmp_immediate_does_not_write_sp() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.sp = 0x8000;
    m.cores.active_mut().state.x[0] = 0x5555_5555;
    execute(&mut m, Instruction(0x7100_041f)).unwrap();
    assert_eq!(m.cores.active().state.sp, 0x8000);
}

#[test]
fn t8027_mov_logical_immediate_replicates_byte() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pstate = 0xa000_0000;
    // MOV X3, #0x0101010101010101 (alias of ORR X3, XZR, #imm).
    execute(&mut m, Instruction(0xb200_c3e3)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0x0101_0101_0101_0101);
    assert_eq!(m.cores.active().state.pstate, 0xa000_0000);
}

#[test]
fn ands_logical_immediate_sets_flags() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[0] = 0xff00;
    // ANDS XZR, X0, #0xff.
    execute(&mut m, Instruction(0xf240_1c1f)).unwrap();
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b0100);
}

#[test]
fn extr_uses_imm6_and_concatenates_rn_above_rm() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[0] = 0x1122_3344_5566_7788;
    m.cores.active_mut().state.x[1] = 0x99aa_bbcc_ddee_ff00;

    // EXTR X2, X0, X1, #8.
    execute(&mut m, Instruction(0x93c1_2002)).unwrap();
    assert_eq!(m.cores.active().state.x[2], 0x8899_aabb_ccdd_eeff);

    m.cores.active_mut().state.x[3] = 0x1122_3344;
    m.cores.active_mut().state.x[4] = 0xaabb_ccdd;
    // EXTR W5, W3, W4, #12.
    execute(&mut m, Instruction(0x1384_3065)).unwrap();
    assert_eq!(m.cores.active().state.x[5], 0x344a_abbc);
}

#[test]
fn extr_rejects_reserved_n_and_32_bit_shift_encodings() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    // 64-bit EXTR requires N=1; 32-bit EXTR requires imm6<5>=0.
    assert_eq!(
        execute(&mut m, Instruction(0x9381_2002)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert_eq!(
        execute(&mut m, Instruction(0x1384_b065)),
        Err(iphem_arm::ExecError::Undefined)
    );
}

#[test]
fn logical_shifted_n_bit_inverts_operand_without_controlling_flags() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[1] = 0xff0f;
    m.cores.active_mut().state.x[2] = 0x00ff;
    m.cores.active_mut().state.pstate = 0xa000_0000;

    // ORR X0, X1, X2 must not change NZCV.
    execute(&mut m, Instruction(0xaa02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0xffff);
    assert_eq!(m.cores.active().state.pstate, 0xa000_0000);

    // BIC X3, X1, X2; ORN X4, XZR, X2; EON X5, XZR, X2.
    execute(&mut m, Instruction(0x8a22_0023)).unwrap();
    execute(&mut m, Instruction(0xaa22_03e4)).unwrap();
    execute(&mut m, Instruction(0xca22_03e5)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0xff00);
    assert_eq!(m.cores.active().state.x[4], !0x00ffu64);
    assert_eq!(m.cores.active().state.x[5], !0x00ffu64);

    // BICS X6, X1, X1 produces zero and sets Z.
    execute(&mut m, Instruction(0xea21_0026)).unwrap();
    assert_eq!(m.cores.active().state.x[6], 0);
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b0100);
}

#[test]
fn logical_shifted_32_bit_rejects_imm6_bit_five() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    // AND W0, W1, W2, LSL #32 is reserved.
    assert_eq!(
        execute(&mut m, Instruction(0x0a02_8020)),
        Err(iphem_arm::ExecError::Undefined)
    );
}

#[test]
fn add_extended_uses_full_imm3_and_cmp_does_not_write_sp() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[0] = 5;
    m.cores.active_mut().state.x[1] = 1;
    m.cores.active_mut().state.sp = 0x8000;

    // ADD X2, X0, W1, UXTW #4.
    execute(&mut m, Instruction(0x8b21_5002)).unwrap();
    assert_eq!(m.cores.active().state.x[2], 21);

    // CMP X0, W1, UXTW #4 (SUBS XZR, X0, ...).
    execute(&mut m, Instruction(0xeb21_501f)).unwrap();
    assert_eq!(m.cores.active().state.sp, 0x8000);
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b1000);
}

#[test]
fn add_shifted_and_extended_reject_reserved_shift_encodings() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    // ADD W0, W1, W2, LSL #32; ADD X0, X1, X2, ROR #1;
    // CMP X0, W1, UXTW #5.
    for instruction in [0x0b02_8020, 0x8bc2_0420, 0xeb21_541f] {
        assert_eq!(
            execute(&mut m, Instruction(instruction)),
            Err(iphem_arm::ExecError::Undefined)
        );
    }
}

#[test]
fn add_sub_flags_use_signed_overflow_not_carry_parity() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.x[0] = 5;
    // SUBS XZR, X0, #16: negative, no borrow-complement carry, no overflow.
    execute(&mut m, Instruction(0xf100_401f)).unwrap();
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b1000);

    m.cores.active_mut().state.x[0] = i64::MAX as u64;
    // ADDS XZR, X0, #1: signed positive overflow.
    execute(&mut m, Instruction(0xb100_041f)).unwrap();
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b1001);

    m.cores.active_mut().state.x[0] = i64::MIN as u64;
    // SUBS XZR, X0, #1: signed negative overflow with carry/no borrow.
    execute(&mut m, Instruction(0xf100_041f)).unwrap();
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b0011);
}

#[test]
fn dp1_reverse_and_count_encodings_match_aarch64() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[1] = 0x1122_3344;

    // REV16 W0, W1; REV W2, W1.
    execute(&mut m, Instruction(0x5ac0_0420)).unwrap();
    execute(&mut m, Instruction(0x5ac0_0822)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0x2211_4433);
    assert_eq!(m.cores.active().state.x[2], 0x4433_2211);

    m.cores.active_mut().state.x[4] = 0x1122_3344_5566_7788;
    // REV32 X3, X4; REV X5, X4.
    execute(&mut m, Instruction(0xdac0_0883)).unwrap();
    execute(&mut m, Instruction(0xdac0_0c85)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0x4433_2211_8877_6655);
    assert_eq!(m.cores.active().state.x[5], 0x8877_6655_4433_2211);

    m.cores.active_mut().state.x[7] = 0xf0;
    m.cores.active_mut().state.x[9] = 0;
    // CLZ X6, X7; CLS X8, X9.
    execute(&mut m, Instruction(0xdac0_10e6)).unwrap();
    execute(&mut m, Instruction(0xdac0_1528)).unwrap();
    assert_eq!(m.cores.active().state.x[6], 56);
    assert_eq!(m.cores.active().state.x[8], 63);
}

#[test]
fn dp2_divide_and_asrv_are_width_exact_and_panic_free() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.x[1] = (-9i64) as u64;
    m.cores.active_mut().state.x[2] = 2;
    // SDIV X0, X1, X2.
    execute(&mut m, Instruction(0x9ac2_0c20)).unwrap();
    assert_eq!(m.cores.active().state.x[0], (-4i64) as u64);

    m.cores.active_mut().state.x[1] = i64::MIN as u64;
    m.cores.active_mut().state.x[2] = u64::MAX;
    execute(&mut m, Instruction(0x9ac2_0c20)).unwrap();
    assert_eq!(m.cores.active().state.x[0], i64::MIN as u64);

    m.cores.active_mut().state.x[4] = 0xffff_ffff_0000_000a;
    m.cores.active_mut().state.x[5] = 0xaaaa_aaaa_0000_0002;
    // UDIV W3, W4, W5 ignores the high halves.
    execute(&mut m, Instruction(0x1ac5_0883)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 5);

    m.cores.active_mut().state.x[7] = 0xffff_ffff_8000_0000;
    m.cores.active_mut().state.x[8] = 1;
    // ASRV W6, W7, W8 sign-extends bit 31 and zero-extends the result register.
    execute(&mut m, Instruction(0x1ac8_28e6)).unwrap();
    assert_eq!(m.cores.active().state.x[6], 0xc000_0000);
}

#[test]
fn dp3_multiply_families_use_architectural_operand_widths() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.x[1] = 0xffff_ffff_0000_0003;
    m.cores.active_mut().state.x[2] = 0xaaaa_aaaa_0000_0004;
    m.cores.active_mut().state.x[3] = 5;
    // MADD W0, W1, W2, W3.
    execute(&mut m, Instruction(0x1b02_0c20)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 17);

    m.cores.active_mut().state.x[1] = u32::MAX as u64;
    m.cores.active_mut().state.x[2] = 2;
    m.cores.active_mut().state.x[3] = 10;
    // SMADDL X0, W1, W2, X3 uses signed W sources: 10 + (-1 * 2) = 8.
    execute(&mut m, Instruction(0x9b22_0c20)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 8);

    m.cores.active_mut().state.x[5] = u32::MAX as u64;
    m.cores.active_mut().state.x[6] = 2;
    m.cores.active_mut().state.x[7] = 1;
    // UMADDL X4, W5, W6, X7.
    execute(&mut m, Instruction(0x9ba6_1ca4)).unwrap();
    assert_eq!(m.cores.active().state.x[4], 1 + 2 * u32::MAX as u64);

    m.cores.active_mut().state.x[9] = u64::MAX;
    m.cores.active_mut().state.x[10] = 2;
    // SMULH X8, X9, X10: high half of -1 * 2 is all ones.
    execute(&mut m, Instruction(0x9b4a_7d28)).unwrap();
    assert_eq!(m.cores.active().state.x[8], u64::MAX);

    m.cores.active_mut().state.x[12] = u64::MAX;
    m.cores.active_mut().state.x[13] = 2;
    // UMULH X11, X12, X13.
    execute(&mut m, Instruction(0x9bcd_7d8b)).unwrap();
    assert_eq!(m.cores.active().state.x[11], 1);
}

#[test]
fn adc_sbc_take_carry_from_nzcv_c_not_v() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[1] = 1;
    m.cores.active_mut().state.x[2] = 2;

    m.cores.active_mut().state.nzcv = 0b0010;
    // ADC X0, X1, X2 with C=1.
    execute(&mut m, Instruction(0x9a02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 4);

    m.cores.active_mut().state.nzcv = 0b0001;
    // V=1 and C=0 must not contribute a carry-in.
    execute(&mut m, Instruction(0x9a02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 3);

    m.cores.active_mut().state.x[1] = 5;
    m.cores.active_mut().state.x[2] = 3;
    m.cores.active_mut().state.nzcv = 0b0010;
    // SBC X0, X1, X2 with C=1 computes 5-3.
    execute(&mut m, Instruction(0xda02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 2);

    m.cores.active_mut().state.nzcv = 0;
    // With C=0, SBC subtracts the additional borrow.
    execute(&mut m, Instruction(0xda02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 1);
}

#[test]
fn conditional_select_family_uses_true_rn_and_transformed_false_rm() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[1] = 0x11;
    m.cores.active_mut().state.x[2] = 0x22;
    m.cores.active_mut().state.pstate = 0x4000_0000;
    // CSEL X0, X1, X2, EQ takes Rn when Z=1.
    execute(&mut m, Instruction(0x9a82_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0x11);

    m.cores.active_mut().state.pstate = 0;
    m.cores.active_mut().state.x[4] = 0x44;
    m.cores.active_mut().state.x[5] = 0x55;
    // CSINC X3, X4, X5, EQ takes Rm+1 when false.
    execute(&mut m, Instruction(0x9a85_0483)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0x56);

    m.cores.active_mut().state.x[7] = 0x77;
    m.cores.active_mut().state.x[8] = 0x00ff;
    // CSINV X6, X7, X8, EQ takes NOT Rm when false.
    execute(&mut m, Instruction(0xda88_00e6)).unwrap();
    assert_eq!(m.cores.active().state.x[6], !0x00ffu64);

    m.cores.active_mut().state.x[10] = 10;
    m.cores.active_mut().state.x[11] = 3;
    // CSNEG W9, W10, W11, EQ takes -Wm and zero-extends.
    execute(&mut m, Instruction(0x5a8b_0549)).unwrap();
    assert_eq!(m.cores.active().state.x[9], 0xffff_fffd);
}

#[test]
fn logical_w_flags_take_n_from_bit_31() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[1] = 0xffff_ffff_8000_0000;
    m.cores.active_mut().state.x[2] = 0xffff_ffff;

    // ANDS W0, W1, W2.
    execute(&mut m, Instruction(0x6a02_0020)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0x8000_0000);
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b1000);

    m.cores.active_mut().state.x[2] = 0;
    execute(&mut m, Instruction(0x6a02_0020)).unwrap();
    assert_eq!((m.cores.active().state.pstate >> 28) & 0xf, 0b0100);
}

#[test]
fn logical_shifted_register_accepts_ror() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[8] = 0x0123_4567_89ab_cdef;
    m.cores.active_mut().state.x[9] = 0xfedc_ba98_7654_3210;

    // EOR X8, X9, X8, ROR #51 (SecureROM bulk-copy loop).
    execute(&mut m, Instruction(0xcac8_cd28)).unwrap();
    let rotated = 0x0123_4567_89ab_cdefu64.rotate_right(51);
    assert_eq!(m.cores.active().state.x[8], 0xfedc_ba98_7654_3210 ^ rotated);
}

#[test]
fn lse_atomics_use_source_and_result_registers_architecturally() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x4000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();

    m.mem_write(base, 8, 0x11).unwrap();
    m.cores.active_mut().state.x[1] = 0xaa;
    m.cores.active_mut().state.x[2] = 0xdead;
    m.cores.active_mut().state.x[3] = base;
    // SWP X1, X2, [X3].
    execute(&mut m, Instruction(0xf821_8062)).unwrap();
    assert_eq!(m.mem_read(base, 8).unwrap(), 0xaa);
    assert_eq!(m.cores.active().state.x[2], 0x11);

    m.mem_write(base + 8, 8, 10).unwrap();
    m.cores.active_mut().state.x[4] = 5;
    m.cores.active_mut().state.x[5] = 0xdead;
    m.cores.active_mut().state.x[6] = base + 8;
    // LDADD X4, X5, [X6].
    execute(&mut m, Instruction(0xf824_00c5)).unwrap();
    assert_eq!(m.mem_read(base + 8, 8).unwrap(), 15);
    assert_eq!(m.cores.active().state.x[5], 10);

    m.mem_write(base + 16, 4, 0xff).unwrap();
    m.cores.active_mut().state.x[7] = 0x0f;
    m.cores.active_mut().state.x[8] = 0xdead;
    m.cores.active_mut().state.x[9] = base + 16;
    // LDCLR W7, W8, [X9].
    execute(&mut m, Instruction(0xb827_1128)).unwrap();
    assert_eq!(m.mem_read(base + 16, 4).unwrap(), 0xf0);
    assert_eq!(m.cores.active().state.x[8], 0xff);

    m.mem_write(base + 24, 8, 0x33).unwrap();
    m.cores.active_mut().state.x[10] = 0x33;
    m.cores.active_mut().state.x[11] = 0x44;
    m.cores.active_mut().state.x[12] = base + 24;
    // CAS X10, X11, [X12].
    execute(&mut m, Instruction(0xc8aa_7d8b)).unwrap();
    assert_eq!(m.mem_read(base + 24, 8).unwrap(), 0x44);
    assert_eq!(m.cores.active().state.x[10], 0x33);
    assert_eq!(m.cores.active().state.x[11], 0x44);
}

#[test]
fn exclusive_store_requires_matching_live_monitor() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x6000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 8, 0x11).unwrap();
    m.cores.active_mut().state.x[2] = base;

    // LDXR X1, [X2].
    execute(&mut m, Instruction(0xc85f_7c41)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 0x11);
    assert_eq!(m.cores.active().exclusive_address, Some(base));

    m.cores.active_mut().state.x[3] = 0x22;
    // STXR W0, X3, [X2] succeeds once.
    execute(&mut m, Instruction(0xc800_7c43)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0);
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x22);
    assert_eq!(m.cores.active().exclusive_address, None);

    m.cores.active_mut().state.x[3] = 0x33;
    execute(&mut m, Instruction(0xc800_7c43)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 1);
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x22);

    execute(&mut m, Instruction(0xc85f_7c41)).unwrap();
    m.mem_write(base + 8, 8, 0xaa).unwrap();
    m.cores.active_mut().state.x[3] = 0x44;
    execute(&mut m, Instruction(0xc800_7c43)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 1);
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x22);
}

#[test]
fn clrex_clears_the_local_exclusive_monitor() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x5000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 8, 0x11).unwrap();
    m.cores.active_mut().state.x[2] = base;

    execute(&mut m, Instruction(0xc85f_7c41)).unwrap(); // LDXR X1,[X2]
    assert_eq!(m.cores.active().exclusive_address, Some(base));
    execute(&mut m, Instruction(0xd503_3f5f)).unwrap(); // CLREX
    assert_eq!(m.cores.active().exclusive_address, None);

    m.cores.active_mut().state.x[3] = 0x22;
    execute(&mut m, Instruction(0xc800_7c43)).unwrap(); // STXR W0,X3,[X2]
    assert_eq!(m.cores.active().state.x[0], 1);
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x11);
}

#[test]
fn exception_entry_clears_the_local_exclusive_monitor() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x7000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 8, 0x11).unwrap();
    m.cores.active_mut().state.x[2] = base;

    // LDXR X1, [X2] establishes the reservation.
    execute(&mut m, Instruction(0xc85f_7c41)).unwrap();
    assert_eq!(m.cores.active().exclusive_address, Some(base));

    // Any exception entry (here SVC #0) must put the monitor back in the
    // Open Access state, per ARM B2.3.7.
    assert!(execute(&mut m, Instruction(0xd400_0001)).is_err());
    assert_eq!(m.cores.active().exclusive_address, None);
    assert_eq!(m.cores.active().exclusive_size, 0);

    // The reserved STXR now fails rather than committing the stale write.
    m.cores.active_mut().state.x[3] = 0x22;
    execute(&mut m, Instruction(0xc800_7c43)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 1);
    assert_eq!(m.mem_read(base, 8).unwrap(), 0x11);
}

#[test]
fn exclusive_and_lse_accesses_require_natural_alignment() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x6000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;
    m.cores.active_mut().state.x[2] = base + 1;
    assert_eq!(
        execute(&mut m, Instruction(0xc85f_7c41)), // LDXR X1,[X2]
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.elr_el1, 0x10_000);
    assert_eq!(m.cores.active().state.far_el1, base + 1);
    assert_eq!(m.cores.active().state.esr_el1 & 0x3f, 0x21);

    m.cores.active_mut().state.pc = 0x11_000;
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.x[3] = base + 1;
    m.cores.active_mut().state.x[1] = 0xaa;
    assert_eq!(
        execute(&mut m, Instruction(0xf821_8062)), // SWP X1,X2,[X3]
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_ne!(m.cores.active().state.esr_el1 & (1 << 6), 0);
}

#[test]
fn sctlr_sa_and_sa0_enforce_sixteen_byte_sp_alignment() {
    use iphem_core::{BusRegion, ExceptionLevel, SparseRam};
    let base = 0x6000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.cores.active_mut().state.sp = base + 8;
    m.cores.active_mut().state.x[0] = 0x1122_3344_5566_7788;
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;

    // STR X0,[SP] is legal while SA is clear even though SP is not 16-byte aligned.
    execute(&mut m, Instruction(0xf900_03e0)).unwrap();
    assert_eq!(
        m.mem_read(base + 8, 8).unwrap(),
        m.cores.active().state.x[0]
    );

    m.cores.active_mut().state.pc = 0x11_000;
    m.cores.active_mut().state.sctlr_el1 = 1 << 3; // SA
    assert_eq!(
        execute(&mut m, Instruction(0xf900_03e0)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.elr_el1, 0x11_000);
    assert_eq!(m.cores.active().state.esr_el1 >> 26, 0x26);

    // SA0 independently controls SP checks for accesses executed at EL0.
    m.cores.active_mut().state.el = ExceptionLevel::El0;
    m.cores.active_mut().state.currentel = 0;
    m.cores.active_mut().state.pstate = 0;
    m.cores.active_mut().state.sp = base + 8;
    m.cores.active_mut().state.pc = 0x12_000;
    m.cores.active_mut().state.sctlr_el1 = 1 << 4; // SA0
    assert_eq!(
        execute(&mut m, Instruction(0xf900_03e0)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.elr_el1, 0x12_000);
    assert_eq!(m.cores.active().state.esr_el1 >> 26, 0x26);
    assert_eq!(m.cores.active().state.pc, 0x20_400);
}

#[test]
fn step_preserves_faulting_pc_and_distinguishes_fetch_faults() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x8000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.pc = base;
    assert_eq!(step(&mut m), Err(iphem_arm::ExecError::Fetch));
    assert_eq!(m.cores.active().state.pc, base);

    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 4, 0xffff_ffff).unwrap();
    assert_eq!(step(&mut m), Ok(()));
    assert_ne!(m.cores.active().state.pc, base);

    m.cores.active_mut().state.pc = base;
    // LDR X0, [X1] with an unmapped data address.
    m.mem_write(base, 4, 0xf940_0020).unwrap();
    m.cores.active_mut().state.x[1] = 0xdead_0000;
    assert_eq!(step(&mut m), Err(iphem_arm::ExecError::Data));
    assert_eq!(m.cores.active().state.pc, base);
}

#[test]
fn step_keeps_synchronous_exception_vector_pc() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0xa000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    // SVC #0.
    m.mem_write(base, 4, 0xd400_0001).unwrap();
    m.cores.active_mut().state.pc = base;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;

    assert_eq!(step(&mut m), Ok(()));
    assert_eq!(m.cores.active().state.pc, 0x20_200);
    assert_eq!(m.cores.active().state.elr_el1, base + 4);
}

#[test]
fn step_preserves_mmu_data_abort_vector_and_faulting_elr() {
    use iphem_core::{BusRegion, SparseRam};
    let code = 0xb000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: code,
            size: 0x1000,
            device: Box::new(SparseRam::new(code, 0x1000)),
        })
        .unwrap();
    m.bus
        .map(BusRegion {
            base: 0x4000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x4000, 0x1000)),
        })
        .unwrap();
    // Write LDR X0,[X1] before enabling an empty Stage-1 regime.
    m.mem_write(code, 4, 0xf940_0020).unwrap();
    m.cores.active_mut().state.pc = code;
    m.cores.active_mut().state.x[1] = 0xdead_0000;
    m.cores.active_mut().state.ttbr0_el1 = 0x4000;
    m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16) | (2 << 30);
    m.cores.active_mut().state.sctlr_el1 = 1;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;

    // Fetch itself also needs a mapping, so cache the code translation before
    // enabling the empty tables.
    m.tlb.insert(
        code,
        0,
        0,
        false,
        iphem_core::Translation {
            pa: code,
            attr_index: 0,
            writable: true,
            executable: true,
            global: true,
        },
    );
    assert_eq!(step(&mut m), Ok(()));
    assert_eq!(m.cores.active().state.pc, 0x20_200);
    assert_eq!(m.cores.active().state.elr_el1, code);
    assert_eq!(m.cores.active().state.far_el1, 0xdead_0000);
}

#[test]
fn literal_ldrsw_sign_extends_and_prfm_is_non_faulting_hint() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0xc000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base + 8, 4, 0xffff_ff80).unwrap();
    m.cores.active_mut().state.pc = base;

    // LDRSW X0, PC+8.
    execute(&mut m, Instruction(0x9800_0040)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0xffff_ffff_ffff_ff80);

    // PRFM PLDL1KEEP, PC+8 is a hint and must not perform a faulting data read.
    m.cores.active_mut().state.pc = 0xdead_0000;
    execute(&mut m, Instruction(0xd800_0040)).unwrap();
    assert_eq!(m.cores.active().state.pc, 0xdead_0004);
}

#[test]
fn bitfield_masks_support_merge_extract_sign_and_lsl_aliases() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.x[0] = 0xffff_0000_0000_0000;
    m.cores.active_mut().state.x[1] = 0xaa;
    // BFI X0, X1, #8, #8 (BFM X0, X1, #56, #7).
    execute(&mut m, Instruction(0xb378_1c20)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0xffff_0000_0000_aa00);

    m.cores.active_mut().state.x[3] = 0xabcde;
    // UBFX X2, X3, #12, #8.
    execute(&mut m, Instruction(0xd34c_4c62)).unwrap();
    assert_eq!(m.cores.active().state.x[2], 0xab);

    m.cores.active_mut().state.x[5] = 0x8000;
    // SBFX X4, X5, #8, #8.
    execute(&mut m, Instruction(0x9348_3ca4)).unwrap();
    assert_eq!(m.cores.active().state.x[4], 0xffff_ffff_ffff_ff80);

    m.cores.active_mut().state.x[7] = 0x0123_4567_89ab_cdef;
    // LSL X6, X7, #4 (UBFM X6, X7, #60, #59).
    execute(&mut m, Instruction(0xd37c_ece6)).unwrap();
    assert_eq!(m.cores.active().state.x[6], 0x1234_5678_9abc_def0);
}

#[test]
fn architectural_currentel_nzcv_and_daif_register_layouts() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El2;
    // MRS X0, CurrentEL.
    execute(&mut m, Instruction(0xd538_4240)).unwrap();
    assert_eq!(m.cores.active().state.x[0], 0b1000);

    m.cores.active_mut().state.nzcv = 0b1010;
    m.cores.active_mut().state.pstate = 0xa000_0000;
    // MRS X1, NZCV.
    execute(&mut m, Instruction(0xd53b_4201)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 0xa000_0000);

    m.cores.active_mut().state.x[2] = 0x5000_0000;
    // MSR NZCV, X2.
    execute(&mut m, Instruction(0xd51b_4202)).unwrap();
    assert_eq!(m.cores.active().state.nzcv, 0b0101);
    assert_eq!(m.cores.active().state.pstate & 0xf000_0000, 0x5000_0000);

    m.cores.active_mut().state.daif = 0b1010;
    // MRS X3, DAIF.
    execute(&mut m, Instruction(0xd53b_4223)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0b1010 << 6);

    m.cores.active_mut().state.x[4] = 0b0101 << 6;
    // MSR DAIF, X4.
    execute(&mut m, Instruction(0xd51b_4224)).unwrap();
    assert_eq!(m.cores.active().state.daif, 0b0101);
}

#[test]
fn eret_restores_spsr_state_and_selected_stack_pointer() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.elr_el1 = 0x1234_5678;
    m.cores.active_mut().state.spsr_el1 = 0xa000_0000 | (0b0101 << 6);
    m.cores.active_mut().state.sp = 0x9000;
    m.cores.active_mut().state.sp_el[0] = 0x8000;
    m.cores.active_mut().state.sp_el[1] = 0x9000;

    execute(&mut m, Instruction(0xd69f_03e0)).unwrap();

    assert_eq!(m.cores.active().state.pc, 0x1234_5678);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El0);
    assert_eq!(m.cores.active().state.nzcv, 0b1010);
    assert_eq!(m.cores.active().state.daif, 0b0101);
    assert_eq!(m.cores.active().state.sp, 0x8000);
    assert_eq!(m.cores.active().state.sp_el[1], 0x9000);
    assert!(!m.cores.active().state.spsel);
}

#[test]
fn spsel_preserves_each_stack_bank_and_mrs_reports_selection() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = 0x1110;
    m.cores.active_mut().state.sp_el[0] = 0x2220;
    m.cores.active_mut().state.sp_el[1] = 0x1110;

    // MSR SPSel, #0; MRS X0, SPSel.
    execute(&mut m, Instruction(0xd500_40bf)).unwrap();
    execute(&mut m, Instruction(0xd538_4200)).unwrap();
    assert_eq!(m.cores.active().state.sp, 0x2220);
    assert_eq!(m.cores.active().state.sp_el[1], 0x1110);
    assert_eq!(m.cores.active().state.x[0], 0);

    m.cores.active_mut().state.sp = 0x3330;
    // MSR SPSel, #1; MRS X1, SPSel.
    execute(&mut m, Instruction(0xd500_41bf)).unwrap();
    execute(&mut m, Instruction(0xd538_4201)).unwrap();
    assert_eq!(m.cores.active().state.sp_el[0], 0x3330);
    assert_eq!(m.cores.active().state.sp, 0x1110);
    assert_eq!(m.cores.active().state.x[1], 1);
}

#[test]
fn lower_el_sync_uses_lower_vector_and_banks_stack_until_eret() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.currentel = 0;
    m.cores.active_mut().state.spsel = false;
    m.cores.active_mut().state.sp = 0x7000;
    m.cores.active_mut().state.sp_el[0] = 0x7000;
    m.cores.active_mut().state.sp_el[1] = 0x9000;
    m.cores.active_mut().state.pc = 0x1000;
    m.cores.active_mut().state.vbar_el1 = 0x20_000;

    assert_eq!(
        execute(&mut m, Instruction(0xd400_0001)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.pc, 0x20_400);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El1);
    assert!(m.cores.active().state.spsel);
    assert_eq!(m.cores.active().state.sp, 0x9000);
    assert_eq!(m.cores.active().state.sp_el[0], 0x7000);

    m.cores.active_mut().state.sp = 0x9100;
    execute(&mut m, Instruction(0xd69f_03e0)).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1004);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El0);
    assert!(!m.cores.active().state.spsel);
    assert_eq!(m.cores.active().state.sp, 0x7000);
    assert_eq!(m.cores.active().state.sp_el[1], 0x9100);
}

#[test]
fn supervisor_hypervisor_and_monitor_calls_populate_esr() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.vbar_el1 = 0x20_000;
    m.cores.active_mut().state.vbar_el2 = 0x30_000;
    m.cores.active_mut().state.vbar_el3 = 0x40_000;

    for (instruction, ec, target, vector) in [
        (
            0xd400_2461,
            0x15u64,
            iphem_core::ExceptionLevel::El1,
            0x20_200,
        ),
        (
            0xd400_2462,
            0x16u64,
            iphem_core::ExceptionLevel::El2,
            0x30_400,
        ),
        (
            0xd400_2463,
            0x17u64,
            iphem_core::ExceptionLevel::El3,
            0x40_400,
        ),
    ] {
        m.cores.active_mut().state.pc = 0x10_000;
        m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
        m.cores.active_mut().state.currentel = 1;
        m.cores.active_mut().state.spsel = true;
        m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
        assert_eq!(
            execute(&mut m, Instruction(instruction)),
            Err(iphem_arm::ExecError::Synchronous)
        );
        let (esr, elr) = match target {
            iphem_core::ExceptionLevel::El1 => (
                m.cores.active().state.esr_el1,
                m.cores.active().state.elr_el1,
            ),
            iphem_core::ExceptionLevel::El2 => (
                m.cores.active().state.esr_el2,
                m.cores.active().state.elr_el2,
            ),
            iphem_core::ExceptionLevel::El3 => (
                m.cores.active().state.esr_el3,
                m.cores.active().state.elr_el3,
            ),
            iphem_core::ExceptionLevel::El0 => unreachable!(),
        };
        assert_eq!(esr, (ec << 26) | 0x123);
        assert_eq!(elr, 0x10_004);
        assert_eq!(m.cores.active().state.el, target);
        assert_eq!(m.cores.active().state.pc, vector);
    }
}

#[test]
fn hvc_and_smc_never_drop_privilege_and_are_unavailable_at_el0() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    assert_eq!(
        execute(&mut m, Instruction(0xd400_0002)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert_eq!(
        execute(&mut m, Instruction(0xd400_0003)),
        Err(iphem_arm::ExecError::Undefined)
    );

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.hcr_el2 = 1 << 29; // HCD
    assert_eq!(
        execute(&mut m, Instruction(0xd400_0002)),
        Err(iphem_arm::ExecError::Undefined)
    );

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.vbar_el3 = 0x30_000;
    m.cores.active_mut().state.pc = 0x10_000;
    assert_eq!(
        execute(&mut m, Instruction(0xd400_2462)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El3);
    assert_eq!(m.cores.active().state.pc, 0x30_200);
    assert_eq!(m.cores.active().state.elr_el3, 0x10_004);
    assert_eq!(m.cores.active().state.esr_el3, (0x16u64 << 26) | 0x123);
}

#[test]
fn hcr_tvm_and_trvm_trap_el1_vm_register_access_to_el2() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el2 = 0x20_000;
    m.cores.active_mut().state.hcr_el2 = 1 << 26; // TVM
    m.cores.active_mut().state.x[0] = 1;

    // MSR SCTLR_EL1, X0.
    assert_eq!(
        execute(&mut m, Instruction(0xd518_1000)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.sctlr_el1, 0);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x20_400);
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0x18);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.pc = 0x11_000;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.hcr_el2 = 1 << 30; // TRVM
    m.cores.active_mut().state.ttbr0_el1 = 0xfeed_0000;
    m.cores.active_mut().state.x[1] = 0xaaaa;
    // MRS X1, TTBR0_EL1.
    assert_eq!(
        execute(&mut m, Instruction(0xd538_2001)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.x[1], 0xaaaa);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x20_400);
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0x18);
}

#[test]
fn el2_translation_registers_round_trip_and_reject_el1() {
    fn msr(key: u16, rt: u32) -> Instruction {
        Instruction(0xd500_0000 | (3 << 19) | (u32::from(key) << 5) | rt)
    }
    fn mrs(key: u16, rt: u32) -> Instruction {
        Instruction(0xd500_0000 | (7 << 19) | (u32::from(key) << 5) | rt)
    }

    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El2;
    m.cores.active_mut().state.currentel = 2;
    for (key, value) in [
        (0xe108, 0x1234_5000),      // VTTBR_EL2
        (0xe10a, 0x8000_0040),      // VTCR_EL2
        (0xe304, 0x0000_abcd_1230), // HPFAR_EL2
    ] {
        m.cores.active_mut().state.x[0] = value;
        execute(&mut m, msr(key, 0)).unwrap();
        m.cores.active_mut().state.x[1] = 0;
        execute(&mut m, mrs(key, 1)).unwrap();
        assert_eq!(m.cores.active().state.x[1], value);
    }

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    assert_eq!(
        execute(&mut m, mrs(0xe304, 2)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
}

#[test]
fn higher_el_sctlr_registers_round_trip_and_control_sp_alignment() {
    fn msr(key: u16, rt: u32) -> Instruction {
        Instruction(0xd500_0000 | (3 << 19) | (u32::from(key) << 5) | rt)
    }
    fn mrs(key: u16, rt: u32) -> Instruction {
        Instruction(0xd500_0000 | (7 << 19) | (u32::from(key) << 5) | rt)
    }

    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;
    for (key, value) in [(0xe080, 0x8), (0xf080, 0xa)] {
        m.cores.active_mut().state.x[0] = value;
        execute(&mut m, msr(key, 0)).unwrap();
        execute(&mut m, mrs(key, 1)).unwrap();
        assert_eq!(m.cores.active().state.x[1], value);
    }

    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el3 = 0x20_000;
    m.cores.active_mut().state.sp = 0x6008;
    assert_eq!(
        execute(&mut m, Instruction(0xf900_03e0)), // STR X0,[SP]
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.esr_el3 >> 26, 0x26);
    assert_eq!(m.cores.active().state.elr_el3, 0x10_000);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    assert_eq!(
        execute(&mut m, mrs(0xe080, 2)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
}

#[test]
fn apple_apctl_el1_has_nonzero_default_capability_bits() {
    fn mrs(key: u16, rt: u32) -> Instruction {
        Instruction(0xd500_0000 | (7 << 19) | (u32::from(key) << 5) | rt)
    }

    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;

    execute(&mut m, mrs(0xe784, 0)).unwrap();
    assert_ne!(m.cores.active().state.x[0] & !0x3f, 0);
    execute(&mut m, Instruction(0x927f_0001)).unwrap();
    assert_ne!(m.cores.active().state.x[1], 0);
}

#[test]
fn pointer_auth_data_forms_execute_as_identity_until_keys_exist() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.x[0] = 0xfeed_face_cafe_babe;
    m.cores.active_mut().state.x[2] = 0x1111_2222_3333_4444;
    m.cores.active_mut().state.x[3] = 0x5555_6666_7777_8888;
    m.cores.active_mut().state.x[4] = 0x9999_aaaa_bbbb_cccc;
    m.cores.active_mut().state.x[17] = 0x1234_5678_9abc_def0;

    execute(&mut m, Instruction(0xdac1_0011)).unwrap();
    execute(&mut m, Instruction(0xdac1_0402)).unwrap();
    execute(&mut m, Instruction(0xdac1_0803)).unwrap();
    execute(&mut m, Instruction(0xdac1_0c04)).unwrap();

    assert_eq!(m.cores.active().state.x[17], 0x1234_5678_9abc_def0);
    assert_eq!(m.cores.active().state.x[2], 0x1111_2222_3333_4444);
    assert_eq!(m.cores.active().state.x[3], 0x5555_6666_7777_8888);
    assert_eq!(m.cores.active().state.x[4], 0x9999_aaaa_bbbb_cccc);
}

#[test]
fn translation_control_writes_invalidate_cached_entries() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.tlb.insert(
        0x1000,
        0,
        0,
        false,
        iphem_core::Translation {
            pa: 0x2000,
            attr_index: 0,
            writable: true,
            executable: true,
            global: true,
        },
    );
    assert_eq!(m.tlb.get_entries().len(), 1);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El2;
    m.cores.active_mut().state.currentel = 2;
    m.cores.active_mut().state.x[0] = 0x1234_5000;
    let msr_vttbr_el2 = Instruction(0xd500_0000 | (3 << 19) | (0xe108 << 5));
    execute(&mut m, msr_vttbr_el2).unwrap();
    assert!(m.tlb.get_entries().is_empty());
}

#[test]
fn eret_uses_the_current_exception_levels_register_bank() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = 0x3300;
    m.cores.active_mut().state.sp_el[3] = 0x3300;
    m.cores.active_mut().state.sp_el[1] = 0x1100;
    m.cores.active_mut().state.elr_el1 = 0xdead;
    m.cores.active_mut().state.elr_el3 = 0x1234;
    m.cores.active_mut().state.spsr_el3 = 0x5; // EL1h

    execute(&mut m, Instruction(0xd69f_03e0)).unwrap();
    assert_eq!(m.cores.active().state.pc, 0x1234);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El1);
    assert_eq!(m.cores.active().state.sp, 0x1100);
    assert_eq!(m.cores.active().state.sp_el[3], 0x3300);
}

#[test]
fn el2_and_el3_exception_system_registers_round_trip() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;

    // MSR VBAR_EL3, X0; MRS X1, VBAR_EL3.
    m.cores.active_mut().state.x[0] = 0x1234_0000;
    execute(&mut m, Instruction(0xd51e_c000)).unwrap();
    execute(&mut m, Instruction(0xd53e_c001)).unwrap();
    assert_eq!(m.cores.active().state.vbar_el3, 0x1234_0000);
    assert_eq!(m.cores.active().state.x[1], 0x1234_0000);

    // MSR SPSR_EL2, X2; MRS X3, SPSR_EL2.
    m.cores.active_mut().state.x[2] = 0xa000_03c5;
    execute(&mut m, Instruction(0xd51c_4003)).unwrap();
    execute(&mut m, Instruction(0xd53c_4003)).unwrap();
    assert_eq!(m.cores.active().state.spsr_el2, 0xa000_03c5);
    assert_eq!(m.cores.active().state.x[3], 0xa000_03c5);

    // MSR ELR_EL3, X4; MRS X5, ELR_EL3.
    m.cores.active_mut().state.x[4] = 0xfeed_face;
    execute(&mut m, Instruction(0xd51e_4024)).unwrap();
    execute(&mut m, Instruction(0xd53e_4025)).unwrap();
    assert_eq!(m.cores.active().state.elr_el3, 0xfeed_face);
    assert_eq!(m.cores.active().state.x[5], 0xfeed_face);
}

#[test]
fn sp_el2_system_register_round_trips_and_updates_active_bank() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = 0x8300;
    m.cores.active_mut().state.x[0] = 0x8200;

    // MSR SP_EL2, X0; MRS X1, SP_EL2.
    execute(&mut m, Instruction(0xd51e_4100)).unwrap();
    execute(&mut m, Instruction(0xd53e_4101)).unwrap();

    assert_eq!(m.cores.active().state.sp_el[2], 0x8200);
    assert_eq!(m.cores.active().state.sp, 0x8300);
    assert_eq!(m.cores.active().state.x[1], 0x8200);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El2;
    m.cores.active_mut().state.currentel = 2;
    m.cores.active_mut().state.spsel = false;
    m.cores.active_mut().state.sp_el[0] = 0x8000;
    m.cores.active_mut().state.sp = 0x8000;
    // MSR SPSel, #1 selects the bank configured by EL3.
    execute(&mut m, Instruction(0xd500_41bf)).unwrap();
    assert_eq!(m.cores.active().state.sp, 0x8200);
}

#[test]
fn system_register_access_rejects_lower_exception_levels() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.x[0] = 0xaaaa;

    // MRS X0, VBAR_EL2 is UNDEFINED at EL1 and traps to EL2 (EC=0).
    assert_eq!(
        execute(&mut m, Instruction(0xd53c_c000)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.x[0], 0xaaaa);
    assert_eq!(m.cores.active().state.esr_el2 >> 26, 0);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(
        m.cores.active().state.pc,
        m.cores.active().state.vbar_el2 + 0x400
    );

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.x[1] = 0x1234_0000;
    // MSR VBAR_EL1, X1 is UNDEFINED at EL0 and traps to EL1 (EC=0).
    assert_eq!(
        execute(&mut m, Instruction(0xd518_c001)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.vbar_el1, 0);
    assert_eq!(m.cores.active().state.esr_el1 >> 26, 0);
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El1);
    assert_eq!(
        m.cores.active().state.pc,
        m.cores.active().state.vbar_el1 + 0x400
    );

    // MRS X2, TPIDR_EL0 remains accessible at EL0.
    m.cores.active_mut().state.tpidr_el0 = 0x55aa;
    execute(&mut m, Instruction(0xd53b_d042)).unwrap();
    assert_eq!(m.cores.active().state.x[2], 0x55aa);
}

#[test]
fn read_only_system_register_writes_are_undefined() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.x[0] = 0xffff;

    // MSR CurrentEL, X0 and MSR MIDR_EL1, X0 are architecturally read-only.
    assert_eq!(
        execute(&mut m, Instruction(0xd518_4240)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert_eq!(
        execute(&mut m, Instruction(0xd518_0000)),
        Err(iphem_arm::ExecError::Undefined)
    );

    // CNTFRQ_EL0 is readable below EL3 but writable only at EL3.
    assert_eq!(
        execute(&mut m, Instruction(0xd51b_e000)),
        Err(iphem_arm::ExecError::Undefined)
    );
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    execute(&mut m, Instruction(0xd51b_e000)).unwrap();
    execute(&mut m, Instruction(0xd53b_e001)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 0xffff);
}

#[test]
fn el0_daif_immediates_are_allowed_but_spsel_is_privileged() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.currentel = 0;
    m.cores.active_mut().state.spsel = false;
    m.cores.active_mut().state.daif = 0;

    // AArch64 permits DAIFSet/DAIFClr immediates at EL0.
    execute(&mut m, Instruction(0xd503_42df)).unwrap(); // DAIFSet #2
    assert_eq!(m.cores.active().state.daif, 2);
    execute(&mut m, Instruction(0xd503_42ff)).unwrap(); // DAIFClr #2
    assert_eq!(m.cores.active().state.daif, 0);

    // MSR SPSel, #1 remains privileged.
    assert_eq!(
        execute(&mut m, Instruction(0xd500_41bf)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert!(!m.cores.active().state.spsel);

    // Register-form DAIF access is also privileged in this model (UMA=0);
    // it traps to EL1 as an UNDEFINED-instruction exception (EC=0).
    assert_eq!(
        execute(&mut m, Instruction(0xd53b_4220)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.esr_el1 >> 26, 0);
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.currentel = 0;
    m.cores.active_mut().state.esr_el1 = 0;
    assert_eq!(
        execute(&mut m, Instruction(0xd51b_4220)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El1);
}

#[test]
fn pan_and_uao_immediates_update_architectural_pstate_bits() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.tlb.insert(
        0x1000,
        0,
        0,
        false,
        iphem_core::Translation {
            pa: 0x2000,
            attr_index: 0,
            writable: true,
            executable: true,
            global: true,
        },
    );
    execute(&mut m, Instruction(0xd500_419f)).unwrap(); // MSR PAN, #1
    execute(&mut m, Instruction(0xd500_417f)).unwrap(); // MSR UAO, #1
    assert_ne!(m.cores.active().state.pstate & (1 << 22), 0);
    assert_ne!(m.cores.active().state.pstate & (1 << 23), 0);
    assert!(m.tlb.get_entries().is_empty());

    execute(&mut m, Instruction(0xd500_409f)).unwrap(); // MSR PAN, #0
    execute(&mut m, Instruction(0xd500_407f)).unwrap(); // MSR UAO, #0
    assert_eq!(m.cores.active().state.pstate & ((1 << 22) | (1 << 23)), 0);
}

#[test]
fn el0_cannot_execute_modeled_cache_or_tlb_system_operations() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x4000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 8, u64::MAX).unwrap();
    m.cores.active_mut().state.x[3] = base;
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;

    // EL0 access is controlled by SCTLR_EL1.DZE and DCZID_EL0.DZP.
    assert_eq!(
        execute(&mut m, Instruction(0xd50b_7423)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert_eq!(m.mem_read(base, 8).unwrap(), u64::MAX);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    execute(&mut m, Instruction(0xd50b_7423)).unwrap();
    assert_eq!(m.mem_read(base, 8).unwrap(), 0);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.sctlr_el1 |= 1 << 14; // DZE: permit EL0 DC ZVA.
    m.mem_write(base, 8, u64::MAX).unwrap();
    execute(&mut m, Instruction(0xd50b_7423)).unwrap();
    assert_eq!(m.mem_read(base, 8).unwrap(), 0);

    m.cores
        .active_mut()
        .state
        .vendor_sysregs
        .insert(0xd807, (1 << 4) | 4); // DZP.
    m.mem_write(base, 8, u64::MAX).unwrap();
    assert_eq!(
        execute(&mut m, Instruction(0xd50b_7423)),
        Err(iphem_arm::ExecError::Undefined)
    );
    assert_eq!(m.mem_read(base, 8).unwrap(), u64::MAX);
}

#[test]
fn el0_dc_zva_obeys_dze_dzp_and_reported_block_size() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x4000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    for offset in (0..128).step_by(8) {
        m.mem_write(base + offset, 8, u64::MAX).unwrap();
    }
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El0;
    m.cores.active_mut().state.x[3] = base + 0x25;
    m.cores.active_mut().state.sctlr_el1 |= 1 << 14; // DZE permits EL0 use.
    m.cores.active_mut().state.vendor_sysregs.insert(0xd807, 3); // 4 << BS = 32 bytes.

    execute(&mut m, Instruction(0xd53b_00e4)).unwrap(); // MRS X4, DCZID_EL0
    assert_eq!(m.cores.active().state.x[4], 3);
    execute(&mut m, Instruction(0xd50b_7423)).unwrap(); // DC ZVA, X3
    assert_eq!(m.mem_read(base + 0x18, 8).unwrap(), u64::MAX);
    assert_eq!(m.mem_read(base + 0x20, 8).unwrap(), 0);
    assert_eq!(m.mem_read(base + 0x38, 8).unwrap(), 0);
    assert_eq!(m.mem_read(base + 0x40, 8).unwrap(), u64::MAX);

    m.cores
        .active_mut()
        .state
        .vendor_sysregs
        .insert(0xd807, (1 << 4) | 3); // DZP disables it.
    assert_eq!(
        execute(&mut m, Instruction(0xd50b_7423)),
        Err(iphem_arm::ExecError::Undefined)
    );
}

#[test]
fn hcr_and_scr_interrupt_routing_registers_round_trip() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El3;
    m.cores.active_mut().state.currentel = 3;

    // MSR HCR_EL2, X0; MRS X1, HCR_EL2.
    m.cores.active_mut().state.x[0] = 1 << 4;
    execute(&mut m, Instruction(0xd51c_1100)).unwrap();
    execute(&mut m, Instruction(0xd53c_1101)).unwrap();
    assert_eq!(m.cores.active().state.hcr_el2, 1 << 4);
    assert_eq!(m.cores.active().state.x[1], 1 << 4);

    // MSR SCR_EL3, X2; MRS X3, SCR_EL3.
    m.cores.active_mut().state.x[2] = 1 << 1;
    execute(&mut m, Instruction(0xd51e_1102)).unwrap();
    execute(&mut m, Instruction(0xd53e_1103)).unwrap();
    assert_eq!(m.cores.active().state.scr_el3, 1 << 1);
    assert_eq!(m.cores.active().state.x[3], 1 << 1);
}

#[test]
fn msr_spsel_register_form_switches_banked_stack() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp_el[0] = 0x4000;
    m.cores.active_mut().state.sp_el[1] = 0x8000;
    m.cores.active_mut().state.sp = 0x8000;

    // MRS X1, SPSel.
    execute(&mut m, Instruction(0xd538_4201)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 1);

    // MSR SPSel, #0: switch EL1 to SP_EL0.
    execute(&mut m, Instruction(0xd500_40bf)).unwrap();
    assert!(!m.cores.active().state.spsel);
    assert_eq!(m.cores.active().state.sp, 0x4000);
    assert_eq!(m.cores.active().state.sp_el[1], 0x8000);

    // MSR SPSel, X1: restore the bank selected by the saved register value.
    execute(&mut m, Instruction(0xd518_4201)).unwrap();
    assert!(m.cores.active().state.spsel);
    assert_eq!(m.cores.active().state.sp, 0x8000);
    assert_eq!(m.cores.active().state.sp_el[0], 0x4000);
}

#[test]
fn irq_routing_honors_hcr_and_scr_with_el3_priority() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el2 = 0x20_000;
    m.cores.active_mut().state.vbar_el3 = 0x30_000;
    m.cores.active_mut().state.hcr_el2 = 1 << 4; // IMO
    m.cores.active_mut().pending_irq = Some(7);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x20_480);
    assert_eq!(m.cores.active().state.elr_el2, 0x10_000);

    m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x11_000;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.scr_el3 = 1 << 1; // IRQ
    m.cores.active_mut().pending_irq = Some(8);

    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, iphem_core::ExceptionLevel::El3);
    assert_eq!(m.cores.active().state.pc, 0x30_480);
    assert_eq!(m.cores.active().state.elr_el3, 0x11_000);
}

#[test]
fn breakpoint_and_halt_instructions_populate_exception_state() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.vbar_el1 = 0x30_000;

    for (instruction, ec) in [
        (0xd422_4680, 0x3cu64), // BRK #0x1234
        (0xd442_4680, 0x34u64), // HLT #0x1234
    ] {
        m.cores.active_mut().state.pc = 0x11_000;
        m.cores.active_mut().state.el = iphem_core::ExceptionLevel::El1;
        m.cores.active_mut().state.spsel = true;
        assert_eq!(
            execute(&mut m, Instruction(instruction)),
            Err(iphem_arm::ExecError::Synchronous)
        );
        assert_eq!(m.cores.active().state.esr_el1, (ec << 26) | 0x1234);
        assert_eq!(m.cores.active().state.elr_el1, 0x11_004);
        assert_eq!(m.cores.active().state.pc, 0x30_200);
        assert_eq!(m.cores.active().state.far_el1, 0);
    }
}

#[test]
fn fiq_observes_f_mask_and_routes_via_hcr_and_scr() {
    use iphem_core::{BusRegion, ExceptionLevel, SparseRam};
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    // Map a large RAM region so vector addresses and test PCs are reachable.
    let ram_base = 0;
    let ram_size = 0x200_000;
    m.bus
        .map(BusRegion {
            base: ram_base,
            size: ram_size,
            device: Box::new(SparseRam::new(ram_base, ram_size)),
        })
        .unwrap();
    // Write NOP at all vector offsets and test PCs used by the test so step() can fetch.
    let nop = 0xd503201fu32;
    for vbar in [0x50_000u64, 0x60_000, 0x70_000, 0x80_000] {
        for off in [0x100u64, 0x300, 0x500] {
            m.mem_write(vbar + off, 4, nop as u64).unwrap();
        }
    }
    // Write NOPs at every PC address and the PC+4 continuation used across cases.
    for pc in [0x10_000u64, 0x10_004, 0x15_000, 0x16_000, 0x17_000] {
        m.mem_write(pc, 4, nop as u64).unwrap();
    }

    // 1) F masked (DAIF.F=1) → FIQ not delivered.
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el1 = 0x50_000;
    m.cores.active_mut().state.daif = 0b0001; // F masked
    m.cores.active_mut().pending_fiq = Some(3);
    step(&mut m).unwrap();
    assert_eq!(
        m.cores.active().state.pc,
        0x10_004,
        "FIQ must not deliver when F is masked"
    );
    assert_eq!(
        m.cores.active().pending_fiq,
        Some(3),
        "pending FIQ retained"
    );

    // 2) F unmasked → FIQ delivered to EL1, vector +0x300 (same-EL, SPx).
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().pending_fiq = Some(4);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El1);
    assert_eq!(m.cores.active().state.pc, 0x50_300);
    assert_eq!(m.cores.active().state.elr_el1, 0x10_004);
    assert!(
        m.cores.active().state.daif & 0b0001 != 0,
        "DAIF.F set on entry"
    );

    // 3) HCR_EL2.FMO routes EL1 FIQ to EL2, vector +0x500.
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x15_000;
    m.cores.active_mut().state.vbar_el2 = 0x60_000;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.hcr_el2 = 1 << 3; // FMO
    m.cores.active_mut().pending_fiq = Some(5);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El2);
    assert_eq!(m.cores.active().state.pc, 0x60_500);
    assert_eq!(m.cores.active().state.elr_el2, 0x15_000);

    // 4) SCR_EL3.FIQ + HCR_EL2.FMO → EL3 wins (higher priority), vector +0x500.
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x16_000;
    m.cores.active_mut().state.vbar_el3 = 0x70_000;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.scr_el3 = 1 << 2; // FIQ
    m.cores.active_mut().state.hcr_el2 = 1 << 3; // FMO
    m.cores.active_mut().pending_fiq = Some(6);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El3);
    assert_eq!(m.cores.active().state.pc, 0x70_500);
    assert_eq!(m.cores.active().state.elr_el3, 0x16_000);

    // 5) FIQ takes priority over IRQ when both pending.
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.currentel = 1;
    m.cores.active_mut().state.spsel = true;
    m.cores.active_mut().state.sp = m.cores.active().state.sp_el[1];
    m.cores.active_mut().state.pc = 0x17_000;
    m.cores.active_mut().state.vbar_el1 = 0x80_000;
    m.cores.active_mut().state.daif = 0;
    m.cores.active_mut().state.hcr_el2 = 0;
    m.cores.active_mut().state.scr_el3 = 0;
    m.cores.active_mut().pending_irq = Some(7);
    m.cores.active_mut().pending_fiq = Some(8);
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.el, ExceptionLevel::El1);
    assert_eq!(
        m.cores.active().state.pc,
        0x80_300,
        "FIQ vector taken, not IRQ"
    );
    assert_eq!(
        m.cores.active().pending_irq,
        Some(7),
        "pending IRQ retained when FIQ wins"
    );
}

#[test]
fn indirect_branch_to_unaligned_pc_takes_alignment_exception() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0x10_000;
    m.cores.active_mut().state.vbar_el1 = 0x40_000;
    m.cores.active_mut().state.x[0] = 0x12_345;

    // BR X0.
    assert_eq!(
        execute(&mut m, Instruction(0xd61f_0000)),
        Err(iphem_arm::ExecError::Synchronous)
    );
    assert_eq!(m.cores.active().state.esr_el1, 0x22u64 << 26);
    assert_eq!(m.cores.active().state.far_el1, 0x12_345);
    assert_eq!(m.cores.active().state.elr_el1, 0x10_000);
    assert_eq!(m.cores.active().state.pc, 0x40_200);
}

#[test]
fn ldpsw_sign_extends_and_pair_pre_post_modes_use_correct_addresses() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0xe000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 4, 0xffff_ffff).unwrap();
    m.mem_write(base + 4, 4, 0x8000_0000).unwrap();
    m.mem_write(base + 8, 4, 0x7fff_ffff).unwrap();
    m.mem_write(base + 12, 4, 2).unwrap();

    m.cores.active_mut().state.x[2] = base;
    // LDPSW X0, X1, [X2].
    execute(&mut m, Instruction(0x6940_0440)).unwrap();
    assert_eq!(m.cores.active().state.x[0], u64::MAX);
    assert_eq!(m.cores.active().state.x[1], 0xffff_ffff_8000_0000);
    assert_eq!(m.cores.active().state.x[2], base);

    m.cores.active_mut().state.x[5] = base;
    // LDPSW X3, X4, [X5, #8]! reads after writeback offset.
    execute(&mut m, Instruction(0x69c1_10a3)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 0x7fff_ffff);
    assert_eq!(m.cores.active().state.x[4], 2);
    assert_eq!(m.cores.active().state.x[5], base + 8);

    m.cores.active_mut().state.x[8] = base;
    // LDPSW X6, X7, [X8], #8 reads old base, then updates it.
    execute(&mut m, Instruction(0x68c1_1d06)).unwrap();
    assert_eq!(m.cores.active().state.x[6], u64::MAX);
    assert_eq!(m.cores.active().state.x[7], 0xffff_ffff_8000_0000);
    assert_eq!(m.cores.active().state.x[8], base + 8);
}

#[test]
fn reserved_integer_pair_opcodes_are_undefined() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });

    // opc=01 has no store encoding; opc=11 is reserved for integer pairs.
    for instruction in [0x6900_0440, 0xe940_0440] {
        assert_eq!(
            execute(&mut m, Instruction(instruction)),
            Err(iphem_arm::ExecError::Undefined)
        );
    }
}

#[test]
fn t8027_ldrb_register_offset_uses_unshifted_index() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x1000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base + 7, 1, 0xab).unwrap();
    m.cores.active_mut().state.x[0] = base;
    m.cores.active_mut().state.x[8] = 7;
    // LDRB W9, [X0, X8].
    execute(&mut m, Instruction(0x3868_6809)).unwrap();
    assert_eq!(m.cores.active().state.x[9], 0xab);
}

#[test]
fn ldrb_unsigned_immediate_zero_extends_high_byte() {
    use iphem_core::{BusRegion, SparseRam};
    let base = 0x1000;
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base,
            size: 0x1000,
            device: Box::new(SparseRam::new(base, 0x1000)),
        })
        .unwrap();
    m.mem_write(base, 1, 0xf2).unwrap();
    m.cores.active_mut().state.x[0] = base;
    // LDRB W1, [X0].
    execute(&mut m, Instruction(0x3940_0001)).unwrap();
    assert_eq!(m.cores.active().state.x[1], 0xf2);
}

#[test]
fn ldrsw_register_offset_disassembles_signed_jump_table_entry() {
    // LLVM-verified LDRSW X8, [X9, X8, LSL #2], as used by the T8020 ROM's
    // PMGR event jump table.
    assert_eq!(disassemble(0, 0xb8a8_7928), "ldrsw\tx8, [x9, x8, lsl #2]");
}
