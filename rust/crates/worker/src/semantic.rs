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

#[derive(Clone, Copy)]
struct Register {
    index: u32,
    wide: bool,
    stack_pointer: bool,
}

impl Register {
    fn name(self) -> String {
        register(self.index, self.wide, self.stack_pointer)
    }

    fn mask(self, value: u64) -> u64 {
        if self.wide {
            value
        } else {
            value & 0xffff_ffff
        }
    }
}

#[derive(Clone, Copy)]
enum LogicalOp {
    And,
    Orr,
    Eor,
}

impl LogicalOp {
    fn symbol(self) -> &'static str {
        match self {
            Self::And => "&",
            Self::Orr => "|",
            Self::Eor => "^",
        }
    }

    fn apply(self, left: u64, right: u64) -> u64 {
        match self {
            Self::And => left & right,
            Self::Orr => left | right,
            Self::Eor => left ^ right,
        }
    }
}

/// Decode an AArch64 bitmask immediate to its 64-bit `wmask` (Architecture
/// Reference Manual `DecodeBitMasks` with `immediate = true`). Returns `None` for
/// reserved encodings so they stay opaque.
fn decode_bitmask(n: u32, immr: u32, imms: u32, wide: bool) -> Option<u64> {
    if !wide && n != 0 {
        return None;
    }
    let total = if wide { 64 } else { 32 };
    let selector = (n << 6) | ((!imms) & 0x3f);
    if selector == 0 {
        return None;
    }
    let len = 31 - selector.leading_zeros();
    if len < 1 || (!wide && len > 5) {
        return None;
    }
    let levels = (1u32 << len) - 1;
    let s = imms & levels;
    let r = immr & levels;
    if s == levels {
        return None;
    }
    let esize = 1u32 << len;
    let welem = rotate_right(elem_mask(s + 1), r, esize) & elem_mask(esize);
    let mut mask = 0u64;
    let mut shift = 0u32;
    while shift < total {
        mask |= welem << shift;
        shift += esize;
    }
    Some(mask & if wide { u64::MAX } else { 0xffff_ffff })
}

fn elem_mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn rotate_right(value: u64, amount: u32, width: u32) -> u64 {
    let amount = amount % width;
    if amount == 0 {
        return value;
    }
    ((value >> amount) | (value << (width - amount))) & elem_mask(width)
}

