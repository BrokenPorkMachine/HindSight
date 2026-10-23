# Chimera master list of completion

This is the implementation order for the remaining work in [ROADMAP.md](ROADMAP.md) and [DECOMPILER_GAP_ROADMAP.md](DECOMPILER_GAP_ROADMAP.md). Source recreation is the first product milestone. The original checklists retain their scope and completion status; this plan reorganizes their dependencies without dropping requirements.

Inputs: the two existing roadmaps, the independent [reconstruction-first draft](RECONSTRUCTION_FIRST_ROADMAP.md), the [Apple Silicon application specification](Apple_Silicon_Reverse_Engineering_App_Spec_v1.md), and the current implementation. No separate replacement roadmap from the other model was available when this synthesis was written. The user directed work to proceed with these inputs.

## Current actionable queue (2026-10-22)

This short queue is the live handoff; C00–C16 below retain the full acceptance
criteria. `[~]` means implemented in part or under verification, never complete.
The first five items build one usable AArch64 source-recreation path. The
current execution focus is source reconstruction and semantic coverage;
server readiness and GitHub publication retain their separate gates.

Dump-all shortcut (2026-10-23): both reconstruction menus offer all four formats
for whole binary, current section and address scopes; existing batches have the
same shortcut. A single reconstruction batch feeds `c/`, `h/`, `m/` and `n/`
inside the chosen new destination. Per-format manifests retain unavailable and
failed counts; the aggregate completion manifest is written only after all four
exports pass checkpoint identity and aggregate byte-budget checks. Existing
output is never overwritten. Dump-all intent is checkpointed for app resume.
Portable artifact contracts and both app typechecks pass. The real app/XPC gate
verifies all four folders and exact equality with individual format outputs.
This shortcut does not upgrade review-only code into qualified recovered source.

Objective-C header reconstruction now carries target-local class, ivar,
property and method declarations into per-function `.h` artifacts for uniquely
owned IMPs. Where an exact C-callable IMP declaration is available it is retained
alongside the class review declarations; otherwise the review header replaces
the generic wrapper. Conflicting method encodings are withheld and ambiguous
owners do not supply a class. Header language contracts require Objective-C or
Objective-C++. Superclass/protocol/type dependencies remain explicitly unresolved;
this is not a complete class-layout or implementation claim. All 370 worker tests
and 16 artifact contracts pass. The native fixture verifies actual emitted class
headers at `-O0`/`-O2` and syntax-checks each in Objective-C and Objective-C++.
Class review headers additionally retain superclass and protocol-adoption names
when a unique class record matches both the owner metadata address and class
name. Conflicting relationships remain withheld. A real local-subclass fixture
verifies `HSNativeCatalogDerived : HSNativeCatalogFixture <NSCopying>` in emitted
headers at both optimization levels; unrelated-address and contradictory-parent
unit cases are rejected. External superclass pointers without resolved bind
evidence remain unresolved rather than being guessed from host runtime state.
Ordinary legacy dyld binds now provide typed superclass-slot evidence directly
from the opcode decoder. Only completed streams, checked segment ranges,
zero-addend non-weak pointer binds and known positive library ordinals may supply
an external class name; contradictory binds or local/imported names are withheld.
The actual fixture now recovers `HSNativeCatalogFixture : NSObject` with its
recorded dyld provider at both optimization levels. Tests cover missing segments,
range/VA overflow, unsupported/truncated streams and zero-valued operands that
must not be confused with a DONE opcode. Chained superclass binds remain open.

Supported scalar Objective-C IMPs now additionally emit `recovered-objective-c`:
the validated IR-derived C-callable body plus a synthetic category method that
forwards `self`, `_cmd` and the exact declared arguments. `.m` exports prefer this
representation; `.c` retains the C body. Source review, individual exports,
retained batches, native/CLI format dumps and dump-all preserve the extra `.m`
with exact evidence hashes. Category packaging is explicitly synthetic, not
original source or a complete class implementation. The native fixture checks
method syntax at `-O0`/`-O2`, passes each actual artifact through the Swift
decoder/batch/all-format contract, and checks CLI `.m` dumps against the artifact
bytes. Worker tests (371), strict Clippy, 16 artifact contracts, both app
typechecks and portable source validation pass; broad Objective-C dispatch,
FP/aggregate method bodies and behavioral category deployment remain open.
The scalar method fixture now also compiles and executes the generated category
over an intentionally incorrect base implementation. Six zero/boundary/wraparound
inputs pass at `-O0` and `-O2`; a separately compiled no-adapter control fails as
expected at both levels. This proves the generated adapter is actually selected
for this fixture, not general category deployment, ARC or dispatch equivalence.
Declaration matching now collapses byte-identical SDK/external declarations
while retaining corroborating SDK and hash-bound-catalog provenance. Different
declarations remain separate conflict candidates; matching provenance alone does
not establish ABI equivalence for differently spelled types or parameter lists.

Reconstruction linkage integration (pass 62, in progress): per-function requests
now join validated dyld export names and indirect-symbol call stubs with nlist
names before declaration lookup. Pointer slots are not admitted as callable
addresses. SDK and external-catalog disagreements remain separate candidates
instead of being hidden by lookup precedence. All 369 worker tests pass.
The native Darwin catalog gate now verifies that a source-bound Apple Clang
declaration reaches an actual reconstructed header at both `-O0` and `-O2`,
in addition to existing pseudocode, layout and source-identity checks. Automatic
SDK discovery in the app, provider-qualified matching, complete class/interface
exports and general implementation recovery remain open. Review-only fallback
code is not qualified recovered source. The earlier candidate build predates
these changes; rebuild/requalification and GitHub publication remain pending.

Public SDK lookup now has a source-bound command-line path documented in
[SDK_DECLARATION_LOOKUP.md](SDK_DECLARATION_LOOKUP.md). It can consume an analysis
report or invoke the worker directly, select public import providers, and emit
Clang declaration evidence or strict worker TSV. The real worker-to-SDK test
exposed duplicate built-in/header declarations for `fwrite`; comparison now
ignores only top-level parameter `restrict`, preserving pointed-to qualifiers
and conflicting type shapes. All 14 catalog tests pass, including the actual
captured Mach-O → worker imports → SDK declarations path. This is not yet
automatic app export integration or runtime provider identity qualification.

0.5.13 candidate preparation: flow and evidence graphs now start at 80% zoom;
both minimaps use a fixed 190×120 surface, aspect-preserving projection of the
entire graph extent, scaled node bounds and an exact-scale viewport indicator.
The array declarator regression was reproduced by a failing test: unsupported
row/pointer-array/VLA shapes could silently flatten. Pass 61 retains supported
single-dimension scalar-array decay and withholds unsupported shapes. Native
release qualification is in progress; credentialed gates remain deferred.

SDK scalar memory recovery (pass 60): exact NSInteger/NSUInteger, CF integer
typedefs and dispatch_once_t pointees now preserve declared types in memory AST
nodes and volatile helpers. The native matrix passes 648 configurations and
59,400 comparisons at four optimization levels, with SDK width/signedness static
assertions, exact helper/AST types, header syntax, UBSan and surrounding-memory
checks. Non-Foundation implementations compile as C; Foundation cases compile
as Objective-C. Worker tests (366), strict Clippy, 16 artifact fixtures, portable
source checks, both app typechecks and real app/XPC integration pass. This does
not qualify arbitrary typedefs, aggregate memory or dispatch synchronization.

App batch outcome reporting now persists recovered/review-only/failed counts and
completed/cancelled/incomplete status; failed attempts are no longer described as
exported artifacts. Native checkpoint validation recomputes the counters, and
format dumps retain failed-function counts. The real app/XPC gate verifies a
stale-identity reply produces one failure and no function files; the resulting
header dump retains that failure and emits no fabricated representation. Existing
normal, stale-context, cancellation and resume paths pass independently.
Native contracts reject forged outcome status and each counter while preserving
legacy checkpoints without the additive summary. Both app typecheck targets,
16 artifact fixtures and portable source checks pass; manual UI interaction
qualification remains open.

CLI batch outcome reporting now distinguishes recovered/review-only/failed
functions in checkpoints and final manifests. Any recorded execution/output
failure produces a nonzero CLI exit (also on resume), while preserving evidence;
valid review-only output is not mislabeled as a failure. Native tests verify
byte-budget failure reporting, retained-failure resume and successful mixed
recovered/review-only batches. This does not qualify full binary recovery.
The 92-case native regression passes 4,176 comparisons, including existing
tamper/SIGKILL-resume checks; the full Python suite passes 230 tests with six
optional skips.

Language-aware batch qualification now checks every header language explicitly
listed in the artifact and records compiler-standard/hash-bound per-language
results. Legacy evidence uses the prior Objective-C/Objective-C++ checks with an
explicit legacy-default label, not inferred C compatibility. The SDK gate
verifies 152 successful per-language batch qualification records across four
optimization levels; portable tests check both current and legacy scheduling.
The full Python suite passes 229 tests with six optional skips, and the existing
92-case reconstruction regression passes 4,176 native comparisons plus batch
tamper rejection and SIGKILL/resume checks with language-aware qualification.

Header language contract (pass 59): worker artifacts now record C-family
language eligibility explicitly; the native decoder validates it, Source review
displays it, and native/CLI format manifests retain it. Rehashed invalid language
contracts are refused before publication. Both app typechecks and real app/XPC
format dumps/cancellation/resume pass. This is declaration metadata, not body
equivalence or general language reconstruction.
The native header gate verifies both C-compatible and Foundation-only headers
through the actual Swift decoder and all format exports; 40 configurations,
152 syntax checks, eight language rejections and exact output hashes pass.
Portable tests pass 228 cases with six optional skips.

SDK declaration expansion (pass 58): exact NSRange/CFRange and CoreGraphics
point/size/rect/affine-transform prototypes now emit native-SDK-backed headers,
without aggregate implementation claims. The expanded fixture gate passes 40
configurations, 152 successful C-family syntax checks and whole-batch exports
with exact hashes at four optimization levels. Foundation-only declarations
explicitly require Objective-C/Objective-C++, not fabricated C type substitutes.
Eight negative language checks verify the clear Foundation diagnostic. Worker
tests (366), strict Clippy, 16 artifact fixtures, portable source checks, both
app typecheck targets and real app/XPC dump/cancellation/resume integration pass
with pass 58. General aggregate body recovery remains open.

Declaration-only expansion (pass 57): header validation no longer depends on
the body's register-only integer ABI eligibility. Exact float/double/long-double
and stack-passed scalar prototypes emit headers without invented register
locations or body claims. The native gate passes 16 configurations, 64 C-family
syntax checks and whole-batch header exports with exact hash checks. Worker
tests (366), strict Clippy, 16 artifact fixtures and portable source checks pass;
unsupported types, variadics and C++ declarations remain withheld. FP/stack
implementation recovery and general language header reconstruction remain open.
Both app typecheck targets and the real app/XPC format-dump/cancellation/resume
gate pass with the declaration-only expansion.
The 92-case void/call/Boolean regression also passes 4,176 native comparisons,
tamper rejection and actual CLI SIGKILL/resume checks with pass 57.

