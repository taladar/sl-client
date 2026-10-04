---
id: protocol-cap-interest-list
title: Switch the interest list mode over the InterestList capability
topic: protocol
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-network-debug-tools, viewer-area-search, viewer-360-snapshot]
blocked_by: [gridspec-object-update-stream]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `InterestList`. POST `{mode:"default"|"360"}` makes the
simulator send every object around the agent rather than only those in the
view frustum; DELETE resets it. Firestorm applies it to every region and again
after a region change, and switches it from the 360° capture, area search and
the Advanced menu.

Add a command and keep the mode across region changes; then use it from the
360° snapshot (which today captures only what the frustum streamed), area
search ([[viewer-area-search]]) and the debug menu
([[viewer-network-debug-tools]]).

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.

## Done together with [[gridspec-object-update-stream]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.
