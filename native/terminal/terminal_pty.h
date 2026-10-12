#ifndef HINDSIGHT_TERMINAL_PTY_H
#define HINDSIGHT_TERMINAL_PTY_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

typedef struct vg_terminal_pty vg_terminal_pty;

vg_terminal_pty *vg_terminal_pty_start(const char *directory, const char *shell,
                                       uint16_t columns, uint16_t rows,
                                       char *error, size_t error_length);
ssize_t vg_terminal_pty_read(vg_terminal_pty *terminal, void *buffer, size_t length);
ssize_t vg_terminal_pty_write(vg_terminal_pty *terminal, const void *buffer, size_t length);
int vg_terminal_pty_resize(vg_terminal_pty *terminal, uint16_t columns, uint16_t rows);
int vg_terminal_pty_interrupt(vg_terminal_pty *terminal);
int vg_terminal_pty_eof(vg_terminal_pty *terminal);
int vg_terminal_pty_is_running(vg_terminal_pty *terminal);
void vg_terminal_pty_stop(vg_terminal_pty *terminal);

#endif
