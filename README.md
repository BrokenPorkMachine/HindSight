# HindSight Research Tool

The app bundle and executable are **HindSight.app** and **HindSight**. Open `HindSight.xcodeproj`; the shared scheme and app target are `HindSight`. Existing project data, plugins, and protocol identities remain compatible. To recreate a clean source copy without local build caches, run `python3 scripts/package-source.py` after committing your changes. See [project recreation](docs/PROJECT_RECREATION.md).

This source release extends the bounded pseudocode preview to distinguish ARM64 and Intel x86-64 instruction-effect categories without claiming source-level recovery.

## Build and launch

On macOS, build HindSight from the project root with:

```sh
./build.sh
```

To build and launch the app in one step:

```sh
./run.sh
```

The scripts build the `Debug` configuration by default. Set `CONFIGURATION=Release` to build or launch the release configuration instead.

## Firmware and multi-architecture imports

Import now supports thin/universal Mach-O 32/64 for Intel and AArch64, plus ARM32/Thumb-2 for SEPOS composites. The import dialog exposes format, instruction mode, raw load address and embedded image offset. Plaintext SecureROM/iBoot images use explicit raw mappings. The worker extracts headers/sections, symbol and export names, function starts, strings, relocations, indirect-symbol/pointer tables, direct control-flow references and prologue/epilogue candidates. LLVM provides the instruction display; the established ARM64 semantic analyzer remains available for ordinary thin ARM64 images.

See [format coverage, importing SEPI and remaining analysis gaps](docs/BINARY_FORMATS_AND_ANALYSIS.md).

## Current baseline (0.5.11)

## 0.5.11 — Evidence graph navigation and cache control

The current release aligns Evidence Graph and Flow Graph navigation: minimaps show the same viewport rectangle, support click-to-jump and drag navigation, and preserve graph geometry while panning. Function and symbol names are used consistently in function-oriented evidence and runtime surfaces. Analysis results can be cleared from the analysis toolbar with bounded project-local cache removal.

## 0.5.8 — Scheduler, Demangling, Structural Pseudocode

Adds bounded priority-aware dispatch lanes for scripting/firmware, a Rust scheduling policy with memory-lease/generation gating, a Swift ABI demangler CLI, and a conservative pseudocode preview in the IR workspace. The worker remains on its existing admission governor; see `docs/SCHEDULING_DEMANGLING_PSEUDOCODE_0.5.8.md` for integration boundaries.

## Completion-audit hotfix

The 0.5.6 completion-audit package closes the actionable placeholders found after Unified Symbols: real bounded HFS+ file/fork traversal replaces the previous `NotFound` path; eMMC data commands use persistent card backing instead of fake completion; invalid ARM64 logical immediates are never fabricated; DART timing is consumed by the translation path; and stale native-test/VectorEmu release identities are corrected. Production-source TODO/FIXME/unimplemented marker scans are clean. See `docs/AUDIT_0.5.6_COMPLETION.md` for the complete audit and explicit supported boundaries.

## 0.5.6 — Unified Symbols

HindSight 0.5.6 advances the analysis engine to `arm64-analysis-v15-schema71` and the compatibility report to exactly **71 fields**. The new **Symbol Resolver** joins direct `nlist` names, validated function starts, `LC_DYSYMTAB` stub/pointer evidence, and dyld import/export names into one bounded, provenance-preserving symbol view.

Resolution is deliberately conservative: exact code symbols outrank exact indirect-symbol names; unnamed function starts remain explicitly unnamed; imports/exports without a proven address stay name-only. HindSight still does not invent dSYM/DWARF source lines or depend on Apple private Symbolication frameworks at runtime.

0.5.6 retains the 0.5.4 anti-hang work: bounded structural/value-flow passes, indexed CFG/relocation paths, off-main report decoding/fingerprinting, and staged progress reporting.

See `docs/UNIFIED_SYMBOLS_0.5.6.md` for resolver semantics and remaining symbolication gaps.

## 0.5.1 — Typed Worker Evidence

HindSight 0.5.1 moves typed evidence across the worker boundary without breaking the exact 66-field compatibility report. The worker emits bounded typed navigation records and stage fingerprints; Coordinator validates/caches them in envelope schema 3; Swift prefers native records and supplements only unmigrated channels through the 0.5.0 adapter. Existing schema-2 caches remain readable. See [`docs/TYPED_WORKER_EVIDENCE_0.5.1.md`](docs/TYPED_WORKER_EVIDENCE_0.5.1.md).

## 0.5.0 — Evidence Core

HindSight 0.5.0 begins the analysis-core migration: the proven ABI-1 / 66-field worker report remains the wire and cache compatibility format, while the app converts it once into a bounded typed evidence index off the SwiftUI render path. Search, Triage, Related Evidence and current collection-health checks use stable semantic evidence identities plus address/symbol indices rather than repeatedly reparsing report text. Existing projects and valid analysis caches remain compatible. See [`docs/EVIDENCE_CORE_0.5.0.md`](docs/EVIDENCE_CORE_0.5.0.md).

