---
id: protocol-cap-home-location
title: Set home over the HomeLocation capability
topic: protocol
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, gridspec-landmarks-home, viewer-places-landmarks]
---

Context: [context/protocol.md](../context/protocol.md).

Both grids grant `HomeLocation`. Firestorm's "Set Home to Here" POSTs
`{HomeLocation:{LocationId, LocationPos:{X,Y,Z}, LocationLookAt:{X,Y,Z}}}` and
reads `{success, HomeLocation:{LocationPos}}` back
(`LLAgent::setStartPosition`), falling back to UDP `SetStartLocationRequest`
without the capability.

Our client sends the UDP request (`set_start_location`, both runtimes; REPL
only — the viewer menu entry is [[viewer-places-landmarks]]). Switch to the
capability where granted and surface the reply (success, the position the grid
stored) as an event; OpenSim also sends an alert.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
