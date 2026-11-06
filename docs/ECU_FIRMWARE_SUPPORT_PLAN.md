# ECU Firmware Support & Task-Tailored Layout — Plan

Execution priority is governed by the [master roadmap](MASTER_ROADMAP.md); this document
is the detailed ECU design/evidence reference.

Status: **IMPLEMENTED THROUGH STATIC ECU MODE.** Phases 1–4 are operational, including
structured checksum staging/export and schema-3 per-profile workspace persistence.
Phase 6 has bounded XDF import, paired-reference multi-anchor relocation, explicit
verified/candidate/ambiguous/not-found evidence, and JSON export. Phase 5 now has a
production worker entry point, app workspace, and deterministic bounded core for register immediates,
register moves, add/add-with-carry/subtract/subtract-with-borrow/compare flags,
all 16 condition-code predicates, conditional and unconditional absolute/relative
calls and jumps, returns, DPP-mapped direct/indirect/indexed byte and word memory,
a separate 64 KiB stack window, trace caps, fail-closed unsupported instructions,
and deterministic run/step/continue/reset controls with stop-before-execute breakpoints
and DPP-translated physical read/write watchpoints. Explicit, bounded physical RAM seed
spans overlay a private sparse RAM image, leaving captured flash immutable and exposing
seed/write counts plus a bounded preview. CP, SP, MDH, and MDL immediate-SFR
state is visible and CP switches among 64 preallocated register banks. Word XOR/AND/OR
register, short-immediate, and full-immediate forms execute with documented PSW flag
semantics. Core SFRs are aliased through page-3 long-memory accesses, including DPP,
CP, SP, MDH:MDL, and PSW flag bits. Byte register ALU, direct-memory word ALU,
explicit PUSH/POP, and byte immediates cover the corresponding real-image forms.
ROL/ROR/SHL/SHR/ASHR register-count and immediate-count forms execute with
the documented carry and right-shift rounding flags. Signed/unsigned MUL writes full MDH:MDL with
word-fit overflow flags; DIV/DIVU/DIVL/DIVLU use their documented MD dividend widths
and stop before mutation when a zero divisor or extended quotient overflow makes the
hardware result invalid. Additional ALU/data forms and image checksum-routine
qualification remain.
Scope: HindSight gains (A) a real ECU-firmware backend (Infineon C167 core, Bosch ME7-class
containers) and (B) a task-tailored UI in which the loaded artifact decides the toolset: a
Mach-O gets the Apple/Mach-O surfaces it gets today; an ECU image gets an ECU toolset
(disassembly, container regions, checksums, staged patching, and an ECU emulator) while the
Apple-specific surfaces are hidden for it.

Driving example (verified against real files): Volvo Bosch ME7 512 KB / 1 MB flash dumps,
C167 core, `assets/bins` corpus of 115 files. The design is deliberately generic — "ECU
firmware, C166-family core" — not Volvo-only.

---

## 0. Verified groundwork already done (evidence ledger)

Everything below was established by byte-level analysis of the Volvo-ME7-BINs corpus
(115 BINs) with ordinary shell/Python tools; no HindSight code was changed. These facts
de-risk Phases 1–2 substantially.

