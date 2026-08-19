---
id: viewer-particle-pick-mute
title: Particle picking + the muted-particle-source pie
topic: viewer
status: ready
origin: follow-up from viewer-object-context-menu (2026-07-21)
refs: [viewer-object-context-menu, viewer-avatar-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

Everything "mute the particle owner" is unimplemented across the pies:
`Part. Owner` in the object pie's Mute > (greyed), `Mute Particle Owner` in
the other-avatar pie's Mute > (greyed), and the reference's seventh pie —
`menu_pie_mute_particle.xml`, the single-slice pie shown when the pick lands
on **particles** — is not declared at all, because the renderer cannot pick
particles ([[viewer-object-context-menu]] deferred it rather than shipping a
pie with no open path).

Scope:

- **Particle picking**: resolve a ray against the live particle billboards
  (`particles.rs`) to the *source object*, as the reference's `PICK_PARTICLE`
  does. Coarse is fine — a hit on any live particle of a system resolves to
  that system's source; precedence goes to solid geometry (an object / avatar
  hit in front of the particles wins).
- **The muted-particle-source pie**: declare the reference's one-slice pie
  (`Mute Part. Own.` at east), open it when a right-click resolves to
  particles and nothing nearer, and pin its (single-entry) address table like
  every other pie.
- **Wire the three slices** to muting the particle source's *owner*: the
  source object's `owner_id` is already decoded (it is only meaningful on
  objects with particles or sound, which is exactly this case). Mute by agent
  id, as the reference's `Particle.Mute` / `EnableMuteParticle` do — enabled
  only when an owner id is known.
- Muting the owner should also stop *rendering* their particles (the point of
  the feature); tie into the mute list the session already tracks.

## Parity-audit addendum (2026-08-19)

Minor scope addition from the parity audit: the task enumerates the
object pie, the avatar-other pie and the one-slice 7th pie, but not
the **land pie's** `mute-particles` slice (menu_pie_land.xml /
`land_menu.rs`) nor the flat menus' Block Particle Owner entries
(menu_land.xml, menu_avatar_self.xml, menu_avatar_other.xml,
menu_object.xml). Same wiring — list all of them so no placeholder is
left behind.
