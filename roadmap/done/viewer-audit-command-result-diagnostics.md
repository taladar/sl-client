---
id: viewer-audit-command-result-diagnostics
title: The bevy command dispatcher discards 300 protocol send results with no log
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 5
refs: [protocol-audit-runtime-parity-gaps]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy/src/lib.rs` ends 300 calls in `.ok();` — the command
dispatcher's ~356 `Command::` arms (`:2522-2528` and neighbours) are all of the
form `session.delete_objects(local_ids, now).ok();`. No log, no event, no UI
feedback: a failed send is indistinguishable from success, so a user's delete,
rez, terraform or parcel edit can silently do nothing.

The **same** `sl-proto` calls in `sl-client-tokio/src/lib.rs:1485-1491`
propagate with `?`. Two runtimes, opposite error policies, and the user-facing
one is the one that drops failures.

This is the single largest violation of the project's never-suppress-an-error
rule; 465 `.ok();` exist workspace-wide, 337 of them in this crate.

Scope: fold the result into a `Diagnostic` / `SessionEvent` so the viewer can
surface a failed action. Where a send failure is genuinely expected and benign,
say so at the call site rather than discarding uniformly. `sl-client-tokio`'s
own remaining `.ok()` sites (51, mostly `events.send(...)` on a closed channel)
should get the same review — `appearance.rs:26-36` swallows all three failure
paths of `request_server_appearance_update`.

## Resolved

The Bevy dispatcher now has the **same** error policy as the tokio runtime.

- `sl-proto`: `Command::name()` — an exhaustive 350-arm match giving each
  command a stable `&'static str` label, so a failed send can name the action
  that did not happen. A new variant fails to compile until it is named.
- `sl-client-bevy`: the ~3000-line command `match` moved out of
  `advance_running` into `apply_command(...) -> Result<(), Error>`, and all 268
  `session.…().ok()` became `?`. `advance_running` reports the first failing
  send per command through `report_command_failed`, which logs it and hands it
  to the app side as the new `NetOutbound::CommandFailed` → `SlCommandFailed`
  Bevy message (always on, unlike the diagnostics-gated `SlDiagnostic`, since
  this is outbound work the app explicitly asked for). No `?` sits inside a
  loop, so no arm's remaining iterations are skipped; the three arms that
  transcribe an outbound IM / group / conference line now skip that line when
  the send failed, which is what the transcript should say.
- The viewer surfaces it: `announce_command_failures` (`sl-viewer-notices`)
  raises the new `ViewerCommandSendFailed` catalogue template with `[COMMAND]`
  and `[REASON]`. The template is `unique` on the command name and the system
  coalesces repeats on a 10 s cooldown, so a per-frame command (camera,
  controls) failing every frame while the circuit is down cannot flood the
  screen — the network thread still logs every occurrence.

The rest of the never-suppress-an-error sweep this finding called for:

- **Every remaining `.ok();` in `sl-client-bevy` and `sl-client-tokio` is
  gone**, down to one documented discard per crate. Channel sends route through
  `report` / `deliver`, whose doc says once why a closed channel is the one
  correct discard (the receiver is gone, so there is nobody to tell).
- Real failures that were being dropped now report: `handle_datagram`,
  `handle_caps_event`, `notify_capabilities_ready`, `socket.send_to`, the
  socket mode/timeout calls, and the abuse-report screenshot upload all log;
  `fetch_folder_contents` (both runtimes) propagates its UDP-fallback send.
- The three fire-and-forget one-way capability POSTs per runtime were
  triplicated bodies that swallowed both the client-build and the POST error;
  they are now one `post_llsd_oneway` helper each, logging a transport failure
  or a rejecting status (never the cap URL — it carries the session token).
- `sl-client-tokio`'s `request_server_appearance_update` reported none of its
  three failure paths; each now logs and calls `report_caps_failure`, so a bake
  request that never got its reply surfaces as `ExpectedReplyMissing` instead
  of the appearance simply never updating.

Tests: `name_agrees_with_the_debug_variant` / `sample_names_are_distinct`
(`sl-proto`), `a_command_with_no_circuit_reports_its_failure` /
`a_reported_failure_names_the_command` (`sl-client-bevy`),
`a_failed_command_raises_a_named_notification` /
`repeat_failures_are_coalesced_per_command` (`sl-viewer-notices`).

Verification is client-side only: a real send failure is not something a live
grid can be asked for on demand, and the toast itself renders through the
ordinary `Notify` channel the notification host already surfaces. The unit
tests cover the whole chain — `Session` error → `apply_command` → the
`NetOutbound::CommandFailed` report → the raised `ShowNotification` and its
coalescing.

Not done here: the viewer enables `SlClientPlugin::diagnostics` but no viewer
system reads `SlDiagnostic`, so inbound diagnostics are still written into a
queue nothing drains — filed separately as
[[viewer-audit-diagnostic-stream-unread]].
