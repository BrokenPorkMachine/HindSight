#include "terminal_pty.h"

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>
#include <util.h>
#include <poll.h>

struct vg_terminal_pty {
    pid_t child;
    int master;
};

static void set_error(char *error, size_t length, const char *message) {
    if (error == NULL || length == 0) return;
    snprintf(error, length, "%s", message ? message : "PTY operation failed.");
}

vg_terminal_pty *vg_terminal_pty_start(const char *directory, const char *shell,
                                       uint16_t columns, uint16_t rows,
                                       char *error, size_t error_length) {
    if (directory == NULL || shell == NULL || directory[0] != '/' || shell[0] != '/') {
        set_error(error, error_length, "Invalid terminal directory or shell.");
        return NULL;
    }
    struct winsize size = { .ws_row = rows ? rows : 32, .ws_col = columns ? columns : 120 };
    int master = -1;
    pid_t child = forkpty(&master, NULL, NULL, &size);
    if (child < 0) {
        set_error(error, error_length, strerror(errno));
        return NULL;
    }
    if (child == 0) {
        if (chdir(directory) != 0) _exit(126);
        setenv("TERM", "xterm-256color", 1);
        setenv("COLORTERM", "truecolor", 1);
        execl(shell, "zsh", "-l", (char *)NULL);
        _exit(127);
    }
    int flags = fcntl(master, F_GETFL, 0);
    if (flags >= 0) (void)fcntl(master, F_SETFL, flags | O_NONBLOCK);
    vg_terminal_pty *terminal = calloc(1, sizeof(*terminal));
    if (terminal == NULL) {
        kill(child, SIGHUP);
        close(master);
        (void)waitpid(child, NULL, 0);
        set_error(error, error_length, "Unable to allocate terminal session.");
        return NULL;
    }
    terminal->child = child;
    terminal->master = master;
    return terminal;
}

ssize_t vg_terminal_pty_read(vg_terminal_pty *terminal, void *buffer, size_t length) {
    if (terminal == NULL || terminal->master < 0 || buffer == NULL || length == 0) return -1;
    return read(terminal->master, buffer, length);
}

ssize_t vg_terminal_pty_write(vg_terminal_pty *terminal, const void *buffer, size_t length) {
    if (terminal == NULL || terminal->master < 0 || buffer == NULL || length == 0) return -1;
    const unsigned char *bytes = buffer;
    size_t written = 0;
    while (written < length) {
        ssize_t result = write(terminal->master, bytes + written, length - written);
        if (result > 0) { written += (size_t)result; continue; }
        if (result < 0 && errno == EINTR) continue;
        if (result < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            struct pollfd descriptor = { .fd = terminal->master, .events = POLLOUT };
            if (poll(&descriptor, 1, 2000) > 0) continue;
        }
        return written > 0 ? (ssize_t)written : -1;
    }
    return (ssize_t)written;
}

int vg_terminal_pty_resize(vg_terminal_pty *terminal, uint16_t columns, uint16_t rows) {
    if (terminal == NULL || terminal->master < 0) return -1;
    struct winsize size = { .ws_row = rows ? rows : 1, .ws_col = columns ? columns : 1 };
    return ioctl(terminal->master, TIOCSWINSZ, &size);
}

int vg_terminal_pty_interrupt(vg_terminal_pty *terminal) {
    if (terminal == NULL || terminal->child <= 0) return -1;
    return kill(-terminal->child, SIGINT);
}

int vg_terminal_pty_eof(vg_terminal_pty *terminal) {
    const unsigned char eof = 0x04;
    return vg_terminal_pty_write(terminal, &eof, 1) == 1 ? 0 : -1;
}

int vg_terminal_pty_is_running(vg_terminal_pty *terminal) {
    if (terminal == NULL || terminal->child <= 0) return 0;
    int status = 0;
    pid_t result = waitpid(terminal->child, &status, WNOHANG);
    return result == 0 ? 1 : 0;
}

void vg_terminal_pty_stop(vg_terminal_pty *terminal) {
    if (terminal == NULL) return;
    if (terminal->child > 0) {
        kill(-terminal->child, SIGHUP);
        kill(terminal->child, SIGTERM);
        (void)waitpid(terminal->child, NULL, 0);
    }
    if (terminal->master >= 0) close(terminal->master);
    free(terminal);
}
