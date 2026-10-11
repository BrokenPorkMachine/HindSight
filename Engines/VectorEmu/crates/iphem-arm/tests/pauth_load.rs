use iphem_arm::{disassemble, execute, step, ExecError, Instruction};
use iphem_core::{BusRegion, ExceptionLevel, Machine, MachineConfig, SparseRam};

fn machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.bus
        .map(BusRegion {
            base: 0x4000,
            size: 0x4000,
            device: Box::new(SparseRam::new(0x4000, 0x4000)),
        })
        .unwrap();
    m.cores.active_mut().state.el = ExceptionLevel::El1;
    m.cores.active_mut().state.tcr_el1 = 16 | (16 << 16);
    m.cores.active_mut().state.sctlr_el1 = (1 << 27) | (1 << 13);
    m.cores.active_mut().state.apda_lo = 0x0123_4567_89ab_cdef;
    m.cores.active_mut().state.apda_hi = 0xfedc_ba98_7654_3210;
    m.cores.active_mut().state.apdb_lo = 0x1020_3040_5060_7080;
    m.cores.active_mut().state.apdb_hi = 0x8070_6050_4030_2010;
    m.cores.active_mut().state.vbar_el1 = 0x8000;
    m.cores.active_mut().state.pc = 0x4000;
    m
}

fn signed(m: &mut Machine, key_b: bool, pointer: u64) -> u64 {
    m.cores.active_mut().state.x[8] = pointer;
    // PACDZA X8 / PACDZB X8, assembled independently with clang.
    execute(
        m,
        Instruction(if key_b { 0xdac1_2fe8 } else { 0xdac1_2be8 }),
    )
    .unwrap();
    m.cores.active().state.x[8]
}

fn ldra(key_b: bool, writeback: bool, rn: u32, rt: u32, offset: i64) -> u32 {
    assert!(offset % 8 == 0 && (-4096..=4088).contains(&offset));
    let imm = ((offset / 8) as u32) & 0x3ff;
    0xf820_0400
        | ((key_b as u32) << 23)
        | ((imm >> 9) << 22)
        | ((imm & 0x1ff) << 12)
        | ((writeback as u32) << 11)
        | (rn << 5)
        | rt
}

#[test]
fn kernel_ldraa_authenticates_before_offset_and_writes_back() {
    let mut m = machine();
    let pointer = signed(&mut m, false, 0x5000);
    assert_ne!(pointer, 0x5000);
    m.mem_write(0x5060, 8, 0x1234_5678_9abc_def0).unwrap();
    execute(&mut m, Instruction(0xf820_cd09)).unwrap();
    assert_eq!(m.cores.active().state.x[8], 0x5060);
    assert_eq!(m.cores.active().state.x[9], 0x1234_5678_9abc_def0);
}

#[test]
fn both_keys_and_full_signed_offset_range() {
    for key_b in [false, true] {
        for offset in [-4096, -8, 0, 8, 4088] {
            for writeback in [false, true] {
                let mut m = machine();
                let pointer = signed(&mut m, key_b, 0x5000);
                let address = 0x5000u64.wrapping_add(offset as u64);
                m.mem_write(address, 8, 0xdead_beef_cafe_1234).unwrap();
                execute(&mut m, Instruction(ldra(key_b, writeback, 8, 9, offset))).unwrap();
                assert_eq!(m.cores.active().state.x[9], 0xdead_beef_cafe_1234);
                assert_eq!(
                    m.cores.active().state.x[8],
                    if writeback { address } else { pointer }
                );
            }
        }
    }
}

#[test]
fn independent_encodings_disassemble() {
    for (word, expected) in [
        (0xf820_cd09, "ldraa\tx9, [x8, #0x60]!"),
        (0xf820_c509, "ldraa\tx9, [x8, #0x60]"),
        (0xf8e0_0d09, "ldrab\tx9, [x8, #-0x1000]!"),
        (0xf820_0409, "ldraa\tx9, [x0]"),
    ] {
        assert_eq!(disassemble(0, word), expected);
    }
}

#[test]
fn disabled_authentication_still_loads_and_writes_back_at_each_el() {
    for el in [
        ExceptionLevel::El0,
        ExceptionLevel::El1,
        ExceptionLevel::El2,
        ExceptionLevel::El3,
    ] {
        for key_b in [false, true] {
            let mut m = machine();
            m.cores.active_mut().state.el = el;
            m.cores.active_mut().state.sctlr_el1 = 0;
            m.cores.active_mut().state.sctlr_el2 = 0;
            m.cores.active_mut().state.sctlr_el3 = 0;
            m.cores.active_mut().state.x[8] = 0x5000;
            m.mem_write(0x5008, 8, 42).unwrap();
            execute(&mut m, Instruction(ldra(key_b, true, 8, 9, 8))).unwrap();
            assert_eq!(m.cores.active().state.x[8], 0x5008);
            assert_eq!(m.cores.active().state.x[9], 42);
        }
    }
}

#[test]
fn authentication_controls_are_independent_for_data_keys() {
    for key_b in [false, true] {
        let mut m = machine();
        m.cores.active_mut().state.sctlr_el1 = if key_b { 1 << 27 } else { 1 << 13 };
        m.cores.active_mut().state.x[8] = 0x5000;
        m.mem_write(0x5000, 8, 42).unwrap();
        execute(&mut m, Instruction(ldra(key_b, false, 8, 9, 0))).unwrap();
        assert_eq!(m.cores.active().state.x[9], 42);
    }
}

