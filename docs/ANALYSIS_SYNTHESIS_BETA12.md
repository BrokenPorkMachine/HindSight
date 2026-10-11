# HindSight 0.4.0-beta.12 — Analysis synthesis

Beta.12 adds a synthesis layer above the existing bounded ARM64/Mach-O evidence model. The goal is to reduce time from **raw recovered facts → evidence-backed investigation pivot** without promoting heuristics to facts.

## Function Intelligence

Each discovered function is summarized with its bounded size, incoming/outgoing direct BL edges, non-fallthrough branch count, basic-block count, and direct self-call count. Rows are sorted by a disclosed attention score:

`5 × incoming + 3 × outgoing + 2 × non-fallthrough branches + basic blocks + min(size / 64, 25)`

The score is deliberately a navigation heuristic. It is **not** confidence, vulnerability severity, maliciousness, semantic importance, runtime reachability, or a prediction that a function is interesting. Because the formula is visible, an analyst can decide whether it is useful for the current question.

## Validated data references

The ARM64 pass recovers only explicit PC-relative forms that can be checked against parsed Mach-O section ranges:

- `ADR`: the signed instruction immediate is resolved from the instruction PC.
- adjacent `ADRP` + `ADD (immediate)`: the page-relative value is promoted only when the immediately following instruction consumes the same source register. Non-adjacent chains are omitted rather than propagated through unknown semantics.
- literal-immediate references: the signed PC-relative literal address is surfaced when the target lies in a parsed section.

Targets outside mapped sections are omitted. For `__cstring`, Objective-C runtime string sections, and Swift reflection strings, text is attached only after the target address is proven to belong to the corresponding section and a bounded NUL-terminated read succeeds.

Control-flow instructions terminate pending ADRP evidence, and incomplete chains are discarded. This intentionally trades recall for correctness.

## Analysis Insights

The synthesis workspace reports bounded counts, the highest-ranked structural function rows, and representative validated data references. Its output includes an interpretation guardrail so the summary cannot be mistaken for recovered intent or a security verdict.

## Transport and compatibility

The analysis result remains protocol kind `analysis_result` under protocol ABI 1, but its exact release-matched schema grows from 60 to 63 fields with:

- `function_intelligence`
- `data_references`
- `analysis_insights`

The existing exact-field parity validation still applies. Mixed beta.11/beta.12 app/Coordinator/worker components are unsupported because the product-version handshake is exact.

## UI contract

The new surfaces use the beta.11 responsive shell. Analysis Insights appears in the Static Analysis sidebar; Function Intelligence and Data References remain discoverable through the workspace catalog/command palette and global analysis navigator without adding permanent chrome.
