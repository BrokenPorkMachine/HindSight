// Original HindSight qualification fixture; CC0 / public domain.
#include "nameflow_sample.h"
#include <stdint.h>
#include <string.h>

int main(void) {
    uint64_t seed = UINT64_C(0x8461d9c30fbe527a);
    for (unsigned i = 0; i < 260; ++i) {
        seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        unsigned long original = i == 0 ? 0 : i == 1 ? ~0UL : seed;
        unsigned long pair[2] = {original, original ^ 0x81ea2b93UL};
        if (hs_copy_chain(original) != original || hs_frame_spill(original) != original) return 1;
        if (hs_pair_spill(pair[0], pair[1]) != pair[0] + pair[1]) return 2;
        if (hs_preindexed(pair) != pair[1] || hs_load_overlap(pair) != pair[0] + pair[1]) return 3;
        if (hs_csel_same(original) != original) return 5;
        if (hs_xor_zero(original) != original) return 7;
        if (hs_bic_zero(original) != original) return 8;
        if (hs_w_overwrite(original) != 1) return 9;
        if (hs_add_sub_zero(original) != original) return 10;
        if (hs_shifted_zero_ops(original) != original) return 11;
        if (hs_zero_distance_shift(original) != original) return 12;
        if (hs_w_add_sub_zero((uint32_t)original) != (uint32_t)original) return 13;
        if (hs_call_preserved(original) != original) return 14;
        // Preserve arbitrary FP bit patterns, including negative zero and NaNs;
        // the fixture performs moves/spills only, not FP arithmetic.
        uint64_t bits = i == 0 ? 0 : i == 1 ? UINT64_C(0x8000000000000000) :
            i == 2 ? UINT64_C(0x7ff8000000001234) : seed;
        double value; memcpy(&value, &bits, sizeof(value));
        double restored = hs_fp_frame(value);
        uint64_t actual; memcpy(&actual, &restored, sizeof(actual));
        if (actual != bits) return 4;
        double selected = hs_fcsel_same(value);
        memcpy(&actual, &selected, sizeof(actual));
        if (actual != bits) return 6;
        double called = hs_call_preserved_fp(value);
        memcpy(&actual, &called, sizeof(actual));
        if (actual != bits) return 15;
    }
    return 0;
}
