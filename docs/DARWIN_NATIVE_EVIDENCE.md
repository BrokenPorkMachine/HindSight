# Darwin native evidence for reconstruction

The native reconstruction tools collect libobjc/dyld observations and import C function declarations with Apple Clang. The worker now joins structured, target-local property/ivar layouts to AArch64 pseudocode, qualified against those native observations. These support the reconstruction track in the [master roadmap](MASTER_ROADMAP.md). General runtime object/type inference and arbitrary aggregate ingestion remain open.

## Native runtime catalog

```sh
python3 scripts/collect-darwin-evidence.py --class NSObject --class NSString --symbol malloc --symbol objc_msgSend
```

The command builds an isolated Objective-C helper against Foundation, CoreFoundation, CoreGraphics and libobjc, then emits JSON. It queries libraries already loaded in that helper. It does not load a selected target binary or treat the helper's classes as target-defined classes.

Records include:

- Class/metaclass names, superclass, instance size and declaring image.
- Instance/class selectors, runtime names, method encodings, argument/return encodings and supported size/alignment evidence, IMP addresses and their actual image ownership.
- Property attributes, type encodings, custom getter/setter selectors, backing ivars and ownership qualifiers.
- Ivar names, encodings, offsets and supported layout evidence.
- Native symbol lookup, owning image path/install name/UUID, CPU/subtype, encoded platform/build information, runtime base, preferred address and image offset.
- Compiler-measured `NSRange`, `CFRange`, `CGPoint`, `CGSize`, `CGRect` and Mach message-header layouts, including named field offsets and sizes.
- OS build, architecture, helper/source SHA-256 identities, SDK path, missing requests and truncation status.

The nearest symbol reported by `dladdr` is distinguished from an exact symbol-address match. A class's declaring image and each method's implementation image are separate: category and inherited/runtime behavior cannot be assigned one owner from the class name alone. Runtime-reported encodings retain their actual spelling, including anonymous struct encodings such as the SDK's `CFRange`; known source typedefs are separate compiler evidence.

Requests are bounded to 32 classes and 128 symbols. Per class/metaclass, collection admits 512 methods and 256 properties; instance ivars are bounded to 256 and method arguments to 64. Omitted records set `truncated`. JSON output is capped at 8 MiB. Runtime pointers are hexadecimal strings. Collecting metadata does not call the recorded IMPs or invoke external services.

## Apple Clang C declarations

```sh
python3 scripts/collect-darwin-sdk.py --function strlcpy --function NSPageSize --function CFStringGetMaximumSizeForEncoding
```

Apple Clang parses [the catalog headers](../native/reconstruction/sdk_declarations.h). The JSON catalog retains spelled and canonical parameter types, names, declarations, ABI-oriented declarations, attributes, locations, SDK build, compiler version and target triple. Temporary Clang AST object addresses are excluded from the retained types.

Custom project headers and installed iOS SDKs can be selected explicitly:

```sh
python3 scripts/collect-darwin-sdk.py --header /absolute/path/project.h --function project_function
python3 scripts/collect-darwin-sdk.py --sdk iphoneos --target arm64-apple-ios16.0 --function malloc
```

An SDK/target mismatch is rejected. Filtered AST output is spooled and bounded to 4 MiB per requested function; at most 128 distinct functions are admitted. Unsupported callback/array/function-pointer declarators retain their structured type evidence and are not reduced to fabricated scalar prototypes. Calling-convention attribute handling and feeding layouts into the project type library remain open.

Named C `struct` and `union` layout can be qualified directly for a recorded Apple target:

```sh
python3 scripts/collect-darwin-record-layouts.py /absolute/path/types.h \
  --record HSPoint --record HSRect --target arm64-apple-macos14.0
```

The collector uses Apple Clang's record-layout dump and returns record size,
alignment, direct field bit offsets, and named bitfield widths. It excludes
indented child-record rows from a parent's direct field list; the nested-record
fixture also checks that capture-bound catalog export preserves the full
declaration context. Its JSON binds
those results to the exact declaration-context SHA-256, compiler, SDK build and
target triple. It rejects incomplete or ambiguously named layouts. The artifact
does not prove that the declarations match a captured image or that a machine
access corresponds to a source field. With the capture hash option below, the
layout is retained on the matching project type candidate. A bounded worker
case now uses that layout for a read or write through a catalog-backed
`struct *` at an exact constant byte offset, provided one non-bitfield scalar
field uniquely matches both offset and access width. The emitted member access
retains the layout report and capture binding, while captured type identity
remains unproven; unions and ambiguous or mismatched accesses stay unresolved.

