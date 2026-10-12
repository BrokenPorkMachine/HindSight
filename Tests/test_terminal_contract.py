from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text()


def test_terminal_protocol_and_coordinator_routes_are_bounded():
    messages = read("Shared/TerminalMessages.swift")
    host = read("PlatformServices/CoordinatorService/TerminalHost.swift")
    coordinator = read("PlatformServices/CoordinatorService/main.swift")

    assert "struct TerminalReply: Codable" in messages
    assert "maxInputBytes = 64 * 1024" in host
    assert "maxOutputBytes = 48 * 1024" in host
    assert "while output.count < maxOutputBytes" in host
    assert "String(decoding: data, as: UTF8.self)" in messages
    for route in (
        "terminalStart",
        "terminalWrite",
        "terminalRead",
        "terminalResize",
        "terminalInterrupt",
        "terminalEOF",
        "terminalStop",
    ):
        assert f"func {route}" in coordinator


def test_terminal_native_backend_owns_lifecycle_and_validates_inputs():
    header = read("native/terminal/terminal_pty.h")
    source = read("native/terminal/terminal_pty.c")

    for symbol in (
        "vg_terminal_pty_start",
        "vg_terminal_pty_read",
        "vg_terminal_pty_write",
        "vg_terminal_pty_resize",
        "vg_terminal_pty_interrupt",
        "vg_terminal_pty_eof",
        "vg_terminal_pty_is_running",
        "vg_terminal_pty_stop",
    ):
        assert symbol in header
        assert symbol in source
    assert 'directory[0] != \'/\' || shell[0] != \'/\'' in source
    assert "setenv(\"TERM\", \"xterm-256color\", 1)" in source
    assert "kill(-terminal->child, SIGHUP)" in source
    assert "waitpid(terminal->child, NULL, 0)" in source


def test_terminal_output_sanitizer_preserves_state_across_chunks():
    messages = read("Shared/TerminalMessages.swift")

    assert "private enum State { case normal, escape, csi, osc, oscEscape }" in messages
    assert "func append(_ text: String) -> String" in messages
    assert "static func sanitized(_ text: String) -> String" in messages
    assert "func reset()" in messages
