---
id: server-fake-grid-opensim-arrival-bakes
title: The OpenSim flavour names solid fixture bakes where OpenSim names the default placeholder
topic: server
status: ideas
origin: server-bake-service (2026-10-20)
refs: [gridspec-appearance-baking, server-bake-service, viewer-bake-cache-query]
---

Context: [context/server.md](../context/server.md).

On arrival the local OpenSim sent an avatar nothing had baked an appearance
with `IMG_DEFAULT_AVATAR` on every baked slot, and from then on whatever its
viewer last published with `AgentSetAppearance`, persisted across logins
(`book/src/gridspec/appearance.md` § Baking). `FakeOpensim` instead names the
fixture's solid green bakes at every arrival, which a viewer then fetches by
asset id — an avatar that looks baked on a grid where nobody baked it.

Done means: `FakeOpensim` names the default placeholder until the agent
publishes, keeps the publish per account across logins (as it keeps homes),
and states it at the next arrival; `appearance-baking`'s arrival bakes are
held on both flavours. Check the viewer's own-avatar render tests on the OpenSim
flavour, which today see the green bake; with the viewer's client-side bake
(`bake_publish.rs`) they should see the viewer's own composite instead.
