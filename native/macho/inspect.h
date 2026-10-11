#ifndef RE_MACHO_INSPECT_H
#define RE_MACHO_INSPECT_H
#include <stddef.h>
#include <stdint.h>
/* data contains the 32-byte header and all load-command bytes, not the whole file.
   Return 0: supported range checks passed, 1: unsupported format, -1: invalid/limited.
   out is always terminated if capacity > 0. Never validates signatures or instruction semantics. */
int re_macho_inspect(const uint8_t *data, size_t length, uint64_t file_size, char *out, size_t capacity);
/* Select the single regular __text instruction section in a loaded ARM64 image.
   Return 0 when code bytes are statically decodable, 2 when the mapping is valid
   but LC_ENCRYPTION_INFO(_64) declares encrypted code, 1 for unsupported input,
   and -1 for malformed/bounded-validation failure. LC_FUNCTION_STARTS and
   LC_DATA_IN_CODE are metadata and never veto an otherwise valid mapping. */
int re_macho_code_range(const uint8_t *data, size_t length, uint64_t file_size,
                       uint64_t *offset, uint64_t *address, uint64_t *size,
                       char *reason, size_t capacity);
size_t re_macho_window_size(const uint8_t *header,size_t length,uint64_t file_size);
#endif
