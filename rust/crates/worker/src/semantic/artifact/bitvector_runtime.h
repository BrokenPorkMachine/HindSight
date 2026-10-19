/* C17 unsigned bitvector primitives shared by strict and recovered output. */
#include <stdint.h>
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

static inline uint64_t hs_flags_arith(uint64_t left, uint64_t right, uint64_t result, unsigned bits, int is_sub) {
    uint64_t mask = hs_mask(bits);
    unsigned n = (unsigned)((result >> (bits - 1)) & 1);
    unsigned z = (result == 0) ? 1 : 0;
    unsigned c, v;
    if (!is_sub) {
        c = (left > mask - right) ? 1 : 0;
        v = (unsigned)((((~(left ^ right)) & (left ^ result)) >> (bits - 1)) & 1);
    } else {
        c = (left >= right) ? 1 : 0;
        v = (unsigned)((((left ^ right) & (left ^ result)) >> (bits - 1)) & 1);
    }
    return ((uint64_t)n << 3) | ((uint64_t)z << 2) | ((uint64_t)c << 1) | v;
}
