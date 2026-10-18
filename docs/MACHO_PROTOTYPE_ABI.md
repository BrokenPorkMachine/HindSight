# Mach-O signature, prototype and ABI resolution

The ARM64 semantic path reconstructs source declarations from validated
Objective-C method metadata, exact SDK/API catalog matches and a bounded subset
of Itanium mangled parameter types. Unknown return types and layouts remain
explicit. Prototype ABI interpretation is versioned independently as
`prototype-abi-v1` in analysis/cache/continuity identities.

Supported Apple ARM64 LP64 rules include:

- Independent `x0`–`x7` and `s0`/`d0`–`s7`/`d7` argument banks. FP overflow
  goes to the stack even when GP registers remain available.
- Objective-C IMP receiver/selector arguments in `x0`/`x1`.
- Exact scalar, pointer and selected SDK typedef classification. Pointer types
  containing the words float/double remain GP values. Function pointers, block
  pointers, references and decayed array parameters retain pointer locations.
- Natural size/alignment for fixed scalar stack arguments. After GP exhaustion,
  `char`, `short`, `int`, pointer arguments occupy offsets 0, 2, 4, 8, rather
  than four eight-byte slots. Locations are relative to call-site SP, not a
  decoded function's post-prologue frame.
- Known `CFRange`/`NSRange` two-word integer aggregates and `CGPoint`/`CGSize`
  two-double / `CGRect` four-double homogeneous FP aggregates. Register groups
  and whole-aggregate stack spills retain their known SDK type. A failed group
  allocation exhausts that bank while preserving the other bank.
- Exact matching Objective-C LP64 aggregate encodings, including their nested
  member layouts. Familiar struct names with different encodings are rejected.
- Typed scalar return locations (`w0`, `x0`, `s0`, `d0`), known SDK aggregate
  return register groups, and exact void-return handling. `void *` returns
  remain pointer values. Unknown aggregate/HFA/indirect return layouts stay
  unresolved instead of being assigned to `x0`.

Call Values includes prototype ABI bindings and return evidence for address-backed
signatures. Pseudocode uses those locations for declarations, direct/resolved
indirect calls and returns, including structured conditional returns. The API
catalog includes printf/fprintf/sprintf/snprintf and NSLog fixed prototypes;
anonymous variadic arguments carry an explicit unresolved marker.

Unknown typedefs and aggregate layouts can change both register banks and stack
offsets. The unsupported parameter and subsequent bindings remain unresolved.
Nested Itanium names do not prove whether a function needs an implicit C++
receiver; those argument bindings remain unresolved without membership evidence.

This is progress toward complete resolution. It does not recover arbitrary C/C++
layouts, Swift ABI, DWARF types, user header libraries, stack-passed values,
anonymous variadic values, hidden ABI attributes, or caller/callee constraints.
Unresolved GP return candidates and register-use arity evidence remain explicitly
weaker than source declarations. Objective-C selector evidence alone does not
establish a receiver class or uniquely select a method prototype.

Validation combines Rust regressions, the existing decompilation corpus and
`python3 scripts/test-prototype-abi.py rust/target/debug/reworker`. The latter
compiles public-domain Objective-C fixtures into real Mach-O binaries at `-O0`
and `-O2`, analyzes them without execution, and checks metadata-backed locations
and typed pseudocode returns. It runs in the macOS gate. Separate Apple Clang
assembly probes confirmed packed scalar spills and whole two-word aggregate
spills when only one GP argument register remains.

References: [Apple ARM64 ABI](https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms)
and [LLVM AArch64 calling-convention implementation](https://github.com/llvm/llvm-project/blob/main/llvm/lib/Target/AArch64/AArch64CallingConvention.td).
