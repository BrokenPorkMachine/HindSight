//! Bounded, instruction-derived AArch64 register semantics. This is a
//! machine-level preview, not recovered source or a general decompiler.

const MAX_FUNCTIONS: usize = 8;
const MAX_WORDS_PER_FUNCTION: usize = 128;
const MAX_OUTPUT: usize = 48_000;

fn register(index: u32, wide: bool, stack_pointer: bool) -> String {
    if index == 31 {
        return if stack_pointer {
            if wide {
                "sp"
            } else {
                "wsp"
            }
        } else if wide {
            "xzr"
        } else {
            "wzr"
        }
        .into();
    }
    format!("{}{}", if wide { "x" } else { "w" }, index)
}

fn signed_target(pc: u64, displacement: i64) -> u64 {
    pc.wrapping_add_signed(displacement)
}

/// Return a statement only for encodings whose operands and effects are
/// modeled here. Decoded instructions outside this set remain opaque.
fn lift(word: u32, pc: u64) -> String {
    if word == 0xd503_201f {
        return "/* nop */".into();
    }
    if word & 0xffff_fc1f == 0xd65f_0000 {
        return format!(
            "return /* ABI result register x0; return type unknown, via {} */;",
            register((word >> 5) & 31, true, false)
        );
    }
    if word & 0xfc00_0000 == 0x9400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return format!(
            "call({:#x}); /* signature, arguments, result and side effects unresolved */",
            signed_target(pc, imm as i64)
        );
    }
    if word & 0xfc00_0000 == 0x1400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return format!("goto {:#x};", signed_target(pc, imm as i64));
    }
    // ADD/SUB (immediate), excluding flag-setting variants and reserved shift.
    if word & 0x1f00_0000 == 0x1100_0000 && word & (1 << 29) == 0 {
        let shift = (word >> 22) & 3;
        if shift <= 1 {
            let wide = word >> 31 != 0;
            let rd = word & 31;
            let rn = (word >> 5) & 31;
            let imm = ((word >> 10) & 0xfff) << if shift == 1 { 12 } else { 0 };
            return format!(
                "{} = {} {} {:#x};",
                register(rd, wide, true),
                register(rn, wide, true),
                if word & (1 << 30) == 0 { "+" } else { "-" },
                imm
            );
        }
    }
    // ADD/SUB (shifted register), only unshifted LSL #0 and no flags.
    if word & 0x1f20_0000 == 0x0b00_0000
        && word & (1 << 29) == 0
        && word & (3 << 22) == 0
        && word & (0x3f << 10) == 0
    {
        let wide = word >> 31 != 0;
        let rd = word & 31;
        let rn = (word >> 5) & 31;
        let rm = (word >> 16) & 31;
        if rd == 31 {
            return "/* arithmetic result discarded in zero register */".into();
        }
        return format!(
            "{} = {} {} {};",
            register(rd, wide, false),
            register(rn, wide, false),
            if word & (1 << 30) == 0 { "+" } else { "-" },
            register(rm, wide, false)
        );
    }
    // MOVZ: exact constant definition; no source-level type is inferred.
    if word & 0x7f80_0000 == 0x5280_0000 {
        let wide = word >> 31 != 0;
        let hw = (word >> 21) & 3;
        if wide || hw < 2 {
            if word & 31 == 31 {
                return "/* constant discarded in zero register */".into();
            }
            let value = (((word >> 5) & 0xffff) as u64) << (hw * 16);
            return format!("{} = {value:#x};", register(word & 31, wide, false));
        }
    }
    "/* opaque instruction: semantics unavailable; value flow stops here */".into()
}

pub fn render(
    code: &[u8],
    base: u64,
    file_offset: u64,
    starts: &[u64],
    data_ranges: &[(u64, u64, u16)],
) -> String {
    if code.is_empty() || starts.is_empty() {
        return String::new();
    }
    let end = base.saturating_add(code.len() as u64);
    let mut out = String::from("AArch64 register semantics v1; machine registers, not recovered source variables. Every statement cites its unslid instruction address and word. Unknown effects are barriers.\n");
    for (index, &start) in starts.iter().enumerate().take(MAX_FUNCTIONS) {
        let function_end = starts.get(index + 1).copied().unwrap_or(end).min(end);
        if start < base || start >= function_end || (start - base) % 4 != 0 {
            continue;
        }
        out.push_str(&format!(
            "\nfunction {start:#018x}..{function_end:#018x} {{\n"
        ));
        let low = (start - base) as usize;
        let high =
            ((function_end - base) as usize).min(low.saturating_add(MAX_WORDS_PER_FUNCTION * 4));
        for (i, bytes) in code[low..high].chunks_exact(4).enumerate() {
            let pc = start + (i * 4) as u64;
            let position = file_offset.saturating_add(pc - base);
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            let statement = if data_ranges
                .iter()
                .any(|&(lo, hi, _)| position < hi && position.saturating_add(4) > lo)
            {
                "/* data-in-code barrier */".into()
            } else {
                lift(word, pc)
            };
            out.push_str(&format!("  {pc:#018x}  {statement} // word={word:08x}\n"));
            if out.len() >= MAX_OUTPUT {
                break;
            }
        }
        if high < (function_end - base) as usize {
            out.push_str("  /* function preview truncated at instruction budget */\n");
        }
        out.push_str("}\n");
        if out.len() >= MAX_OUTPUT {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{lift, render};

    #[test]
    fn lifts_exact_integer_operations_with_provenance() {
        let words = [0x5280_0020u32, 0x1100_0400, 0x0b01_0000, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("0x0000000000001000  w0 = 0x1; // word=52800020"));
        assert!(report.contains("0x0000000000001004  w0 = w0 + 0x1;"));
        assert!(report.contains("0x0000000000001008  w0 = w0 + w1;"));
        assert!(report.contains("return /* ABI result register x0"));
    }

    #[test]
    fn unsupported_words_and_data_ranges_are_barriers() {
        assert!(lift(0xffff_ffff, 0x1000).contains("opaque instruction"));
        assert!(lift(0x3100_0400, 0x1000).contains("opaque instruction"));
        let code = 0x5280_0020u32.to_le_bytes();
        assert!(
            render(&code, 0x1000, 0x200, &[0x1000], &[(0x200, 0x204, 1)])
                .contains("data-in-code barrier")
        );
    }

    #[test]
    fn branch_target_is_signed_and_bounded() {
        assert_eq!(lift(0x17ff_ffff, 0x1000), "goto 0xffc;");
        assert_eq!(
            lift(0x9400_0001, 0x1000),
            "call(0x1004); /* signature, arguments, result and side effects unresolved */"
        );
    }

    #[test]
    fn zero_register_writes_are_not_claimed_as_assignments() {
        assert!(lift(0x5280_003f, 0x1000).contains("discarded"));
        assert!(lift(0x0b01_001f, 0x1000).contains("discarded"));
        assert_eq!(lift(0x1100_07ff, 0x1000), "wsp = wsp + 0x1;");
    }
}
