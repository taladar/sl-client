---
id: viewer-login-refusal-localised-text
title: Show a login refusal in the user's language from its message_id
topic: viewer
status: done
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

## Done (2026-11-27)

- `DisconnectReason::LoginFailed` now carries `message_id`, `message_args`
  and `error_code` (built by `DisconnectReason::refused` from a
  `LoginFailure`, `login_failed` for a client-made failure), and
  `MfaChallenge` parses and builds a `message_id`, both over XML-RPC and
  LLSD.
- `sl-client-bevy-viewer/src/login_refusal.rs`: the reference's
  `LoginFailed*` / `LogoutFailed*` strings, 31 of them, each under a
  `login-refusal-` key in the English bundle; `GridText::resolve` finds the
  key and fills `$time` / `$version` / `$start` / `$end`, a suspension's or a
  maintenance's `TIME` put on the user's clock when it is an RFC 3339 instant
  and marked "Pacific Time" otherwise, as `llstartup.cpp` does; an unknown
  key and OpenSim's keyless refusals show the grid's `message`. The
  second-factor challenge is worded the same way.
- The login panel shows the incident id in a row under the notice
  (`login-incident-row`) with a Copy button, hidden when the notice has none.
- Tests: the key table, the argument resolution, the time handling, the
  fallbacks; `locale_bundles` formats three strings through real Fluent;
  `e2e_login_screen`'s wrong-password stage now sees Second Life's key said
  from the viewer's string and the incident row.

Not measured: no grid has been seen sending a `message_id` with a second
factor challenge, or a `TIME` argument, so the instant format is the
reference's reading (`LLDate::fromString`), not an observed one.