Source type expansion (pass 56): ABI classification, source declarations and
typed pointer recovery now admit explicit `signed short int`, `signed long`,
`signed long int`, `signed long long` and `signed long long int` spellings.
The native pointee matrix passes 408 configurations / 37,400 comparisons at
`-O0/-O1/-O2/-Os`, preserving exact declared volatile access types and checking
narrowing, returns and surrounding memory under UBSan. Worker tests (365),
strict Clippy, 16 artifact fixtures, portable source checks and arm64/experimental
Intel app typechecks pass. Engine/cache/reconstruction versions advance together;
general typedefs, aggregates and C++/Swift implementation recovery remain open.
The real app/XPC integration also passes with pass 56, including all four format
dumps, exact output hashes and requested-format cancellation/resume.
The broader recovered-source regression passes 544 function/configuration cases
and 100,640 native comparisons with the same worker.

Discoverable format dump workflow: main-menu **Reconstruction** now offers
explicit `.c/.m/.h/.n` dumps by whole-binary/current-section/address scope,
using sequential app/XPC reconstruction rather than current-function review
fallbacks. Requested formats are exported to a dump subfolder with retained
evidence, exact hashes and unavailable counts. Existing validated app batches
can also be dumped without re-analysis. Native dump contracts and both app
type-check targets pass. The real app/XPC gate exports all four formats and
independently verifies artifact identity, output hashes and unavailable counts,
while the existing batch cancellation/resume checks remain green.
App checkpoints now preserve original scope and requested dump format across
cancellation/resume; conflicting requests and existing dump destinations are
preserved without overwriting. The real app/XPC gate verifies resumed native
dump hashes. CLI format manifests also retain worker/prototype digests, source
size and original batch scope. Five portable CLI tests cover review-only and
failed functions, foreign rehashed evidence and refusal-before-publication;
the full Python gate passes 227 tests with six optional skips.
CLI resume now independently validates retained evidence graphs, exact generated
representations and recovery states, not just file hashes, before replacing any
checkpoint. The native gate rejects seven rehashed evidence/state/identity
tamper cases in qualification, dump and resume without changing retained
metadata; valid partial and direct-callee closure resumes still pass. This does
not qualify recovery across a mid-worker crash or app/service restart.
The native fixture gate additionally SIGKILLs the real CLI process after its
first checkpoint and after a second function's files precede its checkpoint.
Fresh processes resume both cases to the uninterrupted three-function result,
preserving checkpointed and orphan output bytes and mtimes. Partial-file writes,
power-loss durability and app/XPC restart remain unqualified.
The equivalent CLI exporter passes whole-batch and range/address checks,
exporting 25 bodies, 31 headers and 21 native outputs from the test fixture.
Manual UI qualification and broader C++/Swift source recovery remain open.

Batch qualification consistency: the qualifier now independently checks each
retained reconstruction graph and binds source identity, function bounds,
recovery state and every exported byte before compilation. Compilation uses a
bounded private byte snapshot, preventing post-verification input changes from
changing compiler input. Seven rehashed tamper cases are rejected, and a race
fixture confirms compilation consumes the verified snapshot. These checks do
not constitute producer authentication or original-image behavioral equivalence.
The focused gate passes 92 configurations, 4,176 native comparisons and 24
body-rejection/header cases, including 31 syntax-checked headers and 25 linked
implementations. Source validation passes 222 tests (six existing optional
skips) plus native contracts. No app sources changed for this qualification slice.

Large-analysis report bounds: fixed a reproduced `itunescloudd` analysis
failure where `prototype_conflicts` reached 88,112 bytes and violated the native
65,535-byte section limit. The producer now uses a 60,000-byte budget, retains
whole UTF-8 rows, prioritizes conflicts, and reports omitted/displayed counts.
The same binary now yields 59,641 bytes for this section (largest section:
61,436 bytes) and passes native identity/section decoding. All 364 worker tests
and strict Clippy pass. Decoder limits were not raised.

Export representation correctness: `.h`, `.c`/`.m`, and `.n` review exports
now require header, recovered-body and machine-state artifacts respectively,
without first-source fallbacks. Unavailable representations stay explicitly
unresolved; C++/Objective-C++/Swift translations remain commented evidence.
Native contracts pass all seven formats and both app type-check targets pass.
Real app/XPC format rendering passes alongside batch, hash and cancellation/
resume checks. Native integration requires actual machine-state source; the
save panel permits the custom `.n` extension. Manual save-panel qualification
remains open.

Pass 55 integer store conversions: declaration-proven integer values can now
be converted to an exact declared pointee type at the decoded store width.
Narrow register views carry their explicit unsigned cast type and retain the
original declaration on the child expression; unresolved and pointer values
remain blocked. Volatile helpers retain the actual declared pointee type rather
than an equal-width but possibly C-incompatible typedef; distinct native helpers
have distinct identities. All 362 worker tests and strict Clippy pass. Expanded
native qualification passes 288 configurations and 26,400 comparisons, including
actual volatile helper pointer-type assertions. Source validation passes 222
tests (six optional skips) plus native contracts; both app type-check targets
pass. Full source regression passes 544 configurations and 100,640 native
comparisons; the existing focused gate passes 92 configurations and 4,176
comparisons, and all 16 worker/native artifact fixtures pass.

Pass 54 constant stores: exact declared integer pointee types now constrain
known constant stores, including volatile zero/all-ones writes. Structured
constant-bit evidence survives register-width views and copies without parsing
rendered expressions. Unknown values and noncanonical Boolean stores remain
blocked. Expanded native qualification passes 240 configurations and 20,400
comparisons. All 360 worker tests and strict Clippy pass; source validation
passes 222 tests (six optional skips) plus native contracts, and both app
type-check targets pass. Full source regression passes 544 configurations and
100,640 native comparisons. The existing focused gate passes 92 configurations
and 4,176 comparisons; all 16 worker/native artifact fixtures pass.

Pass 53 native C pointees: source recovery preserves exact native integer
spelling, width and signedness through pointer reads and volatile accesses,
including indexed load/store/exchange schedules. The new native gate passes
144 configurations and 18,000 comparisons over 12 representative spellings;
358 worker tests and strict Clippy pass. Full source regression passes 544
configurations and 100,640 native comparisons; the existing focused gate passes
92 configurations and 4,176 comparisons. All 16 worker/native artifact fixtures,
both app type-check targets and source validation (222 tests, six optional
skips, plus native contracts) pass. Plain `char` assumes the default signed Apple LP64 spelling; alternate
compiler signedness modes remain outside the qualified contract.

Declaration-only qualification: the batch qualifier now offers an explicit
`--headers-only` mode with separate retained evidence and a syntax-only claim.
It does not compile implementations or link a library. Default implementation
qualification still requires recovered bodies; empty header sets cannot pass.
The focused gate passes header-only success, implementation-mode refusal,
missing-declaration refusal and preservation of previous reports. Native
reconstruction still passes 92 configurations and 4,176 comparisons; source
validation passes 222 tests (six existing optional skips) plus native contracts.

Scope correctness: reconstruction batches and review exports now share exact
section/address planning. Current-section exports no longer include unrelated
sections; missing/overflowing bounds, ambiguous address matches and silently
truncated scopes are rejected. Address actions offer explicit hex/decimal entry.
Native scope contracts pass with the 16 artifact fixtures; both app type-check
targets pass. Real app/XPC verification selects section/address functions from
a force-verified instruction page and passes the three-function batch, output
hash, cancellation/resume and HTTPS transaction checks. Manual address-dialog
interaction remains unqualified.

Pass 52 separates exact declaration/header recovery from body eligibility.
Supported declarations survive rejected or trap-blocked bodies without claiming
implementation recovery. Native tests retain and C/C++-parse headers across
24 body-rejection cases; header-only native decode/export/checkpoint checks
pass. Batch output has 25 recovered implementations and six review-only entries.
357 worker tests, strict Clippy, 16 worker/native-decoder artifact fixtures and
both app type-check targets pass. Full source regression passes 544
configurations and 100,640 native comparisons. All 31 batch headers pass
Objective-C/Objective-C++ syntax checks, including the six review-only entries;
the 25 recovered implementations compile and link separately.

Pass 51 focused checkpoint: indexed volatile Boolean load/store/exchange
recovery retains proven address types and ordered memory events. The native
gate passes 92 configurations, 4,176 comparisons and 20 rejected invalid cases;
batch qualification compiles and links 25 functions with five review-only.
357 worker tests, strict Clippy and all 16 worker/native-decoder artifact
fixtures pass. The full source regression passes 544 configurations and
100,640 native comparisons. Arm64 and experimental Intel app type-checks
pass; source validation passes 222 tests (six existing optional skips).

Pass 50 focused checkpoint: 357 worker tests and strict Clippy pass; the
void/call/Boolean native matrix passes 80 configurations, 336 comparisons and
20 rejection cases. Batch qualification compiles and links 22 recovered
functions, leaving five explicitly review-only. All 16 worker/native-decoder
artifact fixtures pass. Source validation passes 222 tests (six existing
optional skips) plus native contracts. The full pass-50 matrix also passes
544 configurations and 100,640 native comparisons.

Last full source checkpoint (pass 48, prototype/cache revision 12): 356
worker tests and strict Clippy pass; 16 worker/native-decoder artifact fixtures
pass; the full recovered-source matrix passes 544 function/configuration pairs
and 100,640 native comparisons. The focused void/call/Boolean matrix passes 56
configurations, 240 native comparisons and eight negative cases. Batch export,
resume, tamper refusal, direct-callee closure, C/C++ headers, and a linked
15-function recovered subset are exercised in that focused gate. Full source
validation passes 222 tests (six existing optional skips) plus native contracts.
These results qualify the supported slices; the remaining C00–C16 requirements
and interactive batch/recovery qualification stay open.

1. [~] **Keep the current worker sound.** Vector integer ADD/SUB/MUL and MLA/MLS now retain exact arrangements, reject reserved encodings, and fold with independent lane-width modular wraparound; MLA/MLS read the prior destination as an SSA input, and Q=0 writes prove upper-half zeroing. Reverified on 2026-10-22: 353 worker tests; the 16-variant decompilation corpus (1,006 address-mapped instruction rows); 544 recovered-source function/config pairs (100,640 native comparisons); 36 machine-state functions (5,220 native comparisons); 82 native vector-extract forms (42,640 result comparisons); 12 native MLA/MLS forms (3,120 comparisons); and 40 vector FP forms (9,600 finite-normal native comparisons over `.4h`, `.8h`, `.2s`, `.4s`, and `.2d`). The FP gate excludes NaNs, signed-zero selection, FPCR modes and exception flags. Apple-Clang encoding-table tests cover lane-wise versus pairwise/fused SIMD FP neighbors and reserved `.1d` barriers; vector FP def-use ranges are checked in worker contracts and reconstruction artifacts. Barrier artifacts expose per-instruction read/write partitions and nearest surrounding accesses only across unique contiguous CFG fallthrough, labeled as program order—not proof that a barrier domain orders every access. A frame store–DMB–load fixture verifies emitted links, while a branch-around/join fixture verifies they are withheld across ambiguous control flow. Floating-point reconstruction now records FPCR/FPSR unknown-effect blockers and withholds strict machine-state output when those effects are reachable. Broad SIMD/system fallbacks, domain-specific ordering semantics, and all reconstruction consumers remain under audit. (C01–C03)
2. [~] **Finish the per-function source review path in the macOS app.** The coordinator request and Source pane are wired in the working tree. A real worker-produced artifact passes the native Swift decoder contract, which rejects a stale source digest, mismatched function bounds, and a tampered dangling graph edge. Its review-context predicate rejects a changed project, record, analysis revision, or source digest; app and coordinator sources typecheck for arm64 and x86_64. The extended macOS integration transaction passed: the test app imported a captured Mach-O, requested source-bound analysis, reconstructed a function through CoordinatorService/XPC and the bundled worker, decoded it with the app’s native helper, checked its review context, and exported/validated the emitted artifact. Flow-graph branches now use available map dimensions, include call edges in component placement, and route fan-outs through distinct ports/channels. Evidence families now occupy separate lanes across the full map, with per-edge routes for fan-outs; layout/route contracts pass. Coordinator analysis failures identify request-ID, digest, count, and section-size mismatch classes; an oversized section diagnostic now reports its field and actual byte count. Reconstruction cancellation now tracks its own request and reports accepted, already-finished, and failed cancellation outcomes; contract coverage confirms origin spans navigate to their instruction addresses. Direct interactive UI qualification of origin selection/cancellation remains open. The reported interactive analysis mismatch is not reproduced by the fixture transaction; its specific input/result remains unverified. (C00/C11/C15)
   System-register audit: corrected NZCV, FPCR, FPSR, DAIF, and CurrentEL encodings. Shared IR now models `MRS/MSR NZCV` with explicit GPR and flag-file effects; other system-register accesses remain conservative and strict export is blocked where their state effects are not modeled. Strict machine-state C and interpretation now execute `MRS/MSR NZCV`; a native readback/write/condition fixture passes in the 36-function, 5,220-comparison machine-state gate. Other system-register state effects remain explicit blockers.