#[derive(Clone, Copy)]
enum Operation {
    Nop,
    Return(Register),
    Call(u64),
    Branch(u64),
    AddImmediate {
        dst: Register,
        src: Register,
        value: u64,
        subtract: bool,
    },
    AddRegister {
        dst: Register,
        left: Register,
        right: Register,
        subtract: bool,
    },
    MoveZero {
        dst: Register,
        value: u64,
    },
    MoveNot {
        dst: Register,
        value: u64,
    },
    MoveKeep {
        dst: Register,
        value: u16,
        shift: u32,
    },
    LogicalImmediate {
        dst: Register,
        src: Register,
        op: LogicalOp,
        mask: u64,
    },
    LogicalRegister {
        dst: Register,
        left: Register,
        right: Register,
        op: LogicalOp,
        negate_right: bool,
    },
    Address {
        dst: Register,
        value: u64,
        page: bool,
    },
    Discard(&'static str),
    Opaque,
}

/// Decode only encodings with modeled operand and effect semantics.
fn decode(word: u32, pc: u64) -> Operation {
    if word == 0xd503_201f {
        return Operation::Nop;
    }
    if word & 0xffff_fc1f == 0xd65f_0000 {
        return Operation::Return(Register {
            index: (word >> 5) & 31,
            wide: true,
            stack_pointer: false,
        });
    }
    if word & 0xfc00_0000 == 0x9400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return Operation::Call(signed_target(pc, imm as i64));
    }
    if word & 0xfc00_0000 == 0x1400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return Operation::Branch(signed_target(pc, imm as i64));
    }
    // PC-relative addressing: ADR (op=0) adds a signed byte offset to PC; ADRP
    // (op=1) adds a signed 4 KiB page offset to the page-aligned PC. Both write a
    // 64-bit address constant carrying the instruction as provenance.
    if word & 0x1f00_0000 == 0x1000_0000 {
        if word & 31 == 31 {
            return Operation::Discard("address discarded in zero register");
        }
        let page = word >> 31 != 0;
        let immlo = (word >> 29) & 3;
        let immhi = (word >> 5) & 0x7ffff;
        let raw = ((immhi << 2) | immlo) as i64;
        let imm = if raw & (1 << 20) != 0 {
            raw - (1 << 21)
        } else {
            raw
        };
        let base = if page { pc & !0xfff } else { pc };
        let offset = if page { imm << 12 } else { imm };
        return Operation::Address {
            dst: Register {
                index: word & 31,
                wide: true,
                stack_pointer: false,
            },
            value: base.wrapping_add_signed(offset),
            page,
        };
    }
    // ADD/SUB (immediate), excluding flag-setting variants and reserved shift.
    if word & 0x1f00_0000 == 0x1100_0000 && word & (1 << 29) == 0 {
        let shift = (word >> 22) & 3;
        if shift <= 1 {
            let wide = word >> 31 != 0;
            let rd = word & 31;
            let rn = (word >> 5) & 31;
            let imm = ((word >> 10) & 0xfff) << if shift == 1 { 12 } else { 0 };
            return Operation::AddImmediate {
                dst: Register {
                    index: rd,
                    wide,
                    stack_pointer: true,
                },
                src: Register {
                    index: rn,
                    wide,
                    stack_pointer: true,
                },
                value: imm as u64,
                subtract: word & (1 << 30) != 0,
            };
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
            return Operation::Discard("arithmetic result discarded in zero register");
        }
        return Operation::AddRegister {
            dst: Register {
                index: rd,
                wide,
                stack_pointer: false,
            },
            left: Register {
                index: rn,
                wide,
                stack_pointer: false,
            },
            right: Register {
                index: rm,
                wide,
                stack_pointer: false,
            },
            subtract: word & (1 << 30) != 0,
        };
    }
    // Logical (immediate): flag-neutral AND/ORR/EOR with a bitmask immediate.
    // ANDS is excluded because flag effects are not modeled, and reserved
    // bitmask encodings stay opaque.
    if word & 0x1f80_0000 == 0x1200_0000 {
        let opc = (word >> 29) & 3;
        if opc != 3 {
            let wide = word >> 31 != 0;
            let n = (word >> 22) & 1;
            let immr = (word >> 16) & 0x3f;
            let imms = (word >> 10) & 0x3f;
            if let Some(mask) = decode_bitmask(n, immr, imms, wide) {
                if word & 31 == 31 {
                    return Operation::Discard("logical result discarded in zero register");
                }
                return Operation::LogicalImmediate {
                    dst: Register {
                        index: word & 31,
                        wide,
                        stack_pointer: false,
                    },
                    src: Register {
                        index: (word >> 5) & 31,
                        wide,
                        stack_pointer: false,
                    },
                    op: match opc {
                        0 => LogicalOp::And,
                        1 => LogicalOp::Orr,
                        _ => LogicalOp::Eor,
                    },
                    mask,
                };
            }
        }
    }
    // Logical (shifted register): flag-neutral AND/ORR/EOR and their negated
    // BIC/ORN/EON forms with an unshifted register operand. Flag-setting
    // variants and shifted operands stay opaque.
    if word & 0x1f00_0000 == 0x0a00_0000 {
        let opc = (word >> 29) & 3;
        let shift = (word >> 22) & 3;
        let imm6 = (word >> 10) & 0x3f;
        if opc != 3 && shift == 0 && imm6 == 0 {
            let wide = word >> 31 != 0;
            if word & 31 == 31 {
                return Operation::Discard("logical result discarded in zero register");
            }
            return Operation::LogicalRegister {
                dst: Register {
                    index: word & 31,
                    wide,
                    stack_pointer: false,
                },
                left: Register {
                    index: (word >> 5) & 31,
                    wide,
                    stack_pointer: false,
                },
                right: Register {
                    index: (word >> 16) & 31,
                    wide,
                    stack_pointer: false,
                },
                op: match opc {
                    0 => LogicalOp::And,
                    1 => LogicalOp::Orr,
                    _ => LogicalOp::Eor,
                },
                negate_right: word & (1 << 21) != 0,
            };
        }
    }
    // Move wide immediate family. MOVN (opc=00), MOVZ (opc=10), and MOVK (opc=11)
    // share `sf opc:2 100101 hw:2 imm16:16 Rd:5`; opc=01 and 32-bit hw>=2 are
    // unallocated. Only exact constant definitions and merges are modeled, and no
    // source-level type is inferred.
    if word & 0x1f80_0000 == 0x1280_0000 {
        let opc = (word >> 29) & 3;
        let wide = word >> 31 != 0;
        let hw = (word >> 21) & 3;
        if opc != 1 && (wide || hw < 2) {
            if word & 31 == 31 {
                return Operation::Discard("constant discarded in zero register");
            }
            let imm = (word >> 5) & 0xffff;
            let shift = hw * 16;
            let dst = Register {
                index: word & 31,
                wide,
                stack_pointer: false,
            };
            return match opc {
                0 => {
                    let raw = (imm as u64) << shift;
                    Operation::MoveNot {
                        dst,
                        value: if wide { !raw } else { (!raw) & 0xffff_ffff },
                    }
                }
                2 => Operation::MoveZero {
                    dst,
                    value: (imm as u64) << shift,
                },
                _ => Operation::MoveKeep {
                    dst,
                    value: imm as u16,
                    shift,
                },
            };
        }
    }
    Operation::Opaque
}

