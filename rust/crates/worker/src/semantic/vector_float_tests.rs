//! Vector floating-point three-same qualification against Apple Clang.
//!
//! The oracle in this module is the encoding table measured with Apple Clang
//! (`xcrun clang -target arm64-apple-macos14.0`) and read back with
//! `xcrun llvm-objdump -d`. Every row names the instruction Apple assembles
//! for one `(Q, U, op, sz, opcode)` triple. The worker may only claim the
//! lane-wise operation when the measured name is that lane-wise operation;
//! the pairwise and fused forms that share an opcode must stay opaque rather
//! than being rendered under the lane-wise name.
use super::*;

/// Measured instruction name for each `(U, op, opcode)` triple of the
/// single/double three-same group. `None` marks a triple Apple does not
/// define; the assembly probe rejected or disassembled those as unknown.
fn fp_measured(u: bool, op: u8, opcode: u8) -> Option<&'static str> {
    Some(match (u, op, opcode) {
        (false, 0, 0b11010) => "FADD",
        (false, 1, 0b11010) => "FSUB",
        (true, 0, 0b11010) => "FADDP",
        (true, 1, 0b11010) => "FABD",
        (false, 0, 0b11011) => "FMULX",
        (false, 1, 0b11011) => "FAMAX",
        (true, 0, 0b11011) => "FMUL",
        (true, 1, 0b11011) => "FAMIN",
        (false, 0, 0b11111) => "FRECPS",
        (false, 1, 0b11111) => "FRSQRTS",
        (true, 0, 0b11111) => "FDIV",
        (true, 1, 0b11111) => "FSCALE",
        (false, 0, 0b11110) => "FMAX",
        (false, 1, 0b11110) => "FMIN",
        (true, 0, 0b11110) => "FMAXP",
        (true, 1, 0b11110) => "FMINP",
        (false, 0, 0b11000) => "FMAXNM",
        (false, 1, 0b11000) => "FMINNM",
        (true, 0, 0b11000) => "FMAXNMP",
        (true, 1, 0b11000) => "FMINNMP",
        _ => return None,
    })
}

/// Measured instruction name for each `(U, op, opcode)` triple of the
/// half-precision three-same group.
fn half_measured(u: bool, op: u8, opcode: u8) -> Option<&'static str> {
    Some(match (u, op, opcode) {
        (false, 0, 0b00010) => "FADD",
        (false, 1, 0b00010) => "FSUB",
        (true, 0, 0b00010) => "FADDP",
        (true, 1, 0b00010) => "FABD",
        (false, 0, 0b00011) => "FMULX",
        (false, 1, 0b00011) => "FAMAX",
        (true, 0, 0b00011) => "FMUL",
        (true, 1, 0b00011) => "FAMIN",
        (false, 0, 0b00111) => "FRECPS",
        (false, 1, 0b00111) => "FRSQRTS",
        (true, 0, 0b00111) => "FDIV",
        (true, 1, 0b00111) => "FSCALE",
        (false, 0, 0b00110) => "FMAX",
        (false, 1, 0b00110) => "FMIN",
        (true, 0, 0b00110) => "FMAXP",
        (true, 1, 0b00110) => "FMINP",
        (false, 0, 0b00000) => "FMAXNM",
        (false, 1, 0b00000) => "FMINNM",
        (true, 0, 0b00000) => "FMAXNMP",
        (true, 1, 0b00000) => "FMINNMP",
        _ => return None,
    })
}

/// The lane-wise operation Apple assembles, if the measured instruction is a
/// lane-wise operation of the modelled family.
fn lane_wise_name(measured: &str) -> Option<&'static str> {
    match measured {
        "FADD" => Some("fadd"),
        "FSUB" => Some("fsub"),
        "FMUL" => Some("fmul"),
        "FDIV" => Some("fdiv"),
        "FMAX" => Some("fmax"),
        "FMIN" => Some("fmin"),
        "FMAXNM" => Some("fmaxnm"),
        "FMINNM" => Some("fminnm"),
        _ => None,
    }
}

