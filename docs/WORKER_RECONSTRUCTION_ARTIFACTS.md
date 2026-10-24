# Worker reconstruction artifacts: first transport slice

The app now exposes **Reconstruction** in the main menu, with explicit
**Dump C (.c)**, **Dump Objective-C (.m)**, **Dump headers (.h)** and **Dump native
(.n)** actions for the whole discovered binary, verified current section or a
function at an entered address. These run the sequential app/XPC reconstruction
batch and create a `dump-c`, `dump-m`, `dump-h` or `dump-n` subfolder alongside
retained per-function evidence and resumable checkpoints. **Dump existing batch**
exports a previously validated app checkpoint to a new directory without
re-running reconstruction. Missing representations are counted as unavailable,
not substituted. The real app/XPC gate exports all four formats, independently
verifies artifact identity, exact file hashes and unavailable counts, and
continues to pass batch cancellation/resume checks. Manual menu and save-panel
interaction remains unqualified.

App checkpoints retain the requested dump format and original scope. Resuming a
cancelled batch automatically finishes that format's dump after reconstruction;
conflicting format requests are rejected without changing the checkpoint.
Existing dump destinations are preserved. The real app/XPC gate verifies native
dump intent across cancellation/resume, output hashes and retained-file identity.

The CLI counterpart is `scripts/export-reconstruction-batch.py BATCH NEW_OUTPUT --format c|objc|header|native`.
Optional `--start/--end` bounds or `--address`
select retained functions. Its export manifest binds the input manifest hash,
source identity, chosen representation, exact output hashes and unavailable
functions; it does not claim complete binary discovery or equivalence. Tests
also preserve worker/prototype digests, source size and original batch scope in
the dump manifest. Portable CLI tests exercise review-only/failed output without
inventing source, foreign rehashed graphs, refusal-before-publication and
preservation of existing destinations. Native fixture tests
export 25 C/Objective-C bodies, 31 headers and 21 native outputs, syntax-check
the `.n` files as C, and reject tampering, overwrites, empty scopes and exhausted
budgets before publishing a destination. C++/Swift source recovery remains open.

Batch qualification now preflights retained reconstruction graphs, captured
source identities, function bounds, recovery states and exact exported bytes
before compiling any source. Merely updating a changed file's manifest hash
does not bypass the retained artifact's content identities. Complete entries
must retain all generated outputs; failed entries remain explicitly unrecovered.
Compilation/header checks use a private snapshot of the verified bytes, not
mutable input paths. Reads enforce per-file and aggregate byte budgets.
Tests reject seven rehashed body/header/state/identity/bounds/graph/omission
mutations and verify that a later input-body change cannot alter compiled bytes.
This establishes artifact/export consistency, not producer authentication or
behavioral equivalence with the original image.

Scope review exports select an exact validated representation: `.h` requires
`recovered-header`, `.c`/`.m` require `recovered-source`, and `.n` requires
`machine-state-c`. Missing output remains an explicit unresolved comment;
another artifact kind is never substituted merely to fill the requested file.
C++/Objective-C++/Swift review exports retain recovered C/Objective-C bodies
as commented, untranslated evidence rather than claiming language recovery.
Native format-selection contracts pass for all seven menu formats, including
missing-kind and reordered-source cases. Real app/XPC format rendering checks
pass alongside batch/hash/cancellation/resume tests; the final gate additionally
requires actual native machine-state source. The save panel explicitly allows
the custom `.n` extension. Manual save-panel interaction remains unqualified.
This UI/export correction does not change worker pass 55.

Pass 61 prevents unsupported array declarators from silently flattening into
scalar pointers. A single unsized or positive decimal-sized scalar array still
decays to its parameter pointer. Multidimensional arrays, pointer arrays, VLA
bounds and trailing syntax remain unresolved rather than producing false header
or body declarations. A regression test reproduced the old multidimensional
flattening before the fix and verifies both declaration and body eligibility.

Pass 60 extends scalar pointee recovery to exact Apple SDK integer typedefs:
`NSInteger`, `NSUInteger`, `CFIndex`, `CFTypeID`, `CFOptionFlags`,
`CFStringEncoding`, `CFNumberType`, `CFURLPathStyle`, `CFPropertyListFormat` and
`dispatch_once_t`. Their declared spelling, width and signedness are preserved
in typed memory AST nodes and volatile helpers rather than replaced with a
potentially incompatible integer pointer type. Foundation-dependent cases use
Objective-C; non-Foundation generated implementations remain compiled as C.
This does not infer arbitrary typedefs, atomic/dispatch-once synchronization,
aggregate memory semantics or general SDK behavior.
The expanded mixed-C/Objective-C native gate passes 648 function/configuration
cases and 59,400 comparisons over `-O0/-O1/-O2/-Os`. SDK width/signedness static
assertions, exact AST/helper types, C++/Objective-C++ header syntax, UBSan,
indexed load/store/exchange/narrowing and surrounding-memory checks all pass.

