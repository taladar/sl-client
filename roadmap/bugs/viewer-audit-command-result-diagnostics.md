---
id: viewer-audit-command-result-diagnostics
title: The bevy command dispatcher discards 300 protocol send results with no log
topic: viewer
status: bugs
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
