use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn machine() -> Machine {
    Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    })
}

#[test]
fn acceptance_gate_whilelo_ld1d_cntb_cntd() {
    let mut m = machine();
    let base = 0x1000;
    m.bus
        .map(BusRegion {
            base,
            size: 0x200,
            device: Box::new(SparseRam::new(base, 0x200)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x2000;
    // x0=64, x1=200 => whilelo p0.d, x0, x1 fills the low 2 D lanes.
    m.cores.active_mut().state.x[0] = 64;
    m.cores.active_mut().state.x[1] = 200;
    m.cores.active_mut().state.x[2] = base;
    m.mem_write(base, 8, 0x1011_1213_1415_1617).unwrap();
    m.mem_write(base + 8, 8, 0x2021_2223_2425_2627).unwrap();

    // whilelo p0.d, x0, x1 = 0x25e11c00
    execute(&mut m, Instruction(0x25e11c00)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0b11);
    // ld1d { z0.d }, p0/z, [x2] = 0xa5e0a040
    execute(&mut m, Instruction(0xa5e0a040)).unwrap();
    assert_eq!(
        m.cores.active().state.v[0],
        0x2021_2223_2425_2627_1011_1213_1415_1617
    );
    // cntb x3 = 0x0420e3e3
    execute(&mut m, Instruction(0x0420e3e3)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 16);
    // cntd x4 = 0x04e0e3e4
    execute(&mut m, Instruction(0x04e0e3e4)).unwrap();
    assert_eq!(m.cores.active().state.x[4], 2);
}

#[test]
fn whilelo_lt_ls_le_unsigned_and_signed() {
    let mut m = machine();
    m.cores.active_mut().state.x[0] = 64;
    m.cores.active_mut().state.x[1] = 200;
    // whilelo p0.d: 2 D lanes, a0=64<200, a1=128<200 -> all active.
    execute(&mut m, Instruction(0x25e11c00)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0x3);
    m.cores.active_mut().state.p[0] = 0;
    // whilelt p0.d (signed): same -> 0x3.
    execute(&mut m, Instruction(0x25e11400)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0x3);
    // whilels p0.d, x0=200, x1=200: lane0 only (a == rm includes equality).
    m.cores.active_mut().state.p[0] = 0;
    m.cores.active_mut().state.x[0] = 200;
    m.cores.active_mut().state.x[1] = 200;
    execute(&mut m, Instruction(0x25e11c10)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0x1);
    // whilele p0.d same inputs: lane0 only (a0=200==200, a1=264 != 200).
    m.cores.active_mut().state.p[0] = 0;
    execute(&mut m, Instruction(0x25e11410)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0x1);
    // whilelo p0.s, w0=0, w1=200: esize=32, lanes a = 0,32,64,96 all < 200.
    m.cores.active_mut().state.p[0] = 0;
    m.cores.active_mut().state.x[0] = 0;
    m.cores.active_mut().state.x[1] = 200;
    execute(&mut m, Instruction(0x25a10c00)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0b1111);
    // signed 32-bit: whilelt p0.s, w0=0, w1=0xffffffff(-1) => no lane true.
    m.cores.active_mut().state.p[0] = 0;
    m.cores.active_mut().state.x[1] = 0xffff_ffff;
    execute(&mut m, Instruction(0x25a10400)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0);
    // signed 64-bit: whilelt p0.d, x0=0, x1=-1 => no lane true.
    m.cores.active_mut().state.p[0] = 0;
    m.cores.active_mut().state.x[1] = u64::MAX;
    execute(&mut m, Instruction(0x25e11400)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0);
}

#[test]
fn while_sign_extended_imm_and_12bit_growth() {
    let mut m = machine();
    m.cores.active_mut().state.x[0] = 64;
    m.cores.active_mut().state.x[1] = 0x1_0000;
    // whilelo p0.d, x0, x1 wraps? No: 64-bit additions, no wrap here.
    execute(&mut m, Instruction(0x25e11c00)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0x3);
}

#[test]
fn cnt_addr_mult_forms() {
    let mut m = machine();
    // cntb x3, all (default mul #1) -> 16.
    execute(&mut m, Instruction(0x0420e3e3)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 16);
    // cnth x4 -> 8.
    execute(&mut m, Instruction(0x0460e3e4)).unwrap();
    assert_eq!(m.cores.active().state.x[4], 8);
    // cntw x5 -> 4.
    execute(&mut m, Instruction(0x04a0e3e5)).unwrap();
    assert_eq!(m.cores.active().state.x[5], 4);
    // cntd x6 -> 2.
    execute(&mut m, Instruction(0x04e0e3e6)).unwrap();
    assert_eq!(m.cores.active().state.x[6], 2);
    // cntd xzr, all, mul #4 = 0x04e3e3ff; xzr write suppressed.
    m.cores.active_mut().state.x[3] = 7;
    execute(&mut m, Instruction(0x04e3e3ff)).unwrap();
    assert_eq!(m.cores.active().state.x[3], 7);
}

#[test]
fn incp_decp_scalar_and_vector() {
    let mut m = machine();
    m.cores.active_mut().state.p[3] = 0b1010_1010;
    m.cores.active_mut().state.p[4] = 0b0011; // 2 active D lanes
    m.cores.active_mut().state.x[7] = 100;
    // incp x7, p3.b = 0x252c8867: +4 (4 active bits of p3).
    execute(&mut m, Instruction(0x252c8867)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 104);
    // decp x7, p3.b = 0x252d8867: -4.
    execute(&mut m, Instruction(0x252d8867)).unwrap();
    assert_eq!(m.cores.active().state.x[7], 100);
    // incp z4.d, p4.d = 0x25ec8084: 8-byte lanes, p4=0b11 (2 active lanes).
    m.cores.active_mut().state.v[4] = 0x10 | (0x10_u128 << 64);
    execute(&mut m, Instruction(0x25ec8084)).unwrap();
    assert_eq!(m.cores.active().state.v[4], 0x11_u128 | (0x11_u128 << 64));
    // decp z4.d, p4.d = 0x25ed8084: each lane -2.
    execute(&mut m, Instruction(0x25ed8084)).unwrap();
    assert_eq!(m.cores.active().state.v[4], 0x10_u128 | (0x10_u128 << 64));
}

#[test]
fn predicate_logical_family() {
    // and p5.b, p6/z, p7.b, p8.b = 0x250858e5
    let mut m = machine();
    m.cores.active_mut().state.p[6] = 0xff;
    m.cores.active_mut().state.p[7] = 0x0f;
    m.cores.active_mut().state.p[8] = 0x33;
    execute(&mut m, Instruction(0x250858e5)).unwrap();
    assert_eq!(m.cores.active().state.p[5], 0x03);
    // eor = 0x25085ae5 -> 0x0f ^ 0x33 = 0x3c.
    m.cores.active_mut().state.p[5] = 0;
    execute(&mut m, Instruction(0x25085ae5)).unwrap();
    assert_eq!(m.cores.active().state.p[5], 0x3c & 0xff);
    // orr = 0x258858e5 -> 0x0f | 0x33 = 0x3f.
    m.cores.active_mut().state.p[5] = 0;
    execute(&mut m, Instruction(0x258858e5)).unwrap();
    assert_eq!(m.cores.active().state.p[5], 0x3f);
    // bic = 0x250858f5 -> 0x0f & ~0x33 = 0x0c.
    m.cores.active_mut().state.p[5] = 0;
    execute(&mut m, Instruction(0x250858f5)).unwrap();
    assert_eq!(m.cores.active().state.p[5], 0x0c);
    // ands p5.b, p6/z, p7.b, p8.b = 0x254858e5 (S=bit22) sets NZCV on zero result.
    m.cores.active_mut().state.nzcv = 0;
    m.cores.active_mut().state.p[6] = 0;
    m.cores.active_mut().state.p[7] = 0xff;
    m.cores.active_mut().state.p[8] = 0xff;
    execute(&mut m, Instruction(0x254858e5)).unwrap(); // ands
    assert_eq!(m.cores.active().state.p[5], 0);
    assert_ne!(m.cores.active().state.nzcv & 0x2, 0);
}

#[test]
fn predicate_logical_pd_pg_pn_pm_independent() {
    // and p0.b, p1/z, p2.b, p3.b = 0x25034440 (previously rejected by the
    // mask that forced Pn[2:0]=111).
    let mut m = machine();
    m.cores.active_mut().state.p[1] = 0xff;
    m.cores.active_mut().state.p[2] = 0xf0;
    m.cores.active_mut().state.p[3] = 0xcc;
    execute(&mut m, Instruction(0x25034440)).unwrap();
    assert_eq!(m.cores.active().state.p[0], 0xc0);
}

#[test]
fn predicate_not_and_mov_aliases() {
    // not p6.b, p3/z, p1.b = 0x25034e26: EOR(P1, Pm=P3).
    let mut m = machine();
    m.cores.active_mut().state.p[3] = 0xff;
    m.cores.active_mut().state.p[1] = 0x0f;
    execute(&mut m, Instruction(0x25034e26)).unwrap();
    assert_eq!(m.cores.active().state.p[6], 0xf0);
    // mov p2.b, p7/z, p4.b = 0x25045c82: AND(P4, Pm=P4) = P4 gated by P7.
    m.cores.active_mut().state.p[7] = 0xff;
    m.cores.active_mut().state.p[4] = 0x55;
    execute(&mut m, Instruction(0x25045c82)).unwrap();
    assert_eq!(m.cores.active().state.p[2], 0x55);
}

#[test]
fn ld1_st1_scalar_base_with_mul_vl() {
    let mut m = machine();
    let base = 0x1000;
    m.bus
        .map(BusRegion {
            base,
            size: 0x400,
            device: Box::new(SparseRam::new(base, 0x400)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x2000;
    m.cores.active_mut().state.p[0] = 0xff; // all 16 B lanes / 8 H / 4 S / 2 D active
    m.cores.active_mut().state.x[2] = base;
    m.mem_write(base, 8, 0x0102).unwrap();
    m.mem_write(base + 8, 8, 0x0304).unwrap();
    // ld1d {z0.d}, p0/z, [x2] = 0xa5e0a040
    execute(&mut m, Instruction(0xa5e0a040)).unwrap();
    assert_eq!(m.cores.active().state.v[0], (0x0304_u128 << 64) | 0x0102);
    // ld1d with #1 MUL VL = 0xa5e1a040 -> base+16.
    m.mem_write(base + 16, 8, 0x1111).unwrap();
    m.mem_write(base + 24, 8, 0x2222).unwrap();
    execute(&mut m, Instruction(0xa5e1a040)).unwrap();
    assert_eq!(m.cores.active().state.v[0], (0x2222_u128 << 64) | 0x1111);
    // st1d {z0.d}, p0, [x2, #2, MUL VL] = 0xe5e2e040 -> base+32. v[0] lane0 =
    // 0x1111 writes base+32, lane1 = 0x2222 writes base+40.
    execute(&mut m, Instruction(0xe5e2e040)).unwrap();
    assert_eq!(m.mem_read(base + 32, 8).unwrap(), 0x1111);
    assert_eq!(m.mem_read(base + 40, 8).unwrap(), 0x2222);
}

#[test]
fn ld1_partial_predicate_merges() {
    let mut m = machine();
    let base = 0x1000;
    m.bus
        .map(BusRegion {
            base,
            size: 0x400,
            device: Box::new(SparseRam::new(base, 0x400)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x2000;
    m.cores.active_mut().state.p[0] = 0x1; // only lane 0 of D active
    m.cores.active_mut().state.x[2] = base;
    m.cores.active_mut().state.v[0] = 0xDEAD_u128 << 64;
    m.mem_write(base, 8, 0xAAAA).unwrap();
    m.mem_write(base + 8, 8, 0xBBBB).unwrap();
    execute(&mut m, Instruction(0xa5e0a040)).unwrap();
    assert_eq!(m.cores.active().state.v[0], 1051556645921812989913770_u128);
}

#[test]
fn st1_partial_predicate_writes_only_active() {
    let mut m = machine();
    let base = 0x1000;
    m.bus
        .map(BusRegion {
            base,
            size: 0x400,
            device: Box::new(SparseRam::new(base, 0x400)),
        })
        .unwrap();
    m.cores.active_mut().state.pc = 0x2000;
    m.cores.active_mut().state.p[0] = 0x2; // only lane 1 of D active
    m.cores.active_mut().state.x[2] = base;
    m.cores.active_mut().state.v[0] = 0x11223344_55667788_u128 << 64;
    execute(&mut m, Instruction(0xe5e0e040)).unwrap();
    assert_eq!(m.mem_read(base, 8).unwrap(), 0);
    assert_eq!(m.mem_read(base + 8, 8).unwrap(), 0x11223344_55667788);
}

#[test]
fn sve_disassemble_battery() {
    assert_eq!(disassemble(0, 0x25e11c00), "whilelo\tp0.d, x0, x1");
    assert_eq!(disassemble(0, 0x25e11400), "whilelt\tp0.d, x0, x1");
    assert_eq!(disassemble(0, 0x25e11c10), "whilels\tp0.d, x0, x1");
    assert_eq!(disassemble(0, 0x25e11410), "whilele\tp0.d, x0, x1");
    assert_eq!(disassemble(0, 0x25a10c00), "whilelo\tp0.s, w0, w1");
    assert_eq!(disassemble(0, 0x0420e3e3), "cntb\tx3, all, mul #1");
    assert_eq!(disassemble(0, 0x04e0e3e4), "cntd\tx4, all, mul #1");
    assert_eq!(disassemble(0, 0x04e3e3ff), "cntd\txzr, all, mul #4");
    assert_eq!(disassemble(0, 0x252c8867), "incp\tx7, p3.b");
    assert_eq!(disassemble(0, 0x25ec8084), "incp\tz4.d, p4.d");
    assert_eq!(disassemble(0, 0x250858e5), "and\tp5.b, p6/z, p7.b, p8.b");
    assert_eq!(disassemble(0, 0x254858e5), "ands\tp5.b, p6/z, p7.b, p8.b");
    assert_eq!(disassemble(0, 0x258858e5), "orr\tp5.b, p6/z, p7.b, p8.b");
    assert_eq!(disassemble(0, 0x25085ae5), "eor\tp5.b, p6/z, p7.b, p8.b");
    assert_eq!(disassemble(0, 0x250858f5), "bic\tp5.b, p6/z, p7.b, p8.b");
    assert_eq!(disassemble(0, 0xa5e0a040), "ld1d\t{z0.d}, p0/z, [x2]");
    assert_eq!(
        disassemble(0, 0xa5e1a040),
        "ld1d\t{z0.d}, p0/z, [x2, #1, MUL VL]"
    );
    assert_eq!(disassemble(0, 0xe5e0e040), "st1d\t{z0.d}, p0, [x2]");
    assert_eq!(
        disassemble(0, 0xe5e2e040),
        "st1d\t{z0.d}, p0, [x2, #2, MUL VL]"
    );
}
