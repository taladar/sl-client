---
id: viewer-stars-srgb-linearize
title: Linearize the star field like the sky / clouds
topic: viewer
status: done
origin: viewer-clouds-sun-occlusion-horizon-contact sky-colour fix (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact, viewer-stars-add-in-encoded-space]
---

Context: [context/viewer.md](../context/viewer.md).

The 2026-08-03 sky-colour fix added the reference `srgb_to_linear` (which
`softenLight` applies to WL-sky pixels before tone-mapping) to the **sky dome**
and **clouds** so they are no longer washed out. The **star field**
(`stars.wgsl`, `AlphaMode::Add`) was deliberately left out of that pass — it is
night-only, faint, and additive, so getting the additive-blend interaction with
the linearization right needs its own thought, and it was not part of the
reported issue.

**Work:** apply the same `srgb_to_linear` treatment to the star output (matching
the reference, which linearizes the whole composited WL-sky including the
additive stars), and confirm on a night sky that the field's brightness / colour
still reads right against the (now linearized) night sky. Small, low priority.

## Done (2026-11-26)

By [[viewer-stars-add-in-encoded-space]], more exactly than asked: the star
pass now adds its texel to the encoded sky behind it and decodes the sum with
the sky's own `srgb_to_linear` and `sky_hdr_scale`, which is how the reference
linearises the composited WL sky. Checked on a night sky against Firestorm.