Pass 59 adds structured `languages` metadata to declaration-only headers. Native
decoding validates it, Source review displays it, and both app and CLI format
manifests retain it without reparsing emitted source. Foundation-only headers
carry Objective-C/Objective-C++; C-compatible headers list all four C-family
languages. Legacy CLI evidence without the field remains unknown, not inferred.
Portable tests reject empty, duplicate, unknown and wrongly typed language
contracts even with valid recomputed graph IDs and file hashes.
Batch qualification consumes that contract and syntax-checks every listed
language, recording individual `header_checks` with language, standard, exact
header hash and metadata origin. Legacy headers retain the previous two-language
Objective-C/Objective-C++ check, labeled `legacy-qualification-default`, without
claiming plain-C compatibility. The native SDK header gate verifies all 152
per-language qualification records in addition to its direct syntax checks.

Pass 58 adds declaration-only headers for the known Apple SDK aggregate types
`NSRange`, `CFRange`, `CGPoint`, `CGSize`, `CGRect` and `CGAffineTransform`, with
native SDK includes rather than invented struct definitions. Foundation headers
explicitly require Objective-C/Objective-C++; CF/CoreGraphics headers remain
C-compatible. The expanded native gate passes 40 configurations, 152 successful
C-family syntax checks and whole-batch exports at four optimization levels.
Eight negative compilation checks verify Foundation's explicit language guard.
Aggregate implementations, arbitrary user structs, C++ and Swift recovery are
not implied.

Pass 57 separates safe scalar declaration recovery from body register eligibility.
Exact `float`, `double`, `long double` and stack-passed scalar argument declarations
now produce `.h` output even while the body remains unsupported. The declaration
model carries no invented register locations and headers retain their
`declaration-only-uncompiled` status. Unknown types, duplicate/reserved names,
variadics and C++ declarations remain rejected. The new native header gate checks
four prototype shapes at four optimization levels with C17, C++17, Objective-C
and Objective-C++ syntax checks. This does not qualify FP/stack body recovery,
general aggregates, C++ or Swift headers.

Pass 56 admits explicit signed integer declaration spellings (`signed short
int`, `signed long`, `signed long int`, `signed long long`, and `signed long
long int`) throughout ABI classification, source signatures and scalar pointee
recovery. Generated declarations and volatile accesses retain the exact spelling,
width and signedness. The expanded native matrix passes 408 configurations and
37,400 comparisons across four optimization levels, including typed loads,
stores, exchanges, zero/all-ones writes and wide-value narrowing, with C++ header
syntax checks, UBSan and surrounding-memory comparisons. This is not general
typedef, aggregate or C++ body recovery.

Pass 55 emits explicit integer conversions for stores whose source and pointee
types are both proven, including truncation and signed/unsigned reinterpretation
under the qualified Apple LP64 representation. A narrowed register view of a
declared integer carries its explicit unsigned cast type rather than borrowing
the original wider spelling. Pointer and unresolved values remain ineligible
for this conversion, and Boolean stores still require canonical values.
The native gate adds wide-value stores to every tested scalar pointee type,
including values with nonzero discarded high bits. Volatile helpers dereference
the actual declared integer type, not an equal-width typedef that may be
C-incompatible (such as `long long` versus `long`). Distinct native type helpers
cannot collide within one function. All 362 worker tests and strict Clippy pass.
The native pointee matrix passes 288 configurations and 26,400 comparisons,
including exact volatile helper pointer-type checks. Source validation passes
222 tests (six optional skips) plus native contracts, and both app type-check
targets pass. Full source regression passes 544 configurations and 100,640
native comparisons. The existing focused gate passes 92 configurations and
4,176 comparisons, and all 16 worker/native artifact fixtures pass.

Pass 54 propagates exact integer store types into proven constant bit patterns,
enabling declaration-typed volatile zero/all-ones stores. Constants retain
structured bit-pattern evidence through modeled register-width views and exact
copies; this proof no longer reparses rendered C literals. Unknown expressions
do not gain a type from the store, and Boolean writes still require canonical
0/1 values. Narrowing follows the qualified Apple LP64 integer representation.
The expanded native pointee gate adds zero/all-ones stores and checks the stored
AST type as well as native results and surrounding memory. It passes 240
configurations and 20,400 native comparisons. All 360 worker tests and strict
Clippy pass; source validation passes 222 tests (six existing optional skips)
plus native contracts, and both app type-check targets pass. The full source
regression passes 544 configurations and 100,640 native comparisons; the
existing focused gate passes 92 configurations and 4,176 comparisons, with
all 16 worker/native artifact fixtures passing.

Pass 53 retains declared native C integer pointee spellings, widths and
signedness alongside fixed-width types. Native `char`/`short`/`int`/`long`/
`long long`, their admitted unsigned/signed spellings, and LP64 integer typedefs
can now feed exact-width volatile accesses without discarding type evidence.
This uses the default Apple LP64 ABI, including signed plain `char`; alternate
compiler character-signedness modes are not qualified. Pointer depth, qualifier
ownership, exact volatile widths and ordered-access checks remain unchanged.
The new `scripts/test-recovered-pointees.py` gate passes 144 configurations and
18,000 native result/surrounding-memory comparisons over 12 representative
spellings at four optimization levels. Generated headers parse as C++, and
generated implementations compile against them with warnings-as-errors and
undefined-behavior sanitization. 358 worker tests and strict Clippy pass.
The full source matrix passes 544 configurations and 100,640 comparisons;
the existing focused matrix passes 92 configurations and 4,176 comparisons.
All 16 worker/native artifact fixtures, both app type-check targets, and source
validation (222 tests, six existing optional skips, plus native contracts) pass.