| # | Finding | Evidence |
|---|---------|----------|
| F1 | ME7 container: file starts with a C167 trap/interrupt **vector table** — consecutive 4-byte `JMPA cc_UC` instructions (`EA 00 cLo cHi`); the "magic" bytes `EA 00 00 02 EA 00` are literally vectors 0 and 1. Tail padding on 1 MB dumps ends `44 44 44 44 83 83`. | hexdumps of all 115 BINs; 108/115 fully consistent |
| F2 | Sizes cluster: 103×1,048,576 (1 MB flash), 6×524,288 (512 KB logical dumps ending in `0x44` padding, no tail block), 1×589,840 (non-standard, two-part). 512 KB dumps fail the naive tail check but are otherwise valid — the tail check must be a *corroborating* signal, not a gate. | size survey + tail dumps |
| F3 | **Checksum descriptor table at file offset 0x1F810** for both 1 MB (ME7.0/7.0.1/7.5) and 512 KB (ME7.0) files: up to 31×16-byte entries `[startAddr:u32le][endAddrInclusive:u32le][sum:u32le][~sum:u32le]`; `sum` = little-endian 16-bit word sum over the inclusive range `[start,end]` (stock entries use an odd end, so the last pair is `end-1,end`). Verified against the current corpus by `scripts/qualify-c167-corpus.py`; invalid community edits are reported rather than normalized. | full-corpus verifier; matches the existing Volvo-ME7-BINs JS tool (`assets/js/checksum.js`) |
| F4 | Tuned-vs-stock diffs concentrate in a calibration band ≈ `0x012600–0x01FC00` (1 MB files), plus ID bytes at `0x006005` and EPK/version strings near `0x02B5xx`. Descriptor ranges themselves give an honest, per-file segmentation source. | pairwise diff clusters (stock↔tuned) |
| F5 | Firmware headers are parseable: EPK strings, `"<ver>Vatlab VOLVO ID.<...>"` header line, variant/engine/transmission/region tokens, ECU variant marker `99 99 99 00 0A` + 7 bytes. The Volvo-ME7-BINs repo ships a working parser (`scripts/bin_info.py`) whose regexes are validated against its metadata for all 115 files. | `bin_info.py` + `_data/bins_meta_*.yml` |
| F6 | Map definitions (TunerPro XDF) are **per-revision and unreliable across revisions**: in `50WRHJ.xdf`, KFZW1's axis address 0x14E6E contains garbage in all five 50WRHJ revisions while true monotonic RPM axes sit at 0x14FD8/0x14FFA/0x15D0A — no uniform shift. Conclusion: map-location support must be *anchor-verified* per file, never trusted blindly. | axis scans + 7 XDF variants compared |
| F7 | C166/C167 ISA facts: instructions are 2 or 4 bytes; control flow = `JMPS/JMPA/JMPI/JMPR/JB/JNB/JBC/JNBS/CALLA/CALLS/CALLR/CALLI/PCALL`, returns `RET/RETS/RETP/RETI`; IP-relative displacement wraps within its 64 KB segment; 20 first-byte opcodes are undefined. A byte-verified decoder table exists (Apache-2.0, `therudywolf/MS4X tools/c166.py`, validated against the Infineon C166 Instruction Set Manual V2.0 and round-tripped on stock images) — a clean porting reference with attribution. | MS4X c166.py + Infineon manual opcode map |
| F8 | HindSight has zero C167/C16x/Infineon coverage today (its engines are ARM64/ARM32/Thumb/x86 + LLVM display; LLVM has **no** C16x target, and the worker's LLVM bridge caps architecture ids at 5) — so a native decoder is required, not an LLVM adapter. | codebase grep; `native/macho/llvm_decode.c:45` |
| F9 | Relevant HindSight mechanisms already exist to build on: staged multi-edit binary patches (`VGBinaryPatch`, original-byte checks, overlap rejection, atomic save), `VG_EVIDENCE` sideband → typed evidence, `VGWorkspaceCatalog`, host-capability gating pattern, LaunchSession/continuity persistence. | 0.5.15 release notes; code refs in §3–4 |

---

## 1. Design principles (non-negotiable, inherited from the repo)

1. **Facts vs candidates.** Container validity, sizes, vector counts, checksum verify
   results = *facts*. Code/calibration boundaries, EPK identifiers, map locations =
   *candidates*, labelled as such. No CFG edges from table candidates; no invented entry
   points.
2. **Bounded everything.** Every new scan obeys the established budgets (4 MiB scan,
   262,144 instructions, 60,000-byte text views). Descriptor verification must cap total
   summed bytes.
3. **No protocol break.** The 75-field `AnalysisReport` parity gate
   (`scripts/validate-source.py:749-762`, `Tests/analysis_protocol_contract.swift:19`)
   must not be touched: ECU findings ride in existing fields (`metadata`,
   `analysis_insights`, `summary`) and the `VG_EVIDENCE` sideband under existing
   workspace names ("Metadata", "Xrefs", "Functions", "Strings").
4. **Artifact decides the toolset.** A *profile* is derived from the record
   (`HeaderResult.file_kind` / `architecture` / format). Profiles gate which workspaces
   are offered; they never mutate analysis semantics. Host-capability gating
   (Apple Silicon etc.) composes with profile gating (both must pass).
5. **Existing Mach-O flow unchanged.** Profile "macho" (default for today's records)
   must behave byte-for-byte like the current UI. The pinned contract strings in
   `scripts/validate-source.py` (139–142: `hostUnavailableWorkspace("Native ARM64
   Debugger"` etc.) stay intact.
6. **Honest failure.** Importing an ECU file that doesn't validate yields an actionable
   error ("not an ME7-class vector table; use Raw for other ECU firmware"), the same way
   raw imports require an explicit architecture today.

---

## 2. Profiles — the core UI concept

| Profile (derived) | Source predicate | Toolset shown | Apple-specific surfaces |
|---|---|---|---|
| `macho` (default) | `file_kind` Mach-O family, arch ∈ {AArch64, x86, ARM32…} | Everything current | shown |
| `ecu` | format `ecu`, architecture `Infineon C167` | ECU toolset (§4): ECU Overview, Container & Regions, Checksums, Disassembly, Strings, Notes/Evidence, Staged Patches, ECU Emulator (Phase 5), Definitions (Phase 6) | hidden |
| `raw-other` | raw/securerom/iboot with non-C167 arch | current generic set (unchanged behavior) | per host rules (current behavior) |

Derivation is a pure function of `ProjectRecord.result` (+ `import_options.format`),
computed in `WorkbenchModel` once per selected record. Profile is **not** persisted as
user state (it's derived); only the *last workspace per profile* persists (§3.4).

---

## 3. Phase 1 — Worker: C167 architecture + decoder (no UI)

**Goal:** `--page`/`--analyze-image` work for C167 code with real instruction text and
correct direct-transfer extraction.

### 3.1 New module `rust/crates/worker/src/c167.rs` (pure Rust, no LLVM)

- Port the encoding table from the Apache-2.0 reference (F7) **with attribution header**
  (`SPDX-License-Identifier: Apache-2.0`, credit line) — table-driven: first byte →
  form; every decoder returns `Option<(len, text, flow)>`; reserved-field-nonzero and the
  20 undefined opcodes decode as `None` (barriers), matching repo philosophy.
- Text style: `mnemonic operands` using the reference syntax (`r0–r15`, `rl0–rh7`,
  `[0x8100]`, `[r4+#0x0010]`, `#0x0005`, bit `PSW.3`, `jmpr cc_EQ, 0x…`, `calls
  0x0C:0x9CF8`), plus the C167CR SFR/ESFR name table (facts from the C167CR manual, F7)
  so listings are readable.
- Flow classifier for `branch()`:
  | instruction | kind | target |
  |---|---|---|
  | `JMPA cc_UC` / `JMPR cc_UC` | `branch` | `(pc & !0xFFFF) \| caddr16` / segment-wrapped `pc+2+2·rel` |
  | `JMPA/JMPR cc≠UC`, `JB/JNB/JBC/JNBS` | `conditional` | same math |
  | `CALLA`, `CALLS`, `CALLR`, `PCALL` | `call` | caddr / `seg<<16 \| caddr` / wrapped rel |
  | `JMPI`, `CALLI` | none (unresolved indirect) | — |
  | `RET/RETS/RETP/RETI` | return (via `return_instruction`) | — |
  Note: on flat ME7 dumps file offset = physical address, so a far target
  `seg<<16|caddr` maps directly onto the section VA when `load_address=0`.

### 3.2 `binary.rs` wiring

- `Arch::C167 = 6`: `name()` "Infineon C167", `unit()` 2 (paging in words; variable
  2/4-byte instructions advance by decoded length), `width()` 4, `option("c167")`,
  `cpu()` stays unsupported (never a Mach-O CPU), `compatible()` false.
- `Options::parse`: format += `"ecu"` (Phase 2), architecture += `"c167"`.
- `Decoder`: hold `Option<*mut c_void>`; for `Arch::C167`, `new()` succeeds without LLVM
  and `instruction()` routes to `c167::instruction` (undefined → existing
  `.byte …; undecoded` fallback). `Drop` frees only when present. No other call-site
  changes — `page()`/`analyze()` are architecture-generic.
- `branch()` → C167 arm calling `c167::flow`; `return_instruction()` → `rets/ret/retp/reti`
  text check; `prologue()` → `false` (C167 prologues vary; don't invent);
  `epilogue()` → false; `semantic_hint()` → text-prefix hints (`call*`→direct-call,
  `jmpr/jmps/jmpa`→control-flow, conditional jmpr/jb→conditional-control-flow,
  `mov*`→data-movement, `ret*`→return); `data_reference()` → `None` for C167 (DPP-mapped
  16-bit data addresses are *not* VAs; mapping them requires DPP state we don't model —
  document this in `docs/BINARY_FORMATS_AND_ANALYSIS.md`).

### 3.3 Tests (Phase 1 done = all green)

- Rust unit tests (in `c167.rs` + `binary.rs` `checked_branch_addresses` pattern): one
  encoded case per flow class incl. segment-wrap negative displacement, undefined
  opcodes (F7 list), reserved-field rejection, paging-continuity (a 4-byte instruction
  straddling the 512-byte page window).
- `scripts/test-binary-formats.py`: raw-format C167 cases (synthetic bytes): capture →
  page decodes a `JMPA` vector and a `CALLA`; analysis report contains
  `direct-call candidate`; undecoded opcodes terminate flow.
- Gates: `cargo test`, `cargo clippy -D warnings` (worker lint allowlist in `main.rs`
  must not need new entries — write warning-clean), `python3
  scripts/test-binary-formats.py <worker>`.

**Files touched:** `rust/crates/worker/src/c167.rs` (new), `binary.rs`, `main.rs`
(inspect label table: optional "C167 vector table candidate" kind), unit tests,
`scripts/test-binary-formats.py`, `docs/BINARY_FORMATS_AND_ANALYSIS.md` (coverage row),
`Shared/WorkbenchMessages.swift` (architecture whitelist += `c167`),
`App/WorkbenchModel.swift` (arch popup). *Swift format whitelist widens in Phase 2.*

---

## 4. Phase 2 — Worker: `ecu` format + ME7 container/region analysis

**Goal:** import a `.bin` ECU dump and get a validated container, segmented evidence, and
a checksum report — with zero UI changes.

### 4.1 Container validation `ecu_container()` (in `binary.rs::load()` before raw fallback)

- Format `"ecu"`, architecture `auto` (resolves to C167 after validation) or `c167`.
- Facts established: size ∈ {0x80000, 0x100000} (tolerate non-standard sizes with a
  noted caveat, min 0x20000, cap 0x200000), vector table at 0x0000: require ≥6
  consecutive decodable `JMPA` vectors (F1); tail padding recorded as corroborating
  note only (F2 — never a gate).
- Synthesized `Image`: format `"ME7"`-class label, `bits` 16, one executable section
  `ecu-code` spanning the mapped range (VA = `load_address + offset`, mirroring raw),
  `raw: true` (keeps strings scanning active), inventory text stating what is *fact*
  (vector count, size class, tail bytes) vs unknown (code/data split, entry point beyond
  vector 0).
- Validation failure → actionable error suggesting the `raw` format for non-ME7 ECU
  firmware.

### 4.2 `ecu_regions()` bounded pass (runs inside `analyze()`; results → existing report fields)

- **Vector inventory**: decode 0x0000–0x01FF (the C167 trap/interrupt area), list
  `trap/vector N → target` lines (fact-level: these are decoded transfers).
- **Checksum descriptor table** (F3): parse ≤31 entries at fixed offset `0x1F810`
  (constant, documented as ME7-class layout; absent table → one honest line, no scan
  guessing). For each entry: range, stored sum, computed word-sum, `verified|INVALID`,
  plus overall verdict. Cap total summed bytes at `SCAN`. INVALID entries carry the
  computed value — this is the data a fixer (Phase 4) stages.
- **Region candidates**: union of descriptor inclusive `[start,end]` ranges → "checksummed
  region" lines; calibration band candidate = descriptor ranges intersecting the
  empirical calibration window, labelled *candidate* (F4).
- **Identifiers**: bounded scan for the EPK marker `99 99 99 00 0A` + following bytes
  (candidate label), plus reuse of the generic strings scan (which already covers ASCII
  headers). No Volvo-only regexes in the worker; family-specific parsing stays out of
  the core (Phase 6 handles family semantics app-side).
- Emission: lines appended to `values["metadata"]` (workspace "Metadata") and a summary
  paragraph into `analysis_insights`/`summary`; `VG_EVIDENCE` records flow from the
  existing evidence loop (source "Metadata"/"Xrefs"), so typed evidence works with
  **no** Swift catalog changes.

### 4.3 Tests

- Synthetic container builder in `test-binary-formats.py`: valid vectors + descriptor
  table at 0x1F810 (one verified entry + one INVALID entry) + tail padding → assert
  metadata facts, INVALID detection, candidate labelling; an invalid-container case →
  actionable error; a 512 KB no-tail case → valid with caveat note.
- Rust/unit and corpus tests for the descriptor parser (inclusive odd `end`, `end >= len`, all-zero entries,
  non-zero reserved bytes).

**Files touched:** `binary.rs` (Options format list, `load()`, `ecu_regions`, tests),
`Shared/WorkbenchMessages.swift` (format whitelist += `ecu`),
`App/WorkbenchModel.swift` (format popup + auto-architecture guard for `ecu`),
`scripts/test-binary-formats.py`, docs. **No report-schema, protocol, or XPC changes.**

---

## 5. Phase 3 — UI: task-tailored layout (profiles)

**Goal:** load a Mach-O → today's app; load an ECU image → the ECU toolset, with
Apple-specific surfaces hidden. No behavioral change for existing records.

### 5.1 Catalog: platform tags

- Add a `platforms: Set<VGWorkspacePlatform>` field (default `.all`) to
  `VGWorkspaceDescriptor`, and tag the Apple-specific descriptors (all `.machine`
  group entries incl. VectorEmu/iPhemeral/Hypervisor/Firmware-Runtime/dyld-IPSW, the
  debugger, ObjC/Swift/dyld/chained/ARM64e/Apple-Symbolication/SSA-IR family in
  `.staticAnalysis`). ECU workspaces (Phase 4) are added with `.ecu` platform tag.
  Generic workspaces (Functions, CFG, Strings, Xrefs, Metadata, Notes, Binary
  Workspace, Evidence Graph, …) stay `.all`.
- `scripts/validate-source.py` contract updates: catalog additions are additive; the
  pinned strings (host-unavailable fallbacks, `workspaceRequiresAppleSilicon`,
  LaunchSession validation, `search`) remain present — verify by running
  `python3 scripts/validate-source.py` before/after.

### 5.2 Profile derivation + gating (4 catalog-driven surfaces + 3 hardcoded ones)

- `WorkbenchModel`: `@Published private(set) var profile: VGArtifactProfile` recomputed
  in `selectedID.didSet` from `record.result` (+ import options). Pure function,
  unit-testable (`Tests/profile_derivation.swift` via `swiftc`, following the existing
  contract-test pattern).
- Gate at every surface that offers workspaces:
  1. sidebar (`sidebarWorkspaceButton`, ContentView 2655) — compose with existing host
     gating: `availableOnThisHost && profileAllows`;
  2. tool cards (`reToolCard`, 2606);
  3. ⌘K palette (`VGCommandPalette.results`, WorkbenchChrome 93);
  4. menu bar `CommandMenu("Workspace")` (HindSightApp 72–102) — make it
     catalog-driven by profile (it's hardcoded today);
  5. header quick menu sections (2892+) — hide Apple entries for `ecu`;
  6. evidence-hit routing (`AnalysisNavigation.applyAnalysisHit`) — hits routed to a
     workspace hidden by the active profile fall back to the closest allowed
     workspace (e.g., ECU Disassembly) instead of navigating nowhere;
  7. dispatch fallback: `workspace(record)`/`workspaceCanvas()` — if
     `model.workspace` isn't profile-allowed (e.g., restored session, record switch),
     render a calm "not available for this artifact" state and auto-select the
     profile's home workspace (ECU Overview / Investigation Home).
- MRU tab strip: drop entries not allowed by the active profile when switching records
  (remember them per profile so switching back restores them).

### 5.3 Persistence

- Extend `LaunchSession` (schema 3, reading schema 2): add
  `workspaceByProfile: [String: String]` (validated against catalog names, same rules
  as today at `writeLaunchSessionNow` 1176–1177). Per-project, per-profile last
  workspace; restore on record switch. Continuity schema stays untouched.

### 5.4 Tests / gates

- Swift contract tests: profile derivation matrix (macho/ecu/raw), gating predicate
  table, LaunchSession v3 round-trip + v2 read.
- `scripts/test-macos.sh` typechecks; `scripts/validate-source.py` green (contract
  strings preserved); manual smoke in the built app (plan step, needs a signed local
  build).

**Files touched:** `App/WorkspaceCatalog.swift` (platform field + tags),
`App/WorkbenchModel.swift` (profile + gating API + LaunchSession v3),
`App/ContentView.swift` (gating call sites + dispatch fallback),
`App/WorkbenchChrome.swift` (palette filter), `App/HindSightApp.swift` (menu),
`App/AnalysisNavigation.swift` (routing fallback),
`scripts/validate-source.py` (new additive pins only), `Tests/`.

---

## 6. Phase 4 — ECU toolset surfaces (profile `ecu`)

New workspaces (catalog group ".staticAnalysis"/new group "ECU"), all driven by the
Phase 2 evidence + existing report fields; read-only except where noted.

1. **ECU Overview** — profile home: identity card (size class, arch, vector count,
   checksum verdict, identifier candidates), tool grid, next-step guidance.
2. **Container & Regions** — vector table listing (target → jump), checksummed-region
   candidates, coverage gaps; click-to-navigate into Disassembly/Binary Workspace.
3. **Checksums** — the 31-descriptor table with per-entry verify state; "Stage fixes"
   button computes correct sums for INVALID entries and hands them to the **existing
   staged-patch flow** (`VGBinaryPatch` edits with original-byte checks, overlap
   rejection, atomic save — 0.5.15 feature; no new write path, no coordinator change).
   The fixer algorithm is the verified F3 word-sum + `~sum` store; re-verify after
   patch preview.
4. **Disassembly** — the existing Binary Workspace/page surface (already
   architecture-generic via Phase 1) with C167-friendly defaults (2-byte paging unit,
   vector-table entry points listed).
5. **Strings / Notes / Evidence Graph / Binary Diff** — reused as-is (`.all`).

Data source remains the 75-field report + evidence records → no Swift/worker protocol
changes beyond what Phases 1–3 added.

---

## 7. Phase 5 — C167 bounded emulator (the ECU "machine lab")

Replaces VectorEmu in the `ecu` profile (VectorEmu stays for Apple profiles).

- Deterministic C166-family interpreter on top of the Phase 1 decoder (register file,
  24-bit physical addressing with DPP semantics *only where the modeled routine sets
  them*, bounded step budget, no side effects beyond modeled RAM/SFR window).
- First capability (proves the design): **run the image's own checksum routine** over
  descriptor ranges and compare with stored sums — the same validation philosophy the
  MS4X project uses to prove its decoder on stock images. Watchpoints: DPP0–3, SP, CP,
  checksum accumulator.
- UI: "ECU Emulator" workspace (registers, code, trace, run/step/reset) mirroring the
  VectorEmu lab structure; host-independent (pure software), so no Apple-Silicon gate.
- Boundaries stated in help text: bounded subset, no peripheral timing, no CAN/ADC
  modeling; it executes *validated straight-line + call/return code*, not arbitrary
  peripherals.
- Implemented execution coverage includes DPP-mapped direct and indirect access,
  short indirect/post-increment ALU operands, exact-count `EXTP`, compare-and-adjust
  loop controls (`CMPI1/2`, `CMPD1/2`), near calls/returns, and segmented
  `CALLS`/`RETS` with CSP state. A real Volvo checksum primitive reaches and runs its
  checksum loop. The next qualification boundary is a separate initialized RAM model:
  launching a firmware subroutine without captured/seeded runtime globals must not be
  presented as a meaningful checksum verdict.
- Tests: golden vectors (hand-assembled routines with known register/memory results);
  property test: emulator checksum verdict must equal the static verifier on all corpus
  files that statically verify.

---

## 8. Phase 6 — Definitions & map overlay (bridge to the tuning app)

- **Definitions import** (ECU Definitions workspace): parse TunerPro XDF (and later
  WinOLS/OpenMoose) *app-side* (SwiftUI app can bundle a parser; worker stays
  format-agnostic).
- **Anchor-verified alignment** (F6): for each table, locate the z-block/axes in the
  loaded image by signature search (distinctive axis words), score by monotonicity and
  equation plausibility, report per-table `verified@0x… | candidate@0x… (Δ0x…) |
  not-found` — never silently trusting stale addresses. Verified-aligned maps render as
  evidence-backed regions (grid + Plotly-style preview is app-side).
- **Export**: aligned definitions JSON (addresses + equations + units + verification
  state) — the exact input the standalone ME7 tuner app (separate PWA project, tracked
  separately) consumes for map editing with the same checksum fixer (F3) and container
  parser (F1/F2). HindSight is the analysis/verification authority; the PWA is the
  editor.

---

## 9. Sequencing, effort, and where help is needed

| Phase | Depends on | Parallelizable | Est. size | Best helper |
|---|---|---|---|---|
| 1 (C167 decoder) | — | yes | ~600–900 LOC Rust + tests | Rust engineer comfortable with table-driven ISAs |
| 2 (ecu format + regions) | P1 | yes (contract-first) | ~400 LOC + tests | Rust engineer; careful with bounds/parity gates |
| 3 (profiles UI) | — (contract for profile names only) | **yes — fully parallel to P1/P2** | ~500 LOC Swift + contract tests | SwiftUI engineer who reads validate-source.py contracts first |
| 4 (ECU surfaces) | P2+P3 | split by workspace | ~600 LOC Swift | SwiftUI engineer |
| 5 (C167 emulator) | P1 | yes (pure module) | ~800 LOC Rust + UI | Rust engineer; emulator experience helpful |
| 6 (definitions) | P2 evidence | yes | ~700 LOC Swift | App engineer + tuning-domain reviewer |

Suggested help split (matches "I'll get us some help"):
- **Agent/engineer A — Worker (P1→P2→P5 core):** c167.rs port + ecu container + emulator.
- **Agent/engineer B — UI (P3→P4→P6):** profiles, ECU workspaces, definitions.
- **Integration point:** profile name strings + evidence workspace names ("Metadata",
  "Xrefs", "Functions", "Strings") are the only shared contract; fix them first in a
  30-minute joint pass before parallel work starts.

Cross-phase acceptance commands (definition of done for every PR):
```
cargo test -p reworker && cargo clippy -p reworker -- -D warnings
python3 scripts/test-binary-formats.py <path-to-reworker>
python3 scripts/test-worker.py <path-to-reworker>
python3 scripts/validate-source.py
./build.sh            # when Swift-side phases land
./scripts/test-macos.sh
```

Real-corpus regression (new, added in P2): run the worker through
`scripts/qualify-c167-corpus.py` over the current 118-file Volvo corpus; the 2026-11-06
qualification found 111 fully valid tables and 7 invalid images, with zero verdict
mismatches between the independent Python verifier and production worker. The same
gate independently scans the fixed-byte core of the MIT-licensed ME7RomTool multipoint
checksum needle and compares its hit count with production evidence. It currently
identifies an exact checksum-code signature in 75 images; the worker reports the nearest
preceding `RETS` boundary as a containing-function entry candidate, explicitly not as
calling-convention proof.

## 10. Risks & mitigations

| Risk | Mitigation |
|---|---|
| C167 decode errors corrupt flow candidates | Port the byte-verified table (F7); round-trip property tests; unknown opcodes are barriers, never guesses |
| Parity/contract gates break | No schema changes; additive catalog fields; run validate-source.py in CI before merge |
| Stale XDF addresses mislead users | Nothing auto-trusts definitions; anchor verification + explicit candidate labels (F6) |
| Far-target VA mismatch on non-flat dumps | Document flat-dump assumption; `load_address` override exists; non-flat layouts stay `raw` + analyst mapping until measured |
| Emulator over-reach | Bounded subset, no peripheral modeling, golden-vector tests, explicit "validated routines only" boundary |
| Scope creep toward a Volvo-only tool | Family logic (EPK parsing, definitions) stays app-side/plugin-side; worker stays "C166-core + ME7-class container" generic |

## 11. Non-goals (explicit)

- No remapping/flash-write support, no OBD/K-line/KWP2000 tooling.
- No decompilation/pseudocode for C167 (structural decode only, stated boundary).
- No DTC deletion or emissions-legal-sensitive patch features built into HindSight; the
  staged-patch mechanism stays generic.
- No changes to the existing ARM64 semantic/decompiler paths.

---

## 12. Work status ledger (coordination — READ BEFORE EDITING ANY FILE)

> **Rule for all agents:** before touching any file, read this ledger. Append entries
> ONLY under your own subsection with the edit tool, anchored on your unique heading.
> Never edit another track's subsection, and never restructure this ledger. Format:
> one line per action: `- <UTC time> — <status: STARTED|IN-PROGRESS|DONE|BLOCKED> —
> <files touched> — <gates run / results> — <handoff notes>`. Claim your file
> territory here BEFORE editing it, so parallel agents don't collide.

### Track A — C167 decoder (`rust/crates/worker/src/c167.rs` only)
- (awaiting first entry from the decoder agent)

### Track B — Swift import surface (`Shared/WorkbenchMessages.swift`, `App/WorkbenchModel.swift` only)
- 2026-11-05 22:11 UTC — DONE — `Shared/WorkbenchMessages.swift` (validated(): format whitelist +"ecu", architecture whitelist +"c167"; 2 lines), `App/WorkbenchModel.swift` (selectBinary(): formats array +"ecu", format picker +"ECU firmware (ME7-class)", architectures array +"c167", architecture picker +"Infineon C167" and "From header" title extended with "; ECU auto-detects C167"; 4 lines) — gates: all 3 find-strings matched exactly on first edit attempt (no reformat/rename); `git diff --stat Shared/WorkbenchMessages.swift App/WorkbenchModel.swift` verbatim: ` App/WorkbenchModel.swift       | 213 ++++++++++++++++++++++++++++++++++++++---` / ` Shared/WorkbenchMessages.swift | 183 +++++++++++++++++++++++++++++++++--` / ` 2 files changed, 376 insertions(+), 20 deletions(-)`; no compile gate run (out of task scope, diff-verification only) — handoff: edits were completed just before ledger onboarding, hence a single DONE entry (no prior STARTED); the 213/183-line per-file diffs are mostly PRE-EXISTING uncommitted parent-session work (analysisRevision prototype-abi-v38, ProjectTypeLayoutEvidence, FunctionNameExchange, batch-retry/resume logic) — Track B changed ONLY the 6 lines listed above; "ecu"/"c167" now pass validated() and appear in the import dialog pickers; no other files touched by this track.

### Track C — Worker integration & tests (binary.rs, main.rs, scripts/test-binary-formats.py, docs)
- 2026-11-05 — DONE — `binary.rs` (Arch::C167, Options "ecu"/"c167", Decoder routing, branch/return/prologue/semantic_hint hooks, `ecu_container()`, `ecu_regions()`, analyze() wiring, unit tests), `main.rs` (inspect label), bootstrap `c167.rs` — `cargo test -p reworkbench-worker` 428/428 green; clippy pending component install — decoder agent (Track A) owns c167.rs content from here; integration (Track C) stays out of c167.rs.
- 2026-11-05 — DONE — `scripts/test-binary-formats.py` (synthetic ME7 container suite: capture/page/analyze, checksum verify + INVALID entry, logical-dump tail note, 4 rejection cases), `docs/BINARY_FORMATS_AND_ANALYSIS.md` (ECU coverage + decoder row + validation paragraph), `CHANGELOG.md` (Unreleased section) — harness run pending Track A completion (needs final worker build).
