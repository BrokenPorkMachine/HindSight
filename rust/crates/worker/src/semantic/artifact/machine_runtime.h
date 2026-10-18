/* HindSight strict machine-state C17 runtime, bounded-flat-memory-v1.
 * Unsigned bitvectors only: no signed overflow or implementation-defined shifts.
 * Return statuses: 0 RET, 1 invalid PC, 2 step budget, 3 memory fault.
 * Faults are model outcomes, not a claim of precise AArch64 exception state.
 */
#include <stdint.h>
#define HS_MEM_BASE UINT64_C(0x100000)
#define HS_MEM_SIZE 4096u
typedef struct {
    uint64_t x[31], sp, pc, steps;
    uint8_t nzcv, memory[HS_MEM_SIZE];
    int fault;
} hs_state;
static inline unsigned hs_range(hs_state *s, uint64_t a, unsigned bytes) {
    if(a < HS_MEM_BASE || a-HS_MEM_BASE > HS_MEM_SIZE || bytes > HS_MEM_SIZE-(a-HS_MEM_BASE)) {s->fault=1;return 0;}
    return 1;
}
static inline uint64_t hs_load(hs_state *s, uint64_t a, unsigned bytes, unsigned sign) {
    uint64_t v=0;
    if(!hs_range(s,a,bytes))return 0;
    for(unsigned i=0;i<bytes;i++)v|=(uint64_t)s->memory[a-HS_MEM_BASE+i]<<(8*i);
    return hs_extend(v,bytes*8,sign);
}
static inline void hs_store(hs_state *s, uint64_t a, uint64_t v, unsigned bytes) {
    if(!hs_range(s,a,bytes))return;
    for(unsigned i=0;i<bytes;i++)s->memory[a-HS_MEM_BASE+i]=(uint8_t)(v>>(8*i));
}
