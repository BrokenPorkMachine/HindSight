# SDK declaration lookup

The public SDK can supply declarations for captured imports; it does not supply
the captured program's implementation. Automatic app integration remains open.

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
