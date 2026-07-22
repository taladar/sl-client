---
id: viewer-login-tos
title: Login TOS / critical-message acceptance
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-login-screen]
---

Context: [context/viewer.md](../context/viewer.md).

The login-flow interstitials SL requires: when the login response returns
`tos` (terms-of-service update) or `critical` (critical message), the
login must pause, display the message (TOS is HTML — render via the
embedded browser when present, else a text extraction fallback), collect
acceptance, and retry the login with `agree_to_tos` / `read_critical`
set — declining aborts cleanly back to the login screen
([[viewer-login-screen]]). The login-request builder (`protocol-53`)
already models the request fields; this is the UI step and the retry
loop.

Reference (Firestorm, read-only): `llfloatertos`, `llstartup` (TOS state),
`floater_tos.xml`, `floater_critical.xml`.

Deps: [[viewer-login-screen]] (the flow this interrupts).
