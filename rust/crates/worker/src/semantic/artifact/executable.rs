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
        writeln!(s,"\nint hs_run(hs_state *s,uint64_t limit){{(void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;(void)hs_range;(void)hs_load;(void)hs_store;s->pc={};s->steps=0;s->fault=0;for(;;){{if(s->steps==limit)return 2;switch(s->pc){{",literal(self.start)).unwrap();
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
    fn eval(&self, t: &Tree) -> Result<u64, ()> {
        let child = |i| self.eval(&t.children[i]);
        Ok(match t.kind {
            "register" => self.read(t),
            "constant" => num(t, "hex") & mask(num(t, "bits")),
            "unary" => {
                let v = child(0)?;
                (if text(t, "operator") == "~" {
                    !v
                } else {
                    v.wrapping_neg()
                }) & mask(num(t, "bits"))
            }
            "binary" => {
                let a = child(0)?;
                let b = child(1)?;
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
                let a = child(0)? & mask(bits);
                let b = child(1)? & mask(bits);
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
            "extend" => extend(child(0)?, num(t, "from_bits"), flag(t, "signed")),
            "bit-extract" => extend(
                child(0)? >> num(t, "lsb"),
                num(t, "width"),
                flag(t, "signed"),
            ),
            "flag-condition" => u64::from(
                condition_holds(self.nzcv, lift_flag_predicate(condition(t).unwrap())).unwrap(),
            ),
            "select" => {
                if child(0)? != 0 {
                    child(1)?
                } else {
                    child(2)?
                }
            }
            "memory-load" => self.load(child(0)?, num(t, "bytes"), flag(t, "signed"))?,
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
            "memory-store" => self.store(
                self.eval(&t.children[0])?,
                self.eval(&t.children[1])?,
                num(t, "bytes"),
            )?,
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
