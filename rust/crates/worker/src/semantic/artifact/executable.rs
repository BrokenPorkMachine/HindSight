//! Execute and compile the structured machine tree; never touch host memory.
use super::*;
const BASE: u64 = 0x100000;
const SIZE: usize = 4096;
pub(super) const CONTRACT: &str = "aarch64-gpr-nzcv-flat4096-le-v1";

fn num(t: &Tree, key: &str) -> u64 {
    match t.attributes.get(key) {
        Some(V::Int(v)) => *v as u64,
        Some(V::String(v)) => u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap(),
        _ => panic!("missing internal AST attribute {key}"),
    }
}
fn text<'a>(t: &'a Tree, key: &str) -> &'a str {
    match t.attributes.get(key) {
        Some(V::String(v)) => v,
        _ => "",
    }
}
fn flag(t: &Tree, key: &str) -> bool {
    t.attributes.get(key) == Some(&V::Bool(true))
}
fn mask(bits: u64) -> u64 {
    if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}
fn literal(v: u64) -> String {
    format!("UINT64_C(0x{v:x})")
}
fn reg(t: &Tree) -> Result<(usize, u64), String> {
    if t.kind != "register" {
        return Err("not a GPR".into());
    }
    let i = match text(t, "class") {
        "general" => text(t, "name")[1..].parse().map_err(|_| "invalid GPR")?,
        "stack-pointer" => 31,
        "zero" => 32,
        _ => return Err("non-GPR register".into()),
    };
    if i > 32 {
        return Err("invalid GPR".into());
    }
    Ok((i, num(t, "bits")))
}
fn condition(t: &Tree) -> Result<u8, String> {
    Ok(match text(t, "predicate") {
        "eq" => 0,
        "ne" => 1,
        "hs" | "cs" => 2,
        "lo" | "cc" => 3,
        "mi" => 4,
        "pl" => 5,
        "vs" => 6,
        "vc" => 7,
        "hi" => 8,
        "ls" => 9,
        "ge" => 10,
        "lt" => 11,
        "gt" => 12,
        "le" => 13,
        "al" => 14,
        // Shared IR currently maps NV to Never; that is not qualified here.
        _ => return Err("unqualified NZCV predicate".into()),
    })
}
fn compare(t: &Tree) -> Result<u8, String> {
    Ok(match text(t, "operator") {
        "cmp" => 0,
        "cmn" => 1,
        "tst" => 2,
        _ => return Err("unqualified compare".into()),
    })
}
fn extend(v: u64, bits: u64, signed: bool) -> u64 {
    let v = v & mask(bits);
    if signed && v & (1u64 << (bits - 1)) != 0 {
        v | !mask(bits)
    } else {
        v
    }
}
fn shift(v: u64, n: u64, bits: u64, kind: &str) -> u64 {
    let v = v & mask(bits);
    let n = n & (bits - 1);
    if n == 0 {
        return v;
    }
    (match kind {
        "<<" => v << n,
        ">>" => v >> n,
        "ror" => (v >> n) | (v << (bits - n)),
        "asr" => {
            (v >> n)
                | if v >> (bits - 1) != 0 {
                    mask(bits) << (bits - n)
                } else {
                    0
                }
        }
        _ => unreachable!(),
    }) & mask(bits)
}
fn expr(t: &Tree) -> Result<String, String> {
    let child = |i| expr(&t.children[i]);
    Ok(match t.kind {
        "register" => {
            let (i, b) = reg(t)?;
            format!(
                "(({}) & {})",
                match i {
                    0..=30 => format!("s->x[{i}]"),
                    31 => "s->sp".into(),
                    _ => "UINT64_C(0)".into(),
                },
                literal(mask(b))
            )
        }
        "constant" => literal(num(t, "hex") & mask(num(t, "bits"))),
        "unary" => {
            let op = text(t, "operator");
            if !matches!(op, "~" | "-") {
                return Err("unqualified unary".into());
            }
            format!(
                "(({}({})) & {})",
                if op == "-" { "UINT64_C(0)-" } else { "~" },
                child(0)?,
                literal(mask(num(t, "bits")))
            )
        }
        "binary" => {
            let op = text(t, "operator");
            let b = num(t, "bits");
            if matches!(op, "<<" | ">>" | "asr" | "ror") {
                format!(
                    "hs_shift({}, {}, {b}, {})",
                    child(0)?,
                    child(1)?,
                    match op {
                        "<<" => 0,
                        ">>" => 1,
                        "asr" => 2,
                        _ => 3,
                    }
                )
            } else if matches!(op, "+" | "-" | "*" | "&" | "|" | "^" | "==" | "!=") {
                format!(
                    "((({}) {op} ({})) & {})",
                    child(0)?,
                    child(1)?,
                    literal(mask(b))
                )
            } else {
                return Err("unqualified binary".into());
            }
        }
        "integer-divide" => format!(
            "hs_div({}, {}, {}, {})",
            child(0)?,
            child(1)?,
            num(t, "bits"),
            u8::from(flag(t, "signed"))
        ),
        "extend" => format!(
            "hs_extend({}, {}, {})",
            child(0)?,
            num(t, "from_bits"),
            u8::from(flag(t, "signed"))
        ),
        "bit-extract" => format!(
            "hs_extend(({}) >> {}, {}, {})",
            child(0)?,
            num(t, "lsb"),
            num(t, "width"),
            u8::from(flag(t, "signed"))
        ),
        "select" => format!("(({}) ? ({}) : ({}))", child(0)?, child(1)?, child(2)?),
        "flag-condition" => format!("hs_condition(s->nzcv, {})", condition(t)?),
        "flag-carry" => "((s->nzcv >> 1) & 1)".into(),
        "add-with-carry" => format!(
            "hs_add_carry(s, {}, {}, {}, {}, {}, {})",
            child(0)?, child(1)?, child(2)?, num(t, "bits"),
            u8::from(flag(t, "subtract")), u8::from(flag(t, "set_flags")),
        ),
        "memory-load" => {
            let n = num(t, "bytes");
            if !matches!(n, 1 | 2 | 4 | 8) {
                return Err("unqualified load width".into());
            }
            format!(
                "hs_load(s, {}, {n}, {})",
                child(0)?,
                u8::from(flag(t, "signed"))
            )
        }
        _ => return Err(format!("unqualified expression {}", t.kind)),
    })
}
fn write_reg(t: &Tree, value: &str) -> Result<String, String> {
    let (i, b) = reg(t)?;
    Ok(match i {
        0..=30 => format!("s->x[{i}]=({value}) & {};", literal(mask(b))),
        31 => format!("s->sp=({value}) & {};", literal(mask(b))),
        _ => format!("(void)({value});"),
    })
}
fn stmt(t: &Tree) -> Result<String, String> {
    let child = |i| expr(&t.children[i]);
    Ok(match t.kind {
        "no-op" => String::new(),
        "assignment" => format!(
            "uint64_t value={};if(s->fault)return 3;{}",
            child(1)?,
            write_reg(&t.children[0], "value")?
        ),
        "sequence" => t
            .children
            .iter()
            .map(|v| stmt(v).map(|s| format!("{{{s}}}")))
            .collect::<Result<Vec<_>, _>>()?
            .join("\n"),
        "flags-compare" => format!(
            "s->nzcv=hs_compare({}, {}, {}, {});",
            child(0)?,
            child(1)?,
            num(t, "bits"),
            compare(t)?
        ),
        "flags-arithmetic" => {
            let op = text(t, "operator");
            let bits = num(t, "bits");
            let mask = format!("hs_mask({})", bits);
            let left = child(0)?;
            let right = child(1)?;
            let result = if op == "add" {
                format!("(({left} + {right}) & {mask})")
            } else {
                format!("(({left} - {right}) & {mask})")
            };
            let is_sub = if op == "add" { 0 } else { 1 };
            format!("s->nzcv=hs_flags_arith({left},{right},{result},{bits},{is_sub});")
        },
        "system-register-read" if text(t, "register") == "NZCV" => {
            if t.children.len() != 1 {
                return Err("unqualified NZCV read".into());
            }
            write_reg(&t.children[0], "((uint64_t)s->nzcv<<28)")?
        }
        "system-register-write" if text(t, "register") == "NZCV" => {
            if t.children.len() != 1 {
                return Err("unqualified NZCV write".into());
            }
            format!("s->nzcv=(uint8_t)(({})>>28)&15;", child(0)?)
        }
        "memory-store" => {
            let n = num(t, "bytes");
            if !matches!(n, 1 | 2 | 4 | 8) {
                return Err("unqualified store width".into());
            }
            format!(
                "hs_store(s, {}, {}, {n});if(s->fault)return 3;",
                child(0)?,
                child(1)?
            )
        }
        "memory-load-pair" | "memory-store-pair" => {
            let n = num(t, "bytes_per_value");
            if !matches!(n, 4 | 8) {
                return Err("unqualified pair width".into());
            }
            let mut s = format!(
                "uint64_t addr={};if(!hs_range(s,addr,{n}*2))return 3;",
                child(0)?
            );
            if t.kind == "memory-load-pair" {
                write!(
                    s,
                    "uint64_t a=hs_load(s,addr,{n},{}),b=hs_load(s,addr+{n},{n},{});{}{}",
                    u8::from(flag(t, "signed")),
                    u8::from(flag(t, "signed")),
                    write_reg(&t.children[1], "a")?,
                    write_reg(&t.children[2], "b")?
                )
                .unwrap();
            } else {
                write!(
                    s,
                    "uint64_t a={},b={};hs_store(s,addr,a,{n});hs_store(s,addr+{n},b,{n});",
                    child(1)?,
                    child(2)?
                )
                .unwrap();
            }
            s
        }
        "goto" => format!("s->pc={};continue;", literal(num(t, "target"))),
        "conditional-goto" => format!(
            "if({}){{s->pc={};continue;}}",
            child(0)?,
            literal(num(t, "target"))
        ),
        "machine-return" => {
            if t.children.len() != 1 {
                return Err("unqualified return".into());
            }
            format!("s->pc={};return 0;", child(0)?)
        }
        _ => return Err(format!("unqualified statement {}", t.kind)),
    })
}
pub(super) struct Program {
    start: u64,
    body: Vec<Tree>,
}

