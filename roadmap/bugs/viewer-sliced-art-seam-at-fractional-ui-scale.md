---
id: viewer-sliced-art-seam-at-fractional-ui-scale
title: A nine-sliced button's frame changes thickness along the quad's diagonal at a fractional UI scale
topic: viewer
status: bugs
origin: live look during viewer-skin-text-shadow-role (2026-09-24)
refs: [viewer-skin-image-backed-widgets, viewer-vintage-skin]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

## Observation

The gallery under Graphite's **Relief** theme, on a 4K screen at a Wayland
UI scale of **1.5**: the `button` specimen's frame is not even. Along the top
edge, the first ~34 px from the left corner show a 1 px black frame over a
3 px highlight, and past that point a 2 px frame over a 2 px highlight. The
bottom edge has the same step, mirrored, ~34 px in from the bottom-right. The
art (`skins/graphite/widgets/push-button.png`) is uniform: a 1 px frame and a
2 px bevel on every side, 24×24, sliced `8px` all round.

Measured off the screenshot (pixel rows at x = 40 vs x = 50 across the top
band), the only colours present are the art's own, so this is texel choice,
not blending.

## Diagnosis

- The art is sampled **nearest** on purpose (each PNG's `.meta`, pinned by
  `every_file_the_theme_names_decodes_and_is_sampled_nearest`), so a 2 px
  bevel does not blur.
- At a scale of 1.5, an 8-texel corner spans 12 px, and texel edges fall at
  every 1.5 px, i.e. **exactly on some pixel centres**. Which texel such a pixel
  takes is decided by the last bit of the interpolated UV.
- `bevy_ui_render` draws the node as **two triangles**. Their affine UV
  interpolation agrees mathematically and not in floating point, so the two
  halves round a knife-edge pixel differently. The change of thickness sits
  where the shared diagonal crosses the edge band: for a ~1100×50 px button
  and a band ~1.5 px deep, the diagonal crosses at about 1.5 / 50 × 1100 ≈
  33 px from the corner, which is where it is seen.

`ui_texture_slice.wgsl`'s `map_axis_with_repeat` maps a corner as
`(p / il) * tl`, the same for every x, so the slicing maths is not at fault.
The rounding is.

## What would fix it

Two options, to be chosen when this is taken up:

1. **Draw sliced corners and edges at a whole-number scale**, e.g. round the
   slice scale to the nearest integer multiple of the physical pixel, so
   every texel edge lands between pixel centres. A 1 px line then stays 1 or
   2 px everywhere, never a mixture. Probably a `bevy_ui_render` fork change
   in `compute_texture_slices`, since the insets and the scale factor meet
   there.
2. **Make the texel choice deterministic in the shader**: compute the slice
   coordinate from the fragment's physical pixel position (or nudge it by a
   sub-texel epsilon) rather than from the interpolated UV, so both triangles
   agree. It is less invasive, but a 1 px line would still show as an uneven
   1-and-2 px pattern along an edge.

Option 1 is closer to what a classic skin wants (crisp, even bevels); check
what the reference does at a fractional `UIScaleFactor` before settling.

## Done when

At a UI scale of 1.5 the Relief button's frame and bevel are the same
thickness along each whole edge, and a test pins the slice geometry at a
fractional scale.
