---
id: viewer-rlv-enforce-info-hiding
title: RLV — information hiding / anonymisation layer
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state, viewer-name-tags-decorations, viewer-minimap]
---

Context: [context/viewer.md](../context/viewer.md).

Route the viewer's own display through **one anonymisation filter** so a
restriction hides what it must, everywhere it could leak. This family touches
the viewer's chat overlay, name tags ([[viewer-name-tags-decorations]]) and
mini-map ([[viewer-minimap]]) — every surface that shows a name, a location, or
hover text — consulting [[viewer-rlv-restriction-state]] at a single filter.

The behaviours:

- `@shownames` / `@shownametags` — obfuscate every name, including in chat and
  on the mini-map. The reference has a whole anonymisation layer
  (`RlvUtil::filterNames`) exactly so no display path prints a real name;
- `@showloc` — hide the region / parcel name;
- `@showminimap` / `@showworldmap` — suppress the map surfaces;
- `@showhovertext*` — hide in-world float text;
- `@showself` / `@showselfhead` — hide the user's own avatar.

The point of copying the reference's single-filter shape is that names, location
strings, hover text and map markers all pass through *one* place, so a new
display surface is covered for free. Wire each surface's text through it rather
than special-casing per widget.

Reference (Firestorm, read-only): `rlvhandler.cpp`, `rlvcommon.cpp`
(`RlvUtil::filterNames`), `rlvactions.h`.
