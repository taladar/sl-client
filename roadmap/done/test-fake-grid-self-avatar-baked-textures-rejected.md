---
id: test-fake-grid-self-avatar-baked-textures-rejected
title: The agent's own head, upper and lower bakes are fetched, served, and then discarded
topic: test
status: done
origin: Firestorm cross-check harness run (2026-09-02)
points: 3
refs: [test-firestorm-crosscheck-runner, viewer-bake-publish-morph-mask]
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

## Root cause: a bake is five components, and ours were three

**A baked avatar texture is not an ordinary texture.** The reference
viewer uploads one as a **five**-component J2C — `R G B alpha mask`,
its own `RGBHM` comment — where the fifth plane is the clothing morph
mask (`LLViewerTexLayerSetBuffer::doUpload`,
`indra/newview/llviewertexlayer.cpp:639`: `const S32
baked_image_components = 5; // red green blue [bump] clothing`).

Our fixture bakes went through `sl_j2c_encode::encode_rgba8`, which drops
a fully-opaque image's redundant alpha plane — so a flat-coloured fixture
bake was **three** components. That is what the three slots died of:

- `LLVOAvatar::onFirstTEMessageReceived` registers
  `onBakedTextureMasksLoaded` with `needs_aux = true` for **exactly**
  `BAKED_HEAD`, `BAKED_UPPER` and `BAKED_LOWER` — the three slots that
  carry a morph mask. Eyes and hair get no aux callback. That is the
  observed split, and nothing else in the fetcher produces it.
- An aux fetch decodes with `decodeChannels(aux, .., first_channel = 4,
  max_channel_count = 4)`, i.e. component index 4 — the fifth. With three
  components `channels = 3 - 4 = -1` and the aux decode fails; with four
  it is `0` and fails just the same.
- `ImageRequest::finishRequest` reports `success = completed &&
  mDecodedRaw && (!mNeedsAux || mDecodedAux)`, so the failed aux decode
  **discards the perfectly good colour decode with it**. The worker
  reaches `DONE` with no raw image, and `processFetchResults` marks the
  texture a missing asset.

So the fetch, the HTTP status and the pixels were all fine, and the
missing fifth plane threw them away.

Fixed (2026-09-02): `sl_j2c_encode::encode_baked_avatar` /
`sl_texture::encode_baked_avatar_j2c` / `RgbaImage::baked_avatar_j2c`
write the five-component form, with an all-`255` morph mask — the value
`LLTexLayerSet::gatherMorphMaskAlpha` starts from before each worn layer
subtracts its coverage, i.e. "nothing masks this body". The NPC and
own-avatar fixtures use it, and `client_end_to_end` asserts the served
head, upper and lower bakes declare five components.

`sl-viewer-world-avatar`'s own bake **publish** had the same defect for
the same reason — a client-side bake uploaded to OpenSim as an ordinary
opaque texture is one no reference viewer can decode — and now publishes
the five-component form too. Computing a real mask instead of the
constant is [[viewer-bake-publish-morph-mask]].

Not the cause, each ruled out by experiment: the appearance service being
unnamed (fixed — the login response now carries
`agent_appearance_service`, and every bake is requested and served);
`Range` handling (added, no change); missing wearable layer textures
(added, no 404s remain); an incomplete Current Outfit Folder (fixed);
unbaked eye and hair slots (fixed). Dropping the self bakes entirely so
the viewer must composite locally does **not** de-cloud it either — the
avatar stays at rez status 0 by a different branch — so simply modelling
the fake grid as a legacy-bake grid is not the answer on its own.

What it cost while it stood: the agent's own avatar rendered as the cloud
particle rather than a body, so [[test-firestorm-crosscheck-runner]]'s
frames could not be compared anywhere the avatar was in shot. Everything
else in the scene rendered. Other avatars were unaffected — an NPC with
the same fabricated bakes loaded fully, because a non-self avatar's rez
status asks only whether its baked slots are *named*, not whether they
loaded.

## What the fix uncovered: nothing was standing the avatar up

With the body visible at last, it was visibly wrong: folded forwards,
head craned back, no hands. That was a second, independent gap — the
grid signalled the arriving agent **no animation at all**, and the
reference viewer then draws an avatar in the raw rest pose its skeleton
was authored in. A real simulator always has an answer (OpenSim's
`ScenePresence` stands an arriving agent up before it has moved a metre),
so `push_own_animation` now sends an `AvatarAnimation` playing the
built-in `stand` — an asset every viewer ships, so the grid serves
nothing extra. The pose came right with it, and so did the **left** hand.

The right one did not, and that was a third thing again — not the grid's
at all, but an upstream bug in the reference viewer's skin shader
(`avatarSkinV.glsl`: `mWristRight` lands in the last palette slot and its
blend partner is read one past the end of the array, so `NaN * 0` empties
the 388 vertices bound to it). Reported as
[secondlife/viewer#6240](https://github.com/secondlife/viewer/issues/6240);
the chase is recorded in [[test-firestorm-crosscheck-report]]. Worth
knowing here only because it masqueraded as part of this bug for an
afternoon: with the avatar clouded, then unposed, then one-handed, it was
natural to keep reading each new symptom as more of the same fixture
problem.

Verified live (2026-09-02) against the Firestorm capture harness
(`--credentials` / `--gridfile` / `--screenshot-dir`) pointed at
`scripts/fake-grid.sh --scenario catalogue`: the run's log carries zero
`Self is clouded` and zero `Fetch failure` lines, its scene dump reports
the self avatar `is_fully_loaded`, and its frames show a green body
standing upright. (The frames from that run still show only the left
hand — the shader bug above, fixed separately in the Firestorm fork.)

One thing that is **not** a bug, recorded so the next run does not chase
it: with `--camera-position` / `--camera-look-at` forced, the self avatar
bends towards the camera's focus — the reference viewer tracks the
agent's own head and body to where the camera looks. A pinned camera in
front of and below the avatar therefore produces a crouched, craned pose
that is gone the moment the camera is left alone. A cross-check that
photographs the *self* avatar has to account for that, or it will read
its own camera as a rendering divergence.
