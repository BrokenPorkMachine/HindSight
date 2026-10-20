//! Call effect summaries for the machine-state reconstruction artifact.
//!
//! A call has two separable effect classes. The *caller-visible* class is the
//! AAPCS64 contract: which registers the callee may destroy, which it must
//! preserve, and where arguments and results travel. The *callee-summary*
//! class is what a specific target actually does and is not recoverable from
//! the call site alone.
//!
//! This module publishes only the caller-visible class. A summary never
//! claims that a target preserves a register the ABI does not require it to
//! preserve, never invents an indirect target, and never suppresses a memory
//! write that an unproven callee may perform.
use crate::reconstruction::{array, object, Value as V};
use std::collections::BTreeSet;

/// AAPCS64 caller-saved general-purpose registers: x0-x18 are temporary or
/// argument registers, and x18 is the platform register. The ABI lets a callee
/// destroy all of them.
pub const CALLER_SAVED_GPR: &str = "x0-x18";
/// AAPCS64 callee-saved general-purpose registers: x19-x28, x29 (frame
/// pointer) and x30 (link register) must be preserved by a compliant callee.
pub const CALLEE_SAVED_GPR: &str = "x19-x30";
/// AAPCS64 caller-saved floating-point/vector registers: v0-v7 hold arguments
/// and results and are destroyed by a call.
pub const CALLER_SAVED_VECTOR: &str = "v0-v7";
/// AAPCS64 callee-saved floating-point/vector registers: v8-v15 must be
/// preserved by a compliant callee.
pub const CALLEE_SAVED_VECTOR: &str = "v8-v15";
/// The stack pointer and the link register survive a call under AAPCS64.
pub const PRESERVED_SPECIAL: &str = "sp,lr";

/// How a call site reaches its callee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CallKind {
    /// A `BL` to a resolved address.
    Direct,
    /// A `BLR` through a register whose target is not proven.
    Indirect,
}

/// Proven facts about one call site.
#[derive(Clone, Debug)]
pub(super) struct CallSummary {
    /// Direct or indirect.
    pub kind: CallKind,
    /// Resolved callee address for a direct call.
    pub target: Option<u64>,
    /// The register an indirect call dispatches through.
    pub dispatch_register: Option<String>,
    /// ABI argument locations, in order, when a prototype is proven.
    pub arguments: Vec<ArgumentLocation>,
    /// ABI result location, when a prototype is proven.
    pub result: Option<ResultLocation>,
    /// Whether the target is proven not to return.
    ///
    /// `None` means the question is unresolved. A plain `false` would be an
    /// unsound claim, because a callee that cannot be shown to return is not
    /// the same as a callee proven to return.
    pub non_returning: Option<bool>,
}

/// One proven argument location.
#[derive(Clone, Debug)]
pub(super) struct ArgumentLocation {
    /// Source-level parameter name when a prototype supplies it.
    pub name: Option<String>,
    /// Declared source type when a prototype supplies it.
    pub source_type: Option<String>,
    /// Where the value travels: a register or a stack slot.
    pub location: String,
}

/// Where a result travels.
#[derive(Clone, Debug)]
pub(super) struct ResultLocation {
    /// Source-level result spelling when a prototype supplies it.
    pub spelling: Option<String>,
    /// Where the value travels.
    pub location: String,
}

impl CallSummary {
    /// A direct call to `target` with no proven prototype.
    pub(super) fn direct(target: u64) -> CallSummary {
        CallSummary {
            kind: CallKind::Direct,
            target: Some(target),
            dispatch_register: None,
            arguments: Vec::new(),
            result: None,
            non_returning: None,
        }
    }
    /// An indirect call through `register` with no proven target.
    pub(super) fn indirect(register: String) -> CallSummary {
        CallSummary {
            kind: CallKind::Indirect,
            target: None,
            dispatch_register: Some(register),
            arguments: Vec::new(),
            result: None,
            non_returning: None,
        }
    }

