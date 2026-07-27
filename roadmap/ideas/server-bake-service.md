---
id: server-bake-service
title: Server-side appearance bake service
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

SL's server-side baking: compositing an avatar's wearable textures into
the baked body-region textures on the grid (the P14-era client work
consumed this; the client also knows the pre-bake composite path), so
all observers fetch one baked set instead of every layer.

Takes the current-outfit inventory + wearable assets, composites
per-region (head/upper/lower/eyes/skirt/hair + the universal/BoM
channels), uploads results to the asset service, and answers the
appearance flow (`UpdateAvatarAppearance` cap). The client workspace
already has the compositing logic and the UV/flip gotchas documented
from the viewer side — a server implementation would share that code.
Optional for an OpenSim-style grid (client-side baking suffices there),
required for SL parity.
