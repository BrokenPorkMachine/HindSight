# Mach-O / dSYM identity validation — independent first-stage tool

Run `python3 scripts/macho_uuid.py /path/to/Executable` to list the validated
`LC_UUID` values for thin or universal binaries. Run
`python3 scripts/macho_uuid.py /path/to/Executable --dsym /path/to/App.dSYM/Contents/Resources/DWARF/App`
to compare UUID and exact CPU subtype per slice. Exit code 0 means all binary
slices match; 1 means at least one mismatched/missing slice; 2 means malformed
input or an I/O failure. Results are JSON.

This is a **standalone foundation tool**, not wired into HindSight's worker,
Symbol Resolver, cache, or UI. Matching UUIDs are necessary for safe dSYM pairing
but do not prove that DWARF has been parsed or that source locations are
recovered. The parser enforces bounded architecture/load-command counts and
byte ranges and rejects duplicate UUID commands or overlapping FAT slices.

## Follow-on integration

1. Add a versioned `DebugArtifactIdentity` typed evidence record to the Rust
   worker, keyed by captured source SHA-256 and exact Mach-O architecture slice.
2. Read validated `LC_UUID` from immutable captured bytes, without following
   arbitrary project-controlled symlink paths.
3. Pair user-provided dSYMs on exact CPU type, subtype and UUID only. Reject
   ambiguous inputs and never infer a match from a filename.
4. Parse DWARF in a resource-bounded helper; preserve CU/DIE and byte-offset
   provenance for recovered types and line locations.
5. Add cache invalidation on debug-source identity changes, source/object fixture
   regressions, and UI states distinguishing verified, missing and mismatched
   debug information.

## Local tests

`python3 -m unittest discover -s python/tests -p test_macho_uuid.py -v`
