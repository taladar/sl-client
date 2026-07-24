---
id: viewer-ui-color-picker-advanced
title: Color picker — SV square, hue strip, palette & eyedropper
topic: viewer
status: ready
origin: user request (2026-07-24) while reviewing the RGB-slider color picker
  shipped with viewer-prim-texture-editing
blocked_by: [viewer-ui-color-picker]
refs: [viewer-prim-texture-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The colour picker shipped with the build-tool Texture tab
([[viewer-prim-texture-editing]]) is the useful core of
[[viewer-ui-color-picker]]: a reusable swatch + an `OpenColorPicker` /
`ColorPicked` floater with **R/G/B sliders, a live preview swatch, an
original-colour compare, and OK/Cancel**. This task adds the rest of the
reference's `LLFloaterColorPicker`:

- the **saturation/value square** + **hue strip** (2-D + 1-D drag pickers),
- the **hex** entry field,
- the **eyedropper** — pick a colour from anywhere on screen (the
  reference's `LLFloaterColorPicker::onColorSelect` pipette / `mPipetteBtn`,
  reading the framebuffer pixel under the cursor),
- the **saved-swatch palette** (persisted in the settings store),
- **live-apply while dragging** with revert-on-cancel (the reference applies
  continuously to the object being tuned; the current picker applies only on
  OK to avoid flooding the simulator — this task adds a throttled live path).

Reference (Firestorm, read-only): `llfloatercolorpicker.cpp`,
`floater_color_picker.xml`, `llcolorswatch.cpp`.
