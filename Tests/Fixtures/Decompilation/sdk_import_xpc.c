#include <stdio.h>
__attribute__((visibility("default"), noinline, used))
size_t HSWritePrefix(FILE *stream, const void *bytes, size_t count) {
    return fwrite(bytes, 1, count, stream);
}
int main(void) { return 0; }
