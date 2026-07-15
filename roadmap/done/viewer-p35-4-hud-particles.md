---
id: viewer-p35-4
title: HUD particles
topic: viewer
status: done
origin: found while doing viewer-p35-2 (the reference has a whole second particle partition for HUDs)
refs: [viewer-p35-1, viewer-p35-2, viewer-p30-1, viewer-p30-2]
---

Context: [context/viewer.md](../context/viewer.md).

**P35.4. Emit a HUD prim's particles into HUD space** rather than into the
world. A particle source on a **HUD** attachment ([[viewer-p35-1]]) is the way a
HUD draws snow / rain / sparkles across the screen, a damage flash, or a puff
when a button is pressed — the particles must live in the HUD's screen space,
drift across the *screen*, and be drawn by the HUD camera.

Today [[viewer-p30-2]]'s `drive_particles` spawns one cloud entity per source,
at the scene root, with the particles integrated in **world** coordinates and
billboarded at the fly camera. A HUD emitter therefore throws its particles into
a corner of the region (wherever the HUD screen's origin happens to sit), on the
world layer, where the HUD camera cannot see them and the fly camera sees them
as a strange little cloud at the region corner. It is the exact bug
[[viewer-p35-1]] fixed for HUD *geometry*, one pipeline over.

The reference viewer models it as a second particle partition end to end:

- `LLViewerPartSource::update` flags every particle from a HUD emitter
  `LLPartData::LL_PART_HUD` (`if (mSourceObjectp->isHUDAttachment())`);
- `LLViewerPartSim::createViewerPartGroup(…, hud)` puts those in a **separate**
  group — `LLVOHUDPartGroup`, partition `PARTITION_HUD_PARTICLE`, drawable
  `setLit(false)`, render type `RENDER_TYPE_HUD_PARTICLES` — and
  `LLViewerPartGroup::addPart` refuses to mix the two (`if (part->mFlags &
  LL_PART_HUD && !mHud) return false`);
- it billboards against the HUD camera, not the eye: `LLVOHUDPartGroup::
  getCameraPosition()` returns `(-1, 0, 0)`, the HUD view point;
- and it is **off by default** behind `RenderHUDParticles`
  (`render_hud_attachments` turns `RENDER_TYPE_HUD_PARTICLES` off unless the
  setting is on), presumably because a full-screen particle HUD is both a
  frame-rate and a griefing surface. Worth mirroring as a viewer flag / env,
  defaulted on for us (we have no settings UI, and the point of the phase is to
  see it work).

Shape of the work: recognise a HUD-attachment particle source (the
[[viewer-p35-1]] classification is already there — `in_hud_attachment` / the HUD
render layer), integrate its particles in the emitter's HUD space instead of
world space, parent the cloud entity under the HUD screen (so it inherits
`HUD_RENDER_LAYER` by propagation, as every routed attachment does), billboard
the quads at the HUD camera's fixed view direction rather than the fly camera,
and render them unlit like the rest of the HUD ([[viewer-p35-2]]'s
`apply_hud_fullbright` covers object faces, not the particle material, which is
its own).

Verify live on OpenSim: a scripted `llParticleSystem` on a prim worn on a HUD
point — the particles must fall across the *screen*, follow it as the camera
flies, and not appear anywhere in the world.
