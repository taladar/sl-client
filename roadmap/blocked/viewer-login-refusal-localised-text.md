---
id: viewer-login-refusal-localised-text
title: Show a login refusal in the user's language from its message_id
topic: viewer
status: blocked
origin: gridspec-login-refusals (2026-10-06)
refs: [gridspec-login-refusals, viewer-notification-catalogue-login-session]
blocked_by: [viewer-login-screen]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life sends every login refusal with a `message_id` (measured:
`LoginFailedAuthenticationFailed` for a wrong password or an unknown
account), its `message_args`, and an incident id (`Linden_Error_Code`) —
`book/src/gridspec/login.md` § Refusals. `LoginFailure` carries all three;
nothing in the viewer reads them, so a refusal is shown in the grid's
English `message` whatever the locale.

The reference (`llstartup.cpp`, the `LoginFailedHeader` block) looks the
`message_id` up in its string table with `message_args` substituted and
falls back to `message` when it has no such string; an MFA challenge is
shown the same way (`lllogininstance.cpp`). OpenSim sends no `message_id`,
so its text is always shown as sent.

Do: the `LoginFailed*` strings in the Fluent catalogue, the lookup with the
fallback, and the incident id shown where the user can copy it.
