# ARM64 instruction-derived semantics

The ARM64 worker now emits a separate `semantic_preview` analysis field. It is
computed from captured instruction words, not from rendered assembly text. Each
row begins with its unslid instruction address and includes the source word, so
the IR view can pivot to the exact disassembly evidence. Addressed rows also
enter the worker's typed evidence sideband and the synthesis fingerprint.

This first slice models `MOVZ`, flag-neutral `ADD`/`SUB` with immediate operands,
unshifted register `ADD`/`SUB`, direct `B`/`BL`, `RET`, and `NOP`. It emits an
explicit opaque barrier for every other instruction and for `LC_DATA_IN_CODE`
ranges. Output is bounded to eight functions and 128 words per function. The
structural backend emits an empty field. The cache engine identity advances to
`arm64-analysis-v18-schema72` so reports from older workers are not reused as
if they contained semantic output.

Statements name machine registers. `RET` labels x0 only as an ABI result-register
candidate because the return type is unknown. `BL` does not infer argument
count, call signature, result, or side effects. A typed operation representation
tracks exact integer constants through supported straight-line instructions and
records the contributing instruction addresses. Opaque instructions, calls,
branches, returns, data ranges, and function boundaries clear that knowledge. Tracking
stops if provenance would exceed eight instruction addresses. The preview does
not yet propagate expressions across basic blocks, reconstruct source variables,
recover Objective-C sends or Swift calls, or model ARM64e pointer authentication.
It is the testable instruction-to-semantics foundation for those passes, not a
general source decompiler.

Verification uses exact encoding unit tests and the bundled ARM64 Mach-O fixture
through the worker wire protocol. The fixture asserts a known `x0 = x0 + x1`
statement, its instruction word and address, plus an opaque barrier.
