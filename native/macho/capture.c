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
#include "capture.h"
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef __APPLE__
#include <CommonCrypto/CommonDigest.h>
typedef CC_SHA256_CTX Hash;
static int init(Hash*h){return CC_SHA256_Init(h);}
static int update(Hash*h,const void*p,size_t n){return CC_SHA256_Update(h,p,(CC_LONG)n);}
static int finish(Hash*h,unsigned char*out){return CC_SHA256_Final(out,h);}
static void release(Hash*h){(void)h;}
#else
#include <openssl/evp.h>
typedef EVP_MD_CTX* Hash;
static int init(Hash*h){*h=EVP_MD_CTX_new();return *h && EVP_DigestInit_ex(*h,EVP_sha256(),NULL);}
static int update(Hash*h,const void*p,size_t n){return EVP_DigestUpdate(*h,p,n);}
static int finish(Hash*h,unsigned char*out){unsigned n=0;return EVP_DigestFinal_ex(*h,out,&n)&&n==32;}
static void release(Hash*h){if(*h)EVP_MD_CTX_free(*h);}
#endif
static int room(int fd,uint64_t required){struct statvfs v;if(fstatvfs(fd,&v))return 0;long double available=(long double)v.f_bavail*v.f_frsize;return available >= required;}
int re_sha256_bytes(const unsigned char *data,size_t size,char digest[65]){
 if(!digest||(!data&&size)||size>32u*1024u*1024u)return -1;
 Hash hash={0};int result=-1;unsigned char bytes[32];digest[0]=0;
 if(!init(&hash))goto done;
 if(!update(&hash,data,size)||!finish(&hash,bytes))goto done;
 for(size_t i=0;i<32;i++)snprintf(digest+2*i,3,"%02x",bytes[i]);
 digest[64]=0;result=0;
done: release(&hash);return result;
}
static int same(const struct stat*a,const struct stat*b){
 if(a->st_size!=b->st_size || a->st_ino!=b->st_ino || a->st_dev!=b->st_dev)return 0;
#ifdef __APPLE__
 return a->st_mtimespec.tv_sec==b->st_mtimespec.tv_sec && a->st_mtimespec.tv_nsec==b->st_mtimespec.tv_nsec && a->st_ctimespec.tv_sec==b->st_ctimespec.tv_sec && a->st_ctimespec.tv_nsec==b->st_ctimespec.tv_nsec;
#else
 return a->st_mtim.tv_sec==b->st_mtim.tv_sec && a->st_mtim.tv_nsec==b->st_mtim.tv_nsec && a->st_ctim.tv_sec==b->st_ctim.tv_sec && a->st_ctim.tv_nsec==b->st_ctim.tv_nsec;
#endif
}
int re_capture(const char*source,const char*destination,uint64_t maximum,uint64_t reserve,char digest[65],char*error,size_t capacity){
 int in=-1,out=-1,owned=0,result=-1;unsigned char*buffer=NULL;Hash hash={0};int hash_ready=0;
 const char*message="capture failed";struct stat before,after;uint64_t total=0;
 if(!source||!destination||!digest||!error||!capacity)return -1;
 error[0]=0;digest[0]=0;
 in=open(source,O_RDONLY|O_NONBLOCK|O_NOFOLLOW);if(in<0){message="Cannot open source (symlinks are not accepted).";goto done;}
 if(fstat(in,&before)||!S_ISREG(before.st_mode)||before.st_size<0){message="Source must be a regular file.";goto done;}
 if((uint64_t)before.st_size>maximum){message="Source exceeds capture size limit.";goto done;}
 if(reserve>UINT64_MAX-(uint64_t)before.st_size){message="Disk reservation overflow.";goto done;}
 out=open(destination,O_CREAT|O_EXCL|O_WRONLY|O_NOFOLLOW,0600);if(out<0){message="Cannot create unique staging file.";goto done;}owned=1;
 if(!room(out,reserve+(uint64_t)before.st_size)){message="Insufficient disk headroom.";goto done;}
 buffer=malloc(1024*1024);if(!buffer){message="Buffer allocation failed.";goto done;}
 hash_ready=1;if(!init(&hash)){message="SHA-256 initialization failed.";goto done;}
 for(;;){
  ssize_t count=read(in,buffer,1024*1024);if(count<0 && errno==EINTR)continue;
  if(count<0){message="Source read failed.";goto done;}if(!count)break;
  if((uint64_t)count>maximum-total || (uint64_t)count>(uint64_t)before.st_size-total){message="Source grew during capture.";goto done;}
  if(!update(&hash,buffer,(size_t)count)){message="SHA-256 update failed.";goto done;}
  size_t written=0;while(written<(size_t)count){ssize_t n=write(out,buffer+written,(size_t)count-written);if(n<0&&errno==EINTR)continue;if(n<=0){message="Staging write failed.";goto done;}written+=(size_t)n;}
  total+=(uint64_t)count;
  if(!room(out,reserve)){message="Disk headroom exhausted.";goto done;}
 }
 if(total!=(uint64_t)before.st_size || fstat(in,&after)||!same(&before,&after)){message="Source changed during capture.";goto done;}
 unsigned char bytes[32];if(!finish(&hash,bytes)){message="SHA-256 finalization failed.";goto done;}
 if(fchmod(out,0444)||fsync(out)){message="Cannot synchronize staged source.";goto done;}
 for(size_t i=0;i<32;i++){snprintf(digest+i*2,3,"%02x",bytes[i]);}
 digest[64]=0;
 result=0;
done:
 if(hash_ready){release(&hash);}
 free(buffer);if(in>=0)close(in);
 if(out>=0 && close(out) && result==0){result=-1;message="Staging close failed.";}
 if(result && owned)unlink(destination);
 if(result)snprintf(error,capacity,"%s",message);
 return result;
}