# HindSight 0.4.9

## 0.5.0 — Evidence Core

HindSight 0.5.0 begins the analysis-core migration: the proven ABI-1 / 66-field worker report remains the wire and cache compatibility format, while the app converts it once into a bounded typed evidence index off the SwiftUI render path. Search, Triage, Related Evidence and current collection-health checks use stable semantic evidence identities plus address/symbol indices rather than repeatedly reparsing report text. This preserves existing projects/caches while materially reducing main-thread analysis-navigation work. See [`docs/EVIDENCE_CORE_0.5.0.md`](docs/EVIDENCE_CORE_0.5.0.md).


> **See the machine, not just the file.**

HindSight Research Tool is a native macOS reverse-engineering research studio built to connect the parts of analysis that are normally separated: static structure, live process state, deterministic emulation, virtualized experiments, automation, and the evidence that links every conclusion back to its source.

HindSight began as a product split from two research lines: REWorkbench contributes the mature analyst shell and static/dynamic workflows; iPhemeral contributes a deterministic ARM64 machine laboratory. HindSight turns those capabilities into a general-purpose platform rather than an iPhone emulator or a single-file disassembler.




## 0.4.9 Hotfix 5 — Universal macOS / Intel support

HindSight now builds as a universal macOS application with **arm64 + x86_64** slices. Static ARM64/ARM64e analysis remains available on Intel because target decoding is cross-host; projects, evidence/review workflows, scripting/plugins, dyld/IPSW tooling, and the software **VectorEmu** backend remain enabled.

Only implementations that are genuinely tied to the Apple-Silicon AArch64 host are closed out when the running process is x86_64: the current **Hypervisor RE / SoC Virtualization execution backend** and the current **Native ARM64 Debugger**. Those checks exist in both the UI and Coordinator/native layer. Rust bridge/worker and VectorEmu packaging now produce universal artifacts, and macOS type-check/release scripts explicitly cover both architectures. See [`docs/INTEL_MAC_COMPATIBILITY.md`](docs/INTEL_MAC_COMPATIBILITY.md).

## 0.4.9 hotfix hardening and audit closure

The 0.4.9 hotfix line closes three real beta-test failures and hardens the release gate that should have caught them. Hotfix 1 adds the icon-capable `VGTagPill(text:systemImage:)` shared API; Hotfix 2 makes collection-backed SwiftUI `ForEach` identity explicit; Hotfix 3 bounds Function Neighborhoods UI/worker detail so large firmware call graphs do not wedge the workbench.

The audit-closure pass also makes macOS Swift type-check coverage derive directly from `Config/project-topology.json`, preventing new app/service files such as `AnalysisNavigation.swift` or `AnalysisContinuity.swift` from being omitted by stale shell lists. Project `objects`, `staging`, and `analysis-cache` directory components now reject symbolic-link substitution, while staged capture/cache publication require regular non-symlink files. Product/protocol/cache identity remains 0.4.9 / ABI 1 / `arm64-analysis-v11-schema66`. See [`docs/AUDIT_0.4.9-HF4.md`](docs/AUDIT_0.4.9-HF4.md).

## 0.4.9 review actions & follow-up queue

HindSight 0.4.9 turns the review workflow into an executable next-action loop. Every investigation collection can now carry a bounded **Follow-Up Queue** with explicit Open/Completed state, a concise title/context note, and up to 16 exact evidence identities captured from the collection at creation time. Follow-ups are analyst-authored workflow tasks, not findings or severity/importance judgments.

The Review surface keeps follow-ups separate from journal entries, shows open/completed counts, supports completion/reopen/removal, and includes unresolved actions in the generated Investigation Handoff. Continuity advances compatibly to schema 5 while continuing to read schemas 1–4 inside the existing 1 MiB project-local budget.

0.4.9 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, project database schema v1, and `arm64-analysis-v11-schema66` unchanged, so compatible 0.4.2–0.4.8 analysis caches remain reusable. Product/bundle identity is `0.4.9`, build `4109`, release channel `stable`. See [`docs/REVIEW_ACTIONS_0.4.9.md`](docs/REVIEW_ACTIONS_0.4.9.md).


## 0.4.8 review journal & handoff

HindSight 0.4.8 extends collection review into a durable analyst workflow. Every investigation collection can carry a bounded **Review Journal** with explicit workflow state (**Open**, **In Review**, **Follow Up**, or **Reviewed**), an optional note, timestamp, and the exact collection evidence identities present when the entry was recorded. These states are analyst-authored organizational context; they are not evidence-quality, vulnerability, exploitability, reachability, correctness, or semantic scores.

