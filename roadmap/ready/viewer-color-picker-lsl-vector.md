---
id: viewer-color-picker-lsl-vector
title: Colour picker — the LSL vector tab and Copy LSL
topic: viewer
status: ready
origin: split out of viewer-ui-color-picker-advanced (2026-09-13), which shipped
  the rest of the reference floater
blocked_by: [viewer-ui-color-picker-advanced]
refs: [viewer-ui-color-picker-advanced, viewer-lsl-editor-widget]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's colour picker puts its numeric entry in a three-tab strip — **RGB**
(0..255 spinners), **LSL** (0..1 float spinners) and **Hex** — and adds a **Copy
LSL** button that writes the current colour to the clipboard as the
`<r, g, b>` vector a script wants (`floater_color_picker.xml`:
`rspin_lsl` / `gspin_lsl` / `bspin_lsl`, `copy_lsl_btn`; the `<FS:Zi> Add float
LSL color entry widgets` change).

[[viewer-ui-color-picker-advanced]] shipped the R/G/B and H/S/L sliders and the
hex field side by side rather than in tabs — with the hue × saturation field
taking the room the reference gives its tab strip, there was no case for hiding
two of three numeric views behind tabs. What is genuinely missing is the **LSL
half**: a scripter reading a colour out of the picker gets six hex digits and
has to divide by 255 three times by hand.

Add:

- a `<r, g, b>` read-out beside the hex field, editable the same way (committed
  on `Enter` or focus loss, three floats in `0..1`);
- a **Copy LSL** button that puts `<0.5, 0.25, 1.0>` on the clipboard
  (`sl-viewer-platform`'s clipboard, which the widget crate does not depend on
  today — so this is either a dependency the widget layer takes or a reply the
  composition root answers, the same question
  [[viewer-color-picker-face-pipette]] asks).

Reference (Firestorm, read-only): `floater_color_picker.xml`,
`llfloatercolorpicker.cpp` (`onClickCopyLSL`, `updateTextEntry`).
