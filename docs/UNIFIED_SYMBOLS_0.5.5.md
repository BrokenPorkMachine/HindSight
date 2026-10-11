# HindSight 0.5.5 — Unified Symbols

## Goal

0.5.5 turns previously separate symbol evidence into a bounded, provenance-preserving resolver without claiming source-level information that the binary does not contain.

## Resolver inputs

The worker combines only validated evidence already recovered by the analysis pipeline:

- direct `nlist` code symbols with exact virtual addresses;
- admitted function starts from `LC_FUNCTION_STARTS`, direct symbols, and the bounded executable scan;
- `LC_DYSYMTAB` indirect-symbol facts for stubs and pointer slots;
- decoded dyld import names; and
- decoded export-trie names.

The resolver does not load Apple private frameworks and does not invent dSYM/DWARF source locations.

## Precedence

For an admitted executable range, the selected display symbol is chosen in this order:

1. exact direct `nlist` symbol at the function start;
2. exact indirect-symbol name at the same address, preferring a recovered stub record;
3. an explicit `<unnamed-function>` label backed only by the validated function boundary.

Additional direct or linkage names at the same address are retained as aliases rather than discarded.

Imports and exports that do not carry a proven in-image address are emitted as **name-only** evidence. Export-trie address fields are not silently promoted to absolute virtual addresses because their interpretation depends on image layout/base semantics.

## Bounded behavior

- At most 4,096 function-range rows are emitted.
- At most 2,048 non-function linkage-address rows are emitted.
- At most 1,024 unique name-only import/export rows are emitted.
- The complete text channel remains under the existing 64 KiB report-field contract.
- Resolver generation is based on indexed maps/sets rather than repeated whole-table scans.

## Integration

The analysis report grows from 70 to **71 fields** by adding `symbol_resolver`. The cache/engine identity is `arm64-analysis-v15-schema71`; this intentionally invalidates older cached reports. A new **Symbol Resolver** workspace participates in typed evidence indexing, address/symbol pivots, triage, and stage fingerprinting.

## Still incomplete

The resolver is not a replacement for a complete symbolication stack. The following remain future work:

- dSYM bundle ingestion and UUID matching;
- DWARF DIE / line-program / inline-frame decoding;
- dyld shared-cache local-symbol and subcache symbol recovery;
- runtime slide composition with the static resolver;
- Objective-C method IMP-to-selector synthesis;
- Swift demangling/thunk/witness classification; and
- universal-binary per-slice symbol-source selection.
