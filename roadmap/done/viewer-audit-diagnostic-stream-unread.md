---
id: viewer-audit-diagnostic-stream-unread
title: The viewer collects protocol diagnostics and drains none of them
topic: viewer
status: done
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

## Resolved

Both halves of "either drain them or stop collecting them" are now true: they
are drained, and collection is a switch.

**The renderer moved to where the type lives.** `sl-repl`'s `write_diagnostic`
/ `hexdump` were the only rendering of a `Diagnostic` in the workspace, in a
crate the viewer has no business depending on for this. They are now
`impl Display for Diagnostic` (the one-line summary), `Diagnostic::hexdump()`
(the captured bytes, `None` for the four variants that capture none) and
`sl_proto::hexdump` — all beside the type. `sl_repl::format_diagnostic`
composes the two and is byte-identical to before, which its own tests pin. The
`Display` match is exhaustive from inside the crate, so a variant added
upstream fails to compile until it is given a rendering.

**The drain** is `ingest_protocol_diagnostics` (`sl-viewer-notices`), beside
`ingest_alert_messages` / `announce_command_failures`. Level per variant,
because the five are not equally interesting:

- `DecodeFailed` / `CapsDecodeFailed` — genuine protocol gaps in this client,
  and rare: `warn`, with the failed decode's bytes following at `debug` (a
  hexdump does not belong in a warning line).
- `ExpectedReplyMissing` — `warn`, plus the toast below.
- `UnhandledMessage` / `UnknownCapsEvent` — *expected*: they name traffic this
  client does not model, they repeat on every arrival, and on some grids they
  never stop. `debug`, and each distinct one logged **once**, with the dedup key
  built only when that level is actually enabled, so an ordinary run pays
  nothing. The log-once set is capped (the keys are grid-controlled strings) and
  degrades to logging every occurrence rather than growing without bound.

**One class reaches the user, and only part of it.** `ExpectedReplyMissing`
raises the new `ViewerRequestNoReply` toast, `unique` on the request label and
coalesced on the same cooldown as `ViewerCommandSendFailed` (the rule is now one
shared `off_cooldown` helper) — but through an **allowlist**,
`USER_VISIBLE_REQUESTS`, holding only `Diagnostic::SIT_REQUEST`.

The allowlist is what the live OpenSim run bought. The first version raised the
toast for every missing reply, and the very first login produced
`ExpectedReplyMissing request=SimulatorFeatures` — stock OpenSim does not serve
that capability at all, so the toast would have fired on every login there,
reporting a grid's feature set as a failed user action. The label is an open
vocabulary (a capability name, a wire message name, or one of two operation
names), and everything in it except a sit is either background traffic or
already surfaced by something bigger: a logout by the logout itself, an
exhausted reliable packet by the disconnect that immediately follows. A sit is
the one where the session keeps running, nothing else is said, and the agent is
simply left standing. `LOGOUT_REQUEST` and `SIT_REQUEST` are now named in
`sl-proto` and used at both producer and consumer instead of the same literal
written twice; `only_agent_operations_are_user_visible` pins the list so it
cannot quietly widen.

**The flag follows a setting.** New `Command::SetDiagnostics(bool)` (handled by
both runtimes) makes collection runtime-toggleable, driven by the new
`diagnostics/CollectProtocolDiagnostics` setting (**default on** — with the
drain in place, collection is no longer wasted). `apply_diagnostics_setting`
pushes it on change via `ViewerSettings` change detection rather than the
preferences floater's apply hook, so the raw debug-settings editor works as a
writer; it is also an Advanced ▸ Collect Protocol Diagnostics check item, since
leaving it on costs something and a developer switch should be reachable.

`sl-client-bevy` now re-exports `MessageId` and `WireError`, without which a
consumer cannot match on a `Diagnostic`'s fields at all.

Live-verified against the local OpenSim grid: the Advanced menu item's check
mark tracks the setting and the entry shows in the debug-settings editor (both
confirmed interactively); the drain logged a real `ExpectedReplyMissing` and the
toast rendered with its `[REQUEST]` substitution resolved through Fluent. That
same run showed **no** `UnhandledMessage` or `DecodeFailed` at all against
OpenSim, so the collection volume that motivated the log-once rule is smaller in
practice than the worst case it guards against.

Tests: `display_is_one_line_without_the_bytes`,
`only_a_failed_decode_carries_a_hexdump`, `an_empty_capture_says_so`
(`sl-proto`); `a_missing_reply_raises_a_named_notification`,
`background_missing_replies_are_not_raised`,
`only_agent_operations_are_user_visible`,
`developer_diagnostics_stay_out_of_the_ui`,
`repeat_missing_replies_are_coalesced_per_request`,
`the_collection_switch_is_pushed_on_change_only` (`sl-viewer-notices`); plus
`sl-repl`'s existing `diagnostic_renders_literally` /
`decode_failed_renders_header_and_marked_hexdump`, which now guard the move.
