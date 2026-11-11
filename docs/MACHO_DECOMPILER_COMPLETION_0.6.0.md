# Mach-O-first semantic decompiler completion

**Status:** execution plan, not a claim of completed decompilation.
**Priority:** after the bounded x86-64 QEMU pilot; before extending general ELF/PE
semantic decompilation. This is a track within `MASTER_ROADMAP.md`, not a
second authoritative roadmap.

## Product contract

Given an admitted Mach-O binary, HindSight should expose a readable,
address-linked semantic representation and a source-reconstruction artifact for
every function it can prove, with explicit blockers for every function it cannot.
C, Objective-C, C++, Objective-C++ and supported Swift metadata are handled
according to their *actual* evidence; no symbol name alone proves an implementation.

The output classes are distinct:
1. **Disassembly:** decoded instruction observations.
2. **Semantic IR:** modeled machine behavior with unknown effects preserved.
3. **Pseudocode:** human-readable reconstruction, not necessarily recompilable.
4. **Source reconstruction:** independently validated typed AST + supported C,
   Objective-C or Objective-C++ output, not the original developer's source.
5. **Qualified behavior:** compilation/linkage and differential execution evidence
   only for individually eligible and safely executable fixtures.

## Mach-O ARM64 / ARM64e completion sequence

- [ ] **M0 Baseline:** compile worker and app; run existing corpus and recovered
      source tests on Apple Silicon. Record per-function recovery, review-only,
      missing, opaque, rejected, coverage/limits and elapsed/RSS evidence for
      stripped/unstripped, O0/O1/O2/Os.
- [ ] **M1 Lift correctness:** close opaque-but-common ARM64/ARM64e effects
      needed by the corpus, preserving unknown or privileged behavior where
      unsupported; cover vector/FP, flags, load/store, atomic, PAC and calls.
      Do not equate a decoded instruction with a proven memory effect.
- [ ] **M2 Memory and local SSA:** canonicalize memory partitions, stack/frame
      objects, overlapping stores, aliases, calls and register/flags/FP state
      across CFG joins and loops. Generate bounded failure reasons, not guessed
      locals.
- [ ] **M3 Types and ABI:** reconcile function boundaries, real symbol metadata,
      Apple arm64 ABI, Objective-C/Swift runtime metadata, demangled C++,
      SDK/project type catalogs, indirect returns, variadics and cross-function
      constraints. Keep conflict alternatives visible.
- [ ] **M4 Structured AST/pseudocode:** recover nested if/else, short-circuit
      expressions, natural loops, switch dispatch and typed expressions only
      from proven CFG and effect edges; link every statement to instruction
      evidence. Provide clear fallback blocks where control flow is unresolved.
- [ ] **M5 Recompilable source:** materialize recoverable independent function
      implementations and project types; preserve dependencies, headers and
      imports. Rejected or merely guessed code must not masquerade as recovered.
- [ ] **M6 Differential verification:** use deterministic, authorization-safe
      fixtures and independently compare original machine behavior with
      compiled recovered code. Track exact supported inputs and observed
      differences, rather than counting compilation as equivalence.
- [ ] **M7 Studio UX:** show source/assembly/CFG and evidence at usable sizes;
      choose one main surface by default, optional linked split views, click
      through to exact instruction/source identity, review blockers and export
      compilable subsets with proof reports.
- [ ] **M8 Scale:** bounded per-function and whole-project queues, cancellation,
      cache invalidation, 8 GiB memory pressure limits, deterministic revisions
      and safe resumable batch output. No silent partial success.

## x86-64 Mach-O follow-on

- [ ] **X0:** add an x86-64 Mach-O lifter into the *same typed semantic IR*,
      respecting implicit flags, RIP-relative addressing, stack effects, vector
      register widths and exceptional/opaque behavior.
- [ ] **X1:** Apple x86-64/System V-derived calling convention, ObjC dispatch,
      C++ member conventions, varargs/stack alignment and return values.
- [ ] **X2:** feed the common AST/source generator, not a separate text-based
      pseudocode renderer.
- [ ] **X3:** independently qualify x86-64 Mach-O fixture behavior on a supported
      Intel execution host, or explicitly skip execution on Apple Silicon.
      QEMU TCG/HVF is an observation backend, not proof of recovered-source
      equivalence.

## Immediate implemented increment: measured coverage

`scripts/macho-reconstruction-coverage.py` validates an existing reconstruction
batch through `reconstruction_batch.read_batch_manifest` and
`verified_outputs`, optionally verifies source Mach-O identity, and reports
per-function recovery states, source blockers, unresolved dependencies, and
reconstruction fraction. It explicitly labels compilation and behavioral
equivalence **NOT TESTED**. This coverage instrument is a starting diagnostic,
not a semantic algorithm upgrade.

Example after a worker/batch run:

```sh
python3 scripts/reconstruct-binary.py ./example.macho ./build/macho-batch \
  --worker ./build/rust/debug/reworker --max-functions 256
python3 scripts/macho-reconstruction-coverage.py ./build/macho-batch \
  --source ./example.macho --require-no-failures
```

Run the existing validation corpus first:

```sh
python3 scripts/run-decompilation-corpus.py --worker ./build/rust/debug/reworker
python3 scripts/test-recovered-source.py ./build/rust/debug/reworker
```

The second command's exact CLI is subject to the current test harness; verify
its usage on the release host. No test pass is claimed by this document.

## Release gates

- Each recovered source output is hash-bound to original source and worker.
- Every semantic/AST statement has exact instruction provenance.
- Unknown ABI, aliasing, control flow, side effects or type evidence blocks
  corresponding source claims.
- Native/differential tests run independently; unsupported host conditions
  are recorded **SKIPPED**, never **PASS**.
- Corpus spans C, Objective-C, C++ and Objective-C++ at multiple optimizations,
  with stripped and unstripped samples.
- Workspace supports code/CFG/source pivots, narrow window layout and provenance
  inspection without hidden/overlapping panels.
- A large sample fits the 8 GiB target or reports bounded refusal, not a hang.
- Full project coverage accounting includes omitted functions and failed analysis.
- Only after these gates should milestone M8 and the Mach-O component be called
  complete. Do not confuse full decompilation fidelity with exact original source
  recovery, which is generally impossible.
