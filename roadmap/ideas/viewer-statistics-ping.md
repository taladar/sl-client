---
id: viewer-statistics-ping
title: Show the ping to the simulator, and its packet loss, in the viewer
topic: viewer
status: ideas
origin: gridspec-circuit (2026-10-06)
refs: [gridspec-circuit]
---

Context: [context/viewer.md](../context/viewer.md).

The session measures the round trip of every keep-alive ping and reports it
as `Event::Ping`, per circuit. Nothing in the viewer reads it: there is no
"ping sim" figure anywhere, and no count of the retransmissions a circuit is
drawing — the two numbers the reference's statistics floater leads with, and
the first thing anybody looks at when a region feels slow.

The fake grid can now be made distant (`FakeGridBuilder::link_latency`), so a
figure can be held to a known round trip in an e2e test.
