# External XNU/SEP symbol catalogs

HindSight accepts an analyst-supplied symbol catalog through the opt-in
`HINDSIGHT_SYMBOL_DB` environment variable. This is the supported way to use
private XNU or SEP symbols without embedding proprietary databases in the
application.

The file is UTF-8 text with one record per line:

```text
0xfffffff007004000 _start
0xfffffff007004120 kernel_task
```

Rules:

- addresses must be hexadecimal with a `0x` prefix;
- names are bounded to 256 bytes and comments begin with `#`;
- only addresses inside validated executable sections are accepted;
- ARM32 Thumb address bit 0 is normalized;
- nlist and export-trie names always outrank catalog names;
- accepted names carry `source=external-symbol-db` provenance;
- the file is capped at 16 MiB and 200,000 lines and is never downloaded automatically;
- an optional `# source_sha256=<64 hex digits>` comment binds the catalog to the
  verified captured artifact; a mismatch rejects analysis before names are applied.

Example:

```bash
HINDSIGHT_SYMBOL_DB=/absolute/path/xnu-symbols.txt \
  ./build/worker/reworker --analyze-image ...
```

This provides authoritative naming when the analyst supplies an authoritative
catalog. It does not manufacture a complete Apple private XNU/SEP database.

Catalogs apply to the structural analysis backend. The legacy ARM64 semantic
backend does not read them. Function-name exchanges from other reversing tools
can produce hash-bound catalogs; see [function exchange](REVERSING_INTEROP.md).

## Project C prototype catalogs

The Mach-O semantic backend accepts a separate, opt-in C prototype catalog via
`HINDSIGHT_PROTOTYPE_DB`. This file must contain a matching SHA-256 binding and
uses a tab between the exact Mach-O symbol and its declaration:

```text
# source_sha256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
_custom_parse	int custom_parse(const char *bytes, size_t length);
```

The catalog is capped at 4 MiB and 20,000 lines. Symbols and declarations are
bounded and validated; control characters, bodies, initializers, quoted text,
preprocessor text, and unsupported nested declarators are rejected. The symbol
must match the function name after the Mach-O leading underscore is removed.
Catalog prototypes apply only when that exact symbol exists in the analyzed
image. Exact built-in SDK declarations have higher precedence. Unknown parameter
layouts stop subsequent ABI-location claims rather than guessing them.
