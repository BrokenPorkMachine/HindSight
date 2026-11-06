//! C166/C167 (Infineon/Siemens 16-bit CISC) bounded decoder for HindSight.
//!
//! Encoding facts follow the Infineon C166 Family Instruction Set Manual V2.0 (2001);
//! the instruction-form table is ported from the Apache-2.0 reference implementation
//! `therudywolf/MS4X tools/c166.py` (credit: rudywolf, github.com/therudywolf/MS4X),
//! which was byte-verified against stock C167 images. SPDX-License-Identifier: Apache-2.0.
//!
//! Boundaries: instructions are 2 or 4 bytes; 20 first-byte opcodes are undefined and
//! reserved fields must be zero — anything else decodes as unknown (a flow barrier),
//! never as a guess. Indirect transfers (JMPI/CALLI) resolve to no target. Data
//! addresses are DPP-mapped 16-bit values, not VAs, so no data references are produced.
//! Extended-register (EXTR/EXTPR/EXTSR) context from preceding instructions is not
//! threaded through linear decode: SFR operands are named from the standard 0xFE00/0xFF00
//! maps, so extended-alias operands print as their word addresses. This is a display
//! simplification only; instruction lengths and control-flow extraction are unaffected.
//!
//! Coverage: every defined first-byte opcode 0x00–0xFF (ALU word/byte families, BFLD,
//! bit-to-bit ops, multiply/divide/shifts, conditional JMPR, BCLR/BSET, CMPD/CMPI,
//! MOV/MOVB direct/indirect/indexed/immediate forms, MOVBZ/MOVBS, SCXT, NEG/CPL,
//! ATOMIC/EXTR, EXTP/EXTS/EXTPR/EXTSR with constants or registers, PCALL, TRAP,
//! CALLI/JMPI, CALLR, PUSH/POP/RETP, the watchdog/idle/system fixed patterns, and the
//! JMPA/CALLA/CALLS/JMPS/JB/JNB/JBC/JNBS transfer family).

const CC: [&str; 16] = [
    "cc_UC", "cc_NET", "cc_EQ", "cc_NE", "cc_V", "cc_NV", "cc_N", "cc_NN", "cc_ULT", "cc_UGE",
    "cc_SGT", "cc_SLE", "cc_SLT", "cc_SGE", "cc_UGT", "cc_ULE",
];

