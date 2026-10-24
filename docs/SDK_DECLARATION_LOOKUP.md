# SDK declaration lookup

The public SDK can supply declarations for captured imports; it does not supply
the captured program's implementation. The app now makes a bounded lookup for
reconstruction when a current analysis cache contains eligible imports. It
passes supported declarations to that worker request and reports missing SDK
tooling. A real app/XPC gate verifies that a `fwrite` declaration reaches a
reconstruction artifact. The same gate cancels lookup before worker launch and
verifies the cancelled status and absence of an artifact. Persistent app-side
catalog evidence is written as canonical JSON under the project's
`analysis-cache/` directory. Its filename includes both captured-source and
catalog SHA-256 identities; missing and unsupported matches remain recorded.
Reconstruction only receives prototypes after catalog publication succeeds.
The content-addressed project store reuses an existing catalog only when its
bytes match exactly and refuses to overwrite changed evidence at the same hash
path.
The app reports the catalog filename and hash, and Binary Workspace can export
that catalog after rechecking its content hash and source identity. Batch
checkpoints retain deduplicated catalogs under `sdk-catalogs/`, include them in
the 256 MiB evidence budget, and verify their canonical encoding, source digest,
catalog digest and file metadata on resume and dump. Format dumps include the
same catalogs as `sdk-catalogs/` sidecars and list their hashes in the format
manifest; the all-format manifest also reports the catalog set included in each
format folder. The coordinator resolves SDK, compiler, and bundled-header
identity once per lookup request and reuses it for each candidate symbol while
checking the header before and after parsing. The coordinator may reuse a result
for later functions only when source digest, candidate imports and the full
target/SDK/compiler/header identity match; the in-process cache holds at most
four entries and clears on project switch, analysis-cache clearing, or disconnect.
The coordinator refuses cache clearing while SDK collection is in flight, so
pending work cannot recreate cleared catalog evidence. Each function attempt
and its exported manifest entry also name
the catalog hash used for that request, and checkpoint validation rejects a
missing catalog association, including when an artifact contains an SDK-derived
call candidate; unreferenced catalogs are rejected as well. App export prompts and completion status report
the retained catalog sidecar count so the declaration evidence is visible in
the workflow. For one function, “Export evidence package” creates a new folder
with its validated reconstruction JSON, generated source representations, an
optional verified SDK catalog, and a SHA-256 manifest.
Binary Workspace can verify a package by checking the manifest and file hashes,
decoding the source-bound artifact, comparing each generated source to its
artifact record, and validating the optional catalog's canonical encoding.
The verifier also confirms each SDK-attributed call candidate's symbol and ABI
declaration occur in that catalog; package creation enforces the same match.
The decoder checks every SDK-attributed candidate across all metadata settings
nodes in the bounded artifact, even though the UI displays at most 128 candidates.
Catalog reads are bounded to 4 MiB and open the file relative to a pinned
parent-directory descriptor without following symlinks. File identity,
modification time and change time are checked across the read.
Checkpoint and retained batch artifact reads use the same bounded no-follow
descriptor validation.
Native catalog encoding also checks import/declaration pairing and consistent
target, SDK, compiler, and header identities across every collected row, plus
the architecture/platform triple with a full three-part deployment version and captured
import-name/symbol pairing, and uses the live selector's same predicate to
revalidate the provider's public system path and leading-letter import name.
The app rejects a reply that labels a declaration as Apple SDK catalog evidence
but omits the matching source-bound catalog reference; package verification
applies the same rule.