Scope selection is shared by batch reconstruction and single-file review export.
Current-section export now requires a verified four-byte instruction page and
includes only functions completely contained in its checked section bounds;
it no longer relabels the entire discovered-function list as one section.
Address actions prompt for a hexadecimal or decimal captured-image address,
require one unambiguous containing function, and treat function ends as exclusive.
Missing bounds, overflow, invalid addresses and scopes above 4,096 functions
are rejected rather than silently falling back or truncating. Native scope
contracts and arm64/experimental-Intel app type-checks pass. The real app/XPC
gate verifies section/address selections against a force-verified instruction
page and passes batch export/hash and cancellation/resume checks. Manual
address-dialog interaction remains unqualified.

Pass 52 separates declaration headers from body recovery. A unique supported
exact signature emits a declaration-only `.h` even if source reconstruction is
rejected or reachable effects block it. The header links to declaration evidence,
does not claim a recovered AST body, and explicitly disclaims implementation
and equivalence recovery. Missing/conflicting/unsupported declarations still
produce no fabricated header. All 24 body-rejection cases retain headers that
parse as C and C++; a trap-blocked header-only artifact passes native decode,
export and checkpoint checks. Batch qualification also syntax-checks every
header as Objective-C and Objective-C++, independently of body status.
The fixture batch passes all 31 header checks and compiles/links 25 recovered
implementations while retaining six review-only entries. The full pass-52
source matrix passes 544 configurations and 100,640 native comparisons.

Pass 51 preserves declaration-proven pointer types through constant and typed
dynamic byte offsets even when the pointee is volatile. Address calculation
is not itself a volatile access; the eventual access still requires an exact
scalar width/type and participates in the existing per-path event schedule.
Native indexed Boolean load/store/exchange fixtures cover every five-element
Boolean array pattern, all in-bounds indices and both replacement values,
checking returns and the entire surrounding object. The focused native gate
passes 92 configurations, 4,176 comparisons and 20 invalid-result rejections;
batch qualification compiles and links 25 recovered functions, leaving five
review-only. All 16 worker/native-decoder artifact fixtures pass. The full
regression passes 544 configurations and 100,640 native comparisons.

Pass 50 reconstructs declaration-proven Boolean pointer loads, stores,
exchanges and volatile accesses. Reads explicitly require live Boolean objects
with canonical 0/1 representations; that precondition is retained as an
assumption in the artifact and generated source. Stores require a proven
canonical value, rejecting noncanonical writes. Scalar pointee recovery now
distinguishes pointee qualifiers from pointer qualifiers and rejects pointer
depth greater than one instead of treating it as a scalar pointer.
Qualification: 357 worker tests and strict Clippy; 80 focused native
configurations with 336 comparisons and 20 rejected invalid cases; 22 linked
batch functions; 16 worker/native-decoder artifact fixtures; and the full
544-configuration recovered-source matrix with 100,640 native comparisons.

Pass 49 joins proven Boolean callee results into caller source expressions.
The complete memory-free leaf summary must pass, and the same source walker
must prove canonical returns within validated captured bounds. Call artifacts
retain the declaration and proof, while noncanonical leaves/callers are blocked.
Relevant direct-callee prototype candidates now bind the settings digest;
changing a callee declaration changes that identity. The focused native gate
covers 60 configurations, 256 comparisons, 16 rejected cases and 17 linked
recovered functions. Indirect/effectful/imported Boolean callees remain open.

The macOS app's Source pane offers sequential scope reconstruction through its
coordinator. The real app integration gate builds a three-function fixture,
imports it, reconstructs all functions through XPC and the bundled worker,
validates native review artifacts and verifies the exported manifest/files.
It also checks pre-cancelled and stale-context batches submit no function work
and clear busy/request state. Run bash scripts/test-server-client-integration.sh
on macOS. In-flight cancellation, reconnect recovery and manual menu interaction
retain separate qualification work.
The app also publishes atomic per-function checkpoints and offers Resume batch.
Resume verifies the source, bundled worker, pass/cache versions, saved project,
record and function plan. Retained file hashes and their exact correspondence
to validated artifact bodies/headers are checked off the main actor before reuse.
The integration gate resumes its cancelled three-function batch; native contract
tests reject changed worker identities and tampered output. Mid-request service
failure and recovery across restarts remain outside this tested scope.
The integration gate also stops a batch after its first function publication,
resumes it with exactly one retained entry and verifies that the retained files'
modification times do not change. Final manifests report the retained count;
all resulting file hashes are independently checked. Cancellation while a worker
request is still running retains separate qualification work.