/// SFR (0xFE00–0xFFFF) and ESFR (0xF000–0xF1FF) word addresses of the C167CR, from
/// table 22-4 of the C167CR Derivatives User's Manual V3.1. Sorted for binary search.
const SFR: [(u16, &str); 209] = [
    (0xF030, "PT0"), (0xF032, "PT1"), (0xF034, "PT2"), (0xF036, "PT3"), (0xF038, "PP0"),
    (0xF03A, "PP1"), (0xF03C, "PP2"), (0xF03E, "PP3"), (0xF050, "T7"), (0xF052, "T8"),
    (0xF054, "T7REL"), (0xF056, "T8REL"), (0xF0A0, "ADDAT2"), (0xF0AA, "PDCR"),
    (0xF0B0, "SSCTB"), (0xF0B2, "SSCRB"), (0xF0B4, "SSCBR"), (0xF100, "DP0L"),
    (0xF102, "DP0H"), (0xF104, "DP1L"), (0xF106, "DP1H"), (0xF108, "RP0H"),
    (0xF160, "CC16IC"), (0xF162, "CC17IC"), (0xF164, "CC18IC"), (0xF166, "CC19IC"),
    (0xF168, "CC20IC"), (0xF16A, "CC21IC"), (0xF16C, "CC22IC"), (0xF16E, "CC23IC"),
    (0xF170, "CC24IC"), (0xF172, "CC25IC"), (0xF174, "CC26IC"), (0xF176, "CC27IC"),
    (0xF178, "CC28IC"), (0xF17A, "T7IC"), (0xF17C, "T8IC"), (0xF17E, "PWMIC"),
    (0xF184, "CC29IC"), (0xF186, "XP0IC"), (0xF18C, "CC30IC"), (0xF18E, "XP1IC"),
    (0xF194, "CC31IC"), (0xF196, "XP2IC"), (0xF19C, "S0TBIC"), (0xF19E, "XP3IC"),
    (0xF1C0, "EXICON"), (0xF1C2, "ODP2"), (0xF1C4, "PICON"), (0xF1C6, "ODP3"),
    (0xF1CE, "ODP6"), (0xF1D2, "ODP7"), (0xF1D6, "ODP8"), (0xFE00, "DPP0"), (0xFE02, "DPP1"),
    (0xFE04, "DPP2"), (0xFE06, "DPP3"), (0xFE08, "CSP"), (0xFE0C, "MDH"), (0xFE0E, "MDL"),
    (0xFE10, "CP"), (0xFE12, "SP"), (0xFE14, "STKOV"), (0xFE16, "STKUN"),
    (0xFE18, "ADDRSEL1"), (0xFE1A, "ADDRSEL2"), (0xFE1C, "ADDRSEL3"), (0xFE1E, "ADDRSEL4"),
    (0xFE30, "PW0"), (0xFE32, "PW1"), (0xFE34, "PW2"), (0xFE36, "PW3"), (0xFE40, "T2"),
    (0xFE42, "T3"), (0xFE44, "T4"), (0xFE46, "T5"), (0xFE48, "T6"), (0xFE4A, "CAPREL"),
    (0xFE50, "T0"), (0xFE52, "T1"), (0xFE54, "T0REL"), (0xFE56, "T1REL"), (0xFE60, "CC16"),
    (0xFE62, "CC17"), (0xFE64, "CC18"), (0xFE66, "CC19"), (0xFE68, "CC20"), (0xFE6A, "CC21"),
    (0xFE6C, "CC22"), (0xFE6E, "CC23"), (0xFE70, "CC24"), (0xFE72, "CC25"), (0xFE74, "CC26"),
    (0xFE76, "CC27"), (0xFE78, "CC28"), (0xFE7A, "CC29"), (0xFE7C, "CC30"), (0xFE7E, "CC31"),
    (0xFE80, "CC0"), (0xFE82, "CC1"), (0xFE84, "CC2"), (0xFE86, "CC3"), (0xFE88, "CC4"),
    (0xFE8A, "CC5"), (0xFE8C, "CC6"), (0xFE8E, "CC7"), (0xFE90, "CC8"), (0xFE92, "CC9"),
    (0xFE94, "CC10"), (0xFE96, "CC11"), (0xFE98, "CC12"), (0xFE9A, "CC13"), (0xFE9C, "CC14"),
    (0xFE9E, "CC15"), (0xFEA0, "ADDAT"), (0xFEAE, "WDT"), (0xFEB0, "S0TBUF"),
    (0xFEB2, "S0RBUF"), (0xFEB4, "S0BG"), (0xFEC0, "PECC0"), (0xFEC2, "PECC1"),
    (0xFEC4, "PECC2"), (0xFEC6, "PECC3"), (0xFEC8, "PECC4"), (0xFECA, "PECC5"),
    (0xFECC, "PECC6"), (0xFECE, "PECC7"), (0xFF00, "P0L"), (0xFF02, "P0H"), (0xFF04, "P1L"),
    (0xFF06, "P1H"), (0xFF0C, "BUSCON0"), (0xFF0E, "MDC"), (0xFF10, "PSW"),
    (0xFF12, "SYSCON"), (0xFF14, "BUSCON1"), (0xFF16, "BUSCON2"), (0xFF18, "BUSCON3"),
    (0xFF1A, "BUSCON4"), (0xFF1C, "ZEROS"), (0xFF1E, "ONES"), (0xFF20, "T78CON"),
    (0xFF22, "CCM4"), (0xFF24, "CCM5"), (0xFF26, "CCM6"), (0xFF28, "CCM7"),
    (0xFF30, "PWMCON0"), (0xFF32, "PWMCON1"), (0xFF40, "T2CON"), (0xFF42, "T3CON"),
    (0xFF44, "T4CON"), (0xFF46, "T5CON"), (0xFF48, "T6CON"), (0xFF50, "T01CON"),
    (0xFF52, "CCM0"), (0xFF54, "CCM1"), (0xFF56, "CCM2"), (0xFF58, "CCM3"), (0xFF60, "T2IC"),
    (0xFF62, "T3IC"), (0xFF64, "T4IC"), (0xFF66, "T5IC"), (0xFF68, "T6IC"), (0xFF6A, "CRIC"),
    (0xFF6C, "S0TIC"), (0xFF6E, "S0RIC"), (0xFF70, "S0EIC"), (0xFF72, "SSCTIC"),
    (0xFF74, "SSCRIC"), (0xFF76, "SSCEIC"), (0xFF78, "CC0IC"), (0xFF7A, "CC1IC"),
    (0xFF7C, "CC2IC"), (0xFF7E, "CC3IC"), (0xFF80, "CC4IC"), (0xFF82, "CC5IC"),
    (0xFF84, "CC6IC"), (0xFF86, "CC7IC"), (0xFF88, "CC8IC"), (0xFF8A, "CC9IC"),
    (0xFF8C, "CC10IC"), (0xFF8E, "CC11IC"), (0xFF90, "CC12IC"), (0xFF92, "CC13IC"),
    (0xFF94, "CC14IC"), (0xFF96, "CC15IC"), (0xFF98, "ADCIC"), (0xFF9A, "ADEIC"),
    (0xFF9C, "T0IC"), (0xFF9E, "T1IC"), (0xFFA0, "ADCON"), (0xFFA2, "P5"),
    (0xFFA4, "P5DIDIS"), (0xFFAC, "TFR"), (0xFFAE, "WDTCON"), (0xFFB0, "S0CON"),
    (0xFFB2, "SSCCON"), (0xFFC0, "P2"), (0xFFC2, "DP2"), (0xFFC4, "P3"), (0xFFC6, "DP3"),
    (0xFFC8, "P4"), (0xFFCA, "DP4"), (0xFFCC, "P6"), (0xFFCE, "DP6"), (0xFFD0, "P7"),
    (0xFFD2, "DP7"), (0xFFD4, "P8"), (0xFFD6, "DP8"),
];

/// The 20 first-byte opcodes with no defined form on the C167.
const UNDEFINED: [u8; 20] = [
    0x3B, 0x44, 0x45, 0x83, 0x85, 0x8B, 0x8C, 0x93, 0x95, 0xA3, 0xB3, 0xC1, 0xC3, 0xC7, 0xD3,
    0xE3, 0xE5, 0xF5, 0xF8, 0xF9,
];

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

/// IP-relative displacement wraps inside its 64 KB segment.
fn near(pc: u64, size: u64, rel: i8) -> u64 {
    let target = (pc as i64 + size as i64 + 2 * rel as i64) as u64;
    (pc & !0xFFFF) | (target & 0xFFFF)
}

fn absolute(pc: u64, caddr: u16) -> u64 {
    (pc & !0xFFFF) | caddr as u64
}

fn far(pc: u64, seg: u8, caddr: u16) -> u64 {
    (pc & !0x00FF_FFFF) | (((seg as u64) << 16) | caddr as u64)
}

fn word_name(r: u8) -> String {
    format!("r{}", r & 0x0F)
}

fn byte_name(r: u8) -> String {
    let n = (r & 0x0F) / 2;
    if r & 1 == 0 {
        format!("rl{n}")
    } else {
        format!("rh{n}")
    }
}

fn sfr_name(addr: u16) -> Option<&'static str> {
    SFR
        .binary_search_by_key(&addr, |entry| entry.0)
        .ok()
        .map(|index| SFR[index].1)
}

/// Direct word register or SFR operand (`reg` in the reference): values 0xF0–0xFF name
/// general-purpose registers, values below address the SFR space at 0xFE00.
fn reg_name(r: u8, byte: bool) -> String {
    if r >= 0xF0 {
        if byte {
            byte_name(r)
        } else {
            word_name(r)
        }
    } else {
        let addr = 0xFE00 + 2 * u16::from(r);
        sfr_name(addr)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("0x{addr:04x}"))
    }
}

