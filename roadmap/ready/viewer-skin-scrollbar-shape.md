---
id: viewer-skin-scrollbar-shape
title: The scrollbar is a bar — the reference's has ends, a track and a shape
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-ui-virtualized-list, viewer-vintage-skin, viewer-skin-image-backed-widgets]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

`virtual_list.rs` draws a scrollbar as two rectangles: a 10 px track in
`srgb(0.12, 0.14, 0.18)` and a thumb in `srgb(0.40, 0.48, 0.60)`, both
hardcoded, with a 24 px minimum thumb. That is every scrollbar in the viewer.

The reference's (`llscrollbar.cpp`) is four parts — a decrement button, a
track, a thumb and an increment button — and every skin dresses all four.
Vintage ships `ScrollArrow_{Up,Down,Left,Right}`, `ScrollThumb_{Vert,Horiz}`
and `ScrollTrack_{Vert,Horiz}`, nine-sliced, and then *tints* them from
`colors.xml`: `ScrollbarThumbColor` white (so the art shows through) over
`ScrollbarTrackColor` `#999999`. The measured art is a dark blue thumb
(`#3c4c7c`) in a mid-grey groove.

Two separate gaps, and they are worth keeping separate:

1. **Shape and colour are unskinnable.** Even the flat rectangles we draw
   should take their colours from role tokens, and be able to take a
   nine-sliced image instead ([[viewer-skin-image-backed-widgets]]).
2. **The end buttons do not exist.** A classic skin without them does not read
   as classic, and clicking an arrow to step a list is behaviour a Second Life
   user of long standing has in their hands. This is a small widget feature,
   not only a paint job: press-and-hold repeat, a step size, and the
   interaction rules the rest of the widget set already follows
   ([[viewer-ui-interaction-contracts]]).

## What to do

- `--scrollbar-track`, `--scrollbar-thumb`, `--scrollbar-thumb-hover`,
  `--scrollbar-arrow` roles; `virtual_list` consumes them.
- Optional end buttons on the scrollbar, on by default when the active skin
  says so — an arrow glyph or a skin-supplied image, with click-to-step and
  hold-to-repeat.
- Keep the thumb's minimum length and the existing scroll maths untouched; the
  ends take their thickness out of the track, not out of the content.

## Done when

The scrollbar's colours come from the skin, a skin can give it arrow ends, the
arrows step and repeat, and the list's scroll maths is unchanged (its existing
tests still pass with the ends both on and off).