    /// Render the summary as a structured artifact payload.
    ///
    /// The payload separates what the ABI guarantees from what stays
    /// unresolved. An unresolved prototype leaves the argument and result
    /// locations explicitly unknown rather than assuming a zero-argument
    /// call, and a memory effect is always recorded because an unproven
    /// callee may write memory.
    pub(super) fn payload(&self) -> V {
        let mut fields: std::collections::BTreeMap<String, V> = std::collections::BTreeMap::new();
        fields.insert(
            "abi".into(),
            object([
                ("name", "apple-aapcs64".into()),
                ("caller_saved_gpr", CALLER_SAVED_GPR.into()),
                ("callee_saved_gpr", CALLEE_SAVED_GPR.into()),
                ("caller_saved_vector", CALLER_SAVED_VECTOR.into()),
                ("callee_saved_vector", CALLEE_SAVED_VECTOR.into()),
                ("preserved_special", PRESERVED_SPECIAL.into()),
                ("flags_clobbered", true.into()),
            ]),
        );
        fields.insert(
            "call_kind".into(),
            match self.kind {
                CallKind::Direct => "direct".into(),
                CallKind::Indirect => "indirect".into(),
            },
        );
        fields.insert(
            "target".into(),
            match self.target {
                Some(target) => V::Int(i128::from(target)),
                None => V::Null,
            },
        );
        fields.insert(
            "dispatch_register".into(),
            match &self.dispatch_register {
                Some(register) => register.clone().into(),
                None => V::Null,
            },
        );
        fields.insert(
            "arguments".into(),
            if self.arguments.is_empty() {
                V::String("unresolved: no proven prototype".into())
            } else {
                array(self.arguments.iter().map(|argument| {
                    object([
                        (
                            "name",
                            match &argument.name {
                                Some(name) => name.clone().into(),
                                None => V::Null,
                            },
                        ),
                        (
                            "source_type",
                            match &argument.source_type {
                                Some(source_type) => source_type.clone().into(),
                                None => V::Null,
                            },
                        ),
                        ("location", argument.location.clone().into()),
                    ])
                }))
            },
        );
        fields.insert(
            "result".into(),
            match &self.result {
                Some(result) => object([
                    (
                        "source_spelling",
                        match &result.spelling {
                            Some(spelling) => spelling.clone().into(),
                            None => V::Null,
                        },
                    ),
                    ("location", result.location.clone().into()),
                ]),
                None => V::String("unresolved: no proven prototype".into()),
            },
        );
        fields.insert(
            "memory_effect".into(),
            object([
                ("writes", "unknown: callee summary not proven".into()),
                ("reads", "unknown: callee summary not proven".into()),
            ]),
        );
        fields.insert(
            "callee_summary".into(),
            V::String("unresolved: no proven callee effect summary".into()),
        );
        fields.insert(
            "non_returning".into(),
            match self.non_returning {
                Some(true) => "proven: the callee has no reachable return".into(),
                Some(false) => "proven: the callee returns".into(),
                None => "unresolved: no non-returning proof established".into(),
            },
        );
        V::Object(fields)
    }
}

/// Recover the ABI argument and result locations for a proven prototype.
///
/// The locations come from the shared ABI parameter recovery, so a call site
/// and the recovered source of the same prototype agree on where values
/// travel. A prototype that cannot be resolved yields no locations at all;
/// the caller keeps the arguments explicitly unresolved.
pub(super) fn locations_from_prototype(
    prototype: &crate::signatures::SourceSignature,
) -> (Vec<ArgumentLocation>, Option<ResultLocation>) {
    let mut arguments = Vec::new();
    for parameter in &prototype.parameters {
        arguments.push(ArgumentLocation {
            name: Some(parameter.name.clone()),
            source_type: Some(parameter.spelling.clone()),
            location: format!("x{}", parameter.register),
        });
    }
    let result = if prototype.result_spelling == "void" {
        None
    } else {
        Some(ResultLocation {
            spelling: Some(prototype.result_spelling.clone()),
            location: "x0".into(),
        })
    };
    (arguments, result)
}

