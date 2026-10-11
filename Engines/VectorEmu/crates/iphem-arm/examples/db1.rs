use iphem_arm::disassemble;
fn main() {
    for w in [0xd503201fu32, 0xd50330bfu32] {
        println!("0x{:08x} -> {}", w, disassemble(0, w));
    }
}
