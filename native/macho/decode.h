#ifndef RE_ARM64_DECODE_H
#define RE_ARM64_DECODE_H
#include <stdint.h>
#include <stddef.h>
/* 1 decoded, 0 unsupported or unrepresentable target, -1 invalid/output too small.
   All other words remain .inst; no inference of code/data or instruction validity. */
int re_arm64_decode(uint32_t word, uint64_t pc, char *out, size_t capacity);
int re_page_range(uint64_t offset,uint64_t address,uint64_t bytes,uint64_t start,
                  uint64_t *position,uint64_t *pc,uint32_t *count);
#endif