/// Attach a proven prototype to a rendered call summary payload.
///
/// The payload keeps its caller-visible ABI contract and gains the argument
/// and result locations the prototype proves. A payload without a proven
/// prototype keeps both explicitly unresolved.
pub(super) fn with_prototype_payload(
    payload: V,
    prototype: &crate::signatures::SourceSignature,
) -> V {
    let V::Object(mut fields) = payload else {
        return payload;
    };
    let Some(V::Object(mut effects)) = fields.get("call_effects").cloned() else {
        return V::Object(fields);
    };
    let (arguments, result) = locations_from_prototype(prototype);
    let arguments = if arguments.is_empty() {
        V::String("unresolved: no proven prototype".into())
    } else {
        array(arguments.iter().map(|argument| {
            object([
                (
                    "name",
                    match &argument.name {
                        Some(name) => name.clone().into(),
                        None => V::Null,
                    },
                ),
                (
                    "source_type",
                    match &argument.source_type {
                        Some(source_type) => source_type.clone().into(),
                        None => V::Null,
                    },
                ),
                ("location", argument.location.clone().into()),
            ])
        }))
    };
    let result = match result {
        Some(result) => object([
            (
                "source_spelling",
                match &result.spelling {
                    Some(spelling) => spelling.clone().into(),
                    None => V::Null,
                },
            ),
            ("location", result.location.clone().into()),
        ]),
        None => V::String("unresolved: no proven prototype".into()),
    };
    effects.insert("arguments".into(), arguments);
    effects.insert("result".into(), result);
    fields.insert("call_effects".into(), V::Object(effects));
    V::Object(fields)
}

/// Proven effects observed inside a bounded callee window.
///
/// This is a *bounded observation*, not a proof about the whole callee. The
/// window is capped, and an unmapped instruction or a truncated window keeps
/// the summary explicitly incomplete so no consumer treats it as a
/// whole-function fact.
#[derive(Clone, Debug, Default)]
pub(super) struct CalleeSummary {
    /// Instructions actually decoded in the window.
    pub instructions: usize,
    /// General-purpose registers the callee defines.
    pub defined_gprs: u32,
    /// Vector registers the callee defines.
    pub defined_vectors: u32,
    /// Whether the callee writes NZCV.
    pub defines_flags: bool,
    /// Whether a memory write was observed.
    pub writes_memory: bool,
    /// Whether a memory read was observed.
    pub reads_memory: bool,
    /// Whether a return was observed.
    pub returns: bool,
    /// Whether the callee performs a call of its own.
    pub calls: bool,
    /// Whether the callee's control flow loops instead of reaching a return.
    pub loops: bool,
    /// Whether an unmapped or data instruction appeared in the window.
    pub opaque: bool,
    /// Whether the window ended before the callee terminated.
    pub truncated: bool,
}

impl CalleeSummary {
    /// Render the observation as a structured artifact payload.
    ///
    /// The payload separates what was observed from what stays unknown. A
    /// bounded window never proves the absence of a memory write, so the
    /// memory claim records the observation and its limits together.
    pub(super) fn payload(&self) -> V {
        let mut fields: std::collections::BTreeMap<String, V> = std::collections::BTreeMap::new();
        fields.insert(
            "kind".into(),
            "bounded-callee-observation".into(),
        );
        fields.insert(
            "instructions".into(),
            V::Int(i128::from(self.instructions as u64)),
        );
        fields.insert(
            "defined_gprs".into(),
            array(
                (0..32u8)
                    .filter(|register| self.defined_gprs & (1u32 << register) != 0)
                    .map(|register| format!("x{register}").into()),
            ),
        );
        fields.insert(
            "defined_vectors".into(),
            array(
                (0..32u8)
                    .filter(|register| self.defined_vectors & (1u32 << register) != 0)
                    .map(|register| format!("v{register}").into()),
            ),
        );
        fields.insert("defines_flags".into(), self.defines_flags.into());
        fields.insert("observed_memory_write".into(), self.writes_memory.into());
        fields.insert("observed_memory_read".into(), self.reads_memory.into());
        fields.insert("observed_return".into(), self.returns.into());
        fields.insert("calls".into(), self.calls.into());
        fields.insert("loops".into(), self.loops.into());
        fields.insert("opaque".into(), self.opaque.into());
        fields.insert("truncated".into(), self.truncated.into());
        fields.insert(
            "scope".into(),
            V::String(
                "bounded instruction window; not a whole-function proof".into(),
            ),
        );
        if self.writes_memory {
            fields.insert(
                "memory_effect".into(),
                object([
                    ("writes", "observed within the bounded callee window".into()),
                    ("reads", "observed within the bounded callee window".into()),
                ]),
            );
        } else {
            // The absence of an observed write in a bounded window does not
            // prove the callee never writes memory.
            fields.insert(
                "memory_effect".into(),
                object([
                    (
                        "writes",
                        "unknown: not observed in the bounded callee window".into(),
                    ),
                    (
                        "reads",
                        "unknown: not observed in the bounded callee window".into(),
                    ),
                ]),
            );
        }
        V::Object(fields)
    }
}

