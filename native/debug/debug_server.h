#ifndef REWORKBENCH_DEBUG_SERVER_H
#define REWORKBENCH_DEBUG_SERVER_H
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
int rw_debug_attach(int pid, char *err, size_t errcap);
int rw_debug_launch(const char *path, int *pid_out, char *err, size_t errcap);
int rw_debug_detach(char *err, size_t errcap);
int rw_debug_continue(char *err, size_t errcap);
int rw_debug_interrupt(char *err, size_t errcap);
int rw_debug_step(char *err, size_t errcap);
int rw_debug_set_breakpoint(unsigned long long address, char *err, size_t errcap);
int rw_debug_remove_breakpoint(unsigned long long address, char *err, size_t errcap);
int rw_debug_read_memory(unsigned long long address, size_t length, char **json_out, char *err, size_t errcap);
int rw_debug_snapshot(char **json_out, char *err, size_t errcap);
int rw_debug_poll_event(char **json_out, char *err, size_t errcap);
int rw_debug_threads(char **text_out, char *err, size_t errcap);
int rw_debug_select_thread(unsigned long long thread_id, char *err, size_t errcap);
int rw_debug_memory_map(char **text_out, char *err, size_t errcap);
int rw_debug_modules(char **text_out, char *err, size_t errcap);
int rw_debug_backtrace(char **text_out, char *err, size_t errcap);
int rw_debug_disassembly(char **text_out, char *err, size_t errcap);
void rw_debug_free_string(char *p);
#ifdef __cplusplus
}
#endif
#endif
