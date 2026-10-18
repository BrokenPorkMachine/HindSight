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
static inline uint64_t hs_mask(unsigned bits) {
    return bits == 64 ? UINT64_MAX : (UINT64_C(1) << bits) - 1;
}
static inline uint64_t hs_extend(uint64_t v, unsigned bits, unsigned sign) {
    uint64_t mask = hs_mask(bits), top = UINT64_C(1) << (bits-1);
    v &= mask;
    return sign && (v & top) ? v | ~mask : v;
}
static inline uint64_t hs_shift(uint64_t v, uint64_t n, unsigned bits, unsigned kind) {
    v &= hs_mask(bits); n &= bits-1;
    if (!n) return v;
    if (kind == 0) return (v << n) & hs_mask(bits);
    if (kind == 1) return v >> n;
    if (kind == 3) return ((v >> n) | (v << (bits-n))) & hs_mask(bits);
    return ((v >> n) | ((v >> (bits-1)) ? hs_mask(bits) << (bits-n) : 0)) & hs_mask(bits);
}
static inline uint64_t hs_div(uint64_t a, uint64_t b, unsigned bits, unsigned sign) {
    uint64_t mask = hs_mask(bits), top = UINT64_C(1) << (bits-1);
    a &= mask; b &= mask;
    if (!b) return 0;
    unsigned na = sign && (a & top), nb = sign && (b & top);
    uint64_t ua = na ? (UINT64_C(0)-a) & mask : a;
    uint64_t ub = nb ? (UINT64_C(0)-b) & mask : b;
    uint64_t q = ua / ub;
    return ((na != nb) ? UINT64_C(0)-q : q) & mask;
}
static inline unsigned hs_condition(unsigned f, unsigned c) {
    unsigned n=(f>>3)&1, z=(f>>2)&1, carry=(f>>1)&1, v=f&1;
    switch(c) {
        case 0:return z; case 1:return !z; case 2:return carry; case 3:return !carry;
        case 4:return n; case 5:return !n; case 6:return v; case 7:return !v;
        case 8:return carry&&!z; case 9:return !carry||z; case 10:return n==v;
        case 11:return n!=v; case 12:return !z&&(n==v); case 13:return z||(n!=v);
        default:return 1;
    }
}
static inline uint8_t hs_compare(uint64_t a, uint64_t b, unsigned bits, unsigned kind) {
    uint64_t mask=hs_mask(bits), top=UINT64_C(1)<<(bits-1), r;
    unsigned c=0,v=0;
    a &= mask; b &= mask;
    if(kind==2) r=a&b;
    else if(kind==0) {r=(a-b)&mask;c=a>=b;v=((a^b)&(a^r)&top)!=0;}
    else {r=(a+b)&mask;c=bits==64 ? r<a : a+b>mask;v=(~(a^b)&(a^r)&top)!=0;}
    return (uint8_t)(((r&top)!=0)*8+(r==0)*4+c*2+v);
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