/// Repair-oriented C retains independently supported instructions, but stops
/// before every unsupported effect and on leaving the captured prefix. It is
/// deliberately separate from Program::new and cannot certify strict coverage.
pub(super) fn review_source(start:u64,body:&[Tree],blocked:&std::collections::BTreeSet<u64>,truncated:bool)->String{
    let mut source=format!("/* BEST-EFFORT MACHINE-STATE C RECONSTRUCTION.\n * Not an original prototype or a complete implementation.\n * Status 4 stops BEFORE unresolved effects or outside captured code.\n * Captured instruction tree truncated: {truncated}.\n * Flat 4096-byte private memory model; no host memory or external calls.\n */\n{}\n{}",include_str!("bitvector_runtime.h"),include_str!("machine_runtime.h"));
    writeln!(source,"\nstatic int hs_review_run(hs_state *s,uint64_t limit){{(void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;(void)hs_range;(void)hs_load;(void)hs_store;(void)hs_flags_arith;(void)hs_add_carry;s->pc={};s->steps=0;s->fault=0;for(;;){{if(s->steps==limit)return 2;switch(s->pc){{",literal(start)).unwrap();
    for (index,tree) in body.iter().enumerate(){
        let pc=start+index as u64*4;
        let statement=if blocked.contains(&pc){"return 4;".to_owned()}else{stmt(tree).unwrap_or_else(|_|"return 4;".to_owned())};
        writeln!(source,"case {}:{{/* loc_{pc:x}: instruction */s->steps++;{statement}s->pc={};break;}}",literal(pc),literal(pc+4)).unwrap();
    }
    source.push_str("default:return 4;}}}\n");
    writeln!(source,"typedef struct hs_review_context {{ hs_state state; uint64_t limit; int status; }} hs_review_context;\nvoid hs_review_{start:x}(hs_review_context *context){{if(context)context->status=hs_review_run(&context->state,context->limit);}}").unwrap();
    source
}
impl Program {
    pub(super) fn new(start: u64, body: Vec<Tree>) -> Result<Self, String> {
        if body.is_empty() || body.len() > 128 {
            return Err("incomplete or over-budget function".into());
        }
        for t in &body {
            stmt(t)?;
        }
        let end = start + body.len() as u64 * 4;
        let mut pending = vec![start];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(pc) = pending.pop() {
            if !visited.insert(pc) {
                continue;
            }
            if pc < start || pc >= end || (pc - start) % 4 != 0 {
                return Err("unresolved function exit".into());
            }
            let t = &body[((pc - start) / 4) as usize];
            match t.kind {
                "machine-return" => {}
                "goto" => pending.push(num(t, "target")),
                "conditional-goto" => {
                    pending.push(num(t, "target"));
                    pending.push(pc + 4)
                }
                _ => pending.push(pc + 4),
            }
        }
        Ok(Self { start, body })
    }
    pub(super) fn source(&self) -> String {
        let mut s = include_str!("bitvector_runtime.h").to_string();
        s.push_str(include_str!("machine_runtime.h"));
        writeln!(s,"\nint hs_run(hs_state *s,uint64_t limit){{(void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;(void)hs_range;(void)hs_load;(void)hs_store;(void)hs_flags_arith;(void)hs_add_carry;s->pc={};s->steps=0;s->fault=0;for(;;){{if(s->steps==limit)return 2;switch(s->pc){{",literal(self.start)).unwrap();
        for (i, t) in self.body.iter().enumerate() {
            let pc = self.start + i as u64 * 4;
            writeln!(
                s,
                "case {}:{{/* instruction {pc:#x} */s->steps++;{}s->pc={};break;}}",
                literal(pc),
                stmt(t).unwrap(),
                literal(pc + 4)
            )
            .unwrap();
        }
        s.push_str("default:return 1;}}}\n");
        s
    }
    pub(super) fn run(&self, inputs: [u64; 8], limit: u64) -> V {
        let mut s = State {
            x: [0; 31],
            sp: BASE + SIZE as u64,
            pc: self.start,
            steps: 0,
            nzcv: 0,
            memory: [0; SIZE],
        };
        s.x[..8].copy_from_slice(&inputs);
        let status = loop {
            if s.steps == limit {
                break 2;
            }
            let t =
                s.pc.checked_sub(self.start)
                    .filter(|o| o % 4 == 0)
                    .and_then(|o| usize::try_from(o / 4).ok())
                    .and_then(|i| self.body.get(i));
            let Some(t) = t else { break 1 };
            s.steps += 1;
            match s.statement(t) {
                Ok(Control::Next) => s.pc = s.pc.wrapping_add(4),
                Ok(Control::Branch(pc)) => s.pc = pc,
                Ok(Control::Return(pc)) => {
                    s.pc = pc;
                    break 0;
                }
                Err(()) => break 3,
            }
        };
        object([
            ("status", (status as u64).into()),
            ("registers", array(s.x.into_iter().map(address))),
            ("sp", address(s.sp)),
            ("pc", address(s.pc)),
            ("steps", s.steps.into()),
            ("nzcv", s.nzcv.into()),
            (
                "memory_hex",
                s.memory
                    .iter()
                    .map(|v| format!("{v:02x}"))
                    .collect::<String>()
                    .into(),
            ),
        ])
    }
}
struct State {
    x: [u64; 31],
    sp: u64,
    pc: u64,
    steps: u64,
    nzcv: u8,
    memory: [u8; SIZE],
}
enum Control {
    Next,
    Branch(u64),
    Return(u64),
}
impl State {
    fn read(&self, t: &Tree) -> u64 {
        let (i, b) = reg(t).unwrap();
        (match i {
            0..=30 => self.x[i],
            31 => self.sp,
            _ => 0,
        }) & mask(b)
    }
    fn write(&mut self, t: &Tree, value: u64) {
        let (i, b) = reg(t).unwrap();
        let v = value & mask(b);
        match i {
            0..=30 => self.x[i] = v,
            31 => self.sp = v,
            _ => {}
        }
    }
    fn range(&self, a: u64, n: u64) -> Result<usize, ()> {
        let i = a.checked_sub(BASE).ok_or(())?;
        if i > SIZE as u64 || n > SIZE as u64 - i {
            return Err(());
        }
        Ok(i as usize)
    }
    fn load(&self, a: u64, n: u64, signed: bool) -> Result<u64, ()> {
        let i = self.range(a, n)?;
        let mut v = 0;
        for j in 0..n as usize {
            v |= u64::from(self.memory[i + j]) << (j * 8)
        }
        Ok(extend(v, n * 8, signed))
    }
    fn store(&mut self, a: u64, v: u64, n: u64) -> Result<(), ()> {
        let i = self.range(a, n)?;
        for j in 0..n as usize {
            self.memory[i + j] = (v >> (j * 8)) as u8
        }
        Ok(())
    }
    fn eval(&mut self, t: &Tree) -> Result<u64, ()> {
        macro_rules! child {
            ($i:expr) => {
                self.eval(&t.children[$i])?
            };
        }
        Ok(match t.kind {
            "register" => self.read(t),
            "constant" => num(t, "hex") & mask(num(t, "bits")),
            "unary" => {
                let v = child!(0);
                (if text(t, "operator") == "~" {
                    !v
                } else {
                    v.wrapping_neg()
                }) & mask(num(t, "bits"))
            }
            "binary" => {
                let a = child!(0);
                let b = child!(1);
                let bits = num(t, "bits");
                (match text(t, "operator") {
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "&" => a & b,
                    "|" => a | b,
                    "^" => a ^ b,
                    "==" => u64::from(a == b),
                    "!=" => u64::from(a != b),
                    k => shift(a, b, bits, k),
                }) & mask(bits)
            }
            "integer-divide" => {
                let bits = num(t, "bits");
                let a = child!(0) & mask(bits);
                let b = child!(1) & mask(bits);
                if b == 0 {
                    0
                } else {
                    let na = flag(t, "signed") && a >> (bits - 1) != 0;
                    let nb = flag(t, "signed") && b >> (bits - 1) != 0;
                    let a = if na { a.wrapping_neg() & mask(bits) } else { a };
                    let b = if nb { b.wrapping_neg() & mask(bits) } else { b };
                    let q = a / b;
                    if na != nb {
                        q.wrapping_neg() & mask(bits)
                    } else {
                        q
                    }
                }
            }
            "extend" => extend(child!(0), num(t, "from_bits"), flag(t, "signed")),
            "bit-extract" => extend(
                child!(0) >> num(t, "lsb"),
                num(t, "width"),
                flag(t, "signed"),
            ),
            "flag-condition" => u64::from(
                condition_holds(self.nzcv, lift_flag_predicate(condition(t).unwrap())).unwrap(),
            ),
            "flag-carry" => u64::from((self.nzcv & 2) != 0),
            "nzcv-value" => u64::from(self.nzcv) << 28,
            "add-with-carry" => {
                let left = child!(0);
                let right = child!(1);
                let carry = child!(2) != 0;
                let bits = num(t, "bits");
                let mask = mask(bits);
                let a = left & mask;
                let b = right & mask;
                let operand = if flag(t, "subtract") { !b & mask } else { b };
                let sum = a as u128 + operand as u128 + u128::from(carry);
                let result = sum as u64 & mask;
                if flag(t, "set_flags") {
                    let sign = 1u64 << (bits - 1);
                    let n = u8::from(result & sign != 0);
                    let z = u8::from(result == 0);
                    let c = u8::from(sum > mask as u128);
                    let v = u8::from(if flag(t, "subtract") {
                        ((a ^ b) & (a ^ result) & sign) != 0
                    } else {
                        ((!(a ^ b)) & (a ^ result) & sign) != 0
                    });
                    self.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
                }
                result
            }
            "select" => {
                if child!(0) != 0 {
                    child!(1)
                } else {
                    child!(2)
                }
            }
            "memory-load" => {
                let address = child!(0);
                self.load(address, num(t, "bytes"), flag(t, "signed"))?
            }
            _ => unreachable!("validated machine AST expression"),
        })
    }
    fn statement(&mut self, t: &Tree) -> Result<Control, ()> {
        match t.kind {
            "no-op" => {}
            "assignment" => {
                let v = self.eval(&t.children[1])?;
                self.write(&t.children[0], v)
            }
            "sequence" => {
                for v in &t.children {
                    self.statement(v)?;
                }
            }
            "flags-compare" => {
                self.nzcv = compare_flags(
                    match compare(t).unwrap() {
                        0 => IrCompareKind::Compare,
                        1 => IrCompareKind::CompareNegative,
                        _ => IrCompareKind::Test,
                    },
                    self.eval(&t.children[0])?,
                    self.eval(&t.children[1])?,
                    num(t, "bits") == 64,
                )
            }
            "flags-arithmetic" => {
                let op = text(t, "operator");
                let bits = num(t, "bits");
                let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
                    let left = self.eval(&t.children[0])?;
                let right = self.eval(&t.children[1])?;
                let result = if op == "add" {
                    left.wrapping_add(right) & mask
                } else {
                    left.wrapping_sub(right) & mask
                };
                let n = ((result >> (bits - 1)) & 1) as u8;
                let z = if result == 0 { 1 } else { 0 };
                let c = if op == "add" {
                    (left > mask - right) as u8
                } else {
                    (left >= right) as u8
                };
                let v = if op == "add" {
                    ((((!(left ^ right)) & (left ^ result)) >> (bits - 1)) & 1) as u8
                } else {
                    ((((left ^ right) & (left ^ result)) >> (bits - 1)) & 1) as u8
                };
                self.nzcv = (n << 3) | (z << 2) | (c << 1) | v;
            }
            "system-register-read" if text(t, "register") == "NZCV" => {
                let value = u64::from(self.nzcv) << 28;
                self.write(&t.children[0], value);
            }
            "system-register-write" if text(t, "register") == "NZCV" => {
                let source = self.eval(&t.children[0])?;
                self.nzcv = ((source >> 28) & 15) as u8;
            }
            "memory-store" => {
                let address = self.eval(&t.children[0])?;
                let value = self.eval(&t.children[1])?;
                self.store(address, value, num(t, "bytes"))?
            }
            "memory-load-pair" => {
                let a = self.eval(&t.children[0])?;
                let n = num(t, "bytes_per_value");
                self.range(a, n * 2)?;
                let first = self.load(a, n, flag(t, "signed"))?;
                let second = self.load(a + n, n, flag(t, "signed"))?;
                self.write(&t.children[1], first);
                self.write(&t.children[2], second)
            }
            "memory-store-pair" => {
                let a = self.eval(&t.children[0])?;
                let n = num(t, "bytes_per_value");
                self.range(a, n * 2)?;
                let first = self.eval(&t.children[1])?;
                let second = self.eval(&t.children[2])?;
                self.store(a, first, n)?;
                self.store(a + n, second, n)?;
            }
            "goto" => return Ok(Control::Branch(num(t, "target"))),
            "conditional-goto" => {
                if self.eval(&t.children[0])? != 0 {
                    return Ok(Control::Branch(num(t, "target")));
                }
            }
            "machine-return" => return Ok(Control::Return(self.eval(&t.children[0])?)),
            _ => unreachable!("validated machine AST statement"),
        }
        Ok(Control::Next)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn from_words(words: &[u32]) -> Result<Program, String> {
        let trees = words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                let pc = 0x1000 + index as u64 * 4;
                let operation = decode(*word, pc);
                instruction_model(operation, lift_operation(operation, pc), *word)
                    .ok_or_else(|| "unmodeled".into())
            })
            .collect::<Result<Vec<_>, String>>()?;
        Program::new(0x1000, trees)
    }
    #[test]
    fn flags_read_original_aliased_operands() {
        let program = from_words(&[0xb1000400, 0xd65f03c0]).unwrap(); // ADDS X0,X0,#1;RET
        let result = program.run([u64::MAX, 0, 0, 0, 0, 0, 0, 0], 10);
        assert_eq!(result.get("nzcv"), Some(&V::Int(6)));
        assert_eq!(result.get("status"), Some(&V::Int(0)));
        let result = program.run([0x7fffffffffffffff, 0, 0, 0, 0, 0, 0, 0], 10);
        assert_eq!(result.get("nzcv"), Some(&V::Int(9)));
    }
    #[test]
    fn add_with_carry_updates_machine_state_and_nzcv_at_both_widths() {
        let x0 = |result: &V| match result.get("registers") {
            Some(V::Array(registers)) => registers[0].clone(),
            _ => panic!("missing x0 in machine-state result"),
        };
        let add64 = from_words(&[0xf100_047f, 0xba02_0020, 0xd65f_03c0]).unwrap();
        let result = add64.run([0, u64::MAX, 0, 1, 0, 0, 0, 0], 10);
        assert_eq!(result.get("status"), Some(&V::Int(0)));
        assert_eq!(x0(&result), V::String("0x0".into()));
        assert_eq!(result.get("nzcv"), Some(&V::Int(6)));

        let sub64 = from_words(&[0xf100_047f, 0xfa02_0020, 0xd65f_03c0]).unwrap();
        let result = sub64.run([0, 0, 0, 0, 0, 0, 0, 0], 10);
        assert_eq!(x0(&result), V::String("0xffffffffffffffff".into()));
        assert_eq!(result.get("nzcv"), Some(&V::Int(8)));

        let add32 = from_words(&[0x7100_047f, 0x3a02_0020, 0xd65f_03c0]).unwrap();
        let result = add32.run([0, u32::MAX as u64, 0, 1, 0, 0, 0, 0], 10);
        assert_eq!(x0(&result), V::String("0x0".into()));
        assert_eq!(result.get("nzcv"), Some(&V::Int(6)));
    }
    #[test]
    fn nzcv_system_register_round_trip_updates_machine_flags() {
        let program = from_words(&[
            0xf100_045f, // CMP X2,#1
            0xd53b_4203, // MRS X3,NZCV
            0xd51b_421f, // MSR NZCV,XZR
            0x9a9f_27e4, // CSET X4,CS
            0xca04_0060, // EOR X0,X3,X4
            0xd65f_03c0, // RET
        ]).unwrap();
        let result = program.run([0, 0, 1, 0, 0, 0, 0, 0], 10);
        assert_eq!(result.get("status"), Some(&V::Int(0)));
        assert_eq!(result.get("nzcv"), Some(&V::Int(0)));
        match result.get("registers") {
            Some(V::Array(registers)) => {
                assert_eq!(registers[3], V::String("0x60000000".into()));
                assert_eq!(registers[4], V::String("0x1".into()));
            }
            _ => panic!("missing machine-state registers"),
        }
    }
    #[test]
    fn unknown_exits_and_unpredictable_aliases_fail_closed() {
        assert!(from_words(&[0x14000008]).is_err()); // outside-function B
        assert!(from_words(&[0xd503201f]).is_err()); // fallthrough outside function
        assert!(from_words(&[0xf8408400, 0xd65f03c0]).is_err()); // LDR writeback alias
        let spin = from_words(&[0x14000000]).unwrap();
        let result = spin.run([0; 8], 7);
        assert_eq!(result.get("status"), Some(&V::Int(2)));
        assert_eq!(result.get("steps"), Some(&V::Int(7)));
    }
    #[test]
    fn exact_bits() {
        assert_eq!(shift(0x80000000, 31, 32, "asr"), u32::MAX as u64);
        assert_eq!(shift(1, 0, 64, "ror"), 1);
        assert_eq!(extend(0x80, 8, true), u64::MAX - 127)
    }
    #[test]
    fn memory_bounds() {
        let s = State {
            x: [0; 31],
            sp: 0,
            pc: 0,
            steps: 0,
            nzcv: 0,
            memory: [0; SIZE],
        };
        assert!(s.range(u64::MAX, 8).is_err());
        assert!(s.range(BASE - 1, 1).is_err());
        assert!(s.range(BASE + SIZE as u64 - 7, 8).is_err());
        assert!(s.range(BASE + SIZE as u64 - 8, 8).is_ok())
    }
}
