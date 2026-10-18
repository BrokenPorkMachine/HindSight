/* CC0. Analyst-supplied declarations; function bodies are not recovery inputs. */
#include <stdint.h>
uint64_t hs_c_mix(uint64_t first, uint64_t second, uint64_t multiplier, uint64_t shifted, uint64_t shift);
uint64_t hs_c_select(uint64_t first, uint64_t second);
uint64_t hs_c_stack(uint64_t first, uint64_t second);
int64_t hs_c_signed(int64_t first, int64_t second);
int32_t hs_c_narrow(int32_t first, int32_t second);
uint32_t hs_c_u32(uint32_t first, uint32_t second);
uint64_t hs_c_nested(uint64_t first, uint64_t second, uint64_t alternate);
uint64_t hs_m_signed_div(uint64_t dividend, uint64_t divisor);
uint64_t hs_m_word(uint64_t first, uint64_t second);
uint64_t hs_m_rotate(uint64_t value, uint64_t amount);
uint64_t hs_m_pair(uint64_t first, uint64_t second);
uint64_t hs_m_keep(uint64_t value);
uint64_t hs_m_flagwrite(uint64_t value);
uint64_t hs_m_addflags(uint64_t first, uint64_t second);
uint64_t hs_m_andflags(uint64_t first, uint64_t second);
uint64_t hs_m_loop(uint64_t initial, uint64_t iterations);
uint64_t hs_m_memory(uint64_t address, uint64_t value);
