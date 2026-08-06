---
id: viewer-avatar-mesh-hair-and-hairbase-both-render
title: Avatar shows mesh hair and a legacy hairbase at the same time
topic: viewer
status: deferred
origin: user report during viewer-facelight-too-bright replay review (2026-08-06)
refs: [viewer-facelight-too-bright]
---

Context: [context/viewer.md](../context/viewer.md).

On the captured avatar replayed for
[viewer-facelight-too-bright](../done/viewer-facelight-too-bright.md) (bundle
agent `52ed4c6a`), the avatar's head shows a **dark brown "shell"** over/among
the ginger mesh hair — a dark band at the forehead/hairline and a darker region
down the back — that looks like it shouldn't be there.

## Investigation (2026-08-06) — original hypothesis was wrong

The original framing (a **legacy system hairbase** showing through mesh hair)
is **not** what is happening. Verified on the replay:

- **All base-body parts are hidden for this avatar.** The `hair`-region base
  mesh (`avatar_hair.llm`) is hidden because the hair bake slot is
  `IMG_INVISIBLE`; head/upper/lower are hidden by `IMG_USE_BAKED_*`. So the
  system hair / base scalp is **not** rendering. The dark shell is the worn
  **rigged mesh-hair attachment itself**.
- The hair is a fatpack: ~8 geometrically identical colour-variant layers all on
  texture `774f5cb9` (6 hidden via tint-alpha 0, 2 visible ginger). The visible
  hair is mesh `37c30e61`, which renders as **4 cards**:
  - Card **#2** (small, forward, z≈0.37): UVs `U[0.851,0.999]` land on the
    **opaque dark region** of the atlas (that patch is ~92% opaque, mean
    luminance ~32). This is the "dark forehead piece."
  - Cards **#1** and **#3** (both ~14k verts, near-coincident position/UV): two
    large overlapping semi-transparent sheets — the "darker hair" at the back.
    Being coincident they share a sort point, so neither a per-face skinned
    transparent-sort experiment nor forcing opaque could separate them.
- The dark-brown atlas region is (per the texture) the hair's **undyed root
  colour**, painted opaque — a ginger strand in the atlas has a dark tip at its
  root end. So card #2 renders that dark region **faithfully**.

**Ruled out** (each forced in the replay and observed): lighting / normals
(unlit), backface culling / winding (double-sided — made it worse), transparency
draw-order (opaque; and a per-face skinned centroid re-sort), a dark **tint**
(every hair face is white or fully-transparent tint; only the eyelashes are
black-tinted), a baked skin hairbase (the head bake is clean skin), and a
texture-decode / UV error (the other cards decode consistently; #2 genuinely
maps to an opaque dark region).

## Why deferred

There is **no confirmable render defect** — what we draw is faithful to the
asset (an opaque dark-root card plus two coincident overlapping alpha sheets).
The remaining difference from Firestorm is almost certainly subtle card
**occlusion / positioning** of this specific stacked-alpha hair, and the
reference avatar has since left, so "correct" **cannot be verified**. Parked
until a comparable avatar is available live to compare against Firestorm.

Tooling from this investigation was kept: the **`P`-key worn-attachment pick**
now reports the full stack of worn rigged attachments under the crosshair with
each object's textures/tints (`AvatarPicker::all_worn_hits`), which is what let
the layers be identified through the overlapping fatpack.
