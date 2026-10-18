//! Evidence-scoped source expressions, not renamed machine-register strings.
//! Pure acyclic functions: symbolic frame objects/copies disappear only after
//! every reachable return proves restored SP, LR and Darwin nonvolatile GPRs.
use super::*;
use crate::signatures::SourceSignature;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Symbol {
    Value(usize, u8),
    Frame(i64),
    Unknown(u64),
}
#[derive(Clone)]
struct State {
    registers: Vec<Symbol>,
    flags: Option<usize>,
    frame: BTreeMap<(i64, u64), (Symbol, BTreeSet<u64>)>,
    path: usize,
}
struct Expr {
    kind: &'static str,
    template: String,
    children: Vec<usize>,
    origins: BTreeSet<u64>,
}
struct Dag {
    nodes: Vec<Expr>,
}
impl Dag {
    fn add(
        &mut self,
        kind: &'static str,
        template: String,
        children: Vec<usize>,
        pc: u64,
    ) -> Result<usize, String> {
        if self.nodes.len() >= 2048 {
            return Err("source expression budget exhausted".into());
        }
        let mut origins = BTreeSet::from([pc]);
        for child in &children {
            origins.extend(self.nodes[*child].origins.iter().copied());
        }
        let id = self.nodes.len();
        self.nodes.push(Expr {
            kind,
            template,
            children,
            origins,
        });
        Ok(id)
    }
    fn constant(&mut self, value: u64, pc: u64) -> Result<usize, String> {
        self.add(
            "source-constant",
            format!("UINT64_C(0x{value:x})"),
            vec![],
            pc,
        )
    }
    fn literal(&self, symbol: &Symbol) -> Option<u64> {
        let Symbol::Value(id, _) = symbol else {
            return None;
        };
        let node = &self.nodes[*id];
        (node.kind == "source-constant")
            .then(|| {
                node.template
                    .strip_prefix("UINT64_C(0x")
                    .and_then(|s| s.strip_suffix(')'))
                    .and_then(|s| u64::from_str_radix(s, 16).ok())
            })
            .flatten()
    }
    fn value(&mut self, symbol: Symbol, bits: u8, pc: u64) -> Result<usize, String> {
        match symbol {
            Symbol::Value(id, known) if bits <= known => {
                if bits == 64 {
                    Ok(id)
                } else {
                    self.add("source-cast", format!("((uint{bits}_t)($0))"), vec![id], pc)
                }
            }
            _ => Err("source expression depends on unknown bits or a frame address".into()),
        }
    }
}
fn number(t: &Tree, key: &str) -> u64 {
    match t.attributes.get(key) {
        Some(V::Int(v)) => *v as u64,
        Some(V::String(v)) => u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap(),
        _ => 0,
    }
}
fn attribute<'a>(t: &'a Tree, key: &str) -> &'a str {
    match t.attributes.get(key) {
        Some(V::String(v)) => v,
        _ => "",
    }
}
fn flag(t: &Tree, key: &str) -> u8 {
    u8::from(t.attributes.get(key) == Some(&V::Bool(true)))
}
fn register(t: &Tree) -> Result<(usize, u8), String> {
    let index = match attribute(t, "class") {
        "general" => attribute(t, "name")[1..]
            .parse()
            .map_err(|_| "invalid GPR")?,
        "stack-pointer" => 31,
        "zero" => 32,
        _ => return Err("unsupported register bank".into()),
    };
    Ok((index, number(t, "bits") as u8))
}
fn read(t: &Tree, state: &State, dag: &mut Dag, pc: u64) -> Result<Symbol, String> {
    let child = |i, dag: &mut Dag| read(&t.children[i], state, dag, pc);
    if t.kind == "register" {
        let (i, bits) = register(t)?;
        let value = if i == 32 {
            Symbol::Value(dag.constant(0, pc)?, 64)
        } else {
            state.registers[i].clone()
        };
        return match value {
            Symbol::Value(_, known) if bits <= known => {
                Ok(Symbol::Value(dag.value(value, bits, pc)?, 64))
            }
            Symbol::Frame(_) if bits == 64 => Ok(value),
            Symbol::Unknown(_) if bits == 64 => Ok(value),
            _ => Ok(Symbol::Unknown(pc ^ 0x8000000000000000)),
        };
    }
    if t.kind == "constant" {
        let bits = number(t, "bits");
        let value = number(t, "hex")
            & if bits == 64 {
                u64::MAX
            } else {
                (1u64 << bits) - 1
            };
        return Ok(Symbol::Value(dag.constant(value, pc)?, 64));
    }
    if t.kind == "memory-load" {
        let Symbol::Frame(offset) = child(0, dag)? else {
            return Err("external or unresolved memory read".into());
        };
        let bytes = number(t, "bytes");
        frame_range(state, offset, bytes)?;
        let Some(((slot, size), (value, origins))) =
            state.frame.iter().find(|((slot, size), _)| {
                *slot <= offset && offset + bytes as i64 <= *slot + *size as i64
            })
        else {
            return Err("uninitialized frame read".into());
        };
        let shift = (offset - *slot) as u64 * 8;
        let value = value.clone();
        let origins = origins.clone();
        let size = *size;
        if size == bytes && shift == 0 {
            match value {
                Symbol::Unknown(_) | Symbol::Frame(_) if bytes == 8 => return Ok(value),
                _ => {}
            }
        }
        let value = dag.value(value, 64, pc)?;
        let id = dag.add(
            "source-frame-reload",
            format!(
                "hs_extend(($0) >> {shift}, {}, {})",
                bytes * 8,
                flag(t, "signed")
            ),
            vec![value],
            pc,
        )?;
        dag.nodes[id].origins.extend(origins);
        return Ok(Symbol::Value(id, 64));
    }
    if t.kind == "flag-condition" {
        let code = match attribute(t, "predicate") {
            "eq" => 0,
            "ne" => 1,
            "hs" => 2,
            "lo" => 3,
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
            "al" => return Ok(Symbol::Value(dag.constant(1, pc)?, 64)),
            _ => return Err("unqualified flag condition".into()),
        };
        let flags = state.flags.ok_or("condition depends on entry NZCV")?;
        return Ok(Symbol::Value(
            dag.add(
                "source-condition",
                format!("hs_condition($0, {code})"),
                vec![flags],
                pc,
            )?,
            64,
        ));
    }
    let values = t
        .children
        .iter()
        .map(|v| read(v, state, dag, pc))
        .collect::<Result<Vec<_>, _>>()?;
    if t.kind == "binary" && matches!(attribute(t, "operator"), "+" | "-") {
        if let (Symbol::Frame(offset), Some(value)) = (&values[0], dag.literal(&values[1])) {
            let next = if attribute(t, "operator") == "+" {
                offset.checked_add(value as i64)
            } else {
                offset.checked_sub(value as i64)
            }
            .ok_or("frame offset overflow")?;
            return Ok(Symbol::Frame(next));
        }
    }
    if values.iter().any(|v| matches!(v, Symbol::Unknown(_))) {
        return Ok(Symbol::Unknown(pc ^ 0x4000000000000000));
    }
    let bits = number(t, "bits");
    let children = values
        .into_iter()
        .map(|v| dag.value(v, 64, pc))
        .collect::<Result<Vec<_>, _>>()?;
    let template = match t.kind {
        "binary" => {
            let op = attribute(t, "operator");
            if matches!(op, "<<" | ">>" | "asr" | "ror") {
                format!(
                    "hs_shift($0, $1, {bits}, {})",
                    match op {
                        "<<" => 0,
                        ">>" => 1,
                        "asr" => 2,
                        _ => 3,
                    }
                )
            } else if matches!(op, "+" | "-" | "*" | "&" | "|" | "^" | "==" | "!=") {
                if bits == 64 {
                    format!("(($0) {op} ($1))")
                } else {
                    format!("((uint{bits}_t)(($0) {op} ($1)))")
                }
            } else {
                return Err("unqualified source binary operation".into());
            }
        }
        "unary" => {
            let op = attribute(t, "operator");
            if !matches!(op, "~" | "-") {
                return Err("unqualified unary".into());
            }
            format!(
                "((uint{bits}_t)({}($0)))",
                if op == "-" { "UINT64_C(0)-" } else { "~" }
            )
        }
        "extend" => format!(
            "hs_extend($0, {}, {})",
            number(t, "from_bits"),
            flag(t, "signed")
        ),
        "bit-extract" => format!(
            "hs_extend(($0) >> {}, {}, {})",
            number(t, "lsb"),
            number(t, "width"),
            flag(t, "signed")
        ),
        "integer-divide" => format!("hs_div($0, $1, {bits}, {})", flag(t, "signed")),
        "select" => "(($0) ? ($1) : ($2))".into(),
        _ => return Err(format!("unqualified source expression {}", t.kind)),
    };
    Ok(Symbol::Value(
        dag.add("source-expression", template, children, pc)?,
        64,
    ))
}
fn frame_range(state: &State, offset: i64, bytes: u64) -> Result<(), String> {
    let Symbol::Frame(sp) = state.registers[31] else {
        return Err("unknown SP".into());
    };
    if sp % 16 != 0
        || offset < sp - 128
        || offset < -4096
        || offset.checked_add(bytes as i64).map_or(true, |end| end > 0)
    {
        return Err("memory access is not a proven local frame/red-zone object".into());
    }
    Ok(())
}
fn write(t: &Tree, value: Symbol, state: &mut State, dag: &mut Dag, pc: u64) -> Result<(), String> {
    let (i, bits) = register(t)?;
    if i == 32 {
        return Ok(());
    }
    let value = if bits == 32 {
        match value {
            Symbol::Value(_, _) => Symbol::Value(dag.value(value, 32, pc)?, 64),
            _ => Symbol::Unknown(pc ^ 0x2000000000000000),
        }
    } else {
        value
    };
    let value = match value {
        Symbol::Value(id, known) => {
            Symbol::Value(dag.add("source-copy", "$0".into(), vec![id], pc)?, known)
        }
        other => other,
    };
    if i == 31 && !matches!(value,Symbol::Frame(offset) if (-4096..=0).contains(&offset)) {
        return Err("dynamic or escaped frame".into());
    }
    state.registers[i] = value;
    Ok(())
}
fn store(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64) -> Result<(), String> {
    let Symbol::Frame(offset) = read(&t.children[0], state, dag, pc)? else {
        return Err("external or escaped memory write".into());
    };
    let bytes = number(t, "bytes");
    frame_range(state, offset, bytes)?;
    let value = read(&t.children[1], state, dag, pc)?;
    state
        .frame
        .retain(|(slot, size), _| offset + bytes as i64 <= *slot || *slot + *size as i64 <= offset);
    state
        .frame
        .insert((offset, bytes), (value, BTreeSet::from([pc])));
    Ok(())
}
fn statement(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64) -> Result<(), String> {
    match t.kind {
        "no-op" => Ok(()),
        "assignment" => {
            let value = read(&t.children[1], state, dag, pc)?;
            write(&t.children[0], value, state, dag, pc)
        }
        "sequence" => {
            for child in &t.children {
                statement(child, state, dag, pc)?;
            }
            Ok(())
        }
        "memory-store" => store(t, state, dag, pc),
        "flags-compare" => {
            let a = read(&t.children[0], state, dag, pc)?;
            let b = read(&t.children[1], state, dag, pc)?;
            match (a, b) {
                (Symbol::Value(a, _), Symbol::Value(b, _)) => {
                    let kind = match attribute(t, "operator") {
                        "cmp" => 0,
                        "cmn" => 1,
                        "tst" => 2,
                        _ => return Err("unqualified flag write".into()),
                    };
                    state.flags = Some(dag.add(
                        "source-nzcv",
                        format!("hs_compare($0, $1, {}, {kind})", number(t, "bits")),
                        vec![a, b],
                        pc,
                    )?);
                }
                _ => state.flags = None,
            }
            Ok(())
        }
        "memory-load-pair" | "memory-store-pair" => {
            let address = read(&t.children[0], state, dag, pc)?;
            let Symbol::Frame(offset) = address else {
                return Err("external memory pair".into());
            };
            let bytes = number(t, "bytes_per_value");
            frame_range(state, offset, bytes * 2)?;
            if t.kind == "memory-store-pair" {
                let first = read(&t.children[1], state, dag, pc)?;
                let second = read(&t.children[2], state, dag, pc)?;
                state.frame.retain(|(slot, size), _| {
                    offset + bytes as i64 * 2 <= *slot || *slot + *size as i64 <= offset
                });
                state
                    .frame
                    .insert((offset, bytes), (first, BTreeSet::from([pc])));
                state.frame.insert(
                    (offset + bytes as i64, bytes),
                    (second, BTreeSet::from([pc])),
                );
            } else {
                let first = state
                    .frame
                    .get(&(offset, bytes))
                    .cloned()
                    .ok_or("unknown frame pair")?;
                let second = state
                    .frame
                    .get(&(offset + bytes as i64, bytes))
                    .cloned()
                    .ok_or("unknown frame pair")?;
                // Evaluate both original values before either base/destination changes.
                let mut loaded = Vec::new();
                for (value, origins) in [first, second] {
                    let value = match value {
                        Symbol::Value(id, _) => {
                            let id = dag.add(
                                "source-frame-reload",
                                format!("hs_extend($0, {}, {})", bytes * 8, flag(t, "signed")),
                                vec![id],
                                pc,
                            )?;
                            dag.nodes[id].origins.extend(origins);
                            Symbol::Value(id, 64)
                        }
                        other if bytes == 8 => other,
                        _ => return Err("partial unknown frame restore".into()),
                    };
                    loaded.push(value);
                }
                write(&t.children[1], loaded[0].clone(), state, dag, pc)?;
                write(&t.children[2], loaded[1].clone(), state, dag, pc)?;
            }
            Ok(())
        }
        _ => Err(format!("unqualified source statement {}", t.kind)),
    }
}
pub(super) struct Recovery {
    dag: Dag,
    result: usize,
    signature: SourceSignature,
    paths: usize,
}
pub(super) fn recover(
    start: u64,
    trees: &[Tree],
    operations: &[IrOperation],
    signature: SourceSignature,
) -> Result<Recovery, String> {
    if operations
        .iter()
        .any(|v| matches!(v, IrOperation::Address { .. }))
    {
        return Err("relocatable address dependencies unresolved".into());
    }
    let mut dag = Dag { nodes: Vec::new() };
    let truth = dag.constant(1, start)?;
    let mut state = State {
        registers: (0..32).map(Symbol::Unknown).collect(),
        flags: None,
        frame: BTreeMap::new(),
        path: truth,
    };
    state.registers[31] = Symbol::Frame(0);
    for parameter in &signature.parameters {
        let name = &parameter.name;
        let bits = parameter.bits;
        if name.starts_with("hs_")
            || [
                "hs_mask",
                "hs_extend",
                "hs_shift",
                "hs_div",
                "hs_condition",
                "hs_compare",
            ]
            .contains(&signature.name.as_str())
        {
            return Err("source/helper namespace collision".into());
        }
        let code = if parameter.pointer {
            format!("((uint64_t)(uintptr_t){name})")
        } else if bits == 64 {
            format!("((uint64_t){name})")
        } else if parameter.signed {
            format!("((uint64_t)(uint32_t)(int32_t){name})")
        } else {
            format!("((uint64_t)(uint32_t){name})")
        };
        let id = dag.add("source-parameter", code, vec![], start)?;
        state.registers[parameter.register as usize] =
            Symbol::Value(id, if bits < 64 { 32 } else { 64 });
    }
    let mut pending = vec![(0usize, state, BTreeSet::new())];
    let mut returned = Vec::new();
    let mut visits = 0;
    while let Some((mut index, mut state, mut seen)) = pending.pop() {
        loop {
            visits += 1;
            if visits > 2048 || pending.len() + returned.len() > 32 {
                return Err("source path budget exhausted".into());
            }
            if !seen.insert(index) {
                return Err("loop region requires canonical loop/phi recovery".into());
            }
            let t = trees.get(index).ok_or("unresolved source exit")?;
            let pc = start + index as u64 * 4;
            match t.kind {
                "machine-return" => {
                    if t.children.len() != 1
                        || read(&t.children[0], &state, &mut dag, pc)? != Symbol::Unknown(30)
                    {
                        return Err("return target is not the entry link register".into());
                    }
                    if state.registers[31] != Symbol::Frame(0)
                        || (18..=29).any(|r| state.registers[r] != Symbol::Unknown(r as u64))
                        || state.registers[30] != Symbol::Unknown(30)
                    {
                        return Err(
                            "nonvolatile registers, SP or return target not restored".into()
                        );
                    }
                    let value = dag.value(
                        state.registers[0].clone(),
                        signature.result_bits.max(32),
                        pc,
                    )?;
                    let value = dag.add("source-return", "$0".into(), vec![value], pc)?;
                    returned.push((state.path, value));
                    break;
                }
                "goto" | "conditional-goto" => {
                    let target = number(t, "target");
                    let delta = target.checked_sub(start).ok_or("source tail dependency")?;
                    if delta % 4 != 0 || delta / 4 >= trees.len() as u64 {
                        return Err("source branch outside function".into());
                    }
                    if t.kind == "goto" {
                        index = (delta / 4) as usize;
                        continue;
                    }
                    let condition = read(&t.children[0], &state, &mut dag, pc)?;
                    let condition = dag.value(condition, 64, pc)?;
                    let mut taken = state.clone();
                    taken.path = dag.add(
                        "source-path",
                        "(($0) && ($1))".into(),
                        vec![state.path, condition],
                        pc,
                    )?;
                    state.path = dag.add(
                        "source-path",
                        "(($0) && !($1))".into(),
                        vec![state.path, condition],
                        pc,
                    )?;
                    pending.push(((delta / 4) as usize, taken, seen.clone()));
                }
                _ => statement(t, &mut state, &mut dag, pc)?,
            }
            index += 1;
        }
    }
    let paths = returned.len();
    let Some((_, mut result)) = returned.pop() else {
        return Err("no qualified source return".into());
    };
    while let Some((condition, value)) = returned.pop() {
        result = dag.add(
            "source-select",
            "(($0) ? ($1) : ($2))".into(),
            vec![condition, value, result],
            start,
        )?;
    }
    Ok(Recovery {
        dag,
        result,
        signature,
        paths,
    })
}
impl Recovery {
    pub(super) fn emit(
        &self,
        builder: &mut Builder,
        declaration: &str,
        ir: &[String],
        start: u64,
    ) -> io::Result<(String, String)> {
        let mut live = BTreeSet::new();
        let mut pending = vec![self.result];
        while let Some(id) = pending.pop() {
            if live.insert(id) {
                pending.extend(self.dag.nodes[id].children.iter().copied());
            }
        }
        let mut uses = vec![0usize; self.dag.nodes.len()];
        for &id in &live {
            for child in &self.dag.nodes[id].children {
                uses[*child] += 1;
            }
        }
        let mut rendered = BTreeMap::<usize, String>::new();
        let mut asts = BTreeMap::<usize, String>::new();
        let mut declarations = String::new();
        for &id in &live {
            let node = &self.dag.nodes[id];
            let mut expression = node.template.clone();
            for (index, child) in node.children.iter().enumerate() {
                expression = expression.replace(&format!("${index}"), &rendered[child]);
            }
            let children = node.children.iter().map(|i| asts[i].clone()).collect();
            let evidence = node
                .origins
                .iter()
                .filter_map(|pc| ir.get(((pc - start) / 4) as usize))
                .cloned()
                .chain(std::iter::once(declaration.to_owned()))
                .collect::<Vec<_>>();
            let origins = array(
                node.origins
                    .iter()
                    .map(|pc| object([("start", address(*pc)), ("end", address(*pc + 4))])),
            );
            let ast = builder.ast_spans(
                node.kind,
                Some("uint64_t"),
                children,
                &evidence,
                origins,
                object([
                    ("template", node.template.clone().into()),
                    (
                        "span_role",
                        if node.kind == "source-parameter" {
                            "entry ABI binding"
                        } else {
                            "instruction dependencies"
                        }
                        .into(),
                    ),
                ]),
            )?;
            asts.insert(id, ast);
            if expression.len() > 160
                || (uses[id] > 1 && !matches!(node.kind, "source-constant" | "source-parameter"))
            {
                let name = format!("hs_value_{id}");
                declarations.push_str(&format!("    uint64_t {name} = {expression};\n"));
                expression = name;
            }
            if declarations.len() > 128 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source rendering budget exhausted",
                ));
            }
            rendered.insert(id, expression);
        }
        let s = &self.signature;
        let mut headers=String::from("#include <stdint.h>\n#include <stddef.h>\n#include <string.h>\n#include <sys/types.h>\n#include <objc/objc.h>\n");
        let mentions = |prefix: &str| {
            std::iter::once(s.result_spelling.as_str())
                .chain(s.parameters.iter().map(|p| p.spelling.as_str()))
                .any(|ty| {
                    ty.split(|c: char| c.is_whitespace() || c == '*')
                        .any(|word| word.starts_with(prefix))
                })
        };
        if mentions("NS") {
            headers.push_str("#import <Foundation/Foundation.h>\n");
        }
        if mentions("CF") {
            headers.push_str("#include <CoreFoundation/CoreFoundation.h>\n");
        }
        if mentions("dispatch_") {
            headers.push_str("#include <dispatch/dispatch.h>\n");
        }
        let parameters = s
            .parameters
            .iter()
            .map(|p| format!("{} {}", p.spelling, p.name))
            .collect::<Vec<_>>()
            .join(", ");
        let result = &rendered[&self.result];
        let return_code = if s.result_pointer {
            format!(
                "    return ({}) (uintptr_t)({result});\n",
                s.result_spelling
            )
        } else if s.result_signed {
            format!("    uint{0}_t hs_result_bits = (uint{0}_t)({result});\n    int{0}_t hs_signed_result;\n    memcpy(&hs_signed_result, &hs_result_bits, sizeof hs_result_bits);\n    return ({1})hs_signed_result;\n",s.result_bits,s.result_spelling)
        } else {
            format!("    return ({})({result});\n", s.result_spelling)
        };
        let assertions=format!("    _Static_assert(sizeof(void *) == 8 && sizeof(long) == 8, \"Apple LP64 ABI required\");\n    _Static_assert(sizeof({}) == {}, \"result type layout mismatch\");\n",s.result_spelling,s.result_bits/8)
            + &s.parameters.iter().map(|p|format!("    _Static_assert(sizeof({}) == {}, \"parameter type layout mismatch\");\n",p.spelling,p.bits/8)).collect::<String>();
        let unused = s
            .parameters
            .iter()
            .map(|p| format!("    (void){};\n", p.name))
            .collect::<String>();
        let output=format!("{headers}{}\n/* Recovered from bounded acyclic IR; ordinary local frame effects eliminated. */\n{} {}({}) {{\n{assertions}    (void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;\n{unused}{declarations}{return_code}}}\n",include_str!("bitvector_runtime.h"),s.result_spelling,s.name,if parameters.is_empty(){"void"}else{&parameters});
        let root = builder.ast(
            "source-function",
            Some(&s.result_spelling),
            vec![asts[&self.result].clone()],
            declaration,
            (start, start + ir.len() as u64 * 4),
            object([
                ("name", s.name.clone().into()),
                ("return_paths", (self.paths as u64).into()),
                ("result_bits", s.result_bits.into()),
                ("result_signed", s.result_signed.into()),
                ("result_pointer", s.result_pointer.into()),
                (
                    "parameters",
                    array(s.parameters.iter().map(|p| {
                        object([
                            ("name", p.name.clone().into()),
                            ("source_spelling", p.spelling.clone().into()),
                            ("bits", p.bits.into()),
                            ("signed", p.signed.into()),
                            ("pointer", p.pointer.into()),
                            ("abi_gpr", p.register.into()),
                        ])
                    })),
                ),
            ]),
        )?;
        let node=builder.record("metadata","derived",object([("artifact_kind","recovered-source".into()),("source",output.into()),("status","generated-uncompiled".into()),("name",s.name.clone().into()),("ast_root",root.clone().into()),("declaration",s.signature.declaration.clone().into()),("contract","apple-aapcs64-lp64-pure-acyclic-v1".into()),("observables","declared scalar result and ABI nonvolatile GPR/SP restoration; ordinary private frame only; no volatile/fault/source-fidelity/formal-equivalence claim".into())]),array([]))?;
        builder.edge(declaration, &node, "types")?;
        let assumption=builder.record("memory-effect","assumption",object([("contract","ordinary private frame/red-zone storage; single-threaded Apple LP64; no volatile or precise fault-state observables".into()),("red_zone_bytes",128u64.into())]),array([]))?;
        builder.edge(&assumption, &node, "assumes")?;
        for id in ir {
            builder.edge(id, &node, "supports")?;
        }
        Ok((root, node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn attempt(words: &[u32], declaration: &str) -> Result<Recovery, String> {
        let signature = crate::signatures::source_signature(
            "_hs_test",
            &crate::signatures::Signature {
                language: "C",
                declaration: declaration.into(),
                source: "test declaration",
            },
        )
        .ok_or("bad test signature")?;
        let operations = words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                let pc = 0x1000 + i as u64 * 4;
                let operation = decode(*word, pc);
                (operation, lift_operation(operation, pc), *word)
            })
            .collect::<Vec<_>>();
        let trees = operations
            .iter()
            .map(|(operation, portable, word)| {
                instruction_model(*operation, *portable, *word)
                    .ok_or("unmodeled test instruction".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        recover(
            0x1000,
            &trees,
            &operations
                .iter()
                .map(|(_, portable, _)| *portable)
                .collect::<Vec<_>>(),
            signature,
        )
    }
    #[test]
    fn known_parameters_and_widths_not_entry_register_guesses() {
        assert!(attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);"
        )
        .is_ok());
        assert!(attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint32_t hs_test(uint32_t first, uint32_t second);"
        )
        .is_err());
        assert!(attempt(
            &[0x0b010000, 0xd65f03c0],
            "uint32_t hs_test(uint32_t first, uint32_t second);"
        )
        .is_ok());
        assert!(attempt(
            &[0xaa0803e0, 0xd65f03c0],
            "uint64_t hs_test(uint64_t value);"
        )
        .is_err());
    }
    #[test]
    fn source_requires_real_return_and_restored_frame_and_nonvolatiles() {
        let signature = "uint64_t hs_test(uint64_t value);";
        for words in [
            &[0xaa0003f3, 0xd65f03c0][..],
            &[0xd10043ff, 0xd65f03c0],
            &[0x910003e0, 0xd65f03c0],
            &[0xd65f0000],
            &[0xf9400000, 0xd65f03c0],
            &[0x14000000],
        ] {
            assert!(attempt(words, signature).is_err(), "{words:x?}");
        }
    }
    #[test]
    fn overlapping_frame_writes_do_not_reuse_a_stale_wide_value() {
        assert!(attempt(
            &[0xd10043ff, 0xf90003e0, 0xb90007e1, 0xf94003e0, 0x910043ff, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);"
        )
        .is_err());
    }
    #[test]
    fn acyclic_return_paths_are_preserved() {
        let recovery = attempt(
            &[
                0xb4000080, 0x91000400, 0xd65f03c0, 0xd503201f, 0xd1000400, 0xd65f03c0,
            ],
            "uint64_t hs_test(uint64_t value);",
        )
        .unwrap();
        assert_eq!(recovery.paths, 2);
    }
    #[test]
    fn unsafe_and_unsupported_source_declarations_are_not_emitted() {
        for declaration in [
            "void hs_test(void);",
            "uint64_t hs_test(uint64_t first, uint64_t first);",
            "uint64_t hs_test(uint64_t switch);",
            "int; void *hs_test(void);",
            "uint64_t hs_test(double value);",
        ] {
            assert!(
                attempt(&[0xd65f03c0], declaration).is_err(),
                "{declaration}"
            );
        }
    }
}
