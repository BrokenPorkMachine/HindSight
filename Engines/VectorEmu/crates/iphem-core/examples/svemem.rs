use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};
fn main() {
    let mut m = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    let base = 0x1000;
    m.bus
        .map(BusRegion {
            base,
            size: 0x200,
            device: Box::new(SparseRam::new(base, 0x200)),
        })
        .unwrap();
    m.mem_write(base, 8, 0x1011_1213_1415_1617).unwrap();
    println!("read base   = {:#x}", m.mem_read(base, 8).unwrap());
    println!("read base+8 = {:#x}", m.mem_read(base + 8, 8).unwrap());
}
