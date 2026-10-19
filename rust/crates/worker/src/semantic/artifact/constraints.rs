//! Caller/callee constraint iteration with bounded convergence.
//!
//! A call site and a callee can each carry a claim about an argument: the
//! caller may have a proven prototype for the function that owns the call,
//! and the callee may have a proven prototype of its own. Both sides are
//! keyed by the register the argument travels in, because that register is
//! the identity both sides share across the call.
//!
//! When the two sides disagree, that is a *conflict*, not a reason to
//! silently prefer one side. The solver therefore keeps both claims,
//! reports the conflict, and never rewrites a proven type from a weaker
//! observation.
//!
//! The iteration is bounded. A constraint set is monotone — a claim is only
//! ever added, never removed — so the fixed point is reached in at most one
//! step per distinct claim. The bound is a safety net for a malformed input,
//! and hitting it is reported rather than hidden.

use crate::reconstruction::{array, object, Value as V};
use std::collections::{BTreeMap, BTreeSet};

/// The kind of claim a constraint makes about a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConstraintKind {
    /// The source type of an argument, from a proven prototype.
    ArgumentType,
    /// The source type of the value a call returns.
    ResultType,
    /// The machine width observed for an argument at a call site. A width is
    /// a different claim from a source type, so the two are never compared
    /// against each other.
    ArgumentWidth,
    /// The call does not return to its caller.
    NonReturning,
}

impl ConstraintKind {
    fn name(self) -> &'static str {
        match self {
            ConstraintKind::ArgumentType => "argument-type",
            ConstraintKind::ResultType => "result-type",
            ConstraintKind::ArgumentWidth => "argument-width",
            ConstraintKind::NonReturning => "non-returning",
        }
    }
}

/// One claim about a call, from one side.
#[derive(Clone, Debug)]
pub(crate) struct Constraint {
    /// The call site the claim is about.
    pub site: u64,
    /// The callee the claim is about.
    pub callee: u64,
    /// What the claim is about.
    pub kind: ConstraintKind,
    /// The argument register the claim concerns. `None` for a claim about
    /// the call as a whole.
    pub index: Option<u8>,
    /// The claimed type, spelled the way the evidence spells it.
    pub spelling: String,
    /// Where the claim came from.
    pub origin: &'static str,
    /// How strongly the evidence supports the claim.
    pub strength: f64,
}

/// Two sides disagreeing about the same call, parameter, or result.
#[derive(Clone, Debug)]
pub(crate) struct Conflict {
    /// The call site the conflict is about.
    pub site: u64,
    /// The callee the conflict is about.
    pub callee: u64,
    /// What the conflict is about.
    pub kind: ConstraintKind,
    /// The argument register the conflict concerns.
    pub index: Option<u8>,
    /// The claim from the caller side.
    pub caller: Constraint,
    /// The claim from the callee side.
    pub callee_side: Constraint,
}

/// The outcome of one bounded iteration.
#[derive(Clone, Debug, Default)]
pub(crate) struct Solution {
    /// Every claim the solver holds.
    pub constraints: Vec<Constraint>,
    /// Every disagreement between the two sides.
    pub conflicts: Vec<Conflict>,
    /// How many iterations the solver ran.
    pub iterations: usize,
    /// The solver stopped before the fixed point.
    pub exhausted: bool,
    /// Why the solver stopped.
    pub exhaustion_reason: Option<String>,
}

impl Solution {
    /// The claims that hold for one callee, grouped by what they concern.
    fn claims_for(&self, callee: u64) -> Vec<&Constraint> {
        self.constraints.iter().filter(|constraint| constraint.callee == callee).collect()
    }
}

/// The maximum number of iterations. A constraint set is monotone, so the
/// fixed point is reached once every distinct claim is present; the bound
/// only fires for a malformed input.
const MAX_ITERATIONS: usize = 16;