/// Word address of a bit-addressable word (`bitoff`); general-purpose registers name
/// themselves.
fn bitword_name(q: u8) -> String {
    if q >= 0xF0 {
        return word_name(q);
    }
    let addr = if q >= 0x80 {
        0xFF00 + 2 * u16::from(q - 0x80)
    } else {
        0xFD00 + 2 * u16::from(q)
    };
    sfr_name(addr)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("0x{addr:04x}"))
}

fn bit_name(q: u8, bit: u8) -> String {
    format!("{}.{}", bitword_name(q), bit)
}

fn fmt_imm8(v: u16) -> String {
    if v <= 0xFF {
        format!("#0x{v:02x}")
    } else {
        format!("#0x{v:04x}")
    }
}

fn gpr(n: u8, byte: bool) -> String {
    if byte {
        byte_name(n)
    } else {
        word_name(n)
    }
}

/// The ALU families of rows 0–7: ADD/ADDC/SUB/SUBC/CMP/XOR/AND/OR, word and byte.
const ALU: [&str; 8] = ["add", "addc", "sub", "subc", "cmp", "xor", "and", "or"];

/// Decode one instruction: `Some((length, text))` or `None` when the bytes are not an
/// instruction (undefined opcode, reserved field set, or truncated buffer).
pub fn instruction(b: &[u8], pc: u64) -> Option<(usize, String)> {
    if b.len() < 2 {
        return None;
    }
    let op = b[0];
    if UNDEFINED.contains(&op) {
        return None;
    }
    let hi = op >> 4;
    let lo = op & 0x0F;
    // Every form in these columns is 4 bytes; refuse truncated buffers outright.
    if matches!(lo, 0x02..=0x07 | 0x0A) && b.len() < 4 {
        return None;
    }
    // Fixed system-control patterns first: their encodings are exact byte sequences.
    for (raw, text) in [
        (&[0xA5u8, 0x5A, 0xA5, 0xA5][..], "diswdt"),
        (&[0xB5, 0x4A, 0xB5, 0xB5][..], "einit"),
        (&[0x87, 0x78, 0x87, 0x87][..], "idle"),
        (&[0x97, 0x68, 0x97, 0x97][..], "pwrdn"),
        (&[0xA7, 0x58, 0xA7, 0xA7][..], "srvwdt"),
        (&[0xB7, 0x48, 0xB7, 0xB7][..], "srst"),
    ] {
        if b.len() >= 4 && b[..4] == *raw {
            return Some((4, text.to_string()));
        }
    }
    if op == 0xCB && b[1] == 0x00 {
        return Some((2, "ret".into()));
    }
    if op == 0xDB && b[1] == 0x00 {
        return Some((2, "rets".into()));
    }
    if op == 0xFB && b[1] == 0x88 {
        return Some((2, "reti".into()));
    }
    if op == 0xCC && b[1] == 0x00 {
        return Some((2, "nop".into()));
    }
    if hi < 8 {
        return decode_row_low(op, hi, lo, b, pc);
    }
    decode_row_high(op, hi, lo, b, pc)
}

