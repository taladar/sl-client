---
id: viewer-color-picker-face-pipette
title: Colour picker pipette — sample a prim face's stored tint
topic: viewer
status: ready
origin: split out of viewer-ui-color-picker-advanced (2026-09-13), which shipped
  the framebuffer eyedropper that task specified
blocked_by: [viewer-ui-color-picker-advanced]
refs: [viewer-prim-texture-editing, viewer-ui-color-picker-advanced]
---

Context: [context/viewer.md](../context/viewer.md).

The eyedropper that shipped with [[viewer-ui-color-picker-advanced]] samples the
**rendered frame**: it reads back the window and hands over the pixel under the
pointer, so it can sample anything on screen — a texture, the sky, another
panel. That is what that task asked for, and it is the more general tool.

It is *not* what the reference's pipette does. `LLToolPipette` world-picks the
face under the cursor and reads that face's `LLTextureEntry` colour
(`LLFloaterColorPicker::onColorSelect` ← `setToolSelectCallback`), so it gives
back the **stored tint**, unlit and unfogged, and refuses anything that is not a
prim face. Sampling a shaded pixel and sampling a stored tint are different
answers to different questions: "make this prim the same colour as that one"
wants the tint, and gets the wrong number from the frame the moment either prim
is in shadow.

So add the reference's mode beside the one that shipped. The pipette button
becomes two-state (frame / face, the reference's behaviour being the one the
build tools' swatches default to):

- world-pick the face under the pointer (`ObjectPicker` + `SurfaceInfo`, as the
  Select Face tool and [[viewer-texture-drag-drop]] do), read its
  `TextureEntry` colour, and feed that to the picker;
- highlight the hovered face while the tool is armed, as
  `LLToolPipette::pickCallback`'s `highlightObjectOnly` does;
- refuse (and say so) over anything that is not a prim face.

The wiring is the interesting part: the colour picker lives in
`sl-viewer-ui-widgets`, which is *below* every crate that knows what a prim is.
The frame eyedropper could live in the widget because a framebuffer is not
world knowledge; a face pipette cannot. It wants the same shape the picker's
own reply has — the widget emits "somebody sample a face for me" and whatever
layer owns the scene answers with a colour.

Reference (Firestorm, read-only): `lltoolpipette.cpp`,
`llfloatercolorpicker.cpp` (`onClickPipette`, `onColorSelect`,
`stopUsingPipette`).
