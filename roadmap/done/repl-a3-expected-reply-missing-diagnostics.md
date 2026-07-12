---
id: repl-a3
title: Expected-reply-missing diagnostics
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase A — core diagnostics (sl-wire / sl-proto), no REPL yet
---

Context: [context/repl.md](../context/repl.md).

**A3. Expected-reply-missing diagnostics.** Emit
`Diagnostic::ExpectedReplyMissing` when a reliable packet exhausts
`MAX_RESEND_ATTEMPTS` (root *and* child circuits), and for the logout/sit
timeouts (teleport stays `Event::TeleportFailed`). A sit timeout had no
mechanism yet, so added a `Timers::sit` timer (`SIT_TIMEOUT` 15s) armed in
`sit_on`, disarmed on `AvatarSitResponse`; it surfaces the diagnostic without
closing the session. `UnackedPacket` now carries the message name to label the
diagnostic; `process_resends` returns the exhausted `(sequence, name)` pairs
and drops them so each is reported once. `tracing::warn!` at every site.
