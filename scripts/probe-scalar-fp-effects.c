// Classifies the architectural effect of scalar floating-point instructions on
// the upper bits of their destination vector register.
//
// Every case starts from a destination whose 128 bits are a known pattern and
// whose first source carries a different known pattern, so the result says
// exactly where the upper bits came from:
//   preserved           the destination's own upper bits survive
//   zeroed              the write cleared the upper bits
//   merged-from-source  the upper bits came from the first source operand
//   other               the upper bits are a computed vector result
//
// Run through scripts/test-scalar-fp-effects.sh. The script needs an
// AArch64 host because the classification is a property of the machine.
#include <stdio.h>
#include <stdint.h>

#define DEST_LO 0x1111111111111111ULL
#define DEST_HI 0x2222222222222222ULL
#define SRC_LO  0x3333333333333333ULL
#define SRC_HI  0x4444444444444444ULL

static uint64_t dest[2] = {DEST_LO, DEST_HI};
static uint64_t src[2]  = {SRC_LO,  SRC_HI};
static uint64_t out[2];

static const char *classify(uint64_t high) {
    if (high == DEST_HI) return "preserved";
    if (high == 0) return "zeroed";
    if (high == SRC_HI) return "merged-from-source";
    return "other";
}

#define RUN(name, body) \
    do { \
        out[0] = out[1] = 0; \
        __asm__ volatile( \
            "ldr q0, [%[d]]\n" \
            "ldr q1, [%[s]]\n" \
            "ldr q2, [%[s]]\n" \
            "movi v3.2d, #0\n" \
            "movi v4.2d, #0\n" \
            body \
            "str q0, [%[o]]\n" \
            : : [d]"r"(dest), [s]"r"(src), [o]"r"(out) \
            : "v0", "v1", "v2", "v3", "v4", "memory"); \
        printf("%-30s high=0x%016llx  %s\n", name, \
               (unsigned long long)out[1], classify(out[1])); \
    } while (0)

int main(void) {
    // The sources carry zero in their low half and the pattern in the high
    // half, so every operation below is defined and any merge is visible.
    src[0] = 0; src[1] = SRC_HI;

    RUN("fadd d0, d1, d2", "fadd d0, d1, d2\n");
    RUN("fsub d0, d1, d2", "fsub d0, d1, d2\n");
    RUN("fmul d0, d1, d2", "fmul d0, d1, d2\n");
    RUN("fdiv d0, d1, d2", "fdiv d0, d1, d2\n");
    RUN("fmadd d0, d1, d2, d3", "fmadd d0, d1, d2, d3\n");
    RUN("fmsub d0, d1, d2, d3", "fmsub d0, d1, d2, d3\n");
    RUN("fnmadd d0, d1, d2, d3", "fnmadd d0, d1, d2, d3\n");
    RUN("fabs d0, d1", "fabs d0, d1\n");
    RUN("fneg d0, d1", "fneg d0, d1\n");
    RUN("fsqrt d0, d1", "fsqrt d0, d1\n");
    RUN("frinta d0, d1", "frinta d0, d1\n");
    RUN("fcsel d0, d1, d2, eq", "fcsel d0, d1, d2, eq\n");
    RUN("fmov d0, d1", "fmov d0, d1\n");
    RUN("fcvt d0, s1", "fcvt d0, s1\n");
    RUN("fcvt s0, d1", "fcvt s0, d1\n");
    RUN("scvtf d0, x3", "scvtf d0, x3\n");
    RUN("ucvtf d0, x3", "ucvtf d0, x3\n");
    RUN("fmov d0, x3", "fmov d0, x3\n");
    RUN("fadd s0, s1, s2", "fadd s0, s1, s2\n");
    RUN("fcvt h0, s1", "fcvt h0, s1\n");
    RUN("fadd h0, h1, h2", "fadd h0, h1, h2\n");
    RUN("fmov h0, h1", "fmov h0, h1\n");
    // Immediate forms: a scalar immediate write is also a whole-register
    // write on this machine.
    RUN("fmov d0, #1.0", "fmov d0, #1.0\n");
    RUN("fmov s0, #1.0", "fmov s0, #1.0\n");
    RUN("fmov h0, #1.0", "fmov h0, #1.0\n");
    // The vector forms for contrast: a 64-bit vector form clears the upper
    // half, while a 128-bit vector form computes every lane.
    RUN("fadd v0.2s, v1.2s, v2.2s", "fadd v0.2s, v1.2s, v2.2s\n");
    RUN("fadd v0.2d, v1.2d, v2.2d", "fadd v0.2d, v1.2d, v2.2d\n");
    RUN("fadd v0.4s, v1.4s, v2.4s", "fadd v0.4s, v1.4s, v2.4s\n");
    return 0;
}
