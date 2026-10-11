use iphem_arm::{execute, ExecError, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn undecoded(w: u32) -> bool {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    for i in 0..31usize {
        m.cores.active_mut().state.x[i] = 0x100000;
    }
    for i in 0..32usize {
        m.cores.active_mut().state.v[i] = 0x1122334455667788_aabbccddeeff0011;
    }
    m.bus
        .map(BusRegion {
            base: 0x100000,
            size: 0x100000,
            device: Box::new(SparseRam::new(0x100000, 0x100000)),
        })
        .unwrap();
    matches!(execute(&mut m, Instruction(w)), Err(ExecError::Undefined))
}

#[test]
fn classify() {
    eprintln!(
        "0x{:08x}\tldr\tq1, [x2, #0x40]\tbattery1\t{}",
        0x3dc01041u32,
        undecoded(0x3dc01041u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tq1, [x2, #0x40]\tbattery1\t{}",
        0x3d801041u32,
        undecoded(0x3d801041u32)
    );
    eprintln!(
        "0x{:08x}\tldr\td1, [x2, #0x40]\tbattery1\t{}",
        0xfd402041u32,
        undecoded(0xfd402041u32)
    );
    eprintln!(
        "0x{:08x}\tstr\td1, [x2]\tbattery1\t{}",
        0xfd000041u32,
        undecoded(0xfd000041u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2, #0x40]\tbattery1\t{}",
        0xbd404041u32,
        undecoded(0xbd404041u32)
    );
    eprintln!(
        "0x{:08x}\tstr\ts1, [x2, #0x40]\tbattery1\t{}",
        0xbd004041u32,
        undecoded(0xbd004041u32)
    );
    eprintln!(
        "0x{:08x}\tldr\th1, [x2, #0x20]\tbattery1\t{}",
        0x7d404041u32,
        undecoded(0x7d404041u32)
    );
    eprintln!(
        "0x{:08x}\tstr\th1, [x2, #0x20]\tbattery1\t{}",
        0x7d004041u32,
        undecoded(0x7d004041u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tb1, [x2, #0x10]\tbattery1\t{}",
        0x3d404041u32,
        undecoded(0x3d404041u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tb1, [x2, #0x10]\tbattery1\t{}",
        0x3d004041u32,
        undecoded(0x3d004041u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tq1, [x2, #0x40]!\tbattery1\t{}",
        0x3cc40c41u32,
        undecoded(0x3cc40c41u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tq1, [x2, #0x40]!\tbattery1\t{}",
        0x3c840c41u32,
        undecoded(0x3c840c41u32)
    );
    eprintln!(
        "0x{:08x}\tldr\td1, [x2, #-0x20]!\tbattery1\t{}",
        0xfc5e0c41u32,
        undecoded(0xfc5e0c41u32)
    );
    eprintln!(
        "0x{:08x}\tstr\td1, [x2, #-0x20]!\tbattery1\t{}",
        0xfc1e0c41u32,
        undecoded(0xfc1e0c41u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2, #0x10]!\tbattery1\t{}",
        0xbc410c41u32,
        undecoded(0xbc410c41u32)
    );
    eprintln!(
        "0x{:08x}\tstr\ts1, [x2, #0x10]!\tbattery1\t{}",
        0xbc010c41u32,
        undecoded(0xbc010c41u32)
    );
    eprintln!(
        "0x{:08x}\tldr\th1, [x2, #0x20]!\tbattery1\t{}",
        0x7c420c41u32,
        undecoded(0x7c420c41u32)
    );
    eprintln!(
        "0x{:08x}\tstr\th1, [x2, #-0x20]!\tbattery1\t{}",
        0x7c1e0c41u32,
        undecoded(0x7c1e0c41u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tb1, [x2, #0x10]!\tbattery1\t{}",
        0x3c410c41u32,
        undecoded(0x3c410c41u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tb1, [x2, #-0x10]!\tbattery1\t{}",
        0x3c1f0c41u32,
        undecoded(0x3c1f0c41u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tq1, [x2], #0x40\tbattery1\t{}",
        0x3cc40441u32,
        undecoded(0x3cc40441u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tq1, [x2], #0x40\tbattery1\t{}",
        0x3c840441u32,
        undecoded(0x3c840441u32)
    );
    eprintln!(
        "0x{:08x}\tldr\td1, [x2], #-0x20\tbattery1\t{}",
        0xfc5e0441u32,
        undecoded(0xfc5e0441u32)
    );
    eprintln!(
        "0x{:08x}\tstr\td1, [x2], #0x20\tbattery1\t{}",
        0xfc020441u32,
        undecoded(0xfc020441u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2], #0x10\tbattery1\t{}",
        0xbc410441u32,
        undecoded(0xbc410441u32)
    );
    eprintln!(
        "0x{:08x}\tstr\ts1, [x2], #0x10\tbattery1\t{}",
        0xbc010441u32,
        undecoded(0xbc010441u32)
    );
    eprintln!(
        "0x{:08x}\tldr\th1, [x2], #0x20\tbattery1\t{}",
        0x7c420441u32,
        undecoded(0x7c420441u32)
    );
    eprintln!(
        "0x{:08x}\tstr\th1, [x2], #-0x20\tbattery1\t{}",
        0x7c1e0441u32,
        undecoded(0x7c1e0441u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tb1, [x2], #0x10\tbattery1\t{}",
        0x3c410441u32,
        undecoded(0x3c410441u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tb1, [x2], #-0x10\tbattery1\t{}",
        0x3c1f0441u32,
        undecoded(0x3c1f0441u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tq1, [x2, x3]\tbattery1\t{}",
        0x3ce36841u32,
        undecoded(0x3ce36841u32)
    );
    eprintln!(
        "0x{:08x}\tstr\tq1, [x2, x3]\tbattery1\t{}",
        0x3ca36841u32,
        undecoded(0x3ca36841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2, x3, lsl #2]\tbattery1\t{}",
        0xbc637841u32,
        undecoded(0xbc637841u32)
    );
    eprintln!(
        "0x{:08x}\tstr\ts1, [x2, x3, lsl #2]\tbattery1\t{}",
        0xbc237841u32,
        undecoded(0xbc237841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\td1, [x2, x3, lsl #3]\tbattery1\t{}",
        0xfc637841u32,
        undecoded(0xfc637841u32)
    );
    eprintln!(
        "0x{:08x}\tstr\td1, [x2, x3]\tbattery1\t{}",
        0xfc236841u32,
        undecoded(0xfc236841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tb1, [x2, w3, uxtw]\tbattery1\t{}",
        0x3c634841u32,
        undecoded(0x3c634841u32)
    );
    eprintln!(
        "0x{:08x}\tstr\th1, [x2, w3, uxtw]\tbattery1\t{}",
        0x7c234841u32,
        undecoded(0x7c234841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2, w3, sxtw]\tbattery1\t{}",
        0xbc63c841u32,
        undecoded(0xbc63c841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\tq1, [x2, w3, sxtw]\tbattery1\t{}",
        0x3ce3c841u32,
        undecoded(0x3ce3c841u32)
    );
    eprintln!(
        "0x{:08x}\tldr\ts1, [x2, w6, sxtw]\tbattery1\t{}",
        0xbc66c841u32,
        undecoded(0xbc66c841u32)
    );
    eprintln!(
        "0x{:08x}\tldur\tq1, [x2, #-0x10]\tbattery1\t{}",
        0x3cdf0041u32,
        undecoded(0x3cdf0041u32)
    );
    eprintln!(
        "0x{:08x}\tstur\tq1, [x2, #-0x10]\tbattery1\t{}",
        0x3c9f0041u32,
        undecoded(0x3c9f0041u32)
    );
    eprintln!(
        "0x{:08x}\tldur\td1, [x2, #0x40]\tbattery1\t{}",
        0xfc440041u32,
        undecoded(0xfc440041u32)
    );
    eprintln!(
        "0x{:08x}\tstur\ts1, [x2, #-0x40]\tbattery1\t{}",
        0xbc1c0041u32,
        undecoded(0xbc1c0041u32)
    );
    eprintln!(
        "0x{:08x}\tldur\th1, [x2, #0x20]\tbattery1\t{}",
        0x7c420041u32,
        undecoded(0x7c420041u32)
    );
    eprintln!(
        "0x{:08x}\tstur\tb1, [x2, #-0x20]\tbattery1\t{}",
        0x3c1e0041u32,
        undecoded(0x3c1e0041u32)
    );
    eprintln!(
        "0x{:08x}\tldp\tq1, q2, [x3]\tbattery1\t{}",
        0xad400861u32,
        undecoded(0xad400861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\tq1, q2, [x3]\tbattery1\t{}",
        0xad000861u32,
        undecoded(0xad000861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\tq1, q2, [x3, #0x20]\tbattery1\t{}",
        0xad410861u32,
        undecoded(0xad410861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\tq1, q2, [x3, #-0x20]\tbattery1\t{}",
        0xad3f0861u32,
        undecoded(0xad3f0861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\tq1, q2, [x3, #-0x40]!\tbattery1\t{}",
        0xadfe0861u32,
        undecoded(0xadfe0861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\tq1, q2, [x3, #0x10]!\tbattery1\t{}",
        0xad808861u32,
        undecoded(0xad808861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\tq1, q2, [x3], #0x40\tbattery1\t{}",
        0xacc20861u32,
        undecoded(0xacc20861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\tq1, q2, [x3], #-0x40\tbattery1\t{}",
        0xacbe0861u32,
        undecoded(0xacbe0861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\ts1, s2, [x3, #0x10]!\tbattery1\t{}",
        0x2dc20861u32,
        undecoded(0x2dc20861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\ts1, s2, [x3], #0x10\tbattery1\t{}",
        0x2c820861u32,
        undecoded(0x2c820861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\ts1, s2, [x3]\tbattery1\t{}",
        0x2d400861u32,
        undecoded(0x2d400861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\ts1, s2, [x3, #-0x10]!\tbattery1\t{}",
        0x2dbe0861u32,
        undecoded(0x2dbe0861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\td1, d2, [x3]\tbattery1\t{}",
        0x6d400861u32,
        undecoded(0x6d400861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\td1, d2, [x3, #0x40]\tbattery1\t{}",
        0x6d040861u32,
        undecoded(0x6d040861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\td1, d2, [x3], #0x40\tbattery1\t{}",
        0x6cc40861u32,
        undecoded(0x6cc40861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\td1, d2, [x3, #-0x20]!\tbattery1\t{}",
        0x6dbe0861u32,
        undecoded(0x6dbe0861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\td1, d2, [x3, #0x20]!\tbattery1\t{}",
        0x6dc20861u32,
        undecoded(0x6dc20861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\td1, d2, [x3], #-0x40\tbattery1\t{}",
        0x6cbc0861u32,
        undecoded(0x6cbc0861u32)
    );
    eprintln!(
        "0x{:08x}\tldp\ts1, s2, [x3, #0x10]\tbattery1\t{}",
        0x2d420861u32,
        undecoded(0x2d420861u32)
    );
    eprintln!(
        "0x{:08x}\tstp\ts1, s2, [x3, #0x10]\tbattery1\t{}",
        0x2d020861u32,
        undecoded(0x2d020861u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\ts0, w1\tbattery2\t{}",
        0x1e220020u32,
        undecoded(0x1e220020u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\td0, w1\tbattery2\t{}",
        0x1e620020u32,
        undecoded(0x1e620020u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\ts0, x1\tbattery2\t{}",
        0x9e220020u32,
        undecoded(0x9e220020u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\td0, x1\tbattery2\t{}",
        0x9e620020u32,
        undecoded(0x9e620020u32)
    );
    eprintln!(
        "0x{:08x}\tucvtf\ts0, w1\tbattery2\t{}",
        0x1e230020u32,
        undecoded(0x1e230020u32)
    );
    eprintln!(
        "0x{:08x}\tucvtf\td0, w1\tbattery2\t{}",
        0x1e630020u32,
        undecoded(0x1e630020u32)
    );
    eprintln!(
        "0x{:08x}\tucvtf\ts0, x1\tbattery2\t{}",
        0x9e230020u32,
        undecoded(0x9e230020u32)
    );
    eprintln!(
        "0x{:08x}\tucvtf\td0, x1\tbattery2\t{}",
        0x9e630020u32,
        undecoded(0x9e630020u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\td1, d2, #0x5\tbattery2\t{}",
        0x5f7be441u32,
        undecoded(0x5f7be441u32)
    );
    eprintln!(
        "0x{:08x}\tscvtf\ts1, s2, #0x5\tbattery2\t{}",
        0x5f3be441u32,
        undecoded(0x5f3be441u32)
    );
    eprintln!(
        "0x{:08x}\tucvtf\td1, d2, #0x5\tbattery2\t{}",
        0x7f7be441u32,
        undecoded(0x7f7be441u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\tw1, s0\tbattery2\t{}",
        0x1e380001u32,
        undecoded(0x1e380001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\tx1, d0\tbattery2\t{}",
        0x9e780001u32,
        undecoded(0x9e780001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\tw1, d0\tbattery2\t{}",
        0x1e780001u32,
        undecoded(0x1e780001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\tx1, s0\tbattery2\t{}",
        0x9e380001u32,
        undecoded(0x9e380001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzu\tw1, s0\tbattery2\t{}",
        0x1e390001u32,
        undecoded(0x1e390001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzu\tx1, d0\tbattery2\t{}",
        0x9e790001u32,
        undecoded(0x9e790001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzu\tw1, d0\tbattery2\t{}",
        0x1e790001u32,
        undecoded(0x1e790001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzu\tx1, s0\tbattery2\t{}",
        0x9e390001u32,
        undecoded(0x9e390001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtas\tw1, s0\tbattery2\t{}",
        0x1e240001u32,
        undecoded(0x1e240001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtas\tx1, d0\tbattery2\t{}",
        0x9e640001u32,
        undecoded(0x9e640001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtau\tw1, s0\tbattery2\t{}",
        0x1e250001u32,
        undecoded(0x1e250001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtau\tx1, d0\tbattery2\t{}",
        0x9e650001u32,
        undecoded(0x9e650001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtns\tw1, s0\tbattery2\t{}",
        0x1e200001u32,
        undecoded(0x1e200001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtns\tx1, d0\tbattery2\t{}",
        0x9e600001u32,
        undecoded(0x9e600001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtnu\tw1, s0\tbattery2\t{}",
        0x1e210001u32,
        undecoded(0x1e210001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtnu\tx1, d0\tbattery2\t{}",
        0x9e610001u32,
        undecoded(0x9e610001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtps\tw1, d0\tbattery2\t{}",
        0x1e680001u32,
        undecoded(0x1e680001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtps\tx1, s0\tbattery2\t{}",
        0x9e280001u32,
        undecoded(0x9e280001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtpu\tw1, s0\tbattery2\t{}",
        0x1e290001u32,
        undecoded(0x1e290001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtpu\tx1, d0\tbattery2\t{}",
        0x9e690001u32,
        undecoded(0x9e690001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtms\tw1, s0\tbattery2\t{}",
        0x1e300001u32,
        undecoded(0x1e300001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtms\tx1, d0\tbattery2\t{}",
        0x9e700001u32,
        undecoded(0x9e700001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtmu\tw1, s0\tbattery2\t{}",
        0x1e310001u32,
        undecoded(0x1e310001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtmu\tx1, d0\tbattery2\t{}",
        0x9e710001u32,
        undecoded(0x9e710001u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\td1, d2, #0x5\tbattery2\t{}",
        0x5f7bfc41u32,
        undecoded(0x5f7bfc41u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzs\ts1, s2, #0x5\tbattery2\t{}",
        0x5f3bfc41u32,
        undecoded(0x5f3bfc41u32)
    );
    eprintln!(
        "0x{:08x}\tfcvtzu\td1, d2, #0x5\tbattery2\t{}",
        0x7f7bfc41u32,
        undecoded(0x7f7bfc41u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\th0, s1\tbattery2\t{}",
        0x1e23c020u32,
        undecoded(0x1e23c020u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\th0, d1\tbattery2\t{}",
        0x1e63c020u32,
        undecoded(0x1e63c020u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\ts0, h1\tbattery2\t{}",
        0x1ee24020u32,
        undecoded(0x1ee24020u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\td0, h1\tbattery2\t{}",
        0x1ee2c020u32,
        undecoded(0x1ee2c020u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\ts0, d1\tbattery2\t{}",
        0x1e624020u32,
        undecoded(0x1e624020u32)
    );
    eprintln!(
        "0x{:08x}\tfcvt\td0, s1\tbattery2\t{}",
        0x1e22c020u32,
        undecoded(0x1e22c020u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\ts0, w1\tbattery2\t{}",
        0x1e270020u32,
        undecoded(0x1e270020u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\td0, x1\tbattery2\t{}",
        0x9e670020u32,
        undecoded(0x9e670020u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\tw1, s0\tbattery2\t{}",
        0x1e260001u32,
        undecoded(0x1e260001u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\tx1, d0\tbattery2\t{}",
        0x9e660001u32,
        undecoded(0x9e660001u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\ts0, s1\tbattery2\t{}",
        0x1e204020u32,
        undecoded(0x1e204020u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tv0[1], x1\tbattery2\t{}",
        0x4e181c20u32,
        undecoded(0x4e181c20u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tx1, v0[1]\tbattery2\t{}",
        0x4e183c01u32,
        undecoded(0x4e183c01u32)
    );
    eprintln!(
        "0x{:08x}\tmov.s\tv0[2], w1\tbattery2\t{}",
        0x4e141c20u32,
        undecoded(0x4e141c20u32)
    );
    eprintln!(
        "0x{:08x}\tmov.s\tw1, v0[0]\tbattery2\t{}",
        0x0e043c01u32,
        undecoded(0x0e043c01u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\ts1, #1.00000000\tbattery2\t{}",
        0x1e2e1001u32,
        undecoded(0x1e2e1001u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\td1, #-0.50000000\tbattery2\t{}",
        0x1e7c1001u32,
        undecoded(0x1e7c1001u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\th0, w1\tbattery2\t{}",
        0x1ee70020u32,
        undecoded(0x1ee70020u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\tw0, h1\tbattery2\t{}",
        0x1ee60020u32,
        undecoded(0x1ee60020u32)
    );
    eprintln!(
        "0x{:08x}\tfmov\th0, h1\tbattery2\t{}",
        0x1ee04020u32,
        undecoded(0x1ee04020u32)
    );
    eprintln!(
        "0x{:08x}\tcmgt.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb43545u32,
        undecoded(0x4eb43545u32)
    );
    eprintln!(
        "0x{:08x}\tcmge.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb43d45u32,
        undecoded(0x4eb43d45u32)
    );
    eprintln!(
        "0x{:08x}\tcmeq.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb48d45u32,
        undecoded(0x6eb48d45u32)
    );
    eprintln!(
        "0x{:08x}\tcmhi.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb43545u32,
        undecoded(0x6eb43545u32)
    );
    eprintln!(
        "0x{:08x}\tcmhs.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb43d45u32,
        undecoded(0x6eb43d45u32)
    );
    eprintln!(
        "0x{:08x}\tcmtst.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb48d45u32,
        undecoded(0x4eb48d45u32)
    );
    eprintln!(
        "0x{:08x}\tcmgt.16b\tv5, v10, v20\tbattery3\t{}",
        0x4e343545u32,
        undecoded(0x4e343545u32)
    );
    eprintln!(
        "0x{:08x}\tcmeq.2d\tv5, v10, v20\tbattery3\t{}",
        0x6ef48d45u32,
        undecoded(0x6ef48d45u32)
    );
    eprintln!(
        "0x{:08x}\tsmin.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb46d45u32,
        undecoded(0x4eb46d45u32)
    );
    eprintln!(
        "0x{:08x}\tsmax.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb46545u32,
        undecoded(0x4eb46545u32)
    );
    eprintln!(
        "0x{:08x}\tumin.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb46d45u32,
        undecoded(0x6eb46d45u32)
    );
    eprintln!(
        "0x{:08x}\tumax.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb46545u32,
        undecoded(0x6eb46545u32)
    );
    eprintln!(
        "0x{:08x}\tsmin.8h\tv5, v10, v20\tbattery3\t{}",
        0x4e746d45u32,
        undecoded(0x4e746d45u32)
    );
    eprintln!(
        "0x{:08x}\taddp.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb4bd45u32,
        undecoded(0x4eb4bd45u32)
    );
    eprintln!(
        "0x{:08x}\taddp.16b\tv5, v10, v20\tbattery3\t{}",
        0x4e34bd45u32,
        undecoded(0x4e34bd45u32)
    );
    eprintln!(
        "0x{:08x}\taddp.8h\tv5, v10, v20\tbattery3\t{}",
        0x4e74bd45u32,
        undecoded(0x4e74bd45u32)
    );
    eprintln!(
        "0x{:08x}\tsqadd.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb40d45u32,
        undecoded(0x4eb40d45u32)
    );
    eprintln!(
        "0x{:08x}\tsqsub.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb42d45u32,
        undecoded(0x4eb42d45u32)
    );
    eprintln!(
        "0x{:08x}\tuqadd.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb40d45u32,
        undecoded(0x6eb40d45u32)
    );
    eprintln!(
        "0x{:08x}\tuqsub.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb42d45u32,
        undecoded(0x6eb42d45u32)
    );
    eprintln!(
        "0x{:08x}\tsqadd.16b\tv5, v10, v20\tbattery3\t{}",
        0x4e340d45u32,
        undecoded(0x4e340d45u32)
    );
    eprintln!(
        "0x{:08x}\tuqadd.2d\tv5, v10, v20\tbattery3\t{}",
        0x6ef40d45u32,
        undecoded(0x6ef40d45u32)
    );
    eprintln!(
        "0x{:08x}\tsqneg.4s\tv5, v10\tbattery3\t{}",
        0x6ea07945u32,
        undecoded(0x6ea07945u32)
    );
    eprintln!(
        "0x{:08x}\tsqabs.4s\tv5, v10\tbattery3\t{}",
        0x4ea07945u32,
        undecoded(0x4ea07945u32)
    );
    eprintln!(
        "0x{:08x}\tsshl.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb44545u32,
        undecoded(0x4eb44545u32)
    );
    eprintln!(
        "0x{:08x}\tushl.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb44545u32,
        undecoded(0x6eb44545u32)
    );
    eprintln!(
        "0x{:08x}\tshadd.8h\tv5, v10, v20\tbattery3\t{}",
        0x4e740545u32,
        undecoded(0x4e740545u32)
    );
    eprintln!(
        "0x{:08x}\tuhadd.8h\tv5, v10, v20\tbattery3\t{}",
        0x6e740545u32,
        undecoded(0x6e740545u32)
    );
    eprintln!(
        "0x{:08x}\tshsub.8h\tv5, v10, v20\tbattery3\t{}",
        0x4e742545u32,
        undecoded(0x4e742545u32)
    );
    eprintln!(
        "0x{:08x}\tuhsub.8h\tv5, v10, v20\tbattery3\t{}",
        0x6e742545u32,
        undecoded(0x6e742545u32)
    );
    eprintln!(
        "0x{:08x}\tsrshl.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb45545u32,
        undecoded(0x4eb45545u32)
    );
    eprintln!(
        "0x{:08x}\turshl.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb45545u32,
        undecoded(0x6eb45545u32)
    );
    eprintln!(
        "0x{:08x}\tsmaxp.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb4a545u32,
        undecoded(0x4eb4a545u32)
    );
    eprintln!(
        "0x{:08x}\tumaxp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4a545u32,
        undecoded(0x6eb4a545u32)
    );
    eprintln!(
        "0x{:08x}\tsminp.4s\tv5, v10, v20\tbattery3\t{}",
        0x4eb4ad45u32,
        undecoded(0x4eb4ad45u32)
    );
    eprintln!(
        "0x{:08x}\tuminp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4ad45u32,
        undecoded(0x6eb4ad45u32)
    );
    eprintln!(
        "0x{:08x}\tfcmeq.4s\tv5, v10, v20\tbattery3\t{}",
        0x4e34e545u32,
        undecoded(0x4e34e545u32)
    );
    eprintln!(
        "0x{:08x}\tfcmgt.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4e545u32,
        undecoded(0x6eb4e545u32)
    );
    eprintln!(
        "0x{:08x}\tfcmge.4s\tv5, v10, v20\tbattery3\t{}",
        0x6e34e545u32,
        undecoded(0x6e34e545u32)
    );
    eprintln!(
        "0x{:08x}\tfacgt.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4ed45u32,
        undecoded(0x6eb4ed45u32)
    );
    eprintln!(
        "0x{:08x}\tfacge.4s\tv5, v10, v20\tbattery3\t{}",
        0x6e34ed45u32,
        undecoded(0x6e34ed45u32)
    );
    eprintln!(
        "0x{:08x}\tfcmla.4s\tv5, v10, v20, #0\tbattery3\t{}",
        0x6e94c545u32,
        undecoded(0x6e94c545u32)
    );
    eprintln!(
        "0x{:08x}\tfcadd.4s\tv5, v10, v20, #90\tbattery3\t{}",
        0x6e94e545u32,
        undecoded(0x6e94e545u32)
    );
    eprintln!(
        "0x{:08x}\tfaddp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6e34d545u32,
        undecoded(0x6e34d545u32)
    );
    eprintln!(
        "0x{:08x}\tfmaxp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6e34f545u32,
        undecoded(0x6e34f545u32)
    );
    eprintln!(
        "0x{:08x}\tfminp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4f545u32,
        undecoded(0x6eb4f545u32)
    );
    eprintln!(
        "0x{:08x}\tfmaxnmp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6e34c545u32,
        undecoded(0x6e34c545u32)
    );
    eprintln!(
        "0x{:08x}\tfminnmp.4s\tv5, v10, v20\tbattery3\t{}",
        0x6eb4c545u32,
        undecoded(0x6eb4c545u32)
    );
    eprintln!(
        "0x{:08x}\tfmul.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f949945u32,
        undecoded(0x4f949945u32)
    );
    eprintln!(
        "0x{:08x}\tfmla.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f941945u32,
        undecoded(0x4f941945u32)
    );
    eprintln!(
        "0x{:08x}\tfmls.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f945945u32,
        undecoded(0x4f945945u32)
    );
    eprintln!(
        "0x{:08x}\tfmulx.4s\tv5, v10, v20[3]\tbattery3\t{}",
        0x6fb49945u32,
        undecoded(0x6fb49945u32)
    );
    eprintln!(
        "0x{:08x}\tmul.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f948945u32,
        undecoded(0x4f948945u32)
    );
    eprintln!(
        "0x{:08x}\tsqdmulh.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f94c945u32,
        undecoded(0x4f94c945u32)
    );
    eprintln!(
        "0x{:08x}\tsqrdmulh.4s\tv5, v10, v20[2]\tbattery3\t{}",
        0x4f94d945u32,
        undecoded(0x4f94d945u32)
    );
    eprintln!(
        "0x{:08x}\tsdot\tv5, v10, v20[1]\tbattery3\t{}",
        0x4fb4e145u32,
        undecoded(0x4fb4e145u32)
    );
    eprintln!(
        "0x{:08x}\tudot\tv5, v10, v20[3]\tbattery3\t{}",
        0x6fb4e945u32,
        undecoded(0x6fb4e945u32)
    );
    eprintln!(
        "0x{:08x}\tsdot\tv5, v10, v20[1]\tbattery3\t{}",
        0x0fb4e145u32,
        undecoded(0x0fb4e145u32)
    );
    eprintln!(
        "0x{:08x}\tshl.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x4f235545u32,
        undecoded(0x4f235545u32)
    );
    eprintln!(
        "0x{:08x}\tshl.2d\tv5, v10, #0x4\tbattery4\t{}",
        0x4f445545u32,
        undecoded(0x4f445545u32)
    );
    eprintln!(
        "0x{:08x}\tshl.8h\tv5, v10, #0x1\tbattery4\t{}",
        0x4f115545u32,
        undecoded(0x4f115545u32)
    );
    eprintln!(
        "0x{:08x}\tsshr.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x4f3d0545u32,
        undecoded(0x4f3d0545u32)
    );
    eprintln!(
        "0x{:08x}\tushr.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f3d0545u32,
        undecoded(0x6f3d0545u32)
    );
    eprintln!(
        "0x{:08x}\tssra.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x4f3d1545u32,
        undecoded(0x4f3d1545u32)
    );
    eprintln!(
        "0x{:08x}\tusra.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f3d1545u32,
        undecoded(0x6f3d1545u32)
    );
    eprintln!(
        "0x{:08x}\tsrsra.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x4f3d3545u32,
        undecoded(0x4f3d3545u32)
    );
    eprintln!(
        "0x{:08x}\tursra.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f3d3545u32,
        undecoded(0x6f3d3545u32)
    );
    eprintln!(
        "0x{:08x}\tsri.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f3d4545u32,
        undecoded(0x6f3d4545u32)
    );
    eprintln!(
        "0x{:08x}\tsli.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f235545u32,
        undecoded(0x6f235545u32)
    );
    eprintln!(
        "0x{:08x}\tsqshl.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x4f237545u32,
        undecoded(0x4f237545u32)
    );
    eprintln!(
        "0x{:08x}\tuqshl.4s\tv5, v10, #0x3\tbattery4\t{}",
        0x6f237545u32,
        undecoded(0x6f237545u32)
    );
    eprintln!(
        "0x{:08x}\tsqshl.16b\tv5, v10, #0x2\tbattery4\t{}",
        0x4f0a7545u32,
        undecoded(0x4f0a7545u32)
    );
    eprintln!(
        "0x{:08x}\tsshl.2d\tv5, v10, v20\tbattery4\t{}",
        0x4ef44545u32,
        undecoded(0x4ef44545u32)
    );
    eprintln!(
        "0x{:08x}\tsshll.4s\tv5, v10, #0x0\tbattery4\t{}",
        0x0f10a545u32,
        undecoded(0x0f10a545u32)
    );
    eprintln!(
        "0x{:08x}\tushll.4s\tv5, v10, #0x0\tbattery4\t{}",
        0x2f10a545u32,
        undecoded(0x2f10a545u32)
    );
    eprintln!(
        "0x{:08x}\tsshll.2d\tv5, v10, #0x0\tbattery4\t{}",
        0x0f20a545u32,
        undecoded(0x0f20a545u32)
    );
    eprintln!(
        "0x{:08x}\tushll.8h\tv5, v10, #0x0\tbattery4\t{}",
        0x2f08a545u32,
        undecoded(0x2f08a545u32)
    );
    eprintln!(
        "0x{:08x}\tssubl.4s\tv5, v10, v20\tbattery4\t{}",
        0x0e742145u32,
        undecoded(0x0e742145u32)
    );
    eprintln!(
        "0x{:08x}\tusubl.4s\tv5, v10, v20\tbattery4\t{}",
        0x2e742145u32,
        undecoded(0x2e742145u32)
    );
    eprintln!(
        "0x{:08x}\tsaba.4s\tv5, v10, v20\tbattery4\t{}",
        0x4eb47d45u32,
        undecoded(0x4eb47d45u32)
    );
    eprintln!(
        "0x{:08x}\tuaba.4s\tv5, v10, v20\tbattery4\t{}",
        0x6eb47d45u32,
        undecoded(0x6eb47d45u32)
    );
    eprintln!(
        "0x{:08x}\tsabd.4s\tv5, v10, v20\tbattery4\t{}",
        0x4eb47545u32,
        undecoded(0x4eb47545u32)
    );
    eprintln!(
        "0x{:08x}\tuabd.4s\tv5, v10, v20\tbattery4\t{}",
        0x6eb47545u32,
        undecoded(0x6eb47545u32)
    );
    eprintln!(
        "0x{:08x}\tdup.4s\tv5, v10[3]\tbattery4\t{}",
        0x4e1c0545u32,
        undecoded(0x4e1c0545u32)
    );
    eprintln!(
        "0x{:08x}\tdup.8h\tv5, v10[2]\tbattery4\t{}",
        0x4e0a0545u32,
        undecoded(0x4e0a0545u32)
    );
    eprintln!(
        "0x{:08x}\tdup.16b\tv5, v10[0]\tbattery4\t{}",
        0x4e010545u32,
        undecoded(0x4e010545u32)
    );
    eprintln!(
        "0x{:08x}\tdup.2d\tv5, v10[1]\tbattery4\t{}",
        0x4e180545u32,
        undecoded(0x4e180545u32)
    );
    eprintln!(
        "0x{:08x}\tdup.4s\tv5, w10\tbattery4\t{}",
        0x4e040d45u32,
        undecoded(0x4e040d45u32)
    );
    eprintln!(
        "0x{:08x}\tdup.16b\tv5, w10\tbattery4\t{}",
        0x4e010d45u32,
        undecoded(0x4e010d45u32)
    );
    eprintln!(
        "0x{:08x}\tdup.2d\tv5, x10\tbattery4\t{}",
        0x4e080d45u32,
        undecoded(0x4e080d45u32)
    );
    eprintln!(
        "0x{:08x}\tmov.s\tv5[2], w10\tbattery4\t{}",
        0x4e141d45u32,
        undecoded(0x4e141d45u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tv5[1], x10\tbattery4\t{}",
        0x4e181d45u32,
        undecoded(0x4e181d45u32)
    );
    eprintln!(
        "0x{:08x}\tmov.b\tv5[3], w10\tbattery4\t{}",
        0x4e071d45u32,
        undecoded(0x4e071d45u32)
    );
    eprintln!(
        "0x{:08x}\tmov.s\tv5[1], v20[2]\tbattery4\t{}",
        0x6e0c4685u32,
        undecoded(0x6e0c4685u32)
    );
    eprintln!(
        "0x{:08x}\tmov.s\tw1, v5[1]\tbattery4\t{}",
        0x0e0c3ca1u32,
        undecoded(0x0e0c3ca1u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tx1, v5[1]\tbattery4\t{}",
        0x4e183ca1u32,
        undecoded(0x4e183ca1u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tv5[1], x1\tbattery4\t{}",
        0x4e181c25u32,
        undecoded(0x4e181c25u32)
    );
    eprintln!(
        "0x{:08x}\tsmov.h\tw1, v5[2]\tbattery4\t{}",
        0x0e0a2ca1u32,
        undecoded(0x0e0a2ca1u32)
    );
    eprintln!(
        "0x{:08x}\tmov.d\tx1, v5[0]\tbattery4\t{}",
        0x4e083ca1u32,
        undecoded(0x4e083ca1u32)
    );
    eprintln!(
        "0x{:08x}\ttbl.16b\tv5, {{ v10 }}, v20\tbattery4\t{}",
        0x4e140145u32,
        undecoded(0x4e140145u32)
    );
    eprintln!(
        "0x{:08x}\ttbl.16b\tv5, {{ v10, v11 }}, v20\tbattery4\t{}",
        0x4e142145u32,
        undecoded(0x4e142145u32)
    );
    eprintln!(
        "0x{:08x}\ttbl.16b\tv5, {{ v10, v11, v12 }}, v20\tbattery4\t{}",
        0x4e144145u32,
        undecoded(0x4e144145u32)
    );
    eprintln!(
        "0x{:08x}\ttbl.16b\tv5, {{ v10, v11, v12, v13 }}, v20\tbattery4\t{}",
        0x4e146145u32,
        undecoded(0x4e146145u32)
    );
    eprintln!(
        "0x{:08x}\ttbx.16b\tv5, {{ v10 }}, v20\tbattery4\t{}",
        0x4e141145u32,
        undecoded(0x4e141145u32)
    );
    eprintln!(
        "0x{:08x}\ttbx.16b\tv5, {{ v10, v11, v12, v13 }}, v20\tbattery4\t{}",
        0x4e147145u32,
        undecoded(0x4e147145u32)
    );
    eprintln!(
        "0x{:08x}\ttbl.8b\tv5, {{ v10 }}, v20\tbattery4\t{}",
        0x0e140145u32,
        undecoded(0x0e140145u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.16b\tv5, v10, v20\tbattery4\t{}",
        0x4e143945u32,
        undecoded(0x4e143945u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.8h\tv5, v10, v20\tbattery4\t{}",
        0x4e543945u32,
        undecoded(0x4e543945u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e943945u32,
        undecoded(0x4e943945u32)
    );
    eprintln!(
        "0x{:08x}\tzip2.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e947945u32,
        undecoded(0x4e947945u32)
    );
    eprintln!(
        "0x{:08x}\tuzp1.8h\tv5, v10, v20\tbattery4\t{}",
        0x4e541945u32,
        undecoded(0x4e541945u32)
    );
    eprintln!(
        "0x{:08x}\tuzp1.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e941945u32,
        undecoded(0x4e941945u32)
    );
    eprintln!(
        "0x{:08x}\tuzp2.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e945945u32,
        undecoded(0x4e945945u32)
    );
    eprintln!(
        "0x{:08x}\ttrn1.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e942945u32,
        undecoded(0x4e942945u32)
    );
    eprintln!(
        "0x{:08x}\ttrn1.8b\tv5, v10, v20\tbattery4\t{}",
        0x0e142945u32,
        undecoded(0x0e142945u32)
    );
    eprintln!(
        "0x{:08x}\ttrn2.4s\tv5, v10, v20\tbattery4\t{}",
        0x4e946945u32,
        undecoded(0x4e946945u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.2d\tv5, v10, v20\tbattery4\t{}",
        0x4ed43945u32,
        undecoded(0x4ed43945u32)
    );
    eprintln!(
        "0x{:08x}\tuzp1.2d\tv5, v10, v20\tbattery4\t{}",
        0x4ed41945u32,
        undecoded(0x4ed41945u32)
    );
    eprintln!(
        "0x{:08x}\ttrn1.2d\tv5, v10, v20\tbattery4\t{}",
        0x4ed42945u32,
        undecoded(0x4ed42945u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.8b\tv5, v10, v20\tbattery4\t{}",
        0x0e143945u32,
        undecoded(0x0e143945u32)
    );
    eprintln!(
        "0x{:08x}\text.16b\tv5, v10, v20, #0x4\tbattery4\t{}",
        0x6e142145u32,
        undecoded(0x6e142145u32)
    );
    eprintln!(
        "0x{:08x}\text.8b\tv5, v10, v20, #0x2\tbattery4\t{}",
        0x2e141145u32,
        undecoded(0x2e141145u32)
    );
    eprintln!(
        "0x{:08x}\trev16.16b\tv5, v10\tbattery4\t{}",
        0x4e201945u32,
        undecoded(0x4e201945u32)
    );
    eprintln!(
        "0x{:08x}\trev16.8b\tv5, v10\tbattery4\t{}",
        0x0e201945u32,
        undecoded(0x0e201945u32)
    );
    eprintln!(
        "0x{:08x}\trev32.8b\tv5, v10\tbattery4\t{}",
        0x2e200945u32,
        undecoded(0x2e200945u32)
    );
    eprintln!(
        "0x{:08x}\trev32.8h\tv5, v10\tbattery4\t{}",
        0x6e600945u32,
        undecoded(0x6e600945u32)
    );
    eprintln!(
        "0x{:08x}\trev64.4s\tv5, v10\tbattery4\t{}",
        0x4ea00945u32,
        undecoded(0x4ea00945u32)
    );
    eprintln!(
        "0x{:08x}\trev64.8b\tv5, v10\tbattery4\t{}",
        0x0e200945u32,
        undecoded(0x0e200945u32)
    );
    eprintln!(
        "0x{:08x}\trev64.8h\tv5, v10\tbattery4\t{}",
        0x4e600945u32,
        undecoded(0x4e600945u32)
    );
    eprintln!(
        "0x{:08x}\trbit.16b\tv5, v10\tbattery4\t{}",
        0x6e605945u32,
        undecoded(0x6e605945u32)
    );
    eprintln!(
        "0x{:08x}\tsqxtn.4h\tv5, v10\tbattery4\t{}",
        0x0e614945u32,
        undecoded(0x0e614945u32)
    );
    eprintln!(
        "0x{:08x}\tuqxtn.4h\tv5, v10\tbattery4\t{}",
        0x2e614945u32,
        undecoded(0x2e614945u32)
    );
    eprintln!(
        "0x{:08x}\tsqxtn2.8h\tv5, v10\tbattery4\t{}",
        0x4e614945u32,
        undecoded(0x4e614945u32)
    );
    eprintln!(
        "0x{:08x}\tsqxtun.4h\tv5, v10\tbattery4\t{}",
        0x2e612945u32,
        undecoded(0x2e612945u32)
    );
    eprintln!(
        "0x{:08x}\ttrn1.4h\tv5, v10, v20\tbattery4\t{}",
        0x0e542945u32,
        undecoded(0x0e542945u32)
    );
    eprintln!(
        "0x{:08x}\tuzp1.4h\tv5, v10, v20\tbattery4\t{}",
        0x0e541945u32,
        undecoded(0x0e541945u32)
    );
    eprintln!(
        "0x{:08x}\tzip1.4h\tv5, v10, v20\tbattery4\t{}",
        0x0e543945u32,
        undecoded(0x0e543945u32)
    );
    eprintln!(
        "0x{:08x}\tldxp\tw1, w2, [x3]\tbattery5\t{}",
        0x887f0861u32,
        undecoded(0x887f0861u32)
    );
    eprintln!(
        "0x{:08x}\tldxp\tx1, x2, [x3]\tbattery5\t{}",
        0xc87f0861u32,
        undecoded(0xc87f0861u32)
    );
    eprintln!(
        "0x{:08x}\tstxp\tw4, x1, x2, [x3]\tbattery5\t{}",
        0xc8240861u32,
        undecoded(0xc8240861u32)
    );
    eprintln!(
        "0x{:08x}\tstxp\tw4, w1, w2, [x3]\tbattery5\t{}",
        0x88240861u32,
        undecoded(0x88240861u32)
    );
    eprintln!(
        "0x{:08x}\tldaxp\tx1, x2, [x3]\tbattery5\t{}",
        0xc87f8861u32,
        undecoded(0xc87f8861u32)
    );
    eprintln!(
        "0x{:08x}\tstlxp\tw4, x1, x2, [x3]\tbattery5\t{}",
        0xc8248861u32,
        undecoded(0xc8248861u32)
    );
    eprintln!(
        "0x{:08x}\tldaxp\tw1, w2, [x3]\tbattery5\t{}",
        0x887f8861u32,
        undecoded(0x887f8861u32)
    );
    eprintln!(
        "0x{:08x}\tstlxp\tw4, w1, w2, [x3]\tbattery5\t{}",
        0x88248861u32,
        undecoded(0x88248861u32)
    );
    eprintln!(
        "0x{:08x}\tldar\tw1, [x3]\tbattery5\t{}",
        0x88dffc61u32,
        undecoded(0x88dffc61u32)
    );
    eprintln!(
        "0x{:08x}\tldar\tx1, [x3]\tbattery5\t{}",
        0xc8dffc61u32,
        undecoded(0xc8dffc61u32)
    );
    eprintln!(
        "0x{:08x}\tldarb\tw1, [x3]\tbattery5\t{}",
        0x08dffc61u32,
        undecoded(0x08dffc61u32)
    );
    eprintln!(
        "0x{:08x}\tldarh\tw1, [x3]\tbattery5\t{}",
        0x48dffc61u32,
        undecoded(0x48dffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlr\tw1, [x3]\tbattery5\t{}",
        0x889ffc61u32,
        undecoded(0x889ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlr\tx1, [x3]\tbattery5\t{}",
        0xc89ffc61u32,
        undecoded(0xc89ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlrb\tw1, [x3]\tbattery5\t{}",
        0x089ffc61u32,
        undecoded(0x089ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlrh\tw1, [x3]\tbattery5\t{}",
        0x489ffc61u32,
        undecoded(0x489ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tldaxr\tw1, [x3]\tbattery5\t{}",
        0x885ffc61u32,
        undecoded(0x885ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tldaxr\tx1, [x3]\tbattery5\t{}",
        0xc85ffc61u32,
        undecoded(0xc85ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlxr\tw4, w1, [x3]\tbattery5\t{}",
        0x8804fc61u32,
        undecoded(0x8804fc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlxr\tw4, x1, [x3]\tbattery5\t{}",
        0xc804fc61u32,
        undecoded(0xc804fc61u32)
    );
    eprintln!(
        "0x{:08x}\tldaxrb\tw1, [x3]\tbattery5\t{}",
        0x085ffc61u32,
        undecoded(0x085ffc61u32)
    );
    eprintln!(
        "0x{:08x}\tstlxrb\tw4, w1, [x3]\tbattery5\t{}",
        0x0804fc61u32,
        undecoded(0x0804fc61u32)
    );
    eprintln!(
        "0x{:08x}\tsev\tbattery5\t{}",
        0xd503209fu32,
        undecoded(0xd503209fu32)
    );
    eprintln!(
        "0x{:08x}\tsevl\tbattery5\t{}",
        0xd50320bfu32,
        undecoded(0xd50320bfu32)
    );
    eprintln!(
        "0x{:08x}\tyield\tbattery5\t{}",
        0xd503203fu32,
        undecoded(0xd503203fu32)
    );
    eprintln!(
        "0x{:08x}\tnop\tbattery5\t{}",
        0xd503201fu32,
        undecoded(0xd503201fu32)
    );
    eprintln!(
        "0x{:08x}\tdmb\tish\tbattery5\t{}",
        0xd5033bbfu32,
        undecoded(0xd5033bbfu32)
    );
    eprintln!(
        "0x{:08x}\tdsb\tish\tbattery5\t{}",
        0xd5033b9fu32,
        undecoded(0xd5033b9fu32)
    );
    eprintln!(
        "0x{:08x}\tisb\tbattery5\t{}",
        0xd5033fdfu32,
        undecoded(0xd5033fdfu32)
    );
    eprintln!(
        "0x{:08x}\tclrex\tbattery5\t{}",
        0xd5033f5fu32,
        undecoded(0xd5033f5fu32)
    );
    eprintln!(
        "0x{:08x}\tesb\tbattery5\t{}",
        0xd503221fu32,
        undecoded(0xd503221fu32)
    );
    eprintln!(
        "0x{:08x}\tcsdb\tbattery5\t{}",
        0xd503229fu32,
        undecoded(0xd503229fu32)
    );
    eprintln!(
        "0x{:08x}\tcbnz\tx1, 0x180 <ltmp0+0x180>\tbattery5\t{}",
        0xb5000801u32,
        undecoded(0xb5000801u32)
    );
    eprintln!(
        "0x{:08x}\ttbnz\tw1, #0x7, 0x184 <ltmp0+0x184>\tbattery5\t{}",
        0x37380801u32,
        undecoded(0x37380801u32)
    );
    eprintln!(
        "0x{:08x}\tadrp\tx1, 0x80000000 <ltmp0+0x80000000>\tbattery5\t{}",
        0x90400001u32,
        undecoded(0x90400001u32)
    );
    eprintln!(
        "0x{:08x}\tubfx\tx1, x2, #3, #5\tbattery5\t{}",
        0xd3431c41u32,
        undecoded(0xd3431c41u32)
    );
    eprintln!(
        "0x{:08x}\tsbfx\tw1, w2, #1, #30\tbattery5\t{}",
        0x13017841u32,
        undecoded(0x13017841u32)
    );
    eprintln!(
        "0x{:08x}\tbfi\tx1, x2, #4, #8\tbattery5\t{}",
        0xb37c1c41u32,
        undecoded(0xb37c1c41u32)
    );
    eprintln!(
        "0x{:08x}\tlsl\tx1, x2, #3\tbattery5\t{}",
        0xd37df041u32,
        undecoded(0xd37df041u32)
    );
    eprintln!(
        "0x{:08x}\tlsr\tw1, w2, #4\tbattery5\t{}",
        0x53047c41u32,
        undecoded(0x53047c41u32)
    );
    eprintln!(
        "0x{:08x}\tasr\tx1, x2, #5\tbattery5\t{}",
        0x9345fc41u32,
        undecoded(0x9345fc41u32)
    );
    eprintln!(
        "0x{:08x}\tror\tw1, w2, #0x6\tbattery5\t{}",
        0x13821841u32,
        undecoded(0x13821841u32)
    );
    eprintln!(
        "0x{:08x}\tmrs\tx1, CNTFRQ_EL0\tbattery5\t{}",
        0xd53be001u32,
        undecoded(0xd53be001u32)
    );
    eprintln!(
        "0x{:08x}\tmsr\tDAIFSet, #0x3\tbattery5\t{}",
        0xd50343dfu32,
        undecoded(0xd50343dfu32)
    );
    eprintln!(
        "0x{:08x}\tmsr\tSPSel, #0x1\tbattery5\t{}",
        0xd50041bfu32,
        undecoded(0xd50041bfu32)
    );
    eprintln!(
        "0x{:08x}\tmrs\tx1, TPIDR_EL1\tbattery5\t{}",
        0xd538d081u32,
        undecoded(0xd538d081u32)
    );
    eprintln!(
        "0x{:08x}\tpacia\tx1, x2\tbattery5\t{}",
        0xdac10041u32,
        undecoded(0xdac10041u32)
    );
    eprintln!(
        "0x{:08x}\tautia\tx1, x2\tbattery5\t{}",
        0xdac11041u32,
        undecoded(0xdac11041u32)
    );
    eprintln!(
        "0x{:08x}\tretaa\tbattery5\t{}",
        0xd65f0bffu32,
        undecoded(0xd65f0bffu32)
    );
    eprintln!(
        "0x{:08x}\tswp\tx1, x2, [x3]\tbattery5\t{}",
        0xf8218062u32,
        undecoded(0xf8218062u32)
    );
    eprintln!(
        "0x{:08x}\tstadd\tx1, [x3]\tbattery5\t{}",
        0xf821007fu32,
        undecoded(0xf821007fu32)
    );
    eprintln!(
        "0x{:08x}\tcas\tw1, w2, [x3]\tbattery5\t{}",
        0x88a17c62u32,
        undecoded(0x88a17c62u32)
    );
    eprintln!(
        "0x{:08x}\tldadd\tx1, x2, [x3]\tbattery5\t{}",
        0xf8210062u32,
        undecoded(0xf8210062u32)
    );
    eprintln!(
        "0x{:08x}\tldeor\tw1, w2, [x3]\tbattery5\t{}",
        0xb8212062u32,
        undecoded(0xb8212062u32)
    );
    eprintln!(
        "0x{:08x}\tldclr\tx1, x2, [x3]\tbattery5\t{}",
        0xf8211062u32,
        undecoded(0xf8211062u32)
    );
    eprintln!(
        "0x{:08x}\tldset\tx1, x2, [x3]\tbattery5\t{}",
        0xf8213062u32,
        undecoded(0xf8213062u32)
    );
    eprintln!(
        "0x{:08x}\tbfxil\tw1, w2, #0, #1\tbattery5\t{}",
        0x33000041u32,
        undecoded(0x33000041u32)
    );
}

fn fresh_machine() -> Machine {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    for i in 0..31usize {
        m.cores.active_mut().state.x[i] = 0x100000;
    }
    for i in 0..32usize {
        m.cores.active_mut().state.v[i] = 0;
    }
    m.bus
        .map(BusRegion {
            base: 0x100000,
            size: 0x100000,
            device: Box::new(SparseRam::new(0x100000, 0x100000)),
        })
        .unwrap();
    m
}

fn set_b(v: u128, byte: usize, val: u8) -> u128 {
    v & !((0xffu128) << (byte * 8)) | ((val as u128) << (byte * 8))
}

// Independent reference: signed (sdot) or unsigned (udot) dot of one lane.
fn dot_ref(signed: bool, vn: u128, vm: u128, lane: usize, elem: Option<usize>, acc: i64) -> i64 {
    let src = if let Some(e) = elem {
        (vm >> (e * 32)) as u32
    } else {
        (vm >> (lane * 32)) as u32
    };
    let mut a = acc;
    for b in 0..4usize {
        let a8 = (vn >> (lane * 32 + b * 8)) as u8;
        let b8 = (src >> (b * 8)) as u8;
        if signed {
            a += (a8 as i8 as i64) * (b8 as i8 as i64);
        } else {
            a += (a8 as i64) * (b8 as i64);
        }
    }
    a
}

// Clang aarch64 ground-truth encodings of ARMv8.2 dot product.
const SDOT_VEC_4S: u32 = 0x4e949545; // sdot v5.4S, v10.16B, v20.16B
const SDOT_VEC_4S_V0: u32 = 0x4e829420; // sdot v0.4S, v1.16B, v2.16B (was mis-decoded as MLA)
const UDOT_VEC_4S: u32 = 0x6e949545; // udot v5.4S, v10.16B, v20.16B
const SDOT_VEC_2S: u32 = 0x0e949545; // sdot v5.2S, v10.8B, v20.8B
const UDOT_VEC_2S: u32 = 0x2e949545; // udot v5.2S, v10.8B, v20.8B
const SDOT_ELEM: u32 = 0x4fb4e145; // sdot v5.4S, v10.16B, v20.4B[1]
const UDOT_ELEM: u32 = 0x6fb4e945; // udot v5.4S, v10.16B, v20.4B[3]
const MLA_4S: u32 = 0x4ea29420; // mla v0.4S, v1.4S, v2.4S (must NOT decode as dot)

#[test]
fn dot_products_decode_and_execute() {
    for w in [
        SDOT_VEC_4S,
        UDOT_VEC_4S,
        SDOT_VEC_2S,
        UDOT_VEC_2S,
        SDOT_ELEM,
        UDOT_ELEM,
    ] {
        assert_eq!(
            execute(&mut fresh_machine(), Instruction(w)),
            Ok(()),
            "dot opcode {w:#010x} must decode and execute"
        );
    }
    assert_eq!(
        iphem_arm::disassemble(0, SDOT_VEC_4S_V0),
        "sdot\tv0, v1.16b, v2.16b"
    );
}

#[test]
fn sdot_vector_matches_reference() {
    let mut vn = 0u128;
    for (i, b) in [1u8, 2, 3, 4, 5, 6, 7, 8, 0xff, 0xfe, 0xfd, 0xfc, 2, 2, 2, 2]
        .iter()
        .enumerate()
    {
        vn = set_b(vn, i, *b);
    }
    let mut vm = 0u128;
    for (i, b) in [3u8, 1, 3, 1, 2, 2, 2, 2, 1, 1, 1, 1, 4, 4, 4, 4]
        .iter()
        .enumerate()
    {
        vm = set_b(vm, i, *b);
    }
    let mut m = fresh_machine();
    m.cores.active_mut().state.v[10] = vn;
    m.cores.active_mut().state.v[20] = vm;
    m.cores.active_mut().state.v[5] =
        0x1000 | (0x2000u128 << 32) | (0x3000u128 << 64) | (0x4000u128 << 96);
    execute(&mut m, Instruction(SDOT_VEC_4S)).unwrap();
    let got = m.cores.active().state.v[5];
    for lane in 0..4usize {
        let exp = dot_ref(true, vn, vm, lane, None, (lane as i64 + 1) * 0x1000) as u64 as u128;
        assert_eq!((got >> (lane * 32)) as u32, exp as u32, "sdot lane {lane}");
    }
}

#[test]
fn udot_vector_matches_reference() {
    let mut vn = 0u128;
    for (i, b) in [1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4]
        .iter()
        .enumerate()
    {
        vn = set_b(vn, i, *b);
    }
    let mut vm = 0u128;
    for (i, b) in [5u8, 5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 8, 8, 8, 8]
        .iter()
        .enumerate()
    {
        vm = set_b(vm, i, *b);
    }
    let mut m = fresh_machine();
    m.cores.active_mut().state.v[10] = vn;
    m.cores.active_mut().state.v[20] = vm;
    m.cores.active_mut().state.v[5] = 0;
    execute(&mut m, Instruction(UDOT_VEC_4S)).unwrap();
    let got = m.cores.active().state.v[5];
    for lane in 0..4usize {
        let exp = dot_ref(false, vn, vm, lane, None, 0) as u64 as u128;
        assert_eq!((got >> (lane * 32)) as u32, exp as u32, "udot lane {lane}");
    }
}

#[test]
fn dot_by_element_matches_reference() {
    // signed: sdot v5.4S, v10.16B, v20.4B[1]
    let mut vn = 0u128;
    for (i, b) in [2u8, 2, 2, 2, 1, 1, 1, 1, 3, 3, 3, 3, 4, 4, 4, 4]
        .iter()
        .enumerate()
    {
        vn = set_b(vn, i, *b);
    }
    let mut vm = 0u128;
    for (i, b) in [9u8, 9, 9, 9, 1, 2, 3, 4, 8, 8, 8, 8, 7, 7, 7, 7]
        .iter()
        .enumerate()
    {
        vm = set_b(vm, i, *b);
    }
    let mut m = fresh_machine();
    m.cores.active_mut().state.v[10] = vn;
    m.cores.active_mut().state.v[20] = vm;
    m.cores.active_mut().state.v[5] = 0;
    execute(&mut m, Instruction(SDOT_ELEM)).unwrap();
    let got = m.cores.active().state.v[5];
    for lane in 0..4usize {
        let exp = dot_ref(true, vn, vm, lane, Some(1), 0) as u64 as u128;
        assert_eq!(
            (got >> (lane * 32)) as u32,
            exp as u32,
            "sdot[x] lane {lane}"
        );
    }
    // unsigned: udot v5.4S, v10.16B, v20.4B[3]
    for (i, b) in [1u8, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 100, 100, 100, 100]
        .iter()
        .enumerate()
    {
        vn = set_b(vn, i, *b);
    }
    for (i, b) in [1u8, 2, 3, 4, 5, 6, 7, 8, 5, 5, 5, 5, 1, 1, 2, 2]
        .iter()
        .enumerate()
    {
        vm = set_b(vm, i, *b);
    }
    let mut m = fresh_machine();
    m.cores.active_mut().state.v[10] = vn;
    m.cores.active_mut().state.v[20] = vm;
    m.cores.active_mut().state.v[5] = 0;
    execute(&mut m, Instruction(UDOT_ELEM)).unwrap();
    let got = m.cores.active().state.v[5];
    for lane in 0..4usize {
        let exp = dot_ref(false, vn, vm, lane, Some(3), 0) as u64 as u128;
        assert_eq!(
            (got >> (lane * 32)) as u32,
            exp as u32,
            "udot[x] lane {lane}"
        );
    }
}

#[test]
fn dot_64bit_preserves_upper_half() {
    let mut vn = 0u128;
    for (i, b) in [1u8, 1, 1, 1, 2, 2, 2, 2].iter().enumerate() {
        vn = set_b(vn, i, *b);
    }
    let mut vm = 0u128;
    for (i, b) in [3u8, 3, 3, 3, 4, 4, 4, 4].iter().enumerate() {
        vm = set_b(vm, i, *b);
    }
    let mut m = fresh_machine();
    m.cores.active_mut().state.v[10] = vn;
    m.cores.active_mut().state.v[20] = vm;
    m.cores.active_mut().state.v[5] = 0xdead_beef_cafe_f00d_0000_0000_0000_0000;
    execute(&mut m, Instruction(SDOT_VEC_2S)).unwrap();
    let got = m.cores.active().state.v[5];
    assert_eq!(
        got as u32,
        dot_ref(true, vn, vm, 0, None, 0) as u32,
        "lane 0 (2S)"
    );
    assert_eq!(
        (got >> 32) as u32,
        dot_ref(true, vn, vm, 1, None, 0) as u32,
        "lane 1 (2S)"
    );
    assert_eq!(got >> 64, 0xdead_beef_cafe_f00d, "upper half preserved");
    assert_eq!(
        iphem_arm::disassemble(0, SDOT_VEC_2S),
        "sdot\tv5, v10.8b, v20.8b"
    );
}

#[test]
fn mla_is_not_misdecoded_as_dot() {
    assert!(
        !iphem_arm::disassemble(0, MLA_4S).contains("dot"),
        "mla v0.4s,v1.4s,v2.4s must stay an MLA: {}",
        iphem_arm::disassemble(0, MLA_4S)
    );
    // And the sdot v0.4s, v1.16b, v2.16b word must NOT be claimed by MLA:
    assert_eq!(
        iphem_arm::disassemble(0, SDOT_VEC_4S_V0),
        "sdot\tv0, v1.16b, v2.16b"
    );
}