The Review surface now works with a single collection and can generate a bounded Markdown **Investigation Handoff** containing artifact/source identity, workflow state, collection health, exact review drift, preserved evidence, and recent journal entries. Handoff generation is explicit and copies to the clipboard; nothing is transmitted externally. Continuity advances compatibly to schema 4 while continuing to read schemas 1–3 inside the same 1 MiB project-local state budget.

0.4.8 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, project database schema v1, and `arm64-analysis-v11-schema66` unchanged, so compatible 0.4.2–0.4.7 analysis caches remain reusable. Product/bundle identity is `0.4.8`, build `4108`, release channel `stable`. See [`docs/REVIEW_JOURNAL_0.4.8.md`](docs/REVIEW_JOURNAL_0.4.8.md).

## 0.4.7 collection review & evidence drift

HindSight 0.4.7 turns named investigation collections into explicit review checkpoints. A collection can be marked reviewed, preserving its exact pin membership plus the subset present in the bounded current analysis corpus. Later reviews show exact-identity membership drift and current/preserved transitions without interpreting those changes as severity, correctness, exploitability, or semantic importance.

Analysis Triage adds a responsive **Review** surface for pairwise collection comparison: shared evidence, findings unique to collection A, and findings unique to collection B remain navigable evidence cards. Review baselines are stored inside the same 1 MiB project-local continuity document, which advances compatibly to schema 3 while continuing to read schema 1 and 2 state.

0.4.7 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, project database schema v1, and `arm64-analysis-v11-schema66` unchanged, so compatible 0.4.2–0.4.6 analysis caches remain reusable. Product/bundle identity is `0.4.7`, build `4107`, release channel `stable`. See [`docs/COLLECTION_REVIEW_0.4.7.md`](docs/COLLECTION_REVIEW_0.4.7.md).

## 0.4.6 investigation workflow & collection health

HindSight 0.4.6 turns 0.4.5 collections into active investigation sets. Collections can be searched across their names and preserved evidence, renamed without changing evidence identity, and compared against a bounded corpus of the currently loaded validated report. The health indicator distinguishes evidence that is present in that corpus from evidence preserved only in project continuity state; absence is not interpreted as invalidity, severity, or semantic obsolescence. Recent evidence also gains explicit Previous/Next traversal plus menu shortcuts.

0.4.6 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, and `arm64-analysis-v11-schema66` unchanged, so compatible 0.4.2–0.4.5 analysis caches remain reusable. Product/bundle identity is `0.4.6`, build `4106`, release channel `stable`. See [`docs/INVESTIGATION_WORKFLOW_0.4.6.md`](docs/INVESTIGATION_WORKFLOW_0.4.6.md).

## 0.4.5 investigation collections & resume

HindSight 0.4.5 turns durable pins and recent pivots into a more deliberate investigation workflow. Analysis Triage now supports named **Collections** over project-local pinned findings, with up to 32 collections per artifact and 64 findings in each collection. Collection membership is organizational evidence only: it does not imply severity, causality, exploitability, semantic importance, or runtime reachability.

The bounded recent-evidence trail can now be replayed directly with **Resume Latest Evidence** or per-visit Resume controls. Replay restores the recorded source/code/CFG/symbol destination without creating a fake new history entry. The continuity document migrates compatibly from schema 1 to schema 2 while retaining the same 1 MiB symlink-aware, atomically written project-local store.

0.4.5 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, and `arm64-analysis-v11-schema66` unchanged, so valid 0.4.2–0.4.4 analysis caches remain reusable. Product/bundle identity is `0.4.5`, build `4105`, release channel `stable`. See [`docs/INVESTIGATION_COLLECTIONS_0.4.5.md`](docs/INVESTIGATION_COLLECTIONS_0.4.5.md).


## 0.4.4 investigation continuity

HindSight 0.4.4 makes the 0.4.3 triage layer durable. Evidence cards can be **pinned** into project-local continuity state, successful source/code/CFG/symbol pivots feed a bounded **Recent** history, and Analysis Triage gains a **Changes** view that compares the latest complete analysis against the previous project-local snapshot by evidence channel. Change detection is deliberately structural: it reports channel fingerprints and line-count changes, not semantic importance or risk.

The launch-session record advances compatibly to schema 2 and restores the selected artifact, workspace/layout, address, CFG focus, search query, and selected symbol. Existing schema-1 launch records still decode. Project-local continuity state is capped at 1 MiB, rejects symlinked metadata paths/files, atomically publishes JSON, and bounds pins/history to 128/96 entries. No analyst notes are copied into this state.

