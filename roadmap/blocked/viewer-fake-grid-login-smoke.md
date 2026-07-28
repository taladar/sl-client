---
id: viewer-fake-grid-login-smoke
title: The real client stack against the fake grid
topic: viewer
status: blocked
origin: user request (2026-07) — full client testing without a real server
points: 5
blocked_by: [viewer-fake-grid]
---

Context: [context/viewer.md](../context/viewer.md).

The only tier exercising the socket-owning `drive` system,
retransmission, and CAPS polling end-to-end: run `SlClientPlugin` in a
headless app pointed at an in-process fake grid.

Assert the full pipeline — login → circuit handshake → `RegionHandshake`
→ `SlEvent` stream → `maintain_world` region/parcel/object state — and a
few command round-trips (chat out → decoded server-side as a
`ServerEvent`; object update in → viewer `ObjectState`).

Deliberately a smoke tier: behaviour lives in the headless
interaction/world tiers; this proves the plumbing those tiers bypass is
sound.
