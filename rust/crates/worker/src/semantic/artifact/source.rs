//! Evidence-scoped source expressions, not renamed machine-register strings.
//! Pure acyclic functions: symbolic frame objects/copies disappear only after
//! every reachable return proves restored SP, LR and Darwin nonvolatile GPRs.
use super::*;
use crate::signatures::SourceSignature;
use std::collections::{BTreeMap, BTreeSet};

/// Declaration recovery does not depend on proving the implementation body.
pub(super) fn emit_header(builder: &mut Builder, s: &crate::signatures::SourceDeclaration, declaration: &str,
                         start: u64, end: u64, objc_header: Option<&str>) -> io::Result<String> {
    let mut headers = String::from("#include <stdint.h>\n#include <stdbool.h>\n#include <stddef.h>\n#include <sys/types.h>\n#include <objc/objc.h>\n");
    let mentions = |prefix: &str| std::iter::once(s.result_spelling.as_str())
        .chain(s.parameters.iter().map(|parameter| parameter.0.as_str()))
        .any(|ty| ty.split(|c: char| c.is_whitespace() || c == '*').any(|word| word.starts_with(prefix)));
    if mentions("NS") { headers.push_str("/* Foundation SDK declarations require Objective-C or Objective-C++. */\n#if !defined(__OBJC__)\n#error Foundation SDK declarations require Objective-C or Objective-C++\n#endif\n#import <Foundation/Foundation.h>\n"); }
    if mentions("CF") { headers.push_str("#include <CoreFoundation/CoreFoundation.h>\n"); }
    if mentions("CG") { headers.push_str("#include <CoreGraphics/CoreGraphics.h>\n"); }
    if mentions("dispatch_") { headers.push_str("#include <dispatch/dispatch.h>\n"); }
    let parameters = s.parameters.iter()
        .map(|parameter| format!("{} {}", parameter.0.replace("_Bool", "bool"), parameter.1))
        .collect::<Vec<_>>().join(", ");
    let mut text = format!(
        "/* Recovered declaration for {start:#x}..{end:#x}; Apple LP64 ABI.\n * Declaration evidence only: implementation recovery and behavioral equivalence are not implied. */\n#pragma once\n{headers}\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n{} {}({});\n#ifdef __cplusplus\n}}\n#endif\n",
        s.result_spelling.replace("_Bool", "bool"), s.name,
        if parameters.is_empty() { "void" } else { &parameters });
    if let Some(declarations)=objc_header { text.push('\n');text.push_str(declarations); }
    let header = builder.record("metadata", "derived", object([
        ("artifact_kind", "recovered-header".into()), ("source", text.into()),
        ("languages", array(if mentions("NS") || objc_header.is_some() { vec!["objective-c".into(), "objective-c++".into()] }
            else { vec!["c".into(), "c++".into(), "objective-c".into(), "objective-c++".into()] })),
        ("status", "declaration-only-uncompiled".into()), ("name", s.name.clone().into()),
        ("declaration", s.signature.declaration.clone().into()),
        ("contract", if objc_header.is_some(){"exact IMP entry declaration plus target-local class/member review declarations; superclass/protocol/type dependencies unresolved; no complete class ABI or implementation claim"}else{"exact declaration for Apple LP64 and named SDK types; no register-location, implementation or equivalence claim"}.into()),
    ]), array([]))?;
    builder.edge(declaration, &header, "types")?;
    Ok(header)
}

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
    external_loads: Vec<ExternalLoad>,
    external_stores: Vec<ExternalStore>,
    external_access_pcs: Vec<u64>,
    path: usize,
    has_call: bool,
    call_pcs: Vec<u64>,
    calls: Vec<CallResult>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CallResult {
    expression: usize,
    guard: usize,
    pc: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExternalStore {
    address: usize,
    value: usize,
    guard: usize,
    bytes: u64,
    pc: u64,
    volatile: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExternalLoad {
    expression: usize,
    address: usize,
    guard: usize,
    bytes: u64,
    pc: u64,
    volatile: bool,
}
struct Expr {
    /// Proven bit pattern, retained through explicitly modeled width views.
    constant_value: Option<u64>,
    /// Proven value domain {0,1}, independent of source spelling.
    boolean_value: bool,
    boolean_constant: Option<bool>,
    kind: &'static str,
    template: String,
    children: Vec<usize>,
    origins: BTreeSet<u64>,
    /// The deterministic role this expression plays in the recovered source.
    role: &'static str,
    /// The proven source type of this expression, when one is established.
    ///
    /// The machine representation stays `uint64_t`; this records the semantic
    /// type separately so a consumer does not have to read the rendered
    /// expression to learn what a value means. `None` means the type is not
    /// proven and stays unresolved.
    source_type: Option<SourceType>,
}

/// A semantic type proven from a source declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceType {
    /// The declared source spelling, such as `int32_t` or `uint64_t`.
    spelling: String,
    /// The declared width in bits.
    bits: u8,
    /// Whether the declared type is signed.
    signed: bool,
    /// Whether the declared type is a pointer.
    pointer: bool,
}

impl SourceType {
    /// The C spelling used for a value of this type.
    fn spelling(&self) -> &str {
        &self.spelling
    }
}

/// Recover a scalar pointee type only when the declaration names one of the
/// fixed-width or native Apple LP64 integer types understood by this slice. The AArch64
/// load's signed bit alone cannot distinguish `int64_t` from `uint64_t`,
/// because both use the same 64-bit load instruction.
fn scalar_pointee_type(pointer: &SourceType) -> Option<SourceType> {
    if !pointer.pointer { return None; }
    let (base, suffix) = pointer.spelling.rsplit_once('*')?;
    if base.contains('*') || !suffix.split_whitespace().all(|word| matches!(
        word, "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Nonnull" | "_Nullable"
    )) { return None; }
    let base = base.split_whitespace()
        .filter(|word| !matches!(*word, "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Nonnull" | "_Nullable"))
        .collect::<Vec<_>>()
        .join(" ");
    let (bits, signed) = match base.as_str() {
        "int8_t" | "char" | "signed char" => (8, true),
        "uint8_t" | "unsigned char" => (8, false),
        "int16_t" | "short" | "short int" | "signed short" | "signed short int" => (16, true),
        "uint16_t" | "unsigned short" | "unsigned short int" => (16, false),
        "int32_t" | "int" | "signed" | "signed int" => (32, true),
        "uint32_t" | "unsigned" | "unsigned int" | "CFStringEncoding" => (32, false),
        "int64_t" | "long" | "long int" | "signed long" | "signed long int" | "long long" | "long long int" | "signed long long" | "signed long long int"
            | "intptr_t" | "ssize_t" | "ptrdiff_t" | "NSInteger" | "CFIndex" | "dispatch_once_t"
            | "CFNumberType" | "CFURLPathStyle" | "CFPropertyListFormat" => (64, true),
        "uint64_t" | "unsigned long" | "unsigned long int" | "unsigned long long"
            | "unsigned long long int" | "uintptr_t" | "size_t" | "NSUInteger" | "CFTypeID" | "CFOptionFlags" => (64, false),
        "bool" | "_Bool" => (8, false),
        _ => return None,
    };
    Some(SourceType { spelling: base, bits, signed, pointer: false })
}

fn pointee_qualified(pointer: &SourceType, qualifier: &str) -> bool {
    pointer.spelling.split_once('*').is_some_and(|(pointee, _)|
        pointee.split_whitespace().any(|word| word == qualifier))
}

fn volatile_helper(store: bool, bytes: u64, ty: &SourceType) -> (String, String) {
    let canonical = format!("{}{}_t", if ty.signed { "int" } else { "uint" }, bytes * 8);
    // Boolean objects may be inspected through unsigned character storage;
    // other native integers require their actual compatible declared type.
    let spelling = if matches!(ty.spelling.as_str(), "bool" | "_Bool") { "uint8_t".to_owned() } else { ty.spelling.clone() };
    let suffix = if spelling == canonical { String::new() } else { format!("_{}", spelling.split_whitespace().collect::<Vec<_>>().join("_")) };
    (format!("hs_v{}_{}{}{suffix}", if store { "store" } else { "load" }, if ty.signed { "s" } else { "u" }, bytes), spelling)
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
        self.add_role(kind, "value", template, children, pc)
    }
    /// Add an expression that plays a named role in the recovered source.
    fn add_role(
        &mut self,
        kind: &'static str,
        role: &'static str,
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
        let boolean_value = kind == "source-condition"
            || (matches!(kind, "source-copy" | "source-cast" | "source-frame-reload" | "source-return")
                && children.len() == 1 && self.nodes[children[0]].boolean_value)
            || (kind == "source-select" && children.len() == 3
                && self.nodes[children[1]].boolean_value && self.nodes[children[2]].boolean_value);
        self.nodes.push(Expr {
            constant_value: if kind == "source-copy" && children.len() == 1 {
                self.nodes[children[0]].constant_value
            } else { None },
            boolean_value,
            boolean_constant: if matches!(kind, "source-copy" | "source-cast" | "source-frame-reload" | "source-return") && children.len() == 1 {
                self.nodes[children[0]].boolean_constant
            } else { None },
            kind,
            template,
            children,
            origins,
            role,
            source_type: None,
        });
        Ok(id)
    }
    fn constant(&mut self, value: u64, pc: u64) -> Result<usize, String> {
        let id = self.add(
            "source-constant",
            format!("UINT64_C(0x{value:x})"),
            vec![],
            pc,
        )?;
        self.nodes[id].boolean_value = value <= 1;
        self.nodes[id].constant_value = Some(value);
        self.nodes[id].boolean_constant = if value <= 1 { Some(value == 1) } else { None };
        Ok(id)
    }
    /// Record the proven source type of an expression.
    ///
    /// A type is only recorded when the evidence establishes it. An
    /// expression whose type cannot be established keeps no type, so a
    /// consumer sees an unresolved type rather than an assumed one.
    fn type_node(&mut self, id: usize, source_type: Option<SourceType>) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.source_type = source_type;
        }
    }
    /// The proven source type of an expression, when one is recorded.
    fn type_of(&self, id: usize) -> Option<&SourceType> {
        self.nodes.get(id).and_then(|node| node.source_type.as_ref())
    }
    fn literal(&self, symbol: &Symbol) -> Option<u64> {
        let Symbol::Value(id, _) = symbol else {
            return None;
        };
        self.nodes[*id].constant_value
    }
    fn value(&mut self, symbol: Symbol, bits: u8, pc: u64) -> Result<usize, String> {
        match symbol {
            Symbol::Value(id, known) if bits <= known => {
                if bits == 64 {
                    Ok(id)
                } else {
                    let cast = self.add_role("source-cast", "narrowed", format!("((uint{bits}_t)($0))"), vec![id], pc)?;
                    // Preserve a proven source type when this machine-width
                    // view retains every source bit. Truncation changes the
                    // value: a declaration-proven integer then yields the
                    // explicit unsigned C cast type, not its old source type.
                    let narrowed = match self.type_of(id) {
                        Some(source_type) if source_type.bits <= bits => Some(SourceType {
                            spelling: source_type.spelling.clone(),
                            bits: source_type.bits,
                            signed: source_type.signed,
                            pointer: false,
                        }),
                        Some(source_type) if !source_type.pointer => Some(SourceType {
                            spelling: format!("uint{bits}_t"), bits, signed: false, pointer: false,
                        }),
                        _ => None,
                    };
                    self.type_node(cast, narrowed);
                    self.nodes[cast].constant_value = self.nodes[id].constant_value.map(|value| value & ((1u64 << bits) - 1));
                    Ok(cast)
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
fn read(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64) -> Result<Symbol, String> {
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
    // A full-width OR with architectural zero copies the operand bits. If a
    // narrow ABI input leaves upper register bits unqualified, retain the
    // proven low bits so a narrower declared return remains recoverable; later
    // full-width uses still fail their width checks.
    if t.kind == "binary" && attribute(t, "operator") == "|" && t.children.len() == 2 {
        let is_zero = |child: &Tree| {
            (child.kind == "register" && register(child).is_ok_and(|(index, _)| index == 32))
                || (child.kind == "constant" && number(child, "hex") == 0)
        };
        let source = if is_zero(&t.children[0]) {
            Some(&t.children[1])
        } else if is_zero(&t.children[1]) {
            Some(&t.children[0])
        } else {
            None
        };
        if let Some(source) = source {
            if source.kind == "register" {
                let (index, bits) = register(source)?;
                if index < state.registers.len() {
                    if let Symbol::Value(id, known) = state.registers[index].clone() {
                        if bits > known {
                            let source_type = dag.type_of(id).cloned();
                            let copy = dag.add_role("source-copy", "copy", "$0".into(), vec![id], pc)?;
                            dag.type_node(copy, source_type);
                            return Ok(Symbol::Value(copy, known));
                        }
                    }
                }
            }
        }
    }
    let mut child = |i, dag: &mut Dag| read(&t.children[i], state, dag, pc);
    if t.kind == "memory-load" {
        let address = child(0, dag)?;
        let bytes = number(t, "bytes");
        if !matches!(bytes, 1 | 2 | 4 | 8) {
            return Err("external memory read width is unsupported".into());
        }
        if let Symbol::Value(address_id, _) = &address {
            let pointer_type = dag.type_of(*address_id).cloned()
                .filter(|ty| ty.pointer)
                .ok_or_else(|| format!("external memory address is not a proven pointer (address={address:?}, type={:?})", dag.type_of(*address_id)))?;
            let volatile = pointee_qualified(&pointer_type, "volatile");
            let declared_pointee = scalar_pointee_type(&pointer_type).filter(|ty| u64::from(ty.bits) == bytes * 8);
            if volatile && declared_pointee.is_none() {
                return Err("volatile reads require an exact fixed-width scalar pointee".into());
            }
            let signed = declared_pointee.as_ref().map_or(flag(t, "signed") != 0, |ty| ty.signed);
            let load_helper = if volatile {
                volatile_helper(false, bytes, declared_pointee.as_ref().expect("validated volatile pointee")).0
            } else {
                format!("hs_load{bytes}")
            };
            let loaded = dag.add_role(
                "source-memory-load",
                "loaded",
                format!("{load_helper}((uintptr_t)($0))"),
                vec![*address_id],
                pc,
            )?;
            dag.nodes[loaded].source_type = Some(declared_pointee.unwrap_or(SourceType {
                spelling: format!("{}{}_t", if signed { "int" } else { "uint" }, bytes * 8),
                bits: (bytes * 8) as u8,
                signed,
                pointer: false,
            }));
            // Typed Boolean memory is a source-language precondition: the
            // pointed-to live object must have canonical representation.
            dag.nodes[loaded].boolean_value = dag.type_of(loaded)
                .is_some_and(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool"));
            state.external_loads.push(ExternalLoad {
                expression: loaded,
                address: *address_id,
                guard: state.path,
                bytes,
                pc,
                volatile,
            });
            state.external_access_pcs.push(pc);
            let extended = dag.add_role(
                "source-memory-value",
                "loaded",
                format!("hs_extend($0, {}, {})", bytes * 8, flag(t, "signed")),
                vec![loaded],
                pc,
            )?;
            dag.nodes[extended].boolean_value = dag.nodes[loaded].boolean_value;
            let (spelling, bits, promoted_signed) = if bytes < 4 {
                ("int32_t".to_owned(), 32, true)
            } else {
                (format!("{}{}_t", if signed { "int" } else { "uint" }, bytes * 8), (bytes * 8) as u8, signed)
            };
            dag.type_node(extended, Some(SourceType {
                spelling,
                bits,
                signed: promoted_signed,
                pointer: false,
            }));
            return Ok(Symbol::Value(extended, 64));
        }
        let Symbol::Frame(offset) = address else {
            return Err("external or unresolved memory read".into());
        };
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
        let recovered_type = (size == bytes && shift == 0)
            .then(|| dag.type_of(value).cloned())
            .flatten();
        let id = dag.add_role(
            "source-frame-reload",
            "slot",
            format!(
                "hs_extend(($0) >> {shift}, {}, {})",
                bytes * 8,
                flag(t, "signed")
            ),
            vec![value],
            pc,
        )?;
        dag.nodes[id].origins.extend(origins);
        dag.type_node(id, recovered_type);
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
            dag.add_role(
                "source-condition",
                "condition",
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
        if let (Symbol::Value(pointer, _), Symbol::Value(_, _)) = (&values[0], &values[1]) {
            let pointer_type = dag.type_of(*pointer).cloned();
            // Computing an address is not a volatile access. Preserve the
            // declaration here; the eventual load/store validates its width
            // and retains the volatile event in the ordered access schedule.
            if pointer_type.as_ref().is_some_and(|ty| ty.pointer) {
                let op = attribute(t, "operator");
                let (template, children) = if let Some(offset) = dag.literal(&values[1]) {
                    (
                        format!("((uintptr_t)($0) {op} UINT64_C(0x{offset:x}))"),
                        vec![*pointer],
                    )
                } else {
                    let offset_id = match &values[1] {
                        Symbol::Value(id, _) if dag.type_of(*id).is_some_and(|ty| !ty.pointer) => *id,
                        _ => return Err("dynamic pointer offset is not a proven integer".into()),
                    };
                    (
                        format!("((uintptr_t)($0) {op} (uintptr_t)($1))"),
                        vec![*pointer, offset_id],
                    )
                };
                let id = dag.add_role(
                    "source-pointer-offset",
                    "address",
                    template,
                    children,
                    pc,
                )?;
                dag.type_node(id, pointer_type);
                return Ok(Symbol::Value(id, 64));
            }
        }
    }
    if t.kind == "binary" && attribute(t, "operator") == "|" {
        let identity = if dag.literal(&values[0]) == Some(0) {
            Some(values[1].clone())
        } else if dag.literal(&values[1]) == Some(0) {
            Some(values[0].clone())
        } else {
            None
        };
        if let Some(Symbol::Value(id, known)) = identity {
            let source_type = dag.type_of(id).cloned();
            let copy = dag.add_role("source-copy", "copy", "$0".into(), vec![id], pc)?;
            dag.type_node(copy, source_type);
            return Ok(Symbol::Value(copy, known));
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
    // The role names the operation the expression performs. It is derived
    // from the decoded operation, never from the rendered text.
    let role: &'static str = match t.kind {
        "binary" => match attribute(t, "operator") {
            "+" => "sum",
            "-" => "difference",
            "*" => "product",
            "&" => "bit_and",
            "|" => "bit_or",
            "^" => "bit_xor",
            "<<" | ">>" | "asr" | "ror" => "shifted",
            "==" | "!=" => "compared",
            _ => "value",
        },
        "unary" => "negated",
        "extend" => "extended",
        "bit-extract" => "extracted",
        "integer-divide" => "quotient",
        "select" => "selected",
        _ => "value",
    };
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
    let id = dag.add_role("source-expression", role, template, children.clone(), pc)?;
    dag.nodes[id].boolean_value = match t.kind {
        "binary" => match attribute(t, "operator") {
            "==" | "!=" => true,
            "&" => children.iter().any(|child| dag.nodes[*child].boolean_value),
            "|" | "^" => children.iter().all(|child| dag.nodes[*child].boolean_value),
            "+" => children.iter().all(|child| dag.nodes[*child].boolean_value)
                && children.iter().any(|child| dag.nodes[*child].boolean_constant == Some(false)),
            _ => false,
        },
        "select" => children[1..].iter().all(|child| dag.nodes[*child].boolean_value),
        "extend" => dag.nodes[children[0]].boolean_value
            && (flag(t, "signed") == 0 || number(t, "from_bits") > 1),
        "bit-extract" => (number(t, "width") == 1 && flag(t, "signed") == 0)
            || (dag.nodes[children[0]].boolean_value && number(t, "lsb") == 0
                && (flag(t, "signed") == 0 || number(t, "width") > 1)),
        _ => false,
    };
    // Propagate the proven type through the operation. An arithmetic or
    // bitwise result keeps the operand type only when both operands agree on
    // it; a comparison yields a condition rather than a value, and a select
    // keeps the type of its two result arms when they agree.
    let propagated = match t.kind {
        "binary" => {
            let op = attribute(t, "operator");
            if matches!(op, "==" | "!=") {
                None
            } else if matches!(op, "<<" | ">>" | "asr" | "ror") {
                // C shift expressions retain the promoted left operand's
                // integer type; the shift count does not participate in the
                // usual arithmetic conversions. This also keeps declaration
                // evidence attached to compiler-scaled dynamic array indices.
                dag.type_of(children[0]).filter(|ty| !ty.pointer).cloned()
            } else {
                let left = dag.type_of(children[0]);
                let right = dag.type_of(children[1]);
                match (left, right) {
                    (Some(left), Some(right)) if left == right && !left.pointer => {
                        Some(if matches!(left.spelling.as_str(), "bool" | "_Bool") {
                            SourceType { spelling: "int32_t".into(), bits: 32, signed: true, pointer: false }
                        } else { left.clone() })
                    }
                    _ => None,
                }
            }
        }
        "unary" => dag.type_of(children[0]).cloned(),
        "select" => {
            let left = dag.type_of(children[1]);
            let right = dag.type_of(children[2]);
            match (left, right) {
                (Some(left), Some(right)) if left == right => Some(left.clone()),
                _ => None,
            }
        }
        _ => None,
    };
    dag.type_node(id, propagated);
    Ok(Symbol::Value(id, 64))
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
            // A copy carries the source value's proven type unchanged.
            let source_type = dag.type_of(id).cloned();
            let copy = dag.add_role("source-copy", "copy", "$0".into(), vec![id], pc)?;
            dag.type_node(copy, source_type);
            Symbol::Value(copy, known)
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
    let bytes = number(t, "bytes");
    let address = read(&t.children[0], state, dag, pc)?;
    if let Symbol::Value(address, _) = address {
        let pointer = dag
            .type_of(address)
            .cloned()
            .ok_or("external store address has no proven type")?;
        if !pointer.pointer {
            return Err("external store requires a proven pointer".into());
        }
        let volatile = pointee_qualified(&pointer, "volatile");
        if pointee_qualified(&pointer, "const") {
            return Err("external store through a const-qualified pointer is unsupported".into());
        }
        if !matches!(bytes, 1 | 2 | 4 | 8) {
            return Err("external memory write width is unsupported".into());
        }
        let value = read(&t.children[1], state, dag, pc)?;
        let mut value = dag.value(value, 64, pc)?;
        let pointee = scalar_pointee_type(&pointer).filter(|ty| u64::from(ty.bits) == bytes * 8);
        if let Some(boolean) = pointee.as_ref().filter(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool")) {
            if !dag.nodes[value].boolean_value {
                return Err("Boolean store representation is not proven canonical".into());
            }
            value = dag.add_role("source-cast", "narrowed", format!("(({})$0)", boolean.spelling),
                                 vec![value], pc)?;
            dag.type_node(value, Some(boolean.clone()));
        } else if let Some(integer) = pointee.as_ref().filter(|_| {
            dag.literal(&Symbol::Value(value, 64)).is_some()
                || dag.type_of(value).is_some_and(|ty| !ty.pointer && matches!(ty.bits, 8 | 16 | 32 | 64))
        }) {
            // The decoded store proves which low bits reach the exact scalar
            // object. Attach an explicit conversion only for a known constant
            // or a declaration-proven integer, never an unresolved value.
            let role = if dag.literal(&Symbol::Value(value, 64)).is_some() { "store_constant" } else { "store_integer" };
            value = dag.add_role("source-cast", role, format!("(({})$0)", integer.spelling),
                                 vec![value], pc)?;
            dag.type_node(value, Some(integer.clone()));
        }
        if volatile && pointee.is_none() {
            return Err("volatile writes require an exact fixed-width scalar pointee".into());
        }
        if volatile {
            let value_type = dag.type_of(value).cloned()
                .ok_or_else(|| {
                    let expression = &dag.nodes[value];
                    format!("volatile store value type is unresolved at expression {value} ({} / {} / {})",
                        expression.kind, expression.role, expression.template)
                })?;
            if !pointee.as_ref().is_some_and(|pointee| {
                value_type.bits == pointee.bits
                    && value_type.signed == pointee.signed
                    && !value_type.pointer
            }) {
                return Err("volatile store value and pointee types disagree".into());
            }
        }
        state.external_stores.push(ExternalStore {
            address,
            value,
            guard: state.path,
            bytes,
            pc,
            volatile,
        });
        state.external_access_pcs.push(pc);
        return Ok(());
    }
    let Symbol::Frame(offset) = address else {
        return Err("external or escaped memory write".into());
    };
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

/// A proven frame object and the memory version it currently holds.
///
/// The frame is a private stack region, so its objects are partitioned by
/// their exact slot extent: two objects that overlap are the same memory
/// location, and two that do not are distinct. The version is the store
/// that last wrote the object, so a consumer can see which write a load
/// reads without reparsing the recovered expression.
#[derive(Clone, Debug)]
struct FrameObject {
    /// The frame-relative offset of the object.
    offset: i64,
    /// The object's extent in bytes.
    bytes: u64,
    /// The instruction that last wrote the object.
    version: u64,
}

/// Collect the frame objects a path state proves, merged across every path.
///
/// Two objects at the same extent are one memory location and keep the
/// later version. Objects at different extents stay separate, so a partial
/// write never claims to replace a neighbouring slot.
fn frame_objects(state: &State) -> BTreeMap<(i64, u64), u64> {
    state
        .frame
        .iter()
        .map(|(&(offset, bytes), (_, origins))| {
            let version = origins.iter().copied().max().unwrap_or(0);
            ((offset, bytes), version)
        })
        .collect()
}

fn zero_test(t: &Tree) -> Option<(usize, bool)> {
    if t.kind != "binary" || !matches!(attribute(t, "operator"), "==" | "!=") || t.children.len() != 2 {
        return None;
    }
    let (register_tree, constant_tree) = if t.children[0].kind == "register" {
        (&t.children[0], &t.children[1])
    } else {
        (&t.children[1], &t.children[0])
    };
    if constant_tree.kind != "constant" || number(constant_tree, "hex") != 0 {
        return None;
    }
    let (register, _) = register(register_tree).ok()?;
    Some((register, attribute(t, "operator") == "=="))
}

struct LoopVariable {
    value: usize,
    initial: usize,
}

#[derive(Clone, Debug)]
struct LoopAssignment {
    destination: usize,
    value: usize,
    pc: u64,
}

#[derive(Clone, Debug)]
enum LoopBodyEvent {
    Assignment(LoopAssignment),
    Load(ExternalLoad),
    Store(ExternalStore),
}

struct SourceLoop {
    condition: usize,
    variables: Vec<LoopVariable>,
    assignments: Vec<LoopAssignment>,
    loads: Vec<ExternalLoad>,
    stores: Vec<ExternalStore>,
    pcs: Vec<u64>,
}

/// Recognize a natural loop with one zero-test preheader, one complementary
/// zero-test backedge, and a body with straight-line scalar assignments plus
/// ordered external stores. Values carried
/// through the backedge become mutable, named source locals; every update is
/// still built from the original instruction tree.
fn natural_loop(
    start: u64,
    preheader: usize,
    trees: &[Tree],
    state: &mut State,
    dag: &mut Dag,
    call_prototypes: &BTreeMap<u64, SourceSignature>,
) -> Result<Option<(SourceLoop, usize)>, String> {
    let Some(preheader_tree) = trees.get(preheader).filter(|tree| tree.kind == "conditional-goto") else {
        return Ok(None);
    };
    let header = preheader + 1;
    let target_index = |target: u64| -> Option<usize> {
        let delta = target.checked_sub(start)?;
        (delta % 4 == 0 && delta / 4 < trees.len() as u64).then_some((delta / 4) as usize)
    };
    let Some(exit) = target_index(number(preheader_tree, "target")) else { return Ok(None) };
    let backedges = (header..trees.len()).filter(|&index| {
        trees[index].kind == "conditional-goto"
            && target_index(number(&trees[index], "target")) == Some(header)
    }).collect::<Vec<_>>();
    if backedges.len() != 1 { return Ok(None); }
    let tail = backedges[0];
    if tail <= header || tail + 1 != exit { return Ok(None); }
    let Some((test_register, exits_on_zero)) = zero_test(&preheader_tree.children[0]) else { return Ok(None) };
    let Some((tail_register, continues_on_zero)) = zero_test(&trees[tail].children[0]) else { return Ok(None) };
    if test_register != tail_register || exits_on_zero == continues_on_zero {
        return Ok(None);
    }
    // The loop must be a single-entry region. The preheader exit and backedge
    // are its only control-flow edges; all other body operations are assignments.
    for (index, tree) in trees.iter().enumerate() {
        if !matches!(tree.kind, "goto" | "conditional-goto") || index == preheader || index == tail { continue; }
        if target_index(number(tree, "target")).is_some_and(|target| (header..=tail).contains(&target)) {
            return Ok(None);
        }
    }
    if !state.external_loads.is_empty() || !state.external_stores.is_empty() || state.has_call {
        return Ok(None);
    }
    let mut written = BTreeSet::new();
    for tree in &trees[header..tail] {
        if !matches!(tree.kind, "assignment" | "memory-store") || tree.children.len() != 2 { return Ok(None); }
        if tree.kind == "memory-store" { continue; }
        let (register, _) = match register(&tree.children[0]) { Ok(value) => value, Err(_) => return Ok(None) };
        // x18 through x30, SP and the zero register cannot become loop locals
        // in this source slice because their ABI restoration rules differ.
        if register > 17 { return Ok(None); }
        written.insert(register);
    }
    if !written.contains(&test_register) { return Ok(None); }

    let mut variables = Vec::new();
    let mut variable_by_register = BTreeMap::new();
    for register in written.iter().copied() {
        let Symbol::Value(initial, known) = state.registers[register].clone() else { return Ok(None) };
        if known < 32 { return Ok(None); }
        let role = if register == test_register { "induction" } else { "accumulator" };
        let id = dag.nodes.len();
        let name = format!("hs_{role}_{id}");
        let value = dag.add_role("source-loop-variable", role, name, vec![], start + header as u64 * 4)?;
        // Inductive {0,1} invariant: seed from the preheader, then require
        // every write in the modeled body to preserve the same domain.
        // Never copy a constant fact into a loop-carried variable.
        dag.nodes[value].boolean_value = dag.nodes[initial].boolean_value
            && dag.type_of(initial).is_some_and(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool"));
        dag.type_node(value, dag.type_of(initial).cloned());
        variable_by_register.insert(register, (value, known));
        variables.push(LoopVariable { value, initial });
        state.registers[register] = Symbol::Value(value, known);
    }
    let test_value = variable_by_register[&test_register].0;
    let condition_template = if exits_on_zero { "(($0) != UINT64_C(0))" } else { "(($0) == UINT64_C(0))" };
    let condition = dag.add_role(
        "source-loop-condition", "condition", condition_template.into(), vec![test_value], start + header as u64 * 4
    )?;
    dag.type_node(condition, None);

    let mut assignments = Vec::new();
    let mut loads = Vec::new();
    let mut stores = Vec::new();
    let mut type_conflicts = BTreeSet::new();
    for (index, tree) in trees.iter().enumerate().take(tail).skip(header) {
        let pc = start + index as u64 * 4;
        let register = if tree.kind == "assignment" {
            Some(register(&tree.children[0])?.0)
        } else {
            None
        };
        let before_flags = state.flags;
        let before_frame = state.frame.clone();
        let before_loads = state.external_loads.len();
        let before_stores = state.external_stores.len();
        statement(tree, state, dag, pc, call_prototypes)?;
        if state.flags != before_flags { return Err("loop body changes flags without a loop-carried flag value".into()); }
        if state.frame != before_frame || state.external_loads.len() < before_loads {
            return Err("loop body has memory effects without loop-carried memory SSA".into());
        }
        let body_loads = state.external_loads.split_off(before_loads);
        if body_loads.len() > 1 || (tree.kind == "memory-store" && !body_loads.is_empty()) {
            return Err("loop instruction has an unsupported number of memory reads".into());
        }
        loads.extend(body_loads);
        match register {
            Some(register) => {
                if state.external_stores.len() != before_stores {
                    return Err("loop assignment unexpectedly produced a memory store".into());
                }
                let Symbol::Value(value, _) = state.registers[register].clone() else { return Err("loop update depends on unknown bits".into()) };
                let (destination, known) = variable_by_register[&register];
                if dag.nodes[destination].boolean_value && !dag.nodes[value].boolean_value {
                    return Err("loop update does not preserve its canonical Boolean domain".into());
                }
                let destination_type = dag.type_of(destination).cloned();
                let update_type = dag.type_of(value).cloned();
                match (destination_type, update_type) {
                    (Some(current), _) if matches!(current.spelling.as_str(), "bool" | "_Bool")
                        && dag.nodes[value].boolean_value => {}
                    (None, Some(update_type)) => dag.type_node(destination, Some(update_type)),
                    (Some(current), Some(update)) if current.bits == update.bits && current.signed == update.signed && current.pointer == update.pointer => {}
                    (Some(_), None) => {}
                    (None, None) => {}
                    _ => {
                        dag.type_node(destination, None);
                        type_conflicts.insert(destination);
                    }
                }
                assignments.push(LoopAssignment { destination, value, pc });
                state.registers[register] = Symbol::Value(destination, known);
            }
            None => {
                if state.external_stores.len() != before_stores + 1 {
                    return Err("loop memory store is not a single external write".into());
                }
                stores.push(state.external_stores.pop().expect("store count checked"));
            }
        }
    }
    if !type_conflicts.is_empty() {
        return Err("loop-carried value type changes or becomes unresolved".into());
    }
    let pcs = (preheader..=tail).map(|index| start + index as u64 * 4).collect();
    Ok(Some((SourceLoop { condition, variables, assignments, loads, stores, pcs }, exit)))
}
fn statement(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64, call_prototypes: &BTreeMap<u64, SourceSignature>) -> Result<(), String> {
    match t.kind {
        "no-op" => Ok(()),
        "assignment" => {
            let value = read(&t.children[1], state, dag, pc)?;
            write(&t.children[0], value, state, dag, pc)
        }
        "sequence" => {
            for child in &t.children {
                statement(child, state, dag, pc, call_prototypes)?;
            }
            Ok(())
        }
        "memory-store" => store(t, state, dag, pc),
        "call" => {
            let target = number(t, "target");
            let callee = call_prototypes.get(&target).ok_or("call has no unique exact scalar prototype")?;
            let V::Object(effects) = t.attributes.get("call_effects").ok_or("self-call prototype missing")? else { return Err("self-call prototype missing".into()); };
            let V::Array(arguments) = effects.get("arguments").ok_or("self-call arguments unresolved")? else { return Err("self-call arguments unresolved".into()); };
            if arguments.len() != callee.parameters.len() { return Err("call argument prototype mismatch".into()); }
            let mut children = Vec::new();
            let mut rendered = Vec::new();
            for (argument, parameter) in arguments.iter().zip(&callee.parameters) {
                let V::Object(fields) = argument else { return Err("self-call argument proof malformed".into()); };
                if fields.get("source_type") != Some(&V::String(parameter.spelling.clone()))
                    || fields.get("location") != Some(&V::String(format!("x{}", parameter.register))) {
                return Err("call argument ABI mismatch".into());
                }
                let value = dag.value(state.registers[parameter.register as usize].clone(), parameter.bits.max(32), pc)?;
                if matches!(parameter.spelling.as_str(), "bool" | "_Bool") && !dag.nodes[value].boolean_value {
                    return Err("Boolean call argument representation is not proven canonical".into());
                }
                let cast = dag.add_role("source-call-argument", "argument", format!("(({})$0)", parameter.spelling), vec![value], pc)?;
                children.push(cast);
                rendered.push(format!("${}", children.len()-1));
            }
            let result_location = if callee.result_bits == 0 { V::Null } else { "x0".into() };
            if effects.get("result") != Some(&object([("source_spelling", callee.result_spelling.clone().into()), ("location", result_location)])) {
                return Err("call result ABI mismatch".into());
            }
            let value = dag.add_role("source-call", "value", format!("{}({})", callee.name, rendered.join(", ")), children, pc)?;
            if matches!(callee.result_spelling.as_str(), "bool" | "_Bool") {
                if effects.get("canonical_boolean_result") != Some(&V::Bool(true)) {
                    return Err("callee Boolean return representation is not proven canonical".into());
                }
                dag.nodes[value].boolean_value = true;
            }
            dag.type_node(value, Some(SourceType { spelling: callee.result_spelling.clone(), bits: callee.result_bits, signed: callee.result_signed, pointer: callee.result_pointer }));
            for register in 0..=18 { state.registers[register] = Symbol::Unknown(register as u64); }
            // The call's result width belongs to the callee prototype. The
            // caller may return a narrower or wider value after further
            // operations; using its result declaration here loses valid
            // callee bits or claims bits the callee did not define.
            if callee.result_bits != 0 {
                state.registers[0] = Symbol::Value(value, callee.result_bits.max(32));
            }
            state.flags = None;
            state.has_call = true;
            state.call_pcs.push(pc);
            state.calls.push(CallResult { expression: value, guard: state.path, pc });
            Ok(())
        }
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
                    state.flags = Some(dag.add_role(
                        "source-nzcv",
                        "nzcv",
                        format!("hs_compare($0, $1, {}, {kind})", number(t, "bits")),
                        vec![a, b],
                        pc,
                    )?);
                }
                _ => state.flags = None,
            }
            Ok(())
        }
        "flags-arithmetic" => {
            let a = read(&t.children[0], state, dag, pc)?;
            let b = read(&t.children[1], state, dag, pc)?;
            match (a, b) {
                (Symbol::Value(a, _), Symbol::Value(b, _)) => {
                    let bits = number(t, "bits");
                    let is_sub = match attribute(t, "operator") {
                        "add" => 0,
                        "sub" => 1,
                        _ => return Err("unqualified flag write".into()),
                    };
                    let op = if is_sub == 0 { "+" } else { "-" };
                    state.flags = Some(dag.add_role(
                        "source-nzcv",
                        "nzcv",
                        format!("hs_flags_arith($0, $1, (($0 {op} $1) & hs_mask({bits})), {bits}, {is_sub})"),
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
                            let id = dag.add_role(
                                "source-frame-reload",
                                "slot",
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
/// A deterministic role name for a recovered-source local.
///
/// The name states what the expression is, not what it was named in the
/// original source. The original spelling is not recoverable from the
/// machine code, so a role description keeps the recovered source readable
/// without inventing an identifier the evidence does not support. The numeric
/// suffix keeps two roles of the same kind distinct.
fn role_name(node: &Expr, id: usize) -> String {
    format!("hs_{}_{id}", node.role)
}

pub(super) struct Recovery {
    dag: Dag,
    result: usize,
    signature: SourceSignature,
    paths: usize,
    /// The proven frame objects and the memory version each one holds.
    frame_objects: Vec<FrameObject>,
    external_loads: Vec<ExternalLoad>,
    external_stores: Vec<ExternalStore>,
    called_prototypes: Vec<SourceSignature>,
    calls: Vec<CallResult>,
    ordered_call_memory: bool,
    ordered_volatile_accesses: bool,
    loops: Vec<SourceLoop>,
}
pub(super) fn recover(
    start: u64,
    trees: &[Tree],
    operations: &[IrOperation],
    signature: SourceSignature,
    call_prototypes: &BTreeMap<u64, SourceSignature>,
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
        external_loads: Vec::new(),
        external_stores: Vec::new(),
        external_access_pcs: Vec::new(),
        path: truth,
        has_call: false,
        call_pcs: Vec::new(),
        calls: Vec::new(),
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
                "hs_flags_arith",
                "hs_load1",
                "hs_load2",
                "hs_load4",
                "hs_load8",
                "hs_store1",
                "hs_store2",
                "hs_store4",
                "hs_store8",
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
        let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
        // The parameter's declared type is proven by the exact source
        // declaration, so the machine value and the semantic type are known
        // separately for this expression.
        dag.type_node(
            id,
            Some(SourceType {
                spelling: parameter.spelling.clone(),
                bits,
                signed: parameter.signed,
                pointer: parameter.pointer,
            }),
        );
        if matches!(parameter.spelling.as_str(), "bool" | "_Bool") {
            dag.nodes[id].boolean_value = true;
        }
        state.registers[parameter.register as usize] =
            Symbol::Value(id, if bits < 64 { 32 } else { 64 });
    }
    let mut pending = vec![(0usize, state, BTreeSet::new())];
    let mut returned = Vec::new();
    let mut source_loops = Vec::new();
    let mut visits = 0;
    // The proven frame objects, keyed by their exact extent. A later write to
    // the same extent replaces the earlier version; a write to a different
    // extent adds an object without disturbing its neighbour.
    let mut objects: BTreeMap<(i64, u64), u64> = BTreeMap::new();
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
                    if !source_loops.is_empty()
                        && (state.has_call || !state.external_loads.is_empty() || !state.external_stores.is_empty())
                    {
                        return Err("source loop combined with calls or memory effects is not modeled".into());
                    }
                    if state.external_access_pcs.windows(2).any(|pair| pair[0] >= pair[1]) {
                        return Err("external memory schedule is not in instruction order".into());
                    }
                    if t.children.len() != 1
                        || read(&t.children[0], &mut state, &mut dag, pc)? != Symbol::Unknown(30)
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
                    // A void declaration establishes that x0 is not a source
                    // result. It may be unknown or still contain a pointer.
                    let value = if signature.result_bits == 0 {
                        dag.constant(0, pc)?
                    } else { dag.value(
                        state.registers[0].clone(),
                        signature.result_bits,
                        pc,
                    )? };
                    if matches!(signature.result_spelling.as_str(), "bool" | "_Bool")
                        && !dag.nodes[value].boolean_value {
                        return Err("Boolean return representation is not proven canonical".into());
                    }
                    let value = dag.add_role("source-return", "result", "$0".into(), vec![value], pc)?;
                    // The returned value has the function's declared result
                    // type, which the exact source declaration proves.
                    dag.type_node(
                        value,
                        Some(SourceType {
                            spelling: signature.result_spelling.clone(),
                            bits: signature.result_bits,
                            signed: signature.result_signed,
                            pointer: signature.result_pointer,
                        }),
                    );
                    returned.push((state.path, value, state.external_loads.clone(), state.external_stores.clone(), state.has_call, state.call_pcs.clone(), state.calls.clone()));
                    // The frame objects this path proves join the shared
                    // object map. The same extent keeps the later version.
                    for ((offset, bytes), version) in frame_objects(&state) {
                        let entry = objects.entry((offset, bytes)).or_insert(version);
                        if *entry < version {
                            *entry = version;
                        }
                    }
                    break;
                }
                "goto" | "conditional-goto" => {
                    if t.kind == "conditional-goto" {
                        if let Some((source_loop, exit)) = natural_loop(
                            start, index, trees, &mut state, &mut dag, call_prototypes
                        )? {
                            for body_index in index + 1..exit { seen.insert(body_index); }
                            source_loops.push(source_loop);
                            index = exit;
                            continue;
                        }
                    }
                    let target = number(t, "target");
                    let delta = target.checked_sub(start).ok_or("source tail dependency")?;
                    if delta % 4 != 0 || delta / 4 >= trees.len() as u64 {
                        return Err("source branch outside function".into());
                    }
                    let target_index = (delta / 4) as usize;
                    if t.kind == "goto" {
                        index = target_index;
                        continue;
                    }
                    let condition = read(&t.children[0], &mut state, &mut dag, pc)?;
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
                    pending.push((target_index, taken, seen.clone()));
                }
                _ => statement(t, &mut state, &mut dag, pc, call_prototypes)?,
            }
            index += 1;
        }
    }
    let paths = returned.len();
    if !source_loops.is_empty() && paths != 1 {
        return Err("source loop requires one proven exit path".into());
    }
    // Calls are execution events regardless of result liveness. Materialize
    // each result once so expression reuse cannot duplicate an invocation,
    // and dead results cannot erase a call from the executed path.
    let ordered_call_memory = returned.iter().any(|path| !path.6.is_empty());
    let ordered_volatile_accesses = returned.iter().any(|(_, _, loads, stores, has_call, _, _)| {
        let volatile = loads.iter().any(|load| load.volatile) || stores.iter().any(|store| store.volatile);
        volatile && (*has_call || loads.len() + stores.len() > 1)
    });
    let Some((_, mut result, mut external_loads, mut external_stores, _, _, mut calls)) = returned.pop() else {
        return Err("no qualified source return".into());
    };
    while let Some((condition, value, path_loads, path_stores, _, _, path_calls)) = returned.pop() {
        for access in path_loads {
            if !external_loads.contains(&access) {
                external_loads.push(access);
            }
        }
        for effect in path_stores {
            if !external_stores.contains(&effect) {
                external_stores.push(effect);
            }
        }
        for call in path_calls {
            if !calls.contains(&call) { calls.push(call); }
        }
        // A join of two return paths keeps the type only when both paths
        // agree on it; otherwise the merged value stays untyped.
        let joined = match (dag.type_of(value), dag.type_of(result)) {
            (Some(left), Some(right)) if left == right => Some(left.clone()),
            _ => None,
        };
        let select = dag.add_role(
            "source-select",
            "selected",
            "(($0) ? ($1) : ($2))".into(),
            vec![condition, value, result],
            start,
        )?;
        dag.type_node(select, joined);
        result = select;
    }
    let mut called_prototypes = Vec::new();
    for target in operations.iter().filter_map(|operation| match operation {
        IrOperation::DirectCall { target } if *target != start => Some(*target),
        _ => None,
    }) {
        if let Some(prototype) = call_prototypes.get(&target) {
            if !called_prototypes.iter().any(|existing: &SourceSignature| existing.signature.declaration == prototype.signature.declaration) {
                called_prototypes.push(prototype.clone());
            }
        }
    }
    Ok(Recovery {
        dag,
        result,
        signature,
        paths,
        external_loads,
        external_stores,
        called_prototypes,
        calls,
        ordered_call_memory,
        ordered_volatile_accesses,
        loops: source_loops,
        frame_objects: objects
            .into_iter()
            .map(|((offset, bytes), version)| FrameObject {
                offset,
                bytes,
                version,
            })
            .collect(),
    })
}

/// Reuse the source walker to prove every reachable return of a bounded,
/// memory-free leaf has canonical Boolean representation. Callers separately
/// establish the callee's complete return/effect/ABI summary.
pub(super) fn canonical_boolean_callee(code: &[u8], start: u64, signature: &SourceSignature) -> bool {
    if code.is_empty() || code.len() > 128 * 4 || code.len() % 4 != 0 {
        return false;
    }
    let mut trees = Vec::new();
    let mut operations = Vec::new();
    for (index, bytes) in code.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        let pc = start + index as u64 * 4;
        let decoded = decode(word, pc);
        let operation = lift_operation(decoded, pc);
        trees.push(instruction_model(decoded, operation, word).unwrap_or_else(|| {
            tree("unsupported-operation", None, object([]), vec![])
        }));
        operations.push(operation);
    }
    recover(start, &trees, &operations, signature.clone(), &BTreeMap::new()).is_ok()
}
impl Recovery {
    pub(super) fn emit(
        &self,
        builder: &mut Builder,
        declaration: &str,
        ir: &[String],
        memory_evidence_by_pc: &BTreeMap<u64, Vec<String>>,
        start: u64,
        objc_method: Option<(&str,&str)>,
    ) -> io::Result<(String, String)> {
        let ordered_mixed_accesses = !self.external_loads.is_empty() && !self.external_stores.is_empty();
        let loop_memory_effects = self.loops.iter().any(|source_loop| !source_loop.loads.is_empty() || !source_loop.stores.is_empty());
        let ordered_effect_schedule = ordered_mixed_accesses || self.ordered_call_memory || self.ordered_volatile_accesses || loop_memory_effects;
        let mut live = BTreeSet::new();
        let mut pending = vec![self.result];
        for source_loop in &self.loops {
            pending.push(source_loop.condition);
            for variable in &source_loop.variables {
                pending.extend([variable.value, variable.initial]);
            }
            for assignment in &source_loop.assignments {
                pending.extend([assignment.destination, assignment.value]);
            }
            for load in &source_loop.loads {
                pending.extend([load.expression, load.address, load.guard]);
            }
            for store in &source_loop.stores {
                pending.extend([store.address, store.value, store.guard]);
            }
        }
        while let Some(id) = pending.pop() {
            if live.insert(id) {
                pending.extend(self.dag.nodes[id].children.iter().copied());
            }
        }
        let mut loop_local_ids = BTreeSet::new();
        let mut loop_pending = self.loops.iter().flat_map(|source_loop| {
            std::iter::once(source_loop.condition)
                .chain(source_loop.variables.iter().map(|variable| variable.value))
                .chain(source_loop.assignments.iter().map(|assignment| assignment.value))
                .chain(source_loop.loads.iter().flat_map(|load| [load.expression, load.address, load.guard]))
                .chain(source_loop.stores.iter().flat_map(|store| [store.address, store.value, store.guard]))
        }).collect::<Vec<_>>();
        while let Some(id) = loop_pending.pop() {
            if loop_local_ids.insert(id) {
                loop_pending.extend(self.dag.nodes[id].children.iter().copied());
            }
        }
        for effect in &self.external_stores {
            pending.extend([effect.address, effect.value, effect.guard]);
        }
        if ordered_effect_schedule {
            pending.extend(self.external_loads.iter().flat_map(|load| [load.expression, load.guard]));
        }
        if self.ordered_call_memory {
            pending.extend(self.calls.iter().flat_map(|call| [call.expression, call.guard]));
        }
        while let Some(id) = pending.pop() {
            if live.insert(id) {
                pending.extend(self.dag.nodes[id].children.iter().copied());
            }
        }
        let mut uses = vec![0usize; self.dag.nodes.len()];
        let mut call_dependent = vec![false; self.dag.nodes.len()];
        for &id in &live {
            call_dependent[id] = self.dag.nodes[id].kind == "source-call"
                || self.dag.nodes[id].children.iter().any(|child| call_dependent[*child]);
            for child in &self.dag.nodes[id].children {
                uses[*child] += 1;
            }
        }
        let mut rendered = BTreeMap::<usize, String>::new();
        let mut asts = BTreeMap::<usize, String>::new();
        let mut call_invocations = BTreeMap::<usize, String>::new();
        let mut declarations = String::new();
        let ordered_load_ids = if ordered_effect_schedule {
            self.external_loads.iter().map(|load| load.expression)
                .chain(self.loops.iter().flat_map(|source_loop| source_loop.loads.iter().map(|load| load.expression)))
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        for &id in &live {
            let node = &self.dag.nodes[id];
            let mut expression = if ordered_load_ids.contains(&id) { format!("hs_loaded_{id}") } else { node.template.clone() };
            for (index, child) in node.children.iter().enumerate() {
                expression = expression.replace(&format!("${index}"), &rendered[child]);
            }
            if (self.ordered_call_memory || self.ordered_volatile_accesses) && node.kind == "source-call" {
                let name = format!("hs_call_result_{id}");
                call_invocations.insert(id, expression);
                let spelling = node.source_type.as_ref().ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData, "ordered call has no proven result type",
                ))?.spelling();
                if spelling == "void" {
                    // This node is an execution event with no source value.
                    expression = String::new();
                } else {
                    declarations.push_str(&format!("    {spelling} {name};\n"));
                    expression = name;
                }
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
            let machine_type = if node.kind == "source-call" && node.source_type.as_ref().is_some_and(|ty| ty.bits == 0) {
                "void"
            } else { "uint64_t" };
            let ast = builder.ast_spans(
                node.kind,
                // The node type stays the machine representation. The proven
                // semantic type travels in its own attribute so a consumer
                // never has to read the rendered expression to learn it.
                Some(machine_type),
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
                    ("machine_type", machine_type.into()),
                    ("canonical_boolean_proven", node.boolean_value.into()),
                    (
                        "source_type",
                        match &node.source_type {
                            Some(source_type) => object([
                                ("spelling", source_type.spelling().into()),
                                ("bits", source_type.bits.into()),
                                ("signed", source_type.signed.into()),
                                ("pointer", source_type.pointer.into()),
                            ]),
                            None => V::String("unresolved".into()),
                        },
                    ),
                ]),
            )?;
            asts.insert(id, ast);
            if !ordered_effect_schedule && !call_dependent[id] && !loop_local_ids.contains(&id) && (expression.len() > 160
                || (uses[id] > 1 && !matches!(node.kind, "source-constant" | "source-parameter"))
            ) {
                let name = role_name(node, id);
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
        let mut headers=String::from("#include <stdint.h>\n#include <stdbool.h>\n#include <stddef.h>\n#include <string.h>\n#include <sys/types.h>\n#include <objc/objc.h>\n");
        let boolean_memory_reads = self.external_loads.iter()
            .chain(self.loops.iter().flat_map(|source_loop| source_loop.loads.iter()))
            .any(|load| self.dag.type_of(load.expression)
                .is_some_and(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool")));
        if boolean_memory_reads {
            headers.push_str("/* Boolean pointer reads require live Boolean objects with canonical 0/1 representation. */\n");
        }
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
        let call_declarations = self.called_prototypes.iter()
            .map(|prototype| format!("{}\n", prototype.signature.declaration))
            .collect::<String>();
        let result = &rendered[&self.result];
        let mut effects_code = String::new();
        let mut effect_asts = Vec::new();
        let mut loop_code = String::new();
        let mut memory_records = Vec::<(V, V)>::new();
        for source_loop in &self.loops {
            let loop_pc = *source_loop.pcs.first().unwrap_or(&start);
            let mut loop_children = vec![asts[&source_loop.condition].clone()];
            for load in &source_loop.loads {
                declarations.push_str(&format!("    uint64_t hs_loaded_{};\n", load.expression));
            }
            for variable in &source_loop.variables {
                let name = &rendered[&variable.value];
                loop_code.push_str(&format!("    uint64_t {name} = {};\n", rendered[&variable.initial]));
                let binding = builder.ast_spans(
                    "source-loop-binding",
                    None,
                    vec![asts[&variable.value].clone(), asts[&variable.initial].clone()],
                    &ir.get(((loop_pc - start) / 4) as usize).into_iter().cloned().chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>(),
                    array([object([("start", address(loop_pc)), ("end", address(loop_pc + 4))])]),
                    object([("template", "uint64_t $0 = $1".into()), ("role", self.dag.nodes[variable.value].role.into())]),
                )?;
                loop_children.push(binding);
            }
            loop_code.push_str(&format!("    while ({}) {{\n", rendered[&source_loop.condition]));
            let mut body_events = source_loop.loads.iter().cloned().map(LoopBodyEvent::Load)
                .chain(source_loop.assignments.iter().cloned().map(LoopBodyEvent::Assignment))
                .chain(source_loop.stores.iter().cloned().map(LoopBodyEvent::Store)).collect::<Vec<_>>();
            body_events.sort_by_key(|event| match event {
                LoopBodyEvent::Load(load) => (load.pc, 0u8),
                LoopBodyEvent::Assignment(assignment) => (assignment.pc, 1u8),
                LoopBodyEvent::Store(store) => (store.pc, 2u8),
            });
            for event in body_events {
                match event {
                    LoopBodyEvent::Load(load) => {
                        let helper = if load.volatile {
                            volatile_helper(false, load.bytes, self.dag.type_of(load.expression).expect("validated volatile pointee")).0
                        } else {
                            format!("hs_load{}", load.bytes)
                        };
                        let statement = format!("        hs_loaded_{} = {}((uintptr_t)({}));\n",
                            load.expression, helper, rendered[&load.address]);
                        if self.dag.literal(&Symbol::Value(load.guard, 64)) == Some(1) {
                            loop_code.push_str(&statement);
                        } else {
                            loop_code.push_str(&format!("        if ({}) {{\n{}        }}\n", rendered[&load.guard], statement));
                        }
                        let mut evidence = ir.get(((load.pc - start) / 4) as usize).into_iter().cloned()
                            .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
                        evidence.extend(memory_evidence_by_pc.get(&load.pc).into_iter().flatten().cloned());
                        let ordering = "per-iteration instruction order under the recovered path guard";
                        let ast = builder.ast_spans(
                            "source-memory-read",
                            None,
                            vec![asts[&load.address].clone(), asts[&load.expression].clone(), asts[&load.guard].clone()],
                            &evidence,
                            array([object([("start", address(load.pc)), ("end", address(load.pc + 4))])]),
                            object([
                                ("bytes", load.bytes.into()),
                                ("partition", if load.volatile { "declaration-proven volatile pointer" } else { "declaration-proven ordinary pointer" }.into()),
                                ("ordering", ordering.into()),
                                ("template", format!("if ($2) {helper}((uintptr_t)($0))").into()),
                            ]),
                        )?;
                        loop_children.push(ast);
                        memory_records.push((object([
                            ("artifact_kind", "external-memory-read".into()),
                            ("partition", if load.volatile { "declaration-proven volatile pointer" } else { "declaration-proven ordinary pointer" }.into()),
                            ("bytes", load.bytes.into()),
                            ("ordering", ordering.into()),
                            ("guard_ast", asts[&load.guard].clone().into()),
                            ("address_ast", asts[&load.address].clone().into()),
                            ("read_ast", asts[&load.expression].clone().into()),
                        ]), ranges(load.pc, load.pc + 4)));
                    }
                    LoopBodyEvent::Assignment(assignment) => {
                        let target = &rendered[&assignment.destination];
                        let value = &rendered[&assignment.value];
                        loop_code.push_str(&format!("        {target} = {value};\n"));
                        let evidence = ir.get(((assignment.pc - start) / 4) as usize).into_iter().cloned()
                            .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
                        let ast = builder.ast_spans(
                            "source-loop-assignment",
                            None,
                            vec![asts[&assignment.destination].clone(), asts[&assignment.value].clone()],
                            &evidence,
                            array([object([("start", address(assignment.pc)), ("end", address(assignment.pc + 4))])]),
                            object([("template", "$0 = $1".into())]),
                        )?;
                        loop_children.push(ast);
                    }
                    LoopBodyEvent::Store(store) => {
                        let (helper, partition) = if store.volatile {
                            let pointee = scalar_pointee_type(self.dag.type_of(store.address).expect("validated volatile pointer type"))
                                .filter(|ty| u64::from(ty.bits) == store.bytes * 8)
                                .expect("validated volatile pointee type");
                            (volatile_helper(true, store.bytes, &pointee).0,
                             "declaration-proven volatile pointer")
                        } else {
                            (format!("hs_store{}", store.bytes), "declaration-proven ordinary pointer")
                        };
                        let statement = format!("        {}((uintptr_t)({}), (uint64_t)({}));\n", helper,
                            rendered[&store.address], rendered[&store.value]);
                        if self.dag.literal(&Symbol::Value(store.guard, 64)) == Some(1) {
                            loop_code.push_str(&statement);
                        } else {
                            loop_code.push_str(&format!("        if ({}) {{\n{}        }}\n", rendered[&store.guard], statement));
                        }
                        let mut evidence = ir.get(((store.pc - start) / 4) as usize).into_iter().cloned()
                            .chain(std::iter::once(declaration.to_owned()))
                            .collect::<Vec<_>>();
                        evidence.extend(memory_evidence_by_pc.get(&store.pc).into_iter().flatten().cloned());
                        let ordering = "per-iteration instruction order under the recovered path guard";
                        let ast = builder.ast_spans(
                            "source-memory-store",
                            None,
                            vec![asts[&store.address].clone(), asts[&store.value].clone(), asts[&store.guard].clone()],
                            &evidence,
                            array([object([("start", address(store.pc)), ("end", address(store.pc + 4))])]),
                            object([
                                ("bytes", store.bytes.into()),
                                ("partition", partition.into()),
                                ("ordering", ordering.into()),
                                ("template", format!("if ($2) {helper}((uintptr_t)($0), (uint64_t)($1))").into()),
                            ]),
                        )?;
                        loop_children.push(ast);
                        memory_records.push((object([
                            ("artifact_kind", "external-memory-write".into()),
                            ("partition", partition.into()),
                            ("bytes", store.bytes.into()),
                            ("ordering", ordering.into()),
                            ("guard_ast", asts[&store.guard].clone().into()),
                        ]), ranges(store.pc, store.pc + 4)));
                    }
                }
            }
            loop_code.push_str("    }\n");
            let evidence = source_loop.pcs.iter().filter_map(|pc| ir.get(((*pc - start) / 4) as usize).cloned())
                .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
            let origins = array(source_loop.pcs.iter().map(|pc| object([("start", address(*pc)), ("end", address(*pc + 4))])));
            let ast = builder.ast_spans(
                "source-while",
                None,
                loop_children,
                &evidence,
                origins,
                object([("template", "while ($0) { ... }".into()), ("control_flow", "single-entry natural loop with one zero-test exit and complementary backedge".into())]),
            )?;
            effect_asts.push(ast);
        }
        if ordered_effect_schedule {
            for load in &self.external_loads {
                effects_code.push_str(&format!("    uint64_t hs_loaded_{};\n", load.expression));
            }
        }
        let mut events = self.external_loads.iter().enumerate().map(|(index, load)| (load.pc, 0u8, index)).chain(
            self.external_stores.iter().enumerate().map(|(index, store)| (store.pc, 1u8, index))
        ).chain(self.calls.iter().enumerate().filter(|_| self.ordered_call_memory || self.ordered_volatile_accesses).map(|(index, call)| (call.pc, 2u8, index)))
            .collect::<Vec<_>>();
        events.sort_by_key(|event| (event.0, event.1));
        for (pc, event_kind, event_index) in events {
            if event_kind == 2 {
                let call = &self.calls[event_index];
                let invocation = call_invocations.get(&call.expression).ok_or_else(|| io::Error::new(
                    io::ErrorKind::InvalidData, "ordered call expression was not rendered",
                ))?;
                let call_assignment = if self.dag.nodes[call.expression].source_type.as_ref()
                    .is_some_and(|ty| ty.bits == 0) {
                    format!("    {invocation};\n")
                } else {
                    format!("    {} = {invocation};\n    (void){};\n",
                        rendered[&call.expression], rendered[&call.expression])
                };
                if self.dag.literal(&Symbol::Value(call.guard, 64)) == Some(1) {
                    effects_code.push_str(&call_assignment);
                } else {
                    effects_code.push_str(&format!("    if ({}) {{\n{}    }}\n", rendered[&call.guard], call_assignment));
                }
                let evidence = ir.get(((call.pc - start) / 4) as usize).into_iter().cloned()
                    .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
                effect_asts.push(builder.ast_spans(
                    "source-call-event",
                    None,
                    vec![asts[&call.expression].clone(), asts[&call.guard].clone()],
                    &evidence,
                    array([object([("start", address(call.pc)), ("end", address(call.pc + 4))])]),
                    object([("template", "call-result = call(arguments)".into()), ("ordering", "instruction order with caller memory effects".into())]),
                )?);
                continue;
            }
            if event_kind == 0 {
                if !ordered_effect_schedule { continue; }
                let load = &self.external_loads[event_index];
                let helper = if load.volatile {
                    volatile_helper(false, load.bytes, self.dag.type_of(load.expression).expect("validated volatile pointee")).0
                } else { format!("hs_load{}", load.bytes) };
                let read = format!("    hs_loaded_{} = {}((uintptr_t)({}));\n", load.expression, helper, rendered[&load.address]);
                if self.dag.literal(&Symbol::Value(load.guard, 64)) == Some(1) {
                    effects_code.push_str(&read);
                } else {
                    effects_code.push_str(&format!("    if ({}) {{\n{}    }}\n", rendered[&load.guard], read));
                }
                memory_records.push((object([
                    ("artifact_kind", "external-memory-read".into()),
                    ("partition", if load.volatile { "declaration-proven volatile pointer" } else { "declaration-proven ordinary pointer" }.into()),
                    ("bytes", load.bytes.into()),
                    ("ordering", "instruction order under the recovered path guard".into()),
                    ("guard_ast", asts[&load.guard].clone().into()),
                    ("address_ast", asts[&load.address].clone().into()),
                    ("read_ast", asts[&load.expression].clone().into()),
                ]), ranges(pc, pc + 4)));
                let mut evidence = ir
                    .get(((load.pc - start) / 4) as usize)
                    .into_iter()
                    .cloned()
                    .chain(std::iter::once(declaration.to_owned()))
                    .collect::<Vec<_>>();
                evidence.extend(memory_evidence_by_pc.get(&load.pc).into_iter().flatten().cloned());
                effect_asts.push(builder.ast_spans(
                    "source-memory-read",
                    None,
                    vec![asts[&load.address].clone(), asts[&load.expression].clone(), asts[&load.guard].clone()],
                    &evidence,
                    array([object([("start", address(load.pc)), ("end", address(load.pc + 4))])]),
                    object([
                        ("bytes", load.bytes.into()),
                        ("partition", if load.volatile { "declaration-proven volatile pointer" } else { "declaration-proven ordinary pointer" }.into()),
                        ("ordering", "instruction order under the recovered path guard".into()),
                        ("template", format!("if ($2) {helper}((uintptr_t)($0))").into()),
                    ]),
                )?);
                continue;
            }
            let effect = &self.external_stores[event_index];
            let (store_helper, partition, ordering) = if effect.volatile {
                let pointee = scalar_pointee_type(self.dag.type_of(effect.address).expect("validated volatile pointer type"))
                    .filter(|ty| u64::from(ty.bits) == effect.bytes * 8)
                    .expect("validated volatile pointee type");
                (volatile_helper(true, effect.bytes, &pointee).0,
                 "declaration-proven volatile pointer", "instruction order under the recovered path guard")
            } else {
                (format!("hs_store{}", effect.bytes),
                 "declaration-proven ordinary pointer", "instruction order under the recovered path guard")
            };
            let store = format!(
                "    {}((uintptr_t)({}), (uint64_t)({}));\n",
                store_helper,
                rendered[&effect.address],
                rendered[&effect.value],
            );
            let guard_is_true = self.dag.literal(&Symbol::Value(effect.guard, 64)) == Some(1);
            if guard_is_true {
                effects_code.push_str(&store);
            } else {
                effects_code.push_str(&format!("    if ({}) {{\n{}    }}\n", rendered[&effect.guard], store));
            }
            let mut evidence = ir
                .get(((effect.pc - start) / 4) as usize)
                .into_iter()
                .cloned()
                .chain(std::iter::once(declaration.to_owned()))
                .collect::<Vec<_>>();
            evidence.extend(memory_evidence_by_pc.get(&effect.pc).into_iter().flatten().cloned());
            let ast = builder.ast_spans(
                "source-memory-store",
                None,
                vec![asts[&effect.address].clone(), asts[&effect.value].clone(), asts[&effect.guard].clone()],
                &evidence,
                array([object([("start", address(effect.pc)), ("end", address(effect.pc + 4))])]),
                object([
                    ("bytes", effect.bytes.into()),
                    ("partition", partition.into()),
                    ("ordering", ordering.into()),
                    ("template", format!("if ($2) {store_helper}((uintptr_t)($0), (uint64_t)($1))").into()),
                ]),
            )?;
            effect_asts.push(ast);
            memory_records.push((object([
                ("artifact_kind", "external-memory-write".into()),
                ("partition", partition.into()),
                ("bytes", effect.bytes.into()),
                ("ordering", ordering.into()),
                ("guard_ast", asts[&effect.guard].clone().into()),
            ]), ranges(pc, pc + 4)));
        }
        let return_code = if s.result_bits == 0 {
            "    return;\n".to_owned()
        } else if s.result_pointer {
            format!(
                "    return ({}) (uintptr_t)({result});\n",
                s.result_spelling
            )
        } else if s.result_signed {
            format!("    uint{0}_t hs_result_bits = (uint{0}_t)({result});\n    int{0}_t hs_signed_result;\n    memcpy(&hs_signed_result, &hs_result_bits, sizeof hs_result_bits);\n    return ({1})hs_signed_result;\n",s.result_bits,s.result_spelling)
        } else {
            format!("    return ({})({result});\n", s.result_spelling)
        };
        let result_assertion = if s.result_bits == 0 { String::new() } else {
            format!("    _Static_assert(sizeof({}) == {}, \"result type layout mismatch\");\n", s.result_spelling, s.result_bits / 8)
        };
        let assertions=format!("    _Static_assert(sizeof(void *) == 8 && sizeof(long) == 8, \"Apple LP64 ABI required\");\n{result_assertion}")
            + &s.parameters.iter().map(|p|format!("    _Static_assert(sizeof({}) == {}, \"parameter type layout mismatch\");\n",p.spelling,p.bits/8)).collect::<String>();
        let unused = s
            .parameters
            .iter()
            .map(|p| format!("    (void){};\n", p.name))
            .collect::<String>();
        let mut volatile_helpers = String::new();
        let mut helpers = BTreeMap::<String, (bool, String)>::new();
        for load in self.external_loads.iter().filter(|load| load.volatile) {
            let (name, spelling) = volatile_helper(false, load.bytes, self.dag.type_of(load.expression).expect("validated volatile pointee"));
            helpers.insert(name, (false, spelling));
        }
        for load in self.loops.iter().flat_map(|source_loop| &source_loop.loads).filter(|load| load.volatile) {
            let (name, spelling) = volatile_helper(false, load.bytes, self.dag.type_of(load.expression).expect("validated volatile pointee"));
            helpers.insert(name, (false, spelling));
        }
        for store in self.external_stores.iter().filter(|store| store.volatile) {
            let pointee = scalar_pointee_type(self.dag.type_of(store.address).expect("validated volatile pointer type"))
                .expect("validated volatile pointee type");
            let (name, spelling) = volatile_helper(true, store.bytes, &pointee);
            helpers.insert(name, (true, spelling));
        }
        for store in self.loops.iter().flat_map(|source_loop| &source_loop.stores).filter(|store| store.volatile) {
            let pointee = scalar_pointee_type(self.dag.type_of(store.address).expect("validated volatile pointer type"))
                .expect("validated volatile pointee type");
            let (name, spelling) = volatile_helper(true, store.bytes, &pointee);
            helpers.insert(name, (true, spelling));
        }
        for (name, (is_store, spelling)) in helpers {
            if is_store {
                volatile_helpers.push_str(&format!(
                    "static inline void {name}(uintptr_t a, uint64_t x) {{ volatile {spelling} *p=(volatile {spelling} *)a; *p=({spelling})x; }}\n"
                ));
            } else {
                volatile_helpers.push_str(&format!(
                    "static inline uint64_t {name}(uintptr_t a) {{ volatile {spelling} *p=(volatile {spelling} *)a; return (uint64_t)(*p); }}\n"
                ));
            }
        }
        let output=format!("{headers}{call_declarations}{}\nstatic inline uint64_t hs_load1(uintptr_t a) {{ uint8_t v; memcpy(&v,(const void *)a,1); return v; }}\nstatic inline uint64_t hs_load2(uintptr_t a) {{ uint16_t v; memcpy(&v,(const void *)a,2); return v; }}\nstatic inline uint64_t hs_load4(uintptr_t a) {{ uint32_t v; memcpy(&v,(const void *)a,4); return v; }}\nstatic inline uint64_t hs_load8(uintptr_t a) {{ uint64_t v; memcpy(&v,(const void *)a,8); return v; }}\nstatic inline void hs_store8(uintptr_t a, uint64_t x) {{ uint64_t v=x; memcpy((void *)a,&v,8); }}\nstatic inline void hs_store4(uintptr_t a, uint64_t x) {{ uint32_t v=(uint32_t)x; memcpy((void *)a,&v,4); }}\nstatic inline void hs_store2(uintptr_t a, uint64_t x) {{ uint16_t v=(uint16_t)x; memcpy((void *)a,&v,2); }}\nstatic inline void hs_store1(uintptr_t a, uint64_t x) {{ uint8_t v=(uint8_t)x; memcpy((void *)a,&v,1); }}\n{volatile_helpers}/* Recovered from bounded structured IR; external memory effects require valid memory; volatile accesses retain instruction order alongside qualified scalar calls on each supported path; loop-carried register values are explicit locals; local frame effects are eliminated. */\n{} {}({}) {{\n{assertions}    (void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;(void)hs_flags_arith;(void)hs_load1;(void)hs_load2;(void)hs_load4;(void)hs_load8;(void)hs_store1;(void)hs_store2;(void)hs_store4;(void)hs_store8;\n{unused}{declarations}{loop_code}{effects_code}{return_code}}}\n",include_str!("bitvector_runtime.h"),s.result_spelling,s.name,if parameters.is_empty(){"void"}else{&parameters});
        let root = builder.ast(
            "source-function",
            Some(&s.result_spelling),
            effect_asts.into_iter().chain(std::iter::once(asts[&self.result].clone())).collect(),
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
        let objc_output=objc_method.and_then(|(name,header)|{
            let (class,_) = name.get(2..name.len().checked_sub(1)?)?.split_once(' ')?;
            if !crate::objc::identifier(class)||s.signature.language!="Objective-C / Objective-C++"{return None}
            let method=s.signature.declaration.strip_suffix(';')?;
            let arguments=s.parameters.iter().map(|parameter|parameter.name.as_str()).collect::<Vec<_>>().join(", ");
            let call=format!("{}{}({arguments});",if s.result_spelling=="void"{""}else{"return "},s.name);
            Some(format!("/* Recovered Objective-C method adapter over the validated IR-derived IMP body below.\n * Category packaging is synthetic; not original source or a complete class implementation.\n * Class/type dependencies and deployment integration still require review. */\n{header}\n{output}\n@interface {class} (HSRecovered_{start:x})\n{method};\n@end\n@implementation {class} (HSRecovered_{start:x})\n{method} {{\n    {call}\n}}\n@end\n"))
        });
        let node=builder.record("metadata","derived",object([("artifact_kind","recovered-source".into()),("source",output.into()),("status","generated-uncompiled".into()),("name",s.name.clone().into()),("ast_root",root.clone().into()),("declaration",s.signature.declaration.clone().into()),("contract","apple-aapcs64-lp64-reads-writes-calls-volatile-call-ordered-natural-loop-guarded-call-events-loop-memory-v14".into()),("observables","declared scalar result, bounded ordinary memory reads and ordered ordinary writes under supported path guards, exact-prototype scalar direct calls whose complete returning callee walk proves no memory reads, writes, or onward calls, supported exact self-recursion, ABI nonvolatile GPR/SP restoration, exact-width volatile accesses scheduled in instruction order with supported calls and path guards, and supported sequential single-entry scalar natural loops with per-iteration ordered external loads and stores; caller memory effects before or after proven calls are emitted in per-path instruction order for bounded acyclic control flow; call instructions still block strict machine-state C; fault/source-fidelity/formal-equivalence behavior is not claimed".into())]),array([]))?;
        if let Some(source)=objc_output{
            let objc=builder.record("metadata","derived",object([
                ("artifact_kind","recovered-objective-c".into()),("source",source.into()),
                ("status","generated-uncompiled".into()),("ast_root",root.clone().into()),
                ("contract","synthetic Objective-C category adapter over the validated recovered IMP body; exact method encoding and argument forwarding; not original class implementation, deployment or source-equivalence qualification".into())]),array([]))?;
            builder.edge(&node,&objc,"renders-as")?;
        }
        builder.edge(declaration, &node, "types")?;
        let assumption=builder.record("memory-effect","assumption",object([("contract","ordinary private frame/red-zone storage and valid ordinary external memory reads/writes in instruction order when mixed accesses have a single acyclic path; single-threaded Apple LP64; declaration-proven volatile accesses retain per-path instruction order; calls require exact scalar prototypes and bounded supported callee proofs; no precise fault-state observables".into()),("red_zone_bytes",128u64.into())]),array([]))?;
        builder.edge(&assumption, &node, "assumes")?;
        if boolean_memory_reads {
            let boolean_assumption = builder.record("memory-effect", "assumption", object([
                ("contract", "Boolean pointer reads require live Boolean objects with canonical 0/1 representation; invalid representations are outside recovered-source qualification".into()),
                ("artifact_kind", "boolean-memory-precondition".into()),
            ]), array([]))?;
            builder.edge(&boolean_assumption, &node, "assumes")?;
        }
        for (payload, span) in memory_records {
            let record = builder.record("memory-effect", "derived", payload, span)?;
            builder.edge(&record, &node, "supports")?;
        }
        // Publish the proven frame objects and the memory version each one
        // holds. The alias partition is exact: the frame is private, so two
        // objects are the same location only when their extents are equal.
        for frame_object in &self.frame_objects {
            let record = builder.record(
                "memory-object",
                "derived",
                object([
                    ("artifact_kind", "frame-object".into()),
                    ("partition", "private-frame".into()),
                    ("offset", V::Int(i128::from(frame_object.offset))),
                    ("bytes", frame_object.bytes.into()),
                    ("version_pc", address(frame_object.version)),
                    (
                        "alias_policy",
                        "exact extent match; distinct extents are distinct objects".into(),
                    ),
                ]),
                ranges(frame_object.version, frame_object.version + 4),
            )?;
            if let Some(memory_versions) = memory_evidence_by_pc.get(&frame_object.version) {
                for memory_version in memory_versions {
                    builder.edge(memory_version, &record, "backs-frame-object-version")?;
                }
            }
            builder.edge(&record, &node, "supports")?;
        }
        for id in ir {
            builder.edge(id, &node, "supports")?;
        }
        Ok((root, node))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_signature(name: &str, declaration: &str) -> SourceSignature {
        crate::signatures::source_signature(
            name,
            &crate::signatures::Signature {
                language: "C",
                declaration: declaration.into(),
                source: "test declaration",
            },
        ).expect("test source signature")
    }

    #[test]
    fn call_result_value_width_comes_from_the_callee_prototype() {
        for (callee_result, caller_result, expected_bits) in [
            ("uint64_t", "uint32_t", 64),
            ("uint32_t", "uint64_t", 32),
        ] {
            let caller = test_signature("_hs_test", &format!("{caller_result} hs_test(void);"));
            let callee = test_signature("_hs_helper", &format!("{callee_result} hs_helper(void);"));
            assert_ne!(caller.result_bits, callee.result_bits, "the regression needs distinct ABI widths");
            let tree = tree("call", None, object([
                ("target", 0x2000u64.into()),
                ("call_effects", object([
                    ("arguments", array([])),
                    ("result", object([
                        ("source_spelling", callee.result_spelling.clone().into()),
                        ("location", "x0".into()),
                    ])),
                ])),
            ]), vec![]);
            let mut state = State {
                registers: (0..33).map(Symbol::Unknown).collect(),
                flags: None,
                frame: BTreeMap::new(),
                external_loads: Vec::new(),
                external_stores: Vec::new(),
                external_access_pcs: Vec::new(),
                path: 0,
                has_call: false,
                call_pcs: Vec::new(),
                calls: Vec::new(),
            };
            let mut dag = Dag { nodes: Vec::new() };
            let prototypes = BTreeMap::from([(0x2000, callee.clone())]);
            statement(&tree, &mut state, &mut dag, 0x1000, &prototypes)
                .expect("exact scalar call is modeled");
            assert!(matches!(state.registers[0], Symbol::Value(_, bits) if bits == expected_bits));
            let Symbol::Value(value, _) = state.registers[0] else { unreachable!() };
            assert_eq!(dag.type_of(value).map(SourceType::spelling), Some(callee_result));
        }
    }

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
            &BTreeMap::new(),
        )
    }
    #[test]
    fn a_proven_declaration_types_the_parameter_and_result_expressions() {
        let recovery = attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);",
        )
        .expect("declaration recovers");
        // The parameter bindings carry the declared parameter type.
        let parameters: Vec<&SourceType> = recovery
            .dag
            .nodes
            .iter()
            .filter(|node| node.kind == "source-parameter")
            .filter_map(|node| node.source_type.as_ref())
            .collect();
        assert_eq!(parameters.len(), 2);
        for parameter in parameters {
            assert_eq!(parameter.spelling(), "uint64_t");
            assert_eq!(parameter.bits, 64);
            assert!(!parameter.signed);
            assert!(!parameter.pointer);
        }
        // The returned expression carries the declared result type.
        let result = recovery
            .dag
            .type_of(recovery.result)
            .expect("the result expression has a proven type");
        assert_eq!(result.spelling(), "uint64_t");
        assert_eq!(result.bits, 64);
    }

    #[test]
    fn an_unproven_expression_keeps_its_type_unresolved() {
        // A constant has no proven source type, so an operation over it stays
        // unresolved rather than inheriting a machine width as a type.
        let recovery = attempt(
            &[0x91000400, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first);",
        )
        .expect("declaration recovers");
        let untyped = recovery
            .dag
            .nodes
            .iter()
            .filter(|node| node.kind == "source-expression")
            .filter(|node| node.source_type.is_none())
            .count();
        assert!(untyped > 0, "computed expressions over constants stay untyped");
    }

    #[test]
    fn an_operation_over_a_proven_parameter_keeps_its_type() {
        // `add x0, x0, x1` adds two proven `uint64_t` parameters, so the
        // result keeps the agreed type.
        let recovery = attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);",
        )
        .expect("declaration recovers");
        let typed: Vec<&SourceType> = recovery
            .dag
            .nodes
            .iter()
            .filter(|node| node.kind == "source-expression")
            .filter_map(|node| node.source_type.as_ref())
            .collect();
        assert!(!typed.is_empty(), "the addition keeps the proven type");
        for source_type in typed {
            assert_eq!(source_type.spelling(), "uint64_t");
        }
    }

    #[test]
    fn a_recovered_local_carries_a_deterministic_role_name() {
        // `xor x0, x0, x1` then `add x0, x0, x2`: the two reused expressions
        // are named for the operation they perform, not for an opaque counter.
        let recovery = attempt(
            &[0xca01_0000, 0x8b02_0000, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second, uint64_t third);",
        )
        .expect("declaration recovers");
        let roles: Vec<&str> = recovery
            .dag
            .nodes
            .iter()
            .filter(|node| node.kind == "source-expression")
            .map(|node| node.role)
            .collect();
        assert!(roles.contains(&"bit_xor"), "{roles:?}");
        assert!(roles.contains(&"sum"), "{roles:?}");
        // The role vocabulary never leaks a raw operator or a template.
        for role in &roles {
            assert!(
                role.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "role names are identifiers: {role}"
            );
        }
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
    fn a_proven_frame_object_publishes_its_slot_extent_and_version() {
        // `sub sp, sp, #16` then `str x0, [sp, #8]` then `ldr x0, [sp, #8]`.
        // The recovered function proves one frame object at offset -8 and
        // names the store that last wrote it.
        let recovery = attempt(
            &[
                0xd10043ff, 0xf90007e0, 0xf94007e0, 0x910043ff, 0xd65f03c0,
            ],
            "uint64_t hs_test(uint64_t first);",
        )
        .expect("declaration recovers");
        assert!(!recovery.frame_objects.is_empty(), "a frame object is proven");
        for object in &recovery.frame_objects {
            assert_eq!(object.bytes, 8);
            assert!(object.offset < 0, "a frame object lives below the frame pointer");
            assert!(object.version >= 0x1000, "the version names an instruction");
        }
        // Two objects at the same extent are one memory location, so the
        // proven objects never repeat an extent.
        let extents: Vec<(i64, u64)> = recovery
            .frame_objects
            .iter()
            .map(|object| (object.offset, object.bytes))
            .collect();
        let mut unique = extents.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(extents.len(), unique.len(), "{extents:?}");
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
    fn void_returns_do_not_require_an_x0_value_but_keep_effects_and_abi_checks() {
        let empty = attempt(&[0xd65f03c0], "void hs_test(void);")
            .expect("void return has no incoming result dependency");
        assert_eq!(empty.signature.result_bits, 0);
        let store = attempt(&[0xf9000001, 0xd65f03c0],
            "void hs_test(uint64_t *destination, uint64_t value);")
            .expect("void setter retains its memory effect");
        assert_eq!(store.external_stores.len(), 1);
        assert_eq!(store.external_stores[0].bytes, 8);
        assert!(attempt(&[0x91000413, 0xd65f03c0], "void hs_test(void);").is_err(),
            "void must still reject unrestored x19");
        assert!(attempt(&[0xd10043ff, 0xd65f03c0], "void hs_test(void);").is_err(),
            "void must still reject unrestored SP");
    }

    #[test]
    fn scalar_pointees_preserve_pointer_depth_and_qualifier_ownership() {
        let pointer = |spelling: &str| SourceType {
            spelling: spelling.into(), bits: 64, signed: false, pointer: true,
        };
        assert_eq!(scalar_pointee_type(&pointer("const bool * restrict")).unwrap().spelling, "bool");
        assert!(scalar_pointee_type(&pointer("bool **")).is_none());
        assert!(scalar_pointee_type(&pointer("int64_t **")).is_none());
        assert!(pointee_qualified(&pointer("const bool *"), "const"));
        assert!(!pointee_qualified(&pointer("bool * const"), "const"));
        assert!(pointee_qualified(&pointer("volatile bool *"), "volatile"));
        assert!(!pointee_qualified(&pointer("bool * volatile"), "volatile"));
    }

    #[test]
    fn constant_bit_patterns_survive_explicit_width_views_and_copies() {
        let mut dag = Dag { nodes: vec![] };
        let value = dag.constant(0x1_ffff_ffff, 0x1000).unwrap();
        let narrowed = dag.value(Symbol::Value(value, 64), 8, 0x1004).unwrap();
        let copied = dag.add_role("source-copy", "copy", "$0".into(), vec![narrowed], 0x1008).unwrap();
        assert_eq!(dag.literal(&Symbol::Value(copied, 64)), Some(255));
        assert_eq!(dag.nodes[copied].origins, BTreeSet::from([0x1000, 0x1004, 0x1008]));
        let unknown = dag.add("source-parameter", "parameter".into(), vec![], 0x100c).unwrap();
        let unknown = dag.value(Symbol::Value(unknown, 64), 32, 0x1010).unwrap();
        assert_eq!(dag.literal(&Symbol::Value(unknown, 64)), None);
    }

    #[test]
    fn exact_integer_store_type_can_constrain_a_known_constant() {
        let recovered = attempt(&[0x52800001, 0xb9000001, 0xd65f03c0],
            "void hs_test(volatile int *destination);").unwrap();
        let store = &recovered.external_stores[0];
        let ty = recovered.dag.type_of(store.value).unwrap();
        assert_eq!((ty.spelling.as_str(), ty.bits, ty.signed), ("int", 32, true));
        assert!(attempt(&[0x52800041, 0x39000001, 0xd65f03c0],
            "void hs_test(volatile bool *destination);").is_err(), "noncanonical Boolean stores remain blocked");
    }

    #[test]
    fn exact_integer_stores_convert_declared_scalars_but_not_pointers() {
        let recovered = attempt(&[0x39000001, 0xd65f03c0],
            "void hs_test(volatile uint8_t *destination, uint64_t value);").unwrap();
        let stored = recovered.dag.type_of(recovered.external_stores[0].value).unwrap();
        assert_eq!((stored.spelling.as_str(), stored.bits, stored.signed), ("uint8_t", 8, false));
        assert!(attempt(&[0x39000001, 0xd65f03c0],
            "void hs_test(volatile uint8_t *destination, void *value);").is_err());
    }

    #[test]
    fn equal_width_native_volatile_helpers_do_not_alias_distinct_c_types() {
        let ty = |spelling: &str| SourceType { spelling: spelling.into(), bits: 64, signed: true, pointer: false };
        let long = volatile_helper(false, 8, &ty("long"));
        let long_long = volatile_helper(false, 8, &ty("long long"));
        assert_ne!(long.0, long_long.0);
        assert_eq!(long.1, "long");
        assert_eq!(long_long.1, "long long");
        assert_eq!(volatile_helper(false, 8, &ty("int64_t")).0, "hs_vload_s8");
    }

    #[test]
    fn native_lp64_pointees_keep_declared_spelling_width_and_signedness() {
        for (spellings, bits, signed) in [
            (vec!["char", "signed char"], 8, true), (vec!["unsigned char"], 8, false),
            (vec!["short", "short int", "signed short"], 16, true),
            (vec!["unsigned short", "unsigned short int"], 16, false),
            (vec!["int", "signed", "signed int"], 32, true),
            (vec!["unsigned", "unsigned int"], 32, false),
            (vec!["long", "long int", "long long", "long long int", "intptr_t", "ssize_t", "ptrdiff_t"], 64, true),
            (vec!["unsigned long", "unsigned long int", "unsigned long long", "unsigned long long int", "uintptr_t", "size_t"], 64, false),
        ] {
            for spelling in spellings {
                let pointer = SourceType { spelling: format!("const volatile {spelling} * restrict"),
                                           bits: 64, signed: false, pointer: true };
                let pointee = scalar_pointee_type(&pointer).unwrap();
                assert_eq!((pointee.spelling.as_str(), pointee.bits, pointee.signed, pointee.pointer),
                           (spelling, bits, signed, false));
            }
        }
    }

    #[test]
    fn boolean_results_require_a_canonical_machine_value() {
        assert!(attempt(&[0xd65f03c0], "bool hs_test(bool value);").is_ok());
        assert!(attempt(&[0x52800020, 0xd65f03c0], "bool hs_test(void);").is_ok());
        assert!(attempt(&[0x52800040, 0xd65f03c0], "bool hs_test(void);").is_err());
        assert!(attempt(&[0xd65f03c0], "bool hs_test(uint8_t value);").is_err());
        assert!(attempt(&[0x52800020, 0xd65f03c0], "bool hs_test(bool true);").is_err());
    }

    #[test]
    fn explicit_signed_integer_spellings_keep_declared_width_and_signedness() {
        for (spelling, bits) in [("signed short int", 16), ("signed long", 64),
            ("signed long int", 64), ("signed long long", 64), ("signed long long int", 64)] {
            let pointer = SourceType { spelling: format!("const volatile {spelling} *"),
                bits: 64, signed: false, pointer: true };
            let pointee = scalar_pointee_type(&pointer).expect("standard signed integer pointee");
            assert_eq!(pointee.spelling, spelling);
            assert_eq!(pointee.bits, bits);
            assert!(pointee.signed);
            assert!(!pointee.pointer);
            assert!(attempt(&[0xd65f03c0], &format!("{spelling} hs_test({spelling} value);")).is_ok());
        }
    }
    #[test]
    fn unsafe_and_unsupported_source_declarations_are_not_emitted() {
        for declaration in [
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