0.4.4 keeps protocol ABI 1, the exact 66-field `AnalysisReport`, cache envelope schema 2, and `arm64-analysis-v11-schema66` unchanged, so valid 0.4.2/0.4.3 analysis caches remain reusable. Product/bundle identity is `0.4.4`, build `4104`, release channel `stable`. See [`docs/ANALYSIS_CONTINUITY_0.4.4.md`](docs/ANALYSIS_CONTINUITY_0.4.4.md).

## 0.4.3 analysis navigation & triage

HindSight 0.4.3 turns the growing analysis corpus into a navigable investigation surface. **Analysis Triage** builds a bounded source-prioritized focus queue from the existing validated evidence channels, while **Search & Navigate** now returns source-labelled evidence cards instead of flattening every hit into anonymous text. Address-bearing findings can jump directly to Disassembly or CFG; explicit symbol findings can open Selected Symbol; every finding can return to its owning source workspace.

The global evidence inspector also gains **Related Evidence**, correlating the currently selected row across channels by exact virtual-address identity or explicit symbol text. These are navigation correlations only: 0.4.3 does not turn shared identifiers into causal, semantic, vulnerability, or severity claims. Generic clickable evidence rows gain context-menu jumps without adding permanent button chrome.

This release deliberately leaves the ARM64 engine and exact 66-field `AnalysisReport` unchanged, so the cache engine identity remains `arm64-analysis-v11-schema66` and valid 0.4.2 schema-2 analysis caches remain reusable. Product/bundle identity was `0.4.3`, build `4103`, release channel `stable`, protocol ABI 1. See [`docs/ANALYSIS_NAVIGATION_0.4.3.md`](docs/ANALYSIS_NAVIGATION_0.4.3.md).

## 0.4.2 staged analysis execution

HindSight 0.4.2 keeps the stable 0.4.1 evidence model and makes long firmware analysis observable instead of presenting a spinner around a monolithic worker. The Rust worker emits bounded progress records for verification, mapping/metadata, decode, CFG, synthesis, evidence formatting, and finalization. Coordinator turns those records into stage + percentage status, while the workbench shows determinate progress and keeps cancellation available. The percentage is pipeline position, not a time estimate.

Progress never shares the worker's framed stdout result channel. It uses a strict `VG_PROGRESS` stderr record that Coordinator parses and removes from failure diagnostics; ordinary stderr remains bounded diagnostic evidence. Final `AnalysisReport` publication remains atomic, so an incomplete stage is never presented as validated static evidence.

The project-local analysis cache moves to **envelope schema 2**, separating `engineVersion` from `producerVersion`. Because 0.4.2 does not change ARM64 analysis v11 or the 66-field report schema, a structurally valid 0.4.1 cache can be reused once and migrated in place. Invalid/unreadable cache state is a cache miss, not an analysis failure. See [`docs/ANALYSIS_EXECUTION_0.4.2.md`](docs/ANALYSIS_EXECUTION_0.4.2.md).

At the 0.4.2 milestone the source tree advertised release channel `stable`, product/bundle version `0.4.2`, and build `4102`. **Public distribution still requires the macOS release-host matrix in [`docs/RELEASE_READINESS.md`](docs/RELEASE_READINESS.md)**; this source package does not claim signing/notarization, the 8 GiB pressure run, or real-target execution in a non-macOS environment.

## 0.4.2 UI density hardening

0.4.2 also completes a second responsive-workbench pass. The beta.11 shell could correctly collapse panes while individual child surfaces still assumed desktop-width rows; Targets exposed this most visibly because its address-space and pivot tables reserved several fixed columns inside a pane allowed to shrink to a few hundred points. Targets now uses adaptive cards, wrapping capability tags, stacked key/value evidence, and horizontal-or-stacked input controls. No target field is intentionally hidden to make the surface fit.

The same audit replaces dense one-line action bands throughout debugger, plugin/extension, virtualization, VectorEmu, firmware/IPSW, Hypervisor RE, and high-frequency analysis workspaces with wrapping `VGFlowLayout`, `ViewThatFits`, adaptive grids, or independent evidence overflow bands. Fixed evidence columns remain only where the owning data view scrolls horizontally or where the width is intrinsic to a graph/table representation. See [`docs/UI_POLISH_0.4.2.md`](docs/UI_POLISH_0.4.2.md).

## 0.4.1 release consolidation

HindSight 0.4.1 consolidates the beta.8–beta.14 hardening line into a stable product identity while preserving the same protocol ABI. The workbench keeps the responsive wide/medium/compact topology, Coordinator/session recovery, bounded firmware-scale analysis, evidence-quality reporting, Function Intelligence, Data References, Function Neighborhoods, and explicit candidate-vs-proven evidence labeling.