3. [~] **Join exact Apple names and types to that path.** Extend the existing native dyld/libobjc/Foundation/CoreFoundation/Clang SDK catalog with target image/ABI/version checks; surface trusted class, selector, method, property, ivar, C declaration, struct and field evidence. Add a project type catalog and reject conflicting candidates. Reuse qualified shared macOS/iOS semantics at their actual ABI scope. (C00N/C04–C06)
4. [~] **Extend structured SSA and the typed source AST.** Complete flags, vector and memory effects, stack/alias lifetimes, variables, expressions and type constraints without reparsing rendered text. Expand the bounded scalar walking skeleton through external memory, real calls, aggregates, loops and switches; unsupported reachable effects remain blockers. Reconstruction artifacts include a per-function coverage summary for decoded instructions, shared IR, modeled trees, data-in-code and unsupported-effect reasons. The independent artifact gate checks every reported count across 16 fixtures and confirms deterministic output; the native decoder validates the counts and the Source pane displays them as evidence availability, not full semantic coverage. Acyclic nonvolatile pointer reads and writes now preserve per-path instruction order, including guarded branch-dependent mixed access schedules whose memory events remain monotonically ordered by instruction address. Exact-prototype scalar recursion and direct symbol-resolved, memory-free leaf calls are supported when the callee returns and preserves ABI callee-saved state. Caller memory effects before and after proven calls now emit in per-path instruction order for bounded acyclic control flow; guarded branch-call schedules, two sequential calls to a proven pure leaf, and exact self-recursion followed by caller stores pass the expanded native matrix. General external calls, indirect calls, loops, aggregates, and broader alias/lifetime semantics remain open. (C01–C08)
5. [~] **Make supported source recompilable and test it.** Export dependencies and source maps with C17/Objective-C functions, compile against native Apple libraries where the recovered behavior requires them, and compare original/IR/generated behavior. Keep source resemblance, behavioral agreement and formal claims separate. Expand `-O0/-O1/-O2/-Os`, stripped/symbol-rich and macOS/iOS shared qualification. (C09/C10/C12)
6. [~] **Qualify Linux static processing in Docker.** A non-root, read-only, no-network Rust/C worker image, one-shot launcher and amd64/arm64 CI jobs are in the working tree. The test stage (253 worker tests, five protocol tests, fixture suites) and restricted runtime smoke pass for Linux amd64 under Docker emulation on this Apple Silicon host. Native Linux amd64/arm64 CI results remain required. Then integrate a trusted scheduler adapter if the app-hosted server is to offer Linux jobs. This does not complete Linux process attach. ([Linux Docker contract](LINUX_DOCKER.md), C13/C15)
7. [~] **Implement app-hosted remote communication.** The app hosts an opt-in HTTPS gateway (loopback by default, explicit private IPv4 bind and exact HTTPS browser origin), with a Keychain TLS identity, per-start bearer token, project snapshot, and bounded asynchronous analysis jobs with status polling, normalized progress, result retrieval, cancellation and ten-minute retry idempotency. `GET /v1/openapi.json` supplies the client model and truthful capabilities. The full gate passes, including a real captured Mach-O import and source-bound analysis through XPC and the native worker to a certificate-verifying HTTPS client. Per-user project/record grants, job ownership and a bounded durable user/grant/audit store are wired into the gateway. User deactivation persists and immediately revokes live bearer tokens; Settings supports identity enrollment, token issuance/re-issuance, identity deactivation, per-record and project-wide grants, per-grant revocation, and review of the latest 100 audit entries. Project grants include project snapshot reads and analysis capabilities for every captured record; enrollment grants no access by itself. Bearer tokens are ephemeral, replacing a token revokes its predecessor, and gateway stop revokes all live tokens. Failed identity/grant persistence rolls back in-memory authorization and is regression-tested. Remaining: target-level authorization, uploads, sequenced events/backpressure/reconnect, and cross-device security qualification. Debugger and VM actions remain unavailable until ownership, consent and isolation are enforced. See the [remote acceptance gate](HINDSIGHT_REMOTE.md). (C14/C15)
8. [x] **Audit and publish the v1 client API.** The v1 endpoint contract is served by the app, the supported scope passed correctness/security gates and a real app → XPC → worker → HTTPS client transaction, and commit `b0c8999` is on `BrokenPorkMachine/HindSight:main`. Cross-device/browser qualification and the additional product capabilities remain open under item 7 and [HINDSIGHT_REMOTE.md](HINDSIGHT_REMOTE.md). (C15/C16)

## Active priority: AArch64 pseudocode and source reconstruction

Re-plotted 2026-10-18. The next product outcome is dependable, named, typed AArch64 pseudocode for Apple binaries; the next is evidence-backed, recompilable C17/Objective-C reconstruction. Near-1:1 fidelity is a separate measured goal, not permission to invent erased source information. ARM64e uses this pipeline with authentication effects explicit until qualified.

Existing inputs are not restart tasks: normalized function boundaries, shared instruction/CFG records, bounded GPR SSA, prototype ABI recovery, isolated Darwin collection, Apple Clang prototype import, and bounded target property/ivar joins already exist. Their limited scope does not close general semantics, memory SSA, type inference or source reconstruction. Extend these implementations rather than build a second decompiler.

The table is the active delivery sequence; C00–C16 below remain the detailed scope/status registry. Each row delivers a working function slice. Do not wait for the whole ISA, every SDK declaration or a general solver before improving supported pseudocode. Evidence tracing, compilation and differential checks begin with the earliest eligible slice, not after the last row.

| Priority | Deliver next | Dependencies / scope | Exit evidence |
| --- | --- | --- | --- |
| 1 — now | Dependable named AArch64 pseudocode: preserve parameters through proven frame/copy/join operations; enforce exact instruction effects, widths and invalidation; expose unsupported effects | Existing function/IR pipeline; C00/C01/early C02–C06 | Scalar integer/FP, frame, call and member fixtures keep correct names and effects at `-O0/-O2`; unknown/reserved encodings cannot retain stale identities. |
| 2 | Canonical register/flags/vector and memory SSA, frame objects, aliases and lifetimes; retire text-reparsed identity/effect decisions | Priority 1; C02/C03 | Copies, partial writes, spills, overlapping slots, joins, loops and calls have correct structured def-use/memory versions and bounded analysis. |
| 3 | Complete the first slice's ABI, calls and native type joins: stack/aggregate arguments and results, C declarations, class/selector/method names, properties/ivars, struct fields and provider images | Priorities 1–2; C00N/C04–C06, iterated together | Compiler/libobjc/SDK oracle agreement; mismatched images/layouts/ABIs and ambiguous dispatch retain conflicts instead of fabricated names/types. |
| 4 | Typed source AST and source-level variables/expressions: parameters, locals, globals, fields, arrays, casts and side-effect-safe expression folding | Priorities 2–3; C05/C07 | Pseudocode replaces registers only where proven; every AST node links to bytes, IR, SSA and its name/type derivation. Real worker artifacts pass C00 validation. |
| 5 | Nested conditions, short circuit, loops, switches, early returns and tail calls | Canonical CFG plus priorities 2–4; C02/C08 | AST-to-CFG/effect checks preserve every supported reachable edge, exit and side effect; irreducible/unresolved flow stays explicit. |
| 6 | Export complete supported C17/Objective-C functions and dependencies; retain a strict machine-state C mode for the same semantics | Early strict emitter/interpreter from priority 1; recovered AST from priorities 3–5; C01/C09 | Recorded native toolchain compiles deterministic output with declarations, globals, imports, layouts, helpers and source maps; unresolved reachable effects block strict export. |
| 7 | Qualify the end-to-end AArch64 reconstruction milestone with behavior and near-original fidelity reported separately | Checks start at priority 1 and expand through priority 6; C10/first C12 gate | Original/IR/generated execution agrees on recorded returns, memory, traps and calls; source scoring covers names/types/AST independently. Full `-O0/-O1/-O2/-Os`, stripped/symbol-rich matrix. |
| 8 | Complete integrated review and corrections for that pipeline | Minimum source/evidence navigation accompanies priorities 1–7; full C11/C15 | Expression selection reaches all instruction origins; analyst edits invalidate only dependent work; cancellation/reanalysis preserves honest completeness. |
| 9 — after the first milestone | Extend AArch64 C++/Swift and remaining ARM64e semantics, then x86-64, ELF/PE/COFF, broad interoperability, team/on-prem and release completion | Qualified first C/Objective-C slice; remaining C12–C16 | Each added language/ISA/platform passes its own declared gates; all original requirements remain in scope. |

### Immediate implementation queue

These are the next bounded tasks within priority 1, not new completion claims or a duplicate checkbox ledger:

1. Keep the worker/decompilation/native-catalog/prototype-ABI gates green on the current tree. The parameter-name/frame-update regression now has a passing prologue test and unchanged 16-variant corpus gates; preserve negative clobber/join coverage as naming moves to structured IR.
2. Audit broad SIMD/system fallbacks and reserved/neighboring encodings. Replace unsound no-effect classifications with exact effects or conservative unknown barriers, including GPR, vector, flags and memory invalidation. Protect every reconstruction consumer, not only member substitution. Verified progress: per-instruction flag/vector effects and per-bit/per-byte SSA identities are exposed in the preview and reconstruction derivation graph, including phi links and partial vector ranges. The NZCV file is modeled bit by bit, so partial flag writes preserve unaffected flag identities. Native AArch64 qualification confirms scalar and Q=0 SIMD writes zero upper bits; narrow loads and scalar operations define the full vector register, while source reads remain lane-precise. SIMD integer arithmetic retains lane width and now folds with per-lane wraparound, bounded provenance, and Q=0 upper-half zeroing. Apple-Clang-qualified SIMD FP three-same encodings distinguish lane-wise operations from pairwise/fused neighbors across half/single/double forms; exact arrangements and GPR/NZCV/vector effects are tested, and artifacts retain instruction-scoped vector def-use bytes. Reserved `.1d` FP forms are opaque barriers across all register files; worker-to-artifact fixtures prove unknown GPR, NZCV, vector, and frame-memory identities reach later consumers through evidence edges. DMB/DSB domains and ISB/CLREX system effects have exact-span events without fabricated memory writes; artifacts also link each event to nearest memory accesses across unique contiguous fallthrough using explicitly program-order-only relations, verified by a frame store–DMB–load fixture, while a branch-around/join fixture verifies no such link crosses ambiguous flow. These links do not prove shareability-domain coverage or ordering across control-flow joins. Unmodeled target-specific operations and data-in-code consume and invalidate the full flag and vector files; lane extracts depend only on selected vector bytes, with malformed internal lane forms falling back to a whole-register dependency. ADCS/SBCS now consume C, define all NZCV bits, and fold exact results when their operands and carry are proven. The remaining SIMD/system encoding space, full consumer qualification, and integration into source variable/type recovery remain under audit.
3. Parameter-name/copy/clobber decisions now use [bounded shared-effect identities](AARCH64_SOURCE_VARIABLES.md), including frame spills, paired writes and agreeing CFG paths. The bounded IR preview and reconstruction derivation graph now carry GPR, per-bit NZCV, per-byte vector identities and phi evidence across joins; memory versions now have path-sensitive CFG phis, alias-wide read/write dependencies and proven 64-bit constant pointer offsets. Recovered-source memory read/write AST records cite their instruction-scoped memory SSA use/definition records, and each proven frame-object version links back to the corresponding memory definition; the 544-function native matrix verifies those links and unchanged behavior. Using path-specific reaching versions and phis to establish source variable lifetimes/value flow remains open, as do rendered control-flow structuring and other text-derived reconstruction decisions.
4. Extend existing worker-produced function artifacts with richer native type/image origins and source/variable/effect evidence, preserving source/function/pass/settings identities, exact dependency spans, blockers and budget exhaustion. Join these records to coordinator/client review and export; offline schema tests alone do not finish that integration. Partial progress: pass-4 flag/vector evidence and pass-6 memory definitions, uses and CFG phis now enter the app’s bounded explanation trail with navigable instruction spans; pass 11 emits function-matched nlist/export names and Objective-C class layouts and no longer attaches image-wide tables to a function code span. Target image/provider joins, SDK declarations, conflict handling, app navigation, and exported source-map integration remain open.
5. Expand the now-qualified C/Objective-C scalar walking skeleton through loops, external memory, native calls and source-level layouts, while exposing its recovered-source view in the app. The emitter qualifies bounded reads, declaration-typed dynamic byte offsets, and ordinary external writes through declaration-proven pointers. Writes are emitted as separate statements with AST and memory-effect provenance; acyclic branch guards preserve conditional writes. The native matrix now includes dynamic byte-index stores and source-reconstructed natural loops with bounded induction registers; one single-entry loop slice also preserves a declaration-proven volatile external store on each iteration with memory SSA provenance and final-memory differential checks. Volatile paths with indirect, unresolved or effectful calls, external read/write mixes with unproven ordering, unproven offsets, unsupported calls, and loop shapes outside the qualified natural-loop form still block the path. Preserve differential checks throughout; do not hide unsupported effects behind stubs or postpone validation until general recovery is complete.

Darwin evidence is an early and continuous dependency, not a deferred catalog project. Use native dyld/Mach-O, libobjc, Foundation/CoreFoundation, Apple Clang SDK layouts, libSystem/dispatch and applicable Mach/XPC contracts. Keep target facts separate from helper-host observations. Qualify shared macOS/iOS dyld layouts and semantic contracts once at their demonstrated architecture/ABI/version scope; test only remaining platform-specific deltas separately. No blanket claim that macOS execution proves every iOS behavior.

Persistence, memory/work budgets, cancellation, cache invalidation and crash recovery from C15 accompany the affected passes. Additional formats, generalized team features and cosmetic work do not displace this queue unless they directly unblock it. Preserve already claimed work and coordinate overlapping changes; reprioritization does not authorize discarding another contributor's edits.

## What the comparison changed

| Question | Existing roadmaps | Independent draft | Master decision |
| --- | --- | --- | --- |
| First outcome | Broad target expansion alongside decompiler features | High fidelity source reconstruction | Source reconstruction first; breadth follows a validated working pipeline. |
| Foundation | CFG, shared IR, bounded GPR SSA, metadata, pseudocode already exist | Evidence schema and complete semantics before later phases | Reuse the existing foundation; add structured provenance and eligibility around real operations immediately. |
| Call/type ordering | Some structuring precedes complete ABI/type inference | Interprocedural analysis precedes type and expression recovery | Establish effects, ABI locations, memory SSA, call summaries, and basic types before high level expressions and structuring. Iterate these analyses together. |
| Scope of an iteration | Instruction families and individual features | Large sequential phases R0–R10 | Deliver repeated end-to-end vertical slices; never wait for the entire ISA or all languages before testing generated source. |
| Evidence graph | Investigation graph and text/navigation records | Full reconstruction derivation graph | Connect bytes → IR → SSA → types → AST → validation with stable identities from the first slice. Keep investigation and derivation graphs interoperable. |
| Validation | Useful structural corpus and source-map checks | Recompilation, differential tests, SMT, fidelity scoring | Add each gate as soon as its corresponding output exists. Differential agreement is bounded evidence; universal equivalence requires an explicit proof domain. |
| C dialect/output | Specification requires C17 and three output levels | Draft says C11 and two modes | Use C17: readable pseudocode, strict machine-state C, and recovered source. Each declares its completeness and execution model. |
| Completion | Some broad “foundation complete” labels cover still-open details | Many newly defined completion gates | Feature checkboxes require their full stated gates. Existing partial features are inputs, not proof that full reconstruction works. |

## The first outcome the user must be able to use

Open an ARM64/ARM64e Mach-O, select a function, inspect reconstructed source and all contributing machine evidence, export a complete supported function with its dependencies, compile it, and compare its execution with the original under a recorded ABI and memory model. Extend that working path until C and Objective-C corpus functions cover scalar integer/FP code, frames, globals, aggregates, calls, nested branches, loops, and switches. ARM64e authentication semantics remain explicit until qualified.

Original names, layout, idioms, and structure should be recovered whenever available evidence supports them. The fidelity score measures resemblance to known original source separately from behavioral correctness. Information erased by compilation remains an uncertainty or an analyst choice. Avoid a single unexplained confidence score.

Completion evidence must identify the source hash, architecture, ABI, function bounds, analysis revision, toolchain, execution model, assumptions, test seeds, supported paths, and unresolved effects. “Compiled,” “exercised,” “differential-tested,” and “formally checked under assumptions” are different claims.

## Darwin native runtime and shared macOS/iOS qualification

Apple reconstruction uses Darwin native facilities: SDK Mach-O definitions and available inspection helpers; dyld image/slide/linkage APIs; libSystem, libobjc, libdispatch, CoreFoundation and Foundation; and Mach/XPC for applicable process and service contracts. Reconstructed programs link real native libraries when those libraries supply the recovered behavior. The existing Mach debugger and NSXPC coordinator are integration points.

macOS execution qualifies the shared foundation of iOS semantics when the relevant contract and implementation are shared. Reuse instruction/IR, C/Objective-C, Foundation/CoreFoundation, dispatch, allocation, and messaging qualification at the level established by evidence. Record function/API, architecture, ABI/data layout, implementation/version identity or source-backed shared contract, preconditions, environment, and validation scope. Version/ABI/platform differences become explicit unqualified deltas; simulator/device/guest checks address those deltas.

In particular, use one shared Mach-O/dyld decoder and test corpus for both platforms. Common load commands, binding/export opcodes, chained-fixup pointer formats, and cache structures are selected by their encoded layout/version and feature fields. The originating OS is metadata rather than a reason to duplicate parsing. Successful macOS fixtures qualify the same dyld decoding paths for iOS; platform-specific records and unsupported format versions retain their own coverage status.

| Facility | Reconstruction use | Qualification evidence |
| --- | --- | --- |
| SDK `mach-o/loader.h`, `getsect.h`, `dyld.h`, `dyld_images.h`, fixup definitions | Image/slice/platform identity, sections, loaded images, slides, imports/fixups | SDK/build identity, UUID, load commands and mapped range agreement |
| dyld / `dladdr` / validated symbol lookup | Runtime resolution and binding evidence | Actual provider image/symbol, image identity, architecture, symbol contract |
| libobjc, Foundation, CoreFoundation | Objects, encodings, dispatch, ARC/runtime behavior and real framework calls | Shared contract/implementation evidence, metadata, layout, object and return effects |
| libSystem and libdispatch | Memory, strings, synchronization, blocks and execution primitives | ABI, ordering/lifetime contract, native observations and recorded environment |
| Mach and XPC/NSXPC | Task/VM/port/message and service-boundary behavior | Rights, message format, connection/endpoint contract, actual supported environment |

Native helpers inspect mapped runtime images; bounded static parsers inspect captured files. Each runtime observation stays attached to its platform and implementation, with an explicit shared-qualification edge where applicable.

## Execution rules

1. Follow the active delivery order above, starting with current AArch64 pseudocode correctness. Extend thin C00 contracts, C00N native evidence and canonical function parsing wherever that slice requires them; do not make catalog or ISA completeness a prerequisite for useful output. Shared evidence identities, budget accounting, persistence, cancellation, and source maps accompany every pass.
2. Preserve machine widths and effects before simplifying. Unknown reachable instructions/calls/memory effects block strict source export unless a defined intrinsic or explicit dependency models them.
3. The canonical pipeline consumes structured IR and a typed source AST. Rendered pseudocode is a presentation and optional parsed input language, not an analysis database.
4. Every task below is still open unless evidence explicitly establishes its complete acceptance gate. A schema draft or row-count test alone cannot close an integrated feature.
5. Continue original roadmap work after the reconstruction milestone; priority changes do not remove the rest of the objective.

## C00 — Artifact, provenance, eligibility, and corpus contracts

Dependencies: current captured-source identity, normalized function boundaries, worker IR/CFG.

- [~] Define versioned reconstruction AST, effects, types, variables, hypotheses, source spans, dependencies, and validation artifacts; reconcile the draft JSON schema with C17 and the worker transport. *(in progress — claimed)*
- [~] Implement content-derived identities scoped to binary, function, pass version, and analysis settings; validate uniqueness, references, spans, graph integrity, limits, and artifact hashes. *(in progress — claimed)*
- [x] Separate observed facts, derivations, assumptions, analyst assertions, rejected alternatives, and conflicts; support bounded “why,” alternatives, dependency, contradiction, and invalidation queries.
- [x] Introduce per-function review/export/validation eligibility with explicit blockers and completeness; incomplete analysis cannot acquire an equivalence label.
- [~] Extend the redistributable corpus to `-O0/-O1/-O2/-Os`, stripped/symbol-rich builds, compiler variants, and per-feature ground truth. Keep scoring-only source/debug oracles out of the recovery path. *(in progress — claimed)*

Acceptance: real worker artifacts pass structural and semantic validation; mutations of content, references, provenance, completeness, or eligibility are rejected. Reports distinguish decoded coverage, semantic coverage, fidelity, compilation, and tested behavior.