Pass 48 preserves a proven canonical Boolean domain for declaration-typed loop
locals in the qualified single-entry natural-loop shape. Initialization must
already prove {0,1}; every modeled update must preserve it. A constant initializer
is not treated as a constant on later iterations. Native counted-toggle fixtures
compare zero, odd and even iteration counts, while a noncanonical update is
rejected. The focused gate now covers 56 configurations, 240 native comparisons,
eight negative cases and a linked 15-function reconstructed subset.

Pass 47 adds bool/_Bool scalar declarations and a structured {0,1} value-domain
proof for parameters, constants, copies, comparisons, selected values and proven
bitwise operations. Returns and call arguments with unproven Boolean
representations are rejected. Boolean arithmetic follows C integer promotion;
generated headers use stdbool-compatible spelling for C/C++ consumption.
The focused matrix covers 52 function/configurations, 200 native comparisons,
C/C++ header compilation and a linked 14-function reconstructed subset.

Pass 46 adds void functions/calls, exact empty-argument and void-result ABI
records, single-execution call scheduling and declaration-bound recovered headers.
The native decoder rejects artifacts from earlier passes and requires regeneration.
Prototype/cache revision 11 invalidates prior call-ABI analysis cache identities.

Batch export processes every discovered function in a binary or supplied range:

```sh
python3 scripts/reconstruct-binary.py /absolute/image /new/output-directory \
  --worker rust/target/debug/reworker --prototypes /absolute/prototypes.tsv
```

Add `--start 0xSTART --end 0xEND` to include functions fully contained in a known
section/address range. Default budgets: 4096 functions and 256 MiB of output;
Add `--include-callees` to follow structured direct-call targets into exact
discovered functions outside that range. The manifest retains direct dependencies,
the scheduled queue and unresolved targets (including budget-limited additions).
Cycles and self-recursion schedule each function once. Resume verifies the
dependency queue and requires the same closure function limit. Indirect calls,
external imports and undiscovered functions remain unresolved dependencies.
`--max-functions` and `--max-bytes` adjust these within enforced ceilings.
The output directory must be new. Each function gets validated evidence JSON
and available recovered `.c`, `.h`, and machine-state `.machine.c` files.
The final manifest records source identity, file hashes/sizes, recovery status,
failures, blockers and functions omitted by limits. Discovery is bounded by
worker analysis; undiscovered functions and compilation remain unqualified.
It also records recovered/review-only/failed function counts and a final batch
status. CLI exit status is nonzero when any attempted function failed, including
retained failures on resume; the manifest and completed evidence are preserved.
A review-only function is not an execution failure, and a completed bounded batch
does not imply full discovery, compilation or equivalence. The final stdout
JSON summary includes the counts and omitted-function count for automation.
App checkpoints also retain recovered/review-only/failed counts and a batch
status distinguishing completed, completed-with-failures, cancelled and
incomplete. Review status no longer calls failed attempts exported artifacts.
Checkpoint validation recomputes outcomes from retained attempts, and app format
dumps preserve failed-function counts separately from unavailable representations.
The real app/XPC gate injects a stale expected source digest into one worker reply
validation: no function file is published, the manifest reports one failure, and
its header dump reports zero available / one unavailable / one failed function.
Interruption leaves completed function files without a complete manifest.
Each completed function also publishes an atomic checkpoint. Continue the same
output directory with `--resume`; source bytes, worker binary, prototype catalog,
scope, function discovery and every retained file hash/size must match.
Retained graphs, exact generated source/header bytes and recovery states are
also independently validated before checkpoint publication; updating file hashes
does not authorize edits to generated output. Checkpoint reads are byte-bounded.
Increasing the function limit allows a limited batch to continue through more functions.
Existing completed files are retained without rewriting. Recorded failures are
retained as failures; resume continues unattempted functions. Uncheckpointed files
are reused only when freshly regenerated bytes match. Changed files are refused.

The native fixture gate now SIGKILLs the actual CLI process immediately after
its first completed checkpoint and, separately, after the second function's
outputs but before its checkpoint publication. A fresh process resumes each
batch to three functions and matches the uninterrupted reference entries;
checkpointed and uncheckpointed completed files retain exact bytes and mtimes.
This qualifies those CLI process-interruption boundaries only, not partial-file
writes, power-loss durability, mid-worker crashes or app/XPC restart recovery.

Compile and link the recovered subset on macOS with:

```sh
python3 scripts/qualify-reconstruction-batch.py /batch/output-directory
```

This verifies manifest file hashes, compiles separate Objective-C/C17 translation
units with warnings as errors, and links a dynamic library with unresolved symbols
rejected. A fresh build-qualification directory retains the library and a report
bound to the manifest hash, compiler identity, compiled files and unrecovered
functions. Build failure is retained as a failed report. The command does not
execute generated code. Native fixture tests separately exercise a linked setter
and reused call result and require failure when a needed callee is omitted.

Declaration-only batches can be qualified separately:

```sh
python3 scripts/qualify-reconstruction-batch.py /batch/output-directory --headers-only
```

