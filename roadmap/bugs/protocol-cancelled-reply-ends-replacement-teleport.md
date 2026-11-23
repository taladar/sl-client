---
id: protocol-cancelled-reply-ends-replacement-teleport
title: A replaced teleport's TPCancelled reply may end the teleport that replaced it
topic: protocol
status: bugs
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Plausible. The client side is traced, but Second Life's reply order is
unconfirmed.

## Bad order

`teleport_to` cancels teleport A and requests B, then A's `TPCancelled` failure
arrives while B is running.

## Cause

`teleport_refused` cannot tell whose failure it is, so it ends B: the session
goes back to active, the timer is cleared and the overlay closes. B's finish
should still be honoured when it arrives.

## Fix

Count outstanding cancels and swallow that many `TPCancelled` replies.
