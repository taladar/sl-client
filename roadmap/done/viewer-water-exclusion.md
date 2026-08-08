---
id: viewer-water-exclusion
title: Water-exclusion surfaces (invisiprim successor)
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-r25]
---

Context: [context/viewer.md](../context/viewer.md).

Water-exclusion surfaces: faces carrying the sentinel "invisible" texture
(the legacy invisiprim UUIDs) punch a hole in the **water plane** — the
modern reference repurposed the old invisiprim pass into a dedicated
water-exclusion draw pool, and boat / dock content relies on it to keep
hulls dry. Without it, such prims render as odd solids (they currently fall
through as ordinary textured faces).

Scope: detect the sentinel texture ids on faces at ingest, exclude those
faces from normal rendering, and mask the water surface where they are
(reference approach: render exclusion volumes into a mask the water shader
samples). Include the **legacy invisiprim** behaviour question explicitly:
old content also expected avatar/sky occlusion — decide and document how
far we follow the modern reference (which dropped that part) vs legacy
(per the support-legacy-content policy, match today's reference: water
exclusion only).

Reference (Firestorm, read-only): `lldrawpoolwaterexclusion.{cpp,h}`,
`PASS_INVISIBLE`, `llviewertexturelist` (sentinel ids), `llvowater`.

Builds on: the water renderer (`water.rs`) and face ingest (`textures.rs`).

## Done

A faithful port of `LLDrawPoolWaterExclusion` / `doWaterExclusionMask` /
`exclusionTex`, in three layers.

**Detection (`sl-proto`, Bevy-free).** New `IMG_ALPHA_GRAD` /
`IMG_ALPHA_GRAD_2D` constants (the two ids the reference forces to a
single-channel `GL_ALPHA` format in `llviewertexturelist`; `IMG_ALPHA_GRAD`
is what the build tool's "Hide water" checkbox applies,
`LLPanelFace::onCommitHideWater`) and `TextureFace::is_water_exclusion`, which
tests a face's diffuse id against them. Unit-tested (`cargo test -p sl-proto`).
The reference's runtime detection is by texture *format* (`GL_ALPHA`), which is
those two forced sentinels plus any user-uploaded alpha-only texture; matching
the two canonical ids covers modern water-exclusion content and everything the
"Hide water" tool produces. Broadening to a decode-time alpha-only test (for
arbitrary legacy invisiprim textures) is a possible later refinement.

**Rendering (`sl-client-bevy` + viewer `water_exclusion.rs`).** Bevy 0.19 has
no render-graph and the water surface is drawn in the main pass's transparent
phase, so the reference's "render the mask just before the water pass" is done
with a dedicated **mask camera**: slaved each frame to the main
[`ViewerCamera`]'s pose + projection, rendered first (`order = -1`) into an
`R8` image target, it renders **only** the exclusion faces — routed onto a new
`WATER_EXCLUSION_LAYER` (invisible to the main view and every probe), wearing a
flat-black double-sided unlit material — as black on a white clear. The shared
`WaterMaterial` gains an `exclusion_mask` texture binding (a white `1×1`
placeholder until wired), and `water.wgsl` samples it by the fragment's screen
position and `discard`s the sea where it reads black (the reference
`if (water_mask < 1) discard`, at a 0.5 threshold). Faces are diverted by a
`convert_water_exclusion_faces` system on `Changed<FaceTextureDebug>` (reusing
the per-face debug component every face already carries), so no face-spawn path
changed. Because the mask is a 2-D silhouette rendered double-sided, it excludes
the sea from every viewing angle, including looking down into an open hull.

**Legacy vs modern decision.** Follows the modern reference exactly: **water
exclusion only**. The legacy invisiprim's avatar / object / sky occlusion is
deliberately **not** reproduced (per support-legacy-content: match today's
reference). Because only the water shader samples the mask, nothing else is
affected — avatars, objects, and the sky render normally through an exclusion
surface.

**Simplification (documented).** The reference depth-tests the exclusion faces
against the scene depth so a hull hidden behind opaque geometry does not mark
the mask; the mask camera here has its own exclusion-only depth and cannot read
the main scene depth before the main pass runs, so an exclusion surface behind
opaque geometry still marks the mask — needs an occluded exclusion surface with
visible water beyond it, a rare combination, left for later.

**Live-test fixture.** `sl-client-tokio/examples/rez_water_exclusion.rs` rezzes
a large cube straddling the water height and textures it `IMG_ALPHA_GRAD`
(placement env-overridable), then logs out leaving it in-world for the viewer.