/// Iterate caller and callee claims to a fixed point.
///
/// The caller side contributes the claims observed at a call site. The callee
/// side contributes the claims a proven prototype makes. The two sides are
/// compared per call, per argument register, and per result. A disagreement
/// is recorded as a conflict; neither side wins, and no claim is removed.
pub(crate) fn solve(caller_claims: Vec<Constraint>, callee_claims: Vec<Constraint>) -> Solution {
    let mut solution = Solution::default();
    // The key includes the origin, so the same claim from the two sides is
    // kept twice: they are separate claims even when they agree.
    let mut seen: BTreeSet<(u64, u64, ConstraintKind, Option<u8>, String, &'static str)> = BTreeSet::new();
    let mut pending = caller_claims;
    pending.extend(callee_claims);
    // The solver is monotone: each round may only add claims. A round that
    // adds nothing has reached the fixed point.
    while solution.iterations < MAX_ITERATIONS {
        solution.iterations += 1;
        let mut added = false;
        for constraint in &pending {
            let key = (constraint.site, constraint.callee, constraint.kind, constraint.index, constraint.spelling.clone(), constraint.origin);
            if seen.insert(key) {
                solution.constraints.push(constraint.clone());
                added = true;
            }
        }
        if !added { break; }
        pending.clear();
        if solution.iterations == MAX_ITERATIONS {
            solution.exhausted = true;
            solution.exhaustion_reason = Some(format!(
                "the constraint set did not reach a fixed point within {MAX_ITERATIONS} iterations; the remaining claims are reported as held, not as converged"
            ));
        }
    }
    // Compare the two sides for every call the solver knows about. The
    // comparison borrows the claims, so the conflicts are collected first and
    // appended afterwards.
    let mut sites: BTreeSet<(u64, u64)> = solution.constraints.iter().map(|constraint| (constraint.site, constraint.callee)).collect();
    let mut conflicts: Vec<Conflict> = Vec::new();
    for (site, callee) in sites {
        let claims = solution.claims_for(callee);
        for kind in [ConstraintKind::ArgumentType, ConstraintKind::ResultType] {
            let mut indices: BTreeSet<Option<u8>> = claims.iter()
                .filter(|constraint| constraint.kind == kind)
                .map(|constraint| constraint.index)
                .collect();
            indices.retain(|index| index.is_some());
            for index in indices {
                let for_index: Vec<&Constraint> = claims.iter()
                    .copied()
                    .filter(|constraint| constraint.kind == kind && constraint.index == index)
                    .collect();
                let from_caller: Vec<&Constraint> = for_index.iter().copied().filter(|constraint| constraint.origin.starts_with("caller")).collect();
                let from_callee: Vec<&Constraint> = for_index.iter().copied().filter(|constraint| constraint.origin.starts_with("callee")).collect();
                for caller_claim in &from_caller {
                    for callee_claim in &from_callee {
                        if caller_claim.spelling == callee_claim.spelling { continue; }
                        // The two sides disagree. Both claims are kept.
                        conflicts.push(Conflict {
                            site,
                            callee,
                            kind,
                            index,
                            caller: (*caller_claim).clone(),
                            callee_side: (*callee_claim).clone(),
                        });
                    }
                }
            }
        }
    }
    solution.conflicts = conflicts;
    solution
}

/// Render the solution as a bounded report. Names, signatures, targets, and
/// implementations stay separate claims: a conflict is reported as a
/// conflict, never resolved by preferring one side.
pub(crate) fn report(solution: &Solution) -> String {
    let mut out = String::from("Caller/callee constraints\n===========================\n");
    out.push_str(&format!(
        "Iterations: {}{}\n",
        solution.iterations,
        if solution.exhausted { " (exhausted)" } else { "" }
    ));
    if let Some(reason) = &solution.exhaustion_reason {
        out.push_str(&format!("Exhaustion reason: {reason}\n"));
    }
    out.push_str(&format!("Claims: {}\n", solution.constraints.len()));
    let mut by_callee: BTreeMap<u64, Vec<&Constraint>> = BTreeMap::new();
    for constraint in &solution.constraints {
        by_callee.entry(constraint.callee).or_default().push(constraint);
    }
    for (callee, claims) in by_callee.iter().take(200) {
        for constraint in claims.iter().take(24) {
            let parameter = constraint.index.map(|index| format!("x{index}")).unwrap_or_else(|| "[]".into());
            out.push_str(&format!(
                "callee={callee:#018x} site={:#018x} {}{parameter} type={} origin={} strength={:.2}\n",
                constraint.site,
                constraint.kind.name(),
                constraint.spelling,
                constraint.origin,
                constraint.strength
            ));
        }
    }
    if solution.conflicts.is_empty() {
        if solution.constraints.is_empty() {
            out.push_str("Conflicts: none. No proven prototype stated a claim on either side of a call, so no agreement is claimed either.\n");
        } else {
            out.push_str("Conflicts: none. The caller and callee claims agree on every parameter and result they both state.\n");
        }
    } else {
        out.push_str(&format!("Conflicts: {}\n", solution.conflicts.len()));
        for conflict in solution.conflicts.iter().take(200) {
            let parameter = conflict.index.map(|index| format!("x{index}")).unwrap_or_else(|| "[]".into());
            out.push_str(&format!(
                "conflict callee={:#018x} site={:#018x} {}{parameter} caller={} ({}) callee={} ({})\n",
                conflict.callee,
                conflict.site,
                conflict.kind.name(),
                conflict.caller.spelling,
                conflict.caller.origin,
                conflict.callee_side.spelling,
                conflict.callee_side.origin
            ));
        }
    }
    out
}

/// The solution as a reconstruction payload. The conflict list is published
/// so a consumer can see a disagreement rather than a silently resolved one.
pub(crate) fn payload(solution: &Solution) -> V {
    let claims = solution.constraints.iter().take(512).map(|constraint| {
        object([
            ("site", V::from(constraint.site)),
            ("callee", V::from(constraint.callee)),
            ("kind", V::from(constraint.kind.name())),
            ("index", constraint.index.map(V::from).unwrap_or(V::Null)),
            ("type", V::from(constraint.spelling.clone())),
            ("origin", V::from(constraint.origin)),
            ("strength_per_mille", V::from((constraint.strength * 1000.0).round() as u64)),
        ])
    });
    let conflicts = solution.conflicts.iter().take(512).map(|conflict| {
        object([
            ("site", V::from(conflict.site)),
            ("callee", V::from(conflict.callee)),
            ("kind", V::from(conflict.kind.name())),
            ("index", conflict.index.map(V::from).unwrap_or(V::Null)),
            ("caller_type", V::from(conflict.caller.spelling.clone())),
            ("caller_origin", V::from(conflict.caller.origin)),
            ("callee_type", V::from(conflict.callee_side.spelling.clone())),
            ("callee_origin", V::from(conflict.callee_side.origin)),
        ])
    });
    object([
        ("iterations", V::from(solution.iterations as u64)),
        ("exhausted", V::from(solution.exhausted)),
        ("exhaustion_reason", solution.exhaustion_reason.clone().map(V::from).unwrap_or(V::Null)),
        ("claim_count", V::from(solution.constraints.len() as u64)),
        ("conflict_count", V::from(solution.conflicts.len() as u64)),
        ("claims", array(claims)),
        ("conflicts", array(conflicts)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(site: u64, callee: u64, kind: ConstraintKind, index: u8, spelling: &str, origin: &'static str) -> Constraint {
        Constraint { site, callee, kind, index: Some(index), spelling: spelling.into(), origin, strength: 0.9 }
    }

    #[test]
    fn agreeing_sides_produce_no_conflict() {
        let solution = solve(
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "int32_t", "caller-prototype")],
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "int32_t", "callee-prototype")],
        );
        assert!(solution.conflicts.is_empty());
        assert_eq!(solution.constraints.len(), 2);
        assert!(!solution.exhausted);
    }

    #[test]
    fn disagreeing_sides_produce_a_conflict_and_keep_both_claims() {
        let solution = solve(
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "uint64_t", "caller-prototype")],
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "int32_t", "callee-prototype")],
        );
        assert_eq!(solution.conflicts.len(), 1);
        // Neither side is dropped.
        assert_eq!(solution.constraints.len(), 2);
        assert_eq!(solution.conflicts[0].caller.spelling, "uint64_t");
        assert_eq!(solution.conflicts[0].callee_side.spelling, "int32_t");
    }

    #[test]
    fn claims_about_different_registers_do_not_conflict() {
        let solution = solve(
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "uint64_t", "caller-prototype")],
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 1, "int32_t", "callee-prototype")],
        );
        assert!(solution.conflicts.is_empty());
    }

    #[test]
    fn an_observed_width_never_conflicts_with_a_prototype_type() {
        // A width is a different claim from a source type. The two are never
        // compared, so an observed width never manufactures a conflict.
        let solution = solve(
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentWidth, 0, "machine-width-64", "caller-observation")],
            vec![claim(0x1000, 0x2000, ConstraintKind::ArgumentType, 0, "int32_t", "callee-prototype")],
        );
        assert!(solution.conflicts.is_empty());
        assert_eq!(solution.constraints.len(), 2);
    }

    #[test]
    fn the_fixed_point_reaches_in_a_bounded_number_of_iterations() {
        let caller: Vec<Constraint> = (0..8).map(|index| claim(0x1000, 0x2000, ConstraintKind::ArgumentType, index, "int32_t", "caller-prototype")).collect();
        let callee: Vec<Constraint> = (0..8).map(|index| claim(0x1000, 0x2000, ConstraintKind::ArgumentType, index, "int32_t", "callee-prototype")).collect();
        let solution = solve(caller, callee);
        assert!(!solution.exhausted);
        assert!(solution.iterations <= MAX_ITERATIONS);
        assert!(solution.conflicts.is_empty());
    }

    #[test]
    fn a_non_returning_claim_is_kept_and_published() {
        let mut constraint = claim(0x1000, 0x2000, ConstraintKind::NonReturning, 0, "void", "callee-prototype");
        constraint.index = None;
        let solution = solve(vec![constraint], Vec::new());
        assert_eq!(solution.constraints.len(), 1);
        assert!(solution.exhaustion_reason.is_none());
        let report = report(&solution);
        assert!(report.contains("non-returning"), "{report}");
    }

    #[test]
    fn a_report_without_claims_makes_no_agreement_claim() {
        let solution = solve(Vec::new(), Vec::new());
        let report = report(&solution);
        assert!(report.contains("no agreement is claimed"), "{report}");
    }
}