#[derive(Clone)]
struct KnownValue {
    value: u64,
    origins: Vec<u64>,
}

struct Flow {
    registers: [Option<KnownValue>; 31],
}

impl Flow {
    fn new() -> Self {
        Self {
            registers: std::array::from_fn(|_| None),
        }
    }

    fn clear(&mut self) {
        self.registers.fill(None);
    }

    fn read(&self, reg: Register) -> Option<KnownValue> {
        if reg.index == 31 {
            return (!reg.stack_pointer).then_some(KnownValue {
                value: 0,
                origins: Vec::new(),
            });
        }
        self.registers[reg.index as usize]
            .as_ref()
            .map(|v| KnownValue {
                value: reg.mask(v.value),
                origins: v.origins.clone(),
            })
    }

    fn write(&mut self, reg: Register, value: Option<KnownValue>) {
        if reg.index < 31 {
            self.registers[reg.index as usize] = value.map(|mut v| {
                v.value = reg.mask(v.value);
                v
            });
        }
    }
}

fn combine_origins(left: &[u64], right: &[u64], pc: u64) -> Option<Vec<u64>> {
    let mut result = Vec::new();
    for origin in left.iter().chain(right).chain(std::iter::once(&pc)) {
        if !result.contains(origin) {
            result.push(*origin);
        }
        if result.len() > 8 {
            return None;
        }
    }
    Some(result)
}

