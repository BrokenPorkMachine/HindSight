# HindSight 0.5.4 — Build Fix 1

This source hotfix corrects the Rust compile failure in the initial 0.5.4 ANALYSIS-HANGFIX archive.

## Root cause

The analysis entry point owns the recovered symbol table as `Vec<(u64, String)>`, while the optimized `symbol_name_at` helper accepts `&[(u64, String)]`. Three top-level free-function calls passed the owned `Vec` rather than borrowing it. Rust does not perform method-style auto-borrowing for these free-function arguments, producing three type errors.

## Fix

- Borrow `&symbols` when formatting the function list.
- Borrow `&symbols` for both caller and callee lookup in the call graph.
- Move the SLEB128 decoder's `byte` declaration into the loop, removing the unrelated `value assigned to byte is never read` warning.
- Add a portable regression contract scoped to `pub fn analyze` so these three owned-vector call sites cannot regress silently when Cargo is unavailable on a validation host.

No analysis report fields, protocol ABI, engine/cache identity, or analysis semantics changed.
