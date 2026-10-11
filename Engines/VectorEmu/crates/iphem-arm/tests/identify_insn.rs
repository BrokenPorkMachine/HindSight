use iphem_arm::disassemble;

#[test]
fn identify_insn() {
    let insns = [0xf820cd09, 0xf2e94d48, 0xf800cd09, 0xf820c909];
    for &insn in &insns {
        let asm = disassemble(0xfffffff00800fc44, insn);
        println!("0x{:08x}: {}", insn, asm);
    }
}
