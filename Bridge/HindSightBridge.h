#ifndef REWORKBENCH_BRIDGE_H
#define REWORKBENCH_BRIDGE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Process-lifetime immutable UTF-8 JSON. Borrowed: never free or modify it. */
const char *reworkbench_capabilities_json(void);
uint32_t reworkbench_abi_version(void);
/* Returns false on overflow or null output. Caller supplies a valid aligned output. */
bool reworkbench_checked_address_add(uint64_t base, uint64_t offset, uint64_t *output);
typedef struct VectorGlassGovernor VectorGlassGovernor;
/* Queue-confined owner. Never share this handle concurrently or use it after free. */
VectorGlassGovernor *reworkbench_governor_new(uint64_t budget_mib, size_t workers);
void reworkbench_governor_free(VectorGlassGovernor *governor);
uint64_t reworkbench_governor_admit(VectorGlassGovernor *governor, uint64_t mib);
bool reworkbench_governor_revoke(VectorGlassGovernor *governor, uint64_t lease);
bool reworkbench_governor_may_publish(VectorGlassGovernor *governor, uint64_t lease);
typedef struct VectorGlassPageCache VectorGlassPageCache;
/* Queue-confined cache; borrowed values must be copied before any mutation/free. */
VectorGlassPageCache *reworkbench_cache_new(size_t bytes, size_t entries);
void reworkbench_cache_free(VectorGlassPageCache *cache);
void reworkbench_cache_clear(VectorGlassPageCache *cache);
int32_t reworkbench_cache_put(VectorGlassPageCache *cache,const uint8_t *key,size_t key_size,const uint8_t *value,size_t value_size);
const uint8_t *reworkbench_cache_get(VectorGlassPageCache *cache,const uint8_t *key,size_t key_size,size_t *value_size);
int32_t reworkbench_file_stamp(const char *path,char *out,size_t capacity);
#include "../native/debug/debug_server.h"
#include "../native/virtualization/hv_vm.h"
#include "../native/macho/decode.h"

#ifdef __cplusplus
}
#endif
#endif