/// `0 Q U 01110 <op> <sz> 1 Rm <opcode> 1 Rn Rd` with bit 21 set, which marks
/// the single/double floating-point three-same group.
fn fp_word(quad: bool, u: bool, op: u8, sz: u8, opcode: u8) -> u32 {
    0x0e00_0000
        | (u32::from(quad) << 30)
        | (u32::from(u) << 29)
        | (u32::from(op) << 23)
        | (u32::from(sz) << 22)
        | 0x0020_0000
        | (2 << 16)
        | (u32::from(opcode) << 11)
        | 0x400
        | (1 << 5)
}

/// The half-precision group: bit 21 = 0 and bit 22 = 1.
fn half_word(quad: bool, u: bool, op: u8, opcode: u8) -> u32 {
    fp_word(quad, u, op, 1, opcode) & !0x0020_0000
}

/// `0 Q U 01110 1 <sz> 1 Rm 1 10110 Rn Rd`: the two-register reciprocal
/// estimate, distinguished from the three-same group by bit 10 = 0.
fn reciprocal_word(quad: bool, u: bool, sz: u8, rm: u8) -> u32 {
    0x0e00_0000
        | (u32::from(quad) << 30)
        | (u32::from(u) << 29)
        | (1 << 23)
        | (u32::from(sz) << 22)
        | 0x0020_0000
        | (u32::from(rm) << 16)
        | (0b11011 << 11)
        | (1 << 5)
}

fn lift(word: u32, pc: u64) -> String {
    decode(word, pc).render(&mut Flow::new(), pc)
}