0.4.1 also adds a **project-local analysis cache**. Successful analysis reports are stored outside the SQLite project database in a bounded `analysis-cache/` directory, keyed by the captured source SHA-256 and bound to the exact HindSight product version. Cache files are atomically published, rejected if they are symlinks/non-regular/oversized, decoded through the same `AnalysisReport` limits, and rebound to the new request ID only after source identity validation. A cache miss or cache failure always falls back to the ordinary isolated worker path.

At the 0.4.1 milestone, the source tree advertised release channel `stable`, product/bundle version `0.4.1`, and build `4101`. **Public distribution still requires the macOS release-host matrix in [`docs/RELEASE_READINESS.md`](docs/RELEASE_READINESS.md)**; this source package does not pretend that signing/notarization, the 8 GiB pressure run, or real-target execution were performed in a non-macOS build environment.

## Analysis evidence quality in 0.4.0-beta.14

0.4.0-beta.14 makes large-binary analysis easier to trust and navigate. **Analysis Quality** reports executable scan coverage, decoder coverage, data-in-code exclusions, dominator budget omissions, and evidence classes so a missing row is not confused with negative evidence. **Function Neighborhoods** joins direct BL callers/callees with separately labelled cross-function `B` transfers. **Tail-Call Candidates** includes only unconditional direct branches that cross a discovered function boundary and land exactly on another admitted function entry; the UI keeps them candidates rather than silently promoting them to calls. Direct BL collection now preserves targets outside the bounded scan as external outgoing evidence, while Function Intelligence only assigns incoming counts to targets that actually belong to an admitted internal function.

## Analysis scalability hardening in 0.4.0-beta.13

0.4.0-beta.13 fixes a real large-CFG failure mode exposed during firmware analysis. The old dominator solver materialized an all-block dominator set for every basic block, making memory usage quadratic in the complete CFG. On branch-heavy firmware that could exhaust an 8 GB development host before analysis returned. The solver is now **per-function and budgeted**: ordinary functions retain full dominator/IDOM/frontier detail, while a pathological function above the explicit block budget reports that structural detail was withheld instead of consuming unbounded memory. CFG edges and the rest of analysis remain available.

Function Intelligence also replaces repeated linear symbol-table searches with an indexed lookup, reducing large-symbol-table analysis cost. Worker terminations by Unix signal are now reported explicitly as isolated worker failures so a SIGKILL/SIGABRT is distinguishable from an application crash.

## Analysis synthesis in 0.4.0-beta.12

0.4.0-beta.12 builds on the responsive workbench with an evidence-synthesis layer rather than another raw-report pass. **Analysis Insights** summarizes proven structural evidence; **Function Intelligence** ranks discovered functions using a fully disclosed connectivity/size formula; and **Data References** resolves architecturally explicit ARM64 ADR, adjacent ADRP+ADD, and literal-immediate references only when the target lands inside a parsed Mach-O section. String text is attached only for validated string-bearing sections. The ranking is a navigation heuristic, never a semantic-importance, security-risk, or vulnerability score.

The release also carries the beta.11 compile hotfix for the VectorEmu run-step `Stepper`: `UInt64` remains the bound value/range type while the increment is correctly inferred as `UInt64.Stride` (`Int`).

## Responsive workbench re-architecture in 0.4.0-beta.11

0.4.0-beta.11 redesigns the workbench composition model so dense surfaces no longer depend on mutually incompatible fixed pane widths. The flagship analysis desks now use adaptive topology: wide windows keep navigator / canvas / inspector side-by-side, medium widths protect the primary canvas and multiplex auxiliary panes, and compact widths move auxiliary panes below the canvas. The global inspector follows the same rule.

The application minimum window is now 900 × 680 instead of 1180 × 760, the default opens at 1480 × 920, dense headers are split into semantic rows, and status/metric bands no longer dictate the width of the canvas. Binary Workspace, CFG, Trace Explorer, Runtime Correlation, Evidence Graph, Investigation Ledger, Binary Diff, Debugger, Targets, scripting/plugin output, virtualization, VectorEmu, Selected Symbol, and Stop Context are all part of this pass. See [`docs/UX_REARCHITECTURE_BETA11.md`](docs/UX_REARCHITECTURE_BETA11.md).

## Launch/session continuity in 0.4.0-beta.10

0.4.0-beta.10 extends beta.9 recovery across a **full HindSight process relaunch**. After the Coordinator handshake succeeds, the app can automatically reopen the last project and restore the selected artifact, workspace, and layout preset. Saved launch metadata is bounded and validated before use; a missing/stale project path falls back to the ordinary project picker instead of leaving the workbench wedged in a restore state.

The continuity record deliberately does **not** duplicate unsaved note contents, debugger state, runtime memory, or analysis reports into preferences. Unsaved drafts still survive a CoordinatorService restart while the app stays alive, but terminating the app discards unsaved note text. The Support menu now exposes **Reopen Last Project** and **Forget Saved Project**, and diagnostics disclose only the saved project basename. See [`docs/SESSION_CONTINUITY_BETA10.md`](docs/SESSION_CONTINUITY_BETA10.md).

