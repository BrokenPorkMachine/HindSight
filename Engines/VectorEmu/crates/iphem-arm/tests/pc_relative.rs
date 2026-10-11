use iphem_arm::{execute, Instruction};
use iphem_core::{Machine, MachineConfig};

#[test]
fn adr_uses_signed_byte_offsets_while_adrp_uses_pages() {
    for offset in [-1048576i64, -4097, -4, -1, 0, 1, 2, 3, 4, 4097, 1048575] {
        for page in [false, true] {
            let mut m = Machine::new(MachineConfig {
                trace: false,
                quantum: 1,
            });
            let pc = 0xfffffff007a60c94u64;
            m.cores.active_mut().state.pc = pc;
            let imm = (offset as u32) & 0x1fffff;
            let word = 0x1000000c | (u32::from(page) << 31) | ((imm & 3) << 29) | ((imm >> 2) << 5);
            execute(&mut m, Instruction(word)).unwrap();
            let expected = if page {
                (pc & !0xfff).wrapping_add((offset as u64) << 12)
            } else {
                pc.wrapping_add(offset as u64)
            };
            assert_eq!(
                m.cores.active().state.x[12],
                expected,
                "offset={offset}, page={page}"
            );
        }
    }
}

#[test]
fn ios12_uuid_formatter_adr_points_at_its_jump_table() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    m.cores.active_mut().state.pc = 0xfffffff007a60c94;
    execute(&mut m, Instruction(0x100059cc)).unwrap();
    assert_eq!(m.cores.active().state.x[12], 0xfffffff007a617cc);
}