fn resolved_suffix(value: Option<&KnownValue>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let sources = value
        .origins
        .iter()
        .map(|pc| format!("{pc:#x}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(" /* constant={:#x} proven_by=[{sources}] */", value.value)
}

impl Operation {
    fn render(self, flow: &mut Flow, pc: u64) -> String {
        match self {
            Self::Nop => "/* nop */".into(),
            Self::Return(via) => {
                let candidate = flow.read(Register {
                    index: 0,
                    wide: true,
                    stack_pointer: false,
                });
                let statement = format!(
                    "return /* ABI result register x0{}; return type unknown, via {} */;",
                    resolved_suffix(candidate.as_ref()),
                    via.name()
                );
                flow.clear();
                statement
            }
            Self::Call(target) => {
                flow.clear();
                format!("call({target:#x}); /* signature, arguments, result and side effects unresolved */")
            }
            Self::Branch(target) => {
                flow.clear();
                format!("goto {target:#x};")
            }
            Self::AddImmediate {
                dst,
                src,
                value,
                subtract,
            } => {
                let known = flow.read(src).and_then(|input| {
                    Some(KnownValue {
                        value: dst.mask(if subtract {
                            input.value.wrapping_sub(value)
                        } else {
                            input.value.wrapping_add(value)
                        }),
                        origins: combine_origins(&input.origins, &[], pc)?,
                    })
                });
                flow.write(dst, known.clone());
                format!(
                    "{} = {} {} {value:#x};{}",
                    dst.name(),
                    src.name(),
                    if subtract { "-" } else { "+" },
                    resolved_suffix(known.as_ref())
                )
            }
            Self::AddRegister {
                dst,
                left,
                right,
                subtract,
            } => {
                let known = flow.read(left).zip(flow.read(right)).and_then(|(a, b)| {
                    Some(KnownValue {
                        value: dst.mask(if subtract {
                            a.value.wrapping_sub(b.value)
                        } else {
                            a.value.wrapping_add(b.value)
                        }),
                        origins: combine_origins(&a.origins, &b.origins, pc)?,
                    })
                });
                flow.write(dst, known.clone());
                format!(
                    "{} = {} {} {};{}",
                    dst.name(),
                    left.name(),
                    if subtract { "-" } else { "+" },
                    right.name(),
                    resolved_suffix(known.as_ref())
                )
            }
            Self::MoveZero { dst, value } => {
                let known = KnownValue {
                    value: dst.mask(value),
                    origins: vec![pc],
                };
                flow.write(dst, Some(known));
                format!("{} = {value:#x};", dst.name())
            }
            Self::MoveNot { dst, value } => {
                let known = KnownValue {
                    value: dst.mask(value),
                    origins: vec![pc],
                };
                flow.write(dst, Some(known));
                format!("{} = {value:#x};", dst.name())
            }
            Self::MoveKeep { dst, value, shift } => {
                let field = 0xffffu64 << shift;
                let inserted = (value as u64) << shift;
                // MOVK merges into the previous value of the same register, so a
                // proven constant requires a known prior definition; otherwise the
                // merged value is unknown and only the bitfield insert is rendered.
                let known = flow.read(dst).and_then(|prior| {
                    Some(KnownValue {
                        value: dst.mask((prior.value & !field) | inserted),
                        origins: combine_origins(&prior.origins, &[], pc)?,
                    })
                });
                flow.write(dst, known.clone());
                format!(
                    "{} = ({} & {:#x}) | {:#x};{}",
                    dst.name(),
                    dst.name(),
                    dst.mask(!field),
                    dst.mask(inserted),
                    resolved_suffix(known.as_ref())
                )
            }
            Self::Discard(reason) => format!("/* {reason} */"),
            Self::LogicalImmediate { dst, src, op, mask } => {
                let known = flow.read(src).and_then(|input| {
                    Some(KnownValue {
                        value: dst.mask(op.apply(input.value, mask)),
                        origins: combine_origins(&input.origins, &[], pc)?,
                    })
                });
                flow.write(dst, known.clone());
                format!(
                    "{} = {} {} {:#x};{}",
                    dst.name(),
                    src.name(),
                    op.symbol(),
                    dst.mask(mask),
                    resolved_suffix(known.as_ref())
                )
            }
            Self::LogicalRegister {
                dst,
                left,
                right,
                op,
                negate_right,
            } => {
                let known = flow.read(left).zip(flow.read(right)).and_then(|(a, b)| {
                    let rhs = if negate_right {
                        dst.mask(!b.value)
                    } else {
                        b.value
                    };
                    Some(KnownValue {
                        value: dst.mask(op.apply(a.value, rhs)),
                        origins: combine_origins(&a.origins, &b.origins, pc)?,
                    })
                });
                flow.write(dst, known.clone());
                let rhs = if negate_right {
                    format!("~{}", right.name())
                } else {
                    right.name()
                };
                format!(
                    "{} = {} {} {};{}",
                    dst.name(),
                    left.name(),
                    op.symbol(),
                    rhs,
                    resolved_suffix(known.as_ref())
                )
            }
            Self::Address { dst, value, page } => {
                let value = dst.mask(value);
                flow.write(
                    dst,
                    Some(KnownValue {
                        value,
                        origins: vec![pc],
                    }),
                );
                let label = if page {
                    "pc-relative page"
                } else {
                    "pc-relative"
                };
                format!("{} = {value:#x}; /* {label} */", dst.name())
            }
            Self::Opaque => {
                flow.clear();
                "/* opaque instruction: semantics unavailable; value flow stops here */".into()
            }
        }
    }
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
    let mut out = String::from("AArch64 register semantics v5; machine registers, not recovered source variables. Every statement cites its unslid instruction address and word. Exact straight-line constants carry instruction provenance; unknown effects are barriers.\n");
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
        let mut flow = Flow::new();
        for (i, bytes) in code[low..high].chunks_exact(4).enumerate() {
            let pc = start + (i * 4) as u64;
            let position = file_offset.saturating_add(pc - base);
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            let statement = if data_ranges
                .iter()
                .any(|&(lo, hi, _)| position < hi && position.saturating_add(4) > lo)
            {
                flow.clear();
                "/* data-in-code barrier */".into()
            } else {
                decode(word, pc).render(&mut flow, pc)
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
    use super::{decode, render, Flow};

    fn lift(word: u32, pc: u64) -> String {
        decode(word, pc).render(&mut Flow::new(), pc)
    }

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

    #[test]
    fn constants_propagate_only_through_modeled_straight_line_operations() {
        let words = [0x5280_0020u32, 0x1100_0400, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("w0 = w0 + 0x1; /* constant=0x2 proven_by=[0x1000,0x1004] */"));
        assert!(
            report.contains("ABI result register x0 /* constant=0x2 proven_by=[0x1000,0x1004] */")
        );

        for barrier in [0xffff_ffffu32, 0x9400_0001, 0x1400_0001] {
            let words = [0x5280_0020, barrier, 0xd65f_03c0];
            let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
            let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
            assert!(!report.contains("ABI result register x0 /* constant="));
        }
    }

    #[test]
    fn move_not_defines_a_complemented_constant() {
        // MOVN writes the bitwise complement of the shifted immediate.
        assert_eq!(lift(0x1280_0000, 0x1000), "w0 = 0xffffffff;");
        assert_eq!(lift(0x9280_0000, 0x1000), "x0 = 0xffffffffffffffff;");
        assert_eq!(lift(0x1280_0020, 0x1000), "w0 = 0xfffffffe;");
        // A MOVN into the zero register is discarded, not claimed as an assignment.
        assert!(lift(0x1280_001f, 0x1000).contains("discarded"));
    }

    #[test]
    fn move_keep_merges_into_a_known_constant_with_provenance() {
        // movz x0, #0x1234, lsl #0 ; movk x0, #0x5678, lsl #16 ; ret
        let words = [0xd282_4680u32, 0xf2aa_cf00, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains(
            "0x0000000000001004  x0 = (x0 & 0xffffffff0000ffff) | 0x56780000; /* constant=0x56781234 proven_by=[0x1000,0x1004] */"
        ));
        assert!(report.contains(
            "ABI result register x0 /* constant=0x56781234 proven_by=[0x1000,0x1004] */"
        ));
    }

    #[test]
    fn move_keep_without_a_known_base_is_not_claimed_as_a_constant() {
        assert_eq!(
            lift(0xf280_0020, 0x1000),
            "x0 = (x0 & 0xffffffffffff0000) | 0x1;"
        );
        // The 32-bit form masks the kept field to the register width.
        assert_eq!(lift(0x7280_0020, 0x1000), "w0 = (w0 & 0xffff0000) | 0x1;");
    }

    #[test]
    fn unallocated_move_wide_forms_remain_opaque() {
        // opc=01 is unallocated.
        assert!(lift(0x3280_0000, 0x1000).contains("opaque instruction"));
        // 32-bit MOVZ/MOVK with hw>=2 are unallocated.
        assert!(lift(0x52c0_0000, 0x1000).contains("opaque instruction"));
        assert!(lift(0x72c0_0000, 0x1000).contains("opaque instruction"));
    }

    #[test]
    fn function_boundaries_and_data_barriers_clear_constants() {
        let words = [0x5280_0020u32, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000, 0x1004], &[]);
        assert!(!report.contains("ABI result register x0 /* constant="));

        let words = [0x5280_0020u32, 0xd503_201f, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[(0x204, 0x208, 1)]);
        assert!(!report.contains("ABI result register x0 /* constant="));

        let words = [0x5280_0020u32, 0xd65f_03c0, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert_eq!(
            report
                .matches("ABI result register x0 /* constant=")
                .count(),
            1
        );
    }

    #[test]
    fn lifts_logical_immediate_forms() {
        assert_eq!(lift(0x1200_0000, 0x1000), "w0 = w0 & 0x1;");
        assert_eq!(lift(0x1200_0920, 0x1000), "w0 = w9 & 0x7;");
        assert_eq!(lift(0x9240_0000, 0x1000), "x0 = x0 & 0x1;");
        assert_eq!(lift(0x1201_0000, 0x1000), "w0 = w0 & 0x80000000;");
        assert_eq!(lift(0x5200_1c00, 0x1000), "w0 = w0 ^ 0xff;");
        assert_eq!(lift(0x9240_1c00, 0x1000), "x0 = x0 & 0xff;");
        // ORR against the zero register materializes the bitmask and is proven.
        assert_eq!(
            lift(0x3200_0be0, 0x1000),
            "w0 = wzr | 0x7; /* constant=0x7 proven_by=[0x1000] */"
        );
    }

    #[test]
    fn reserved_logical_immediates_remain_opaque() {
        // selector == 0 (N=0, imms=all ones).
        assert!(lift(0x1200_fc00, 0x1000).contains("opaque instruction"));
        // immediate && (imms & levels) == levels.
        assert!(lift(0x1200_7c00, 0x1000).contains("opaque instruction"));
    }

    #[test]
    fn lifts_logical_register_and_negated_forms() {
        assert_eq!(lift(0x8a01_0000, 0x1000), "x0 = x0 & x1;");
        assert_eq!(lift(0xca02_0020, 0x1000), "x0 = x1 ^ x2;");
        assert_eq!(lift(0x2a01_0000, 0x1000), "w0 = w0 | w1;");
        assert_eq!(lift(0x0a24_0062, 0x1000), "w2 = w3 & ~w4;");
        assert_eq!(lift(0x4a2b_0149, 0x1000), "w9 = w10 ^ ~w11;");
        // MVN is ORN with a zero register source.
        assert_eq!(lift(0xaa21_03e0, 0x1000), "x0 = xzr | ~x1;");
    }

    #[test]
    fn flag_setting_and_shifted_logical_forms_remain_opaque() {
        // ANDS sets flags, which are not modeled.
        assert!(lift(0x6a01_0000, 0x1000).contains("opaque instruction"));
        // A shifted register operand is out of scope.
        assert!(lift(0x8a02_0c20, 0x1000).contains("opaque instruction"));
    }

    #[test]
    fn logical_writes_to_the_zero_register_are_discarded() {
        assert!(lift(0x0a01_001f, 0x1000).contains("discarded"));
        assert!(lift(0x1200_001f, 0x1000).contains("discarded"));
    }

    #[test]
    fn logical_operations_fold_known_constants_including_complements() {
        // movz w0, #0x1234 ; and w0, w0, #0xff ; ret
        let words = [0x5282_4680u32, 0x1200_1c00, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("w0 = w0 & 0xff; /* constant=0x34 proven_by=[0x1000,0x1004] */"));

        // movz w0, #0xff ; movz w1, #0xf ; eor w0, w0, w1 ; ret
        let words = [0x5280_1fe0u32, 0x5280_01e1, 0x4a01_0000, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(
            report.contains("w0 = w0 ^ w1; /* constant=0xf0 proven_by=[0x1000,0x1004,0x1008] */")
        );

        // movz w0, #0xf0 ; movz w1, #0xf ; orn w0, w0, w1 ; ret
        // ORN complements the right operand within the register width.
        let words = [0x5280_1e00u32, 0x5280_01e1, 0x2a21_0000, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report
            .contains("w0 = w0 | ~w1; /* constant=0xfffffff0 proven_by=[0x1000,0x1004,0x1008] */"));
    }

    #[test]
    fn lifts_pc_relative_addresses() {
        assert_eq!(lift(0x1000_0080, 0x1000), "x0 = 0x1010; /* pc-relative */");
        // A negative byte offset wraps below the instruction address.
        assert_eq!(lift(0x10ff_ffe1, 0x1000), "x1 = 0xffc; /* pc-relative */");
        // ADRP adds a signed page offset to the page-aligned PC.
        assert_eq!(
            lift(0xb000_0002, 0x1000),
            "x2 = 0x2000; /* pc-relative page */"
        );
        assert_eq!(
            lift(0xf000_0903, 0x1000),
            "x3 = 0x124000; /* pc-relative page */"
        );
    }

    #[test]
    fn adr_and_adrp_fold_into_pointer_constants_with_provenance() {
        // adrp x0, #0x1000 ; add x0, x0, #0x10 ; ret  (unslid pc = 0x1000)
        let words = [0xb000_0000u32, 0x9100_4000, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("x0 = 0x2000; /* pc-relative page */"));
        assert!(report.contains("x0 = x0 + 0x10; /* constant=0x2010 proven_by=[0x1000,0x1004] */"));
        assert!(report
            .contains("ABI result register x0 /* constant=0x2010 proven_by=[0x1000,0x1004] */"));
    }

    #[test]
    fn address_writes_to_the_zero_register_are_discarded() {
        assert!(lift(0x1000_005f, 0x1000).contains("discarded"));
        assert!(lift(0xb000_001f, 0x1000).contains("discarded"));
    }
}
