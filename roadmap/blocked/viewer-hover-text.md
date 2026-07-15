---
id: viewer-hover-text
title: Object hover text (llSetText floating text)
topic: viewer
status: blocked
origin: user request (2026-07)
blocked_by: [viewer-name-tags-billboard-render]
refs: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The floating text an object sets with `llSetText` — vendors, rental boxes, HUD
readouts, scripted signs. It is **already decoded and thrown away**: every
`Object` carries `text: String` and `text_color: [u8; 4]` (the colour plus its
alpha), filled in on both the full `ObjectUpdate` path
(`sl-proto/src/session/conversions.rs`) and the compressed one
(`sl-proto/src/object_update/compressed.rs`, behind
`CompressedFlags::HAS_TEXT`). Nothing in the viewer ever reads either field.

Scope: render the text above the object (multi-line, wrapped, camera-facing),
honour the colour and its alpha (alpha 0 means "set but invisible" — a common
way to attach text a script later reveals), fade with distance and hide beyond a
range limit, decide the depth / occlusion behaviour, update on every
`ObjectUpdated` so a script changing its text is reflected live, and clear the
text when the object is removed. Add the show-hover-text preference
([[viewer-preferences-floater]]).

This is the same world-anchored-text machinery as the name-tag renderer
([[viewer-name-tags-billboard-render]]) — billboarded, size-clamped,
occlusion-aware text over a world entity — so it should reuse whatever that task
lands rather than growing a second text path.

Reference (Firestorm, read-only): `llhudtext`, `LLVOVolume::updateText`.

Deps: [[viewer-name-tags-billboard-render]] (shares the world-anchored text
renderer).
