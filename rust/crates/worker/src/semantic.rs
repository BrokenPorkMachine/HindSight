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
enum BranchTest {
    /// Condition-code test named directly from the encoding field; PSTATE flags
    /// are not tracked.
    Code(u8),
    /// Register zero / nonzero test for CBZ / CBNZ.
    Zero { reg: Register, nonzero: bool },
    /// Single-bit test for TBZ / TBNZ.
    Bit {
        reg: Register,
        bit: u8,
        nonzero: bool,
    },
}

fn condition_name(code: u8) -> &'static str {
    const NAMES: [&str; 16] = [
        "eq", "ne", "hs", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le", "al",
        "nv",
    ];
    NAMES[(code & 15) as usize]
}

#[derive(Clone, Copy)]
enum ShiftKind {
    Left,
    LogicalRight,
    ArithmeticRight,
}

impl ShiftKind {
    fn operator(self) -> &'static str {
        match self {
            Self::Left => "<<",
            Self::LogicalRight => ">>",
            // Verilog-style arithmetic right shift (sign-propagating).
            Self::ArithmeticRight => ">>>",
        }
    }
}

fn shift_value(value: u64, kind: ShiftKind, amount: u8, wide: bool) -> u64 {
    if wide {
        match kind {
            ShiftKind::Left => value << amount,
            ShiftKind::LogicalRight => value >> amount,
            ShiftKind::ArithmeticRight => ((value as i64) >> amount) as u64,
        }
    } else {
        let value = value as u32;
        match kind {
            ShiftKind::Left => (value << amount) as u64,
            ShiftKind::LogicalRight => (value >> amount) as u64,
            ShiftKind::ArithmeticRight => (((value as i32) >> amount) as u32) as u64,
        }
    }
}

fn extend_value(value: u64, signed: bool, from: u8, wide: bool) -> u64 {
    let truncated = value & ((1u64 << from) - 1);
    if signed {
        let shift = 64 - from;
        let sign_extended = (((truncated << shift) as i64) >> shift) as u64;
        if wide {
            sign_extended
        } else {
            sign_extended & 0xffff_ffff
        }
    } else {
        truncated
    }
}

/// Applies an optional register-operand shift, leaving `value` unchanged when the
/// operand is unshifted.
fn shifted_value(value: u64, shift: Option<(ShiftKind, u8)>, wide: bool) -> u64 {
    match shift {
        Some((kind, amount)) => shift_value(value, kind, amount, wide),
        None => value,
    }
}

/// Renders a register operand with its optional shift, parenthesized so the
/// shift binds before the enclosing arithmetic or logical operator.
fn render_shifted(name: &str, shift: Option<(ShiftKind, u8)>) -> String {
    match shift {
        Some((kind, amount)) => format!("({name} {} {amount})", kind.operator()),
        None => name.to_string(),
    }
}

