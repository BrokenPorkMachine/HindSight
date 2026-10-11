#if defined(__APPLE__)
/* Expose Darwin extensions used below (st_*timespec and O_NOFOLLOW). */
#ifndef _DARWIN_C_SOURCE
#define _DARWIN_C_SOURCE 1
#endif
#else
#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 200809L
#endif
#endif
#include "cache.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <inttypes.h>
#include <sys/stat.h>
struct Entry {struct Entry *previous,*next;size_t key_size,value_size;uint8_t bytes[];};
struct REPageCache {struct Entry *first,*last;size_t bytes,count,byte_limit,entry_limit;};
static void detach(REPageCache*c,struct Entry*e){
 if(e->previous)e->previous->next=e->next;else c->first=e->next;
 if(e->next)e->next->previous=e->previous;else c->last=e->previous;
}
static void append(REPageCache*c,struct Entry*e){
 e->previous=c->last;e->next=NULL;
 if(c->last)c->last->next=e;else c->first=e;c->last=e;
}
static void discard(REPageCache*c,struct Entry*e){detach(c,e);c->bytes-=sizeof(*e)+e->key_size+e->value_size;c->count--;free(e);}
static struct Entry*find(REPageCache*c,const uint8_t*key,size_t n){
 for(struct Entry*e=c->first;e;e=e->next){if(e->key_size==n && !memcmp(e->bytes,key,n))return e;}return NULL;
}
REPageCache*re_cache_new(size_t bytes,size_t entries){
 if(!bytes||bytes>1024*1024||!entries||entries>16)return NULL;
 REPageCache*c=calloc(1,sizeof(*c));if(c){c->byte_limit=bytes;c->entry_limit=entries;}return c;
}
void re_cache_clear(REPageCache*c){if(c){while(c->first)discard(c,c->first);}}
void re_cache_free(REPageCache*c){re_cache_clear(c);free(c);}
size_t re_cache_bytes(const REPageCache*c){return c?c->bytes:0;}
size_t re_cache_count(const REPageCache*c){return c?c->count:0;}
int re_cache_put(REPageCache*c,const uint8_t*key,size_t n,const uint8_t*value,size_t size){
 if(!c||!key||!value||!n||n>32768||!size||size>131072)return 0;
 size_t cost=sizeof(struct Entry)+n+size;if(cost>c->byte_limit)return 0;
 struct Entry*e=malloc(cost);if(!e)return 0;
 e->key_size=n;e->value_size=size;memcpy(e->bytes,key,n);memcpy(e->bytes+n,value,size);
 struct Entry*old=find(c,key,n);if(old)discard(c,old);
 while(c->first && (c->count>=c->entry_limit || c->bytes>c->byte_limit-cost)){discard(c,c->first);}
 append(c,e);c->bytes+=cost;c->count++;return 1;
}
const uint8_t*re_cache_get(REPageCache*c,const uint8_t*key,size_t n,size_t*size){
 if(!size)return NULL;
 *size=0;if(!c||!key||!n||n>32768)return NULL;
 struct Entry*e=find(c,key,n);if(!e)return NULL;
 detach(c,e);append(c,e);*size=e->value_size;return e->bytes+e->key_size;
}
int re_file_stamp(const char*path,char*out,size_t capacity){
 if(!path||!out||!capacity)return -1;
 out[0]=0;struct stat s;
 if(lstat(path,&s)||!S_ISREG(s.st_mode)||s.st_size<0)return -1;
#ifdef __APPLE__
 int64_t mt=s.st_mtimespec.tv_sec,mn=s.st_mtimespec.tv_nsec,ct=s.st_ctimespec.tv_sec,cn=s.st_ctimespec.tv_nsec;
#else
 int64_t mt=s.st_mtim.tv_sec,mn=s.st_mtim.tv_nsec,ct=s.st_ctim.tv_sec,cn=s.st_ctim.tv_nsec;
#endif
 int n=snprintf(out,capacity,"%" PRIuMAX ":%" PRIuMAX ":%" PRIuMAX ":%" PRId64 ":%" PRId64 ":%" PRId64 ":%" PRId64,
 (uintmax_t)s.st_dev,(uintmax_t)s.st_ino,(uintmax_t)s.st_size,mt,mn,ct,cn);
 return n<0||(size_t)n>=capacity?-1:0;
}