This checks manifest hashes and Objective-C/Objective-C++ header syntax without
compiling implementations, linking a library or executing target code. Its fresh
`header-qualification/report.json` declares `mode=headers-only` and a syntax-only
claim; there is no library hash or generated library. It can pass for review-only
functions with supported declarations, but rejects batches with no headers.
The default implementation mode still rejects a batch with no recovered bodies.
The two modes retain separate reports, and existing report directories are never
overwritten. End-to-end tests cover a trapped-body header-only batch, missing
declaration rejection, implementation-mode refusal and retained report bytes.
The mixed 31-function batch also passes header-only qualification with zero
compiled implementations, while its separate implementation report still
retains the 25-function compilation/linkage result unchanged.

The local worker command below emits a real per-function artifact conforming to `schemas/reconstruction.schema.json`. Its primary rendering remains review-only; supported functions additionally contain compilable machine-state C and, with validated signature evidence, recovered scalar source as separate metadata artifacts. Generation alone is never proof of compilation or equivalence.

```text
reworker --reconstruct ABSOLUTE_PATH SHA256 SIZE 0xSTART 0xEND REQUEST_UUID
```

Use a normalized function range returned by ordinary analysis. The worker verifies the captured regular file without following a final symlink, checks its SHA-256 and size, admits thin unencrypted little-endian AArch64 Mach-O, and requires the supplied range to match the shared function-boundary admission path. Its executable scan is limited to 4 MiB. The returned artifact uses the worker's existing four-byte big-endian length framing; errors use the existing error envelope. This command adds no listener, authentication or remote API.

Records link captured instruction bytes, decoded instructions, shared typed IR, a conservative instruction-granularity CFG, structured register SSA definitions/uses/phis, and machine-state AST nodes. The AST is built from shared IR, not parsed from pseudocode text. Rendered text is a structural review view with unresolved declarations and source-result ABI. CFG partitioning and reachability policy are explicit and need not match the presentation report's basic blocks or pruned edges.

