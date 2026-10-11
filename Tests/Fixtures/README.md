# ARM64 decoder sample

Import `arm64-integer-sample.macho` to see a small structural Mach-O fixture containing 16 words: 13 supported scalar instructions followed by 3 deliberately unsupported words (PAC, SIMD and 0xffffffff). The section begins at file offset 0xb8 / unslid VA 0x1000000b8.

This is test data, not a runnable macOS executable. It has only the header/segment/section structure needed to exercise import, mapping and decoding. Do not attempt to execute it. Bytes are original synthetic fixtures and contain no proprietary program code.

For the complete analyst walkthrough using this fixture, see `docs/HINDSIGHT_TUTORIAL.md`.
