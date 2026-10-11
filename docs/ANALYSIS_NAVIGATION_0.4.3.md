# HindSight 0.4.3 — Structured Analysis Navigation

HindSight already produced broad static-analysis evidence by 0.4.2, but many channels still behaved like isolated text reports. 0.4.3 changes the consumption layer without changing the 66-field `AnalysisReport` ABI or the ARM64 analysis-engine identity.

## Analysis Triage

The **Analysis Triage** workspace presents a bounded focus queue assembled from existing validated report channels. Sources are ordered for navigation efficiency, beginning with synthesis/quality/function-connectivity evidence and then moving through data, control-flow, and linkage evidence.

This ordering is deliberately **not** a vulnerability, exploitability, semantic-importance, reachability, or severity score. HindSight does not infer those claims from structural density.

Each triage card preserves:

- the originating analysis source;
- the original evidence line;
- the first explicit virtual address, when present;
- an explicit symbol identity when the report line provides one; and
- the workspace that owns the underlying source evidence.

From a card the analyst can select evidence, open its source workspace, jump to Disassembly or CFG when a virtual address is available, or open Selected Symbol when an explicit symbol identity exists.

## Structured Search & Navigate

The Navigator no longer flattens all matching evidence into an anonymous monospaced text blob. Search results are source-labelled cards with the same navigation affordances as Analysis Triage. Search remains bounded to 500 results and individual evidence lines are bounded before presentation.

The report itself is unchanged. 0.4.3 indexes and routes the evidence already present in the validated result rather than adding a second analysis interpretation layer.

## Related Evidence

The global evidence inspector now correlates the selected row across analysis channels using identities already present in the evidence:

- exact parsed virtual-address equality; or
- explicit selected-symbol text.

Related evidence remains source-labelled and independently navigable. A correlation means only that the channels share the selected address or symbol identity; it is not promoted into a causal or semantic relationship.

## Generic evidence rows

Existing clickable report surfaces keep their compact text presentation, but address-bearing rows gain a context menu for direct Disassembly and CFG navigation. This avoids adding permanent button chrome to thousands of evidence rows.

## Bounds and continuity

- structured search: maximum 500 results (hard internal ceiling 1000);
- triage queue: maximum 180 results (hard internal ceiling 300);
- related evidence: maximum 120 results (hard internal ceiling 300);
- admitted evidence line: maximum 16 KiB;
- analysis protocol: unchanged, ABI 1;
- `AnalysisReport`: unchanged at 66 exact fields;
- analysis engine identity: unchanged at `arm64-analysis-v11-schema66`;
- project-cache envelope: unchanged at schema 2.

Because the analysis engine/report identity is unchanged, valid 0.4.2 schema-2 analysis caches remain reusable in 0.4.3.
