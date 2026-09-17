---
id: viewer-objectupdate-truncated-block-drops-message
title: An ObjectUpdate from aditi fails to decode and every object in it is dropped
topic: viewer
status: bugs
origin: aditi live check of viewer-neighbour-object-caps-use-root-region (2026-09-17)
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

On aditi (the tutorial area around Ahern), shortly after login, two
`ObjectUpdate` messages (`High(12)`) were dropped whole:

```text
dropping undecodable inbound message id=High(12) name="ObjectUpdate"
  error=unexpected end of data: needed 4 more byte(s), had 3
protocol diagnostic: DecodeFailed ... failed_offset=1713
protocol diagnostic: DecodeFailed ... failed_offset=1578
```

One `ObjectUpdate` carries a batch of objects, so a decode failure anywhere in
it loses **all** of them, not just the object with the odd block. Firestorm
decodes the same stream, so either our template decode is stricter than
`LLMessageSystem` about a short trailing field or a variable block, or a
hand-written sub-decoder reads past its block.

## Investigate

- Capture the datagram: `sl-conformance-trace` over a `tcpdump` of an aditi
  session, or log the raw bytes of the undecodable message, and decode it
  offline to see which block and field is 3 bytes long at offset
  1578 / 1713.
- Compare with the reference's `LLMessageSystem` handling of that field: does
  it tolerate a short final variable block, or zero-fill it?
- Whatever the cause, consider whether one bad object block should cost the
  rest of the message.
