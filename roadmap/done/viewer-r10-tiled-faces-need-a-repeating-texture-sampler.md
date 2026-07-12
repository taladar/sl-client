---
id: viewer-r10
title: Tiled faces need a repeating texture sampler
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R10. Tiled faces need a repeating texture sampler.** The real cause of
the half-cylinder / disk "streaked toward the edges, coherent in the centre"
look (diagnosed from a live `pick` dump of the face's `TextureFace`: both
faces were `planar=false`, so R9 was a red herring; the tell was the
**repeats** — `scale_s`/`scale_t` of `(2, 1.6)` on the disk cap and `(10, 1)`
on the railing wall). Repeats above one push the face UVs outside `[0, 1]` to
tile the texture, but prim/mesh face images were uploaded with Bevy's default
**clamp-to-edge** sampler, which smears the edge texel across every
out-of-range tile instead of wrapping — heavy streaking at the rim, worse at
higher repeats. Second Life samples object faces with a **repeat/wrap**
address mode. Fixed in the viewer's `prim_image`: prim/mesh face textures now
upload with a repeating sampler (`address_mode_u/v/w = Repeat`, linear
filtering); avatar-bake and terrain paths are untouched. Also added a per-face
texture-placement dump to the `pick` crosshair tool (`FaceTextureDebug`:
repeats / offset / rotation / texgen / texture id) — the ground-truth
diagnostic that found this. **User-confirmed:**
the tiled faces now render "much closer to Firestorm". (A remaining colour /
brightness difference is suspected to be lighting / tonemapping rather than
texturing — a separate follow-up, not pursued here.)
