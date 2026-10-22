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
static inline uint64_t hs_add_carry(hs_state *s, uint64_t a, uint64_t b, unsigned carry, unsigned bits, unsigned is_sub, unsigned set_flags) {
    uint64_t mask=hs_mask(bits), sign=UINT64_C(1)<<(bits-1);
    a&=mask;b&=mask;
    uint64_t operand=is_sub?(~b&mask):b;
    uint64_t partial=a+operand, wide=partial+(carry&1u);
    unsigned carry_out=bits==64?((partial<a)||(wide<partial)):((wide>>bits)&1u);
    uint64_t result=wide&mask;
    if(set_flags) {
        unsigned n=(unsigned)((result&sign)!=0),z=(unsigned)(result==0),v;
        if(is_sub)v=(unsigned)((((a^b)&(a^result)&sign)!=0));
        else v=(unsigned)((((~(a^b))&(a^result)&sign)!=0));
        s->nzcv=(uint8_t)((n<<3)|(z<<2)|(carry_out<<1)|v);
    }
    return result;
}
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