Content identities include source digest, function, pass version and analysis-settings digest using the same canonical encoding as the offline Python validator. Request UUIDs do not alter content identities. Analysis engine identity is `arm64-analysis-v104-schema130`; these additive commands have their own `arm64-reconstruction-63` pass identity and do not extend the ordinary 75-field report. Pass 4 added structured NZCV-bit and SIMD-byte definitions, uses, and phi provenance. Pass 5 corrected scalar FP register-file effects: measured scalar results zero the upper bits and read only their source lanes. Pass 6 adds reaching memory definitions, reads and CFG-phi edges to the reconstruction graph. Pass 7 corrects full-register zero-extension for scalar and Q=0 vector results and narrow SIMD loads. Pass 8 preserves SIMD integer lane widths in lifted expressions and treats reserved lane arrangements as opaque barriers. Pass 9 adds lane-wise modular constant evaluation with bounded provenance. Pass 10 publishes function-relevant export/unwind boundary facts without assigning them fictitious instruction-byte spans. Pass 11 scopes Objective-C layouts and exported names to the analyzed function and omits image-wide records with misleading function spans. Pass 12 emits exact-span DMB/DSB/ISB/CLREX system-event evidence, including ordering domains, without inventing application-memory writes. Pass 13 records vector MLA/MLS destination read-before-write dependencies. Pass 14 makes unmodeled FPCR/FPSR effects explicit blockers for floating-point operations and refuses strict machine-state output when those effects are reachable. Pass 15 preserves declaration-proven pointer types through exact-width private-frame spills and reloads, enabling qualified ordinary-memory reads. Pass 16 admits an ordered ordinary pointer write on one single acyclic path and emits typed source and memory-effect evidence. Pass 17 preserves multiple ordinary writes in machine instruction order on that path. Pass 18 recovers bounded dynamic byte offsets when the offset is a declaration-proven integer expression, while keeping invalid-memory fault behavior outside the claim. Pass 19 retains acyclic branch guards on external stores and emits each store under its recovered path predicate. Pass 20 adds a function-level semantic-coverage summary with explicit counts for decoded instructions, shared IR, modeled trees, data-in-code, and unsupported-effect reasons; the record states that these counts measure evidence availability and do not certify complete semantics. Pass 21 preserves ordering for bounded ordinary pointer reads and writes on a single acyclic path, including both store-then-load and load-then-store alias cases. Pass 22 admits exact-prototype direct self calls with scalar register arguments and a scalar result; call-dependent expressions stay inline so return-path conditionals retain lazy branch evaluation. Positive and negative recursive native fixtures pass at four optimization levels with and without symbols: scalar-only one-call and two-call recursion are recovered and differentially checked, while a recursive function with pointer effects is rejected. Pass 23 admits direct symbol-resolved C calls with unique exact prototypes when the complete bounded callee walk proves a return, no memory reads/writes, no onward call, and ABI callee-saved state preservation; generated source carries the callee declaration. A global assembly leaf-helper fixture passes all optimization and stripping configurations. Pass 24 preserves the order of caller memory effects before such a call on a single acyclic path; the store-then-call fixture passes differential testing. The expanded store-load-call fixture proves the same ordering when an alias-sensitive read occurs between the write and the pure call, for both aliased and disjoint pointers; call-before-memory remains rejected. Pass 25 admits acyclic branch-guarded mixed reads and writes when each explored path observes a strictly increasing instruction-address schedule. Reads and writes retain their path predicates in generated code and memory-effect evidence. Conditional one-arm and two-arm stores followed by an alias-sensitive read pass native differential qualification. Optimized code that combines alternate writes into one value/store and code that retains separate path writes both pass; unresolved schedules, loops and volatile effects remain rejected. Pass 26 applies the caller memory-before-call check independently to every returned path, admitting branch-local memory effects before proven pure leaf calls when other paths omit that memory effect. The branch-store-and-leaf-call fixture passes the native matrix; a memory effect after a call on the same path remains rejected. Pass 27 preserves proven low bits through an exact OR-with-zero register copy when the source ABI value is narrower than the machine register; a declared narrow return may consume those bits, while later full-width uses remain blocked. Signed and unsigned 16-bit and 32-bit external stores pass native compilation and differential checks, including surrounding memory. Pass 28 records signedness on loaded memory expressions and the corresponding integer-promoted value when the decoded load proves sign extension. Signed and unsigned 8-, 16-, 32- and 64-bit load AST types are asserted by the native qualification gate. Exact fixed-width pointer declarations establish signedness when the load encoding does not carry it, such as 64-bit loads; the declaration is applied only when its pointee width matches the load. C integer promotion types are also checked for sub-32-bit loads. Pass 29 preserves a proven source type through register-width views that retain every source bit; truncating casts remain unresolved unless widths agree. This lets signed and unsigned 16-bit and 32-bit store AST values retain their declaration-proven width and signedness. Pass 30 uses exact fixed-width pointee declarations to recover loaded signedness when it is absent from the instruction, and propagates a shift expression’s proven left operand type so compiler-scaled dynamic array indices remain typed. Dynamic signed 16-bit and unsigned 32-bit element loads pass native differential qualification at four optimization levels with and without symbols. Dynamic signed 16-bit and unsigned 32-bit element stores pass with final surrounding memory checked. Pass 31 admits multiple exact-width, declaration-proven volatile loads or stores and retains instruction order on each executed path when there are no calls; two-load and two-store volatile fixtures pass native differential qualification. Pass 32 structures single-entry natural loops with complementary zero-test backedges and straight-line GPR assignments as `while` regions. Modified caller-saved scalar registers become explicit accumulator/induction locals; each loop AST retains its entry, backedge, and updates, and later loop-local bindings initialize after earlier loops so sequential loop-carried state is preserved. The native `hs_m_loop` fixture contains two sequential loops and compiles and compares at four optimization levels with and without symbols. Loop memory effects, calls, flag-carried conditions, multiple exits, nesting, and irreducible control flow remain blocked. Pass 33 links recovered-source memory read/write AST evidence to instruction-scoped memory SSA use/definition records and links proven frame-object versions to reaching memory definitions; the 544-function/configuration Apple-Silicon matrix validates the artifact links and preserves native behavior. These are provenance links only; path-specific reaching versions and phis do not yet drive source variable lifetime or value recovery. Pass 34 adds guarded call-result events to the ordered schedule. Caller loads/stores after a proven pure leaf call now emit under the call path guard and in instruction order, including two-arm call/store schedules; exact self-recursion followed by caller stores is also qualified. The expanded `-O0/-O1/-O2/-Os`, symbol-rich/stripped matrix now covers 544 function/configuration pairs and 100,640 native result comparisons, including call-then-load and call-load-store result checks and final memory checks for the branch-call/store and recursive-store fixtures. Calls with incomplete, effectful, unresolved, or conflicting callees remain blocked. Mixed access schedules without a proven per-path instruction order, volatile paths with indirect, unresolved or effectful calls, unsupported loop shapes, and unresolved control flow remain blocked. Exact signature candidates also bind the settings digest, so changed declarations cannot reuse a prior source identity. Pass 35 schedules multiple declaration-proven volatile loads and stores in per-path instruction order; straight-line and branch-guarded two-read fixtures plus a two-write fixture pass native differential qualification. Pass 36 qualifies two sequential calls to a proven pure leaf callee, checking the call nodes in the artifact and generated nested call behavior. Pass 37 schedules declaration-proven volatile stores around supported scalar calls; fixtures cover store-before-call and call-before-store. Pass 38 qualifies declaration-proven volatile loads before and after supported scalar calls. Pass 39 qualifies a combined volatile store, supported pure scalar call, and volatile load schedule, checking store-before-call-before-load order, both memory AST records, and native return and memory results. Pass 40 admits a single-entry zero-test natural loop containing ordered declaration-proven external stores, preserving per-iteration machine order and memory SSA provenance; the volatile countdown-store fixture checks generated C and native return/final-memory results. Calls still require exact prototypes and bounded callee proofs; indirect, unresolved and effectful calls remain blocked. Pass 41 schedules a declaration-proven volatile loop load before the dependent register update and store on every iteration; the volatile accumulation fixture checks read/store AST provenance, event order, native return and final memory. Loop-carried types may be seeded from a proven update when the initial value is untyped; conflicting proven update types reject the loop. The system-register table now uses compiler-verified NZCV, FPCR, FPSR, DAIF and CurrentEL encodings; shared IR models MRS/MSR NZCV as GPR and flag-file effects, while strict machine-state export records an explicit blocker. The reconstruction artifact identity advances to arm64-reconstruction-55. Pass 43 models ADCS/SBCS carry consumption, full NZCV definitions, and bounded constant folding when incoming carry and operands are known. Pass 44 adds strict machine-state interpretation and generated C for ADCS/SBCS, including exact N/Z/C/V updates at 32- and 64-bit widths; native fixtures qualify carry-dependent results and flags. Pass 45 emits the modeled MRS/MSR NZCV operations in strict machine-state C and interpretation, preserving NZCV bit placement in readback and verifying that writes change subsequent conditions. The expanded matrix now covers 544 function/configuration pairs and 100,640 native comparisons.