#[test]
fn every_measured_single_and_double_three_same_form_is_claimed_or_opaque() {
    let mut decoded_lane_wise = 0usize;
    let mut decoded_opaque = 0usize;
    for quad in [false, true] {
        for u in [false, true] {
            for op in [0u8, 1] {
                for sz in [0u8, 1] {
                    for opcode in [0b11010u8, 0b11011, 0b11111, 0b11110, 0b11000] {
                        let word = fp_word(quad, u, op, sz, opcode);
                        let measured = fp_measured(u, op, opcode).expect("measured table covers every opcode");
                        let double = sz == 1;
                        // Double precision has no 64-bit destination view, so
                        // `.1d` is a reserved encoding.
                        if double && !quad {
                            assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x} {measured}");
                            decoded_opaque += 1;
                            continue;
                        }
                        match lane_wise_name(measured) {
                            Some(name) => {
                                let expected_bits = if double { 64 } else { 32 };
                                let lanes = (if quad { 128 } else { 64 }) / expected_bits;
                                let suffix = if double { 'd' } else { 's' };
                                let expected = format!(
                                    "v0.{lanes}{suffix} = {name}(v1.{lanes}{suffix}, v2.{lanes}{suffix}); /* FPCR/FPSR rounding, exceptions, NaNs and signed-zero behavior unresolved */"
                                );
                                assert_eq!(lift(word, 0x1000), expected, "{word:#010x} {measured}");
                                assert!(
                                    matches!(
                                        lift_operation(decode(word, 0x1000), 0x1000),
                                        IrOperation::VectorFloatBinary { element_bits, .. } if element_bits == expected_bits
                                    ),
                                    "{word:#010x}"
                                );
                                decoded_lane_wise += 1;
                            }
                            None => {
                                // Pairwise and fused forms share an opcode with
                                // a lane-wise operation. They must never be
                                // rendered under the lane-wise name.
                                assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x} {measured}");
                                let rendered = lift(word, 0x1000);
                                assert!(!rendered.contains("fadd(") && !rendered.contains("fsub(")
                                    && !rendered.contains("fmul(") && !rendered.contains("fdiv(")
                                    && !rendered.contains("fmax(") && !rendered.contains("fmin("),
                                    "{word:#010x} {measured} rendered {rendered}");
                                decoded_opaque += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    // Every measured triple is covered: 2 Q views x 2 U x 2 op x 2 sz x 5
    // opcodes. The single-precision views are legal in both widths, the
    // double-precision view only in the 128-bit one, and eight of the twenty
    // (U, op, opcode) triples are lane-wise operations.
    assert_eq!(decoded_lane_wise + decoded_opaque, 2 * 2 * 2 * 2 * 5);
    assert_eq!(decoded_lane_wise, 24, "eight lane-wise operations over three legal views");
    assert_eq!(decoded_opaque, 56);
}

#[test]
fn every_measured_half_three_same_form_is_claimed_or_opaque() {
    let mut decoded_lane_wise = 0usize;
    let mut decoded_opaque = 0usize;
    for quad in [false, true] {
        for u in [false, true] {
            for op in [0u8, 1] {
                for opcode in [0b00010u8, 0b00011, 0b00111, 0b00110, 0b00000] {
                    let word = half_word(quad, u, op, opcode);
                    let measured = half_measured(u, op, opcode).expect("measured table covers every opcode");
                    match lane_wise_name(measured) {
                        Some(name) => {
                            let lanes = if quad { 8 } else { 4 };
                            let expected = format!("v0.{lanes}h = {name}(v1.{lanes}h, v2.{lanes}h); /* FPCR/FPSR rounding, exceptions, NaNs and signed-zero behavior unresolved */");
                            assert_eq!(lift(word, 0x1000), expected, "{word:#010x} {measured}");
                            assert!(
                                matches!(
                                    lift_operation(decode(word, 0x1000), 0x1000),
                                    IrOperation::VectorHalfBinary { element_bits: 16, .. }
                                ),
                                "{word:#010x}"
                            );
                            decoded_lane_wise += 1;
                        }
                        None => {
                            assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x} {measured}");
                            decoded_opaque += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(decoded_lane_wise, 16, "eight lane-wise half operations over two views");
    assert_eq!(decoded_opaque, 24);
}

#[test]
fn the_double_precision_lane_width_survives_into_the_ir() {
    for (sz, expected_bits, lanes, suffix) in [(0u8, 32u8, 4usize, 's'), (1, 64, 2, 'd')] {
        let word = fp_word(true, false, 0, sz, 0b11010);
        match lift_operation(decode(word, 0x1000), 0x1000) {
            IrOperation::VectorFloatBinary { element_bits, operator, .. } => {
                assert_eq!(element_bits, expected_bits);
                assert_eq!(operator, IrVectorFloatBinaryOperator::Add);
            }
            other => panic!("unexpected lift {other:?}"),
        }
        assert_eq!(lift(word, 0x1000), format!("v0.{lanes}{suffix} = fadd(v1.{lanes}{suffix}, v2.{lanes}{suffix}); /* FPCR/FPSR rounding, exceptions, NaNs and signed-zero behavior unresolved */"));
    }
}

#[test]
fn the_single_and_double_views_render_distinct_arrangements() {
    // The same instruction name at two precisions must not collapse into one
    // arrangement: `.4s` and `.2d` name different lane counts and widths.
    let single = fp_word(true, false, 0, 0, 0b11010);
    let double = fp_word(true, false, 0, 1, 0b11010);
    assert_ne!(lift(single, 0x1000), lift(double, 0x1000));
    assert!(lift(single, 0x1000).contains("v0.4s"));
    assert!(lift(double, 0x1000).contains("v0.2d"));
    // The 64-bit single-precision view is `.2s`, not `.4s`.
    assert!(lift(fp_word(false, false, 0, 0, 0b11010), 0x1000).contains("v0.2s"));
}

#[test]
fn reserved_double_precision_64_bit_views_are_opaque_barriers() {
    for u in [false, true] {
        for op in [0u8, 1] {
            for opcode in [0b11010u8, 0b11011, 0b11111, 0b11110, 0b11000] {
                let word = fp_word(false, u, op, 1, opcode);
                assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x}");
            }
        }
    }
}

#[test]
fn the_reciprocal_estimate_keeps_its_measured_precision_and_arrangement() {
    // Single precision: Rm = 0b00001 with bit 22 = 0.
    assert_eq!(lift(reciprocal_word(true, false, 2, 0b00001), 0x1000), "v0.4s = frecpe(v1.4s); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(false, false, 2, 0b00001), 0x1000), "v0.2s = frecpe(v1.2s); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(true, true, 2, 0b00001), 0x1000), "v0.4s = frsqrte(v1.4s); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(false, true, 2, 0b00001), 0x1000), "v0.2s = frsqrte(v1.2s); /* FPCR/FPSR estimate and exception behavior unresolved */");
    // Double precision: Rm = 0b00001 with bit 22 = 1.
    assert_eq!(lift(reciprocal_word(true, false, 3, 0b00001), 0x1000), "v0.2d = frecpe(v1.2d); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(true, true, 3, 0b00001), 0x1000), "v0.2d = frsqrte(v1.2d); /* FPCR/FPSR estimate and exception behavior unresolved */");
    // Half precision: Rm = 0b11001 with bit 22 = 1.
    assert_eq!(lift(reciprocal_word(true, false, 3, 0b11001), 0x1000), "v0.8h = frecpe(v1.8h); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(false, false, 3, 0b11001), 0x1000), "v0.4h = frecpe(v1.4h); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(true, true, 3, 0b11001), 0x1000), "v0.8h = frsqrte(v1.8h); /* FPCR/FPSR estimate and exception behavior unresolved */");
    assert_eq!(lift(reciprocal_word(false, true, 3, 0b11001), 0x1000), "v0.4h = frsqrte(v1.4h); /* FPCR/FPSR estimate and exception behavior unresolved */");
}

#[test]
fn the_reciprocal_estimate_rejects_reserved_and_neighbouring_encodings() {
    // The 64-bit double-precision view would be `.1d`, which is reserved.
    for u in [false, true] {
        assert!(matches!(decode(reciprocal_word(false, u, 3, 0b00001), 0x1000), Operation::Opaque));
    }
    // Half precision has no `.2h` form and no other Rm value.
    for quad in [false, true] {
        for u in [false, true] {
            for sz in [0u8, 1] {
                for rm in [0u8, 2, 24, 26, 31] {
                    let word = reciprocal_word(quad, u, sz, rm);
                    assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x}");
                }
            }
        }
    }
    // Half precision uses Rm = 0b11001; every other Rm in the same size field
    // is a different instruction or reserved.
    for quad in [false, true] {
        for u in [false, true] {
            for rm in [0u8, 2, 3, 24, 26, 30, 31] {
                let word = reciprocal_word(quad, u, 3, rm);
                assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x}");
            }
        }
    }
    // Single precision uses Rm = 0b00001; the half magic value in the single
    // size field is not a reciprocal estimate.
    for quad in [false, true] {
        for u in [false, true] {
            let word = reciprocal_word(quad, u, 2, 0b11001);
            assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x}");
        }
    }
}

#[test]
fn the_neighbouring_convert_instructions_are_not_reciprocal_estimates() {
    // `scvtf v0.2d, v1` (0x4e61d820) and `ucvtf v0.2d, v1` (0x6e61d820) share
    // the fixed bits of the reciprocal estimate group but select a different
    // instruction with bit 23 = 0.
    for word in [0x4e61_d820u32, 0x6e61_d820, 0x4e21_d820, 0x6e21_d820] {
        assert!(matches!(decode(word, 0x1000), Operation::Opaque), "{word:#010x}");
    }
}

#[test]
fn the_floating_point_group_does_not_capture_the_integer_or_half_groups() {
    // The integer three-same group keeps bit 21 = 0 and its own opcodes.
    for word in [0x4e22_8420u32, 0x4e22_9c20, 0x0e42_1420, 0x4e42_1420] {
        let decoded = decode(word, 0x1000);
        assert!(
            !matches!(decoded, Operation::VectorFloatBinary { .. }),
            "{word:#010x} decoded as a single/double floating-point operation"
        );
    }
}
