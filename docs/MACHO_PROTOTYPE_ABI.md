# Mach-O signature, prototype and ABI resolution

The ARM64 semantic path reconstructs source declarations from validated
Objective-C method metadata, exact SDK/API catalog matches and a bounded subset
of Itanium mangled parameter types. Unknown return types and layouts remain
explicit. Prototype ABI interpretation is versioned independently as
`prototype-abi-v10` in analysis/cache/continuity identities.

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

Proven GP argument constants now retain the formal source type. Pseudocode casts
interpret the low ABI bits at the declared width, including signed narrow
integers, unsigned values, pointer aliases and booleans. Evidence keeps the raw
register value and instruction provenance beside the cast. Pointer casts do not
assert pointee validity. Noncanonical bool representations remain explicit
bitcasts instead of silently becoming true. Arguments without a supported
prototype retain raw machine values.

Direct and resolved indirect calls also substitute proven scalar integer stack
arguments from bounded, straight-line SP-relative stores. The recovered store
must cover the formal width at the exact call-site offset; its instruction and
source-register provenance accompany the typed cast. An intervening call,
possible memory alias, opaque instruction, or SP update discards earlier stack
values. FP and aggregate stack contents remain unresolved.

Foundation catalog refinements must agree with the runtime encoding's ABI
width, register class and integer interpretation. A conflicting encoding keeps
its own declaration and reports the SDK conflict instead of adopting the catalog.

The opt-in `HINDSIGHT_PROTOTYPE_DB` project catalog supplies exact C declarations
for symbols present in the image. Catalogs require a matching source SHA-256,
use bounded tab-separated records, and reject malformed or unsafe declarations.
Built-in exact SDK declarations retain precedence. Supported catalog types feed
the same argument, return, cast, and Apple ARM64 location recovery as built-ins;
unknown layouts remain explicitly unresolved.

Validated protocol objects now contribute scoped method declarations to Call
Values, preserving required/optional and class/instance membership. Each
prototype retains its protocol name and ABI interpretation, and explicitly marks
its implementation as unproven. A selector shared by different protocols does
not establish one dispatch signature. Missing protocol implementations do not
create functions, call targets or function boundaries. Object size/extents and
supported selector/type encodings are checked before prototype reconstruction.
Validated adopted-protocol lists preserve inheritance in declarations such as
`@protocol Child <Parent>`. The edge establishes inherited requirements only;
it does not establish class conformance, an IMP, or a dispatch target.
Validated `class_ro_t` protocol lists now preserve class declarations such as
`@interface Class <Protocol>`. Conformance does not establish that required or
optional methods have local implementations, and does not select dispatch targets.
Validated category records resolve their class pointer and protocol list into
`@interface Class (Category) <Protocol>` declarations. Category conformance stays
separate from the base class record and carries the same implementation limits.
When the static linker folds a local category protocol list into `class_ro_t`,
the report preserves the effective class conformance because category provenance
is no longer present in the linked image.
Locally resolvable superclass pointers propagate protocol sets through bounded,
cycle-checked superclass chains. These rows identify inherited conformance and
do not rewrite it as a direct declaration on the subclass.
Quoted Objective-C object encodings preserve validated class and protocol
qualifiers, including `id<Protocol>` and `Class<Protocol> *`, as pointer-width
arguments and results. Malformed or unbounded qualifier lists remain unresolved.
For protocol methods, bounded `extendedMethodTypes` entries override the lossy
base method encoding by the runtime-defined required/optional and instance/class
list order. The enclosing protocol record, table, strings, and list counts must
all remain inside validated Mach-O sections.

Unknown typedefs and aggregate layouts can change both register banks and stack
offsets. The unsupported parameter and subsequent bindings remain unresolved.
Nested Itanium names do not prove whether a function needs an implicit C++
receiver; those argument bindings remain unresolved without membership evidence.
Itanium free-function parameters preserve bounded unqualified and nested record
names behind pointers/references, plus const, volatile, and restrict qualifiers.
Templates, substitutions, member pointers, and implicit receivers remain unresolved.

This is progress toward complete resolution. It does not recover arbitrary C/C++
layouts, Swift ABI, DWARF types, user header libraries, most stack-passed values,
anonymous variadic values, hidden ABI attributes, or caller/callee constraints.
Conflict resolution across receiver hypotheses remains open.
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
