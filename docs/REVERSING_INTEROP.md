# Function-name exchange

`python/hindsight_interop.py` exchanges function names with Ghidra (PyGhidra /
Python 3), IDA, and Binary Ninja. It also converts named worker analysis evidence
into exchange JSON, and exchange JSON into a structural-backend symbol catalog.
The exchange contains the original artifact SHA-256, producer, source image base,
and function records with image-relative offsets, names and provenance. A name
from another tool remains a tool/analyst assertion.

Export a worker `analysis_result` JSON report with an explicit preferred base:

```sh
python3 python/hindsight_interop.py from-report analysis.json \
  --image-base 0x100000000 --output functions.json
```

Report input supports ARM64 start/end/size function rows and structural
`name=... source=...` rows. Unnamed ARM64 `<discovered>` entries are excluded.
JSON exports are deterministic by function offset. Existing output files are
never overwritten.

In the third-party Python console, add this repository's `python` directory to
`sys.path`, then select the adapter for the currently loaded program:

```python
import sys
sys.path.insert(0, "/absolute/path/to/HindSight/python")
import hindsight_interop as exchange

# Ghidra with PyGhidra:
adapter = exchange.Ghidra(currentProgram)
# Or IDA:
adapter = exchange.IDA()
# Or Binary Ninja: supply the current database's image base explicitly.
adapter = exchange.BinaryNinja(bv, "/absolute/path/original-binary", 0x100000000)

exchange.export_adapter(adapter, "/absolute/path/tool-functions.json")
plan = exchange.import_adapter(adapter, "/absolute/path/functions.json")
print(plan)
# Preview replacements of different names:
print(exchange.import_adapter(adapter, "/absolute/path/functions.json", replace=True))
# Apply those replacements after inspecting the plan:
result = exchange.import_adapter(adapter, "/absolute/path/functions.json", replace=True, dry_run=False)
```

Only exact existing function entries are renamed. Missing entries and conflicting
names are reported, and functions are never created or resized. Database image
bases translate relative offsets; source and destination hashes must match.
Duplicate offsets, 64-bit overflow, invalid names/provenance, and ambiguous Binary
Ninja platform entries are rejected. Planning finishes before mutations begin.
Ghidra and Binary Ninja group changes in undo transactions; IDA returns rejected
names individually and can retain earlier successful renames if a later API call
raises. Preserve the returned plan alongside the exchange for review.

Ghidra and IDA use the original input hash stored in the database. Binary Ninja
hashes its raw view and compares it to the supplied original artifact; patched
raw bytes fail that check. Exchanges have 20,000-function and 16 MiB JSON limits.

To bring exported names into HindSight's structural backend:

```sh
python3 python/hindsight_interop.py to-catalog tool-functions.json \
  --artifact /absolute/path/captured-binary --image-base 0x100000000 \
  --output tool-names.txt
HINDSIGHT_SYMBOL_DB=/absolute/path/tool-names.txt /absolute/path/reworker \
  --analyze-image /absolute/path/captured-binary SOURCE_SHA256 SIZE REQUEST_UUID \
  auto auto 0 0
```

Use the captured artifact's preferred base, SHA-256, byte size and request UUID.
Catalog generation verifies the original bytes, retains provenance comments, and
adds a SHA-256 binding rechecked by the worker. Existing nlist and export-trie
names retain precedence. Catalogs currently affect the structural backend (ELF,
PE, Intel/ARM32 Mach-O, raw/composite mappings); the legacy ARM64 semantic path
does not ingest them. This limitation is explicit rather than silently replacing
semantic analysis with structural analysis. GUI import, type/signature/comment
exchange, and native tool project parsing remain future work.

Tests exercise format validation, export/import round trips, rebasing, conflict
preview, write ordering, hash mismatches, budget enforcement, tool-adapter
contracts and worker catalog admission. Live Ghidra/IDA/Binary Ninja integration
has not been qualified on this host.

API references: [Ghidra Program](https://ghidra.re/ghidra_docs/api/ghidra/program/model/listing/Program.html),
[IDAPython input identity](https://python.docs.hex-rays.com/ida_nalt/index.html),
[Binary Ninja views and undo](https://api.binary.ninja/binaryninja.binaryview-module.html).
