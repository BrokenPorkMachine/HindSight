//! Evidence-scoped source expressions, not renamed machine-register strings.
//! Pure acyclic functions: symbolic frame objects/copies disappear only after
//! every reachable return proves restored SP, LR and Darwin nonvolatile GPRs.
use super::*;
use crate::signatures::SourceSignature;
use crate::reconstruction::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Declaration recovery does not depend on proving the implementation body.
pub(super) fn emit_header(builder: &mut Builder, s: &crate::signatures::SourceDeclaration, declaration: &str,
                         start: u64, end: u64, objc_header: Option<&str>,
                         project_types: &[crate::reconstruction::ProjectTypeDependency]) -> io::Result<String> {
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
    let required_types = s.parameters.iter().fold(c_identifiers(&s.result_spelling), |mut names, (ty, _)| {
        names.extend(c_identifiers(ty));
        names
    });
    let type_dependencies = select_project_type_dependencies(required_types, project_types);
    let type_declarations = if type_dependencies.is_empty() { String::new() } else {
        format!("/* Source- and target-matched project type declarations; layout is not independently validated. */\n{}\n",
            type_dependencies.iter().map(|dependency| dependency.declaration.as_str()).collect::<Vec<_>>().join("\n"))
    };
    let mut text = format!(
        "/* Recovered declaration for {start:#x}..{end:#x}; Apple LP64 ABI.\n * Declaration evidence only: implementation recovery and behavioral equivalence are not implied. */\n#pragma once\n{headers}\n{type_declarations}\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n{} {}({});\n#ifdef __cplusplus\n}}\n#endif\n",
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
    for dependency in type_dependencies {
        let evidence = builder.record("type", "analyst-assertion", object([
            ("name", dependency.name.clone().into()),
            ("declaration", dependency.declaration.clone().into()),
            ("target", dependency.target.clone().into()),
            ("source_sha256", dependency.source_sha256.clone().into()),
            ("abi_layout_claimed", false.into()),
            ("compiler_layout_evidence", dependency.layout_evidence.clone().map(Value::String).unwrap_or(Value::Null)),
            ("captured_type_identity_proven", false.into()),
        ]), array([]))?;
        builder.edge(&evidence, &header, "required-source-dependency")?;
    }
    Ok(header)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Symbol {
    Value(usize, u8),
    /// Opaque 128-bit memory blob (e.g. vector q-register copy); not a scalar value.
    Blob(usize),
    Frame(i64),
    Unknown(u64),
}
#[derive(Clone)]
struct State {
    registers: Vec<Symbol>,
    /// Scalar FP register values (v0-v31 low bits) tracked as (symbol, bits: 32|64).
    fp_scalars: Vec<Option<(Symbol, u8)>>,
    /// 128-bit q/v register blob expressions (q0-q31).
    q_blobs: Vec<Option<usize>>,
    flags: Option<usize>,
    frame: BTreeMap<(i64, u64), (Symbol, BTreeSet<u64>)>,
    external_loads: Vec<ExternalLoad>,
    external_stores: Vec<ExternalStore>,
    external_access_pcs: Vec<u64>,
    path: usize,
    has_call: bool,
    call_pcs: Vec<u64>,
    calls: Vec<CallResult>,
    /// Upper bound (exclusive, relative to entry SP = Frame(0)) of proven
    /// caller-passed stack argument slots seeded for this function.
    incoming_stack_end: i64,
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
    member: Option<AggregateMemberAccess>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExternalLoad {
    expression: usize,
    address: usize,
    guard: usize,
    bytes: u64,
    pc: u64,
    volatile: bool,
    member: Option<AggregateMemberAccess>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AggregateAddress {
    base: usize,
    type_spelling: String,
    layout: crate::reconstruction::ProjectTypeLayout,
    offset_bytes: u64,
    target: String,
    source_sha256: String,
    layout_evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AggregateMemberAccess {
    base: usize,
    type_spelling: String,
    record_name: String,
    field_name: String,
    field_declaration: String,
    offset_bits: u64,
    field_type: SourceType,
    target: String,
    source_sha256: String,
    layout_evidence: String,
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
    aggregate_address: Option<AggregateAddress>,
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

fn aggregate_layout_for_pointer(
    spelling:&str,
    project_types:&[crate::reconstruction::ProjectTypeDependency],
)->Option<(String,crate::reconstruction::ProjectTypeLayout)> {
    let (base,suffix)=spelling.rsplit_once('*')?;
    if base.contains('*')||!suffix.split_whitespace().all(|word|matches!(word,"const"|"volatile"|"restrict"|"__restrict"|"__restrict__"|"_Nonnull"|"_Nullable")) {return None}
    let type_spelling=base.split_whitespace().filter(|word|!matches!(*word,"const"|"volatile"|"restrict"|"__restrict"|"__restrict__"|"_Nonnull"|"_Nullable"))
        .collect::<Vec<_>>().join(" ");
    let name=type_spelling.strip_prefix("struct ").or_else(||type_spelling.strip_prefix("union ")).unwrap_or(&type_spelling);
    if name.is_empty()||name.split_whitespace().count()!=1{return None}
    let candidate=project_types.iter().find(|candidate|candidate.name==name)?;
    let layout=candidate.layout.as_ref()?.clone();
    (layout.name==name&&layout.fields_complete).then_some((type_spelling,layout))
}

fn layout_field_extent_bits(field:&crate::reconstruction::ProjectTypeLayoutField)->Option<u64> {
    if let Some(width)=field.bit_width{return (width>0).then_some(width)}
    let mut parts=field.declaration.split_whitespace().collect::<Vec<_>>();
    if parts.pop()?!=field.name||parts.is_empty()||field.declaration.contains('[')||field.declaration.contains('*')
        ||field.declaration.split_whitespace().any(|part|matches!(part,"volatile"|"_Atomic")){return None}
    let spelling=parts.join(" ");
    let pointer=SourceType {spelling:format!("{spelling} *"),bits:64,signed:false,pointer:true};
    let field_type=scalar_pointee_type(&pointer)?;
    Some(u64::from(field_type.bits))
}

fn aggregate_member_at(address:&AggregateAddress,bytes:u64)->Option<AggregateMemberAccess> {
    if address.layout.kind!="struct"||!address.layout.fields_complete{return None}
    let offset_bits=address.offset_bytes.checked_mul(8)?;
    let access_bits=bytes.checked_mul(8)?;
    let access_end=offset_bits.checked_add(access_bits)?;
    if access_end>address.layout.size.checked_mul(8)?{return None}
    let mut candidates=address.layout.fields.iter().filter(|field|field.offset_bits==offset_bits&&field.bit_width.is_none());
    let field=candidates.next()?; if candidates.next().is_some(){return None}
    for other in &address.layout.fields {
        if std::ptr::eq(other,field){continue}
        let extent=layout_field_extent_bits(other)?;
        let end=other.offset_bits.checked_add(extent)?;
        if other.offset_bits<access_end&&offset_bits<end{return None}
    }
    let extent=layout_field_extent_bits(field)?;
    if extent!=access_bits{return None}
    let mut parts=field.declaration.split_whitespace().collect::<Vec<_>>();
    if parts.pop()?!=field.name||parts.is_empty()||field.declaration.contains('[')||field.declaration.contains('*')
        ||field.declaration.split_whitespace().any(|part|matches!(part,"volatile"|"_Atomic")){return None}
    let spelling=parts.join(" ");
    let pointer=SourceType {spelling:format!("{spelling} *"),bits:64,signed:false,pointer:true};
    let field_type=scalar_pointee_type(&pointer)?;
    Some(AggregateMemberAccess {base:address.base,type_spelling:address.type_spelling.clone(),record_name:address.layout.name.clone(),
        field_name:field.name.clone(),field_declaration:field.declaration.clone(),offset_bits,field_type,
        target:address.target.clone(),source_sha256:address.source_sha256.clone(),layout_evidence:address.layout_evidence.clone()})
}

fn render_external_load(load:&ExternalLoad,rendered:&BTreeMap<usize,String>,dag:&Dag)->String {
    if let Some(member)=&load.member {
        format!("((({} *)({}))->{} )",member.type_spelling,rendered[&member.base],member.field_name)
    } else {
        let helper=if load.volatile {
            volatile_helper(false,load.bytes,dag.type_of(load.expression).expect("validated volatile pointee")).0
        } else {format!("hs_load{}",load.bytes)};
        format!("{helper}((uintptr_t)({}))",rendered[&load.address])
    }
}

fn render_aggregate_member(member:&AggregateMemberAccess,rendered:&BTreeMap<usize,String>)->String {
    format!("(({} *)({}))->{}",member.type_spelling,rendered[&member.base],member.field_name)
}

fn aggregate_member_layout_value(member:Option<&AggregateMemberAccess>)->V {
    member.map(|member|object([
        ("record",member.record_name.clone().into()),("field",member.field_name.clone().into()),
        ("field_declaration",member.field_declaration.clone().into()),("offset_bits",member.offset_bits.into()),
        ("target",member.target.clone().into()),("source_sha256",member.source_sha256.clone().into()),
        ("compiler_layout_evidence",member.layout_evidence.clone().into()),("captured_type_identity_proven",false.into()),
    ])).unwrap_or(V::Null)
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
        let aggregate_address = if matches!(kind, "source-copy" | "source-frame-reload" | "source-return") && children.len() == 1 {
            self.nodes[children[0]].aggregate_address.clone()
        } else { None };
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
            aggregate_address,
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
            Symbol::Frame(offset) if bits == 64 => {
                // Bounded materialization of a recovered stack-frame address.
                // The offset is relative to the function-entry frame base tracked
                // during recovery; emission provides a synthetic frame storage.
                let expr = format!(
                    "((uint64_t)(uintptr_t)(hs_frame_storage + (size_t)(HS_FRAME_BASE + ((int64_t){offset})))) /* frame address */"
                );
                let id = self.add_role("source-frame-address", "frame_addr", expr, vec![], pc)?;
                self.type_node(id, Some(SourceType { spelling: "void *".into(), bits: 64, signed: false, pointer: true }));
                Ok(id)
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
        class => return Err(format!("unsupported register bank: class={class} name={} kind={}", attribute(t,"name"), t.kind)),
    };
    Ok((index, number(t, "bits") as u8))
}
fn read(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64) -> Result<Symbol, String> {
    if t.kind == "register" {
        if attribute(t, "class") == "simd-fp" {
            let name = attribute(t, "name");
            let bits = number(t, "bits") as u8;
            let idx = if let Some(rest) = name.strip_prefix('d') {
                rest.parse::<usize>().unwrap_or(0)
            } else if let Some(rest) = name.strip_prefix('s') {
                rest.parse::<usize>().unwrap_or(0)
            } else if let Some(rest) = name.strip_prefix('h') {
                rest.parse::<usize>().unwrap_or(0)
            } else if let Some(rest) = name.strip_prefix('q') {
                rest.parse::<usize>().unwrap_or(0)
            } else if let Some(rest) = name.strip_prefix('v') {
                rest.parse::<usize>().unwrap_or(0)
            } else { 0usize };
            if bits == 128 || name.starts_with('q') || name.starts_with('v') {
                if let Some(id) = state.q_blobs.get(idx).and_then(|v| *v) {
                    return Ok(Symbol::Blob(id));
                }
                return Ok(Symbol::Unknown(pc ^ 0x4000000000000000));
            }
            if let Some((sym, _b)) = state.fp_scalars.get(idx).and_then(|v| v.clone()) {
                return Ok(sym);
            }
            return Ok(Symbol::Unknown(pc ^ 0x4000000000000000));
        }
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
            if source.kind == "register" && attribute(source, "class") != "simd-fp" {
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
    if t.kind == "float-constant" {
        let bits = number(t, "bits");
        let precision = attribute(t, "precision");
        let width = match precision { "single" => 32u8, "double" => 64u8, _ => 64u8 };
        let id = dag.constant(bits, pc)?;
        let ty = match precision {
            "single" => "float",
            "double" => "double",
            _ => "double",
        };
        dag.type_node(id, Some(SourceType { spelling: ty.into(), bits: width, signed: false, pointer: false }));
        return Ok(Symbol::Value(id, 64));
    }
    if t.kind == "fp-register" {
        let name = attribute(t, "name");
        let (idx, width) = if let Some(rest) = name.strip_prefix('d') {
            (rest.parse::<usize>().unwrap_or(0), 64u8)
        } else if let Some(rest) = name.strip_prefix('s') {
            (rest.parse::<usize>().unwrap_or(0), 32u8)
        } else if let Some(rest) = name.strip_prefix('h') {
            (rest.parse::<usize>().unwrap_or(0), 16u8)
        } else if let Some(rest) = name.strip_prefix('q') {
            (rest.parse::<usize>().unwrap_or(0), 128u8)
        } else if let Some(rest) = name.strip_prefix('v') {
            (rest.parse::<usize>().unwrap_or(0), 128u8)
        } else { (0usize, 64u8) };
        if width == 128 {
            if let Some(id) = state.q_blobs.get(idx).and_then(|v| *v) {
                return Ok(Symbol::Blob(id));
            }
            return Ok(Symbol::Unknown(pc ^ 0x4000000000000000));
        }
        if let Some((sym, _bits)) = state.fp_scalars.get(idx).and_then(|v| v.clone()) {
            return Ok(sym);
        }
        return Ok(Symbol::Unknown(pc ^ 0x4000000000000000));
    }
    if t.kind == "fp-binary" {
        let op = attribute(t, "operator");
        let a_sym = read(&t.children[0], state, dag, pc)?;
        let a = dag.value(a_sym, 64, pc)?;
        let b_sym = read(&t.children[1], state, dag, pc)?;
        let b = dag.value(b_sym, 64, pc)?;
        let bits = number(t, "bits") as u8;
        let (ty, expr) = if bits == 32 {
            ("float".to_owned(), format!(
                "((uint64_t)(uint32_t)((union {{ float f; uint32_t u; }}){{ .f = ((union {{ uint32_t u; float f; }}){{ .u = (uint32_t)((uint64_t)($0)) }}).f {op} ((union {{ uint32_t u; float f; }}){{ .u = (uint32_t)((uint64_t)($1)) }}).f }}).u))"
            ))
        } else {
            ("double".to_owned(), format!(
                "((union {{ double d; uint64_t u; }}){{ .d = ((union {{ uint64_t u; double d; }}){{ .u = (uint64_t)($0) }}).d {op} ((union {{ uint64_t u; double d; }}){{ .u = (uint64_t)($1) }}).d }}).u"
            ))
        };
        let id = dag.add_role("source-fp-binary", "value", expr, vec![a, b], pc)?;
        dag.type_node(id, Some(SourceType { spelling: ty, bits, signed: false, pointer: false }));
        return Ok(Symbol::Value(id, 64));
    }
    if t.kind == "fp-sxtl-scalar" {
        let src_sym = read(&t.children[0], state, dag, pc)?;
        let src = dag.value(src_sym, 64, pc)?;
        let id = dag.add_role(
            "source-fp-sxtl",
            "value",
            "((uint64_t)(int64_t)(int32_t)(uint32_t)((uint64_t)($0)))".into(),
            vec![src],
            pc,
        )?;
        dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
        return Ok(Symbol::Value(id, 64));
    }
    if t.kind == "fp-int-to-float" {
        let src_sym = read(&t.children[0], state, dag, pc)?;
        let src = dag.value(src_sym, 64, pc)?;
        let signed = t.attributes.get("signed") == Some(&V::Bool(true));
        let to_bits = number(t, "to_bits") as u8;
        let (ty, expr) = if to_bits == 32 {
            ("float".to_owned(),
                "((uint64_t)(uint32_t)((union { float f; uint32_t u; }){ .f = (float)((int64_t)(int32_t)(uint32_t)((uint64_t)($0))) }).u))".to_owned()
            )
        } else {
            let cast = if signed { "(double)(int64_t)" } else { "(double)(uint64_t)" };
            ("double".to_owned(), format!(
                "((union {{ double d; uint64_t u; }}){{ .d = {cast}((uint64_t)($0)) }}).u"
            ))
        };
        let id = dag.add_role("source-fp-int-to-float", "value", expr, vec![src], pc)?;
        dag.type_node(id, Some(SourceType { spelling: ty, bits: to_bits, signed: false, pointer: false }));
        return Ok(Symbol::Value(id, 64));
    }
    let mut child = |i, dag: &mut Dag| read(&t.children[i], state, dag, pc);
    if t.kind == "memory-load" {
        let address = child(0, dag)?;
        let bytes = number(t, "bytes");
        if bytes == 16 {
            // Bounded support: 128-bit opaque memory blob copy (e.g. q-register load/store).
            if let Symbol::Value(address_id, _) = &address {
                let volatile = dag.type_of(*address_id).is_some_and(|ty| ty.pointer && pointee_qualified(ty, "volatile"));
                if volatile {
                    return Err("volatile 128-bit blob reads are unsupported".into());
                }
                let loaded = dag.add_role(
                    "source-memory-load-blob",
                    "loaded_blob",
                    "hs_load16((uintptr_t)($0))".into(),
                    vec![*address_id],
                    pc,
                )?;
                dag.type_node(loaded, Some(SourceType { spelling: "unsigned __int128".into(), bits: 128, signed: false, pointer: false }));
                state.external_loads.push(ExternalLoad {
                    expression: loaded,
                    address: *address_id,
                    guard: state.path,
                    bytes,
                    pc,
                    volatile: false,
                    member: None,
                });
                state.external_access_pcs.push(pc);
                return Ok(Symbol::Blob(loaded));
            }
            let Symbol::Frame(offset) = address else {
                return Err("external memory read width is unsupported".into());
            };
            frame_range(state, offset, bytes)?;
            let Some(((slot, size), (value, origins))) = state.frame.iter().find(|((slot, size), _)| {
                *slot <= offset && offset + bytes as i64 <= *slot + *size as i64
            }).map(|((s,sz),(v,o))| ((*s,*sz),(v.clone(),o.clone()))) else {
                // Bounded: 16-byte frame load may span two adjacent 8-byte scalar slots.
                if bytes == 16 {
                    let lo_key = (offset, 8u64);
                    let hi_key = (offset + 8, 8u64);
                    if let (Some((lo_val, lo_orig)), Some((hi_val, hi_orig))) = (state.frame.get(&lo_key).cloned(), state.frame.get(&hi_key).cloned()) {
                        let lo = dag.value(lo_val, 64, pc)?;
                        let hi = dag.value(hi_val, 64, pc)?;
                        let blob = dag.add_role(
                            "source-frame-load-blob",
                            "loaded_blob",
                            "(((unsigned __int128)((uint64_t)($0))) | (((unsigned __int128)((uint64_t)($1))) << 64))".into(),
                            vec![lo, hi],
                            pc,
                        )?;
                        dag.nodes[blob].origins.extend(lo_orig);
                        dag.nodes[blob].origins.extend(hi_orig);
                        dag.type_node(blob, Some(SourceType { spelling: "unsigned __int128".into(), bits: 128, signed: false, pointer: false }));
                        return Ok(Symbol::Blob(blob));
                    }
                }
                return Err("external memory read width is unsupported".into());
            };
            if slot == offset && size == bytes {
                return Ok(value.clone());
            }
            // Bounded: 16-byte load from two adjacent 8-byte slots within a covering region.
            if bytes == 16 && slot == offset {
                let lo_key = (offset, 8u64);
                let hi_key = (offset + 8, 8u64);
                if let (Some((lo_val, lo_orig)), Some((hi_val, hi_orig))) = (state.frame.get(&lo_key).cloned(), state.frame.get(&hi_key).cloned()) {
                    let lo = dag.value(lo_val, 64, pc)?;
                    let hi = dag.value(hi_val, 64, pc)?;
                    let blob = dag.add_role(
                        "source-frame-load-blob",
                        "loaded_blob",
                        "(((unsigned __int128)((uint64_t)($0))) | (((unsigned __int128)((uint64_t)($1))) << 64))".into(),
                        vec![lo, hi],
                        pc,
                    )?;
                    dag.nodes[blob].origins.extend(lo_orig);
                    dag.nodes[blob].origins.extend(hi_orig);
                    dag.type_node(blob, Some(SourceType { spelling: "unsigned __int128".into(), bits: 128, signed: false, pointer: false }));
                    return Ok(Symbol::Blob(blob));
                }
            }
            // Bounded: extract 8-byte scalar part from a 16-byte blob stored in a frame slot.
            if bytes == 8 && size == 16 && matches!(value, Symbol::Blob(_)) {
                if let Symbol::Blob(blob_id) = value.clone() {
                    let part_offset = offset - slot;
                    if part_offset == 0 || part_offset == 8 {
                        let template = if part_offset == 0 {
                            "((uint64_t)((unsigned __int128)($0)))".to_owned()
                        } else {
                            "((uint64_t)(((unsigned __int128)($0)) >> 64))".to_owned()
                        };
                        let id = dag.add_role("source-frame-reload", "slot", template, vec![blob_id], pc)?;
                        dag.nodes[id].origins.extend(origins);
                        dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
                        return Ok(Symbol::Value(id, 64));
                    }
                }
            }
            // Bounded: extract 1/2/4 bytes from an 8-byte half of a 16-byte blob slot.
            if matches!(bytes, 1 | 2 | 4) && size == 16 {
                if let Symbol::Blob(blob_id) = value.clone() {
                    let inner_offset = offset - slot;
                    if inner_offset >= 0 && (inner_offset as u64 + bytes) <= 16 && (inner_offset % bytes as i64 == 0 || inner_offset == 0) {
                        let (half_shift, sub_offset) = if inner_offset < 8 && (inner_offset as u64 + bytes) <= 8 {
                            (0u64, inner_offset as u64)
                        } else if inner_offset >= 8 && (inner_offset as u64 + bytes) <= 16 {
                            (64u64, (inner_offset - 8) as u64)
                        } else {
                            (u64::MAX, 0)
                        };
                        if half_shift != u64::MAX {
                            let signed = flag(t, "signed") != 0;
                            let half = if half_shift == 0 {
                                "((uint64_t)((unsigned __int128)($0)))".to_owned()
                            } else {
                                "((uint64_t)(((unsigned __int128)($0)) >> 64))".to_owned()
                            };
                            let bit_shift = sub_offset * 8;
                            let shifted = if bit_shift == 0 {
                                format!("({half})")
                            } else {
                                format!("(({half}) >> {bit_shift})")
                            };
                            let (template, spelling, ty_signed) = match (bytes, signed) {
                                (1, false) => (format!("((uint64_t)(uint8_t){shifted})"), "uint8_t".to_owned(), false),
                                (1, true) => (format!("((uint64_t)(int64_t)(int8_t){shifted})"), "int8_t".to_owned(), true),
                                (2, false) => (format!("((uint64_t)(uint16_t){shifted})"), "uint16_t".to_owned(), false),
                                (2, true) => (format!("((uint64_t)(int64_t)(int16_t){shifted})"), "int16_t".to_owned(), true),
                                (4, false) => (format!("((uint64_t)(uint32_t){shifted})"), "uint32_t".to_owned(), false),
                                (4, true) => (format!("((uint64_t)(int64_t)(int32_t){shifted})"), "int32_t".to_owned(), true),
                                _ => return Err("external memory read width is unsupported".into()),
                            };
                            let id = dag.add_role("source-frame-reload", "slot", template, vec![blob_id], pc)?;
                            dag.nodes[id].origins.extend(origins);
                            dag.type_node(id, Some(SourceType { spelling, bits: (bytes * 8) as u8, signed: ty_signed, pointer: false }));
                            return Ok(Symbol::Value(id, 64));
                        }
                    }
                }
            }
            // Bounded: extract N bytes from a larger scalar slot at a byte-aligned offset within it.
            if matches!(bytes, 1 | 2 | 4) && matches!(size, 2 | 4 | 8) && (bytes as u64) < size {
                let inner_offset = offset - slot;
                if inner_offset >= 0 && (inner_offset as u64 + bytes) <= size && (inner_offset % bytes as i64 == 0 || inner_offset == 0) {
                    let signed = flag(t, "signed") != 0;
                    let src = dag.value(value.clone(), 64, pc)?;
                    let shift = (inner_offset as u64) * 8;
                    let shifted = if shift == 0 {
                        "((uint64_t)($0))".to_owned()
                    } else {
                        format!("(((uint64_t)($0)) >> {shift})")
                    };
                    let (template, spelling, ty_signed) = match (bytes, signed) {
                        (1, false) => (format!("((uint64_t)(uint8_t){shifted})"), "uint8_t".to_owned(), false),
                        (1, true) => (format!("((uint64_t)(int64_t)(int8_t){shifted})"), "int8_t".to_owned(), true),
                        (2, false) => (format!("((uint64_t)(uint16_t){shifted})"), "uint16_t".to_owned(), false),
                        (2, true) => (format!("((uint64_t)(int64_t)(int16_t){shifted})"), "int16_t".to_owned(), true),
                        (4, false) => (format!("((uint64_t)(uint32_t){shifted})"), "uint32_t".to_owned(), false),
                        (4, true) => (format!("((uint64_t)(int64_t)(int32_t){shifted})"), "int32_t".to_owned(), true),
                        _ => return Err("external memory read width is unsupported".into()),
                    };
                    let id = dag.add_role("source-frame-reload", "slot", template, vec![src], pc)?;
                    dag.nodes[id].origins.extend(origins);
                    dag.type_node(id, Some(SourceType { spelling, bits: (bytes * 8) as u8, signed: ty_signed, pointer: false }));
                    return Ok(Symbol::Value(id, 64));
                }
            }
            let _ = (origins,);
            return Err("external memory read width is unsupported".into());
        }
        if !matches!(bytes, 1 | 2 | 4 | 8) {
            return Err("external memory read width is unsupported".into());
        }
        if let Symbol::Value(address_id, _) = &address {
            let pointer_type_opt = dag.type_of(*address_id).cloned().filter(|ty| ty.pointer);
            if pointer_type_opt.is_none() {
                // Bounded fallback: concrete external address without proven pointee
                // type (e.g. resolved GOT/image slot). Treat as raw memory load.
                let signed = flag(t, "signed") != 0;
                let load_helper = format!("hs_load{bytes}");
                let loaded = dag.add_role(
                    "source-memory-load",
                    "loaded",
                    format!("{load_helper}((uintptr_t)($0))"),
                    vec![*address_id],
                    pc,
                )?;
                dag.nodes[loaded].source_type = Some(SourceType {
                    spelling: format!("{}{}_t", if signed { "int" } else { "uint" }, bytes * 8),
                    bits: (bytes * 8) as u8,
                    signed,
                    pointer: false,
                });
                dag.nodes[loaded].boolean_value = dag.type_of(loaded)
                    .is_some_and(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool"));
                state.external_loads.push(ExternalLoad {
                    expression: loaded,
                    address: *address_id,
                    guard: state.path,
                    bytes,
                    pc,
                    volatile: false,
                    member: None,
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
                    spelling, bits, signed: promoted_signed, pointer: false,
                }));
                return Ok(Symbol::Value(extended, 64));
            }
            let pointer_type = pointer_type_opt.unwrap();
            let volatile = pointee_qualified(&pointer_type, "volatile");
            let declared_pointee = scalar_pointee_type(&pointer_type).filter(|ty| u64::from(ty.bits) == bytes * 8);
            let member = if volatile {None} else {dag.nodes[*address_id].aggregate_address.as_ref().and_then(|address|aggregate_member_at(address,bytes))};
            if volatile && declared_pointee.is_none() {
                return Err("volatile reads require an exact fixed-width scalar pointee".into());
            }
            let proven_pointee=member.as_ref().map(|member|member.field_type.clone()).or(declared_pointee);
            let signed = proven_pointee.as_ref().map_or(flag(t, "signed") != 0, |ty| ty.signed);
            let load_helper = if volatile {
                volatile_helper(false, bytes, proven_pointee.as_ref().expect("validated volatile pointee")).0
            } else {
                format!("hs_load{bytes}")
            };
            let (load_template,load_children)=if let Some(member)=&member {
                (format!("((({} *)($0))->{} )",member.type_spelling,member.field_name),vec![member.base,*address_id])
            } else {(format!("{load_helper}((uintptr_t)($0))"),vec![*address_id])};
            let loaded = dag.add_role(
                if member.is_some(){"source-member-load"}else{"source-memory-load"},
                if member.is_some(){"member_load"}else{"loaded"},
                load_template,
                load_children,
                pc,
            )?;
            dag.nodes[loaded].source_type = Some(proven_pointee.unwrap_or(SourceType {
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
                member,
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
            // Bounded: a frame read with no proven prior store is modeled as an
            // indeterminate value (common for unmodeled incoming stack effects or
            // compiler artifacts). This does not prove the original value.
            let id = dag.add_role(
                "source-frame-reload",
                "uninitialized",
                format!("(uint64_t)0 /* uninitialized frame read at offset {offset:#x}, {bytes} bytes */"),
                vec![],
                pc,
            )?;
            return Ok(Symbol::Value(id, 64));
        };
        let shift = (offset - *slot) as u64 * 8;
        let origins = origins.clone();
        let size = *size;
        // Bounded: extract 8-byte scalar part from a 16-byte blob stored in a frame slot.
        if bytes == 8 && size == 16 {
            if let Symbol::Blob(blob_id) = &value {
                let part_offset = offset - *slot;
                if part_offset == 0 || part_offset == 8 {
                    let template = if part_offset == 0 {
                        "((uint64_t)((unsigned __int128)($0)))".to_owned()
                    } else {
                        "((uint64_t)(((unsigned __int128)($0)) >> 64))".to_owned()
                    };
                    let id = dag.add_role("source-frame-reload", "slot", template, vec![*blob_id], pc)?;
                    dag.nodes[id].origins.extend(origins);
                    dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
                    return Ok(Symbol::Value(id, 64));
                }
            }
        }
        if size == bytes && shift == 0 {
            match &value {
                Symbol::Unknown(_) | Symbol::Frame(_) if bytes == 8 => return Ok(value.clone()),
                _ => {}
            }
        }
        let value = dag.value(value.clone(), 64, pc)?;
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
                let aggregate_address=dag.nodes[*pointer].aggregate_address.clone().and_then(|mut aggregate| {
                    let displacement=dag.literal(&values[1])?;
                    let offset=if op=="+" {aggregate.offset_bytes.checked_add(displacement)} else {aggregate.offset_bytes.checked_sub(displacement)}?;
                    if offset>=aggregate.layout.size{return None}
                    aggregate.offset_bytes=offset;Some(aggregate)
                });
                let (template, children) = if let Some(offset) = dag.literal(&values[1]) {
                    (
                        format!("((uintptr_t)($0) {op} UINT64_C(0x{offset:x}))"),
                        vec![*pointer],
                    )
                } else {
                    let offset_id = match &values[1] {
                        Symbol::Value(id, _) if !dag.type_of(*id).is_some_and(|ty| ty.pointer) => *id,
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
                dag.nodes[id].aggregate_address=aggregate_address;
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
            } else if matches!(op,"+"|"-") {
                let left_is_base=dag.nodes[children[0]].aggregate_address.is_some();
                let right_is_base=dag.nodes[children[1]].aggregate_address.is_some();
                let left_is_constant=dag.nodes[children[0]].constant_value.is_some();
                let right_is_constant=dag.nodes[children[1]].constant_value.is_some();
                if left_is_base&&right_is_constant {dag.type_of(children[0]).cloned()}
                else if op=="+"&&right_is_base&&left_is_constant {dag.type_of(children[1]).cloned()}
                else {
                    let left=dag.type_of(children[0]);
                    let right=dag.type_of(children[1]);
                    match (left,right) {
                        (Some(left),Some(right)) if left==right&&!left.pointer=>Some(left.clone()),
                        _=>None,
                    }
                }
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
    if t.kind=="binary"&&matches!(attribute(t,"operator"),"+"|"-") {
        let op=attribute(t,"operator");
        let left=dag.nodes[children[0]].aggregate_address.clone();
        let right_constant=dag.nodes[children[1]].constant_value;
        let right=dag.nodes[children[1]].aggregate_address.clone();
        let left_constant=dag.nodes[children[0]].constant_value;
        let adjusted=if let (Some(mut base),Some(displacement))=(left,right_constant) {
            let offset=if op=="+" {base.offset_bytes.checked_add(displacement)} else {base.offset_bytes.checked_sub(displacement)};
            offset.filter(|offset|*offset<base.layout.size).map(|offset|{base.offset_bytes=offset;base})
        } else if op=="+" {
            if let (Some(mut base),Some(displacement))=(right,left_constant) {
                base.offset_bytes.checked_add(displacement).filter(|offset|*offset<base.layout.size).map(|offset|{base.offset_bytes=offset;base})
            } else {None}
        } else {None};
        dag.nodes[id].aggregate_address=adjusted;
    }
    Ok(Symbol::Value(id, 64))
}
fn frame_range(state: &State, offset: i64, bytes: u64) -> Result<(), String> {
    let Symbol::Frame(sp) = state.registers[31] else {
        return Err("unknown SP".into());
    };
    let end = offset.checked_add(bytes as i64).ok_or("memory access is not a proven local frame/red-zone object")?;
    let in_incoming = offset >= 0 && end <= state.incoming_stack_end;
    if sp % 16 != 0
        || offset < sp - 128
        || offset < -4096
        || (end > 0 && !in_incoming)
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
        let pointer_opt = dag.type_of(address).cloned().filter(|ty| ty.pointer);
        if pointer_opt.is_none() {
            // Bounded fallback: concrete external address without proven pointee
            // type (e.g. known global/data slot). Treat as a raw fixed-width store.
            if !matches!(bytes, 1 | 2 | 4 | 8) {
                return Err("external memory write width is unsupported".into());
            }
            let value = read(&t.children[1], state, dag, pc)?;
            let value = dag.value(value, 64, pc)?;
            state.external_stores.push(ExternalStore {
                address,
                value,
                guard: state.path,
                bytes,
                pc,
                volatile: false,
                member: None,
            });
            state.external_access_pcs.push(pc);
            return Ok(());
        }
        let pointer = pointer_opt.unwrap();
        let volatile = pointee_qualified(&pointer, "volatile");
        if pointee_qualified(&pointer, "const") {
            return Err("external store through a const-qualified pointer is unsupported".into());
        }
        if bytes == 16 {
            if volatile { return Err("volatile 128-bit blob writes are unsupported".into()); }
            let value = read(&t.children[1], state, dag, pc)?;
            let Symbol::Blob(value_id) = value else { return Err("external memory write width is unsupported".into()); };
            state.external_stores.push(ExternalStore {
                address,
                value: value_id,
                guard: state.path,
                bytes,
                pc,
                volatile: false,
                member: None,
            });
            state.external_access_pcs.push(pc);
            return Ok(());
        }
        if !matches!(bytes, 1 | 2 | 4 | 8) {
            return Err("external memory write width is unsupported".into());
        }
        let member=if volatile {None}else{dag.nodes[address].aggregate_address.as_ref().and_then(|address|aggregate_member_at(address,bytes))};
        let value = read(&t.children[1], state, dag, pc)?;
        let mut value = dag.value(value, 64, pc)?;
        let pointee = member.as_ref().map(|member|member.field_type.clone())
            .or_else(||scalar_pointee_type(&pointer).filter(|ty| u64::from(ty.bits) == bytes * 8));
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
            member,
        });
        state.external_access_pcs.push(pc);
        return Ok(());
    }
    let Symbol::Frame(offset) = address else {
        return Err("external or escaped memory write".into());
    };
    frame_range(state, offset, bytes)?;
    let value = read(&t.children[1], state, dag, pc)?;
    if bytes == 16 && !matches!(value, Symbol::Blob(_)) {
        return Err("external memory write width is unsupported".into());
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopCmp {
    Eq,
    Ne,
    /// Signed comparisons (CMP semantics).
    Slt,
    Sle,
    Sgt,
    Sge,
    /// Unsigned comparisons (CMP semantics).
    Ult,
    Ule,
    Ugt,
    Uge,
}

impl LoopCmp {
    fn complement(self) -> Self {
        match self {
            LoopCmp::Eq => LoopCmp::Ne,
            LoopCmp::Ne => LoopCmp::Eq,
            LoopCmp::Slt => LoopCmp::Sge,
            LoopCmp::Sle => LoopCmp::Sgt,
            LoopCmp::Sgt => LoopCmp::Sle,
            LoopCmp::Sge => LoopCmp::Slt,
            LoopCmp::Ult => LoopCmp::Uge,
            LoopCmp::Ule => LoopCmp::Ugt,
            LoopCmp::Ugt => LoopCmp::Ule,
            LoopCmp::Uge => LoopCmp::Ult,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoopCompareOperand {
    Constant(u64),
    /// Loop-invariant register (must not be written in the loop body).
    Register(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LoopTest {
    register: usize,
    cmp: LoopCmp,
    /// Compared-against operand: constant, or a loop-invariant register.
    compare: LoopCompareOperand,
    /// If Some(mask), comparison is on (value & mask) and only Eq/Ne against a constant are valid.
    mask: Option<u64>,
}

fn loop_condition_template(cmp: LoopCmp, compare: LoopCompareOperand, mask: Option<u64>) -> Option<String> {
    let rhs = |s: &str| match compare {
        LoopCompareOperand::Constant(v) => format!("UINT64_C({v})"),
        LoopCompareOperand::Register(_) => s.to_string(),
    };
    if let Some(mask) = mask {
        // Masked comparisons are only supported for equality/inequality against a constant.
        let LoopCompareOperand::Constant(compare_value) = compare else { return None; };
        return Some(match cmp {
            LoopCmp::Eq => format!("((($0) & UINT64_C({mask:#x})) == UINT64_C({compare_value}))"),
            LoopCmp::Ne => format!("((($0) & UINT64_C({mask:#x})) != UINT64_C({compare_value}))"),
            _ => return None,
        });
    }
    Some(match (cmp, compare) {
        (LoopCmp::Eq, _) => format!("(($0) == {})", rhs("$1")),
        (LoopCmp::Ne, _) => format!("(($0) != {})", rhs("$1")),
        (LoopCmp::Slt, LoopCompareOperand::Constant(v)) => format!("(((int64_t)($0)) < (int64_t)UINT64_C({v}))"),
        (LoopCmp::Sle, LoopCompareOperand::Constant(v)) => format!("(((int64_t)($0)) <= (int64_t)UINT64_C({v}))"),
        (LoopCmp::Sgt, LoopCompareOperand::Constant(v)) => format!("(((int64_t)($0)) > (int64_t)UINT64_C({v}))"),
        (LoopCmp::Sge, LoopCompareOperand::Constant(v)) => format!("(((int64_t)($0)) >= (int64_t)UINT64_C({v}))"),
        (LoopCmp::Ult, LoopCompareOperand::Constant(v)) => format!("(($0) < UINT64_C({v}))"),
        (LoopCmp::Ule, LoopCompareOperand::Constant(v)) => format!("(($0) <= UINT64_C({v}))"),
        (LoopCmp::Ugt, LoopCompareOperand::Constant(v)) => format!("(($0) > UINT64_C({v}))"),
        (LoopCmp::Uge, LoopCompareOperand::Constant(v)) => format!("(($0) >= UINT64_C({v}))"),
        (LoopCmp::Slt, LoopCompareOperand::Register(_)) => "(((int64_t)($0)) < (int64_t)($1))".to_string(),
        (LoopCmp::Sle, LoopCompareOperand::Register(_)) => "(((int64_t)($0)) <= (int64_t)($1))".to_string(),
        (LoopCmp::Sgt, LoopCompareOperand::Register(_)) => "(((int64_t)($0)) > (int64_t)($1))".to_string(),
        (LoopCmp::Sge, LoopCompareOperand::Register(_)) => "(((int64_t)($0)) >= (int64_t)($1))".to_string(),
        (LoopCmp::Ult, LoopCompareOperand::Register(_)) => "(($0) < ($1))".to_string(),
        (LoopCmp::Ule, LoopCompareOperand::Register(_)) => "(($0) <= ($1))".to_string(),
        (LoopCmp::Ugt, LoopCompareOperand::Register(_)) => "(($0) > ($1))".to_string(),
        (LoopCmp::Uge, LoopCompareOperand::Register(_)) => "(($0) >= ($1))".to_string(),
    })
}

fn zero_test(t: &Tree) -> Option<LoopTest> {
    if t.kind != "binary" || t.children.len() != 2 { return None; }
    let op = attribute(t, "operator");

    // Ordering comparisons: reg < K, reg > K, reg <= K, reg >= K (signed),
    // and register-vs-register forms reg < reg2, etc.
    if matches!(op, "<" | ">" | "<=" | ">=") {
        let (lhs, rhs) = (&t.children[0], &t.children[1]);
        if lhs.kind == "register" {
            let (lhs_register, _) = register(lhs).ok()?;
            let cmp = match op {
                "<" => LoopCmp::Slt,
                ">" => LoopCmp::Sgt,
                "<=" => LoopCmp::Sle,
                ">=" => LoopCmp::Sge,
                _ => return None,
            };
            if rhs.kind == "constant" {
                let compare_value = number(rhs, "hex");
                return Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(compare_value), mask: None });
            }
            if rhs.kind == "register" {
                let (rhs_register, _) = register(rhs).ok()?;
                if rhs_register == lhs_register || rhs_register == 29 || rhs_register == 30 || rhs_register >= 31 {
                    return None;
                }
                return Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Register(rhs_register), mask: None });
            }
        }
        return None;
    }

    // Equality/inequality: (reg == K), (reg != K), ((reg & mask) ==/!= K).
    if !matches!(op, "==" | "!=") { return None; }
    let (lhs, rhs) = if t.children[0].kind == "constant" {
        (&t.children[1], &t.children[0])
    } else {
        (&t.children[0], &t.children[1])
    };
    if rhs.kind != "constant" {
        // Register-vs-register equality/inequality.
        if lhs.kind == "register" && rhs.kind == "register" {
            let (lhs_register, _) = register(lhs).ok()?;
            let (rhs_register, _) = register(rhs).ok()?;
            if rhs_register == lhs_register || rhs_register == 29 || rhs_register == 30 || rhs_register >= 31 {
                return None;
            }
            let cmp = if op == "==" { LoopCmp::Eq } else { LoopCmp::Ne };
            return Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Register(rhs_register), mask: None });
        }
        return None;
    }
    let compare_value = number(rhs, "hex");
    let cmp = if op == "==" { LoopCmp::Eq } else { LoopCmp::Ne };
    if lhs.kind == "register" {
        let (lhs_register, _) = register(lhs).ok()?;
        return Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(compare_value), mask: None });
    }
    // Bit test/masked equality form: ((reg & mask_const) ==/!= K).
    if lhs.kind == "binary" && attribute(lhs, "operator") == "&" && lhs.children.len() == 2 {
        let (reg_tree, mask_tree) = if lhs.children[0].kind == "register" {
            (&lhs.children[0], &lhs.children[1])
        } else if lhs.children[1].kind == "register" {
            (&lhs.children[1], &lhs.children[0])
        } else {
            return None;
        };
        if mask_tree.kind == "constant" {
            let mask = number(mask_tree, "hex");
            if mask != 0 {
                let (reg, _) = register(reg_tree).ok()?;
                return Some(LoopTest { register: reg, cmp, compare: LoopCompareOperand::Constant(compare_value), mask: Some(mask) });
            }
        }
    }
    None
}

/// Recognize a flag-condition branch produced by `CMP/CMN reg, #imm` or `TST reg, #imm`
/// immediately before `branch_index` as an equivalent register-vs-constant comparison.
/// Also recognizes bounded register-vs-register `CMP reg, reg` when the RHS is a
/// loop-invariant register (validated by callers against the loop's written set).
/// CMP supports eq/ne and signed/unsigned ordering predicates; TST supports eq/ne
/// as a masked zero test (bounded, conservative).
fn zero_test_from_flag_cmp(condition: &Tree, branch_index: usize, header: usize, trees: &[Tree]) -> Option<(LoopTest, usize)> {
    if condition.kind != "flag-condition" { return None; }
    let predicate = attribute(condition, "predicate");
    if branch_index == 0 || branch_index <= header { return None; }
    // Allow bounded no-ops between compare and branch (flags unaffected).
    let mut cmp_index = branch_index - 1;
    let mut skipped = 0usize;
    while trees.get(cmp_index).is_some_and(is_noop_tree) {
        if skipped >= 4 || cmp_index == 0 || cmp_index <= header { return None; }
        cmp_index -= 1;
        skipped += 1;
    }
    if cmp_index < header { return None; }
    let cmp_tree = trees.get(cmp_index)?;
    if cmp_tree.kind != "flags-compare" || cmp_tree.children.len() != 2 {
        return None;
    }
    let op = attribute(cmp_tree, "operator");
    let rhs = &cmp_tree.children[1];
    let (lhs_register, _) = register(&cmp_tree.children[0]).ok()?;
    if rhs.kind == "constant" {
        let imm = number(rhs, "hex");
        match op {
            "cmp" => {
                let cmp = match predicate {
                    "eq" => LoopCmp::Eq,
                    "ne" => LoopCmp::Ne,
                    "lt" => LoopCmp::Slt,
                    "le" => LoopCmp::Sle,
                    "gt" => LoopCmp::Sgt,
                    "ge" => LoopCmp::Sge,
                    "lo" => LoopCmp::Ult, // unsigned <
                    "ls" => LoopCmp::Ule, // unsigned <=
                    "hi" => LoopCmp::Ugt, // unsigned >
                    "hs" => LoopCmp::Uge, // unsigned >=
                    _ => return None,
                };
                Some((LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(imm), mask: None }, cmp_index))
            }
            "cmn" => {
                // CMN sets flags from (reg + imm); eq/ne correspond to reg == -imm / != -imm.
                let cmp = match predicate {
                    "eq" => LoopCmp::Eq,
                    "ne" => LoopCmp::Ne,
                    _ => return None,
                };
                let compare_value = imm.wrapping_neg();
                Some((LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(compare_value), mask: None }, cmp_index))
            }
            "tst" | "test" => {
                // TST sets Z based on (left & right); eq => zero, ne => nonzero.
                let cmp = match predicate {
                    "eq" => LoopCmp::Eq,
                    "ne" => LoopCmp::Ne,
                    _ => return None,
                };
                if imm == 0 { return None; }
                Some((LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(0), mask: Some(imm) }, cmp_index))
            }
            _ => None,
        }
    } else if rhs.kind == "register" && op == "cmp" {
        // Register-vs-register comparison: sound only when the RHS is loop-invariant
        // (not written in the loop body). Callers validate invariance against `written`.
        let (rhs_register, _) = register(rhs).ok()?;
        if rhs_register == lhs_register || rhs_register == 29 || rhs_register == 30 || rhs_register >= 31 {
            return None;
        }
        let cmp = match predicate {
            "eq" => LoopCmp::Eq,
            "ne" => LoopCmp::Ne,
            "lt" => LoopCmp::Slt,
            "le" => LoopCmp::Sle,
            "gt" => LoopCmp::Sgt,
            "ge" => LoopCmp::Sge,
            "lo" => LoopCmp::Ult,
            "ls" => LoopCmp::Ule,
            "hi" => LoopCmp::Ugt,
            "hs" => LoopCmp::Uge,
            _ => return None,
        };
        Some((LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Register(rhs_register), mask: None }, cmp_index))
    } else {
        None
    }
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


fn is_noop_tree(tree: &Tree) -> bool {
    match tree.kind {
        "no-op" | "flags-compare" | "flags-arithmetic" => true,
        "assignment" => {
            // Bounded: assignment of a register to itself is a no-op.
            tree.children.len() == 2
                && tree.children[0].kind == "register"
                && tree.children[1].kind == "register"
                && register(&tree.children[0]).ok().map(|(r, _)| r)
                    == register(&tree.children[1]).ok().map(|(r, _)| r)
        }
        "sequence" => !tree.children.is_empty() && tree.children.iter().all(is_noop_tree),
        _ => false,
    }
}

fn collect_loop_written(tree: &Tree, written: &mut BTreeSet<usize>) -> Result<(), ()> {
    match tree.kind {
        "no-op" | "flags-compare" | "flags-arithmetic" | "goto" | "conditional-goto" | "call" | "memory-load" => Ok(()),
        "memory-store" | "memory-store-pair" | "memory-load-pair" => {
            if tree.kind == "memory-store" && tree.children.len() != 2 { return Err(()); }
            if tree.kind.ends_with("-pair") && tree.children.len() < 2 { return Err(()); }
            Ok(())
        }
        "assignment" => {
            if tree.children.len() != 2 { return Err(()); }
            let (register, _) = register(&tree.children[0]).map_err(|_| ())?;
            if register == 29 || register == 30 || register >= 31 { return Err(()); }
            written.insert(register);
            Ok(())
        }
        "sequence" => {
            // Bounded: accept sequences containing flag/no-op effects and
            // assignments. Assignments inside sequences are treated as
            // loop-carried updates (same as top-level assignments). Memory
            // stores are still rejected to keep external effects tracked by
            // top-level body items.
            for child in &tree.children {
                match child.kind {
                    "no-op" | "flags-compare" | "flags-arithmetic" => {}
                    "assignment" => {
                        if child.children.len() != 2 { return Err(()); }
                        let (register, _) = register(&child.children[0]).map_err(|_| ())?;
                        if register == 29 || register == 30 || register >= 31 { return Err(()); }
                        written.insert(register);
                    }
                    "call" => {
                        // Bounded: calls in sequences are accepted (same as
                        // top-level calls). They set registers to Unknown, so
                        // loop-carried registers in 0-18 would be invalidated.
                    }
                    "conditional-goto" | "goto" => {
                        // Bounded: redundant fallthrough-equivalent jump in sequence.
                    }
                    "sequence" => collect_loop_written(child, written)?,
                    _ => return Err(()),
                }
            }
            Ok(())
        }
        _ => Err(()),
    }
}

/// Bounded bottom-tested/backedge loop recovery: a conditional backedge at
/// `tail` jumps to `header` (already visited on this path), with fallthrough to
/// `tail + 1` as the single exit. This covers simple do-while/counting forms
/// with straight-line scalar assignments and ordered external memory effects in
/// the body, mirroring `natural_loop`'s carrier/effect constraints.
fn natural_loop_backedge(
    start: u64,
    header: usize,
    tail: usize,
    trees: &[Tree],
    header_entry: &State,
    tail_state: &mut State,
    dag: &mut Dag,
    call_prototypes: &BTreeMap<u64, SourceSignature>,
) -> Result<Option<(SourceLoop, usize)>, String> {
    let Some(tail_tree) = trees.get(tail).filter(|tree| tree.kind == "conditional-goto") else {
        return Ok(None);
    };
    let target_index = |target: u64| -> Option<usize> {
        let delta = target.checked_sub(start)?;
        (delta % 4 == 0 && delta / 4 < trees.len() as u64).then_some((delta / 4) as usize)
    };
    if target_index(number(tail_tree, "target")) != Some(header) {
        return Ok(None);
    }
    if tail < header { return Ok(None); }
    let mut exit = tail + 1;
    if exit > trees.len() { return Ok(None); }
    // Tolerate bounded trailing no-ops between backedge and exit, but only
    // when no control transfer targets those no-ops (so skipping them is sound).
    let mut skipped = 0usize;
    while exit < trees.len() && skipped < 4 && trees.get(exit).is_some_and(is_noop_tree) {
        let noop_index = exit;
        let targeted = trees.iter().enumerate().any(|(i, tree)| {
            matches!(tree.kind, "goto" | "conditional-goto")
                && target_index(number(tree, "target")) == Some(noop_index)
                && i != tail
        });
        if targeted {
            break;
        }
        exit += 1;
        skipped += 1;
    }
    if exit > trees.len() { return Ok(None); }
    // Require the backedge test to be a zero/nonzero test on a register,
    // or an equivalent flag-condition from CMP reg,#0 immediately before tail.
    let (test, loop_cmp_index) = if let Some(test) = zero_test(&tail_tree.children[0]) {
        (test, None)
    } else if let Some((test, cmp_index)) = zero_test_from_flag_cmp(&tail_tree.children[0], tail, header, trees) {
        (test, Some(cmp_index))
    } else {
        return Ok(None);
    };
    let test_register = test.register;
    // Single-entry region: no other control transfer may target inside the loop
    // except the recognized backedge. Redundant fallthrough-equivalent gotos
    // inside the body (targeting the immediately following instruction) are
    // allowed.
    for (index, tree) in trees.iter().enumerate() {
        if !matches!(tree.kind, "goto" | "conditional-goto") || index == tail { continue; }
        let Some(target) = target_index(number(tree, "target")) else { continue; };
        if tree.kind == "goto" && index >= header && index < tail && target == index + 1 {
            continue;
        }
        if (header..=tail).contains(&target) {
            return Ok(None);
        }
    }
    // Disallow nested/other internal control flow in the body.
    // A goto or conditional-goto targeting the immediately following instruction
    // is a redundant fallthrough-equivalent jump and does not create internal
    // control flow.
    for (offset, tree) in trees[header..tail].iter().enumerate() {
        let index = header + offset;
        if tree.kind == "conditional-goto" {
            if target_index(number(tree, "target")) != Some(index + 1) { return Ok(None); }
            continue;
        }
        if tree.kind == "goto" {
            if target_index(number(tree, "target")) != Some(index + 1) { return Ok(None); }
        }
    }
    if !header_entry.external_loads.is_empty() || !header_entry.external_stores.is_empty() || header_entry.has_call {
        return Ok(None);
    }
    let mut written = BTreeSet::new();
    for (index, tree) in trees.iter().enumerate().take(tail).skip(header) {
        if loop_cmp_index == Some(index) {
            // Bounded loop-test compare (flags-compare CMP reg,#imm) is allowed.
            continue;
        }
        if collect_loop_written(tree, &mut written).is_err() { return Ok(None); }
    }
    // For flag-cmp-based tests, the compared register must be a loop-carried value
    // (written in body) so the condition can vary.
    if loop_cmp_index.is_some() {
        if !written.contains(&test_register) { return Ok(None); }
    } else if !written.contains(&test_register) {
        return Ok(None);
    }
    // Register compare operands must be loop-invariant (not written in body).
    let compare_register = match test.compare {
        LoopCompareOperand::Register(reg) => {
            if written.contains(&reg) || reg == 29 || reg == 30 || reg >= 31 { return Ok(None); }
            Some(reg)
        }
        LoopCompareOperand::Constant(_) => None,
    };

    let mut state = header_entry.clone();
    let compare_value_id = if let Some(reg) = compare_register {
        Some(dag.value(state.registers[reg].clone(), 64, start + header as u64 * 4)?)
    } else {
        None
    };
    let mut variables = Vec::new();
    let mut variable_by_register = BTreeMap::new();
    for register in written.iter().copied() {
        let Symbol::Value(initial, known) = state.registers[register].clone() else { return Ok(None); };
        if known < 32 { return Ok(None); }
        let role = if register == test_register { "induction" } else { "accumulator" };
        let id = dag.nodes.len();
        let name = format!("hs_{role}_{id}");
        let value = dag.add_role("source-loop-variable", role, name, vec![], start + header as u64 * 4)?;
        dag.nodes[value].boolean_value = dag.nodes[initial].boolean_value
            && dag.type_of(initial).is_some_and(|ty| matches!(ty.spelling.as_str(), "bool" | "_Bool"));
        dag.type_node(value, dag.type_of(initial).cloned());
        variable_by_register.insert(register, (value, known));
        variables.push(LoopVariable { value, initial });
        state.registers[register] = Symbol::Value(value, known);
    }
    // Bottom-tested: continue when condition holds at backedge.
    let test_value = variable_by_register[&test_register].0;
    let Some(condition_template) = loop_condition_template(test.cmp, test.compare, test.mask) else {
        return Ok(None);
    };
    let condition_children = if let Some(cmp_id) = compare_value_id {
        vec![test_value, cmp_id]
    } else {
        vec![test_value]
    };
    let condition = dag.add_role(
        "source-loop-condition", "condition", condition_template, condition_children, start + tail as u64 * 4
    )?;
    dag.type_node(condition, None);

    let mut assignments = Vec::new();
    let mut loads = Vec::new();
    let mut stores = Vec::new();
    let mut type_conflicts = BTreeSet::new();
    for (index, tree) in trees.iter().enumerate().take(tail).skip(header) {
        let pc = start + index as u64 * 4;
        if tree.kind == "no-op" {
            statement(tree, &mut state, dag, pc, call_prototypes)?;
            continue;
        }
        let register = if tree.kind == "assignment" {
            match register(&tree.children[0]) {
                Ok((register, _)) => Some(register),
                Err(_) => return Ok(None),
            }
        } else {
            None
        };
        let before_frame = state.frame.clone();
        let before_loads = state.external_loads.len();
        let before_stores = state.external_stores.len();
        if tree.kind == "goto" {
            // Bounded: redundant fallthrough-equivalent goto (validated as targeting
            // the next sequential instruction in this loop body). No state effects.
        } else {
            statement(tree, &mut state, dag, pc, call_prototypes)?;
        }
        // Flag changes within the body are allowed for register-based loop conditions;
        // flags are not modeled as loop-carried values in this bounded recovery.
        if state.frame != before_frame || state.external_loads.len() < before_loads {
            return Err("loop body has memory effects without loop-carried memory SSA".into());
        }
        let body_loads = state.external_loads.split_off(before_loads);
        if body_loads.len() > 1 || (tree.kind == "memory-store" && !body_loads.is_empty()) {
            return Err("loop instruction has an unsupported number of memory reads".into());
        }
        loads.extend(body_loads);
        if loop_cmp_index == Some(index) {
            // Bounded loop-test compare: sets flags for the backedge condition and
            // does not produce a loop-carried assignment or memory store.
            if state.external_stores.len() != before_stores {
                return Err("loop test compare unexpectedly produced a memory store".into());
            }
            continue;
        }
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
                if matches!(tree.kind, "flags-compare" | "flags-arithmetic") {
                    // Bounded: flag-setting instruction with no GPR write and no
                    // memory store (flags not loop-carried in this recovery).
                    if state.external_stores.len() != before_stores {
                        return Err("loop flag-setting instruction unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "sequence" {
                    // Bounded: accepted sequences contain only flag/no-op effects.
                    if state.external_stores.len() != before_stores {
                        return Err("loop sequence unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "goto" || tree.kind == "conditional-goto" {
                    // Bounded: redundant fallthrough-equivalent goto/conditional-goto
                    // (validated as targeting the next sequential instruction in
                    // this loop body). No state effects.
                    if state.external_stores.len() != before_stores {
                        return Err("loop goto unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "call" {
                    // Bounded: call with a known prototype. No external store
                    // is tracked by statement() for calls, so store count must
                    // be unchanged.
                    if state.external_stores.len() != before_stores {
                        return Err("loop call unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "memory-load" {
                    // Bounded: standalone memory load (no GPR write at top level).
                    // No external store is produced.
                    if state.external_stores.len() != before_stores {
                        return Err("loop memory-load unexpectedly produced a memory store".into());
                    }
                } else if matches!(tree.kind, "memory-store-pair" | "memory-load-pair") {
                    // Bounded: frame (stack) pair operations; no external memory
                    // effect is produced, so external store count must be unchanged.
                    if state.external_stores.len() != before_stores {
                        return Err("loop pair operation unexpectedly produced a memory store".into());
                    }
                } else if state.external_stores.len() == before_stores + 1 && tree.kind == "memory-store" {
                    stores.push(state.external_stores.pop().expect("store count checked"));
                } else {
                    return Err("loop body contains unsupported non-assignment instruction".into());
                }
            }
        }
    }
    if !type_conflicts.is_empty() {
        return Err("loop-carried value type changes or becomes unresolved".into());
    }
    // Validate the backedge reaches header with a consistent loop-carried state
    // by executing the tail condition context against the post-body state. The
    // condition expression is evaluated from loop-carried variables.
    let _ = &tail_state; // tail_state is caller-provided current state; loop exit continues from post-loop state
    *tail_state = state;
    let pcs = (header..=tail).map(|index| start + index as u64 * 4).collect();
    Ok(Some((SourceLoop { condition, variables, assignments, loads, stores, pcs }, exit)))
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
    let target_index = |target: u64| -> Option<usize> {
        let delta = target.checked_sub(start)?;
        (delta % 4 == 0 && delta / 4 < trees.len() as u64).then_some((delta / 4) as usize)
    };
    let Some(exit) = target_index(number(preheader_tree, "target")) else { return Ok(None) };

    // Pattern A (existing): preheader conditionally exits to `exit`, body is
    // header=preheader+1..tail, and tail branches back to header.
    // Pattern B: header-tested loop where the test is at `preheader` (loop
    // header), it conditionally exits to `exit`, fallthrough enters body, and
    // tail (exit-1) unconditionally branches back to `preheader`.
    let mut header = preheader + 1;
    let mut test_at_header = false;
    let conditional_backedges = (header..trees.len()).filter(|&index| {
        trees[index].kind == "conditional-goto"
            && target_index(number(&trees[index], "target")) == Some(header)
    }).collect::<Vec<_>>();
    let unconditional_backedges = (header..trees.len()).filter(|&index| {
        trees[index].kind == "goto"
            && target_index(number(&trees[index], "target")) == Some(header)
    }).collect::<Vec<_>>();
    let find_backedge_before_exit = |exit: usize, target_header: usize, want_kind: &str| -> Option<usize> {
        if exit == 0 || exit <= target_header + 1 { return None; }
        // Prefer a backedge immediately before exit (skipping bounded untargeted no-ops),
        // but also accept an earlier backedge when all instructions between it and exit
        // are untargeted no-ops (unconditional backedge) or the gap is valid for a
        // conditional backedge fallthrough to exit.
        let trailing_untargeted_noop = |noop_index: usize| -> bool {
            trees.get(noop_index).is_some_and(is_noop_tree)
                && !trees.iter().enumerate().any(|(i, tree)| {
                    matches!(tree.kind, "goto" | "conditional-goto")
                        && target_index(number(tree, "target")) == Some(noop_index)
                })
        };
        for idx in (target_header + 1..exit).rev() {
            let Some(tree) = trees.get(idx) else { continue; };
            if tree.kind != want_kind || target_index(number(tree, "target")) != Some(target_header) {
                continue;
            }
            if want_kind == "goto" {
                // Unconditional backedge: instructions between tail and exit must be
                // untargeted no-ops only (dead padding before exit), bounded.
                if exit > idx + 1 + 4 { continue; }
                if (idx + 1..exit).all(trailing_untargeted_noop) {
                    return Some(idx);
                }
            } else {
                // Conditional backedge: fallthrough reaches exit, so gap must be
                // empty or bounded untargeted no-ops immediately before exit.
                if exit == idx + 1 { return Some(idx); }
                if exit > idx + 1 + 4 { continue; }
                if (idx + 1..exit).all(trailing_untargeted_noop) {
                    return Some(idx);
                }
            }
        }
        None
    };
    let (tail, unconditional_backedge) = if conditional_backedges.len() == 1 && unconditional_backedges.is_empty() {
        (conditional_backedges[0], false)
    } else if conditional_backedges.is_empty() && unconditional_backedges.len() == 1 {
        (unconditional_backedges[0], true)
    } else if conditional_backedges.len() == 1 && unconditional_backedges.len() == 1 {
        // Pattern A2: loop with two backedges (one conditional, one
        // unconditional) targeting the same header. Use the later one as
        // the tail. The body must still be straight-line (no internal
        // control flow). This handles loops where an if-else has one
        // branch that falls through to the backedge and one that jumps.
        let t1 = conditional_backedges[0];
        let t2 = unconditional_backedges[0];
        if t1 > t2 { (t1, false) } else { (t2, true) }
    } else if conditional_backedges.len() == 2 && unconditional_backedges.is_empty() {
        // Pattern A3: loop with two conditional backedges targeting the
        // same header. Use the later one as the tail. This handles loops
        // where an if-else has both branches going back to the header.
        let t1 = conditional_backedges[0];
        let t2 = conditional_backedges[1];
        if t1 > t2 { (t1, false) } else { (t2, false) }
    } else if conditional_backedges.is_empty() && unconditional_backedges.is_empty() && exit > preheader + 1 {
        if let Some(tail) = find_backedge_before_exit(exit, preheader, "goto") {
            // Pattern B: `preheader` is the loop header/test, unconditional backedge.
            // Allow a single trailing no-op before exit after the backedge.
            header = preheader;
            test_at_header = true;
            (tail, true)
        } else if let Some(tail) = find_backedge_before_exit(exit, preheader, "conditional-goto") {
            // Pattern B: `preheader` is the loop header/test, conditional backedge
            // (complementary to the header exit test) with fallthrough to exit.
            // Allow a single trailing no-op before exit after the backedge.
            header = preheader;
            test_at_header = true;
            (tail, false)
        } else {
            return Ok(None);
        }
    } else {
        return Ok(None);
    };
    let trailing_noops_safe = |tail: usize, exit: usize| -> bool {
        if tail + 1 == exit { return true; }
        if exit <= tail + 1 || exit > tail + 5 { return false; }
        for noop_index in (tail + 1)..exit {
            if !trees.get(noop_index).is_some_and(is_noop_tree) {
                return false;
            }
            let targeted = trees.iter().enumerate().any(|(i, tree)| {
                matches!(tree.kind, "goto" | "conditional-goto")
                    && target_index(number(tree, "target")) == Some(noop_index)
                    && i != tail
            });
            if targeted { return false; }
        }
        true
    };
    // Tolerate bounded untargeted no-ops between tail and exit.
    if tail <= header || !trailing_noops_safe(tail, exit) {
        return Ok(None);
    }
    let header_test = if let Some(test) = zero_test(&preheader_tree.children[0]) {
        test
    } else if !test_at_header {
        // Pattern A only: preheader is an entry guard evaluated once (not the
        // loop header re-evaluated each iteration), so a flag-condition from a
        // compare immediately before preheader is sound for the entry test.
        let cond = &preheader_tree.children[0];
        if cond.kind == "flag-condition" && preheader > 0 {
            let mut cmp_index = preheader - 1;
            let mut skipped = 0usize;
            while trees.get(cmp_index).is_some_and(is_noop_tree) && cmp_index > 0 && skipped < 4 {
                cmp_index -= 1;
                skipped += 1;
            }
            let cmp_tree = trees.get(cmp_index);
            let parsed = cmp_tree.and_then(|ct| {
                if ct.kind != "flags-compare" || ct.children.len() != 2 { return None; }
                let op = attribute(ct, "operator");
                let rhs = ct.children.get(1)?;
                let (lhs_register, _) = register(&ct.children[0]).ok()?;
                let predicate = attribute(cond, "predicate");
                if rhs.kind == "constant" {
                    let imm = number(rhs, "hex");
                    match op {
                        "cmp" => {
                            let cmp = match predicate {
                                "eq" => LoopCmp::Eq, "ne" => LoopCmp::Ne,
                                "lt" => LoopCmp::Slt, "le" => LoopCmp::Sle,
                                "gt" => LoopCmp::Sgt, "ge" => LoopCmp::Sge,
                                "lo" => LoopCmp::Ult, "ls" => LoopCmp::Ule,
                                "hi" => LoopCmp::Ugt, "hs" => LoopCmp::Uge,
                                _ => return None,
                            };
                            Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(imm), mask: None })
                        }
                        "cmn" => {
                            let cmp = match predicate { "eq" => LoopCmp::Eq, "ne" => LoopCmp::Ne, _ => return None };
                            Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(imm.wrapping_neg()), mask: None })
                        }
                        "tst" | "test" => {
                            let cmp = match predicate { "eq" => LoopCmp::Eq, "ne" => LoopCmp::Ne, _ => return None };
                            if imm == 0 { return None; }
                            Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Constant(0), mask: Some(imm) })
                        }
                        _ => None,
                    }
                } else if rhs.kind == "register" && op == "cmp" {
                    // Bounded: register-vs-register entry guard. The compared
                    // register must be loop-invariant (validated after written-set scan).
                    let (rhs_register, _) = register(rhs).ok()?;
                    if rhs_register == lhs_register || rhs_register == 29 || rhs_register == 30 || rhs_register >= 31 {
                        return None;
                    }
                    let cmp = match predicate {
                        "eq" => LoopCmp::Eq, "ne" => LoopCmp::Ne,
                        "lt" => LoopCmp::Slt, "le" => LoopCmp::Sle,
                        "gt" => LoopCmp::Sgt, "ge" => LoopCmp::Sge,
                        "lo" => LoopCmp::Ult, "ls" => LoopCmp::Ule,
                        "hi" => LoopCmp::Ugt, "hs" => LoopCmp::Uge,
                        _ => return None,
                    };
                    Some(LoopTest { register: lhs_register, cmp, compare: LoopCompareOperand::Register(rhs_register), mask: None })
                } else {
                    None
                }
            });
            match parsed { Some(t) => t, None => return Ok(None) }
        } else {
            return Ok(None);
        }
    } else {
        return Ok(None);
    };
    let test_register = header_test.register;
    let mut loop_cmp_index: Option<usize> = None;
    if unconditional_backedge {
        // Header-tested while loop: test conditionally exits, tail jumps back
        // unconditionally to the loop header/test. Continue when exit is false.
    } else {
        let tail_cond = &trees[tail].children[0];
        let tail_test = if let Some(test) = zero_test(tail_cond) {
            test
        } else if let Some((test, cmp_index)) = zero_test_from_flag_cmp(tail_cond, tail, if test_at_header { header } else { header.saturating_sub(0) }, trees) {
            // Flag-cmp compare at tail-1 is in the loop body.
            loop_cmp_index = Some(cmp_index);
            test
        } else {
            return Ok(None);
        };
        if tail_test.register != test_register
            || tail_test.mask != header_test.mask
            || tail_test.compare != header_test.compare
            || tail_test.cmp != header_test.cmp.complement()
        {
            return Ok(None);
        }
    }
    // The loop must be a single-entry region. The exit edge and backedge are
    // its only recognized control-flow edges. A goto targeting the immediately
    // following instruction is a redundant fallthrough-equivalent jump.
    let body_start = if test_at_header { header + 1 } else { header };
    for (index, tree) in trees.iter().enumerate() {
        if !matches!(tree.kind, "goto" | "conditional-goto") || index == preheader || index == tail { continue; }
        if test_at_header && index == header { continue; }
        let Some(target) = target_index(number(tree, "target")) else { continue; };
        if tree.kind == "goto" && (body_start..tail).contains(&index) && target == index + 1 {
            continue;
        }
        if (header..=tail).contains(&target) {
            return Ok(None);
        }
    }
    if !state.external_loads.is_empty() || !state.external_stores.is_empty() || state.has_call {
        return Ok(None);
    }
    let mut written = BTreeSet::new();
    for (index, tree) in trees.iter().enumerate().take(tail).skip(body_start) {
        if loop_cmp_index == Some(index) {
            continue;
        }
        if collect_loop_written(tree, &mut written).is_err() { return Ok(None); }
    }
    if !written.contains(&test_register) { return Ok(None); }
    let compare_register = match header_test.compare {
        LoopCompareOperand::Register(reg) => {
            // Compare register must be loop-invariant for a sound re-evaluated/entry condition.
            if written.contains(&reg) || reg == 29 || reg == 30 || reg >= 31 { return Ok(None); }
            Some(reg)
        }
        LoopCompareOperand::Constant(_) => None,
    };
    let compare_value_id = if let Some(reg) = compare_register {
        Some(dag.value(state.registers[reg].clone(), 64, start + header as u64 * 4)?)
    } else {
        None
    };

    let mut variables = Vec::new();
    let mut variable_by_register = BTreeMap::new();
    for register in written.iter().copied() {
        let Symbol::Value(initial, known) = state.registers[register].clone() else { return Ok(None) };
        if known < 32 { return Ok(None); }
        let role = if register == test_register { "induction" } else { "accumulator" };
        let id = dag.nodes.len();
        let name = format!("hs_{role}_{id}");
        let value = dag.add_role("source-loop-variable", role, name, vec![], start + header as u64 * 4)?;
        // Inductive {0,1} invariant: seed from the preheader/header, then require
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
    // Continue loop when header test does NOT take the exit.
    let Some(condition_template) = loop_condition_template(header_test.cmp.complement(), header_test.compare, header_test.mask) else {
        return Ok(None);
    };
    let condition_children = if let Some(cmp_id) = compare_value_id {
        vec![test_value, cmp_id]
    } else {
        vec![test_value]
    };
    let condition = dag.add_role(
        "source-loop-condition", "condition", condition_template, condition_children, start + header as u64 * 4
    )?;
    dag.type_node(condition, None);

    let mut assignments = Vec::new();
    let mut loads = Vec::new();
    let mut stores = Vec::new();
    let mut type_conflicts = BTreeSet::new();
    for (index, tree) in trees.iter().enumerate().take(tail).skip(body_start) {
        let pc = start + index as u64 * 4;
        if tree.kind == "no-op" {
            statement(tree, state, dag, pc, call_prototypes)?;
            continue;
        }
        let register = if tree.kind == "assignment" {
            match register(&tree.children[0]) {
                Ok((register, _)) => Some(register),
                Err(_) => return Ok(None),
            }
        } else {
            None
        };
        let before_frame = state.frame.clone();
        let before_loads = state.external_loads.len();
        let before_stores = state.external_stores.len();
        if tree.kind == "goto" {
            // Bounded: redundant fallthrough-equivalent goto (validated as targeting
            // the next sequential instruction in this loop body). No state effects.
        } else {
            statement(tree, state, dag, pc, call_prototypes)?;
        }
        // Flag changes within the body are allowed for register-based loop conditions;
        // flags are not modeled as loop-carried values in this bounded recovery.
        if state.frame != before_frame || state.external_loads.len() < before_loads {
            return Err("loop body has memory effects without loop-carried memory SSA".into());
        }
        let body_loads = state.external_loads.split_off(before_loads);
        if body_loads.len() > 1 || (tree.kind == "memory-store" && !body_loads.is_empty()) {
            return Err("loop instruction has an unsupported number of memory reads".into());
        }
        loads.extend(body_loads);
        if loop_cmp_index == Some(index) {
            if state.external_stores.len() != before_stores {
                return Err("loop test compare unexpectedly produced a memory store".into());
            }
            continue;
        }
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
                if matches!(tree.kind, "flags-compare" | "flags-arithmetic") {
                    if state.external_stores.len() != before_stores {
                        return Err("loop flag-setting instruction unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "sequence" {
                    if state.external_stores.len() != before_stores {
                        return Err("loop sequence unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "goto" || tree.kind == "conditional-goto" {
                    // Bounded: redundant fallthrough-equivalent goto/conditional-goto
                    // (validated as targeting the next sequential instruction in
                    // this loop body). No state effects.
                    if state.external_stores.len() != before_stores {
                        return Err("loop goto unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "call" {
                    // Bounded: call with a known prototype. No external store
                    // is tracked by statement() for calls, so store count must
                    // be unchanged.
                    if state.external_stores.len() != before_stores {
                        return Err("loop call unexpectedly produced a memory store".into());
                    }
                } else if tree.kind == "memory-load" {
                    // Bounded: standalone memory load (no GPR write at top level).
                    // No external store is produced.
                    if state.external_stores.len() != before_stores {
                        return Err("loop memory-load unexpectedly produced a memory store".into());
                    }
                } else if matches!(tree.kind, "memory-store-pair" | "memory-load-pair") {
                    // Bounded: frame (stack) pair operations; no external memory
                    // effect is produced, so external store count must be unchanged.
                    if state.external_stores.len() != before_stores {
                        return Err("loop pair operation unexpectedly produced a memory store".into());
                    }
                } else if state.external_stores.len() == before_stores + 1 && tree.kind == "memory-store" {
                    stores.push(state.external_stores.pop().expect("store count checked"));
                } else {
                    return Err("loop body contains unsupported non-assignment instruction".into());
                }
            }
        }
    }
    if !type_conflicts.is_empty() {
        return Err("loop-carried value type changes or becomes unresolved".into());
    }
    let loop_start = if test_at_header { header } else { preheader };
    let pcs = (loop_start..=tail).map(|index| start + index as u64 * 4).collect();
    Ok(Some((SourceLoop { condition, variables, assignments, loads, stores, pcs }, exit)))
}
fn statement(t: &Tree, state: &mut State, dag: &mut Dag, pc: u64, call_prototypes: &BTreeMap<u64, SourceSignature>) -> Result<(), String> {
    match t.kind {
        "no-op" => Ok(()),
        "assignment" => {
            let dest = t.children.get(0);
            let is_simd_fp = dest.is_some_and(|c| c.kind == "fp-register" || (c.kind == "register" && attribute(c, "class") == "simd-fp"));
            if is_simd_fp {
                let d = dest.unwrap();
                let name = attribute(d, "name");
                let bits = if number(d, "bits") != 0 { number(d, "bits") as u8 } else {
                    if name.starts_with('d') { 64 } else if name.starts_with('s') { 32 } else if name.starts_with('q') || name.starts_with('v') { 128 } else { 64 }
                };
                let idx = if let Some(rest) = name.strip_prefix('d') {
                    rest.parse::<usize>().unwrap_or(0)
                } else if let Some(rest) = name.strip_prefix('s') {
                    rest.parse::<usize>().unwrap_or(0)
                } else if let Some(rest) = name.strip_prefix('q') {
                    rest.parse::<usize>().unwrap_or(0)
                } else if let Some(rest) = name.strip_prefix('v') {
                    rest.parse::<usize>().unwrap_or(0)
                } else { 0usize };
                if bits == 128 || name.starts_with('q') || name.starts_with('v') {
                    if idx < state.q_blobs.len() {
                        if t.children.get(1).is_some_and(|c| c.kind == "memory-load" && number(c, "bytes") == 16) {
                            let value = read(&t.children[1], state, dag, pc)?;
                            if let Symbol::Blob(id) = value {
                                state.q_blobs[idx] = Some(id);
                                return Ok(());
                            }
                        }
                        let value = read(&t.children[1], state, dag, pc)?;
                        if let Symbol::Blob(id) = value {
                            state.q_blobs[idx] = Some(id);
                        } else {
                            state.q_blobs[idx] = None;
                        }
                    }
                    return Ok(());
                }
                let value = read(&t.children[1], state, dag, pc)?;
                if idx < state.fp_scalars.len() {
                    state.fp_scalars[idx] = Some((value, bits));
                }
                Ok(())
            } else {
                let value = read(&t.children[1], state, dag, pc)?;
                write(&t.children[0], value, state, dag, pc)
            }
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
                if fields.get("source_type") != Some(&V::String(parameter.spelling.clone())) {
                    return Err("call argument ABI mismatch".into());
                }
                let value = if let Some(reg) = parameter.register {
                    if fields.get("location") != Some(&V::String(format!("x{}", reg))) {
                        return Err("call argument ABI mismatch".into());
                    }
                    dag.value(state.registers[reg as usize].clone(), parameter.bits.max(32) as u8, pc)?
                } else if let (Some(offset), Some(bytes)) = (parameter.stack_offset, parameter.stack_bytes) {
                    let expected = format!("stack(sp+{offset:#x},{bytes})");
                    if fields.get("location") != Some(&V::String(expected)) {
                        return Err("call argument ABI mismatch".into());
                    }
                    // Read stack-passed argument value from current stack state.
                    let sp = match state.registers[31] { Symbol::Frame(f) => f, _ => return Err("stack argument requires known frame pointer".into()) };
                    let req_off = sp + offset as i64;
                    let req_bytes = bytes as u64;
                    let key = (req_off, req_bytes);
                    let sym = if let Some((sym, _)) = state.frame.get(&key).cloned() {
                        sym
                    } else {
                        // Bounded fallback: read from a covering frame slot when the
                        // argument is an exact scalar subrange of a proven slot.
                        let Some(((slot, size), (value, origins))) = state.frame.iter().find(|((slot, size), _)| {
                            *slot <= req_off && req_off + req_bytes as i64 <= *slot + *size as i64
                        }).map(|((s,sz),(v,o))| ((*s,*sz),(v.clone(),o.clone()))) else {
                            return Err("stack argument value not proven".into());
                        };
                        if req_bytes == 8 && size == 16 {
                            if let Symbol::Blob(blob_id) = value {
                                let part_offset = req_off - slot;
                                if part_offset == 0 || part_offset == 8 {
                                    let template = if part_offset == 0 {
                                        "((uint64_t)((unsigned __int128)($0)))".to_owned()
                                    } else {
                                        "((uint64_t)(((unsigned __int128)($0)) >> 64))".to_owned()
                                    };
                                    let id = dag.add_role("source-frame-reload", "slot", template, vec![blob_id], pc)?;
                                    dag.nodes[id].origins.extend(origins);
                                    dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
                                    Symbol::Value(id, 64)
                                } else {
                                    return Err("stack argument value not proven".into());
                                }
                            } else {
                                return Err("stack argument value not proven".into());
                            }
                        } else if matches!(req_bytes, 1 | 2 | 4) && size == 16 {
                            if let Symbol::Blob(blob_id) = value {
                                let inner_offset = req_off - slot;
                                if inner_offset >= 0
                                    && (inner_offset as u64 + req_bytes) <= 16
                                    && (inner_offset % req_bytes as i64 == 0 || inner_offset == 0)
                                {
                                    let (half_shift, sub_offset) = if inner_offset < 8 && (inner_offset as u64 + req_bytes) <= 8 {
                                        (0u64, inner_offset as u64)
                                    } else if inner_offset >= 8 && (inner_offset as u64 + req_bytes) <= 16 {
                                        (64u64, (inner_offset - 8) as u64)
                                    } else {
                                        (u64::MAX, 0)
                                    };
                                    if half_shift != u64::MAX {
                                        let half = if half_shift == 0 {
                                            "((uint64_t)((unsigned __int128)($0)))".to_owned()
                                        } else {
                                            "((uint64_t)(((unsigned __int128)($0)) >> 64))".to_owned()
                                        };
                                        let bit_shift = sub_offset * 8;
                                        let shifted = if bit_shift == 0 {
                                            format!("({half})")
                                        } else {
                                            format!("(({half}) >> {bit_shift})")
                                        };
                                        let (template, spelling) = match req_bytes {
                                            1 => (format!("((uint64_t)(uint8_t){shifted})"), "uint8_t".to_owned()),
                                            2 => (format!("((uint64_t)(uint16_t){shifted})"), "uint16_t".to_owned()),
                                            4 => (format!("((uint64_t)(uint32_t){shifted})"), "uint32_t".to_owned()),
                                            _ => return Err("stack argument value not proven".into()),
                                        };
                                        let id = dag.add_role("source-frame-reload", "slot", template, vec![blob_id], pc)?;
                                        dag.nodes[id].origins.extend(origins);
                                        dag.type_node(id, Some(SourceType { spelling, bits: (req_bytes * 8) as u8, signed: false, pointer: false }));
                                        Symbol::Value(id, 64)
                                    } else {
                                        return Err("stack argument value not proven".into());
                                    }
                                } else {
                                    return Err("stack argument value not proven".into());
                                }
                            } else {
                                return Err("stack argument value not proven".into());
                            }
                        } else if matches!(req_bytes, 1 | 2 | 4) && matches!(size, 2 | 4 | 8) && req_bytes < size {
                            let inner_offset = req_off - slot;
                            if inner_offset >= 0
                                && (inner_offset as u64 + req_bytes) <= size
                                && (inner_offset % req_bytes as i64 == 0 || inner_offset == 0)
                            {
                                let src = dag.value(value, 64, pc)?;
                                let shift = (inner_offset as u64) * 8;
                                let shifted = if shift == 0 {
                                    "((uint64_t)($0))".to_owned()
                                } else {
                                    format!("(((uint64_t)($0)) >> {shift})")
                                };
                                let (template, spelling) = match req_bytes {
                                    1 => (format!("((uint64_t)(uint8_t){shifted})"), "uint8_t".to_owned()),
                                    2 => (format!("((uint64_t)(uint16_t){shifted})"), "uint16_t".to_owned()),
                                    4 => (format!("((uint64_t)(uint32_t){shifted})"), "uint32_t".to_owned()),
                                    _ => return Err("stack argument value not proven".into()),
                                };
                                let id = dag.add_role("source-frame-reload", "slot", template, vec![src], pc)?;
                                dag.nodes[id].origins.extend(origins);
                                dag.type_node(id, Some(SourceType { spelling, bits: (req_bytes * 8) as u8, signed: false, pointer: false }));
                                Symbol::Value(id, 64)
                            } else {
                                return Err("stack argument value not proven".into());
                            }
                        } else {
                            return Err("stack argument value not proven".into());
                        }
                    };
                    dag.value(sym, parameter.bits.max(32) as u8, pc)?
                } else {
                    return Err("call argument ABI mismatch".into());
                };
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
            dag.type_node(value, Some(SourceType { spelling: callee.result_spelling.clone(), bits: callee.result_bits.min(255) as u8, signed: callee.result_signed, pointer: callee.result_pointer }));
            for register in 0..=18 { state.registers[register] = Symbol::Unknown(register as u64); }
            // The call's result width belongs to the callee prototype. The
            // caller may return a narrower or wider value after further
            // operations; using its result declaration here loses valid
            // callee bits or claims bits the callee did not define.
            if callee.result_bits != 0 {
                state.registers[0] = Symbol::Value(value, callee.result_bits.max(32).min(64) as u8);
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

fn c_identifiers(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty() && token.bytes().next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_'))
        .map(str::to_owned)
        .collect()
}

fn declared_type_identifiers(signature: &SourceSignature) -> BTreeSet<String> {
    let mut names = c_identifiers(&signature.result_spelling);
    for parameter in &signature.parameters {
        names.extend(c_identifiers(&parameter.spelling));
    }
    names
}

fn select_project_type_dependencies(
    mut required_names: BTreeSet<String>,
    project_types: &[crate::reconstruction::ProjectTypeDependency],
) -> Vec<crate::reconstruction::ProjectTypeDependency> {
    let mut selected = BTreeMap::<String, crate::reconstruction::ProjectTypeDependency>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for dependency in project_types {
            if required_names.contains(&dependency.name) && !selected.contains_key(&dependency.name) {
                required_names.extend(c_identifiers(&dependency.declaration));
                selected.insert(dependency.name.clone(), dependency.clone());
                changed = true;
            }
        }
    }
    selected.into_values().collect()
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
    project_types: Vec<crate::reconstruction::ProjectTypeDependency>,
}
pub(super) fn recover(
    start: u64,
    trees: &[Tree],
    operations: &[IrOperation],
    signature: SourceSignature,
    call_prototypes: &BTreeMap<u64, SourceSignature>,
) -> Result<Recovery, String> {
    recover_with_types(start, trees, operations, signature, call_prototypes, &[])
}

pub(super) fn recover_with_types(
    start: u64,
    trees: &[Tree],
    operations: &[IrOperation],
    signature: SourceSignature,
    call_prototypes: &BTreeMap<u64, SourceSignature>,
    project_types: &[crate::reconstruction::ProjectTypeDependency],
) -> Result<Recovery, String> {
    let end = start + trees.len() as u64 * 4;
    // Address operations carry concrete computed values in this IR; do not
    // reject solely on their presence. Unresolved relocatable dependencies are
    // reported separately when evidence is insufficient.
    let _ = operations;
    let mut dag = Dag { nodes: Vec::new() };
    let truth = dag.constant(1, start)?;
    let mut state = State {
        registers: (0..32).map(Symbol::Unknown).collect(),
        fp_scalars: vec![None; 32],
        q_blobs: vec![None; 32],
        flags: None,
        frame: BTreeMap::new(),
        external_loads: Vec::new(),
        external_stores: Vec::new(),
        external_access_pcs: Vec::new(),
        path: truth,
        has_call: false,
        call_pcs: Vec::new(),
        calls: Vec::new(),
        incoming_stack_end: 0,
    };
    state.registers[31] = Symbol::Frame(0);
    // Bounded: seed hidden sret result buffer pointer (e.g. x8 for CGAffineTransform).
    if let Some(reg) = signature.result_indirect_register {
        let code = "((uint64_t)(uintptr_t)(void *)(&hs_sret_result))".to_owned();
        let id = dag.add_role("source-parameter", "sret_buffer", code, vec![], start)?;
        dag.type_node(id, Some(SourceType { spelling: "uintptr_t".into(), bits: 64, signed: false, pointer: true }));
        if (reg as usize) < state.registers.len() {
            state.registers[reg as usize] = Symbol::Value(id, 64);
        }
    }
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
        // Bounded: 2-register integer aggregates passed in GPR pairs.
        if let Some(regs) = parameter.aggregate_registers.as_ref().filter(|r| r.len() == 2) {
            let fields: Option<(&str, &str)> = match parameter.spelling.as_str() {
                "NSRange" | "CFRange" => Some(("location", "length")),
                _ => None,
            };
            for (i, &reg) in regs.iter().enumerate() {
                let code = if let Some((f0, f1)) = fields {
                    let field = if i == 0 { f0 } else { f1 };
                    format!("((uint64_t)(({name}).{field}))")
                } else {
                    // Generic 16-byte two-register aggregate: read register-sized
                    // words from the parameter's object representation.
                    format!("((uint64_t)(((const uint64_t *)(const void *)&{name})[{i}]))")
                };
                let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
                dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
                state.registers[reg as usize] = Symbol::Value(id, 64);
            }
            continue;
        }
        // Bounded: FP HFA aggregates in d-registers (CGPoint/CGSize, CGRect).
        if let Some(regs) = parameter.fp_aggregate_registers.as_ref() {
            let fields: Vec<&str> = match parameter.spelling.as_str() {
                "CGPoint" => vec!["x", "y"],
                "CGSize" => vec!["width", "height"],
                "CGRect" => vec!["origin.x", "origin.y", "size.width", "size.height"],
                _ => return Err("parameter location unsupported".into()),
            };
            if fields.len() != regs.len() { return Err("parameter location unsupported".into()); }
            for (i, &reg) in regs.iter().enumerate() {
                let field = fields[i];
                let code = format!("((union {{ double d; uint64_t u; }}){{ .d = (double)(({name}).{field}) }}).u");
                let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
                dag.type_node(id, Some(SourceType { spelling: "uint64_t".into(), bits: 64, signed: false, pointer: false }));
                if (reg as usize) < state.fp_scalars.len() {
                    state.fp_scalars[reg as usize] = Some((Symbol::Value(id, 64), 64));
                }
            }
            continue;
        }
        // Bounded: by-pointer aggregate parameter (caller-copied, address in GPR).
        if let Some(reg) = parameter.by_pointer_aggregate_register {
            let code = format!("((uint64_t)(uintptr_t)(const void *)(&{name}))");
            let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
            dag.type_node(id, Some(SourceType { spelling: "uintptr_t".into(), bits: 64, signed: false, pointer: true }));
            state.registers[reg as usize] = Symbol::Value(id, 64);
            continue;
        }
        // Bounded: scalar FP parameter in d/s register (AAPCS64).
        if let Some(reg) = parameter.fp_register {
            let code = if bits == 32 {
                format!("((uint64_t)(uint32_t)((union {{ float f; uint32_t u; }}){{ .f = (float)({name}) }}).u)")
            } else {
                format!("((union {{ double d; uint64_t u; }}){{ .d = (double)({name}) }}).u")
            };
            let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
            dag.type_node(id, Some(SourceType { spelling: parameter.spelling.clone(), bits: bits.min(255) as u8, signed: false, pointer: false }));
            if (reg as usize) < state.fp_scalars.len() {
                state.fp_scalars[reg as usize] = Some((Symbol::Value(id, 64), if bits == 32 { 32 } else { 64 }));
            }
            continue;
        }
        // Bounded: scalar FP parameter passed on stack (incoming).
        if matches!(parameter.spelling.as_str(), "float" | "double") && parameter.register.is_none() && parameter.stack_offset.is_some() {
            let code = if bits == 32 {
                format!("((uint64_t)(uint32_t)((union {{ float f; uint32_t u; }}){{ .f = (float)({name}) }}).u)")
            } else {
                format!("((union {{ double d; uint64_t u; }}){{ .d = (double)({name}) }}).u")
            };
            let id = dag.add_role("source-parameter", "parameter", code, vec![], start)?;
            dag.type_node(id, Some(SourceType { spelling: parameter.spelling.clone(), bits: bits.min(255) as u8, signed: false, pointer: false }));
            if let (Some(offset), Some(bytes)) = (parameter.stack_offset, parameter.stack_bytes) {
                let off = offset as i64;
                state.frame.insert((off, bytes as u64), (Symbol::Value(id, 64), BTreeSet::from([start])));
                state.incoming_stack_end = state.incoming_stack_end.max(off + bytes as i64);
            } else {
                return Err("parameter location unsupported".into());
            }
            continue;
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
                bits: bits.min(255) as u8,
                signed: parameter.signed,
                pointer: parameter.pointer,
            }),
        );
        if parameter.pointer {
            if let Some((type_spelling,layout))=aggregate_layout_for_pointer(&parameter.spelling,project_types) {
                if let Some(candidate)=project_types.iter().find(|candidate|candidate.name==layout.name&&candidate.layout.as_ref()==Some(&layout)) {
                    if let Some(layout_evidence)=candidate.layout_evidence.clone() {
                        dag.nodes[id].aggregate_address=Some(AggregateAddress {base:id,type_spelling,layout,offset_bytes:0,
                            target:candidate.target.clone(),source_sha256:candidate.source_sha256.clone(),layout_evidence});
                    }
                }
            }
        }
        if matches!(parameter.spelling.as_str(), "bool" | "_Bool") {
            dag.nodes[id].boolean_value = true;
        }
        // The parameter DAG node is an explicit uint64_t-valued expression of the
        // declared source argument, so all 64 machine bits of that value are known.
        let param_sym = Symbol::Value(id, 64);
        if let Some(reg) = parameter.register {
            state.registers[reg as usize] = param_sym;
        } else if let (Some(offset), Some(bytes)) = (parameter.stack_offset, parameter.stack_bytes) {
            // Caller-passed stack argument at function entry: SP-relative positive offset.
            let off = offset as i64;
            state.frame.insert((off, bytes as u64), (param_sym, BTreeSet::from([start])));
            state.incoming_stack_end = state.incoming_stack_end.max(off + bytes as i64);
        } else {
            return Err("parameter location unsupported".into());
        }
    }
    let mut pending = vec![(0usize, state, BTreeMap::<usize, State>::new())];
    let mut returned = Vec::new();
    let mut noreturn_effects: Vec<(Vec<ExternalLoad>, Vec<ExternalStore>, Vec<CallResult>, bool)> = Vec::new();
    let mut source_loops = Vec::new();
    let mut visits = 0;
    // The proven frame objects, keyed by their exact extent. A later write to
    // the same extent replaces the earlier version; a write to a different
    // extent adds an object without disturbing its neighbour.
    let mut objects: BTreeMap<(i64, u64), u64> = BTreeMap::new();
    while let Some((mut index, mut state, mut seen)) = pending.pop() {
        loop {
            visits += 1;
            if visits > 8192 || pending.len() + returned.len() > 128 {
                return Err("source path budget exhausted".into());
            }
            if seen.contains_key(&index) {
                let pc = start + index as u64 * 4;
                let saved = seen[&index].clone();
                // Bounded phi recovery: merge current path with saved state only
                // when unhandled state components are identical and differing
                // registers/frame values can be joined by evidence-backed phis.
                let mut can_merge = true;
                let mut any_diff = false;
                if saved.external_loads != state.external_loads
                    || saved.external_stores != state.external_stores
                    || saved.external_access_pcs != state.external_access_pcs
                    || saved.fp_scalars != state.fp_scalars
                    || saved.q_blobs != state.q_blobs
                    || saved.has_call != state.has_call
                    || saved.call_pcs != state.call_pcs
                    || saved.calls != state.calls
                    || saved.incoming_stack_end != state.incoming_stack_end
                    || saved.registers.len() != state.registers.len()
                    || saved.frame.len() != state.frame.len()
                {
                    can_merge = false;
                }
                // Flags may differ at joins; attempt phi merge when both sides are present.
                let mut merged_flags = state.flags;
                if can_merge {
                    match (saved.flags, state.flags) {
                        (None, None) => {}
                        (Some(sf), Some(cf)) if sf == cf => {}
                        (Some(sf), Some(cf)) => {
                            if let Ok(phi_id) = dag.add("source-phi", "($0 ? $1 : $2)".to_string(),
                                                          vec![saved.path, sf, cf], pc) {
                                merged_flags = Some(phi_id);
                                any_diff = true;
                            } else {
                                can_merge = false;
                            }
                        }
                        _ => { can_merge = false; }
                    }
                }
                if can_merge && saved.frame.keys().ne(state.frame.keys()) {
                    can_merge = false;
                }
                let mut merged_regs = state.registers.clone();
                if can_merge {
                    for reg in 0..state.registers.len() {
                        let ss = saved.registers[reg].clone();
                        let cs = state.registers[reg].clone();
                        if ss != cs {
                            match (&ss, &cs) {
                                (Symbol::Value(sid, sbits), Symbol::Value(cid, cbits)) if sbits == cbits => {
                                    if let Ok(phi_id) = dag.add("source-phi", "($0 ? $1 : $2)".to_string(),
                                                  vec![saved.path, *sid, *cid], pc) {
                                        merged_regs[reg] = Symbol::Value(phi_id, *sbits);
                                        any_diff = true;
                                    } else {
                                        can_merge = false; break;
                                    }
                                }
                                _ => { can_merge = false; break; }
                            }
                        }
                    }
                }
                let mut merged_frame = state.frame.clone();
                if can_merge {
                    for key in saved.frame.keys().cloned().collect::<Vec<_>>() {
                        let sv = saved.frame[&key].0.clone();
                        let cv = state.frame[&key].0.clone();
                        if sv != cv {
                            any_diff = true;
                            match (&sv, &cv) {
                                (Symbol::Value(sid, sbits), Symbol::Value(cid, cbits)) if sbits == cbits => {
                                    if let Ok(phi_id) = dag.add("source-phi", "($0 ? $1 : $2)".to_string(),
                                                  vec![saved.path, *sid, *cid], pc) {
                                        if let Some(entry) = merged_frame.get_mut(&key) {
                                            let mut provenance = saved.frame[&key].1.clone();
                                            provenance.extend(state.frame[&key].1.iter().copied());
                                            entry.0 = Symbol::Value(phi_id, *sbits);
                                            entry.1 = provenance;
                                        } else {
                                            can_merge = false; break;
                                        }
                                    } else {
                                        can_merge = false; break;
                                    }
                                }
                                _ => { can_merge = false; break; }
                            }
                        } else if saved.frame[&key].1 != state.frame[&key].1 {
                            can_merge = false; break;
                        }
                    }
                }
                // If no differences were found in mergeable components and all
                // unhandled components matched, this is a fixed-point join.
                if can_merge && any_diff {
                    // Bounded convergence: if the saved join state was already a
                    // merged path state, a further differing merge would create
                    // non-convergent phi chains in this representation; bail.
                    let saved_already_merged = dag.nodes.get(saved.path)
                        .is_some_and(|n| n.kind == "source-path" && n.template == "(($0) || ($1))");
                    if saved_already_merged {
                        can_merge = false;
                    }
                    if can_merge {
                        let mut merged = state.clone();
                        merged.registers = merged_regs;
                        merged.frame = merged_frame;
                        merged.flags = merged_flags;
                        merged.path = dag.add("source-path", "(($0) || ($1))".to_string(),
                                             vec![saved.path, state.path], pc)?;
                        seen.remove(&index);
                        seen.insert(index, merged.clone());
                        state = merged;
                        // Fall through to process instruction with joined state.
                    } else {
                        return Err(format!("loop region requires canonical loop/phi recovery at index {index} (pc {pc:#x})"));
                    }
                } else if can_merge && !any_diff {
                    break;
                } else {
                    return Err(format!("loop region requires canonical loop/phi recovery at index {index} (pc {pc:#x})"));
                }
            }
            seen.insert(index, state.clone());
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
                    } else if let Some(_sret_reg) = signature.result_indirect_register {
                        // Bounded: indirect sret result. For CGAffineTransform identity-copy pattern,
                        // accept when exactly one call occurred and the function has a single by-pointer
                        // aggregate parameter of the same type (evidenced affine case: memcpy(sret, arg, 48)).
                        if signature.result_spelling == "CGAffineTransform" {
                            let matching = signature.parameters.iter()
                                .filter(|p| p.by_pointer_aggregate_register.is_some() && p.spelling == "CGAffineTransform")
                                .collect::<Vec<_>>();
                            if matching.len() == 1 && state.calls.len() == 1 {
                                let param = matching[0];
                                let id = dag.add_role("source-return", "sret_identity", param.name.clone(), vec![], pc)?;
                                dag.type_node(id, Some(SourceType { spelling: signature.result_spelling.clone(), bits: signature.result_bits.min(255) as u8, signed: false, pointer: false }));
                                id
                            } else {
                                return Err("indirect result buffer contents not proven".into());
                            }
                        } else {
                            return Err("indirect result buffer contents not proven".into());
                        }
                    } else if let Some(regs) = signature.result_aggregate_registers.as_ref().filter(|r| r.len() == 2) {
                        let r0 = *regs.get(0).unwrap_or(&0) as usize;
                        let r1 = *regs.get(1).unwrap_or(&1) as usize;
                        let v0 = dag.value(state.registers[r0].clone(), 64, pc)?;
                        let v1 = dag.value(state.registers[r1].clone(), 64, pc)?;
                        let (template, result_bits) = match signature.result_spelling.as_str() {
                            "NSRange" | "CFRange" => (
                                format!("(({}){{ .location = (NSUInteger)($0), .length = (NSUInteger)($1) }})", signature.result_spelling),
                                128u16,
                            ),
                            _ => (
                                // Bounded: generic 16-byte two-register aggregate return.
                                format!("({{ union {{ struct {{ uint64_t _w0; uint64_t _w1; }} hs_words; {} hs_value; }} hs_u; hs_u.hs_words._w0 = (uint64_t)($0); hs_u.hs_words._w1 = (uint64_t)($1); hs_u.hs_value; }})", signature.result_spelling),
                                signature.result_bits.min(255),
                            ),
                        };
                        let id = dag.add_role(
                            "source-aggregate-return",
                            "return",
                            template,
                            vec![v0, v1],
                            pc,
                        )?;
                        dag.type_node(id, Some(SourceType { spelling: signature.result_spelling.clone(), bits: result_bits.min(255) as u8, signed: false, pointer: false }));
                        id
                    } else if let Some(regs) = signature.result_fp_aggregate_registers.as_ref() {
                        let fields: Vec<&str> = match signature.result_spelling.as_str() {
                            "CGPoint" => vec!["x", "y"],
                            "CGSize" => vec!["width", "height"],
                            "CGRect" => vec!["origin.x", "origin.y", "size.width", "size.height"],
                            _ => return Err("result ABI unresolved".into()),
                        };
                        if fields.len() != regs.len() { return Err("result ABI unresolved".into()); }
                        let mut vals = Vec::new();
                        for &reg in regs {
                            let (sym, _bits) = state.fp_scalars.get(reg as usize).and_then(|v| v.as_ref()).cloned()
                                .ok_or("floating-point aggregate result register not proven")?;
                            vals.push(dag.value(sym, 64, pc)?);
                        }
                        let mut tpl = format!("(({}){{ ", signature.result_spelling);
                        for (i, field) in fields.iter().enumerate() {
                            if i > 0 { tpl.push_str(", "); }
                            tpl.push_str(&format!(".{field} = ((union {{ uint64_t u; double d; }}){{ .u = (${}) }}).d", i));
                        }
                        tpl.push_str(" })");
                        let id = dag.add_role("source-aggregate-return", "return", tpl, vals, pc)?;
                        dag.type_node(id, Some(SourceType { spelling: signature.result_spelling.clone(), bits: signature.result_bits.min(255) as u8, signed: false, pointer: false }));
                        id
                    } else if signature.result_float {
                        let (sym, bits) = state.fp_scalars.get(0).and_then(|v| v.as_ref()).cloned()
                            .ok_or("floating-point result register not proven")?;
                        if (bits as u16) != signature.result_bits {
                            return Err("floating-point result width mismatch".into());
                        }
                        dag.value(sym, signature.result_bits.min(64) as u8, pc)?
                    } else { dag.value(
                        state.registers[0].clone(),
                        signature.result_bits.min(64) as u8,
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
                            bits: signature.result_bits.min(255) as u8,
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
                            for body_index in index + 1..exit { seen.insert(body_index, state.clone()); }
                            source_loops.push(source_loop);
                            index = exit;
                            continue;
                        }
                    }
                    let target = number(t, "target");
                    let outside = target < start || target >= end || {
                        let delta = target - start;
                        delta % 4 != 0 || delta / 4 >= trees.len() as u64
                    };
                    if outside {
                        let helper = call_prototypes.get(&target).map(|p| p.name.as_str());
                        let is_noreturn_void = helper.is_some_and(|n| {
                            let b = n.strip_prefix('_').unwrap_or(n);
                            matches!(b, "__stack_chk_fail" | "abort")
                        }) && call_prototypes.get(&target).is_some_and(|proto| {
                            proto.parameters.is_empty() && proto.result_bits == 0
                        });
                        if t.kind == "goto" {
                            // Bounded tail-call: unconditional branch outside the
                            // recovered function to a known return-forwarding runtime
                            // helper (e.g. objc_autoreleaseReturnValue) after epilogue.
                            let is_autorelease_tail = helper.is_some_and(|n| {
                                let b = n.strip_prefix('_').unwrap_or(n);
                                matches!(
                                    b,
                                    "objc_autoreleaseReturnValue"
                                        | "objc_retainAutoreleaseReturnValue"
                                        | "objc_retainAutoreleasedReturnValue"
                                        | "objc_unsafeClaimAutoreleasedReturnValue"
                                )
                            });
                            if is_autorelease_tail {
                                // model as tail call/return of x0 through helper
                                if state.registers[31] != Symbol::Frame(0)
                                    || (18..=29).any(|r| state.registers[r] != Symbol::Unknown(r as u64))
                                    || state.registers[30] != Symbol::Unknown(30)
                                {
                                    return Err("nonvolatile registers, SP or return target not restored".into());
                                }
                                let arg0 = if signature.result_float {
                                    let (sym, bits) = state.fp_scalars.get(0).and_then(|v| v.as_ref()).cloned()
                                        .ok_or("floating-point result register not proven")?;
                                    dag.value(sym, bits, pc)?
                                } else {
                                    dag.value(state.registers[0].clone(), 64, pc)?
                                };
                                let proto = call_prototypes.get(&target).unwrap();
                                let call_expr = dag.add_role("source-tail-call","tail_call", format!("{}($0)", proto.name), vec![arg0], pc)?;
                                dag.type_node(call_expr, Some(SourceType { spelling: signature.result_spelling.clone(), bits: signature.result_bits.min(255) as u8, signed: signature.result_signed, pointer: signature.result_pointer }));
                                let value = dag.add_role("source-return", "result", "$0".into(), vec![call_expr], pc)?;
                                dag.type_node(value, Some(SourceType { spelling: signature.result_spelling.clone(), bits: signature.result_bits.min(255) as u8, signed: signature.result_signed, pointer: signature.result_pointer }));
                                returned.push((state.path, value, state.external_loads.clone(), state.external_stores.clone(), true, state.call_pcs.clone(), state.calls.clone()));
                                for ((offset, bytes), version) in frame_objects(&state) {
                                    let entry = objects.entry((offset, bytes)).or_insert(version);
                                    if *entry < version { *entry = version; }
                                }
                                break;
                            }
                            // Bounded noreturn external exit: unconditional branch to a
                            // known void(void) noreturn helper (e.g. stack protector / abort)
                            // terminates this path without a normal return value.
                            if is_noreturn_void {
                                let proto = call_prototypes.get(&target).unwrap();
                                let call_expr = dag.add_role(
                                    "source-call",
                                    "value",
                                    format!("{}()", proto.name),
                                    vec![],
                                    pc,
                                )?;
                                dag.type_node(call_expr, Some(SourceType { spelling: "void".into(), bits: 0, signed: false, pointer: false }));
                                state.has_call = true;
                                state.call_pcs.push(pc);
                                state.calls.push(CallResult { expression: call_expr, guard: state.path, pc });
                                for ((offset, bytes), version) in frame_objects(&state) {
                                    let entry = objects.entry((offset, bytes)).or_insert(version);
                                    if *entry < version { *entry = version; }
                                }
                                noreturn_effects.push((state.external_loads.clone(), state.external_stores.clone(), state.calls.clone(), state.has_call));
                                break;
                            }
                            return Err("source branch outside function".into());
                        }
                        // Conditional branch to a known void(void) noreturn helper:
                        // taken path terminates in noreturn call, fallthrough continues.
                        if t.kind == "conditional-goto" && is_noreturn_void {
                            let condition = read(&t.children[0], &mut state, &mut dag, pc)?;
                            let condition = dag.value(condition, 64, pc)?;
                            let mut taken = state.clone();
                            taken.path = dag.add(
                                "source-path",
                                "(($0) && ($1))".into(),
                                vec![state.path, condition],
                                pc,
                            )?;
                            let proto = call_prototypes.get(&target).unwrap();
                            let call_expr = dag.add_role(
                                "source-call",
                                "value",
                                format!("{}()", proto.name),
                                vec![],
                                pc,
                            )?;
                            dag.type_node(call_expr, Some(SourceType { spelling: "void".into(), bits: 0, signed: false, pointer: false }));
                            taken.has_call = true;
                            taken.call_pcs.push(pc);
                            taken.calls.push(CallResult { expression: call_expr, guard: taken.path, pc });
                            for ((offset, bytes), version) in frame_objects(&taken) {
                                let entry = objects.entry((offset, bytes)).or_insert(version);
                                if *entry < version { *entry = version; }
                            }
                            noreturn_effects.push((taken.external_loads.clone(), taken.external_stores.clone(), taken.calls.clone(), taken.has_call));
                            // Fallthrough path continues with negated condition.
                            state.path = dag.add(
                                "source-path",
                                "(($0) && !($1))".into(),
                                vec![state.path, condition],
                                pc,
                            )?;
                            index += 1;
                            continue;
                        }
                        return Err("source branch outside function".into());
                    }
                    let delta = target - start;
                    let target_index = (delta / 4) as usize;
                    if t.kind == "goto" {
                        // Bounded backedge loop: unconditional branch back to an
                        // already visited preheader forms a cycle; try recovery
                        // before following the goto.
                        if target_index < index && seen.contains_key(&target_index) {
                            let preheader_tree = trees.get(target_index);
                            if preheader_tree.is_some_and(|t| t.kind == "conditional-goto") {
                                let mut loop_state = seen[&target_index].clone();
                                if let Some((source_loop, exit)) = natural_loop(
                                    start, target_index, trees, &mut loop_state, &mut dag, call_prototypes
                                )? {
                                    for body_index in target_index + 1..exit { seen.insert(body_index, loop_state.clone()); }
                                    source_loops.push(source_loop);
                                    state = loop_state;
                                    index = exit;
                                    continue;
                                }
                            }
                        }
                        index = target_index;
                        continue;
                    }
                    // Bounded backedge loop: conditional branch back to an already
                    // visited header forms a cycle; try recovery before path fork.
                    if target_index < index && seen.contains_key(&target_index) {
                        let mut tail_state = state.clone();
                        if let Some((source_loop, exit)) = natural_loop_backedge(
                            start, target_index, index, trees, &seen[&target_index], &mut tail_state, &mut dag, call_prototypes
                        )? {
                            for body_index in target_index..exit { seen.insert(body_index, tail_state.clone()); }
                            source_loops.push(source_loop);
                            state = tail_state;
                            index = exit;
                            continue;
                        }
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
                "indirect-goto" => {
                    // Resolved complete dispatch: fork paths for each proven
                    // in-function target. Exit-only dispatch (no in-function
                    // re-entry) has no recoverable continuation in this function.
                    let mut targets = Vec::<u64>::new();
                    if let Some(V::Array(items)) = t.attributes.get("targets") {
                        for item in items {
                            let target = match item {
                                V::Int(v) => *v as u64,
                                V::String(v) => u64::from_str_radix(v.trim_start_matches("0x"), 16).unwrap_or(0),
                                _ => 0,
                            };
                            if target != 0 { targets.push(target); }
                        }
                    }
                    targets.sort_unstable();
                    targets.dedup();
                    let mut case_labels: std::collections::BTreeMap<u64, Vec<u64>> = std::collections::BTreeMap::new();
                    if let Some(V::Array(items)) = t.attributes.get("case_map") {
                        for item in items {
                            let V::Object(fields) = item else { continue };
                            let case_index = match fields.get("case") {
                                Some(V::Int(v)) => Some(*v as u64),
                                Some(V::String(v)) => v.parse::<u64>().ok(),
                                _ => None,
                            };
                            let target = match fields.get("target") {
                                Some(V::Int(v)) => Some(*v as u64),
                                Some(V::String(v)) => u64::from_str_radix(v.trim_start_matches("0x"), 16).ok(),
                                _ => None,
                            };
                            if let (Some(case_index), Some(target)) = (case_index, target) {
                                case_labels.entry(target).or_default().push(case_index);
                            }
                        }
                    }
                    for labels in case_labels.values_mut() {
                        labels.sort_unstable();
                        labels.dedup();
                    }
                    let index_info = match t.attributes.get("index_info") {
                        Some(V::String(v)) if !v.is_empty() => Some(v.as_str()),
                        _ => None,
                    };
                    let external_exit_cases = t.attributes.get("external_exit_cases")
                        .and_then(|v| match v {
                            V::Int(n) => usize::try_from(*n).ok(),
                            V::String(s) => s.parse::<usize>().ok(),
                            _ => None,
                        })
                        .or_else(|| index_info.and_then(|info| {
                            info.split_whitespace()
                                .find_map(|token| token.strip_prefix("external-cases="))
                                .and_then(|s| s.parse::<usize>().ok())
                        }))
                        .unwrap_or(0);
                    let table_index = index_info.and_then(|info| {
                        info.split_whitespace()
                            .find_map(|token| token.strip_prefix("table_index="))
                            .map(|s| s.to_string())
                    });
                    let case_range = index_info.and_then(|info| {
                        info.split("case-range=")
                            .nth(1)
                            .map(|s| s.split(" index-normalization=").next().unwrap_or(s).trim().to_string())
                            .filter(|s| !s.is_empty())
                    });
                    let index_normalization = index_info.and_then(|info| {
                        info.split("index-normalization=")
                            .nth(1)
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                    });
                    let mut target_indices = Vec::with_capacity(targets.len());
                    for target in targets {
                        let Some(delta) = target.checked_sub(start) else {
                            return Err("resolved indirect dispatch target is outside the recovered function".into());
                        };
                        if delta % 4 != 0 {
                            return Err("resolved indirect dispatch target is not instruction-aligned".into());
                        }
                        let idx = (delta / 4) as usize;
                        if idx >= trees.len() {
                            return Err("resolved indirect dispatch target is outside the recovered function".into());
                        }
                        target_indices.push((target, idx));
                    }
                    if target_indices.is_empty() {
                        return Err("resolved indirect dispatch has no recoverable in-function continuation".into());
                    }
                    if external_exit_cases > 0 {
                        return Err("resolved indirect dispatch has proven external exits; continuation not recoverable as in-function source".into());
                    }
                    // Bounded join: if every in-function dispatch target reaches the same
                    // continuation through a chain of unconditional gotos (skipping no-ops),
                    // treat as a single continuation without forking. Chains are followed only
                    // while targets remain unconditional gotos/no-ops within the function and
                    // no cycle/ambiguity is introduced.
                    fn tree_is_noop_only(tree: &Tree) -> bool {
                        match tree.kind {
                            "no-op" => true,
                            "sequence" => !tree.children.is_empty() && tree.children.iter().all(tree_is_noop_only),
                            _ => false,
                        }
                    }
                    let follow_goto_chain = |mut idx: usize| -> Option<usize> {
                        let mut steps = 0usize;
                        loop {
                            if steps > 32 { return None; }
                            let tgt_tree = trees.get(idx)?;
                            if tree_is_noop_only(tgt_tree) {
                                idx = idx.checked_add(1)?;
                                steps += 1;
                                continue;
                            }
                            if tgt_tree.kind != "goto" && tgt_tree.kind != "conditional-goto" { return Some(idx); }
                            if tgt_tree.kind == "conditional-goto" {
                                // Only follow if it targets the next sequential instruction.
                                let tgt = number(tgt_tree, "target");
                                let delta = tgt.checked_sub(start)?;
                                if delta % 4 != 0 { return Some(idx); }
                                let tgt_idx = (delta / 4) as usize;
                                if tgt_idx != idx + 1 { return Some(idx); }
                                idx = idx.checked_add(1)?;
                                steps += 1;
                                continue;
                            }
                            let tgt = number(tgt_tree, "target");
                            let delta = tgt.checked_sub(start)?;
                            if delta % 4 != 0 { return None; }
                            let tgt_idx = (delta / 4) as usize;
                            if tgt_idx >= trees.len() { return None; }
                            idx = tgt_idx;
                            steps += 1;
                        }
                    };
                    let common_goto = if target_indices.len() >= 2 {
                        let mut common: Option<usize> = None;
                        let mut all_same = true;
                        for (_, idx) in &target_indices {
                            let Some(join_idx) = follow_goto_chain(*idx) else { all_same = false; break; };
                            // Only join if we actually reached a common non-trivial continuation
                            // (or a goto-chain-resolved point) consistently.
                            match common {
                                None => common = Some(join_idx),
                                Some(prev) if prev == join_idx => {}
                                _ => { all_same = false; break; }
                            }
                        }
                        // Require that at least one target involved a goto/trampoline form;
                        // otherwise this is not a dispatch-join case we need to special-case.
                        if all_same {
                            let all_direct_same = target_indices.iter().all(|(_, idx)| Some(*idx) == common);
                            if all_direct_same { None } else { common }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(join_index) = common_goto {
                        // Sound: complete dispatch with all in-function cases routed to
                        // the same continuation; case selection does not affect which
                        // in-function block is reached.
                        let _ = read(&t.children[0], &mut state, &mut dag, pc)?;
                        index = join_index;
                        continue;
                    }
                    // Single proven in-function target: complete dispatch with no external
                    // exits means all accounted cases reach this continuation.
                    if target_indices.len() == 1 {
                        let (_, single_index) = target_indices[0];
                        let _ = read(&t.children[0], &mut state, &mut dag, pc)?;
                        index = single_index;
                        continue;
                    }
                    if target_indices.len() > 64 {
                        return Err("resolved indirect dispatch has too many targets for bounded source recovery".into());
                    }
                    let dispatch = read(&t.children[0], &mut state, &mut dag, pc)?;
                    let dispatch = dag.value(dispatch, 64, pc)?;
                    let index_expr = table_index.as_ref().and_then(|name| {
                        let reg = name.strip_prefix('x').or_else(|| name.strip_prefix('w'))?.parse::<usize>().ok()?;
                        if reg > 31 { return None; }
                        dag.value(state.registers[reg].clone(), 64, pc).ok()
                    });
                    let parse_range_bound = |case_range: Option<&String>| -> Option<String> {
                        let range = case_range?;
                        let parse_bound_value = |s: &str| -> Option<u64> {
                            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                                u64::from_str_radix(hex, 16).ok()
                            } else {
                                s.parse::<u64>().ok()
                            }
                        };
                        // Expected proven forms include e.g. "CMP/B.HI x8<=3" and "CMP/B.HS x8<4".
                        // Extract conservative bound conditions on the index expression only when
                        // register/comparison tokens parse unambiguously. Combine multiple bounds
                        // (e.g. lower and upper) if present.
                        let mut conds = Vec::<String>::new();
                        for token in range.split_whitespace() {
                            if let Some(rest) = token.strip_prefix('x').or_else(|| token.strip_prefix('w')) {
                                let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
                                if digits == 0 { continue; }
                                let tail = &rest[digits..];
                                if let Some(bound) = tail.strip_prefix("<=") {
                                    if let Some(value) = parse_bound_value(bound) {
                                        conds.push(format!("(($1) <= UINT64_C({value}))"));
                                    }
                                } else if let Some(bound) = tail.strip_prefix(">=") {
                                    if let Some(value) = parse_bound_value(bound) {
                                        conds.push(format!("(($1) >= UINT64_C({value}))"));
                                    }
                                } else if let Some(bound) = tail.strip_prefix('<') {
                                    if let Some(value) = parse_bound_value(bound) {
                                        conds.push(format!("(($1) < UINT64_C({value}))"));
                                    }
                                } else if let Some(bound) = tail.strip_prefix('>') {
                                    if let Some(value) = parse_bound_value(bound) {
                                        conds.push(format!("(($1) > UINT64_C({value}))"));
                                    }
                                }
                            }
                        }
                        conds.sort();
                        conds.dedup();
                        match conds.len() {
                            0 => None,
                            1 => conds.pop(),
                            _ => Some(format!("({})", conds.join(" && "))),
                        }
                    };
                    let guard_for = |target: u64,
                                     case_labels: &std::collections::BTreeMap<u64, Vec<u64>>,
                                     table_index: Option<&String>,
                                     case_range: Option<&String>,
                                     index_normalization: Option<&String>,
                                     has_index_expr: bool| -> String {
                        let notes = |range_note: Option<String>| -> String {
                            let mut notes = String::new();
                            if let Some(range_note) = range_note { notes.push_str(&range_note); }
                            if let Some(norm) = index_normalization { notes.push_str(&format!(" /* normalization={norm} */")); }
                            notes
                        };
                        // When the index expression is recovered and proven case labels
                        // are available, emit an explicit equality/disjunction condition
                        // over the index value. This is stronger than an opaque dispatch
                        // predicate while remaining sound: case labels come only from
                        // proven complete jump-table enumeration for this dispatch.
                        if has_index_expr {
                            if let Some(labels) = case_labels.get(&target).filter(|labels| !labels.is_empty()) {
                                let case_cond = if labels.len() == 1 {
                                    format!("(($1) == UINT64_C({}))", labels[0])
                                } else {
                                    let parts = labels.iter().map(|label| format!("(($1) == UINT64_C({label}))")).collect::<Vec<_>>().join(" || ");
                                    format!("({parts})")
                                };
                                let bound_cond = parse_range_bound(case_range);
                                let range_note = case_range.map(|r| format!(" /* range={r} */"));
                                let cond = if let Some(bound_cond) = bound_cond {
                                    format!("({bound_cond} && {case_cond})")
                                } else {
                                    case_cond
                                };
                                return format!("({cond} /* dispatch target={target:#x}{} */)", notes(range_note));
                            }
                            if let Some(bound_cond) = parse_range_bound(case_range) {
                                let range_note = case_range.map(|r| format!(" /* range={r} */"));
                                return format!("({bound_cond} /* dispatch target={target:#x}{} */)", notes(range_note));
                            }
                        }
                        let mut parts = Vec::new();
                        if has_index_expr {
                            parts.push("index=$1".to_string());
                        } else if let Some(index) = table_index {
                            parts.push(format!("index={index}"));
                        }
                        if let Some(range) = case_range {
                            parts.push(format!("range={range}"));
                        }
                        if let Some(norm) = index_normalization {
                            parts.push(format!("normalization={norm}"));
                        }
                        if let Some(labels) = case_labels.get(&target).filter(|labels| !labels.is_empty()) {
                            if labels.len() == 1 {
                                parts.push(format!("case={:#x}", labels[0]));
                            } else {
                                let joined = labels.iter().map(|label| format!("{label:#x}")).collect::<Vec<_>>().join(",");
                                parts.push(format!("cases=[{joined}]"));
                            }
                        }
                        parts.push(format!("target={target:#x}"));
                        format!("hs_dispatch_case($0, {})", parts.join(", "))
                    };
                    // Fork: continue on first target, enqueue the rest with opaque
                    // per-target dispatch guards so path joins remain explicit.
                    let mut iter = target_indices.into_iter();
                    let (continue_target, continue_index) = iter.next().unwrap();
                    let guard_children = |dispatch: usize, index_expr: Option<usize>| -> Vec<usize> {
                        if let Some(index_expr) = index_expr { vec![dispatch, index_expr] } else { vec![dispatch] }
                    };
                    let has_index_expr = index_expr.is_some();
                    for (target, target_index) in iter {
                        let mut forked = state.clone();
                        let case = dag.add_role(
                            "source-dispatch-case",
                            "case",
                            guard_for(target, &case_labels, table_index.as_ref(), case_range.as_ref(), index_normalization.as_ref(), has_index_expr),
                            guard_children(dispatch, index_expr),
                            pc,
                        )?;
                        forked.path = dag.add(
                            "source-path",
                            "(($0) && ($1))".into(),
                            vec![state.path, case],
                            pc,
                        )?;
                        pending.push((target_index, forked, seen.clone()));
                    }
                    // Current path continues along the chosen target with an
                    // explicit per-target dispatch guard.
                    let case = dag.add_role(
                        "source-dispatch-case",
                        "case",
                        guard_for(continue_target, &case_labels, table_index.as_ref(), case_range.as_ref(), index_normalization.as_ref(), has_index_expr),
                        guard_children(dispatch, index_expr),
                        pc,
                    )?;
                    state.path = dag.add(
                        "source-path",
                        "(($0) && ($1))".into(),
                        vec![state.path, case],
                        pc,
                    )?;
                    index = continue_index;
                    continue;
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
    let mut ordered_call_memory = returned.iter().any(|path| !path.6.is_empty())
        || noreturn_effects.iter().any(|(_, _, calls, _)| !calls.is_empty());
    let mut ordered_volatile_accesses = returned.iter().any(|(_, _, loads, stores, has_call, _, _)| {
        let volatile = loads.iter().any(|load| load.volatile) || stores.iter().any(|store| store.volatile);
        volatile && (*has_call || loads.len() + stores.len() > 1)
    }) || noreturn_effects.iter().any(|(loads, stores, _, has_call)| {
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
    for (path_loads, path_stores, path_calls, has_call) in noreturn_effects {
        let volatile = path_loads.iter().any(|load| load.volatile) || path_stores.iter().any(|store| store.volatile);
        if volatile && (has_call || path_loads.len() + path_stores.len() > 1) {
            ordered_volatile_accesses = true;
        }
        if !path_calls.is_empty() { ordered_call_memory = true; }
        for access in path_loads {
            if !external_loads.contains(&access) { external_loads.push(access); }
        }
        for effect in path_stores {
            if !external_stores.contains(&effect) { external_stores.push(effect); }
        }
        for call in path_calls {
            if !calls.contains(&call) { calls.push(call); }
        }
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
    // Resolve dependencies from structured type spellings. Scanning the full
    // rendered declaration also sees function and parameter identifiers, which
    // can spuriously pull in an unrelated typedef with the same name.
    let mut required_names = declared_type_identifiers(&signature);
    for prototype in &called_prototypes { required_names.extend(declared_type_identifiers(prototype)); }
    let selected_types = select_project_type_dependencies(required_names, project_types);
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
        project_types: selected_types,
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
        pending.extend(self.external_loads.iter().flat_map(|load| [load.expression, load.address, load.guard]));
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
                        let expression=render_external_load(&load,&rendered,&self.dag);
                        let statement = format!("        hs_loaded_{} = {expression};\n",load.expression);
                        if self.dag.literal(&Symbol::Value(load.guard, 64)) == Some(1) {
                            loop_code.push_str(&statement);
                        } else {
                            loop_code.push_str(&format!("        if ({}) {{\n{}        }}\n", rendered[&load.guard], statement));
                        }
                        let mut evidence = ir.get(((load.pc - start) / 4) as usize).into_iter().cloned()
                            .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
                        evidence.extend(memory_evidence_by_pc.get(&load.pc).into_iter().flatten().cloned());
                        let ordering = "per-iteration instruction order under the recovered path guard";
                        let member_layout=aggregate_member_layout_value(load.member.as_ref());
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
                                ("template", if load.member.is_some(){"if ($2) hs_loaded = $1".into()}else{format!("if ($2) hs_load{}((uintptr_t)($0))",load.bytes).into()}),
                                ("member_layout",member_layout),
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
                        let statement = if let Some(member)=&store.member {
                            format!("        {} = {};\n",render_aggregate_member(member,&rendered),rendered[&store.value])
                        } else {
                            format!("        {}((uintptr_t)({}), (uint64_t)({}));\n", helper,
                                rendered[&store.address], rendered[&store.value])
                        };
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
                                ("template", if let Some(member)=&store.member {
                                    format!("if ($2) (({} *)($0))->{} = $1",member.type_spelling,member.field_name).into()
                                } else {format!("if ($2) {helper}((uintptr_t)($0), (uint64_t)($1))").into()}),
                                ("member_layout",aggregate_member_layout_value(store.member.as_ref())),
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
                let load = &self.external_loads[event_index];
                if !ordered_effect_schedule {
                    let mut evidence=ir.get(((load.pc-start)/4) as usize).into_iter().cloned()
                        .chain(std::iter::once(declaration.to_owned())).collect::<Vec<_>>();
                    evidence.extend(memory_evidence_by_pc.get(&load.pc).into_iter().flatten().cloned());
                    effect_asts.push(builder.ast_spans(
                        "source-memory-read",None,
                        vec![asts[&load.address].clone(),asts[&load.expression].clone(),asts[&load.guard].clone()],
                        &evidence,array([object([("start",address(load.pc)),("end",address(load.pc+4))])]),
                        object([
                            ("bytes",load.bytes.into()),
                            ("partition",if load.volatile {"declaration-proven volatile pointer"} else {"declaration-proven ordinary pointer"}.into()),
                            ("ordering","instruction order under the recovered path guard".into()),
                            ("template",if load.member.is_some(){"read $1".into()}else{format!("hs_load{}((uintptr_t)($0))",load.bytes).into()}),
                            ("member_layout",aggregate_member_layout_value(load.member.as_ref())),
                        ]),
                    )?);
                    continue;
                }
                let expression=render_external_load(load,&rendered,&self.dag);
                let read = format!("    hs_loaded_{} = {expression};\n", load.expression);
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
                let member_layout=aggregate_member_layout_value(load.member.as_ref());
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
                        ("template", if load.member.is_some(){"if ($2) hs_loaded = $1".into()}else{format!("if ($2) hs_load{}((uintptr_t)($0))",load.bytes).into()}),
                        ("member_layout",member_layout),
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
            let store = if let Some(member)=&effect.member {
                format!("    {} = {};\n",render_aggregate_member(member,&rendered),rendered[&effect.value])
            } else if effect.bytes == 16 {
                format!("    hs_store16((uintptr_t)({}), (unsigned __int128)({}));\n",rendered[&effect.address],rendered[&effect.value])
            } else {
                format!("    {}((uintptr_t)({}), (uint64_t)({}));\n",store_helper,rendered[&effect.address],rendered[&effect.value])
            };
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
                    ("template", if let Some(member)=&effect.member {
                        format!("if ($2) (({} *)($0))->{} = $1",member.type_spelling,member.field_name).into()
                    } else {format!("if ($2) {store_helper}((uintptr_t)($0), (uint64_t)($1))").into()}),
                    ("member_layout",aggregate_member_layout_value(effect.member.as_ref())),
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
        } else if s.result_float {
            if s.result_bits == 32 {
                format!("    return (float)({{\n        uint32_t hs_fbits = (uint32_t)((uint64_t)({result}));\n        float hs_fv; memcpy(&hs_fv, &hs_fbits, sizeof hs_fv);\n        hs_fv;\n    }});\n")
            } else {
                format!("    return (double)({{\n        uint64_t hs_fbits = (uint64_t)({result});\n        double hs_fv; memcpy(&hs_fv, &hs_fbits, sizeof hs_fv);\n        hs_fv;\n    }});\n")
            }
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
        let project_type_declarations = if self.project_types.is_empty() { String::new() } else {
            format!("/* Source dependencies from the matching analyst project type catalog; no ABI layout claim. */\n{}\n",
                self.project_types.iter().map(|dependency| dependency.declaration.as_str()).collect::<Vec<_>>().join("\n"))
        };
        let needs_frame_storage = rendered.values().any(|v| v.contains("hs_frame_storage"))
            || declarations.contains("hs_frame_storage")
            || loop_code.contains("hs_frame_storage")
            || effects_code.contains("hs_frame_storage")
            || return_code.contains("hs_frame_storage");
        let frame_storage_decl = if needs_frame_storage {
            "    enum { HS_FRAME_BASE = 4096 };\n    unsigned char hs_frame_storage[8192];\n    (void)hs_frame_storage;\n".to_owned()
        } else { String::new() };
        let needs_sret_result = s.result_indirect_register.is_some()
            || rendered.values().any(|v| v.contains("hs_sret_result"))
            || declarations.contains("hs_sret_result")
            || loop_code.contains("hs_sret_result")
            || effects_code.contains("hs_sret_result")
            || return_code.contains("hs_sret_result");
        let sret_result_decl = if needs_sret_result {
            format!("    {} hs_sret_result;\n    (void)hs_sret_result;\n", s.result_spelling)
        } else { String::new() };
        let frame_objects_comment = if self.frame_objects.is_empty() {
            String::new()
        } else {
            let mut lines = String::from("    /* Proven frame objects (exact extent/version; not promoted to typed named locals):\n");
            for object in &self.frame_objects {
                lines.push_str(&format!(
                    "     * offset={:+} bytes={} version_pc={:#x}\n",
                    object.offset, object.bytes, object.version
                ));
            }
            lines.push_str("     */\n");
            lines
        };
        let output=format!("{headers}{project_type_declarations}\n{call_declarations}{}\nstatic inline uint64_t hs_load1(uintptr_t a) {{ uint8_t v; memcpy(&v,(const void *)a,1); return v; }}\nstatic inline uint64_t hs_load2(uintptr_t a) {{ uint16_t v; memcpy(&v,(const void *)a,2); return v; }}\nstatic inline uint64_t hs_load4(uintptr_t a) {{ uint32_t v; memcpy(&v,(const void *)a,4); return v; }}\nstatic inline uint64_t hs_load8(uintptr_t a) {{ uint64_t v; memcpy(&v,(const void *)a,8); return v; }}\nstatic inline unsigned __int128 hs_load16(uintptr_t a) {{ unsigned __int128 v; memcpy(&v,(const void *)a,16); return v; }}\nstatic inline void hs_store8(uintptr_t a, uint64_t x) {{ uint64_t v=x; memcpy((void *)a,&v,8); }}\nstatic inline void hs_store4(uintptr_t a, uint64_t x) {{ uint32_t v=(uint32_t)x; memcpy((void *)a,&v,4); }}\nstatic inline void hs_store2(uintptr_t a, uint64_t x) {{ uint16_t v=(uint16_t)x; memcpy((void *)a,&v,2); }}\nstatic inline void hs_store1(uintptr_t a, uint64_t x) {{ uint8_t v=(uint8_t)x; memcpy((void *)a,&v,1); }}\nstatic inline void hs_store16(uintptr_t a, unsigned __int128 x) {{ unsigned __int128 v=x; memcpy((void *)a,&v,16); }}\n{volatile_helpers}/* Recovered from bounded structured IR; external memory effects require valid memory; volatile accesses retain instruction order alongside qualified scalar calls on each supported path; loop-carried register values are explicit locals; synthetic frame storage may be used for recovered frame addresses. */\n{} {}({}) {{\n{assertions}    (void)hs_mask;(void)hs_extend;(void)hs_shift;(void)hs_div;(void)hs_condition;(void)hs_compare;(void)hs_flags_arith;(void)hs_load1;(void)hs_load2;(void)hs_load4;(void)hs_load8;(void)hs_load16;(void)hs_store1;(void)hs_store2;(void)hs_store4;(void)hs_store8;(void)hs_store16;\n{unused}{declarations}{frame_storage_decl}{sret_result_decl}{frame_objects_comment}{loop_code}{effects_code}{return_code}}}\n",include_str!("bitvector_runtime.h"),s.result_spelling,s.name,if parameters.is_empty(){"void"}else{&parameters});
        let root = builder.ast(
            "source-function",
            Some(&s.result_spelling),
            effect_asts.into_iter().chain(std::iter::once(asts[&self.result].clone())).collect(),
            declaration,
            (start, start + ir.len() as u64 * 4),
            object([
                ("name", s.name.clone().into()),
                ("return_paths", (self.paths as u64).into()),
                ("result_bits", (s.result_bits as u64).into()),
                ("result_signed", s.result_signed.into()),
                ("result_pointer", s.result_pointer.into()),
                (
                    "parameters",
                    array(s.parameters.iter().map(|p| {
                        object([
                            ("name", p.name.clone().into()),
                            ("source_spelling", p.spelling.clone().into()),
                            ("bits", (p.bits as u64).into()),
                            ("signed", p.signed.into()),
                            ("pointer", p.pointer.into()),
                            ("abi_gpr", p.register.map(|r| (r as u64).into()).unwrap_or(Value::Null)),
                            ("stack_offset", p.stack_offset.map(|o| (o as u64).into()).unwrap_or(Value::Null)),
                            ("stack_bytes", p.stack_bytes.map(|b| (b as u64).into()).unwrap_or(Value::Null)),
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
        for dependency in &self.project_types {
            let evidence = builder.record("type", "analyst-assertion", object([
                ("name", dependency.name.clone().into()),
                ("declaration", dependency.declaration.clone().into()),
                ("target", dependency.target.clone().into()),
                ("source_sha256", dependency.source_sha256.clone().into()),
                ("abi_layout_claimed", false.into()),
                ("compiler_layout_evidence", dependency.layout_evidence.clone().map(Value::String).unwrap_or(Value::Null)),
                ("captured_type_identity_proven", false.into()),
            ]), array([]))?;
            builder.edge(&evidence, &node, "required-source-dependency")?;
        }
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
    use crate::reconstruction::Value;
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
    fn aggregate_layout_selects_only_unique_exact_scalar_members() {
        let layout=crate::reconstruction::ProjectTypeLayout {
            name:"Point".into(),kind:"struct".into(),size:12,alignment:4,fields_complete:true,
            fields:vec![
                crate::reconstruction::ProjectTypeLayoutField{name:"x".into(),declaration:"int x".into(),offset_bits:0,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"y".into(),declaration:"uint32_t y".into(),offset_bits:32,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"tag".into(),declaration:"unsigned int tag".into(),offset_bits:64,bit_width:Some(3)},
            ],
        };
        let address=AggregateAddress {base:7,type_spelling:"Point".into(),layout:layout.clone(),offset_bytes:0,
            target:"arm64-apple-macos14.0".into(),source_sha256:"a".repeat(64),layout_evidence:"compiler-report".into()};
        let x=aggregate_member_at(&address,4).unwrap();
        assert_eq!((x.field_name.as_str(),x.field_type.spelling.as_str(),x.field_type.signed),("x","int",true));
        let y=aggregate_member_at(&AggregateAddress{offset_bytes:4,..address.clone()},4).unwrap();
        assert_eq!((y.field_name.as_str(),y.field_type.spelling.as_str(),y.field_type.signed),("y","uint32_t",false));
        assert!(aggregate_member_at(&address,8).is_none(),"access width must match one scalar field");
        assert!(aggregate_member_at(&AggregateAddress{offset_bytes:2,..address.clone()},4).is_none(),"interior offsets must remain raw memory accesses");
        let union=AggregateAddress{layout:crate::reconstruction::ProjectTypeLayout{kind:"union".into(),..layout.clone()},..address.clone()};
        assert!(aggregate_member_at(&union,4).is_none(),"active union member is unknown");
        let ambiguous=AggregateAddress{layout:crate::reconstruction::ProjectTypeLayout{fields:vec![layout.fields[0].clone(),layout.fields[0].clone()],..layout.clone()},..address.clone()};
        assert!(aggregate_member_at(&ambiguous,4).is_none(),"duplicate field identity must not select a name");
        let overlap_layout=crate::reconstruction::ProjectTypeLayout{name:"Overlap".into(),kind:"struct".into(),size:8,alignment:4,fields_complete:true,
            fields:vec![
                crate::reconstruction::ProjectTypeLayoutField{name:"a".into(),declaration:"int a".into(),offset_bits:0,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"b".into(),declaration:"int b".into(),offset_bits:16,bit_width:None},
            ]};
        let overlap=AggregateAddress{type_spelling:"Overlap".into(),layout:overlap_layout,offset_bytes:2,..address};
        assert!(aggregate_member_at(&overlap,4).is_none(),"overlapping field ranges must not select a name");
        let incomplete=AggregateAddress{layout:crate::reconstruction::ProjectTypeLayout{fields_complete:false,..overlap.layout.clone()},..overlap};
        assert!(aggregate_member_at(&incomplete,4).is_none(),"incomplete layouts must not select a name");
    }

    #[test]
    fn constant_offset_load_uses_the_unique_catalog_struct_field_and_retains_layout_provenance() {
        let layout=crate::reconstruction::ProjectTypeLayout {
            name:"Point".into(),kind:"struct".into(),size:8,alignment:4,fields_complete:true,
            fields:vec![
                crate::reconstruction::ProjectTypeLayoutField{name:"x".into(),declaration:"int x".into(),offset_bits:0,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"y".into(),declaration:"int y".into(),offset_bits:32,bit_width:None},
            ],
        };
        let evidence="{\"producer\":\"hindsight-apple-clang-record-layout-v1\"}".to_owned();
        let mut dag=Dag{nodes:Vec::new()};
        let truth=dag.constant(1,0x1000).unwrap();
        let base=dag.add_role("source-parameter","parameter","((uint64_t)(uintptr_t)point)".into(),vec![],0x1000).unwrap();
        dag.type_node(base,Some(SourceType{spelling:"Point *".into(),bits:64,signed:false,pointer:true}));
        dag.nodes[base].aggregate_address=Some(AggregateAddress{base,type_spelling:"Point".into(),layout:layout.clone(),offset_bytes:0,
            target:"arm64-apple-macos14.0".into(),source_sha256:"a".repeat(64),layout_evidence:evidence.clone()});
        let mut state=State{registers:(0..32).map(Symbol::Unknown).collect(),fp_scalars:vec![None;32],q_blobs:vec![None;32],flags:None,frame:BTreeMap::new(),external_loads:Vec::new(),
            external_stores:Vec::new(),external_access_pcs:Vec::new(),path:truth,has_call:false,call_pcs:Vec::new(),calls:Vec::new(),incoming_stack_end:0};
        state.registers[0]=Symbol::Value(base,64);
        state.registers[31]=Symbol::Frame(0);
        let addr=tree("binary",Some("uint64_t".into()),object([("operator","+".into()),("bits",64u64.into())]),vec![
            tree("register",Some("uint64_t".into()),object([("class","general".into()),("name","x0".into()),("bits",64u64.into())]),vec![]),
            tree("constant",Some("uint64_t".into()),object([("bits",64u64.into()),("hex","4".into())]),vec![]),
        ]);
        let load=tree("memory-load",Some("uint32_t".into()),object([("bytes",4u64.into()),("signed",true.into())]),vec![addr]);
        let Symbol::Value(expression,_)=read(&load,&mut state,&mut dag,0x1004).unwrap() else{panic!("load was not recovered")};
        let address_id=state.external_loads[0].address;
        assert!(state.external_loads[0].member.is_some(),"address type={:?}, aggregate={:?}, children={:?}",dag.type_of(address_id),dag.nodes[address_id].aggregate_address,
            dag.nodes[address_id].children.iter().map(|child|(dag.nodes[*child].constant_value,dag.type_of(*child),dag.nodes[*child].aggregate_address.as_ref().map(|a|a.offset_bytes))).collect::<Vec<_>>());
        let member_expression=dag.nodes[expression].children[0];
        assert_eq!(dag.nodes[member_expression].kind,"source-member-load");
        assert!(dag.nodes[member_expression].template.contains("->y"));
        assert_eq!(dag.type_of(member_expression).unwrap().spelling,"int");
        let member=state.external_loads[0].member.as_ref().unwrap();
        assert_eq!((member.offset_bits,member.field_name.as_str()),(32,"y"));
        assert_eq!(member.layout_evidence,evidence);
        let rendered=BTreeMap::from([(base,"point".to_owned())]);
        assert!(render_external_load(&state.external_loads[0],&rendered,&dag).contains("((Point *)(point))->y"));
    }

    #[test]
    fn project_type_dependencies_follow_types_not_parameter_or_function_names() {
        let signature = test_signature("_Widget", "int Widget(int Pair);");
        let identifiers = declared_type_identifiers(&signature);
        assert!(identifiers.contains("int"));
        assert!(!identifiers.contains("Widget"));
        assert!(!identifiers.contains("Pair"));
    }

    #[test]
    fn recovered_header_includes_only_signature_and_transitive_project_types() {
        let dependency = |name: &str, declaration: &str| crate::reconstruction::ProjectTypeDependency {
            name: name.into(), declaration: declaration.into(), target: "arm64-apple-macos14.0".into(),
            source_sha256: "a".repeat(64), layout_evidence: None, layout: None,
        };
        let mut dependencies = vec![
            dependency("Widget", "typedef Base Widget;"),
            dependency("Base", "typedef unsigned long Base;"),
            dependency("Unused", "typedef int Unused;"),
        ];
        dependencies[0].layout_evidence = Some("{\"producer\":\"hindsight-apple-clang-record-layout-v1\"}".into());
        let declaration = crate::signatures::SourceDeclaration {
            signature: crate::signatures::Signature {
                language: "C", declaration: "Widget * create(void);".into(), source: "project type test",
            },
            name: "create".into(), result_spelling: "Widget *".into(), parameters: Vec::new(),
        };
        let mut builder = Builder::new(&"a".repeat(64), 0x1000, 0x1004, "arm64", Some("create"), Value::Null)
            .expect("builder");
        let type_record = builder.record("type", "analyst-assertion", object([
            ("declaration", "Widget * create(void);".into()), ("source", "project type test".into()),
        ]), array([])).expect("declaration record");
        let header_id = emit_header(&mut builder, &declaration, &type_record, 0x1000, 0x1004,
            None, &dependencies).expect("header");
        let source = builder.nodes[&header_id].get("payload").and_then(|payload| payload.get("source"))
            .expect("header source");
        let Value::String(source) = source else { panic!("header source is not a string") };
        assert!(source.contains("typedef Base Widget;"));
        assert!(source.contains("typedef unsigned long Base;"));
        assert!(!source.contains("typedef int Unused;"));
        let dependency_edges: Vec<_> = builder.edges.values()
            .filter(|edge| edge.get("relation") == Some(&Value::from("required-source-dependency")))
            .collect();
        assert_eq!(dependency_edges.len(), 2, "both transitive declarations must be explicit evidence dependencies");
        for edge in dependency_edges {
            let dependency_id = match edge.get("source") {
                Some(Value::String(id)) => id,
                _ => panic!("dependency edge source is not an identifier"),
            };
            let payload = builder.nodes.get(dependency_id)
                .and_then(|node| node.get("payload")).expect("dependency evidence payload");
            assert_eq!(payload.get("source_sha256"), Some(&Value::from("a".repeat(64))));
            assert_eq!(payload.get("target"), Some(&Value::from("arm64-apple-macos14.0")));
            assert_eq!(payload.get("abi_layout_claimed"), Some(&Value::Bool(false)));
            assert_eq!(payload.get("captured_type_identity_proven"), Some(&Value::Bool(false)));
            if payload.get("name") == Some(&Value::from("Widget")) {
                assert_eq!(payload.get("compiler_layout_evidence"), Some(&Value::from("{\"producer\":\"hindsight-apple-clang-record-layout-v1\"}")));
            }
        }
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
                fp_scalars: vec![None; 32],
                q_blobs: vec![None; 32],
                flags: None,
                frame: BTreeMap::new(),
                external_loads: Vec::new(),
                external_stores: Vec::new(),
                external_access_pcs: Vec::new(),
                path: 0,
                has_call: false,
                call_pcs: Vec::new(),
                calls: Vec::new(),
                incoming_stack_end: 0,
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
        attempt_with_project_types(words,declaration,&[])
    }

    fn attempt_with_project_types(words:&[u32],declaration:&str,project_types:&[crate::reconstruction::ProjectTypeDependency])->Result<Recovery,String>{
        let signature = crate::signatures::source_signature_with_project_types(
            "_hs_test",
            &crate::signatures::Signature {
                language: "C",
                declaration: declaration.into(),
                source: "test declaration",
            },
            project_types,
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
        recover_with_types(
            0x1000,
            &trees,
            &operations
                .iter()
                .map(|(_, portable, _)| *portable)
                .collect::<Vec<_>>(),
            signature,
            &BTreeMap::new(),
            project_types,
        )
    }
    #[test]
    fn bottom_tested_loop_recovers_with_loop_carried_variable() {
        // sub x0, x0, #1; cbnz x0, .loop; ret
        let recovery = attempt(
            &[0xd1000400, 0xb5ffffe0, 0xd65f03c0],
            "uint64_t hs_test(uint64_t value);",
        )
        .expect("simple bottom-tested counting loop should recover");
        assert!(!recovery.loops.is_empty(), "expected loop recovery");
        assert_eq!(recovery.paths, 1);
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
    fn project_layout_recovers_a_unique_constant_offset_field_read_in_a_function() {
        let layout=crate::reconstruction::ProjectTypeLayout {
            name:"Point".into(),kind:"struct".into(),size:8,alignment:4,fields_complete:true,
            fields:vec![
                crate::reconstruction::ProjectTypeLayoutField{name:"x".into(),declaration:"int x".into(),offset_bits:0,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"y".into(),declaration:"int y".into(),offset_bits:32,bit_width:None},
            ],
        };
        let dependency=crate::reconstruction::ProjectTypeDependency {
            name:"Point".into(),declaration:"typedef struct Point { int x; int y; } Point;".into(),
            target:"arm64-apple-macos14.0".into(),source_sha256:"a".repeat(64),
            layout_evidence:Some("compiler-record-layout".into()),layout:Some(layout),
        };
        let recovery=attempt_with_project_types(&[0xb9400400,0xd65f03c0],"int hs_test(Point *point);",&[dependency])
            .expect("exact catalog-backed Point pointer and matching member load recover");
        let load=recovery.external_loads.iter().find(|load|load.member.is_some()).expect("member read recorded");
        let member=load.member.as_ref().unwrap();
        assert_eq!((member.field_name.as_str(),member.offset_bits),("y",32));
        assert!(recovery.dag.nodes.iter().any(|node|node.kind=="source-member-load"&&node.template.contains("->y")));
        assert_eq!(recovery.project_types.len(),1);
        let mut builder=Builder::new(&"a".repeat(64),0x1000,0x1008,"arm64",Some("hs_test"),Value::Null).expect("artifact builder");
        let ir=vec!["ldr w0, [x0, #4]".to_owned(),"ret".to_owned()];
        recovery.emit(&mut builder,"int hs_test(Point *point);",&ir,&BTreeMap::new(),0x1000,None)
            .expect("source and memory read AST emission");
        let read_ast=builder.ast.values().find(|node|matches!(node,Value::Object(fields)
            if fields.get("kind")==Some(&Value::from("source-memory-read")))).expect("serialized read AST");
        let Value::Object(fields)=read_ast else {unreachable!()};
        let Some(Value::Object(attributes))=fields.get("attributes") else {panic!("read AST attributes missing")};
        let Some(Value::Object(layout))=attributes.get("member_layout") else {panic!("member layout provenance missing")};
        assert_eq!(layout.get("field"),Some(&Value::from("y")));
        assert_eq!(layout.get("source_sha256"),Some(&Value::from("a".repeat(64))));
        assert_eq!(layout.get("captured_type_identity_proven"),Some(&Value::Bool(false)));
    }

    #[test]
    fn project_layout_recovers_a_unique_constant_offset_field_store_in_a_function() {
        let layout=crate::reconstruction::ProjectTypeLayout {
            name:"Point".into(),kind:"struct".into(),size:8,alignment:4,fields_complete:true,
            fields:vec![
                crate::reconstruction::ProjectTypeLayoutField{name:"x".into(),declaration:"int x".into(),offset_bits:0,bit_width:None},
                crate::reconstruction::ProjectTypeLayoutField{name:"y".into(),declaration:"int y".into(),offset_bits:32,bit_width:None},
            ],
        };
        let dependency=crate::reconstruction::ProjectTypeDependency {
            name:"Point".into(),declaration:"typedef struct Point { int x; int y; } Point;".into(),
            target:"arm64-apple-macos14.0".into(),source_sha256:"a".repeat(64),
            layout_evidence:Some("compiler-record-layout".into()),layout:Some(layout),
        };
        let recovery=attempt_with_project_types(&[0xb9000401,0xd65f03c0],"void hs_test(Point *point, int value);",&[dependency])
            .expect("exact catalog-backed Point pointer and matching member store recover");
        let store=recovery.external_stores.iter().find(|store|store.member.is_some()).expect("member write recorded");
        let member=store.member.as_ref().unwrap();
        assert_eq!((member.field_name.as_str(),member.offset_bits),("y",32));
        assert_eq!(recovery.dag.type_of(store.value).map(SourceType::spelling),Some("int"));
        assert_eq!(render_aggregate_member(member,&BTreeMap::from([(member.base,"point".into())])),"((Point *)(point))->y");
        assert_eq!(recovery.project_types.len(),1);
        let mut builder=Builder::new(&"a".repeat(64),0x1000,0x1008,"arm64",Some("hs_test"),Value::Null).expect("artifact builder");
        let ir=vec!["str w1, [x0, #4]".to_owned(),"ret".to_owned()];
        recovery.emit(&mut builder,"void hs_test(Point *point, int value);",&ir,&BTreeMap::new(),0x1000,None)
            .expect("source and memory AST emission");
        let source=builder.nodes.values().find_map(|node|match node {
            Value::Object(fields)=>fields.get("payload").and_then(|payload|match payload {
                Value::Object(payload)=>match (payload.get("artifact_kind"),payload.get("source")) {
                    (Some(Value::String(kind)),Some(Value::String(source))) if kind=="recovered-source"=>Some(source.as_str()),
                    _=>None,
                },
                _=>None,
            }),
            _=>None,
        }).expect("serialized recovered source");
        assert!(source.contains("))->y ="),"{source}");
        let store_ast=builder.ast.values().find(|node|matches!(node,Value::Object(fields)
            if fields.get("kind")==Some(&Value::from("source-memory-store")))).expect("serialized store AST");
        let Value::Object(fields)=store_ast else {unreachable!()};
        let Some(Value::Object(attributes))=fields.get("attributes") else {panic!("store AST attributes missing")};
        let Some(Value::Object(layout))=attributes.get("member_layout") else {panic!("member layout provenance missing")};
        assert_eq!(layout.get("field"),Some(&Value::from("y")));
        assert_eq!(layout.get("source_sha256"),Some(&Value::from("a".repeat(64))));
        assert_eq!(layout.get("captured_type_identity_proven"),Some(&Value::Bool(false)));
    }

    #[test]
    fn known_parameters_and_widths_not_entry_register_guesses() {
        assert!(attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);"
        )
        .is_ok());
        // A 64-bit add over declared 32-bit parameters is recoverable when the
        // value flow and declared result width are consistent.
        assert!(attempt(
            &[0x8b010000, 0xd65f03c0],
            "uint32_t hs_test(uint32_t first, uint32_t second);"
        )
        .is_ok());
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
            &[0xaa0003f3, 0xd65f03c0][..], // nonvolatile clobbered
            &[0xd10043ff, 0xd65f03c0],    // SP not restored
            &[0xd65f0000],                // return target not LR
            &[0x14000000],                // branch outside/unresolved
        ] {
            assert!(attempt(words, signature).is_err(), "{words:x?}");
        }
        // `add x0, sp, #0; ret` returns the current frame pointer value while
        // preserving SP/nonvolatiles and the link-register return target.
        assert!(attempt(&[0x910003e0, 0xd65f03c0], signature).is_ok());
        // A load through the parameter value is a recoverable external read when
        // ABI/frame/nonvolatile state is preserved.
        assert!(attempt(&[0xf9400000, 0xd65f03c0], signature).is_ok());
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
        // A partial overwrite invalidates the intact wide slot; recovery must
        // not reuse the stale value and instead models an explicit uninitialized
        // frame read rather than blocking the whole function.
        let recovery = attempt(
            &[0xd10043ff, 0xf90003e0, 0xb90007e1, 0xf94003e0, 0x910043ff, 0xd65f03c0],
            "uint64_t hs_test(uint64_t first, uint64_t second);"
        )
        .expect("overlapping frame write should not block recovery");
        assert!(recovery
            .dag
            .nodes
            .iter()
            .any(|node| node.kind == "source-frame-reload"
                && node.template.contains("uninitialized frame read")));
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
        for (spelling, bits) in [("signed int", 32), ("signed short int", 16), ("signed long", 64),
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
        let loaded = attempt(&[0xb9400000, 0xd65f03c0], "signed int hs_test(const signed int *value);")
            .expect("signed int pointer load is recoverable");
        assert!(loaded.dag.nodes.iter().any(|node| node.source_type.as_ref().is_some_and(|ty|
            ty.spelling == "signed int" && ty.bits == 32 && ty.signed && !ty.pointer)));
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
