#ifndef RE_PAGE_CACHE_H
#define RE_PAGE_CACHE_H
#include <stddef.h>
#include <stdint.h>
typedef struct REPageCache REPageCache;
REPageCache *re_cache_new(size_t bytes,size_t entries);
void re_cache_free(REPageCache *cache);
void re_cache_clear(REPageCache *cache);
int re_cache_put(REPageCache *cache,const uint8_t *key,size_t key_size,const uint8_t *value,size_t value_size);
/* Borrowed until any mutation/free, single queue only. A miss writes size=0. */
const uint8_t *re_cache_get(REPageCache *cache,const uint8_t *key,size_t key_size,size_t *size);
size_t re_cache_bytes(const REPageCache *cache);
size_t re_cache_count(const REPageCache *cache);
int re_file_stamp(const char *path,char *out,size_t capacity);
#endif
