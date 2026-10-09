---
id: server-fake-grid-object-update-forms
title: Fake grid — send compressed, terse and cached object updates as each grid does
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-object-update-stream]
---

Context: [context/server.md](../context/server.md).

## What

Today every object goes out as a full `ObjectUpdate` at arrival. Send the forms
each grid sends (compressed, terse, cache probes honouring the handshake flags).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## What was measured (2026-10-09)

`book/src/gridspec/objects.md` § Updates. In short: compressed updates
four or five to a message for prims, full ones for avatars; a cache probe
of every object first unless the handshake reply's bit 1 is set; on
`FakeSl` the draw-distance cull by linkset root, the re-send on return, bit
0's whole region, and the kills of probed objects out of range; on
`FakeOpensim` none of those. `object-update-decode` and
`object-handshake-flags` already run offline and skip exactly these rows on
a fake grid (`check_live`, the `!grid.is_fake()` branch): holding the fake
flavours to them is this task's acceptance.
