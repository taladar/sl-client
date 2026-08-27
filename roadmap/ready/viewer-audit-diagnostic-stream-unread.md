---
id: viewer-audit-diagnostic-stream-unread
title: The viewer collects protocol diagnostics and drains none of them
topic: viewer
status: ready
origin: found while fixing viewer-audit-command-result-diagnostics (2026-08-27)
points: 2
refs: [viewer-audit-command-result-diagnostics]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy-viewer/src/lib.rs` adds `SlClientPlugin` with
`diagnostics: true`, so the session pays the raw-byte capture and bookkeeping
cost for every anomaly it would otherwise silently drop — a datagram whose body
failed to decode, a decoded message with no handler, an unknown or malformed
CAPS event-queue payload, a reliable request whose expected reply never
arrived.

Nothing in any viewer crate reads `MessageReader<SlDiagnostic>`. The messages
are written into a Bevy message queue that no system drains, so they age out
of the double buffer two frames later, unseen. The only consumer in the whole
workspace is `sl-repl-bevy/src/bin/sl-repl-bevy.rs`, which `warn!`s each one.

So the viewer is in the worst of both worlds: it pays for collection and sees
nothing. Either drain them or stop collecting them.

Scope: a bridge system beside `announce_command_failures` /
`ingest_alert_messages` in `sl-viewer-notices`. At minimum log each diagnostic
(the `sl-repl-bevy` `format_diagnostic` rendering is the reference). Decide per
variant whether any of them deserves a user-visible surface —
`ExpectedReplyMissing` for a user-initiated request is the plausible candidate,
`UnhandledMessage` plainly is not — and whether the flag should instead follow
a debug setting rather than being hard-coded on.
