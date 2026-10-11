# Binary Diff — HindSight 0.4.3

Binary Diff compares two captured artifacts in the same HindSight project using the bounded analysis evidence produced by the existing worker. It is designed for firmware, library, executable, and application build-to-build review where raw file offsets and preferred addresses may move between versions.

## Evidence model

HindSight does not call two functions equivalent merely because they occupy the same address or ordinal position. Matching proceeds in conservative stages:

1. **Unique symbol identity.** A non-generic symbol that occurs exactly once on each side is accepted as strong function-identity evidence.
2. **Exact normalized code fingerprint.** Remaining functions may match when their normalized ARM64 instruction streams are exactly identical. Absolute hexadecimal addresses are normalized so image slides/layout movement do not create a false change.
3. **No fuzzy fallback.** Functions that cannot be paired by those rules remain **Added** or **Removed**. HindSight deliberately avoids speculative similarity matching in this beta.

The fingerprint is a stable FNV-1a implementation used only as a local identity aid. It is not a cryptographic authenticity claim.

## Change dimensions

For an accepted function pair, Binary Diff reports change independently across:

- normalized instruction stream;
- CFG block/edge shape;
- direct reference/call structure; and
- bounded lifted IR statements.

A function can therefore move in the image without appearing changed, or show a CFG/reference change even when its total byte size happens to remain constant.

**Lifted semantic change** means the bounded worker IR changed after HindSight normalization. It is not a proof that high-level program behavior changed, and it is not presented as recovered source code.

## Analysis cache

The workbench keeps analyzed reports for project records during the current session. `Analyze Pair` analyzes only the missing side(s) and does not require changing the selected artifact. The cache is session-local in 0.4.0-beta.12; persistent cross-session analysis-result caching is a later storage feature.

## UI

The Binary Diff workspace provides:

- baseline/candidate artifact selectors;
- changed/added/removed/semantic/CFG filters;
- function/address search;
- summary metrics;
- explicit match evidence;
- dimension-by-dimension counts; and
- side-by-side bounded disassembly with shared address pivots.

Binary Diff is an analysis aid, not an automated security verdict. Analysts should use the Evidence Graph and Investigation Ledger to preserve conclusions and the evidence supporting them.
