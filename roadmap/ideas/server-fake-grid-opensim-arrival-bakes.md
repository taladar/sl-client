---
id: server-fake-grid-opensim-arrival-bakes
title: The OpenSim flavour names solid fixture bakes where OpenSim names the default placeholder
topic: server
status: ideas
origin: server-bake-service (2026-10-20)
refs: [gridspec-appearance-baking, server-bake-service, viewer-bake-cache-query]
---

Context: [context/server.md](../context/server.md).

On arrival the local OpenSim sent an avatar nothing had baked an appearance with
`IMG_DEFAULT_AVATAR` on every baked slot, and a login straight after a publish
again names the default placeholder on every slot
(`book/src/gridspec/appearance.md` § Baking, Across a login, 2026-10-20).
`FakeOpensim` instead names the fixture's solid green bakes at every arrival,
which a viewer then fetches by asset id — an avatar that looks baked on a grid
where nobody baked it.

Done means: `FakeOpensim` names the default placeholder at every arrival;
`appearance-baking`'s arrival and relogin bakes are held on both flavours. (An
earlier run read a publish as surviving into the next run's login; the relogin
leg has not seen that, so check what persists — the visual parameters? — before
imitating it.) Check the viewer's own-avatar render tests on the OpenSim
flavour, which today see the green bake; with the viewer's client-side bake
(`bake_publish.rs`) they should see the viewer's own composite instead.
