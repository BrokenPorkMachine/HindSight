# AArch64 source-parameter identities

The pseudocode renderer consumes a bounded, shared-IR parameter-identity
analysis in `rust/crates/worker/src/semantic/names.rs`. Assignment strings,
comment capitalization and rendered branch targets no longer decide whether
a source parameter survives. Text remains a presentation layer; general typed
AST reconstruction and canonical memory/FP/flags SSA remain open.

## Implemented scope

- Supported ABI declarations seed parameter identity and source width.
  Untyped GP candidates select their register view from entry-reaching IR
  reads; a later use after replacement cannot retroactively type the input.
- Copy chains retain entry identity rather than growing display strings.
  Distinct copies of the same entry value can agree at a CFG join; conflicts,
  killed paths and entry/backedge disagreements retain machine registers and
  produce bounded join-alternative notes.
- W/X and S/D views are not interchangeable source values. Narrowing a known
  64-bit parameter loses its name; scalar conversions and SIMD lane extracts
  are not source-variable copies. Both destinations of paired writes matter.
- Modeled SP/frame adjustments and exact non-overlapping spills/restores can
  retain parameter identities, including GP/FP pairs and pre/post writeback.
  Saved values require compatible bank and sufficient access width. Partial
  overlapping writes, possible aliases, calls and unknown effects invalidate
  affected saved state. Runtime call summaries and general alias/memory SSA
  are not implied.
- Per-instruction input and output names are separate. An overwritten register
  does not keep its former parameter name on the assignment's left side.
  Presentation substitution advances names after each emitted assignment,
  preserves comments/quoted text/callable names, and handles writeback order.
- Scalar pre-indexed accesses use the updated base without applying the offset
  twice. An `LDP` whose first destination overwrites its addressing base
  captures both accesses' original effective address in an instruction-scoped
  temporary before either load.

Analysis requires complete aligned function bytes, at most 128 instructions,
32 seeds and 64 saved identity slots. The identity fixed point has a 32,768-step
budget; frame offsets are bounded by the existing ±1 MiB frame model. Unknown
indirect successors, unavailable/truncated bytes and exhausted analysis budgets
withhold substitutions with an explicit reason. Join annotations have a 4 KiB
per-function budget and explicit truncation. Data-in-code acts as an unknown
effect, not an instruction carrying an identity.

## Qualification

```sh
cargo test --manifest-path rust/Cargo.toml -p reworkbench-worker
python3 scripts/test-arm64-nameflow.py rust/target/debug/reworker
python3 scripts/run-decompilation-corpus.py --worker rust/target/debug/reworker
```

Unit tests exercise raw instruction words and structured effects, including
widths, copies, joins, loops, unreachable predecessors, spills, alias/overlap
invalidation, paired writes, writeback, data-in-code and budgets. The previous
text-only naming fixtures now exercise the production bytecode analysis.

Six CC0 assembly functions are built by Apple Clang. Hash-bound declarations
from their header supply legitimate name/type evidence. The integration gate
checks actual recovered pseudocode and, on Apple Silicon, 1,560 original-native
result checks over copy/frame/pair/FP/pre-index/overlapping-base fixtures.
Intel macOS cross-assembles and checks pseudocode without claiming execution.
These are naming/effect checks, not differential execution of reconstructed
source: typed AST export and original/IR/generated equivalence are still open.
