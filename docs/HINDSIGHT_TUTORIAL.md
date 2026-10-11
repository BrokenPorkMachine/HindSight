# HindSight tutorial: first evidence-backed investigation

This tutorial uses the bundled synthetic fixture:

`Tests/Fixtures/arm64-integer-sample.macho`

It is deterministic test data, not a runnable application. It contains a small ARM64 `__text` section with 16 instruction words: 13 supported scalar instructions and three deliberately unsupported words. The fixture has no proprietary code and does not require a device, firmware image, or network access.

## Workflow

1. Create a project from Quick Start.
2. Import `arm64-integer-sample.macho` using Auto / composite format.
3. Run Analyze on the captured artifact.
4. Open Analysis Triage and select an evidence row.
5. Follow the selection into Annotated Code or Analysis Quality.
6. Add a note, tag, and confidence classification to one evidence item.
7. Open Investigation Ledger and export a HindSight Case, Markdown, PDF, or JSON handoff.

## What to verify

- The captured source remains tied to its content identity.
- The executable section begins at file offset `0xb8` and unslid virtual address `0x1000000b8`.
- Supported instructions decode into bounded rows with bytes, addresses, and mnemonics.
- Unsupported words remain visible as decoder gaps; they are not silently treated as valid semantics.
- Analysis Quality reports decoder coverage and unresolved evidence boundaries.
- The exported handoff keeps observations, annotations, and analyst conclusions distinct.

The fixture is intentionally small enough to inspect manually and stable enough to use in regression tests. For a real artifact, keep the same sequence but preserve the import mapping, source hash, analysis status, and unresolved evidence in the exported handoff.
