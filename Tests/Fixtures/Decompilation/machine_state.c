/* CC0 / public-domain qualification source. Never an analysis input oracle. */
#include <stdint.h>
#include <string.h>
__attribute__((noinline)) uint64_t hs_c_mix(uint64_t a,uint64_t b,uint64_t c,uint64_t d,uint64_t e) {
    return ((a ^ b) + c*3) ^ (d >> (e&63));
}
__attribute__((noinline)) uint64_t hs_c_select(uint64_t a,uint64_t b) {
    return a < b ? a+7 : b-3;
}
__attribute__((noinline)) uint64_t hs_c_stack(uint64_t a,uint64_t b) {
    volatile uint64_t values[2]; values[0]=a; values[1]=b;
    return (values[0]+3)^values[1];
}
__attribute__((noinline)) int64_t hs_c_signed(int64_t a,int64_t b) {
    uint64_t bits=(uint64_t)a ^ (uint64_t)b; int64_t result;
    memcpy(&result,&bits,sizeof result); return result;
}
__attribute__((noinline)) int32_t hs_c_narrow(int32_t a,int32_t b) { return (a^b)>>3; }
__attribute__((noinline)) uint32_t hs_c_u32(uint32_t a,uint32_t b) { return (a+b)^(a>>(b&31)); }
__attribute__((noinline)) uint64_t hs_c_nested(uint64_t a,uint64_t b,uint64_t c) {
    if(a==b)return c^a;
    if(a>b)return a+c;
    return b-c;
}
