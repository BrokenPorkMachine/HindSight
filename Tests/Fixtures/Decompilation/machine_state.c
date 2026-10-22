/* CC0 / public-domain qualification source. Never an analysis input oracle. */
#include <stdint.h>
#include <string.h>
#include "machine_state.h"
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
__attribute__((noinline)) uint64_t hs_c_load(const uint64_t *value) { return *value; }
__attribute__((noinline)) uint32_t hs_c_load_volatile_u32(const volatile uint32_t *value) { return *value; }
__attribute__((noinline,optnone)) uint64_t hs_c_load_volatile_twice(const volatile uint64_t *value) { uint64_t first=*value; uint64_t second=*value; return first+second; }
__attribute__((noinline,optnone)) uint64_t hs_c_load_volatile_branch(const volatile uint64_t *value, uint64_t enabled) { if (enabled) { uint64_t first=*value; uint64_t second=*value; return first+second; } return 7; }
__attribute__((noinline)) int64_t hs_c_load_signed16(const int16_t *value) { return *value; }
__attribute__((noinline)) int64_t hs_c_load_signed32(const int32_t *value) { return *value; }
__attribute__((noinline)) int64_t hs_c_load_signed64(const int64_t *value) { return *value; }
__attribute__((noinline)) uint64_t hs_c_load_unsigned8(const uint8_t *value) { return *value; }
__attribute__((noinline)) uint64_t hs_c_load_unsigned16(const uint16_t *value) { return *value; }
__attribute__((noinline)) int64_t hs_c_load_signed_offset(const int8_t *value) { return value[1]; }
__attribute__((noinline)) uint64_t hs_c_load_word_offset(const uint32_t *value) { return value[1]; }
__attribute__((noinline)) uint64_t hs_c_load_dynamic(const uint8_t *base, uint64_t offset) { return base[offset]; }
__attribute__((noinline)) int64_t hs_c_load_dynamic_s16(const int16_t *base, uint64_t offset) { return base[offset]; }
__attribute__((noinline)) uint64_t hs_c_load_dynamic_u32(const uint32_t *base, uint64_t offset) { return base[offset]; }
__attribute__((noinline)) uint64_t hs_c_store(uint64_t *target, uint64_t value) { *target=value; return value; }
__attribute__((noinline)) uint16_t hs_c_store_u16(uint16_t *target, uint16_t value) { *target=value; return value; }
__attribute__((noinline)) uint32_t hs_c_store_u32(uint32_t *target, uint32_t value) { *target=value; return value; }
__attribute__((noinline)) int16_t hs_c_store_s16(int16_t *target, int16_t value) { *target=value; return value; }
__attribute__((noinline)) int32_t hs_c_store_s32(int32_t *target, int32_t value) { *target=value; return value; }
__attribute__((noinline)) uint64_t hs_c_store_volatile(volatile uint64_t *target, uint64_t value) { *target=value; return value; }
__attribute__((noinline)) uint64_t hs_c_store_volatile_twice(volatile uint64_t *target, uint64_t value) { *target=value; *target=value; return value; }
__attribute__((noinline)) uint64_t hs_c_store_twice(uint64_t *target, uint64_t value) { target[0]=value; target[2]=value+1; return value; }
__attribute__((noinline)) uint64_t hs_c_store_conditional(uint64_t *target, uint64_t value) { if(value) *target=value; return value; }
__attribute__((noinline)) uint64_t hs_c_store_dynamic(uint8_t *base, uint64_t offset, uint64_t value) { base[offset]=value; return value; }
__attribute__((noinline)) uint32_t hs_c_store_dynamic_u32(uint32_t *base, uint64_t offset, uint32_t value) { base[offset]=value; return value; }
__attribute__((noinline)) int16_t hs_c_store_dynamic_s16(int16_t *base, uint64_t offset, int16_t value) { base[offset]=value; return value; }
__attribute__((noinline)) uint64_t hs_c_store_then_load(uint64_t *target, const uint64_t *source, uint64_t value) { *target=value; return *source; }
__attribute__((noinline)) uint64_t hs_c_load_then_store(uint64_t *target, const uint64_t *source, uint64_t value) { uint64_t prior=*source; *target=value; return prior; }
__attribute__((noinline,optnone)) uint64_t hs_c_recursive(uint64_t n) { if (n < 2) return n + 1; return hs_c_recursive(n >> 1) + n * 3; }
__attribute__((noinline,optnone)) uint64_t hs_c_recursive_pair(uint64_t n) { if (n < 2) return n + 1; return hs_c_recursive_pair(n - 1) + hs_c_recursive_pair(n - 2); }
__attribute__((noinline,optnone)) uint64_t hs_c_call_helper(uint64_t value) { return hs_c_leaf_helper(value) * 3 + 7; }
__attribute__((noinline,optnone)) uint64_t hs_c_call_twice(uint64_t value) { uint64_t first=hs_c_leaf_helper(value); uint64_t second=hs_c_leaf_helper(first); return second; }
__attribute__((noinline,optnone)) uint64_t hs_c_store_then_call(uint64_t *target, uint64_t ignored, uint64_t value) { *target=value; return hs_c_leaf_helper(value); }
__attribute__((noinline,optnone)) uint64_t hs_c_volatile_store_then_call(volatile uint64_t *target, uint64_t value) { *target=value; return hs_c_leaf_helper(value); }
__attribute__((noinline,optnone)) uint64_t hs_c_call_then_volatile_store(volatile uint64_t *target, uint64_t value) { uint64_t result=hs_c_leaf_helper(value); *target=result; return result; }
__attribute__((noinline,optnone)) uint64_t hs_c_volatile_load_then_call(const volatile uint64_t *source, uint64_t value) { uint64_t loaded=*source; uint64_t result=hs_c_leaf_helper(value); return loaded^result; }
__attribute__((noinline,optnone)) uint64_t hs_c_call_then_volatile_load(const volatile uint64_t *source, uint64_t value) { uint64_t result=hs_c_leaf_helper(value); uint64_t loaded=*source; return loaded^result; }
__attribute__((noinline,optnone)) uint64_t hs_c_volatile_store_call_load(volatile uint64_t *target, uint64_t value) { *target=value; uint64_t result=hs_c_leaf_helper(value); uint64_t loaded=*target; return loaded^result; }
__attribute__((noinline,optnone)) uint64_t hs_c_store_load_then_call(uint64_t *target, const uint64_t *source, uint64_t value) { *target=value; uint64_t prior=*source; return hs_c_leaf_helper(prior); }
__attribute__((noinline,optnone)) uint64_t hs_c_branch_store_call(uint64_t *target, uint64_t ignored, uint64_t value) { if(value&1) { *target=value; return hs_c_leaf_helper(value); } return hs_c_leaf_helper(value+1); }
__attribute__((noinline,optnone)) uint64_t hs_c_call_then_store(uint64_t *target, uint64_t ignored, uint64_t value) { uint64_t result=hs_c_leaf_helper(value); *target=result; return result; }
__attribute__((noinline,optnone)) uint64_t hs_c_call_then_store_branch(uint64_t *target, uint64_t ignored, uint64_t value) {
    if (value & 1) { uint64_t result=hs_c_leaf_helper(value); *target=result; return result; }
    uint64_t result=hs_c_leaf_helper(value+1); *target=result; return result;
}
__attribute__((noinline,optnone)) uint64_t hs_c_conditional_call_store(uint64_t *target, uint64_t ignored, uint64_t value) {
    if (value & 1) { uint64_t result=hs_c_leaf_helper(value); *target=result; return result; }
    *target=value;
    return value;
}
__attribute__((noinline,optnone)) uint64_t hs_c_call_then_load(const uint64_t *source, uint64_t value) {
    uint64_t result=hs_c_leaf_helper(value);
    return *source+result;
}
__attribute__((noinline,optnone)) uint64_t hs_c_call_load_store(uint64_t *target, const uint64_t *source, uint64_t value) {
    uint64_t result=hs_c_leaf_helper(value);
    uint64_t prior=*source;
    uint64_t combined=prior+result;
    *target=combined;
    return combined;
}
__attribute__((noinline,optnone)) uint64_t hs_c_recursive_store(uint64_t *target, uint64_t n) { if (n < 2) { *target=n; return n; } uint64_t value=hs_c_recursive_store(target,n >> 1); *target=value; return value; }
__attribute__((noinline)) uint64_t hs_c_conditional_store_then_load(uint64_t *target, const uint64_t *source, uint64_t value) { if(value) *target=value; return *source; }
__attribute__((noinline)) uint64_t hs_c_alternate_store_then_load(uint64_t *target, const uint64_t *source, uint64_t value) { if(value&1) *target=value; else *target=value+1; return *source; }