Current progress: a C17 artifact schema, offline content/reference/AST/eligibility validator, and bounded evidence-query implementation exist with executable positive/adversarial tests. The worker now produces bounded, review-only AArch64 machine-state AST/evidence artifacts directly from shared IR, normalized function boundaries, conservative instruction CFG and structured register SSA. Cross-language content-ID, mutation, truncation and captured-source checks qualify this first transport slice; recovered-source compilation, memory/type semantics, native catalog joins and general semantic validation remain open. See [worker artifact scope](WORKER_RECONSTRUCTION_ARTIFACTS.md); this does not close C00.

## C00N — Darwin native naming, declarations, images, and type evidence

Dependencies: captured image identity and the thin C00 evidence contract. This work begins before general SSA/type inference and supplies its initial names and types.

- [x] Establish [isolated Darwin runtime collection and Apple Clang C prototype import](DARWIN_NATIVE_EVIDENCE.md): real helper-process image/selector/property/ivar/layout records, compiler-derived declarations, source/platform-bound worker prototype input, and `-O0/-O2` pseudocode integration. Rich native-record joins, full type-library ingestion and target-runtime collection remain in the open tasks below.

- [~] Build one version-aware Mach-O/dyld path for macOS/iOS: UUIDs, architecture/subtype, platform/build version, segments/sections, symbols/exports, dylib ordinals, reexports, stubs/GOT/authenticated pointers, chained fixups and shared-cache image identities. *(in progress — claimed)*
- [~] Collect native loaded-image ownership and slide/symbol evidence through dyld, `dladdr` and applicable Mach APIs; join file/preferred/runtime addresses only through a validated mapping. Native introspection runs in its own helper or qualified target runtime. *(in progress — claimed)*
- [~] Recover function names from exact target symbols, validated exports/import providers, runtime IMPs, debug records, and SDK declarations. Preserve exact names and raw mangling alongside inferred aliases and provenance; a nearby symbol address alone does not establish an exact function identity. *(in progress — claimed)*
- [~] Collect Objective-C class/metaclass names, superclasses, class/instance methods, selectors, complete method encodings, return/argument types and IMP ownership using target metadata and applicable libobjc APIs (`class_getName`, `class_getSuperclass`, `class_copyMethodList`, `method_getName`, `sel_getName`, `method_getTypeEncoding`, `method_getImplementation`). *(in progress — claimed)*
- [~] Recover properties and their attributes, declared types, getter/setter selectors, backing ivars and ownership qualifiers; recover ivar names, encodings, offsets, widths, alignment and inheritance boundaries using metadata plus `class_copyPropertyList`, `property_getAttributes`, `class_copyIvarList`, `ivar_getTypeEncoding`, and `ivar_getOffset` where runtime access exists. *(in progress — claimed)*
- [~] Import C-style function declarations and SDK definitions through the Apple Clang AST: typedefs, enums, pointers/function pointers, callbacks/blocks, structs/unions, field offsets, packing, alignment, bitfields, variadic prototypes and ABI attributes. Associate declarations with the actual providing image and retain aliases/source spelling. *(in progress — claimed)*
- [~] Use SDK/Clang record layouts, `sizeof`/alignment/offset probes, Objective-C encodings and Foundation size/alignment helpers to establish structure layouts and types. Runtime encodings seed layout evidence; missing field names or erased typedefs remain explicitly unresolved until headers/debug/type constraints supply them. *(in progress — claimed)*
- [~] Feed this catalog into function-boundary parsing, prototype/ABI recovery, selector/message-send reconstruction, class/property/ivar expressions and struct field access. Reuse libSystem, libobjc, Foundation/CoreFoundation, dispatch, Mach and XPC contracts and implementations where applicable. *(in progress — claimed)*
- [~] Record shared macOS/iOS qualification per layout/decoder/runtime contract, including version/architecture/ABI applicability. Run identical shared-format fixtures through the same dyld decoder; only the platform-specific deltas require separate qualification. *(in progress — claimed)*

Acceptance: native fixtures exercise images, C functions, classes/metaclasses, instance/class selectors, properties/ivars, C structs and ABI layouts. Their names/types/layouts agree with SDK/Clang/runtime ground truth. Catalog joins reject image/UUID/ABI mismatches; shared-format macOS/iOS fixtures yield the same semantic decoding. Every catalog-derived pseudocode construct links back to the record that supplied it.

## C01 — Exact semantics, effects, and executable machine-state C

Dependencies: C00. Extend semantics as each corpus slice requires; full ISA qualification continues throughout C02–C12.

Verified progress: integer SIMD-to-GPR lane extracts now use explicit shared effects across all 82 legal forms. Apple-assembled fixtures and 42,640 Apple-Silicon comparisons qualify normal EL0 non-streaming lane results. Lane-wise integer ADD/SUB/MUL plus MLA/MLS now have exact arrangement decoding, reserved-form rejection and modular lane folds; MLA/MLS effects include the prior destination, and 12 assembled forms pass 3,120 native comparisons. Supported vector FP three-same operations pass Apple-Silicon comparisons on 40 assembled half/single/double forms with finite normal operands; the test excludes NaN, signed-zero, FPCR and exception behavior. Structured effects tests qualify Q=0/Q=1 source widths and retain the aliased left operand as a read. The unsound broad SIMD preservation fallback is removed, and vector-bitwise decoding checks fixed bit 24. General SIMD/system effects, memory/flags SSA, strict emission and whole-function equivalence remain open.