int re_open_source(const char*path){
 int fd=open(path,O_RDONLY|O_NONBLOCK|O_NOFOLLOW);struct stat value;
 if(fd<0)return -1;
 if(fstat(fd,&value)||!S_ISREG(value.st_mode)){close(fd);return -1;}
 return fd;
}
int re_verify_fd(int fd,const char*expected,uint64_t expected_size,char*error,size_t capacity){
 if(!expected||!error||!capacity)return -1;
 error[0]=0;const char*message="Source verification failed.";struct stat before,after;
 Hash hash={0};unsigned char*buffer=NULL;int ready=0,result=-1;uint64_t total=0;
 if(strlen(expected)!=64){message="Invalid digest length.";goto done;}
 if(fstat(fd,&before)||!S_ISREG(before.st_mode)||before.st_size<0 || (uint64_t)before.st_size!=expected_size || expected_size>UINT64_C(8589934592)){message="Captured file size or type changed.";goto done;}
 buffer=malloc(1024*1024);if(!buffer)goto done;
 ready=1;if(!init(&hash))goto done;
 while(total<expected_size){
  size_t count=(size_t)((expected_size-total)<1024*1024?(expected_size-total):1024*1024);
  ssize_t n=pread(fd,buffer,count,(off_t)total);
  if(n<0 && errno==EINTR)continue;
  if(n<=0 || !update(&hash,buffer,(size_t)n))goto done;
  total+=(uint64_t)n;
 }
 if(fstat(fd,&after)||!same(&before,&after)){message="Captured file changed during verification.";goto done;}
 unsigned char bytes[32];char digest[65];if(!finish(&hash,bytes))goto done;
 for(size_t i=0;i<32;i++){snprintf(digest+2*i,3,"%02x",bytes[i]);}digest[64]=0;
 if(strcmp(digest,expected)){message="Captured file SHA-256 mismatch.";goto done;}
 result=0;
done:
 if(ready){release(&hash);}free(buffer);
 if(result)snprintf(error,capacity,"%s",message);
 return result;
}