The initial instruction budget is 128 words. Larger functions retain their full admitted bounds but publish only the bounded prefix and explicit truncation/exhaustion blockers. Unsupported instructions/effects, unresolved call contracts and unqualified runtime markers produce blockers, even when conservatively captured outside known-edge reachability. Review AST memory nodes retain their unqualified hardware fault-model limitation. Review-only is not a semantic-completeness certificate.

Reconstruction pass 13 records vector MLA/MLS destination read-before-write
dependencies in the SIMD byte SSA, while the analysis engine v94 identity
prevents reuse of reports produced before those semantics were qualified.

Qualification: `python3 scripts/test-worker-reconstruction.py PATH_TO_REWORKER` compiles six CC0 AArch64 Mach-O fixtures without executing the imported targets. It checks real worker output with the independent Python validator, cross-language content identities, graph-to-byte queries, deterministic requests, SSA phi presence, flag-write nodes, unsupported blockers, truncation and adversarial source/range/content/eligibility mutations. This test is included in macOS CI and the macOS qualification script.

## Executable machine-state slice

Supported functions also contain one `metadata` evidence node with `artifact_kind=machine-state-c`, complete C17 source, an entry address and the `aarch64-gpr-nzcv-flat4096-le-v1` execution contract. The source uses the same structured tree as the review graph. Its dispatch cases identify original instruction addresses. Unsupported operations, incomplete function bytes, constrained-unpredictable writeback aliases and unresolved reachable exits reject the entire strict export rather than inserting stubs. Generation alone is labeled `generated-uncompiled`; it never promotes review rendering to recompilable recovered source.

The slice covers supported integer assignments/arithmetic/logical operations, shifts, division, extensions/extracts, MOVK, comparisons, conditional selects, branches, GPR loads/stores/pairs and returns. Encoded arithmetic/logical NZCV writes are augmented from instruction bits before aliased destination updates. This does not imply canonical flags SSA or repair every legacy preview consumer.

The generated runtime uses unsigned modular bitvectors (including defined signed-division overflow/zero-divisor results), explicit W/X write policies and checked little-endian guest memory. It never dereferences a guest address as a host pointer. Guest memory is 4,096 ordinary, nonvolatile bytes at `0x100000`; there is no concurrency, MMIO, privileged state, precise hardware exception-state or arbitrary native import model. Status codes are 0 return, 1 invalid PC, 2 step budget and 3 modeled memory fault. A return records the target and ends the analyzed region; it does not execute that address.

```text
reworker --interpret ABSOLUTE_PATH SHA256 SIZE 0xSTART 0xEND 0xX0,0xX1,0xX2,0xX3,0xX4,0xX5,0xX6,0xX7 REQUEST_UUID
```

This command interprets the admitted structured tree, not the binary on the host. Other GPRs and NZCV start at zero, SP at `0x101000`, and guest memory at zero. It has a 4,096-step limit. Its artifact adds a content-bound `machine-state-run` metadata record containing inputs, execution contract, outcome, GPRs/SP/PC/NZCV/steps and guest-memory bytes. It shares all captured-source and function-boundary admission checks with `--reconstruct`.

`python3 scripts/test-machine-state.py PATH_TO_REWORKER` runs the CC0 test-owned fixtures on Apple Silicon: C functions, an Objective-C method resolved through native libobjc, and assembly cases at `-O0/-O2`. It compares native x0 and designated external buffers with interpreter/generated results, and compares all modeled GPRs, SP, PC, NZCV, steps and memory between interpreter and generated C. Generated C compiles with C17, warnings-as-errors and undefined-behavior sanitization. Original fixtures explicitly disable stack protection for this no-import slice; a separate protected fixture must be rejected for its unresolved native call. Native faults are not deliberately executed.

Qualification artifacts under `build/qualification/` retain source/function/pass/settings identities, toolchain/flags, seeds and exact input vectors, output/source/executable digests, and graph-linked compilation/differential observations. Compilation and bounded differential success are separate from recovered-source fidelity and formal equivalence. The hardware test explicitly skips on hosts other than Apple Silicon macOS; a skip is not native qualification.

Remaining: recovered variables and source-level types, complete effects and memory/flags/vector SSA, native catalog joins, semantically validated recovered-source AST, native call/import adapters, broader C17/Objective-C emission, complete behavior gates and coordinator/client integration. Those remain open in C00–C11 of the master roadmap.