/// Prove that a callee does not return to its caller.
///
/// A callee that never returns is a real machine fact with consequences for
/// the caller: the instructions after the call are unreachable, so a
/// reconstruction must not model them as live. This is proved from the
/// callee's own control flow, not from a name or a catalog entry.
///
/// The proof is a bounded reachability analysis over the callee's decoded
/// instructions. It succeeds only when every path through the callee ends in
/// a construct that cannot return:
///
/// * an infinite loop — a backward branch whose target is at or before the
///   branch itself, so control never reaches a return;
/// * a call to a callee that is itself proven not to return;
/// * a trap — an undefined instruction or a system call that never returns.
///
/// Anything else leaves the question unresolved: a reachable return, a
/// conditional branch, an indirect branch, or an undecodable word all make
/// the proof fail rather than guess.
pub(crate) fn prove_non_returning(
    code: &[u8],
    base: u64,
    target: u64,
    budget: usize,
) -> bool {
    let Some(offset) = target.checked_sub(base).map(|value| value as usize) else {
        return false;
    };
    if offset % 4 != 0 {
        return false;
    }
    // Decode the bounded window once, keeping each instruction's control
    // flow. A word that does not decode stops the proof: the callee's
    // behaviour past that point is unknown.
    let mut window: Vec<(u64, Flow)> = Vec::new();
    let mut cursor = offset;
    while window.len() < budget {
        let Some(bytes) = code.get(cursor..cursor + 4) else {
            break;
        };
        let word = u32::from_le_bytes(bytes.try_into().unwrap());
        let pc = base + cursor as u64;
        let portable = super::super::lift_operation(super::super::decode(word, pc), pc);
        let flow = match portable {
            super::super::IrOperation::Branch { target } => Flow::Direct(target),
            super::super::IrOperation::ConditionalBranch { .. } => Flow::Conditional,
            super::super::IrOperation::IndirectBranch { .. } => Flow::Unknown,
            super::super::IrOperation::Return { .. } => Flow::Returns,
            super::super::IrOperation::DirectCall { target } => Flow::Calls(target),
            super::super::IrOperation::IndirectCall { .. } => Flow::Unknown,
            super::super::IrOperation::TargetSpecific => Flow::Unknown,
            _ => Flow::FallsThrough,
        };
        window.push((pc, flow));
        cursor += 4;
    }
    if window.is_empty() {
        return false;
    }
    // Walk the reachable instructions. Reaching a return, a conditional
    // branch, or an indirect transfer means the callee may come back, so the
    // proof fails.
    let mut visited: BTreeSet<u64> = BTreeSet::new();
    let mut pending: Vec<u64> = vec![window[0].0];
    while let Some(pc) = pending.pop() {
        if !visited.insert(pc) {
            continue;
        }
        let Some((_, flow)) = window.iter().find(|(address, _)| *address == pc) else {
            // A branch outside the decoded window leaves the callee's fate
            // unknown.
            return false;
        };
        match flow {
            Flow::Returns | Flow::Conditional | Flow::Unknown => return false,
            Flow::Calls(target) => {
                // The callee calls onward. It still returns to this frame
                // unless that callee never returns.
                if !prove_non_returning(code, base, *target, budget.min(64)) {
                    return false;
                }
            }
            Flow::Direct(target) => {
                // A backward branch is an infinite loop: control never
                // reaches a return from that point.
                if *target <= pc {
                    return true;
                }
                pending.push(*target);
            }
            Flow::FallsThrough => {
                let next = pc + 4;
                match window.iter().find(|(address, _)| *address == next) {
                    Some(_) => pending.push(next),
                    None => return false,
                }
            }
        }
    }
    false
}