## Coordinator recovery hardening in 0.4.0-beta.9

0.4.0-beta.9 closes a recovery gap left after the beta.8 workbench polish. XPC reachability and project-session readiness are now distinct states: if CoordinatorService is interrupted or relaunched, HindSight validates the service capability handshake, reconnects, and automatically reopens the currently displayed project before project-bound actions are enabled again. A mismatched app/service version, ABI mismatch, incomplete analysis capability set, or failed project restore is surfaced explicitly instead of being treated as a healthy session.

Unsaved note drafts and the current artifact selection stay client-side across successful project rehydration. Reconnect now creates a fresh XPC generation instead of reusing a possibly stale connection, the status bar exposes **Connecting**, **Restoring project**, **Coordinator online**, and **Project session unavailable** states, and a Support menu provides explicit reconnect/restore actions plus privacy-bounded diagnostics that omit full project paths. See [`docs/COORDINATOR_RECOVERY_BETA9.md`](docs/COORDINATOR_RECOVERY_BETA9.md).

At the beta.9 milestone, the feature scope was intentionally frozen while recovery behavior matured. The 0.4.2 line carries that recovery contract forward; current release-host qualification is documented in [`docs/RELEASE_READINESS.md`](docs/RELEASE_READINESS.md).

## End-to-end workbench polish in 0.4.0-beta.8

0.4.0-beta.8 made first-presentation Coordinator state truthful, added explicit reconnect UI, aligned Import Binary enablement with its actual prerequisites, split first-run and empty-project onboarding, added no-result search feedback, disclosed large-project sidebar overflow, and made clipped status text inspectable.

## Analysis admission correctness in 0.4.0-beta.7

0.4.0-beta.7 fixes the Mach-O admission defect uncovered by real beta testing. The native mapper previously confused `LC_FUNCTION_STARTS` (`0x26`) with `LC_DATA_IN_CODE` (`0x29`), causing ordinary modern ARM64 binaries to be rejected before the Rust analysis worker could run. Function-start and data-in-code metadata now remain admissible; declared data-in-code bytes are treated as decode/SSA barriers instead of instructions. A real nonzero Mach-O encryption ID no longer vetoes the entire artifact: HindSight performs bounded metadata/linkage analysis while explicitly withholding static instruction pages and code semantics rather than decoding ciphertext. The beta.6 observable job lifecycle remains intact.

The repeated `com.apple.linkd.autoShortcut` diagnostics seen under some Xcode/macOS builds are system App Intents/Shortcuts service messages and are not used as an analysis-success signal. The invalid `doc.binary` SF Symbol reference has also been removed.

## Analysis lifecycle repair in 0.4.0-beta.5

0.4.0-beta.5 repairs the analysis lifecycle uncovered by real beta testing. CoordinatorService now validates the complete modern analysis schema, uses a dedicated bounded bulk-analysis frame instead of the 256 KiB control frame, accepts the corresponding bounded XPC reply, and supervises worker stdout/stderr/process termination asynchronously to avoid QoS priority inversion. The beta feature freeze remains intact; this is correctness and reliability work, not feature expansion. See [`docs/BETA_READINESS.md`](docs/BETA_READINESS.md).

## Binary Diff in 0.3.8

The 0.3.8 workbench added **Binary Diff**, a build-to-build comparison desk for two captured project artifacts. Function identity is conservative: HindSight accepts unique symbol matches and exact normalized instruction fingerprints, then leaves weak cases as added/removed rather than inventing fuzzy matches. Accepted pairs are compared independently across code, CFG shape, direct references/calls, and bounded lifted semantics. Absolute address churn is normalized so a function moving in the image does not automatically become a code change. See [`docs/BINARY_DIFF.md`](docs/BINARY_DIFF.md).


## Investigation Ledger in 0.3.7

The 0.3.7 workbench adds a persistent **Investigation Ledger**. Projects now carry a bounded case timeline, first-class hypotheses, bookmarks, and restorable analysis-context snapshots. Machine observations, static derivations, evidence-backed correlations, and analyst assertions retain distinct provenance. A snapshot restores the workbench context but never claims to rewind a live target. The new `.vgcase` single-file handoff format packages artifact identities/hashes, ledger state, selected static evidence, debugger observations, and explicit memory reads without silently embedding the original binary. Markdown, PDF, and JSON exports make findings usable outside HindSight. See [`docs/INVESTIGATION_LEDGER.md`](docs/INVESTIGATION_LEDGER.md).


## Evidence Graph in 0.3.6