Executable progress: the [worker machine-state slice](WORKER_RECONSTRUCTION_ARTIFACTS.md#executable-machine-state-slice) now interprets shared-tree integer/branch/checked-memory semantics and emits complete C17 for supported captured functions. Native C/Objective-C/assembly `-O0/-O2` fixtures qualify bounded returns and explicit buffers; interpreter/generated runs additionally agree on the full modeled register/memory state. Graph-linked compilation/differential observations remain separate from recovered-source fidelity and formal equivalence. General instruction/call/FP/exception coverage and canonical flags/memory SSA remain open; this does not close C01 or C10.

- [~] Inventory exact, intrinsic, privileged/runtime, and unsupported instruction families; finish shared ARM64/ARM64e semantics including vector/half FP, flags, partial lanes, SP, PAC, atomics, barriers, traps, and exception effects. *(in progress — claimed)*
- [~] Specify typed bitvectors, modular arithmetic, comparisons, shifts, extensions, poison/unknowns, endianness, memory alignment, FP modes, and architectural aliases. *(in progress — claimed)* The scalar floating-point file effect is now measured rather than assumed: every scalar form writes the whole destination register and leaves the bits above its lane zero, so no FPCR-dependent upper-bit merge is claimed and no dependency on the old destination is invented. The measurement, its scope and the model it supports are recorded in [AArch64 scalar floating-point file effects](AARCH64_SCALAR_FP_EFFECTS.md) and run as a qualification step on an Apple Silicon host. The remaining FP modes, exception behaviour and NaN propagation rules are open.
- [~] Build an IR interpreter and strict C17 emitter with checked guest-memory helpers and explicit imports/intrinsics. Preserve ordering and side effects. *(in progress — claimed)*
- [~] Connect the Darwin native support runtime and record reusable macOS/iOS semantic qualifications with explicit ABI/environment deltas. *(in progress — claimed)*
- [~] Differential-test semantics against hardware or a qualified independent engine; exercise reserved and neighboring encodings and architectural edge cases. *(in progress — claimed)*
- [~] Replace text reparsing in reconstruction consumers with structured effects. *(in progress — claimed)*
- [~] Render named/typed low-level pseudocode directly from the structured effects and C00N evidence before general source recovery is complete; keep expression source ranges and explicit unresolved effects throughout. *(in progress — claimed)*

Acceptance: eligible machine-state C recompiles and agrees with original execution on recorded tests; unsupported reachable effects fail eligibility. FP/atomic/PAC claims require their dedicated state/model tests.

## C02 — Function discovery and complete control-flow facts

Dependencies: C00 and C00N seed identity/boundaries; C01 effects complete the CFG for the supported slice. Initial function parsing proceeds using the existing decoder/IR while semantics expand.

- [~] Reconcile symbol, unwind, export, runtime, direct-call, prologue, thunk, tail-call, and cross-reference evidence for stripped/optimized code; retain competing boundaries and cold fragments. *(in progress — claimed)*
- [~] Canonicalize per-function CFG with reachability, dominators/post-dominators, loop forest, control dependence, exceptional exits, and unknown successors. *(in progress — claimed)*
- [~] Recover indirect destinations and switch data paths with bounds/range/index-normalization proof; retain explicit unresolved targets. *(in progress — claimed)*

Acceptance: known-source fixture CFGs agree, including disconnected/unreachable code, tails, exceptional paths, and optimized switch lowering; no missing path is silently treated as a function return.

## C03 — Register, flags, vector, and memory SSA

Dependencies: C01, C02.

- [~] Promote GPR SSA evidence to canonical structured definitions, uses, phis, and value IDs with W/X, SP, vector lanes, FP, and flags. *(in progress — claimed)* The bounded IR preview and the reconstruction derivation graph now carry a value identity for all three files: the general-purpose file, the NZCV file modelled bit by bit, and the vector file modelled byte by byte. A partial write keeps the identity of the part it preserved, a merge whose incoming values agree is not a merge, and a call clobbers exactly the caller-saved vector registers and the whole flag file. The published identities are still per-file rather than one canonical value namespace, and the FP/vector lane widths, memory versions and value IDs are not yet canonical.
- [~] Add memory SSA, call clobbers, ordered/volatile/atomic effects, and conservative alias partitions for frame objects, globals, constants, heap, and escaped/unknown memory. *(in progress — claimed)* A recovered function now publishes its proven frame objects as `memory-object` records carrying the slot extent, alias partition and reaching store. The bounded IR preview now renames memory versions along the CFG dominator tree and emits per-partition phis at joins, so a write in one branch cannot falsely reach its sibling by address-order traversal. Frame/global provenance survives proven 64-bit constant add/subtract; dynamic or ambiguous addresses stay unknown. Escaped/unknown reads and writes widen across the alias set, calls clobber each writable partition, and acquire/release markers remain attached to the actual ordered access instead of inventing writes to unrelated objects. Recovered-source read/write AST records now cite same-instruction memory SSA uses/definitions, and frame-object version records link to their reaching memory definition; a 544-function/configuration native matrix validates the new links and behavior. Using path-specific reaching versions/phis to drive source value flow, plus full loop/volatile qualification, remains open.
- [~] Prove frame adjustments and saved-register behavior; handle dynamic alignment, overlapping slots, spills, and lifetimes. *(in progress — claimed)*
- [~] Preserve instruction origins through SSA transforms, copies, merges, and dead-value elimination; bound fixed points and publish exhaustion reasons. *(in progress — claimed)* Verified progress: structured per-definition origin records now carry the relation between a definition and the origins it derives from — a same-width register move records the source register and origin, a phi records the distinct origins it joins, and a fresh definition records no relation. The architectural `MOV` aliases (`ORR Xd, XZR, Xm`, `ADD Xd, XZR, Xm`) are recognized as copies so the value identity survives the real decoder output. Dead definitions are marked but keep their origin, so eliminating a value never loses its provenance. The preserved chain is published in the semantic preview as `ssa-origin` rows and `ssa-merge` rows, and the root of a chain resolves back to the entry value or the real merge. The fixed point over the relation chain is bounded at 64 steps.

Acceptance: dominance, phi inputs, interference, def-use, memory versions, read-before-write, loops, aliasing, and unknown-effect fixtures agree with the reference model and traces.

## C04 — Calls, ABI, summaries, and the call graph

Boolean callee results: pass 49 reuses the bounded source walker to prove every
reachable return of an exact-prototype memory-free leaf is canonical before
passing the Boolean value into its caller AST. Artifacts retain that proof and
callee declaration. The native gate accepts a Boolean leaf/caller pair and
rejects a leaf returning 2 and its caller at all four optimization levels.
Changing the leaf's declared return type changes the settings digest and
invalidates the caller proof. Indirect, imported and effectful callees stay open.

Dependencies: C00N names/prototypes/layouts, C02 and C03; basic library types seed the solver before general C05 propagation.

- [~] Complete Apple AAPCS64 parameter/return recovery: GP/FP banks, stack values, homogeneous/arbitrary aggregates, varargs, hidden result/context values, returns, tails, and nonreturning calls. *(in progress — claimed)* Verified progress: the indirect result location is modelled as the dedicated x8 register, so a result that is a composite larger than 16 bytes and is not a homogeneous floating-point aggregate is reported as returned through that location, and the result location never shifts the argument registers. A homogeneous floating-point aggregate of at most four members stays in the floating-point registers whatever its size, so `CGRect` is still reported in d0-d3, and a larger composite argument is reported as passed by pointer in the register that carries the caller's copy. The rule is qualified against the Apple toolchain: a compiler-built fixture returns `CGAffineTransform` through x8 and passes it by pointer, and the prototype-ABI gate asserts both forms. Varargs, stack values, context values, tails, and nonreturning calls remain open.
- [~] Build typed direct/import/tail/thunk/indirect/dispatch call edges, confidence, candidate sets, and caller/callee navigation. *(in progress — claimed)* Verified progress: the typed call graph and the caller-visible call-effect summary now have workbench surfaces. Each proven edge type — `direct`, `import`, `dispatch`, `thunk`, `indirect`, `unresolved`, `tail` — is published with its own confidence and is reachable as a navigable evidence source, so a selection follows the edge back to the workspace that owns it. A register-indirect transfer is published as a candidate set rather than a resolved target. The call-effect surface states the caller-visible AAPCS64 contract per call site and keeps argument and result locations unresolved until a callee prototype is proven. The prototype-conflict surface publishes the bounded caller/callee constraint iteration: a disagreement between the caller side's observed claims and the callee side's proven prototype claims is shown with both claims kept, so neither side wins silently, and a report with no proven prototype on either side says so instead of claiming agreement. The new sources rank below the raw call graph in the bounded focus queue, so the queue is not filled with near-duplicate call rows. 
- [~] Compute effects, clobbers, memory, ownership/allocation, throws/nonreturn, arguments/returns, and target summaries; iterate caller/callee constraints with bounded convergence. *(in progress — claimed)* Reconstruction artifacts now publish a caller-visible AAPCS64 call-effect summary per call site: the caller-saved and callee-saved register classes, preserved `sp`/`lr`, NZCV clobbering, the call kind, the resolved target or dispatch register, the prototype-derived argument and result locations when a callee prototype is proven, and explicit unresolved markers for the callee effect summary and for memory reads and writes. An indirect call never claims a target and a call never suppresses a possible memory write. A direct call into the captured image additionally records a bounded observation of the callee: the registers it defines, NZCV writes, observed memory reads and writes, an observed return, onward calls, and whether an unmapped instruction or a truncated window leaves the observation incomplete. The observation is explicitly scoped, and an unobserved memory write never becomes a claim that the callee performs none. A direct call target is now proven not to return from the callee's own control flow — a backward branch or a cycle in its reachable instructions — and the proof is published as evidence; the non-returning claim is stated as a reason, so an unproven callee reads as unresolved rather than implying that it returns. The callee observation follows the callee's own reachable control flow instead of reading straight through the image, and it becomes a whole-function summary exactly when every reachable instruction decoded within the budget and every reachable path terminated. A complete walk with no onward call therefore proves the absence of a memory write, while an incomplete walk keeps the explicit unknown marker. The observation also records the flag and vector files the general-purpose effects omit. The remaining caller/callee constraint iteration and cross-function summaries stay open.
- [~] Preserve prototype conflicts and per-parameter evidence; treat names, signatures, targets, and implementations as separate claims. *(in progress — claimed)* Verified progress: a caller/callee constraint solver now iterates both sides to a bounded fixed point. The callee side states the type of each argument from a proven prototype and the caller side states the type the caller's own proven prototype gives that argument, both keyed by the register the argument travels in, because that register is the identity the two sides share across the call. A machine width observed at a call site is a separate claim kind and is never compared against a source type, so an observation never manufactures a conflict. When the two sides disagree, the conflict is reported with both claims kept; neither side is preferred and no proven type is rewritten from a weaker observation. A report with no proven prototype on either side makes no agreement claim at all. The iteration is bounded and hitting the bound is published as an exhaustion reason rather than hidden. Cross-function callee summaries and the remaining caller/callee constraint iteration stay open.

Acceptance: compiled ABI fixtures and execution agree on argument/return locations and call effects; caller constraints improve recovery without inventing indirect targets or suppressing unknown memory writes.

## C05 — Type library, layout, and constraint propagation

Dependencies: C00/C00N seed the library and layouts; iterate with C03/C04 for inferred types.

- [~] Import project/SDK C declarations, DWARF/dSYM, structs/unions/enums/typedefs/prototypes, and validated runtime types; persist source and version. *(in progress — claimed)*
- [~] Preserve native C declarations, Objective-C property/ivar attributes and actual compiler field layouts while propagating them into parameters, locals, fields, method declarations and expressions. *(in progress — claimed)*
- [~] Solve types bidirectionally across SSA, calls, returns, casts, loads/stores, and pointer arithmetic, separating machine representation, semantic type, and source spelling. *(in progress — claimed)* Recovered-source AST nodes now state their machine representation and their proven semantic type separately: a parameter binding and the returned expression carry the type the exact source declaration proves, a register-width view preserves the source type when it does not discard source bits; a truncating integer view carries its explicit unsigned cast type while pointer or unknown views remain unresolved, an arithmetic or bitwise result keeps the operand type only when both operands agree on it, a comparison yields a condition rather than a value, a copy carries its source type unchanged, a join of two return paths keeps the type only when both paths agree, and a computed expression over an unproven operand stays explicitly unresolved. Bidirectional solving across SSA, calls and pointer arithmetic remains open.
- [~] Infer stack/global/pointer-relative aggregate layouts and fields from width/offset/use/lifetime/alias evidence; retain incompatible layouts and alternatives. *(in progress — claimed)*
- [~] Recover signedness, nullability, mutability, constness, ownership, and lifetime only to the degree supported by evidence. *(in progress — claimed)*

Acceptance: held-out fixture types/layouts match the source oracle where determined; incompatible constraints remain inspectable and never disappear through silent casts.

## C06 — Apple object models, strings, and library idioms

Dependencies: C00N establishes class/selector/property/ivar facts early; C04/C05 add receiver, dispatch and data-flow inference.

Verified progress: target-local structured property/ivar layouts now feed declarations, object accessor types and bounded shared-IR-proven `self->ivar` accesses. Native-oracle fixtures qualify LP64 offsets and class/metaclass method lists; ambiguous receivers/layouts remain explicit. This does not complete general dispatch/receiver/type inference below.

- [~] Complete Objective-C class/metaclass/superclass/category/protocol/ivar/property/IMP relationships, selector stubs, receiver classes, and dynamic dispatch candidate sets. *(in progress — claimed)*
- [~] Render validated property/ivar access, class/instance method declarations and C-style prototypes with their owning image and type/layout evidence; retain property-accessor calls when an accessor rewrite has not been justified. *(in progress — claimed)*
- [~] Recover strings, pointer/string xrefs, CFString/CFData, globals, block captures, tagged pointers, bounds checks, ownership/runtime calls, and Foundation/CoreFoundation idioms from data flow. *(in progress — claimed)*
- [~] Connect runtime observations and their validated image mappings to static hypotheses without promoting temporal proximity to causation. *(in progress — claimed)*

Acceptance: metadata, message sends, receiver/argument types, data references, and idiom rewrites have graph evidence and preserve machine effects; dynamic alternatives remain visible.

## C07 — Source variables, expressions, and editable hypotheses

Dependencies: C03–C06.

Verified progress: parameter identity flow no longer reparses machine statements. Entry-based identities, width-specific views, bounded frame saves/restores, paired writes and CFG must-joins feed separate input/output naming maps. Six Apple-assembled fixture functions and 1,560 original-native result checks supplement the existing corpus. This is not complete SSA-backed source-variable recovery, typed expression AST or reconstructed-source behavioral validation.

Source progress: [named scalar recovery](WORKER_RECONSTRUCTION_ARTIFACTS.md#named-scalar-source-recovery) joins exact C/Objective-C declarations to structured ABI inputs, produces an instruction/declaration-linked expression DAG, folds proven private-frame copies, preserves proven pointer types through exact-width spills/reloads, handles acyclic return paths, and emits scalar C/C-style IMP adapters. The qualification matrix covers four optimization levels and symbol-rich/local-symbol-stripped fixtures, with native result comparisons and generated-source sanitization. Bounded reads through declaration-proven non-volatile pointer parameters support constant offsets and typed dynamic byte offsets; direct signed and unsigned 8-, 16-, 32- and 64-bit loads, constant element offsets, and dynamic byte and typed-element offsets pass native compile/differential qualification. Compiler-scaled index expressions retain the proven integer type through C shifts. Load AST types use exact fixed-width pointee declarations when widths match, including signedness that the machine instruction cannot encode for 64-bit loads; integer-promoted values retain C promotion types. Multiple external stores on a straight-line path remain ordered, acyclic branch guards preserve conditional writes with path-condition AST evidence, dynamic byte-index stores are qualified, and dynamic signed 16-bit and unsigned 32-bit element stores preserve index scaling, declared value type, access width, and surrounding memory. Declaration-typed volatile loads and stores are emitted as typed volatile C accesses in proven per-path instruction order, including schedules around supported scalar calls; straight-line and branch-guarded two-load fixtures plus a two-store fixture pass native qualification. Single-entry natural loops with complementary zero-test backedges reconstruct as typed `while` regions with explicit accumulator/induction locals. Two sequential register-only loops pass the existing matrix, and loop bodies now preserve declaration-proven external read/store order with memory SSA links; the native volatile accumulation loop exercises a load feeding a store and loop-carried value. Calls, frame effects, and broader control-flow shapes remain unqualified. Direct signed and unsigned 16-bit and 32-bit typed pointer stores pass with final surrounding memory checked, and their store-value AST nodes retain the declared scalar width and signedness. Mixed reads/writes preserve per-path instruction order for aliasing and disjoint pointers; store-then-load, load-then-store, and branch-guarded one-arm and two-arm stores followed by an alias-sensitive read pass native comparisons at all four optimization levels. Schedules that cannot prove per-path instruction order remain blocked. Volatile accesses retain per-path order for multiple exact-width declared accesses and schedules around supported scalar calls; a combined store-call-load fixture verifies all three event classes in one schedule. The first natural-loop slice preserves scalar loop-carried registers across sequential single-entry loops and their unique exits; one exact single-entry loop shape also emits ordered declaration-proven volatile stores per iteration. More complex loop read/write mixes, frame effects, calls, nested, post-tested, flag-carrying, and multi-exit loops remain open. Exact-prototype self-recursion and symbol-resolved pure C leaf calls are admitted under their respective recursion/callee proofs, including return and ABI callee-saved state preservation. Caller memory effects before and after proven calls now emit as guarded, instruction-ordered events on bounded acyclic paths. `hs_c_call_then_store`, call-then-load, call-load-store, two-arm branch-call/store and one-arm conditional-call/store fixtures, and exact self-recursion followed by caller stores pass native comparisons at -O0/-O1/-O2/-Os with symbol-rich and local-symbol-stripped inputs. Callee declarations remain in generated source, and calls continue to block strict machine-state C. Indirect calls, unresolved/effectful callees, loops, general SSA-backed lifetimes/layouts, modern chained Objective-C metadata joins, UI integration and broader source fidelity remain open; C07–C10 are not complete.

Memory provenance integration: recovered-source read/write AST nodes now cite the exact instruction-scoped memory SSA use/definition evidence, and frame-object version records link to the reaching memory definition. These links are validated across the 544-function/configuration native matrix; path-specific memory versions and phis do not yet drive source variable lifetime or value recovery.

- [~] Recover parameters, locals, aggregates, fields, globals, captures, lifetimes, and coalesced SSA variables; name from factual evidence or deterministic role descriptions. *(in progress — claimed)* A recovered-source local now carries a deterministic role name derived from its decoded operation — `hs_sum_N`, `hs_bit_xor_N`, `hs_shifted_N`, `hs_slot_N`, `hs_nzcv_N` and similar — instead of an opaque counter. The role comes from the decoded operation rather than the rendered text, and the original source spelling stays unrecovered.
- [~] Form expression trees with use counts, dominance, effect ordering, widths, casts, lvalues, array/field access, and defined overflow/shift semantics. *(in progress — claimed)*
- [~] Recognize induction variables, accumulators, state variables, and compiler temporaries with retained supporting facts. *(in progress — claimed)*
- [~] Persist analyst variable/type edits, split/merge, switch marking, and cross-function undo/redo; invalidate and recompute dependent hypotheses by analysis revision. *(in progress — claimed)*

Acceptance: regenerated expressions retain def-use and memory/call ordering; branch joins, alias conflicts, partial writes, and register reuse cannot leak an old source identity.

## C08 — Nested source control flow

Boolean loop invariants: pass 48 seeds a canonical {0,1} domain only from a
declaration-typed Boolean initializer and requires every modeled loop update to
preserve it. The native counted-toggle fixture passes at four optimization
levels for zero/odd/even iteration counts. Its noncanonical-update companion
is rejected. Loop constants are not assumed invariant across iterations;
general loop/phi solving and broader control flow remain open.

Dependencies: C02–C05, C07.

- [~] Build typed regions for nested conditions, short circuit/ternaries, guards/early exits, loops, multi-exit regions, switches/defaults/fallthrough, tails, and supported exception constructs. *(in progress — claimed)*
- [~] Use induction/bounds evidence for `for` and normalized switch indices; represent irreducible regions explicitly. *(in progress — claimed)*
- [~] Regenerate and compare CFG and effects from the source AST after each structural transform. *(in progress — claimed)*

Acceptance: nested and optimized fixtures retain all reachable exits and side effects; no extra edge or removed case is justified solely by a pretty rendering.

## C09 — Reconstructed source export and pseudocode round trip

Boolean source recovery now admits bool/_Bool parameters and returns only when
structured operation facts prove canonical {0,1} representations. Native
identity, negation, AND and comparison fixtures pass all four optimization levels;
noncanonical and unproven returns are rejected. The expanded focused matrix
covers 52 function/configurations, 200 native comparisons and 14 linked recovered
functions. Pass 47 and prototype/cache revision 12 identify the new semantics.

Recovered headers: supported scalar C and C-style Objective-C IMP functions
now retain a worker-produced recovered-header artifact with exact parameter
and result types, provider includes, function bounds and declaration evidence.
The Source output menu exposes it as Recovered header (.h), with identity-bound
export. Headers use C linkage guards for C++ consumers. The 36-configuration
void/call matrix parses every header as C and C++ and compiles the generated
implementation against its header before native comparison. Broader C++/Swift
declaration recovery and whole-image type dependencies remain open.
Scope-export audit: removed generic argument-list declarations and the invented
Objective-C wrapper from the whole/address/section export surface. Exports now
include the current validated worker header or recovered body when its exact
bounds belong to the requested scope, with unresolved discovered functions
listed as comments. C++/Swift language translation remains explicitly unresolved;
their current output retains C/Objective-C evidence as comments. Scope exports
remain bounded partial exports; automatic whole-binary reconstruction is open.
The CLI batch path now processes every discovered function or an exact range
via scripts/reconstruct-binary.py, retaining headers, bodies, machine-state C
and evidence with a hash/coverage/blocker manifest. The native fixture verifies
11 discovered functions (10 recovered, one review-only), file hashes and exact
range selection. App-driven batches, resume and whole-image dependency/link
qualification remain open.
CLI resume now publishes per-function atomic checkpoints and verifies source,
worker, prototype and scope identities plus retained file hashes before
continuing. Native integration tests grow a one-function batch to all 11
discovered functions, verify the retained file's modification time is unchanged,
and reject a changed artifact without overwriting it. App resume and whole-image
dependency/link qualification remain open.
App batch implementation: the Source pane now offers whole-binary, verified
current-code-section and address-function batches, with sequential coordinator
requests, per-function source/function/revision validation, cancellation between
functions, retained outputs and a hash/blocker manifest. Batches enforce 4096
functions and 256 MiB output, refuse existing output directories, and stop when
the project, record, analysis revision or service readiness changes. App target
typechecking and existing review contracts cover wiring; interactive batch and
failure/reconnect qualification still require end-to-end evidence. Resume and
whole-image dependency/link qualification remain open.
Native batch publication now uses the shared validated-artifact exporter rather
than app-local filename/hash logic. It preflights all output paths and byte limits,
rejects duplicate output kinds and symlink directories, and refuses overwrite.
The app runs hashing/writes off the main actor. Native contracts exercise budget
rejection without partial output, file-hash agreement and overwrite/symlink refusal
on worker-produced artifacts, including a recovered Boolean loop/header fixture.
The real test app now imports a three-function captured Mach-O, runs the same
batch routine through CoordinatorService/XPC and the bundled worker, validates
the artifacts with the native decoder, exports them and independently verifies
each file hash and bound source/function identity. The full Xcode build and HTTPS
client transaction pass. Pre-cancelled and stale-context batches export no
function files and clear active/busy state. Manual menu interaction, in-flight
cancellation and service failure/reconnect remain unqualified.
App checkpoint/resume is now implemented: the Source menu accepts an existing
batch directory, verifies source/worker/pass/cache identities, project/record and
function-plan consistency, then checks retained artifacts and all source files
against their hashes and validated native artifact content. Per-function atomic
checkpoints preserve completed work. The real Xcode/app/XPC/worker gate resumes
a cancelled three-function batch and verifies resulting exports. Native contracts
reject a changed worker and tampered retained artifact. Manual menu interaction,
mid-request cancellation and restart/reconnect remain
separate qualification work; recorded function failures remain failures on resume.
Partial-progress recovery now passes the real app/XPC gate: the batch cancels
after its first published function, resumes with one retained entry, finishes
the remaining two functions and preserves the original file modification times.
Independent checks verify all final hashes, function counts, retained count and
cancel/busy/request cleanup. This qualifies cancellation between functions;
mid-worker cancellation and recovery across app/service restarts remain open.

Void-function reconstruction (2026-10-22): exact C/Objective-C void declarations
now admit the existing scalar recovery path without requiring an x0 result.
Empty functions, ordinary pointer setters and conditional setters compile and
agree with their originals at -O0/-O1/-O2/-Os. The expanded matrix includes a
void caller discarding a proven scalar leaf's result: 16 function/configuration
pairs and 48 native comparisons, with surrounding memory checked under UBSan.
Discarded calls survive as guarded executable AST events and generated C
statements; the fixture checks exactly one call event and invocation.
The same scheduling now applies to all supported scalar callers: discarded
results retain the call and repeated expression uses share one result local.
The focused matrix covers 24 function/configuration pairs and 56 native
comparisons; the full recovered-source matrix still passes 544 function/config
pairs and 100,640 native comparisons after this scheduling change.
SP/LR and nonvolatile restoration checks still apply. Calls to exact-prototype
void leaves now emit standalone guarded statements when the existing callee
walk proves return, ABI preservation and no external memory effects or onward
calls. Their AST nodes have void type, no result local is invented, and all
caller-saved registers including x0 remain unknown after the call. The focused
matrix covers 36 function/configuration pairs, 88 native comparisons and four
rejections of attempts to use a void call as an x0 result. The
void-call fixtures include a conditional call and a call before a pointer
store, checking guarded AST events and generated call/store instruction order.
The void call's AST type and machine-type attribute both state void.
Effectful/imported
void callees remain open under the existing callee-summary requirements. The dedicated
scripts/test-recovered-void.py gate runs in the decompilation CI workflow.
Call evidence now represents an exact zero-argument prototype as an empty
argument list, and a void result as a proven void type without a result
register. Missing prototypes retain their unresolved markers and conservative
memory effects. Regression tests cover both cases independently.

Dependencies: C04–C08 and the machine-state emitter from C01.

- [~] Emit deterministic compilable recovered C17/Objective-C from the typed AST, with declarations, dependencies, globals, runtime adapters, helpers, ABI annotations, and source maps. *(in progress — claimed)*
- [~] Define/parse the restricted pseudocode grammar with typed diagnostics and explicit arithmetic/memory semantics; generated supported text round-trips to the same AST. *(in progress — claimed)*
- [~] Offer readable pseudocode, machine-state C, and recovered source with independent completeness/validation labels and exact unresolved dependency manifests. *(in progress — claimed)*
- [~] Preserve packing, alignment, volatility, atomic ordering, signed behavior, FP assumptions, and authentication effects; reject export where a reachable dependency is unresolved. *(in progress — claimed)*

Acceptance: exported projects compile under recorded flags and their address/AST/evidence maps validate; no silent stub, zero return, or invented framework behavior makes a function appear complete.

## C10 — Behavioral validation and reconstruction fidelity

Direct dependency closure: CLI range batches can now include exact discovered
callees outside their seed range from structured IR call targets. The bounded
queue deduplicates cycles, persists across resume and records unresolved targets.
Native integration verifies a single caller range cannot link alone, then
includes its leaf, compiles and links both recovered functions, and resumes
without resubmitting completed work. Imported/indirect dependencies and general
whole-image dependency completeness remain open.

Batch build qualification now verifies all manifest file hashes, compiles recovered
C/Objective-C functions independently and links them with undefined symbols
rejected. The native batch fixture links 10 recovered functions, retains one
unrecovered function in its report, and executes test-owned setter/call-result
checks against the library. A missing-leaf fixture requires linker failure.
The report records the manifest hash and compiler identity. Arbitrary whole-image
dependency completion, ABI breadth and app-integrated build qualification remain open.

Dependencies: C00/C01; expand continuously with C02–C09.

- [~] Execute original and generated code with matched ISA, ABI, inputs, mapped memory, deterministic imports, and bounds; compare returns/registers, memory writes, traps, and call events. *(in progress — claimed)*
- [~] Exercise integer/FP boundary cases, invalid memory, random seeds, aliasing, and supported concurrency/atomic models. *(in progress — claimed)*
- [~] Add bounded SMT equivalence for supported pure/region models, including assumptions, timeouts, and counterexamples. *(in progress — claimed)*
- [~] Score recovered AST structure, variables/types/names, CFG/def-use/effects, recompilation, and tested behavior separately by language/optimization/feature/compiler. *(in progress — claimed)*
- [~] Store input/toolchain/backend/test identities and results as graph evidence; enforce per-category regression thresholds in CI. *(in progress — claimed)*
- [~] Reuse macOS qualification for shared AArch64, Mach-O/dyld, native type/layout and Darwin runtime semantics on iOS; report the tested common foundation and the remaining platform/version/ABI deltas separately. *(in progress — claimed)*

Acceptance: zero unexplained behavioral mismatches in the qualification matrix. A differential pass reports its tested domain; a formal result states its proof model and assumptions. Compilation alone never grants equivalence.

## C11 — Evidence-centered source review

Dependencies: C00; extend alongside C03–C10.

- [~] Synchronize source/AST, types, SSA/memory SSA, CFG, calls, assembly, bytes, and runtime observations; one selection follows all contributing instruction spans. *(in progress — claimed)*
- [~] Expose why/alternatives/conflicts/assumptions/blockers and variable uses at expression granularity. *(in progress — claimed)*
- [~] Add per-function reanalysis, analysis bounds, simplification controls, cancellation/progress, and retained completed work. *(in progress — claimed)*
- [~] Export/import typed signatures, types, call graphs, annotations, AST/evidence graphs, manifests, and hashes; qualify GUI import and native adapters. *(in progress — claimed)*

Acceptance: multi-instruction expressions navigate to every origin; constants/call targets never steal the source selection. Edits trigger the correct dependent work and cancelled results retain accurate completeness.

## C12 — Reconstruction qualification and language expansion

Dependencies: C00–C11 for the first supported slice.

- [~] Qualify C and Objective-C on ARM64/ARM64e Mach-O under the full corpus, execution, fidelity, provenance, resource, and recovery gates. *(in progress — claimed)*
- [~] Extend C++: Itanium ABI, receiver/aggregate recovery, vtables, templates/debug types, exceptions, thunks, and optimized fixtures. *(in progress — claimed)*
- [~] Extend Swift: versioned ABI, signatures, metadata, generics/existentials, ownership, closures, async and runtime adapters with dedicated fixtures. *(in progress — claimed)*
- [~] Add the x86-64 Mach-O lifter, ABI, shared analyses and calling-convention/differential tests through the same reconstruction pipeline. *(in progress — claimed)*

Acceptance: each language/ISA declares and passes its own supported matrix. Structural ingestion of a new ISA does not qualify reconstructed source.

## C13 — Format and platform completion

Dependencies: C00/C01 contracts; format work serves reconstruction after the first qualification milestone.

Linux static-processing packaging: [Docker worker](LINUX_DOCKER.md) adds a
bounded one-shot Rust/C container and amd64/arm64 CI gates. This does not close
Linux native attach or the app-hosted remote server integration gates.

- [x] ELF sectionless dynamic linking and relocations (`PT_DYNAMIC`/`DT_*` evidence for images without section headers): bounded `DT_RELA`/`DT_REL`/`DT_JMPREL` relocation tables, `DT_HASH`/`DT_GNU_HASH`-proven symbol counts, and `DT_SYMTAB` names/imports/exports/function starts; `DT_RELR` remains unapplied.
- [~] Qualify Linux native attach with its process/ABI/image identity model (environmental gate; requires a Linux qualification host). *(in progress — claimed)*
- [x] PE imports, exports and base relocations from data directories (DLL/symbol names, ordinals, export addresses and relocation sites). Standalone COFF objects, PDB and Authenticode remain separate.
- [x] Standalone COFF objects (COFF header, section table, symbols and relocations for `.obj` inputs).
- [x] PDB validation (MSF container, stream discovery and type/line/symbol evidence for Windows debug databases).
- [x] Authenticode validation (PE security directory, PKCS#7/CMS signed data structural validation and certificate metadata).
- [~] Complete broader native project/type/comment/signature interoperability, GUI import, and live Ghidra/IDA/Binary Ninja adapter qualification. *(in progress — claimed)*

Acceptance: malformed/overflow/ambiguous fixtures fail with bounded diagnostics; live and static mappings retain exact identity; round trips preserve supported artifacts and conflicts.

## C14 — Investigation and team completion

Dependencies: C00 identities, C11 artifacts, C15 persistence/protocols.

- [~] Shareable investigation bundles with content hashing and replayable experiment recipes. *(in progress — claimed)*
- [~] Findings, review/comment state, evidence links, and reproducible experiment records. *(in progress — claimed)*
- [~] Extension signatures, capability policy, allowlists, trust and organization settings. *(in progress — claimed)*
- [~] Optional on-premises execution node with versioned authenticated headless jobs and returned evidence identities. *(in progress — claimed)*

Acceptance: bundle tampering and dependency changes are detected; team assertions retain authorship; extension and remote execution permissions enforce their declared capabilities.

## C15 — Persistence, resource, and recovery prerequisites

Reconstruction compatibility: producer and native consumer now require pass 55
for void/Boolean semantics, exact call scheduling, recovered headers and Boolean
loop invariants, proven Boolean callee results and Boolean pointer access.
Boolean memory reads retain an explicit canonical-object precondition; stores
require canonical values. Pointer depth and qualifier ownership remain checked.
Pass 51 retains volatile pointee types through proven offset calculations;
the final access still enforces its exact type and event ordering.
Pass 52 emits supported exact declaration headers independently of body
eligibility, retaining explicit declaration-only status for blocked functions.
Pass 53 extends exact scalar pointee typing to admitted native Apple LP64
integer spellings. Pass 54 adds declaration-constrained constant stores and
structured constant-bit propagation. Pass 55 adds declaration-proven integer
store conversions and exact unsigned types for narrowed integer register views.
The native artifact contract rejects pass-54 documents;
prototype/cache revision 12 prevents reuse of earlier call-ABI analysis output.
This versions the changed behavior;
general checkpoint/schema migration acceptance remains open.

Remote integration companion: [app-hosted server contract and client-readiness/push gate](HINDSIGHT_REMOTE.md). The HindSight application owns the heavy work and streams results to authenticated clients. Client repository/branch placement is deferred until server completion; no remote-server completion is claimed here.

Dependencies: existing project/cache/job model. Implement affected portions with each reconstruction pass.

- [~] Version project/analysis schemas and tested migrations beyond v1; stable documented SDK/target protocol and explicit compatibility negotiation. *(in progress — claimed)*
- [~] Add worker checkpoints, coordinator recovery, resumable long jobs, atomic publication, and dependency-based invalidation. *(in progress — claimed)*
- [~] Enforce CI regression budgets for memory, latency, work/cardinality, queue/backpressure, cancellation, deterministic output, and the 8 GiB profile. *(in progress — claimed)*

Acceptance: interruption/corruption/migration tests preserve completed evidence and reject incomplete publication; checkpoint replay agrees with a clean run; measured supported workloads meet their published budgets.

## C16 — Distribution and product completion

Dependencies: reconstruction qualification, C13–C15 and all applicable release gates.

- [~] Official macOS Apple-Silicon (arm64) distribution; Intel support is experimental. Updater/rollback, entitlements and update authenticity remain open. Credentialed signing/notarization validation is intentionally deferred until further notice.
- [~] Licensing tiers that preserve offline/local analysis and documented license-state recovery. *(in progress — claimed)*
- [~] Guided labs, redistributable datasets, onboarding, complete documentation, and enterprise deployment material. *(in progress — claimed)*
- [~] Re-run applicable original 0.1, 0.5, 1.0 and native execution gates against current arm64 artifacts. Intel/universal builds are experimental and non-blocking. Credentialed signing/notarization and manual release-host qualification are intentionally deferred until further notice.

Acceptance: clean-install/update/offline/recovery tests and reproducible lab walkthroughs pass on officially supported macOS arm64 hosts. Credentialed release validation remains deferred until the user re-enables it; no signing credentials are required for current reconstruction work.

## Traceability: every remaining original checklist item

An original checkbox closes only when all mapped work and its stated gate are proven. `DG` refers to `DECOMPILER_GAP_ROADMAP.md`; `RM` to `ROADMAP.md`. Baseline and aggregate rows deliberately overlap detailed rows.

| Original remaining requirement | Master dependencies |
| --- | --- |
| DG general source-variable, stack-local, type recovery (in progress) | C03–C07, C09–C12 |
| DG general C control flow, switches, architecture-independent decompilation | C01–C03, C08–C12 |
| DG complete shared IR coverage | C01, C10, C12 |
| DG replace machine-register temporaries | C03–C07, C09–C11 |
| DG structure common control flow | C02, C08–C10 |
| DG improve call-site reconstruction | C03–C06, C09–C10 |
| DG keep source mapping in the UI | C00, C07–C11 |
| DG recover function signatures | C04–C06, C10, C12 |
| DG infer stack locals and aggregate layouts | C03, C05, C07, C10 |
| DG add project type library | C05, C11–C12, C15 |
| DG model Objective-C dispatch and objects | C04–C06, C10–C12 |
| DG add type propagation and casts | C03–C05, C07, C10 |
| DG add analyst overrides | C00, C07, C11, C15 |
| DG recover data references and strings | C03, C05–C07, C10 |
| DG recognize library idioms | C04–C07, C10 |
| DG recover switches and indirect control flow | C02–C04, C06, C08, C10 |
| DG add another architecture through IR | C01, C10, C12 |
| DG handle stripped and optimized code | C00–C10, C12 |
| DG add function/call graphs | C02, C04, C06, C11 |
| DG add per-function controls | C00, C11, C15 |
| DG expose uncertainty and alternatives | C00, C04–C08, C11 |
| DG resource/cancellation controls | C00, C11, C15 |
| DG export standard artifacts | C09, C11, C13 |
| RM ELF sectionless linking/relocations and Linux attach | C13 |
| RM COFF, PE linkage/relocations, PDB, Authenticode | C13 |
| RM architecture abstraction beyond ARM64 | C01, C12 |
| RM reversing artifact interop, GUI, live adapters | C11, C13 |
| RM P1 Types aggregate | C03–C07, C10–C12, C15 |
| RM P2 Coverage aggregate | C01–C08, C10–C12 |
| RM shareable investigation bundles | C00, C11, C14–C15 |
| RM comments/review/findings/evidence/recipes | C11, C14–C15 |
| RM extension policy/signatures/allowlists/org settings | C14–C15 |
| RM on-prem execution node | C14–C15 |
| RM stable project schema/migrations | C15 |
| RM stable SDK/target protocol | C15 |
| RM crash recovery/resumable long jobs | C15 |
| RM performance regression budgets | C10–C11, C15 |
| RM signed/notarized distribution/updater | C16 |
| RM offline-compatible licensing tiers | C16 |
| RM documentation/labs/datasets/enterprise material | C00, C10, C14, C16 |

## Next executable work

Execute the [current actionable queue](#current-actionable-queue-2026-10-18) in order.
Its early steps deliver a function end to end; the remote gateway and conditional
GitHub push retain the explicit acceptance checks in
[HINDSIGHT_REMOTE.md](HINDSIGHT_REMOTE.md).
