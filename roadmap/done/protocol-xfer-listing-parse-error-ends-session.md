---
id: protocol-xfer-listing-parse-error-ends-session
title: A malformed Xfer listing ends the whole session
topic: protocol
status: done
origin: test-phase-z-deferred-04 (2026-10-03)
refs: [test-phase-z-deferred-04]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

When a task-inventory contents listing failed to parse (Second Life leaves out
`group_owned`, which the parser then required), the error propagated out of
`Session::finish_xfer_download` through datagram handling and **ended the
session**: the runtime's event channel closed and the conformance run died
with "disconnected: event channel closed", the parse error surfacing only in
the logout warning.

## Fix

One unparsable file is the failure of one request, not of the circuit:
surface it as an event the requester sees (a failed
`TaskInventoryContents` / mute-list load, carrying the parse error) and keep
the session running. The same applies to every `XferPurpose` and to other
per-request payload decodes reached from datagram handling. Unit-test that a
malformed listing yields the error event and the session stays usable.

## Resolution

Wider than Xfer: `sl-client-tokio` ended its run loop on **any** error out of
`Session::handle_datagram` — every handler `?` (a `ParcelInfoReply` with a bad
`SimName` or area, an unparsable listing) and every framing error — while
`sl-client-bevy` logged the same errors at warn and carried on. Now, kept loud:

- `handle_datagram` drops a message its handler rejects: `tracing::error!`
  naming the message, plus `Diagnostic::HandlerFailed`. Only a framing error
  (header, zero-coding, message id) still returns `Err`, and both runtimes log
  that at error and keep reading.
- A mute list or task listing that fails to parse is logged at error and
  surfaced as `Event::XferDecodeFailed { xfer_id, file: XferListing, error }`.
- Consumers stop waiting on it: the viewer's contents list shows "Could not
  read the contents" and re-fetches only once the prim's serial moves; the
  conformance cases fail the step with the parse error
  (`support::wait_for_task_listing`, `mute_list`).
- Tests: a malformed mute list followed by a good one, and a rejected
  `ParcelInfoReply` followed by a good one (`sl-proto/tests/lifecycle.rs`);
  the contents cache's retry rule (`sl-viewer-edit`).