The 0.3.6 workbench adds **Evidence Graph**, a provenance-first investigation surface that makes relationships between static analysis, debugger observations, runtime↔static correlations, explicit memory reads, and analyst notes visible without pretending they are the same kind of fact. Every graph node and edge carries explicit confidence language: **observed**, **derived**, **correlated**, **asserted**, or **unresolved**. Search/provenance filters keep the graph bounded, the relationship inspector explains why a connection exists, and address-bearing nodes pivot directly into Binary Workspace, CFG, Runtime Correlation, Trace Explorer, or Notes.

## Trace Explorer in 0.3.5

The 0.3.5 workbench adds **Trace Explorer**, a temporal desk over the bounded debugger evidence already captured by Runtime Correlation. It orders observed debugger stops/snapshots, correlates endpoints to static code only through the same unique loaded-module/slide evidence, classifies transitions only when the previous static instruction supports that classification, and keeps unknown transfers explicitly unresolved. Register-change bands, nearby explicit memory reads, observed function frequency, and static code context stay synchronized to the selected event. HindSight labels this as an **observed timeline**, not a complete instruction trace.

## Runtime Correlation in 0.3.4

The 0.3.4 workbench adds a runtime-correlation desk that keeps observed debugger state separate from static evidence while making the relationship between them immediately usable. Debugger stops are captured as a bounded history with register deltas and breakpoint state; explicit memory reads become session observations; and runtime PCs are translated into static preferred addresses only when the analyzed binary uniquely matches a loaded dyld image. The workspace overlays observed stop frequency on static disassembly, exposes the exact slide/module evidence used for translation, and leaves ambiguous mappings unresolved rather than guessing.

## Binary Workspace in 0.3.3

The 0.3.3 workbench adds the day-to-day code-reading desk that sits between overview and specialist analysis: a synchronized function browser, ARM64 disassembly, conservative lifted statements / raw IR, direct xrefs and calls, and instruction-word data inspection. Selection flows through the same shared address/evidence context as CFG and Targets, while an available debugger PC can drive the workspace through **Follow PC**. The Lifted view is intentionally evidence-bounded: opaque instructions stay opaque and modeled control-flow statements are labeled rather than presented as recovered source.

## Graph Workspace in 0.3.2

The 0.3.2 workbench turns the CFG surface into a real analyst canvas: typed basic blocks and edges, deterministic hierarchical layout, zoom/pan, a minimap, function navigation, debugger-PC highlighting, and a synchronized Code / IR / Data Flow pane. Selection feeds the same evidence/address context used by disassembly and targets. The semantic pane labels IR and modeled data flow explicitly; HindSight does not present them as decompiled C or as facts beyond the underlying evidence.

## What is already here

- Native SwiftUI workbench with a curated task sidebar, global ⌘K command palette, contextual evidence inspector, persistent status surface, and deep Apple Silicon/Mach-O analysis.
- Rust worker for bounded, out-of-process parsing and analysis.
- Native Mach/ptrace debugging with registers, threads, memory maps, modules, backtraces, breakpoints, live disassembly, and stop context.
- **VectorEmu**, a built-in deterministic ARM64 execution laboratory extracted from the reusable iPhemeral CPU/MMU/device/SoC core.
- Hypervisor.framework research suite for isolated Apple Silicon guest experiments.
- Evidence-aware views for provenance, linkage, chained fixups, authenticated pointers, runtime metadata, and recovered semantics.
- **Binary Diff** for conservative function matching and code/CFG/xref/lifted-semantic comparison across two captured builds.
- **Evidence Graph** for provenance-aware relationships across static derivations, debugger observations, runtime/static correlations, explicit memory reads, and analyst assertions.
- **Investigation Ledger** for persistent timeline provenance, hypotheses, bookmarks, replayable analysis context, session snapshots, and `.vgcase` / Markdown / PDF / JSON handoff.
- Out-of-process Python automation API v2 (`from hindsight import api as vg`).
- Capability-gated `.vgplugin` extensions with signing/trust, panels, commands, settings/state, analysis passes, debugger filters, and annotations.
- **Mentor Mode**: contextual explanations that make advanced tooling usable without hiding the underlying machinery.
- **Unified Target Model**: one capability-driven identity for static artifacts, native processes, VectorEmu machines, and Hypervisor guests.
- **Evidence-backed Address Pivot**: translate among file, preferred VM, runtime, emulated, and guest address spaces only when the mapping is provable.
- Exportable target-session JSON for reproducible handoff and future evidence bundles.
- Resource governors, size limits, timeouts, local-only debugger endpoints, and process isolation intended to remain useful on constrained developer Macs.

## Workbench UX in 0.3

The 0.3 shell is organized around analyst jobs rather than internal feature count. Common workflows stay visible in the sidebar; specialist SSA, linker, Apple-runtime, ARM64e, virtualization, and firmware tools remain globally searchable through `⌘K`. The main canvas is paired with an optional evidence inspector and a persistent target/service status surface. See [`docs/UX_SYSTEM.md`](docs/UX_SYSTEM.md).

