---
id: viewer-quit-graceful-logout
title: Graceful logout on menu Quit and window close (fix intermittent exit hang)
topic: viewer
status: done
origin: quit-hang investigation during the perf session (2026-08-12)
refs: [viewer-perf-pbr-shadow-cluster-rez]
---

Context: [context/viewer.md](../context/viewer.md).

Two quit paths bypassed the grid logout: **menu ▸ Quit** wrote `AppExit`
directly (`menu_bar.rs`), and the **window close** button used Bevy's default
close-to-exit. Only `Ctrl+Q` (`handle_quit_input`) logged out gracefully. An
abrupt exit strands the grid session (which can block the next login) and — the
symptom that surfaced this — **intermittently hangs the process on exit**.

Observed: on **aditi** the process sometimes lingered after "session ended"
(logged *after* `app.run()` returns, so the Bevy app had already exited) and had
to be `SIGTERM`ed; on **local OpenSim it never lingered**, and the same run
could exit cleanly one time and hang the next. That signature — intermittent,
aditi-only, post-app-loop — is a **teardown blocking on an in-flight network
operation** (a real HTTPS/CAPS request on the shared, not-Bevy-owned tokio
runtime) that a clean logout avoids; on localhost the connection closes
instantly.

Fix: route **both** menu Quit and window close through a graceful logout. A new
`QuitRequested` message (menu Quit) and `WindowCloseRequested` (window / Wayland
compositor close) are read by `handle_quit_requests`, which calls the existing
`request_logout` (queues `Command::Logout`, arms the quit deadline); the actual
`AppExit` still comes from `drive_session` on `LoggedOut`, with
`enforce_quit_deadline` as the grace fallback. Bevy's default close-to-exit is
disabled (`WindowPlugin { close_when_requested: false }`) so our handler owns
the close.

Verified on aditi: both menu Quit and window close now log
`quit requested; logging out` → `logged out cleanly; exiting` → `session ended`
and exit cleanly (no linger, no stranded session). The linger was racy, so this
is the plausible-but-not-100%-proven cure for the hang; the session-stranding
fix is unconditional. If a linger ever recurs, capture a thread backtrace of the
lingering process (`eu-stack`/`gdb -p`) to pin the exact blocking thread.