For a single named `struct` or `union`, add
`--project-source-sha256 <captured SHA-256>`. The declaration context may
include helper records, such as a nested struct definition; the selected
record's compiler layout and the full context are retained in the candidate.
The command emits a one-candidate catalog JSON that can be imported from
Binary Workspace. The coordinator stores the compiler report as analyst-linked
evidence and rechecks its target and declaration hash; the worker independently
accepts the source-bound nested declaration context and selected record layout.
Neither check treats the report as proof that the binary contains an instance
of that type.

## Analyst project type catalog

The workspace's Project Type Catalog stores bounded analyst declarations (`typedef`, `struct`, `union` or `enum`) alongside a target label and the SHA-256 identity of a source already captured in that project. The coordinator checks that the declared architecture matches the captured source's inspected architecture, validates the declaration shape, and groups conflicting declarations for review. Catalog records are project-local and persist with the project database. Conflict-free declarations with exact source and target matches can be attached to recovered source as analyst-provided dependencies; they are explicitly not ABI-layout evidence.

These candidates remain analyst assertions. They do not establish runtime identity. A unique matching catalog declaration can qualify a single-level pointer spelling in an exact C signature; by-value custom aggregates and nested pointers remain unsupported. Compiler layout evidence is attached and persisted when its target and declaration hash match. When a declaration is required by a recovered signature, its bounded layout JSON is passed through the worker request; the worker parses the record and fields, checks top-level producer, declaration hash and target, and rejects duplicate names or out-of-record offsets/widths. The artifact retains that checked report while marking captured type identity unproven. For catalog-backed `struct *` values, constant-offset loads and stores are rendered as member accesses only when one non-bitfield scalar field matches the exact offset and access width. This narrow path carries layout provenance into the memory-effect artifact; it does not prove the captured image uses the declared type or generalize to unions, ambiguous fields, arrays, pointers, bitfields or nonconstant offsets.

Use **Export JSON** in the Project Type Catalog workspace to write a bounded, validated catalog for review or handoff. **Import JSON** accepts only candidates bound to the selected capture digest and matching architecture, then persists them through a transaction. Conflicts remain visible and are withheld from source dependencies. The export preserves candidate rows, analyst source label, target and captured-source SHA-256; its SHA-256 is reported after writing. A declaration can be a source dependency while its separate layout record remains compiler-qualified review evidence; the worker does not yet consume layout records.

## Feed declarations to AArch64 pseudocode

The importer can emit the worker's existing hash-bound prototype format:

```sh
python3 scripts/collect-darwin-sdk.py --function NSPageSize --prototype-source /absolute/path/captured.macho > /private/tmp/prototypes.tsv
HINDSIGHT_PROTOTYPE_DB=/private/tmp/prototypes.tsv rust/target/debug/reworker --analyze /absolute/path/captured.macho SOURCE_SHA256 SOURCE_BYTE_LENGTH REQUEST_UUID
```

The worker returns its existing framed report. The source argument must be a thin little-endian 64-bit Mach-O whose architecture and platform match the compiler target. Load-command framing is bounded and checked; digest calculation uses the inspected descriptor and rejects observed source changes. The worker independently validates the source SHA-256 catalog binding.

An imported declaration supplies parameters, ABI locations, typed call arguments and signature evidence through the existing prototype path. SDK declarations establish an API/ABI contract; actual runtime-provider UUID/version matching remains separate evidence. Host class/property/ivar observations do not replace captured-binary metadata. The worker's historical external-prototype source label still says “hash-bound analyst C prototype catalog”; the TSV retains the Apple Clang producer, SDK and target metadata.

## Target members in pseudocode

The string inventory distinguishes `__cstring` payloads from `__cfstring` constant-object records. `__cfstring` is not a C string: its bytes encode an object record, and printable fragments inside its pointer/flags/length fields are deliberately not surfaced as literals. Payload decoding remains unresolved until the target's fixups, record layout, character pointer and length are independently validated. The same rule applies to Foundation collections: an import or nearby class-name string is API evidence, not proof that an arbitrary data object is an `NSString`, `NSArray` or `NSDictionary`.