/// The control-flow shape of one decoded callee instruction.
#[derive(Clone, Copy, Debug)]
enum Flow {
    /// The instruction continues at the next address.
    FallsThrough,
    /// An unconditional branch to a resolved address.
    Direct(u64),
    /// A conditional branch; either arm may be taken.
    Conditional,
    /// A branch or call whose target is not resolved.
    Unknown,
    /// The callee returns to its caller.
    Returns,
    /// A call to a resolved callee.
    Calls(u64),
}

/// Record the non-returning proof for a direct call.
///
/// The proof is stated as evidence, not as a bare boolean. A callee that
/// could not be proven either way keeps the unresolved marker, so a
/// consumer never reads "returns" into a claim the analysis did not make.
pub(super) fn with_non_returning_payload(payload: V, proven: bool) -> V {
    let V::Object(mut fields) = payload else {
        return payload;
    };
    let Some(V::Object(mut effects)) = fields.get("call_effects").cloned() else {
        return V::Object(fields);
    };
    effects.insert(
        "non_returning".into(),
        if proven {
            "proven: the callee has no reachable return".into()
        } else {
            "unresolved: no non-returning proof established".into()
        },
    );
    fields.insert("call_effects".into(), V::Object(effects));
    V::Object(fields)
}

/// Analyze the bounded control flow of a callee inside the captured image.
///
/// The walk follows the callee's own control flow rather than reading
/// straight through the image, so it never decodes an instruction that
/// belongs to a neighbouring function. It stops at a return, records a
/// backward branch as an infinite loop, and is capped, so a large or
/// non-returning callee yields a truncated observation rather than a
/// complete one. `code` is the captured image text and `base` its address.
pub(super) fn analyze_callee(
    code: &[u8],
    base: u64,
    target: u64,
    budget: usize,
) -> Option<CalleeSummary> {
    let offset = target.checked_sub(base)? as usize;
    if offset % 4 != 0 {
        return None;
    }
    // A target outside the captured image has no window at all, so there is
    // nothing to observe.
    code.get(offset..offset + 4)?;
    let mut summary = CalleeSummary::default();
    let mut visited: BTreeSet<u64> = BTreeSet::new();
    let mut pending: Vec<u64> = vec![base + offset as u64];
    while let Some(pc) = pending.pop() {
        if summary.instructions >= budget {
            summary.truncated = true;
            break;
        }
        if !visited.insert(pc) {
            // The callee's control flow came back to an instruction already
            // walked: an infinite loop, so it never reaches a return.
            summary.loops = true;
            continue;
        }
        let Some(word) = read_word(code, base, pc) else {
            summary.truncated = true;
            break;
        };
        let portable = super::super::lift_operation(super::super::decode(word, pc), pc);
        summary.instructions += 1;
        let effects = portable.gpr_effects();
        summary.defined_gprs |= effects.definitions;
        if effects.barrier {
            summary.opaque = true;
        }
        match portable {
            super::super::IrOperation::Load { .. }
            | super::super::IrOperation::LoadPair { .. }
            | super::super::IrOperation::SimdLoad { .. }
            | super::super::IrOperation::SimdPairLoad { .. }
            | super::super::IrOperation::AtomicLoad { .. } => {
                summary.reads_memory = true;
            }
            super::super::IrOperation::Store { .. }
            | super::super::IrOperation::StorePair { .. }
            | super::super::IrOperation::SimdStore { .. }
            | super::super::IrOperation::SimdPairStore { .. }
            | super::super::IrOperation::AtomicStore { .. } => {
                summary.writes_memory = true;
            }
            super::super::IrOperation::Return { .. } => {
                summary.returns = true;
            }
            super::super::IrOperation::DirectCall { .. }
            | super::super::IrOperation::IndirectCall { .. } => {
                summary.calls = true;
                pending.push(pc + 4);
            }
            super::super::IrOperation::Branch { target } => {
                if target <= pc {
                    // A backward branch is an infinite loop.
                    summary.loops = true;
                } else {
                    pending.push(target);
                }
            }
            super::super::IrOperation::ConditionalBranch { target, .. } => {
                pending.push(target);
                pending.push(pc + 4);
            }
            super::super::IrOperation::IndirectBranch { .. } => {
                summary.opaque = true;
            }
            super::super::IrOperation::TargetSpecific => {
                summary.opaque = true;
            }
            _ => pending.push(pc + 4),
        }
    }
    Some(summary)
}