#[test]
fn bad_signature_faults_through_mmu_without_writeback() {
    for key_b in [false, true] {
        for tbi in [false, true] {
            for upper in [false, true] {
                for el in [ExceptionLevel::El0, ExceptionLevel::El1] {
                    let mut m = machine();
                    if tbi {
                        m.cores.active_mut().state.tcr_el1 |= (1 << 37) | (1 << 38);
                    }
                    let raw = if upper { 0xffff_ffff_ffff_5000 } else { 0x5000 };
                    let pointer = signed(&mut m, key_b, raw) ^ (1 << 48);
                    m.cores.active_mut().state.x[8] = pointer;
                    m.cores.active_mut().state.x[9] = 0xaaaa;
                    m.cores.active_mut().state.el = el;
                    m.cores.active_mut().state.sctlr_el1 |= 1; // MMU enabled: invalid VA faults before a table read.
                    m.cores.active_mut().state.pc = 0x4000;
                    m.current_instruction_pc = Some(0x4000);
                    let result = execute(&mut m, Instruction(ldra(key_b, true, 8, 9, 8)));
                    assert!(matches!(result, Err(ExecError::Data)), "{result:?}");
                    let shift = if tbi { 53 } else { 61 };
                    let poisoned =
                        (raw & !(3u64 << shift)) | ((if key_b { 2 } else { 1 }) << shift);
                    assert_eq!(m.cores.active().state.far_el1, poisoned + 8);
                    assert_eq!(
                        m.cores.active().state.esr_el1 >> 26,
                        if el == ExceptionLevel::El0 {
                            0x24
                        } else {
                            0x25
                        }
                    );
                    assert_eq!(m.cores.active().state.esr_el1 & 0x3f, 0); // Address size fault, level 0.
                    assert_eq!(m.cores.active().state.elr_el1, 0x4000);
                    assert_eq!(m.cores.active().state.x[8], pointer);
                    assert_eq!(m.cores.active().state.x[9], 0xaaaa);
                }
            }
        }
    }
}

#[test]
fn wrong_key_does_not_authenticate() {
    let mut m = machine();
    let pointer = signed(&mut m, false, 0x5000);
    m.cores.active_mut().state.sctlr_el1 |= 1;
    assert!(execute(&mut m, Instruction(ldra(true, true, 8, 9, 0))).is_err());
    assert_eq!(m.cores.active().state.x[8], pointer);
    assert_eq!(m.cores.active().state.far_el1, (2u64 << 61) | 0x5000);
}

#[test]
fn bus_fault_preserves_base_and_destination() {
    let mut m = machine();
    let pointer = signed(&mut m, false, 0x8000);
    m.cores.active_mut().state.x[9] = 0xaaaa;
    assert!(matches!(
        execute(&mut m, Instruction(ldra(false, true, 8, 9, 8))),
        Err(ExecError::Data)
    ));
    assert_eq!(m.cores.active().state.x[8], pointer);
    assert_eq!(m.cores.active().state.x[9], 0xaaaa);
}

#[test]
fn offset_form_allows_base_destination_alias() {
    let mut m = machine();
    signed(&mut m, false, 0x5000);
    m.mem_write(0x5000, 8, 42).unwrap();
    execute(&mut m, Instruction(ldra(false, false, 8, 8, 0))).unwrap();
    assert_eq!(m.cores.active().state.x[8], 42);
}

#[test]
fn writeback_overlap_takes_undefined_at_faulting_instruction() {
    let mut m = machine();
    let pointer = signed(&mut m, false, 0x5000);
    m.mem_write(0x4000, 4, ldra(false, true, 8, 8, 8) as u64)
        .unwrap();
    m.cores.active_mut().state.pc = 0x4000;
    step(&mut m).unwrap();
    assert_eq!(m.cores.active().state.esr_el1, 0);
    assert_eq!(m.cores.active().state.elr_el1, 0x4000);
    assert_eq!(m.cores.active().state.x[8], pointer);
}

#[test]
fn sp_base_and_zero_destination_still_perform_the_load() {
    let mut m = machine();
    m.cores.active_mut().state.sp = signed(&mut m, true, 0x5000);
    m.mem_write(0x5008, 8, 42).unwrap();
    execute(&mut m, Instruction(ldra(true, true, 31, 31, 8))).unwrap();
    assert_eq!(m.cores.active().state.sp, 0x5008);
    m.cores.active_mut().state.sp = signed(&mut m, true, 0x8000);
    let old_sp = m.cores.active().state.sp;
    assert!(execute(&mut m, Instruction(ldra(true, true, 31, 31, 0))).is_err());
    assert_eq!(m.cores.active().state.sp, old_sp);
}

#[test]
fn sp_alignment_fault_precedes_load_and_writeback() {
    let mut m = machine();
    m.cores.active_mut().state.sp = signed(&mut m, false, 0x5008);
    let old_sp = m.cores.active().state.sp;
    m.cores.active_mut().state.sctlr_el1 |= 1 << 3;
    m.cores.active_mut().state.pc = 0x4000;
    assert!(matches!(
        execute(&mut m, Instruction(ldra(false, true, 31, 9, 8))),
        Err(ExecError::Synchronous)
    ));
    assert_eq!(m.cores.active().state.esr_el1 >> 26, 0x26);
    assert_eq!(m.cores.active().state.elr_el1, 0x4000);
    assert_eq!(m.cores.active().state.sp_el[1], old_sp);
}
