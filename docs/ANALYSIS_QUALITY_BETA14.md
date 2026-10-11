# HindSight 0.4.0-beta.14 — Analysis evidence quality

Beta.14 adds an explicit quality/provenance layer to the bounded static-analysis result.

## Analysis Quality

The quality ledger reports executable bytes admitted vs. the executable mapping, supported ARM64 decoder coverage, data-in-code exclusions, discovered functions/basic blocks, direct BL edges, cross-function direct-B candidates, relocations, validated PC-relative references, and any functions whose dominator detail was omitted by the per-function memory budget. Decoder coverage is explicitly not semantic completeness.

## Cross-function transfers

An unconditional direct `B` is surfaced as a tail-call candidate only when its source belongs to one admitted function and its target is exactly the entry of a different admitted function. It remains a candidate because the instruction alone does not prove high-level call/return intent.

## Function neighborhoods

Neighborhoods are one hop and evidence-bounded. Direct `BL` edges are shown as calls. Cross-function `B` candidates are shown separately. Out-of-range direct-call targets remain external and are never assigned to the nearest/last internal function.

## Non-claims

These views do not infer vulnerability, intent, maliciousness, exploitability, or runtime reachability. Missing evidence caused by scan limits, unsupported semantics, encryption, or explicit budgets is reported as bounded/unavailable rather than treated as proof that an edge or behavior does not exist.