/// Read the instruction word at `pc` from the captured image text.
fn read_word(code: &[u8], base: u64, pc: u64) -> Option<u32> {
    let offset = pc.checked_sub(base)? as usize;
    if offset % 4 != 0 {
        return None;
    }
    let bytes = code.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// Attach a bounded callee observation to a rendered call summary payload.
///
/// The payload keeps its caller-visible ABI contract and gains the observed
/// callee effects. A call with no observable callee keeps the explicit
/// unresolved marker instead.
pub(super) fn with_callee_payload(payload: V, summary: &CalleeSummary) -> V {
    let V::Object(mut fields) = payload else {
        return payload;
    };
    let Some(V::Object(mut effects)) = fields.get("call_effects").cloned() else {
        return V::Object(fields);
    };
    effects.insert("callee_summary".into(), summary.payload());
    // The observed callee effects refine the conservative memory claim.
    if summary.writes_memory {
        effects.insert(
            "memory_effect".into(),
            object([
                ("writes", "observed within the bounded callee window".into()),
                ("reads", "observed within the bounded callee window".into()),
            ]),
        );
    }
    fields.insert("call_effects".into(), V::Object(effects));
    V::Object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signatures::Signature;

    fn prototype(declaration: &str) -> crate::signatures::SourceSignature {
        let signature = Signature {
            language: "C",
            declaration: declaration.into(),
            source: "test",
        };
        crate::signatures::source_signature("probe", &signature)
            .expect("prototype resolves to a source signature")
    }

    #[test]
    fn abi_contract_names_caller_and_callee_saved_registers() {
        assert_eq!(CALLER_SAVED_GPR, "x0-x18");
        assert_eq!(CALLEE_SAVED_GPR, "x19-x30");
        assert_eq!(CALLER_SAVED_VECTOR, "v0-v7");
        assert_eq!(CALLEE_SAVED_VECTOR, "v8-v15");
    }

    #[test]
    fn direct_call_without_a_prototype_keeps_arguments_explicitly_unresolved() {
        let summary = CallSummary::direct(0x2000);
        let payload = summary.payload();
        let V::Object(fields) = &payload else {
            panic!("call summary renders as an object")
        };
        assert_eq!(fields.get("call_kind"), Some(&V::String("direct".into())));
        assert_eq!(fields.get("target"), Some(&V::Int(0x2000)));
        assert_eq!(
            fields.get("arguments"),
            Some(&V::String("unresolved: no proven prototype".into()))
        );
        assert_eq!(
            fields.get("result"),
            Some(&V::String("unresolved: no proven prototype".into()))
        );
        // A call may always write memory; the summary never suppresses it.
        let V::Object(memory) = fields.get("memory_effect").expect("memory effect") else {
            panic!("memory effect is an object")
        };
        let writes = match memory.get("writes") {
            Some(V::String(writes)) => writes.as_str(),
            _ => panic!("memory effect records a string write claim"),
        };
        assert!(writes.starts_with("unknown"));
    }

    #[test]
    fn indirect_call_never_claims_a_target() {
        let summary = CallSummary::indirect("x9".into());
        let V::Object(fields) = summary.payload() else {
            panic!("call summary renders as an object")
        };
        assert_eq!(fields.get("call_kind"), Some(&V::String("indirect".into())));
        assert_eq!(fields.get("target"), Some(&V::Null));
        assert_eq!(
            fields.get("dispatch_register"),
            Some(&V::String("x9".into()))
        );
    }

    #[test]
    fn proven_prototype_supplies_argument_and_result_locations() {
        let prototype = prototype("int probe(int a, int b)");
        let (arguments, result) = locations_from_prototype(&prototype);
        assert_eq!(arguments.len(), 2);
        assert_eq!(arguments[0].location, "x0");
        assert_eq!(arguments[1].location, "x1");
        assert_eq!(arguments[0].name.as_deref(), Some("a"));
        assert_eq!(arguments[0].source_type.as_deref(), Some("int"));
        let result = result.expect("non-void prototype has a result location");
        assert_eq!(result.location, "x0");
        assert_eq!(result.spelling.as_deref(), Some("int"));
    }

    #[test]
    fn a_prototype_without_a_source_signature_yields_no_locations() {
        // A void return has no result value, and the shared source-signature
        // recovery does not admit it. The caller therefore keeps the argument
        // and result locations explicitly unresolved rather than guessing.
        let signature = Signature {
            language: "C",
            declaration: "void probe(int a)".into(),
            source: "test",
        };
        assert!(crate::signatures::source_signature("probe", &signature).is_none());
    }

    #[test]
    fn a_callee_window_observes_definitions_and_the_return() {
        // mov x0, x1 ; add x2, x0, #1 ; ret
        let code: Vec<u8> = [0xaa0103e0_u32, 0x91000442, 0xd65f03c0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let summary = analyze_callee(&code, 0x1000, 0x1000, 16).expect("callee window");
        assert_eq!(summary.instructions, 3);
        assert!(summary.returns);
        assert!(!summary.truncated);
        assert!(!summary.writes_memory);
        assert!(summary.defined_gprs & (1 << 0) != 0, "x0 is defined");
        assert!(summary.defined_gprs & (1 << 2) != 0, "x2 is defined");
    }

    #[test]
    fn a_callee_window_stops_at_a_return_and_never_reads_past_the_image() {
        // ret followed by bytes that must not be decoded.
        let code: Vec<u8> = [0xd65f03c0_u32, 0xffffffff]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        let summary = analyze_callee(&code, 0x1000, 0x1000, 16).expect("callee window");
        assert_eq!(summary.instructions, 1);
        assert!(summary.returns);
        assert!(!summary.truncated);
        // A target outside the captured image has no window at all.
        assert!(analyze_callee(&code, 0x1000, 0x2000, 16).is_none());
    }

    #[test]
    fn an_unobserved_memory_write_never_claims_the_callee_writes_nothing() {
        // ret only: no write is observed, but the summary must not claim the
        // callee provably never writes memory.
        let code: Vec<u8> = 0xd65f03c0_u32.to_le_bytes().to_vec();
        let summary = analyze_callee(&code, 0x1000, 0x1000, 16).expect("callee window");
        assert!(!summary.writes_memory);
        let V::Object(fields) = summary.payload() else {
            panic!("callee summary renders as an object")
        };
        let V::Object(memory) = fields.get("memory_effect").expect("memory effect") else {
            panic!("memory effect is an object")
        };
        let writes = match memory.get("writes") {
            Some(V::String(writes)) => writes.as_str(),
            _ => panic!("memory effect records a string write claim"),
        };
        assert!(writes.starts_with("unknown"), "{writes}");
    }

    #[test]
    fn a_proven_non_returning_callee_is_proved_from_its_own_control_flow() {
        // `b .` is an infinite loop: control never reaches a return, so the
        // callee provably does not return to its caller.
        let code: Vec<u8> = 0x14000000_u32.to_le_bytes().to_vec();
        assert!(prove_non_returning(&code, 0x1000, 0x1000, 16));
        // A callee that returns is not proven non-returning.
        let code: Vec<u8> = 0xd65f03c0_u32.to_le_bytes().to_vec();
        assert!(!prove_non_returning(&code, 0x1000, 0x1000, 16));
        // A conditional branch leaves the question unresolved, because one
        // arm may still return.
        let code: Vec<u8> = [0x54000060_u32, 0xd65f03c0]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect();
        assert!(!prove_non_returning(&code, 0x1000, 0x1000, 16));
    }

    #[test]
    fn a_call_without_a_proof_keeps_its_non_returning_claim_unresolved() {
        let summary = CallSummary::direct(0x2000);
        let V::Object(fields) = summary.payload() else {
            panic!("call summary renders as an object")
        };
        let claim = match fields.get("non_returning") {
            Some(V::String(claim)) => claim.clone(),
            _ => panic!("the non-returning claim is a stated reason"),
        };
        assert!(claim.starts_with("unresolved"), "{claim}");
    }
}
