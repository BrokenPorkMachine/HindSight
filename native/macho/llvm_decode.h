#ifndef HINDSIGHT_LLVM_DECODE_H
#define HINDSIGHT_LLVM_DECODE_H
#include <stddef.h>
#include <stdint.h>
/* Architectures: 1=AArch64, 2=i386, 3=x86_64, 4=ARM, 5=Thumb.
   Context owns its LLVM decoder; library stays loaded for process lifetime. */
void *hs_decoder_new(unsigned architecture);
size_t hs_decoder_instruction(void *context, const uint8_t *bytes, size_t length,
                              uint64_t address, char *output, size_t capacity);
void hs_decoder_free(void *context);
#endif
