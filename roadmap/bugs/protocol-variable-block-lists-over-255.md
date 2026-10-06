---
id: protocol-variable-block-lists-over-255
title: A request listing more than 255 objects fails to encode and ends the session
topic: protocol
status: bugs
origin: gridspec-circuit (2026-10-06)
refs: [gridspec-circuit]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

A `Variable` block list's count is one byte on the wire, so a message can
carry at most 255 entries. `Command::RequestObjects` with 300 ids failed in
`RequestMultipleObjects::encode_body` with "variable-length value of 300
bytes exceeds the 255-byte capacity" (the wording is the encoder's; it is
counting blocks), and `sl-client-tokio`'s run loop returned the error — the
whole session ended over one request. Found by `throttle-set` on aditi.

`Circuit::send_request_multiple_objects` now splits its ids across messages
(`MAX_VARIABLE_BLOCKS`). Every other request that takes a caller's list of
objects is still unsplit: `ObjectSelect`, `ObjectDeselect`, and the edit,
link, delete, return and permission messages built the same way in
`sl-proto/src/session/circuit.rs`. A viewer selecting a large linkset or
box-selecting a build reaches them.

## Wanted

- Split every caller-sized block list at 255, as the reference viewer does
  (it starts a new message when the current one is full).
- Decide what a command that fails to encode should cost. Today the tokio
  driver ends the session and the Bevy driver reports the command failed and
  carries on, which is not parity, and
  one bad request should not be the end of a login.
- The encoder's error text should say blocks, not bytes, when it is counting
  blocks.