/// Decodes the 2-bit shift selector and 6-bit amount of a shifted-register
/// operand. A zero amount is the unshifted operand.
fn operand_shift(selector: u32, amount: u8) -> Option<(ShiftKind, u8)> {
    if amount == 0 {
        return None;
    }
    match selector {
        0 => Some((ShiftKind::Left, amount)),
        1 => Some((ShiftKind::LogicalRight, amount)),
        2 => Some((ShiftKind::ArithmeticRight, amount)),
        _ => None,
    }
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
        /// Immediate `LSL`/`LSR`/`ASR` applied to `right`; `None` is unshifted.
        shift: Option<(ShiftKind, u8)>,
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
        /// Immediate `LSL`/`LSR`/`ASR` applied to `right`; `None` is unshifted.
        shift: Option<(ShiftKind, u8)>,
    },
    Address {
        dst: Register,
        value: u64,
        page: bool,
    },
    ConditionalBranch {
        target: u64,
        test: BranchTest,
    },
    IndirectBranch {
        target: Register,
    },
    IndirectCall {
        target: Register,
    },
    Shift {
        dst: Register,
        src: Register,
        kind: ShiftKind,
        amount: u8,
    },
    MultiplyAdd {
        dst: Register,
        mul_left: Register,
        mul_right: Register,
        /// `None` when the accumulator is the zero register (`MUL` / `MNEG`).
        addend: Option<Register>,
        subtract: bool,
    },
    Extend {
        dst: Register,
        src: Register,
        signed: bool,
        from: u8,
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
    // BR / BLR: indirect transfer through a register. The target value is not
    // resolved here; the register operand is named.
    if word & 0xffff_fc1f == 0xd61f_0000 {
        return Operation::IndirectBranch {
            target: Register {
                index: (word >> 5) & 31,
                wide: true,
                stack_pointer: false,
            },
        };
    }
    if word & 0xffff_fc1f == 0xd63f_0000 {
        return Operation::IndirectCall {
            target: Register {
                index: (word >> 5) & 31,
                wide: true,
                stack_pointer: false,
            },
        };
    }
    if word & 0xfc00_0000 == 0x9400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return Operation::Call(signed_target(pc, imm as i64));
    }
    if word & 0xfc00_0000 == 0x1400_0000 {
        let imm = ((word & 0x03ff_ffff) as i32) << 6 >> 4;
        return Operation::Branch(signed_target(pc, imm as i64));
    }
    // B.cond: a PC-relative signed imm19 branch tested against a condition code.
    // The condition code is named directly; PSTATE flags are not modeled.
    if word & 0xff00_0010 == 0x5400_0000 {
        let imm = (((word >> 5) & 0x7ffff) as i32) << 13 >> 11;
        return Operation::ConditionalBranch {
            target: signed_target(pc, imm as i64),
            test: BranchTest::Code((word & 15) as u8),
        };
    }
    // CBZ / CBNZ: PC-relative signed imm19 branch on a register being zero.
    if word & 0x7e00_0000 == 0x3400_0000 {
        let imm = (((word >> 5) & 0x7ffff) as i32) << 13 >> 11;
        return Operation::ConditionalBranch {
            target: signed_target(pc, imm as i64),
            test: BranchTest::Zero {
                reg: Register {
                    index: word & 31,
                    wide: word >> 31 != 0,
                    stack_pointer: false,
                },
                nonzero: word & (1 << 24) != 0,
            },
        };
    }
    // TBZ / TBNZ: PC-relative signed imm14 branch on a single bit. Bit 31 extends
    // the bit index past 31 and selects the 64-bit register naming.
    if word & 0x7e00_0000 == 0x3600_0000 {
        let high = word >> 31 != 0;
        let imm = (((word >> 5) & 0x3fff) as i32) << 18 >> 16;
        let bit = ((word >> 19) & 0x1f) as u8 + if high { 32 } else { 0 };
        return Operation::ConditionalBranch {
            target: signed_target(pc, imm as i64),
            test: BranchTest::Bit {
                reg: Register {
                    index: word & 31,
                    wide: high,
                    stack_pointer: false,
                },
                bit,
                nonzero: word & (1 << 24) != 0,
            },
        };
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
    // ADD/SUB (shifted register), without flags. The shift is LSL/LSR/ASR with a
    // 6-bit amount; ROR (shift=11) is not encodable here and stays opaque.
    if word & 0x1f20_0000 == 0x0b00_0000 && word & (1 << 29) == 0 {
        let shift = (word >> 22) & 3;
        let amount = ((word >> 10) & 0x3f) as u8;
        if shift != 3 {
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
                shift: operand_shift(shift, amount),
            };
        }
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
    // BIC/ORN/EON forms with an LSL/LSR/ASR-shifted register operand.
    // Flag-setting variants and ROR stay opaque.
    if word & 0x1f00_0000 == 0x0a00_0000 {
        let opc = (word >> 29) & 3;
        let shift = (word >> 22) & 3;
        let amount = ((word >> 10) & 0x3f) as u8;
        if opc != 3 && shift != 3 {
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
                shift: operand_shift(shift, amount),
            };
        }
    }
    // Bitfield group `sf opc:2 100110 N:1 immr:6 imms:6`. Only the canonical
    // single-field shift and extend aliases are modeled. LSR is UBFM with
    // imms == width-1, ASR is SBFM with imms == width-1, and LSL is UBFM with
    // imms == width-1-shift and immr == (width-shift) mod width. The sign/zero
    // extends are immr == 0 with a narrow field: UXTB/UXTH (UBFM) and
    // SXTB/SXTH/SXTW (SBFM). N must equal sf; other bitfield forms (BFM and
    // general extracts) stay opaque.
    if word & 0x1f80_0000 == 0x1300_0000 {
        let opc = (word >> 29) & 3;
        let sf = word >> 31 != 0;
        let n = (word >> 22) & 1;
        let immr = ((word >> 16) & 0x3f) as u8;
        let imms = ((word >> 10) & 0x3f) as u8;
        let width: u8 = if sf { 64 } else { 32 };
        if n == u32::from(sf) {
            let (kind, amount) = if imms == width - 1 {
                match opc {
                    2 => (Some(ShiftKind::LogicalRight), immr),
                    0 => (Some(ShiftKind::ArithmeticRight), immr),
                    _ => (None, immr),
                }
            } else if opc == 2 && immr != 0 {
                let shift = (width - immr) % width;
                if imms == width - 1 - shift {
                    (Some(ShiftKind::Left), shift)
                } else {
                    (None, immr)
                }
            } else {
                (None, immr)
            };
            if let Some(kind) = kind {
                if word & 31 == 31 {
                    return Operation::Discard("shift result discarded in zero register");
                }
                return Operation::Shift {
                    dst: Register {
                        index: word & 31,
                        wide: sf,
                        stack_pointer: false,
                    },
                    src: Register {
                        index: (word >> 5) & 31,
                        wide: sf,
                        stack_pointer: false,
                    },
                    kind,
                    amount,
                };
            }
            let extend = if immr == 0 {
                match (opc, imms) {
                    (2, 7) => Some((false, 8u8)),
                    (2, 15) => Some((false, 16)),
                    (0, 7) => Some((true, 8)),
                    (0, 15) => Some((true, 16)),
                    (0, 31) if sf => Some((true, 32)),
                    _ => None,
                }
            } else {
                None
            };
            if let Some((signed, from)) = extend {
                if word & 31 == 31 {
                    return Operation::Discard("extend result discarded in zero register");
                }
                return Operation::Extend {
                    dst: Register {
                        index: word & 31,
                        wide: sf,
                        stack_pointer: false,
                    },
                    // The extend aliases read the narrow (32-bit) view of the
                    // source; the low `from` bits are identical either way.
                    src: Register {
                        index: (word >> 5) & 31,
                        wide: false,
                        stack_pointer: false,
                    },
                    signed,
                    from,
                };
            }
        }
    }
    // Data-processing (3 source): MADD (o0=0) and MSUB (o0=1) with 32x32
    // operands and an optional addend. When Ra is the zero register the addend is
    // omitted, which is the MUL (o0=0) / MNEG (o0=1) alias. Other op31 variants
    // (SMADDL/UMADDL/...) stay opaque.
    if word & 0x7fe0_0000 == 0x1b00_0000 {
        if word & 31 == 31 {
            return Operation::Discard("arithmetic result discarded in zero register");
        }
        let sf = word >> 31 != 0;
        let ra = (word >> 10) & 31;
        return Operation::MultiplyAdd {
            dst: Register {
                index: word & 31,
                wide: sf,
                stack_pointer: false,
            },
            mul_left: Register {
                index: (word >> 5) & 31,
                wide: sf,
                stack_pointer: false,
            },
            mul_right: Register {
                index: (word >> 16) & 31,
                wide: sf,
                stack_pointer: false,
            },
            addend: (ra != 31).then_some(Register {
                index: ra,
                wide: sf,
                stack_pointer: false,
            }),
            subtract: word & (1 << 15) != 0,
        };
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

fn proven_by(value: &KnownValue) -> String {
    let sources = value
        .origins
        .iter()
        .map(|origin| format!("{origin:#x}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("proven_by=[{sources}]")
}

fn resolved_suffix(value: Option<&KnownValue>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    format!(" /* constant={:#x} {} */", value.value, proven_by(value))
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
                shift,
            } => {
                let known = flow.read(left).zip(flow.read(right)).and_then(|(a, b)| {
                    let b_value = shifted_value(b.value, shift, dst.wide);
                    Some(KnownValue {
                        value: dst.mask(if subtract {
                            a.value.wrapping_sub(b_value)
                        } else {
                            a.value.wrapping_add(b_value)
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
                    render_shifted(&right.name(), shift),
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
                shift,
            } => {
                let known = flow.read(left).zip(flow.read(right)).and_then(|(a, b)| {
                    let b_value = shifted_value(b.value, shift, dst.wide);
                    let rhs = if negate_right {
                        dst.mask(!b_value)
                    } else {
                        b_value
                    };
                    Some(KnownValue {
                        value: dst.mask(op.apply(a.value, rhs)),
                        origins: combine_origins(&a.origins, &b.origins, pc)?,
                    })
                });
                flow.write(dst, known.clone());
                let rhs = render_shifted(&right.name(), shift);
                let rhs = if negate_right { format!("~{rhs}") } else { rhs };
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
            Self::ConditionalBranch { target, test } => {
                let condition = match test {
                    BranchTest::Code(code) => condition_name(code).to_string(),
                    BranchTest::Zero { reg, nonzero } => {
                        format!("{} {} 0", reg.name(), if nonzero { "!=" } else { "==" })
                    }
                    BranchTest::Bit { reg, bit, nonzero } => format!(
                        "({} & {:#x}) {} 0",
                        reg.name(),
                        1u64 << bit,
                        if nonzero { "!=" } else { "==" }
                    ),
                };
                flow.clear();
                format!("if ({condition}) goto {target:#x};")
            }
            Self::IndirectBranch { target } => {
                // A proven constant target is resolved; an untracked register
                // stays named as an unresolved indirect target. Either way the
                // transfer is a barrier.
                let resolved = flow.read(target).filter(|value| !value.origins.is_empty());
                flow.clear();
                match resolved {
                    Some(value) => format!(
                        "goto {:#x}; /* indirect via {}; {} */",
                        value.value,
                        target.name(),
                        proven_by(&value)
                    ),
                    None => format!("goto {}; /* indirect target unresolved */", target.name()),
                }
            }
            Self::IndirectCall { target } => {
                let resolved = flow.read(target).filter(|value| !value.origins.is_empty());
                flow.clear();
                match resolved {
                    Some(value) => format!(
                        "call({:#x}); /* indirect via {}; {}; signature, arguments, result and side effects unresolved */",
                        value.value,
                        target.name(),
                        proven_by(&value)
                    ),
                    None => format!(
                        "call({}); /* indirect; signature, arguments, result and side effects unresolved */",
                        target.name()
                    ),
                }
            }
            Self::Shift {
                dst,
                src,
                kind,
                amount,
            } => {
                let known = flow.read(src).and_then(|input| {
                    Some(KnownValue {
                        value: dst.mask(shift_value(input.value, kind, amount, dst.wide)),
                        origins: combine_origins(&input.origins, &[], pc)?,
                    })
                });
                flow.write(dst, known.clone());
                format!(
                    "{} = {} {} {amount};{}",
                    dst.name(),
                    src.name(),
                    kind.operator(),
                    resolved_suffix(known.as_ref())
                )
            }
            Self::MultiplyAdd {
                dst,
                mul_left,
                mul_right,
                addend,
                subtract,
            } => {
                let known =
                    flow.read(mul_left)
                        .zip(flow.read(mul_right))
                        .and_then(|(left, right)| {
                            let product = left.value.wrapping_mul(right.value);
                            let (addend_value, addend_origins) = match addend {
                                Some(reg) => {
                                    let value = flow.read(reg)?;
                                    (value.value, value.origins)
                                }
                                None => (0, Vec::new()),
                            };
                            let value = if subtract {
                                addend_value.wrapping_sub(product)
                            } else {
                                addend_value.wrapping_add(product)
                            };
                            let mut product_origins = left.origins.clone();
                            product_origins.extend_from_slice(&right.origins);
                            Some(KnownValue {
                                value: dst.mask(value),
                                origins: combine_origins(&product_origins, &addend_origins, pc)?,
                            })
                        });
                flow.write(dst, known.clone());
                let suffix = resolved_suffix(known.as_ref());
                match (addend, subtract) {
                    (Some(reg), false) => format!(
                        "{} = {} * {} + {};{}",
                        dst.name(),
                        mul_left.name(),
                        mul_right.name(),
                        reg.name(),
                        suffix
                    ),
                    (Some(reg), true) => format!(
                        "{} = {} - {} * {};{}",
                        dst.name(),
                        reg.name(),
                        mul_left.name(),
                        mul_right.name(),
                        suffix
                    ),
                    (None, false) => format!(
                        "{} = {} * {};{}",
                        dst.name(),
                        mul_left.name(),
                        mul_right.name(),
                        suffix
                    ),
                    (None, true) => format!(
                        "{} = 0 - {} * {};{}",
                        dst.name(),
                        mul_left.name(),
                        mul_right.name(),
                        suffix
                    ),
                }
            }
            Self::Extend {
                dst,
                src,
                signed,
                from,
            } => {
                let known = flow.read(src).and_then(|input| {
                    Some(KnownValue {
                        value: dst.mask(extend_value(input.value, signed, from, dst.wide)),
                        origins: combine_origins(&input.origins, &[], pc)?,
                    })
                });
                flow.write(dst, known.clone());
                let op = if signed { "sext" } else { "zext" };
                format!(
                    "{} = {op}({}, {from});{}",
                    dst.name(),
                    src.name(),
                    resolved_suffix(known.as_ref())
                )
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
    let mut out = String::from("AArch64 register semantics v11; machine registers, not recovered source variables. Every statement cites its unslid instruction address and word. Exact straight-line constants carry instruction provenance; unknown effects are barriers.\n");
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
    fn flag_setting_and_rotated_logical_forms_remain_opaque() {
        // ANDS sets flags, which are not modeled.
        assert!(lift(0x6a01_0000, 0x1000).contains("opaque instruction"));
        // A ROR-shifted register operand is out of scope.
        assert!(lift(0x0ac2_0c20, 0x1000).contains("opaque instruction"));
        assert!(lift(0x8ac2_1420, 0x1000).contains("opaque instruction"));
    }

    #[test]
    fn lifts_shifted_register_operands() {
        // add w0, w1, w2, lsl #3 = 0x0b020c20
        assert_eq!(lift(0x0b02_0c20, 0x1000), "w0 = w1 + (w2 << 3);");
        // add w0, w1, w2, lsr #3 = 0x0b420c20
        assert_eq!(lift(0x0b42_0c20, 0x1000), "w0 = w1 + (w2 >> 3);");
        // add w0, w1, w2, asr #3 = 0x0b820c20
        assert_eq!(lift(0x0b82_0c20, 0x1000), "w0 = w1 + (w2 >>> 3);");
        // sub w0, w1, w2, lsl #2 = 0x4b020820
        assert_eq!(lift(0x4b02_0820, 0x1000), "w0 = w1 - (w2 << 2);");
        // add x0, x1, x2, lsl #4 = 0x8b021020
        assert_eq!(lift(0x8b02_1020, 0x1000), "x0 = x1 + (x2 << 4);");
        // sub x0, x1, x2, asr #5 = 0xcb821420
        assert_eq!(lift(0xcb82_1420, 0x1000), "x0 = x1 - (x2 >>> 5);");
        // and w0, w1, w2, lsl #3 = 0x0a020c20
        assert_eq!(lift(0x0a02_0c20, 0x1000), "w0 = w1 & (w2 << 3);");
        // orr w0, w1, w2, lsr #3 = 0x2a420c20
        assert_eq!(lift(0x2a42_0c20, 0x1000), "w0 = w1 | (w2 >> 3);");
        // eor w0, w1, w2, asr #3 = 0x4a820c20
        assert_eq!(lift(0x4a82_0c20, 0x1000), "w0 = w1 ^ (w2 >>> 3);");
        // bic w0, w1, w2, lsl #3 = 0x0a220c20
        assert_eq!(lift(0x0a22_0c20, 0x1000), "w0 = w1 & ~(w2 << 3);");
    }

    #[test]
    fn folds_shifted_register_operands_into_constants() {
        let words = [
            0x5280_00a1u32, // movz w1, #5
            0x5280_0062,    // movz w2, #3
            0x0b02_0820,    // add w0, w1, w2, lsl #2 -> 5 + 12 = 0x11
            0x5280_0203,    // movz w3, #0x10
            0x2a43_1064,    // orr w4, w3, w3, lsr #4 -> 0x10 | 0x1 = 0x11
            0x5280_1fe5,    // movz w5, #0xff
            0x0a25_10a6,    // bic w6, w5, w5, lsl #4 -> 0xff & ~0xff0 = 0xf
        ];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report
            .contains("w0 = w1 + (w2 << 2); /* constant=0x11 proven_by=[0x1000,0x1004,0x1008] */"));
        assert!(
            report.contains("w4 = w3 | (w3 >> 4); /* constant=0x11 proven_by=[0x100c,0x1010] */")
        );
        assert!(
            report.contains("w6 = w5 & ~(w5 << 4); /* constant=0xf proven_by=[0x1014,0x1018] */")
        );
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

    #[test]
    fn lifts_conditional_branches() {
        assert_eq!(lift(0x5400_0140, 0x1000), "if (eq) goto 0x1028;");
        assert_eq!(lift(0x54ff_ffe1, 0x1000), "if (ne) goto 0xffc;");
        assert_eq!(lift(0x5400_004e, 0x1000), "if (al) goto 0x1008;");
        assert_eq!(lift(0x54ff_feeb, 0x1000), "if (lt) goto 0xfdc;");
    }

    #[test]
    fn lifts_zero_and_bit_test_branches() {
        assert_eq!(lift(0x3400_0100, 0x1000), "if (w0 == 0) goto 0x1020;");
        assert_eq!(lift(0xb5ff_ffa1, 0x1000), "if (x1 != 0) goto 0xff4;");
        assert_eq!(
            lift(0x3618_00c2, 0x1000),
            "if ((w2 & 0x8) == 0) goto 0x1018;"
        );
        assert_eq!(
            lift(0xb747_ff63, 0x1000),
            "if ((x3 & 0x10000000000) != 0) goto 0xfec;"
        );
    }

    #[test]
    fn lifts_indirect_branches_and_calls() {
        assert_eq!(
            lift(0xd61f_0100, 0x1000),
            "goto x8; /* indirect target unresolved */"
        );
        assert_eq!(
            lift(0xd63f_0120, 0x1000),
            "call(x9); /* indirect; signature, arguments, result and side effects unresolved */"
        );
    }

    #[test]
    fn resolves_indirect_targets_from_proven_constants() {
        // movz x8, #0x30 ; br x8  -> the indirect target is a proven constant.
        let words = [0xd280_0608u32, 0xd61f_0100];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("goto 0x30; /* indirect via x8; proven_by=[0x1000] */"));

        // movz x9, #0x40 ; blr x9  -> the indirect call target is a proven constant.
        let words = [0xd280_0809u32, 0xd63f_0120];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains(
            "call(0x40); /* indirect via x9; proven_by=[0x1000]; signature, arguments, result and side effects unresolved */"
        ));
    }

    #[test]
    fn lifts_immediate_shifts() {
        // lsl w2, w3, #3 (UBFM) = 0x531d7062
        assert_eq!(lift(0x531d_7062, 0x1000), "w2 = w3 << 3;");
        // lsr w8, w9, #3 = 0x53037d28
        assert_eq!(lift(0x5303_7d28, 0x1000), "w8 = w9 >> 3;");
        // asr w14, w15, #3 = 0x13037dee
        assert_eq!(lift(0x1303_7dee, 0x1000), "w14 = w15 >>> 3;");
        // lsl x2, x3, #3 = 0xd37df062
        assert_eq!(lift(0xd37d_f062, 0x1000), "x2 = x3 << 3;");
        // lsr x6, x7, #3 = 0xd343fce6
        assert_eq!(lift(0xd343_fce6, 0x1000), "x6 = x7 >> 3;");
        // asr x8, x9, #3 = 0x9343fd28
        assert_eq!(lift(0x9343_fd28, 0x1000), "x8 = x9 >>> 3;");
        // lsr w0, w1, #0 = 0x53007c20
        assert_eq!(lift(0x5300_7c20, 0x1000), "w0 = w1 >> 0;");
    }

    #[test]
    fn folds_immediate_shifts_into_constants() {
        let words = [
            0x1280_0001u32, // movn w1, #0     -> w1 = 0xffffffff
            0x1304_7c22,    // asr w2, w1, #4  -> 0xffffffff (sign-propagating)
            0x5304_7c23,    // lsr w3, w1, #4  -> 0x0fffffff
            0x529f_ffe4,    // movz w4, #0xffff
            0x530c_2c84,    // lsl w4, w4, #20 -> 0xfff00000 (truncated to 32 bits)
        ];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(
            report.contains("w2 = w1 >>> 4; /* constant=0xffffffff proven_by=[0x1000,0x1004] */")
        );
        assert!(report.contains("w3 = w1 >> 4; /* constant=0xfffffff proven_by=[0x1000,0x1008] */"));
        assert!(
            report.contains("w4 = w4 << 20; /* constant=0xfff00000 proven_by=[0x100c,0x1010] */")
        );
    }

    #[test]
    fn lifts_multiply_add_forms() {
        // mul w0, w1, w2 = MADD with a zero addend = 0x1b027c20
        assert_eq!(lift(0x1b02_7c20, 0x1000), "w0 = w1 * w2;");
        // madd w0, w1, w2, w3 = 0x1b020c20
        assert_eq!(lift(0x1b02_0c20, 0x1000), "w0 = w1 * w2 + w3;");
        // msub w0, w1, w2, w3 = 0x1b028c20
        assert_eq!(lift(0x1b02_8c20, 0x1000), "w0 = w3 - w1 * w2;");
        // mul x4, x5, x6 = 0x9b067ca4
        assert_eq!(lift(0x9b06_7ca4, 0x1000), "x4 = x5 * x6;");
        // madd x4, x5, x6, x7 = 0x9b061ca4
        assert_eq!(lift(0x9b06_1ca4, 0x1000), "x4 = x5 * x6 + x7;");
        // msub x4, x5, x6, x7 = 0x9b069ca4
        assert_eq!(lift(0x9b06_9ca4, 0x1000), "x4 = x7 - x5 * x6;");
        // mneg w8, w9, w10 = MSUB with a zero addend = 0x1b0afd28
        assert_eq!(lift(0x1b0a_fd28, 0x1000), "w8 = 0 - w9 * w10;");
    }

    #[test]
    fn folds_multiply_add_into_constants() {
        let words = [
            0x5280_0061u32, // movz w1, #3
            0x5280_00a2,    // movz w2, #5
            0x1b02_7c23,    // mul w3, w1, w2    -> 0xf
            0x1b02_fc24,    // mneg w4, w1, w2   -> 0xfffffff1
            0x5280_0025,    // movz w5, #1
            0x1b02_1420,    // madd w0, w1, w2, w5 -> 0x10
        ];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(
            report.contains("w3 = w1 * w2; /* constant=0xf proven_by=[0x1000,0x1004,0x1008] */")
        );
        assert!(report.contains(
            "w4 = 0 - w1 * w2; /* constant=0xfffffff1 proven_by=[0x1000,0x1004,0x100c] */"
        ));
        assert!(report.contains(
            "w0 = w1 * w2 + w5; /* constant=0x10 proven_by=[0x1000,0x1004,0x1010,0x1014] */"
        ));
    }

    #[test]
    fn lifts_sign_and_zero_extends() {
        // uxtb w0, w1 = 0x53001c20
        assert_eq!(lift(0x5300_1c20, 0x1000), "w0 = zext(w1, 8);");
        // uxth w4, w5 = 0x53003ca4
        assert_eq!(lift(0x5300_3ca4, 0x1000), "w4 = zext(w5, 16);");
        // sxtb w8, w9 = 0x13001d28
        assert_eq!(lift(0x1300_1d28, 0x1000), "w8 = sext(w9, 8);");
        // sxtb x10, w11 = 0x93401d6a (64-bit destination, still a W source)
        assert_eq!(lift(0x9340_1d6a, 0x1000), "x10 = sext(w11, 8);");
        // sxth w12, w13 = 0x13003dac
        assert_eq!(lift(0x1300_3dac, 0x1000), "w12 = sext(w13, 16);");
        // sxth x14, w15 = 0x93403dee
        assert_eq!(lift(0x9340_3dee, 0x1000), "x14 = sext(w15, 16);");
        // sxtw x16, w17 = 0x93407e30
        assert_eq!(lift(0x9340_7e30, 0x1000), "x16 = sext(w17, 32);");
    }

    #[test]
    fn folds_sign_and_zero_extends_into_constants() {
        let words = [
            0x5280_1001u32, // movz w1, #0x80
            0x1300_1c22,    // sxtb w2, w1  -> 0xffffff80
            0x5300_1c23,    // uxtb w3, w1  -> 0x80
            0x5290_0005,    // movz w5, #0x8000
            0x5310_3ca5,    // lsl w5, w5, #16 -> 0x80000000
            0x9340_7ca6,    // sxtw x6, w5  -> 0xffffffff80000000
        ];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report
            .contains("w2 = sext(w1, 8); /* constant=0xffffff80 proven_by=[0x1000,0x1004] */"));
        assert!(report.contains("w3 = zext(w1, 8); /* constant=0x80 proven_by=[0x1000,0x1008] */"));
        assert!(report.contains(
            "x6 = sext(w5, 32); /* constant=0xffffffff80000000 proven_by=[0x100c,0x1010,0x1014] */"
        ));
    }

    #[test]
    fn conditional_and_indirect_branches_clear_constants() {
        // movz w0, #0x1 ; b.eq 0x102c ; ret  -> the branch clears the constant.
        let words = [0x5280_0020u32, 0x5400_0140, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("if (eq) goto 0x102c;"));
        assert!(!report.contains("ABI result register x0 /* constant="));

        // movz x0, #0x1 ; br x8 ; ret  -> the indirect branch clears the constant.
        let words = [0xd280_0020u32, 0xd61f_0100, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("goto x8; /* indirect target unresolved */"));
        assert!(!report.contains("ABI result register x0 /* constant="));

        // movz x0, #0x1 ; movz x8, #0x8 ; br x8 ; ret  -> a resolved indirect
        // branch is still a barrier that clears the earlier constant.
        let words = [0x5280_0020u32, 0xd280_0108, 0xd61f_0100, 0xd65f_03c0];
        let code: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        let report = render(&code, 0x1000, 0x200, &[0x1000], &[]);
        assert!(report.contains("goto 0x8; /* indirect via x8; proven_by=[0x1004] */"));
        assert!(!report.contains("ABI result register x0 /* constant="));
    }
}
