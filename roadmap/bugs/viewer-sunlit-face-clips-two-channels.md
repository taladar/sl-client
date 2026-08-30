---
id: viewer-sunlit-face-clips-two-channels
title: A sunlit opaque face pins red and green rather than showing its texture
topic: viewer
status: bugs
origin: observed while settling viewer-translucent-top-face-reads-opaque (2026-08-30)
refs: [viewer-translucent-top-face-reads-opaque, viewer-tonemap-auto-exposure]
---

Context: [context/viewer.md](../context/viewer.md).

An **opaque** plywood prim face pointing at the sun pins its red channel across
the *whole* cap and green across most of it. Plywood is a mid-tone wood texture,
so a face wearing it in ordinary daylight should not be running out of range.

Measured on the local grid at a pinned day position
(`SL_VIEWER_SKY_DAY_POSITION=0.35`), over a block of the North Region box's top
cap rather than a few pixels:

| tint | R | G | B | pixels at 255 |
| --- | --- | --- | --- | --- |
| opaque | med 255 | med 255 | med 180 | **R 100 %, G 89 %** |
| 50 % | med 242 | med 200 | med 147 | R 0.2 %, G 0 %, B 0 % |

Note the second row: the prim as it normally stands is **not** blown out — it is
an ordinary plywood tan, and nothing about it is near white. It is the *opaque*
case that runs out of range, and only that case. The name of this task was
"clips to white" at first, which overstated it: the clipped colour is a pale
yellow, since blue still has room.

## Why this is what made a blend unreadable

The gap between those rows is the whole of
[[viewer-translucent-top-face-reads-opaque]] — but the mechanism is **not**
mainly the clipping, and a first draft of this said it was.

Alpha blending happens on **linear radiance**, in the HDR buffer, before the
tone mapper. What you look at is the **display-encoded** value after it. So what
reaches the screen is

```text
T(a·L + (1 - a)·S)      and not      a·T(L) + (1 - a)·T(S)
```

and because `T` is compressive those are far apart when the face `L` is much
brighter than the background `S`. With a Reinhard-ish curve, a face at linear
8.0 over water at 0.1: `T(8.0) = 0.89` and `T(0.1) = 0.09`, so half of each
*appearance* would be `0.49` — while the real result is `T(4.05) = 0.80`. At
half coverage the face keeps four fifths of its opaque appearance. Coverage is
geometric; appearance follows radiance through a curve.

That is physically right and the reference does the same (see below). Clipping
is a second, smaller effect on top of it, and it shows in the **hue**: mixing
with blue-grey water ought to pull the face toward blue, and instead `R - B`
goes from 75 opaque to 95 at half coverage — *warmer*. Blue was free to fall
toward the sea's ~101 (180 → 147, almost exactly half and half) while red was
pinned at the ceiling and could only give up 13. Clipping protected red from the
mix.

So the blend is arithmetically correct and its evidence is drowned: mostly by
the brightness ratio, secondarily by red being pinned. What is left worth fixing
is only the over-range part — a face that did not run out of range would carry
the sea visibly at half coverage, and that report would never have been
ambiguous.

## The reference composites the same way

Checked, so nobody re-opens the compositing half of this:

- The alpha pools draw into `mRT->screen`, allocated `GL_RGBA16F`
  (`pipeline.cpp:970`) — a linear **HDR** target where values pass 1.0 freely.
- `renderFinalize` tone-maps that buffer **afterwards**
  (`tonemap(&mRT->screen, ...)`, `pipeline.cpp:8928`), the alpha pools having
  run in `renderGeomPostDeferred`.

Blend in linear HDR, compress after — the same ordering this viewer has, so the
"a bright face keeps most of its appearance at half coverage" behaviour is the
reference's too and is not a divergence.

Not to be confused with the reference's **8-bit gbuffer** clamp, which is a
different buffer on the sky/cloud path (memory
`sl-client-wl-sky-8bit-gbuffer-clamp`), not the alpha screen target.

## What is actually open

Only this: **is a sunlit plywood face supposed to be that bright?** Ours reaches
roughly twice the display ceiling. If Firestorm's does not, the exposure or the
sunlight scale is hot, and the visible cost is that transparency reads weaker
than it should on every lit surface.

- Compare against Firestorm on the same region at the same pinned day position —
  the only thing that can answer it, and the local grid supports it.
- The scene has its own tone mapper (`crate::tonemap`, P33.3) and a camera
  `Exposure`; see [[viewer-tonemap-auto-exposure]] for what that pass already
  does.
- The memory `sl-client-sky-brightness-is-authored-data` records that the
  frame's `sunlight_color` is authored data, and that night is dark *because the
  data says so* — the same lever in the other direction is the first thing to
  check.

## Not to be confused with

The **glow** pass, which does brighten what it blooms but is not this: with
`SL_VIEWER_DISABLE_GLOW=1` the same face lands within 1–3/255.
