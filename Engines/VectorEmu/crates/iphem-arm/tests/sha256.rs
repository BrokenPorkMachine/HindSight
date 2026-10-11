use iphem_arm::{disassemble, execute, Instruction};
use iphem_core::{Machine, MachineConfig};

fn machine() -> Machine {
    Machine::new(MachineConfig {
        trace: false,
        quantum: 1,
    })
}

fn pack(words: [u32; 4]) -> u128 {
    words
        .iter()
        .enumerate()
        .fold(0, |v, (i, word)| v | ((*word as u128) << (i * 32)))
}

fn unpack(value: u128) -> [u32; 4] {
    std::array::from_fn(|i| (value >> (i * 32)) as u32)
}

fn small0(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

fn small1(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

fn reference_hash(x: u128, y: u128, schedule: u128, part1: bool) -> u128 {
    let mut state = [0u32; 8];
    state[..4].copy_from_slice(&unpack(x));
    state[4..].copy_from_slice(&unpack(y));
    for word in unpack(schedule) {
        let a = state[0];
        let b = state[1];
        let c = state[2];
        let d = state[3];
        let e = state[4];
        let f = state[5];
        let g = state[6];
        let h = state[7];
        let choose = (a & f) ^ (!a & g);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t1 = h
            .wrapping_add(e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25))
            .wrapping_add(choose)
            .wrapping_add(word);
        let t2 =
            (a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22)).wrapping_add(majority);
        state[3] = d.wrapping_add(t1);
        state[7] = t1.wrapping_add(t2);
        state.rotate_right(1);
    }
    pack(if part1 {
        state[..4].try_into().unwrap()
    } else {
        state[4..].try_into().unwrap()
    })
}

#[test]
fn sha256h_and_h2_perform_distinct_hash_update_halves() {
    let mut machine = machine();
    let d = pack([0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a]);
    let n = pack([0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19]);
    let schedule = pack([0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5]);
    machine.cores.active_mut().state.v[0] = d;
    machine.cores.active_mut().state.v[1] = n;
    machine.cores.active_mut().state.v[2] = schedule;

    // SHA256H Q0, Q1, V2.4S.
    execute(&mut machine, Instruction(0x5e02_4020)).unwrap();
    assert_eq!(
        machine.cores.active().state.v[0],
        reference_hash(d, n, schedule, true)
    );

    machine.cores.active_mut().state.v[3] = d;
    machine.cores.active_mut().state.v[4] = n;
    machine.cores.active_mut().state.v[5] = schedule;
    // SHA256H2 Q3, Q4, V5.4S swaps the input halves and returns part 2.
    execute(&mut machine, Instruction(0x5e05_5083)).unwrap();
    assert_eq!(
        machine.cores.active().state.v[3],
        reference_hash(n, d, schedule, false)
    );
    assert_ne!(
        machine.cores.active().state.v[0],
        machine.cores.active().state.v[3]
    );
}

#[test]
fn sha256_schedule_updates_follow_word_lane_dependencies() {
    let mut machine = machine();
    let d_words = [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344];
    let n_words = [0xa409_3822, 0x299f_31d0, 0x082e_fa98, 0xec4e_6c89];
    let m_words = [0x4528_21e6, 0x38d0_1377, 0xbe54_66cf, 0x34e9_0c6c];
    machine.cores.active_mut().state.v[6] = pack(d_words);
    machine.cores.active_mut().state.v[7] = pack(n_words);
    machine.cores.active_mut().state.v[8] = pack(d_words);
    machine.cores.active_mut().state.v[9] = pack(n_words);
    machine.cores.active_mut().state.v[10] = pack(m_words);

    // SHA256SU0 V6.4S, V7.4S.
    execute(&mut machine, Instruction(0x5e28_28e6)).unwrap();
    let t = [d_words[1], d_words[2], d_words[3], n_words[0]];
    let expected0 = std::array::from_fn(|i| d_words[i].wrapping_add(small0(t[i])));
    assert_eq!(unpack(machine.cores.active().state.v[6]), expected0);

    // SHA256SU1 V8.4S, V9.4S, V10.4S.
    execute(&mut machine, Instruction(0x5e0a_6128)).unwrap();
    let t0 = [n_words[1], n_words[2], n_words[3], m_words[0]];
    let mut expected1 = [0; 4];
    for i in 0..2 {
        expected1[i] = d_words[i]
            .wrapping_add(t0[i])
            .wrapping_add(small1(m_words[i + 2]));
    }
    for i in 2..4 {
        expected1[i] = d_words[i]
            .wrapping_add(t0[i])
            .wrapping_add(small1(expected1[i - 2]));
    }
    assert_eq!(unpack(machine.cores.active().state.v[8]), expected1);
}

#[test]
fn sha256_instructions_disassemble_with_four_word_vectors() {
    let cases = [
        (0x5e02_4020, "sha256h\tv0.4s, v1.4s, v2.4s"),
        (0x5e05_5083, "sha256h2\tv3.4s, v4.4s, v5.4s"),
        (0x5e28_28e6, "sha256su0\tv6.4s, v7.4s"),
        (0x5e0a_6128, "sha256su1\tv8.4s, v9.4s, v10.4s"),
    ];
    for (word, expected) in cases {
        assert_eq!(disassemble(0, word), expected, "{word:#010x}");
    }
}
