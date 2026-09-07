---
id: test-fake-grid-imitates-server-bakes
title: The fake grid runs a bake service on a grid that says it is OpenSim
topic: test
status: ready
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 3
refs: [test-fake-grid-object-asset-id-divergence]
---

Context: [context/testing.md](../context/testing.md).

Server-side avatar baking is a Second Life thing. The login response names an
`agent_appearance_service`, the viewer decides the avatar is server-baked, and
every baked slot is fetched from that service by URL rather than by asset id
(`LLVOAvatar::getImageURL`). A stock OpenSim region does not run one; the
viewer bakes locally and uploads.

The fake grid always names the service — `runtime.rs` sets
`success.agent_appearance_service` unconditionally, and `http_service.rs`
mounts the route per session — so an OpenSim-flavoured grid is still Second
Life about appearance.

**The trap, which is why this is not a one-line derivation.** Dropping the
service alone is worse than leaving it: the comment already in `runtime.rs`
records what happens, and it was paid for once. A viewer that has decided an
avatar is server-baked and finds no service URL leaves every avatar — the
agent's own included — permanently a cloud, *silently*, because `getImageURL`
returns an empty string rather than a request that fails. So the decision the
viewer makes has to flip with the service: what tells it "this avatar is
server-baked" (the bake texture ids the grid hands out, and what
`SimulatorFeatures` says) has to say the other thing too.

The work is therefore: find every input the viewer's server-baked decision
reads, make the whole set follow `ImitatedGrid`, and prove the OpenSim-flavoured
grid produces an avatar that is not a cloud — which is a screenshot question,
not a unit-test one, so it wants the full-stack viewer harness.

Acceptance: `Grid::FakeOpensim` advertises no appearance service and its
avatars still render, `Grid::FakeSl` keeps the service, and the pair is pinned
by something that would have caught the silent-cloud failure.
