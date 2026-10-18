# Darwin native evidence for reconstruction

The native reconstruction tools collect libobjc/dyld observations and import C function declarations with Apple Clang. The worker now joins structured, target-local property/ivar layouts to AArch64 pseudocode, qualified against those native observations. These support C00N in the [Chimera completion roadmap](CHIMERA_COMPLETION_ROADMAP.md). General runtime object/type inference, arbitrary aggregate ingestion and native-catalog import UI remain open.

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

An SDK/target mismatch is rejected. Filtered AST output is spooled and bounded to 4 MiB per requested function; at most 128 distinct functions are admitted. Unsupported callback/array/function-pointer declarators retain their structured type evidence and are not reduced to fabricated scalar prototypes. Complete record/bitfield layout ingestion and calling-convention attribute handling remain part of the project type-library work.

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
