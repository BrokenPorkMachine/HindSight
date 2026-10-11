use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{Machine, MachineConfig};

fn machine() -> Machine {
    Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    })
}

fn bytes(value: u128) -> [u8; 16] {
    value.to_le_bytes()
}

fn vector(value: [u8; 16]) -> u128 {
    u128::from_le_bytes(value)
}

#[test]
fn aese_and_aesd_transform_the_full_state_and_round_trip() {
    let mut machine = machine();
    machine.cores.active_mut().state.v[0] = vector(std::array::from_fn(|i| i as u8));
    machine.cores.active_mut().state.v[1] = 0;

    // AESE V0.16B, V1.16B.
    execute(&mut machine, Instruction(0x4e28_4820)).unwrap();
    assert_eq!(
        bytes(machine.cores.active().state.v[0]),
        [
            0x63, 0x6b, 0x67, 0x76, 0xf2, 0x01, 0xab, 0x7b, 0x30, 0xd7, 0x77, 0xc5, 0xfe, 0x7c,
            0x6f, 0x2b,
        ]
    );

    // AESD V0.16B, V1.16B reverses the zero-round-key transform.
    execute(&mut machine, Instruction(0x4e28_5820)).unwrap();
    assert_eq!(
        bytes(machine.cores.active().state.v[0]),
        std::array::from_fn(|i| i as u8)
    );
}

#[test]
fn aesmc_matches_the_aes_mixcolumns_vector_and_aesimc_inverts_it() {
    let mut machine = machine();
    machine.cores.active_mut().state.v[5] = vector([
        0xdb, 0x13, 0x53, 0x45, 0xf2, 0x0a, 0x22, 0x5c, 0x01, 0x01, 0x01, 0x01, 0xc6, 0xc6, 0xc6,
        0xc6,
    ]);

    // AESMC V4.16B, V5.16B.
    execute(&mut machine, Instruction(0x4e28_68a4)).unwrap();
    assert_eq!(
        bytes(machine.cores.active().state.v[4]),
        [
            0x8e, 0x4d, 0xa1, 0xbc, 0x9f, 0xdc, 0x58, 0x9d, 0x01, 0x01, 0x01, 0x01, 0xc6, 0xc6,
            0xc6, 0xc6,
        ]
    );

    // AESIMC V6.16B, V4.16B.
    execute(&mut machine, Instruction(0x4e28_7886)).unwrap();
    assert_eq!(
        machine.cores.active().state.v[6],
        machine.cores.active().state.v[5]
    );
}

#[test]
fn aes_opcodes_disassemble_with_architectural_vector_operands() {
    let cases = [
        (0x4e28_4820, "aese\tv0.16b, v1.16b"),
        (0x4e28_5862, "aesd\tv2.16b, v3.16b"),
        (0x4e28_68a4, "aesmc\tv4.16b, v5.16b"),
        (0x4e28_78e6, "aesimc\tv6.16b, v7.16b"),
    ];
    for (word, expected) in cases {
        assert_eq!(disassemble(0, word), expected, "{word:#010x}");
    }
}
