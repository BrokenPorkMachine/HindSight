#include "../native/terminal/terminal_pty.c"
#include <assert.h>

int main(void) {
    int ready[2]; assert(pipe(ready) == 0);
    pid_t child = fork(); assert(child >= 0);
    if (child == 0) {
        close(ready[0]); setsid();
        signal(SIGHUP, SIG_IGN); signal(SIGTERM, SIG_IGN);
        assert(write(ready[1], "r", 1) == 1);
        for (;;) pause();
    }
    close(ready[1]); char byte;
    assert(read(ready[0], &byte, 1) == 1); close(ready[0]);
    vg_terminal_pty *terminal = calloc(1, sizeof(*terminal)); assert(terminal);
    terminal->child = child; terminal->master = -1;
    int64_t start = monotonic_ms();
    vg_terminal_pty_stop(terminal);
    assert(monotonic_ms() - start < 1500);
    assert(waitpid(child, NULL, WNOHANG) == -1 && errno == ECHILD);

    int master, slave; assert(openpty(&master, &slave, NULL, NULL, NULL) == 0);
    assert(fcntl(master, F_SETFL, O_NONBLOCK) == 0);
    struct termios raw; assert(tcgetattr(slave, &raw) == 0);
    cfmakeraw(&raw); assert(tcsetattr(slave, TCSANOW, &raw) == 0);
    vg_terminal_pty writer = { .child = -1, .master = master };
    size_t size = 4 * 1024 * 1024;
    char *input = calloc(size, 1); assert(input);
    start = monotonic_ms();
    ssize_t written = vg_terminal_pty_write(&writer, input, size);
    assert(written < (ssize_t)size && monotonic_ms() - start < 1500);
    free(input); close(master); close(slave);
    puts("Terminal dispatch contracts passed: stubborn shell termination, bounded input backpressure.");
}