The worker decodes target-local class, property and ivar records into structured member layouts. LP64 class and metaclass method lists use the same RO offsets. Ivar offsets dereference a 32-bit slot, with encoded width/alignment and instance bounds checked. These layouts follow [Apple's objc4 definitions](https://github.com/apple-oss-distributions/objc4/blob/main/runtime/objc-runtime-new.h) and are tested against the helper's public libobjc results.

When the captured image declares `__objc_classlist` but its static records yield no layouts, the worker may realize the image with libobjc in a short-lived copy of itself and merge the runtime layouts back. This fallback is disabled by default and requires `HINDSIGHT_TRUSTED_RUNTIME_OBJC=1`. The child is killed on timeout or failure, so a malformed image cannot take down the analysis process. Runtime-derived layouts carry `runtime: true` and are labeled `libobjc-runtime` in artifacts; runtime IMP symbols use `libobjc-runtime-imp`. Runtime class, ivar and IMP addresses are rebased onto the captured file's virtual addresses against the Mach-O `__TEXT` segment vmaddr (the image base), never the `__text` section VA. Runtime data cannot supply static metadata facts: the runtime path emits `offset_ptr=0x0` and uses the class record as the ivar/property owner, and IR field-access substitution is therefore not attempted from runtime-only layouts.

`dlopen` runs the image's initializers and Objective-C `+load` methods. The fallback is unsafe for untrusted inputs and must be treated as executing the captured artifact, not as passive metadata parsing. Static decoding remains the default and is preferred whenever it succeeds.

Pseudocode includes `@interface`/`@property` declarations with custom accessors, ownership attributes, backing ivars and metadata addresses. An object property's class-qualified type can refine a getter/setter only when its own validated method encoding otherwise supplies generic `id`; explicit conflicting method types retain their original evidence.

A bounded must-analysis over decoded shared IR tracks an instance IMP's incoming `x0` through full-width copies, frame addresses, intact spills and CFG joins. A compatible memory access can then render `x0 = self->_count;` or `self->_weight = (double)d0;`, with the ivar record and offset-slot addresses attached. Dynamic ivar-offset identity stays symbolic rather than being folded to a potentially runtime-adjusted numeric offset. Direct ivar access is not rewritten as a property send, since accessor dispatch can have different behavior.

Unknown/call effects, unmodeled SIMD/FP operations, inconsistent predecessor paths, partial receiver writes, overlapping saved-state writes, data-in-code, unresolved indirect transfers and exhausted proof budgets suppress the substitution. Shared IMPs with multiple class owners, ambiguous member layouts, wrong widths/banks and unresolved offsets also keep machine expressions. Proof is limited to complete functions of at most 128 instructions, 64 saved slots and 32,768 worklist steps. The general source-variable, memory-SSA and type-propagation milestones remain open.

## Shared macOS/iOS qualification

The same encoded Mach-O/dyld records use the same decoder on both platforms. The integration fixture changes only `LC_BUILD_VERSION.platform` between macOS and iOS and asserts identical import, legacy-linking, chained-fixup, export, indirect-symbol and Objective-C metadata records. Architecture/platform checks still prevent accidental binding of one target's declaration catalog to a different target.

This qualifies those shared parsing paths. Shared instruction/runtime contracts can reuse further macOS execution evidence under their recorded ABI/version applicability; simulator/device checks address remaining platform-specific behavior.

## Verification

```sh
python3 -m unittest discover -s python/tests -p test_darwin_catalog.py -v
python3 scripts/test-darwin-catalog.py rust/target/debug/reworker
```

The native fixture supplies methods, custom properties, ivars, a C function and an aggregate. Tests compare libobjc/SDK layouts and image ownership, preserve unsupported callback declarations, and reject invalid requests, architecture/platform mismatches and malformed commands. Real `-O0/-O2` builds verify declaration import, typed properties/accessors, class-method recovery and named ivar accesses. Mutated offset pointers and widths suppress field reconstruction. Unit tests cover receiver copies, stack overlap, CFG joins/backedges, aliases, barriers and proof budgets. The integration gate runs in the macOS test script and decompilation-corpus CI.
