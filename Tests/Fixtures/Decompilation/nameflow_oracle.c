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
        // Preserve arbitrary FP bit patterns, including negative zero and NaNs;
        // the fixture performs moves/spills only, not FP arithmetic.
        uint64_t bits = i == 0 ? 0 : i == 1 ? UINT64_C(0x8000000000000000) :
            i == 2 ? UINT64_C(0x7ff8000000001234) : seed;
        double value; memcpy(&value, &bits, sizeof(value));
        double restored = hs_fp_frame(value);
        uint64_t actual; memcpy(&actual, &restored, sizeof(actual));
        if (actual != bits) return 4;
    }
    return 0;
}
