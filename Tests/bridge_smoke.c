#include "HindSightBridge.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>
int main(void) {
    assert(reworkbench_abi_version() == 1);
    assert(strstr(reworkbench_capabilities_json(), "\"analysis\":false") != 0);
    uint64_t result = 99;
    assert(reworkbench_checked_address_add(0xfffffff000000000ULL, 8, &result));
    assert(result == 0xfffffff000000008ULL);
    assert(!reworkbench_checked_address_add(UINT64_MAX, 1, &result));
    assert(result == 0xfffffff000000008ULL);
    assert(!reworkbench_checked_address_add(1, 2, 0));
    VectorGlassGovernor *g = reworkbench_governor_new(128, 1);
    assert(g);
    uint64_t lease = reworkbench_governor_admit(g, 128);
    assert(lease && reworkbench_governor_may_publish(g, lease));
    assert(reworkbench_governor_admit(g, 1) == 0);
    assert(reworkbench_governor_revoke(g, lease));
    assert(!reworkbench_governor_may_publish(g, lease));
    reworkbench_governor_free(g);
    return 0;
}
