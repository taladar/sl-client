---
id: protocol-cancelled-reply-ends-replacement-teleport
title: A replaced teleport's TPCancelled reply may end the teleport that replaced it
topic: protocol
status: done
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Plausible. The client side is traced, but Second Life's reply order is
unconfirmed.

## Fix (2026-11-24)

Each cancel sent to the grid is counted for 10 s. A `TPCancelled` answer that
arrives while another teleport is running uses up one of them and leaves that
teleport alone. With nothing in flight the answer still surfaces, so the
overlay closes on it as before.

## Bad order

`teleport_to` cancels teleport A and requests B, then A's `TPCancelled` failure
arrives while B is running.

## Cause

`teleport_refused` cannot tell whose failure it is, so it ends B: the session
goes back to active, the timer is cleared and the overlay closes. B's finish
should still be honoured when it arrives.

## Fix

Count outstanding cancels and swallow that many `TPCancelled` replies.
