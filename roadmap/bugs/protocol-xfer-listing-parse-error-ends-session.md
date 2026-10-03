---
id: protocol-xfer-listing-parse-error-ends-session
title: A malformed Xfer listing ends the whole session
topic: protocol
status: bugs
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
