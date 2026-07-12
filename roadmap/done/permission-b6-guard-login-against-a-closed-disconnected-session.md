---
id: permission-b6
title: Guard login against a closed / disconnected Session
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B6 (from Open-question #3). Guard login against a closed / disconnected
    `Session`.** A relogin uses a **new** `Session` (no live-session reuse, so
    `script_grants` / `taken_controls` need no `close` hook — matching the
    `objects`-cache convention). Make that contract enforceable: a `Session`
    that has reached its terminal `Closed` / `Disconnected` state must
    **reject** a fresh login rather than half-reuse stale state.
    - **Where**: the login entry point on `Session` (the constructor /
    `login`-style method in `sl-proto/src/session`). Check the session
    lifecycle state (the existing `Closed` / `DisconnectReason` machinery) and
    return an `Err(Error::…)` (a new descriptive variant, e.g.
    `SessionClosed`) when login is attempted on an already-closed/disconnected
    session, instead of proceeding.
    - **Scope note**: this is a general `Session`-lifecycle guard, not
    permission-specific; it is tracked here because Open-question #3 surfaced
    it. It touches no permission state.
    - **Tests** (`lifecycle.rs`): drive a session to `close` /
    disconnect, then assert a login attempt returns the new error (and does
    not mutate state). Wire the new `Error` variant through the runtimes only
    if their login paths surface `Session` errors (check tokio/bevy/REPL
    parity). Independent of B1.5–B5; may land at any point after sign-off.
    **Done 2026-06-26.** Login is valid exactly once, from the freshly
    constructed `New` state, so `handle_login_response` now guards on `New`
    *before any mutation* and rejects every other state — **broadened beyond
    the closed case** after review found a live double-login was the worse
    gap: a second response on an `Active`/`AwaitingHandshake`/`Teleporting`/
    `LoggingOut` session would overwrite the live circuit and reset to
    `AwaitingHandshake` while stranding the rest of the session state. Two
    variants: terminal `Closed` → `Error::SessionClosed`; any live state →
    new `Error::AlreadyLoggedIn` (`DisconnectReason` is the `Disconnected`
    event payload, not a state, so `Closed` is the only terminal state). Both
    runtimes already propagate `handle_login_response` via `?`
    (`sl-client-tokio/src/lib.rs:201`, `sl-client-bevy/src/lib.rs:359`) and
    the REPL drives login through them, so the two `#[non_exhaustive] Error`
    variants reach all three with no further wiring — parity holds. Tests
    (lifecycle.rs): `closed_session_rejects_relogin` drives a session to
    `Closed` via a login failure, then asserts a fresh
    `handle_login_response(success)` → `Err(SessionClosed)`, stays closed,
    establishes no circuit; `live_session_rejects_relogin` does the same on an
    established session → `Err(AlreadyLoggedIn)`, stays live, rebuilds no
    circuit. **No permission state touched** (general lifecycle guard).