## The core idea

A reverse-engineering claim should be inspectable as a chain of evidence:

**bytes → decoded structure → inferred semantics → modeled hardware → observed runtime behavior**

HindSight keeps those layers distinguishable. A guessed function purpose should not look as authoritative as a decoded Mach-O load command; emulated behavior should not be silently presented as physical-hardware observation. The UI is being built around that principle.

## VectorEmu

VectorEmu is intentionally narrower than iPhemeral. It includes reusable ARM64 CPU, MMU, memory, SoC/device, tracing, and debugger machinery, but the commercial HindSight core does **not** bundle third-party firmware, device secrets, signing material, restore/jailbreak flows, SSH payloads, or key-acquisition machinery.

The engine starts paused and binds its debugger only to loopback. The HindSight coordinator exposes a read/control-oriented subset of the engine protocol for registers, memory decoding, regions, breakpoints, stepping, trace capture, and reset. Full iPhemeral can remain an optional external research backend for authorized labs that need its specialized Apple-device environment.

See [`docs/VECTOREMU_PROTOCOL.md`](docs/VECTOREMU_PROTOCOL.md) and [`Engines/VectorEmu/ORIGIN.md`](Engines/VectorEmu/ORIGIN.md).

## Build on macOS

Requirements: Apple Silicon Mac, macOS 14+, Xcode command-line tools/Xcode, Rust/Cargo **1.98.1** for release-host qualification, Python 3.11+. Shipped crates retain a declared Rust **1.90** MSRV.

```bash
./scripts/doctor.sh
./scripts/test-macos.sh
# Candidate package gate (Debug + Release + signing + embedded worker):
./scripts/test-beta-macos.sh
# Distribution gate: requires Developer ID + notarization environment variables.
./scripts/test-release-macos.sh
open HindSight.xcodeproj
```

The CoordinatorService build also builds and stages `vg-emu-server`. Set `VECTORGLASS_EMU_SERVER=/absolute/path/to/vg-emu-server` to test another engine build. Existing REWorkbench Cargo/library names are retained internally during the 0.x migration to keep ABI/build churn low-risk; they are implementation details, not the public product API.


### Lockfile discipline

Release builds intentionally use Cargo `--locked`. Do not remove it to work around manifest drift. If a Cargo manifest is intentionally changed, regenerate and verify both workspace locks:

```bash
./scripts/regenerate-lockfiles.sh
./scripts/check-lockfiles.py
```

The 0.2 release adds this preflight directly to the Xcode Rust build phases so stale packaged lockfiles fail with an actionable diagnostic.

## Product direction

HindSight is designed around five recurring analyst jobs:

1. **Orient** — identify format, architecture, mappings, symbols, runtime metadata, and suspicious/interesting regions.
2. **Explain** — correlate disassembly, CFG/SSA/data flow, imports/exports, runtime types, and provenance.
3. **Execute** — attach natively, emulate deterministically, or run an isolated hypervisor target.
4. **Prove** — preserve the observation, address, bytes, trace, state, and confidence behind a conclusion.
5. **Automate** — turn a successful workflow into a bounded script, analysis pass, or extension.

The long-term goal is not “more panes.” It is fewer context switches between hypothesis and proof.

## Ethical and authorized research

HindSight is intended for software, firmware, devices, and environments you own or are authorized to examine; malware analysis; interoperability; education; vulnerability research; debugging; and defensive engineering. It is not designed to hide activity, steal credentials, persist on third-party systems, or automate unauthorized access.

## Status

`0.4.4` is the current stable-line source identity. Stable workbench paths have explicit failure/recovery behavior and the source validator rejects production crash primitives. Experimental machine profiles remain labelled Experimental. Distribution qualification requires the Apple Silicon real-target, memory-pressure, Release signing, and notarization matrix in [`docs/RELEASE_READINESS.md`](docs/RELEASE_READINESS.md).

Read next: [`docs/PRODUCT_VISION.md`](docs/PRODUCT_VISION.md), [`docs/TARGET_MODEL.md`](docs/TARGET_MODEL.md), [`docs/ARCHITECTURE_HINDSIGHT.md`](docs/ARCHITECTURE_HINDSIGHT.md), [`docs/ROADMAP.md`](docs/ROADMAP.md), and [`docs/COMMERCIAL_MODEL.md`](docs/COMMERCIAL_MODEL.md).

### Crash address resolution (0.5.6)

The standalone `scripts/crash-resolve.py` reconciles text-format `.crash` frame addresses with the existing unified-symbol report. It requires a matching Mach-O UUID and explicit static image base; it does not decode `.ips` JSON or DWARF source lines. See `docs/CRASH_RESOLUTION_0.5.6.md`.