Worker prototype parsing preserves the distinction between analyst-supplied
prototypes and Apple Clang SDK declarations. SDK origin is accepted only when
the source digest and bounded target, SDK path, compiler, SDK build, and
header-hash metadata are present in the catalog preamble. For Apple SDK
prototypes, the worker checks that the target platform and simulator suffix
match the SDK path's directory name; the CLI applies the same check before
emitting prototypes. Before emission, the CLI also checks source binding,
resolved coverage, unique import names/symbols, declaration shape, and provider
eligibility. The origin is retained in signature evidence.
Binary Workspace displays the retained catalog filename and hash beside the
function artifact, or states that no catalog was retained for that request.
Its call declaration disclosure shows up to 128 symbol, target, declaration and
origin records, makes target addresses navigable, reports the displayed and
total counts, and states that declarations do not supply function
implementations. SDK-derived rows also show the recorded provider from the
verified catalog, with a reminder that linkage metadata does not prove runtime
resolution. The catalog panel also displays the target triple, SDK build,
SDK path, compiler and header hash. The app rejects a reply that labels a declaration as Apple SDK
catalog evidence but omits the matching source-bound catalog reference; package
verification applies the same rule.

Given a thin captured Mach-O and its worker `analysis_result` JSON:

```sh
python3 scripts/darwin_sdk_catalog.py /absolute/capture /absolute/analysis.json
```

The command prints a JSON evidence catalog. It verifies the capture's SHA-256,
architecture and platform, selects public-system imports from `linkage_records`,
and asks Apple Clang for matching declarations. Each match retains the captured
provider and SDK/compiler identity. Private frameworks, conflicting providers,
special ordinals and nonzero import addends do not seed lookup. Requests above
128 eligible symbols are rejected rather than silently truncated.

To analyze and look up declarations in one command, omit the analysis file:

```sh
python3 scripts/darwin_sdk_catalog.py /absolute/capture --worker /absolute/reworker
```

The command verifies the target before analysis, clears inherited analyst
prototype settings, validates the bounded worker frame, then rechecks the source
identity before collecting declarations. It never loads the captured executable.

`--target` and `--sdk` select a compatible installed Apple SDK. The default is
`arm64-apple-macos14.0` with `macosx`. `--header` selects the header aggregation;
the default includes the public C, Foundation, CoreFoundation, CoreGraphics,
Objective-C runtime, Mach, dyld, dispatch and XPC declarations.

`--worker-prototypes` prints source-bound TSV suitable for the reconstruction
CLI's existing `--prototypes` option. Missing declarations or unsupported
declarators reject TSV publication; the default JSON retains that evidence.
This command writes only to standard output and does not modify a project.

A recorded import provider is not proof of the image loaded at runtime.
Interposition, reexports, target SDK differences and private APIs still require
separate qualification. SDK declarations must not be represented as recovered
application bodies or as proof of behavioral equivalence.

## Automatic reconstruction lookup (unreleased)

```sh
python3 scripts/reconstruct-binary.py /absolute/capture /absolute/new-batch --worker /absolute/reworker --sdk-imports
```

This reuses discovery analysis and supplies supported public SDK import
declarations to each reconstruction request. The full lookup catalog, including
missing and unsupported declarations, is retained in the batch manifest and
checkpoint with a source-bound catalog hash. Only eligible declarations enter
the worker TSV; no function implementation is supplied by the SDK. The SDK build
is included in the prototype identity. `--sdk` and `--sdk-target` choose an
installed SDK and compatible target; defaults are macOS/Apple Silicon.

Resume with the same options plus `--resume`. Changed SDK evidence, prototypes,
source or worker identities are refused; lookup mode cannot silently be removed.
`--sdk-imports` and `--prototypes` are mutually exclusive. There must be eligible
public-system import names to look up; unsupported binaries or empty requests
are rejected before a batch directory is created. App reconstruction persists
its complete lookup catalog in project evidence and carries it through batch
checkpoints and exported format sidecars. Both app and CLI batch verification
match every SDK-attributed call candidate's symbol and ABI declaration to the
retained catalog. CLI batch reads also use bounded no-follow descriptors and
verify metadata across each read. The CLI checks Apple Clang catalog schema,
1 KiB declaration bounds, target/SDK agreement, public provider paths and the
import/function/missing-name association, plus the
bounded call-candidate field contract and the SDK C-language attribution rule.
Worker-eligible catalog entries require a nonempty ABI declaration.
Retained declaration text must match the collector's bounded C function form.
App batch qualification remains open.