## Named scalar source recovery

The `apple-aapcs64-lp64-reads-writes-calls-volatile-ordered-natural-loop-guarded-call-events-v11` transform consumes the same structured machine tree plus the existing declaration/ABI adapter. Exact C symbols join source-hash-bound declarations imported by Apple Clang; validated target Objective-C method encodings supply method declarations, selector-derived argument names and implicit self/_cmd bindings. A symbol name alone does not establish a signature. Contradictory catalog entries fail closed, and multiple exact declaration candidates remain an explicit source-recovery rejection rather than silently selecting one. The source emitter keeps call-dependent expressions inline, allowing enclosing return-path conditionals to guard recursive calls lazily. It also supports a direct C call to a symbol-resolved callee when an exact scalar ABI declaration is proven and the complete bounded callee walk proves a return, no memory read/write, no onward call, and preserved callee-saved state; the recovered source carries that callee prototype. Caller memory effects before or after a proven call are emitted as guarded, instruction-ordered events on bounded acyclic paths. The native matrix also qualifies branch-dependent leaf calls with stores and exact self-recursion followed by caller stores. Incomplete/effectful callees, unresolved/stripped target names, conflicting declarations, and unsupported ABI shapes remain blocked.

Supported scalar expressions use named parameters, width-aware copies and casts, modular operations, exact shift/division helpers, and acyclic conditional/early-return paths. Symbolic frame objects track private stack slots, invalidating overlapping writes. Proven non-volatile pointer parameters may feed bounded scalar loads, including constant and declaration-typed dynamic byte offsets; generated reads use `memcpy` helpers to avoid alignment and aliasing assumptions. Direct signed and unsigned 8-, 16-, 32- and 64-bit loads, signed 8-bit loads at a constant byte offset, unsigned 32-bit loads at a constant element offset, dynamic byte-index loads, dynamic signed 16-bit and unsigned 32-bit element loads, and dynamic signed 16-bit and unsigned 32-bit element stores with surrounding memory checks, compile and compare successfully with the native originals across four optimization levels and stripped/unstripped binaries. Ordered external stores through declaration-proven non-volatile pointers are emitted as separate source statements with AST nodes and memory-effect records; path predicates guard branch-dependent writes. Mixed reads and writes are scheduled in instruction order under recovered path guards when every acyclic path proves the same increasing instruction-address order. Fixtures cover both load/store orderings, aliasing and disjoint pointers, and branch-guarded one-arm and two-arm stores followed by an alias-sensitive read; unresolved schedules remain rejected. These effects assume valid ordinary memory and do not claim precise fault behavior. Frame addresses may not escape; every reachable return must restore SP and Darwin nonvolatile GPRs and return through the entry link-register identity. Multiple declaration-proven volatile scalar accesses per path are supported with no calls when their instruction order is proven; two-load and two-store fixtures pass native qualification. Unproven qualifiers/widths and volatile calls remain blocked. One single-entry zero-test loop shape supports per-iteration declaration-proven external reads and stores; reads feed typed loop-carried values and are scheduled before dependent assignments and stores. Calls, frame effects, additional exits and other unsupported loop shapes remain blocked. Unknown entry bits, unresolved flags, unproven dynamic offsets, dynamic frames, and relocated addresses also block this source view. Its limits are 2,048 expression nodes, 2,048 path visits and 32 pending/return paths; general memory/flags SSA and loop reconstruction remain separate work.

The emitted `recovered-source` record contains a complete function and dependencies, its declaration, a secondary source-AST root and its own observable contract. Source nodes retain instruction-dependency spans and declaration evidence; parameter spans explicitly describe entry-ABI bindings, not the bytes that supplied an original parameter name. Source function attributes retain parameter spelling, signedness, width and GP location. Multiple-use expressions receive deterministic local names instead of duplicating large expressions. Signed results are bitcast through `memcpy`, not implementation-defined arithmetic conversion; native type-size assertions enforce the qualified LP64 layouts. Objective-C output is a C-style IMP adapter, not a claim to have reconstructed an entire class implementation.

Ordinary single-threaded private frame/red-zone storage is an explicit assumption node. Volatile behavior, precise fault state, complete register/stack traces and original-source fidelity are not qualified by this transform. The primary review AST keeps `compilable=false` and review eligibility; actual compilation observations bind the separate generated-source record in qualification artifacts and do not create a formal-equivalence label.

`scripts/test-recovered-source.py` qualifies C, Objective-C and assembly fixtures at `-O0/-O1/-O2/-Os`, both symbol-rich and `strip -S -x` variants (public exports remain). It compiles generated C17/C-style Objective-C with warnings-as-errors and undefined-behavior sanitization, compares declared scalar result bits against native execution, validates complete artifact graphs/content IDs and checks deterministic identities, unsafe inputs, signature binding and unsupported cases. Headers, not function bodies, are supplied as type evidence. The Objective-C metadata fixtures use classic rebasing (`-Wl,-no_fixup_chains`); modern chained metadata-to-source-signature joins are not qualified by this test. Native execution is limited to test-owned CC0 fixtures on Apple Silicon macOS.