/// Rows 0x00–0x7F: ALU families, BFLD, bit-to-bit, multiply/divide/shifts, JMPR,
/// BCLR/BSET, and the low MOV forms.
fn decode_row_low(op: u8, hi: u8, lo: u8, b: &[u8], pc: u64) -> Option<(usize, String)> {
    let byte = lo & 1 == 1;
    let name: String = if byte {
        format!("{}b", ALU[hi as usize])
    } else {
        ALU[hi as usize].to_string()
    };
    match lo {
        0 | 1 => Some((
            2,
            format!("{name} {}, {}", gpr(b[1] >> 4, byte), gpr(b[1] & 0x0F, byte)),
        )),
        2 | 3 => Some((
            4,
            format!(
                "{name} {}, [0x{:04x}]",
                gpr(b[1] >> 4, byte),
                le16(b, 2)
            ),
        )),
        4 | 5 if hi != 4 => Some((
            4,
            format!(
                "{name} [0x{:04x}], {}",
                le16(b, 2),
                gpr(b[1] >> 4, byte)
            ),
        )),
        6 => Some((4, format!("{name} {}, #0x{:04x}", word_name(b[1] & 0x0F), le16(b, 2)))),
        7 => Some((4, format!("{name} {}, {}", reg_name(b[1], true), fmt_imm8(le16(b, 2))))),
        8 | 9 => {
            let n = b[1] >> 4;
            let s = b[1] & 0x0F;
            let operand = if s < 8 {
                format!("#{s}")
            } else if s < 12 {
                format!("[r{}]", s & 3)
            } else {
                format!("[r{}+]", s & 3)
            };
            let destination = if byte { format!("rl{n}") } else { word_name(n) };
            Some((2, format!("{name} {destination}, {operand}")))
        }
        0xA => match op {
            0x0A => Some((
                4,
                format!(
                    "bfldl {}, #0x{:02x}, #0x{:02x}",
                    bitword_name(b[1]),
                    b[2],
                    b[3]
                ),
            )),
            0x1A => Some((
                4,
                format!(
                    "bfldh {}, #0x{:02x}, #0x{:02x}",
                    bitword_name(b[1]),
                    b[3],
                    b[2]
                ),
            )),
            _ => {
                let mn = match op {
                    0x2A => "bcmp",
                    0x3A => "bmovn",
                    0x4A => "bmov",
                    0x5A => "bor",
                    0x6A => "band",
                    _ => "bxor",
                };
                Some((
                    4,
                    format!(
                        "{mn} {}, {}",
                        bit_name(b[2], b[3] & 0x0F),
                        bit_name(b[1], b[3] >> 4)
                    ),
                ))
            }
        },
        0xB => match hi {
            0 => Some((2, format!("mul {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)))),
            1 => Some((2, format!("mulu {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)))),
            2 => Some((2, format!("prior {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)))),
            4 | 5 | 6 | 7 => {
                if b[1] >> 4 != b[1] & 0x0F {
                    return None;
                }
                let mn = match hi {
                    4 => "div",
                    5 => "divu",
                    6 => "divl",
                    _ => "divlu",
                };
                Some((2, format!("{mn} {}", word_name(b[1] & 0x0F))))
            }
            _ => None,
        },
        0xC => {
            let (mn, register_form) = match hi {
                0 => ("rol", true),
                1 => ("rol", false),
                2 => ("ror", true),
                3 => ("ror", false),
                4 => ("shl", true),
                5 => ("shl", false),
                6 => ("shr", true),
                _ => ("shr", false),
            };
            if register_form {
                Some((
                    2,
                    format!("{mn} {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)),
                ))
            } else {
                Some((2, format!("{mn} {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4)))
            }
        }
        0xD => {
            let target = near(pc, 2, b[1] as i8);
            Some((2, format!("jmpr {}, 0x{target:x}", CC[hi as usize])))
        }
        0xE => Some((2, format!("bclr {}", bit_name(b[1], hi)))),
        0xF => Some((2, format!("bset {}", bit_name(b[1], hi)))),
        _ => None,
    }
}

/// Rows 0x80–0xFF: CMPD/CMPI, MOV families, EXT sequences, bit jumps and the transfer
/// family (shared with rows 0–7 for JMPR/BCLR/BSET through the caller).
fn decode_row_high(op: u8, hi: u8, lo: u8, b: &[u8], pc: u64) -> Option<(usize, String)> {
    match lo {
        0 => match op {
            0x80 => Some((2, format!("cmpi1 {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4))),
            0x90 => Some((2, format!("cmpi2 {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4))),
            0xA0 => Some((2, format!("cmpd1 {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4))),
            0xB0 => Some((2, format!("cmpd2 {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4))),
            0xC0 => Some((2, format!("movbz {}, {}", word_name(b[1] & 0x0F), byte_name(b[1] >> 4)))),
            0xD0 => Some((2, format!("movbs {}, {}", word_name(b[1] & 0x0F), byte_name(b[1] >> 4)))),
            0xE0 => Some((2, format!("mov {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4))),
            0xE1 => Some((2, format!("movb rl{}, #{}", b[1] & 0x0F, b[1] >> 4))),
            0xF0 => Some((
                2,
                format!("mov {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)),
            )),
            0xF1 => Some((
                2,
                format!("movb {}, {}", byte_name(b[1] >> 4), byte_name(b[1] & 0x0F)),
            )),
            _ => None,
        },
        1 => match op {
            0xE1 => Some((2, format!("movb rl{}, #{}", b[1] & 0x0F, b[1] >> 4))),
            0xF1 => Some((2, format!("movb rl{}, rl{}", b[1] >> 4, b[1] & 0x0F))),
            0x81 | 0x91 | 0xA1 | 0xB1 if b[1] & 0x0F == 0 => {
                let mn = match op {
                    0x81 => "neg",
                    0x91 => "cpl",
                    0xA1 => "negb",
                    _ => "cplb",
                };
                let register = if op == 0xA1 || op == 0xB1 {
                    format!("rl{}", b[1] >> 4)
                } else {
                    word_name(b[1] >> 4)
                };
                Some((2, format!("{mn} {register}")))
            }
            0xD1 => {
                let sub = b[1] >> 6;
                if b[1] & 0x0F != 0 || (sub != 0 && sub != 2) {
                    return None;
                }
                let n = ((b[1] >> 4) & 3) + 1;
                if sub == 0 {
                    Some((2, format!("atomic #{n}")))
                } else {
                    Some((2, format!("extr #{n}")))
                }
            }
            _ => None,
        },
        2 => match op {
            0x82 | 0x92 | 0xA2 | 0xB2 if b[1] >> 4 == 0xF => {
                let mn = match op {
                    0x82 => "cmpi1",
                    0x92 => "cmpi2",
                    0xA2 => "cmpd1",
                    _ => "cmpd2",
                };
                Some((
                    4,
                    format!("{mn} {}, [0x{:04x}]", word_name(b[1] & 0x0F), le16(b, 2)),
                ))
            }
            0xC2 | 0xD2 | 0xF2 | 0xF3 => {
                let byte = op == 0xF3;
                let mn = if op == 0xC2 {
                    "movbz"
                } else if op == 0xD2 {
                    "movbs"
                } else if op == 0xF2 {
                    "mov"
                } else {
                    "movb"
                };
                Some((
                    4,
                    format!(
                        "{mn} {}, [0x{:04x}]",
                        reg_name(b[1], byte),
                        le16(b, 2)
                    ),
                ))
            }
            0xE2 => Some((
                4,
                format!("pcall {}, 0x{:04x}", reg_name(b[1], false), le16(b, 2)),
            )),
            _ => None,
        },
        4 => match op {
            0x84 | 0xA4 if b[1] >> 4 == 0 => {
                let mn = if op == 0x84 { "mov" } else { "movb" };
                Some((
                    4,
                    format!("{mn} [r{}], [0x{:04x}]", b[1] & 0x0F, le16(b, 2)),
                ))
            }
            0x94 | 0xB4 if b[1] >> 4 == 0 => {
                let mn = if op == 0x94 { "mov" } else { "movb" };
                Some((
                    4,
                    format!("{mn} [0x{:04x}], [r{}]", le16(b, 2), b[1] & 0x0F),
                ))
            }
            0xC4 | 0xD4 | 0xE4 | 0xF4 => {
                let byte = op >= 0xE4;
                let mn = if byte { "movb" } else { "mov" };
                let data = if byte {
                    format!("rl{}", b[1] >> 4)
                } else {
                    word_name(b[1] >> 4)
                };
                let indexed = format!("[r{}+#0x{:04x}]", b[1] & 0x0F, le16(b, 2));
                if op == 0xC4 || op == 0xE4 {
                    Some((4, format!("{mn} {indexed}, {data}")))
                } else {
                    Some((4, format!("{mn} {data}, {indexed}")))
                }
            }
            _ => None,
        },
        5 => match op {
            0xC5 | 0xD5 => {
                let byte = true;
                let mn = match op {
                    0xC5 => "movbz",
                    0xD5 => "movbs",
                    _ => "movb",
                };
                Some((
                    4,
                    format!(
                        "{mn} [0x{:04x}], {}",
                        le16(b, 2),
                        reg_name(b[1], byte)
                    ),
                ))
            }
            _ => None,
        },
        6 => match op {
            0x86 | 0x96 | 0xA6 | 0xB6 if b[1] >> 4 == 0xF => {
                let mn = match op {
                    0x86 => "cmpi1",
                    0x96 => "cmpi2",
                    0xA6 => "cmpd1",
                    _ => "cmpd2",
                };
                Some((
                    4,
                    format!("{mn} {}, #0x{:04x}", word_name(b[1] & 0x0F), le16(b, 2)),
                ))
            }
            0xC6 => Some((
                4,
                format!("scxt {}, #0x{:04x}", reg_name(b[1], false), le16(b, 2)),
            )),
            0xD6 => Some((
                4,
                format!("scxt {}, [0x{:04x}]", reg_name(b[1], false), le16(b, 2)),
            )),
            0xE6 => Some((4, format!("mov {}, #0x{:04x}", reg_name(b[1], false), le16(b, 2)))),
            0xF6 => Some((4, format!("mov [0x{:04x}], {}", le16(b, 2), reg_name(b[1], false)))),
            _ => None,
        },
        7 => {
            if op == 0xE7 {
                Some((4, format!("movb {}, #0x{:02x}", reg_name(b[1], true), b[2])))
            } else if op == 0xF7 {
                Some((4, format!("movb [0x{:04x}], {}", le16(b, 2), reg_name(b[1], true))))
            } else if op == 0xD7 {
                let sub = b[1] >> 6;
                if b[1] & 0x0F != 0 {
                    return None;
                }
                let n = ((b[1] >> 4) & 3) + 1;
                let mn = ["exts", "extp", "extsr", "extpr"][sub as usize];
                if sub & 1 == 1 {
                    if b[3] & 0xFC != 0 {
                        return None;
                    }
                    let page = u16::from(b[2]) | (u16::from(b[3] & 3) << 8);
                    Some((4, format!("{mn} #0x{page:03x}, #{n}")))
                } else {
                    if b[3] != 0 {
                        return None;
                    }
                    Some((4, format!("{mn} #0x{:02x}, #{n}", b[2])))
                }
            } else {
                None
            }
        }
        8 | 9 => {
            // Indirect move encodings 0x88–0xF8: (destination kind, source kind).
            let byte = lo == 9;
            let op = op & !1;
            let hi_n = b[1] >> 4;
            let lo_n = b[1] & 0x0F;
            let pointer = |kind: u8, n: u8| -> String {
                match kind {
                    b'i' => format!("[r{n}]"),
                    b'+' => format!("[r{n}+]"),
                    _ => format!("[-r{n}]"),
                }
            };
            let (first, second) = match op {
                0x88 => (b'd', b'r'),
                0x98 => (b'r', b'+'),
                0xA8 => (b'r', b'i'),
                0xB8 => (b'i', b'r'),
                0xC8 => (b'i', b'i'),
                0xD8 => (b'+', b'i'),
                _ => (b'i', b'+'),
            };
            // Register operands read the high nibble; with two pointer operands the
            // first reads the high nibble and the second the low one.
            let (first_n, second_n) = if first == b'r' || second != b'r' {
                (hi_n, lo_n)
            } else {
                (lo_n, hi_n)
            };
            let left = if first == b'r' {
                gpr(hi_n, byte)
            } else {
                pointer(first, first_n)
            };
            let right = if second == b'r' {
                gpr(hi_n, byte)
            } else {
                pointer(second, second_n)
            };
            let mn = if byte { "movb" } else { "mov" };
            Some((2, format!("{mn} {left}, {right}")))
        }
        0xA => match op {
            0x8A | 0x9A | 0xAA | 0xBA if b[3] & 0x0F == 0 => {
                let mn = match op {
                    0x8A => "jb",
                    0x9A => "jnb",
                    0xAA => "jbc",
                    _ => "jnbs",
                };
                let target = near(pc, 4, b[2] as i8);
                Some((
                    4,
                    format!(
                        "{mn} {}, 0x{target:x}",
                        bit_name(b[1], b[3] >> 4)
                    ),
                ))
            }
            0xCA | 0xEA if b[1] & 0x0F == 0 => {
                let mn = if op == 0xCA { "calla" } else { "jmpa" };
                Some((
                    4,
                    format!(
                        "{mn} {}, 0x{:04x}",
                        CC[(b[1] >> 4) as usize],
                        le16(b, 2)
                    ),
                ))
            }
            0xDA | 0xFA => {
                let mn = if op == 0xDA { "calls" } else { "jmps" };
                Some((4, format!("{mn} 0x{:02x}:0x{:04x}", b[1], le16(b, 2))))
            }
            _ => None,
        },
        0xB => match op {
            0x9B if b[1] & 1 == 0 => Some((2, format!("trap #{}", b[1] >> 1))),
            0xAB => {
                Some((
                    2,
                    format!("calli {}, [r{}]", CC[(b[1] >> 4) as usize], b[1] & 0x0F),
                ))
            }
            0xBB => {
                let target = near(pc, 2, b[1] as i8);
                Some((2, format!("callr 0x{target:x}")))
            }
            0xEB => Some((2, format!("retp {}", reg_name(b[1], false)))),
            _ => None,
        },
        0xC => {
            // 0xAC/0xBC ashr share the column: register/register and register/#shift.
            if op == 0xAC {
                return Some((
                    2,
                    format!("ashr {}, {}", word_name(b[1] >> 4), word_name(b[1] & 0x0F)),
                ));
            }
            if op == 0xBC {
                return Some((2, format!("ashr {}, #{}", word_name(b[1] & 0x0F), b[1] >> 4)));
            }
            match op {
                0x9C => Some((
                    2,
                    format!("jmpi {}, [r{}]", CC[(b[1] >> 4) as usize], b[1] & 0x0F),
                )),
                0xDC => {
                    let sub = b[1] >> 6;
                    let n = ((b[1] >> 4) & 3) + 1;
                    let mn = ["exts", "extp", "extsr", "extpr"][sub as usize];
                    Some((2, format!("{mn} {}, #{n}", word_name(b[1] & 0x0F))))
                }
                0xEC | 0xFC => Some((
                    2,
                    format!(
                        "{} {}",
                        if op == 0xEC { "push" } else { "pop" },
                        reg_name(b[1], false)
                    ),
                )),
                _ => None,
            }
        }
        0xD => {
            let target = near(pc, 2, b[1] as i8);
            Some((2, format!("jmpr {}, 0x{target:x}", CC[hi as usize])))
        }
        0xE => Some((2, format!("bclr {}", bit_name(b[1], hi)))),
        0xF => Some((2, format!("bset {}", bit_name(b[1], hi)))),
        _ => None,
    }
}

/// Direct-transfer extraction for `branch()`: `(target, kind)` where kind is one of
/// `"call"`, `"branch"`, `"conditional"`. Indirect transfers return `None`.
pub fn flow(b: &[u8], pc: u64) -> Option<(u64, &'static str)> {
    let op = *b.first()?;
    match op {
        0xEA if b.len() >= 4 && b[1] & 0x0F == 0 => {
            let caddr = le16(b, 2);
            if b[1] >> 4 == 0 {
                Some((absolute(pc, caddr), "branch"))
            } else {
                Some((absolute(pc, caddr), "conditional"))
            }
        }
        0xCA if b.len() >= 4 && b[1] & 0x0F == 0 => {
            Some((absolute(pc, le16(b, 2)), "call"))
        }
        0xDA if b.len() >= 4 => Some((far(pc, b[1], le16(b, 2)), "call")),
        0xFA if b.len() >= 4 => Some((far(pc, b[1], le16(b, 2)), "branch")),
        0xE2 if b.len() >= 4 => Some((absolute(pc, le16(b, 2)), "call")),
        0xBB if b.len() >= 2 => Some((near(pc, 2, b[1] as i8), "call")),
        op if op & 0x0F == 0x0D && b.len() >= 2 => {
            let target = near(pc, 2, b[1] as i8);
            if op >> 4 == 0 {
                Some((target, "branch"))
            } else {
                Some((target, "conditional"))
            }
        }
        0x8A | 0x9A | 0xAA | 0xBA if b.len() >= 4 && b[3] & 0x0F == 0 => {
            Some((near(pc, 4, b[2] as i8), "conditional"))
        }
        _ => None,
    }
}

/// Return-family detection (`RET`, `RETS`, `RETP reg`, `RETI`).
pub fn is_return(_b: &[u8], text: &str) -> bool {
    text == "ret" || text == "rets" || text == "reti" || text.starts_with("retp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_control_flow_matches_reference_semantics() {
        // ME7 reset vector: jmpa cc_UC, 0x0200.
        assert_eq!(
            instruction(&[0xEA, 0x00, 0x00, 0x02], 0),
            Some((4, "jmpa cc_UC, 0x0200".into()))
        );
        assert_eq!(flow(&[0xEA, 0x00, 0x00, 0x02], 0), Some((0x200, "branch")));
        // Conditional JMPR with negative displacement, in-segment wrap.
        assert_eq!(flow(&[0xBD, 0xFE], 0x1234), Some((0x1232, "conditional")));
        // Unconditional JMPR (cc_UC = opcode 0x0D).
        assert_eq!(flow(&[0x0D, 0x02], 0x1000), Some((0x1006, "branch")));
        // CALLA and CALLS on a flat dump.
        assert_eq!(flow(&[0xCA, 0x00, 0x44, 0x80], 0x10000), Some((0x18044, "call")));
        assert_eq!(flow(&[0xDA, 0x0C, 0xF8, 0x9C], 0x1234), Some((0xC9CF8, "call")));
        // Truncated buffers and non-transfer bytes.
        assert_eq!(instruction(&[0xEA, 0x00], 0), None);
        assert_eq!(flow(&[0x00, 0x00], 0), None);
        // Reserved low nibble set on JMPA is not an instruction.
        assert_eq!(instruction(&[0xEA, 0x01, 0x00, 0x02], 0), None);
    }

    #[test]
    fn undefined_opcodes_are_barriers() {
        for op in UNDEFINED {
            assert_eq!(instruction(&[op, 0x00, 0x00, 0x00], 0), None, "op {op:#04x}");
            assert_eq!(flow(&[op, 0x00], 0), None, "op {op:#04x}");
        }
    }

    #[test]
    fn alu_families_decode() {
        assert_eq!(instruction(&[0x00, 0x01], 0), Some((2, "add r0, r1".into())));
        assert_eq!(instruction(&[0x01, 0x23], 0), Some((2, "addb rl1, rh1".into())));
        assert_eq!(
            instruction(&[0x02, 0x45, 0x34, 0x12], 0),
            Some((4, "add r4, [0x1234]".into()))
        );
        assert_eq!(
            instruction(&[0x04, 0x45, 0x00, 0xFE], 0),
            Some((4, "add [0xfe00], r4".into()))
        );
        assert_eq!(
            instruction(&[0x06, 0xF1, 0x34, 0x12], 0),
            Some((4, "add r1, #0x1234".into()))
        );
        assert_eq!(instruction(&[0x08, 0x37], 0), Some((2, "add r3, #7".into())));
        assert_eq!(instruction(&[0x08, 0x3A], 0), Some((2, "add r3, [r2]".into())));
        assert_eq!(instruction(&[0x08, 0x3C], 0), Some((2, "add r3, [r0+]".into())));
        assert_eq!(instruction(&[0x09, 0x37], 0), Some((2, "addb rl3, #7".into())));
        // CMP has no mem, reg form: 0x44/0x45 are undefined.
        assert_eq!(instruction(&[0x44, 0x00], 0), None);
    }

    #[test]
    fn bit_and_field_families_decode() {
        assert_eq!(
            instruction(&[0x0A, 0x88, 0x0F, 0x03], 0),
            Some((4, "bfldl PSW, #0x0f, #0x03".into()))
        );
        assert_eq!(
            instruction(&[0x1A, 0x88, 0x0F, 0x03], 0),
            Some((4, "bfldh PSW, #0x03, #0x0f".into()))
        );
        assert_eq!(
            instruction(&[0x6A, 0xFE, 0x88, 0x41], 0),
            Some((4, "band PSW.1, r14.4".into()))
        );
        assert_eq!(instruction(&[0x0E, 0xFE], 0), Some((2, "bclr r14.0".into())));
        assert_eq!(instruction(&[0x2E, 0x88], 0), Some((2, "bclr PSW.2".into())));
        assert_eq!(instruction(&[0x0F, 0x88], 0), Some((2, "bset PSW.0".into())));
        assert_eq!(instruction(&[0x8E, 0xFF], 0), Some((2, "bclr r15.8".into())));
    }

    #[test]
    fn multiply_divide_shift_families_decode() {
        assert_eq!(instruction(&[0x0B, 0x12], 0), Some((2, "mul r1, r2".into())));
        assert_eq!(instruction(&[0x1B, 0x12], 0), Some((2, "mulu r1, r2".into())));
        assert_eq!(instruction(&[0x2B, 0x12], 0), Some((2, "prior r1, r2".into())));
        assert_eq!(instruction(&[0x4B, 0x33], 0), Some((2, "div r3".into())));
        assert_eq!(instruction(&[0x4B, 0x31], 0), None);
        assert_eq!(instruction(&[0x7B, 0x55], 0), Some((2, "divlu r5".into())));
        assert_eq!(instruction(&[0x0C, 0x12], 0), Some((2, "rol r1, r2".into())));
        assert_eq!(instruction(&[0x1C, 0x82], 0), Some((2, "rol r2, #8".into())));
        assert_eq!(instruction(&[0xBC, 0x23], 0), Some((2, "ashr r3, #2".into())));
    }

    #[test]
    fn move_families_decode() {
        assert_eq!(instruction(&[0x80, 0xF3], 0), Some((2, "cmpi1 r3, #15".into())));
        assert_eq!(
            instruction(&[0xA2, 0xF5, 0x00, 0xFD], 0),
            Some((4, "cmpd1 r5, [0xfd00]".into()))
        );
        assert_eq!(
            instruction(&[0xB6, 0xF2, 0x34, 0x12], 0),
            Some((4, "cmpd2 r2, #0x1234".into()))
        );
        assert_eq!(instruction(&[0xC0, 0x43], 0), Some((2, "movbz r3, rl2".into())));
        assert_eq!(instruction(&[0xD0, 0x43], 0), Some((2, "movbs r3, rl2".into())));
        assert_eq!(
            instruction(&[0xC2, 0x18, 0x00, 0xFD], 0),
            Some((4, "movbz PW0, [0xfd00]".into()))
        );
        assert_eq!(instruction(&[0xE0, 0x54], 0), Some((2, "mov r4, #5".into())));
        assert_eq!(instruction(&[0xE1, 0x54], 0), Some((2, "movb rl4, #5".into())));
        assert_eq!(instruction(&[0xF0, 0x12], 0), Some((2, "mov r1, r2".into())));
        assert_eq!(instruction(&[0xF1, 0x12], 0), Some((2, "movb rl1, rl2".into())));
        assert_eq!(instruction(&[0x81, 0x80], 0), Some((2, "neg r8".into())));
        assert_eq!(instruction(&[0xA1, 0x30], 0), Some((2, "negb rl3".into())));
        assert_eq!(instruction(&[0x81, 0x81], 0), None);
        // Direct/indirect/indexed MOV forms.
        assert_eq!(
            instruction(&[0x84, 0x02, 0x00, 0xFE], 0),
            Some((4, "mov [r2], [0xfe00]".into()))
        );
        assert_eq!(
            instruction(&[0x94, 0x02, 0x00, 0xFE], 0),
            Some((4, "mov [0xfe00], [r2]".into()))
        );
        assert_eq!(
            instruction(&[0xC4, 0x54, 0x00, 0x12], 0),
            Some((4, "mov [r4+#0x1200], r5".into()))
        );
        assert_eq!(
            instruction(&[0xD4, 0x54, 0x00, 0x12], 0),
            Some((4, "mov r5, [r4+#0x1200]".into()))
        );
        assert_eq!(
            instruction(&[0xF4, 0x54, 0x00, 0x12], 0),
            Some((4, "movb rl5, [r4+#0x1200]".into()))
        );
        assert_eq!(
            instruction(&[0xC5, 0x18, 0x00, 0xFD], 0),
            Some((4, "movbz [0xfd00], PW0".into()))
        );
        assert_eq!(
            instruction(&[0xF6, 0xF4, 0x34, 0x12], 0),
            Some((4, "mov [0x1234], r4".into()))
        );
        assert_eq!(
            instruction(&[0xF7, 0xF4, 0x34, 0x12], 0),
            Some((4, "movb [0x1234], rl2".into()))
        );
        assert_eq!(
            instruction(&[0xE6, 0xF1, 0x34, 0x12], 0),
            Some((4, "mov r1, #0x1234".into()))
        );
        assert_eq!(
            instruction(&[0xE7, 0x45, 0x34, 0x12], 0),
            Some((4, "movb CC5, #0x34".into()))
        );
        assert_eq!(
            instruction(&[0xE6, 0x45, 0x34, 0x12], 0),
            Some((4, "mov CC5, #0x1234".into()))
        );
        assert_eq!(
            instruction(&[0xC6, 0x45, 0x34, 0x12], 0),
            Some((4, "scxt CC5, #0x1234".into()))
        );
        assert_eq!(
            instruction(&[0xD6, 0x45, 0x34, 0x12], 0),
            Some((4, "scxt CC5, [0x1234]".into()))
        );
    }

    #[test]
    fn indirect_move_encodings_decode() {
        assert_eq!(instruction(&[0x88, 0x54], 0), Some((2, "mov [-r4], r5".into())));
        assert_eq!(instruction(&[0x89, 0x54], 0), Some((2, "movb [-r4], rh2".into())));
        assert_eq!(instruction(&[0x98, 0x54], 0), Some((2, "mov r5, [r4+]".into())));
        assert_eq!(instruction(&[0xA8, 0x54], 0), Some((2, "mov r5, [r4]".into())));
        assert_eq!(instruction(&[0xB8, 0x54], 0), Some((2, "mov [r4], r5".into())));
        assert_eq!(instruction(&[0xC8, 0x54], 0), Some((2, "mov [r5], [r4]".into())));
        assert_eq!(instruction(&[0xD8, 0x54], 0), Some((2, "mov [r5+], [r4]".into())));
        assert_eq!(instruction(&[0xE8, 0x54], 0), Some((2, "mov [r5], [r4+]".into())));
    }

    #[test]
    fn extension_and_system_sequences_decode() {
        assert_eq!(instruction(&[0xD1, 0x00], 0), Some((2, "atomic #1".into())));
        assert_eq!(instruction(&[0xD1, 0x80], 0), Some((2, "extr #1".into())));
        assert_eq!(instruction(&[0xD1, 0x81], 0), None);
        assert_eq!(
            instruction(&[0xD7, 0x40, 0x23, 0x01], 0),
            Some((4, "extp #0x123, #1".into()))
        );
        assert_eq!(
            instruction(&[0xD7, 0x00, 0x12, 0x00], 0),
            Some((4, "exts #0x12, #1".into()))
        );
        assert_eq!(instruction(&[0xD7, 0x41, 0x23, 0x01], 0), None);
        assert_eq!(instruction(&[0xDC, 0x41], 0), Some((2, "extp r1, #1".into())));
        assert_eq!(
            instruction(&[0xA5, 0x5A, 0xA5, 0xA5], 0),
            Some((4, "diswdt".into()))
        );
        assert_eq!(
            instruction(&[0xB5, 0x4A, 0xB5, 0xB5], 0),
            Some((4, "einit".into()))
        );
        assert_eq!(
            instruction(&[0x87, 0x78, 0x87, 0x87], 0),
            Some((4, "idle".into()))
        );
        assert_eq!(
            instruction(&[0x97, 0x68, 0x97, 0x97], 0),
            Some((4, "pwrdn".into()))
        );
        assert_eq!(
            instruction(&[0xA7, 0x58, 0xA7, 0xA7], 0),
            Some((4, "srvwdt".into()))
        );
        assert_eq!(
            instruction(&[0xB7, 0x48, 0xB7, 0xB7], 0),
            Some((4, "srst".into()))
        );
        assert_eq!(instruction(&[0xA5, 0x00, 0x00, 0x00], 0), None);
    }

    #[test]
    fn stack_and_indirect_transfers_decode() {
        assert_eq!(instruction(&[0xE2, 0x45, 0x00, 0x08], 0), Some((4, "pcall CC5, 0x0800".into())));
        assert_eq!(flow(&[0xE2, 0x45, 0x00, 0x08], 0x10000), Some((0x10800, "call")));
        assert_eq!(instruction(&[0x9B, 0x2A], 0), Some((2, "trap #21".into())));
        assert_eq!(instruction(&[0x9B, 0x2B], 0), None);
        assert_eq!(instruction(&[0xAB, 0x91], 0), Some((2, "calli cc_UGE, [r1]".into())));
        assert_eq!(flow(&[0xAB, 0x91], 0), None);
        assert_eq!(instruction(&[0x9C, 0x02], 0), Some((2, "jmpi cc_UC, [r2]".into())));
        assert_eq!(flow(&[0x9C, 0x02], 0), None);
        assert_eq!(instruction(&[0xBB, 0x02], 0x1000), Some((2, "callr 0x1006".into())));
        assert_eq!(flow(&[0xBB, 0xFE], 0x1000), Some((0x0FFE, "call")));
        assert_eq!(instruction(&[0xEB, 0xF1], 0), Some((2, "retp r1".into())));
        assert_eq!(instruction(&[0xEC, 0xF0], 0), Some((2, "push r0".into())));
        assert_eq!(instruction(&[0xFC, 0xF0], 0), Some((2, "pop r0".into())));
        assert!(is_return(&[0xCB, 0x00], "ret"));
        assert!(is_return(&[0xDB, 0x00], "rets"));
        assert!(is_return(&[0xFB, 0x88], "reti"));
        assert!(is_return(&[0xEB, 0xF1], "retp r1"));
        assert!(!is_return(&[0xEC, 0xF0], "push r0"));
    }

    #[test]
    fn bit_jump_families_decode() {
        assert_eq!(
            instruction(&[0x8A, 0x88, 0x10, 0x20], 0x1000),
            Some((4, "jb PSW.2, 0x1024".into()))
        );
        assert_eq!(flow(&[0x8A, 0x88, 0x10, 0x20], 0x1000), Some((0x1024, "conditional")));
        assert_eq!(flow(&[0x9A, 0x88, 0xFE, 0x20], 0x1000), Some((0x1000, "conditional")));
        assert_eq!(flow(&[0xAA, 0x88, 0x02, 0x20], 0x1000), Some((0x1008, "conditional")));
        assert_eq!(flow(&[0xBA, 0x88, 0x02, 0x20], 0x1000), Some((0x1008, "conditional")));
        // Reserved low nibble of the fourth byte is a barrier.
        assert_eq!(instruction(&[0x8A, 0x88, 0x10, 0x21], 0), None);
    }
}
