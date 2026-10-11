#ifndef RE_CAPTURE_H
#define RE_CAPTURE_H
#include <stdint.h>
#include <stddef.h>
/* Creates destination exclusively, streams and fsyncs it, sets mode 0444 on success.
   Failed owned destination is removed. Caller owns crash/kill cleanup after process exit. */
int re_capture(const char *source,const char *destination,uint64_t maximum,uint64_t reserve,char digest[65],char *error,size_t capacity);
int re_open_source(const char *path);
int re_verify_fd(int fd,const char *expected,uint64_t expected_size,char *error,size_t capacity);
#endif
