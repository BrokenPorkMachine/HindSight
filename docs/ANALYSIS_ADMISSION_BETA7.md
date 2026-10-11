# Analysis admission correctness — 0.4.0-beta.7

## Fixed regression

The beta.6 native Mach-O mapper used numeric load-command constants directly and accidentally treated `LC_FUNCTION_STARTS` (`0x26`) as unsupported data-in-code metadata. Modern Apple binaries commonly contain `LC_FUNCTION_STARTS`, so this rejected otherwise valid ARM64 images before analysis started.

## Current behavior

- `LC_FUNCTION_STARTS` participates in function discovery and never vetoes mapping.
- `LC_DATA_IN_CODE` (`0x29`) never vetoes mapping. Its entries are parsed with bounded range checks and overlapping instruction words become explicit data barriers.
- `LC_ENCRYPTION_INFO` / `LC_ENCRYPTION_INFO_64` reject static code decode only when `cryptid != 0`.
- Encrypted images remain eligible for bounded metadata, symbol, linkage, runtime-record and string analysis.
- Instruction-page requests for truly encrypted code fail explicitly instead of returning ciphertext disassembly.
- The header/preview protocol exposes `metadata-only` as a valid disassembly status.

This distinction is intentional: inability to prove code bytes are decodable is not equivalent to inability to analyze the artifact at all.
