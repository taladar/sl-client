---
id: test-fake-grid-self-avatar-baked-textures-rejected
title: The agent's own head, upper and lower bakes are fetched, served, and then discarded
topic: test
status: bugs
origin: Firestorm cross-check harness run (2026-09-02)
points: 3
refs: [test-firestorm-crosscheck-runner]
---

Context: [context/testing.md](../context/testing.md).

Logged into the fake grid, the agent's **own** avatar stays a cloud. Its
wearables are worn, its Current Outfit Folder is complete, every library
texture resolves, and the grid answers all five of its bakes with `200`
— and the viewer throws three of them away anyway:

```text
processFetchResults : ba4e0008-… Fetch failure, setting as missing,
    mRawDiscardLevel 32767 current_discard -1 stats 00c80000 worker state 14
doLoadedCallbacks : baked texture: ba4e0008-…is missing.
```

`stats 00c80000` is HTTP **200** and worker state 14 is `DONE`, so the
fetch completed and returned bytes; `current_discard -1` says nothing was
decoded from them. The bytes are not the problem: the served codestream
is a valid 512×512 JPEG2000 that `opj_decompress` decodes at every
reduction from 0 to 5.

The three that fail are exactly the slots the *self* avatar composites
locally — `HEAD_BAKED`, `UPPER_BAKED`, `LOWER_BAKED`. The two added
alongside them, `EYES_BAKED` and `HAIR_BAKED`, are fetched from the same
route, from the same store, in the same burst, and are kept. Two other
things separate the two groups, either of which may be the real one:

- the kept pair are fetched with `Range: bytes=0-599`; the discarded
  three with no `Range` at all. Teaching the appearance route to answer
  ranges (206 + `Content-Range`, `Accept-Ranges` on every response, which
  it now does) changed nothing, so the header alone is not it.
- the discarded three are the slots `LLVOAvatarSelf` has a
  `LLTexLayerSet` for. A viewer that intends to composite them itself may
  be abandoning the server's copy on purpose, in which case the "missing"
  is self-inflicted and the gate to chase is
  `isLocalTextureDataAvailable`, not the fetch.

Not the cause, each ruled out by experiment: the appearance service being
unnamed (fixed — the login response now carries
`agent_appearance_service`, and every bake is requested and served);
`Range` handling (added, no change); missing wearable layer textures
(added, no 404s remain); an incomplete Current Outfit Folder (fixed);
unbaked eye and hair slots (fixed). Dropping the self bakes entirely so
the viewer must composite locally does **not** de-cloud it either — the
avatar stays at rez status 0 by a different branch — so simply modelling
the fake grid as a legacy-bake grid is not the answer on its own.

What this costs: the agent's own avatar renders as the cloud particle
rather than a body, so [[test-firestorm-crosscheck-runner]]'s frames
cannot be compared anywhere the avatar is in shot. Everything else in the
scene renders. Other avatars are unaffected — an NPC with the same
fabricated bakes loads fully.
