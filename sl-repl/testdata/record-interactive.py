#!/usr/bin/env python3
"""Drive an interactive sl-repl session under a pseudo-terminal.

The REPL records a replayable `.repl` transcript only in interactive (TTY)
mode, so exercising `--script-out` needs a real terminal. This helper allocates
a pty, lets the session log in, types a few lines, then sends Ctrl-D (EOF) to
trigger a clean logout.

Usage:
    DELAY_LOGIN=14 python3 record-interactive.py \\
        ../../target/debug/sl-repl-tokio \\
        --credentials credentials.toml \\
        --log-file rec.log --script-out rec.repl

The lines typed after login are fixed below; edit `LINES` to record a different
transcript.
"""

import os
import select
import sys
import time

LINES = [
    'chat "recorded chat line"',
    'im $self "recorded im via $self"',
]


def main() -> None:
    argv = sys.argv[1:]
    if not argv:
        sys.exit("usage: record-interactive.py <sl-repl-binary> [args...]")
    delay_login = float(os.environ.get("DELAY_LOGIN", "14"))

    import pty

    pid, fd = pty.fork()
    if pid == 0:
        os.execvp(argv[0], argv)
        os._exit(127)

    start = time.time()
    sent = False
    while True:
        readable, _, _ = select.select([fd], [], [], 0.5)
        if fd in readable:
            try:
                data = os.read(fd, 4096)
            except OSError:
                break
            if not data:
                break
        now = time.time()
        if not sent and now - start > delay_login:
            for line in LINES:
                os.write(fd, (line + "\r").encode())
                time.sleep(1.5)
            time.sleep(3)
            os.write(fd, b"\x04")  # Ctrl-D -> EOF -> logout
            sent = True
        if now - start > delay_login + 25:
            break

    try:
        os.close(fd)
    except OSError:
        pass
    os.waitpid(pid, 0)


if __name__ == "__main__":
    main()
