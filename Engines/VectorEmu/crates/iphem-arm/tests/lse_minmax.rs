use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{BusRegion, Machine, MachineConfig, SparseRam};

fn machine() -> Machine {
    let mut machine = Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    });
    machine
        .bus
        .map(BusRegion {
            base: 0x4000,
            size: 0x1000,
            device: Box::new(SparseRam::new(0x4000, 0x1000)),
        })
        .unwrap();
    machine
}

#[test]
fn signed_word_min_and_max_compare_as_signed_and_return_old_value() {
    let mut machine = machine();
    let address = 0x4080;
    machine.cores.active_mut().state.x[10] = address;
    machine.cores.active_mut().state.x[8] = 3;
    machine.cores.active_mut().state.x[9] = 0;

    machine
        .mem_write(address, 4, (-4i32) as u32 as u64)
        .unwrap();
    // LDSMAXAL W8, W9, [X10]: old -4 is less than operand 3.
    execute(&mut machine, Instruction(0xb8e8_4149)).unwrap();
    assert_eq!(machine.mem_read(address, 4).unwrap(), 3);
    assert_eq!(machine.cores.active().state.x[9], (-4i32) as u32 as u64);

    machine.cores.active_mut().state.x[8] = (-9i32) as u32 as u64;
    // LDSMIN W8, W9, [X10]: signed -9 is less than the old value 3.
    execute(&mut machine, Instruction(0xb828_5149)).unwrap();
    assert_eq!(machine.mem_read(address, 4).unwrap(), (-9i32) as u32 as u64);
    assert_eq!(machine.cores.active().state.x[9], 3);
}

#[test]
fn unsigned_minmax_and_byte_halfword_widths_use_only_the_selected_lane() {
    let mut machine = machine();
    let address = 0x4080;
    machine.cores.active_mut().state.x[10] = address;

    machine.mem_write(address, 4, 0x8000_0000).unwrap();
    machine.cores.active_mut().state.x[8] = 0x7fff_ffff;
    // LDUMAXAL W8, W9, [X10].
    execute(&mut machine, Instruction(0xb8e8_6149)).unwrap();
    assert_eq!(machine.mem_read(address, 4).unwrap(), 0x8000_0000);
    assert_eq!(machine.cores.active().state.x[9], 0x8000_0000);

    machine
        .mem_write(address, 8, 0xffff_ffff_ffff_fff0)
        .unwrap();
    machine.cores.active_mut().state.x[8] = 0x20;
    // LDUMIN X8, X9, [X10].
    execute(&mut machine, Instruction(0xf828_7149)).unwrap();
    assert_eq!(machine.mem_read(address, 8).unwrap(), 0x20);

    machine.mem_write(address, 1, 0xf0).unwrap();
    machine.cores.active_mut().state.x[8] = 0x1_0000_0001;
    // LDSMINB W8, W9, [X10] compares -16 and 1, storing -16.
    execute(&mut machine, Instruction(0x38e8_5149)).unwrap();
    assert_eq!(machine.mem_read(address, 1).unwrap(), 0xf0);

    machine.mem_write(address, 2, 0xfff0).unwrap();
    machine.cores.active_mut().state.x[8] = 0x1_0000_0005;
    // LDUMINH W8, W9, [X10] compares 0xfff0 and 5, storing 5.
    execute(&mut machine, Instruction(0x7828_7149)).unwrap();
    assert_eq!(machine.mem_read(address, 2).unwrap(), 5);
}

#[test]
fn store_aliases_update_memory_without_writing_a_result_register() {
    let mut machine = machine();
    let address = 0x4080;
    machine.cores.active_mut().state.x[10] = address;
    machine.cores.active_mut().state.x[8] = 0x7fff_ffff;
    machine.mem_write(address, 4, 0x8000_0000).unwrap();

    // STSMAX W8, [X10] uses Rt=31 and the same atomic operation encoding.
    execute(&mut machine, Instruction(0xb828_415f)).unwrap();
    assert_eq!(machine.mem_read(address, 4).unwrap(), 0x7fff_ffff);

    let cases = [
        (0xb828_415f, "stsmax\tw8, [x10]"),
        (0xb829_515f, "stsmin\tw9, [x10]"),
        (0xb828_615f, "stumax\tw8, [x10]"),
        (0xb829_715f, "stumin\tw9, [x10]"),
    ];
    for (word, expected) in cases {
        assert_eq!(disassemble(0, word), expected);
    }
}

#[test]
fn lse_disassembly_preserves_operation_width_and_ordering_suffixes() {
    let cases = [
        (0xb8e8_4149, "ldsmaxal\tw8, w9, [x10]"),
        (0xf828_5149, "ldsmin\tx8, x9, [x10]"),
        (0xb8e8_6149, "ldumaxal\tw8, w9, [x10]"),
        (0xc8e9_7c01, "casa\tx9, x1, [x0]"),
    ];
    for (word, expected) in cases {
        assert_eq!(disassemble(0, word), expected, "{word:#010x}");
    }
}
